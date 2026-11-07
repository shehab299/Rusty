//! Providers supported by this application, independently of the SDK's catalog.

use ai_sdk::providers::{Anthropic, ModelError, OpenAI, OpenRouter, Provider};
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
    pub fn settings(&self, id: ProviderId) -> Option<ProviderConfig> {
        match id {
            ProviderId::OpenAI => self.openai.clone().map(ProviderConfig::OpenAI),
            ProviderId::OpenRouter => self.openrouter.clone().map(ProviderConfig::OpenRouter),
            ProviderId::Anthropic => self.anthropic.clone().map(ProviderConfig::Anthropic),
        }
    }

    pub fn configure(&mut self, settings: ProviderConfig) {
        match settings {
            ProviderConfig::OpenAI(config) => self.openai = Some(config),
            ProviderConfig::OpenRouter(config) => self.openrouter = Some(config),
            ProviderConfig::Anthropic(config) => self.anthropic = Some(config),
        }
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

    pub fn build(&self) -> Result<Box<dyn Provider>, ModelError> {
        Ok(match self {
            Self::OpenAI(config) => Box::new(OpenAI::new(&config.api_key)?),
            Self::OpenRouter(config) => Box::new(OpenRouter::new(&config.api_key)?),
            Self::Anthropic(config) => Box::new(Anthropic::new(&config.api_key)?),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    OpenAI,
    OpenRouter,
    Anthropic,
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
