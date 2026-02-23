use codas::types::Text;
use logos::{Lexer, Logos, Span};

use crate::proc::{
    Asset, Context, ContextValue, Environment, MediaCategory, ProcessesAssets, ProcessingError,
    context_from_toml,
};
use crate::tool::procs::ASSET_PATH_CONTEXT_KEY_PREFIX;

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
    fn process(
        &self,
        _env: &Environment,
        context: &mut Context,
        asset: &mut Asset,
    ) -> Result<(), ProcessingError> {
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
                                        for (i, item) in items.iter().enumerate() {
                                            if i > 0 {
                                                s.push_str(", ");
                                            }
                                            match item {
                                                ContextValue::Text(t) => s.push_str(t),
                                                other => {
                                                    s.push_str(&format!("{:?}", other));
                                                }
                                            }
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

                        // If statement:
                        //   {~ if [not] condition } ... {~ end }
                        //   {~ if var is [not] value } ... {~ end }
                        "if" => {
                            let first_arg = args
                                .first()
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing variable identifier in if expression".into(),
                                })?
                                .try_as_identifier()?;

                            // Detect comparison form: {~ if var is [not] value }
                            let is_comparison = args
                                .get(1)
                                .and_then(|a| a.try_as_identifier().ok())
                                .is_some_and(|id| id == "is");

                            let should_render = if is_comparison {
                                let identifier = &first_arg;

                                // Check for "not" after "is".
                                let (negate, value_index) = if args
                                    .get(2)
                                    .and_then(|a| a.try_as_identifier().ok())
                                    .is_some_and(|id| id == "not")
                                {
                                    (true, 3)
                                } else {
                                    (false, 2)
                                };

                                let compare_arg =
                                    args.get(value_index).ok_or(ProcessingError::Compilation {
                                        message: "missing value in 'is' comparison".into(),
                                    })?;

                                // The right-hand side can be a string literal or
                                // an identifier resolved against the context.
                                let rhs = match compare_arg {
                                    TemplateExpression::String(s) => Some(s.clone()),
                                    TemplateExpression::Identifier(id) => {
                                        match Self::resolve_dotted(context, id) {
                                            Some(ContextValue::Text(t)) => Some(t.clone()),
                                            _ => None,
                                        }
                                    }
                                    _ => None,
                                };

                                let lhs = match Self::resolve_dotted(context, identifier) {
                                    Some(ContextValue::Text(t)) => Some(t.clone()),
                                    _ => None,
                                };

                                let matches = lhs.is_some() && lhs == rhs;
                                if negate { !matches } else { matches }
                            } else {
                                // Truthiness form: {~ if [not] condition }
                                let (negate, identifier) = if first_arg.as_str() == "not" {
                                    let second_arg = args
                                        .get(1)
                                        .ok_or(ProcessingError::Compilation {
                                            message: "missing variable identifier after 'not'"
                                                .into(),
                                        })?
                                        .try_as_identifier()?;
                                    (true, second_arg)
                                } else {
                                    (false, first_arg)
                                };

                                let truthy =
                                    match Self::resolve_dotted(context, identifier.as_str()) {
                                        Some(ContextValue::Text(text)) => {
                                            text != "false" && text != "0" && !text.is_empty()
                                        }
                                        Some(ContextValue::List(list)) => !list.is_empty(),
                                        Some(ContextValue::Table(table)) => !table.is_empty(),
                                        None => false,
                                    };

                                if negate { !truthy } else { truthy }
                            };

                            // If the condition passes, compile the contents of the block.
                            let block_span: std::ops::Range<usize> =
                                Self::traverse_template_block(lexer)?;
                            if should_render {
                                let block_text = &lexer.source()[block_span];
                                let mut block_lexer = Token::lexer(block_text);
                                Self::compile_template(context, &mut block_lexer, output)?;
                            }
                        }

                        // Use statement:
                        //   {~ use "path/to/part" }
                        //   {~ use "path", with "Value" as key, with var as key2 }
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

                            // Parse `with <value> as <key>` clauses.
                            let mut i = 1;
                            while i < args.len() {
                                let keyword = args[i].try_as_identifier()?;
                                if keyword != "with" {
                                    return Err(ProcessingError::Compilation {
                                        message: format!(
                                            "expected 'with' in use expression, got '{}'",
                                            keyword
                                        )
                                        .into(),
                                    });
                                }

                                let value_arg =
                                    args.get(i + 1).ok_or(ProcessingError::Compilation {
                                        message: "missing value after 'with' in use expression"
                                            .into(),
                                    })?;
                                let value = match value_arg {
                                    TemplateExpression::String(s) => ContextValue::Text(s.clone()),
                                    TemplateExpression::Identifier(id) => {
                                        match Self::resolve_dotted(context, id) {
                                            Some(v) => v.clone(),
                                            None => ContextValue::Text("".into()),
                                        }
                                    }
                                    _ => {
                                        return Err(ProcessingError::Compilation {
                                            message: "invalid value in 'with' clause".into(),
                                        });
                                    }
                                };

                                let as_keyword = args
                                    .get(i + 2)
                                    .ok_or(ProcessingError::Compilation {
                                        message: "missing 'as' in 'with' clause".into(),
                                    })?
                                    .try_as_identifier()?;
                                if as_keyword != "as" {
                                    return Err(ProcessingError::Compilation {
                                        message: format!(
                                            "expected 'as' in 'with' clause, got '{}'",
                                            as_keyword
                                        )
                                        .into(),
                                    });
                                }

                                let key = args
                                    .get(i + 3)
                                    .ok_or(ProcessingError::Compilation {
                                        message: "missing key after 'as' in 'with' clause".into(),
                                    })?
                                    .try_as_identifier()?;

                                part_context.insert(key, value);
                                i += 4;
                            }

                            // Compile the part content with the merged context.
                            let mut part_lexer = Token::lexer(body);
                            Self::compile_template(&part_context, &mut part_lexer, output)?;
                        }

                        // For loop:
                        //   {~ for item in collection } ... {~ end }
                        //   {~ for key, val in table } ... {~ end }
                        //   {~ for item in assets "path" } ... {~ end }
                        "for" => {
                            let first = args
                                .first()
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing item identifier in for loop".into(),
                                })?
                                .try_as_identifier()?;

                            // Detect 4-arg form: key, val, in, table
                            let is_kv_form = args.len() == 4
                                && args
                                    .get(2)
                                    .and_then(|a| a.try_as_identifier().ok())
                                    .is_some_and(|id| id == "in");

                            // Detect assets query form: item, in, assets, "path"
                            let is_assets_query = !is_kv_form
                                && args.len() == 4
                                && args
                                    .get(1)
                                    .and_then(|a| a.try_as_identifier().ok())
                                    .is_some_and(|id| id == "in")
                                && args
                                    .get(2)
                                    .and_then(|a| a.try_as_identifier().ok())
                                    .is_some_and(|id| id == "assets")
                                && matches!(args.get(3), Some(TemplateExpression::String(_)));

                            let block_span = Self::traverse_template_block(lexer)?;

                            // Table iteration: {~ for key, val in table }
                            if is_kv_form {
                                let key_identifier = first;
                                let val_identifier = args[1].try_as_identifier()?;
                                let table_identifier = args[3].try_as_identifier()?;
                                let resolved = Self::resolve_dotted(context, &table_identifier);

                                if let Some(ContextValue::Table(table)) = resolved
                                    && !table.is_empty()
                                {
                                    let block_text = &lexer.source()[block_span];

                                    for (k, v) in table {
                                        let mut loop_context = context.clone();
                                        loop_context.insert(
                                            key_identifier.clone(),
                                            ContextValue::Text(k.clone()),
                                        );
                                        loop_context.insert(val_identifier.clone(), v.clone());

                                        let mut block_lexer = Token::lexer(block_text);
                                        Self::compile_template(
                                            &loop_context,
                                            &mut block_lexer,
                                            output,
                                        )?;
                                    }
                                }

                            // Path query: {~ for item in assets "path" }
                            } else if is_assets_query {
                                let item_identifier = first;
                                let dir_path = args[3].try_as_string()?;
                                let assets_key: Text =
                                    format!("{}{}", ASSET_PATH_CONTEXT_KEY_PREFIX, dir_path).into();

                                match context.get(&assets_key) {
                                    // Assets have completed — iterate them.
                                    Some(ContextValue::List(items)) if !items.is_empty() => {
                                        let block_text = &lexer.source()[block_span];

                                        for item in items {
                                            let mut loop_context = context.clone();
                                            loop_context
                                                .insert(item_identifier.clone(), item.clone());

                                            let mut block_lexer = Token::lexer(block_text);
                                            Self::compile_template(
                                                &loop_context,
                                                &mut block_lexer,
                                                output,
                                            )?;
                                        }
                                    }

                                    // Path exists but no assets have completed yet.
                                    Some(ContextValue::List(_)) => {
                                        return Err(ProcessingError::Deferred);
                                    }

                                    // Path does not exist.
                                    _ => {
                                        return Err(ProcessingError::Compilation {
                                            message: format!(
                                                "no assets found at path: {}",
                                                dir_path
                                            )
                                            .into(),
                                        });
                                    }
                                }

                            // List iteration: {~ for item in collection }
                            } else {
                                let item_identifier = first;
                                let collection_identifier = args
                                    .get(2)
                                    .ok_or(ProcessingError::Compilation {
                                        message: "missing collection identifier in for loop".into(),
                                    })?
                                    .try_as_identifier()?;
                                let collection =
                                    Self::resolve_dotted(context, &collection_identifier);

                                if let Some(ContextValue::List(items)) = collection
                                    && !items.is_empty()
                                {
                                    let block_text = &lexer.source()[block_span];

                                    for item in items {
                                        let mut loop_context = context.clone();
                                        loop_context.insert(item_identifier.clone(), item.clone());

                                        let mut block_lexer = Token::lexer(block_text);
                                        Self::compile_template(
                                            &loop_context,
                                            &mut block_lexer,
                                            output,
                                        )?;
                                    }
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
    use std::path::PathBuf;

    use crate::proc::{Asset, MediaType};

    use super::*;

    fn get_text(ctx: &Context, key: &str) -> Option<Text> {
        let key: Text = key.into();
        match ctx.get(&key) {
            Some(ContextValue::Text(t)) => Some(t.clone()),
            _ => None,
        }
    }

    fn get_list(ctx: &Context, key: &str) -> Option<Vec<Text>> {
        let key: Text = key.into();
        match ctx.get(&key) {
            Some(ContextValue::List(items)) => Some(
                items
                    .iter()
                    .filter_map(|v| match v {
                        ContextValue::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    fn test_env() -> Environment {
        Environment {
            source_root: PathBuf::from("."),
            kit_imports: Default::default(),
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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
            ContextValue::List(vec![
                ContextValue::Text("apple".into()),
                ContextValue::Text("banana".into()),
                ContextValue::Text("cherry".into()),
            ]),
        )]
        .into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(get_text(&ctx, "title"), Some("Hello".into()));
        assert_eq!(get_text(&ctx, "author"), Some("Test".into()));

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        let tags = get_list(&ctx, "tags").expect("expected list");
        assert_eq!(tags, vec!["rust", "web", "cli"]);
    }

    #[test]
    fn handles_no_frontmatter() {
        let content = "<h1>No frontmatter here</h1>";
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(get_text(&ctx, "name"), Some("test".into()));
        assert_eq!(get_text(&ctx, "count"), Some("42".into()));
        assert_eq!(get_text(&ctx, "ratio"), Some("3.14".into()));
        assert_eq!(get_text(&ctx, "enabled"), Some("true".into()));
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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
            ContextValue::List(vec![
                ContextValue::Text("a".into()),
                ContextValue::Text("b".into()),
                ContextValue::Text("c".into()),
            ]),
        );
        let mut ctx: Context = [("data".into(), ContextValue::Table(nested))].into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

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
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "My Site");
    }

    #[test]
    fn part_not_found_error() {
        let content = r#"{~ use "_missing.html"}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        let result = TemplateProcessor.process(&test_env(), &mut ctx, &mut asset);
        assert!(result.is_err());
    }

    #[test]
    fn use_with_string_param() {
        let content = r#"{~ use "_greeting.html", with "Hello" as message}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        let part_key: Text = format!("{}_greeting.html", PART_CONTEXT_PREFIX).into();
        ctx.insert(
            part_key,
            ContextValue::Text("<p>{~ get message}</p>".into()),
        );

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();
        assert_eq!(asset.as_text().unwrap(), "<p>Hello</p>");
    }

    #[test]
    fn use_with_identifier_param() {
        let content = r#"{~ use "_greeting.html", with author as name}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        ctx.insert("author".into(), ContextValue::Text("Alice".into()));

        let part_key: Text = format!("{}_greeting.html", PART_CONTEXT_PREFIX).into();
        ctx.insert(
            part_key,
            ContextValue::Text("<p>By {~ get name}</p>".into()),
        );

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();
        assert_eq!(asset.as_text().unwrap(), "<p>By Alice</p>");
    }

    #[test]
    fn use_with_multiple_params() {
        let content = r#"{~ use "_card.html", with "Welcome" as title, with author as byline}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        ctx.insert("author".into(), ContextValue::Text("Bob".into()));

        let part_key: Text = format!("{}_card.html", PART_CONTEXT_PREFIX).into();
        ctx.insert(
            part_key,
            ContextValue::Text("<h1>{~ get title}</h1><p>{~ get byline}</p>".into()),
        );

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();
        assert_eq!(asset.as_text().unwrap(), "<h1>Welcome</h1><p>Bob</p>");
    }

    #[test]
    fn use_with_params_no_commas() {
        let content = r#"{~ use "_card.html" with "Welcome" as title with author as byline}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        ctx.insert("author".into(), ContextValue::Text("Bob".into()));

        let part_key: Text = format!("{}_card.html", PART_CONTEXT_PREFIX).into();
        ctx.insert(
            part_key,
            ContextValue::Text("<h1>{~ get title}</h1><p>{~ get byline}</p>".into()),
        );

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();
        assert_eq!(asset.as_text().unwrap(), "<h1>Welcome</h1><p>Bob</p>");
    }

    #[test]
    fn use_with_param_overrides_frontmatter() {
        let content = r#"{~ use "_header.html", with "Override" as title}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        let part_key: Text = format!("{}_header.html", PART_CONTEXT_PREFIX).into();
        ctx.insert(
            part_key,
            ContextValue::Text("title = \"Default\"\n\n***\n<h1>{~ get title}</h1>".into()),
        );

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();
        assert_eq!(asset.as_text().unwrap(), "\n<h1>Override</h1>");
    }

    #[test]
    fn use_with_dotted_identifier_param() {
        let content = r#"{~ use "_tag.html", with site.name as label}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        let mut site = Context::default();
        site.insert("name".into(), ContextValue::Text("My Site".into()));
        ctx.insert("site".into(), ContextValue::Table(site));

        let part_key: Text = format!("{}_tag.html", PART_CONTEXT_PREFIX).into();
        ctx.insert(
            part_key,
            ContextValue::Text("<span>{~ get label}</span>".into()),
        );

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();
        assert_eq!(asset.as_text().unwrap(), "<span>My Site</span>");
    }

    #[test]
    fn for_loop_with_table_items() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for user in users}{~ get user.name}: {~ get user.role}
{~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut alice = Context::default();
        alice.insert("name".into(), ContextValue::Text("Alice".into()));
        alice.insert("role".into(), ContextValue::Text("admin".into()));
        let mut bob = Context::default();
        bob.insert("name".into(), ContextValue::Text("Bob".into()));
        bob.insert("role".into(), ContextValue::Text("editor".into()));

        let mut ctx: Context = [(
            "users".into(),
            ContextValue::List(vec![ContextValue::Table(alice), ContextValue::Table(bob)]),
        )]
        .into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "Alice: admin\nBob: editor\n");
    }

    #[test]
    fn for_loop_with_mixed_items() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for item in items}{~ get item} {~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [(
            "items".into(),
            ContextValue::List(vec![
                ContextValue::Text("plain".into()),
                ContextValue::Text("text".into()),
            ]),
        )]
        .into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "plain text ");
    }

    #[test]
    fn arrays_of_tables_from_toml() {
        let content = r#"[[links]]
label = "Home"
url = "/"

[[links]]
label = "About"
url = "/about"

***
{~ for link in links}<a href="{~ get link.url}">{~ get link.label}</a>
{~ end}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(
            asset.as_text().unwrap(),
            "\n<a href=\"/\">Home</a>\n<a href=\"/about\">About</a>\n"
        );
    }

    #[test]
    fn get_renders_list_of_tables() {
        let mut asset = Asset::new("test.html".into(), r#"{~ get items}"#.as_bytes().to_vec());
        asset.set_media_type(MediaType::Html);

        let mut entry = Context::default();
        entry.insert("name".into(), ContextValue::Text("x".into()));

        let mut ctx: Context = [(
            "items".into(),
            ContextValue::List(vec![
                ContextValue::Text("a".into()),
                ContextValue::Table(entry),
            ]),
        )]
        .into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        let result = asset.as_text().unwrap();
        // Should render text items directly and non-text items with debug format.
        assert!(result.starts_with("[a, "));
        assert!(result.ends_with(']'));
    }

    #[test]
    fn for_kv_iterates_table() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for key, val in colors}{~ get key}={~ get val} {~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut colors = Context::default();
        colors.insert("blue".into(), ContextValue::Text("#00f".into()));
        colors.insert("red".into(), ContextValue::Text("#f00".into()));

        let mut ctx: Context = [("colors".into(), ContextValue::Table(colors))].into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        // BTreeMap iterates alphabetically.
        assert_eq!(asset.as_text().unwrap(), "blue=#00f red=#f00 ");
    }

    #[test]
    fn for_kv_with_table_values() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for name, info in people}{~ get name}: {~ get info.role}
{~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut alice = Context::default();
        alice.insert("role".into(), ContextValue::Text("admin".into()));
        let mut bob = Context::default();
        bob.insert("role".into(), ContextValue::Text("editor".into()));

        let mut people = Context::default();
        people.insert("alice".into(), ContextValue::Table(alice));
        people.insert("bob".into(), ContextValue::Table(bob));

        let mut ctx: Context = [("people".into(), ContextValue::Table(people))].into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "alice: admin\nbob: editor\n");
    }

    #[test]
    fn for_kv_empty_table() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"before{~ for k, v in empty}nope{~ end}after"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [("empty".into(), ContextValue::Table(Context::default()))].into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "beforeafter");
    }

    #[test]
    fn for_kv_from_toml_frontmatter() {
        let content = r#"[env]
dev = "http://localhost"
prod = "https://example.com"

***
{~ for name, url in env}{~ get name}: {~ get url}
{~ end}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(
            asset.as_text().unwrap(),
            "\ndev: http://localhost\nprod: https://example.com\n"
        );
    }

    #[test]
    fn if_is_string_match() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if role is "admin"}yes{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [("role".into(), ContextValue::Text("admin".into()))].into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "yes");
    }

    #[test]
    fn if_is_string_no_match() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if role is "admin"}yes{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [("role".into(), ContextValue::Text("editor".into()))].into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "");
    }

    #[test]
    fn if_is_not_string() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if role is not "admin"}restricted{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [("role".into(), ContextValue::Text("editor".into()))].into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "restricted");
    }

    #[test]
    fn if_is_not_string_when_matching() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if role is not "admin"}restricted{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [("role".into(), ContextValue::Text("admin".into()))].into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "");
    }

    #[test]
    fn if_is_identifier() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if color is favorite}match{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx: Context = [
            ("color".into(), ContextValue::Text("blue".into())),
            ("favorite".into(), ContextValue::Text("blue".into())),
        ]
        .into();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "match");
    }

    #[test]
    fn if_is_missing_variable() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if missing is "value"}yes{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx = Context::default();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        assert_eq!(asset.as_text().unwrap(), "");
    }

    #[test]
    fn if_is_not_missing_variable() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if missing is not "value"}yes{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx = Context::default();
        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();

        // Missing variable is not equal to "value", so `is not` renders.
        assert_eq!(asset.as_text().unwrap(), "yes");
    }

    #[test]
    fn for_assets_query_errors_on_unknown_path() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for post in assets "blog"}{~ get post.title}{~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx = Context::default();
        let result = TemplateProcessor.process(&test_env(), &mut ctx, &mut asset);
        assert!(matches!(result, Err(ProcessingError::Compilation { .. })));
    }

    #[test]
    fn for_assets_query_defers_when_pending() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for post in assets "blog"}{~ get post.title}{~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        // Empty list means the directory exists but no assets have completed.
        let mut ctx = Context::default();
        ctx.insert("_assets:blog".into(), ContextValue::List(vec![]));

        let result = TemplateProcessor.process(&test_env(), &mut ctx, &mut asset);
        assert_eq!(result, Err(ProcessingError::Deferred));
    }

    #[test]
    fn for_assets_query_iterates() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for post in assets "blog"}{~ get post.title} {~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut entry1 = Context::default();
        entry1.insert("title".into(), ContextValue::Text("hello".into()));
        let mut entry2 = Context::default();
        entry2.insert("title".into(), ContextValue::Text("world".into()));

        let mut ctx = Context::default();
        ctx.insert(
            "_assets:blog".into(),
            ContextValue::List(vec![
                ContextValue::Table(entry1),
                ContextValue::Table(entry2),
            ]),
        );

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();
        assert_eq!(asset.as_text().unwrap(), "hello world ");
    }

    #[test]
    fn for_assets_query_accesses_nested_fields() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for item in assets "posts"}<a href="{~ get item.path}">{~ get item.title}</a>
{~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut entry = Context::default();
        entry.insert("title".into(), ContextValue::Text("My Post".into()));
        entry.insert(
            "path".into(),
            ContextValue::Text("https://example.com/posts/my-post/".into()),
        );

        let mut ctx = Context::default();
        ctx.insert(
            "_assets:posts".into(),
            ContextValue::List(vec![ContextValue::Table(entry)]),
        );

        TemplateProcessor
            .process(&test_env(), &mut ctx, &mut asset)
            .unwrap();
        assert_eq!(
            asset.as_text().unwrap(),
            "<a href=\"https://example.com/posts/my-post/\">My Post</a>\n"
        );
    }
}
