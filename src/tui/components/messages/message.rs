use crate::tui::components::Render;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget, Wrap},
};

pub enum MessageComponent {
    User {
        content: String,
    },
    Assistant {
        content: String,
    },
    ToolCall {
        name: String,
        arguments: String,
    },
    ToolResult {
        name: String,
        output: String,
        success: bool,
    },
}

impl MessageComponent {
    pub fn height(&self, width: u16) -> u16 {
        // Paragraph expects the content width, excluding horizontal borders/padding.
        let horizontal_space = match self {
            Self::User { .. } | Self::Assistant { .. } => 2,
            Self::ToolCall { .. } | Self::ToolResult { .. } => 4,
        };
        let content_width = width.saturating_sub(horizontal_space);
        self.widget()
            .line_count(content_width.max(1))
            .clamp(1, usize::from(u16::MAX)) as u16
    }

    pub fn widget(&self) -> Paragraph<'_> {
        match self {
            Self::User { content } => Paragraph::new(content.as_str())
                .style(Style::default().add_modifier(Modifier::BOLD))
                .block(
                    Block::default()
                        .borders(Borders::LEFT)
                        .border_style(Style::default().fg(Color::Cyan))
                        .padding(Padding::new(1, 0, 0, 0)),
                ),
            Self::Assistant { content } => Paragraph::new(content.as_str())
                .block(Block::default().padding(Padding::new(2, 0, 0, 0))),
            Self::ToolCall { name, arguments } => Paragraph::new(arguments.as_str()).block(
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
            Self::ToolResult {
                name,
                output,
                success,
            } => {
                let (symbol, status, color) = if *success {
                    ("✓", "completed", Color::Green)
                } else {
                    ("✗", "failed", Color::Red)
                };
                let (output, output_style) = if output.is_empty() {
                    ("No output.", Style::default().fg(Color::DarkGray))
                } else {
                    (output.as_str(), Style::default())
                };
                Paragraph::new(output).style(output_style).block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(if *success {
                            Color::DarkGray
                        } else {
                            Color::Red
                        }))
                        .title(Line::styled(
                            format!(" {symbol} {name} {status} "),
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
