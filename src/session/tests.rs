use super::*;
use crate::config::provider::ProviderId;

#[test]
fn setup_without_a_prompt_does_not_create_a_log() {
    let directory = tempfile::tempdir().unwrap();
    let store = directory.path().join("sessions");
    let mut session = Session::create(&store).unwrap();
    session
        .record(Event::SessionStarted {
            system_prompt: "system".into(),
            directory: directory.path().into(),
        })
        .unwrap();
    session
        .record(Event::ModelSelected {
            provider: ProviderId::OpenAI,
            model: "model".into(),
        })
        .unwrap();
    session
        .record(Event::DirectoryChanged {
            directory: directory.path().join("project"),
        })
        .unwrap();
    session.record(Event::ConversationCleared).unwrap();
    drop(session);

    assert!(!store.exists());
}

#[test]
fn first_prompt_persists_setup_and_subsequent_events() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = Session::create(directory.path()).unwrap();
    assert_eq!(
        session
            .record(Event::SessionStarted {
                system_prompt: "system".into(),
                directory: directory.path().into(),
            })
            .unwrap(),
        1
    );
    session
        .record(Event::ModelSelected {
            provider: ProviderId::OpenAI,
            model: "model".into(),
        })
        .unwrap();
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);

    assert_eq!(
        session
            .record(Event::PromptSubmitted {
                content: "hello".into(),
            })
            .unwrap(),
        3
    );
    let Storage::Durable(log) = &session.storage else {
        panic!("submitted prompt must persist the session");
    };
    let path = log.path.clone();
    assert_eq!(SessionData::load(&path).unwrap(), session.data);

    session.record(Event::ConversationCleared).unwrap();
    assert_eq!(SessionData::load(&path).unwrap(), session.data);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    drop(session);

    let mut session = Session::open(&path).unwrap();
    session
        .record(Event::PromptSubmitted {
            content: "next prompt".into(),
        })
        .unwrap();
    assert_eq!(SessionData::load(&path).unwrap(), session.data);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn invalid_prompt_does_not_create_a_log() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = Session::create(directory.path()).unwrap();
    assert!(
        session
            .record(Event::PromptSubmitted {
                content: "hello".into(),
            })
            .is_err()
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}
