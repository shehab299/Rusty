use crate::tui::components::Component;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui_textarea::{Input, Key, TextArea, WrapMode};

pub struct PromptComponent {
    prompt: TextArea<'static>,
    history: Vec<String>,
    history_idx: Option<usize>,
    draft: String,
}

impl PromptComponent {
    pub fn new() -> Self {
        let mut prompt = TextArea::default();
        prompt.set_wrap_mode(WrapMode::WordOrGlyph);
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
            prompt,
            history: Vec::new(),
            history_idx: None,
            draft: String::new(),
        }
    }

    pub fn content(&self) -> String {
        self.prompt.lines().join("\n")
    }

    pub fn height(&self, available_width: u16, available_height: u16) -> u16 {
        // Measure soft-wrapped rows inside the border, not just explicit newlines.
        let lines: Vec<Line<'_>> = self
            .prompt
            .lines()
            .iter()
            .map(|line| Line::from(line.as_str()))
            .collect();
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .line_count(available_width.saturating_sub(2).max(1))
            .saturating_add(2)
            .clamp(3, usize::from(available_height.max(3) / 2 + 2)) as u16
    }

    pub fn insert_newline(&mut self) {
        self.prompt.insert_newline();
    }

    pub fn insert_text(&mut self, text: &str) {
        let mut line = String::new();
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '\n' => {
                    self.prompt.insert_str(&line);
                    line.clear();
                    self.prompt.insert_newline();
                }
                '\r' => {
                    self.prompt.insert_str(&line);
                    line.clear();
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    self.prompt.insert_newline();
                }
                _ => line.push(ch),
            }
        }

        self.prompt.insert_str(&line);
    }

    pub fn clear(&mut self) {
        self.prompt.clear();
        self.history_idx = None;
        self.draft.clear();
    }

    pub fn set_content(&mut self, content: &str) {
        self.clear();
        self.prompt.insert_str(content);
    }

    pub fn record_submission(&mut self, content: String) {
        self.history.push(content);
        self.clear();
    }

    pub fn input(&mut self, input: Input) {
        match input {
            Input {
                key: Key::Up,
                ctrl: false,
                alt: false,
                shift: false,
            } if self.prompt.cursor().0 == 0 => self.previous_history(),
            Input {
                key: Key::Down,
                ctrl: false,
                alt: false,
                shift: false,
            } if self.prompt.cursor().0 == self.prompt.lines().len() - 1 => {
                self.next_history();
            }
            _ => {
                self.prompt.input(input);
            }
        }
    }

    fn previous_history(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.history_idx {
            Some(0) => return,
            Some(idx) => idx - 1,
            None => {
                self.draft = self.content();
                self.history.len() - 1
            }
        };
        self.history_idx = Some(idx);
        self.prompt.clear();
        self.prompt.insert_str(&self.history[idx]);
    }

    fn next_history(&mut self) {
        let Some(idx) = self.history_idx else {
            return;
        };
        self.prompt.clear();
        if idx + 1 < self.history.len() {
            self.history_idx = Some(idx + 1);
            self.prompt.insert_str(&self.history[idx + 1]);
        } else {
            self.history_idx = None;
            self.prompt.insert_str(&self.draft);
            self.draft.clear();
        }
    }
}

impl Component for PromptComponent {
    fn render(&self, f: &mut Frame, area: Rect) {
        f.render_widget(&self.prompt, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_grows_with_wrapped_rows_and_resizes() {
        let mut prompt = PromptComponent::new();
        assert_eq!(prompt.height(12, 40), 3);
        prompt.insert_text("abcdefghijabcdefghijabcde");
        assert_eq!(prompt.height(12, 40), 5);
        assert_eq!(prompt.height(32, 40), 3);
        assert_eq!(prompt.content(), "abcdefghijabcdefghijabcde");
    }

    #[test]
    fn height_preserves_newlines_and_stays_capped() {
        let mut prompt = PromptComponent::new();
        prompt.insert_text("first\nsecond\n");
        assert_eq!(prompt.height(20, 40), 5);
        prompt.insert_text(&"x".repeat(1000));
        assert_eq!(prompt.height(12, 20), 12);
    }
}
