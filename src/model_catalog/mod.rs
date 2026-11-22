//! The models.dev metadata used by the application's model picker.
//! Fetch at startup and fall back to the last persisted catalog when offline.

use crate::{config::home::get_cached_model_catalog_path, error::AppError};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path, time::Duration};

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct Catalog(BTreeMap<String, CatalogProvider>);

impl Catalog {
    pub async fn from_cached() -> Result<Self, AppError> {
        let path = get_cached_model_catalog_path()?;
        let bytes = tokio::fs::read(path).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn from_internet() -> Result<Self, AppError> {
        let path = get_cached_model_catalog_path()?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        let bytes = client
            .get("https://models.dev/api.json")
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec();

        let catalog = serde_json::from_slice::<Catalog>(&bytes)?;
        Self::persist_catalog(&path, &bytes).await?;
        Ok(catalog)
    }

    async fn persist_catalog(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = async {
            tokio::fs::write(&temporary, bytes).await?;
            tokio::fs::rename(&temporary, path).await
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(temporary).await;
        }
        result
    }

    pub fn model(&self, provider: &str, model: &str) -> Option<&CatalogModel> {
        self.0.get(provider)?.models.get(model)
    }

    /// Check whether a provider's catalog contains the given model ID.
    pub fn has_model(&self, provider: &str, model: &str) -> bool {
        self.0
            .get(provider)
            .is_some_and(|p| p.models.contains_key(model))
    }

    /// List a provider's models in stable model-ID order.
    pub fn models(&self, provider: &str) -> impl Iterator<Item = &CatalogModel> + use<'_> {
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
    pub modalities: ModelModalities,
    pub cost: Option<ModelCost>,
    #[serde(default)]
    pub limit: ModelLimits,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelModalities {
    #[serde(default)]
    pub output: Vec<String>,
}

/// USD per million tokens. Missing prices must not be treated as free.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelCost {
    pub input: Option<f64>,
    pub output: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
pub struct ModelLimits {
    #[serde(default)]
    pub context: Option<u64>,
    pub output: Option<u64>,
}

impl CatalogModel {
    pub fn is_free(&self) -> bool {
        self.cost
            .as_ref()
            .is_some_and(|cost| cost.input == Some(0.0) && cost.output == Some(0.0))
    }
}
