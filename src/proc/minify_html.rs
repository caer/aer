use lol_html::{RewriteStrSettings, doc_comments, rewrite_str};

use super::{Asset, Environment, LayeredContext, MediaType, ProcessesAssets, ProcessingError};

/// Minifies HTML assets by removing comments.
///
/// @caer: todo: At the time of writing, no well-supported HTML minifier is
/// _compatible with the rest of our dependencies_ is available. Specifically,
/// `minify-html-onepass` isn't compatible. ;~;
pub struct MinifyHtmlProcessor;

impl ProcessesAssets for MinifyHtmlProcessor {
    fn process(
        &self,
        _env: &Environment,
        _context: &LayeredContext,
        asset: &mut Asset,
    ) -> Result<bool, ProcessingError> {
        if asset.media_type() != &MediaType::Html {
            return Ok(false);
        }

        tracing::trace!("minify_html: {}", asset.path());

        let html = asset.as_text()?;

        let minified = rewrite_str(
            html,
            RewriteStrSettings {
                document_content_handlers: vec![doc_comments!(|comment| {
                    comment.remove();
                    Ok(())
                })],
                strict: false,
                ..RewriteStrSettings::new()
            },
        )
        .map_err(|e| ProcessingError::Compilation {
            message: e.to_string().into(),
        })?;

        asset.replace_with_text(minified.into(), MediaType::Html);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::LayeredContext;

    #[test]
    fn removes_html_comments() {
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
            .process(
                &Environment::test(),
                &LayeredContext::from_flat(Default::default()),
                &mut asset,
            )
            .unwrap();

        let result = asset.as_text().unwrap();
        assert!(!result.contains("<!--"));
        assert!(!result.contains("This is a comment"));
        assert!(result.contains("<p>"));
    }

    #[test]
    fn skips_non_html() {
        let mut asset = Asset::new("style.css".into(), b"body { }".to_vec());
        let modified = MinifyHtmlProcessor
            .process(
                &Environment::test(),
                &LayeredContext::from_flat(Default::default()),
                &mut asset,
            )
            .unwrap();
        assert!(!modified);
        assert_eq!(asset.as_text().unwrap(), "body { }");
    }
}
