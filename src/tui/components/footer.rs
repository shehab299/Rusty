use crate::tui::components::Render;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

pub struct FooterComponent {
    status: String,
    context_size: Option<u64>,
    context_limit: Option<u64>,
}

impl FooterComponent {
    pub fn new() -> Self {
        let mut footer = Self {
            status: String::new(),
            context_size: None,
            context_limit: None,
        };
        footer.set_default_status();
        footer
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    pub fn set_context_size(&mut self, tokens: u64) {
        self.context_size = Some(tokens);
    }

    pub fn set_context_limit(&mut self, context_limit: Option<u64>) {
        self.context_limit = context_limit.filter(|limit| *limit > 0);
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
        let context = match (self.context_size, self.context_limit) {
            (Some(tokens), Some(limit)) => {
                let percent = tokens as f64 / limit as f64 * 100.0;
                format!("Context: {tokens}/{limit} tokens ({percent:.1}%)")
            }
            (Some(tokens), None) => format!("Context: {tokens} tokens"),
            (None, Some(limit)) => format!("Context: —/{limit} tokens"),
            (None, None) => "Context: —".to_string(),
        };
        let areas = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(context.chars().count() as u16 + 1),
        ])
        .split(area);
        f.render_widget(
            Paragraph::new(Line::styled(
                &self.status,
                Style::default().fg(Color::DarkGray),
            )),
            areas[0],
        );
        f.render_widget(
            Paragraph::new(Line::styled(context, Style::default().fg(Color::DarkGray)))
                .right_aligned(),
            areas[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn render_footer(footer: &FooterComponent, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|frame| footer.render(frame, frame.area()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn context_is_unknown_until_updated() {
        let footer = FooterComponent::new();
        assert!(render_footer(&footer, 120).contains("Context: —"));
    }

    #[test]
    fn context_is_replaced_and_preserved_across_status_changes() {
        let mut footer = FooterComponent::new();
        footer.set_context_size(1234);
        footer.set_busy_status();
        let rendered = render_footer(&footer, 120);
        assert!(rendered.contains("Thinking…"));
        assert!(rendered.contains("Context: 1234 tokens"));

        footer.set_context_size(42);
        footer.set_default_status();
        assert!(render_footer(&footer, 120).contains("Context: 42 tokens"));

        footer.set_status("An error occurred");
        assert!(render_footer(&footer, 120).contains("Context: 42 tokens"));
    }

    #[test]
    fn context_limit_is_visible_before_usage_arrives() {
        let mut footer = FooterComponent::new();
        footer.set_context_limit(Some(128000));
        assert!(render_footer(&footer, 120).contains("Context: —/128000 tokens"));
    }

    #[test]
    fn context_displays_usage_limit_and_percentage() {
        let mut footer = FooterComponent::new();
        footer.set_context_limit(Some(128000));
        footer.set_context_size(12500);
        assert!(render_footer(&footer, 120).contains("Context: 12500/128000 tokens (9.8%)"));

        footer.set_context_limit(Some(10000));
        assert!(render_footer(&footer, 120).contains("(125.0%)"));

        footer.set_context_limit(None);
        assert!(render_footer(&footer, 120).contains("Context: 12500 tokens"));
        footer.set_context_limit(Some(0));
        assert!(render_footer(&footer, 120).contains("Context: 12500 tokens"));
    }

    #[test]
    fn context_renders_on_narrow_terminals() {
        let mut footer = FooterComponent::new();
        footer.set_context_size(0);
        assert!(render_footer(&footer, 20).contains("Context: 0 tokens"));
        render_footer(&footer, 5);
    }
}
