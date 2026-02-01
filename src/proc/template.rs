use codas::types::Text;
use logos::{Lexer, Logos, Span};

use crate::proc::{
    Asset, Context, ContextValue, MediaCategory, ProcessesAssets, ProcessingError,
    context_from_toml,
};

mod tokenizer;

use tokenizer::{TemplateExpression, Token};

pub const FRONTMATTER_DELIMITER: &str = "***";

/// Prefix used to store parts in the processing context.
pub const PART_CONTEXT_PREFIX: &str = "_part:";

/// Processes text assets containing template expressions wrapped in
/// `{~ }`, drawing values from a context of key-value pairs.
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
/// <div> Hi {~ get name}! It's {~ date "yyyy-mm-dd"}.</div>
/// {~ if admin}
///     <p> You're an administrator, btw.</p>
///     <ul>
///     {~ for user in users}
///         <li>{~ get user}</li>
///     {~ end}
///     </ul>
/// {~ end}
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
            return Ok(());
        }

        tracing::trace!("template: {}", asset.path());

        // Extract frontmatter before processing templates.
        let template = Self::extract_frontmatter(context, asset.as_text()?);
        let mut lexer = Token::lexer(template);
        let mut output = String::with_capacity(template.len());
        Self::compile_template(context, &mut lexer, &mut output)?;
        asset.replace_with_text(output.into(), asset.media_type().clone());

        Ok(())
    }
}

impl TemplateProcessor {
    /// Extracts TOML frontmatter from some text,
    /// merges it into context, and returns the remaining content.
    ///
    /// If the content before `***` is not valid TOML, returns the original
    /// content unchanged (the `***` might just be regular content).
    fn extract_frontmatter<'a>(context: &mut Context, text: &'a Text) -> &'a str {
        // Look for the frontmatter delimiter on its own line.
        let delimiter_pattern = format!("\n{}\n", FRONTMATTER_DELIMITER);
        let split_pos = if text.starts_with(&format!("{}\n", FRONTMATTER_DELIMITER)) {
            Some(0)
        } else {
            text.find(&delimiter_pattern)
        };

        // No frontmatter found - return content as-is.
        let Some(pos) = split_pos else {
            return text.as_str();
        };

        // Split into frontmatter and body.
        let frontmatter = &text[..pos];
        let body_start = if pos == 0 {
            FRONTMATTER_DELIMITER.len() + 1
        } else {
            pos + delimiter_pattern.len() - 1
        };
        let body = &text[body_start..];

        // Try to parse the frontmatter as TOML. If parsing fails, treat
        // it as no frontmatter (*** might just be in regular content).
        match Self::parse_toml(frontmatter) {
            Ok(parsed) => {
                context.extend(parsed);
                body
            }
            Err(_) => text,
        }
    }

    /// Parses TOML content into context values.
    fn parse_toml(content: &str) -> Result<Context, ProcessingError> {
        let table: toml::Table =
            toml::from_str(content).map_err(|e| ProcessingError::Malformed {
                message: format!("invalid TOML frontmatter: {}", e).into(),
            })?;
        context_from_toml(table)
    }

    /// Resolves an identifier against the context, traversing nested
    /// tables for dotted identifiers like `user.name`.
    fn resolve_dotted<'a>(context: &'a Context, identifier: &str) -> Option<&'a ContextValue> {
        if !identifier.contains('.') {
            let key: Text = identifier.into();
            return context.get(&key);
        }

        let segments: Vec<&str> = identifier.split('.').collect();
        let mut current = context;

        for (i, segment) in segments.iter().enumerate() {
            let key: Text = (*segment).into();
            match current.get(&key) {
                Some(ContextValue::Table(table)) if i < segments.len() - 1 => {
                    current = table;
                }
                Some(value) if i == segments.len() - 1 => {
                    return Some(value);
                }
                _ => return None,
            }
        }

        None
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
                        // Variable reference: {~ get variable_name }
                        // Supports fallback chain: {~ get title or name or headline }
                        "get" => {
                            let identifier = args
                                .first()
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing variable identifier in variable reference"
                                        .into(),
                                })?
                                .try_as_identifier()?;

                            // Collect all identifiers in the fallback chain.
                            let mut identifiers = vec![identifier];
                            let mut i = 1;
                            while i < args.len() {
                                let keyword = args[i].try_as_identifier()?;
                                if keyword != "or" {
                                    return Err(ProcessingError::Compilation {
                                        message: format!(
                                            "expected 'or' in get expression, got '{}'",
                                            keyword
                                        )
                                        .into(),
                                    });
                                }
                                let next = args.get(i + 1).ok_or(ProcessingError::Compilation {
                                    message: "missing variable identifier after 'or'".into(),
                                })?;
                                identifiers.push(next.try_as_identifier()?);
                                i += 2;
                            }

                            // Try each identifier until one resolves.
                            let mut resolved = None;
                            for ident in &identifiers {
                                match Self::resolve_dotted(context, ident) {
                                    Some(ContextValue::Text(text)) => {
                                        resolved = Some(text.clone());
                                        break;
                                    }
                                    Some(ContextValue::List(items)) => {
                                        let mut s = String::from("[");
                                        for item in items {
                                            s.push_str(item);
                                            s.push_str(", ");
                                        }
                                        if !items.is_empty() {
                                            s.truncate(s.len() - 2);
                                        }
                                        s.push(']');
                                        resolved = Some(s.into());
                                        break;
                                    }
                                    _ => continue,
                                }
                            }

                            let value = resolved.unwrap_or_else(|| {
                                let chain = identifiers
                                    .iter()
                                    .map(|id| id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" or ");
                                format!("{{~ get {} }}", chain).into()
                            });

                            output.push_str(&value);
                        }

                        // If statement: {~ if [not] condition } ... {~ end }
                        "if" => {
                            let first_arg = args
                                .first()
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing variable identifier in if expression".into(),
                                })?
                                .try_as_identifier()?;

                            // Check for negation keyword.
                            let (negate, identifier) = if first_arg.as_str() == "not" {
                                let second_arg = args
                                    .get(1)
                                    .ok_or(ProcessingError::Compilation {
                                        message: "missing variable identifier after 'not'".into(),
                                    })?
                                    .try_as_identifier()?;
                                (true, second_arg)
                            } else {
                                (false, first_arg)
                            };

                            // A variable reference is "truthy" if it exists and is not "false" or "0".
                            let truthy = match Self::resolve_dotted(context, identifier.as_str()) {
                                Some(ContextValue::Text(text)) => {
                                    text != "false" && text != "0" && !text.is_empty()
                                }
                                Some(ContextValue::List(list)) => !list.is_empty(),
                                Some(ContextValue::Table(table)) => !table.is_empty(),
                                None => false,
                            };

                            // Apply negation if needed.
                            let should_render = if negate { !truthy } else { truthy };

                            // If the condition passes, compile the contents of the block.
                            let block_span: std::ops::Range<usize> =
                                Self::traverse_template_block(lexer)?;
                            if should_render {
                                let block_text = &lexer.source()[block_span];
                                let mut block_lexer = Token::lexer(block_text);
                                Self::compile_template(context, &mut block_lexer, output)?;
                            }
                        }

                        // Use statement: {~ use "path/to/part" }
                        "use" => {
                            let path = args
                                .first()
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing path in use expression".into(),
                                })?
                                .try_as_string()?;

                            // Look up the part in the context.
                            let part_key: Text = format!("{}{}", PART_CONTEXT_PREFIX, path).into();
                            let part_content = match context.get(&part_key) {
                                Some(ContextValue::Text(content)) => content,
                                _ => {
                                    return Err(ProcessingError::Compilation {
                                        message: format!("part not found: {}", path).into(),
                                    });
                                }
                            };

                            // Extract frontmatter from the part and merge into context.
                            let mut part_context = context.clone();
                            let body = Self::extract_frontmatter(&mut part_context, part_content);

                            // Compile the part content with the merged context.
                            let mut part_lexer = Token::lexer(body);
                            Self::compile_template(&part_context, &mut part_lexer, output)?;
                        }

                        // For loop: {~ for item in items } ... {~ end }
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
                            let collection =
                                Self::resolve_dotted(context, collection_identifier.as_str());

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
            r#"{~ if is_empty}This is empty!{~ end}"#.trim().as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [("is_empty".into(), ContextValue::Text("true".into()))].into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(r#"This is empty!"#, asset.as_text().unwrap());
    }

    #[test]
    fn processes_negated_if_template() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if not is_empty}Not empty!{~ end}"#.trim().as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        // When is_empty is false, "not is_empty" should render the block.
        let mut ctx: Context = [("is_empty".into(), ContextValue::Text("false".into()))].into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(r#"Not empty!"#, asset.as_text().unwrap());
    }

    #[test]
    fn processes_negated_if_template_when_true() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if not is_empty}Not empty!{~ end}"#.trim().as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        // When is_empty is true, "not is_empty" should NOT render the block.
        let mut ctx: Context = [("is_empty".into(), ContextValue::Text("true".into()))].into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(r#""#, asset.as_text().unwrap());
    }

    #[test]
    fn processes_negated_if_template_missing_variable() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if not missing}Default content{~ end}"#.trim().as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        // When variable is missing (falsy), "not missing" should render the block.
        let mut ctx = Context::default();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(r#"Default content"#, asset.as_text().unwrap());
    }

    #[test]
    fn processes_for_template() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"Items: [{~ for item in items}{~ get item}, {~ end}]"#
                .trim()
                .as_bytes()
                .to_vec(),
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

<h1>{~ get title}</h1>"#;
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
    fn parses_nested_tables() {
        let content = r#"[user]
name = "Alice"
active = true

***
<p>{~ get user.name}</p>"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert!(ctx.contains_key(&"user".into()));
        assert!(asset.as_text().unwrap().contains("<p>Alice</p>"));
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

    #[test]
    fn includes_part() {
        let content = r#"<html>{~ use "_header.html"}<body>Hello</body></html>"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        // Add a part to the context.
        let part_key: Text = format!("{}_header.html", PART_CONTEXT_PREFIX).into();
        ctx.insert(
            part_key,
            ContextValue::Text("<header>Header</header>".into()),
        );

        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(
            asset.as_text().unwrap(),
            "<html><header>Header</header><body>Hello</body></html>"
        );
    }

    #[test]
    fn includes_part_with_frontmatter() {
        // Part frontmatter is available within the part, but not in the parent.
        let content = r#"<html>{~ use "_meta.html"}</html>"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        // Add a part with frontmatter that uses its own variable.
        // Note: content after *** delimiter includes a leading newline.
        let part_key: Text = format!("{}_meta.html", PART_CONTEXT_PREFIX).into();
        let part_content = "charset = \"utf-8\"\n\n***\n<meta charset=\"{~ get charset}\">";
        ctx.insert(part_key, ContextValue::Text(part_content.into()));

        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        // The part's frontmatter is used within the part itself.
        assert_eq!(
            asset.as_text().unwrap(),
            "<html>\n<meta charset=\"utf-8\"></html>"
        );
    }

    #[test]
    fn includes_nested_parts() {
        let content = r#"{~ use "_layout.html"}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        // Add nested parts.
        let layout_key: Text = format!("{}_layout.html", PART_CONTEXT_PREFIX).into();
        ctx.insert(
            layout_key,
            ContextValue::Text("<html>{~ use \"_header.html\"}<body>Content</body></html>".into()),
        );

        let header_key: Text = format!("{}_header.html", PART_CONTEXT_PREFIX).into();
        ctx.insert(
            header_key,
            ContextValue::Text("<header>Header</header>".into()),
        );

        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(
            asset.as_text().unwrap(),
            "<html><header>Header</header><body>Content</body></html>"
        );
    }

    #[test]
    fn dotted_if_condition() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if user.active}Active!{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut nested = Context::default();
        nested.insert("active".into(), ContextValue::Text("true".into()));
        let mut ctx: Context = [("user".into(), ContextValue::Table(nested))].into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(asset.as_text().unwrap(), "Active!");
    }

    #[test]
    fn dotted_for_loop() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for x in data.items}{~ get x} {~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut nested = Context::default();
        nested.insert(
            "items".into(),
            ContextValue::List(vec!["a".into(), "b".into(), "c".into()]),
        );
        let mut ctx: Context = [("data".into(), ContextValue::Table(nested))].into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(asset.as_text().unwrap(), "a b c ");
    }

    #[test]
    fn deeply_nested_dotted_access() {
        let mut asset = Asset::new("test.html".into(), r#"{~ get a.b.c}"#.as_bytes().to_vec());
        asset.set_media_type(MediaType::Html);

        let mut inner = Context::default();
        inner.insert("c".into(), ContextValue::Text("deep".into()));
        let mut outer = Context::default();
        outer.insert("b".into(), ContextValue::Table(inner));
        let mut ctx: Context = [("a".into(), ContextValue::Table(outer))].into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(asset.as_text().unwrap(), "deep");
    }

    #[test]
    fn missing_dotted_path() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get user.missing}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx = Context::default();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(asset.as_text().unwrap(), "{~ get user.missing }");
    }

    #[test]
    fn get_fallback_uses_first_resolved() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get title or name}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [("name".into(), ContextValue::Text("Alice".into()))].into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(asset.as_text().unwrap(), "Alice");
    }

    #[test]
    fn get_fallback_prefers_first() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get title or name}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [
            ("title".into(), ContextValue::Text("Hello".into())),
            ("name".into(), ContextValue::Text("Alice".into())),
        ]
        .into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(asset.as_text().unwrap(), "Hello");
    }

    #[test]
    fn get_fallback_chain() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get a or b or c}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [("c".into(), ContextValue::Text("third".into()))].into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(asset.as_text().unwrap(), "third");
    }

    #[test]
    fn get_fallback_none_resolved() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get title or name}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx = Context::default();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(asset.as_text().unwrap(), "{~ get title or name }");
    }

    #[test]
    fn get_fallback_with_dotted() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get page.title or site.name}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut site = Context::default();
        site.insert("name".into(), ContextValue::Text("My Site".into()));
        let mut ctx: Context = [("site".into(), ContextValue::Table(site))].into();
        TemplateProcessor.process(&mut ctx, &mut asset).unwrap();

        assert_eq!(asset.as_text().unwrap(), "My Site");
    }

    #[test]
    fn part_not_found_error() {
        let content = r#"{~ use "_missing.html"}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        let result = TemplateProcessor.process(&mut ctx, &mut asset);
        assert!(result.is_err());
    }
}
