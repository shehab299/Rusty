use sdk::{
    GenerateRequest, generate_text,
    providers::{Anthropic, ModelError},
};

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let model = Anthropic::new(
        std::env::var("ANTHROPIC_API_KEY")?,
        std::env::var("ANTHROPIC_MODEL")?,
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
