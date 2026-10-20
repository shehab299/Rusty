#![doc = include_str!("../README.md")]

use crate::providers::{Model, ModelError};

pub mod providers;
mod types;

pub use async_trait::async_trait;
pub use types::{
    AlternativeLogprob, ContentPart, FinishReason, GenerateRequest, GenerateResponse, Image,
    ImageDetail, ImageMediaType, ImageSource, Logprobs, Message, ProviderDetails, ProviderOptions,
    TokenLogprob, Tool, ToolCall, ToolChoice, Usage, UserContent,
};

pub async fn generate_text(
    model: &dyn Model,
    request: GenerateRequest,
) -> Result<GenerateResponse, ModelError> {
    model.generate(request).await
}
