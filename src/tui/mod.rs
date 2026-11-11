use std::io;

use crate::error::AppError;
use crate::model_catalog::Catalog;
use crate::{
    comms::{AgentCommand, AgentEvent},
    config::provider::{
        AnthropicConfig, OpenAIConfig, OpenRouterConfig, ProviderConfig, ProviderId,
        ProvidersConfig,
    },
    provider_manager::ProviderManager,
};
use components::{
    FooterComponent, MessageComponent, MessagesComponent, ModelPopup, ModelPopupAction,
    PromptComponent, ProviderPopup, ProviderPopupAction, Render,
};
use crossterm::event::{Event as TerminalEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};
use ratatui_textarea::Input;
use tokio::sync::mpsc::{Receiver, Sender};

mod components;

pub struct Tui {
    commands_tx: Sender<AgentCommand>,
    events_rx: Receiver<AgentEvent>,

    messages: MessagesComponent,
    prompt: PromptComponent,
    footer: FooterComponent,
    provider_popup: Option<ProviderPopup>,
    model_popup: ModelPopup,

    busy: bool,
    should_quit: bool,
    terminal_active: bool,
    provider_manager: ProviderManager,
}

impl Tui {
    pub fn new(
        commands: Sender<AgentCommand>,
        events: Receiver<AgentEvent>,
        config: ProvidersConfig,
        catalog: Catalog,
    ) -> Result<Self, AppError> {
        let provider_manager = ProviderManager::from_config(config)?;

        let mut model_popup = ModelPopup::new(catalog);
        model_popup.set_configured_providers(provider_manager.configured());

        Ok(Self {
            provider_manager,
            commands_tx: commands,
            events_rx: events,
            messages: MessagesComponent::default(),
            prompt: PromptComponent::new(),
            footer: FooterComponent::new(),
            busy: false,
            should_quit: false,
            terminal_active: false,
            provider_popup: None,
            model_popup,
        })
    }

    // Helpers

    fn configure_provider(&mut self, config: ProviderConfig) -> Result<ProviderId, AppError> {
        let id = self.provider_manager.configure(config)?;
        self.model_popup
            .set_configured_providers(self.provider_manager.configured());
        Ok(id)
    }

    fn set_model(&self, provider: ProviderId, model: String) -> Result<(), AppError> {
        let selection = self.provider_manager.select(provider, model)?;
        self.commands_tx
            .try_send(AgentCommand::SetModel(selection))?;
        Ok(())
    }

    fn handle_model_popup_action(&mut self, action: ModelPopupAction) {
        match action {
            ModelPopupAction::None => {}
            ModelPopupAction::Connect => {
                self.provider_popup = Some(ProviderPopup::new(self.provider_manager.configured()));
            }
            ModelPopupAction::Select { provider, model } => match self.set_model(provider, model) {
                Ok(()) => self.model_popup.visible = false,
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
                let config = match provider {
                    ProviderId::OpenAI => ProviderConfig::OpenAI(OpenAIConfig { api_key }),
                    ProviderId::OpenRouter => {
                        ProviderConfig::OpenRouter(OpenRouterConfig { api_key })
                    }
                    ProviderId::Anthropic => ProviderConfig::Anthropic(AnthropicConfig { api_key }),
                };
                match self.configure_provider(config) {
                    Ok(id) => {
                        self.footer.set_status(format!("Configured {id}"));
                    }
                    Err(error) => {
                        popup.set_message(error.to_string());
                        self.provider_popup = Some(popup);
                    }
                }
            }
        }
    }

    fn handle_slash_commands(&mut self, command: impl Into<String>) -> bool {
        let command = command.into();
        if !command.starts_with('/') {
            return false;
        }

        match command.as_str() {
            "/connect" => {
                self.provider_popup = Some(ProviderPopup::new(self.provider_manager.configured()));
                self.prompt.clear();
                true
            }
            "/models" => {
                self.model_popup.open();
                self.prompt.clear();
                true
            }
            _ => false,
        }
    }

    // Event Handling

    fn handle_terminal_event(&mut self, event: TerminalEvent) {
        let TerminalEvent::Key(key) = event else {
            return;
        };

        if key.kind == KeyEventKind::Release {
            return;
        }

        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            self.should_quit = true;
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

        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) if self.busy => {
                if let Err(error) = self.commands_tx.try_send(AgentCommand::Cancel) {
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

                if self.handle_slash_commands(content.trim()) {
                    return;
                }

                if self.busy || content.trim().is_empty() {
                    return;
                }

                match self
                    .commands_tx
                    .try_send(AgentCommand::SubmitPrompt(content.to_string()))
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
    }

    fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::ModelSelected { provider, model } => {
                self.footer
                    .set_status(format!("Selected {provider}: {model}"));
                self.model_popup.current = Some((provider, model));
            }
            AgentEvent::Started => {
                self.busy = true;
                self.footer.set_busy_status();
            }
            AgentEvent::Completed => {
                self.messages.fail_running_tools("Cancelled.");
                self.busy = false;
                self.footer.set_default_status();
            }
            AgentEvent::Failed(error) => {
                self.busy = false;
                self.footer.set_status(error.clone());
                self.messages.fail_running_tools(&error);
                self.messages
                    .push(MessageComponent::Error { content: error });
            }
            AgentEvent::TextDelta(content) => {
                self.messages.push(MessageComponent::Assistant { content })
            }
            AgentEvent::ToolDelta(delta) => self.messages.update_tool(delta),
            AgentEvent::ToolCall {
                id,
                name,
                arguments,
            } => self.messages.start_tool(id, name, arguments),
            AgentEvent::ToolResult {
                id,
                name,
                output,
                success,
            } => self.messages.finish_tool(id, name, output, success),
        }
    }

    pub async fn run(&mut self) -> io::Result<()> {
        let mut terminal = ratatui::init();
        self.terminal_active = true;
        let mut terminal_events = EventStream::new();

        while !self.should_quit {
            terminal
                .draw(|f| self.render(f, f.area()))
                .map_err(|error| io::Error::other(error.to_string()))?;

            tokio::select! {
                event = self.events_rx.recv() => match event {
                    Some(event) => self.handle_agent_event(event),
                    None => break,
                },
                event = terminal_events.next() => match event {
                    Some(Ok(event)) => self.handle_terminal_event(event),
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
            ratatui::restore();
        }
    }
}

impl Render for Tui {
    fn render(&self, f: &mut Frame, _area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Fill(1),
                Constraint::Length(self.prompt.height(f.area().height)),
                Constraint::Length(1),
            ]);

        let areas = layout.split(f.area());

        self.footer.render(f, areas[2]);
        self.prompt.render(f, areas[1]);
        self.messages.render(f, areas[0]);
        if let Some(popup) = &self.provider_popup {
            popup.render(f, f.area());
        }
        self.model_popup.render(f, f.area());
    }
}
