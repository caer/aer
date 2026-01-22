use codas::types::Text;
use lol_html::{element, rewrite_str, RewriteStrSettings};

use super::{Asset, MediaType, ProcessesAssets, ProcessingError};

/// Canonicalizes relative and absolute URL paths in HTML and CSS assets
/// by converting them to fully-qualified URLs based on a root parameter.
///
/// # Supported transformations
///
/// - Absolute paths (`/path/to/file`) → `{root}/path/to/file`
/// - Relative paths (`./file` or `../file`) → `{root}/file` (resolved)
/// - Bare paths (`file.css`) → `{root}/file.css`
///
/// # Unchanged URLs
///
/// - Already-qualified URLs (`https://...`, `http://...`)
/// - Protocol-relative URLs (`//example.com/...`)
/// - Data URIs (`data:...`)
/// - Fragment-only URLs (`#anchor`)
/// - JavaScript URLs (`javascript:...`)
/// - Mailto URLs (`mailto:...`)
///
/// # HTML processing
///
/// Processes URL-containing attributes (`href`, `src`, `action`, `poster`,
/// `data`, `cite`, `formaction`) and `url()` values in inline `style`
/// attributes. Content inside `<script>` tags is skipped.
///
/// # CSS processing
///
/// Processes `url()` values in stylesheets.
pub struct CanonicalizeProcessor {
    /// The root URL to prepend to relative/absolute paths.
    /// 
    /// Should include the protocol (e.g., `https://example.com`).
    root: Text,
}

impl CanonicalizeProcessor {
    /// Creates a new canonicalize processor with the given root URL.
    pub fn new(root: impl Into<Text>) -> Self {
        let root: Text = root.into();
        // Strip trailing slash for consistent joining.
        let root = if root.ends_with('/') {
            root.trim_end_matches('/').into()
        } else {
            root
        };
        Self { root }
    }

    /// Canonicalizes a URL, returning the transformed URL or the original
    /// if no transformation is needed.
    fn canonicalize_url(&self, url: &str) -> String {
        let url = url.trim();

        // Skip empty URLs.
        if url.is_empty() {
            return url.to_string();
        }

        // Skip already-qualified URLs.
        if url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("//")
            || url.starts_with("data:")
            || url.starts_with("javascript:")
            || url.starts_with("mailto:")
            || url.starts_with('#')
        {
            return url.to_string();
        }

        // Handle absolute paths (starting with /).
        if url.starts_with('/') {
            return format!("{}{}", self.root, url);
        }

        // Handle relative paths (starting with ./ or ../).
        // For simplicity, we just prepend root - a full implementation
        // would resolve .. segments based on the asset's path.
        if url.starts_with("./") {
            return format!("{}/{}", self.root, &url[2..]);
        } else if url.starts_with("../") {
            // Strip leading ../ segments - in a root context, they resolve to root.
            let mut path = url;
            while path.starts_with("../") {
                path = &path[3..];
            }
            return format!("{}/{}", self.root, path);
        }

        // Handle bare paths (no leading / or ./).
        format!("{}/{}", self.root, url)
    }

    /// Processes CSS content, canonicalizing all `url()` values.
    fn process_css(&self, css: &str) -> String {
        let mut result = String::with_capacity(css.len());
        let mut chars = css.char_indices().peekable();

        while let Some((i, c)) = chars.next() {
            // Look for "url(" pattern.
            if c == 'u' && css[i..].starts_with("url(") {
                result.push_str("url(");
                // Skip "url("
                chars.next(); // r
                chars.next(); // l
                chars.next(); // (

                // Skip whitespace.
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_whitespace() {
                        result.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }

                // Determine if quoted and extract URL.
                let quote_char = match chars.peek() {
                    Some(&(_, '"')) | Some(&(_, '\'')) => {
                        let q = chars.next().unwrap().1;
                        result.push(q);
                        Some(q)
                    }
                    _ => None,
                };

                // Extract the URL.
                let mut url = String::new();
                while let Some(&(_, c)) = chars.peek() {
                    if let Some(q) = quote_char {
                        if c == q {
                            break;
                        }
                    } else if c == ')' || c.is_whitespace() {
                        break;
                    }
                    url.push(c);
                    chars.next();
                }

                // Canonicalize and write the URL.
                result.push_str(&self.canonicalize_url(&url));

                // Write closing quote if present.
                if quote_char.is_some() {
                    if let Some(&(_, c)) = chars.peek() {
                        result.push(c);
                        chars.next();
                    }
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    /// Processes HTML content, canonicalizing URLs in attributes.
    fn process_html(&self, html: &str) -> Result<String, ProcessingError> {
        // Attributes that contain URLs.
        let url_attrs = ["href", "src", "action", "poster", "data", "cite", "formaction"];

        let processor = self;

        let result = rewrite_str(
            html,
            RewriteStrSettings {
                element_content_handlers: vec![
                    // Handle elements with URL attributes.
                    element!("*", |el| {
                        // For script elements, only process the src attribute.
                        // We skip processing inline JavaScript content (which lol_html
                        // wouldn't process via element handlers anyway).
                        if el.tag_name() == "script" {
                            if let Some(value) = el.get_attribute("src") {
                                let canonical = processor.canonicalize_url(&value);
                                if canonical != value {
                                    el.set_attribute("src", &canonical).ok();
                                }
                            }
                            return Ok(());
                        }

                        // Process URL attributes.
                        for attr in &url_attrs {
                            if let Some(value) = el.get_attribute(attr) {
                                let canonical = processor.canonicalize_url(&value);
                                if canonical != value {
                                    // Attribute names are known-valid, so this won't fail.
                                    el.set_attribute(attr, &canonical).ok();
                                }
                            }
                        }

                        // Process style attribute for url() values.
                        if let Some(style) = el.get_attribute("style") {
                            let canonical = processor.process_css(&style);
                            if canonical != style {
                                el.set_attribute("style", &canonical).ok();
                            }
                        }

                        Ok(())
                    }),
                ],
                ..Default::default()
            },
        )
        .map_err(|e| ProcessingError::Malformed {
            message: e.to_string().into(),
        })?;

        Ok(result)
    }
}

impl ProcessesAssets for CanonicalizeProcessor {
    fn process(&self, asset: &mut Asset) -> Result<(), ProcessingError> {
        match asset.media_type() {
            MediaType::Html => {
                let html = asset.as_text()?;
                let canonical = self.process_html(html)?;
                asset.replace_with_text(canonical.into(), MediaType::Html);
                Ok(())
            }
            MediaType::Css => {
                let css = asset.as_text()?;
                let canonical = self.process_css(css);
                asset.replace_with_text(canonical.into(), MediaType::Css);
                Ok(())
            }
            _ => {
                tracing::debug!(
                    "skipping asset {}: not HTML or CSS: {}",
                    asset.path(),
                    asset.media_type().name()
                );
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn processor() -> CanonicalizeProcessor {
        CanonicalizeProcessor::new("https://example.com")
    }

    #[test]
    fn canonicalizes_absolute_paths() {
        let p = processor();
        assert_eq!(
            p.canonicalize_url("/path/to/file.css"),
            "https://example.com/path/to/file.css"
        );
        assert_eq!(
            p.canonicalize_url("/images/logo.png"),
            "https://example.com/images/logo.png"
        );
    }

    #[test]
    fn canonicalizes_relative_paths() {
        let p = processor();
        assert_eq!(
            p.canonicalize_url("./styles.css"),
            "https://example.com/styles.css"
        );
        assert_eq!(
            p.canonicalize_url("../images/logo.png"),
            "https://example.com/images/logo.png"
        );
        assert_eq!(
            p.canonicalize_url("../../deep/file.js"),
            "https://example.com/deep/file.js"
        );
    }

    #[test]
    fn canonicalizes_bare_paths() {
        let p = processor();
        assert_eq!(
            p.canonicalize_url("styles.css"),
            "https://example.com/styles.css"
        );
        assert_eq!(
            p.canonicalize_url("images/logo.png"),
            "https://example.com/images/logo.png"
        );
    }

    #[test]
    fn preserves_qualified_urls() {
        let p = processor();
        assert_eq!(
            p.canonicalize_url("https://cdn.example.com/lib.js"),
            "https://cdn.example.com/lib.js"
        );
        assert_eq!(
            p.canonicalize_url("http://example.com/page"),
            "http://example.com/page"
        );
        assert_eq!(
            p.canonicalize_url("//cdn.example.com/lib.js"),
            "//cdn.example.com/lib.js"
        );
    }

    #[test]
    fn preserves_special_urls() {
        let p = processor();
        assert_eq!(p.canonicalize_url("#section"), "#section");
        assert_eq!(
            p.canonicalize_url("data:image/png;base64,abc"),
            "data:image/png;base64,abc"
        );
        assert_eq!(
            p.canonicalize_url("javascript:void(0)"),
            "javascript:void(0)"
        );
        assert_eq!(
            p.canonicalize_url("mailto:test@example.com"),
            "mailto:test@example.com"
        );
    }

    #[test]
    fn processes_css_urls() {
        let p = processor();
        let css = r#"
            .hero { background: url(/images/hero.jpg); }
            .icon { background-image: url("./icons/check.svg"); }
            .logo { background: url('logo.png') no-repeat; }
        "#;
        let result = p.process_css(css);
        assert!(result.contains("url(https://example.com/images/hero.jpg)"));
        assert!(result.contains("url(\"https://example.com/icons/check.svg\")"));
        assert!(result.contains("url('https://example.com/logo.png')"));
    }

    #[test]
    fn processes_html_attributes() {
        let p = processor();
        let html = r#"
            <a href="/about">About</a>
            <img src="./images/photo.jpg" alt="Photo">
            <link rel="stylesheet" href="styles.css">
            <script src="/app.js"></script>
        "#;
        let result = p.process_html(html).unwrap();
        assert!(result.contains(r#"href="https://example.com/about""#));
        assert!(result.contains(r#"src="https://example.com/images/photo.jpg""#));
        assert!(result.contains(r#"href="https://example.com/styles.css""#));
        // Script src should still be processed (the element content is skipped, not attributes).
        assert!(result.contains(r#"src="https://example.com/app.js""#));
    }

    #[test]
    fn processes_inline_styles() {
        let p = processor();
        let html = r#"<div style="background: url(/bg.png)">Content</div>"#;
        let result = p.process_html(html).unwrap();
        assert!(result.contains("url(https://example.com/bg.png)"));
    }

    #[test]
    fn handles_root_with_trailing_slash() {
        let p = CanonicalizeProcessor::new("https://example.com/");
        assert_eq!(
            p.canonicalize_url("/path"),
            "https://example.com/path"
        );
    }

    #[test]
    fn skips_non_html_css_assets() {
        let p = processor();
        let mut asset = Asset::new("script.js".into(), b"const x = '/api'".to_vec());
        p.process(&mut asset).unwrap();
        // Content should be unchanged.
        assert_eq!(asset.as_text().unwrap(), "const x = '/api'");
    }

    #[test]
    fn processes_html_asset() {
        let p = processor();
        let mut asset = Asset::new(
            "index.html".into(),
            b"<a href=\"/page\">Link</a>".to_vec(),
        );
        p.process(&mut asset).unwrap();
        assert!(asset
            .as_text()
            .unwrap()
            .contains("https://example.com/page"));
    }

    #[test]
    fn processes_css_asset() {
        let p = processor();
        let mut asset = Asset::new(
            "styles.css".into(),
            b".bg { background: url(/img.png); }".to_vec(),
        );
        p.process(&mut asset).unwrap();
        assert!(asset
            .as_text()
            .unwrap()
            .contains("https://example.com/img.png"));
    }
}
