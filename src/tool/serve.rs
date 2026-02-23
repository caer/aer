//! Development server with file watching and automatic rebuilding.

mod server;
mod watcher;

use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::proc::context_from_toml;
use crate::tool::DEFAULT_CONFIG_FILE;
use crate::tool::kits::{self, ResolvedKit};
use crate::tool::procs::{ProcessorConfig, build_assets};

/// Runs the development server with file watching.
pub async fn run(port: u16, profile: Option<&str>) -> std::io::Result<()> {
    let config_path = Path::new(DEFAULT_CONFIG_FILE);
    let loaded = crate::tool::load_config(config_path, profile).await?;
    let config = loaded.profile;

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

    // Resolve kits once at startup.
    let resolved_kits = kits::resolve_kits(&loaded.kits, &loaded.config_dir).await?;

    // Run initial build.
    tracing::info!("Running initial build...");
    build(
        &source,
        &target,
        &config.procs,
        &config.context,
        clean_urls,
        &resolved_kits,
    )
    .await?;

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
        match build(
            &source,
            &target,
            &config.procs,
            &config.context,
            clean_urls,
            &resolved_kits,
        )
        .await
        {
            Ok(()) => tracing::info!("Rebuild complete"),
            Err(e) => tracing::error!("Rebuild failed: {}", e),
        }
    }

    // Wait for server to finish (it won't unless there's an error).
    let _ = server_handle.await;

    Ok(())
}

/// Builds all assets from source to target.
async fn build(
    source: &Path,
    target: &Path,
    procs: &std::collections::BTreeMap<String, ProcessorConfig>,
    context_values: &toml::Table,
    clean_urls: bool,
    resolved_kits: &[ResolvedKit],
) -> std::io::Result<()> {
    let mut context = context_from_toml(context_values.clone()).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid context value: {:?}", e),
        )
    })?;

    build_assets(
        source,
        target,
        procs,
        &mut context,
        clean_urls,
        resolved_kits,
    )
    .await
}
