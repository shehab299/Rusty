use crate::{config::provider::ProviderId, provider_manager::ModelSelection};

pub enum AgentCommand {
    SetModel(ModelSelection),
    SubmitPrompt(String),
    Cancel,
    Shutdown,
}

pub enum AgentEvent {
    ModelSelected {
        provider: ProviderId,
        model: String,
    },
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
