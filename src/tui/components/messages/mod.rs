use crate::tui::components::Component;
use crate::tui::theme;
use message::render_message;
pub use message::{MessageComponent, ToolStatus};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

mod message;

#[derive(Default)]
pub struct MessagesComponent {
    messages: Vec<MessageComponent>,
    scroll: u16,
    working_directory: Option<std::path::PathBuf>,
}

impl MessagesComponent {
    pub fn set_working_directory(&mut self, path: std::path::PathBuf) {
        self.working_directory = Some(path);
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll = 0;
    }

    pub fn push(&mut self, message: MessageComponent) {
        self.messages.push(message);
    }

    pub fn update_tool(&mut self, delta: tool::ToolDelta) {
        for message in self.messages.iter_mut().rev() {
            if let MessageComponent::ToolOutput {
                id, output, status, ..
            } = message
                && *id == delta.id
                && *status == ToolStatus::Executing
            {
                output.push_str(&delta.content);
                return;
            }
        }
    }

    pub fn start_tool(&mut self, id: String, name: String, arguments: String) {
        self.push(MessageComponent::ToolCall {
            name: name.clone(),
            arguments,
        });
        self.push(MessageComponent::ToolOutput {
            id,
            name,
            output: String::new(),
            status: ToolStatus::Executing,
        });
    }

    pub fn finish_tool(&mut self, id: String, name: String, output: String, success: bool) {
        let status = if success {
            ToolStatus::Success
        } else {
            ToolStatus::Failure
        };
        for message in self.messages.iter_mut().rev() {
            if let MessageComponent::ToolOutput {
                id: call_id,
                output: accumulated,
                status: current_status,
                ..
            } = message
                && *call_id == id
            {
                if !output.is_empty() {
                    if !accumulated.is_empty() && !accumulated.ends_with('\n') {
                        accumulated.push('\n');
                    }
                    accumulated.push_str(&output);
                }
                *current_status = status;
                return;
            }
        }
        self.push(MessageComponent::ToolOutput {
            id,
            name,
            output,
            status,
        });
    }

    pub fn fail_running_tools(&mut self, reason: &str) {
        for message in &mut self.messages {
            if let MessageComponent::ToolOutput { output, status, .. } = message
                && *status == ToolStatus::Executing
            {
                *status = ToolStatus::Failure;
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(reason);
            }
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_add(10);
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_sub(10);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll = 0;
    }
}

impl Component for MessagesComponent {
    fn render(&self, f: &mut Frame, area: Rect) {
        let [header, _, inner] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(area);
        f.render_widget(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme::RULE)),
            header,
        );
        let [brand, directory] = Layout::horizontal([
            Constraint::Length(if area.width >= 60 { 26 } else { 10 }),
            Constraint::Min(0),
        ])
        .areas(Rect::new(
            header.x,
            header.y.saturating_add(1),
            header.width,
            header.height.saturating_sub(1).min(1),
        ));
        let mut spans = vec![Span::styled(
            "SHEHAB",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )];
        if area.width >= 60 {
            spans.push(Span::styled(
                format!("  {}", env!("CARGO_PKG_VERSION")),
                Style::default().fg(theme::MUTED),
            ));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), brand);
        let path = self
            .working_directory
            .as_ref()
            .map(|path| {
                let home = std::env::var_os("HOME");
                home.and_then(|home| {
                    path.strip_prefix(home)
                        .ok()
                        .map(|relative| format!("~/{}", relative.display()))
                })
                .unwrap_or_else(|| path.display().to_string())
            })
            .unwrap_or_default();
        f.render_widget(
            Paragraph::new(path)
                .right_aligned()
                .style(Style::default().fg(theme::MUTED)),
            directory,
        );
        if inner.is_empty() {
            return;
        }
        if self.messages.is_empty() {
            f.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        "What are we building?",
                        Style::default().fg(theme::INK).add_modifier(Modifier::BOLD),
                    ),
                    Line::raw(""),
                    Line::styled(
                        "Describe a task, or type / to see commands.",
                        Style::default().fg(theme::MUTED),
                    ),
                ])
                .block(Block::default().padding(ratatui::widgets::Padding::new(2, 0, 1, 0))),
                inner,
            );
            return;
        }

        // Scroll the whole conversation, keeping the newest output visible by default.
        let total_height = self.messages.iter().fold(0usize, |height, message| {
            height.saturating_add(usize::from(message.height(inner.width)) + 1)
        });
        let max_scroll = total_height.saturating_sub(usize::from(inner.height));
        let offset = max_scroll.saturating_sub(usize::from(self.scroll));
        let mut position = 0usize;
        for message in &self.messages {
            let height = usize::from(message.height(inner.width));
            let start = position.max(offset);
            let end = position
                .saturating_add(height)
                .min(offset + usize::from(inner.height));
            if start < end {
                render_message(
                    f,
                    message,
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
}
