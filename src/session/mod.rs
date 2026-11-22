//! Session state and durable event history.

pub use event::{Event, TurnOutcome};
use log::Log;
pub use state::{State, ToolStatus};
use std::{io, path::Path};

mod event;
mod log;
mod state;

#[derive(Debug, Default, PartialEq)]
pub struct SessionData {
    pub state: State,
    pub events: Vec<Event>,
    pub updated: u64,
}

impl SessionData {
    /// Load without locking or repairing the log, or invoking external effects.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let (records, updated) = Log::read(path.as_ref())?;
        let mut state = State::default();
        let mut events = Vec::with_capacity(records.len());
        for (sequence, event) in records {
            state.validate(&event)?;
            state.apply(sequence, &event);
            events.push(event);
        }
        Ok(Self {
            state,
            events,
            updated,
        })
    }
}

pub struct Session {
    pub data: SessionData,
    pub(crate) log: Log,
}

impl Session {
    pub fn create(directory: impl AsRef<Path>) -> io::Result<Self> {
        let log = Log::create(directory)?;
        Ok(Self {
            data: SessionData::default(),
            log,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let log = Log::open(path)?;
        let data = SessionData::load(&log.path)?;
        Ok(Self { data, log })
    }

    pub fn record(&mut self, event: Event) -> io::Result<u64> {
        self.data.state.validate(&event)?;
        let (sequence, updated) = self.log.append(event.clone())?;
        self.data.state.apply(sequence, &event);
        self.data.events.push(event);
        self.data.updated = updated;
        Ok(sequence)
    }

    /// Retain partial output when a running tool's future is dropped. The caller
    /// records the interruption before continuing or closing the turn.
    pub(crate) fn preserve_tool_output(&mut self, call_id: &str, output: &str) {
        assert_eq!(
            self.data.state.tools.get(call_id),
            Some(&ToolStatus::Running)
        );
        *self.data.state.tool_content(call_id) = output.to_owned();
    }

    /// Commit interruption facts using any partial output retained in memory.
    pub fn mark_running_tools_interrupted(&mut self) -> io::Result<()> {
        let running_tools_ids: Vec<_> = self
            .data
            .state
            .tools
            .iter()
            .filter(|(_, status)| **status == ToolStatus::Running)
            .map(|(id, _)| id.clone())
            .collect();

        for call_id in running_tools_ids {
            let content = self.data.state.tool_content(&call_id).clone();
            self.record(Event::ToolInterrupted { call_id, content })?;
        }

        Ok(())
    }

    /// Close abandoned operations without repeating external side effects.
    pub fn recover(&mut self) -> io::Result<()> {
        self.mark_running_tools_interrupted()?;

        if self.data.state.pending_request.is_some()
            || (self.data.state.outcome.is_none() && self.data.state.context.len() > 1)
        {
            self.record(Event::TurnFinished {
                outcome: TurnOutcome::Cancelled,
                error: Some("Previous turn interrupted. No operations were rerun.".into()),
            })?;
        }

        Ok(())
    }
}
