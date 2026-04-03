use chrono::NaiveDate;
use codas::types::Text;
use logos::{Lexer, Logos, Span};

use crate::proc::{
    Asset, ContextValue, Environment, LayeredContext, MediaCategory, ProcessesAssets,
    ProcessingError,
};
use crate::tool::procs::ASSET_PATH_CONTEXT_KEY_PREFIX;

mod tokenizer;

use tokenizer::{TemplateExpression, Token};

/// Prefix used to store parts in the processing context.
pub const PART_CONTEXT_PREFIX: &str = "_part:";

/// Prefix used to store part defaults in the processing context.
pub const PART_DEFAULTS_PREFIX: &str = "_part_ctx:";

/// Processes text assets containing template expressions wrapped in
/// `{~ }`, drawing values from a context of key-value pairs.
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
        env: &Environment,
        context: &LayeredContext,
        asset: &mut Asset,
    ) -> Result<bool, ProcessingError> {
        if asset.media_type().category() != MediaCategory::Text {
            return Ok(false);
        }

        tracing::trace!("template: {}", asset.path());

        let template = asset.as_text()?;
        let mut lexer = Token::lexer(template);
        let mut output = String::with_capacity(template.len());
        Self::compile_template(env, context, &mut lexer, &mut output)?;
        asset.replace_with_text(output.into(), asset.media_type().clone());

        Ok(true)
    }
}

impl TemplateProcessor {
    /// Compiles a text template containing zero or more [TemplateExpression]s,
    /// appending the compiled results to `output`.
    fn compile_template(
        env: &Environment,
        context: &LayeredContext,
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
                                match context.resolve(ident) {
                                    Some(ContextValue::Text(text)) => {
                                        resolved = Some(text.clone());
                                        break;
                                    }
                                    Some(ContextValue::AssetRef(path)) => {
                                        if let Some(output_path) =
                                            env.asset_outputs.get(path.as_str())
                                        {
                                            resolved = Some(format!("/{}", output_path).into());
                                        }
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

                        // Date formatting: {~ date variable "format" }
                        // Parses the variable as a date, then formats it
                        // using a chrono strftime format string.
                        "date" => {
                            let identifier = args
                                .first()
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing variable identifier in date expression"
                                        .into(),
                                })?
                                .try_as_identifier()?;

                            let format = args
                                .get(1)
                                .ok_or(ProcessingError::Compilation {
                                    message: "missing format string in date expression".into(),
                                })?
                                .try_as_string()?;

                            if let Some(ContextValue::Text(raw)) = context.resolve(&identifier) {
                                if let Some(date) = Self::parse_date(raw) {
                                    output.push_str(&date.format(format.as_str()).to_string());
                                } else {
                                    output.push_str(raw);
                                }
                            }
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
                                        match context.resolve(id) {
                                            Some(ContextValue::Text(t)) => Some(t.clone()),
                                            _ => None,
                                        }
                                    }
                                    _ => None,
                                };

                                let lhs = match context.resolve(identifier) {
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

                                let truthy = match context.resolve(identifier.as_str()) {
                                    Some(ContextValue::Text(text)) => {
                                        text != "false" && text != "0" && !text.is_empty()
                                    }
                                    Some(ContextValue::AssetRef(_)) => true,
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
                                Self::compile_template(env, context, &mut block_lexer, output)?;
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

                            // Merge part defaults into a scoped context.
                            let mut part_context = context.child_scope();
                            let defaults_key: Text =
                                format!("{}{}", PART_DEFAULTS_PREFIX, path).into();
                            if let Some(ContextValue::Table(defaults)) = context.get(&defaults_key)
                            {
                                part_context.extend_top(defaults.clone());
                            }

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
                                        match context.resolve(id) {
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
                            let mut part_lexer = Token::lexer(part_content);
                            Self::compile_template(env, &part_context, &mut part_lexer, output)?;
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

                            // Detect assets query form: item, in, assets, "path" [sort key [asc|desc]]
                            let is_assets_query = !is_kv_form
                                && args.len() >= 4
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
                                let resolved = context.resolve(&table_identifier);

                                if let Some(ContextValue::Table(table)) = resolved
                                    && !table.is_empty()
                                {
                                    let block_text = &lexer.source()[block_span];

                                    for (k, v) in table {
                                        let mut loop_context = context.child_scope();
                                        loop_context.insert(
                                            key_identifier.clone(),
                                            ContextValue::Text(k.clone()),
                                        );
                                        loop_context.insert(val_identifier.clone(), v.clone());

                                        let mut block_lexer = Token::lexer(block_text);
                                        Self::compile_template(
                                            env,
                                            &loop_context,
                                            &mut block_lexer,
                                            output,
                                        )?;
                                    }
                                }

                            // Path query:
                            //   {~ for item in assets "path" [sort key [asc|desc]] }
                            } else if is_assets_query {
                                let item_identifier = first;
                                let dir_path = args[3].try_as_string()?;
                                let assets_key: Text =
                                    format!("{}{}", ASSET_PATH_CONTEXT_KEY_PREFIX, dir_path).into();

                                // Parse optional sort clause.
                                let sort_clause = if args.len() >= 6
                                    && args
                                        .get(4)
                                        .and_then(|a| a.try_as_identifier().ok())
                                        .is_some_and(|id| id == "sort")
                                {
                                    let sort_key = args[5].try_as_identifier()?;
                                    let descending = args
                                        .get(6)
                                        .and_then(|a| a.try_as_identifier().ok())
                                        .is_some_and(|id| id == "desc");
                                    Some((sort_key, descending))
                                } else {
                                    None
                                };

                                // Collect items from the exact path and all
                                // subdirectories (e.g., "logs" also gathers
                                // from "logs/ldjam-57", "logs/guide-to-ai", etc.).
                                let prefix: Text =
                                    format!("{}{}/", ASSET_PATH_CONTEXT_KEY_PREFIX, dir_path)
                                        .into();
                                let mut all_items: Vec<ContextValue> = Vec::new();

                                for (key, value) in
                                    context.iter_by_prefix(ASSET_PATH_CONTEXT_KEY_PREFIX)
                                {
                                    if (*key == assets_key || key.starts_with(prefix.as_str()))
                                        && let ContextValue::List(items) = value
                                    {
                                        all_items.extend(items.iter().cloned());
                                    }
                                }

                                if !all_items.is_empty() {
                                    let block_text = &lexer.source()[block_span];

                                    let items = if let Some((sort_key, descending)) = &sort_clause {
                                        // Schwartzian transform: parse sort keys
                                        // once rather than on every comparison.
                                        let mut keyed: Vec<_> = all_items
                                            .into_iter()
                                            .map(|item| {
                                                let raw = Self::extract_sort_value(&item, sort_key);
                                                let parsed =
                                                    raw.as_deref().and_then(Self::parse_date);
                                                (parsed, raw, item)
                                            })
                                            .collect();
                                        keyed.sort_by(|(ad, ar, _), (bd, br, _)| {
                                            let cmp = match (ad, bd) {
                                                (Some(a), Some(b)) => a.cmp(b),
                                                (Some(_), None) => std::cmp::Ordering::Less,
                                                (None, Some(_)) => std::cmp::Ordering::Greater,
                                                (None, None) => ar.cmp(br),
                                            };
                                            if *descending { cmp.reverse() } else { cmp }
                                        });
                                        keyed.into_iter().map(|(_, _, item)| item).collect()
                                    } else {
                                        all_items
                                    };

                                    for item in &items {
                                        let mut loop_context = context.child_scope();
                                        loop_context.insert(item_identifier.clone(), item.clone());

                                        let mut block_lexer = Token::lexer(block_text);
                                        Self::compile_template(
                                            env,
                                            &loop_context,
                                            &mut block_lexer,
                                            output,
                                        )?;
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
                                let collection = context.resolve(&collection_identifier);

                                if let Some(ContextValue::List(items)) = collection
                                    && !items.is_empty()
                                {
                                    let block_text = &lexer.source()[block_span];

                                    for item in items {
                                        let mut loop_context = context.child_scope();
                                        loop_context.insert(item_identifier.clone(), item.clone());

                                        let mut block_lexer = Token::lexer(block_text);
                                        Self::compile_template(
                                            env,
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

    /// Extracts a string value from a context value for sort comparison.
    fn extract_sort_value(value: &ContextValue, key: &str) -> Option<String> {
        match value {
            ContextValue::Table(table) => {
                let k: Text = key.into();
                match table.get(&k) {
                    Some(ContextValue::Text(t)) => Some(t.to_string()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Parses a `YYYY-MM-DD` date, with or without a trailing time component.
    fn parse_date(s: &str) -> Option<NaiveDate> {
        let date_part = s.trim().split('T').next()?;
        NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
    }
}

#[cfg(test)]
mod tests {
    use crate::proc::{Asset, Context, MediaType, extract_frontmatter};

    use super::*;

    fn apply_frontmatter(ctx: &mut Context, asset: &mut Asset) {
        let (body, frontmatter) = extract_frontmatter(asset.as_text().unwrap());
        if let Some(parsed) = frontmatter {
            ctx.extend(parsed);
            asset.replace_with_text(body.into(), asset.media_type().clone());
        }
    }

    fn register_part(ctx: &mut Context, path: &str, content: &str) {
        let text: Text = content.into();
        let (body, defaults) = extract_frontmatter(&text);
        let part_key: Text = format!("{}{}", PART_CONTEXT_PREFIX, path).into();
        ctx.insert(part_key, ContextValue::Text(body.into()));
        if let Some(defaults) = defaults {
            let ctx_key: Text = format!("{}{}", PART_DEFAULTS_PREFIX, path).into();
            ctx.insert(ctx_key, ContextValue::Table(defaults));
        }
    }

    /// Wraps a flat context and runs the template processor.
    fn run(ctx: &Context, asset: &mut Asset) {
        let lctx = LayeredContext::from_flat(ctx.clone());
        TemplateProcessor
            .process(&Environment::test(), &lctx, asset)
            .unwrap();
    }

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

    #[test]
    fn processes_if_template() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if is_empty}This is empty!{~ end}"#.trim().as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [("is_empty".into(), ContextValue::Text("true".into()))].into();
        run(&ctx, &mut asset);

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
        let ctx: Context = [("is_empty".into(), ContextValue::Text("false".into()))].into();
        run(&ctx, &mut asset);

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
        let ctx: Context = [("is_empty".into(), ContextValue::Text("true".into()))].into();
        run(&ctx, &mut asset);

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
        let ctx = Context::default();
        run(&ctx, &mut asset);

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

        let ctx: Context = [(
            "items".into(),
            ContextValue::List(vec![
                ContextValue::Text("apple".into()),
                ContextValue::Text("banana".into()),
                ContextValue::Text("cherry".into()),
            ]),
        )]
        .into();
        run(&ctx, &mut asset);

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
        apply_frontmatter(&mut ctx, &mut asset);
        run(&ctx, &mut asset);

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
        apply_frontmatter(&mut ctx, &mut asset);
        run(&ctx, &mut asset);

        let tags = get_list(&ctx, "tags").expect("expected list");
        assert_eq!(tags, vec!["rust", "web", "cli"]);
    }

    #[test]
    fn handles_no_frontmatter() {
        let content = "<h1>No frontmatter here</h1>";
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let ctx = Context::default();
        run(&ctx, &mut asset);

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
        apply_frontmatter(&mut ctx, &mut asset);
        run(&ctx, &mut asset);

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
        apply_frontmatter(&mut ctx, &mut asset);
        run(&ctx, &mut asset);

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
        let ctx = Context::default();
        run(&ctx, &mut asset);

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

        run(&ctx, &mut asset);

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
        register_part(
            &mut ctx,
            "_meta.html",
            "charset = \"utf-8\"\n\n***\n<meta charset=\"{~ get charset}\">",
        );

        run(&ctx, &mut asset);

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

        run(&ctx, &mut asset);

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
        let ctx: Context = [("user".into(), ContextValue::Table(nested))].into();
        run(&ctx, &mut asset);

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
        let ctx: Context = [("data".into(), ContextValue::Table(nested))].into();
        run(&ctx, &mut asset);

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
        let ctx: Context = [("a".into(), ContextValue::Table(outer))].into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "deep");
    }

    #[test]
    fn missing_dotted_path() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get user.missing}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx = Context::default();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "{~ get user.missing }");
    }

    #[test]
    fn get_fallback_uses_first_resolved() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get title or name}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [("name".into(), ContextValue::Text("Alice".into()))].into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "Alice");
    }

    #[test]
    fn get_fallback_prefers_first() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get title or name}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [
            ("title".into(), ContextValue::Text("Hello".into())),
            ("name".into(), ContextValue::Text("Alice".into())),
        ]
        .into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "Hello");
    }

    #[test]
    fn get_fallback_chain() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get a or b or c}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [("c".into(), ContextValue::Text("third".into()))].into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "third");
    }

    #[test]
    fn get_fallback_none_resolved() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ get title or name}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx = Context::default();
        run(&ctx, &mut asset);

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
        let ctx: Context = [("site".into(), ContextValue::Table(site))].into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "My Site");
    }

    #[test]
    fn part_not_found_error() {
        let content = r#"{~ use "_missing.html"}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let ctx = Context::default();

        let lctx = LayeredContext::from_flat(ctx);
        let result = TemplateProcessor.process(&Environment::test(), &lctx, &mut asset);
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

        run(&ctx, &mut asset);
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

        run(&ctx, &mut asset);
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

        run(&ctx, &mut asset);
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

        run(&ctx, &mut asset);
        assert_eq!(asset.as_text().unwrap(), "<h1>Welcome</h1><p>Bob</p>");
    }

    #[test]
    fn use_with_param_overrides_frontmatter() {
        let content = r#"{~ use "_header.html", with "Override" as title}"#;
        let mut asset = Asset::new("page.html".into(), content.as_bytes().to_vec());
        let mut ctx = Context::default();

        register_part(
            &mut ctx,
            "_header.html",
            "title = \"Default\"\n\n***\n<h1>{~ get title}</h1>",
        );

        run(&ctx, &mut asset);
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

        run(&ctx, &mut asset);
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

        let ctx: Context = [(
            "users".into(),
            ContextValue::List(vec![ContextValue::Table(alice), ContextValue::Table(bob)]),
        )]
        .into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "Alice: admin\nBob: editor\n");
    }

    #[test]
    fn for_loop_with_mixed_items() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for item in items}{~ get item} {~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [(
            "items".into(),
            ContextValue::List(vec![
                ContextValue::Text("plain".into()),
                ContextValue::Text("text".into()),
            ]),
        )]
        .into();
        run(&ctx, &mut asset);

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
        apply_frontmatter(&mut ctx, &mut asset);
        run(&ctx, &mut asset);

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

        let ctx: Context = [(
            "items".into(),
            ContextValue::List(vec![
                ContextValue::Text("a".into()),
                ContextValue::Table(entry),
            ]),
        )]
        .into();
        run(&ctx, &mut asset);

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

        let ctx: Context = [("colors".into(), ContextValue::Table(colors))].into();
        run(&ctx, &mut asset);

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

        let ctx: Context = [("people".into(), ContextValue::Table(people))].into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "alice: admin\nbob: editor\n");
    }

    #[test]
    fn for_kv_empty_table() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"before{~ for k, v in empty}nope{~ end}after"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [("empty".into(), ContextValue::Table(Context::default()))].into();
        run(&ctx, &mut asset);

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
        apply_frontmatter(&mut ctx, &mut asset);
        run(&ctx, &mut asset);

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

        let ctx: Context = [("role".into(), ContextValue::Text("admin".into()))].into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "yes");
    }

    #[test]
    fn if_is_string_no_match() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if role is "admin"}yes{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [("role".into(), ContextValue::Text("editor".into()))].into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "");
    }

    #[test]
    fn if_is_not_string() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if role is not "admin"}restricted{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [("role".into(), ContextValue::Text("editor".into()))].into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "restricted");
    }

    #[test]
    fn if_is_not_string_when_matching() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if role is not "admin"}restricted{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [("role".into(), ContextValue::Text("admin".into()))].into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "");
    }

    #[test]
    fn if_is_identifier() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if color is favorite}match{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [
            ("color".into(), ContextValue::Text("blue".into())),
            ("favorite".into(), ContextValue::Text("blue".into())),
        ]
        .into();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "match");
    }

    #[test]
    fn if_is_missing_variable() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if missing is "value"}yes{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx = Context::default();
        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "");
    }

    #[test]
    fn if_is_not_missing_variable() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ if missing is not "value"}yes{~ end}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx = Context::default();
        run(&ctx, &mut asset);

        // Missing variable is not equal to "value", so `is not` renders.
        assert_eq!(asset.as_text().unwrap(), "yes");
    }

    #[test]
    fn for_assets_query_empty_when_unknown_path() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for post in assets "blog"}{~ get post.title}{~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx = Context::default();
        run(&ctx, &mut asset);
        assert_eq!(asset.as_text().unwrap(), "");
    }

    #[test]
    fn for_assets_query_produces_empty_when_no_items() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for post in assets "blog"}{~ get post.title}{~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        // Empty list means the directory exists but no assets have completed yet.
        let mut ctx = Context::default();
        ctx.insert("_assets:blog".into(), ContextValue::List(vec![]));

        run(&ctx, &mut asset);
        assert_eq!(asset.as_text().unwrap(), "");
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

        run(&ctx, &mut asset);
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

        run(&ctx, &mut asset);
        assert_eq!(
            asset.as_text().unwrap(),
            "<a href=\"https://example.com/posts/my-post/\">My Post</a>\n"
        );
    }

    #[test]
    fn for_assets_query_renders_partial_when_subdirectory_empty() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for post in assets "logs"}{~ get post.title}{~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx = Context::default();
        // Hydrated entries already present at the root path.
        let mut entry = Context::default();
        entry.insert("title".into(), ContextValue::Text("External".into()));
        ctx.insert(
            "_assets:logs".into(),
            ContextValue::List(vec![ContextValue::Table(entry)]),
        );
        // Subdirectory empty (articles still processing).
        ctx.insert("_assets:logs/my-article".into(), ContextValue::List(vec![]));

        run(&ctx, &mut asset);
        assert_eq!(asset.as_text().unwrap(), "External");
    }

    #[test]
    fn for_assets_query_collects_from_subdirectories() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ for post in assets "logs"}{~ get post.title}, {~ end}"#
                .as_bytes()
                .to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let mut ctx = Context::default();
        // Entry at root path (e.g., from hydration).
        let mut external = Context::default();
        external.insert("title".into(), ContextValue::Text("External".into()));
        ctx.insert(
            "_assets:logs".into(),
            ContextValue::List(vec![ContextValue::Table(external)]),
        );
        // Entry in a subdirectory (e.g., completed article).
        let mut article = Context::default();
        article.insert("title".into(), ContextValue::Text("Article".into()));
        ctx.insert(
            "_assets:logs/my-article".into(),
            ContextValue::List(vec![ContextValue::Table(article)]),
        );

        run(&ctx, &mut asset);
        // Both entries should appear (order is BTreeMap key order).
        assert_eq!(asset.as_text().unwrap(), "External, Article, ");
    }

    #[test]
    fn for_assets_sort_date_desc() {
        let template = r#"{~ for item in assets "logs" sort date desc}{~ get item.title}: {~ get item.date}
{~ end}"#;
        let mut asset = Asset::new("index.html".into(), template.as_bytes().to_vec());
        asset.set_media_type(MediaType::Html);

        let mut entry1 = Context::default();
        entry1.insert("title".into(), ContextValue::Text("Old Post".into()));
        entry1.insert("date".into(), ContextValue::Text("2023-01-28".into()));

        let mut entry2 = Context::default();
        entry2.insert("title".into(), ContextValue::Text("New Post".into()));
        entry2.insert("date".into(), ContextValue::Text("2025-04-17".into()));

        let mut entry3 = Context::default();
        entry3.insert("title".into(), ContextValue::Text("Mid Post".into()));
        entry3.insert("date".into(), ContextValue::Text("2025-01-13".into()));

        let mut ctx = Context::default();
        ctx.insert(
            "_assets:logs".into(),
            ContextValue::List(vec![
                ContextValue::Table(entry1),
                ContextValue::Table(entry2),
                ContextValue::Table(entry3),
            ]),
        );

        run(&ctx, &mut asset);

        assert_eq!(
            asset.as_text().unwrap(),
            "New Post: 2025-04-17\nMid Post: 2025-01-13\nOld Post: 2023-01-28\n"
        );
    }

    #[test]
    fn for_assets_sort_date_asc() {
        let template = r#"{~ for item in assets "logs" sort date asc}{~ get item.title}, {~ end}"#;
        let mut asset = Asset::new("index.html".into(), template.as_bytes().to_vec());
        asset.set_media_type(MediaType::Html);

        let mut entry1 = Context::default();
        entry1.insert("title".into(), ContextValue::Text("New".into()));
        entry1.insert("date".into(), ContextValue::Text("2025-04-17".into()));

        let mut entry2 = Context::default();
        entry2.insert("title".into(), ContextValue::Text("Old".into()));
        entry2.insert("date".into(), ContextValue::Text("2023-01-28".into()));

        let mut ctx = Context::default();
        ctx.insert(
            "_assets:logs".into(),
            ContextValue::List(vec![
                ContextValue::Table(entry1),
                ContextValue::Table(entry2),
            ]),
        );

        run(&ctx, &mut asset);

        assert_eq!(asset.as_text().unwrap(), "Old, New, ");
    }

    #[test]
    fn date_formats_value() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ date mydate "%m.%d.%Y"}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [("mydate".into(), ContextValue::Text("2025-04-17".into()))].into();
        run(&ctx, &mut asset);
        assert_eq!(asset.as_text().unwrap(), "04.17.2025");
    }

    #[test]
    fn date_formats_iso() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ date d "%m.%d.%Y"}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [(
            "d".into(),
            ContextValue::Text("2025-04-17T00:00:00Z".into()),
        )]
        .into();
        run(&ctx, &mut asset);
        assert_eq!(asset.as_text().unwrap(), "04.17.2025");
    }

    #[test]
    fn date_passes_through_unparseable() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"{~ date d "%m.%d.%Y"}"#.as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        let ctx: Context = [("d".into(), ContextValue::Text("not a date".into()))].into();
        run(&ctx, &mut asset);
        assert_eq!(asset.as_text().unwrap(), "not a date");
    }

    #[test]
    fn parse_date_formats() {
        assert!(TemplateProcessor::parse_date("2025-04-17").is_some());
        assert!(TemplateProcessor::parse_date("2025-04-17T00:00:00Z").is_some());
        assert!(TemplateProcessor::parse_date("not a date").is_none());
    }
}
