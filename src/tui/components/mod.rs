use ratatui::{Frame, layout::Rect};

mod footer;
mod messages;
mod prompt;
mod provider_popup;

pub(super) use footer::FooterComponent;
pub(super) use messages::{MessageComponent, MessagesComponent};
pub(super) use prompt::PromptComponent;
pub(super) use provider_popup::{ProviderPopupAction, ProviderPopup};

pub trait Render {
    fn render(&self, f: &mut Frame, area: Rect);
}
