//! Providers supported by this application, independently of the SDK's catalog.

use serde::Deserialize;
use std::{fmt, str::FromStr};

// No Debug implementations: settings may contain credentials.

#[derive(Clone, Default, Deserialize)]
pub struct ProvidersConfig {
    pub openai: Option<OpenAIConfig>,
    pub openrouter: Option<OpenRouterConfig>,
    pub anthropic: Option<AnthropicConfig>,
}

impl ProvidersConfig {
    pub fn into_providers(self) -> impl Iterator<Item = ProviderConfig> {
        [
            self.openai.map(ProviderConfig::OpenAI),
            self.openrouter.map(ProviderConfig::OpenRouter),
            self.anthropic.map(ProviderConfig::Anthropic),
        ]
        .into_iter()
        .flatten()
    }
}

#[derive(Clone, Deserialize)]
pub struct OpenAIConfig {
    pub api_key: String,
}

#[derive(Clone, Deserialize)]
pub struct OpenRouterConfig {
    pub api_key: String,
}

#[derive(Clone, Deserialize)]
pub struct AnthropicConfig {
    pub api_key: String,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "provider", content = "settings", rename_all = "lowercase")]
pub enum ProviderConfig {
    OpenAI(OpenAIConfig),
    OpenRouter(OpenRouterConfig),
    Anthropic(AnthropicConfig),
}

impl ProviderConfig {
    pub fn id(&self) -> ProviderId {
        match self {
            Self::OpenAI(_) => ProviderId::OpenAI,
            Self::OpenRouter(_) => ProviderId::OpenRouter,
            Self::Anthropic(_) => ProviderId::Anthropic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    OpenAI,
    OpenRouter,
    Anthropic,
}

impl ProviderId {
    pub const ALL: [Self; 3] = [Self::OpenAI, Self::OpenRouter, Self::Anthropic];
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OpenAI => "openai",
            Self::OpenRouter => "openrouter",
            Self::Anthropic => "anthropic",
        })
    }
}

impl FromStr for ProviderId {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openai" => Ok(Self::OpenAI),
            "openrouter" => Ok(Self::OpenRouter),
            "anthropic" => Ok(Self::Anthropic),
            _ => Err("Supported providers: openai, openrouter, anthropic"),
        }
    }
}
