use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;
use std::{fs, result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tool::{DynTool, ToolError, tool};

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
enum ReadToolResult {
    File(String),
    Directory(Vec<String>),
}

#[tool(
    description = "Reads a UTF-8 file or lists a directory in sorted order. Directory names end with /. offset is 1-based (default 1); limit is the maximum number of lines or entries (default 2000)."
)]
fn read(
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

#[tool(description = "Execute a command in user's shell environment")]
async fn shell(command: String, timeout: Option<u64>) -> Result<String, ToolError> {
    let mut shell_result = Vec::<String>::new();

    let mut child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout_open = true;
    let mut stderr_open = true;

    let stdout = child
        .stdout
        .take()
        .ok_or(std::io::Error::other("stdout is not piped"))?;

    let mut stdout_reader = BufReader::new(stdout).lines();

    let stderr = child
        .stderr
        .take()
        .ok_or(std::io::Error::other("stderr is not piped"))?;

    let mut stderr_reader = BufReader::new(stderr).lines();

    let timeout = timeout.unwrap_or(20);
    let deadline = tokio::time::sleep(Duration::from_secs(timeout));
    tokio::pin!(deadline);

    let mut exit_status = None;

    while stdout_open || stderr_open || exit_status.is_none() {
        tokio::select! {
            result = stdout_reader.next_line(), if stdout_open => {
                match result? {
                    Some(line) => {
                        shell_result.push(line);
                    },
                    None => stdout_open = false
                }
            }

            result = stderr_reader.next_line(), if stderr_open => {
                match result? {
                    Some(line) => {
                        shell_result.push(line);
                    },
                    None => stderr_open = false
                }
            }

            result = child.wait(), if exit_status.is_none() => {
                exit_status = Some(result?);
            }

            _ = &mut deadline => {

                shell_result.push("Command timed out!".to_string());

                if exit_status.is_none() {
                    child.kill().await?;
                }

                break;
            }
        };
    }

    Ok(shell_result.join("\n"))
}

pub fn get_tools() -> Vec<Box<dyn DynTool>> {
    vec![Box::new(ReadTool), Box::new(ShellTool)]
}
