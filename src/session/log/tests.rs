use super::*;
use crate::config::provider::ProviderId;
use crate::session::{Event, Session, ToolStatus, TurnOutcome};
use ai_sdk::{Message, ToolCall};

fn start(session: &mut Session) {
    session
        .record(Event::SessionStarted {
            system_prompt: "original system instructions".into(),
            directory: "/project".into(),
        })
        .unwrap();
}

#[test]
fn session_data_matches_read_only_load_after_record_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = Session::create(directory.path()).unwrap();
    assert_eq!(session.data, crate::session::SessionData::default());
    start(&mut session);
    session
        .record(Event::PromptSubmitted {
            content: "hello world".into(),
        })
        .unwrap();
    assert_eq!(session.data.events.len(), 2);
    assert!(session.data.updated > 0);
    let path = session.log.path.clone();
    let loaded = crate::session::SessionData::load(&path).unwrap();
    assert_eq!(session.data, loaded);
    drop(session);
    assert_eq!(Session::open(path).unwrap().data, loaded);
}

#[test]
fn resume_closes_pending_request_and_permits_new_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    wal.record(Event::PromptSubmitted {
        content: "old".into(),
    })
    .unwrap();
    wal.record(Event::ModelRequested).unwrap();
    drop(wal);
    let mut wal = Session::open(&path).unwrap();
    wal.recover().unwrap();
    assert_eq!(wal.data.state.pending_request, None);
    assert_eq!(wal.data.state.outcome, Some(TurnOutcome::Cancelled));
    let sequence = wal.log.sequence;
    wal.recover().unwrap();
    assert_eq!(wal.log.sequence, sequence, "recovery is idempotent");
    wal.record(Event::PromptSubmitted {
        content: "new".into(),
    })
    .unwrap();
}

#[test]
fn records_stop_after_error_and_keep_last_valid_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut log = Log::open(&path).unwrap();
    log.append(Event::ModelRequested).unwrap();
    let valid_bytes = log.file.metadata().unwrap().len();
    log.file.write_all(b"{broken}\n").unwrap();
    // A valid later record must not be visited after the error.
    log.append(Event::ModelRequested).unwrap();

    let mut records = Records::new(&mut log.file).unwrap();
    assert_eq!(records.next().unwrap().unwrap().sequence, 1);
    assert!(records.next().unwrap().is_err());
    assert!(records.next().is_none());
    assert!(records.next().is_none());
    assert_eq!(records.last_sequence, 1);
    assert_eq!(records.valid_bytes, valid_bytes);
}

#[test]
fn records_ignore_incomplete_tail_and_remain_exhausted() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut log = Log::open(&path).unwrap();
    log.append(Event::ModelRequested).unwrap();
    let valid_bytes = log.file.metadata().unwrap().len();
    log.file.write_all(b"{incomplete").unwrap();

    let mut records = Records::new(&mut log.file).unwrap();
    assert_eq!(records.next().unwrap().unwrap().sequence, 1);
    assert!(records.next().is_none());
    assert!(records.next().is_none());
    assert_eq!(records.last_sequence, 1);
    assert_eq!(records.valid_bytes, valid_bytes);
}

fn batch(wal: &mut Session) {
    wal.record(Event::PromptSubmitted {
        content: "run".into(),
    })
    .unwrap();
    let request_sequence = wal.record(Event::ModelRequested).unwrap();
    wal.record(Event::ModelResponded {
        context_tokens: None,
        request_sequence,
        message: Box::new(Message::Assistant {
            content: "working".into(),
            tool_calls: ["first", "second"]
                .into_iter()
                .map(|id| ToolCall {
                    id: id.into(),
                    name: "shell".into(),
                    arguments: "{}".into(),
                })
                .collect(),
            provider_details: None,
        }),
    })
    .unwrap();
}

#[test]
fn legacy_responses_have_unknown_usage_and_do_not_erase_known_usage() {
    let dir = tempfile::tempdir().unwrap();
    let mut wal = Session::open(dir.path().join("session.wal")).unwrap();
    start(&mut wal);
    let request_sequence = wal.record(Event::ModelRequested).unwrap();
    let event: Event = serde_json::from_value(serde_json::json!({
        "type": "model_responded", "request_sequence": request_sequence,
        "message": Message::assistant("legacy answer")
    }))
    .unwrap();
    wal.record(event).unwrap();
    assert_eq!(wal.data.state.context_tokens, None);
    let request_sequence = wal.record(Event::ModelRequested).unwrap();
    wal.record(Event::ModelResponded {
        request_sequence,
        message: Box::new(Message::assistant("counted answer")),
        context_tokens: Some(42),
    })
    .unwrap();
    let request_sequence = wal.record(Event::ModelRequested).unwrap();
    wal.record(Event::ModelResponded {
        request_sequence,
        message: Box::new(Message::assistant("no usage supplied")),
        context_tokens: None,
    })
    .unwrap();
    assert_eq!(wal.data.state.context_tokens, Some(42));
}

#[test]
fn append_preserves_prefix_and_reopen_rebuilds_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    let prefix = std::fs::read(&path).unwrap();
    wal.record(Event::ModelSelected {
        provider: ProviderId::OpenAI,
        model: "model".into(),
    })
    .unwrap();
    wal.record(Event::PromptSubmitted {
        content: "hello\n世界".into(),
    })
    .unwrap();
    let request_sequence = wal.record(Event::ModelRequested).unwrap();
    wal.record(Event::ModelResponded {
        context_tokens: Some(1234),
        request_sequence,
        message: Box::new(Message::assistant("answer")),
    })
    .unwrap();
    wal.record(Event::TurnFinished {
        outcome: TurnOutcome::Completed,
        error: None,
    })
    .unwrap();
    let state = wal.data.state.clone();
    assert!(std::fs::read(&path).unwrap().starts_with(&prefix));
    drop(wal);
    let mut wal = Session::open(&path).unwrap();
    assert_eq!(wal.data.state, state);
    assert_eq!(wal.data.state.context_tokens, Some(1234));
    assert_eq!(wal.log.sequence, 6);
    assert_eq!(wal.record(Event::ConversationCleared).unwrap(), 7);
    assert_eq!(wal.data.state.context_tokens, Some(0));
    assert_eq!(
        wal.data.state.context,
        vec![Message::system("original system instructions")]
    );
    assert_eq!(
        wal.data.state.model,
        Some((ProviderId::OpenAI, "model".into()))
    );
}

#[test]
fn each_append_is_one_readable_json_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    wal.record(Event::PromptSubmitted {
        content: "line one\nline two: 世界 \"quoted\"".into(),
    })
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.iter().filter(|&&byte| byte == b'\n').count(), 2);
    for line in bytes.split_inclusive(|&byte| byte == b'\n') {
        let _: serde_json::Value = serde_json::from_slice(line).unwrap();
    }
    let expected = wal.data.state.clone();
    drop(wal);
    assert_eq!(Session::open(path).unwrap().data.state, expected);
}

#[test]
fn valid_json_whitespace_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let payload = br#"{ "version" : 1, "sequence": 1, "timestamp": 0, "event": {"type":"session_started","system_prompt":"hello","directory":"/project"} }"#;
    let line = Log::encode(payload);
    std::fs::write(&path, &line).unwrap();
    let wal = Session::open(&path).unwrap();
    assert_eq!(wal.data.state.context, vec![Message::system("hello")]);
    drop(wal);
    // JSON whitespace may vary because records are validated by their contents.
    let changed = String::from_utf8(line)
        .unwrap()
        .replace("\"version\" : 1", "\"version\":1");
    std::fs::write(&path, &changed).unwrap();
    assert!(Session::open(&path).is_ok());
}

#[test]
fn corruption_in_a_middle_line_is_not_tail_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    wal.record(Event::PromptSubmitted {
        content: "first prompt".into(),
    })
    .unwrap();
    wal.record(Event::PromptSubmitted {
        content: "second prompt".into(),
    })
    .unwrap();
    drop(wal);
    let contents = std::fs::read_to_string(&path)
        .unwrap()
        .replace("first prompt", "first\u{1}prompt");
    std::fs::write(&path, &contents).unwrap();
    assert!(Session::open(&path).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), contents);
}

#[test]
fn binary_logs_are_rejected_without_modification() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.wal");
    let bytes = b"WAL1\x00\x00\x00\x00";
    std::fs::write(&path, bytes).unwrap();
    assert!(Session::open(&path).is_err());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn every_incomplete_final_line_is_removed_before_next_append() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.wal");
    let mut wal = Session::open(&source).unwrap();
    start(&mut wal);
    let prefix = std::fs::read(&source).unwrap();
    wal.record(Event::PromptSubmitted {
        content: "incomplete prompt".into(),
    })
    .unwrap();
    let full = std::fs::read(&source).unwrap();
    drop(wal);
    let path = dir.path().join("truncated.wal");
    for cut in prefix.len()..full.len() {
        std::fs::write(&path, &full[..cut]).unwrap();
        let mut recovered = Session::open(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), prefix, "cut at {cut}");
        assert_eq!(recovered.log.sequence, 1);
        recovered
            .record(Event::PromptSubmitted {
                content: "replacement".into(),
            })
            .unwrap();
        let expected = recovered.data.state.clone();
        drop(recovered);
        assert_eq!(Session::open(&path).unwrap().data.state, expected);
    }
}

#[test]
fn damaged_complete_lines_are_errors_not_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    drop(wal);
    let original = std::fs::read(&path).unwrap();
    // A complete line with invalid JSON is an error, not tail recovery.
    let mut damaged = original;
    let index = damaged.iter().position(|&byte| byte == b'"').unwrap();
    damaged[index] = b'\x01';
    std::fs::write(&path, &damaged).unwrap();
    assert!(
        matches!(Session::open(&path), Err(error) if error.kind() == io::ErrorKind::InvalidData)
    );
    assert_eq!(std::fs::read(&path).unwrap(), damaged);
}

#[test]
fn unsupported_versions_sequences_and_invalid_json_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.wal");
    for (version, sequence) in [(2, 1), (1, 0), (1, 2)] {
        let record = Record {
            version,
            sequence,
            timestamp: 0,
            event: Event::SessionStarted {
                system_prompt: "system".into(),
                directory: "/project".into(),
            },
        };
        std::fs::write(&path, Log::encode(&serde_json::to_vec(&record).unwrap())).unwrap();
        assert!(Session::open(&path).is_err());
    }
    std::fs::write(&path, Log::encode(b"{broken")).unwrap();
    assert!(Session::open(&path).is_err());
}

#[test]
fn oversized_line_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.wal");
    let mut file = File::create(&path).unwrap();
    io::copy(&mut io::repeat(b' ').take((MAX_LINE + 1) as u64), &mut file).unwrap();
    assert!(Session::open(&path).is_err());
}

#[test]
fn only_one_writer_can_open_a_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let wal = Session::open(&path).unwrap();
    assert!(Session::open(&path).is_err());
    drop(wal);
    assert!(Session::open(&path).is_ok());
}

#[test]
fn failed_write_does_not_install_state_and_poison_prevents_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    let state = wal.data.state.clone();
    // Substitute a read-only descriptor to deterministically inject write failure.
    wal.log.file = File::open(&path).unwrap();
    assert!(
        wal.record(Event::PromptSubmitted {
            content: "not committed".into()
        })
        .is_err()
    );
    assert!(wal.log.poisoned);
    assert_eq!(wal.data.state, state);
    assert_eq!(wal.log.sequence, 1);
    assert!(wal.record(Event::ConversationCleared).is_err());
    drop(wal);
    assert_eq!(Session::open(&path).unwrap().data.state, state);
}

#[test]
fn pending_requests_and_uncertain_tool_effects_are_not_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    batch(&mut wal);
    wal.record(Event::ToolStarted {
        call_id: "first".into(),
    })
    .unwrap();
    drop(wal);
    let mut recovered = Session::open(&path).unwrap();
    assert_eq!(recovered.data.state.tools["first"], ToolStatus::Running);
    assert_eq!(recovered.data.state.tools["second"], ToolStatus::NotStarted);
    assert!(
        matches!(&recovered.data.state.context[3], Message::Tool { content, .. } if content.contains("Outcome unknown"))
    );
    assert!(
        matches!(&recovered.data.state.context[4], Message::Tool { content, .. } if content.contains("Not executed"))
    );
    assert!(recovered.record(Event::ModelRequested).is_err());
    recovered
        .record(Event::ToolInterrupted {
            call_id: "first".into(),
            content: "Interrupted: partial output".into(),
        })
        .unwrap();
    recovered
        .record(Event::TurnFinished {
            outcome: TurnOutcome::Cancelled,
            error: Some("cancelled".into()),
        })
        .unwrap();
    let request = recovered.record(Event::ModelRequested).unwrap();
    drop(recovered);
    assert_eq!(
        Session::open(&path).unwrap().data.state.pending_request,
        Some(request)
    );
}

#[test]
fn completed_tool_content_is_independent_of_display_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    batch(&mut wal);
    wal.record(Event::ToolStarted {
        call_id: "first".into(),
    })
    .unwrap();
    wal.record(Event::ToolFinished {
        call_id: "first".into(),
        content: "authoritative context".into(),
        output: "visible output".into(),
        success: true,
    })
    .unwrap();
    drop(wal);
    let mut wal = Session::open(&path).unwrap();
    assert_eq!(wal.data.state.tools["first"], ToolStatus::Completed);
    assert!(
        matches!(&wal.data.state.context[3], Message::Tool { content, .. } if content == "authoritative context")
    );
    let mut output = None;
    for record in Records::new(&mut wal.log.file).unwrap() {
        let record = record.unwrap();
        if let Event::ToolFinished { output: saved, .. } = record.event {
            output = Some(saved);
        }
    }
    assert_eq!(output.as_deref(), Some("visible output"));
}

#[test]
fn invalid_transition_cannot_change_log_or_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    let original = std::fs::read(&path).unwrap();
    let state = wal.data.state.clone();
    for event in [
        Event::ToolStarted {
            call_id: "missing".into(),
        },
        Event::ModelResponded {
            context_tokens: None,
            request_sequence: 0,
            message: Box::new(Message::assistant("unsolicited")),
        },
        Event::SessionStarted {
            system_prompt: "again".into(),
            directory: "/elsewhere".into(),
        },
    ] {
        assert!(wal.record(event).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(wal.data.state, state);
        assert!(!wal.log.poisoned);
    }
}

#[test]
fn log_persists_records_without_owning_session_transitions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let mut wal = Log::open(&path).unwrap();
    wal.append(Event::ModelRequested).unwrap();
    drop(wal);

    // Transition validation belongs to Session, which rejects the invalid log.
    assert!(Session::open(&path).is_err());
}

#[cfg(unix)]
#[test]
fn logs_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private.wal");
    let _session = Session::open(&path).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

struct AuditedProvider {
    path: std::path::PathBuf,
    requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[ai_sdk::async_trait]
impl ai_sdk::providers::Provider for AuditedProvider {
    fn id(&self) -> &'static str {
        "audited"
    }

    async fn generate(
        &self,
        request: ai_sdk::GenerateRequest,
    ) -> Result<ai_sdk::GenerateResponse, ai_sdk::providers::ModelError> {
        use std::sync::atomic::Ordering;
        self.requests.fetch_add(1, Ordering::SeqCst);
        let mut last = None;
        let mut finished = false;
        let mut file = File::open(&self.path).unwrap();
        for record in Records::new(&mut file).unwrap() {
            let record = record.unwrap();
            finished |= matches!(record.event, Event::ToolFinished { .. });
            last = Some(record.event);
        }
        assert!(matches!(last, Some(Event::ModelRequested)));
        if matches!(request.messages.last(), Some(Message::Tool { .. })) {
            assert!(
                finished,
                "tool result must be committed before requesting the next response"
            );
            Ok(ai_sdk::GenerateResponse {
                text: "done".into(),
                usage: Some(ai_sdk::Usage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    ..Default::default()
                }),
                ..Default::default()
            })
        } else {
            Ok(ai_sdk::GenerateResponse {
                tool_calls: vec![ToolCall {
                    id: "audit-call".into(),
                    name: "audit".into(),
                    arguments: "{}".into(),
                }],
                ..Default::default()
            })
        }
    }
}

struct AuditedTool(std::path::PathBuf);

#[tool::async_trait]
impl tool::DynTool for AuditedTool {
    fn definition(&self) -> tool::ToolDefinition {
        tool::ToolDefinition {
            name: "audit".into(),
            description: "check log before side effects".into(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    async fn execute(
        &self,
        _: serde_json::Value,
        _: tool::ToolContext,
    ) -> Result<serde_json::Value, tool::ToolError> {
        let mut last = None;
        let mut file = File::open(&self.0).unwrap();
        for record in Records::new(&mut file).unwrap() {
            last = Some(record.unwrap().event);
        }
        assert!(matches!(last, Some(Event::ToolStarted { call_id }) if call_id == "audit-call"));
        Ok(serde_json::json!("known result"))
    }
}

#[tokio::test]
async fn external_operations_observe_their_committed_start_events() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    wal.record(Event::PromptSubmitted {
        content: "run".into(),
    })
    .unwrap();
    let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let selection = crate::provider_manager::ModelSelection::new(
        ProviderId::OpenAI,
        "test".into(),
        std::sync::Arc::new(AuditedProvider {
            path: path.clone(),
            requests: requests.clone(),
        }),
    )
    .unwrap();
    let agent = crate::agent::Agent::new(vec![Box::new(AuditedTool(path.clone()))]).unwrap();
    let (events, _receiver) = tokio::sync::mpsc::channel(100);
    agent.run_turn(&selection, &events, &mut wal).await.unwrap();
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(wal.data.state.context_tokens, Some(150));
    let context = wal.data.state.context.clone();
    drop(wal);
    assert_eq!(Session::open(&path).unwrap().data.state.context, context);
    assert_eq!(
        requests.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "replay cannot call providers"
    );
}

#[tokio::test]
async fn failed_request_append_prevents_any_provider_call() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.wal");
    let mut wal = Session::open(&path).unwrap();
    start(&mut wal);
    wal.record(Event::PromptSubmitted {
        content: "run".into(),
    })
    .unwrap();
    let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let selection = crate::provider_manager::ModelSelection::new(
        ProviderId::OpenAI,
        "test".into(),
        std::sync::Arc::new(AuditedProvider {
            path: path.clone(),
            requests: requests.clone(),
        }),
    )
    .unwrap();
    let agent = crate::agent::Agent::new(vec![]).unwrap();
    let (events, _receiver) = tokio::sync::mpsc::channel(100);
    wal.log.file = File::open(path).unwrap();
    assert!(matches!(
        agent.run_turn(&selection, &events, &mut wal).await,
        Err(crate::error::AppError::Wal(_))
    ));
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0);
}
