//! Options from OpenRouter's [routing](https://openrouter.ai/docs/guides/routing/provider-selection)
//! and [reasoning](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens) documentation.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Optional per-request fields. `None` leaves the provider's default in effect.
/// General constraints are checked locally; model-specific support is checked
/// by OpenRouter.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct OpenRouterOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_a: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repetition_penalty: Option<f64>,
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
    /// Legacy alias; prefer `max_completion_tokens`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderRouting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction: Option<Prediction>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<BTreeMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_options: Option<PromptCacheOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl OpenRouterOptions {
    pub(super) fn validate(&self) -> Result<(), super::OpenRouterError> {
        use super::OpenRouterError::Configuration;

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
                self.min_p,
                0.0..=1.0,
                "min_p must be finite and between 0 and 1",
            ),
            (
                self.top_a,
                0.0..=1.0,
                "top_a must be finite and between 0 and 1",
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
            (
                self.repetition_penalty,
                0.0..=2.0,
                "repetition_penalty must be finite and between 0 and 2",
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

        if self.max_tokens == Some(0) {
            return Err(Configuration("max_tokens must be at least 1"));
        }

        if self.max_completion_tokens == Some(0) {
            return Err(Configuration("max_completion_tokens must be at least 1"));
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
            .session_id
            .as_ref()
            .is_some_and(|value| value.chars().count() > 256)
        {
            return Err(Configuration("session_id must not exceed 256 characters"));
        }

        if let Some(reasoning) = &self.reasoning
            && let (Some(effort), Some(shorthand)) = (reasoning.effort, self.reasoning_effort)
            && effort != shorthand
        {
            return Err(Configuration(
                "reasoning_effort must match reasoning.effort when both are set",
            ));
        }

        if let Some(ResponseFormat::JsonSchema { json_schema }) = &self.response_format
            && !json_schema.schema.is_object()
        {
            return Err(Configuration(
                "response format schema must be a JSON object",
            ));
        }

        if let Some(provider) = &self.provider {
            provider.validate()?;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Prediction {
    Content { content: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceTier {
    Auto,
    Default,
    Fast,
    Flex,
    Priority,
    Scale,
    Ultrafast,
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
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CacheControl {
    Ephemeral {
        #[serde(skip_serializing_if = "Option::is_none")]
        ttl: Option<CacheTtl>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CacheTtl {
    #[serde(rename = "5m")]
    Minutes5,
    #[serde(rename = "1h")]
    Hour1,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ReasoningOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProviderRouting {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_fallbacks: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_parameters: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_collection: Option<DataCollection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zdr: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforce_distillable_text: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub quantizations: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<ProviderSort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_min_throughput: Option<PerformancePreference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_max_latency: Option<PerformancePreference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_price: Option<MaxPrice>,
}

impl ProviderRouting {
    fn validate(&self) -> Result<(), super::OpenRouterError> {
        use super::OpenRouterError::Configuration;

        if let Some(preference) = &self.preferred_min_throughput
            && !preference.is_nonnegative_finite()
        {
            return Err(Configuration(
                "provider.preferred_min_throughput values must be finite and nonnegative",
            ));
        }

        if let Some(preference) = &self.preferred_max_latency
            && !preference.is_nonnegative_finite()
        {
            return Err(Configuration(
                "provider.preferred_max_latency values must be finite and nonnegative",
            ));
        }

        if let Some(price) = &self.max_price {
            for (value, message) in [
                (
                    price.prompt,
                    "provider.max_price.prompt must be finite and nonnegative",
                ),
                (
                    price.completion,
                    "provider.max_price.completion must be finite and nonnegative",
                ),
                (
                    price.request,
                    "provider.max_price.request must be finite and nonnegative",
                ),
            ] {
                if value.is_some_and(|value| !value.is_finite() || value < 0.0) {
                    return Err(Configuration(message));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ProviderSort {
    By(SortBy),
    WithPartition {
        by: SortBy,
        #[serde(skip_serializing_if = "Option::is_none")]
        partition: Option<SortPartition>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    Price,
    Throughput,
    Latency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SortPartition {
    Model,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PerformancePreference {
    Value(f64),
    Percentiles(PerformancePercentiles),
}

impl PerformancePreference {
    fn is_nonnegative_finite(&self) -> bool {
        match self {
            Self::Value(value) => value.is_finite() && *value >= 0.0,
            Self::Percentiles(values) => [values.p50, values.p75, values.p90, values.p99]
                .into_iter()
                .flatten()
                .all(|value| value.is_finite() && value >= 0.0),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PerformancePercentiles {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p50: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p75: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p90: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p99: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MaxPrice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DataCollection {
    Allow,
    Deny,
}
