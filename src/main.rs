use crate::{agent::Agent, config::AppConfig, error::AppError, registry::get_tools};

use comms::{AgentCommand, AgentEvent};
use tokio::sync::mpsc;
use tui::Tui;

mod agent;
mod comms;
mod config;
mod error;
mod model_catalog;
mod provider_manager;
mod registry;
mod tui;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let config = AppConfig::load().map_err(AppError::Config)?;
    let catalog = model_catalog::load_catalog().await?;

    let (commands_tx, commands_rx) = mpsc::channel::<AgentCommand>(100);
    let (events_tx, events_rx) = mpsc::channel::<AgentEvent>(100);

    let mut agent = Agent::new(
        include_str!("../public/prompts/system_prompt.md").into(),
        get_tools(),
        events_tx,
        commands_rx,
    )?;

    let agent_task = tokio::spawn(async move { agent.run().await });

    let terminal_result = {
        let mut tui = Tui::new(commands_tx.clone(), events_rx, config.provider, catalog)?;
        tui.run().await
    };
    let _ = commands_tx.send(AgentCommand::Shutdown).await;

    let agent_result = agent_task.await.map_err(AppError::Task)?;
    terminal_result.map_err(AppError::Terminal)?;

    match agent_result {
        Err(AppError::Channel(error)) if error.is::<mpsc::error::SendError<AgentEvent>>() => Ok(()),
        result => result,
    }
}
