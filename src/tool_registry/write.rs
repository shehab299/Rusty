use tokio::io::AsyncWriteExt;
use tool::{ToolError, tool};

#[tool(description = "Writes the given content to the file at the given path")]
pub async fn write(path: String, content: String, append: bool) -> Result<String, ToolError> {
    let input_length = content.len();

    let mut f = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .open(path)
        .await?;

    f.write_all(&content.into_bytes()).await?;

    let output_length = f.metadata().await?.len();

    Ok(format!(
        "Wrote {}/{} bytes successfully",
        input_length, output_length
    ))
}
