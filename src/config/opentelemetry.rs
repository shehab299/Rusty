//! Application-owned tracing settings. Headers may contain credentials; no Debug.

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub struct OpenTelemetryConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Required: trace URL, including `/v1/traces`.
    #[serde(deserialize_with = "deserialize_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_service_name")]
    pub service_name: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_sampling_ratio")]
    pub sampling_ratio: f64,
    #[serde(default = "default_filter")]
    pub filter: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub resource_attributes: HashMap<String, String>,
}

impl Default for OpenTelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            service_name: default_service_name(),
            timeout_ms: default_timeout_ms(),
            sampling_ratio: default_sampling_ratio(),
            filter: default_filter(),
            headers: HashMap::new(),
            resource_attributes: HashMap::new(),
        }
    }
}

fn default_service_name() -> String {
    "rusty".into()
}

fn default_timeout_ms() -> u64 {
    10_000
}

fn default_sampling_ratio() -> f64 {
    1.0
}

fn default_filter() -> String {
    "sdk=info".into()
}

fn deserialize_endpoint<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let endpoint = String::deserialize(deserializer)?;
    if endpoint.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "opentelemetry.endpoint must not be empty",
        ));
    }
    Ok(endpoint)
}
