use crate::session::{CacheUsage, cache_hit_percent};
use crate::tui::components::{Component, Spinner};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use std::time::Duration;

pub struct FooterComponent {
    status: String,
    spinner: Option<Spinner>,
    context_size: Option<u64>,
    context_limit: Option<u64>,
    pub(in crate::tui) cache_usage: CacheUsage,
}

impl FooterComponent {
    pub fn new() -> Self {
        let mut footer = Self {
            status: String::new(),
            spinner: None,
            context_size: None,
            context_limit: None,
            cache_usage: CacheUsage::default(),
        };
        footer.set_default_status();
        footer
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
        self.spinner = None;
    }

    pub fn set_context_size(&mut self, tokens: u64) {
        self.context_size = Some(tokens);
    }

    pub fn set_context_limit(&mut self, context_limit: Option<u64>) {
        self.context_limit = context_limit.filter(|limit| *limit > 0);
    }

    pub fn set_busy_status(&mut self) {
        self.status = "Thinking… Esc: cancel | Ctrl+C: quit".to_string();
        self.spinner.get_or_insert_with(Spinner::default);
    }

    pub fn set_default_status(&mut self) {
        self.set_status("Enter: send | Alt+Enter: newline | PgUp/PgDn: scroll | Ctrl+C: quit");
    }
    fn context_labels(&self) -> (String, String) {
        let compact = match (self.context_size, self.context_limit) {
            (Some(tokens), Some(limit)) => format!("Context: {tokens}/{limit}"),
            (Some(tokens), None) => format!("Context: {tokens}"),
            (None, Some(limit)) => format!("Context: —/{limit}"),
            (None, None) => "Context: —".to_string(),
        };
        let full = match (self.context_size, self.context_limit) {
            (Some(tokens), Some(limit)) => {
                let percent = tokens as f64 / limit as f64 * 100.0;
                format!("{compact} tokens ({percent:.1}%)")
            }
            (None, None) => compact.clone(),
            _ => format!("{compact} tokens"),
        };
        (full, compact)
    }

    fn cache_label(&self) -> String {
        if let Some(percent) = cache_hit_percent(
            Some(self.cache_usage.input_tokens),
            Some(self.cache_usage.cache_hit_tokens),
        ) {
            format!(
                "Conversation cache hit: {percent:.1}%{}",
                if self.cache_usage.incomplete {
                    " (partial telemetry)"
                } else {
                    ""
                }
            )
        } else if self.cache_usage.incomplete {
            "Conversation cache hit: unavailable".to_string()
        } else {
            String::new()
        }
    }

    fn metrics_label(&self, width: u16) -> String {
        // Keep status and keyboard hints readable before allocating secondary metrics.
        let status_width = self.status.chars().count() + if self.spinner.is_some() { 2 } else { 0 };
        let available = usize::from(width).saturating_sub(status_width + 1);
        let (context, compact_context) = self.context_labels();
        let cache = self.cache_label();
        let with_cache = |label: &str| {
            if cache.is_empty() {
                label.to_string()
            } else {
                format!("{label} | {cache}")
            }
        };
        [with_cache(&context), with_cache(&compact_context), context]
            .into_iter()
            .find(|label| label.chars().count() <= available)
            .unwrap_or_default()
    }
}

impl Component for FooterComponent {
    fn update(&mut self, elapsed: Duration) {
        if let Some(spinner) = &mut self.spinner {
            spinner.update(elapsed);
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let metrics = self.metrics_label(area.width);
        let areas = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(if metrics.is_empty() {
                0
            } else {
                metrics.chars().count() as u16 + 1
            }),
        ])
        .split(area);

        let mut status_area = areas[0];

        if let Some(spinner) = &self.spinner {
            let [spinner_area, text_area] =
                Layout::horizontal([Constraint::Length(2), Constraint::Min(0)]).areas(status_area);
            f.buffer_mut()
                .set_style(spinner_area, Style::default().fg(Color::DarkGray));
            spinner.render(f, spinner_area);
            status_area = text_area;
        }

        f.render_widget(
            Paragraph::new(Line::styled(
                &self.status,
                Style::default().fg(Color::DarkGray),
            )),
            status_area,
        );
        f.render_widget(
            Paragraph::new(Line::styled(metrics, Style::default().fg(Color::DarkGray)))
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
    fn busy_spinner_advances_and_wraps() {
        let mut footer = FooterComponent::new();
        footer.set_busy_status();
        let first = render_footer(&footer, 120);
        assert!(first.starts_with("⠋ Thinking…"));

        footer.update(Duration::from_millis(100));
        assert!(render_footer(&footer, 120).starts_with("⠙ Thinking…"));
        // Receiving another busy event must not restart the animation.
        footer.set_busy_status();
        assert!(render_footer(&footer, 120).starts_with("⠙ Thinking…"));
        footer.update(Duration::from_millis(900));
        assert_eq!(render_footer(&footer, 120), first);
    }

    #[test]
    fn spinner_uses_elapsed_time_and_preserves_partial_ticks() {
        let mut footer = FooterComponent::new();
        footer.set_busy_status();
        footer.update(Duration::from_millis(60));
        assert!(render_footer(&footer, 120).starts_with("⠋ Thinking…"));
        footer.update(Duration::from_millis(60));
        assert!(render_footer(&footer, 120).starts_with("⠙ Thinking…"));
        footer.update(Duration::from_millis(280));
        assert!(render_footer(&footer, 120).starts_with("⠼ Thinking…"));

        footer.update(Duration::from_millis(50));
        footer.set_default_status();
        footer.set_busy_status();
        footer.update(Duration::from_millis(50));
        assert!(render_footer(&footer, 120).starts_with("⠋ Thinking…"));
    }

    #[test]
    fn status_changes_stop_the_spinner() {
        let mut footer = FooterComponent::new();
        for status in [None, Some("An error occurred")] {
            footer.set_busy_status();
            match status {
                Some(status) => footer.set_status(status),
                None => footer.set_default_status(),
            }
            let before = render_footer(&footer, 120);
            footer.update(Duration::from_millis(100));
            assert_eq!(render_footer(&footer, 120), before);
            assert!(footer.spinner.is_none());
        }
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
    fn narrow_footer_preserves_cancel_hint_without_cache_usage() {
        let mut footer = FooterComponent::new();
        footer.set_context_size(0);
        footer.set_busy_status();
        assert!(render_footer(&footer, 40).contains("Esc: cancel"));
        render_footer(&footer, 5);
    }

    #[test]
    fn cache_hit_percent_is_token_weighted() {
        let mut footer = FooterComponent::new();
        footer.cache_usage.add(Some(100), Some(25));
        footer.cache_usage.add(Some(300), Some(150));
        assert!(render_footer(&footer, 200).contains("Conversation cache hit: 43.8%"));
    }

    #[test]
    fn missing_cache_telemetry_is_explicit() {
        let mut footer = FooterComponent::new();
        footer.cache_usage.add(Some(100), None);
        assert!(render_footer(&footer, 220).contains("Conversation cache hit: unavailable"));
        footer.cache_usage.add(Some(100), Some(50));
        assert!(render_footer(&footer, 220).contains("50.0% (partial telemetry)"));
        footer.cache_usage = CacheUsage::default();
        assert!(!render_footer(&footer, 220).contains("cache hit"));
    }

    #[test]
    fn narrow_footer_preserves_shortcuts_with_cache_usage() {
        let mut footer = FooterComponent::new();
        footer.set_context_size(12500);
        footer.set_context_limit(Some(128000));
        footer.cache_usage.add(Some(100), Some(50));
        footer.set_busy_status();
        for width in [40, 60, 80] {
            let rendered = render_footer(&footer, width);
            assert!(rendered.contains("Thinking… Esc: cancel | Ctrl+C: quit"));
        }
        footer.set_default_status();
        assert!(render_footer(&footer, 80).contains("PgUp/PgDn: scroll | Ctrl+C: quit"));
        assert!(render_footer(&footer, 128).contains("Conversation cache hit: 50.0%"));
    }
}
