//! Turn execution. Durable facts are journaled at model/tool boundaries;
//! transient output still travels independently over the UI channel.
use std::collections::HashMap;

use ai_sdk::{GenerateRequest, Tool, ToolCall, Usage, generate_text};
use serde_json::Value;
use tokio::sync::mpsc::{self, Sender};
use tool::{DynTool, ToolContext, ToolDelta};
use tracing::Instrument;

use crate::session::{Event, Session};
use crate::{comms::AgentEvent, error::AppError, provider_manager::ModelSelection};

pub struct Agent {
    tools: HashMap<String, Box<dyn DynTool>>,
    definitions: Vec<Tool>,
}

impl Agent {
    pub fn new(tools: Vec<Box<dyn DynTool>>) -> Result<Self, AppError> {
        let mut registered = HashMap::new();
        let mut definitions = Vec::new();
        for tool in tools {
            let definition = tool.definition();
            let name = definition.name.clone();
            definitions.push(Tool::new(
                definition.name,
                definition.description,
                definition.parameters,
            ));
            if registered.insert(name.clone(), tool).is_some() {
                return Err(AppError::Agent(
                    format!("duplicate tool name: {name}").into(),
                ));
            }
        }
        Ok(Self {
            tools: registered,
            definitions,
        })
    }

    /// Updates session-owned context, including partial output on cancellation.
    pub async fn run_turn(
        &self,
        selection: &ModelSelection,
        events: &Sender<AgentEvent>,
        session: &mut Session,
    ) -> Result<(), AppError> {
        let provider = selection.client();
        let model = selection.model().to_owned();
        loop {
            let request_sequence = session
                .record(Event::ModelRequested)
                .map_err(AppError::Wal)?;
            let request =
                GenerateRequest::from_messages(model.clone(), session.data.state.context.clone())
                    .with_tools(self.definitions.clone());

            let request_span = tracing::info_span!(
                "gen_ai.request",
                gen_ai.provider.name = provider.id(),
                gen_ai.request.model = %model,
                gen_ai.response.model = tracing::field::Empty,
                gen_ai.usage.input_tokens = tracing::field::Empty,
                gen_ai.usage.output_tokens = tracing::field::Empty,
                outcome = tracing::field::Empty,
                otel.status_code = tracing::field::Empty,
            );

            let response = async {
                let mut operation = crate::telemetry::Operation::new(tracing::Span::current());
                let result = generate_text(provider, request).await;
                let span = tracing::Span::current();
                operation.complete(result.is_ok());
                if let Ok(response) = &result {
                    if let Some(model) = &response.model {
                        span.record("gen_ai.response.model", model.as_str());
                    }
                    if let Some(usage) = &response.usage {
                        if let Some(tokens) = usage.input_tokens {
                            span.record("gen_ai.usage.input_tokens", tokens);
                        }
                        if let Some(tokens) = usage.output_tokens {
                            span.record("gen_ai.usage.output_tokens", tokens);
                        }
                    }
                }
                result
            }
            .instrument(request_span)
            .await?;

            let usage = response.usage.clone();
            let calls = response.tool_calls.clone();

            let text = response.text.clone();
            let message = response.into_message();
            session
                .record(Event::ModelResponded {
                    request_sequence,
                    context_tokens: usage.as_ref().and_then(|usage| {
                        usage
                            .input_tokens
                            .map(|input| input.saturating_add(usage.output_tokens.unwrap_or(0)))
                    }),
                    message: Box::new(message),
                })
                .map_err(AppError::Wal)?;
            // Recording the response reserves every tool result before any await.

            if let Some(usage) = &usage {
                Self::update_context(usage, events).await?;
            }
            if !text.is_empty() {
                events.send(AgentEvent::TextDelta(text)).await?;
            }

            if calls.is_empty() {
                return Ok(());
            }

            for call in calls {
                let tool_span = tracing::info_span!(
                    "tool.execute",
                    tool.name = %call.name,
                    tool.call.id = %call.id,
                    tool.success = tracing::field::Empty,
                    outcome = tracing::field::Empty,
                    otel.status_code = tracing::field::Empty,
                );

                let mut operation = crate::telemetry::Operation::new(tool_span.clone());

                let result = self
                    .execute_tool(&call, events, session)
                    .instrument(tool_span.clone())
                    .await;

                let success = result.as_ref().is_ok_and(|success| *success);
                tool_span.record("tool.success", success);
                operation.complete(success);

                result?;
            }
        }
    }

    async fn execute_tool(
        &self,
        call: &ToolCall,
        events: &Sender<AgentEvent>,
        session: &mut Session,
    ) -> Result<bool, AppError> {
        let (tool_tx, tool_rx) = mpsc::channel::<ToolDelta>(100);

        events
            .send(AgentEvent::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            })
            .await?;

        session
            .record(Event::ToolStarted {
                call_id: call.id.clone(),
            })
            .map_err(AppError::Wal)?;
        let mut progress = ToolProgress {
            output: String::new(),
            session,
            call_id: &call.id,
            receiver: tool_rx,
            completed: false,
        };
        let mut visible_output = String::new();

        let result = match self.tools.get(&call.name) {
            None => Err(AppError::Agent(
                format!("Unknown tool: {}", call.name).into(),
            )),

            Some(tool) => match serde_json::from_str::<Value>(&call.arguments) {
                Err(error) => Err(AppError::Json(error)),
                Ok(arguments) => {
                    let context = ToolContext::new(call.id.clone(), tool_tx);
                    let execution = tool.execute(arguments, context);
                    tokio::pin!(execution);
                    let result = loop {
                        tokio::select! {
                            biased;
                            Some(delta) = progress.receiver.recv() => {
                                progress.output.push_str(&delta.content);
                                visible_output.push_str(&delta.content);
                                events.send(AgentEvent::ToolDelta(delta)).await?;
                            }
                            result = &mut execution => break result,
                        }
                    };
                    result.map_err(AppError::Tool)
                }
            },
        };

        let mut final_events = Vec::new();
        while let Ok(delta) = progress.receiver.try_recv() {
            progress.output.push_str(&delta.content);
            visible_output.push_str(&delta.content);
            final_events.push(AgentEvent::ToolDelta(delta));
        }

        let (display_output, final_content, success) = match result {
            Ok(value) => (display_tool_result(&value)?, value.to_string(), true),
            Err(error) => {
                let output = error.to_string();
                (output.clone(), output, false)
            }
        };

        if progress.output.is_empty() || !display_output.is_empty() {
            if !progress.output.is_empty() && !progress.output.ends_with('\n') {
                progress.output.push('\n');
            }
            progress.output.push_str(&final_content);
        }

        // Record the completed result before sending its display event.
        progress.completed = true;
        if !display_output.is_empty() {
            if !visible_output.is_empty() && !visible_output.ends_with('\n') {
                visible_output.push('\n');
            }
            visible_output.push_str(&display_output);
        }
        progress
            .session
            .record(Event::ToolFinished {
                call_id: call.id.clone(),
                content: progress.output.clone(),
                output: visible_output,
                success,
            })
            .map_err(AppError::Wal)?;
        for event in final_events {
            events.send(event).await?;
        }
        events
            .send(AgentEvent::ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                output: display_output,
                success,
            })
            .await?;

        Ok(success)
    }

    async fn update_context(usage: &Usage, events: &Sender<AgentEvent>) -> Result<(), AppError> {
        if let Some(input) = usage.input_tokens {
            events
                .send(AgentEvent::ContextUpdate(
                    input.saturating_add(usage.output_tokens.unwrap_or(0)),
                ))
                .await?;
        }
        Ok(())
    }
}

// Preserve output already emitted by a tool even if its execution future is
// dropped before the agent gets to receive the remaining deltas.
struct ToolProgress<'a> {
    output: String,
    session: &'a mut Session,
    call_id: &'a str,
    receiver: mpsc::Receiver<ToolDelta>,
    completed: bool,
}

impl Drop for ToolProgress<'_> {
    fn drop(&mut self) {
        while let Ok(delta) = self.receiver.try_recv() {
            self.output.push_str(&delta.content);
        }
        if !self.completed {
            self.output.insert_str(
                0,
                "Interrupted: tool execution did not finish. Side effects may have occurred.\n",
            );
            self.session
                .preserve_tool_output(self.call_id, &self.output);
        }
    }
}

fn display_tool_result(value: &Value) -> Result<String, serde_json::Error> {
    match value {
        Value::Null => Ok(String::new()),
        Value::String(text) => Ok(text.clone()),
        _ => serde_json::to_string_pretty(value),
    }
}

#[cfg(test)]
mod tests;
