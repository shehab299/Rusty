//! In-process backend: runtime clients, model configuration, and turn execution.
use std::collections::VecDeque;

use ai_sdk::Message;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::Instrument;

use crate::{
    agent::Agent,
    comms::{AppCommand, AppEvent, ModelOption},
    config::{
        ModelConfig,
        provider::{AnthropicConfig, OpenAIConfig, OpenRouterConfig, ProviderConfig, ProviderId},
    },
    error::AppError,
    model_catalog::Catalog,
    provider_manager::{ModelSelection, ProviderManager},
};

#[cfg(test)]
mod tests;

pub struct App {
    agent: Agent,
    context: Vec<Message>,
    selection: Option<ModelSelection>,
    providers: ProviderManager,
    catalog: Catalog,
    events: Sender<AppEvent>,
    commands: Receiver<AppCommand>,
}

impl App {
    pub fn new(
        agent: Agent,
        system_prompt: String,
        model: Option<ModelConfig>,
        providers: ProviderManager,
        catalog: Catalog,
        events: Sender<AppEvent>,
        commands: Receiver<AppCommand>,
    ) -> Result<Self, AppError> {
        let selection = model
            .filter(|model| {
                catalog.has_model(&model.provider.to_string().to_lowercase(), &model.model)
            })
            .map(|model| providers.select(model.provider, model.model))
            .transpose()?;

        Ok(Self {
            agent,
            context: vec![Message::system(system_prompt)],
            selection,
            providers,
            catalog,
            events,
            commands,
        })
    }

    pub async fn run(&mut self) -> Result<(), AppError> {
        self.send_providers().await?;
        self.events
            .send(AppEvent::DirectoryChanged(std::env::current_dir()?))
            .await?;
        if let Some(selection) = &self.selection {
            self.events.send(self.model_selected(selection)).await?;
        }

        let mut pending = VecDeque::new();

        loop {
            let command = match pending.pop_front() {
                Some(command) => command,
                None => match self.commands.recv().await {
                    Some(command) => command,
                    None => return Ok(()),
                },
            };

            match command {
                AppCommand::SetModel { provider, model } => {
                    match self.providers.select(provider, model) {
                        Ok(selection) => {
                            let event = self.model_selected(&selection);
                            self.selection = Some(selection);
                            self.events.send(event).await?;
                        }
                        Err(error) => {
                            self.events
                                .send(AppEvent::ModelSelectionFailed {
                                    error: error.to_string(),
                                })
                                .await?
                        }
                    }
                }
                AppCommand::ConfigureProvider { provider, api_key } => {
                    self.configure_provider(provider, api_key).await?;
                }
                AppCommand::SubmitPrompt(prompt) => {
                    if self.execute_prompt(prompt, &mut pending).await? {
                        return Ok(());
                    }
                }
                AppCommand::Shutdown => return Ok(()),
                AppCommand::ClearConversation => {
                    self.context.truncate(1);
                    self.events.send(AppEvent::ConversationCleared).await?;
                }
                AppCommand::SetDirectory(path) => match std::env::set_current_dir(&path) {
                    Ok(()) => {
                        self.events
                            .send(AppEvent::DirectoryChanged(std::env::current_dir()?))
                            .await?
                    }
                    Err(error) => {
                        self.notice(format!("Cannot change directory: {error}"))
                            .await?
                    }
                },
                AppCommand::Cancel => {}
            }
        }
    }

    async fn configure_provider(
        &mut self,
        provider: ProviderId,
        api_key: String,
    ) -> Result<(), AppError> {
        let config = match provider {
            ProviderId::OpenAI => ProviderConfig::OpenAI(OpenAIConfig { api_key }),
            ProviderId::OpenRouter => ProviderConfig::OpenRouter(OpenRouterConfig { api_key }),
            ProviderId::Anthropic => ProviderConfig::Anthropic(AnthropicConfig { api_key }),
        };

        match self.providers.configure(config) {
            Ok(provider) => {
                // Replace the selected client too when reconnecting its provider.
                if let Some(selection) = &self.selection
                    && selection.provider() == provider
                {
                    self.selection = Some(
                        self.providers
                            .select(provider, selection.model().to_owned())?,
                    );
                }
                self.send_providers().await?;
                self.events
                    .send(AppEvent::ProviderConfigured { provider })
                    .await?;
            }
            Err(error) => {
                self.events
                    .send(AppEvent::ProviderConfigurationFailed {
                        provider,
                        error: error.to_string(),
                    })
                    .await?
            }
        }
        Ok(())
    }

    /// Returns true when shutdown was requested during execution.
    async fn execute_prompt(
        &mut self,
        prompt: String,
        pending: &mut VecDeque<AppCommand>,
    ) -> Result<bool, AppError> {
        let mut shutdown = false;
        let mut clear = false;
        let (agent_events_tx, mut agent_events_rx) = mpsc::channel(100);

        let Some(selection) = self.selection.as_ref() else {
            self.events
                .send(AppEvent::Failed(
                    "No model selected; configure a provider and select a model first".into(),
                ))
                .await?;
            return Ok(false);
        };

        self.context.push(Message::user(prompt));

        let outcome = {
            let turn_span = tracing::info_span!(
                parent: None,
                "agent.turn",
                gen_ai.provider.name = %selection.provider(),
                gen_ai.request.model = %selection.model(),
                outcome = tracing::field::Empty,
                otel.status_code = tracing::field::Empty,
            );

            let mut operation = crate::telemetry::Operation::new(turn_span.clone());

            let turn = self
                .agent
                .run_turn(&mut self.context, selection, &agent_events_tx)
                .instrument(turn_span);
            tokio::pin!(turn);

            self.events.send(AppEvent::Started).await?;

            let outcome = loop {
                tokio::select! {
                    biased;
                    command = self.commands.recv() => match command {
                        Some(AppCommand::Cancel) => break None,
                        Some(AppCommand::ClearConversation) => {
                            // Clear queued prompts, but still acknowledge pending
                            // provider/model configuration requests from the UI.
                            pending.retain(|command| !matches!(command, AppCommand::SubmitPrompt(_)));
                            clear = true;
                            break None;
                        }
                        Some(AppCommand::SetDirectory(_)) => {
                            self.events.send(AppEvent::Notice("Cancel the active turn before changing directories.".into())).await?;
                        }
                        Some(AppCommand::Shutdown) | None => {
                            operation.shutdown();
                            shutdown = true;
                            break None;
                        }
                        Some(command) => pending.push_back(command),
                    },
                    Some(event) = agent_events_rx.recv() => {
                        self.events.send(event.into()).await?;
                    },
                    result = &mut turn => break Some(result),
                }
            };

            if let Some(result) = &outcome {
                operation.complete(result.is_ok());
            }

            outcome
        };

        if shutdown {
            return Ok(true);
        }


        while let Ok(event) = agent_events_rx.try_recv() {
            self.events.send(event.into()).await?;
        }

        let event = if clear {
            self.context.truncate(1);
            AppEvent::ConversationCleared
        } else {
            match outcome {
                Some(Ok(())) => AppEvent::Completed,
                Some(Err(error)) => AppEvent::Failed(error.to_string()),
                None => AppEvent::Failed("Turn cancelled. Tools may have changed files.".into()),
            }
        };
        self.events.send(event).await?;

        Ok(false)
    }

    async fn notice(&self, message: String) -> Result<(), AppError> {
        self.events.send(AppEvent::Notice(message)).await?;
        Ok(())
    }

    fn model_selected(&self, selection: &ModelSelection) -> AppEvent {
        let context_limit = self
            .catalog
            .model(
                &selection.provider().to_string().to_lowercase(),
                selection.model(),
            )
            .and_then(|model| model.limit.context);
        AppEvent::ModelSelected {
            provider: selection.provider(),
            model: selection.model().to_owned(),
            context_limit,
        }
    }

    async fn send_providers(&self) -> Result<(), AppError> {
        let configured = self.providers.configured();
        let models = configured
            .iter()
            .flat_map(|provider| {
                self.catalog
                    .models(&provider.to_string().to_lowercase())
                    .filter(|model| {
                        model
                            .modalities
                            .output
                            .iter()
                            .any(|output| output == "text")
                    })
                    .map(move |model| ModelOption {
                        provider: *provider,
                        id: model.id.clone(),
                        name: model.name.clone(),
                        is_free: model.is_free(),
                    })
            })
            .collect();

        self.events
            .send(AppEvent::ProvidersUpdated { configured, models })
            .await?;

        Ok(())
    }
}
