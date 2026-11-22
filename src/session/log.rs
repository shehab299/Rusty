//! Durable append-only session events. No SQLite, snapshots, or compactor.
//!
//! Each JSONL line is a record. A synced newline is the commit boundary.
//! Opening repairs only an unterminated final line; malformed complete lines
//! are explicit errors.
//! Exclusive file locking prevents concurrent writers. Failed writes poison
//! the handle: uncertain commits must be inspected by reopening, not retried.

use super::Event;
use crate::utils::{create_directory, timestamp};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    iter::FusedIterator,
    path::{Path, PathBuf},
};

const MAX_PAYLOAD: usize = 64 * 1024 * 1024;
const MAX_LINE: usize = MAX_PAYLOAD + 1;
const VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    version: u32,
    sequence: u64,
    timestamp: u64,
    event: Event,
}

struct Records<'a> {
    reader: BufReader<io::Take<&'a mut File>>,
    bytes: Vec<u8>,
    last_sequence: u64,
    valid_bytes: u64,
    finished: bool,
}

impl<'a> Records<'a> {
    fn new(file: &'a mut File) -> io::Result<Self> {
        file.seek(SeekFrom::Start(0))?;
        let length = file.metadata()?.len();
        let mut reader = BufReader::new(file.take(length));
        if reader.fill_buf()?.starts_with(b"WAL1") {
            return Err(invalid("binary WAL files are no longer supported"));
        }
        Ok(Self {
            reader,
            bytes: Vec::new(),
            last_sequence: 0,
            valid_bytes: 0,
            finished: false,
        })
    }

    fn read_next(&mut self) -> io::Result<Option<Record>> {
        self.bytes.clear();
        // Bound allocation even when a corrupt file contains no newline.
        let read = self
            .reader
            .by_ref()
            .take((MAX_LINE + 1) as u64)
            .read_until(b'\n', &mut self.bytes)?;
        if read > MAX_LINE {
            return Err(invalid("event log line exceeds size limit"));
        }
        if self.bytes.last() != Some(&b'\n') {
            return Ok(None);
        }
        let payload = &self.bytes[..self.bytes.len() - 1];
        if payload.len() > MAX_PAYLOAD {
            return Err(invalid("event log record exceeds 64 MiB"));
        }
        let record: Record =
            serde_json::from_slice(payload).map_err(|error| invalid(error.to_string()))?;
        if record.version != VERSION || Some(record.sequence) != self.last_sequence.checked_add(1) {
            return Err(invalid(
                "unsupported event log version or non-contiguous sequence",
            ));
        }
        self.last_sequence = record.sequence;
        self.valid_bytes += read as u64;
        Ok(Some(record))
    }
}

impl Iterator for Records<'_> {
    type Item = io::Result<Record>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        match self.read_next() {
            Ok(Some(record)) => Some(Ok(record)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }
}

impl FusedIterator for Records<'_> {}

pub(crate) struct Log {
    pub path: PathBuf,
    file: File,
    sequence: u64,
    poisoned: bool,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

impl Log {
    /// Each run starts a fresh log. Existing logs are never auto-resumed.
    pub fn create(directory: impl AsRef<Path>) -> io::Result<Self> {
        let directory = directory.as_ref();
        create_directory(directory)?;

        let file = tempfile::Builder::new()
            .prefix("session-")
            .suffix(".jsonl")
            .tempfile_in(directory)?;

        let (file, path) = file.keep().map_err(|error| error.error)?;
        drop(file);
        Self::open(path)
    }

    /// Open the durable log. Session owns state and event-transition validation.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        create_directory(parent)?;

        let mut options = OpenOptions::new();
        options.read(true).append(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let mut file = options.open(path)?;
        file.try_lock().map_err(io::Error::from)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }

        let mut records = Records::new(&mut file)?;
        for record in records.by_ref() {
            record?;
        }
        let sequence = records.last_sequence;
        let valid_len = records.valid_bytes;
        drop(records);

        if file.metadata()?.len() != valid_len {
            file.set_len(valid_len)?;
        }
        file.sync_all()?;
        File::open(parent)?.sync_all()?;

        Ok(Self {
            path: path.to_path_buf(),
            file,
            sequence,
            poisoned: false,
        })
    }

    /// Read committed events without repairing or locking another session.
    pub fn read(path: &Path) -> io::Result<(Vec<(u64, Event)>, u64)> {
        let mut file = File::open(path)?;
        let mut events = Vec::new();
        let mut updated = 0;
        for record in Records::new(&mut file)? {
            let record = record?;
            updated = record.timestamp;
            events.push((record.sequence, record.event));
        }
        Ok((events, updated))
    }

    pub fn append(&mut self, event: Event) -> io::Result<(u64, u64)> {
        if self.poisoned {
            return Err(io::Error::other(
                "event log write failed previously; reopen and inspect before continuing",
            ));
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("event log sequence exhausted"))?;
        let record = Record {
            version: VERSION,
            sequence,
            timestamp: timestamp(),
            event,
        };
        let payload = serde_json::to_vec(&record)?;
        if payload.len() > MAX_PAYLOAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "event log record exceeds 64 MiB",
            ));
        }
        let frame = Self::encode(&payload);
        self.poisoned = true;
        self.file.write_all(&frame)?;
        self.file.sync_all()?;
        self.sequence = sequence;
        self.poisoned = false;
        Ok((sequence, record.timestamp))
    }

    fn encode(payload: &[u8]) -> Vec<u8> {
        let mut line = payload.to_vec();
        line.push(b'\n');
        line
    }
}
