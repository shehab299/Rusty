//! Provider settings loaded from TOML and environment variables.

use std::path::Path;

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Clone, Default, Deserialize)]
pub struct AppConfig {
    pub openai: Option<OpenAIConfig>,
    pub openrouter: Option<OpenRouterConfig>,
    pub anthropic: Option<AnthropicConfig>,
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
pub struct OpenAIConfig {
    pub api_key: String,
    pub model: String,
}

#[derive(Clone, Deserialize)]
pub struct OpenRouterConfig {
    pub api_key: String,
    pub model: String,
}

#[derive(Clone, Deserialize)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub model: String,
}
