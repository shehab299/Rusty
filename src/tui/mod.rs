use std::io;

use crate::comms::{Command, Event};
use components::{FooterComponent, MessageComponent, MessagesComponent, PromptComponent, Render};
use crossterm::event::{Event as TerminalEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
};
use ratatui_textarea::Input;
use tokio::sync::mpsc::{Receiver, Sender};

mod components;

pub struct Tui {
    terminal: DefaultTerminal,
    commands_tx: Sender<Command>,
    events_rx: Receiver<Event>,
    messages: MessagesComponent,
    prompt: PromptComponent,
    footer: FooterComponent,
    busy: bool,
    should_quit: bool,
}

impl Tui {
    pub fn new(commands: Sender<Command>, events: Receiver<Event>) -> Self {
        Self {
            terminal: ratatui::init(),
            commands_tx: commands,
            events_rx: events,
            messages: MessagesComponent::default(),
            prompt: PromptComponent::new(),
            footer: FooterComponent::new(),
            busy: false,
            should_quit: false,
        }
    }

    pub fn handle_terminal_event(&mut self, event: TerminalEvent) {
        let TerminalEvent::Key(key) = event else {
            return;
        };

        if key.kind == KeyEventKind::Release {
            return;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.should_quit = true,
            (KeyCode::Esc, _) if self.busy => {
                if let Err(error) = self.commands_tx.try_send(Command::Cancel) {
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
                if content.trim().is_empty() || self.busy {
                    return;
                }
                match self
                    .commands_tx
                    .try_send(Command::SubmitPrompt(content.clone()))
                {
                    Ok(()) => {
                        self.messages.push(MessageComponent::User { content });
                        self.prompt.clear();
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

    pub fn handle_agent_event(&mut self, event: Event) {
        match event {
            Event::Started => {
                self.busy = true;
                self.footer.set_busy_status();
            }
            Event::Completed => {
                self.busy = false;
                self.footer.set_default_status();
            }
            Event::Failed(error) => {
                self.busy = false;
                self.footer.set_status(error.clone());
                self.messages.push(MessageComponent::ToolResult {
                    name: "Request".into(),
                    output: error,
                    success: false,
                });
            }
            Event::TextDelta(content) => {
                self.messages.push(MessageComponent::Assistant { content })
            }
            Event::ToolCall { name, arguments } => self
                .messages
                .push(MessageComponent::ToolCall { name, arguments }),
            Event::ToolResult {
                name,
                output,
                success,
            } => self.messages.push(MessageComponent::ToolResult {
                name,
                output,
                success,
            }),
        }
    }

    pub async fn run(&mut self) -> io::Result<()> {
        let mut terminal_events = EventStream::new();

        while !self.should_quit {
            self.terminal
                .draw(|f| render(f, &self.messages, &self.prompt, &self.footer))
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
        ratatui::restore();
    }
}

fn render(
    f: &mut Frame,
    messages: &MessagesComponent,
    prompt: &PromptComponent,
    footer: &FooterComponent,
) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(prompt.height(f.area().height)),
            Constraint::Length(1),
        ]);

    let areas = layout.split(f.area());

    footer.render(f, areas[2]);
    prompt.render(f, areas[1]);
    messages.render(f, areas[0]);
}
