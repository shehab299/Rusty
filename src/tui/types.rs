use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};

pub trait Print {
    fn widget(&self) -> Paragraph<'_>;

    fn horizontal_space(&self) -> u16 {
        0
    }

    fn height(&self, width: u16) -> u16 {
        // Paragraph expects the content width, excluding horizontal borders/padding.
        let content_width = width.saturating_sub(self.horizontal_space());
        self.widget()
            .line_count(content_width.max(1))
            .clamp(1, usize::from(u16::MAX)) as u16
    }
}

pub struct UserMessage {
    pub content: String,
}

impl Print for UserMessage {
    fn widget(&self) -> Paragraph<'_> {
        Paragraph::new(self.content.as_str())
            .style(Style::default().add_modifier(Modifier::BOLD))
            .block(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(Style::default().fg(Color::Cyan))
                    .padding(Padding::new(1, 0, 0, 0)),
            )
            .wrap(Wrap { trim: false })
    }

    fn horizontal_space(&self) -> u16 {
        2
    }
}

pub struct AssistantMessage {
    pub content: String,
}

impl Print for AssistantMessage {
    fn widget(&self) -> Paragraph<'_> {
        Paragraph::new(self.content.as_str())
            .block(Block::default().padding(Padding::new(2, 0, 0, 0)))
            .wrap(Wrap { trim: false })
    }

    fn horizontal_space(&self) -> u16 {
        2
    }
}

pub struct ToolCallMessage {
    pub name: String,
    pub arguments: String,
}

impl Print for ToolCallMessage {
    fn widget(&self) -> Paragraph<'_> {
        Paragraph::new(self.arguments.as_str())
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(format!(" {} ", self.name))
                    .title_style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                    .padding(Padding::horizontal(1)),
            )
            .wrap(Wrap { trim: false })
    }

    fn horizontal_space(&self) -> u16 {
        4
    }
}

pub struct ToolResultMessage {
    pub name: String,
    pub output: String,
    pub success: bool,
}

impl Print for ToolResultMessage {
    fn widget(&self) -> Paragraph<'_> {
        let (symbol, status, color) = if self.success {
            ("✓", "completed", Color::Green)
        } else {
            ("✗", "failed", Color::Red)
        };
        let (output, output_style) = if self.output.is_empty() {
            ("No output.", Style::default().fg(Color::DarkGray))
        } else {
            (self.output.as_str(), Style::default())
        };
        Paragraph::new(output)
            .style(output_style)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(if self.success {
                        Color::DarkGray
                    } else {
                        Color::Red
                    }))
                    .title(Line::styled(
                        format!(" {symbol} {} {status} ", self.name),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ))
                    .padding(Padding::horizontal(1)),
            )
            .wrap(Wrap { trim: false })
    }

    fn horizontal_space(&self) -> u16 {
        4
    }
}
