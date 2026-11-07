use crate::{
    comms::{Command, Event},
    config::provider::{ProviderConfig, ProviderId, ProvidersConfig},
};
use ai_sdk::{
    GenerateRequest, Message, Tool, ToolCall, generate_text,
    providers::{ModelError, Provider},
};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use tokio::sync::mpsc::{Receiver, Sender};
use tool::DynTool;

pub struct Agent {
    provider_config: ProvidersConfig,
    selection: Option<ModelSelection>,
    context: Vec<Message>,
    tools: HashMap<String, Box<dyn DynTool>>,
    events_sender: Sender<Event>,
    commands_receiver: Receiver<Command>,
}

struct ModelSelection {
    provider: ProviderId,
    model: String,
    client: Box<dyn Provider>,
}

impl Agent {
    pub fn new(
        system_message: String,
        tools: Vec<Box<dyn DynTool>>,
        events_sender: Sender<Event>,
        commands_receiver: Receiver<Command>,
        provider_config: ProvidersConfig,
    ) -> Result<Self, ModelError> {
        let mut registered = HashMap::new();
        for tool in tools {
            let name = tool.definition().name;
            if registered.insert(name.clone(), tool).is_some() {
                return Err(format!("duplicate tool name: {name}").into());
            }
        }

        Ok(Self {
            provider_config,
            selection: None,
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

    fn configure_provider(&mut self, settings: ProviderConfig) -> Result<(), ModelError> {
        let client = settings.build()?;
        if let Some(selection) = self.selection.as_mut()
            && selection.provider == settings.id()
        {
            selection.client = client;
        }
        self.provider_config.configure(settings);
        Ok(())
    }

    fn set_model(&mut self, provider: ProviderId, model: String) -> Result<(), ModelError> {
        if model.trim().is_empty() {
            return Err("Model must not be empty".into());
        }
        let settings = self
            .provider_config
            .settings(provider)
            .ok_or_else(|| format!("{provider} is not configured"))?;
        let client = settings.build()?;
        self.selection = Some(ModelSelection {
            provider,
            model,
            client,
        });
        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), ModelError> {
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
                Command::ConfigureProvider(settings) => {
                    let id = settings.id();
                    let event = match self.configure_provider(settings) {
                        Ok(()) => Event::ProviderConfigured(id),
                        Err(error) => Event::SettingsFailed(error.to_string()),
                    };
                    self.events_sender.send(event).await?;
                }
                Command::SetModel { provider, model } => {
                    let event = match self.set_model(provider, model.clone()) {
                        Ok(()) => Event::ModelSelected { provider, model },
                        Err(error) => Event::SettingsFailed(error.to_string()),
                    };
                    self.events_sender.send(event).await?;
                }
                Command::SubmitPrompt(prompt) => {
                    let Some(selection) = self.selection.as_ref() else {
                        self.events_sender
                            .send(Event::Failed(
                                "No model selected; configure a provider and select a model first"
                                    .into(),
                            ))
                            .await?;
                        continue;
                    };
                    let context_len = self.context.len();
                    self.context.push(Message::user(prompt));
                    self.events_sender.send(Event::Started).await?;

                    let outcome = {
                        let turn = Self::run_turn(
                            &mut self.context,
                            &self.tools,
                            &self.events_sender,
                            selection.client.as_ref(),
                            selection.model.clone(),
                            &definitions,
                        );
                        tokio::pin!(turn);

                        loop {
                            tokio::select! {
                                biased;
                                command = self.commands_receiver.recv() => match command {
                                    Some(Command::Cancel) => break None,
                                    Some(Command::Shutdown) | None => return Ok(()),
                                    Some(command) => pending.push_back(command),
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
        provider: &dyn Provider,
        model: String,
        definitions: &[Tool],
    ) -> Result<(), ModelError> {
        loop {
            let request = GenerateRequest::from_messages(model.clone(), context.clone())
                .with_tools(definitions.to_vec());
            let response = generate_text(provider, request).await?;
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
