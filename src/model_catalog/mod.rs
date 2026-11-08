//! Fetch at startup and fall back to the last persisted catalog when offline.

use std::{path::Path, time::Duration};

use ai_sdk::providers::ModelError;

mod types;
pub use types::{Catalog, CatalogModel};

pub async fn load_catalog() -> Result<Catalog, ModelError> {
    let agent_root = crate::config::home::get_agent_root()?;
    let path = agent_root.join("cache/models.json");

    let fetched = async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        let bytes = client
            .get("https://models.dev/api.json")
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec();

        let catalog = serde_json::from_slice::<Catalog>(&bytes)?;
        Ok((catalog, bytes))
    }
    .await;

    match fetched {
        Ok((catalog, bytes)) => {
            if let Err(error) = persist_catalog(&path, &bytes).await {
                eprintln!("Could not persist model catalog: {error}");
            }
            Ok(catalog)
        }
        Err(error) => match read_persisted_catalog(&path).await {
            Ok(catalog) => {
                eprintln!("Could not fetch model catalog; using persisted data: {error}");
                Ok(catalog)
            }
            Err(_) => Err(error),
        },
    }
}

async fn read_persisted_catalog(path: &Path) -> Result<Catalog, ModelError> {
    let bytes = tokio::fs::read(path).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn persist_catalog(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = async {
        tokio::fs::write(&temporary, bytes).await?;
        tokio::fs::rename(&temporary, path).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(temporary).await;
    }
    result
}
