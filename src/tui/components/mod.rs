use ratatui::{Frame, layout::Rect};

mod footer;
mod messages;
mod model_popup;
mod prompt;
mod provider_popup;
mod slash_commands_list;

pub(super) use footer::FooterComponent;
pub(super) use messages::{MessageComponent, MessagesComponent};
pub(super) use model_popup::{ModelPopup, ModelPopupAction};
pub(super) use prompt::PromptComponent;
pub(super) use provider_popup::{ProviderPopup, ProviderPopupAction};
pub(super) use slash_commands_list::{SlashCommand, SlashCommandMenu, SlashCommandMenuAction};

pub trait Render {
    fn render(&self, f: &mut Frame, area: Rect);
}
