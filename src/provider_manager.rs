//! Runtime provider clients owned by the application, not the agent.

use std::{collections::HashMap, sync::Arc};

use ai_sdk::providers::{Anthropic, ModelError, OpenAI, OpenRouter, Provider};

use crate::config::provider::{ProviderConfig, ProviderId, ProvidersConfig};

#[derive(Default)]
pub struct ProviderManager {
    clients: HashMap<ProviderId, Arc<dyn Provider>>,
}

impl ProviderManager {
    pub fn from_config(config: ProvidersConfig) -> Result<Self, ModelError> {
        let mut manager = Self::default();
        for provider in config.into_providers() {
            manager.configure(provider)?;
        }
        Ok(manager)
    }

    /// Build before replacing the existing client. Raw settings aren't retained.
    pub fn configure(&mut self, config: ProviderConfig) -> Result<ProviderId, ModelError> {
        let id = config.id();
        let client: Arc<dyn Provider> = match config {
            ProviderConfig::OpenAI(config) => Arc::new(OpenAI::new(config.api_key)?),
            ProviderConfig::OpenRouter(config) => Arc::new(OpenRouter::new(config.api_key)?),
            ProviderConfig::Anthropic(config) => Arc::new(Anthropic::new(config.api_key)?),
        };
        self.clients.insert(id, client);
        Ok(id)
    }

    pub fn configured(&self) -> Vec<ProviderId> {
        ProviderId::ALL
            .into_iter()
            .filter(|id| self.clients.contains_key(id))
            .collect()
    }

    pub fn select(
        &self,
        provider: ProviderId,
        model: String,
    ) -> Result<ModelSelection, ModelError> {
        let client = self
            .clients
            .get(&provider)
            .ok_or_else(|| format!("{provider} is not configured"))?;
        ModelSelection::new(provider, model, Arc::clone(client))
    }
}

pub struct ModelSelection {
    provider: ProviderId,
    model: String,
    client: Arc<dyn Provider>,
}

impl ModelSelection {
    pub fn new(
        provider: ProviderId,
        model: String,
        client: Arc<dyn Provider>,
    ) -> Result<Self, ModelError> {
        if model.trim().is_empty() {
            return Err("Model must not be empty".into());
        }
        Ok(Self {
            provider,
            model,
            client,
        })
    }

    pub fn provider(&self) -> ProviderId {
        self.provider
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn client(&self) -> &dyn Provider {
        self.client.as_ref()
    }
}

