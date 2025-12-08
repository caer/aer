use codas::types::Text;
use logos::{Lexer, Logos};

use crate::proc::ProcessingError;

/// Tokenizer for text assets containing template expressions.
#[derive(Logos, Debug, PartialEq, Eq, Clone)]
pub enum Token {
    /// Opening brace of a template expression.
    #[token(r#"~{"#, parse_template_expression)]
    OpenTemplate(Result<TemplateExpression, String>),
}

/// Tokenizer for an indiviudal template expression.
#[derive(Logos, Debug, PartialEq, Eq, Clone)]
#[logos(skip r"[ \t\n\f]+")]
enum TemplateToken {
    /// An identifier starting with a letter or hash,
    /// followed by letters, numbers, underscores,
    /// or periods (for dotted identifiers)
    #[regex(r"[a-zA-Z#][a-zA-Z0-9_\.]*")]
    Identifier,

    /// A string literal enclosed in double quotes,
    #[regex(r#""([^"\\]|\\.)*""#)]
    String,

    /// Closing brace of a template expression.
    #[token(r#"}"#)]
    CloseTemplate,
}

/// A template expression parsed from a series of [TemplateToken]s.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TemplateExpression {
    /// A literal identifier representing a keyword
    /// or variable on the templating context.
    Identifier(Text),

    /// A literal string.
    String(Text),

    /// A function call with arguments.
    Function {
        name: Text,
        args: Vec<TemplateExpression>,
    },
}

impl TemplateExpression {
    pub fn try_as_identifier(&self) -> Result<Text, ProcessingError> {
        match self {
            TemplateExpression::Identifier(value) => Ok(value.clone()),
            expression => Err(ProcessingError::Compilation {
                message: format!("expected identifier; got {:?}", expression).into(),
            }),
        }
    }
}

/// Parses a series of [TemplateToken]s into a [TemplateExpression].
fn parse_template_expression(lexer: &mut Lexer<Token>) -> Result<TemplateExpression, String> {
    let mut template_lexer = lexer.clone().morph::<TemplateToken>();

    // The first token must be a function identifier.
    let next = template_lexer.next();
    if let Some(Ok(TemplateToken::Identifier)) = next {
        let identifier = template_lexer.slice();

        // The following tokens up to the end of the template must be arguments.
        let mut args = vec![];
        while let Some(Ok(token)) = template_lexer.next() {
            match token {
                TemplateToken::Identifier => {
                    args.push(TemplateExpression::Identifier(
                        template_lexer.slice().into(),
                    ));
                }
                TemplateToken::String => {
                    let slice = template_lexer.slice();
                    // Remove the surrounding quotes and unescape.
                    let unescaped = slice[1..slice.len() - 1]
                        .replace(r#"\""#, r#"""#)
                        .replace(r#"\n"#, "\n")
                        .replace(r#"\t"#, "\t")
                        .replace(r#"\\"#, r#"\"#);
                    args.push(TemplateExpression::String(unescaped.into()));
                }
                TemplateToken::CloseTemplate => {
                    *lexer = template_lexer.morph();
                    break;
                }
            }
        }

        Ok(TemplateExpression::Function {
            name: identifier.into(),
            args,
        })
    } else {
        Err(format!(
            "expected function identifier at start of template expression, got: {:?}",
            next
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_variables() {
        let mut lexer = Token::lexer(r#"~{ # super_dup3r_variable }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::OpenTemplate(Ok(TemplateExpression::Function {
                name: "#".into(),
                args: vec![TemplateExpression::Identifier(
                    "super_dup3r_variable".into()
                )],
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_function_calls() {
        let mut lexer = Token::lexer(r#"~{ concat "hello" " " "world" }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::OpenTemplate(Ok(TemplateExpression::Function {
                name: "concat".into(),
                args: vec![
                    TemplateExpression::String("hello".into()),
                    TemplateExpression::String(" ".into()),
                    TemplateExpression::String("world".into()),
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
            Some(Ok(Token::OpenTemplate(Ok(TemplateExpression::Function {
                name: "if".into(),
                args: vec![TemplateExpression::Identifier("is_empty".into())],
            }))))
        );
        assert_eq!(lexer.next(), None);
    }

    #[test]
    fn lexes_for_blocks() {
        let mut lexer = Token::lexer(r#"~{ for item in items }"#);
        assert_eq!(
            lexer.next(),
            Some(Ok(Token::OpenTemplate(Ok(TemplateExpression::Function {
                name: "for".into(),
                args: vec![
                    TemplateExpression::Identifier("item".into()),
                    TemplateExpression::Identifier("in".into()),
                    TemplateExpression::Identifier("items".into()),
                ],
            }))))
        );
        assert_eq!(lexer.next(), None);
    }
}
