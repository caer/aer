use minify_js::{Session, TopLevelMode, minify};

use super::{Asset, MediaType, ProcessesAssets, ProcessingError};

/// Minifies JavaScript assets by removing unnecessary whitespace and comments.
///
/// Assets with paths ending in `.min.js` are skipped (already minified).
pub struct MinifyJsProcessor;

impl ProcessesAssets for MinifyJsProcessor {
    fn process(&self, asset: &mut Asset) -> Result<(), ProcessingError> {
        if asset.media_type() != &MediaType::JavaScript {
            tracing::debug!(
                "skipping asset {}: not JavaScript: {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        if asset.path().ends_with(".min.js") {
            tracing::debug!("skipping asset {}: already minified", asset.path());
            return Ok(());
        }

        let source = asset.as_bytes();
        let session = Session::new();
        let mut output = Vec::new();

        minify(&session, TopLevelMode::Global, source, &mut output).map_err(|e| {
            ProcessingError::Compilation {
                message: format!("JS minification failed: {:?}", e).into(),
            }
        })?;

        let minified = String::from_utf8(output).map_err(|e| ProcessingError::Malformed {
            message: e.to_string().into(),
        })?;
        asset.replace_with_text(minified.into(), MediaType::JavaScript);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minifies_js() {
        let js = r#"
            // This is a comment
            function hello(name) {
                /* Another comment */
                console.log("Hello, " + name);
            }
        "#;
        let mut asset = Asset::new("script.js".into(), js.as_bytes().to_vec());
        MinifyJsProcessor.process(&mut asset).unwrap();

        let result = asset.as_text().unwrap();
        // Comments should be stripped.
        assert!(!result.contains("// This is a comment"));
        assert!(!result.contains("/* Another comment */"));
        // Code should be preserved (minifier may convert to arrow function).
        assert!(result.contains("hello"));
        assert!(result.contains("console.log"));
        // Should be smaller than original.
        assert!(result.len() < js.len());
    }

    #[test]
    fn skips_non_js() {
        let mut asset = Asset::new("index.html".into(), b"<html></html>".to_vec());
        MinifyJsProcessor.process(&mut asset).unwrap();
        assert_eq!(asset.as_text().unwrap(), "<html></html>");
    }

    #[test]
    fn skips_already_minified() {
        let js = "function test(){console.log('already minified')}";
        let mut asset = Asset::new("vendor.min.js".into(), js.as_bytes().to_vec());
        MinifyJsProcessor.process(&mut asset).unwrap();
        // Content should be unchanged.
        assert_eq!(asset.as_text().unwrap(), js);
    }
}
