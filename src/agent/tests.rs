use super::*;
use crate::comms::AgentEvent;
use crate::config::provider::ProviderId;
use ai_sdk::{
    GenerateResponse,
    providers::{ModelError, Provider},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct ToolProvider;

#[ai_sdk::async_trait]
impl Provider for ToolProvider {
    fn id(&self) -> &'static str {
        "fake"
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        if matches!(request.messages.last(), Some(Message::Tool { .. })) {
            Ok(GenerateResponse {
                text: "done".into(),
                ..Default::default()
            })
        } else {
            Ok(GenerateResponse {
                tool_calls: vec![ToolCall {
                    id: "call".into(),
                    name: "record".into(),
                    arguments: "{}".into(),
                }],
                ..Default::default()
            })
        }
    }
}

struct RecordingTool(Arc<AtomicUsize>);

#[tool::async_trait]
impl DynTool for RecordingTool {
    fn definition(&self) -> tool::ToolDefinition {
        tool::ToolDefinition {
            name: "record".into(),
            description: "test".into(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }
    async fn execute(&self, _: Value, context: ToolContext) -> Result<Value, tool::ToolError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        context.emit("partial").await?;
        Ok(Value::String("result".into()))
    }
}

fn selection() -> ModelSelection {
    ModelSelection::new(ProviderId::OpenAI, "test".into(), Arc::new(ToolProvider)).unwrap()
}

fn context() -> Vec<Message> {
    vec![Message::system("system"), Message::user("run")]
}

struct OutputTool {
    partial: &'static str,
    result: Value,
}

#[tool::async_trait]
impl DynTool for OutputTool {
    fn definition(&self) -> tool::ToolDefinition {
        tool::ToolDefinition {
            name: "output".into(),
            description: "test output".into(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    async fn execute(&self, _: Value, context: ToolContext) -> Result<Value, tool::ToolError> {
        if !self.partial.is_empty() {
            context.emit(self.partial).await?;
        }
        Ok(self.result.clone())
    }
}

#[tokio::test]
async fn tool_output_is_accumulated_once_without_an_interruption_marker_on_completion() {
    for (partial, result, expected) in [
        (
            "partial",
            serde_json::json!("result"),
            "partial\n\"result\"",
        ),
        (
            "partial\n",
            serde_json::json!("result"),
            "partial\n\"result\"",
        ),
        ("partial", Value::Null, "partial"),
        ("partial", serde_json::json!(""), "partial"),
        ("", Value::Null, "null"),
        ("", serde_json::json!("result"), "\"result\""),
    ] {
        let agent = Agent::new(vec![Box::new(OutputTool { partial, result })]).unwrap();
        let call = ToolCall {
            id: "call".into(),
            name: "output".into(),
            arguments: "{}".into(),
        };
        let (sender, _receiver) = tokio::sync::mpsc::channel(100);
        let mut content = "Not executed".into();
        assert!(
            agent
                .execute_tool(&call, &mut content, &sender)
                .await
                .unwrap()
        );
        assert_eq!(content, expected);
    }
}

#[tokio::test]
async fn executes_with_supplied_context_and_sends_events_in_order() {
    let executions = Arc::new(AtomicUsize::new(0));
    let agent = Agent::new(vec![Box::new(RecordingTool(executions.clone()))]).unwrap();
    let mut context = context();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(100);
    agent
        .run_turn(&mut context, &selection(), &sender)
        .await
        .unwrap();
    assert_eq!(context.len(), 5);
    assert_eq!(context.last(), Some(&Message::assistant("done")));
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert!(matches!(
        receiver.try_recv().unwrap(),
        AgentEvent::ToolCall { .. }
    ));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        AgentEvent::ToolDelta(_)
    ));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        AgentEvent::ToolResult { success: true, .. }
    ));
    assert!(matches!(receiver.try_recv().unwrap(), AgentEvent::TextDelta(text) if text == "done"));
}

#[tokio::test]
async fn closed_event_channel_prevents_tool_execution() {
    let executions = Arc::new(AtomicUsize::new(0));
    let agent = Agent::new(vec![Box::new(RecordingTool(executions.clone()))]).unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    drop(receiver);
    let mut context = context();
    let error = agent
        .run_turn(&mut context, &selection(), &sender)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Channel(_)));
    assert_eq!(executions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn channel_closure_during_output_aborts_the_turn() {
    let executions = Arc::new(AtomicUsize::new(0));
    let agent = Agent::new(vec![Box::new(RecordingTool(executions.clone()))]).unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let selection = selection();
    let mut context = context();
    let turn = agent.run_turn(&mut context, &selection, &sender);
    tokio::pin!(turn);
    // ToolCall fills the channel; the next send blocks after tool execution.
    assert!(futures_util::poll!(&mut turn).is_pending());
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    drop(receiver);
    assert!(matches!(turn.await.unwrap_err(), AppError::Channel(_)));
}

#[tokio::test]
async fn full_event_channel_delays_tool_start() {
    let executions = Arc::new(AtomicUsize::new(0));
    let agent = Agent::new(vec![Box::new(RecordingTool(executions.clone()))]).unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    sender.send(AgentEvent::ContextUpdate(0)).await.unwrap();
    let selection = selection();
    let mut context = context();
    let turn = agent.run_turn(&mut context, &selection, &sender);
    tokio::pin!(turn);
    assert!(futures_util::poll!(&mut turn).is_pending());
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    receiver.recv().await.unwrap();
    let receive = async {
        for _ in 0..4 {
            receiver.recv().await.unwrap();
        }
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(turn, receive)
    })
    .await
    .unwrap();
    result.unwrap();
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}

struct BatchProvider;

#[ai_sdk::async_trait]
impl Provider for BatchProvider {
    fn id(&self) -> &'static str {
        "batch"
    }
    async fn generate(&self, _: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        Ok(GenerateResponse {
            text: "Working on the batch".into(),
            tool_calls: ["record", "wait", "record"]
                .into_iter()
                .enumerate()
                .map(|(i, name)| ToolCall {
                    id: i.to_string(),
                    name: name.into(),
                    arguments: "{}".into(),
                })
                .collect(),
            ..Default::default()
        })
    }
}

struct WaitingTool;

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
        context.emit("partial before cancellation").await?;
        std::future::pending().await
    }
}

#[tokio::test]
async fn dropping_turn_preserves_completed_and_interrupted_tool_results() {
    let executions = Arc::new(AtomicUsize::new(0));
    let agent = Agent::new(vec![
        Box::new(RecordingTool(executions.clone())),
        Box::new(WaitingTool),
    ])
    .unwrap();
    let selection =
        ModelSelection::new(ProviderId::OpenAI, "test".into(), Arc::new(BatchProvider)).unwrap();
    let (sender, _receiver) = tokio::sync::mpsc::channel(100);
    let mut context = context();
    {
        let turn = agent.run_turn(&mut context, &selection, &sender);
        tokio::pin!(turn);
        assert!(futures_util::poll!(&mut turn).is_pending());
    }
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(context.len(), 6);
    assert!(
        matches!(&context[2], Message::Assistant { content, tool_calls, .. }
        if content == "Working on the batch" && tool_calls.len() == 3)
    );
    assert!(matches!(&context[3], Message::Tool { content, .. }
        if content.contains("result") && !content.contains("Interrupted")));
    assert!(matches!(&context[4], Message::Tool { content, .. }
        if content.contains("Interrupted") && content.contains("partial before cancellation")));
    assert!(matches!(&context[5], Message::Tool { content, .. }
        if content.contains("Not executed")));
}
