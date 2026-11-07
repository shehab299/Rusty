//! References:
//! - [API reference](https://openrouter.ai/docs/api_reference/overview)
//! - [Tool calling](https://openrouter.ai/docs/guides/features/tool-calling)
//! - [Reasoning](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens)
//! - [Errors](https://openrouter.ai/docs/api_reference/errors-and-debugging)

use std::collections::BTreeMap;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{OpenRouterError, OpenRouterOptions};
use crate::{
    ContentPart, FinishReason as NormalizedFinishReason, GenerateRequest, GenerateResponse,
    ImageDetail, Logprobs, Message, ProviderDetails, ProviderOptions, Tool, ToolCall, ToolChoice,
    Usage, UserContent,
};

#[derive(Serialize)]
pub(super) struct OpenRouterChatCompletionRequest {
    model: String,
    messages: Vec<ConversationMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<FunctionTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolSelection>,
    #[serde(flatten)]
    options: OpenRouterOptions,
}

impl TryFrom<GenerateRequest> for OpenRouterChatCompletionRequest {
    type Error = OpenRouterError;

    fn try_from(request: GenerateRequest) -> Result<Self, Self::Error> {
        let options = match request.provider_options {
            None => OpenRouterOptions::default(),
            Some(ProviderOptions::OpenRouter(options)) => options,
            Some(_) => {
                return Err(OpenRouterError::Configuration(
                    "request options do not belong to OpenRouter",
                ));
            }
        };
        options.validate()?;

        Ok(Self {
            model: request.model,
            messages: request
                .messages
                .into_iter()
                .map(ConversationMessage::try_from)
                .collect::<Result<_, _>>()?,
            stream: false,
            tools: request.tools.into_iter().map(FunctionTool::from).collect(),
            tool_choice: request.tool_choice.map(ToolSelection::from),
            options,
        })
    }
}

#[derive(Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
enum ConversationMessage {
    System {
        content: String,
    },
    User {
        content: UserMessageContent,
    },
    Assistant(AssistantMessage),
    Tool {
        tool_call_id: String,
        content: String,
    },
}

impl TryFrom<Message> for ConversationMessage {
    type Error = OpenRouterError;

    fn try_from(message: Message) -> Result<Self, Self::Error> {
        Ok(match message {
            Message::System { content } => Self::System { content },
            Message::User { content } => {
                content.validate().map_err(OpenRouterError::Configuration)?;

                Self::User {
                    content: content.into(),
                }
            }
            Message::Tool {
                tool_call_id,
                content,
            } => Self::Tool {
                tool_call_id,
                content,
            },
            Message::Assistant {
                content,
                tool_calls,
                provider_details,
            } => {
                let continuation = match provider_details {
                    Some(ProviderDetails::OpenRouter(details)) => ReasoningMetadata {
                        reasoning: details.reasoning,
                        reasoning_details: details.reasoning_details,
                    },
                    _ => ReasoningMetadata::default(),
                };

                Self::Assistant(AssistantMessage {
                    content: if content.is_empty() && !tool_calls.is_empty() {
                        None
                    } else {
                        Some(AssistantContent::Text(content))
                    },
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls.into_iter().map(FunctionCall::from).collect())
                    },
                    continuation,
                })
            }
        })
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum UserMessageContent {
    Text(String),
    Parts(Vec<UserContentPart>),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UserContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Serialize)]
struct ImageUrl {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<ImageDetail>,
}

impl From<UserContent> for UserMessageContent {
    fn from(content: UserContent) -> Self {
        match content {
            UserContent::Text(text) => Self::Text(text),
            UserContent::Parts(parts) => Self::Parts(
                parts
                    .into_iter()
                    .map(|part| match part {
                        ContentPart::Text(text) => UserContentPart::Text { text },
                        ContentPart::Image(image) => UserContentPart::ImageUrl {
                            image_url: ImageUrl {
                                url: image.source.into_url(),
                                detail: image.detail,
                            },
                        },
                    })
                    .collect(),
            ),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct AssistantMessage {
    content: Option<AssistantContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<FunctionCall>>,
    #[serde(flatten)]
    continuation: ReasoningMetadata,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum AssistantContent {
    Text(String),
    Parts(Vec<AssistantContentPart>),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AssistantContentPart {
    Text {
        text: String,
    },
    #[serde(other)]
    Unsupported,
}

impl AssistantContent {
    fn into_text(self) -> Result<String, OpenRouterError> {
        match self {
            Self::Text(text) => Ok(text),
            Self::Parts(parts) => {
                let mut text = String::new();
                for part in parts {
                    match part {
                        AssistantContentPart::Text { text: part } => text.push_str(&part),
                        AssistantContentPart::Unsupported => {
                            return Err(OpenRouterError::InvalidResponse(
                                "unsupported assistant content part; expected text".into(),
                            ));
                        }
                    }
                }
                Ok(text)
            }
        }
    }
}

// Reasoning blocks are opaque: replay their contents and signatures unchanged.
#[derive(Default, Serialize, Deserialize)]
struct ReasoningMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_details: Option<Vec<ReasoningDetail>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ToolType {
    Function,
}

#[derive(Serialize)]
struct FunctionTool {
    #[serde(rename = "type")]
    kind: ToolType,
    function: FunctionDefinition,
}

#[derive(Serialize)]
struct FunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

impl From<Tool> for FunctionTool {
    fn from(tool: Tool) -> Self {
        Self {
            kind: ToolType::Function,
            function: FunctionDefinition {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
struct FunctionCall {
    id: String,
    #[serde(rename = "type")]
    kind: ToolType,
    function: FunctionCallDetails,
}

#[derive(Serialize, Deserialize)]
struct FunctionCallDetails {
    name: String,
    arguments: String,
}

impl From<ToolCall> for FunctionCall {
    fn from(call: ToolCall) -> Self {
        Self {
            id: call.id,
            kind: ToolType::Function,
            function: FunctionCallDetails {
                name: call.name,
                arguments: call.arguments,
            },
        }
    }
}

impl TryFrom<FunctionCall> for ToolCall {
    type Error = OpenRouterError;

    fn try_from(call: FunctionCall) -> Result<Self, Self::Error> {
        if call.id.trim().is_empty() || call.function.name.trim().is_empty() {
            return Err(OpenRouterError::InvalidResponse(
                "tool call ID and name must not be empty".into(),
            ));
        }

        Ok(Self {
            id: call.id,
            name: call.function.name,
            arguments: call.function.arguments,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum ToolSelection {
    Auto,
    None,
    Required,
    #[serde(untagged)]
    Function {
        #[serde(rename = "type")]
        kind: ToolType,
        function: FunctionName,
    },
}

#[derive(Serialize)]
struct FunctionName {
    name: String,
}

impl From<ToolChoice> for ToolSelection {
    fn from(choice: ToolChoice) -> Self {
        match choice {
            ToolChoice::Auto => Self::Auto,
            ToolChoice::None => Self::None,
            ToolChoice::Required => Self::Required,
            ToolChoice::Tool(name) => Self::Function {
                kind: ToolType::Function,
                function: FunctionName { name },
            },
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub(super) enum OpenRouterChatCompletionResponse {
    // Errors take precedence even if the payload also contains choices.
    Error {
        error: ApiErrorDetails,
    },
    Completion {
        id: Option<String>,
        model: Option<String>,
        choices: Vec<CompletionChoice>,
        #[serde(flatten)]
        details: OpenRouterResponseDetails,
    },
}

#[derive(Deserialize)]
pub(super) struct CompletionChoice {
    message: Option<AssistantMessage>,
    finish_reason: Option<FinishReason>,
    error: Option<ApiErrorDetails>,
    logprobs: Option<Logprobs>,
    native_finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct OpenRouterResponseDetails {
    pub created: Option<u64>,
    pub provider: Option<String>,
    pub system_fingerprint: Option<String>,
    pub service_tier: Option<String>,
    pub finish_reason: Option<String>,
    pub reasoning: Option<String>,
    pub reasoning_details: Option<Vec<ReasoningDetail>>,
    pub logprobs: Option<Logprobs>,
    pub usage: Option<OpenRouterUsage>,
}

// Preserve unknown fields so signed reasoning can be replayed without data loss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningDetail {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenRouterUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost: Option<f64>,
    pub is_byok: Option<bool>,
    pub prompt_tokens_details: Option<PromptTokenDetails>,
    pub completion_tokens_details: Option<CompletionTokenDetails>,
    pub cost_details: Option<CostDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PromptTokenDetails {
    pub cached_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub audio_tokens: Option<u64>,
    pub video_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CompletionTokenDetails {
    pub reasoning_tokens: Option<u64>,
    pub audio_tokens: Option<u64>,
    pub image_tokens: Option<u64>,
    pub accepted_prediction_tokens: Option<u64>,
    pub rejected_prediction_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CostDetails {
    pub upstream_inference_cost: Option<f64>,
    pub upstream_inference_prompt_cost: Option<f64>,
    pub upstream_inference_completions_cost: Option<f64>,
}

impl From<&OpenRouterUsage> for Usage {
    fn from(usage: &OpenRouterUsage) -> Self {
        Self {
            input_tokens: Some(usage.prompt_tokens),
            output_tokens: Some(usage.completion_tokens),
            total_tokens: Some(usage.total_tokens),
            cached_input_tokens: usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens),
            cache_creation_input_tokens: usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cache_write_tokens),
            reasoning_tokens: usage
                .completion_tokens_details
                .as_ref()
                .and_then(|details| details.reasoning_tokens),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Error,
    #[serde(untagged)]
    Other(String),
}

impl From<FinishReason> for String {
    fn from(reason: FinishReason) -> Self {
        match reason {
            FinishReason::Stop => "stop".into(),
            FinishReason::Length => "length".into(),
            FinishReason::ToolCalls => "tool_calls".into(),
            FinishReason::ContentFilter => "content_filter".into(),
            FinishReason::Error => "error".into(),
            FinishReason::Other(reason) => reason,
        }
    }
}

impl CompletionChoice {
    pub fn into_response(
        self,
        status: StatusCode,
        id: Option<String>,
        model: Option<String>,
        mut details: OpenRouterResponseDetails,
    ) -> Result<GenerateResponse, OpenRouterError> {
        if let Some(error) = self.error {
            return Err(error.into_error(status));
        }

        if matches!(self.finish_reason, Some(FinishReason::Error)) {
            return Err(OpenRouterError::InvalidResponse("generation failed".into()));
        }

        let message = self
            .message
            .ok_or_else(|| OpenRouterError::InvalidResponse("missing assistant message".into()))?;
        let tool_calls = message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(ToolCall::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        if matches!(self.finish_reason, Some(FinishReason::ToolCalls)) && tool_calls.is_empty() {
            return Err(OpenRouterError::InvalidResponse(
                "tool_calls finish reason without any tool calls".into(),
            ));
        }

        let text = match message.content {
            Some(content) => content.into_text()?,
            None if !tool_calls.is_empty()
                || matches!(
                    self.finish_reason,
                    Some(FinishReason::Length | FinishReason::ContentFilter)
                ) =>
            {
                String::new()
            }
            None => {
                return Err(OpenRouterError::InvalidResponse(
                    "expected text or tool calls in assistant message".into(),
                ));
            }
        };

        let finish_reason = self.finish_reason.as_ref().map(|reason| match reason {
            FinishReason::Stop => NormalizedFinishReason::Stop,
            FinishReason::Length => NormalizedFinishReason::Length,
            FinishReason::ToolCalls => NormalizedFinishReason::ToolCalls,
            FinishReason::ContentFilter => NormalizedFinishReason::ContentFilter,
            _ => NormalizedFinishReason::Other,
        });
        details.finish_reason = self.finish_reason.map(String::from);
        details.reasoning = message.continuation.reasoning;
        details.reasoning_details = message.continuation.reasoning_details;
        details.logprobs = self.logprobs;

        Ok(GenerateResponse {
            id,
            model,
            text,
            tool_calls,
            usage: details.usage.as_ref().map(Usage::from),
            finish_reason,
            native_finish_reason: self
                .native_finish_reason
                .or_else(|| details.finish_reason.clone()),
            provider_details: Some(ProviderDetails::OpenRouter(details)),
        })
    }
}

#[derive(Deserialize)]
pub(super) struct ApiErrorDetails {
    code: Option<ErrorCode>,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorCode {
    Number(serde_json::Number),
    Text(String),
}

impl ApiErrorDetails {
    pub fn into_error(self, status: StatusCode) -> OpenRouterError {
        OpenRouterError::Api {
            status: status.as_u16(),
            code: self.code.map(|code| match code {
                ErrorCode::Number(number) => Value::Number(number),
                ErrorCode::Text(text) => Value::String(text),
            }),
            message: self.message.unwrap_or_else(|| {
                if status.is_success() {
                    "provider returned an error".into()
                } else {
                    status.canonical_reason().unwrap_or("request failed").into()
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn assistant_text_formats_normalize_and_replay_as_text() {
        for content in [
            json!("Hello world"),
            json!([
                {"type": "text", "text": "Hello "},
                {"type": "text", "text": "world"}
            ]),
        ] {
            let payload = json!({
                "choices": [{
                    "message": {"role": "assistant", "content": content},
                    "finish_reason": "stop"
                }]
            });
            let OpenRouterChatCompletionResponse::Completion {
                id,
                model,
                mut choices,
                details,
            } = serde_json::from_value(payload).unwrap()
            else {
                panic!("expected a completion");
            };
            let response = choices
                .remove(0)
                .into_response(StatusCode::OK, id, model, details)
                .unwrap();

            assert_eq!(response.text, "Hello world");
            let replay = ConversationMessage::try_from(response.into_message()).unwrap();
            assert_eq!(
                serde_json::to_value(replay).unwrap()["content"],
                "Hello world"
            );
        }
    }

    #[test]
    fn unsupported_assistant_content_is_explicitly_rejected() {
        let choice: CompletionChoice = serde_json::from_value(json!({
            "message": {"content": [
                {"type": "text", "text": "An image:"},
                {"type": "image_url", "image_url": {"url": "https://example.com/image.png"}}
            ]},
            "finish_reason": "stop"
        }))
        .unwrap();

        assert!(matches!(
            choice.into_response(StatusCode::OK, None, None, Default::default()),
            Err(OpenRouterError::InvalidResponse(message))
                if message == "unsupported assistant content part; expected text"
        ));
    }

    #[test]
    fn reasoning_only_length_response_preserves_usage() {
        let payload = json!({
            "id": "completion-1",
            "model": "reasoning-model",
            "choices": [{
                "message": {"role": "assistant", "content": null},
                "finish_reason": "length"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30,
                "completion_tokens_details": {"reasoning_tokens": 20}
            }
        });
        let OpenRouterChatCompletionResponse::Completion {
            id,
            model,
            mut choices,
            details,
        } = serde_json::from_value(payload).unwrap()
        else {
            panic!("expected a completion");
        };
        let expected_usage = details.usage.clone();
        let response = choices
            .remove(0)
            .into_response(StatusCode::OK, id, model, details)
            .unwrap();

        assert!(response.text.is_empty());
        assert!(response.tool_calls.is_empty());
        assert_eq!(response.id.as_deref(), Some("completion-1"));
        assert_eq!(response.model.as_deref(), Some("reasoning-model"));
        assert_eq!(response.finish_reason, Some(NormalizedFinishReason::Length));
        assert_eq!(response.native_finish_reason.as_deref(), Some("length"));
        let usage = response.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.total_tokens, Some(30));
        assert_eq!(usage.reasoning_tokens, Some(20));
        let Some(ProviderDetails::OpenRouter(details)) = response.provider_details else {
            panic!("expected provider details");
        };
        assert_eq!(details.usage, expected_usage);
    }

    #[test]
    fn null_content_without_tools_is_invalid_for_stop() {
        let choice: CompletionChoice = serde_json::from_value(json!({
            "message": {"role": "assistant", "content": null},
            "finish_reason": "stop"
        }))
        .unwrap();
        assert!(matches!(
            choice.into_response(StatusCode::OK, None, None, Default::default()),
            Err(OpenRouterError::InvalidResponse(_))
        ));
    }
}
