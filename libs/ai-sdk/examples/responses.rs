use ai_sdk::{
    GenerateRequest, generate_text,
    providers::{ModelError, OpenAI, openai::OpenAIApi},
};

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let provider = OpenAI::new(std::env::var("OPENAI_API_KEY")?)?.with_api(OpenAIApi::Responses);
    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5".into());
    let prompt = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let prompt = if prompt.is_empty() {
        "Explain Rust traits in one paragraph".into()
    } else {
        prompt
    };
    println!(
        "{}",
        generate_text(&provider, GenerateRequest::new(model, prompt))
            .await?
            .text
    );
    Ok(())
}
