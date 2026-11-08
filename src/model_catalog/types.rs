//! The models.dev metadata used by the application's model picker.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct Catalog(BTreeMap<String, CatalogProvider>);

impl Catalog {
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

impl CatalogModel {
    pub fn is_free(&self) -> bool {
        self.cost
            .as_ref()
            .is_some_and(|cost| cost.input == Some(0.0) && cost.output == Some(0.0))
    }
}
