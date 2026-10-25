use crate::{config::AppConfig, registry::get_tools};
use ai_sdk::{
    GenerateRequest, Message, Tool, ToolCall, generate_text,
    providers::{Model, ModelError, OpenRouter},
};
use reedline::{DefaultPrompt, DefaultPromptSegment, Reedline, Signal};
use serde_json::Value;
use std::collections::HashMap;
use tool::{DynTool, tool};

mod config;
mod registry;

struct Agent {
    context: Vec<Message>,
    tools: HashMap<String, Box<dyn DynTool>>,
}

impl Agent {
    fn new(system_message: String, tools: Vec<Box<dyn DynTool>>) -> Result<Self, ModelError> {
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
        })
    }

    async fn execute_tool(&mut self, call: &ToolCall) -> Value {
        match self.tools.get(&call.name) {
            Some(tool) => match serde_json::from_str(&call.arguments) {
                Ok(arguments) => match tool.execute(arguments).await {
                    Ok(value) => value,
                    Err(error) => serde_json::json!({"error": error.to_string()}),
                },
                Err(error) => serde_json::json!({"error": error.to_string()}),
            },
            None => serde_json::json!({"error": format!("Unknown tool: {}", call.name)}),
        }
    }

    async fn run(&mut self, model: &dyn Model, input: String) -> Result<String, ModelError> {
        self.context.push(Message::user(input));

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

        loop {
            let request = GenerateRequest::from_messages(self.context.clone())
                .with_tools(definitions.clone());

            let response = generate_text(model, request).await?;
            let calls = response.tool_calls.clone();
            let text = response.text.clone();
            self.context.push(response.into_message());

            if calls.is_empty() {
                return Ok(text);
            }

            for call in calls {
                let result = self.execute_tool(&call).await;

                self.context
                    .push(Message::tool_result(call.id, result.to_string()));
            }
        }
    }
}


#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let config = AppConfig::load()?;

    let mut editor = Reedline::create();

    let prompt = DefaultPrompt::new(
        DefaultPromptSegment::Basic("User ".to_string()),
        DefaultPromptSegment::Empty,
    );

    let openrouter = config
        .openrouter
        .ok_or("OpenRouter is not configured; set [openrouter] in config.toml or SDK__OPENROUTER__API_KEY and SDK__OPENROUTER__MODEL")?;

    let model = OpenRouter::new(openrouter.api_key, openrouter.model)?;

    let mut agent = Agent::new(
        "You are a helpful assistant".into(),
        get_tools(),
    )?;

    loop {
        let result = editor.read_line(&prompt)?;

        match result {
            Signal::Success(input) => match agent.run(&model, input).await {
                Ok(content) => println!("Assistant: {content}"),
                Err(error) => eprintln!("Error: {error}"),
            },

            Signal::CtrlC => {
                break;
            }

            _ => {
                continue;
            }
        }
    }

    Ok(())
}
