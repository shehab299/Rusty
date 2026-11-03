use crate::comms::{Command, Event};
use ai_sdk::{
    GenerateRequest, Message, Tool, ToolCall, generate_text,
    providers::{Model, ModelError},
};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use tokio::sync::mpsc::{Receiver, Sender};
use tool::DynTool;

pub struct Agent {
    context: Vec<Message>,
    tools: HashMap<String, Box<dyn DynTool>>,
    events_sender: Sender<Event>,
    commands_receiver: Receiver<Command>,
}

impl Agent {
    pub fn new(
        system_message: String,
        tools: Vec<Box<dyn DynTool>>,
        events_sender: Sender<Event>,
        commands_receiver: Receiver<Command>,
    ) -> Result<Self, ModelError> {
        let mut registered = HashMap::new();
        for tool in tools {
            let name = tool.definition().name;
            if registered.insert(name.clone(), tool).is_some() {
                return Err(format!("duplicate tool name: {name}").into());
            }
        }

        Ok(Self {
            context: vec![Message::system(system_message)],
            tools: registered,
            events_sender,
            commands_receiver,
        })
    }

    async fn execute_tool(
        tools: &HashMap<String, Box<dyn DynTool>>,
        events_sender: &Sender<Event>,
        call: &ToolCall,
    ) -> Result<Value, ModelError> {
        events_sender
            .send(Event::ToolCall {
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            })
            .await?;

        Ok(match tools.get(&call.name) {
            Some(tool) => match serde_json::from_str(&call.arguments) {
                Ok(arguments) => match tool.execute(arguments).await {
                    Ok(value) => value,
                    Err(error) => serde_json::json!({"error": error.to_string()}),
                },
                Err(error) => serde_json::json!({"error": error.to_string()}),
            },
            None => serde_json::json!({"error": format!("Unknown tool: {}", call.name)}),
        })
    }

    pub async fn run(&mut self, model: &dyn Model) -> Result<(), ModelError> {
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
                Some(prompt) => Command::SubmitPrompt(prompt),
                None => match self.commands_receiver.recv().await {
                    Some(command) => command,
                    None => return Ok(()),
                },
            };

            match command {
                Command::SubmitPrompt(prompt) => {
                    let context_len = self.context.len();
                    self.context.push(Message::user(prompt));
                    self.events_sender.send(Event::Started).await?;

                    let outcome = {
                        let turn = Self::run_turn(
                            &mut self.context,
                            &self.tools,
                            &self.events_sender,
                            model,
                            &definitions,
                        );
                        tokio::pin!(turn);

                        loop {
                            tokio::select! {
                                biased;
                                command = self.commands_receiver.recv() => match command {
                                    Some(Command::SubmitPrompt(prompt)) => pending.push_back(prompt),
                                    Some(Command::Cancel) => break None,
                                    Some(Command::Shutdown) | None => return Ok(()),
                                },
                                result = &mut turn => break Some(result),
                            }
                        }
                    };

                    match outcome {
                        Some(Ok(())) => self.events_sender.send(Event::Completed).await?,
                        Some(Err(error)) => {
                            self.context.truncate(context_len);
                            self.events_sender
                                .send(Event::Failed(error.to_string()))
                                .await?;
                        }
                        None => {
                            // Discard partial tool-call history so the next request is valid.
                            self.context.truncate(context_len);
                            self.events_sender.send(Event::Completed).await?;
                        }
                    }
                }
                Command::Shutdown => {
                    return Ok(());
                }
                Command::Cancel => {}
            };
        }
    }

    async fn run_turn(
        context: &mut Vec<Message>,
        tools: &HashMap<String, Box<dyn DynTool>>,
        events_sender: &Sender<Event>,
        model: &dyn Model,
        definitions: &[Tool],
    ) -> Result<(), ModelError> {
        loop {
            let request =
                GenerateRequest::from_messages(context.clone()).with_tools(definitions.to_vec());
            let response = generate_text(model, request).await?;
            let calls = response.tool_calls.clone();
            let text = response.text.clone();
            context.push(response.into_message());

            if !text.is_empty() {
                events_sender.send(Event::TextDelta(text)).await?;
            }
            if calls.is_empty() {
                return Ok(());
            }

            for call in calls {
                let result = Self::execute_tool(tools, events_sender, &call).await;

                let (event, message) = match result {
                    Ok(v) => (
                        Event::ToolResult {
                            name: call.name,
                            output: serde_json::to_string_pretty(&v)?,
                            success: true,
                        },
                        Message::tool_result(call.id, v.to_string()),
                    ),

                    Err(e) => (
                        Event::ToolResult {
                            name: call.name,
                            output: e.to_string(),
                            success: false,
                        },
                        Message::tool_result(call.id, e.to_string()),
                    ),
                };

                events_sender.send(event).await?;
                context.push(message);
            }
        }
    }
}
