use tool::{ToolError, tool};

#[tool(description = "Writes the given content to the file at the given path")]
async fn write(path: String, content: String, append: bool) -> Result<(), ToolError> {
    Ok(())
}
