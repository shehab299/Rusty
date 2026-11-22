//! Session state projection and event-transition validation.

use std::{collections::BTreeMap, io, path::PathBuf};

use ai_sdk::Message;

use super::{Event, TurnOutcome};
use crate::config::provider::ProviderId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    NotStarted,
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct State {
    pub context: Vec<Message>,
    pub context_tokens: Option<u64>,
    pub directory: Option<PathBuf>,
    pub title: Option<String>,
    pub model: Option<(ProviderId, String)>,
    pub pending_request: Option<u64>,
    pub outcome: Option<TurnOutcome>,
    /// Only the most recent tool batch; prior results remain in context/events.
    pub tools: BTreeMap<String, ToolStatus>,
}

impl State {
    pub(super) fn validate(&self, event: &Event) -> io::Result<()> {
        let valid = match event {
            Event::SessionStarted { .. } => self.context.is_empty(),
            Event::ModelResponded {
                request_sequence,
                message,
                ..
            } => {
                self.pending_request == Some(*request_sequence)
                    && matches!(message.as_ref(), Message::Assistant { .. })
                    && Self::unique_call_ids(message)
            }
            Event::ToolStarted { call_id } => {
                self.tools.get(call_id) == Some(&ToolStatus::NotStarted)
            }
            Event::ToolFinished { call_id, .. } | Event::ToolInterrupted { call_id, .. } => {
                self.tools.get(call_id) == Some(&ToolStatus::Running)
            }
            Event::ModelRequested | Event::PromptSubmitted { .. } => {
                !self.context.is_empty()
                    && self.pending_request.is_none()
                    && !self
                        .tools
                        .values()
                        .any(|status| *status == ToolStatus::Running)
            }
            Event::TurnFinished { .. } => {
                !self.context.is_empty()
                    && !self
                        .tools
                        .values()
                        .any(|status| *status == ToolStatus::Running)
            }
            Event::ConversationCleared => {
                !self.context.is_empty()
                    && self.pending_request.is_none()
                    && !self
                        .tools
                        .values()
                        .any(|status| *status == ToolStatus::Running)
            }
            _ => !self.context.is_empty(),
        };
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid session event transition",
            ));
        }
        Ok(())
    }

    /// Pure state projection: replay never invokes a provider, tool, or chdir.
    pub(super) fn apply(&mut self, sequence: u64, event: &Event) {
        match event {
            Event::SessionStarted {
                system_prompt,
                directory,
            } => {
                self.context.push(Message::system(system_prompt.clone()));
                self.directory = Some(directory.clone());
            }
            Event::ModelSelected { provider, model } => {
                self.model = Some((*provider, model.clone()))
            }
            Event::DirectoryChanged { directory } => self.directory = Some(directory.clone()),
            Event::PromptSubmitted { content } => {
                if self.title.is_none() {
                    self.title = Some(
                        content
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .chars()
                            .take(80)
                            .collect(),
                    );
                }
                self.context.push(Message::user(content.clone()));
                self.outcome = None;
            }
            Event::ModelRequested => self.pending_request = Some(sequence),
            Event::ModelResponded {
                message,
                context_tokens,
                ..
            } => {
                if let Some(tokens) = context_tokens {
                    self.context_tokens = Some(*tokens);
                }
                self.pending_request = None;
                self.tools.clear();
                self.context.push(message.as_ref().clone());
                if let Message::Assistant { tool_calls, .. } = message.as_ref() {
                    for call in tool_calls {
                        self.tools.insert(call.id.clone(), ToolStatus::NotStarted);
                        self.context.push(Message::tool_result(
                            call.id.clone(),
                            "Not executed: turn interrupted before this tool started.",
                        ));
                    }
                }
            }
            Event::ToolStarted { call_id } => {
                self.tools.insert(call_id.clone(), ToolStatus::Running);
                *self.tool_content(call_id) = "Outcome unknown: tool may have run before interruption. Inspect its effects before continuing.".into();
            }
            Event::ToolFinished {
                call_id,
                content,
                success,
                ..
            } => {
                self.tools.insert(
                    call_id.clone(),
                    if *success {
                        ToolStatus::Completed
                    } else {
                        ToolStatus::Failed
                    },
                );
                self.tool_content(call_id).clone_from(content);
            }
            Event::ToolInterrupted { call_id, content } => {
                self.tools.insert(call_id.clone(), ToolStatus::Interrupted);
                self.tool_content(call_id).clone_from(content);
            }
            Event::TurnFinished { outcome, .. } => {
                self.pending_request = None;
                self.outcome = Some(*outcome);
            }
            Event::ConversationCleared => {
                self.context_tokens = Some(0);
                self.context.truncate(1);
                self.tools.clear();
                self.outcome = None;
            }
        }
    }

    pub(super) fn tool_content(&mut self, id: &str) -> &mut String {
        self.context
            .iter_mut()
            .rev()
            .find_map(|message| match message {
                Message::Tool {
                    tool_call_id,
                    content,
                    ..
                } if tool_call_id == id => Some(content),
                _ => None,
            })
            .expect("validated tool batch has a reserved result")
    }

    fn unique_call_ids(message: &Message) -> bool {
        let Message::Assistant { tool_calls, .. } = message else {
            return false;
        };
        let mut ids = std::collections::BTreeSet::new();
        tool_calls.iter().all(|call| ids.insert(&call.id))
    }
}
