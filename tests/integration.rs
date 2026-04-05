//! Integration tests for full site compilation.
//!
//! These tests verify that the entire asset processing pipeline works
//! correctly with realistic site structures similar to production sites.
//!
//! Each test exercises a specific set of processors or
//! pipeline behaviors through the real `procs::run` entry point.

use std::path::Path;
use std::process::Command;

use tokio::fs;

async fn read(dir: &Path, rel: &str) -> String {
    let path = dir.join(rel);
    fs::read_to_string(&path)
        .await
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

async fn read_bytes(dir: &Path, rel: &str) -> Vec<u8> {
    let path = dir.join(rel);
    fs::read(&path)
        .await
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

async fn exists(dir: &Path, rel: &str) -> bool {
    fs::try_exists(dir.join(rel)).await.unwrap_or(false)
}

/// Write an Aer.toml config with the given processors and optional extras.
async fn write_config(root: &Path, site: &Path, public: &Path, procs: &str, extra: &str) {
    let config = format!(
        r#"
[default.paths]
source = "{}"
target = "{}"

[default.context]
site_name = "Test Site"
year = "2026"
description = "A site for testing."

[default.procs]
{procs}

{extra}
"#,
        site.to_string_lossy(),
        public.to_string_lossy(),
    );
    fs::write(root.join("Aer.toml"), config).await.unwrap();
}

async fn run_aer(root: &Path) {
    aer::tool::procs::run(Some(&root.join("Aer.toml")), None)
        .await
        .unwrap();
}

/// Exercises the core content pipeline for a typical website.
///
/// This pipeline includes:
/// - TOML frontmatter
/// - Markdown compilation
/// - Template variable resolution
/// - Pattern wrapping (nested)
/// - Part inclusion with defaults
/// - Asset collection loops with date formatting
/// - Conditionals
/// - Fallback chains
/// - Context layering (global vs. per-page frontmatter)
#[tokio::test]
async fn template_markdown_pattern_pipeline() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(site.join("parts")).await.unwrap();
    fs::create_dir_all(site.join("patterns")).await.unwrap();
    fs::create_dir_all(site.join("posts")).await.unwrap();

    write_config(
        root,
        &site,
        &public,
        "markdown = {}\ntemplate = {}\npattern = {}",
        "",
    )
    .await;

    // Outer pattern (a base HTML doc)
    fs::write(
        site.join("patterns/_base.html"),
        r#"<!DOCTYPE html>
<html>
<head>
<title>{~ get title} | {~ get site_name}</title>
{~ if no_index}<meta name="robots" content="noindex">{~ end}
</head>
<body>
{~ use "parts/_header.html"}
<main>{~ get content}</main>
<footer>copyright {~ get year}</footer>
</body>
</html>"#,
    )
    .await
    .unwrap();

    // Inner pattern (article layout)
    fs::write(
        site.join("patterns/_article.html"),
        r#"pattern = "patterns/_base.html"

***
<article>
<h1>{~ get title}</h1>
<p class="byline">by {~ get author} on {~ date date "%B %d, %Y"}</p>
<p class="subtitle">{~ get subtitle or description}</p>
{~ if not hide_bio}<p class="bio">About the author.</p>{~ end}
{~ get content}
</article>"#,
    )
    .await
    .unwrap();

    // Header part
    fs::write(
        site.join("parts/_header.html"),
        r#"nav_label = "Main"

***
<header>
<nav aria-label="{~ get nav_label}">[{~ get site_name}]</nav>
</header>"#,
    )
    .await
    .unwrap();

    // Markdown template
    fs::write(
        site.join("index.md"),
        r#"title = "Home"
description = "Welcome to the test site."
pattern = "patterns/_base.html"
show_posts = true

***

# {~ get title}

Welcome to **{~ get site_name}**.

{~ if show_posts}
## Recent Posts

{~ for post in assets "posts" sort date desc}
- {~ get post.title} ({~ date post.date "%Y.%m.%d"})
{~ end}
{~ end}"#,
    )
    .await
    .unwrap();

    // Basic Markdown article
    fs::write(
        site.join("posts/hello.md"),
        r#"title = "Hello World"
author = "Tester"
date = "2025-06-15"
description = "The first post."
pattern = "patterns/_article.html"

***

This is the **first** post."#,
    )
    .await
    .unwrap();

    // Markdown article
    fs::write(
        site.join("posts/second.md"),
        r#"title = "Second Post"
subtitle = "A custom subtitle"
author = "Tester"
date = "2026-01-01"
description = "The second post."
pattern = "patterns/_article.html"
hide_bio = true
no_index = true

***

The **second** post."#,
    )
    .await
    .unwrap();

    run_aer(root).await;

    let index = read(&public, "index.html").await;
    let hello = read(&public, "posts/hello.html").await;
    let second = read(&public, "posts/second.html").await;

    // Markdown compilation.
    assert!(index.contains("<h1"), "markdown heading:\n{index}");
    assert!(hello.contains("<strong>first</strong>"), "bold:\n{hello}");

    // Frontmatter removed.
    assert!(!index.contains("***"), "delimiter leaked:\n{index}");
    assert!(!hello.contains("***"), "delimiter leaked:\n{hello}");

    // Template variable resolution.
    assert!(
        index.contains("<title>Home | Test Site</title>"),
        "title:\n{index}"
    );
    assert!(
        hello.contains("<title>Hello World | Test Site</title>"),
        "title:\n{hello}"
    );
    assert!(
        index.contains("Welcome to <strong>Test Site</strong>"),
        "site_name in body:\n{index}"
    );

    // Pattern wrapping (outer).
    assert!(index.contains("<!DOCTYPE html>"), "doctype:\n{index}");
    assert!(
        index.contains("<footer>copyright 2026</footer>"),
        "footer:\n{index}"
    );

    // Nested pattern (article -> base).
    assert!(hello.contains("<!DOCTYPE html>"), "outer:\n{hello}");
    assert!(hello.contains("<article>"), "inner:\n{hello}");

    // Part inclusion with defaults.
    assert!(index.contains("[Test Site]"), "header part:\n{index}");
    assert!(
        index.contains(r#"aria-label="Main""#),
        "part default:\n{index}"
    );

    // Date formatting.
    assert!(hello.contains("June 15, 2025"), "date:\n{hello}");
    assert!(second.contains("January 01, 2026"), "date:\n{second}");

    // Conditionals.
    assert!(index.contains("Recent Posts"), "if true:\n{index}");
    assert!(!hello.contains("noindex"), "if absent:\n{hello}");
    assert!(
        second.contains(r#"<meta name="robots" content="noindex">"#),
        "if true:\n{second}"
    );
    assert!(
        hello.contains("About the author."),
        "if not false:\n{hello}"
    );
    assert!(
        !second.contains("About the author."),
        "if not true:\n{second}"
    );

    // Fallback chains.
    assert!(
        hello.contains("The first post."),
        "fallback to description:\n{hello}"
    );
    assert!(
        second.contains("A custom subtitle"),
        "subtitle preferred:\n{second}"
    );

    // Context layering (page frontmatter overrides global).
    assert!(
        !index.contains("A site for testing."),
        "global should be overridden:\n{index}"
    );

    // Asset collection loop with date formatting.
    assert!(index.contains("Hello World"), "post in loop:\n{index}");
    assert!(index.contains("Second Post"), "post in loop:\n{index}");
    assert!(index.contains("2025.06.15"), "date in loop:\n{index}");
    assert!(index.contains("2026.01.01"), "date in loop:\n{index}");

    // Sort order: desc by date, so Second Post (2026) before Hello (2025).
    let pos_second = index.find("Second Post").unwrap();
    let pos_hello = index.find("Hello World").unwrap();
    assert!(
        pos_second < pos_hello,
        "sort desc: Second Post should appear before Hello World:\n{index}"
    );
}

/// Exercises SCSS-to-CSS compilation with variables and nesting.
#[tokio::test]
async fn scss_compilation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(site.join("styles")).await.unwrap();
    write_config(root, &site, &public, "scss = {}", "").await;

    fs::write(
        site.join("styles/main.scss"),
        r#"$accent: #E48C35;

body {
  color: $accent;
  nav {
    display: flex;
    a { text-decoration: none; }
  }
}"#,
    )
    .await
    .unwrap();

    run_aer(root).await;

    // SCSS should compile to CSS with the variable resolved and nesting flattened.
    let css = read(&public, "styles/main.css").await;
    assert!(css.contains("#E48C35"), "variable not resolved:\n{css}");
    assert!(css.contains("body nav"), "nesting not flattened:\n{css}");
    assert!(
        css.contains("body nav a"),
        "deep nesting not flattened:\n{css}"
    );
    // The .scss file should not exist in output.
    assert!(!exists(&public, "styles/main.scss").await);
}

/// Exercises JavaScript minification.
///
/// Comments and whitespace are stripped, and .min.js files are passed through untouched.
#[tokio::test]
async fn js_minification() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(site.join("js")).await.unwrap();
    write_config(root, &site, &public, "minify_js = {}", "").await;

    fs::write(
        site.join("js/app.js"),
        r#"// Main application script
function greet(name) {
    /* Say hello */
    console.log("Hello, " + name);
}
greet("world");
"#,
    )
    .await
    .unwrap();

    // Pre-minified files should be passed through.
    fs::write(site.join("js/vendor.min.js"), "function v(){return 1}")
        .await
        .unwrap();

    run_aer(root).await;

    let app = read(&public, "js/app.js").await;
    assert!(!app.contains("// Main"), "comment not stripped:\n{app}");
    assert!(
        !app.contains("/* Say"),
        "block comment not stripped:\n{app}"
    );
    assert!(app.contains("Hello, "), "string literal lost:\n{app}");
    assert!(app.len() < 120, "not minified (len={}):\n{app}", app.len());

    let vendor = read(&public, "js/vendor.min.js").await;
    assert_eq!(vendor, "function v(){return 1}", "min.js was modified");
}

/// Exercises HTML comment removal. The current MinifyHtmlProcessor only
/// strips comments, so this test is a bit trivial.
#[tokio::test]
async fn html_comment_removal() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(&site).await.unwrap();
    write_config(root, &site, &public, "minify_html = {}", "").await;

    fs::write(
        site.join("page.html"),
        r#"<!DOCTYPE html>
<html>
<!-- This comment should be removed -->
<body>
<p>Hello</p>
<!-- Another comment -->
</body>
</html>"#,
    )
    .await
    .unwrap();

    run_aer(root).await;

    let html = read(&public, "page.html").await;
    assert!(!html.contains("<!--"), "comment not removed:\n{html}");
    assert!(
        !html.contains("should be removed"),
        "comment text leaked:\n{html}"
    );
    assert!(html.contains("<p>Hello</p>"), "content lost:\n{html}");
}

/// Exercises URL canonicalization across HTML and CSS assets: absolute paths,
/// relative paths, special URLs preserved, meta content attributes.
#[tokio::test]
async fn url_canonicalization() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(site.join("blog")).await.unwrap();
    fs::create_dir_all(site.join("styles")).await.unwrap();

    write_config(
        root,
        &site,
        &public,
        r#"canonicalize = { root = "https://example.com/" }"#,
        "",
    )
    .await;

    fs::write(
        site.join("blog/post.html"),
        concat!(
            "<html>\n",
            "<head>\n",
            "<meta property=\"og:image\" content=\"/img/og.png\">\n",
            "</head>\n",
            "<body>\n",
            "<a href=\"/about\">About</a>\n",
            "<img src=\"../logo.png\">\n",
            "<a href=\"https://external.com\">External</a>\n",
            "<a href=\"#section\">Anchor</a>\n",
            "<a href=\"mailto:test@example.com\">Email</a>\n",
            "<script src=\"/js/app.js\"></script>\n",
            "</body>\n",
            "</html>",
        ),
    )
    .await
    .unwrap();

    fs::write(
        site.join("styles/main.css"),
        r#"body { background: url('../img/bg.png'); }
@font-face { src: url('/fonts/custom.woff2'); }"#,
    )
    .await
    .unwrap();

    run_aer(root).await;

    let html = read(&public, "blog/post.html").await;
    // Absolute path canonicalized.
    assert!(
        html.contains(r#"href="https://example.com/about""#),
        "absolute href:\n{html}"
    );
    // Relative path resolved against asset directory.
    assert!(
        html.contains(r#"src="https://example.com/logo.png""#),
        "relative src:\n{html}"
    );
    // External URL preserved.
    assert!(
        html.contains(r#"href="https://external.com""#),
        "external preserved:\n{html}"
    );
    // Anchor preserved.
    assert!(
        html.contains(r##"href="#section""##),
        "anchor preserved:\n{html}"
    );
    // Mailto preserved.
    assert!(
        html.contains(r#"href="mailto:test@example.com""#),
        "mailto preserved:\n{html}"
    );
    // Script src canonicalized.
    assert!(
        html.contains(r#"src="https://example.com/js/app.js""#),
        "script src:\n{html}"
    );
    // Meta content with absolute path.
    assert!(
        html.contains(r#"content="https://example.com/img/og.png""#),
        "meta content:\n{html}"
    );

    // CSS url() values canonicalized.
    let css = read(&public, "styles/main.css").await;
    assert!(
        css.contains("https://example.com/img/bg.png"),
        "css relative url:\n{css}"
    );
    assert!(
        css.contains("https://example.com/fonts/custom.woff2"),
        "css absolute url:\n{css}"
    );
}

/// Exercises clean URL rewriting: `about.html` becomes `about/index.html`,
/// while `index.html` stays at the root.
#[tokio::test]
async fn clean_urls() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(&site).await.unwrap();

    let config = format!(
        r#"
[default.paths]
source = "{}"
target = "{}"
clean_urls = true

[default.procs]
template = {{}}
"#,
        site.to_string_lossy(),
        public.to_string_lossy(),
    );
    fs::write(root.join("Aer.toml"), config).await.unwrap();

    fs::write(site.join("index.html"), "<p>Home</p>")
        .await
        .unwrap();
    fs::write(site.join("about.html"), "<p>About</p>")
        .await
        .unwrap();

    run_aer(root).await;

    // Root index.html stays in place.
    assert!(exists(&public, "index.html").await, "index.html missing");
    // about.html is rewritten to about/index.html.
    assert!(
        exists(&public, "about/index.html").await,
        "about/index.html missing"
    );
    assert!(
        !exists(&public, "about.html").await,
        "about.html should not exist"
    );
}

/// Exercises image resizing: oversized images are scaled down while images
/// within bounds are passed through unchanged.
#[tokio::test]
async fn image_resizing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(site.join("img")).await.unwrap();

    write_config(
        root,
        &site,
        &public,
        r#"image = { max_width = 200, max_height = 200 }"#,
        "",
    )
    .await;

    // Copy the test PNG (1824x1480) into the site.
    let source_png = std::fs::read("test/example.png").unwrap();
    fs::write(site.join("img/large.png"), &source_png)
        .await
        .unwrap();

    run_aer(root).await;

    let output = read_bytes(&public, "img/large.png").await;
    let img = image::load_from_memory(&output).unwrap();
    assert!(img.width() <= 200, "width {} > 200", img.width());
    assert!(img.height() <= 200, "height {} > 200", img.height());
    // Should be smaller than the original.
    assert!(
        output.len() < source_png.len(),
        "output ({}) should be smaller than input ({})",
        output.len(),
        source_png.len()
    );
}

/// Exercises favicon.png -> favicon.ico conversion.
#[tokio::test]
async fn favicon_conversion() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(&site).await.unwrap();
    write_config(root, &site, &public, "favicon = {}", "").await;

    let source_png = std::fs::read("test/example.png").unwrap();
    fs::write(site.join("favicon.png"), &source_png)
        .await
        .unwrap();

    run_aer(root).await;

    // Should produce favicon.ico (not favicon.png).
    assert!(exists(&public, "favicon.ico").await, "favicon.ico missing");

    let ico = read_bytes(&public, "favicon.ico").await;
    // ICO files start with magic bytes: 00 00 01 00.
    assert!(ico.len() > 4, "ico too small");
    assert_eq!(&ico[0..4], &[0x00, 0x00, 0x01, 0x00], "not ICO format");
}

/// Exercises profile merging: the production profile overrides the
/// canonicalize root while inheriting all other processors.
#[tokio::test]
async fn profile_merging() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(&site).await.unwrap();

    let config = format!(
        r#"
[default.paths]
source = "{}"
target = "{}"

[default.context]
site_name = "Dev Site"

[default.procs]
template = {{}}
canonicalize = {{ root = "http://localhost:1337/" }}

[production.procs]
canonicalize = {{ root = "https://www.example.com/" }}
"#,
        site.to_string_lossy(),
        public.to_string_lossy(),
    );
    fs::write(root.join("Aer.toml"), &config).await.unwrap();

    fs::write(site.join("page.html"), r#"<a href="/about">About</a>"#)
        .await
        .unwrap();

    // Run with production profile.
    aer::tool::procs::run(Some(&root.join("Aer.toml")), Some("production"))
        .await
        .unwrap();

    let html = read(&public, "page.html").await;
    assert!(
        html.contains("https://www.example.com/about"),
        "production root not applied:\n{html}"
    );
    assert!(
        !html.contains("localhost"),
        "dev root should not appear:\n{html}"
    );
}

/// Exercises the Markdown pattern of using a second `***` as an excerpt
/// separator. The first `***` separates frontmatter from body; subsequent
/// `***` become markdown thematic breaks (`<hr/>`).
#[tokio::test]
async fn multiple_frontmatter_delimiters() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(&site).await.unwrap();
    write_config(root, &site, &public, "markdown = {}\ntemplate = {}", "").await;

    fs::write(
        site.join("post.md"),
        r#"title = "My Post"

***

This is the excerpt summary.

***

This is the full article body with **bold** text."#,
    )
    .await
    .unwrap();

    run_aer(root).await;

    let html = read(&public, "post.html").await;
    // Frontmatter should be extracted (not rendered).
    assert!(!html.contains("title ="), "frontmatter leaked:\n{html}");
    // Both sections should be present.
    assert!(html.contains("excerpt summary"), "excerpt missing:\n{html}");
    assert!(
        html.contains("<strong>bold</strong>"),
        "body missing:\n{html}"
    );
    // The second *** becomes an <hr/> thematic break.
    assert!(
        html.contains("<hr/>"),
        "second *** not rendered as hr:\n{html}"
    );
}

/// Exercises the complete pipeline with all processors enabled, verifying
/// they compose correctly: markdown -> template -> pattern -> canonicalize
/// -> minify_html -> minify_js. This mirrors a typical production site build.
#[tokio::test]
async fn full_pipeline_ordering() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(site.join("patterns")).await.unwrap();
    fs::create_dir_all(site.join("js")).await.unwrap();
    fs::create_dir_all(site.join("styles")).await.unwrap();

    write_config(
        root,
        &site,
        &public,
        r#"markdown = {}
template = {}
pattern = {}
scss = {}
canonicalize = { root = "https://example.com/" }
minify_html = {}
minify_js = {}"#,
        "",
    )
    .await;

    // Pattern with a relative URL (canonicalize should process it).
    fs::write(
        site.join("patterns/_layout.html"),
        r#"<!DOCTYPE html>
<head>
<!-- build metadata -->
<link rel="stylesheet" href="/styles/main.css">
</head>
<body>
{~ get content}
<script src="/js/app.js"></script>
</body>"#,
    )
    .await
    .unwrap();

    fs::write(
        site.join("index.md"),
        r#"title = "Home"
pattern = "patterns/_layout.html"

***

# Hello **World**"#,
    )
    .await
    .unwrap();

    fs::write(
        site.join("js/app.js"),
        "// app script\nfunction init() { console.log('ready'); }\ninit();\n",
    )
    .await
    .unwrap();

    fs::write(
        site.join("styles/main.scss"),
        "$color: red;\nbody { color: $color; }\n",
    )
    .await
    .unwrap();

    run_aer(root).await;

    // HTML: markdown compiled, pattern applied, URLs canonicalized,
    // comments stripped.
    let html = read(&public, "index.html").await;
    assert!(html.contains("<h1"), "markdown not compiled:\n{html}");
    assert!(
        html.contains("<!DOCTYPE html>"),
        "pattern not applied:\n{html}"
    );
    assert!(
        html.contains("https://example.com/styles/main.css"),
        "css href not canonicalized:\n{html}"
    );
    assert!(
        html.contains("https://example.com/js/app.js"),
        "js src not canonicalized:\n{html}"
    );
    assert!(
        !html.contains("<!-- build metadata -->"),
        "html comment not stripped:\n{html}"
    );

    // JS: minified.
    let js = read(&public, "js/app.js").await;
    assert!(
        !js.contains("// app script"),
        "js comment not stripped:\n{js}"
    );

    // SCSS -> CSS.
    let css = read(&public, "styles/main.css").await;
    assert!(css.contains("red"), "scss variable not resolved:\n{css}");
    assert!(!exists(&public, "styles/main.scss").await);
}

/// Verifies that binary assets (images, fonts) and unrecognized file types
/// are copied to the output directory without modification.
#[tokio::test]
async fn binary_passthrough() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");

    fs::create_dir_all(&site).await.unwrap();
    write_config(root, &site, &public, "template = {}", "").await;

    let binary = vec![0u8, 1, 2, 3, 0xFF, 0xFE];
    fs::write(site.join("data.bin"), &binary).await.unwrap();

    run_aer(root).await;

    let output = read_bytes(&public, "data.bin").await;
    assert_eq!(output, binary, "binary file was modified");
}

/// Creates a bare git repo containing a `kit/` directory with assets, then
/// configures it as a kit in Aer.toml. Verifies:
/// - Git-based kit resolution (clone from file:// URL)
/// - Kit `kit/` subdirectory requirement
/// - Kit regular assets appear in output at the configured dest path
/// - Kit parts are available to templates via `{~ use "kitname/part"}`
/// - Pre-canonicalization rewrites relative URLs in kit CSS/HTML
#[tokio::test]
async fn kit_resolution_from_git() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let site = root.join("site");
    let public = root.join("public");
    fs::create_dir_all(&site).await.unwrap();

    // Initialize a working repo, add kit/ contents, push to bare.
    let git = |args: &[&str], cwd: &Path| {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };

    // Create bare repo.
    let bare_repo = root.join("test-kit.git");
    std::fs::create_dir_all(&bare_repo).unwrap();
    git(&["init", "--bare"], &bare_repo);

    // Create working repo.
    let work_repo = root.join("test-kit-work");
    std::fs::create_dir_all(&work_repo).unwrap();
    git(&["init"], &work_repo);
    git(
        &["remote", "add", "origin", bare_repo.to_str().unwrap()],
        &work_repo,
    );

    // Populate kit/ directory with assets.
    let kit_dir = work_repo.join("kit");
    std::fs::create_dir_all(kit_dir.join("styles")).unwrap();

    // A CSS file with a relative URL that pre-canonicalization should rewrite.
    std::fs::write(
        kit_dir.join("styles/brand.css"),
        r#"body { background: url("../img/bg.png"); }"#,
    )
    .unwrap();

    // A part (underscore-prefixed) available to templates.
    std::fs::write(
        kit_dir.join("_footer.html"),
        r#"<footer class="kit-footer">Kit Footer</footer>"#,
    )
    .unwrap();

    // A regular HTML file.
    std::fs::write(
        kit_dir.join("credits.html"),
        r#"<p>Credits page from kit</p>"#,
    )
    .unwrap();

    // Commit and push.
    git(&["add", "."], &work_repo);
    git(
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@test.com",
            "commit",
            "-m",
            "initial",
        ],
        &work_repo,
    );
    git(&["push", "origin", "main"], &work_repo);

    // Prepare a TOML config with the kit.
    let config = format!(
        r#"
[kits.brand]
git = "file://{bare}"
ref = "main"
dest = "/vendor/brand"

[default.paths]
source = "{site}"
target = "{public}"

[default.procs]
template = {{}}
"#,
        bare = bare_repo.to_string_lossy(),
        site = site.to_string_lossy(),
        public = public.to_string_lossy(),
    );
    fs::write(root.join("Aer.toml"), config).await.unwrap();

    // Source the kit from the site content.
    fs::write(
        site.join("index.html"),
        r#"<html>
<body>
<p>Site content</p>
{~ use "brand/_footer.html"}
</body>
</html>"#,
    )
    .await
    .unwrap();

    // Build and check.
    run_aer(root).await;

    // Kit regular assets should appear under the dest path.
    assert!(
        exists(&public, "vendor/brand/styles/brand.css").await,
        "kit CSS not in output"
    );
    assert!(
        exists(&public, "vendor/brand/credits.html").await,
        "kit HTML not in output"
    );

    // Kit part should NOT appear as a file in output (parts are context-only).
    assert!(
        !exists(&public, "vendor/brand/_footer.html").await,
        "kit part should not be emitted as file"
    );

    // Kit part should be included in the page via {~ use}.
    let index = read(&public, "index.html").await;
    assert!(
        index.contains("Kit Footer"),
        "kit part not included in page:\n{index}"
    );
    assert!(
        index.contains(r#"class="kit-footer""#),
        "kit part markup missing:\n{index}"
    );

    // Pre-canonicalization should rewrite the relative CSS URL to be absolute
    // under the kit's dest path.
    let css = read(&public, "vendor/brand/styles/brand.css").await;
    assert!(
        css.contains("/vendor/brand/img/bg.png"),
        "kit CSS URL not pre-canonicalized:\n{css}"
    );
}
