use crate::{
    comms::{AgentCommand, AgentEvent},
    error::AppError,
    provider_manager::ModelSelection,
};

use ai_sdk::{GenerateRequest, Message, Tool, ToolCall, generate_text, providers::Provider};
use tool::{DynTool, ToolContext, ToolDelta};

use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use tokio::sync::mpsc::{Receiver, Sender};

fn display_tool_result(value: &Value) -> Result<String, serde_json::Error> {
    match value {
        Value::Null => Ok(String::new()),
        Value::String(text) => Ok(text.clone()),
        _ => serde_json::to_string_pretty(value),
    }
}

pub struct Agent {
    selection: Option<ModelSelection>,
    context: Vec<Message>,
    tools: HashMap<String, Box<dyn DynTool>>,
    events_sender: Sender<AgentEvent>,
    commands_receiver: Receiver<AgentCommand>,
}

impl Agent {
    pub fn new(
        system_message: String,
        tools: Vec<Box<dyn DynTool>>,
        events_sender: Sender<AgentEvent>,
        commands_receiver: Receiver<AgentCommand>,
    ) -> Result<Self, AppError> {
        let mut registered = HashMap::new();
        for tool in tools {
            let name = tool.definition().name;
            if registered.insert(name.clone(), tool).is_some() {
                return Err(AppError::Agent(
                    format!("duplicate tool name: {name}").into(),
                ));
            }
        }

        Ok(Self {
            selection: None,
            context: vec![Message::system(system_message)],
            tools: registered,
            events_sender,
            commands_receiver,
        })
    }

    async fn execute_tool(
        tools: &HashMap<String, Box<dyn DynTool>>,
        call: &ToolCall,
        events_sender: &Sender<AgentEvent>,
    ) -> Result<Message, AppError> {
        let (tool_tx, mut tool_rx) = tokio::sync::mpsc::channel::<ToolDelta>(100);

        events_sender
            .send(AgentEvent::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            })
            .await?;

        let result = async {
            let tool = tools
                .get(&call.name)
                .ok_or_else(|| AppError::Agent(format!("Unknown tool: {}", call.name).into()))?;

            let result = {
                let arguments = serde_json::from_str(&call.arguments)?;
                let context = ToolContext::new(call.id.clone(), tool_tx);

                let execution = tool.execute(arguments, context);
                tokio::pin!(execution);

                loop {
                    tokio::select! {
                        biased;
                        Some(delta) = tool_rx.recv() => {
                            events_sender.send(AgentEvent::ToolDelta(delta)).await?;
                        }
                        result = &mut execution => break result,
                    }
                }
            };

            while let Ok(delta) = tool_rx.try_recv() {
                events_sender.send(AgentEvent::ToolDelta(delta)).await?;
            }

            result.map_err(AppError::Tool)
        }
        .await;

        let (output, content, success) = match result {
            Ok(value) => (display_tool_result(&value)?, value.to_string(), true),
            Err(error) => {
                let output = error.to_string();
                (output.clone(), output, false)
            }
        };

        events_sender
            .send(AgentEvent::ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                output,
                success,
            })
            .await?;

        Ok(Message::tool_result(call.id.clone(), content))
    }

    pub async fn run(&mut self) -> Result<(), AppError> {
        let definitions: Vec<Tool> = self
            .tools
            .values()
            .map(|tool| {
                let definition = tool.definition();
                Tool::new(
                    definition.name,
                    definition.description,
                    definition.parameters,
                )
            })
            .collect();

        let mut pending = VecDeque::new();

        loop {
            let command = match pending.pop_front() {
                Some(command) => command,
                None => match self.commands_receiver.recv().await {
                    Some(command) => command,
                    None => return Ok(()),
                },
            };

            match command {
                AgentCommand::SetModel(selection) => {
                    let event = AgentEvent::ModelSelected {
                        provider: selection.provider(),
                        model: selection.model().to_owned(),
                    };
                    self.selection = Some(selection);
                    self.events_sender.send(event).await?;
                }
                AgentCommand::SubmitPrompt(prompt) => {
                    let Some(selection) = self.selection.as_ref() else {
                        self.events_sender
                            .send(AgentEvent::Failed(
                                "No model selected; configure a provider and select a model first"
                                    .into(),
                            ))
                            .await?;
                        continue;
                    };
                    let context_len = self.context.len();
                    self.context.push(Message::user(prompt));
                    self.events_sender.send(AgentEvent::Started).await?;

                    let outcome = {
                        let turn = Self::run_turn(
                            &mut self.context,
                            &self.tools,
                            &self.events_sender,
                            selection.client(),
                            selection.model().to_owned(),
                            &definitions,
                        );
                        tokio::pin!(turn);

                        loop {
                            tokio::select! {
                                biased;
                                command = self.commands_receiver.recv() => match command {
                                    Some(AgentCommand::Cancel) => break None,
                                    Some(AgentCommand::Shutdown) | None => return Ok(()),
                                    Some(command) => pending.push_back(command),
                                },
                                result = &mut turn => break Some(result),
                            }
                        }
                    };

                    match outcome {
                        Some(Ok(())) => self.events_sender.send(AgentEvent::Completed).await?,
                        Some(Err(error)) => {
                            self.context.truncate(context_len);
                            self.events_sender
                                .send(AgentEvent::Failed(error.to_string()))
                                .await?;
                        }
                        None => {
                            // Discard partial tool-call history so the next request is valid.
                            self.context.truncate(context_len);
                            self.events_sender.send(AgentEvent::Completed).await?;
                        }
                    }
                }
                AgentCommand::Shutdown => {
                    return Ok(());
                }
                AgentCommand::Cancel => {}
            };
        }
    }

    async fn run_turn(
        context: &mut Vec<Message>,
        tools: &HashMap<String, Box<dyn DynTool>>,
        events_sender: &Sender<AgentEvent>,
        provider: &dyn Provider,
        model: String,
        definitions: &[Tool],
    ) -> Result<(), AppError> {
        loop {
            let request = GenerateRequest::from_messages(model.clone(), context.clone())
                .with_tools(definitions.to_vec());
            let response = generate_text(provider, request).await?;
            let calls = response.tool_calls.clone();
            let text = response.text.clone();
            context.push(response.into_message());

            if !text.is_empty() {
                events_sender.send(AgentEvent::TextDelta(text)).await?;
            }

            if calls.is_empty() {
                return Ok(());
            }

            for call in calls {
                let message = Self::execute_tool(tools, &call, events_sender).await?;
                context.push(message);
            }
        }
    }
}
