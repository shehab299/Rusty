use crate::config::provider::{ProviderConfig, ProviderId};

pub enum Command {
    ConfigureProvider(ProviderConfig),
    SetModel { provider: ProviderId, model: String },
    SubmitPrompt(String),
    Cancel,
    Shutdown,
}

pub enum Event {
    ProviderConfigured(ProviderId),
    ModelSelected {
        provider: ProviderId,
        model: String,
    },
    SettingsFailed(String),
    Started,
    TextDelta(String),
    Completed,
    Failed(String),
    ToolCall {
        name: String,
        arguments: String,
    },
    ToolResult {
        name: String,
        output: String,
        success: bool,
    },
}
