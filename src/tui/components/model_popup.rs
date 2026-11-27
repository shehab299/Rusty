use super::Component;
use crate::comms::ModelOption;
use crate::config::provider::ProviderId;
use crate::tui::theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
};
use ratatui_textarea::{Input, TextArea};

pub struct ModelPopup {
    pub visible: bool,
    pub current: Option<(ProviderId, String)>,
    selected: usize,
    search: TextArea<'static>,
    message: String,
    error: Option<String>,
    configured_providers: Vec<ProviderId>,
    models: Vec<ModelOption>,
}

pub enum ModelPopupAction {
    None,
    Connect,
    Select { provider: ProviderId, model: String },
}

impl ModelPopup {
    pub fn new() -> Self {
        Self {
            visible: false,
            configured_providers: vec![],
            current: None,
            models: vec![],
            selected: 0,
            search: Self::new_search(),
            error: None,
            message: "No configured providers. Press ctrl+a to connect one.".into(),
        }
    }

    fn new_search() -> TextArea<'static> {
        let mut search = TextArea::default();
        search.set_placeholder_text("Search");
        theme::style_input(&mut search);
        search
    }

    pub fn set_configured_providers(&mut self, providers: Vec<ProviderId>) {
        self.configured_providers = providers;
        self.message = if self.configured_providers.is_empty() {
            "No configured providers. Press ctrl+a to connect one."
        } else {
            "No models found. Try a different search."
        }
        .into();
    }

    pub fn open(&mut self) {
        self.visible = true;
        self.search = Self::new_search();
        self.error = None;
        self.select_current();
    }

    pub fn set_models(&mut self, mut models: Vec<ModelOption>) {
        models.sort_by_cached_key(|model| {
            (
                ProviderId::ALL.iter().position(|id| id == &model.provider),
                model.name.to_lowercase(),
                model.id.clone(),
            )
        });
        self.models = models;
        self.error = None;
        self.select_current();
    }

    fn select_current(&mut self) {
        self.selected = self
            .filtered()
            .iter()
            .position(|model| {
                self.current
                    .as_ref()
                    .is_some_and(|(id, name)| id == &model.provider && name == &model.id)
            })
            .unwrap_or(0);
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    fn filtered(&self) -> Vec<&ModelOption> {
        let query = self.search.lines()[0].trim().to_lowercase();
        self.models
            .iter()
            .filter(|model| {
                model.name.to_lowercase().contains(&query)
                    || model.id.to_lowercase().contains(&query)
                    || model.provider.to_string().to_lowercase().contains(&query)
            })
            .collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModelPopupAction {
        if !self.visible {
            return ModelPopupAction::None;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.visible = false;
                ModelPopupAction::None
            }
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.visible = false;
                ModelPopupAction::Connect
            }
            (KeyCode::Up, _) => {
                self.selected = self.selected.saturating_sub(1);
                ModelPopupAction::None
            }
            (KeyCode::Down, _) => {
                self.selected = (self.selected + 1).min(self.filtered().len().saturating_sub(1));
                ModelPopupAction::None
            }
            (KeyCode::Enter, KeyModifiers::NONE) => match self.filtered().get(self.selected) {
                Some(model) => ModelPopupAction::Select {
                    provider: model.provider,
                    model: model.id.clone(),
                },
                None => ModelPopupAction::None,
            },
            (KeyCode::Enter, _) => ModelPopupAction::None,
            _ => {
                if self.search.input(Input::from(key)) {
                    self.selected = 0;
                }
                ModelPopupAction::None
            }
        }
    }
}

impl Component for ModelPopup {
    fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let width = area.width.min(70);
        let height = area.height.min(24);
        let panel = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        f.render_widget(Clear, panel);
        f.render_widget(
            Block::bordered()
                .style(theme::base())
                .border_style(Style::default().fg(theme::RULE)),
            panel,
        );

        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(panel.inner(Margin::new(2, 1)));

        let title = Layout::horizontal([Constraint::Min(0), Constraint::Length(3)]).split(rows[0]);
        f.render_widget(Paragraph::new("Select model").bold(), title[0]);
        f.render_widget(Paragraph::new("esc").fg(theme::MUTED), title[1]);

        f.render_widget(&self.search, rows[2]);

        f.render_widget(
            Paragraph::new(
                self.error
                    .as_deref()
                    .unwrap_or("Connect a provider ctrl+a   Select enter"),
            ),
            rows[6],
        );

        let filtered = self.filtered();
        if filtered.is_empty() {
            let message = self
                .error
                .as_ref()
                .cloned()
                .unwrap_or_else(|| self.message.clone());
            f.render_widget(Paragraph::new(message), rows[4]);
        } else {
            let mut items = vec![];
            let mut previous_provider = None;
            let mut selected_row = None;
            for (index, model) in filtered.iter().enumerate() {
                if previous_provider != Some(model.provider) {
                    if previous_provider.is_some() {
                        items.push(ListItem::new(""));
                    }
                    items.push(ListItem::new(Line::styled(
                        model.provider.to_string(),
                        Style::default()
                            .fg(theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    )));
                    previous_provider = Some(model.provider);
                }
                if index == self.selected {
                    selected_row = Some(items.len());
                }
                let mut spans = vec![Span::raw(model.name.clone())];
                if model.is_free {
                    spans.push(Span::styled("  Free", Style::default().fg(theme::MUTED)));
                }
                items.push(ListItem::new(Line::from(spans)));
            }
            let list = List::new(items).highlight_style(theme::selection());
            let mut state = ListState::default().with_selected(selected_row);
            f.render_stateful_widget(list, rows[4], &mut state);
        }
    }
}
