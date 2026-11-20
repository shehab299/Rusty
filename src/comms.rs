use crate::config::provider::ProviderId;
use std::path::PathBuf;


#[derive(Clone, Debug)]
pub enum TurnEvent {
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
}

#[derive(Clone, Debug)]
pub struct ModelOption {
    pub provider: ProviderId,
    pub id: String,
    pub name: String,
    pub is_free: bool,
}

pub enum AppEvent {
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

impl From<TurnEvent> for AppEvent {
    fn from(event: TurnEvent) -> Self {
        match event {
            TurnEvent::TextDelta(content) => Self::TextDelta(content),
            TurnEvent::ToolCall {
                id,
                name,
                arguments,
            } => Self::ToolCall {
                id,
                name,
                arguments,
            },
            TurnEvent::ToolDelta(delta) => Self::ToolDelta(delta),
            TurnEvent::ToolResult {
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
            TurnEvent::ContextUpdate(tokens) => Self::ContextUpdate(tokens),
        }
    }
}
