use sdk::{
    GenerateRequest, Message, Tool, ToolCall, ToolChoice, generate_text,
    providers::{Anthropic, Model, ModelError, OpenAI, OpenRouter},
};
use serde_json::{Value, json};

fn execute(call: &ToolCall) -> Value {
    if call.name != "add" {
        return json!({"error": "Unknown tool"});
    }
    let Ok(arguments) = serde_json::from_str::<Value>(&call.arguments) else {
        return json!({"error": "Arguments must be valid JSON"});
    };
    let (Some(a), Some(b)) = (arguments["a"].as_i64(), arguments["b"].as_i64()) else {
        return json!({"error": "a and b must be integers"});
    };
    match a.checked_add(b) {
        Some(sum) => json!({"sum": sum}),
        None => json!({"error": "Integer overflow"}),
    }
}

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let model: Box<dyn Model> = match std::env::args().nth(1).as_deref().unwrap_or("openrouter") {
        "openrouter" => Box::new(OpenRouter::new(
            std::env::var("OPENROUTER_API_KEY")?,
            std::env::var("OPENROUTER_MODEL")?,
        )?),
        "openai" => Box::new(OpenAI::new(
            std::env::var("OPENAI_API_KEY")?,
            std::env::var("OPENAI_MODEL")?,
        )?),
        "anthropic" => Box::new(Anthropic::new(
            std::env::var("ANTHROPIC_API_KEY")?,
            std::env::var("ANTHROPIC_MODEL")?,
        )?),
        _ => {
            return Err(
                std::io::Error::other("provider must be openrouter, openai, or anthropic").into(),
            );
        }
    };

    let mut request = GenerateRequest::from_messages([
        Message::system("Use the add tool for arithmetic."),
        Message::user("What is 12 + 30?"),
    ])
    .with_tools([Tool::new(
        "add",
        "Add two integers",
        json!({
            "type": "object",
            "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}},
            "required": ["a", "b"],
            "additionalProperties": false,
        }),
    )])
    .with_tool_choice(ToolChoice::Auto);

    for _ in 0..8 {
        let response = generate_text(model.as_ref(), request.clone()).await?;

        if response.tool_calls.is_empty() {
            println!("{}", response.text);
            return Ok(());
        }

        let calls = response.tool_calls.clone();
        request.messages.push(response.into_message());

        for call in calls {
            let result = execute(&call);
            request
                .messages
                .push(Message::tool_result(call.id, result.to_string()));
        }

        request.tool_choice = Some(ToolChoice::Auto);
    }

    Err(std::io::Error::other("generation exceeded the 8-turn limit").into())
}
