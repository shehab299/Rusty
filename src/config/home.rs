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
