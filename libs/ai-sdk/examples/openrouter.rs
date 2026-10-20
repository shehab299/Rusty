use sdk::{
    GenerateRequest, generate_text,
    providers::{ModelError, OpenRouter},
};

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let model = OpenRouter::new(
        std::env::var("OPENROUTER_API_KEY")?,
        std::env::var("OPENROUTER_MODEL")?,
    )?;
    let prompt = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let request = GenerateRequest::new(if prompt.is_empty() {
        "Explain Rust traits in one paragraph".into()
    } else {
        prompt
    });
    let response = generate_text(&model, request).await?;

    println!("{}", response.text);
    Ok(())
}
