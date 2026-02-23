use minify_html_onepass::{Cfg, in_place};

use super::{Asset, Context, Environment, MediaType, ProcessesAssets, ProcessingError};

/// Minifies HTML assets by removing unnecessary whitespace and comments.
pub struct MinifyHtmlProcessor;

impl ProcessesAssets for MinifyHtmlProcessor {
    fn process(
        &self,
        _env: &Environment,
        _context: &mut Context,
        asset: &mut Asset,
    ) -> Result<(), ProcessingError> {
        if asset.media_type() != &MediaType::Html {
            return Ok(());
        }

        tracing::trace!("minify_html: {}", asset.path());

        let mut bytes = asset.as_bytes().to_vec();
        let cfg = Cfg {
            minify_css: true,
            minify_js: false,
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
    use std::path::PathBuf;

    use super::*;

    fn test_env() -> Environment {
        Environment {
            source_root: PathBuf::from("."),
            kit_imports: Default::default(),
        }
    }

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
        MinifyHtmlProcessor
            .process(&test_env(), &mut Context::default(), &mut asset)
            .unwrap();

        let result = asset.as_text().unwrap();
        assert!(!result.contains("<!--"));
        assert!(!result.contains("This is a comment"));
        assert!(result.contains("<p>"));
    }

    #[test]
    fn skips_non_html() {
        let mut asset = Asset::new("style.css".into(), b"body { }".to_vec());
        MinifyHtmlProcessor
            .process(&test_env(), &mut Context::default(), &mut asset)
            .unwrap();
        assert_eq!(asset.as_text().unwrap(), "body { }");
    }
}
