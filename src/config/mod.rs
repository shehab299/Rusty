//! Provider settings loaded from TOML and environment variables.

use config::{Config, ConfigError, Environment, File};
use provider::{ProviderId, ProvidersConfig};
use serde::Deserialize;
use std::path::Path;

pub mod home;
pub mod provider;

#[derive(Clone, Default, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub provider: ProvidersConfig,
    pub model: Option<ModelConfig>,
}

impl AppConfig {
    /// Load an optional `config.toml` in the current working directory.
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from("config.toml")
    }

    /// Load an optional TOML file, then apply `SDK__` environment overrides.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        Config::builder()
            .add_source(File::from(path.as_ref()).required(false))
            .add_source(
                Environment::with_prefix("SDK")
                    .prefix_separator("__")
                    .separator("__"),
            )
            .build()?
            .try_deserialize()
    }
}

#[derive(Clone, Deserialize)]
pub struct ModelConfig {
    pub provider: ProviderId,
    pub model: String,
}
