//! The agent's root directory for persistent application data.

use std::{io, path::PathBuf};

/// Resolve and create `.rust-agent` in the current user's home directory.
pub fn get_agent_root() -> io::Result<PathBuf> {
    let user_home = std::env::home_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Cannot locate the current user's home directory",
        )
    })?;
    let home = user_home.join(".rust-agent");
    std::fs::create_dir_all(&home)?;
    Ok(home)
}

pub fn get_session_store() -> io::Result<PathBuf> {
    let root = get_agent_root()?;
    let session_store = root.join("wal");
    std::fs::create_dir_all(&session_store)?;
    Ok(session_store)
}

pub fn get_cached_model_catalog_path() -> io::Result<PathBuf> {
    let agent_root = crate::config::home::get_agent_root()?;
    let path = agent_root.join("cache/models.json");
    Ok(path)
}
