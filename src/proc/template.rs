use std::collections::BTreeMap;

use codas::types::Text;
use logos::{Lexer, Logos, Span};

use crate::proc::{Asset, MediaCategory, ProcessesAssets, ProcessingError};

mod tokenizer;

use tokenizer::{TemplateExpression, Token};

/// Processes text assets containing template expressions wrapped in
/// `~{ }`, drawing values from a context of key-value pairs.
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
pub struct TemplateProcessor {
    /// Context containing variables that can be used by templates.
    context: BTreeMap<Text, TemplateValue>,
}

impl ProcessesAssets for TemplateProcessor {
    fn process(&self, asset: &mut Asset) -> Result<(), ProcessingError> {
        if asset.media_type().category() != MediaCategory::Text {
            tracing::debug!(
                "skipping asset {}: not text {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        let template = asset.as_text()?;
        let mut lexer = Token::lexer(template.as_str());
        let mut output = String::with_capacity(template.len());
        self.compile_template(&mut lexer, &mut output)?;
        asset.replace_with_text(output.into(), asset.media_type().clone());

        Ok(())
    }
}

impl TemplateProcessor {
    /// Compiles a text template containing zero or more [TemplateExpression]s,
    /// appending the compiled results to `output`.
    fn compile_template(
        &self,
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
                        // Variable reference: ~{ # variable_name }s
                        "#" => {
                            let identifier = args[0].try_as_identifier()?;

                            let value = match self.context.get(&identifier) {
                                Some(TemplateValue::Text(text)) => text.clone(),
                                Some(TemplateValue::List(items)) => {
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
                            let identifier = args[0].try_as_identifier()?;

                            // A variable reference is "truthy" if it exists and is not "false" or "0".
                            let truthy = match self.context.get(&identifier) {
                                Some(TemplateValue::Text(text)) => {
                                    text != "false" && text != "0" && !text.is_empty()
                                }
                                Some(TemplateValue::List(list)) => !list.is_empty(),
                                None => false,
                            };

                            // If the condition is truthy, compile the contents of the block.
                            let block_span = Self::traverse_template_block(lexer)?;
                            if truthy {
                                let block_text = &lexer.source()[block_span];
                                let mut block_lexer = Token::lexer(block_text);
                                self.compile_template(&mut block_lexer, output)?;
                            }
                        }

                        // For loop: ~{ for item in items } ... ~{ end }
                        "for" => {
                            let item_identifier = args[0].try_as_identifier()?;
                            let collection_identifier = args[2].try_as_identifier()?;
                            let collection = self.context.get(&collection_identifier);

                            let block_span = Self::traverse_template_block(lexer)?;
                            if let Some(TemplateValue::List(items)) = collection
                                && !items.is_empty()
                            {
                                let block_text = &lexer.source()[block_span];

                                for item in items {
                                    let mut loop_context = self.context.clone();
                                    loop_context.insert(
                                        item_identifier.clone(),
                                        TemplateValue::Text(item.clone()),
                                    );

                                    let loop_processor = TemplateProcessor {
                                        context: loop_context,
                                    };
                                    let mut block_lexer = Token::lexer(block_text);
                                    loop_processor.compile_template(&mut block_lexer, output)?;
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

/// Value types used in [TemplateProcessor] contexts.
#[derive(Debug, Clone)]
pub enum TemplateValue {
    Text(Text),
    List(Vec<Text>),
}

#[cfg(test)]
mod tests {

    use crate::proc::{Asset, MediaType};

    use super::*;

    #[test]
    fn processes_if_template() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"~{if is_empty}This is empty!~{end}"#.trim().as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        TemplateProcessor {
            context: [("is_empty".into(), TemplateValue::Text("true".into()))].into(),
        }
        .process(&mut asset)
        .unwrap();

        assert_eq!(r#"This is empty!"#, asset.as_text().unwrap());
    }

    #[test]
    fn processes_for_template() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"Items: [~{for item in items}~{# item}, ~{end}]"#.trim().as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        TemplateProcessor {
            context: [(
                "items".into(),
                TemplateValue::List(vec!["apple".into(), "banana".into(), "cherry".into()]),
            )]
            .into(),
        }
        .process(&mut asset)
        .unwrap();

        assert_eq!(
            r#"Items: [apple, banana, cherry, ]"#,
            asset.as_text().unwrap()
        );
    }
}
