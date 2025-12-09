use std::path::Path;

use brk_rolldown::{Bundler, BundlerOptions};
use brk_rolldown_common::Output;

use super::{Asset, MediaType, ProcessesAssets, ProcessingError};

/// Bundles JavaScript entry points and their dependencies into a single file.
///
/// This processor uses [rolldown](https://rolldown.rs) via
/// [brk_rolldown](https://crates.io/crates/brk_rolldown) to bundle
/// JavaScript modules, similar to tools like webpack or rollup.
///
/// Each asset passed to this processor is treated as a distinct entry point,
/// and modules are resolved relative to that entry point's location.
///
/// # Example
///
/// ```ignore
/// use aer::proc::js_bundle::JsBundleProcessor;
/// use aer::proc::{Asset, ProcessesAssets};
///
/// let processor = JsBundleProcessor::new();
/// let mut asset = Asset::new("src/index.js".into(), b"".to_vec());
/// processor.process(&mut asset).unwrap();
/// ```
pub struct JsBundleProcessor {
    /// Whether to minify the output.
    minify: bool,
}

impl JsBundleProcessor {
    /// Creates a new JS bundle processor
    pub fn new() -> Self {
        Self { minify: false }
    }

    /// Creates a new JS bundle processor with minification enabled
    pub fn with_minify(minify: bool) -> Self {
        Self { minify }
    }

    /// Bundles the JavaScript file at `entry_path` and returns the bundled code.
    ///
    /// Modules are resolved relative to the entry point's parent directory.
    fn bundle_js(&self, entry_path: &Path) -> Result<String, ProcessingError> {
        // Get the entry point filename for the bundler input.
        let file_name = entry_path
            .file_name()
            .ok_or_else(|| ProcessingError::Compilation {
                message: format!(
                    "Invalid entry path '{}': must be a file path, not a directory or root",
                    entry_path.display()
                )
                .into(),
            })?;

        // Use the entry point's parent directory as the working directory
        // for module resolution. Default to current directory if no parent.
        let cwd = entry_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf());

        let input_path = format!("./{}", file_name.to_string_lossy());

        // Create bundler options.
        let options = BundlerOptions {
            input: Some(vec![input_path.into()]),
            cwd,
            minify: if self.minify {
                Some(brk_rolldown::RawMinifyOptions::Bool(true))
            } else {
                None
            },
            ..Default::default()
        };

        // Create a new runtime for the async bundling operation.
        // Note: brk_rolldown is built on rolldown which uses async internally,
        // so we need a runtime to execute the bundling operation.
        let rt = tokio::runtime::Runtime::new().map_err(|e| ProcessingError::Compilation {
            message: format!("Failed to create async runtime: {}", e).into(),
        })?;

        // Run the bundler.
        rt.block_on(async {
            let mut bundler = Bundler::new(options).map_err(|e| ProcessingError::Compilation {
                message: format!("Failed to create bundler: {:?}", e).into(),
            })?;

            let output = bundler
                .generate()
                .await
                .map_err(|e| ProcessingError::Compilation {
                    message: format!("Bundling failed: {:?}", e).into(),
                })?;

            // Extract the bundled code from the first chunk.
            for asset in output.assets {
                if let Output::Chunk(chunk) = asset {
                    return Ok(chunk.code.clone());
                }
            }

            Err(ProcessingError::Compilation {
                message: "Bundling produced no output chunks".into(),
            })
        })
    }
}

impl ProcessesAssets for JsBundleProcessor {
    fn process(&self, asset: &mut Asset) -> Result<(), ProcessingError> {
        // Skip assets that aren't JavaScript.
        if *asset.media_type() != MediaType::JavaScript {
            tracing::debug!(
                "skipping asset {}: not JavaScript {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        // Get the path to the JavaScript entry point.
        let entry_path_str = asset.path().clone();
        let entry_path = Path::new(entry_path_str.as_str());

        // Bundle the JavaScript entry point.
        let bundled_code = self.bundle_js(entry_path)?;

        // Update the asset's contents with the bundled code.
        asset.replace_with_text(bundled_code.into(), MediaType::JavaScript);

        tracing::info!("Bundled JavaScript from: {}", entry_path.display());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_non_javascript_assets() {
        let processor = JsBundleProcessor::new();

        // Create a non-JavaScript asset.
        let mut css_asset = Asset::new("style.css".into(), "body {}".as_bytes().to_vec());

        // Processing should succeed (skip) without errors.
        let result = processor.process(&mut css_asset);
        assert!(result.is_ok());
    }

    #[test]
    fn bundles_javascript() {
        let processor = JsBundleProcessor::new();

        // Create a JavaScript asset pointing to our test file.
        let mut js_asset = Asset::new("test/js_bundle/entry.js".into(), "".as_bytes().to_vec());

        // Process the asset.
        let result = processor.process(&mut js_asset);
        assert!(result.is_ok());

        // Check that the bundled code contains content from the entry point
        // and the imported modules.
        let bundled = js_asset.as_text().unwrap();
        assert!(bundled.contains("Hello from bundled JavaScript!"));
        assert!(bundled.contains("greet"));
        assert!(bundled.contains("HELPER_VERSION"));
        assert!(bundled.contains("formatMessage"));
    }
}
