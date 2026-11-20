use crate::app::App;
use crate::provider_manager::ProviderManager;
use crate::{agent::Agent, config::AppConfig, error::AppError, tool_registry::get_tools};

use comms::{AppCommand, AppEvent};
use tokio::{sync::mpsc, task::spawn_blocking};
use tui::Tui;

mod agent;
mod app;
mod comms;
mod config;
mod error;
mod model_catalog;
mod provider_manager;
mod telemetry;
mod tool_registry;
mod tui;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let config = AppConfig::load().map_err(AppError::Config)?;
    let telemetry_config = config.opentelemetry;

    let _telemetry = match spawn_blocking(move || telemetry::init(telemetry_config))
        .await
        .map_err(AppError::Task)?
    {
        Ok(telemetry) => telemetry,
        Err(error) => {
            eprintln!("Telemetry disabled: {error}");
            None
        }
    };

    let catalog = model_catalog::load_catalog().await?;
    let provider_manager = ProviderManager::from_config(config.provider)?;

    let (commands_tx, commands_rx) = mpsc::channel::<AppCommand>(100);
    let (events_tx, events_rx) = mpsc::channel::<AppEvent>(100);

    let mut app = App::new(
        Agent::new(get_tools())?,
        include_str!("../public/prompts/system_prompt.md").into(),
        config.model,
        provider_manager,
        catalog,
        events_tx,
        commands_rx,
    )?;

    let app_task = tokio::spawn(async move { app.run().await });

    let terminal_result = {
        let mut tui = Tui::new(commands_tx.clone(), events_rx);
        tui.run().await
    };

    let _ = commands_tx.send(AppCommand::Shutdown).await;

    let app_result = app_task.await.map_err(AppError::Task)?;
    terminal_result.map_err(AppError::Terminal)?;

    match app_result {
        Err(AppError::Channel(error)) if error.is::<mpsc::error::SendError<AppEvent>>() => Ok(()),
        result => result,
    }
}
