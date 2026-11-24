use ratatui::{Frame, layout::Rect};
use std::time::Duration;

mod footer;
mod messages;
mod model_popup;
mod prompt;
mod provider_popup;
mod session_popup;
mod slash_command_menu;
mod spinner;

pub(super) use footer::FooterComponent;
pub(super) use messages::{MessageComponent, MessagesComponent};
pub(super) use model_popup::{ModelPopup, ModelPopupAction};
pub(super) use prompt::PromptComponent;
pub(super) use provider_popup::{ProviderPopup, ProviderPopupAction};
pub(super) use session_popup::SessionPopup;
pub(super) use slash_command_menu::{SlashCommand, SlashCommandMenu, SlashCommandMenuAction};
pub(super) use spinner::Spinner;

pub trait Component {
    fn update(&mut self, _elapsed: Duration) {}

    fn render(&self, f: &mut Frame, area: Rect);
}
