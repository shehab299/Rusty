use ratatui::{Frame, layout::Rect};

mod footer;
mod messages;
mod prompt;

pub(super) use footer::FooterComponent;
pub(super) use messages::{MessageComponent, MessagesComponent};
pub(super) use prompt::PromptComponent;

pub trait Render {
    fn render(&self, f: &mut Frame, area: Rect);
}
