//! Public model metadata from [models.dev](https://models.dev).
//!
//! Catalog entries describe all models supplied by the provider

use std::{collections::BTreeMap, time::Duration};

use serde::Deserialize;

use crate::providers::ModelError;

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct Catalog(BTreeMap<String, CatalogProvider>);

impl Catalog {
    pub async fn fetch() -> Result<Self, ModelError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(client
            .get("https://models.dev/api.json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// List a provider's models in stable model-ID order.
    pub fn models(&self, provider: &str) -> impl Iterator<Item = &CatalogModel> {
        self.0
            .get(provider)
            .into_iter()
            .flat_map(|p| p.models.values())
    }
}

#[derive(Debug, Deserialize)]
struct CatalogProvider {
    models: BTreeMap<String, CatalogModel>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tool_call: bool,
    #[serde(default)]
    pub modalities: ModelModalities,
    pub cost: Option<ModelCost>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelModalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

/// USD per million tokens. Missing prices must not be treated as free.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelCost {
    pub input: Option<f64>,
    pub output: Option<f64>,
}

impl CatalogModel {
    pub fn is_free(&self) -> bool {
        self.cost
            .as_ref()
            .is_some_and(|cost| cost.input == Some(0.0) && cost.output == Some(0.0))
    }
}
