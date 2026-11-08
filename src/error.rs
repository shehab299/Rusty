use std::{error::Error, fmt, io};

use ai_sdk::providers::ModelError;
use config::ConfigError;
use tokio::task::JoinError;

#[derive(Debug)]
pub enum AppError {
    Config(ConfigError),
    Agent(ModelError),
    Terminal(io::Error),
    Task(JoinError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(f, "failed to load configuration: {error}"),
            Self::Agent(error) => write!(f, "agent failed: {error}"),
            Self::Terminal(error) => write!(f, "terminal failed: {error}"),
            Self::Task(error) => write!(f, "agent task failed: {error}"),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Config(error) => Some(error),
            Self::Agent(error) => Some(error.as_ref()),
            Self::Terminal(error) => Some(error),
            Self::Task(error) => Some(error),
        }
    }
}
