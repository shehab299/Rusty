use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{
    Resource,
    trace::{Sampler, SdkTracerProvider},
};
use std::time::Duration;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{config::opentelemetry::OpenTelemetryConfig, error::AppError};

pub struct Telemetry {
    provider: SdkTracerProvider,
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        // Export remaining spans before exiting.
        let _ = self.provider.shutdown();
    }
}

/// Export only when enabled in application configuration. Never write to the TUI.
pub fn init(config: OpenTelemetryConfig) -> Result<Option<Telemetry>, AppError> {
    if !config.enabled {
        return Ok(None);
    }

    let service_name = config.service_name.clone();

    let filter = validate(&config)?;
    let provider = build_provider(config)?;
    let tracer = provider.tracer(service_name);

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .try_init()
        .map_err(|error| AppError::Telemetry(Box::new(error)))?;

    Ok(Some(Telemetry { provider }))
}

fn validate(config: &OpenTelemetryConfig) -> Result<EnvFilter, AppError> {
    if !(0.0..=1.0).contains(&config.sampling_ratio)
        || config.timeout_ms == 0
        || config.service_name.trim().is_empty()
        || config.endpoint.trim().is_empty()
    {
        return Err(AppError::Telemetry(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "opentelemetry requires sampling_ratio in [0, 1], timeout_ms > 0, and nonempty service_name and endpoint",
        ))));
    }
    EnvFilter::try_new(&config.filter).map_err(|error| AppError::Telemetry(Box::new(error)))
}

fn build_provider(config: OpenTelemetryConfig) -> Result<SdkTracerProvider, AppError> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(config.endpoint)
        .with_timeout(Duration::from_millis(config.timeout_ms))
        .with_headers(config.headers)
        .build()
        .map_err(|error| AppError::Telemetry(Box::new(error)))?;

    Ok(SdkTracerProvider::builder()
        .with_resource(
            Resource::builder_empty()
                .with_attributes(
                    config
                        .resource_attributes
                        .into_iter()
                        .map(|(key, value)| KeyValue::new(key, value)),
                )
                .with_service_name(config.service_name)
                .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
                .build(),
        )
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            config.sampling_ratio,
        ))))
        .with_batch_exporter(exporter)
        .build())
}

pub struct Operation {
    span: tracing::Span,
    outcome: &'static str,
}

impl Operation {
    pub fn new(span: tracing::Span) -> Self {
        Self {
            span,
            outcome: "cancelled",
        }
    }

    pub fn complete(&mut self, success: bool) {
        self.outcome = if success { "completed" } else { "error" };
        if !success {
            self.span.record("otel.status_code", "ERROR");
        }
    }

    pub fn shutdown(&mut self) {
        self.outcome = "shutdown";
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        self.span.record("outcome", self.outcome);
    }
}
