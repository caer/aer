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
    /// An identifier representing a variable on the template context.
    Identifier { name: Text },

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

    // The first token must be an identifier or a function call.
    match template_lexer.next() {
        // If it's an identifier, then the expression is either
        // a variable or a block (if, for, end).
        Some(Ok(TemplateToken::Identifier)) => {
            let identifier = template_lexer.slice();
            match identifier {
                "if" => {
                    // The next token must be an identifier.
                    let identifier = take_next_identifier(&mut template_lexer, "if")?;
                    check_exit_token(lexer, template_lexer)?;
                    Ok(TemplateExpression::IfBlock {
                        expression: Box::new(TemplateExpression::Identifier {
                            name: identifier.into(),
                        }),
                        negated: false,
                    })
                }
                "for" => {
                    // The next token must be an identifier.
                    let identifier = take_next_identifier(&mut template_lexer, "for")?;

                    // The next token must be "in".
                    match template_lexer.next() {
                        Some(Ok(TemplateToken::Identifier)) if template_lexer.slice() == "in" => {}
                        _ => {
                            return Err("expected 'in' after loop variable".to_string());
                        }
                    }

                    // The next token must be the iterable identifier.
                    let iterable = take_next_identifier(&mut template_lexer, "in")?;
                    check_exit_token(lexer, template_lexer)?;
                    Ok(TemplateExpression::ForBlock {
                        loop_variable: identifier.into(),
                        iterable: Box::new(TemplateExpression::Identifier {
                            name: iterable.into(),
                        }),
                    })
                }
                "end" => {
                    check_exit_token(lexer, template_lexer)?;
                    Ok(TemplateExpression::EndBlock)
                }
                variable => {
                    check_exit_token(lexer, template_lexer)?;
                    Ok(TemplateExpression::Identifier {
                        name: variable.into(),
                    })
                }
            }
        }

        // If it's an open paren, then the expression is a function call.
        Some(Ok(TemplateToken::OpenParen)) => {
            // The next token must be the function name identifier.
            let identifier = take_next_identifier(&mut template_lexer, "(")?;

            // The following tokens up to the closing paren must be
            // function arguments (identifiers or string literals).
            let mut args = vec![];
            while let Some(Ok(token)) = template_lexer.next() {
                match token {
                    TemplateToken::Identifier | TemplateToken::String => {
                        args.push(template_lexer.slice().into());
                    }
                    TemplateToken::CloseParen => break,
                    _ => {
                        return Err("unexpected token in function argument list".to_string());
                    }
                }
            }

            check_exit_token(lexer, template_lexer)?;
            Ok(TemplateExpression::FunctionCall {
                name: identifier.into(),
                args,
            })
        }

        _ => Err("template expression must start with an identifier or function call".to_string()),
    }
}

/// Takes the next token off of `template_lexer` and
/// returns `Ok` iff it is a [TemplateToken::Identifier].
fn take_next_identifier<'a>(
    template_lexer: &mut Lexer<'a, TemplateToken>,
    after_token: &str,
) -> Result<&'a str, String> {
    match template_lexer.next() {
        Some(Ok(TemplateToken::Identifier)) => Ok(template_lexer.slice()),
        _ => Err(format!(
            "expected identifier after `{after_token}`; got {}",
            template_lexer.slice()
        )),
    }
}

/// Takes the next token off of `template_lexer` and returns `Ok` iff it is
/// a [TemplateToken::ExitTemplate], re-assigning `template_lexer` back to `lexer`.
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
            Some(Ok(Token::Template(Ok(TemplateExpression::Identifier {
                name: "super_dup3r_variable".into(),
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_function_calls() {
        let mut lexer = Token::lexer(r#"~{ (concat "hello" " " "world") }"#);
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
                expression: Box::new(TemplateExpression::Identifier {
                    name: "is_empty".into(),
                }),
                negated: false,
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
                iterable: Box::new(TemplateExpression::Identifier {
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
