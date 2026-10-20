use std::error::Error;
use std::fmt;
use std::time::Duration;

use reqwest::header::HeaderValue;
use serde_json::Value;

use super::{Model, ModelError};
use crate::{GenerateRequest, GenerateResponse, async_trait};

mod options;
mod types;

pub use options::{
    CacheControl, CacheTtl, DataCollection, JsonSchema, MaxPrice, OpenRouterOptions,
    PerformancePercentiles, PerformancePreference, Prediction, PromptCacheMode, PromptCacheOptions,
    PromptCacheTtl, ProviderRouting, ProviderSort, ReasoningEffort, ReasoningOptions,
    ResponseFormat, ServiceTier, SortBy, SortPartition, StopSequences,
};

pub use types::{
    CompletionTokenDetails, CostDetails, OpenRouterResponseDetails, OpenRouterUsage,
    PromptTokenDetails, ReasoningDetail,
};
use types::{OpenRouterChatCompletionRequest, OpenRouterChatCompletionResponse};

const ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";

#[derive(Clone)]
pub struct OpenRouter {
    client: reqwest::Client,
    authorization: HeaderValue,
    model: String,
    endpoint: String,
}

impl OpenRouter {
    /// Requests time out after 120 seconds; connections time out after 10 seconds.
    pub fn new(
        api_key: impl AsRef<str>,
        model: impl Into<String>,
    ) -> Result<Self, OpenRouterError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(OpenRouterError::Transport)?;

        Self::with_client(api_key, model, client)
    }

    /// Use a custom HTTP client to configure timeouts, proxies, or TLS.
    pub fn with_client(
        api_key: impl AsRef<str>,
        model: impl Into<String>,
        client: reqwest::Client,
    ) -> Result<Self, OpenRouterError> {
        let api_key = api_key.as_ref();

        if api_key.trim().is_empty() {
            return Err(OpenRouterError::Configuration("API key must not be empty"));
        }

        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|_| {
                OpenRouterError::Configuration("API key contains invalid header characters")
            })?;
        authorization.set_sensitive(true);

        let model = model.into();

        if model.trim().is_empty() {
            return Err(OpenRouterError::Configuration("model ID must not be empty"));
        }

        Ok(Self {
            client,
            authorization,
            model,
            endpoint: ENDPOINT.into(),
        })
    }
}

#[async_trait]
impl Model for OpenRouter {
    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        let request_body =
            OpenRouterChatCompletionRequest::try_from((self.model.clone(), request))?;
        let response = self
            .client
            .post(&self.endpoint)
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .json(&request_body)
            .send()
            .await
            .map_err(OpenRouterError::Transport)?;

        let status = response.status();
        let bytes = response.bytes().await.map_err(OpenRouterError::Transport)?;

        match serde_json::from_slice::<OpenRouterChatCompletionResponse>(&bytes) {
            Ok(OpenRouterChatCompletionResponse::Error { error }) => Err(error.into_error(status)),
            _ if !status.is_success() => Err(OpenRouterError::Api {
                status: status.as_u16(),
                code: None,
                message: status.canonical_reason().unwrap_or("request failed").into(),
            }),
            Ok(OpenRouterChatCompletionResponse::Completion {
                id,
                model,
                choices,
                details,
            }) => choices
                .into_iter()
                .next()
                .ok_or_else(|| {
                    OpenRouterError::InvalidResponse("missing completion choice".into())
                })?
                .into_response(status, id, model, details),
            Err(error) => Err(OpenRouterError::InvalidResponse(error.to_string())),
        }
        .map_err(Into::into)
    }
}

/// An OpenRouter failure. Inspect a [`crate::providers::ModelError`] with
/// `error.downcast_ref::<OpenRouterError>()`.
#[derive(Debug)]
pub enum OpenRouterError {
    Configuration(&'static str),
    Transport(reqwest::Error),
    /// An HTTP failure or an API error embedded in an HTTP 200 response.
    Api {
        status: u16,
        code: Option<Value>,
        message: String,
    },
    InvalidResponse(String),
}

impl fmt::Display for OpenRouterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => {
                write!(formatter, "OpenRouter configuration: {message}")
            }

            Self::Transport(error) => write!(formatter, "OpenRouter transport: {error}"),
            Self::Api {
                status, message, ..
            } => {
                write!(formatter, "OpenRouter API (HTTP {status}): {message}")
            }

            Self::InvalidResponse(message) => {
                write!(formatter, "OpenRouter invalid response: {message}")
            }
        }
    }
}

impl Error for OpenRouterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            _ => None,
        }
    }
}
