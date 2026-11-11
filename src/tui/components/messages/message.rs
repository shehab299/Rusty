use crate::tui::components::Render;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget, Wrap},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Executing,
    Failure,
    Success,
}

pub enum MessageComponent {
    User {
        content: String,
    },
    Assistant {
        content: String,
    },
    Error {
        content: String,
    },
    ToolCall {
        name: String,
        arguments: String,
    },
    ToolOutput {
        id: String,
        name: String,
        output: String,
        status: ToolStatus,
    },
}

impl MessageComponent {
    pub fn height(&self, width: u16) -> u16 {
        // Paragraph expects the content width, excluding horizontal borders/padding.
        let horizontal_space = match self {
            Self::User { .. } | Self::Assistant { .. } | Self::Error { .. } => 2,
            Self::ToolCall { .. } | Self::ToolOutput { .. } => 4,
        };
        let content_width = width.saturating_sub(horizontal_space);
        self.widget()
            .line_count(content_width.max(1))
            .clamp(1, usize::from(u16::MAX)) as u16
    }

    pub fn widget(&self) -> Paragraph<'_> {
        match self {
            Self::User { content } => Paragraph::new(content.replace("\\n", "\n"))
                .style(Style::default().add_modifier(Modifier::BOLD))
                .block(
                    Block::default()
                        .borders(Borders::LEFT)
                        .border_style(Style::default().fg(Color::Cyan))
                        .padding(Padding::new(1, 0, 0, 0)),
                ),
            Self::Assistant { content } => Paragraph::new(content.replace("\\n", "\n"))
                .block(Block::default().padding(Padding::new(2, 0, 0, 0))),
            Self::Error { content } => Paragraph::new(content.replace("\\n", "\n"))
                .style(Style::default().fg(Color::Red))
                .block(Block::default().padding(Padding::new(2, 0, 0, 0))),
            Self::ToolCall { name, arguments } => Paragraph::new(arguments.replace("\\n", "\n"))
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .title(format!(" {name} "))
                        .title_style(
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        )
                        .padding(Padding::horizontal(1)),
                ),
            Self::ToolOutput {
                name,
                output,
                status,
                ..
            } => {
                let (symbol, label, color) = match status {
                    ToolStatus::Executing => ("…", "executing", Color::Yellow),
                    ToolStatus::Success => ("✓", "completed", Color::Green),
                    ToolStatus::Failure => ("✗", "failed", Color::Red),
                };
                let (output, output_style) = if output.is_empty() {
                    (
                        if *status == ToolStatus::Executing {
                            "Waiting for output…"
                        } else {
                            "No output."
                        },
                        Style::default().fg(Color::DarkGray),
                    )
                } else {
                    (output.as_str(), Style::default())
                };
                Paragraph::new(output.replace("\\n", "\n"))
                    .style(output_style)
                    .block(
                        Block::bordered()
                            .border_type(BorderType::Rounded)
                            .border_style(Style::default().fg(if *status == ToolStatus::Failure {
                                Color::Red
                            } else {
                                Color::DarkGray
                            }))
                            .title(Line::styled(
                                format!(" {symbol} {name} {label} "),
                                Style::default().fg(color).add_modifier(Modifier::BOLD),
                            ))
                            .padding(Padding::horizontal(1)),
                    )
            }
        }
        .wrap(Wrap { trim: false })
    }
}

impl Render for MessageComponent {
    fn render(&self, f: &mut Frame, area: Rect) {
        f.render_widget(self.widget(), area);
    }
}

pub fn render_message(f: &mut Frame, message: &MessageComponent, visible: Rect, offset: u16) {
    if offset == 0 && visible.height == message.height(visible.width) {
        message.render(f, visible);
        return;
    }

    let full_area = Rect::new(0, 0, visible.width, message.height(visible.width));
    let mut buffer = Buffer::empty(full_area);
    message.widget().render(full_area, &mut buffer);
    for y in 0..visible.height {
        for x in 0..visible.width {
            f.buffer_mut()[(visible.x + x, visible.y + y)] = buffer[(x, y + offset)].clone();
        }
    }
}
