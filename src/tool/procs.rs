//! Runs asset processors based on a TOML configuration file.
//!
//! The `procs` command reads a TOML file containing processor definitions
//! and context values, then executes matching processors against all assets
//! in the source directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::fs;

use serde::Deserialize;

use crate::proc::{
    Asset, Context, ContextValue, Environment, MediaType, ProcessesAssets, ProcessingError,
    canonicalize::CanonicalizeProcessor,
    context_from_toml,
    favicon::FaviconProcessor,
    image::ImageResizeProcessor,
    js_bundle::JsBundleProcessor,
    markdown::MarkdownProcessor,
    minify_html::MinifyHtmlProcessor,
    minify_js::MinifyJsProcessor,
    scss::ScssProcessor,
    template::{PART_CONTEXT_PREFIX, TemplateProcessor},
};
use crate::tool::DEFAULT_CONFIG_FILE;
use crate::tool::kits::{self, ResolvedKit};

/// Path prefix used to identify parts to store in the processing context.
const PART_PATH_PREFIX: &str = "_";

/// Prefix used to store completed asset metadata in the processing context.
pub const ASSET_PATH_CONTEXT_KEY_PREFIX: &str = "_assets:";

/// Returns true if the path represents a part.
pub fn is_part(path: &str) -> bool {
    path.split(['/', '\\'])
        .any(|component| component.starts_with(PART_PATH_PREFIX))
}

/// Runs the procs command with the given configuration file and optional profile.
///
/// If `procs_file` is `None`, looks for `Aer.toml` in the current directory.
pub async fn run(procs_file: Option<&Path>, profile: Option<&str>) -> std::io::Result<()> {
    let config_path = procs_file.unwrap_or(Path::new(DEFAULT_CONFIG_FILE));
    let loaded = crate::tool::load_config(config_path, profile).await?;
    let config = loaded.profile;

    let resolved_kits = kits::resolve_kits(&loaded.kits, &loaded.config_dir).await?;

    // Validate source and target paths.
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

    tracing::info!("Processing assets from {} to {}", source_path, target_path);
    tracing::debug!("Processors: {:?}", config.procs.keys().collect::<Vec<_>>());

    let source = Path::new(source_path);
    let target = Path::new(target_path);
    let clean_urls = config.paths.clean_urls.unwrap_or(false);

    let mut proc_context = context_from_toml(config.context).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid context value: {:?}", e),
        )
    })?;

    build_assets(
        source,
        target,
        &config.procs,
        &mut proc_context,
        clean_urls,
        &resolved_kits,
    )
    .await
}

/// Collects all assets from the source directory.
pub async fn collect_assets(
    root: &Path,
    assets: &mut Vec<(String, Vec<u8>)>,
) -> std::io::Result<()> {
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];

    while let Some(current) = stack.pop() {
        let mut entries = fs::read_dir(&current).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = fs::metadata(&path).await?;

            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() {
                let relative = path.strip_prefix(root).map_err(std::io::Error::other)?;
                let relative_str = relative.to_string_lossy().to_string();
                let content = fs::read(&path).await?;
                assets.push((relative_str, content));
            }
        }
    }

    Ok(())
}

/// Collects, separates, and processes all assets from `source` into `target`.
///
/// Parts (files with `_`-prefixed path components) are cached in `context`
/// and the remaining assets are processed in parallel passes. Assets that
/// return [ProcessingError::Deferred] are retried with an enriched context
/// until all complete or a cycle is detected.
pub async fn build_assets(
    source: &Path,
    target: &Path,
    procs: &BTreeMap<String, ProcessorConfig>,
    context: &mut Context,
    clean_urls: bool,
    resolved_kits: &[ResolvedKit],
) -> std::io::Result<()> {
    // Collect all assets from source directory.
    let mut assets = Vec::new();
    collect_assets(source, &mut assets).await?;
    tracing::info!("Found {} assets", assets.len());

    // Create target directory if it doesn't exist.
    if !fs::try_exists(target).await? {
        fs::create_dir_all(target).await?;
    }

    // Build the environment for processors that need filesystem state.
    let env = Arc::new(Environment {
        source_root: source.to_path_buf(),
        kit_imports: resolved_kits
            .iter()
            .map(|kit| (kit.name.clone(), kit.local_path.clone()))
            .collect(),
    });

    // Separate parts from regular assets and cache them in context.
    let mut regular_assets = Vec::new();
    let mut part_count = 0;
    for (relative_path, content) in assets {
        if is_part(&relative_path) {
            let part_key = format!("{}{}", PART_CONTEXT_PREFIX, relative_path);
            let content_str = String::from_utf8_lossy(&content).to_string();
            context.insert(part_key.into(), ContextValue::Text(content_str.into()));
            part_count += 1;
            tracing::debug!("Found part: {}", relative_path);
        } else {
            regular_assets.push((relative_path, content));
        }
    }
    tracing::info!("Found {} parts", part_count);

    // Collect and integrate kit assets.
    let mut kit_asset_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let project_asset_paths: std::collections::BTreeSet<String> =
        regular_assets.iter().map(|(p, _)| p.clone()).collect();

    for kit in resolved_kits {
        let mut kit_assets = Vec::new();
        collect_assets(&kit.local_path, &mut kit_assets).await?;

        tracing::info!("Kit `{}`: found {} assets", kit.name, kit_assets.len());

        // Pre-canonicalize kit assets.
        let canonicalized = kits::pre_canonicalize_kit_assets(&kit_assets, &kit.dest);

        let dest_trimmed = kit.dest.trim_start_matches('/');

        for (relative_path, content) in canonicalized {
            // Compute the output path for this kit asset.
            let output_path = if dest_trimmed.is_empty() {
                relative_path.clone()
            } else {
                format!("{}/{}", dest_trimmed, &relative_path)
            };

            if is_part(&relative_path) {
                // Store kit parts as {kit_name}/{path} for template resolution.
                let part_key = format!("{}{}/{}", PART_CONTEXT_PREFIX, kit.name, relative_path);
                let content_str = String::from_utf8_lossy(&content).to_string();
                context.insert(part_key.into(), ContextValue::Text(content_str.into()));
                tracing::debug!("Found kit part: {}/{}", kit.name, relative_path);
            } else {
                // Collision detection.
                if project_asset_paths.contains(&output_path) {
                    tracing::warn!(
                        "Kit `{}` asset `{}` collides with project asset at `{}`",
                        kit.name,
                        relative_path,
                        output_path
                    );
                }
                if kit_asset_paths.contains(&output_path) {
                    tracing::warn!(
                        "Kit `{}` asset `{}` collides with another kit asset at `{}`",
                        kit.name,
                        relative_path,
                        output_path
                    );
                }
                kit_asset_paths.insert(output_path.clone());
                regular_assets.push((output_path, content));
            }
        }
    }

    // Seed empty asset lists for every path that contains
    // regular assets. This lets path queries distinguish "path
    // exists but nothing has completed yet" (Deferred) from
    // "path does not exist" (error).
    for (relative_path, _) in &regular_assets {
        let dir = relative_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let key: codas::types::Text = format!("{}{}", ASSET_PATH_CONTEXT_KEY_PREFIX, dir).into();
        context
            .entry(key)
            .or_insert_with(|| ContextValue::List(vec![]));
    }

    // Process assets in parallel passes.
    let procs = Arc::new(procs.clone());
    let target = Arc::new(target.to_path_buf());
    let mut pending_assets = regular_assets;
    let mut passes_without_progress = 0;
    let mut success_count = 0;
    let mut error_count = 0;
    loop {
        if pending_assets.is_empty() {
            break;
        }

        let prev_pending_assets = pending_assets.len();
        let shared_context = Arc::new(context.clone());
        let handles: Vec<_> = pending_assets
            .iter()
            .map(|(relative_path, content)| {
                let procs = Arc::clone(&procs);
                let ctx = Arc::clone(&shared_context);
                let env = Arc::clone(&env);
                let target = Arc::clone(&target);
                let path = relative_path.clone();
                let content = content.clone();
                tokio::spawn(async move {
                    let result =
                        process_asset(&path, content, &procs, &env, &ctx, &target, clean_urls)
                            .await;
                    (path, result)
                })
            })
            .collect();

        let mut deferred_paths: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();

        for handle in handles {
            match handle.await {
                Ok((path, Ok(ProcResult::Complete { context: asset_ctx }))) => {
                    success_count += 1;

                    // Group the completed asset's context by directory
                    // so that path queries can iterate it.
                    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                    let key: codas::types::Text =
                        format!("{}{}", ASSET_PATH_CONTEXT_KEY_PREFIX, dir).into();
                    match context.get_mut(&key) {
                        Some(ContextValue::List(items)) => {
                            items.push(ContextValue::Table(asset_ctx));
                        }
                        _ => {
                            context.insert(
                                key,
                                ContextValue::List(vec![ContextValue::Table(asset_ctx)]),
                            );
                        }
                    }
                }
                Ok((path, Ok(ProcResult::Deferred))) => {
                    deferred_paths.insert(path);
                }
                Ok((path, Err(e))) => {
                    tracing::error!("Error processing {}: {}", path, e);
                    error_count += 1;
                }
                Err(e) => {
                    tracing::error!("Task panicked: {}", e);
                    error_count += 1;
                }
            }
        }

        pending_assets.retain(|(path, _)| deferred_paths.contains(path));

        if pending_assets.is_empty() {
            break;
        }

        // Track consecutive passes where no asset completed.
        // If N assets are all deferred and none complete
        // after N passes, they depend on each other cyclically.
        if pending_assets.len() < prev_pending_assets {
            passes_without_progress = 0;
        } else {
            passes_without_progress += 1;
        }
        if passes_without_progress > pending_assets.len() {
            for (path, _) in &pending_assets {
                tracing::error!("Asset stuck in deferral cycle: {}", path);
            }
            error_count += pending_assets.len();
            break;
        }

        tracing::debug!("{} assets deferred, retrying", pending_assets.len());
    }

    tracing::info!(
        "Processed {} assets ({} errors)",
        success_count,
        error_count
    );

    Ok(())
}

/// Processors that run during phase one of asset processing.
const TRANSFORMATION_PROCESSORS: &[&str] = &[
    "template",
    "markdown",
    "scss",
    "js_bundle",
    "image",
    "favicon",
];

/// Processors that run in phase two of asset processing.
const FINALIZATION_PROCESSORS: &[&str] = &["canonicalize", "minify_html", "minify_js"];

/// Processes a single asset through all matching processors.
///
/// Asset processing is divided into two phases: Content
/// transformation, and content finalization.
///
/// Transformation processors are run in a loop until the
/// asset's media type stabilizes. After all transformations,
/// if a pattern is specified in the context, the content
/// is wrapped in the pattern and processing continutes recursively.
///
/// Finalization processors are run once after all transformations.
pub async fn process_asset(
    path: &str,
    content: Vec<u8>,
    procs: &BTreeMap<String, ProcessorConfig>,
    env: &Environment,
    context: &Context,
    target: &Path,
    clean_urls: bool,
) -> std::io::Result<ProcResult> {
    let mut asset = Asset::new(path.into(), content);
    let mut context = context.clone();

    // If canonicalization is enabled, add the asset's canonical
    // path to the processing context.
    if let Some(config) = procs.get("canonicalize")
        && let Some(root) = &config.root
    {
        // @caer: fixme: This logic is brittle. We should have some way
        //        to predict the target path based on the final applicable
        //        processor.
        let target_path = if path.ends_with(".md") {
            path.trim_end_matches(".md").to_string() + ".html"
        } else {
            path.to_string()
        };

        // With clean URLs, canonical paths omit the .html extension.
        let canonical_target = if clean_urls && target_path.ends_with(".html") {
            rewrite_clean_url_canonical(&target_path)
        } else {
            target_path
        };

        let canonical_path = format!("{}/{}", root.trim_end_matches('/'), canonical_target);
        context.insert("path".into(), ContextValue::Text(canonical_path.into()));
    }

    // Check if pattern processing is enabled.
    let pattern_enabled = procs.contains_key("pattern");

    // Track which processors modified the asset.
    let mut ran_processors: Vec<&str> = Vec::new();

    // Perform phase one of processing (transformation and pattern wrapping).
    loop {
        // Run transformation processors until media type stabilizes.
        let mut processed_types: Vec<MediaType> = Vec::new();
        loop {
            let current_type = asset.media_type().clone();

            // If we've already processed this type, we're done.
            if processed_types.contains(&current_type) {
                break;
            }
            processed_types.push(current_type.clone());

            // Run transformation processors in order.
            for proc_name in TRANSFORMATION_PROCESSORS {
                if let Some(config) = procs.get(*proc_name) {
                    let (modified, result) =
                        run_processor(proc_name, config, env, &mut context, &mut asset);
                    match result {
                        Err(ProcessingError::Deferred) => {
                            return Ok(ProcResult::Deferred);
                        }
                        Err(e) => {
                            tracing::warn!("Processor `{}` failed on {}: {:?}", proc_name, path, e);
                        }
                        Ok(()) if modified => {
                            ran_processors.push(proc_name);
                        }
                        Ok(()) => {}
                    }
                }
            }

            // If the media type changed, loop again to run processors for the new type.
            if asset.media_type() == &current_type {
                break;
            }
        }

        // If pattern processing is enabled and a pattern is specified,
        // wrap the current content in the pattern and re-process recursively.
        if pattern_enabled
            && let Some(ContextValue::Text(pattern_path)) = context.remove(&"pattern".into())
        {
            // Store current content in context for pattern to use.
            let content = asset.as_text().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "pattern wrapping requires text content",
                )
            })?;
            context.insert("content".into(), ContextValue::Text(content.clone()));

            // Look up the pattern content from parts.
            let part_key: codas::types::Text =
                format!("{}{}", PART_CONTEXT_PREFIX, pattern_path).into();
            let pattern_content = match context.get(&part_key) {
                Some(ContextValue::Text(content)) => content.clone(),
                _ => {
                    tracing::warn!("Pattern not found: {}", pattern_path);
                    break;
                }
            };

            // Determine media type from pattern extension.
            let pattern_media_type = pattern_path
                .rsplit('.')
                .next()
                .map(MediaType::from_extension)
                .unwrap_or(MediaType::Html);

            // Create a new asset from the pattern content, preserving
            // the original asset path.
            ran_processors.push("pattern");
            asset = Asset::new(path.into(), pattern_content.as_bytes().to_vec());
            asset.set_media_type(pattern_media_type);

            // Continue loop to process the pattern recursively.
            continue;
        }

        // No pattern found, exit the loop.
        break;
    }

    // Perform phase two of processing (finalization).
    for proc_name in FINALIZATION_PROCESSORS {
        if let Some(config) = procs.get(*proc_name) {
            let (modified, result) =
                run_processor(proc_name, config, env, &mut context, &mut asset);
            match result {
                Err(ProcessingError::Deferred) => {
                    return Ok(ProcResult::Deferred);
                }
                Err(e) => {
                    tracing::warn!("Processor `{}` failed on {}: {:?}", proc_name, path, e);
                }
                Ok(()) if modified => {
                    ran_processors.push(proc_name);
                }
                Ok(()) => {}
            }
        }
    }

    // Determine the assets' extension based on media type.
    let new_extension = asset
        .media_type()
        .extensions()
        .first()
        .expect("all media types have at least one extension");

    // Replace the existing extension.
    let mut processed_path = if let Some(dot_pos) = path.rfind('.') {
        format!("{}.{}", &path[..dot_pos], new_extension)
    } else {
        format!("{}.{}", path, new_extension)
    };

    // With clean URLs, rewrite slug.html to slug/index.html.
    if clean_urls && new_extension == "html" {
        processed_path = rewrite_clean_url_path(&processed_path);
    }

    let target_path = target.join(&processed_path);

    // Write the processed asset to target.
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(&target_path, asset.as_bytes()).await?;

    // Log processing summary.
    // Truncate target path if only the filename/extension changed (not the directory).
    let source_dir = path.rsplit_once('/').map(|(dir, _)| dir);
    let target_dir = processed_path.rsplit_once('/').map(|(dir, _)| dir);
    let target_filename = processed_path.rsplit('/').next().unwrap_or(&processed_path);

    let display_target = if source_dir.is_some() && source_dir == target_dir {
        // Same directory, truncate to /../filename
        format!("/../{}", target_filename)
    } else {
        // Different directory or root-level file, show full path
        format!("/{}", processed_path)
    };

    if ran_processors.is_empty() {
        tracing::debug!("COPY /{} -> {}", path, display_target);
    } else {
        tracing::debug!(
            "PROC /{} -> [{}] -> {}",
            path,
            ran_processors.join(", "),
            display_target
        );
    }

    Ok(ProcResult::Complete { context })
}

/// Runs a single processor against an asset.
///
/// Returns `(modified, result)` where `modified` is true if the
/// processor changed the asset's content or media type.
pub fn run_processor(
    name: &str,
    config: &ProcessorConfig,
    env: &Environment,
    context: &mut Context,
    asset: &mut Asset,
) -> (bool, Result<(), ProcessingError>) {
    // Capture state before processing.
    let before_type = asset.media_type().clone();
    let before_len = asset.as_bytes().len();

    let result = match name {
        "markdown" => MarkdownProcessor {}.process(env, context, asset),
        "template" => TemplateProcessor.process(env, context, asset),
        "favicon" => FaviconProcessor.process(env, context, asset),
        "canonicalize" => {
            let root = config.root.as_deref().unwrap_or("http://localhost/");
            if let Some(processor) = CanonicalizeProcessor::new(root) {
                processor.process(env, context, asset)
            } else {
                Err(ProcessingError::Malformed {
                    message: format!("invalid root URL: {}", root).into(),
                })
            }
        }
        "scss" => ScssProcessor {}.process(env, context, asset),
        "js_bundle" => {
            let minify = config.minify.unwrap_or(false);
            JsBundleProcessor::new(minify).process(env, context, asset)
        }
        "minify_html" => MinifyHtmlProcessor.process(env, context, asset),
        "minify_js" => MinifyJsProcessor.process(env, context, asset),
        "image" => {
            let width = config.max_width.unwrap_or(1920);
            let height = config.max_height.unwrap_or(1920);
            ImageResizeProcessor::new(width, height).process(env, context, asset)
        }
        _ => {
            tracing::warn!("Unknown processor: {}", name);
            Ok(())
        }
    };

    // Check if asset was modified.
    let modified = result.is_ok()
        && (asset.media_type() != &before_type || asset.as_bytes().len() != before_len);

    (modified, result)
}

/// Rewrites an HTML output path for clean URLs.
/// `slug.html` becomes `slug/index.html`; `index.html` is unchanged.
fn rewrite_clean_url_path(path: &str) -> String {
    let filename = path.rsplit('/').next().unwrap_or(path);
    if filename != "index.html" {
        let stem = &path[..path.len() - ".html".len()];
        format!("{}/index.html", stem)
    } else {
        path.to_string()
    }
}

/// Computes the canonical URL suffix for clean URLs.
/// `slug.html` becomes `slug/`; `index.html` becomes empty;
/// `dir/index.html` becomes `dir/`.
fn rewrite_clean_url_canonical(path: &str) -> String {
    let filename = path.rsplit('/').next().unwrap_or(path);
    if filename == "index.html" {
        path[..path.len() - "index.html".len()].to_string()
    } else {
        path[..path.len() - ".html".len()].to_string() + "/"
    }
}

/// The outcome of processing a single asset.
pub enum ProcResult {
    /// The asset was processed successfully.
    Complete { context: Context },

    /// The asset cannot complete until other assets finish processing.
    Deferred,
}

/// Configuration for a single processor.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct ProcessorConfig {
    // canonicalize options
    root: Option<String>,
    // js_bundle options
    minify: Option<bool>,
    // image options
    max_width: Option<u32>,
    max_height: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_parts() {
        // Files starting with underscore are parts.
        assert!(is_part("_header.html"));
        assert!(is_part("_parts/footer.html"));
        assert!(is_part("templates/_layout.html"));
        assert!(is_part("a/b/_c/d.html"));

        // Regular files are not parts.
        assert!(!is_part("index.html"));
        assert!(!is_part("pages/about.html"));
        assert!(!is_part("my_file.html")); // underscore in middle, not at start of component
    }

    #[test]
    fn rewrites_clean_url_paths() {
        // Non-index HTML files are rewritten.
        assert_eq!(rewrite_clean_url_path("about.html"), "about/index.html");
        assert_eq!(
            rewrite_clean_url_path("blog/post.html"),
            "blog/post/index.html"
        );

        // Index files are unchanged.
        assert_eq!(rewrite_clean_url_path("index.html"), "index.html");
        assert_eq!(rewrite_clean_url_path("blog/index.html"), "blog/index.html");
    }

    #[test]
    fn rewrites_clean_url_canonicals() {
        // Non-index HTML files get a trailing slash.
        assert_eq!(rewrite_clean_url_canonical("about.html"), "about/");
        assert_eq!(rewrite_clean_url_canonical("blog/post.html"), "blog/post/");

        // Root index.html becomes empty (root of site).
        assert_eq!(rewrite_clean_url_canonical("index.html"), "");

        // Nested index.html becomes directory path.
        assert_eq!(rewrite_clean_url_canonical("blog/index.html"), "blog/");
    }
}
