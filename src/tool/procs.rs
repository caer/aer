//! Runs asset processors based on a TOML configuration file.
//!
//! The `procs` command reads a TOML file containing processor definitions
//! and context values, then executes matching processors against all assets
//! in the source directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tokio::fs;

use serde::Deserialize;

use crate::proc::{
    Asset, Context, ContextValue, MediaType, ProcessesAssets, ProcessingError,
    canonicalize::CanonicalizeProcessor,
    image::ImageResizeProcessor,
    js_bundle::JsBundleProcessor,
    markdown::MarkdownProcessor,
    minify_html::MinifyHtmlProcessor,
    minify_js::MinifyJsProcessor,
    scss::ScssProcessor,
    template::{PART_CONTEXT_PREFIX, TemplateProcessor},
};
use crate::tool::{Config, DEFAULT_CONFIG_FILE, DEFAULT_CONFIG_PROFILE};

/// Path prefix used to identify parts to store in the processing context.
const PART_PATH_PREFIX: &str = "_";

/// Returns true if the path represents a part.
fn is_part(path: &str) -> bool {
    path.split(['/', '\\'])
        .any(|component| component.starts_with(PART_PATH_PREFIX))
}

/// Runs the procs command with the given configuration file and optional profile.
///
/// If `procs_file` is `None`, looks for `Aer.toml` in the current directory.
pub async fn run(procs_file: Option<&Path>, profile: Option<&str>) -> std::io::Result<()> {
    let config_path = procs_file.unwrap_or(Path::new(DEFAULT_CONFIG_FILE));
    let profile_name = profile.unwrap_or(DEFAULT_CONFIG_PROFILE);

    // Try to read and parse the configuration file.
    let config_toml = fs::read_to_string(config_path).await?;
    let config: Config = toml::from_str(&config_toml).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid TOML: {}", e),
        )
    })?;

    // Load the default profile.
    let default_profile = config.profiles.get(DEFAULT_CONFIG_PROFILE).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("missing default profile: {}", DEFAULT_CONFIG_PROFILE),
        )
    })?;

    // Merge the selected profile over the default.
    let config = if profile_name == DEFAULT_CONFIG_PROFILE {
        default_profile.clone()
    } else {
        let selected_profile = config.profiles.get(profile_name).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("missing selected profile: {}", profile_name),
            )
        })?;
        default_profile.merge(selected_profile)
    };

    // Validate source and target paths.
    let source_path = config.paths.get("source").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "missing paths.source in context",
        )
    })?;
    let target_path = config.paths.get("target").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "missing paths.target in context",
        )
    })?;

    tracing::info!("Processing assets from {} to {}", source_path, target_path);
    tracing::info!("Profile: {}", profile_name);
    tracing::debug!("Processors: {:?}", config.procs.keys().collect::<Vec<_>>());

    // Collect all assets from source directory.
    let source = Path::new(source_path);
    let target = Path::new(target_path);
    let mut assets = Vec::new();
    collect_assets(source, &mut assets).await?;
    tracing::info!("Found {} assets", assets.len());

    // Create target directory if it doesn't exist.
    if !fs::try_exists(target).await? {
        fs::create_dir_all(target).await?;
    }

    // Build processing context from config.
    let mut proc_context = Context::new();
    for (key, value) in config.context {
        proc_context.insert(key.clone().into(), ContextValue::Text(value.clone().into()));
    }

    // Add the asset source root path to context
    // for processors that need filesystem access.
    proc_context.insert(
        "_asset_source_root".into(),
        ContextValue::Text(source_path.clone().into()),
    );

    // Separate parts from regular assets and cache them in context.
    let mut regular_assets = Vec::new();
    let mut part_count = 0;
    for (relative_path, content) in assets {
        if is_part(&relative_path) {
            // Store part content in context (raw, without processing).
            let part_key = format!("{}{}", PART_CONTEXT_PREFIX, relative_path);
            let content_str = String::from_utf8_lossy(&content).to_string();
            proc_context.insert(part_key.into(), ContextValue::Text(content_str.into()));
            part_count += 1;
            tracing::debug!("Cached part: {}", relative_path);
        } else {
            regular_assets.push((relative_path, content));
        }
    }
    tracing::info!("Cached {} parts", part_count);

    // Process each regular asset.
    let mut success_count = 0;
    let mut error_count = 0;
    for (relative_path, content) in regular_assets {
        let result = process_asset(
            &relative_path,
            content,
            &config.procs,
            &proc_context,
            target,
        )
        .await;

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

/// Collects all assets from the source directory.
async fn collect_assets(root: &Path, assets: &mut Vec<(String, Vec<u8>)>) -> std::io::Result<()> {
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

/// Processors that run during phase one of asset processing.
const TRANSFORMATION_PROCESSORS: &[&str] = &["template", "markdown", "scss", "js_bundle", "image"];

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
async fn process_asset(
    path: &str,
    content: Vec<u8>,
    procs: &BTreeMap<String, ProcessorConfig>,
    context: &Context,
    target: &Path,
) -> std::io::Result<()> {
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
        let canonical_path = format!("{}/{}", root.trim_end_matches('/'), target_path,);
        context.insert("path".into(), ContextValue::Text(canonical_path.into()));
    }

    // Check if pattern processing is enabled.
    let pattern_enabled = procs.contains_key("pattern");

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
                    let result = run_processor(proc_name, config, &mut context, &mut asset);
                    if let Err(e) = result {
                        tracing::warn!("Processor `{}` failed on {}: {:?}", proc_name, path, e);
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
            tracing::debug!("Applying pattern {} to {}", pattern_path, path);
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
            let result = run_processor(proc_name, config, &mut context, &mut asset);
            if let Err(e) = result {
                tracing::warn!("Processor `{}` failed on {}: {:?}", proc_name, path, e);
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
    let processed_path = if let Some(dot_pos) = path.rfind('.') {
        format!("{}.{}", &path[..dot_pos], new_extension)
    } else {
        format!("{}.{}", path, new_extension)
    };
    let target_path = target.join(&processed_path);

    // Write the processed asset to target.
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(&target_path, asset.as_bytes()).await?;
    tracing::debug!("{} -> {}", path, processed_path);

    Ok(())
}

/// Runs a single processor against an asset.
fn run_processor(
    name: &str,
    config: &ProcessorConfig,
    context: &mut Context,
    asset: &mut Asset,
) -> Result<(), ProcessingError> {
    match name {
        "markdown" => MarkdownProcessor {}.process(context, asset),
        "template" => TemplateProcessor.process(context, asset),
        "canonicalize" => {
            let root = config.root.as_deref().unwrap_or("http://localhost/");
            if let Some(processor) = CanonicalizeProcessor::new(root) {
                processor.process(context, asset)
            } else {
                Err(ProcessingError::Malformed {
                    message: format!("invalid root URL: {}", root).into(),
                })
            }
        }
        "scss" => ScssProcessor {}.process(context, asset),
        "js_bundle" => {
            let minify = config.minify.unwrap_or(false);
            JsBundleProcessor::new(minify).process(context, asset)
        }
        "minify_html" => MinifyHtmlProcessor.process(context, asset),
        "minify_js" => MinifyJsProcessor.process(context, asset),
        "image" => {
            let width = config.max_width.unwrap_or(1920);
            let height = config.max_height.unwrap_or(1920);
            ImageResizeProcessor::new(width, height).process(context, asset)
        }
        _ => {
            tracing::warn!("Unknown processor: {}", name);
            Ok(())
        }
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
    fn merges_profiles() {
        let toml = r#"
[default.paths]
source = "site/"
target = "public/"

[default.procs]
canonicalize = { root = "http://localhost/" }
js_bundle = { minify = false }

[production.paths]
target = "dist/"

[production.procs]
canonicalize = { root = "https://prod.example.com/" }
js_bundle = { minify = true }
"#;
        let config: Config = toml::from_str(toml).unwrap();
        let default_profile = config.profiles.get("default").unwrap();
        let production_profile = config.profiles.get("production").unwrap();
        let merged = default_profile.merge(production_profile);

        // Paths should be merged (source from default, target from production).
        assert_eq!(merged.paths.get("source").unwrap(), "site/");
        assert_eq!(merged.paths.get("target").unwrap(), "dist/");
        // Procs should be merged with production overrides.
        let canonicalize = merged.procs.get("canonicalize").unwrap();
        assert_eq!(
            canonicalize.root,
            Some("https://prod.example.com/".to_string())
        );
        let js_bundle = merged.procs.get("js_bundle").unwrap();
        assert_eq!(js_bundle.minify, Some(true));
    }
}
