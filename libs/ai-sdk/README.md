# AI SDK

The OpenAI provider supports Chat Completions (the default) and the Responses API:

```rust,no_run
use ai_sdk::{generate_text, GenerateRequest};
use ai_sdk::providers::{OpenAI, openai::OpenAIApi};

# async fn example() -> Result<(), ai_sdk::providers::ModelError> {
let provider = OpenAI::new(std::env::var("OPENAI_API_KEY")?)?
    .with_api(OpenAIApi::Responses);
let response = generate_text(&provider, GenerateRequest::new("gpt-5", "Hello")).await?;
println!("{}", response.text);
# Ok(())
# }
```

Both protocols use the same requests, messages, tools, and responses. Responses
requests send full conversation history and default to `store: false`. Use
`response.into_message()` to retain OpenAI provider details when appending an
assistant message to history: these details preserve ordered output and encrypted
reasoning across tool turns. Do not edit the text or tool calls of a message
without also removing its original Responses details. No server-managed
conversation or hidden per-client conversation state is used.

## OpenAI options with Responses

`OpenAIOptions` remains the per-request options type for both protocols.

| Options | Responses behavior |
| --- | --- |
| `temperature`, `top_p`, `service_tier`, `store`, `parallel_tool_calls`, `metadata` | Passed through (`store` defaults to false) |
| `safety_identifier`, `prompt_cache_key`, `prompt_cache_options`, `prompt_cache_retention`, `user` | Passed through |
| `max_completion_tokens` | Mapped to `max_output_tokens` |
| `reasoning_effort` | Mapped to `reasoning.effort` |
| `verbosity` | Mapped to `text.verbosity` |
| `response_format` | Mapped to `text.format`, flattening JSON schema configuration |
| `logprobs`, `top_logprobs` | `logprobs: true` requests output-text logprobs via `include`; `top_logprobs` is passed through |
| `max_tokens` | Rejected; use `max_completion_tokens` |
| `frequency_penalty`, `presence_penalty`, `logit_bias`, `seed`, `stop`, `prediction` | Rejected before HTTP; use Chat Completions |

Model-specific support is checked by OpenAI. Functions explicitly use non-strict
schemas, matching existing Chat Completions behavior. Incomplete Responses with
function calls return an error rather than exposing potentially partial calls to
the tool executor. Unknown output item types are rejected rather than discarded.
Streaming, hosted tools, background jobs, and the Conversations API are not exposed.

Responses metadata is an optional, defaulted addition to `OpenAIResponseDetails`;
old serialized messages remain readable. Rust code constructing that details
struct should use `..Default::default()` to accommodate the new field.
