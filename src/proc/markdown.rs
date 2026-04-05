use std::collections::HashMap;

use markdown::mdast::{AlignKind, Node};
use markdown::message::Message;

use super::{Asset, Environment, LayeredContext, MediaType, ProcessesAssets, ProcessingError};

impl From<Message> for ProcessingError {
    fn from(error: Message) -> Self {
        ProcessingError::Compilation {
            message: error.to_string().into(),
        }
    }
}
pub struct MarkdownProcessor {}

impl ProcessesAssets for MarkdownProcessor {
    fn process(
        &self,
        _env: &Environment,
        _context: &LayeredContext,
        asset: &mut Asset,
    ) -> Result<bool, ProcessingError> {
        if *asset.media_type() != MediaType::Markdown {
            return Ok(false);
        }

        tracing::trace!("markdown: {}", asset.path());

        let text = asset.as_text()?;

        // Compile markdown into an abstract syntax tree.
        let ast = markdown::to_mdast(
            text,
            &markdown::ParseOptions {
                constructs: markdown::Constructs {
                    gfm_footnote_definition: true,
                    gfm_label_start_footnote: true,
                    gfm_table: true,
                    gfm_strikethrough: true,
                    ..markdown::Constructs::default()
                },
                ..markdown::ParseOptions::default()
            },
        )?;

        // Compile the AST into HTML.
        let mut compiled_html = String::with_capacity(text.len());
        let mut state = CompileState::default();
        compile_ast_node(None, &ast, &mut compiled_html, &mut state);

        // Update the asset's contents and target extension.
        asset.replace_with_text(compiled_html.into(), MediaType::Html);
        Ok(true)
    }
}

/// Compiles a Markdown AST `node` associated
/// with an `asset` into `compiled_html`.
fn compile_ast_node(
    parent_node: Option<&Node>,
    node: &Node,
    compiled_html: &mut String,
    state: &mut CompileState,
) {
    match node {
        // Document root node.
        Node::Root(_) => {
            compile_ast_node_children(node, compiled_html, state);
        }

        // Paragraphs.
        Node::Paragraph(_) => {
            *compiled_html += "<p>";
            compile_ast_node_children(node, compiled_html, state);
            *compiled_html += "</p>";
        }

        // Blockquotes.
        Node::Blockquote(_) => {
            *compiled_html += "<blockquote>";
            compile_ast_node_children(node, compiled_html, state);
            *compiled_html += "</blockquote>";
        }

        // Ordered and unordered lists.
        Node::List(list) => {
            if list.ordered {
                *compiled_html += "<ol>";
            } else {
                *compiled_html += "<ul>";
            }

            compile_ast_node_children(node, compiled_html, state);

            if list.ordered {
                *compiled_html += "</ol>";
            } else {
                *compiled_html += "</ul>";
            }
        }

        // List items.
        Node::ListItem(_) => {
            *compiled_html += "<li>";
            compile_ast_node_children(node, compiled_html, state);
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

            // Deduplicate and associate the anchor tag as the header's ID.
            let unique_id = state.heading_ids.unique_id(&id);
            *compiled_html += " id=\"";
            *compiled_html += &unique_id;
            *compiled_html += "\">";

            // Compile the actual header contents.
            compile_ast_node_children(node, compiled_html, state);

            *compiled_html += "</h";
            *compiled_html += &heading.depth.to_string();
            *compiled_html += ">";
        }

        // Italic text.
        Node::Emphasis(_) => {
            *compiled_html += "<em>";
            compile_ast_node_children(node, compiled_html, state);
            *compiled_html += "</em>";
        }

        // Bold text.
        Node::Strong(_) => {
            *compiled_html += "<strong>";
            compile_ast_node_children(node, compiled_html, state);
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
            compile_ast_node_children(node, compiled_html, state);
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
            compile_ast_node_children(node, compiled_html, state);
            *compiled_html += "</s>";
        }

        // Definitions are not yet supported.
        Node::Definition(_) => {
            tracing::warn!("unsupported markdown node: definition");
        }

        // Footnote definition: rendered inline by compile_ast_node_children.
        // This branch handles the content inside the <section> wrapper.
        Node::FootnoteDefinition(def) => {
            let idx = state.footnotes.index_for(&def.identifier);
            let idx_str = idx.to_string();
            *compiled_html += "<li id=\"fn-";
            *compiled_html += &idx_str;
            *compiled_html += "\">";

            // Compile footnote content.
            let mut inner = String::new();
            for child in node.children().unwrap() {
                compile_ast_node(Some(node), child, &mut inner, state);
            }

            // Insert the back-link inside the last <p> tag.
            let backlink = format!(
                " <a href=\"#fnref-{}\" role=\"doc-backlink\">↩</a>",
                idx_str
            );
            if let Some(pos) = inner.rfind("</p>") {
                compiled_html.push_str(&inner[..pos]);
                compiled_html.push_str(&backlink);
                compiled_html.push_str(&inner[pos..]);
            } else {
                compiled_html.push_str(&inner);
                compiled_html.push_str(&backlink);
            }

            *compiled_html += "</li>";
        }

        // Footnote reference: emit a superscript link.
        Node::FootnoteReference(reference) => {
            let idx = state.footnotes.index_for(&reference.identifier);
            let idx_str = idx.to_string();
            *compiled_html += "<sup><a id=\"fnref-";
            *compiled_html += &idx_str;
            *compiled_html += "\" href=\"#fn-";
            *compiled_html += &idx_str;
            *compiled_html += "\" role=\"doc-noteref\">[";
            *compiled_html += &idx_str;
            *compiled_html += "]</a></sup>";
        }

        // Link/image references are not yet supported.
        Node::LinkReference(_) | Node::ImageReference(_) => {
            tracing::warn!("unsupported markdown node: link/image reference");
        }

        // GFM table.
        Node::Table(table) => {
            *compiled_html += "<table>";

            let mut rows = table.children.iter();

            // First row is the header.
            if let Some(header_node) = rows.next() {
                *compiled_html += "<thead><tr>";
                if let Node::TableRow(row) = header_node {
                    for (i, cell) in row.children.iter().enumerate() {
                        emit_cell_tag("th", table.align.get(i), compiled_html);
                        compile_ast_node_children(cell, compiled_html, state);
                        *compiled_html += "</th>";
                    }
                }
                *compiled_html += "</tr></thead>";
            }

            // Remaining rows are the body.
            let body_rows: Vec<_> = rows.collect();
            if !body_rows.is_empty() {
                *compiled_html += "<tbody>";
                for row_node in body_rows {
                    *compiled_html += "<tr>";
                    if let Node::TableRow(row) = row_node {
                        for (i, cell) in row.children.iter().enumerate() {
                            emit_cell_tag("td", table.align.get(i), compiled_html);
                            compile_ast_node_children(cell, compiled_html, state);
                            *compiled_html += "</td>";
                        }
                    }
                    *compiled_html += "</tr>";
                }
                *compiled_html += "</tbody>";
            }

            *compiled_html += "</table>";
        }

        // Table rows and cells are handled by the Table branch above.
        Node::TableRow(_) | Node::TableCell(_) => {
            compile_ast_node_children(node, compiled_html, state);
        }

        // Embedded languages are not yet supported.
        Node::InlineMath(_)
        | Node::Math(_)
        | Node::MdxJsxFlowElement(_)
        | Node::MdxJsxTextElement(_)
        | Node::MdxjsEsm(_)
        | Node::MdxTextExpression(_)
        | Node::MdxFlowExpression(_)
        | Node::Toml(_)
        | Node::Yaml(_) => {
            tracing::warn!("unsupported markdown node: embedded language");
        }
    }
}

/// Compiles all the children of `node` associated
/// with an `asset` into `compiled_html`.
///
/// Consecutive `FootnoteDefinition` nodes are grouped
/// into a single `<section class="footnotes">` block.
fn compile_ast_node_children(node: &Node, compiled_html: &mut String, state: &mut CompileState) {
    let children = node.children().unwrap();
    let mut in_footnote_section = false;

    for child in children {
        let is_footnote = matches!(child, Node::FootnoteDefinition(_));

        if is_footnote && !in_footnote_section {
            // Set the `start` attribute so the <ol> numbering
            // continues from the correct footnote index.
            if let Node::FootnoteDefinition(def) = child {
                let start = state.footnotes.index_for(&def.identifier);
                *compiled_html += "<section class=\"footnotes\" role=\"doc-endnotes\"><ol start=\"";
                *compiled_html += &start.to_string();
                *compiled_html += "\">";
            }
            in_footnote_section = true;
        } else if !is_footnote && in_footnote_section {
            *compiled_html += "</ol></section>";
            in_footnote_section = false;
        }

        compile_ast_node(Some(node), child, compiled_html, state);
    }

    if in_footnote_section {
        *compiled_html += "</ol></section>";
    }
}

/// Emits an opening `<th>` or `<td>` tag with an optional `align` attribute.
fn emit_cell_tag(tag: &str, align: Option<&AlignKind>, compiled_html: &mut String) {
    *compiled_html += "<";
    *compiled_html += tag;
    match align {
        Some(AlignKind::Left) => *compiled_html += " align=\"left\"",
        Some(AlignKind::Right) => *compiled_html += " align=\"right\"",
        Some(AlignKind::Center) => *compiled_html += " align=\"center\"",
        _ => {}
    }
    *compiled_html += ">";
}

/// Mutable state threaded through AST compilation.
#[derive(Default)]
struct CompileState {
    footnotes: Footnotes,
    heading_ids: HeadingIds,
}

/// Tracks footnote numbering during compilation.
#[derive(Default)]
struct Footnotes {
    /// Maps footnote identifier to its 1-based index.
    indices: HashMap<String, usize>,
    /// Counter for assigning footnote numbers.
    count: usize,
}

impl Footnotes {
    /// Returns the 1-based index for a footnote identifier,
    /// assigning a new one if this is the first reference.
    fn index_for(&mut self, identifier: &str) -> usize {
        if let Some(&idx) = self.indices.get(identifier) {
            idx
        } else {
            self.count += 1;
            self.indices.insert(identifier.to_string(), self.count);
            self.count
        }
    }
}

/// Tracks heading IDs for deduplication.
#[derive(Default)]
struct HeadingIds {
    /// Maps base ID to the number of times it has been used.
    counts: HashMap<String, usize>,
}

impl HeadingIds {
    /// Returns a unique heading ID, appending `-2`, `-3`, etc. for
    /// duplicates. The first occurrence gets no suffix.
    fn unique_id(&mut self, base: &str) -> String {
        let count = self.counts.entry(base.to_string()).or_insert(0);
        *count += 1;
        if *count == 1 {
            base.to_string()
        } else {
            format!("{}-{}", base, count)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::LayeredContext;

    fn compile(md: &str) -> String {
        let mut asset = Asset::new("test.md".into(), md.as_bytes().to_vec());
        MarkdownProcessor {}
            .process(
                &Environment::test(),
                &LayeredContext::from_flat(Default::default()),
                &mut asset,
            )
            .unwrap();
        assert_eq!(asset.media_type(), &MediaType::Html);
        asset.as_text().unwrap().to_string()
    }

    #[test]
    fn headers_paragraphs_bold_italic_blockquote() {
        let html = compile("# Header 1\nBody\n> Quotation in **bold** and _italics_.");
        assert_eq!(
            html,
            "<h1 id=\"header-1\">Header 1</h1><p>Body</p><blockquote><p>Quotation in <strong>bold</strong> and <em>italics</em>.</p></blockquote>"
        );
    }

    #[test]
    fn heading_id_deduplication() {
        let html = compile("# Foo\n## Foo\n### Foo");
        assert!(html.contains("id=\"foo\""));
        assert!(html.contains("id=\"foo-2\""));
        assert!(html.contains("id=\"foo-3\""));
    }

    #[test]
    fn inline_links() {
        let html = compile("[Click here](https://example.com)");
        assert_eq!(
            html,
            "<p><a href=\"https://example.com\">Click here</a></p>"
        );
    }

    #[test]
    fn link_with_title() {
        let html = compile(r#"[text](https://example.com "My Title")"#);
        assert!(html.contains(r#"title="My Title""#));
        assert!(html.contains(r#"href="https://example.com""#));
    }

    #[test]
    fn inline_images() {
        let html = compile("![alt text](/img.png)");
        assert_eq!(html, "<p><img alt=\"alt text\" src=\"/img.png\"></p>");
    }

    #[test]
    fn image_with_title() {
        let html = compile(r#"![photo](/img.png "A photo")"#);
        assert!(html.contains(r#"alt="photo""#));
        assert!(html.contains(r#"src="/img.png""#));
        assert!(html.contains(r#"title="A photo""#));
    }

    #[test]
    fn inline_code() {
        let html = compile("Use `println!` to print.");
        assert!(html.contains("<code>println!</code>"));
    }

    #[test]
    fn fenced_code_block() {
        let html = compile("```rust\nfn main() {}\n```");
        assert!(html.contains(r#"<pre rel="rust"><code class="language-rust">"#));
        assert!(html.contains("fn main() {}"));
        assert!(html.contains("</code></pre>"));
    }

    #[test]
    fn fenced_code_block_no_language() {
        let html = compile("```\nplain code\n```");
        assert!(html.contains("<pre><code>plain code</code></pre>"));
    }

    #[test]
    fn unordered_list() {
        let html = compile("- one\n- two\n- three");
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li><p>one</p></li>"));
        assert!(html.contains("<li><p>two</p></li>"));
        assert!(html.contains("<li><p>three</p></li>"));
        assert!(html.contains("</ul>"));
    }

    #[test]
    fn ordered_list() {
        let html = compile("1. first\n2. second");
        assert!(html.contains("<ol>"));
        assert!(html.contains("<li><p>first</p></li>"));
        assert!(html.contains("<li><p>second</p></li>"));
        assert!(html.contains("</ol>"));
    }

    #[test]
    fn gfm_table() {
        let html = compile("| A | B |\n|---|---|\n| 1 | 2 |");
        assert!(html.contains("<table>"));
        assert!(html.contains("<thead>"));
        assert!(html.contains("<th>A</th>"));
        assert!(html.contains("<th>B</th>"));
        assert!(html.contains("<tbody>"));
        assert!(html.contains("<td>1</td>"));
        assert!(html.contains("<td>2</td>"));
    }

    #[test]
    fn gfm_table_alignment() {
        let html = compile("| L | C | R |\n|:--|:-:|--:|\n| a | b | c |");
        assert!(html.contains(r#"<th align="left">L</th>"#));
        assert!(html.contains(r#"<th align="center">C</th>"#));
        assert!(html.contains(r#"<th align="right">R</th>"#));
        assert!(html.contains(r#"<td align="left">a</td>"#));
    }

    #[test]
    fn gfm_strikethrough() {
        let html = compile("~~deleted~~");
        assert!(html.contains("<s>deleted</s>"));
    }

    #[test]
    fn em_dash_conversion() {
        let html = compile("Hello -- world");
        assert!(html.contains("Hello \u{2014} world"));
    }

    #[test]
    fn thematic_break() {
        let html = compile("above\n\n---\n\nbelow");
        assert!(html.contains("<hr/>"));
    }

    #[test]
    fn raw_html_passthrough() {
        let html = compile("<div class=\"custom\">content</div>");
        assert!(html.contains("<div class=\"custom\">content</div>"));
    }

    #[test]
    fn line_break() {
        let html = compile("line one  \nline two");
        assert!(html.contains("<br/>"));
    }

    #[test]
    fn footnotes() {
        let html = compile("Text[^1] and more[^2].\n\n[^1]: First note.\n[^2]: Second note.");
        // References should be superscript links.
        assert!(html.contains(r##"<sup><a id="fnref-1" href="#fn-1""##));
        assert!(html.contains(r##"<sup><a id="fnref-2" href="#fn-2""##));
        // Definitions should be in a footnotes section.
        assert!(html.contains(r##"<section class="footnotes""##));
        assert!(html.contains(r##"<li id="fn-1">"##));
        assert!(html.contains(r##"<li id="fn-2">"##));
        // Back-links should be present.
        assert!(html.contains(r##"href="#fnref-1""##));
        assert!(html.contains(r##"href="#fnref-2""##));
    }

    #[test]
    fn skips_non_markdown() {
        let mut asset = Asset::new("page.html".into(), b"<h1>Hello</h1>".to_vec());
        let modified = MarkdownProcessor {}
            .process(
                &Environment::test(),
                &LayeredContext::from_flat(Default::default()),
                &mut asset,
            )
            .unwrap();
        assert!(!modified);
        assert_eq!(asset.as_text().unwrap(), "<h1>Hello</h1>");
    }
}
