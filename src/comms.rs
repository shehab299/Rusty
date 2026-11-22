use crate::{
    config::provider::ProviderId,
    session::{CacheUsage, Event, SessionData, cache_hit_percent},
};
use ai_sdk::Message;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum AgentEvent {
    ModelResponse {
        content: String,
        input_tokens: Option<u64>,
        cache_hit_tokens: Option<u64>,
        context_tokens: Option<u64>,
    },
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
    Assistant {
        content: String,
        cache_hit_percent: Option<f64>,
    },
    Tool {
        id: String,
        name: String,
        arguments: String,
        output: String,
        success: bool,
    },
    Error(String),
}

impl From<&SessionData> for Vec<SessionMessage> {
    fn from(data: &SessionData) -> Self {
        let mut messages = Vec::new();
        for event in &data.events {
            match event {
                Event::ConversationCleared => messages.clear(),
                Event::PromptSubmitted { content } => {
                    messages.push(SessionMessage::User(content.clone()))
                }
                Event::ModelResponded {
                    message,
                    input_tokens,
                    cache_hit_tokens,
                    ..
                } => {
                    if let Message::Assistant {
                        content,
                        tool_calls,
                        ..
                    } = message.as_ref()
                    {
                        if !content.is_empty() {
                            messages.push(SessionMessage::Assistant {
                                content: content.clone(),
                                cache_hit_percent: cache_hit_percent(
                                    *input_tokens,
                                    *cache_hit_tokens,
                                ),
                            });
                        }
                        for call in tool_calls {
                            messages.push(SessionMessage::Tool {
                                id: call.id.clone(),
                                name: call.name.clone(),
                                arguments: call.arguments.clone(),
                                output: "Not executed: turn interrupted before this tool started."
                                    .into(),
                                success: false,
                            });
                        }
                    }
                }
                Event::ToolFinished {
                    call_id,
                    output,
                    success,
                    ..
                } => set_tool(&mut messages, call_id, output, *success),
                Event::ToolInterrupted { call_id, content } => {
                    set_tool(&mut messages, call_id, content, false)
                }
                Event::ToolStarted { call_id } => set_tool(
                    &mut messages,
                    call_id,
                    "Outcome unknown: inspect tool effects before continuing.",
                    false,
                ),
                Event::TurnFinished {
                    error: Some(error), ..
                } => messages.push(SessionMessage::Error(error.clone())),
                _ => {}
            }
        }
        messages
    }
}

fn set_tool(messages: &mut [SessionMessage], id: &str, content: &str, succeeded: bool) {
    if let Some(SessionMessage::Tool {
        output, success, ..
    }) = messages.iter_mut().rev().find(
        |message| matches!(message, SessionMessage::Tool { id: call_id, .. } if call_id == id),
    ) {
        *output = content.to_owned();
        *success = succeeded;
    }
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
        cache_usage: CacheUsage,
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
    ModelResponse {
        content: String,
        input_tokens: Option<u64>,
        cache_hit_tokens: Option<u64>,
        context_tokens: Option<u64>,
    },
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
    ConversationCleared,
    Notice(String),
}

impl From<AgentEvent> for AppEvent {
    fn from(event: AgentEvent) -> Self {
        match event {
            AgentEvent::ModelResponse {
                content,
                input_tokens,
                cache_hit_tokens,
                context_tokens,
            } => Self::ModelResponse {
                content,
                input_tokens,
                cache_hit_tokens,
                context_tokens,
            },
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
        }
    }
}
