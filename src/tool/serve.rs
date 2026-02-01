//! Development server with file watching and automatic rebuilding.

mod server;
mod watcher;

use std::path::Path;
use std::sync::Arc;

use tokio::fs;
use tokio::sync::mpsc;

use crate::proc::template::PART_CONTEXT_PREFIX;
use crate::proc::{ContextValue, context_from_toml};
use crate::tool::procs::{ProcessorConfig, collect_assets, is_part, process_asset};
use crate::tool::{Config, ConfigProfile, DEFAULT_CONFIG_FILE, DEFAULT_CONFIG_PROFILE};

use std::collections::BTreeMap;

/// Runs the development server with file watching.
pub async fn run(port: u16, profile: Option<&str>) -> std::io::Result<()> {
    // Load and merge configuration.
    let config = load_config(profile).await?;
    let source_path = config.paths.get("source").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "missing paths.source in config",
        )
    })?;
    let target_path = config.paths.get("target").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "missing paths.target in config",
        )
    })?;

    let source = Path::new(source_path).to_path_buf();
    let target = Path::new(target_path).to_path_buf();

    // Run initial build.
    tracing::info!("Running initial build...");
    build(&source, &target, &config.procs, &config.context).await?;

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
        match build(&source, &target, &config.procs, &config.context).await {
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
    procs: &BTreeMap<String, ProcessorConfig>,
    context_values: &toml::Table,
) -> std::io::Result<()> {
    // Collect all assets from source directory.
    let mut assets = Vec::new();
    collect_assets(source, &mut assets).await?;

    // Create target directory if it doesn't exist.
    if !fs::try_exists(target).await? {
        fs::create_dir_all(target).await?;
    }

    // Build processing context.
    let mut proc_context = context_from_toml(context_values.clone()).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid context value: {:?}", e),
        )
    })?;

    // Add the asset source root path to context.
    proc_context.insert(
        "_asset_source_root".into(),
        ContextValue::Text(source.to_string_lossy().to_string().into()),
    );

    // Separate parts from regular assets and cache them in context.
    let mut regular_assets = Vec::new();
    for (relative_path, content) in assets {
        if is_part(&relative_path) {
            let part_key = format!("{}{}", PART_CONTEXT_PREFIX, relative_path);
            let content_str = String::from_utf8_lossy(&content).to_string();
            proc_context.insert(part_key.into(), ContextValue::Text(content_str.into()));
            tracing::debug!("Cached part: {}", relative_path);
        } else {
            regular_assets.push((relative_path, content));
        }
    }

    // Process each regular asset.
    let mut success_count = 0;
    let mut error_count = 0;
    for (relative_path, content) in regular_assets {
        let result = process_asset(&relative_path, content, procs, &proc_context, target).await;

        match result {
            Ok(()) => success_count += 1,
            Err(e) => {
                tracing::error!("Error processing {}: {}", relative_path, e);
                error_count += 1;
            }
        }
    }

    tracing::info!(
        "Processed {} assets ({} errors)",
        success_count,
        error_count
    );

    Ok(())
}
