use markdown::mdast::Node;

use crate::asset::{media_type::MediaType, Error, ProcessesAssets};

pub struct MarkdownProcessor {}

impl ProcessesAssets for MarkdownProcessor {
    fn process(&self, asset: &mut super::Asset) -> Result<(), Error> {
        let text = asset.contents.try_as_mut_text()?;

        // Compile markdown into an abstract syntax tree.
        let ast = markdown::to_mdast(text, &markdown::ParseOptions::default())?;

        // Compile the AST into HTML.
        let mut compiled_html = String::with_capacity(text.len());
        compile_ast_node(None, &ast, &mut compiled_html);

        // Update the asset's contents and target extension.
        *text.to_mut() = compiled_html;
        asset.media_type = MediaType::Html;
        Ok(())
    }
}

/// Compiles a Markdown AST `node` associated
/// with an `asset` into `compiled_html`.
fn compile_ast_node(parent_node: Option<&Node>, node: &Node, compiled_html: &mut String) {
    match node {
        // Document root node.
        Node::Root(_) => {
            compile_ast_node_children(node, compiled_html);
        }

        // Paragraphs.
        Node::Paragraph(_) => {
            *compiled_html += "<p>";
            compile_ast_node_children(node, compiled_html);
            *compiled_html += "</p>";
        }

        // Blockquotes.
        Node::Blockquote(_) => {
            *compiled_html += "<Blockquote>";
            compile_ast_node_children(node, compiled_html);
            *compiled_html += "</Blockquote>";
        }

        // Ordered and unordered lists.
        Node::List(list) => {
            if list.ordered {
                *compiled_html += "<ol>";
            } else {
                *compiled_html += "<ul>";
            }

            compile_ast_node_children(node, compiled_html);

            if list.ordered {
                *compiled_html += "</ol>";
            } else {
                *compiled_html += "</ul>";
            }
        }

        // List items.
        Node::ListItem(_) => {
            *compiled_html += "<li>";
            compile_ast_node_children(node, compiled_html);
            *compiled_html += "</li>";
        }

        // Headers.
        Node::Heading(heading) => {
            *compiled_html += "<h";
            *compiled_html += &heading.depth.to_string();

            // FIXME: Extended markdown behavior.
            // Convert the _entire_ heading contents
            // to a string, stripping any nested formatting.
            let heading_str = node.to_string();

            // Convert the contents into a sanitized anchor tag.
            let mut id = String::with_capacity(heading_str.len());
            for char in heading_str.chars() {
                if char.is_ascii_alphanumeric() {
                    id.push(char.to_ascii_lowercase())
                } else if id.chars().last().is_some_and(|c| c != '-') {
                    id.push('-');
                }
            }

            // Associate the anchor tag as the header's ID.
            *compiled_html += " id=\"";
            *compiled_html += &id;
            *compiled_html += "\">";

            // Compile the actual header contents.
            compile_ast_node_children(node, compiled_html);

            *compiled_html += "</h";
            *compiled_html += &heading.depth.to_string();
            *compiled_html += ">";
        }

        // Italic text.
        Node::Emphasis(_) => {
            *compiled_html += "<em>";
            compile_ast_node_children(node, compiled_html);
            *compiled_html += "</em>";
        }

        // Bold text.
        Node::Strong(_) => {
            *compiled_html += "<strong>";
            compile_ast_node_children(node, compiled_html);
            *compiled_html += "</strong>";
        }

        // Inline link.
        Node::Link(link) => {
            let link_url = &link.url;

            // Emit HTML.
            *compiled_html += "<a href=\"";
            *compiled_html += &link_url.replace('\"', "").replace("\\\"", "");
            if let Some(title) = link.title.as_ref() {
                *compiled_html += "\" title=\"";
                *compiled_html += &title.replace('\"', "&quot;").replace("\\\"", "&quot;");
            }
            *compiled_html += "\">";
            compile_ast_node_children(node, compiled_html);
            *compiled_html += "</a>";
        }

        // Inline image.
        Node::Image(image) => {
            let image_url = &image.url;

            // Emit HTML.
            *compiled_html += "<img alt=\"";
            *compiled_html += &image.alt.replace('\"', "&quot;").replace("\\\"", "&quot;");
            *compiled_html += "\" src=\"";
            *compiled_html += image_url;
            if let Some(title) = image.title.as_ref() {
                *compiled_html += "\" title=\"";
                *compiled_html += &title.replace('\"', "&quot;").replace("\\\"", "&quot;");
            }
            *compiled_html += "\">";
        }

        // Break (line break).
        Node::Break(_) => {
            *compiled_html += "<br/>";
        }

        // Thematic break (horizontal rule).
        Node::ThematicBreak(_) => {
            *compiled_html += "<hr/>";
        }

        // Raw HTML.
        Node::Html(html) => {
            *compiled_html += &html.value;
        }

        // Raw text.
        Node::Text(text) => {
            // FIXME: Extended markdown behavior.
            // If this text is a direct descendant of a
            // block-level text node, convert `--` to
            // em dashes (`—`).
            if matches!(parent_node, Some(Node::Paragraph(..))) {
                *compiled_html += &text.value.replace("--", "—");
            } else {
                *compiled_html += &text.value;
            }
        }

        // Inline code.
        Node::InlineCode(code) => {
            *compiled_html += "<code>";
            *compiled_html += &code.value;
            *compiled_html += "</code>";
        }

        // Fenced code block.
        Node::Code(code) => {
            // FIXME: Extended markdown behavior.
            if let Some(lang) = &code.lang {
                *compiled_html += "<pre rel=\"";
                *compiled_html += lang;
                *compiled_html += "\"><code class=\"language-";
                *compiled_html += lang;
                *compiled_html += "\">";
            } else {
                *compiled_html += "<pre><code>";
            }

            *compiled_html += &code.value;
            *compiled_html += "</code></pre>";
        }

        // GFM strikethrough extension.
        Node::Delete(_) => {
            *compiled_html += "<s>";
            compile_ast_node_children(node, compiled_html);
            *compiled_html += "</s>";
        }

        // Definitions are unsupported.
        Node::Definition(_) => unimplemented!("definition"),

        // References are unsupported.
        Node::FootnoteDefinition(_)
        | Node::FootnoteReference(_)
        | Node::LinkReference(_)
        | Node::ImageReference(_) => unimplemented!("reference"),

        // Tables are unsupported.
        Node::Table(_) | Node::TableRow(_) | Node::TableCell(_) => unimplemented!("table"),

        // Embedded languages are unsupported.
        Node::InlineMath(_)
        | Node::Math(_)
        | Node::MdxJsxFlowElement(_)
        | Node::MdxJsxTextElement(_)
        | Node::MdxjsEsm(_)
        | Node::MdxTextExpression(_)
        | Node::MdxFlowExpression(_)
        | Node::Toml(_)
        | Node::Yaml(_) => unimplemented!("embedded language"),
    }
}

/// Compiles all the children of `node` associated
/// with an `asset` into `compiled_html`.
fn compile_ast_node_children(node: &Node, compiled_html: &mut String) {
    for child in node.children().unwrap() {
        compile_ast_node(Some(node), child, compiled_html);
    }
}

#[cfg(test)]
mod tests {
    use crate::asset::Asset;

    use super::*;

    #[test]
    fn processes_markdown() {
        let mut markdown_asset = Asset::new(
            "test.md".into(),
            "# Header 1\nBody\n> Quotation in **bold** and _italics_."
                .as_bytes()
                .to_vec(),
        );

        let _ = MarkdownProcessor {}.process(&mut markdown_asset);

        assert_eq!(
            "<h1 id=\"header-1\">Header 1</h1><p>Body</p><Blockquote><p>Quotation in <strong>bold</strong> and <em>italics</em>.</p></Blockquote>",
            markdown_asset.contents.try_as_mut_text().unwrap()
        );
    }
}
