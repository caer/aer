use std::collections::BTreeMap;

use codas::types::Text;
use logos::{Lexer, Logos, Span};

use crate::proc::{Asset, MediaCategory, ProcessesAssets, ProcessingError};

mod tokenizer;

use tokenizer::{TemplateExpression, Token};

/// Processes text assets containing template expressions wrapped in
/// `~{ }`, drawing values from a context of key-value pairs.
pub struct TemplateProcessor {
    /// Context containing variables that can be used by templates.
    context: BTreeMap<Text, Text>,
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
                Ok(Token::Template(Ok(expression))) => {
                    match expression {
                        TemplateExpression::Identifier { name } => {
                            let value = self
                                .context
                                .get(&name)
                                .cloned()
                                .unwrap_or_else(|| format!("~{{ {name} }}~").into())
                                .to_string();
                            output.push_str(&value);
                        }

                        TemplateExpression::FunctionCall { .. } => todo!(),

                        TemplateExpression::IfBlock {
                            expression,
                            negated,
                        } => {
                            // @caer: todo: We assume if blocks can only contain identifier references. Is that a valid assumption?
                            let identifier = match *expression {
                                TemplateExpression::Identifier { name } => {
                                    self.context.get(&name).cloned()
                                }
                                _ => None,
                            };

                            // A variable reference is "truthy" if it exists and is not "false" or "0".
                            let mut truthy =
                                !matches!(identifier.as_deref(), Some("false") | Some("0") | None);

                            // Invert the truthiness if the condition is negated.
                            if negated {
                                truthy = !truthy;
                            }

                            // If the condition is truthy, compile the contents of the block.
                            if truthy {
                                let block_span = Self::traverse_template_block(lexer)?;
                                let block_text = &lexer.source()[block_span];
                                let mut block_lexer = Token::lexer(block_text);
                                self.compile_template(&mut block_lexer, output)?;
                            }
                        }

                        TemplateExpression::ForBlock { .. } => todo!(),

                        // An end block terminates compilation, but only
                        // if we're inside of a block.
                        TemplateExpression::EndBlock => {
                            if lexer.span().start == 0 {
                                return Err(ProcessingError::Compilation {
                                    message: "unexpected end-of-block".into(),
                                });
                            } else {
                                break;
                            }
                        }
                    };
                }

                // Abort processing if the template contains any errors.
                Ok(Token::Template(Err(err))) => {
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

        while let Some(token) = lexer.next() {
            match token {
                Ok(Token::Template(Ok(TemplateExpression::IfBlock { .. })))
                | Ok(Token::Template(Ok(TemplateExpression::ForBlock { .. }))) => {
                    let _ = Self::traverse_template_block(lexer)?;
                }
                Ok(Token::Template(Ok(TemplateExpression::EndBlock))) => {
                    return Ok(start..lexer.span().end);
                }
                _ => {}
            }
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

    #[test]
    fn processes_if_template() {
        let mut asset = Asset::new(
            "test.html".into(),
            r#"~{ if is_empty }This is empty!~{ end }"#.trim().as_bytes().to_vec(),
        );
        asset.set_media_type(MediaType::Html);

        TemplateProcessor {
            context: [("is_empty".into(), "true".into())].into(),
        }
        .process(&mut asset)
        .unwrap();

        assert_eq!(r#"This is empty!"#, asset.as_text().unwrap());
    }
}
