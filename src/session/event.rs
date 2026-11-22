//! Durable session facts, independent of transient UI events.

use std::path::PathBuf;

use ai_sdk::Message;
use serde::{Deserialize, Serialize};

use crate::config::provider::ProviderId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    SessionStarted {
        system_prompt: String,
        directory: PathBuf,
    },
    ModelSelected {
        provider: ProviderId,
        model: String,
    },
    DirectoryChanged {
        directory: PathBuf,
    },
    PromptSubmitted {
        content: String,
    },
    ModelRequested,
    ModelResponded {
        request_sequence: u64,
        message: Box<Message>,
        #[serde(default)]
        context_tokens: Option<u64>,
        #[serde(default)]
        cache_hit_tokens: Option<u64>,
        #[serde(default)]
        input_tokens: Option<u64>,
    },
    ToolStarted {
        call_id: String,
    },
    ToolFinished {
        call_id: String,
        content: String,
        output: String,
        success: bool,
    },
    ToolInterrupted {
        call_id: String,
        content: String,
    },
    TurnFinished {
        outcome: TurnOutcome,
        error: Option<String>,
    },
    ConversationCleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    Completed,
    Failed,
    Cancelled,
}
