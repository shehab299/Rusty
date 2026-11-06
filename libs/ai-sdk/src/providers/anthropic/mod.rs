use std::error::Error;
use std::fmt;
use std::time::Duration;

use reqwest::header::HeaderValue;

use super::{Provider, ModelError};
use crate::{GenerateRequest, GenerateResponse, async_trait};

mod options;
mod types;

pub use options::{
    AnthropicOptions, CacheControl, CacheTtl, Diagnostics, Effort, InferenceSpeed, OutputConfig,
    OutputFormat, RequestMetadata, ServiceTier, Thinking,
};

use types::{AnthropicMessagesRequest, AnthropicMessagesResponse};
pub use types::{
    AnthropicResponseDetails, AnthropicUsage, CacheCreationUsage, ContentBlock, OutputTokenDetails,
    ServerToolUsage, StopDetails,
};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// Requests require a Tokio runtime; construction makes no network requests.
#[derive(Clone)]
pub struct Anthropic {
    client: reqwest::Client,
    api_key: HeaderValue,
}

impl Anthropic {
    /// Requests time out after 120 seconds; connections after 10 seconds.
    pub fn new(api_key: impl AsRef<str>) -> Result<Self, AnthropicError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(AnthropicError::Transport)?;

        Self::with_client(api_key, client)
    }

    /// Supply a custom HTTP client for timeouts, proxies, TLS, or beta headers.
    pub fn with_client(
        api_key: impl AsRef<str>,
        client: reqwest::Client,
    ) -> Result<Self, AnthropicError> {
        let api_key = api_key.as_ref();

        if api_key.trim().is_empty() {
            return Err(AnthropicError::Configuration("API key must not be empty"));
        }

        let mut api_key = HeaderValue::from_str(api_key).map_err(|_| {
            AnthropicError::Configuration("API key contains invalid header characters")
        })?;
        api_key.set_sensitive(true);

        Ok(Self {
            client,
            api_key,
        })
    }
}

#[async_trait]
impl Provider for Anthropic {
    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        let request = AnthropicMessagesRequest::try_from(request)?;
        let response = self
            .client
            .post(ENDPOINT)
            .header("x-api-key", self.api_key.clone())
            .header("anthropic-version", API_VERSION)
            .json(&request)
            .send()
            .await
            .map_err(AnthropicError::Transport)?;

        let status = response.status();
        let request_id = response
            .headers()
            .get("request-id")
            .and_then(|value| value.to_str().ok())
            .map(String::from);
        let bytes = response.bytes().await.map_err(AnthropicError::Transport)?;

        match serde_json::from_slice::<AnthropicMessagesResponse>(&bytes) {
            Ok(AnthropicMessagesResponse::Error {
                error,
                request_id: body_id,
            }) => Err(AnthropicError::Api {
                status: status.as_u16(),
                error_type: Some(error.kind),
                message: error.message,
                request_id: body_id.or(request_id),
            }),
            _ if !status.is_success() => Err(AnthropicError::Api {
                status: status.as_u16(),
                error_type: None,
                message: status.canonical_reason().unwrap_or("request failed").into(),
                request_id,
            }),
            Ok(AnthropicMessagesResponse::Message(message)) => message.into_response(),
            Err(error) => Err(AnthropicError::InvalidResponse(error.to_string())),
        }
        .map_err(Into::into)
    }
}

/// An Anthropic failure. Inspect a [`crate::providers::ModelError`] with
/// `error.downcast_ref::<AnthropicError>()`.
#[derive(Debug)]
pub enum AnthropicError {
    Configuration(&'static str),
    Transport(reqwest::Error),
    Api {
        status: u16,
        error_type: Option<String>,
        message: String,
        request_id: Option<String>,
    },
    Refusal(String),
    InvalidResponse(String),
}

impl fmt::Display for AnthropicError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(formatter, "Anthropic configuration: {message}"),
            Self::Transport(error) => write!(formatter, "Anthropic transport: {error}"),
            Self::Api {
                status, message, ..
            } => write!(formatter, "Anthropic API (HTTP {status}): {message}"),
            Self::Refusal(message) => write!(formatter, "Anthropic refusal: {message}"),
            Self::InvalidResponse(message) => {
                write!(formatter, "Anthropic invalid response: {message}")
            }
        }
    }
}

impl Error for AnthropicError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            _ => None,
        }
    }
}
