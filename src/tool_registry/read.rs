use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use tool::{ToolError, tool};

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ReadToolResult {
    File(String),
    Directory(Vec<String>),
}

#[tool(
    description = "Reads a UTF-8 file or lists a directory in sorted order. Directory names end with /. offset is 1-based (default 1); limit is the maximum number of lines or entries (default 2000)."
)]
pub fn read(
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<ReadToolResult, ToolError> {
    let offset = offset.unwrap_or(1);
    let limit = limit.unwrap_or(2000);

    if offset < 1 {
        return Err("offset must be at least 1".into());
    }

    if limit < 1 {
        return Err("limit must be at least 1".into());
    }

    let metadata = fs::metadata(&path).map_err(|error| format!("Cannot read {path}: {error}"))?;

    if metadata.is_dir() {
        let entries =
            fs::read_dir(&path).map_err(|error| format!("Cannot list {path}: {error}"))?;

        let mut names = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|error| format!("Cannot list {path}: {error}"))?;
            let mut name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() {
                name.push('/');
            }
            names.push(name);
        }

        names.sort();
        return Ok(ReadToolResult::Directory(
            names
                .into_iter()
                .skip(offset - 1)
                .take(limit)
                .collect::<Vec<_>>(),
        ));
    }

    if !metadata.is_file() {
        return Err(format!("Cannot read {path}: not a regular file or directory").into());
    }

    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Cannot read {path} as UTF-8: {error}"))?;

    Ok(ReadToolResult::File(
        contents
            .lines()
            .skip(offset - 1)
            .take(limit)
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    ))
}
