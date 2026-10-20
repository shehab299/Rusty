# Simple AI SDK

A small Rust library with one entry point:

```rust,ignore
let response = generate_text(&model, GenerateRequest::new("Explain Rust traits")).await?;
println!("{}", response.text);
```

`generate_text` accepts a model implementing `Model` and a `GenerateRequest`.
It returns a `GenerateResponse` with text, tool calls, and a finish reason, or the
model's original error. Each call generates one assistant turn.

Built-in providers are OpenRouter, OpenAI, and Anthropic. For OpenRouter:

```rust,no_run
use sdk::{generate_text, GenerateRequest, providers::{OpenRouter, ModelError}};

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let model = OpenRouter::new(
        std::env::var("OPENROUTER_API_KEY")?,
        std::env::var("OPENROUTER_MODEL")?,
    )?;
    let response = generate_text(&model, GenerateRequest::new("Explain Rust traits")).await?;
    println!("{}", response.text);
    Ok(())
}
```

Add `tokio = { version = "1", features = ["macros", "rt-multi-thread"] }` to your
application's dependencies. Set `OPENROUTER_API_KEY` and `OPENROUTER_MODEL` to
your key and an OpenRouter model ID, then run the included example:

```sh
cargo run --example openrouter -- "Explain Rust traits"
```

The adapter uses OpenRouter's [chat completions API](https://openrouter.ai/docs/api/api-reference/chat/create-a-chat-completion)
with bearer authentication. It supports non-streaming conversations and tool calling and uses
120-second request and 10-second connection timeouts. `OpenRouter::with_client`
accepts a custom `reqwest::Client` to configure timeouts and proxies.

API, transport, configuration, and invalid-response errors are exposed as
`providers::openrouter::OpenRouterError`. Use `error.downcast_ref::<OpenRouterError>()`
to inspect an error returned by `generate_text`. API errors preserve the HTTP
status, provider code, and message, including errors embedded in HTTP 200 responses.
Requests are not retried automatically.

For OpenAI, use the same entry point with an `OpenAI` model:

```rust,no_run
use sdk::{generate_text, GenerateRequest, providers::{OpenAI, ModelError}};

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let model = OpenAI::new(
        std::env::var("OPENAI_API_KEY")?,
        std::env::var("OPENAI_MODEL")?,
    )?;
    let response = generate_text(&model, GenerateRequest::new("Explain Rust traits")).await?;
    println!("{}", response.text);
    Ok(())
}
```

Set `OPENAI_API_KEY` and `OPENAI_MODEL` to your API key and a Chat Completions
model ID, then run:

```sh
cargo run --example openai -- "Explain Rust traits"
```

This adapter follows the [official OpenAI Chat Completions documentation](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create)
and sends non-streaming text conversations and function tools to
`https://api.openai.com/v1/chat/completions`. Requests and responses use typed
Serde structs and enums. The shared message roles are system, user, assistant,
and tool. User messages accept text and images. Audio/video inputs, custom tools,
and deprecated `function_call` responses
are outside this SDK's current text/function-tool API.

`providers::openai::OpenAIError` exposes configuration, transport, API, refusal,
and invalid-response errors. API errors preserve the HTTP status, error code,
type, parameter, and message. Assistant refusals are returned as
`OpenAIError::Refusal`. Truncation and content filtering remain visible through
`response.finish_reason`.

OpenAI uses the same 120-second request and 10-second connection timeouts without
automatic retries. `OpenAI::with_client` accepts a custom `reqwest::Client` for
timeouts, proxies, or default organization/project headers. Provider response
details are not sent as OpenAI conversation input.

For Anthropic, use the Messages API adapter:

```rust,no_run
use sdk::{generate_text, GenerateRequest, providers::{Anthropic, ModelError}};

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let model = Anthropic::new(
        std::env::var("ANTHROPIC_API_KEY")?,
        std::env::var("ANTHROPIC_MODEL")?,
    )?;
    let response = generate_text(&model, GenerateRequest::new("Explain Rust traits")).await?;
    println!("{}", response.text);
    Ok(())
}
```

Set `ANTHROPIC_API_KEY` and `ANTHROPIC_MODEL`, then run
`cargo run --example anthropic -- "Explain Rust traits"`.

The adapter follows the [Anthropic Messages API](https://platform.claude.com/docs/en/api/messages/create)
using `x-api-key` authentication and API version `2023-06-01`. Leading system
messages become top-level system blocks. Consecutive tool results are grouped
into one user message. Responses expose normalized `finish_reason` values and
preserve native stop reasons such as `end_turn`, `tool_use`, and `max_tokens`
in `native_finish_reason`.

Anthropic uses the same 120-second request and 10-second connection timeouts,
with `Anthropic::with_client` for custom transport settings. Errors are available
as `providers::anthropic::AnthropicError`; API failures retain the HTTP status,
error type, message, and request ID. Refusals return `AnthropicError::Refusal`.
The adapter covers non-streaming text, image inputs, client tools, and thinking
blocks; documents and Anthropic server tools are outside the current shared API.

Send images through user messages, using the same request with any of the three
providers and a model that supports image input:

```rust
use sdk::{ContentPart, GenerateRequest, Image, Message};

let request = GenerateRequest::from_messages([Message::user([
    ContentPart::text("What is in this image?"),
    ContentPart::image(Image::url("https://example.com/photo.png")),
])]);
```

User content accepts a string or an ordered array/vector of `ContentPart`s,
including multiple images. `Message::user("Hello")` still works. When constructing
`Message::User` directly, its `content` field now takes `UserContent` rather than
`String`; use `content: "Hello".into()` for plain text.

For local bytes, the SDK handles base64 encoding. Already encoded data can be
passed to `Image::base64` without a data URL prefix:

```rust,no_run
use sdk::{Image, ImageMediaType};

let bytes = std::fs::read("photo.png")?;
let image = Image::from_bytes(ImageMediaType::Png, bytes);
let encoded = Image::base64(ImageMediaType::Png, "BASE64_ENCODED_IMAGE_DATA");
# Ok::<(), std::io::Error>(())
```

JPEG, PNG, GIF, and WebP have typed media types. The SDK checks URL schemes,
base64 syntax, and empty content arrays before sending requests. Image formats,
dimensions, request limits, and model capabilities are checked by the provider.
The SDK passes hosted URLs to the provider and does not download images itself.

OpenAI and OpenRouter encode images as `image_url` parts with hosted or data
URLs. They accept `.with_detail(ImageDetail::High)` and other `ImageDetail`
variants where the selected model supports them. Anthropic uses native `image`
source blocks and rejects explicit detail settings instead of silently dropping
them. System, assistant, and tool messages remain text-based in the shared API;
image generation and editing are not included.

These formats follow the [OpenAI image-input reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create),
[Anthropic vision guide](https://platform.claude.com/docs/en/build-with-claude/vision),
and [OpenRouter vision guide](https://openrouter.ai/blog/tutorials/send-image-to-llm/).

Run the vision example with the provider's API key and a vision-capable model
configured through its usual environment variables:

```sh
cargo run --example vision -- openai ./photo.png "Describe this image"
cargo run --example vision -- anthropic https://example.com/photo.png
cargo run --example vision -- openrouter ./photo.webp
```

Attach typed provider-specific fields to an individual request:

```rust
use sdk::{GenerateRequest, providers::openai::{OpenAIOptions, ReasoningEffort, ServiceTier}};

let request = GenerateRequest::new("Solve this problem")
    .with_provider_options(OpenAIOptions {
        reasoning_effort: Some(ReasoningEffort::High),
        max_completion_tokens: Some(2048),
        service_tier: Some(ServiceTier::Flex),
        ..Default::default()
    });
```

OpenRouter has its own reasoning and routing types:

```rust
use sdk::{GenerateRequest, providers::openrouter::{OpenRouterOptions, ProviderRouting, ReasoningEffort, ReasoningOptions}};

let request = GenerateRequest::new("Solve this problem")
    .with_provider_options(OpenRouterOptions {
        reasoning: Some(ReasoningOptions {
            effort: Some(ReasoningEffort::High),
            ..Default::default()
        }),
        provider: Some(ProviderRouting {
            allow_fallbacks: Some(false),
            require_parameters: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    });
```

Anthropic requires an output token limit. The SDK defaults it to 1,024; configure
it and optional thinking or stop sequences per request:

```rust
use sdk::{GenerateRequest, providers::anthropic::{AnthropicOptions, Thinking}};

let request = GenerateRequest::new("Solve this problem")
    .with_provider_options(AnthropicOptions {
        max_tokens: 4096,
        thinking: Some(Thinking::Adaptive),
        ..Default::default()
    });
```

`Thinking` also supports `Enabled { budget_tokens }` and `Disabled` for models
that accept those modes. Manual budgets must be at least 1,024 tokens and below
`max_tokens`. Model-specific support is checked by Anthropic.

Options are serialized into the provider's request using Serde structs. Unset
fields are omitted; explicitly supplied `false` values are preserved. Passing
options to the wrong provider returns a configuration error before
HTTP. Options survive request cloning, including across tool turns, and can be
cleared by setting `request.provider_options = None` when switching providers.

Sampling controls are optional fields on each provider's options:

```rust
use sdk::{GenerateRequest, providers::openai::OpenAIOptions};

let request = GenerateRequest::new("Write a short story")
    .with_provider_options(OpenAIOptions {
        temperature: Some(0.7),
        // Alternatively, configure top_p instead of temperature.
        ..Default::default()
    });
```

The supported request fields include:

| Provider reference | Sampling | Other supported controls |
| --- | --- | --- |
| [OpenAI Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create) | `temperature`, `top_p`, frequency/presence penalties, `logit_bias`, `seed` | Token limits, stop sequences, log probabilities, reasoning effort, service tier, verbosity, storage, parallel tool calls, metadata, response format, predicted output, safety identifier, and request-level prompt caching |
| [Anthropic Messages](https://platform.claude.com/docs/en/api/messages/create) | `temperature`, `top_p`, `top_k` | Token limit, stop sequences, thinking, structured output and effort, user metadata, automatic caching, service tier, speed, inference geography, diagnostics, and parallel tool use |
| [OpenRouter Chat Completions](https://openrouter.ai/docs/api/api-reference/chat/create-a-chat-completion) | `temperature`, `top_p`, `top_k`, `min_p`, `top_a`, frequency/presence/repetition penalties, `logit_bias`, `seed` | Token limits, stop sequences, log probabilities, reasoning, provider routing and price/performance preferences, model fallbacks, parallel tool calls, response format, predicted text output, service tier, metadata, tracing, sessions, and request-level caching |

These structs are **not exhaustive representations of the provider APIs**.
Streaming, audio/video inputs, image outputs, server tools/plugins, and multiple output
choices are outside this SDK's current interface. Per-content cache breakpoints
are also not exposed. Response formats cover text, JSON object, and JSON Schema
for OpenAI/OpenRouter, and JSON Schema for Anthropic.

The SDK validates options before sending a request:

- OpenAI checks sampling and bias ranges, log-probability dependencies, stop
  sequence counts, metadata limits, safety identifier length, and response
  format names and schema objects.
- Anthropic checks sampling ranges, manual thinking budgets, user and diagnostic
  identifier lengths, and output schema objects. `max_tokens: 0` remains valid
  for warming the prompt cache when manual thinking is not enabled.
- OpenRouter checks sampling and bias ranges, positive output token limits,
  log-probability dependencies, stop sequence counts, metadata and session
  limits, conflicting reasoning effort settings, and response schema objects.
  Routing performance thresholds and price caps must be finite and nonnegative;
  zero price caps are allowed for free providers.

Numeric checks also reject NaN and infinity. Providers check model-specific
support, limits, and combinations. In particular,
Anthropic's current reference deprecates sampling controls on newer models;
exposing a field does not mean every model accepts it. Some optional features
also require provider-specific headers, which can be supplied through
`with_client`.

`provider_options` configures a request. Responses expose:

- `id` and `model`: the response ID and actual model reported by the provider.
- `usage`: common input, output, total, cache, and reasoning token counts.
- `finish_reason`: `Stop`, `Length`, `ToolCalls`, `ContentFilter`, `Pause`, or
  `Other`. Unknown provider reasons map to `Other`.
- `native_finish_reason`: the original reason string. OpenRouter uses the
  upstream `native_finish_reason`, falling back to its gateway reason when absent.
- `provider_details`: typed OpenAI, Anthropic, or OpenRouter response details.

Missing response fields remain `None`; token counts are not inferred from text.
The common input count includes cached tokens. Anthropic's uncached input,
cache reads, and cache writes are summed to match that definition, with the
native breakdown preserved in `AnthropicResponseDetails.usage`. Its total is
computed from that input count plus output tokens. Other providers' totals
are taken directly from their responses. Tokenizers differ between models,
so counts are not directly comparable as a measure of text length.

```rust
use sdk::{FinishReason, GenerateResponse, ProviderDetails};

fn inspect(response: &GenerateResponse) {
    if response.finish_reason == Some(FinishReason::Length) {
        println!("Output was truncated: {:?}", response.native_finish_reason);
    }

    if let Some(usage) = &response.usage {
        println!("Input: {:?}, output: {:?}", usage.input_tokens, usage.output_tokens);
    }

    match &response.provider_details {
        Some(ProviderDetails::OpenAI(details)) => {
            println!("Log probabilities: {:?}", details.logprobs);
        }
        Some(ProviderDetails::OpenRouter(details)) => {
            println!("Cost: {:?}", details.usage.as_ref().and_then(|usage| usage.cost));
        }
        Some(ProviderDetails::Anthropic(details)) => {
            println!("Content blocks: {:?}", details.content);
        }
        _ => {}
    }
}
```

Provider details cover the supported text/tool response features, rather than
every field in each provider's API. Refusals and API errors retain their existing
provider error types. `GenerateResponse` and assistant messages now use
`provider_details` instead of the old `provider_metadata` JSON map, and
`finish_reason` is an enum instead of a string.

Supply conversation history in chronological order:

```rust,no_run
use sdk::{generate_text, GenerateRequest, Message, providers::{Model, ModelError}};

async fn conversation(model: &dyn Model) -> Result<(), ModelError> {
    let mut request = GenerateRequest::from_messages([
        Message::system("Keep answers brief."),
        Message::user("My name is Noor."),
        Message::assistant("Hello, Noor!"),
        Message::user("What is my name?"),
    ]);
    let response = generate_text(model, request.clone()).await?;
    println!("{}", response.text);
    request.messages.push(response.into_message());

    request.messages.push(Message::user("Spell it backwards."));
    let response = generate_text(model, request).await?;
    println!("{}", response.text);
    Ok(())
}
```

`GenerateRequest::new(prompt)` is a shortcut for a single user message. Requests
now store `messages` instead of `prompt`; the model does not store history for you.

Declare tools using names, descriptions, and JSON Schema parameters:

```rust
use sdk::{GenerateRequest, Tool, ToolChoice};
use serde_json::json;

let request = GenerateRequest::new("What is 12 + 30?")
    .with_tools([Tool::new("add", "Add two integers", json!({
        "type": "object",
        "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}},
        "required": ["a", "b"],
        "additionalProperties": false,
    }))])
    .with_tool_choice(ToolChoice::Auto);
```

The response's `tool_calls` contains each call's `id`, `name`, and raw JSON
`arguments`. Text may be empty when the model requests tools. Parse and validate
arguments and dispatch the functions in your application. Append
`response.into_message()` first, then one `Message::tool_result(call.id, result)`
for every call before generating the next turn. Keep the tool definitions on
each request. All providers support this function-tool flow, as described in the
[OpenRouter tool documentation](https://openrouter.ai/docs/guides/features/tool-calling)
and [OpenAI Chat Completions reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create),
with Anthropic's [client-tool format](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools)
handled by its adapter;
tool execution and loop limits belong to the application.

`ToolChoice` supports `Auto`, `None`, `Required`, and `Tool(name)` to require a
specific function. Choose a model that supports tools. The runnable
example validates arguments, handles multiple calls, returns tool errors to the
model, and limits generation to eight turns:

```sh
cargo run --example tools
```

For the same tool example with OpenAI, set `OPENAI_API_KEY` and `OPENAI_MODEL`,
then run `cargo run --example tools -- openai`.

For Anthropic, set `ANTHROPIC_API_KEY` and `ANTHROPIC_MODEL`, then run
`cargo run --example tools -- anthropic`. The example uses `ToolChoice::Auto`;
forced tool modes depend on the chosen Claude model.

Use `response.into_message()` to preserve provider continuation details, including
OpenRouter reasoning details and Anthropic thinking signatures and block order,
when replaying assistant messages. Anthropic also retains text and tool blocks
in these details; leave the replayed assistant text, calls, and continuation
details unchanged. Each adapter only replays its own continuation fields;
usage, log probabilities, costs, and response IDs are not sent as message input.

Add other providers by implementing `Model`; the `generate_text` API stays the same:

```rust
use sdk::{async_trait, generate_text, GenerateRequest, GenerateResponse, Message, UserContent, providers::{Model, ModelError}};

struct EchoModel;

#[async_trait]
impl Model for EchoModel {
    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        // Call your provider here. This example echoes the latest user message.
        let text = request.messages.into_iter().rev().find_map(|message| match message {
            Message::User { content: UserContent::Text(content) } => Some(content),
            _ => None,
        }).unwrap_or_default();
        Ok(GenerateResponse { text, ..Default::default() })
    }
}

async fn example() -> Result<(), ModelError> {
    let model = EchoModel;
    let request = GenerateRequest::new("Explain Rust traits");
    let response = generate_text(&model, request).await?;

    println!("{}", response.text);
    Ok(())
}
```

Models can also be selected at runtime using `Box<dyn Model>`; pass
`model.as_ref()` to `generate_text`.

Use `#[async_trait]` on both the `Model` trait and its implementations to retain
runtime provider selection with `async fn`. The SDK re-exports this macro as
`sdk::async_trait`, so custom models can import it directly.

The core API stays provider-independent. All built-in providers use `reqwest`
and `serde_json` and require a Tokio runtime. Custom models choose their own
transport and runtime.

Run `cargo check --all-targets` to check the library and runnable examples without
calling provider APIs.
