use crate::config::provider::ProviderId;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
};
use ratatui_textarea::{Input, TextArea};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Providers,
    ApiKey,
}

pub struct ProviderPopup {
    screen: Screen,
    configured: Vec<ProviderId>,
    selected: ProviderId,
    search: TextArea<'static>,
    api_key: TextArea<'static>,
    message: String,
}

impl ProviderPopup {
    pub fn new(configured: Vec<ProviderId>) -> Self {
        let mut search = TextArea::default();
        search.set_placeholder_text("Search");

        Self {
            screen: Screen::Providers,
            configured,
            selected: ProviderId::ALL[0],
            search,
            api_key: Self::new_api_key_input(),
            message: String::new(),
        }
    }

    fn new_api_key_input() -> TextArea<'static> {
        let mut input = TextArea::default();
        input.set_placeholder_text("API key");
        input.set_mask_char('•');
        input
    }

    fn filtered(&self) -> Vec<ProviderId> {
        let query = self
            .search
            .lines()
            .first()
            .map_or("", String::as_str)
            .trim()
            .to_lowercase();

        ProviderId::ALL
            .into_iter()
            .filter(|id| id.to_string().to_lowercase().contains(&query))
            .collect()
    }

    pub fn selected_provider(&self) -> ProviderId {
        self.selected
    }

    pub fn api_key(&self) -> String {
        self.api_key.lines().join("")
    }

    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ProviderPopupAction {
        match self.screen {
            Screen::Providers => match key.code {
                KeyCode::Esc => ProviderPopupAction::Close,
                KeyCode::Enter => {
                    let filtered = self.filtered();
                    if filtered.is_empty() {
                        return ProviderPopupAction::None;
                    }
                    self.api_key = Self::new_api_key_input();
                    self.screen = Screen::ApiKey;
                    ProviderPopupAction::None
                }
                KeyCode::Up | KeyCode::Down => {
                    let filtered = self.filtered();
                    let count = filtered.len();
                    if count > 0 {
                        let current = filtered
                            .iter()
                            .position(|id| *id == self.selected_provider())
                            .unwrap_or(0);
                        let next = if key.code == KeyCode::Up {
                            current.saturating_sub(1)
                        } else {
                            (current + 1).min(count - 1)
                        };
                        self.selected = filtered[next];
                    }
                    ProviderPopupAction::None
                }
                KeyCode::Backspace | KeyCode::Char(_) => {
                    self.search.input(Input::from(key));
                    if let Some(id) = self.filtered().first() {
                        self.selected = *id;
                    }
                    ProviderPopupAction::None
                }
                _ => ProviderPopupAction::None,
            },
            Screen::ApiKey => match key.code {
                KeyCode::Esc => {
                    self.screen = Screen::Providers;
                    self.message.clear();
                    ProviderPopupAction::None
                }
                KeyCode::Enter if !self.api_key().trim().is_empty() => ProviderPopupAction::Configure {
                    provider: self.selected_provider(),
                    api_key: self.api_key(),
                },
                KeyCode::Enter => ProviderPopupAction::None,
                _ => {
                    self.api_key.input(Input::from(key));
                    ProviderPopupAction::None
                }
            },
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let width = area.width.min(64);
        let desired_height = match self.screen {
            Screen::Providers => 18,
            Screen::ApiKey => 11,
        };
        let height = area.height.min(desired_height);
        let panel = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        f.render_widget(Clear, panel);
        f.render_widget(Block::bordered(), panel);
        let inner = panel.inner(ratatui::layout::Margin::new(2, 1));
        let title = match self.screen {
            Screen::Providers => "Connect a provider".to_owned(),
            Screen::ApiKey => format!("Connect {}", self.selected_provider()),
        };
        let columns = Layout::horizontal([Constraint::Min(0), Constraint::Length(4)])
            .split(Rect::new(inner.x, inner.y, inner.width, 1));
        f.render_widget(Paragraph::new(title), columns[0]);
        f.render_widget(Paragraph::new("esc"), columns[1]);
        let content_area = Rect::new(
            inner.x,
            inner.y + 1,
            inner.width,
            inner.height.saturating_sub(1),
        );
        match self.screen {
            Screen::Providers => {
                let rows = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(0),
                ])
                .split(content_area);
                f.render_widget(&self.search, rows[1]);
                let filtered = self.filtered();
                let items = filtered
                    .iter()
                    .map(|id| {
                        let mark = if self.configured.contains(id) {
                            "✓ "
                        } else {
                            "  "
                        };
                        ListItem::new(Line::from(vec![Span::raw(mark), Span::raw(id.to_string())]))
                    })
                    .collect::<Vec<_>>();
                let selected = filtered
                    .iter()
                    .position(|id| *id == self.selected_provider());
                let mut state = ListState::default().with_selected(selected);
                let list = List::new(items)
                    .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
                f.render_stateful_widget(list, rows[3], &mut state);
            }
            Screen::ApiKey => {
                let rows = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(2),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(0),
                ])
                .split(content_area);
                f.render_widget(&self.api_key, rows[2]);
                let mut footer = vec![Span::raw("enter  submit")];
                if !self.message.is_empty() {
                    footer.push(Span::raw(format!("  {}", self.message)));
                }
                f.render_widget(Paragraph::new(Line::from(footer)), rows[4]);
            }
        }
    }
}

pub enum ProviderPopupAction {
    None,
    Close,
    Configure {
        provider: ProviderId,
        api_key: String,
    },
}
