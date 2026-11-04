use crate::tui::components::Render;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};
use ratatui_textarea::{Input, TextArea};

pub struct PromptComponent {
    prompt: TextArea<'static>,
}

impl PromptComponent {
    pub fn new() -> Self {
        let mut prompt = TextArea::default();
        prompt.set_block(Block::default().borders(Borders::ALL));
        prompt.set_placeholder_text("Ask me anything..");
        prompt.set_cursor_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        prompt.set_cursor_line_style(Style::default());

        Self { prompt }
    }

    pub fn content(&self) -> String {
        self.prompt.lines().join("\n")
    }

    pub fn height(&self, available_height: u16) -> u16 {
        self.prompt
            .lines()
            .len()
            .saturating_add(2)
            .clamp(3, usize::from(available_height.max(3) / 2 + 2)) as u16
    }

    pub fn insert_newline(&mut self) {
        self.prompt.insert_newline();
    }

    pub fn clear(&mut self) {
        self.prompt.clear();
    }

    pub fn input(&mut self, input: Input) {
        self.prompt.input(input);
    }
}

impl Render for PromptComponent {
    fn render(&self, f: &mut Frame, area: Rect) {
        f.render_widget(&self.prompt, area);
    }
}
