//! In-process backend: runtime clients, model configuration, and turn execution.
use std::collections::VecDeque;

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
    session::{Event, Session, TurnOutcome},
};

mod sessions;

pub struct App {
    agent: Agent,
    selection: Option<ModelSelection>,
    provider_manager: ProviderManager,
    catalog: Catalog,
    events: Sender<AppEvent>,
    commands: Receiver<AppCommand>,
    session: Session,
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

        let mut session = Session::create(crate::config::home::get_session_store()?)?;

        if !session.data.state.context.is_empty() {
            return Err(AppError::Wal(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a new app requires an empty event log",
            )));
        }

        session
            .record(Event::SessionStarted {
                system_prompt,
                directory: std::env::current_dir()?,
            })
            .map_err(AppError::Wal)?;

        if let Some(selection) = &selection {
            session
                .record(Event::ModelSelected {
                    provider: selection.provider(),
                    model: selection.model().to_owned(),
                })
                .map_err(AppError::Wal)?;
        }

        Ok(Self {
            agent,
            selection,
            provider_manager: providers,
            catalog,
            events,
            commands,
            session,
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
                AppCommand::ListSessions => {
                    let event = match self.list_sessions() {
                        Ok(sessions) => AppEvent::SessionsListed(sessions),
                        Err(error) => AppEvent::SessionFailed(error.to_string()),
                    };
                    self.events.send(event).await?;
                }
                AppCommand::ResumeSession(id) => {
                    let event = match self.resume_session(&id) {
                        Ok(event) => event,
                        Err(error) => AppEvent::SessionFailed(error.to_string()),
                    };
                    self.events.send(event).await?;
                }
                AppCommand::SetModel { provider, model } => {
                    match self.provider_manager.select(provider, model) {
                        Ok(selection) => {
                            self.session
                                .record(Event::ModelSelected {
                                    provider: selection.provider(),
                                    model: selection.model().to_owned(),
                                })
                                .map_err(AppError::Wal)?;
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
                    self.session
                        .record(Event::ConversationCleared)
                        .map_err(AppError::Wal)?;
                    self.events.send(AppEvent::ConversationCleared).await?;
                }
                AppCommand::SetDirectory(path) => match std::env::set_current_dir(&path) {
                    Ok(()) => {
                        self.session
                            .record(Event::DirectoryChanged {
                                directory: std::env::current_dir()?,
                            })
                            .map_err(AppError::Wal)?;
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

        match self.provider_manager.configure(config) {
            Ok(provider) => {
                // Replace the selected client too when reconnecting its provider.
                if let Some(selection) = &self.selection
                    && selection.provider() == provider
                {
                    self.selection = Some(
                        self.provider_manager
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

        self.session
            .record(Event::PromptSubmitted { content: prompt })
            .map_err(AppError::Wal)?;

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
                .run_turn(selection, &agent_events_tx, &mut self.session)
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
                        Some(AppCommand::ListSessions | AppCommand::ResumeSession(_)) => {
                            self.events.send(AppEvent::SessionFailed("Cancel the active turn before switching sessions.".into())).await?;
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

        // A WAL failure is fail-stop, not a retry of model/tool execution. The
        // append might have reached disk even when fsync reported an error.
        if matches!(&outcome, Some(Err(AppError::Wal(_)))) {
            return Err(outcome.expect("matched outcome").unwrap_err());
        }
        self.session
            .mark_running_tools_interrupted()
            .map_err(AppError::Wal)?;
        let (status, error) = match &outcome {
            Some(Ok(())) => (TurnOutcome::Completed, None),
            Some(Err(error)) => (TurnOutcome::Failed, Some(error.to_string())),
            None => (
                TurnOutcome::Cancelled,
                Some("Turn cancelled. Tools may have changed files.".into()),
            ),
        };
        self.session
            .record(Event::TurnFinished {
                outcome: status,
                error,
            })
            .map_err(AppError::Wal)?;
        if shutdown {
            return Ok(true);
        }

        while let Ok(event) = agent_events_rx.try_recv() {
            self.events.send(event.into()).await?;
        }

        let event = if clear {
            self.session
                .record(Event::ConversationCleared)
                .map_err(AppError::Wal)?;
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
        let configured = self.provider_manager.configured();
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
