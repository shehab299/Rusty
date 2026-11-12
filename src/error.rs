use std::{error::Error, fmt, io};

use config::ConfigError;
use tokio::sync::mpsc::error::{SendError, TrySendError};
use tokio::task::JoinError;

use crate::comms::{AgentCommand, AgentEvent};

#[derive(Debug)]
pub enum AppError {
    Config(ConfigError),
    Agent(Box<dyn Error + Send + Sync + 'static>),
    Tool(Box<dyn Error + Send + Sync + 'static>),
    Provider(Box<dyn Error + Send + Sync + 'static>),
    Io(io::Error),
    HttpRequest(reqwest::Error),
    Json(serde_json::Error),
    Channel(Box<dyn Error + Send + Sync + 'static>),
    Terminal(io::Error),
    Task(JoinError),
    Telemetry(Box<dyn Error + Send + Sync + 'static>),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(f, "failed to load configuration: {error}"),
            Self::Agent(error) => write!(f, "agent failed: {error}"),
            Self::Tool(error) => write!(f, "{error}"),
            Self::Provider(error) => write!(f, "provider failed: {error}"),
            Self::Io(error) => write!(f, "I/O failed: {error}"),
            Self::HttpRequest(error) => write!(f, "HTTP request failed: {error}"),
            Self::Json(error) => write!(f, "JSON processing failed: {error}"),
            Self::Channel(error) => write!(f, "channel failed: {error}"),
            Self::Terminal(error) => write!(f, "terminal failed: {error}"),
            Self::Task(error) => write!(f, "task failed: {error}"),
            Self::Telemetry(error) => write!(f, "telemetry failed: {error}"),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Config(error) => Some(error),
            Self::Agent(error) => Some(error.as_ref()),
            Self::Provider(error) => Some(error.as_ref()),
            Self::Tool(error) => Some(error.as_ref()),
            Self::Io(error) => Some(error),
            Self::HttpRequest(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Channel(error) => Some(error.as_ref()),
            Self::Terminal(error) => Some(error),
            Self::Task(error) => Some(error),
            Self::Telemetry(error) => Some(error.as_ref()),
        }
    }
}

impl From<Box<dyn Error + Send + Sync + 'static>> for AppError {
    fn from(error: Box<dyn Error + Send + Sync + 'static>) -> Self {
        Self::Provider(error)
    }
}

impl From<io::Error> for AppError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<reqwest::Error> for AppError {
    fn from(error: reqwest::Error) -> Self {
        Self::HttpRequest(error)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<SendError<AgentEvent>> for AppError {
    fn from(error: SendError<AgentEvent>) -> Self {
        Self::Channel(Box::new(error))
    }
}

impl From<TrySendError<AgentCommand>> for AppError {
    fn from(error: TrySendError<AgentCommand>) -> Self {
        Self::Channel(Box::new(error))
    }
}
