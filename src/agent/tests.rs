use super::*;
use crate::session::{ToolStatus, TurnOutcome};
use ai_sdk::Message;

struct StreamingTool;

#[tool::async_trait]
impl DynTool for StreamingTool {
    fn definition(&self) -> tool::ToolDefinition {
        tool::ToolDefinition {
            name: "stream".into(),
            description: "stream output then wait".into(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    async fn execute(&self, _: Value, context: ToolContext) -> Result<Value, tool::ToolError> {
        context.emit("first chunk\n").await.unwrap();
        context.emit("second chunk\n").await.unwrap();
        std::future::pending().await
    }
}

#[tokio::test]
async fn cancellation_preserves_streamed_output_in_session_and_replay() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut session = Session::open(&path).unwrap();
    session
        .record(Event::SessionStarted {
            system_prompt: "system".into(),
            directory: directory.path().into(),
        })
        .unwrap();
    let request_sequence = session.record(Event::ModelRequested).unwrap();
    let calls = vec![
        ToolCall {
            id: "running".into(),
            name: "stream".into(),
            arguments: "{}".into(),
        },
        ToolCall {
            id: "unstarted".into(),
            name: "stream".into(),
            arguments: "{}".into(),
        },
    ];
    session
        .record(Event::ModelResponded {
            request_sequence,
            context_tokens: None,
            message: Box::new(Message::Assistant {
                content: String::new(),
                tool_calls: calls.clone(),
                provider_details: None,
            }),
        })
        .unwrap();
    let agent = Agent::new(vec![Box::new(StreamingTool)]).unwrap();
    let (events, mut receiver) = mpsc::channel(100);
    {
        let execution = agent.execute_tool(&calls[0], &events, &mut session);
        tokio::pin!(execution);
        loop {
            tokio::select! {
                result = &mut execution => panic!("tool should wait: {result:?}"),
                event = receiver.recv() => {
                    if matches!(event, Some(AgentEvent::ToolDelta(_))) {
                        break;
                    }
                }
            }
        }
        // Dropping execution must drain buffered deltas into session context.
    }
    assert_eq!(session.data.state.tools["running"], ToolStatus::Running);
    assert_eq!(
        session.data.state.tools["unstarted"],
        ToolStatus::NotStarted
    );
    let Message::Tool { content, .. } = &session.data.state.context[2] else {
        panic!("reserved result missing");
    };
    assert!(content.starts_with("Interrupted:"));
    assert_eq!(content.matches("first chunk").count(), 1);
    assert_eq!(content.matches("second chunk").count(), 1);
    session.mark_running_tools_interrupted().unwrap();
    session
        .record(Event::TurnFinished {
            outcome: TurnOutcome::Cancelled,
            error: None,
        })
        .unwrap();
    let expected = session.data.state.clone();
    drop(session);
    assert_eq!(Session::open(path).unwrap().data.state, expected);
}
