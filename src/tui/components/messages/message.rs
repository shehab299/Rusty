use crate::tui::components::Component;
use crate::tui::theme;
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Padding, Paragraph, Widget, Wrap},
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
        cache_hit_percent: Option<f64>,
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
            Self::User { .. } | Self::Assistant { .. } | Self::Error { .. } => 4,
            Self::ToolCall { .. } | Self::ToolOutput { .. } => 4,
        };
        let content_width = width.saturating_sub(horizontal_space);
        self.widget()
            .line_count(content_width.max(1))
            .clamp(1, usize::from(u16::MAX)) as u16
    }

    pub fn widget(&self) -> Paragraph<'_> {
        match self {
            Self::User { content } => {
                let mut text = Text::from(vec![
                    Line::styled(
                        "You",
                        Style::default()
                            .fg(theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Line::raw(""),
                ]);
                text.extend(Text::from(content.replace("\\n", "\n")));
                Paragraph::new(text)
                    .style(theme::base())
                    .block(Block::default().padding(Padding::horizontal(2)))
            }
            Self::Assistant {
                content,
                cache_hit_percent,
            } => {
                let mut text = Text::from(vec![
                    Line::styled(
                        "Shehab",
                        Style::default()
                            .fg(theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Line::raw(""),
                ]);
                text.extend(Text::from(content.replace("\\n", "\n")));
                if let Some(percent) = cache_hit_percent {
                    text.push_line(Line::styled(
                        format!("[Request cache hit: {percent:.1}%]"),
                        Style::default().fg(theme::MUTED),
                    ));
                }
                Paragraph::new(text)
                    .style(theme::base())
                    .block(Block::default().padding(Padding::horizontal(2)))
            }
            Self::Error { content } => Paragraph::new(content.replace("\\n", "\n"))
                .style(theme::base().fg(theme::ERROR))
                .block(Block::default().padding(Padding::horizontal(2))),
            Self::ToolCall { name, arguments } => Paragraph::new(arguments.replace("\\n", "\n"))
                .block(
                    Block::default()
                        .borders(Borders::LEFT | Borders::TOP)
                        .style(theme::base().fg(theme::MUTED))
                        .border_style(Style::default().fg(theme::RULE))
                        .title(format!(" {name} "))
                        .title_style(
                            Style::default()
                                .fg(theme::MUTED)
                                .add_modifier(Modifier::BOLD),
                        )
                        .padding(Padding::new(2, 1, 0, 0)),
                ),
            Self::ToolOutput {
                name,
                output,
                status,
                ..
            } => {
                let (symbol, label, color) = match status {
                    ToolStatus::Executing => ("…", "executing", theme::ACCENT),
                    ToolStatus::Success => ("✓", "completed", theme::SUCCESS),
                    ToolStatus::Failure => ("✗", "failed", theme::ERROR),
                };
                let (output, output_style) = if output.is_empty() {
                    (
                        if *status == ToolStatus::Executing {
                            "Waiting for output…"
                        } else {
                            "No output."
                        },
                        theme::base().fg(theme::MUTED),
                    )
                } else {
                    (output.as_str(), theme::base().fg(theme::MUTED))
                };
                Paragraph::new(output.replace("\\n", "\n"))
                    .style(output_style)
                    .block(
                        Block::default()
                            .borders(Borders::LEFT | Borders::TOP)
                            .border_style(Style::default().fg(if *status == ToolStatus::Failure {
                                theme::ERROR
                            } else {
                                theme::RULE
                            }))
                            .title(Line::styled(
                                format!(" {symbol} {name} {label} "),
                                Style::default().fg(color).add_modifier(Modifier::BOLD),
                            ))
                            .padding(Padding::new(2, 1, 0, 0)),
                    )
            }
        }
        .wrap(Wrap { trim: false })
    }
}

impl Component for MessageComponent {
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
    buffer.set_style(full_area, theme::base());
    message.widget().render(full_area, &mut buffer);
    for y in 0..visible.height {
        for x in 0..visible.width {
            f.buffer_mut()[(visible.x + x, visible.y + y)] = buffer[(x, y + offset)].clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_metadata_is_compact_and_muted() {
        let message = MessageComponent::Assistant {
            content: "Answer".into(),
            cache_hit_percent: Some(50.0),
        };
        assert_eq!(message.height(80), 4);
        let area = Rect::new(0, 0, 80, 4);
        let mut buffer = Buffer::empty(area);
        message.widget().render(area, &mut buffer);
        assert_eq!(buffer[(2, 0)].symbol(), "S");
        assert_eq!(buffer[(2, 0)].fg, theme::ACCENT);
        assert_eq!(buffer[(2, 2)].symbol(), "A");
        assert_eq!(buffer[(2, 2)].fg, theme::INK);
        assert_eq!(buffer[(2, 3)].symbol(), "[");
        assert_eq!(buffer[(2, 3)].fg, theme::MUTED);
    }

    #[test]
    fn clipped_messages_keep_the_paper_background() {
        let message = MessageComponent::Assistant {
            content: "First line\nSecond line\nThird line".into(),
            cache_hit_percent: None,
        };
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 2)).unwrap();
        terminal
            .draw(|frame| render_message(frame, &message, frame.area(), 2))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(2, 0)].symbol(), "F");
        assert!(buffer.content().iter().all(|cell| cell.bg == theme::PAPER));
    }
}
