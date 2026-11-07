use crate::{agent::Agent, config::AppConfig, registry::get_tools};

use ai_sdk::providers::ModelError;
use comms::{Command, Event};
use tokio::sync::mpsc;
use tui::Tui;

mod agent;
mod comms;
mod config;
mod error;
mod registry;
mod tui;

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let config = AppConfig::load()?;

    let (commands_tx, commands_rx) = mpsc::channel::<Command>(100);
    let (events_tx, events_rx) = mpsc::channel::<Event>(100);

    let mut agent = Agent::new(
        include_str!("../public/prompts/system_prompt.md").into(),
        get_tools(),
        events_tx,
        commands_rx,
        config.provider,
    )?;

    let agent_task = tokio::spawn(async move { agent.run().await });

    let terminal_result = {
        let mut tui = Tui::new(commands_tx.clone(), events_rx);
        tui.run().await
    };
    let _ = commands_tx.send(Command::Shutdown).await;

    let agent_result = agent_task.await?;
    terminal_result?;

    match agent_result {
        Err(error) if error.is::<mpsc::error::SendError<Event>>() => Ok(()),
        result => result,
    }
}
