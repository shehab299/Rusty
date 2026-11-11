use super::Render;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlashCommand {
    Connect,
    Models,
    CD,
}

impl SlashCommand {
    const ALL: [Self; 3] = [Self::Connect, Self::Models, Self::CD];

    pub fn name(self) -> &'static str {
        match self {
            Self::Connect => "/connect",
            Self::Models => "/models",
            Self::CD => "/cd",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Connect => "Connect a provider",
            Self::Models => "Select a model",
            Self::CD => "Change working directory",
        }
    }

    pub fn parse(input: &str) -> (Option<Self>, Vec<&str>) {
        let mut words = input.split_whitespace();
        let name = words.next();

        if let Some(name) = name {
            let command = Self::ALL.into_iter().find(|command| command.name() == name);
            return (command, words.collect());
        }

        (None, words.collect())
    }
}

#[derive(Default)]
pub struct SlashCommandMenu {
    pub visible: bool,
    query: String,
    candidates: Vec<SlashCommand>,
    selected: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SlashCommandMenuAction {
    Ignored,
    Consumed,
    Complete(SlashCommand),
    Execute(SlashCommand),
}

impl SlashCommandMenu {
    pub fn handle_key(&mut self, key: KeyEvent) -> SlashCommandMenuAction {
        if !self.visible {
            return SlashCommandMenuAction::Ignored;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.previous();
                SlashCommandMenuAction::Consumed
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.next();
                SlashCommandMenuAction::Consumed
            }
            (KeyCode::Esc, _) => {
                self.visible = false;
                SlashCommandMenuAction::Consumed
            }
            (KeyCode::Tab, KeyModifiers::NONE) => self.selected().map_or(
                SlashCommandMenuAction::Consumed,
                SlashCommandMenuAction::Complete,
            ),
            (KeyCode::Enter, KeyModifiers::NONE) => self.selected().map_or(
                SlashCommandMenuAction::Consumed,
                SlashCommandMenuAction::Execute,
            ),
            _ => SlashCommandMenuAction::Ignored,
        }
    }

    pub fn update(&mut self, content: &str) {
        if self.query == content {
            return;
        }
        self.query = content.into();
        self.selected = 0;
        self.candidates = if content.starts_with('/') && !content.chars().any(char::is_whitespace) {
            SlashCommand::ALL
                .into_iter()
                .filter(|command| command.name().starts_with(content))
                .collect()
        } else {
            Vec::new()
        };
        self.visible = !self.candidates.is_empty();
    }

    fn selected(&self) -> Option<SlashCommand> {
        self.candidates.get(self.selected).copied()
    }

    fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn next(&mut self) {
        self.selected = (self.selected + 1).min(self.candidates.len().saturating_sub(1));
    }

    pub fn height(&self) -> u16 {
        if self.visible {
            self.candidates.len() as u16 + 2
        } else {
            0
        }
    }
}

impl Render for SlashCommandMenu {
    fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible || area.is_empty() {
            return;
        }
        let items: Vec<_> = self
            .candidates
            .iter()
            .map(|command| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:<12}", command.name()),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(command.description()),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Commands · ↑/↓ · tab complete · enter run · esc"),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› ");
        let mut state = ListState::default().with_selected(Some(self.selected));
        f.render_stateful_widget(list, area, &mut state);
    }
}
