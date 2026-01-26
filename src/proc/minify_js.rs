use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_minifier::{Minifier, MinifierOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;

use super::{Asset, Context, MediaType, ProcessesAssets, ProcessingError};

/// Minifies JavaScript assets by removing unnecessary whitespace and comments.
///
/// Assets with paths ending in `.min.js` are skipped (already minified).
pub struct MinifyJsProcessor;

impl ProcessesAssets for MinifyJsProcessor {
    fn process(&self, _context: &mut Context, asset: &mut Asset) -> Result<(), ProcessingError> {
        if asset.media_type() != &MediaType::JavaScript {
            tracing::debug!(
                "skipping asset {}: not JavaScript: {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        if asset.path().ends_with(".min.js") {
            tracing::debug!("skipping asset {}: already minified", asset.path());
            return Ok(());
        }

        let source = asset.as_text()?;

        let allocator = Allocator::default();
        let source_type = SourceType::mjs();

        // Parse the JavaScript source.
        let ret = Parser::new(&allocator, source, source_type).parse();

        // Check for parse errors.
        if !ret.errors.is_empty() {
            let error_messages: Vec<_> = ret.errors.iter().map(|e| e.to_string()).collect();
            return Err(ProcessingError::Compilation {
                message: format!("JS parse errors: {}", error_messages.join("; ")).into(),
            });
        }

        // Minify the AST.
        let mut program = ret.program;
        let options = MinifierOptions::default();
        Minifier::new(options).minify(&allocator, &mut program);

        // Generate minified output (removes whitespace and comments).
        let output = Codegen::new()
            .with_options(CodegenOptions::minify())
            .build(&program);

        asset.replace_with_text(output.code.into(), MediaType::JavaScript);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minifies_js() {
        let js = r#"
            // This is a comment
            function hello(name) {
                /* Another comment */
                console.log("Hello, " + name);
            }
            hello("world");
        "#;
        let mut asset = Asset::new("script.js".into(), js.as_bytes().to_vec());
        MinifyJsProcessor
            .process(&mut Context::default(), &mut asset)
            .unwrap();

        let result = asset.as_text().unwrap();
        // Comments should be stripped.
        assert!(!result.contains("// This is a comment"));
        assert!(!result.contains("/* Another comment */"));
        // String literals and built-ins should be preserved.
        assert!(result.contains("Hello, "));
        assert!(result.contains("console.log"));
        // Should be smaller than original.
        assert!(result.len() < js.len());
    }

    #[test]
    fn skips_non_js() {
        let mut asset = Asset::new("index.html".into(), b"<html></html>".to_vec());
        MinifyJsProcessor
            .process(&mut Context::default(), &mut asset)
            .unwrap();
        assert_eq!(asset.as_text().unwrap(), "<html></html>");
    }

    #[test]
    fn skips_already_minified() {
        let js = "function test(){console.log('already minified')}";
        let mut asset = Asset::new("vendor.min.js".into(), js.as_bytes().to_vec());
        MinifyJsProcessor
            .process(&mut Context::default(), &mut asset)
            .unwrap();
        // Content should be unchanged.
        assert_eq!(asset.as_text().unwrap(), js);
    }
}
