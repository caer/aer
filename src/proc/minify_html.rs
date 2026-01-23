use minify_html_onepass::{Cfg, in_place};

use super::{Asset, MediaType, ProcessesAssets, ProcessingError};

/// Minifies HTML assets by removing unnecessary whitespace and comments.
pub struct MinifyHtmlProcessor;

impl ProcessesAssets for MinifyHtmlProcessor {
    fn process(&self, asset: &mut Asset) -> Result<(), ProcessingError> {
        if asset.media_type() != &MediaType::Html {
            tracing::debug!(
                "skipping asset {}: not HTML: {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        let mut bytes = asset.as_bytes().to_vec();
        let cfg = Cfg {
            minify_css: true,
            minify_js: true,
        };

        match in_place(&mut bytes, &cfg) {
            Ok(len) => {
                bytes.truncate(len);
                let minified =
                    String::from_utf8(bytes).map_err(|e| ProcessingError::Malformed {
                        message: e.to_string().into(),
                    })?;
                asset.replace_with_text(minified.into(), MediaType::Html);
                Ok(())
            }
            Err(e) => Err(ProcessingError::Compilation {
                message: format!("HTML minification failed at byte {}", e.position).into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minifies_html() {
        let html = r#"
            <!DOCTYPE html>
            <html>
                <head>
                    <title>Test</title>
                </head>
                <body>
                    <!-- This is a comment -->
                    <p>Hello,   world!</p>
                </body>
            </html>
        "#;
        let mut asset = Asset::new("test.html".into(), html.as_bytes().to_vec());
        MinifyHtmlProcessor.process(&mut asset).unwrap();

        let result = asset.as_text().unwrap();
        assert!(!result.contains("<!--"));
        assert!(!result.contains("This is a comment"));
        assert!(result.contains("<p>"));
    }

    #[test]
    fn skips_non_html() {
        let mut asset = Asset::new("style.css".into(), b"body { }".to_vec());
        MinifyHtmlProcessor.process(&mut asset).unwrap();
        assert_eq!(asset.as_text().unwrap(), "body { }");
    }
}
