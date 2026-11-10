use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tool::{ToolError, tool};

#[tool(description = "Execute a command in user's shell environment")]
pub async fn shell(command: String, timeout: Option<u64>) -> Result<String, ToolError> {
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
