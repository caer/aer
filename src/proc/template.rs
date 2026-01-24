use codas::types::Text;
use logos::{Lexer, Logos, Span};
use toml::Value;

use crate::proc::{Asset, Context, ContextValue, MediaCategory, ProcessesAssets, ProcessingError};

mod tokenizer;

use tokenizer::{TemplateExpression, Token};

pub const FRONTMATTER_DELIMITER: &str = "***";

/// Processes text assets containing template expressions wrapped in
/// `~{ }`, drawing values from a context of key-value pairs.
///
/// Before processing template expressions, the processor extracts TOML
/// frontmatter from the asset and merges it into the processing context.
/// Text contains valid frontmatter if it begins with valid TOML content
/// followed by [FRONTMATTER_DELIMITER] on its own line.
///
/// # Example
///
/// Given a context containing `name = 'Aer', admin = 'true', users = ['Ray', 'Roy']`, this template:
///
/// ```html
/// <div> Hi ~{# name}! It's ~{date "yyyy-mm-dd"}.</div>
/// ~{if admin}
///     <p> You're an administrator, btw.</p>
///     <ul>
///     ~{for user in users}
///         <li>~{# user}</li>
///     ~{end}
///     </ul>
/// ~{end}
/// ```
///
/// would compile to:
///
/// ```html
/// <div> Hi Aer! It's [YYYY-MM-DD].</div>
/// <p> You're an administrator, btw.</p>
/// <ul>
///    <li>Ray</li>
///    <li>Roy</li>
/// </ul>
/// ```
pub struct TemplateProcessor;

impl ProcessesAssets for TemplateProcessor {
    fn process(&self, context: &mut Context, asset: &mut Asset) -> Result<(), ProcessingError> {
        if asset.media_type().category() != MediaCategory::Text {
            tracing::debug!(
                "skipping asset {}: not text {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        // Extract frontmatter before processing templates.
        let template = Self::extract_frontmatter(context, asset)?;
        let mut lexer = Token::lexer(&template);
        let mut output = String::with_capacity(template.len());
        Self::compile_template(context, &mut lexer, &mut output)?;
        asset.replace_with_text(output.into(), asset.media_type().clone());

        Ok(())
    }
}

impl TemplateProcessor {
    /// Extracts TOML frontmatter from the asset, merges it into context,
    /// and returns the remaining template content.
    fn extract_frontmatter(
        context: &mut Context,
        asset: &Asset,
    ) -> Result<String, ProcessingError> {
        let content = asset.as_text()?;

        // Look for the frontmatter delimiter on its own line.
        let delimiter_pattern = format!("\n{}\n", FRONTMATTER_DELIMITER);
        let split_pos = if content.starts_with(&format!("{}\n", FRONTMATTER_DELIMITER)) {
            // Edge case: file starts with delimiter (empty frontmatter).
            Some(0)
        } else {
            content.find(&delimiter_pattern)
        };

        // No frontmatter found - return content as-is.
        let Some(pos) = split_pos else {
            tracing::debug!("no frontmatter found in asset {}", asset.path());
            return Ok(content.to_string());
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
                return Ok(content.to_string());
            }
        };

        // Merge parsed values into the shared context.
        context.extend(parsed);

        Ok(body.to_string())
    }

    /// Parses TOML content into context values.
    fn parse_toml(content: &str) -> Result<Context, ProcessingError> {
        let table: toml::Table =
            toml::from_str(content).map_err(|e| ProcessingError::Malformed {
                message: format!("invalid TOML frontmatter: {}", e).into(),
            })?;

        let mut context = Context::default();
        for (key, value) in table {
            let ctx_value = match value {
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
            }?;
            context.insert(key.into(), ctx_value);
        }
        Ok(context)
    }

    /// Compiles a text template containing zero or more [TemplateExpression]s,
    /// appending the compiled results to `output`.
    fn compile_template(
        context: &Context,
        lexer: &mut Lexer<Token>,
        output: &mut String,
    ) -> Result<(), ProcessingError> {
        while let Some(token) = lexer.next() {
            match token {
                // Evaluate the expression.
                Ok(Token::OpenTemplate(Ok(TemplateExpression::Function {
                    name, args, ..
                }))) => {
                    match name.as_str() {
                        // Variable reference: ~{ # variable_name }
                        "#" => {
                            let identifier = args
                                .first()
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing variable identifier in variable reference"
                                        .into(),
                                })?
                                .try_as_identifier()?;

                            let value = match context.get(&identifier) {
                                Some(ContextValue::Text(text)) => text.clone(),
                                Some(ContextValue::List(items)) => {
                                    let mut items_string = String::from("[");
                                    for item in items {
                                        items_string.push_str(item.as_str());
                                        items_string.push_str(", ");
                                    }
                                    if !items.is_empty() {
                                        items_string.truncate(items_string.len() - 2);
                                    }
                                    items_string.push(']');
                                    items_string.into()
                                }
                                None => format!("~{{# {} }}~", identifier).into(),
                            };

                            output.push_str(&value);
                        }

                        // If statement: ~{ if condition } ... ~{ end }
                        "if" => {
                            let identifier = args
                                .first()
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing variable identifier in if expression".into(),
                                })?
                                .try_as_identifier()?;

                            // A variable reference is "truthy" if it exists and is not "false" or "0".
                            let truthy = match context.get(&identifier) {
                                Some(ContextValue::Text(text)) => {
                                    text != "false" && text != "0" && !text.is_empty()
                                }
                                Some(ContextValue::List(list)) => !list.is_empty(),
                                None => false,
                            };

                            // If the condition is truthy, compile the contents of the block.
                            let block_span = Self::traverse_template_block(lexer)?;
                            if truthy {
                                let block_text = &lexer.source()[block_span];
                                let mut block_lexer = Token::lexer(block_text);
                                Self::compile_template(context, &mut block_lexer, output)?;
                            }
                        }

                        // For loop: ~{ for item in items } ... ~{ end }
                        "for" => {
                            let item_identifier = args
                                .first()
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing item identifier in for loop".into(),
                                })?
                                .try_as_identifier()?;
                            let collection_identifier = args
                                .get(2)
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing collection identifier in for loop".into(),
                                })?
                                .try_as_identifier()?;
                            let collection = context.get(&collection_identifier);

                            let block_span = Self::traverse_template_block(lexer)?;
                            if let Some(ContextValue::List(items)) = collection
                                && !items.is_empty()
                            {
                                let block_text = &lexer.source()[block_span];

                                for item in items {
                                    let mut loop_context = context.clone();
                                    loop_context.insert(
                                        item_identifier.clone(),
                                        ContextValue::Text(item.clone()),
                                    );

                                    let mut block_lexer = Token::lexer(block_text);
                                    Self::compile_template(
                                        &loop_context,
                                        &mut block_lexer,
                                        output,
                                    )?;
                                }
                            }
                        }

                        // Valid end-of-block statements should be handled by
                        // the block traversal logic above.
                        "end" => {
                            return Err(ProcessingError::Compilation {
                                message: "unexpected end-of-block".into(),
                            });
                        }

                        // Unknown template function.
                        _ => {
                            let message = format!("unknown template function: {}", name);
                            return Err(ProcessingError::Compilation {
                                message: message.into(),
                            });
                        }
                    }
                }

                // Unexpected template expression error.
                Ok(Token::OpenTemplate(Ok(expression))) => {
                    let message = format!("unexpected template expression: {:?}", expression);
                    return Err(ProcessingError::Compilation {
                        message: message.into(),
                    });
                }

                // Abort processing if the template contains any errors.
                Ok(Token::OpenTemplate(Err(err))) => {
                    return Err(ProcessingError::Compilation {
                        message: format!("template parse error: {}", err).into(),
                    });
                }

                // If the lexer couldn't parse a token, the next value
                // is just text we can copy directly into the compiled template.
                Err(_) => {
                    let text = lexer.slice();
                    output.push_str(text);
                }
            }
        }

        // There's sometimes a remainder from the lexer, which we can
        // append directly to the compiled text.
        output.push_str(lexer.remainder());

        Ok(())
    }

    /// Traverses a template block (e.g., an if block or for loop)
    /// starting at the current position of `lexer`, returning
    /// the span of the block (excluding the opening and closing
    /// template expressions).
    fn traverse_template_block(lexer: &mut Lexer<Token>) -> Result<Span, ProcessingError> {
        // The end of the outermost template block is the end of the template itself.
        if lexer.span().start == 0 {
            return Ok(0..lexer.source().len());
        }

        // The "start" of traversal is the end of the _current_
        // span, since the immediate next token marks the beginning
        // of the traversed block.
        let start = lexer.span().end;
        let mut end = lexer.span().end;

        while let Some(token) = lexer.next() {
            if let Ok(Token::OpenTemplate(Ok(TemplateExpression::Function { name, .. }))) = token {
                match name.as_str() {
                    // Nested block: traverse it fully.
                    "if" | "for" => {
                        let _ = Self::traverse_template_block(lexer)?;
                    }

                    // End of the current block.
                    "end" => {
                        return Ok(start..end);
                    }
                    _ => {}
                }
            }

            end = lexer.span().end;
        }

        Err(ProcessingError::Compilation {
            message: format!(
                "template contained an unclosed block: {}",
                &lexer.source()[start..]
            )
            .into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::proc::{Asset, MediaType};

    use super::*;

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
    fn processes_if_template() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"~{if is_empty}This is empty!~{end}"#.trim().as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [("is_empty".into(), ContextValue::Text("true".into()))].into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(r#"This is empty!"#, asset.as_text().unwrap());
    }

    #[test]
    fn processes_for_template() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"Items: [~{for item in items}~{# item}, ~{end}]"#.trim().as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [(
            "items".into(),
            ContextValue::List(vec!["apple".into(), "banana".into(), "cherry".into()]),
        )]
        .into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(
            r#"Items: [apple, banana, cherry, ]"#,
            asset.as_text().unwrap()
        );
    }

    #[test]
    fn extracts_frontmatter() {
        let content = r#"title = "Hello"
author = "Test"

***

<h1>~{# title}</h1>"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(get_text(&ctx, "title"), Some("Hello".to_string()));
        assert_eq!(get_text(&ctx, "author"), Some("Test".to_string()));

        let body = asset.as_text().unwrap();
        assert!(!body.contains("title ="));
        assert!(!body.contains("***"));
        assert!(body.contains("<h1>Hello</h1>"));
    }

    #[test]
    fn extracts_arrays() {
        let content = r#"tags = ["rust", "web", "cli"]

***

Body"#;
        let mut asset = Asset::new("page.md".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        let tags = get_list(&ctx, "tags").expect("expected list");
        assert_eq!(tags, vec!["rust", "web", "cli"]);
    }

    #[test]
    fn handles_no_frontmatter() {
        let content = "<h1>No frontmatter here</h1>";
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

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
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(get_text(&ctx, "name"), Some("test".to_string()));
        assert_eq!(get_text(&ctx, "count"), Some("42".to_string()));
        assert_eq!(get_text(&ctx, "ratio"), Some("3.14".to_string()));
        assert_eq!(get_text(&ctx, "enabled"), Some("true".to_string()));
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
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

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
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        // Should skip - content unchanged, context empty.
        assert!(ctx.is_empty());
        assert_eq!(asset.as_text().unwrap(), content);
    }
}
