//! Reference: [official OpenAI Chat Completions documentation](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create).

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{OpenAIError, OpenAIOptions};
use crate::{
    ContentPart, FinishReason as NormalizedFinishReason, GenerateRequest, GenerateResponse,
    ImageDetail, Logprobs, Message, ProviderDetails, ProviderOptions, Tool, ToolCall, ToolChoice,
    Usage, UserContent,
};

#[derive(Serialize)]
pub(super) struct OpenAIChatCompletionRequest {
    model: String,
    messages: Vec<ConversationMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<FunctionTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolSelection>,
    #[serde(flatten)]
    options: OpenAIOptions,
}

impl TryFrom<(String, GenerateRequest)> for OpenAIChatCompletionRequest {
    type Error = OpenAIError;

    fn try_from((model, request): (String, GenerateRequest)) -> Result<Self, Self::Error> {
        let options = match request.provider_options {
            None => OpenAIOptions::default(),
            Some(ProviderOptions::OpenAI(options)) => options,
            Some(_) => {
                return Err(OpenAIError::Configuration(
                    "request options do not belong to OpenAI",
                ));
            }
        };

        options.validate()?;

        Ok(Self {
            model,
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
    type Error = OpenAIError;

    fn try_from(message: Message) -> Result<Self, Self::Error> {
        Ok(match message {
            Message::System { content } => Self::System { content },
            Message::User { content } => {
                content.validate().map_err(OpenAIError::Configuration)?;

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
                ..
            } => Self::Assistant(AssistantMessage {
                content: if content.is_empty() && !tool_calls.is_empty() {
                    None
                } else {
                    Some(content)
                },
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls.into_iter().map(FunctionCall::from).collect())
                },
                refusal: None,
            }),
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
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<FunctionCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refusal: Option<String>,
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
    type Error = OpenAIError;

    fn try_from(call: FunctionCall) -> Result<Self, Self::Error> {
        if call.id.trim().is_empty() || call.function.name.trim().is_empty() {
            return Err(OpenAIError::InvalidResponse(
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
pub(super) enum OpenAIChatCompletionResponse {
    Error {
        error: ApiErrorDetails,
    },
    Completion {
        id: Option<String>,
        model: Option<String>,
        choices: Vec<CompletionChoice>,
        #[serde(flatten)]
        details: OpenAIResponseDetails,
    },
}

#[derive(Deserialize)]
pub(super) struct CompletionChoice {
    message: AssistantMessage,
    finish_reason: FinishReason,
    logprobs: Option<Logprobs>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct OpenAIResponseDetails {
    pub created: Option<u64>,
    pub system_fingerprint: Option<String>,
    pub service_tier: Option<String>,
    pub logprobs: Option<Logprobs>,
    pub usage: Option<OpenAIUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub prompt_tokens_details: Option<PromptTokenDetails>,
    pub completion_tokens_details: Option<CompletionTokenDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PromptTokenDetails {
    pub cached_tokens: Option<u64>,
    pub audio_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CompletionTokenDetails {
    pub reasoning_tokens: Option<u64>,
    pub audio_tokens: Option<u64>,
    pub accepted_prediction_tokens: Option<u64>,
    pub rejected_prediction_tokens: Option<u64>,
}

impl From<&OpenAIUsage> for Usage {
    fn from(usage: &OpenAIUsage) -> Self {
        Self {
            input_tokens: Some(usage.prompt_tokens),
            output_tokens: Some(usage.completion_tokens),
            total_tokens: Some(usage.total_tokens),
            cached_input_tokens: usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens),
            cache_creation_input_tokens: None,
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
    FunctionCall,
    #[serde(untagged)]
    Other(String),
}

impl From<FinishReason> for String {
    fn from(reason: FinishReason) -> Self {
        match reason {
            FinishReason::Stop => "stop",
            FinishReason::Length => "length",
            FinishReason::ToolCalls => "tool_calls",
            FinishReason::ContentFilter => "content_filter",
            FinishReason::FunctionCall => "function_call",
            FinishReason::Other(reason) => return reason,
        }
        .into()
    }
}

impl CompletionChoice {
    pub fn into_response(
        self,
        id: Option<String>,
        model: Option<String>,
        mut details: OpenAIResponseDetails,
    ) -> Result<GenerateResponse, OpenAIError> {
        if let Some(refusal) = self.message.refusal {
            return Err(OpenAIError::Refusal(refusal));
        }

        if matches!(self.finish_reason, FinishReason::FunctionCall) {
            return Err(OpenAIError::InvalidResponse(
                "legacy function_call responses are unsupported; use tools".into(),
            ));
        }

        let tool_calls = self
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(ToolCall::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        if matches!(self.finish_reason, FinishReason::ToolCalls) && tool_calls.is_empty() {
            return Err(OpenAIError::InvalidResponse(
                "tool_calls finish reason without any tool calls".into(),
            ));
        }

        let text = match self.message.content {
            Some(text) => text,
            None if !tool_calls.is_empty()
                || matches!(
                    self.finish_reason,
                    FinishReason::Length | FinishReason::ContentFilter
                ) =>
            {
                String::new()
            }
            None => {
                return Err(OpenAIError::InvalidResponse(
                    "expected text or tool calls in assistant message".into(),
                ));
            }
        };

        let finish_reason = match &self.finish_reason {
            FinishReason::Stop => NormalizedFinishReason::Stop,
            FinishReason::Length => NormalizedFinishReason::Length,
            FinishReason::ToolCalls => NormalizedFinishReason::ToolCalls,
            FinishReason::ContentFilter => NormalizedFinishReason::ContentFilter,
            _ => NormalizedFinishReason::Other,
        };
        details.logprobs = self.logprobs;

        Ok(GenerateResponse {
            id,
            model,
            text,
            tool_calls,
            usage: details.usage.as_ref().map(Usage::from),
            finish_reason: Some(finish_reason),
            native_finish_reason: Some(self.finish_reason.into()),
            provider_details: Some(ProviderDetails::OpenAI(details)),
        })
    }
}

#[derive(Deserialize)]
pub(super) struct ApiErrorDetails {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
    code: Option<String>,
    param: Option<String>,
}

impl ApiErrorDetails {
    pub fn into_error(self, status: StatusCode) -> OpenAIError {
        OpenAIError::Api {
            status: status.as_u16(),
            code: self.code,
            error_type: self.error_type,
            param: self.param,
            message: self.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        let OpenAIChatCompletionResponse::Completion {
            id,
            model,
            mut choices,
            details,
        } = serde_json::from_value(payload).unwrap()
        else {
            panic!("expected a completion");
        };
        let expected_usage = details.usage.clone();
        let response = choices.remove(0).into_response(id, model, details).unwrap();

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
        let Some(ProviderDetails::OpenAI(details)) = response.provider_details else {
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
            choice.into_response(None, None, Default::default()),
            Err(OpenAIError::InvalidResponse(_))
        ));
    }
}
