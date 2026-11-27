use super::*;
use crate::providers::{
    Provider,
    openai::{
        JsonSchema, OpenAIApi, Prediction, PredictionContent, ReasoningEffort, StopSequences,
        Verbosity,
    },
};
use crate::{ContentPart, Image, ImageDetail, ImageMediaType, Tool};

fn payload(output: Value) -> Value {
    json!({
        "id": "resp_1", "model": "test-model", "status": "completed", "output": output,
        "created_at": 123, "service_tier": "default",
        "usage": {"input_tokens": 10, "output_tokens": 20, "total_tokens": 30,
            "input_tokens_details": {"cached_tokens": 5},
            "output_tokens_details": {"reasoning_tokens": 12}}
    })
}

fn message(text: &str) -> Value {
    json!({"id": "msg_1", "type": "message", "role": "assistant", "status": "completed",
        "content": [{"type": "output_text", "text": text, "annotations": []}]})
}

fn call(id: &str) -> Value {
    json!({"id": format!("fc_{id}"), "type": "function_call", "call_id": id,
        "name": "weather", "arguments": "{\"city\":\"Paris\"}", "status": "completed"})
}

fn normalize(value: Value) -> Result<GenerateResponse, OpenAIError> {
    serde_json::from_value::<Response>(value)
        .map_err(|e| OpenAIError::InvalidResponse(e.to_string()))?
        .into_response()
}

fn wire(request: GenerateRequest) -> Value {
    serde_json::to_value(Request::try_from(request).unwrap()).unwrap()
}

#[test]
fn protocol_selection_is_opt_in_and_reversible() {
    let provider = OpenAI::new("test").unwrap();
    assert_eq!(provider.api, OpenAIApi::ChatCompletions);
    assert_eq!(provider.endpoint, super::super::ENDPOINT);
    let provider = provider.with_api(OpenAIApi::Responses);
    assert_eq!(provider.id(), "openai");
    assert_eq!(provider.endpoint, super::super::RESPONSES_ENDPOINT);
    assert_eq!(
        provider.with_api(OpenAIApi::ChatCompletions).endpoint,
        super::super::ENDPOINT
    );
}

#[test]
fn requests_map_messages_images_tools_and_choice() {
    let request = GenerateRequest::from_messages(
        "test",
        [
            Message::system("rules"),
            Message::user([
                ContentPart::text("look"),
                ContentPart::image(
                    Image::base64(ImageMediaType::Png, "aGVsbG8=").with_detail(ImageDetail::High),
                ),
                ContentPart::image(Image::url("https://example.com/a.png")),
            ]),
            Message::Assistant {
                content: "checking".into(),
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "weather".into(),
                    arguments: "{}".into(),
                }],
                provider_details: None,
            },
            Message::tool_result("call_1", "sunny"),
        ],
    )
    .with_tools([Tool::new(
        "weather",
        "get weather",
        json!({"type": "object"}),
    )])
    .with_tool_choice(ToolChoice::Tool("weather".into()));
    let body = wire(request);
    assert_eq!(
        body["input"][0],
        json!({"role": "system", "content": "rules"})
    );
    assert_eq!(
        body["input"][1]["content"][1],
        json!({"type": "input_image", "image_url": "data:image/png;base64,aGVsbG8=", "detail": "high"})
    );
    assert_eq!(
        body["input"][1]["content"][2],
        json!({"type": "input_image", "image_url": "https://example.com/a.png"})
    );
    assert_eq!(
        body["input"][2],
        json!({"role": "assistant", "content": "checking"})
    );
    assert_eq!(body["input"][3]["call_id"], "call_1");
    assert_eq!(
        body["input"][4],
        json!({"type": "function_call_output", "call_id": "call_1", "output": "sunny"})
    );
    assert_eq!(body["tools"][0]["strict"], false);
    assert!(body["tools"][0].get("function").is_none());
    assert_eq!(
        body["tool_choice"],
        json!({"type": "function", "name": "weather"})
    );
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], false);
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert!(body.get("previous_response_id").is_none());
}

#[test]
fn tool_choice_modes_map_directly() {
    for (choice, expected) in [
        (ToolChoice::Auto, "auto"),
        (ToolChoice::None, "none"),
        (ToolChoice::Required, "required"),
    ] {
        assert_eq!(
            wire(GenerateRequest::new("test", "hello").with_tool_choice(choice))["tool_choice"],
            expected
        );
    }
}

#[test]
fn options_translate_without_chat_fields() {
    let body = wire(
        GenerateRequest::new("test", "hi").with_provider_options(OpenAIOptions {
            max_completion_tokens: Some(100),
            reasoning_effort: Some(ReasoningEffort::High),
            verbosity: Some(Verbosity::Low),
            store: Some(true),
            logprobs: Some(true),
            top_logprobs: Some(3),
            response_format: Some(ResponseFormat::JsonSchema {
                json_schema: JsonSchema {
                    name: "answer".into(),
                    schema: json!({"type": "object"}),
                    description: Some("an answer".into()),
                    strict: Some(true),
                },
            }),
            ..Default::default()
        }),
    );
    assert_eq!(body["max_output_tokens"], 100);
    assert_eq!(body["reasoning"], json!({"effort": "high"}));
    assert_eq!(body["text"]["verbosity"], "low");
    assert_eq!(
        body["text"]["format"],
        json!({"type": "json_schema", "name": "answer", "schema": {"type": "object"}, "description": "an answer", "strict": true})
    );
    assert_eq!(body["store"], true);
    assert_eq!(body["top_logprobs"], 3);
    assert_eq!(
        body["include"],
        json!([
            "reasoning.encrypted_content",
            "message.output_text.logprobs"
        ])
    );
    for key in [
        "max_completion_tokens",
        "reasoning_effort",
        "verbosity",
        "response_format",
        "logprobs",
    ] {
        assert!(body.get(key).is_none(), "unexpected Chat field {key}");
    }
}

#[test]
fn unsupported_options_are_rejected() {
    let options = [
        OpenAIOptions {
            frequency_penalty: Some(0.0),
            ..Default::default()
        },
        OpenAIOptions {
            presence_penalty: Some(0.0),
            ..Default::default()
        },
        OpenAIOptions {
            logit_bias: Some(Default::default()),
            ..Default::default()
        },
        OpenAIOptions {
            seed: Some(1),
            ..Default::default()
        },
        OpenAIOptions {
            stop: Some(StopSequences::Single("end".into())),
            ..Default::default()
        },
        OpenAIOptions {
            max_tokens: Some(10),
            ..Default::default()
        },
        OpenAIOptions {
            prediction: Some(Prediction::Content {
                content: PredictionContent::Text("hi".into()),
            }),
            ..Default::default()
        },
    ];
    for options in options {
        assert!(matches!(
            Request::try_from(GenerateRequest::new("test", "hi").with_provider_options(options)),
            Err(OpenAIError::Configuration(_))
        ));
    }
}

#[test]
fn text_usage_and_logprobs_normalize() {
    let mut output = message("hello");
    output["content"][0]["logprobs"] =
        json!([{"token": "hello", "logprob": -0.1, "top_logprobs": []}]);
    output["content"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type": "output_text", "text": " world", "annotations": []}));
    let response = normalize(payload(json!([output, message("!")]))).unwrap();
    assert_eq!(response.text, "hello world!");
    assert_eq!(response.id.as_deref(), Some("resp_1"));
    assert_eq!(response.finish_reason, Some(FinishReason::Stop));
    let usage = response.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(20));
    assert_eq!(usage.total_tokens, Some(30));
    assert_eq!(usage.cached_input_tokens, Some(5));
    assert_eq!(usage.reasoning_tokens, Some(12));
    let Some(ProviderDetails::OpenAI(details)) = response.provider_details else {
        panic!()
    };
    assert_eq!(details.logprobs.unwrap().content.unwrap()[0].token, "hello");
    assert!(details.usage.is_none());
    assert_eq!(details.responses.unwrap().usage.unwrap().input_tokens, 10);
}

#[test]
fn reasoning_and_parallel_calls_survive_serialized_history_and_next_turn() {
    let reasoning = json!({"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "checking weather"}],
        "content": [{"type": "reasoning_text", "text": "reasoning context"}], "status": "completed", "encrypted_content": "opaque"});
    let output = json!([
        reasoning,
        message("checking"),
        call("call_1"),
        call("call_2")
    ]);
    let response = normalize(payload(output.clone())).unwrap();
    assert_eq!(response.finish_reason, Some(FinishReason::ToolCalls));
    assert_eq!(response.tool_calls[0].id, "call_1");
    assert_ne!(response.tool_calls[0].id, "fc_call_1");
    let history = serde_json::to_string(&response.into_message()).unwrap();
    let assistant = serde_json::from_str::<Message>(&history).unwrap();
    let body = wire(GenerateRequest::from_messages(
        "test",
        [
            Message::user("weather?"),
            assistant,
            Message::tool_result("call_1", "sunny"),
            Message::tool_result("call_2", "warm"),
        ],
    ));
    assert_eq!(
        body["input"].as_array().unwrap()[1..5],
        *output.as_array().unwrap()
    );
    assert_eq!(body["input"][5]["call_id"], "call_1");
    assert_eq!(body["input"][6]["call_id"], "call_2");
}

#[test]
fn modified_or_invalid_provider_context_is_rejected() {
    let mut assistant = normalize(payload(json!([message("original")])))
        .unwrap()
        .into_message();
    if let Message::Assistant { content, .. } = &mut assistant {
        *content = "edited".into();
    }
    assert!(matches!(
        Request::try_from(GenerateRequest::from_messages("test", [assistant])),
        Err(OpenAIError::Configuration(_))
    ));
    let mut assistant = normalize(payload(json!([call("call_1")])))
        .unwrap()
        .into_message();
    if let Message::Assistant {
        provider_details: Some(ProviderDetails::OpenAI(details)),
        ..
    } = &mut assistant
    {
        details.responses.as_mut().unwrap().output[0]["type"] = json!("unknown");
    }
    assert!(matches!(
        Request::try_from(GenerateRequest::from_messages("test", [assistant])),
        Err(OpenAIError::Configuration(_))
    ));
}

#[test]
fn old_details_and_other_provider_history_remain_usable() {
    let details: OpenAIResponseDetails = serde_json::from_value(json!({"created": 1})).unwrap();
    assert!(details.responses.is_none());
    let assistant = Message::Assistant {
        content: "old".into(),
        tool_calls: vec![],
        provider_details: Some(ProviderDetails::OpenAI(details)),
    };
    assert_eq!(
        wire(GenerateRequest::from_messages("test", [assistant]))["input"][0]["content"],
        "old"
    );
    let details = crate::providers::anthropic::AnthropicResponseDetails::default();
    let assistant = Message::Assistant {
        content: "other".into(),
        tool_calls: vec![],
        provider_details: Some(ProviderDetails::Anthropic(details)),
    };
    assert_eq!(
        wire(GenerateRequest::from_messages("test", [assistant]))["input"][0]["content"],
        "other"
    );
}

#[test]
fn incomplete_reasoning_preserves_usage_and_maps_finish_reason() {
    for (reason, expected) in [
        ("max_output_tokens", FinishReason::Length),
        ("content_filter", FinishReason::ContentFilter),
        ("future_reason", FinishReason::Other),
    ] {
        let mut value = payload(json!([]));
        value["status"] = json!("incomplete");
        value["incomplete_details"] = json!({"reason": reason});
        let response = normalize(value).unwrap();
        assert!(response.text.is_empty());
        assert_eq!(response.finish_reason, Some(expected));
        assert_eq!(response.native_finish_reason.as_deref(), Some(reason));
        assert_eq!(response.usage.unwrap().reasoning_tokens, Some(12));
    }
}

#[test]
fn partial_malformed_and_duplicate_calls_never_escape() {
    for (status, args) in [
        ("incomplete", "{}"),
        ("in_progress", "{}"),
        ("completed", "{"),
        ("completed", "[]"),
    ] {
        let mut item = call("call_1");
        item["status"] = json!(status);
        item["arguments"] = json!(args);
        assert!(matches!(
            normalize(payload(json!([item]))),
            Err(OpenAIError::InvalidResponse(_))
        ));
    }
    assert!(normalize(payload(json!([call("same"), call("same")]))).is_err());
    let mut value = payload(json!([call("call_1")]));
    value["status"] = json!("incomplete");
    assert!(normalize(value).is_err());
}

#[test]
fn refusals_unexpected_status_and_unknown_output_are_errors() {
    let mut item = message("");
    item["content"] = json!([{"type": "refusal", "refusal": "cannot help"}]);
    assert!(
        matches!(normalize(payload(json!([item]))), Err(OpenAIError::Refusal(message)) if message == "cannot help")
    );
    for status in ["queued", "in_progress", "cancelled", "failed", "unknown"] {
        let mut value = payload(json!([]));
        value["status"] = json!(status);
        assert!(normalize(value).is_err());
    }
    assert!(normalize(payload(json!([{"type": "web_search_call"}]))).is_err());
    let mut value = payload(json!([]));
    value["error"] = json!({"code": "server_error", "message": "failed generation"});
    assert!(
        matches!(normalize(value), Err(OpenAIError::Api { code: Some(code), .. }) if code == "server_error")
    );
}

#[tokio::test]
async fn invalid_options_and_images_fail_before_http() {
    let mut provider = OpenAI::new("test").unwrap().with_api(OpenAIApi::Responses);
    provider.endpoint = "not a URL".into();
    for request in [
        GenerateRequest::new("test", "hi").with_provider_options(OpenAIOptions {
            seed: Some(1),
            ..Default::default()
        }),
        GenerateRequest::new("test", "hi")
            .with_provider_options(crate::providers::anthropic::AnthropicOptions::default()),
        GenerateRequest::from_messages(
            "test",
            [Message::user([ContentPart::image(Image::url(
                "file:///private",
            ))])],
        ),
    ] {
        let error = provider.generate(request).await.unwrap_err();
        assert!(matches!(
            error.downcast_ref::<OpenAIError>(),
            Some(OpenAIError::Configuration(_))
        ));
    }
}

// A one-request local HTTP fixture; no live API or extra test dependencies needed.
fn server(status: &str, body: String) -> (String, std::thread::JoinHandle<(String, Value)>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    let status = status.to_owned();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        let (headers, body_start, length) = loop {
            let n = stream.read(&mut buffer).unwrap();
            assert_ne!(n, 0);
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8(bytes[..index].to_vec()).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                break (headers, index + 4, length);
            }
        };
        while bytes.len() < body_start + length {
            let n = stream.read(&mut buffer).unwrap();
            assert_ne!(n, 0);
            bytes.extend_from_slice(&buffer[..n]);
        }
        let request = serde_json::from_slice(&bytes[body_start..body_start + length]).unwrap();
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        (headers, request)
    });
    (endpoint, handle)
}

#[tokio::test]
async fn http_responses_and_chat_protocols_remain_separate() {
    let (endpoint, handle) = server("200 OK", payload(json!([message("hello")])).to_string());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut provider = OpenAI::with_client("test-key", client)
        .unwrap()
        .with_api(OpenAIApi::Responses);
    provider.endpoint = endpoint;
    assert_eq!(
        provider
            .generate(GenerateRequest::new("test", "hi"))
            .await
            .unwrap()
            .text,
        "hello"
    );
    let (headers, body) = handle.join().unwrap();
    assert!(headers.starts_with("POST /v1/responses HTTP/1.1"));
    assert!(
        headers
            .to_lowercase()
            .contains("authorization: bearer test-key")
    );
    assert!(body.get("input").is_some());
    assert!(body.get("messages").is_none());

    let chat = json!({"id": "chat_1", "model": "test", "choices": [{"message": {"content": "chat"}, "finish_reason": "stop"}]});
    let (endpoint, handle) = server("200 OK", chat.to_string());
    let mut provider = provider.with_api(OpenAIApi::ChatCompletions);
    provider.endpoint = endpoint.replace("/v1/responses", "/v1/chat/completions");
    assert_eq!(
        provider
            .generate(GenerateRequest::new("test", "hi"))
            .await
            .unwrap()
            .text,
        "chat"
    );
    let (headers, body) = handle.join().unwrap();
    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
    assert!(body.get("messages").is_some());
    assert!(body.get("input").is_none());
}

#[tokio::test]
async fn http_errors_and_malformed_success_are_reported() {
    for (status, body, expected) in [
        ("429 Too Many Requests", json!({"error": {"message": "slow down", "code": "rate_limit", "type": "rate_limit_error", "param": null}}).to_string(), "api"),
        ("502 Bad Gateway", "not json".into(), "api"),
        ("200 OK", "not json".into(), "invalid"),
        ("200 OK", "{}".into(), "invalid"),
    ] {
        let (endpoint, handle) = server(status, body);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut provider = OpenAI::with_client("test", client).unwrap().with_api(OpenAIApi::Responses);
        provider.endpoint = endpoint;
        let error = provider.generate(GenerateRequest::new("test", "hi")).await.unwrap_err();
        match (expected, error.downcast_ref::<OpenAIError>().unwrap()) {
            ("api", OpenAIError::Api { status, .. }) => assert!(*status >= 400),
            ("invalid", OpenAIError::InvalidResponse(_)) => {},
            _ => panic!("unexpected error: {error}"),
        }
        handle.join().unwrap();
    }
}
