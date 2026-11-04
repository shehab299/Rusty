use crate::tui::components::Render;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

pub struct FooterComponent {
    status: String,
}

impl FooterComponent {
    pub fn new() -> Self {
        let mut footer = Self {
            status: String::new(),
        };
        footer.set_default_status();
        footer
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    pub fn set_busy_status(&mut self) {
        self.status = "Thinking… Esc: cancel | Ctrl+C: quit".to_string()
    }

    pub fn set_default_status(&mut self) {
        self.status =
            "Enter: send | Alt+Enter: newline | PgUp/PgDn: scroll | Ctrl+C: quit".to_string()
    }
}

impl Render for FooterComponent {
    fn render(&self, f: &mut Frame, area: Rect) {
        f.render_widget(
            Paragraph::new(Line::styled(
                &self.status,
                Style::default().fg(Color::DarkGray),
            )),
            area,
        );
    }
}
