use codas::types::Text;
use logos::{Lexer, Logos};

/// Tokenizer for text assets containing template expressions.
#[derive(Logos, Debug, PartialEq, Eq, Clone)]
pub enum Token {
    /// Opening brace of a template expression.
    #[token(r#"~{"#, parse_template_expression)]
    Template(Result<TemplateFunction, String>),
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

    /// Closing brace of a template expression.
    #[token(r#"}"#)]
    ExitTemplate,
}

/// A template expression parsed from a series of [TemplateToken]s.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TemplateExpression {
    /// A literal identifier representing a keyword
    /// or variable on the templating context.
    Identifier(Text),

    /// A literal string.
    String(Text),

    /// The beginning of a templated block.
    BlockStart(Text),

    /// A function call with arguments.
    Function {
        name: Text,
        args: Vec<TemplateExpression>,

        ///
        block: bool,
    },

    /// An end block.
    ///
    /// The parser allows multiple blocks to be chained together
    /// (e.g., `if` ... `else` ... `end`).
    End,
}

/// Parses a series of [TemplateToken]s into a [TemplateFunction].
fn parse_template_expression(lexer: &mut Lexer<Token>) -> Result<TemplateFunction, String> {
    let mut template_lexer = lexer.clone().morph::<TemplateToken>();

    // The first token must be a function identifier.
    if let Some(Ok(TemplateToken::Identifier)) = template_lexer.next() {
        let function_identifier = template_lexer.slice();

        // The following tokens up to the end of the template must be arguments.
        let mut args = vec![];
        while let Some(Ok(token)) = template_lexer.next() {
            match token {
                TemplateToken::Identifier => {
                    args.push(TemplateFunctionExpression::Identifier(
                        template_lexer.slice().into(),
                    ));
                }
                TemplateToken::String => {
                    args.push(TemplateFunctionExpression::String(
                        template_lexer.slice().into(),
                    ));
                }
                TemplateToken::ExitTemplate => break,
                _ => {
                    return Err("unexpected token in function argument list".to_string());
                }
            }
        }

        Ok(TemplateFunction {
            name: function_identifier.into(),
            args,
            block: function_identifier == "if" || function_identifier == "for",
        })
    } else {
        Err("template expression must start with an function identifier".to_string())
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
            Some(Ok(Token::Template(Ok(TemplateFunction {
                name: "super_dup3r_variable".into(),
                args: vec![],
                block: false,
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_function_calls() {
        let mut lexer = Token::lexer(r#"~{ (concat "hello" " " "world") }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::Template(Ok(TemplateFunction {
                name: "concat".into(),
                args: vec![
                    TemplateFunctionExpression::String("hello".into()),
                    TemplateFunctionExpression::String(" ".into()),
                    TemplateFunctionExpression::String("world".into()),
                ],
                block: false,
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_if_blocks() {
        let mut lexer = Token::lexer(r#"~{ (if is_empty) }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::Template(Ok(TemplateFunction {
                name: "if".into(),
                args: vec![TemplateFunctionExpression::Identifier("is_empty".into())],
                block: true
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_for_blocks() {
        let mut lexer = Token::lexer(r#"~{ (for item in items) }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::Template(Ok(TemplateFunction {
                name: "for".into(),
                args: vec![
                    TemplateFunctionExpression::Identifier("item".into()),
                    TemplateFunctionExpression::Identifier("in".into()),
                    TemplateFunctionExpression::Identifier("items".into()),
                ],
                block: true
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_end_blocks() {
        let mut lexer = Token::lexer(r#"~{ (end) }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::Template(Ok(TemplateFunction {
                name: "end".into(),
                args: vec![],
                block: false
            }))))
        );
        assert_eq!(lexer.next(), None);
    }
}
