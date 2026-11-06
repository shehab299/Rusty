use std::error::Error;
use std::fmt;
use std::time::Duration;

use reqwest::header::HeaderValue;

use super::{Provider, ModelError};
use crate::{GenerateRequest, GenerateResponse, async_trait};

mod options;
mod types;

pub use options::{
    JsonSchema, OpenAIOptions, Prediction, PredictionContent, PredictionTextPart, PromptCacheMode,
    PromptCacheOptions, PromptCacheRetention, PromptCacheTtl, ReasoningEffort, ResponseFormat,
    ServiceTier, StopSequences, Verbosity,
};

pub use types::{CompletionTokenDetails, OpenAIResponseDetails, OpenAIUsage, PromptTokenDetails};
use types::{OpenAIChatCompletionRequest, OpenAIChatCompletionResponse};

const ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";

#[derive(Clone)]
pub struct OpenAI {
    client: reqwest::Client,
    authorization: HeaderValue,
    endpoint: String,
}

impl OpenAI {
    pub fn new(api_key: impl AsRef<str>) -> Result<Self, OpenAIError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(OpenAIError::Transport)?;

        Self::with_client(api_key, client)
    }

    /// Supply a custom client for timeouts, proxies, TLS, or default headers
    /// such as `OpenAI-Organization` and `OpenAI-Project`.
    pub fn with_client(
        api_key: impl AsRef<str>,
        client: reqwest::Client,
    ) -> Result<Self, OpenAIError> {
        let api_key = api_key.as_ref();

        if api_key.trim().is_empty() {
            return Err(OpenAIError::Configuration("API key must not be empty"));
        }

        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|_| {
                OpenAIError::Configuration("API key contains invalid header characters")
            })?;
        authorization.set_sensitive(true);

        Ok(Self {
            client,
            authorization,
            endpoint: ENDPOINT.into(),
        })
    }
}

#[async_trait]
impl Provider for OpenAI {
    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        let response = self
            .client
            .post(&self.endpoint)
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .json(&OpenAIChatCompletionRequest::try_from(
                request,
            )?)
            .send()
            .await
            .map_err(OpenAIError::Transport)?;

        let status = response.status();
        let bytes = response.bytes().await.map_err(OpenAIError::Transport)?;

        match serde_json::from_slice::<OpenAIChatCompletionResponse>(&bytes) {
            Ok(OpenAIChatCompletionResponse::Error { error }) => Err(error.into_error(status)),
            _ if !status.is_success() => Err(OpenAIError::Api {
                status: status.as_u16(),
                code: None,
                error_type: None,
                param: None,
                message: status.canonical_reason().unwrap_or("request failed").into(),
            }),
            Ok(OpenAIChatCompletionResponse::Completion {
                id,
                model,
                choices,
                details,
            }) => choices
                .into_iter()
                .next()
                .ok_or_else(|| OpenAIError::InvalidResponse("missing completion choice".into()))?
                .into_response(id, model, details),
            Err(error) => Err(OpenAIError::InvalidResponse(error.to_string())),
        }
        .map_err(Into::into)
    }
}

/// An OpenAI failure. Inspect a [`crate::providers::ModelError`] with
/// `error.downcast_ref::<OpenAIError>()`.
#[derive(Debug)]
pub enum OpenAIError {
    Configuration(&'static str),
    Transport(reqwest::Error),
    Api {
        status: u16,
        code: Option<String>,
        error_type: Option<String>,
        param: Option<String>,
        message: String,
    },
    Refusal(String),
    InvalidResponse(String),
}

impl fmt::Display for OpenAIError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(formatter, "OpenAI configuration: {message}"),
            Self::Transport(error) => write!(formatter, "OpenAI transport: {error}"),
            Self::Api {
                status, message, ..
            } => write!(formatter, "OpenAI API (HTTP {status}): {message}"),
            Self::Refusal(message) => write!(formatter, "OpenAI refusal: {message}"),
            Self::InvalidResponse(message) => {
                write!(formatter, "OpenAI invalid response: {message}")
            }
        }
    }
}

impl Error for OpenAIError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            _ => None,
        }
    }
}
