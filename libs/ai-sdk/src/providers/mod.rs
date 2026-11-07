use std::error::Error;

use crate::models::{CatalogModel, Catalog};
use crate::{GenerateRequest, GenerateResponse, async_trait};

pub type ModelError = Box<dyn Error + Send + Sync + 'static>;

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;

    async fn list_models(&self) -> Result<Vec<CatalogModel>, ModelError> {
        let catalog = Catalog::fetch().await?;
        Ok(catalog.models(self.id()).cloned().collect())
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError>;
}

pub mod anthropic;
pub mod openai;
pub mod openrouter;

pub use anthropic::Anthropic;
pub use openai::OpenAI;
pub use openrouter::OpenRouter;
