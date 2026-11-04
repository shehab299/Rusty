use crate::tui::components::Render;
pub use message::MessageComponent;
use message::render_message;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Text,
    widgets::{Block, Paragraph},
};

mod message;

const LOGO: &str = include_str!("../../../../public/logo");

#[derive(Default)]
pub struct MessagesComponent {
    messages: Vec<MessageComponent>,
    scroll: u16,
}

impl MessagesComponent {
    pub fn push(&mut self, message: MessageComponent) {
        self.messages.push(message);
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

impl Render for MessagesComponent {
    fn render(&self, f: &mut Frame, area: Rect) {
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

        let conversation = Block::bordered();
        let inner = conversation.inner(area);
        f.render_widget(conversation, area);
        if inner.is_empty() {
            return;
        }

        // Scroll the whole conversation, keeping the newest output visible by default.
        let header_height = text.height();
        let total_height = self.messages.iter().fold(header_height, |height, message| {
            height.saturating_add(usize::from(message.height(inner.width)) + 1)
        });
        let max_scroll = total_height.saturating_sub(usize::from(inner.height));
        let offset = max_scroll.saturating_sub(usize::from(self.scroll));
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
