//! Options from the [Anthropic Messages API](https://platform.claude.com/docs/en/api/messages/create).

use serde::Serialize;
use serde_json::Value;

/// Per-request options. The SDK defaults the required output limit to 1,024.
/// General constraints are checked locally; model-specific support is checked
/// by Anthropic.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnthropicOptions {
    /// Zero warms the prompt cache without generating a response.
    pub max_tokens: u32,
    /// Legacy sampling control; newer Claude models restrict accepted values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Legacy nucleus sampling control; support depends on the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Legacy top-K sampling control; newer models may reject it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<OutputConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<RequestMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,
    /// Some models require a fast-mode beta header; use `Anthropic::with_client`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<InferenceSpeed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inference_geo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Diagnostics>,
    /// Encoded inside `tool_choice`, not as a top-level Messages field.
    #[serde(skip)]
    pub disable_parallel_tool_use: Option<bool>,
}

impl Default for AnthropicOptions {
    fn default() -> Self {
        Self {
            max_tokens: 1024,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: Vec::new(),
            thinking: None,
            output_config: None,
            metadata: None,
            cache_control: None,
            service_tier: None,
            speed: None,
            inference_geo: None,
            diagnostics: None,
            disable_parallel_tool_use: None,
        }
    }
}

impl AnthropicOptions {
    pub(super) fn validate(&self) -> Result<(), super::AnthropicError> {
        use super::AnthropicError::Configuration;

        for (value, message) in [
            (
                self.temperature,
                "temperature must be finite and between 0 and 1",
            ),
            (self.top_p, "top_p must be finite and between 0 and 1"),
        ] {
            if value.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
                return Err(Configuration(message));
            }
        }

        if let Some(Thinking::Enabled { budget_tokens }) = &self.thinking
            && (*budget_tokens < 1024 || *budget_tokens >= self.max_tokens)
        {
            return Err(Configuration(
                "thinking budget must be at least 1024 and less than max_tokens",
            ));
        }

        if let Some(metadata) = &self.metadata
            && metadata
                .user_id
                .as_ref()
                .is_some_and(|value| value.chars().count() > 512)
        {
            return Err(Configuration(
                "metadata.user_id must not exceed 512 characters",
            ));
        }

        if let Some(diagnostics) = &self.diagnostics
            && diagnostics
                .previous_message_id
                .as_ref()
                .is_some_and(|value| value.chars().count() > 256)
        {
            return Err(Configuration(
                "diagnostics.previous_message_id must not exceed 256 characters",
            ));
        }

        if let Some(OutputConfig {
            format: Some(OutputFormat::JsonSchema { schema }),
            ..
        }) = &self.output_config
            && !schema.is_object()
        {
            return Err(Configuration(
                "output_config.format.schema must be a JSON object",
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RequestMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OutputConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputFormat {
    JsonSchema { schema: Value },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTier {
    Auto,
    StandardOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InferenceSpeed {
    Standard,
    Fast,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Diagnostics {
    // Explicit null opts in without a prior message to compare.
    pub previous_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Thinking {
    Adaptive,
    Enabled { budget_tokens: u32 },
    Disabled,
}
