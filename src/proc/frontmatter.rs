use codas::types::Text;
use toml::Value;

use super::{Asset, Context, ContextValue, MediaCategory, ProcessesAssets, ProcessingError};

const FRONTMATTER_DELIMITER: &str = "***";

/// Extracts TOML frontmatter from text assets into a processing context.
///
/// Text contains valid frontmatter if it begins with valid TOML content
/// followed by `***` on its own line. The processor strips the frontmatter
/// (including the delimiter) from the asset and writes parsed values to
/// the shared context.
///
/// # Example
///
/// Given this HTML asset:
///
/// ```html
/// title = "Example Page"
/// tags = ["rust", "web"]
///
/// ***
///
/// <h1>Hello, world!</h1>
/// ```
///
/// The processor extracts `title` and `tags` into the context and emits:
///
/// ```html
///
/// <h1>Hello, world!</h1>
/// ```
pub struct FrontmatterProcessor;

impl FrontmatterProcessor {
    /// Parses TOML content into context values.
    fn parse_toml(content: &str) -> Result<Context, ProcessingError> {
        let table: toml::Table =
            toml::from_str(content).map_err(|e| ProcessingError::Malformed {
                message: format!("invalid TOML frontmatter: {}", e).into(),
            })?;

        let mut context = Context::default();
        for (key, value) in table {
            let ctx_value = Self::toml_to_context_value(&value)?;
            context.insert(key.into(), ctx_value);
        }
        Ok(context)
    }

    /// Converts a TOML value to a context value.
    fn toml_to_context_value(value: &Value) -> Result<ContextValue, ProcessingError> {
        match value {
            Value::String(s) => Ok(ContextValue::Text(s.clone().into())),
            Value::Integer(n) => Ok(ContextValue::Text(n.to_string().into())),
            Value::Float(n) => Ok(ContextValue::Text(n.to_string().into())),
            Value::Boolean(b) => Ok(ContextValue::Text(b.to_string().into())),
            Value::Array(arr) => {
                let items: Result<Vec<Text>, _> = arr
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => Ok(s.clone().into()),
                        Value::Integer(n) => Ok(n.to_string().into()),
                        Value::Float(n) => Ok(n.to_string().into()),
                        Value::Boolean(b) => Ok(b.to_string().into()),
                        _ => Err(ProcessingError::Malformed {
                            message: "frontmatter arrays may only contain scalar values".into(),
                        }),
                    })
                    .collect();
                Ok(ContextValue::List(items?))
            }
            Value::Table(_) => Err(ProcessingError::Malformed {
                message: "nested tables in frontmatter are not supported".into(),
            }),
            Value::Datetime(dt) => Ok(ContextValue::Text(dt.to_string().into())),
        }
    }
}

impl ProcessesAssets for FrontmatterProcessor {
    fn process(&self, context: &mut Context, asset: &mut Asset) -> Result<(), ProcessingError> {
        if asset.media_type().category() != MediaCategory::Text {
            tracing::debug!(
                "skipping asset {}: not text: {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        let content = asset.as_text()?;

        // Look for the frontmatter delimiter on its own line.
        let delimiter_pattern = format!("\n{}\n", FRONTMATTER_DELIMITER);
        let split_pos = if content.starts_with(&format!("{}\n", FRONTMATTER_DELIMITER)) {
            // Edge case: file starts with delimiter (empty frontmatter).
            Some(0)
        } else {
            content.find(&delimiter_pattern)
        };

        // No frontmatter found - nothing to do.
        let Some(pos) = split_pos else {
            tracing::debug!("no frontmatter found in asset {}", asset.path());
            return Ok(());
        };

        // Split into frontmatter and body.
        let frontmatter = &content[..pos];
        let body_start = if pos == 0 {
            FRONTMATTER_DELIMITER.len() + 1 // Skip "***\n"
        } else {
            pos + delimiter_pattern.len() - 1 // Skip "\n***\n", keep trailing newline context
        };
        let body = &content[body_start..];

        // Try to parse the frontmatter as TOML.
        // If parsing fails, treat it as no frontmatter (*** might just be in regular content).
        let parsed = match Self::parse_toml(frontmatter) {
            Ok(ctx) => ctx,
            Err(_) => {
                tracing::debug!(
                    "content before *** in {} is not valid TOML, skipping",
                    asset.path()
                );
                return Ok(());
            }
        };

        // Merge parsed values into the shared context.
        context.extend(parsed);

        // Replace asset content with body only.
        asset.replace_with_text(body.into(), asset.media_type().clone());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::Asset;

    fn get_text(ctx: &Context, key: &str) -> Option<String> {
        let key: Text = key.into();
        match ctx.get(&key) {
            Some(ContextValue::Text(t)) => Some(t.to_string()),
            _ => None,
        }
    }

    fn get_list(ctx: &Context, key: &str) -> Option<Vec<String>> {
        let key: Text = key.into();
        match ctx.get(&key) {
            Some(ContextValue::List(items)) => Some(items.iter().map(|t| t.to_string()).collect()),
            _ => None,
        }
    }

    #[test]
    fn extracts_frontmatter() {
        let content = r#"title = "Hello"
author = "Test"

***

<h1>Content</h1>"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        FrontmatterProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(get_text(&ctx, "title"), Some("Hello".to_string()));
        assert_eq!(get_text(&ctx, "author"), Some("Test".to_string()));

        let body = asset.as_text().unwrap();
        assert!(!body.contains("title"));
        assert!(!body.contains("***"));
        assert!(body.contains("<h1>Content</h1>"));
    }

    #[test]
    fn extracts_arrays() {
        let content = r#"tags = ["rust", "web", "cli"]

***

Body"#;
        let mut asset = Asset::new("page.md".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        FrontmatterProcessor.process(&mut ctx, &mut asset).unwrap();

        let tags = get_list(&ctx, "tags").expect("expected list");
        assert_eq!(tags, vec!["rust", "web", "cli"]);
    }

    #[test]
    fn handles_no_frontmatter() {
        let content = "<h1>No frontmatter here</h1>";
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        FrontmatterProcessor.process(&mut ctx, &mut asset).unwrap();

        assert!(ctx.is_empty());
        assert_eq!(asset.as_text().unwrap(), content);
    }

    #[test]
    fn handles_various_types() {
        let content = r#"name = "test"
count = 42
ratio = 3.14
enabled = true

***

Body"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        FrontmatterProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(get_text(&ctx, "name"), Some("test".to_string()));
        assert_eq!(get_text(&ctx, "count"), Some("42".to_string()));
        assert_eq!(get_text(&ctx, "ratio"), Some("3.14".to_string()));
        assert_eq!(get_text(&ctx, "enabled"), Some("true".to_string()));
    }

    #[test]
    fn skips_non_text_assets() {
        let mut asset = Asset::new("image.png".into(), vec![0x89, 0x50, 0x4E, 0x47]);
        let mut ctx = Context::default();
        FrontmatterProcessor.process(&mut ctx, &mut asset).unwrap();
        assert!(ctx.is_empty());
    }

    #[test]
    fn skips_invalid_toml() {
        // Nested tables are not supported, so this should be treated as no frontmatter.
        let content = r#"[nested]
key = "value"

***

Body"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        FrontmatterProcessor.process(&mut ctx, &mut asset).unwrap();

        // Should skip - content unchanged, context empty.
        assert!(ctx.is_empty());
        assert_eq!(asset.as_text().unwrap(), content);
    }

    #[test]
    fn skips_non_toml_content() {
        // Random text before *** is not valid TOML.
        let content = r#"This is just some text
that happens to have
***
a delimiter in it"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        FrontmatterProcessor.process(&mut ctx, &mut asset).unwrap();

        // Should skip - content unchanged, context empty.
        assert!(ctx.is_empty());
        assert_eq!(asset.as_text().unwrap(), content);
    }
}
