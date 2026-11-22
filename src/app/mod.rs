//! In-process backend: runtime clients, model configuration, and turn execution.

use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, MissedTickBehavior, interval},
};
use tracing::Instrument;

use crate::{
    agent::Agent,
    comms::{AppCommand, AppEvent, ModelOption, SessionMessage, SessionSummary},
    config::{
        ModelConfig,
        provider::{
            AnthropicConfig, OpenAIConfig, OpenRouterConfig, ProviderConfig, ProviderId,
            ProvidersConfig,
        },
    },
    error::AppError,
    model_catalog::Catalog,
    provider_manager::{ModelSelection, ProviderManager},
    session::{CacheUsage, Event, Session, TurnOutcome},
};

use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
};

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
    // Lifecycle

    pub async fn new(
        agent: Agent,
        system_prompt: String,
        model_config: Option<ModelConfig>,
        provider_config: ProvidersConfig,
        events: Sender<AppEvent>,
        commands: Receiver<AppCommand>,
    ) -> Result<Self, AppError> {
        let catalog = match Catalog::from_cached().await {
            Ok(catalog) => catalog,
            Err(_) => Catalog::from_internet().await?,
        };

        let provider_manager = ProviderManager::from_config(provider_config)?;

        let selection = model_config
            .filter(|model| {
                catalog.has_model(&model.provider.to_string().to_lowercase(), &model.model)
            })
            .map(|model| provider_manager.select(model.provider, model.model))
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
            provider_manager,
            catalog,
            events,
            commands,
            session,
        })
    }

    pub async fn run(&mut self) -> Result<(), AppError> {
        let mut catalog_rx = Self::spawn_catalog_refresh();

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
                None => tokio::select! {
                    command = self.commands.recv() => match command {
                        Some(command) => command,
                        None => return Ok(()),
                    },
                    Some(catalog) = catalog_rx.recv() => {
                        self.catalog = catalog;
                        self.send_providers().await?;
                        continue;
                    }
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

    // Turn execution

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

    // Providers and models

    fn spawn_catalog_refresh() -> Receiver<Catalog> {
        let (catalog_tx, catalog_rx) = mpsc::channel(1);
        tokio::spawn(async move {
            tokio::select! {
                _ = catalog_tx.closed() => {}
                _ = async {
                    let mut tick = interval(Duration::from_secs(5 * 60));
                    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
                    tick.tick().await; // The initial catalog is already loaded.
                    loop {
                        tick.tick().await;
                        match Catalog::from_internet().await {
                            Ok(catalog) => {
                                if catalog_tx.send(catalog).await.is_err() {
                                    break;
                                }
                            }
                            Err(error) => tracing::warn!(%error, "Failed to refresh model catalog"),
                        }
                    }
                } => {}
            }
        });
        catalog_rx
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

    // Sessions

    pub(super) fn list_sessions(&self) -> Result<Vec<SessionSummary>, AppError> {
        let directory = self.session.log.path.parent().expect("session directory");
        let current = &self.session.log.path;

        let mut sessions = Vec::new();

        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();

            if !entry.file_type()?.is_file()
                || !id.starts_with("session-")
                || !id.ends_with(".jsonl")
            {
                continue;
            }

            let Ok(history) = crate::session::SessionData::load(entry.path()) else {
                continue; // skip damaged session
            };

            let Some(directory) = history.state.directory else {
                continue;
            };

            let title = history
                .state
                .title
                .unwrap_or_else(|| "Empty conversation".into());

            sessions.push(SessionSummary {
                id,
                title,
                directory,
                updated: history.updated,
                current: entry.path() == *current,
            });
        }

        sessions.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
        Ok(sessions)
    }

    pub(super) fn resume_session(&mut self, id: &str) -> Result<AppEvent, AppError> {
        let path = self.get_session_file(id)?;
        let mut session = Session::open(&path).map_err(AppError::Wal)?;

        if session.data.state.context.is_empty() {
            return Err(AppError::Wal(io::Error::other("Session is empty")));
        }

        let selection = session
            .data
            .state
            .model
            .clone()
            .map(|(provider, model)| self.provider_manager.select(provider, model))
            .transpose()?;

        let directory =
            session.data.state.directory.clone().ok_or_else(|| {
                AppError::Wal(io::Error::other("Session has no working directory"))
            })?;

        let previous_directory = std::env::current_dir()?;
        std::env::set_current_dir(&directory)?;

        // Recovery appends only interruption facts; it never invokes tools/models.
        let recovered: Result<Vec<SessionMessage>, io::Error> = (|| {
            session.recover()?;
            Ok::<_, io::Error>(Vec::<SessionMessage>::from(&session.data))
        })();

        let messages = match recovered {
            Ok(messages) => messages,
            Err(error) => {
                std::env::set_current_dir(previous_directory)?;
                return Err(AppError::Wal(error));
            }
        };

        let context_limit = selection.as_ref().and_then(|selection| {
            self.catalog
                .model(
                    &selection.provider().to_string().to_lowercase(),
                    selection.model(),
                )
                .and_then(|model| model.limit.context)
        });

        let model = session.data.state.model.clone();
        let cache_usage = CacheUsage::from(&session.data);
        self.selection = selection;
        self.session = session;

        Ok(AppEvent::SessionResumed {
            context_tokens: self.session.data.state.context_tokens,
            cache_usage,
            messages,
            directory,
            model,
            context_limit,
        })
    }

    pub(super) fn get_session_file(&self, id: &str) -> Result<PathBuf, AppError> {
        if Path::new(id).file_name().and_then(|name| name.to_str()) != Some(id)
            || !id.starts_with("session-")
            || !id.ends_with(".jsonl")
        {
            return Err(AppError::Wal(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid session ID",
            )));
        }

        let path = self
            .session
            .log
            .path
            .parent()
            .expect("session directory")
            .join(id);

        if path == self.session.log.path {
            return Err(AppError::Wal(io::Error::other(
                "This session is already active",
            )));
        }

        // Do not create missing sessions or follow links outside the store.
        let metadata = std::fs::symlink_metadata(&path)?;

        if !metadata.is_file() {
            return Err(AppError::Wal(io::Error::other(
                "Session is not a regular file",
            )));
        }

        Ok(path)
    }

    // Notifications

    async fn notice(&self, message: String) -> Result<(), AppError> {
        self.events.send(AppEvent::Notice(message)).await?;
        Ok(())
    }
}
