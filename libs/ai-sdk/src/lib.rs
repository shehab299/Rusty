#![doc = include_str!("../README.md")]

use crate::providers::{ModelError, Provider};

pub mod providers;
mod types;

pub use async_trait::async_trait;
pub use types::{
    AlternativeLogprob, ContentPart, FinishReason, GenerateRequest, GenerateResponse, Image,
    ImageDetail, ImageMediaType, ImageSource, Logprobs, Message, ProviderDetails, ProviderOptions,
    TokenLogprob, Tool, ToolCall, ToolChoice, Usage, UserContent,
};

pub async fn generate_text(
    provider: &dyn Provider,
    request: GenerateRequest,
) -> Result<GenerateResponse, ModelError> {
    provider.generate(request).await
}
