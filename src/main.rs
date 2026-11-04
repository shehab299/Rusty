use crate::{agent::Agent, config::AppConfig, registry::get_tools};

use ai_sdk::providers::{ModelError, OpenRouter};
use comms::{Command, Event};
use tokio::sync::mpsc;

mod agent;
mod comms;
mod config;
mod registry;
mod tui;

#[tokio::main]
async fn main() -> Result<(), ModelError> {
    let config = AppConfig::load()?;

    let (commands_tx, commands_rx) = mpsc::channel::<Command>(100);
    let (events_tx, events_rx) = mpsc::channel::<Event>(100);

    let openrouter = config
        .openrouter
        .ok_or("OpenRouter is not configured; set [openrouter] in config.toml or SDK__OPENROUTER__API_KEY and SDK__OPENROUTER__MODEL")?;

    let model = OpenRouter::new(openrouter.api_key, openrouter.model)?;

    let mut agent = Agent::new(
        "You are a helpful assistant".into(),
        get_tools(),
        events_tx,
        commands_rx,
    )?;

    let agent_task = tokio::spawn(async move { agent.run(&model).await });

    let terminal_result = tui::run(commands_tx.clone(), events_rx).await;
    let _ = commands_tx.send(Command::Shutdown).await;

    let agent_result = agent_task.await?;
    terminal_result?;

    match agent_result {
        Err(error) if error.is::<mpsc::error::SendError<Event>>() => Ok(()),
        result => result,
    }
}
