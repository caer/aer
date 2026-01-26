//! File watching with debouncing for automatic rebuilds.

use std::path::Path;
use std::time::Duration;

use notify_debouncer_mini::notify::RecommendedWatcher;
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer, notify::RecursiveMode};
use tokio::sync::mpsc;

/// Paths to ignore when watching for changes.
const IGNORED_PATHS: &[&str] = &[".git", "target", ".DS_Store", "node_modules"];

/// Starts a file watcher on the given directory.
///
/// Returns a debouncer that must be kept alive for watching to continue.
/// Sends signals to `rebuild_tx` when changes are detected.
pub fn start(
    source: &Path,
    rebuild_tx: mpsc::Sender<()>,
) -> std::io::Result<Debouncer<RecommendedWatcher>> {
    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        move |result: DebounceEventResult| {
            match result {
                Ok(events) => {
                    // Filter out ignored paths.
                    let relevant_events: Vec<_> = events
                        .iter()
                        .filter(|event| {
                            let path_str = event.path.to_string_lossy();
                            !IGNORED_PATHS
                                .iter()
                                .any(|ignored| path_str.contains(ignored))
                        })
                        .collect();

                    if !relevant_events.is_empty() {
                        tracing::debug!(
                            "File changes detected: {:?}",
                            relevant_events
                                .iter()
                                .map(|e| e.path.display().to_string())
                                .collect::<Vec<_>>()
                        );
                        // Non-blocking send - if the channel is full, skip this signal.
                        let _ = rebuild_tx.try_send(());
                    }
                }
                Err(error) => {
                    tracing::warn!("Watch error: {:?}", error);
                }
            }
        },
    )
    .map_err(|e| std::io::Error::other(format!("watcher error: {}", e)))?;

    debouncer
        .watcher()
        .watch(source, RecursiveMode::Recursive)
        .map_err(|e| std::io::Error::other(format!("watch error: {}", e)))?;

    Ok(debouncer)
}
