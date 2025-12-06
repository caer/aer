use std::path::{Path, PathBuf};

use brk_rolldown::{Bundler, BundlerOptions};
use brk_rolldown_common::Output;

use super::{Asset, MediaType, ProcessesAssets, ProcessingError};

/// A processor that bundles JavaScript entry points
/// and their dependencies into a single file.
///
/// This processor uses [rolldown](https://rolldown.rs) via
/// [brk_rolldown](https://crates.io/crates/brk_rolldown) to bundle
/// JavaScript modules, similar to tools like webpack or rollup.
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
    /// Optional working directory for module resolution.
    /// If not set, uses the parent directory of the entry point.
    cwd: Option<PathBuf>,

    /// Whether to minify the output.
    minify: bool,
}

impl Default for JsBundleProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl JsBundleProcessor {
    /// Creates a new JS bundle processor with default settings.
    pub fn new() -> Self {
        Self {
            cwd: None,
            minify: false,
        }
    }

    /// Sets the working directory for module resolution.
    pub fn with_cwd(mut self, cwd: impl AsRef<Path>) -> Self {
        self.cwd = Some(cwd.as_ref().to_path_buf());
        self
    }

    /// Enables minification of the bundled output.
    pub fn with_minify(mut self, minify: bool) -> Self {
        self.minify = minify;
        self
    }

    /// Bundles the JavaScript file at `entry_path` and returns the bundled code.
    fn bundle_js(&self, entry_path: &Path) -> Result<String, ProcessingError> {
        // Determine working directory.
        let cwd = self.cwd.clone().or_else(|| {
            entry_path.parent().map(|p| p.to_path_buf())
        });

        // Convert entry path to a relative path if we have a cwd.
        let input_path = if let Some(ref cwd) = cwd {
            entry_path
                .strip_prefix(cwd)
                .map(|p| format!("./{}", p.display()))
                .unwrap_or_else(|_| entry_path.display().to_string())
        } else {
            entry_path.display().to_string()
        };

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
        let rt = tokio::runtime::Runtime::new().map_err(|e| ProcessingError::Compilation {
            message: format!("Failed to create async runtime: {}", e).into(),
        })?;

        // Run the bundler.
        rt.block_on(async {
            let mut bundler = Bundler::new(options).map_err(|e| ProcessingError::Compilation {
                message: format!("Failed to create bundler: {:?}", e).into(),
            })?;

            let output = bundler.generate().await.map_err(|e| ProcessingError::Compilation {
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

        tracing::info!(
            "Bundled JavaScript from: {}",
            entry_path.display()
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_processor() {
        let processor = JsBundleProcessor::new()
            .with_cwd("/tmp/project")
            .with_minify(true);

        assert_eq!(processor.cwd.unwrap().to_str().unwrap(), "/tmp/project");
        assert!(processor.minify);
    }

    #[test]
    fn creates_default_processor() {
        let processor = JsBundleProcessor::default();

        assert!(processor.cwd.is_none());
        assert!(!processor.minify);
    }

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
        let mut js_asset = Asset::new(
            "test/js_bundle/entry.js".into(),
            "".as_bytes().to_vec(),
        );

        // Process the asset.
        let result = processor.process(&mut js_asset);
        assert!(result.is_ok());

        // Check that the bundled code contains our original content.
        let bundled = js_asset.as_text().unwrap();
        assert!(bundled.contains("Hello from bundled JavaScript!"));
        assert!(bundled.contains("greet"));
    }
}
