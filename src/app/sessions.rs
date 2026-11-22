use ai_sdk::Message;
use std::{
    io,
    path::{Path, PathBuf},
};

use super::*;
use crate::comms::{SessionMessage, SessionSummary};
use crate::session::{Session, SessionData};

impl App {
    pub(super) fn list_sessions(&self) -> io::Result<Vec<SessionSummary>> {
        let directory = self.session.log.path.parent().expect("session directory");
        let current = &self.session.log.path;

        let mut sessions = Vec::new();

        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();

            if !entry.file_type()?.is_file()
                || !id.starts_with("session-")
                || !id.ends_with(".jsonl")
            {
                continue;
            }

            let Ok(history) = crate::session::SessionData::load(entry.path()) else {
                continue; // skip damaged session
            };

            let Some(directory) = history.state.directory else {
                continue;
            };

            let title = history
                .state
                .title
                .unwrap_or_else(|| "Empty conversation".into());

            sessions.push(SessionSummary {
                id,
                title,
                directory,
                updated: history.updated,
                current: entry.path() == *current,
            });
        }

        sessions.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
        Ok(sessions)
    }

    pub(super) fn get_session_file(&self, id: &str) -> Result<PathBuf, AppError> {
        if Path::new(id).file_name().and_then(|name| name.to_str()) != Some(id)
            || !id.starts_with("session-")
            || !id.ends_with(".jsonl")
        {
            return Err(AppError::Wal(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid session ID",
            )));
        }

        let path = self
            .session
            .log
            .path
            .parent()
            .expect("session directory")
            .join(id);

        if path == self.session.log.path {
            return Err(AppError::Wal(io::Error::other(
                "This session is already active",
            )));
        }

        // Do not create missing sessions or follow links outside the store.
        let metadata = std::fs::symlink_metadata(&path)?;

        if !metadata.is_file() {
            return Err(AppError::Wal(io::Error::other(
                "Session is not a regular file",
            )));
        }

        Ok(path)
    }

    pub(super) fn resume_session(&mut self, id: &str) -> Result<AppEvent, AppError> {
        let path = self.get_session_file(id)?;
        let mut session = Session::open(&path).map_err(AppError::Wal)?;

        if session.data.state.context.is_empty() {
            return Err(AppError::Wal(io::Error::other("Session is empty")));
        }

        let selection = session
            .data
            .state
            .model
            .clone()
            .map(|(provider, model)| self.provider_manager.select(provider, model))
            .transpose()?;

        let directory =
            session.data.state.directory.clone().ok_or_else(|| {
                AppError::Wal(io::Error::other("Session has no working directory"))
            })?;

        let previous_directory = std::env::current_dir()?;
        std::env::set_current_dir(&directory)?;

        // Recovery appends only interruption facts; it never invokes tools/models.
        let recovered: Result<Vec<SessionMessage>, io::Error> = (|| {
            session.recover()?;
            Ok::<_, io::Error>(Vec::<SessionMessage>::from(&session.data))
        })();

        let messages = match recovered {
            Ok(messages) => messages,
            Err(error) => {
                std::env::set_current_dir(previous_directory)?;
                return Err(AppError::Wal(error));
            }
        };

        let context_limit = selection.as_ref().and_then(|selection| {
            self.catalog
                .model(
                    &selection.provider().to_string().to_lowercase(),
                    selection.model(),
                )
                .and_then(|model| model.limit.context)
        });

        let model = session.data.state.model.clone();
        self.selection = selection;
        self.session = session;

        Ok(AppEvent::SessionResumed {
            context_tokens: self.session.data.state.context_tokens,
            messages,
            directory,
            model,
            context_limit,
        })
    }
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
                Event::ModelResponded { message, .. } => {
                    if let Message::Assistant {
                        content,
                        tool_calls,
                        ..
                    } = message.as_ref()
                    {
                        if !content.is_empty() {
                            messages.push(SessionMessage::Assistant(content.clone()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_borrowed_history_without_changing_it() {
        let data = SessionData {
            events: vec![
                Event::PromptSubmitted {
                    content: "cleared prompt".into(),
                },
                Event::ConversationCleared,
                Event::PromptSubmitted {
                    content: "current prompt".into(),
                },
                Event::ModelResponded {
                    request_sequence: 1,
                    context_tokens: Some(42),
                    message: Box::new(Message::Assistant {
                        content: "working".into(),
                        tool_calls: ["first", "second"]
                            .into_iter()
                            .map(|id| ai_sdk::ToolCall {
                                id: id.into(),
                                name: "shell".into(),
                                arguments: "{}".into(),
                            })
                            .collect(),
                        provider_details: None,
                    }),
                },
                Event::ToolStarted {
                    call_id: "first".into(),
                },
                Event::ToolFinished {
                    call_id: "first".into(),
                    content: "model context".into(),
                    output: "visible output".into(),
                    success: true,
                },
                Event::ToolStarted {
                    call_id: "second".into(),
                },
                Event::ToolInterrupted {
                    call_id: "second".into(),
                    content: "partial output".into(),
                },
                Event::TurnFinished {
                    outcome: crate::session::TurnOutcome::Cancelled,
                    error: Some("cancelled".into()),
                },
            ],
            ..Default::default()
        };
        let original_events = data.events.clone();
        let messages = Vec::<SessionMessage>::from(&data);
        assert_eq!(messages.len(), 5);
        assert!(
            matches!(&messages[0], SessionMessage::User(content) if content == "current prompt")
        );
        assert!(matches!(&messages[1], SessionMessage::Assistant(content) if content == "working"));
        assert!(
            matches!(&messages[2], SessionMessage::Tool { output, success: true, .. } if output == "visible output")
        );
        assert!(
            matches!(&messages[3], SessionMessage::Tool { output, success: false, .. } if output == "partial output")
        );
        assert!(matches!(&messages[4], SessionMessage::Error(content) if content == "cancelled"));
        assert_eq!(data.events, original_events);
    }
}
