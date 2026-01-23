use codas::types::Text;
use lol_html::{RewriteStrSettings, element, rewrite_str};
use url::Url;

use super::{Asset, MediaType, ProcessesAssets, ProcessingError};

/// Canonicalizes relative and absolute URL paths in HTML assets
/// by converting them to fully-qualified URLs based on a root parameter.
///
/// Absolute paths are canonicalized relative to `root`:
/// `/path/to/file` becomes `{root}/path/to/file`.
///
/// Relative paths (e.g., `./file`, `../file`, or `file`) are canonicalized
/// relative to `root` and the source asset's declared path. For example,
/// given an asset `/path/to/file.html` containing a URL `../styles.css`,
/// the final canonicalized URL would be `{root}/path/styles.css`.
///
/// URLs within `<script>` tags are not processed. Fully-qualified URLs
/// (like `https://localhost`) and special URLs (`data:`, `javascript:`,
/// `mailto:`, `#anchor`) are not processed.
pub struct CanonicalizeProcessor {
    /// The root URL to use as the base for canonicalization.
    ///
    /// Should include the protocol (e.g., `https://example.com`).
    root: Url,
}

impl CanonicalizeProcessor {
    /// Creates a new canonicalize processor with the given root URL.
    ///
    /// Returns `None` if `root` is not a valid URL.
    pub fn new(root: impl AsRef<str>) -> Option<Self> {
        // Ensure root ends with / for proper URL joining.
        let mut root_str = root.as_ref().to_string();
        if !root_str.ends_with('/') {
            root_str.push('/');
        }
        let root = Url::parse(&root_str).ok()?;
        Some(Self { root })
    }

    /// Canonicalizes a URL relative to the given asset path.
    ///
    /// Returns the transformed URL or the original if no transformation is needed.
    fn canonicalize_url(&self, url: &str, asset_path: &str) -> String {
        let url = url.trim();

        // Skip empty URLs.
        if url.is_empty() {
            return url.to_string();
        }

        // Skip already-qualified URLs and special schemes.
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

        // Resolve absolute paths (starting with /) directly against root.
        if let Some(stripped) = url.strip_prefix('/') {
            return self
                .root
                .join(stripped)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| url.to_string());
        }

        // Resolve relative paths against root and the asset directory.
        let asset_dir = asset_path
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .unwrap_or("");
        let base = if asset_dir.is_empty() {
            self.root.clone()
        } else {
            let dir = asset_dir.trim_start_matches('/');
            self.root
                .join(&format!("{}/", dir))
                .unwrap_or_else(|_| self.root.clone())
        };

        // Resolve the relative URL against the base.
        base.join(url)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| url.to_string())
    }

    /// Processes CSS content, canonicalizing all `url()` values.
    fn process_css(&self, css: &str, asset_path: &str) -> String {
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
                result.push_str(&self.canonicalize_url(&url, asset_path));

                // Write closing quote if present.
                if quote_char.is_some()
                    && let Some(&(_, c)) = chars.peek()
                {
                    result.push(c);
                    chars.next();
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    /// Processes HTML content, canonicalizing URLs in attributes.
    fn process_html(&self, html: &str, asset_path: &Text) -> Result<String, ProcessingError> {
        let url_attrs = [
            "href",
            "src",
            "action",
            "poster",
            "data",
            "cite",
            "formaction",
        ];
        let path = asset_path.clone();

        rewrite_str(
            html,
            RewriteStrSettings {
                element_content_handlers: vec![element!("*", move |el| {
                    // For script elements, only process the src attribute.
                    if el.tag_name() == "script" {
                        if let Some(value) = el.get_attribute("src") {
                            let canonical = self.canonicalize_url(&value, &path);
                            if canonical != value {
                                el.set_attribute("src", &canonical).ok();
                            }
                        }
                        return Ok(());
                    }

                    for attr in &url_attrs {
                        if let Some(value) = el.get_attribute(attr) {
                            let canonical = self.canonicalize_url(&value, &path);
                            if canonical != value {
                                el.set_attribute(attr, &canonical).ok();
                            }
                        }
                    }

                    if let Some(style) = el.get_attribute("style") {
                        let canonical = self.process_css(&style, &path);
                        if canonical != style {
                            el.set_attribute("style", &canonical).ok();
                        }
                    }

                    Ok(())
                })],
                ..Default::default()
            },
        )
        .map_err(|e| ProcessingError::Malformed {
            message: e.to_string().into(),
        })
    }
}

impl ProcessesAssets for CanonicalizeProcessor {
    fn process(&self, asset: &mut Asset) -> Result<(), ProcessingError> {
        if asset.media_type() != &MediaType::Html {
            tracing::debug!(
                "skipping asset {}: not HTML: {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        let canonical = self.process_html(asset.as_text()?, asset.path())?;
        asset.replace_with_text(canonical.into(), MediaType::Html);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn processor() -> CanonicalizeProcessor {
        CanonicalizeProcessor::new("https://example.com").unwrap()
    }

    #[test]
    fn canonicalizes_absolute_paths() {
        let p = processor();
        // Absolute paths ignore asset path.
        assert_eq!(
            p.canonicalize_url("/path/to/file.css", "/some/asset.html"),
            "https://example.com/path/to/file.css"
        );
        assert_eq!(
            p.canonicalize_url("/images/logo.png", "/deep/nested/page.html"),
            "https://example.com/images/logo.png"
        );
    }

    #[test]
    fn canonicalizes_relative_paths_with_asset_context() {
        let p = processor();

        // ./file from /path/to/file.html -> /path/to/file
        assert_eq!(
            p.canonicalize_url("./styles.css", "/path/to/file.html"),
            "https://example.com/path/to/styles.css"
        );

        // ../file from /path/to/file.html -> /path/file
        assert_eq!(
            p.canonicalize_url("../styles.css", "/path/to/file.html"),
            "https://example.com/path/styles.css"
        );

        // ../../file from /path/to/deep/file.html -> /path/file
        assert_eq!(
            p.canonicalize_url("../../styles.css", "/path/to/deep/file.html"),
            "https://example.com/path/styles.css"
        );

        // bare path from /path/to/file.html -> /path/to/file
        assert_eq!(
            p.canonicalize_url("styles.css", "/path/to/file.html"),
            "https://example.com/path/to/styles.css"
        );
    }

    #[test]
    fn canonicalizes_from_root_asset() {
        let p = processor();

        // Relative from root-level asset.
        assert_eq!(
            p.canonicalize_url("./styles.css", "index.html"),
            "https://example.com/styles.css"
        );
        assert_eq!(
            p.canonicalize_url("styles.css", "index.html"),
            "https://example.com/styles.css"
        );
    }

    #[test]
    fn preserves_qualified_urls() {
        let p = processor();
        assert_eq!(
            p.canonicalize_url("https://cdn.example.com/lib.js", "/any/path.html"),
            "https://cdn.example.com/lib.js"
        );
        assert_eq!(
            p.canonicalize_url("http://example.com/page", "/any/path.html"),
            "http://example.com/page"
        );
        assert_eq!(
            p.canonicalize_url("//cdn.example.com/lib.js", "/any/path.html"),
            "//cdn.example.com/lib.js"
        );
    }

    #[test]
    fn preserves_special_urls() {
        let p = processor();
        assert_eq!(p.canonicalize_url("#section", "/any/path.html"), "#section");
        assert_eq!(
            p.canonicalize_url("data:image/png;base64,abc", "/any/path.html"),
            "data:image/png;base64,abc"
        );
        assert_eq!(
            p.canonicalize_url("javascript:void(0)", "/any/path.html"),
            "javascript:void(0)"
        );
        assert_eq!(
            p.canonicalize_url("mailto:test@example.com", "/any/path.html"),
            "mailto:test@example.com"
        );
    }

    #[test]
    fn processes_html_attributes() {
        let p = processor();
        let html = r#"
            <a href="/about">About</a>
            <img src="./images/photo.jpg" alt="Photo">
            <link rel="stylesheet" href="../styles.css">
            <script src="/app.js"></script>
        "#;
        let result = p.process_html(html, &"/path/to/page.html".into()).unwrap();
        assert!(result.contains(r#"href="https://example.com/about""#));
        assert!(result.contains(r#"src="https://example.com/path/to/images/photo.jpg""#));
        assert!(result.contains(r#"href="https://example.com/path/styles.css""#));
        assert!(result.contains(r#"src="https://example.com/app.js""#));
    }

    #[test]
    fn processes_inline_styles() {
        let p = processor();
        let html = r#"<div style="background: url(../bg.png)">Content</div>"#;
        let result = p.process_html(html, &"/path/to/page.html".into()).unwrap();
        assert!(result.contains("url(https://example.com/path/bg.png)"));
    }

    #[test]
    fn handles_root_with_trailing_slash() {
        let p = CanonicalizeProcessor::new("https://example.com/").unwrap();
        assert_eq!(
            p.canonicalize_url("/path", "/index.html"),
            "https://example.com/path"
        );
    }

    #[test]
    fn skips_non_html_assets() {
        let p = processor();
        let mut asset = Asset::new("script.js".into(), b"const x = '/api'".to_vec());
        p.process(&mut asset).unwrap();
        assert_eq!(asset.as_text().unwrap(), "const x = '/api'");
    }

    #[test]
    fn processes_html_asset_with_path() {
        let p = processor();
        let mut asset = Asset::new(
            "/blog/posts/article.html".into(),
            b"<a href=\"../index.html\">Back</a>".to_vec(),
        );
        p.process(&mut asset).unwrap();
        assert!(
            asset
                .as_text()
                .unwrap()
                .contains("https://example.com/blog/index.html")
        );
    }
}
