use std::error::Error;

use crate::{GenerateRequest, GenerateResponse, async_trait};

pub type ModelError = Box<dyn Error + Send + Sync + 'static>;

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError>;
}

pub mod anthropic;
pub mod openai;
pub mod openrouter;

pub use anthropic::Anthropic;
pub use openai::OpenAI;
pub use openrouter::OpenRouter;
