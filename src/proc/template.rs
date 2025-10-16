use logos::{Lexer, Logos};

/// Tokenizer for text assets containing template expressions.
#[allow(dead_code)]
#[derive(Logos, Debug, PartialEq, Eq, Clone)]
enum Token {
    /// Opening brace of a template expression.
    #[token(r#"~{"#, enter_template_lexer)]
    Template(Result<TemplateExpression, String>),

    /// Any other content.
    Content,
}

/// Tokenizer for an indiviudal template expression.
#[derive(Logos, Debug, PartialEq, Eq, Clone)]
#[logos(skip r"[ \t\n\f]+")]
enum TemplateToken {
    /// An identifier starting with a letter,
    /// followed by letters, numbers, or underscores.
    #[regex(r"[a-zA-Z][a-zA-Z0-9_]*")]
    Identifier,

    /// A string literal enclosed in double quotes,
    #[regex(r#""([^"\\]|\\.)*""#)]
    String,

    /// Negates a conditional.
    #[token(r#"!"#)]
    Negation,

    /// Opening paren of a function call argument list.
    #[token(r#"("#)]
    OpenParen,

    /// Closing paren of a function call argument list.
    #[token(r#")"#)]
    CloseParen,

    /// Closing brace of a template expression.
    #[token(r#"}"#)]
    ExitTemplate,

    /// Any unrecognized content ends the input.
    #[allow(dead_code)]
    EndOfInput,
}

/// A template expression parsed from a series of [TemplateToken]s.
#[derive(Debug, PartialEq, Eq, Clone)]
enum TemplateExpression {
    /// A variable identifier.
    Variable { name: String },

    /// A function call with a list of string arguments.
    FunctionCall { name: String, args: Vec<String> },

    /// An if block over an expression.
    IfBlock {
        expression: Box<TemplateExpression>,
        negated: bool,
    },

    /// A for loop over an iterable expression.
    ForBlock {
        loop_variable: String,
        iterable: Box<TemplateExpression>,
    },

    /// Marks the end of a [TemplateExpression::IfBlock] or
    /// [TemplateExpression::ForBlock].
    EndBlock,
}

fn enter_template_lexer(lexer: &mut Lexer<Token>) -> Result<TemplateExpression, String> {
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

            match template_lexer.next() {
                Some(Ok(TemplateToken::ExitTemplate)) => {
                    *lexer = template_lexer.morph();
                    Ok(TemplateExpression::IfBlock {
                        expression: Box::new(TemplateExpression::Variable {
                            name: identifier.to_string(),
                        }),
                        negated,
                    })
                }
                _ => {
                    *lexer = template_lexer.morph();
                    Err("unexpected token after identifier".to_string())
                }
            }
        }

        // A for .. in .. loop.
        "for" => {
            // The next tokon must be the loop variable identifier.
            let loop_variable = match template_lexer.next() {
                Some(Ok(TemplateToken::Identifier)) => template_lexer.slice().to_string(),
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
                    name: template_lexer.slice().to_string(),
                },
                _ => {
                    *lexer = template_lexer.morph();
                    return Err("expected identifier after 'in'".to_string());
                }
            };

            match template_lexer.next() {
                Some(Ok(TemplateToken::ExitTemplate)) => {
                    *lexer = template_lexer.morph();
                    Ok(TemplateExpression::ForBlock {
                        loop_variable,
                        iterable: Box::new(iterable),
                    })
                }
                _ => {
                    *lexer = template_lexer.morph();
                    Err("unexpected token after identifier".to_string())
                }
            }
        }

        // End of a block.
        "end" => match template_lexer.next() {
            Some(Ok(TemplateToken::ExitTemplate)) => {
                *lexer = template_lexer.morph();
                Ok(TemplateExpression::EndBlock)
            }
            _ => {
                *lexer = template_lexer.morph();
                Err("unexpected token after identifier".to_string())
            }
        },

        // A variable or function identifier.
        identifier => {
            match template_lexer.next() {
                // A function call.
                Some(Ok(TemplateToken::OpenParen)) => {
                    let mut args = vec![];

                    while let Some(Ok(token)) = template_lexer.next() {
                        match token {
                            TemplateToken::Identifier | TemplateToken::String => {
                                args.push(template_lexer.slice().to_string());
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

                    match template_lexer.next() {
                        Some(Ok(TemplateToken::ExitTemplate)) => {
                            *lexer = template_lexer.morph();
                            Ok(TemplateExpression::FunctionCall {
                                name: identifier.to_string(),
                                args,
                            })
                        }
                        _ => {
                            *lexer = template_lexer.morph();
                            Err("unexpected token after function call".to_string())
                        }
                    }
                }

                // A simple variable.
                Some(Ok(TemplateToken::ExitTemplate)) => {
                    *lexer = template_lexer.morph();
                    Ok(TemplateExpression::Variable {
                        name: identifier.to_string(),
                    })
                }
                _ => {
                    *lexer = template_lexer.morph();
                    Err("unexpected token after identifier".to_string())
                }
            }
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
                name: "super_dup3r_variable".to_string(),
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
                name: "concat".to_string(),
                args: vec![
                    "\"hello\"".to_string(),
                    "\" \"".to_string(),
                    "\"world\"".to_string()
                ],
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
                    name: "is_empty".to_string(),
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
                    name: "is_empty".to_string(),
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
                loop_variable: "item".to_string(),
                iterable: Box::new(TemplateExpression::Variable {
                    name: "items".to_string(),
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
