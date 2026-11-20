use super::*;
use crate::comms::ModelOption;
use ratatui::{Terminal, backend::TestBackend};

fn test_tui() -> (Tui, Receiver<AppCommand>) {
    let (commands_tx, commands_rx) = tokio::sync::mpsc::channel(100);
    let (_events_tx, events_rx) = tokio::sync::mpsc::channel(100);
    (Tui::new(commands_tx, events_rx), commands_rx)
}

fn provider_state() -> AppEvent {
    AppEvent::ProvidersUpdated {
        configured: vec![ProviderId::OpenAI],
        models: vec![ModelOption {
            provider: ProviderId::OpenAI,
            id: "test-model".into(),
            name: "Visible backend model".into(),
            is_free: true,
        }],
    }
}

#[tokio::test]
async fn model_popup_renders_backend_data_and_waits_for_selection_ack() {
    let (mut tui, mut commands) = test_tui();
    tui.handle_app_event(provider_state());
    tui.handle_slash_commands("/models").await;
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| tui.render(frame, frame.area()))
        .unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(screen.contains("Visible backend model"));
    tui.handle_model_popup_action(ModelPopupAction::Select {
        provider: ProviderId::OpenAI,
        model: "test-model".into(),
    });
    assert!(
        matches!(commands.try_recv().unwrap(), AppCommand::SetModel { provider: ProviderId::OpenAI, model } if model == "test-model")
    );
    assert!(tui.model_popup.visible);
    assert!(tui.model_popup.current.is_none());
    tui.handle_app_event(AppEvent::ModelSelected {
        provider: ProviderId::OpenAI,
        model: "test-model".into(),
        context_limit: Some(1000),
    });
    assert!(!tui.model_popup.visible);
    assert_eq!(
        tui.model_popup.current,
        Some((ProviderId::OpenAI, "test-model".into()))
    );
    assert!(tui.pending_model.is_none());
}

#[tokio::test]
async fn model_selection_failure_preserves_current_model_and_allows_retry() {
    let (mut tui, mut commands) = test_tui();
    tui.handle_app_event(AppEvent::ModelSelected {
        provider: ProviderId::OpenAI,
        model: "old".into(),
        context_limit: None,
    });
    tui.model_popup.open();
    tui.set_model(ProviderId::OpenAI, "new".into()).unwrap();
    commands.try_recv().unwrap();
    tui.handle_app_event(AppEvent::ModelSelectionFailed {
        error: "unavailable".into(),
    });
    assert!(tui.model_popup.visible);
    assert_eq!(
        tui.model_popup.current,
        Some((ProviderId::OpenAI, "old".into()))
    );
    tui.set_model(ProviderId::OpenAI, "retry".into()).unwrap();
    assert!(
        matches!(commands.try_recv().unwrap(), AppCommand::SetModel { model, .. } if model == "retry")
    );
}

#[test]
fn provider_configuration_is_a_command_not_a_local_client() {
    let (mut tui, mut commands) = test_tui();
    let popup = ProviderPopup::new(Vec::new());
    tui.handle_provider_popup_action(
        ProviderPopupAction::Configure {
            provider: ProviderId::OpenAI,
            api_key: "test-key".into(),
        },
        popup,
    );
    assert!(
        matches!(commands.try_recv().unwrap(), AppCommand::ConfigureProvider { provider: ProviderId::OpenAI, api_key } if api_key == "test-key")
    );
    assert!(tui.configured_providers.is_empty());
    assert!(tui.provider_popup.is_some());
    tui.handle_app_event(provider_state());
    tui.handle_app_event(AppEvent::ProviderConfigured {
        provider: ProviderId::OpenAI,
    });
    assert!(tui.provider_popup.is_none());
    assert_eq!(tui.configured_providers, vec![ProviderId::OpenAI]);
}

#[test]
fn provider_failure_keeps_popup_and_releases_pending_request() {
    let (mut tui, _commands) = test_tui();
    tui.pending_provider = Some(ProviderId::OpenAI);
    tui.provider_popup = Some(ProviderPopup::new(Vec::new()));
    tui.handle_app_event(AppEvent::ProviderConfigurationFailed {
        provider: ProviderId::OpenAI,
        error: "invalid key".into(),
    });
    assert!(tui.pending_provider.is_none());
    assert!(tui.provider_popup.is_some());
    assert!(tui.configured_providers.is_empty());
}

#[test]
fn startup_model_update_does_not_acknowledge_a_different_pending_selection() {
    let (mut tui, mut commands) = test_tui();
    tui.model_popup.open();
    tui.set_model(ProviderId::OpenAI, "new".into()).unwrap();
    commands.try_recv().unwrap();
    tui.handle_app_event(AppEvent::ModelSelected {
        provider: ProviderId::OpenAI,
        model: "initial".into(),
        context_limit: None,
    });
    assert_eq!(tui.pending_model, Some((ProviderId::OpenAI, "new".into())));
    assert!(tui.model_popup.visible);
    tui.handle_app_event(AppEvent::ModelSelected {
        provider: ProviderId::OpenAI,
        model: "new".into(),
        context_limit: None,
    });
    assert!(tui.pending_model.is_none());
    assert!(!tui.model_popup.visible);
}

#[tokio::test]
async fn clear_only_resets_view_after_backend_acknowledges() {
    let (mut tui, mut commands) = test_tui();
    tui.busy = true;
    assert!(tui.handle_slash_commands("/clear").await);
    assert!(matches!(
        commands.try_recv().unwrap(),
        AppCommand::ClearConversation
    ));
    assert!(tui.busy);
    assert!(tui.clear_pending);
    tui.handle_app_event(AppEvent::ConversationCleared);
    assert!(!tui.busy);
    assert!(!tui.clear_pending);
    for name in ["/sessions", "/resume", "/new"] {
        assert!(SlashCommand::parse(name).0.is_none());
    }
}
