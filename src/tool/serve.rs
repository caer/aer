//! Development server with file watching and automatic rebuilding.

mod server;
mod watcher;

use std::path::Path;
use std::sync::Arc;

use tokio::fs;
use tokio::sync::mpsc;

use crate::proc::context_from_toml;
use crate::tool::procs::{ProcessorConfig, build_assets};
use crate::tool::{Config, ConfigProfile, DEFAULT_CONFIG_FILE, DEFAULT_CONFIG_PROFILE};

/// Runs the development server with file watching.
pub async fn run(port: u16, profile: Option<&str>) -> std::io::Result<()> {
    // Load and merge configuration.
    let config = load_config(profile).await?;
    let source_path = config.paths.source.as_ref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "missing paths.source in config",
        )
    })?;
    let target_path = config.paths.target.as_ref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "missing paths.target in config",
        )
    })?;

    let source = Path::new(source_path).to_path_buf();
    let target = Path::new(target_path).to_path_buf();

    let clean_urls = config.paths.clean_urls.unwrap_or(false);

    // Run initial build.
    tracing::info!("Running initial build...");
    build(&source, &target, &config.procs, &config.context, clean_urls).await?;

    // Create channel for rebuild signals.
    let (rebuild_tx, mut rebuild_rx) = mpsc::channel::<()>(1);

    // Start file watcher.
    let watcher_source = source.clone();
    let _watcher = watcher::start(&watcher_source, rebuild_tx)?;
    tracing::info!("Watching {} for changes", source.display());

    // Start HTTP server in background.
    let server_target = target.clone();
    let server_handle = tokio::spawn(async move {
        if let Err(e) = server::start(port, &server_target).await {
            tracing::error!("Server error: {}", e);
        }
    });

    tracing::info!("Server running at http://localhost:{}", port);

    // Watch for rebuild signals.
    let config = Arc::new(config);
    while rebuild_rx.recv().await.is_some() {
        tracing::info!("Change detected, rebuilding...");
        match build(&source, &target, &config.procs, &config.context, clean_urls).await {
            Ok(()) => tracing::info!("Rebuild complete"),
            Err(e) => tracing::error!("Rebuild failed: {}", e),
        }
    }

    // Wait for server to finish (it won't unless there's an error).
    let _ = server_handle.await;

    Ok(())
}

/// Loads and merges configuration from Aer.toml.
async fn load_config(profile: Option<&str>) -> std::io::Result<ConfigProfile> {
    let config_path = Path::new(DEFAULT_CONFIG_FILE);
    let profile_name = profile.unwrap_or(DEFAULT_CONFIG_PROFILE);

    let config_toml = fs::read_to_string(config_path).await?;
    let config: Config = toml::from_str(&config_toml).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid TOML: {}", e),
        )
    })?;

    let default_profile = config.profiles.get(DEFAULT_CONFIG_PROFILE).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("missing default profile: {}", DEFAULT_CONFIG_PROFILE),
        )
    })?;

    if profile_name == DEFAULT_CONFIG_PROFILE {
        Ok(default_profile.clone())
    } else {
        let selected_profile = config.profiles.get(profile_name).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("missing selected profile: {}", profile_name),
            )
        })?;
        Ok(default_profile.merge(selected_profile))
    }
}

/// Builds all assets from source to target.
async fn build(
    source: &Path,
    target: &Path,
    procs: &std::collections::BTreeMap<String, ProcessorConfig>,
    context_values: &toml::Table,
    clean_urls: bool,
) -> std::io::Result<()> {
    let mut context = context_from_toml(context_values.clone()).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid context value: {:?}", e),
        )
    })?;

    build_assets(source, target, procs, &mut context, clean_urls).await
}
