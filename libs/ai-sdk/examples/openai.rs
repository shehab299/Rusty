use sdk::{
    GenerateRequest, generate_text,
    providers::{ModelError, OpenAI},
};

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let model = OpenAI::new(
        std::env::var("OPENAI_API_KEY")?,
        std::env::var("OPENAI_MODEL")?,
    )?;
    let prompt = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let request = GenerateRequest::new(if prompt.is_empty() {
        "Explain Rust traits in one paragraph".into()
    } else {
        prompt
    });
    println!("{}", generate_text(&model, request).await?.text);
    Ok(())
}
