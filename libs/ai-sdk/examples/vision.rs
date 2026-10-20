use std::{io, path::Path};

use sdk::{
    ContentPart, GenerateRequest, Image, ImageMediaType, Message, generate_text,
    providers::{Anthropic, Model, ModelError, OpenAI, OpenRouter},
};

fn load_image(source: String) -> Result<Image, ModelError> {
    if source.starts_with("http://") || source.starts_with("https://") {
        return Ok(Image::url(source));
    }

    let extension = Path::new(&source)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let media_type = match extension.as_str() {
        "jpg" | "jpeg" => ImageMediaType::Jpeg,
        "png" => ImageMediaType::Png,
        "gif" => ImageMediaType::Gif,
        "webp" => ImageMediaType::Webp,
        _ => return Err(io::Error::other("expected a JPEG, PNG, GIF, or WebP file").into()),
    };

    Ok(Image::from_bytes(media_type, std::fs::read(source)?))
}

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let mut args = std::env::args().skip(1);
    let provider = args.next().ok_or_else(|| {
        io::Error::other("usage: vision <openai|anthropic|openrouter> <URL|file> [question]")
    })?;
    let source = args
        .next()
        .ok_or_else(|| io::Error::other("an image URL or file path is required"))?;
    let question = args.collect::<Vec<_>>().join(" ");
    let question = if question.is_empty() {
        "Describe this image.".into()
    } else {
        question
    };

    let model: Box<dyn Model> = match provider.as_str() {
        "openai" => Box::new(OpenAI::new(
            std::env::var("OPENAI_API_KEY")?,
            std::env::var("OPENAI_MODEL")?,
        )?),
        "anthropic" => Box::new(Anthropic::new(
            std::env::var("ANTHROPIC_API_KEY")?,
            std::env::var("ANTHROPIC_MODEL")?,
        )?),
        "openrouter" => Box::new(OpenRouter::new(
            std::env::var("OPENROUTER_API_KEY")?,
            std::env::var("OPENROUTER_MODEL")?,
        )?),
        _ => return Err(io::Error::other("unknown provider").into()),
    };

    let request = GenerateRequest::from_messages([Message::user([
        ContentPart::text(question),
        ContentPart::image(load_image(source)?),
    ])]);
    let response = generate_text(model.as_ref(), request).await?;

    println!("{}", response.text);

    Ok(())
}
