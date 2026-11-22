use crate::config::provider::ProviderId;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum AgentEvent {
    TextDelta(String),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolDelta(tool::ToolDelta),
    ToolResult {
        id: String,
        name: String,
        output: String,
        success: bool,
    },
    ContextUpdate(u64),
}

pub enum AppCommand {
    SetModel {
        provider: ProviderId,
        model: String,
    },
    ConfigureProvider {
        provider: ProviderId,
        api_key: String,
    },
    SubmitPrompt(String),
    Cancel,
    Shutdown,
    ClearConversation,
    SetDirectory(String),
    ListSessions,
    ResumeSession(String),
}

#[derive(Clone, Debug)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub directory: PathBuf,
    pub updated: u64,
    pub current: bool,
}

#[derive(Clone, Debug)]
pub enum SessionMessage {
    User(String),
    Assistant(String),
    Tool {
        id: String,
        name: String,
        arguments: String,
        output: String,
        success: bool,
    },
    Error(String),
}

#[derive(Clone, Debug)]
pub struct ModelOption {
    pub provider: ProviderId,
    pub id: String,
    pub name: String,
    pub is_free: bool,
}

pub enum AppEvent {
    SessionsListed(Vec<SessionSummary>),
    SessionFailed(String),
    SessionResumed {
        context_tokens: Option<u64>,
        messages: Vec<SessionMessage>,
        directory: PathBuf,
        model: Option<(ProviderId, String)>,
        context_limit: Option<u64>,
    },
    ModelSelected {
        provider: ProviderId,
        model: String,
        context_limit: Option<u64>,
    },
    ModelSelectionFailed {
        error: String,
    },
    ProvidersUpdated {
        configured: Vec<ProviderId>,
        models: Vec<ModelOption>,
    },
    ProviderConfigured {
        provider: ProviderId,
    },
    ProviderConfigurationFailed {
        provider: ProviderId,
        error: String,
    },
    DirectoryChanged(PathBuf),
    Started,
    TextDelta(String),
    Completed,
    Failed(String),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolDelta(tool::ToolDelta),
    ToolResult {
        id: String,
        name: String,
        output: String,
        success: bool,
    },
    ContextUpdate(u64),
    ConversationCleared,
    Notice(String),
}

impl From<AgentEvent> for AppEvent {
    fn from(event: AgentEvent) -> Self {
        match event {
            AgentEvent::TextDelta(content) => Self::TextDelta(content),
            AgentEvent::ToolCall {
                id,
                name,
                arguments,
            } => Self::ToolCall {
                id,
                name,
                arguments,
            },
            AgentEvent::ToolDelta(delta) => Self::ToolDelta(delta),
            AgentEvent::ToolResult {
                id,
                name,
                output,
                success,
            } => Self::ToolResult {
                id,
                name,
                output,
                success,
            },
            AgentEvent::ContextUpdate(tokens) => Self::ContextUpdate(tokens),
        }
    }
}
