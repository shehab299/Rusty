use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::providers::{
    anthropic::{AnthropicOptions, AnthropicResponseDetails},
    openai::{OpenAIOptions, OpenAIResponseDetails},
    openrouter::{OpenRouterOptions, OpenRouterResponseDetails},
};

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Message {
    System {
        content: String,
    },
    User {
        content: UserContent,
    },
    Assistant {
        content: String,
        tool_calls: Vec<ToolCall>,
        provider_details: Option<ProviderDetails>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::System {
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<UserContent>) -> Self {
        Self::User {
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::Assistant {
            content: content.into(),
            tool_calls: Vec::new(),
            provider_details: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::Tool {
            tool_call_id: tool_call_id.into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl From<String> for UserContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for UserContent {
    fn from(text: &str) -> Self {
        Self::Text(text.into())
    }
}

impl From<Vec<ContentPart>> for UserContent {
    fn from(parts: Vec<ContentPart>) -> Self {
        Self::Parts(parts)
    }
}

impl<const N: usize> From<[ContentPart; N]> for UserContent {
    fn from(parts: [ContentPart; N]) -> Self {
        Self::Parts(parts.into())
    }
}

impl UserContent {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Self::Parts(parts) = self {
            if parts.is_empty() {
                return Err("user content parts must not be empty");
            }

            for part in parts {
                if let ContentPart::Image(image) = part {
                    image.validate()?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    Text(String),
    Image(Image),
}

impl ContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn image(image: Image) -> Self {
        Self::Image(image)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub source: ImageSource,
    pub detail: Option<ImageDetail>,
}

impl Image {
    pub fn url(url: impl Into<String>) -> Self {
        Self {
            source: ImageSource::Url { url: url.into() },
            detail: None,
        }
    }

    pub fn base64(media_type: ImageMediaType, data: impl Into<String>) -> Self {
        Self {
            source: ImageSource::Base64 {
                media_type,
                data: data.into(),
            },
            detail: None,
        }
    }

    pub fn from_bytes(media_type: ImageMediaType, data: impl AsRef<[u8]>) -> Self {
        Self::base64(media_type, STANDARD.encode(data.as_ref()))
    }

    pub fn with_detail(mut self, detail: ImageDetail) -> Self {
        self.detail = Some(detail);

        self
    }

    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        match &self.source {
            ImageSource::Url { url } => {
                let parsed = reqwest::Url::parse(url)
                    .map_err(|_| "image URL must be an absolute HTTP or HTTPS URL")?;

                if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                    return Err("image URL must be an absolute HTTP or HTTPS URL");
                }
            }
            ImageSource::Base64 { data, .. } => {
                if data.is_empty() || STANDARD.decode(data).is_err() {
                    return Err("image data must be nonempty standard base64");
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    Url {
        url: String,
    },
    Base64 {
        media_type: ImageMediaType,
        data: String,
    },
}

impl ImageSource {
    pub(crate) fn into_url(self) -> String {
        match self {
            Self::Url { url } => url,
            Self::Base64 { media_type, data } => {
                format!("data:{};base64,{data}", media_type.as_str())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageMediaType {
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "image/gif")]
    Gif,
    #[serde(rename = "image/webp")]
    Webp,
}

impl ImageMediaType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    Auto,
    Low,
    High,
    Original,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[allow(clippy::large_enum_variant)]
pub enum ProviderOptions {
    Anthropic(AnthropicOptions),
    OpenAI(OpenAIOptions),
    OpenRouter(OpenRouterOptions),
}

impl From<AnthropicOptions> for ProviderOptions {
    fn from(options: AnthropicOptions) -> Self {
        Self::Anthropic(options)
    }
}

impl From<OpenAIOptions> for ProviderOptions {
    fn from(options: OpenAIOptions) -> Self {
        Self::OpenAI(options)
    }
}

impl From<OpenRouterOptions> for ProviderOptions {
    fn from(options: OpenRouterOptions) -> Self {
        Self::OpenRouter(options)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl Tool {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Tool(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenerateRequest {
    pub messages: Vec<Message>,
    pub tools: Vec<Tool>,
    pub tool_choice: Option<ToolChoice>,
    pub provider_options: Option<ProviderOptions>,
}

impl GenerateRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self::from_messages([Message::user(UserContent::Text(prompt.into()))])
    }

    pub fn from_messages(messages: impl IntoIterator<Item = Message>) -> Self {
        Self {
            messages: messages.into_iter().collect(),
            tools: Vec::new(),
            tool_choice: None,
            provider_options: None,
        }
    }

    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Tool>) -> Self {
        self.tools = tools.into_iter().collect();

        self
    }

    pub fn with_tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);

        self
    }

    /// Attach typed options for a built-in provider. A mismatched adapter
    /// returns a configuration error before sending the request.
    pub fn with_provider_options(mut self, options: impl Into<ProviderOptions>) -> Self {
        self.provider_options = Some(options.into());

        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Pause,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ProviderDetails {
    OpenAI(OpenAIResponseDetails),
    Anthropic(AnthropicResponseDetails),
    OpenRouter(OpenRouterResponseDetails),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Logprobs {
    pub content: Option<Vec<TokenLogprob>>,
    pub refusal: Option<Vec<TokenLogprob>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TokenLogprob {
    pub token: String,
    pub logprob: f64,
    pub bytes: Option<Vec<u8>>,
    #[serde(default)]
    pub top_logprobs: Vec<AlternativeLogprob>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AlternativeLogprob {
    pub token: String,
    pub logprob: f64,
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GenerateResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    pub finish_reason: Option<FinishReason>,
    pub native_finish_reason: Option<String>,
    pub provider_details: Option<ProviderDetails>,
}

impl GenerateResponse {
    pub fn into_message(self) -> Message {
        Message::Assistant {
            content: self.text,
            tool_calls: self.tool_calls,
            provider_details: self.provider_details,
        }
    }
}
