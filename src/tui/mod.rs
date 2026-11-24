use std::{io, time::Duration};

use crate::{
    comms::{AppCommand, AppEvent},
    config::provider::ProviderId,
    error::AppError,
    session::cache_hit_percent,
};
use components::{
    Component, FooterComponent, MessageComponent, MessagesComponent, ModelPopup, ModelPopupAction,
    PromptComponent, ProviderPopup, ProviderPopupAction, SessionPopup, SlashCommand,
    SlashCommandMenu, SlashCommandMenuAction,
};
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event as TerminalEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind,
    },
    execute,
};
use futures_util::StreamExt;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};
use ratatui_textarea::Input;
use tokio::{
    sync::mpsc::{Receiver, Sender},
    time::{Instant, interval},
};

mod components;

pub struct Tui {
    commands_tx: Sender<AppCommand>,
    events_rx: Receiver<AppEvent>,

    messages: MessagesComponent,
    prompt: PromptComponent,
    slash_commands_menu: SlashCommandMenu,
    footer: FooterComponent,

    provider_popup: Option<ProviderPopup>,
    model_popup: ModelPopup,
    session_popup: SessionPopup,

    resume_pending: bool,
    clear_pending: bool,
    configured_providers: Vec<ProviderId>,
    pending_model: Option<(ProviderId, String)>,
    pending_provider: Option<ProviderId>,

    busy: bool,
    should_quit: bool,
    terminal_active: bool,
}

impl Tui {
    pub fn new(commands: Sender<AppCommand>, events: Receiver<AppEvent>) -> Self {
        Self {
            commands_tx: commands,
            events_rx: events,
            messages: MessagesComponent::default(),
            prompt: PromptComponent::new(),
            slash_commands_menu: SlashCommandMenu::default(),
            footer: FooterComponent::new(),
            busy: false,
            should_quit: false,
            terminal_active: false,
            provider_popup: None,
            model_popup: ModelPopup::new(),
            session_popup: SessionPopup::default(),
            resume_pending: false,
            clear_pending: false,
            configured_providers: Vec::new(),
            pending_model: None,
            pending_provider: None,
        }
    }

    // Helpers

    fn is_popup_visible(&self) -> bool {
        self.model_popup.visible || self.session_popup.visible || self.provider_popup.is_some()
    }

    fn set_model(&mut self, provider: ProviderId, model: String) -> Result<(), AppError> {
        if self.pending_model.is_some() {
            return Ok(());
        }
        self.commands_tx.try_send(AppCommand::SetModel {
            provider,
            model: model.clone(),
        })?;
        self.pending_model = Some((provider, model));
        Ok(())
    }

    async fn handle_slash_command_menu_action(&mut self, action: SlashCommandMenuAction) -> bool {
        match action {
            SlashCommandMenuAction::Ignored => false,
            SlashCommandMenuAction::Consumed => true,
            SlashCommandMenuAction::Complete(command) => {
                self.prompt.set_content(command.name());
                self.slash_commands_menu.update(command.name());
                true
            }
            SlashCommandMenuAction::Execute(command) => {
                self.handle_slash_commands(command.name()).await;
                true
            }
        }
    }

    fn handle_model_popup_action(&mut self, action: ModelPopupAction) {
        match action {
            ModelPopupAction::None => {}
            ModelPopupAction::Connect => {
                self.provider_popup = Some(ProviderPopup::new(self.configured_providers.clone()));
            }
            ModelPopupAction::Select { provider, model } => match self.set_model(provider, model) {
                Ok(()) => self.model_popup.set_error("Selecting model…".into()),
                Err(error) => self.model_popup.set_error(error.to_string()),
            },
        }
    }

    fn handle_provider_popup_action(
        &mut self,
        action: ProviderPopupAction,
        mut popup: ProviderPopup,
    ) {
        match action {
            ProviderPopupAction::None => self.provider_popup = Some(popup),
            ProviderPopupAction::Close => {}
            ProviderPopupAction::Configure { provider, api_key } => {
                if self.pending_provider.is_some() {
                    self.provider_popup = Some(popup);
                    return;
                }
                match self
                    .commands_tx
                    .try_send(AppCommand::ConfigureProvider { provider, api_key })
                {
                    Ok(()) => {
                        self.pending_provider = Some(provider);
                        popup.set_message("Connecting…");
                    }
                    Err(error) => {
                        popup.set_message(error.to_string());
                    }
                }
                self.provider_popup = Some(popup);
            }
        }
    }

    async fn handle_slash_commands(&mut self, command: impl Into<String>) -> bool {
        let command = command.into();
        if !command.starts_with('/') {
            return false;
        }

        let (slash_command, args) = SlashCommand::parse(&command);

        match slash_command {
            Some(SlashCommand::Session) => {
                self.prompt.clear();
                self.slash_commands_menu.update("");
                if self.busy || self.clear_pending {
                    self.footer
                        .set_status("Cancel the active turn before switching sessions.");
                } else {
                    self.session_popup.open();
                    if let Err(error) = self.commands_tx.try_send(AppCommand::ListSessions) {
                        self.session_popup.set_error(error.to_string());
                    }
                }
                true
            }
            Some(SlashCommand::Connect) => {
                self.provider_popup = Some(ProviderPopup::new(self.configured_providers.clone()));
                self.prompt.clear();
                self.slash_commands_menu.update("");
                true
            }
            Some(SlashCommand::Models) => {
                self.model_popup.open();
                self.prompt.clear();
                self.slash_commands_menu.update("");
                true
            }
            Some(SlashCommand::CD) => {
                let error = match args.first() {
                    Some(path) => self
                        .commands_tx
                        .try_send(AppCommand::SetDirectory((*path).into()))
                        .err()
                        .map(|error| error.to_string()),
                    None => Some("Usage: /cd <path>".to_string()),
                };
                if let Some(content) = error {
                    self.messages.push(MessageComponent::Error { content });
                    self.messages.scroll_to_bottom();
                }
                self.prompt.clear();
                self.slash_commands_menu.update("");
                true
            }
            Some(SlashCommand::Exit) => {
                self.should_quit = true;
                self.prompt.clear();
                self.slash_commands_menu.update("");
                true
            }
            Some(SlashCommand::Clear) => {
                self.prompt.clear();
                self.slash_commands_menu.update("");
                let result = self
                    .commands_tx
                    .try_send(AppCommand::ClearConversation)
                    .map_err(|_| "Failed to clear the conversation".to_string());

                if let Err(e) = result {
                    self.messages.push(MessageComponent::Error {
                        content: e.to_string(),
                    });
                    self.messages.scroll_to_bottom();
                } else {
                    self.clear_pending = true;
                }

                true
            }
            _ => false,
        }
    }

    // Event Handling

    async fn handle_terminal_event(&mut self, event: TerminalEvent) {
        if let TerminalEvent::Mouse(e) = event {
            if self.is_popup_visible() {
                return;
            }
            match e.kind {
                MouseEventKind::ScrollUp => self.messages.scroll_up(),
                MouseEventKind::ScrollDown => self.messages.scroll_down(),
                _ => {}
            }
            return;
        }

        if let TerminalEvent::Paste(s) = event {
            self.prompt.insert_text(&s);
            return;
        }

        let key = match event {
            TerminalEvent::Key(k) => k,
            _ => return,
        };

        if key.kind == KeyEventKind::Release {
            return;
        }

        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            self.should_quit = true;
            return;
        }

        if self.session_popup.visible {
            if let Some(id) = self.session_popup.handle_key(key) {
                match self.commands_tx.try_send(AppCommand::ResumeSession(id)) {
                    Ok(()) => self.resume_pending = true,
                    Err(error) => self.session_popup.set_error(error.to_string()),
                }
            }
            return;
        }

        if self.model_popup.visible {
            let popup_action = self.model_popup.handle_key(key);
            self.handle_model_popup_action(popup_action);
            return;
        }

        if let Some(mut popup) = self.provider_popup.take() {
            let popup_action = popup.handle_key(key);
            self.handle_provider_popup_action(popup_action, popup);
            return;
        }

        let action = self.slash_commands_menu.handle_key(key);
        if self.handle_slash_command_menu_action(action).await {
            return;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) if self.busy => {
                if let Err(error) = self.commands_tx.try_send(AppCommand::Cancel) {
                    self.footer.set_status(error.to_string());
                }
            }
            (KeyCode::PageUp, _) => self.messages.scroll_up(),
            (KeyCode::PageDown, _) => self.messages.scroll_down(),
            (KeyCode::Enter, KeyModifiers::ALT | KeyModifiers::SHIFT) => {
                self.prompt.insert_newline();
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let content = self.prompt.content();

                if self.handle_slash_commands(content.trim()).await {
                    return;
                }

                if self.busy
                    || self.clear_pending
                    || self.resume_pending
                    || content.trim().is_empty()
                {
                    return;
                }

                match self
                    .commands_tx
                    .try_send(AppCommand::SubmitPrompt(content.to_string()))
                {
                    Ok(()) => {
                        self.messages.push(MessageComponent::User {
                            content: content.to_string(),
                        });
                        self.prompt.record_submission(content);
                        self.busy = true;
                        self.messages.scroll_to_bottom();
                        self.footer.set_busy_status();
                    }
                    Err(error) => self.footer.set_status(error.to_string()),
                }
            }
            _ => {
                self.prompt.input(Input::from(key));
            }
        }
        self.slash_commands_menu.update(&self.prompt.content());
    }

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::SessionsListed(sessions) => self.session_popup.set_sessions(sessions),
            AppEvent::SessionFailed(error) => {
                self.resume_pending = false;
                self.session_popup.set_error(error.clone());
                self.footer.set_status(error);
            }
            AppEvent::SessionResumed {
                context_tokens,
                cache_usage,
                messages,
                directory,
                model,
                context_limit,
            } => {
                self.messages.clear();
                for message in messages {
                    use crate::comms::SessionMessage;
                    match message {
                        SessionMessage::User(content) => {
                            self.messages.push(MessageComponent::User { content })
                        }
                        SessionMessage::Assistant {
                            content,
                            cache_hit_percent,
                        } => self.messages.push(MessageComponent::Assistant {
                            content,
                            cache_hit_percent,
                        }),
                        SessionMessage::Error(content) => {
                            self.messages.push(MessageComponent::Error { content })
                        }
                        SessionMessage::Tool {
                            id,
                            name,
                            arguments,
                            output,
                            success,
                        } => {
                            self.messages
                                .start_tool(id.clone(), name.clone(), arguments);
                            self.messages.finish_tool(id, name, output, success);
                        }
                    }
                }
                self.messages.set_working_directory(directory);
                self.messages.scroll_to_bottom();
                self.model_popup.current = model;
                self.footer.set_context_size(context_tokens.unwrap_or(0));
                self.footer.cache_usage = cache_usage;
                self.footer.set_context_limit(context_limit);
                self.footer
                    .set_status("Session resumed. Enter a prompt to continue.");
                self.busy = false;
                self.clear_pending = false;
                self.resume_pending = false;
                self.session_popup.visible = false;
                self.prompt = PromptComponent::new();
            }
            AppEvent::Notice(message) => {
                self.footer.set_status(message.clone());
                self.messages
                    .push(MessageComponent::Error { content: message });
                self.messages.scroll_to_bottom();
            }
            AppEvent::ConversationCleared => {
                self.clear_pending = false;
                self.messages.clear();
                self.busy = false;
                self.footer.set_context_size(0);
                self.footer.cache_usage = Default::default();
                self.footer.set_default_status();
            }
            AppEvent::ModelSelected {
                provider,
                model,
                context_limit,
            } => {
                self.footer.set_context_limit(context_limit);
                self.footer
                    .set_status(format!("Selected {provider}: {model}"));
                if self
                    .pending_model
                    .as_ref()
                    .is_some_and(|(id, name)| *id == provider && name == &model)
                {
                    self.model_popup.visible = false;
                    self.pending_model = None;
                }
                self.model_popup.current = Some((provider, model));
            }
            AppEvent::ModelSelectionFailed { error } => {
                self.pending_model = None;
                self.footer.set_status(error.clone());
                self.model_popup.set_error(error);
            }
            AppEvent::ProvidersUpdated { configured, models } => {
                self.configured_providers = configured.clone();
                self.model_popup
                    .set_configured_providers(configured.clone());
                self.model_popup.set_models(models);
                if let Some(popup) = &mut self.provider_popup {
                    popup.set_configured_providers(configured);
                }
            }
            AppEvent::ProviderConfigured { provider } => {
                if self.pending_provider == Some(provider) {
                    self.pending_provider = None;
                    if self
                        .provider_popup
                        .as_ref()
                        .is_some_and(|popup| popup.selected_provider() == provider)
                    {
                        self.provider_popup = None;
                    }
                }
                self.footer.set_status(format!("Configured {provider}"));
            }
            AppEvent::ProviderConfigurationFailed { provider, error } => {
                if self.pending_provider == Some(provider) {
                    self.pending_provider = None;
                }
                self.footer.set_status(error.clone());
                if let Some(popup) = &mut self.provider_popup
                    && popup.selected_provider() == provider
                {
                    popup.set_message(error);
                }
            }
            AppEvent::DirectoryChanged(path) => {
                self.messages.set_working_directory(path);
            }
            AppEvent::Started => {
                self.busy = true;
                self.footer.set_busy_status();
            }
            AppEvent::Completed => {
                self.messages.fail_running_tools("Cancelled.");
                self.busy = false;
                self.footer.set_default_status();
            }
            AppEvent::Failed(error) => {
                self.busy = false;
                self.footer.set_status(error.clone());
                self.messages.fail_running_tools(&error);
                self.messages
                    .push(MessageComponent::Error { content: error });
            }
            AppEvent::ModelResponse {
                content,
                input_tokens,
                cache_hit_tokens,
                context_tokens,
            } => {
                self.footer.cache_usage.add(input_tokens, cache_hit_tokens);
                if let Some(tokens) = context_tokens {
                    self.footer.set_context_size(tokens);
                }
                if !content.is_empty() {
                    self.messages.push(MessageComponent::Assistant {
                        content,
                        cache_hit_percent: cache_hit_percent(input_tokens, cache_hit_tokens),
                    });
                }
            }
            AppEvent::ToolDelta(delta) => self.messages.update_tool(delta),
            AppEvent::ToolCall {
                id,
                name,
                arguments,
            } => self.messages.start_tool(id, name, arguments),
            AppEvent::ToolResult {
                id,
                name,
                output,
                success,
            } => self.messages.finish_tool(id, name, output, success),
        }
    }

    pub async fn run(&mut self) -> io::Result<()> {
        let mut terminal = ratatui::init();
        execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
        self.terminal_active = true;
        let mut terminal_events = EventStream::new();

        let mut animation_timer = interval(Duration::from_millis(100));
        animation_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_tick = Instant::now();

        while !self.should_quit {
            terminal
                .draw(|f| self.render(f, f.area()))
                .map_err(|error| io::Error::other(error.to_string()))?;

            tokio::select! {
                _ = animation_timer.tick() => {
                    let now = Instant::now();
                    self.update(now.duration_since(last_tick));
                    last_tick = now;
                },
                event = self.events_rx.recv() => match event {
                    Some(event) => self.handle_app_event(event),
                    None => break,
                },
                event = terminal_events.next() => match event {
                    Some(Ok(event)) => self.handle_terminal_event(event).await,
                    Some(Err(error)) => return Err(error),
                    None => break,
                },
            }
        }

        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        if self.terminal_active {
            let _ = execute!(
                std::io::stdout(),
                DisableBracketedPaste,
                DisableMouseCapture
            );
            ratatui::restore();
        }
    }
}

impl Component for Tui {
    fn update(&mut self, elapsed: Duration) {
        self.footer.update(elapsed);
        self.prompt.update(elapsed);
        self.messages.update(elapsed);
        if let Some(popup) = &mut self.provider_popup {
            popup.update(elapsed);
        }
        self.model_popup.update(elapsed);
        self.session_popup.update(elapsed);
    }

    fn render(&self, f: &mut Frame, _area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Fill(1),
                Constraint::Length(self.slash_commands_menu.height()),
                Constraint::Length(self.prompt.height(f.area().width, f.area().height)),
                Constraint::Length(1),
            ]);

        let areas = layout.split(f.area());

        self.footer.render(f, areas[3]);
        self.prompt.render(f, areas[2]);
        self.slash_commands_menu.render(f, areas[1]);
        self.messages.render(f, areas[0]);
        if let Some(popup) = &self.provider_popup {
            popup.render(f, f.area());
        }
        self.model_popup.render(f, f.area());
        self.session_popup.render(f, f.area());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use tokio::sync::mpsc;

    fn tui() -> Tui {
        let (commands, _) = mpsc::channel(10);
        let (_, events) = mpsc::channel(10);
        Tui::new(commands, events)
    }

    fn render(component: &impl Component) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| component.render(frame, frame.area()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(120)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn ticks_reach_components_without_a_busy_turn() {
        let mut tui = tui();
        tui.footer.set_busy_status();
        assert!(!tui.busy);
        tui.update(Duration::from_millis(100));
        assert!(render(&tui.footer).starts_with("⠙ Thinking…"));
        tui.footer.set_default_status();
        let before = render(&tui.footer);
        tui.update(Duration::from_millis(100));
        assert_eq!(render(&tui.footer), before);
    }

    #[test]
    fn tool_only_response_updates_usage_without_adding_a_message() {
        let mut tui = tui();
        let before = render(&tui.messages);
        tui.handle_app_event(AppEvent::ModelResponse {
            content: String::new(),
            input_tokens: Some(100),
            cache_hit_tokens: Some(50),
            context_tokens: Some(125),
        });
        assert_eq!(render(&tui.messages), before);
        assert_eq!(tui.footer.cache_usage.input_tokens, 100);
        assert_eq!(tui.footer.cache_usage.cache_hit_tokens, 50);
        assert!(render(&tui.footer).contains("Context: 125 tokens"));
    }

    #[test]
    fn each_response_uses_its_own_cache_metadata() {
        let mut tui = tui();
        for (content, cached) in [("First answer", Some(25)), ("Second answer", None)] {
            tui.handle_app_event(AppEvent::ModelResponse {
                content: content.into(),
                input_tokens: Some(100),
                cache_hit_tokens: cached,
                context_tokens: None,
            });
        }
        let rendered = render(&tui.messages);
        assert!(rendered.contains("First answer"));
        assert!(rendered.contains("Second answer"));
        assert!(rendered.contains("[Request cache hit: 25.0%]"));
        assert_eq!(rendered.matches("Request cache hit:").count(), 1);
        assert!(tui.footer.cache_usage.incomplete);
    }
}
