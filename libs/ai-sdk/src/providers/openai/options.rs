//! Options from the [OpenAI Chat Completions reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create).

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Optional per-request fields. `None` leaves the provider's default in effect.
/// Documented general constraints are checked locally; model-specific support
/// and limits are checked by OpenAI.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct OpenAIOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<BTreeMap<String, f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<StopSequences>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,
    /// Legacy token limit. Prefer `max_completion_tokens` for newer models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<Verbosity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction: Option<Prediction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_options: Option<PromptCacheOptions>,
    /// Legacy cache control retained for models that use it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<PromptCacheRetention>,
    /// Legacy user identifier. Prefer `safety_identifier` / `prompt_cache_key`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

impl OpenAIOptions {
    pub(super) fn validate(&self) -> Result<(), super::OpenAIError> {
        use super::OpenAIError::Configuration;

        for (value, range, message) in [
            (
                self.temperature,
                0.0..=2.0,
                "temperature must be finite and between 0 and 2",
            ),
            (
                self.top_p,
                0.0..=1.0,
                "top_p must be finite and between 0 and 1",
            ),
            (
                self.frequency_penalty,
                -2.0..=2.0,
                "frequency_penalty must be finite and between -2 and 2",
            ),
            (
                self.presence_penalty,
                -2.0..=2.0,
                "presence_penalty must be finite and between -2 and 2",
            ),
        ] {
            if value.is_some_and(|value| !value.is_finite() || !range.contains(&value)) {
                return Err(Configuration(message));
            }
        }

        if let Some(bias) = &self.logit_bias
            && bias
                .values()
                .any(|value| !value.is_finite() || !(-100.0..=100.0).contains(value))
        {
            return Err(Configuration(
                "logit_bias values must be finite and between -100 and 100",
            ));
        }

        if let Some(count) = self.top_logprobs {
            if count > 20 {
                return Err(Configuration("top_logprobs must be between 0 and 20"));
            }

            if self.logprobs != Some(true) {
                return Err(Configuration("top_logprobs requires logprobs to be true"));
            }
        }

        if let Some(StopSequences::Multiple(sequences)) = &self.stop
            && sequences.len() > 4
        {
            return Err(Configuration("stop accepts at most 4 sequences"));
        }

        if let Some(metadata) = &self.metadata {
            if metadata.len() > 16 {
                return Err(Configuration("metadata accepts at most 16 entries"));
            }

            for (key, value) in metadata {
                if key.chars().count() > 64 {
                    return Err(Configuration("metadata keys must not exceed 64 characters"));
                }

                if value.chars().count() > 512 {
                    return Err(Configuration(
                        "metadata values must not exceed 512 characters",
                    ));
                }
            }
        }

        if self
            .safety_identifier
            .as_ref()
            .is_some_and(|value| value.chars().count() > 128)
        {
            return Err(Configuration(
                "safety_identifier must not exceed 128 characters",
            ));
        }

        if let Some(ResponseFormat::JsonSchema { json_schema }) = &self.response_format {
            if json_schema.name.is_empty()
                || json_schema.name.len() > 64
                || !json_schema
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                return Err(Configuration(
                    "response format name must contain 1 to 64 ASCII letters, digits, underscores, or dashes",
                ));
            }

            if !json_schema.schema.is_object() {
                return Err(Configuration(
                    "response format schema must be a JSON object",
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum StopSequences {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Prediction {
    Content { content: PredictionContent },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum PredictionContent {
    Text(String),
    Parts(Vec<PredictionTextPart>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PredictionTextPart {
    Text { text: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PromptCacheOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<PromptCacheMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<PromptCacheTtl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheMode {
    Implicit,
    Explicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PromptCacheTtl {
    #[serde(rename = "30m")]
    Minutes30,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PromptCacheRetention {
    #[serde(rename = "in_memory")]
    InMemory,
    #[serde(rename = "24h")]
    Hours24,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceTier {
    Auto,
    Default,
    Flex,
    Scale,
    Priority,
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verbosity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    Text,
    JsonObject,
    JsonSchema { json_schema: JsonSchema },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsonSchema {
    pub name: String,
    pub schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}
