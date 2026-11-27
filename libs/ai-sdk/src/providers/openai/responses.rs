//! Private wire adapter for https://developers.openai.com/api/docs/guides/migrate-to-responses.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{OpenAI, OpenAIError, OpenAIOptions, OpenAIResponseDetails, ResponseFormat};
use crate::{
    ContentPart, FinishReason, GenerateRequest, GenerateResponse, Logprobs, Message,
    ProviderDetails, ProviderOptions, TokenLogprob, ToolCall, ToolChoice, Usage, UserContent,
};

/// Context needed to continue a stateless Responses conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAIResponsesDetails {
    pub status: String,
    pub incomplete_reason: Option<String>,
    pub usage: Option<OpenAIResponsesUsage>,
    /// Ordered output items, including encrypted reasoning. Validated on reuse.
    pub output: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAIResponsesUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub input_tokens_details: Option<InputTokenDetails>,
    pub output_tokens_details: Option<OutputTokenDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputTokenDetails {
    pub cached_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputTokenDetails {
    pub reasoning_tokens: Option<u64>,
}

impl From<&OpenAIResponsesUsage> for Usage {
    fn from(usage: &OpenAIResponsesUsage) -> Self {
        Self {
            input_tokens: Some(usage.input_tokens),
            output_tokens: Some(usage.output_tokens),
            total_tokens: Some(usage.total_tokens),
            cached_input_tokens: usage
                .input_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens),
            reasoning_tokens: usage
                .output_tokens_details
                .as_ref()
                .and_then(|d| d.reasoning_tokens),
            cache_creation_input_tokens: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OutputItem {
    Message {
        id: String,
        role: AssistantRole,
        status: String,
        content: Vec<OutputContent>,
    },
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        arguments: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    Reasoning {
        id: String,
        summary: Vec<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<Vec<Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AssistantRole {
    Assistant,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OutputContent {
    OutputText {
        text: String,
        #[serde(default)]
        annotations: Vec<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        logprobs: Option<Vec<TokenLogprob>>,
    },
    Refusal {
        refusal: String,
    },
}

#[derive(Deserialize)]
struct Response {
    id: String,
    model: String,
    status: String,
    output: Vec<OutputItem>,
    usage: Option<OpenAIResponsesUsage>,
    created_at: Option<u64>,
    service_tier: Option<String>,
    incomplete_details: Option<IncompleteDetails>,
    error: Option<ResponseError>,
}

#[derive(Deserialize)]
struct IncompleteDetails {
    reason: String,
}

#[derive(Deserialize)]
struct ResponseError {
    code: String,
    message: String,
}

#[derive(Serialize)]
pub(super) struct Request {
    model: String,
    input: Vec<Value>,
    stream: bool,
    include: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
    #[serde(flatten)]
    options: serde_json::Map<String, Value>,
}

impl TryFrom<GenerateRequest> for Request {
    type Error = OpenAIError;

    fn try_from(request: GenerateRequest) -> Result<Self, Self::Error> {
        let options = match request.provider_options {
            None => OpenAIOptions::default(),
            Some(ProviderOptions::OpenAI(options)) => options,
            Some(_) => {
                return Err(OpenAIError::Configuration(
                    "request options do not belong to OpenAI",
                ));
            }
        };
        let include_logprobs = options.logprobs == Some(true);
        let mut include = vec!["reasoning.encrypted_content"];
        if include_logprobs {
            include.push("message.output_text.logprobs");
        }
        let options = response_options(options)?;
        let mut input = Vec::new();
        for message in request.messages {
            match message {
                Message::System { content } => {
                    input.push(json!({"role": "system", "content": content}))
                }
                Message::User { content } => {
                    content.validate().map_err(OpenAIError::Configuration)?;
                    let content = match content {
                        UserContent::Text(text) => json!(text),
                        UserContent::Parts(parts) => Value::Array(parts.into_iter().map(|part| match part {
                            ContentPart::Text(text) => json!({"type": "input_text", "text": text}),
                            ContentPart::Image(image) => {
                                let mut part = json!({"type": "input_image", "image_url": image.source.into_url()});
                                if let Some(detail) = image.detail { part["detail"] = json!(detail); }
                                part
                            }
                        }).collect()),
                    };
                    input.push(json!({"role": "user", "content": content}));
                }
                Message::Tool {
                    tool_call_id,
                    content,
                } => input.push(json!({
                    "type": "function_call_output", "call_id": tool_call_id, "output": content,
                })),
                Message::Assistant {
                    content,
                    tool_calls,
                    provider_details,
                } => {
                    if let Some(ProviderDetails::OpenAI(OpenAIResponseDetails {
                        responses: Some(details),
                        ..
                    })) = provider_details
                    {
                        let items: Vec<OutputItem> = details
                            .output
                            .into_iter()
                            .map(serde_json::from_value)
                            .collect::<Result<_, _>>()
                            .map_err(|_| {
                                OpenAIError::Configuration(
                                    "invalid Responses context in provider details",
                                )
                            })?;
                        let (text, calls, _) = summarize(&items).map_err(|_| {
                            OpenAIError::Configuration(
                                "invalid Responses context in provider details",
                            )
                        })?;
                        if text != content || calls != tool_calls {
                            return Err(OpenAIError::Configuration(
                                "assistant content must match its OpenAI Responses provider details",
                            ));
                        }
                        for item in items {
                            input.push(
                                serde_json::to_value(item)
                                    .map_err(|e| OpenAIError::InvalidResponse(e.to_string()))?,
                            );
                        }
                    } else {
                        if !content.is_empty() || tool_calls.is_empty() {
                            input.push(json!({"role": "assistant", "content": content}));
                        }
                        for call in tool_calls {
                            input.push(json!({"type": "function_call", "call_id": call.id, "name": call.name, "arguments": call.arguments}));
                        }
                    }
                }
            }
        }
        Ok(Self {
            model: request.model,
            input,
            stream: false,
            include,
            options,
            tools: request
                .tools
                .into_iter()
                .map(|tool| {
                    json!({
                        "type": "function", "name": tool.name, "description": tool.description,
                        "parameters": tool.parameters, "strict": false,
                    })
                })
                .collect(),
            tool_choice: request.tool_choice.map(|choice| match choice {
                ToolChoice::Auto => json!("auto"),
                ToolChoice::None => json!("none"),
                ToolChoice::Required => json!("required"),
                ToolChoice::Tool(name) => json!({"type": "function", "name": name}),
            }),
        })
    }
}

fn response_options(options: OpenAIOptions) -> Result<serde_json::Map<String, Value>, OpenAIError> {
    options.validate()?;
    if options.frequency_penalty.is_some()
        || options.presence_penalty.is_some()
        || options.logit_bias.is_some()
        || options.seed.is_some()
        || options.stop.is_some()
        || options.prediction.is_some()
    {
        return Err(OpenAIError::Configuration(
            "Responses does not support frequency_penalty, presence_penalty, logit_bias, seed, stop, or prediction; use Chat Completions",
        ));
    }
    if options.max_tokens.is_some() {
        return Err(OpenAIError::Configuration(
            "Responses does not support legacy max_tokens; use max_completion_tokens",
        ));
    }
    let mut wire = serde_json::Map::new();
    // An exhaustive destructure makes newly added options require an explicit decision here.
    let OpenAIOptions {
        temperature,
        top_p,
        frequency_penalty: _,
        presence_penalty: _,
        logit_bias: _,
        seed: _,
        stop: _,
        logprobs: _,
        top_logprobs,
        max_tokens: _,
        max_completion_tokens,
        reasoning_effort,
        service_tier,
        verbosity,
        store,
        parallel_tool_calls,
        metadata,
        response_format,
        prediction: _,
        safety_identifier,
        prompt_cache_key,
        prompt_cache_options,
        prompt_cache_retention,
        user,
    } = options;
    macro_rules! insert {
        ($($name:ident),* $(,)?) => {$(if let Some(value) = $name { wire.insert(stringify!($name).into(), json!(value)); })*};
    }
    insert!(
        temperature,
        top_p,
        top_logprobs,
        service_tier,
        parallel_tool_calls,
        metadata,
        safety_identifier,
        prompt_cache_key,
        prompt_cache_options,
        prompt_cache_retention,
        user
    );
    wire.insert("store".into(), json!(store.unwrap_or(false)));
    if let Some(limit) = max_completion_tokens {
        wire.insert("max_output_tokens".into(), json!(limit));
    }
    if let Some(effort) = reasoning_effort {
        wire.insert("reasoning".into(), json!({"effort": effort}));
    }
    let mut text = serde_json::Map::new();
    if let Some(verbosity) = verbosity {
        text.insert("verbosity".into(), json!(verbosity));
    }
    if let Some(format) = response_format {
        text.insert("format".into(), match format {
            ResponseFormat::Text => json!({"type": "text"}),
            ResponseFormat::JsonObject => json!({"type": "json_object"}),
            ResponseFormat::JsonSchema { json_schema } => {
                let mut value = json!({"type": "json_schema", "name": json_schema.name, "schema": json_schema.schema});
                if let Some(description) = json_schema.description { value["description"] = json!(description); }
                if let Some(strict) = json_schema.strict { value["strict"] = json!(strict); }
                value
            }
        });
    }
    if !text.is_empty() {
        wire.insert("text".into(), Value::Object(text));
    }
    Ok(wire)
}

type Summary = (String, Vec<ToolCall>, Vec<TokenLogprob>);

fn summarize(items: &[OutputItem]) -> Result<Summary, OpenAIError> {
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut logprobs = Vec::new();
    for item in items {
        match item {
            OutputItem::Message { content, .. } => {
                for part in content {
                    match part {
                        OutputContent::OutputText {
                            text: part,
                            logprobs: probs,
                            ..
                        } => {
                            text.push_str(part);
                            logprobs.extend(probs.iter().flatten().cloned());
                        }
                        OutputContent::Refusal { refusal } => {
                            return Err(OpenAIError::Refusal(refusal.clone()));
                        }
                    }
                }
            }
            OutputItem::FunctionCall {
                call_id,
                name,
                arguments,
                status,
                ..
            } => {
                if call_id.trim().is_empty()
                    || name.trim().is_empty()
                    || status.as_deref().is_some_and(|s| s != "completed")
                {
                    return Err(OpenAIError::InvalidResponse(
                        "invalid or incomplete function call".into(),
                    ));
                }
                if calls.iter().any(|call: &ToolCall| call.id == *call_id) {
                    return Err(OpenAIError::InvalidResponse(
                        "duplicate function call ID".into(),
                    ));
                }
                if !serde_json::from_str::<Value>(arguments).is_ok_and(|value| value.is_object()) {
                    return Err(OpenAIError::InvalidResponse(
                        "function arguments must be a complete JSON object".into(),
                    ));
                }
                calls.push(ToolCall {
                    id: call_id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                });
            }
            OutputItem::Reasoning { .. } => {}
        }
    }
    Ok((text, calls, logprobs))
}

impl Response {
    fn into_response(self) -> Result<GenerateResponse, OpenAIError> {
        if let Some(error) = self.error {
            return Err(OpenAIError::Api {
                status: 200,
                code: Some(error.code),
                error_type: None,
                param: None,
                message: error.message,
            });
        }
        if !matches!(self.status.as_str(), "completed" | "incomplete") {
            return Err(OpenAIError::InvalidResponse(format!(
                "unexpected response status: {}",
                self.status
            )));
        }
        let (text, tool_calls, logprobs) = summarize(&self.output)?;
        if self.status == "incomplete" && !tool_calls.is_empty() {
            return Err(OpenAIError::InvalidResponse(
                "incomplete response contains function calls".into(),
            ));
        }
        let incomplete_reason = self.incomplete_details.map(|d| d.reason);
        let finish_reason = if self.status == "incomplete" {
            match incomplete_reason.as_deref() {
                Some("max_output_tokens") => FinishReason::Length,
                Some("content_filter") => FinishReason::ContentFilter,
                _ => FinishReason::Other,
            }
        } else if !tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        };
        let output = self
            .output
            .into_iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .map_err(|e| OpenAIError::InvalidResponse(e.to_string()))?;
        Ok(GenerateResponse {
            id: Some(self.id),
            model: Some(self.model),
            text,
            tool_calls,
            usage: self.usage.as_ref().map(Usage::from),
            finish_reason: Some(finish_reason),
            native_finish_reason: Some(
                incomplete_reason
                    .clone()
                    .unwrap_or_else(|| self.status.clone()),
            ),
            provider_details: Some(ProviderDetails::OpenAI(OpenAIResponseDetails {
                created: self.created_at,
                service_tier: self.service_tier,
                logprobs: (!logprobs.is_empty()).then_some(Logprobs {
                    content: Some(logprobs),
                    refusal: None,
                }),
                responses: Some(OpenAIResponsesDetails {
                    status: self.status,
                    incomplete_reason,
                    usage: self.usage,
                    output,
                }),
                ..Default::default()
            })),
        })
    }
}

impl OpenAI {
    pub(super) async fn generate_response(
        &self,
        request: GenerateRequest,
    ) -> Result<GenerateResponse, OpenAIError> {
        let request = Request::try_from(request)?;
        let response = self
            .client
            .post(&self.endpoint)
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .json(&request)
            .send()
            .await
            .map_err(OpenAIError::Transport)?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(OpenAIError::Transport)?;
        #[derive(Deserialize)]
        struct ErrorEnvelope {
            error: super::types::ApiErrorDetails,
        }
        if let Ok(envelope) = serde_json::from_slice::<ErrorEnvelope>(&bytes) {
            return Err(envelope.error.into_error(status));
        }
        if !status.is_success() {
            return Err(OpenAIError::Api {
                status: status.as_u16(),
                code: None,
                error_type: None,
                param: None,
                message: status.canonical_reason().unwrap_or("request failed").into(),
            });
        }
        serde_json::from_slice::<Response>(&bytes)
            .map_err(|e| OpenAIError::InvalidResponse(e.to_string()))?
            .into_response()
    }
}

#[cfg(test)]
mod tests;
