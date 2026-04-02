use lol_html::{RewriteStrSettings, doc_comments, doc_text, rewrite_str};

use super::{Asset, Environment, LayeredContext, MediaType, ProcessesAssets, ProcessingError};

/// Minifies HTML assets by removing comments and whitespace-only text nodes.
///
/// @caer: todo: At the time of writing, no well-supported HTML minifier
/// _compatible with the rest of our dependencies_ is available. Specifically,
/// `minify-html-onepass` isn't compatible. ;~;
///
/// Therefore, this is a conservative minifier. A full minifier would also collapse runs of
/// whitespace within text nodes (e.g. `"hello   world"` → `"hello world"`),
/// since browsers already collapse them during rendering. However, doing so
/// correctly requires tracking ancestor context to preserve whitespace in
/// elements like `<pre>`, `<textarea>`, `<script>`, and `<style>`, as well as
/// any element styled with `white-space: pre`. lol_html's streaming model does
/// not expose the ancestor chain for text chunks, and detecting CSS-driven
/// `white-space` at the HTML level is not feasible.
///
/// Rather than risk corrupting preformatted content, we limit ourselves to
/// safe, context-free transformations: stripping comments and removing text
/// nodes that contain only whitespace (inter-tag indentation and blank lines).
pub struct MinifyHtmlProcessor;

impl ProcessesAssets for MinifyHtmlProcessor {
    fn process(
        &self,
        _env: &Environment,
        _context: &LayeredContext,
        asset: &mut Asset,
    ) -> Result<(), ProcessingError> {
        if asset.media_type() != &MediaType::Html {
            return Ok(());
        }

        tracing::trace!("minify_html: {}", asset.path());

        let html = asset.as_text()?;

        let minified = rewrite_str(
            html,
            RewriteStrSettings {
                document_content_handlers: vec![
                    doc_comments!(|comment| {
                        comment.remove();
                        Ok(())
                    }),
                    doc_text!(|text| {
                        if text.as_str().trim().is_empty() {
                            text.remove();
                        }
                        Ok(())
                    }),
                ],
                strict: false,
                ..RewriteStrSettings::new()
            },
        )
        .map_err(|e| ProcessingError::Compilation {
            message: e.to_string().into(),
        })?;

        asset.replace_with_text(minified.into(), MediaType::Html);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::LayeredContext;

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
        MinifyHtmlProcessor
            .process(
                &Environment::test(),
                &LayeredContext::from_flat(Default::default()),
                &mut asset,
            )
            .unwrap();
        assert_eq!(asset.as_text().unwrap(), "body { }");
    }
}
