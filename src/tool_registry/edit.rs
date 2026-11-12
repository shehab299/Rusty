use std::io::Write;
use tool::{ToolError, tool};

#[tool(
    description = "Replaces exactly one occurrence of old with new in an existing UTF-8 file. old must be non-empty and match exactly once; include more surrounding text to disambiguate."
)]
pub async fn edit(path: String, old: String, new: String) -> Result<(), ToolError> {
    if old.is_empty() {
        return Err("old must not be empty".into());
    }

    let path = tokio::fs::canonicalize(path).await?;
    let mut content = tokio::fs::read_to_string(&path).await?;
    let start = content
        .find(&old)
        .ok_or_else(|| format!("Cannot edit {}: old text was not found", path.display()))?;

    if content.rfind(&old) != Some(start) {
        return Err(format!(
            "Cannot edit {}: old text matches more than once",
            path.display()
        )
        .into());
    }

    content.replace_range(start..start + old.len(), &new);
    tokio::task::spawn_blocking(move || -> Result<(), ToolError> {
        let permissions = std::fs::metadata(&path)?.permissions();
        let mut temporary = tempfile::NamedTempFile::new_in(
            path.parent().ok_or("File path has no parent directory")?,
        )?;
        temporary.write_all(content.as_bytes())?;
        temporary.as_file().set_permissions(permissions)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&path)?;
        Ok(())
    })
    .await??;

    Ok(())
}
