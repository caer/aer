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
    Asset, AssetMetadata, Context, ContextValue, Environment, LayeredContext, MediaType,
    ProcessesAssets, ProcessingError,
    canonicalize::CanonicalizeProcessor,
    context_from_toml, extract_frontmatter,
    favicon::FaviconProcessor,
    image::ImageResizeProcessor,
    js_bundle::JsBundleProcessor,
    markdown::MarkdownProcessor,
    minify_html::MinifyHtmlProcessor,
    minify_js::MinifyJsProcessor,
    scss::ScssProcessor,
    template::{PART_CONTEXT_PREFIX, PART_DEFAULTS_PREFIX, TemplateProcessor},
};
use crate::tool::DEFAULT_CONFIG_FILE;
use crate::tool::kits::{self, ResolvedKit};
use crate::tool::{ToolConfig, ToolsMap, opengraph};

/// Path prefix used to identify parts to store in the processing context.
const PART_PATH_PREFIX: &str = "_";

/// Prefix used to store completed asset metadata in the processing context.
pub const ASSET_PATH_CONTEXT_KEY_PREFIX: &str = "_assets:";

/// Appends a value to the `_assets:` list at `key`, creating it if absent.
fn context_push_asset(context: &mut Context, key: codas::types::Text, value: ContextValue) {
    match context.get_mut(&key) {
        Some(ContextValue::List(items)) => {
            items.push(value);
        }
        _ => {
            context.insert(key, ContextValue::List(vec![value]));
        }
    }
}

/// Registers a part in the processing context by extracting its frontmatter
/// and storing the body and defaults under the appropriate prefix keys.
fn register_part(context: &mut Context, key: &str, content: &[u8]) {
    let content_str: codas::types::Text = String::from_utf8_lossy(content).to_string().into();
    let (body, defaults) = extract_frontmatter(&content_str);
    let part_key = format!("{}{}", PART_CONTEXT_PREFIX, key);
    context.insert(part_key.into(), ContextValue::Text(body.into()));
    if let Some(defaults) = defaults {
        let ctx_key = format!("{}{}", PART_DEFAULTS_PREFIX, key);
        context.insert(ctx_key.into(), ContextValue::Table(defaults));
    }
}

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
        &BuildConfig {
            source,
            target,
            procs: &config.procs,
            tools: &config.tools,
            clean_urls,
            resolved_kits: &resolved_kits,
            project_root: &loaded.config_dir,
        },
        &mut proc_context,
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

/// Parameters for [build_assets].
pub struct BuildConfig<'a> {
    pub source: &'a Path,
    pub target: &'a Path,
    pub procs: &'a BTreeMap<String, ProcessorConfig>,
    pub tools: &'a ToolsMap,
    pub clean_urls: bool,
    pub resolved_kits: &'a [ResolvedKit],
    pub project_root: &'a Path,
}

/// Collects, separates, and processes all assets from `source` into `target`.
///
/// Parts (files with `_`-prefixed path components) are cached in `context`
/// and the remaining assets are processed in a convergence loop: all assets
/// are processed in parallel, then reprocessed until outputs stabilize.
pub async fn build_assets(config: &BuildConfig<'_>, context: &mut Context) -> std::io::Result<()> {
    let source = config.source;
    let target = config.target;
    let procs = config.procs;
    let tools = config.tools;
    let clean_urls = config.clean_urls;
    let resolved_kits = config.resolved_kits;
    let project_root = config.project_root;
    let mut assets = Vec::new();
    collect_assets(source, &mut assets).await?;
    tracing::info!("Found {} assets", assets.len());

    if !fs::try_exists(target).await? {
        fs::create_dir_all(target).await?;
    }

    // Build the base environment for processors.
    let kit_imports: BTreeMap<String, PathBuf> = resolved_kits
        .iter()
        .map(|kit| (kit.name.clone(), kit.local_path.clone()))
        .collect();

    // Separate parts from regular assets and cache them in context.
    let mut regular_assets = Vec::new();
    let mut part_count = 0;
    for (relative_path, content) in assets {
        if is_part(&relative_path) {
            register_part(context, &relative_path, &content);
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
                let kit_relative = format!("{}/{}", kit.name, relative_path);
                register_part(context, &kit_relative, &content);
                tracing::debug!("Found kit part: {}", kit_relative);
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

    // Tool step: extract .aer.toml files, dispatch to tools,
    // and inject results into context before processing begins.
    let mut tool_files = Vec::new();
    regular_assets.retain(|(path, content)| {
        let filename = path.rsplit('/').next().unwrap_or(path);
        if filename.ends_with(".aer.toml") {
            tool_files.push((path.clone(), content.clone()));
            false
        } else {
            true
        }
    });

    // Resolved tool entries, keyed by _assets: key, for replay in the convergence loop.
    let mut tool_entries: Vec<(codas::types::Text, Vec<Context>)> = Vec::new();

    for (path, content) in &tool_files {
        let filename = path.rsplit('/').next().unwrap_or(path);
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");

        match opengraph::tool_for_filename(filename) {
            Some("opengraph") => {
                let content_str = String::from_utf8_lossy(content);
                let og_config = match tools.0.get("opengraph") {
                    Some(ToolConfig::OpenGraph(c)) => c.clone(),
                    _ => Default::default(),
                };

                match opengraph::resolve(&content_str, &og_config, project_root).await {
                    Ok(result) => {
                        tracing::info!("Resolved {} entries from {}", result.entries.len(), path);

                        let key: codas::types::Text =
                            format!("{}{}", ASSET_PATH_CONTEXT_KEY_PREFIX, dir).into();
                        for entry in &result.entries {
                            context_push_asset(
                                context,
                                key.clone(),
                                ContextValue::Table(entry.clone()),
                            );
                        }
                        tool_entries.push((key, result.entries));

                        // Inject vendored images as regular assets for processing.
                        for image in result.images {
                            regular_assets.push(image);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Tool failed for {}: {}", path, e);
                    }
                }
            }
            _ => {
                tracing::warn!("No tool found for {}", path);
            }
        }
    }

    // Process all assets in a convergence loop: process everything,
    // then reprocess until asset outputs stabilize.
    let procs = Arc::new(procs.clone());
    let target = Arc::new(target.to_path_buf());
    let mut asset_outputs: BTreeMap<String, String> = BTreeMap::new();
    let mut error_count;
    let max_passes = 10;

    for pass in 0..max_passes {
        let outputs_before = asset_outputs.clone();

        // Build an immutable environment snapshot for this pass.
        let env = Arc::new(Environment {
            source_root: source.to_path_buf(),
            kit_imports: kit_imports.clone(),
            asset_outputs: asset_outputs.clone(),
        });

        // Share the base context across all tasks via Arc.
        let shared_base: Arc<Context> = Arc::new(context.clone());
        let handles: Vec<_> = regular_assets
            .iter()
            .map(|(relative_path, content)| {
                let procs = Arc::clone(&procs);
                let base = Arc::clone(&shared_base);
                let env = Arc::clone(&env);
                let target = Arc::clone(&target);
                let path = relative_path.clone();
                let content = content.clone();
                tokio::spawn(async move {
                    let result =
                        process_asset(&path, content, &procs, &env, base, &target, clean_urls)
                            .await;
                    (path, result)
                })
            })
            .collect();

        let mut success_count = 0;
        error_count = 0;

        // Collect results and rebuild asset_outputs from scratch.
        asset_outputs.clear();
        let mut pass_results: Vec<(String, AssetMetadata)> = Vec::new();

        for handle in handles {
            match handle.await {
                Ok((path, Ok(result))) => {
                    success_count += 1;
                    asset_outputs.insert(path.clone(), result.output_path);
                    pass_results.push((path, result.metadata));
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

        // Rebuild _assets: context from scratch with this pass's results.
        context.retain(|key, _| !key.starts_with(ASSET_PATH_CONTEXT_KEY_PREFIX));

        // Re-inject saved tool entries (opengraph etc.) without re-resolving.
        for (key, entries) in &tool_entries {
            let items = entries.iter().cloned().map(ContextValue::Table).collect();
            context.insert(key.clone(), ContextValue::List(items));
        }

        // Insert processed asset metadata into context.
        for (path, metadata) in pass_results {
            let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let key: codas::types::Text =
                format!("{}{}", ASSET_PATH_CONTEXT_KEY_PREFIX, dir).into();
            context_push_asset(context, key, ContextValue::Table(metadata));
        }

        // Check if asset outputs changed during this pass.
        let converged = outputs_before == asset_outputs;

        tracing::info!(
            "Pass {}: processed {} assets ({} errors){}",
            pass + 1,
            success_count,
            error_count,
            if converged { " [converged]" } else { "" }
        );

        if converged && pass > 0 {
            break;
        }
    }

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
pub struct ProcessedAsset {
    pub output_path: String,
    pub metadata: AssetMetadata,
}

pub async fn process_asset(
    path: &str,
    content: Vec<u8>,
    procs: &BTreeMap<String, ProcessorConfig>,
    env: &Environment,
    base: Arc<Context>,
    target: &Path,
    clean_urls: bool,
) -> std::io::Result<ProcessedAsset> {
    let mut asset = Asset::new(path.into(), content);
    let mut context = LayeredContext::new(base);
    context.push_layer(); // asset-level overlay

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
    let mut first_pass = true;
    loop {
        if let Ok(text) = asset.as_text() {
            let (body, frontmatter) = extract_frontmatter(text);
            if let Some(parsed) = frontmatter {
                if first_pass {
                    context.extend_top(parsed);
                } else {
                    context.fill(parsed);
                }
                asset.replace_with_text(body.into(), asset.media_type().clone());
            }
        }
        first_pass = false;

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
                    match run_processor(proc_name, config, env, &context, &mut asset) {
                        Ok(true) => {
                            ran_processors.push(proc_name);
                        }
                        Ok(false) => {}
                        Err(e) => {
                            tracing::warn!("Processor `{}` failed on {}: {:?}", proc_name, path, e);
                        }
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

            // Apply the pattern's defaults as a layer below the asset overlay.
            // This gives us: asset > pattern defaults > global — structurally.
            let defaults_key: codas::types::Text =
                format!("{}{}", PART_DEFAULTS_PREFIX, pattern_path).into();
            if let Some(ContextValue::Table(defaults)) = context.get(&defaults_key) {
                let defaults = defaults.clone();
                context.insert_layer_below_top(defaults);
            }

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

        break;
    }

    // Perform phase two of processing (finalization).
    for proc_name in FINALIZATION_PROCESSORS {
        if let Some(config) = procs.get(*proc_name) {
            match run_processor(proc_name, config, env, &context, &mut asset) {
                Ok(true) => {
                    ran_processors.push(proc_name);
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!("Processor `{}` failed on {}: {:?}", proc_name, path, e);
                }
            }
        }
    }

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
    let source_dir = path.rsplit_once('/').map(|(dir, _)| dir);
    let target_dir = processed_path.rsplit_once('/').map(|(dir, _)| dir);
    let target_filename = processed_path.rsplit('/').next().unwrap_or(&processed_path);

    let display_target = if source_dir.is_some() && source_dir == target_dir {
        format!("/../{}", target_filename)
    } else {
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

    // Return only the page-level overlay as asset metadata.
    let page_overlay = context.pop_layer().unwrap_or_default();
    Ok(ProcessedAsset {
        output_path: processed_path,
        metadata: page_overlay,
    })
}

/// Runs a single processor against an asset.
///
/// Returns `(modified, result)` where `modified` is true if the
/// processor reported that it changed the asset.
pub fn run_processor(
    name: &str,
    config: &ProcessorConfig,
    env: &Environment,
    context: &LayeredContext,
    asset: &mut Asset,
) -> Result<bool, ProcessingError> {
    match name {
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
            Ok(false)
        }
    }
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
