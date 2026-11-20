use super::*;
use ai_sdk::{
    GenerateRequest, GenerateResponse, ToolCall,
    providers::{ModelError, Provider},
};
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tool::{DynTool, ToolContext};

struct FakeProvider {
    requests: Mutex<Vec<Vec<Message>>>,
    tool: AtomicBool,
    fail: AtomicBool,
}

#[ai_sdk::async_trait]
impl Provider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }
    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        self.requests.lock().unwrap().push(request.messages);
        if self.fail.load(Ordering::SeqCst) {
            return Err("provider unavailable".into());
        }
        if self.tool.load(Ordering::SeqCst) {
            Ok(GenerateResponse {
                tool_calls: vec![ToolCall {
                    id: "call".into(),
                    name: "wait".into(),
                    arguments: "{}".into(),
                }],
                ..Default::default()
            })
        } else {
            Ok(GenerateResponse {
                text: "answer".into(),
                ..Default::default()
            })
        }
    }
}

struct WaitingTool(Arc<AtomicUsize>);

#[tool::async_trait]
impl DynTool for WaitingTool {
    fn definition(&self) -> tool::ToolDefinition {
        tool::ToolDefinition {
            name: "wait".into(),
            description: "wait".into(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }
    async fn execute(&self, _: Value, context: ToolContext) -> Result<Value, tool::ToolError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        context.emit("partial output").await?;
        std::future::pending().await
    }
}

fn catalog() -> Catalog {
    serde_json::from_value(serde_json::json!({
        "openai": {"models": {
            "fake-model": {"id": "fake-model", "name": "Fake model", "modalities": {"output": ["text"]}, "cost": {"input": 0.0, "output": 0.0}, "limit": {"context": 1000}},
            "image-model": {"id": "image-model", "name": "Image model", "modalities": {"output": ["image"]}}
        }},
        "anthropic": {"models": {
            "other-model": {"id": "other-model", "name": "Other model", "modalities": {"output": ["text"]}}
        }}
    })).unwrap()
}

struct Harness {
    commands: Sender<AppCommand>,
    events: Receiver<AppEvent>,
    task: tokio::task::JoinHandle<Result<Vec<Message>, AppError>>,
    provider: Arc<FakeProvider>,
    executions: Arc<AtomicUsize>,
}

impl Harness {
    async fn new(tool: bool, fail: bool, select: bool) -> Self {
        let (commands, receiver) = tokio::sync::mpsc::channel(100);
        let (sender, events) = tokio::sync::mpsc::channel(100);
        let provider = Arc::new(FakeProvider {
            requests: Mutex::new(Vec::new()),
            tool: AtomicBool::new(tool),
            fail: AtomicBool::new(fail),
        });
        let executions = Arc::new(AtomicUsize::new(0));
        let mut providers = ProviderManager::default();
        providers.set_test_client(ProviderId::OpenAI, provider.clone());
        let mut app = App::new(
            Agent::new(vec![Box::new(WaitingTool(executions.clone()))]).unwrap(),
            "system".into(),
            select.then(|| ModelConfig {
                provider: ProviderId::OpenAI,
                model: "fake-model".into(),
            }),
            providers,
            catalog(),
            sender,
            receiver,
        )
        .unwrap();
        let task = tokio::spawn(async move {
            app.run().await?;
            Ok(app.context)
        });
        let mut harness = Self {
            commands,
            events,
            task,
            provider,
            executions,
        };
        let AppEvent::ProvidersUpdated { configured, models } = harness.next().await else {
            panic!("missing provider state")
        };
        assert_eq!(configured, vec![ProviderId::OpenAI]);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "fake-model");
        assert!(models[0].is_free);
        assert!(matches!(
            harness.next().await,
            AppEvent::DirectoryChanged(_)
        ));
        if select {
            assert!(matches!(
                harness.next().await,
                AppEvent::ModelSelected {
                    context_limit: Some(1000),
                    ..
                }
            ));
        }
        harness
    }

    async fn next(&mut self) -> AppEvent {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.events.recv())
            .await
            .unwrap()
            .unwrap()
    }

    async fn until(&mut self, predicate: impl Fn(&AppEvent) -> bool) -> AppEvent {
        loop {
            let event = self.next().await;
            if predicate(&event) {
                return event;
            }
        }
    }

    async fn submit(&mut self, prompt: &str) {
        self.commands
            .send(AppCommand::SubmitPrompt(prompt.into()))
            .await
            .unwrap();
        self.until(|event| matches!(event, AppEvent::Completed))
            .await;
    }

    async fn stop(self) -> Vec<Message> {
        self.commands.send(AppCommand::Shutdown).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }
}

#[tokio::test]
async fn completed_turns_keep_only_in_memory_context_and_clear_resets_it() {
    let mut harness = Harness::new(false, false, true).await;
    harness.submit("hello").await;
    harness.submit("continue").await;
    assert_eq!(harness.provider.requests.lock().unwrap()[1].len(), 4);
    harness
        .commands
        .send(AppCommand::ClearConversation)
        .await
        .unwrap();
    assert!(matches!(
        harness.next().await,
        AppEvent::ConversationCleared
    ));
    harness.submit("fresh").await;
    assert_eq!(harness.provider.requests.lock().unwrap()[2].len(), 2);
    assert_eq!(harness.stop().await.len(), 3);
}

#[tokio::test]
async fn model_selection_uses_identifiers_and_reports_backend_errors() {
    let mut harness = Harness::new(false, false, false).await;
    harness
        .commands
        .send(AppCommand::SetModel {
            provider: ProviderId::OpenAI,
            model: "fake-model".into(),
        })
        .await
        .unwrap();
    assert!(matches!(
        harness.next().await,
        AppEvent::ModelSelected {
            provider: ProviderId::OpenAI,
            context_limit: Some(1000),
            ..
        }
    ));
    harness
        .commands
        .send(AppCommand::SetModel {
            provider: ProviderId::Anthropic,
            model: "other-model".into(),
        })
        .await
        .unwrap();
    assert!(matches!(
        harness.next().await,
        AppEvent::ModelSelectionFailed { .. }
    ));
    harness.submit("still using original model").await;
    assert_eq!(harness.provider.requests.lock().unwrap().len(), 1);
    harness.stop().await;
}

#[tokio::test]
async fn configuring_provider_updates_available_models_without_a_ui_client() {
    let mut harness = Harness::new(false, false, true).await;
    harness
        .commands
        .send(AppCommand::ConfigureProvider {
            provider: ProviderId::Anthropic,
            api_key: "test-key".into(),
        })
        .await
        .unwrap();
    let AppEvent::ProvidersUpdated { configured, models } = harness.next().await else {
        panic!("missing updated provider state")
    };
    assert_eq!(configured, vec![ProviderId::OpenAI, ProviderId::Anthropic]);
    assert_eq!(models.len(), 2);
    assert!(models.iter().any(|model| model.id == "other-model"));
    assert!(matches!(
        harness.next().await,
        AppEvent::ProviderConfigured {
            provider: ProviderId::Anthropic
        }
    ));
    harness.submit("original provider still selected").await;
    harness.stop().await;
}

#[tokio::test]
async fn invalid_provider_configuration_does_not_replace_the_existing_client() {
    let mut harness = Harness::new(false, false, true).await;
    harness
        .commands
        .send(AppCommand::ConfigureProvider {
            provider: ProviderId::OpenAI,
            api_key: String::new(),
        })
        .await
        .unwrap();
    assert!(matches!(
        harness.next().await,
        AppEvent::ProviderConfigurationFailed {
            provider: ProviderId::OpenAI,
            ..
        }
    ));
    harness.submit("original client still works").await;
    assert_eq!(harness.provider.requests.lock().unwrap().len(), 1);
    harness.stop().await;
}

#[tokio::test]
async fn cancelled_tool_turn_keeps_progress_in_the_next_request_context() {
    let mut harness = Harness::new(true, false, true).await;
    harness
        .commands
        .send(AppCommand::SubmitPrompt("run".into()))
        .await
        .unwrap();
    harness
        .until(|event| matches!(event, AppEvent::ToolDelta(_)))
        .await;
    harness.commands.send(AppCommand::Cancel).await.unwrap();
    harness
        .until(|event| matches!(event, AppEvent::Failed(_)))
        .await;
    harness.provider.tool.store(false, Ordering::SeqCst);
    harness.submit("continue").await;
    {
        let requests = harness.provider.requests.lock().unwrap();
        assert_eq!(requests[1].len(), 5);
        assert!(matches!(&requests[1][3], Message::Tool { content, .. }
            if content.contains("Interrupted") && content.contains("partial output")));
    }
    assert_eq!(harness.executions.load(Ordering::SeqCst), 1);
    assert_eq!(harness.stop().await.len(), 6);
}

#[tokio::test]
async fn failed_turn_keeps_the_submitted_prompt() {
    let mut harness = Harness::new(false, true, true).await;
    harness
        .commands
        .send(AppCommand::SubmitPrompt("fail".into()))
        .await
        .unwrap();
    harness
        .until(|event| matches!(event, AppEvent::Failed(_)))
        .await;
    harness.provider.fail.store(false, Ordering::SeqCst);
    harness.submit("retry").await;
    assert_eq!(harness.provider.requests.lock().unwrap()[1].len(), 3);
    harness.stop().await;
}

#[tokio::test]
async fn shutdown_stops_execution_without_discarding_context() {
    let mut harness = Harness::new(false, false, true).await;
    harness.submit("completed turn").await;
    harness.provider.tool.store(true, Ordering::SeqCst);
    harness
        .commands
        .send(AppCommand::SubmitPrompt("run tool".into()))
        .await
        .unwrap();
    harness
        .until(|event| matches!(event, AppEvent::ToolDelta(_)))
        .await;
    let context = harness.stop().await;
    assert_eq!(context.len(), 6);
    assert_eq!(context[2], Message::assistant("answer"));
    assert_eq!(context[3], Message::user("run tool"));
    assert!(matches!(&context[4], Message::Assistant { tool_calls, .. } if tool_calls.len() == 1));
    assert!(matches!(&context[5], Message::Tool { content, .. }
        if content.contains("Interrupted") && content.contains("partial output")));
}

#[tokio::test]
async fn clear_cancels_an_active_turn_and_keeps_system_context() {
    let mut harness = Harness::new(true, false, true).await;
    harness
        .commands
        .send(AppCommand::SubmitPrompt("run".into()))
        .await
        .unwrap();
    harness
        .until(|event| matches!(event, AppEvent::ToolDelta(_)))
        .await;
    harness
        .commands
        .send(AppCommand::ClearConversation)
        .await
        .unwrap();
    assert!(matches!(
        harness.next().await,
        AppEvent::ConversationCleared
    ));
    assert_eq!(harness.stop().await, vec![Message::system("system")]);
}

#[tokio::test]
async fn clear_discards_queued_prompts_but_accepts_new_ones() {
    let mut harness = Harness::new(true, false, true).await;
    harness
        .commands
        .send(AppCommand::SubmitPrompt("run".into()))
        .await
        .unwrap();
    harness
        .until(|event| matches!(event, AppEvent::ToolDelta(_)))
        .await;
    harness.provider.tool.store(false, Ordering::SeqCst);
    for prompt in ["queued first", "queued second"] {
        harness
            .commands
            .send(AppCommand::SubmitPrompt(prompt.into()))
            .await
            .unwrap();
    }
    harness
        .commands
        .send(AppCommand::ClearConversation)
        .await
        .unwrap();
    assert!(matches!(
        harness.next().await,
        AppEvent::ConversationCleared
    ));
    harness.submit("fresh prompt").await;
    {
        let requests = harness.provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1],
            vec![Message::system("system"), Message::user("fresh prompt")]
        );
    }
    assert_eq!(harness.stop().await.len(), 3);
}

#[tokio::test]
async fn clear_still_acknowledges_queued_configuration_requests() {
    let mut harness = Harness::new(true, false, true).await;
    harness
        .commands
        .send(AppCommand::SubmitPrompt("run".into()))
        .await
        .unwrap();
    harness
        .until(|event| matches!(event, AppEvent::ToolDelta(_)))
        .await;
    harness
        .commands
        .send(AppCommand::ConfigureProvider {
            provider: ProviderId::Anthropic,
            api_key: "test-key".into(),
        })
        .await
        .unwrap();
    harness
        .commands
        .send(AppCommand::SetModel {
            provider: ProviderId::OpenAI,
            model: "fake-model".into(),
        })
        .await
        .unwrap();
    harness
        .commands
        .send(AppCommand::ClearConversation)
        .await
        .unwrap();
    assert!(matches!(
        harness.next().await,
        AppEvent::ConversationCleared
    ));
    assert!(matches!(
        harness.next().await,
        AppEvent::ProvidersUpdated { .. }
    ));
    assert!(matches!(
        harness.next().await,
        AppEvent::ProviderConfigured {
            provider: ProviderId::Anthropic
        }
    ));
    assert!(matches!(
        harness.next().await,
        AppEvent::ModelSelected {
            provider: ProviderId::OpenAI,
            ..
        }
    ));
    harness.stop().await;
}

#[tokio::test]
async fn reconnecting_selected_provider_replaces_the_selected_runtime_client() {
    let (commands, receiver) = tokio::sync::mpsc::channel(100);
    let (sender, _events) = tokio::sync::mpsc::channel(100);
    let mut providers = ProviderManager::default();
    providers.set_test_client(
        ProviderId::OpenAI,
        Arc::new(FakeProvider {
            requests: Mutex::new(Vec::new()),
            tool: AtomicBool::new(false),
            fail: AtomicBool::new(false),
        }),
    );
    let mut app = App::new(
        Agent::new(Vec::new()).unwrap(),
        "system".into(),
        Some(ModelConfig {
            provider: ProviderId::OpenAI,
            model: "fake-model".into(),
        }),
        providers,
        catalog(),
        sender,
        receiver,
    )
    .unwrap();
    assert_eq!(app.selection.as_ref().unwrap().client().id(), "fake");
    app.configure_provider(ProviderId::OpenAI, "new-key".into())
        .await
        .unwrap();
    assert_eq!(app.selection.as_ref().unwrap().client().id(), "openai");
    drop(commands);
}

#[tokio::test]
async fn prompt_without_model_is_rejected_by_backend() {
    let mut harness = Harness::new(false, false, false).await;
    harness
        .commands
        .send(AppCommand::SubmitPrompt("hello".into()))
        .await
        .unwrap();
    assert!(matches!(harness.next().await, AppEvent::Failed(_)));
    assert!(harness.provider.requests.lock().unwrap().is_empty());
    assert_eq!(harness.stop().await, vec![Message::system("system")]);
}

#[tokio::test]
async fn final_turn_events_are_forwarded_before_completion() {
    let mut harness = Harness::new(false, false, true).await;
    harness
        .commands
        .send(AppCommand::SubmitPrompt("hello".into()))
        .await
        .unwrap();
    assert!(matches!(harness.next().await, AppEvent::Started));
    assert!(matches!(harness.next().await, AppEvent::TextDelta(text) if text == "answer"));
    assert!(matches!(harness.next().await, AppEvent::Completed));
    harness.stop().await;
}
