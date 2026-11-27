use ratatui::style::{Color, Modifier, Style};
use ratatui_textarea::TextArea;

pub const PAPER: Color = Color::Rgb(245, 243, 237);
pub const SURFACE: Color = Color::Rgb(236, 234, 227);
pub const INK: Color = Color::Rgb(41, 45, 50);
pub const MUTED: Color = Color::Rgb(105, 108, 112);
pub const RULE: Color = Color::Rgb(214, 212, 205);
pub const ACCENT: Color = Color::Rgb(36, 87, 197);
pub const SUCCESS: Color = Color::Rgb(70, 112, 91);
pub const ERROR: Color = Color::Rgb(166, 57, 57);

pub fn base() -> Style {
    Style::default().fg(INK).bg(PAPER)
}

pub fn selection() -> Style {
    Style::default()
        .fg(PAPER)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn style_input(input: &mut TextArea<'_>) {
    input.set_style(base());
    input.set_placeholder_style(base().fg(MUTED));
    input.set_cursor_line_style(base());
    input.set_cursor_style(Style::default().fg(PAPER).bg(ACCENT));
    input.set_selection_style(selection());
}
