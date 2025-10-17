use codas::types::Text;
use logos::{Lexer, Logos};

/// Tokenizer for text assets containing template expressions.
#[derive(Logos, Debug, PartialEq, Eq, Clone)]
pub enum Token {
    /// Opening brace of a template expression.
    #[token(r#"~{"#, parse_template_expression)]
    Template(Result<TemplateExpression, String>),
}

/// Tokenizer for an indiviudal template expression.
#[derive(Logos, Debug, PartialEq, Eq, Clone)]
#[logos(skip r"[ \t\n\f]+")]
enum TemplateToken {
    /// An identifier starting with a letter,
    /// followed by letters, numbers, underscores,
    /// or periods (for dotted identifiers)
    #[regex(r"[a-zA-Z][a-zA-Z0-9_.]*")]
    Identifier,

    /// A string literal enclosed in double quotes,
    #[regex(r#""([^"\\]|\\.)*""#)]
    String,

    /// Negates a conditional.
    #[token(r#"!"#)]
    Negation,

    /// Opening paren of a function call.
    #[token(r#"("#)]
    OpenParen,

    /// Closing paren of a function call.
    #[token(r#")"#)]
    CloseParen,

    /// Closing brace of a template expression.
    #[token(r#"}"#)]
    ExitTemplate,
}

/// A template expression parsed from a series of [TemplateToken]s.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TemplateExpression {
    /// A variable identifier.
    Variable { name: Text },

    /// A function call with a list of string arguments.
    FunctionCall { name: Text, args: Vec<Text> },

    /// An if block over an expression.
    IfBlock {
        expression: Box<TemplateExpression>,
        negated: bool,
    },

    /// A for loop over an iterable expression.
    ForBlock {
        loop_variable: Text,
        iterable: Box<TemplateExpression>,
    },

    /// Marks the end of a [TemplateExpression::IfBlock] or
    /// [TemplateExpression::ForBlock].
    EndBlock,
}

/// Parses a series of [TemplateToken]s into a [TemplateExpression].
fn parse_template_expression(lexer: &mut Lexer<Token>) -> Result<TemplateExpression, String> {
    let mut template_lexer = lexer.clone().morph::<TemplateToken>();

    // The first token must be an identifier.
    let identifier = match template_lexer.next() {
        Some(Ok(TemplateToken::Identifier)) => template_lexer.slice(),
        _ => {
            *lexer = template_lexer.morph();
            return Err("template expression must start with an identifier".to_string());
        }
    };

    match identifier {
        // An if block.
        "if" => {
            let mut negated = false;

            let identifier = match template_lexer.next() {
                Some(Ok(TemplateToken::Negation)) => {
                    negated = true;
                    match template_lexer.next() {
                        Some(Ok(TemplateToken::Identifier)) => template_lexer.slice(),
                        _ => {
                            *lexer = template_lexer.morph();
                            return Err("expected identifier after negation".to_string());
                        }
                    }
                }
                Some(Ok(TemplateToken::Identifier)) => template_lexer.slice(),
                _ => {
                    *lexer = template_lexer.morph();
                    return Err("expected identifier or negation after if".to_string());
                }
            };

            check_exit_token(lexer, template_lexer)?;
            Ok(TemplateExpression::IfBlock {
                expression: Box::new(TemplateExpression::Variable {
                    name: identifier.into(),
                }),
                negated,
            })
        }

        // A for .. in .. loop.
        "for" => {
            // The next token must be the loop variable identifier.
            let loop_variable = match template_lexer.next() {
                Some(Ok(TemplateToken::Identifier)) => template_lexer.slice().into(),
                _ => {
                    *lexer = template_lexer.morph();
                    return Err("expected identifier after for".to_string());
                }
            };

            // The next token must be "in".
            match template_lexer.next() {
                Some(Ok(TemplateToken::Identifier)) if template_lexer.slice() == "in" => {}
                _ => {
                    *lexer = template_lexer.morph();
                    return Err("expected 'in' after loop variable".to_string());
                }
            }

            // The next token must be the iterable identifier.
            let iterable = match template_lexer.next() {
                Some(Ok(TemplateToken::Identifier)) => TemplateExpression::Variable {
                    name: template_lexer.slice().into(),
                },
                _ => {
                    *lexer = template_lexer.morph();
                    return Err("expected identifier after 'in'".to_string());
                }
            };

            check_exit_token(lexer, template_lexer)?;
            Ok(TemplateExpression::ForBlock {
                loop_variable,
                iterable: Box::new(iterable),
            })
        }

        // End of a block.
        "end" => {
            check_exit_token(lexer, template_lexer)?;
            Ok(TemplateExpression::EndBlock)
        }

        // A variable or function identifier.
        identifier => {
            match template_lexer.next() {
                // A function call.
                Some(Ok(TemplateToken::OpenParen)) => {
                    let mut args = vec![];

                    while let Some(Ok(token)) = template_lexer.next() {
                        match token {
                            TemplateToken::Identifier | TemplateToken::String => {
                                args.push(template_lexer.slice().into());
                            }
                            TemplateToken::CloseParen => break,
                            _ => {
                                *lexer = template_lexer.morph();
                                return Err(
                                    "unexpected token in function argument list".to_string()
                                );
                            }
                        }
                    }

                    check_exit_token(lexer, template_lexer)?;
                    Ok(TemplateExpression::FunctionCall {
                        name: identifier.into(),
                        args,
                    })
                }

                // A simple variable.
                Some(Ok(TemplateToken::ExitTemplate)) => {
                    *lexer = template_lexer.morph();
                    Ok(TemplateExpression::Variable {
                        name: identifier.into(),
                    })
                }
                _ => {
                    let message = format!(
                        "expected closing brace `}}`; got {}",
                        template_lexer.slice()
                    );
                    *lexer = template_lexer.morph();
                    Err(message)
                }
            }
        }
    }
}

/// Takes the next token off of `template_lexer` and returns `Ok` iff it is
/// a [TemplateToken::ExitTemplate]. Otherwise, returns an `Err`. In either
/// case, re-assigns `template_lexer` back to `lexer`.
fn check_exit_token<'a>(
    lexer: &mut Lexer<'a, Token>,
    mut template_lexer: Lexer<'a, TemplateToken>,
) -> Result<(), String> {
    match template_lexer.next() {
        Some(Ok(TemplateToken::ExitTemplate)) => {
            *lexer = template_lexer.morph();
            Ok(())
        }
        _ => {
            let message = format!(
                "expected closing brace `}}`; got {}",
                template_lexer.slice()
            );
            *lexer = template_lexer.morph();
            Err(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_variables() {
        let mut lexer = Token::lexer(r#"~{ super_dup3r_variable }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::Template(Ok(TemplateExpression::Variable {
                name: "super_dup3r_variable".into(),
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_function_calls() {
        let mut lexer = Token::lexer(r#"~{ concat("hello" " " "world") }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::Template(Ok(TemplateExpression::FunctionCall {
                name: "concat".into(),
                args: vec!["\"hello\"".into(), "\" \"".into(), "\"world\"".into()],
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_if_blocks() {
        let mut lexer = Token::lexer(r#"~{ if is_empty }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::Template(Ok(TemplateExpression::IfBlock {
                expression: Box::new(TemplateExpression::Variable {
                    name: "is_empty".into(),
                }),
                negated: false,
            }))))
        );
        assert_eq!(lexer.next(), None);

        // Negated condition.
        let mut lexer = Token::lexer(r#"~{ if !is_empty }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::Template(Ok(TemplateExpression::IfBlock {
                expression: Box::new(TemplateExpression::Variable {
                    name: "is_empty".into(),
                }),
                negated: true,
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_for_blocks() {
        let mut lexer = Token::lexer(r#"~{ for item in items }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::Template(Ok(TemplateExpression::ForBlock {
                loop_variable: "item".into(),
                iterable: Box::new(TemplateExpression::Variable {
                    name: "items".into(),
                }),
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_end_blocks() {
        let mut lexer = Token::lexer(r#"~{ end }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::Template(Ok(TemplateExpression::EndBlock))))
        );
        assert_eq!(lexer.next(), None);
    }
}
