//! Typed text/client-tool wire models for the [Messages API](https://platform.claude.com/docs/en/api/messages/create).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AnthropicError, AnthropicOptions};
use crate::{
    ContentPart, FinishReason, GenerateRequest, GenerateResponse, ImageSource, Message,
    ProviderDetails, ProviderOptions, Tool, ToolCall, ToolChoice, Usage, UserContent,
};

#[derive(Serialize)]
pub(super) struct AnthropicMessagesRequest {
    model: String,
    messages: Vec<ConversationMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    system: Vec<SystemContentBlock>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<FunctionTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolSelection>,
    stream: bool,
    #[serde(flatten)]
    options: AnthropicOptions,
}

impl TryFrom<GenerateRequest> for AnthropicMessagesRequest {
    type Error = AnthropicError;

    fn try_from(request: GenerateRequest) -> Result<Self, Self::Error> {
        let options = match request.provider_options {
            None => AnthropicOptions::default(),
            Some(ProviderOptions::Anthropic(options)) => options,
            Some(_) => {
                return Err(AnthropicError::Configuration(
                    "request options do not belong to Anthropic",
                ));
            }
        };
        options.validate()?;

        let mut system = Vec::new();
        let mut messages: Vec<ConversationMessage> = Vec::new();

        for message in request.messages {
            let (role, content) = match message {
                Message::System { content } => {
                    if !messages.is_empty() {
                        return Err(AnthropicError::Configuration(
                            "Anthropic system messages must precede the conversation",
                        ));
                    }
                    system.push(SystemContentBlock::Text { text: content });
                    continue;
                }
                Message::User { content } => (MessageRole::User, user_content(content)?),
                Message::Tool {
                    tool_call_id,
                    content,
                } => (
                    MessageRole::User,
                    vec![ContentBlock::ToolResult {
                        tool_use_id: tool_call_id,
                        content,
                    }],
                ),
                Message::Assistant {
                    content,
                    tool_calls,
                    provider_details,
                } => {
                    let blocks = if let Some(ProviderDetails::Anthropic(details)) = provider_details
                    {
                        let (original_text, original_calls) = summarize(&details.content)?;

                        if original_text != content || original_calls != tool_calls {
                            return Err(AnthropicError::Configuration(
                                "assistant content must match its Anthropic provider details",
                            ));
                        }
                        details.content
                    } else {
                        let mut blocks = Vec::new();

                        if !content.is_empty() {
                            blocks.push(ContentBlock::Text {
                                text: content,
                                citations: None,
                            });
                        }

                        for call in tool_calls {
                            let input = serde_json::from_str(&call.arguments).map_err(|_| {
                                AnthropicError::Configuration(
                                    "Anthropic tool arguments must be a JSON object",
                                )
                            })?;
                            blocks.push(ContentBlock::ToolUse {
                                id: call.id,
                                name: call.name,
                                input,
                            });
                        }
                        blocks
                    };
                    (MessageRole::Assistant, blocks)
                }
            };

            // A parallel batch of tool results must be one user turn.
            if let Some(last) = messages.last_mut()
                && last.role == role
            {
                last.content.extend(content);
            } else {
                messages.push(ConversationMessage { role, content });
            }
        }

        Ok(Self {
            model: request.model,
            messages,
            system,
            tools: request.tools.into_iter().map(FunctionTool::from).collect(),
            tool_choice: ToolSelection::new(request.tool_choice, options.disable_parallel_tool_use),
            stream: false,
            options,
        })
    }
}

fn user_content(content: UserContent) -> Result<Vec<ContentBlock>, AnthropicError> {
    content.validate().map_err(AnthropicError::Configuration)?;

    let parts = match content {
        UserContent::Text(text) => vec![ContentPart::Text(text)],
        UserContent::Parts(parts) => parts,
    };

    parts
        .into_iter()
        .map(|part| match part {
            ContentPart::Text(text) => Ok(ContentBlock::Text {
                text,
                citations: None,
            }),
            ContentPart::Image(image) => {
                if image.detail.is_some() {
                    return Err(AnthropicError::Configuration(
                        "image detail is unsupported by Anthropic",
                    ));
                }

                Ok(ContentBlock::Image {
                    source: image.source,
                })
            }
        })
        .collect()
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SystemContentBlock {
    Text { text: String },
}

#[derive(Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum MessageRole {
    User,
    Assistant,
}

#[derive(Serialize)]
struct ConversationMessage {
    role: MessageRole,
    content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Image {
        source: ImageSource,
    },
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        citations: Option<Vec<Value>>,
    },
    ToolUse {
        id: String,
        name: String,
        input: BTreeMap<String, Value>,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
}

#[derive(Serialize)]
struct FunctionTool {
    name: String,
    description: String,
    input_schema: Value,
}

impl From<Tool> for FunctionTool {
    fn from(tool: Tool) -> Self {
        Self {
            name: tool.name,
            description: tool.description,
            input_schema: tool.parameters,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ToolSelection {
    Auto {
        #[serde(skip_serializing_if = "Option::is_none")]
        disable_parallel_tool_use: Option<bool>,
    },
    None,
    Any {
        #[serde(skip_serializing_if = "Option::is_none")]
        disable_parallel_tool_use: Option<bool>,
    },
    Tool {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        disable_parallel_tool_use: Option<bool>,
    },
}

impl ToolSelection {
    fn new(choice: Option<ToolChoice>, disable_parallel_tool_use: Option<bool>) -> Option<Self> {
        if choice.is_none() && disable_parallel_tool_use.is_none() {
            return None;
        }
        Some(match choice.unwrap_or(ToolChoice::Auto) {
            ToolChoice::Auto => Self::Auto {
                disable_parallel_tool_use,
            },
            ToolChoice::None => Self::None,
            ToolChoice::Required => Self::Any {
                disable_parallel_tool_use,
            },
            ToolChoice::Tool(name) => Self::Tool {
                name,
                disable_parallel_tool_use,
            },
        })
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub(super) enum AnthropicMessagesResponse {
    Message(AssistantResponse),
    Error {
        error: ApiErrorDetails,
        request_id: Option<String>,
    },
}

#[derive(Deserialize)]
pub(super) struct ApiErrorDetails {
    #[serde(rename = "type")]
    pub kind: String,
    pub message: String,
}

#[derive(Deserialize)]
pub(super) struct AssistantResponse {
    id: Option<String>,
    model: Option<String>,
    stop_reason: StopReason,
    #[serde(flatten)]
    details: AnthropicResponseDetails,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AnthropicResponseDetails {
    pub content: Vec<ContentBlock>,
    pub stop_sequence: Option<String>,
    pub stop_details: Option<StopDetails>,
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation: Option<CacheCreationUsage>,
    pub output_tokens_details: Option<OutputTokenDetails>,
    pub server_tool_use: Option<ServerToolUsage>,
    pub inference_geo: Option<String>,
    pub service_tier: Option<String>,
    pub speed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CacheCreationUsage {
    pub ephemeral_5m_input_tokens: u64,
    pub ephemeral_1h_input_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OutputTokenDetails {
    pub thinking_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServerToolUsage {
    pub web_search_requests: Option<u64>,
    pub web_fetch_requests: Option<u64>,
}

impl From<&AnthropicUsage> for Usage {
    fn from(usage: &AnthropicUsage) -> Self {
        let input_tokens = usage
            .input_tokens
            .checked_add(usage.cache_creation_input_tokens.unwrap_or(0))
            .and_then(|tokens| tokens.checked_add(usage.cache_read_input_tokens.unwrap_or(0)));

        Self {
            input_tokens,
            output_tokens: Some(usage.output_tokens),
            total_tokens: input_tokens.and_then(|tokens| tokens.checked_add(usage.output_tokens)),
            cached_input_tokens: usage.cache_read_input_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            reasoning_tokens: usage
                .output_tokens_details
                .as_ref()
                .and_then(|details| details.thinking_tokens),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StopDetails {
    Refusal {
        explanation: Option<String>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum StopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
    PauseTurn,
    Refusal,
    ModelContextWindowExceeded,
    #[serde(untagged)]
    Other(String),
}

impl From<StopReason> for String {
    fn from(reason: StopReason) -> Self {
        match reason {
            StopReason::EndTurn => "end_turn".into(),
            StopReason::MaxTokens => "max_tokens".into(),
            StopReason::StopSequence => "stop_sequence".into(),
            StopReason::ToolUse => "tool_use".into(),
            StopReason::PauseTurn => "pause_turn".into(),
            StopReason::Refusal => "refusal".into(),
            StopReason::ModelContextWindowExceeded => "model_context_window_exceeded".into(),
            StopReason::Other(reason) => reason,
        }
    }
}

fn summarize(blocks: &[ContentBlock]) -> Result<(String, Vec<ToolCall>), AnthropicError> {
    let mut text = String::new();
    let mut calls = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text { text: part, .. } => text.push_str(part),
            ContentBlock::ToolUse { id, name, input } => {
                if id.trim().is_empty() || name.trim().is_empty() {
                    return Err(AnthropicError::InvalidResponse(
                        "tool call ID and name must not be empty".into(),
                    ));
                }

                let arguments = serde_json::to_string(input)
                    .map_err(|error| AnthropicError::InvalidResponse(error.to_string()))?;
                calls.push(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments,
                });
            }
            ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {}
            ContentBlock::Image { .. } | ContentBlock::ToolResult { .. } => {
                return Err(AnthropicError::InvalidResponse(
                    "unexpected image or tool_result in assistant response".into(),
                ));
            }
        }
    }
    Ok((text, calls))
}

impl AssistantResponse {
    pub fn into_response(self) -> Result<GenerateResponse, AnthropicError> {
        let (text, tool_calls) = summarize(&self.details.content)?;

        if let Some(StopDetails::Refusal { explanation }) = self.details.stop_details.as_ref() {
            return Err(AnthropicError::Refusal(explanation.clone().unwrap_or(text)));
        }

        if matches!(self.stop_reason, StopReason::Refusal) {
            return Err(AnthropicError::Refusal(text));
        }

        if matches!(self.stop_reason, StopReason::ToolUse) && tool_calls.is_empty() {
            return Err(AnthropicError::InvalidResponse(
                "tool_use stop reason without tool calls".into(),
            ));
        }

        let finish_reason = match &self.stop_reason {
            StopReason::EndTurn | StopReason::StopSequence => FinishReason::Stop,
            StopReason::MaxTokens | StopReason::ModelContextWindowExceeded => FinishReason::Length,
            StopReason::ToolUse => FinishReason::ToolCalls,
            StopReason::PauseTurn => FinishReason::Pause,
            _ => FinishReason::Other,
        };

        Ok(GenerateResponse {
            id: self.id,
            model: self.model,
            text,
            tool_calls,
            usage: self.details.usage.as_ref().map(Usage::from),
            finish_reason: Some(finish_reason),
            native_finish_reason: Some(self.stop_reason.into()),
            provider_details: Some(ProviderDetails::Anthropic(self.details)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn refusal_uses_explanation_or_falls_back_to_text() {
        for (explanation, expected) in [
            (json!(null), "I cannot help with that."),
            (json!("Policy refusal"), "Policy refusal"),
        ] {
            let payload = json!({
                "type": "message",
                "content": [{"type": "text", "text": "I cannot help with that."}],
                "stop_reason": "refusal",
                "stop_details": {"type": "refusal", "explanation": explanation}
            });
            let AnthropicMessagesResponse::Message(message) =
                serde_json::from_value(payload).unwrap()
            else {
                panic!("expected a message");
            };

            assert!(matches!(
                message.into_response(),
                Err(AnthropicError::Refusal(message)) if message == expected
            ));
        }
    }
}
