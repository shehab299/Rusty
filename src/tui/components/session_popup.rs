use super::Render;
use crate::comms::SessionSummary;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
};
use ratatui_textarea::{Input, TextArea};

#[derive(Default)]
pub struct SessionPopup {
    pub visible: bool,
    sessions: Vec<SessionSummary>,
    search: TextArea<'static>,
    selected: usize,
    message: String,
    pending: bool,
}

impl SessionPopup {
    pub fn open(&mut self) {
        *self = Self::default();
        self.visible = true;
        self.message = "Loading sessions…".into();
        self.search
            .set_placeholder_text("Search conversations or directories");
        self.search.set_cursor_line_style(Style::default());
    }

    pub fn set_sessions(&mut self, sessions: Vec<SessionSummary>) {
        self.sessions = sessions;
        self.selected = 0;
        self.message = "↑/↓ select · enter resume · esc close".into();
    }

    pub fn set_error(&mut self, error: String) {
        self.pending = false;
        self.message = error;
    }

    fn filtered(&self) -> Vec<&SessionSummary> {
        let query = self.search.lines()[0].to_lowercase();
        self.sessions
            .iter()
            .filter(|session| {
                format!(
                    "{} {} {}",
                    session.title,
                    session.directory.display(),
                    session.id
                )
                .to_lowercase()
                .contains(&query)
            })
            .collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        if self.pending {
            return None;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => self.visible = false,
            (KeyCode::Up, _) => self.selected = self.selected.saturating_sub(1),
            (KeyCode::Down, _) => {
                self.selected = (self.selected + 1).min(self.filtered().len().saturating_sub(1))
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if let Some(session) = self.filtered().get(self.selected) {
                    if session.current {
                        self.message = "This session is already active.".into();
                        return None;
                    }
                    let id = session.id.clone();
                    self.pending = true;
                    self.message = "Resuming…".into();
                    return Some(id);
                }
            }
            (KeyCode::Enter, _) => {}
            _ => {
                if self.search.input(Input::from(key)) {
                    self.selected = 0;
                }
            }
        }
        None
    }
}

impl Render for SessionPopup {
    fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible || area.is_empty() {
            return;
        }
        let [_, center, _] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Percentage(85),
            Constraint::Fill(1),
        ])
        .areas(area);
        let [_, area, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Percentage(75),
            Constraint::Fill(1),
        ])
        .areas(center);
        f.render_widget(Clear, area);
        let block = Block::bordered().title("Sessions · newest first");
        let inner = block.inner(area);
        f.render_widget(block, area);
        let [search, list, status] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(2),
        ])
        .areas(inner);
        f.render_widget(&self.search, search);
        let filtered = self.filtered();
        if filtered.is_empty() {
            f.render_widget(Paragraph::new("No saved sessions found."), list);
        } else {
            let items: Vec<_> = filtered
                .iter()
                .map(|session| {
                    ListItem::new(vec![
                        format!(
                            "{}{}",
                            if session.current { "● " } else { "" },
                            session.title
                        )
                        .into(),
                        format!("  {} · {}", session.directory.display(), session.id).into(),
                    ])
                })
                .collect();
            let mut state = ListState::default().with_selected(Some(self.selected));
            f.render_stateful_widget(
                List::new(items)
                    .highlight_symbol("› ")
                    .highlight_style(Style::default().bg(Color::DarkGray)),
                list,
                &mut state,
            );
        }
        f.render_widget(
            Paragraph::new(self.message.as_str()).style(Style::default().fg(Color::Cyan)),
            status,
        );
    }
}
