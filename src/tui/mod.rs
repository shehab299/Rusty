use std::io;

use crate::comms::{Command, Event};
use crossterm::event::{Event as TerminalEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::{StreamExt};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph, Widget},
};
use ratatui_textarea::{Input, TextArea};
use tokio::sync::mpsc::{Receiver, Sender};
use types::{AssistantMessage, Print, ToolCallMessage, ToolResultMessage, UserMessage};

mod types;

const LOGO: &str = r#"
 ███████╗██╗  ██╗███████╗██╗  ██╗ █████╗ ██████╗
 ██╔════╝██║  ██║██╔════╝██║  ██║██╔══██╗██╔══██╗
 ███████╗███████║█████╗  ███████║███████║██████╔╝
 ╚════██║██╔══██║██╔══╝  ██╔══██║██╔══██║██╔══██╗
 ███████║██║  ██║███████╗██║  ██║██║  ██║██████╔╝
 ╚══════╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝╚═╝╚═╝╚═════╝
"#;

struct App {
    messages: Vec<Box<dyn Print>>,
    prompt: TextArea<'static>,
    busy: bool,
    should_quit: bool,
    status: String,
    scroll: u16,
}

impl Default for App {
    fn default() -> Self {
        let mut prompt = TextArea::default();
        prompt.set_block(Block::default().borders(Borders::ALL));
        prompt.set_placeholder_text("Ask me anything..");
        prompt.set_cursor_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        prompt.set_cursor_line_style(Style::default());
        Self {
            messages: Vec::new(),
            prompt,
            busy: false,
            should_quit: false,
            status: String::new(),
            scroll: 0,
        }
    }
}

impl App {
    fn handle_event(&mut self, event: TerminalEvent, commands: &Sender<Command>) {
        
        let TerminalEvent::Key(key) = event else {
            return;
        };
        
        if key.kind == KeyEventKind::Release {
            return;
        }
        
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.should_quit = true,
            (KeyCode::Esc, _) if self.busy => {
                if let Err(error) = commands.try_send(Command::Cancel) {
                    self.status = error.to_string();
                }
            }
            (KeyCode::PageUp, _) => self.scroll = self.scroll.saturating_add(10),
            (KeyCode::PageDown, _) => self.scroll = self.scroll.saturating_sub(10),
            (KeyCode::Enter, KeyModifiers::ALT | KeyModifiers::SHIFT) => {
                self.prompt.insert_newline();
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let content = self.prompt.lines().join("\n");
                if content.trim().is_empty() || self.busy {
                    return;
                }
                match commands.try_send(Command::SubmitPrompt(content.clone())) {
                    Ok(()) => {
                        self.messages.push(Box::new(UserMessage { content }));
                        self.prompt.clear();
                        self.busy = true;
                        self.scroll = 0;
                        self.status.clear();
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
            _ => {
                self.prompt.input(Input::from(key));
            }
        }
    }

    fn handle_agent_event(&mut self, event: Event) {
        match event {
            Event::Started => self.busy = true,
            Event::Completed => self.busy = false,
            Event::Failed(error) => {
                self.busy = false;
                self.status = error.clone();
                self.messages.push(Box::new(ToolResultMessage {
                    name: "Request".into(),
                    output: error,
                    success: false,
                }));
            }
            Event::TextDelta(content) => self.messages.push(Box::new(AssistantMessage { content })),
            Event::ToolCall { name, arguments } => self
                .messages
                .push(Box::new(ToolCallMessage { name, arguments })),
            Event::ToolResult {
                name,
                output,
                success,
            } => self.messages.push(Box::new(ToolResultMessage {
                name,
                output,
                success,
            })),
        }
    }
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

/// Run the terminal UI, awaiting terminal input and agent events concurrently.
pub async fn run(commands: Sender<Command>, mut events: Receiver<Event>) -> io::Result<()> {
    let terminal = ratatui::try_init()?;
    let _restore = RestoreTerminal;
    run_terminal(terminal, &commands, &mut events).await
}

async fn run_terminal<B: Backend>(
    mut terminal: Terminal<B>,
    commands: &Sender<Command>,
    events: &mut Receiver<Event>
) -> io::Result<()> {

    let mut app = App::default();
    let mut terminal_events = EventStream::new();

    while !app.should_quit {
        
        terminal
            .draw(|f| render(f, &mut app))
            .map_err(|error| io::Error::other(error.to_string()))?;

        tokio::select! {
            event = events.recv() => match event {
                Some(event) => app.handle_agent_event(event),
                None => break,
            },
            event = terminal_events.next() => match event {
                Some(Ok(event)) => app.handle_event(event, commands),
                Some(Err(error)) => return Err(error),
                None => break,
            },
        }

    }

    Ok(())
}

fn render(f: &mut Frame, app: &mut App) {
    let prompt_length = app
        .prompt
        .lines()
        .len()
        .saturating_add(2)
        .clamp(3, usize::from(f.area().height.max(3) / 2 + 2));

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(prompt_length as u16),
            Constraint::Length(1),
        ]);

    let areas = layout.split(f.area());

    let messages_area = areas[0];
    let prompt_area = areas[1];
    let footer_area = areas[2];

    render_footer(f, footer_area, app);
    render_prompt(f, prompt_area, &mut app.prompt);
    render_messages_widget(f, messages_area, app);
}

fn render_prompt(f: &mut Frame, area: Rect, prompt: &mut TextArea) {
    f.render_widget(&*prompt, area);
}

fn render_messages_widget(f: &mut Frame, area: Rect, app: &mut App) {
    let mut text = Text::default();

    text.extend(Text::styled(LOGO, Style::default().fg(Color::Cyan)));

    text.extend(Text::styled(
        format!("\n  version: {}", env!("CARGO_PKG_VERSION")),
        Style::default().fg(Color::Yellow),
    ));

    text.extend(Text::styled(
        format!(
            "\n  cwd: {}\n\n",
            std::env::current_dir().unwrap_or_default().display()
        ),
        Style::default().fg(Color::Yellow),
    ));

    // HEADER
    let conversation = Block::bordered();
    let inner = conversation.inner(area);
    f.render_widget(conversation, area);
    if inner.is_empty() {
        return;
    }

    // Scroll the whole conversation, keeping the newest output visible by default.
    let header_height = text.height();
    let total_height = app.messages.iter().fold(header_height, |height, message| {
        height.saturating_add(usize::from(message.height(inner.width)) + 1)
    });
    let max_scroll = total_height.saturating_sub(usize::from(inner.height));
    let offset = max_scroll.saturating_sub(usize::from(app.scroll));
    f.render_widget(
        Paragraph::new(text).scroll((offset.min(u16::MAX as usize) as u16, 0)),
        Rect::new(
            inner.x,
            inner.y,
            inner.width,
            header_height
                .saturating_sub(offset)
                .min(inner.height as usize) as u16,
        ),
    );

    let mut position = header_height;
    for message in &app.messages {
        let height = usize::from(message.height(inner.width));
        let start = position.max(offset);
        let end = position
            .saturating_add(height)
            .min(offset + usize::from(inner.height));
        if start < end {
            render_message(
                f,
                message.as_ref(),
                Rect::new(
                    inner.x,
                    inner.y + (start - offset) as u16,
                    inner.width,
                    (end - start) as u16,
                ),
                (start - position) as u16,
            );
        }
        position = position.saturating_add(height + 1);
    }
}

fn render_message(f: &mut Frame, message: &dyn Print, visible: Rect, offset: u16) {
    let full_area = Rect::new(0, 0, visible.width, message.height(visible.width));
    let mut buffer = Buffer::empty(full_area);
    message.widget().render(full_area, &mut buffer);
    for y in 0..visible.height {
        for x in 0..visible.width {
            f.buffer_mut()[(visible.x + x, visible.y + y)] = buffer[(x, y + offset)].clone();
        }
    }
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let status = if !app.status.is_empty() {
        app.status.as_str()
    } else if app.busy {
        "Thinking… Esc: cancel | Ctrl+C: quit"
    } else {
        "Enter: send | Alt+Enter: newline | PgUp/PgDn: scroll | Ctrl+C: quit"
    };
    f.render_widget(
        Paragraph::new(Line::styled(status, Style::default().fg(Color::DarkGray))),
        area,
    );
}
