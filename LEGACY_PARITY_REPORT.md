# Aer / Cake Legacy Parity Report

This document analyzes the feature gap between **Aer** (the new Rust codebase) and the legacy **Cake** (caer-make) codebase to identify what needs to be implemented for approximate feature parity.

## Executive Summary

Aer has implemented core asset processing capabilities but is missing the **complete CLI interface**, **site generation workflow**, **configuration system**, and several **template functions** that make Cake a usable static site generator. The main gaps are:

| Category | Status |
|----------|--------|
| Asset Processors | ~80% complete |
| Template System | ~60% complete |
| CLI & Commands | ~0% complete |
| Configuration | ~0% complete |
| Development Server | ~0% complete |
| Build Pipeline | ~0% complete |

---

## 1. CLI Interface & Commands

### Cake Features (Missing in Aer)

| Feature | Description | Priority |
|---------|-------------|----------|
| `site build` command | Compiles all static website files to target directory | **Critical** |
| `site serve` command | Builds site + launches HTTP server on port 1337 | **Critical** |
| `--config` flag | Specifies configuration file path | **Critical** |
| `--context` flag | Selects templating context (e.g., `--context publish`) | **Critical** |
| `-t, --troubles` flag | Enables debug logging, disables minification | High |
| File watching | Auto-rebuild on source file changes (1-second debounce) | High |
| Default config generation | Auto-generates `Config.toml` with commented defaults | Medium |

### Current Aer State

Aer's `main.rs` is an **interactive color palette picker** using `ratatui`, not a site generation CLI. The asset processors exist as library code but have no CLI exposure.

### Implementation Needed

1. Replace or extend `main.rs` with subcommand structure using `clap`
2. Implement `site build` subcommand
3. Implement `site serve` subcommand with:
   - Async HTTP server (Cake uses `warp`)
   - Filesystem watcher (Cake uses `notify-debouncer-full`)
4. Add CLI argument parsing for config path, context selection, debug mode

---

## 2. Configuration System

### Cake Features (Missing in Aer)

| Feature | Description | Priority |
|---------|-------------|----------|
| `Config.toml` parsing | TOML-based project configuration | **Critical** |
| `[paths]` section | Source/target directory configuration | **Critical** |
| `[context.default]` | Base template context values | **Critical** |
| `[context.<name>]` | Named contexts that merge with default | **Critical** |
| Context merging | Later contexts override earlier values | High |
| `canon` context variable | Canonical URL for the site | High |

### Cake Config Example
```toml
[paths]
source = "site"
target = "public"

[context.default]
canon = "http://localhost:1337/"
title = "My Site"

[context.publish]
canon = "https://www.example.com/"
```

### Implementation Needed

1. Create `Config` struct with serde deserialization
2. Implement path configuration (source, target, working directory)
3. Implement context system with merging semantics
4. Add auto-generation of default config with comments

---

## 3. Template System

### Syntax Comparison

| Feature | Cake Syntax | Aer Syntax | Status |
|---------|-------------|------------|--------|
| Variable substitution | `{> key }` | `~{# key}` | **Different syntax** |
| Conditionals | `{> if key }...{> end }` | `~{if key}...~{end}` | **Implemented** |
| Negated conditionals | `{> if !key }` | Not implemented | **Missing** |
| Loops | `{> for item in items }` | `~{for item in items}` | **Implemented** |
| Function calls | `{> func(arg1, arg2) }` | `~{func arg1 arg2}` | **Different syntax** |
| Nested key access | `{> test.name }` | `~{# test.name}` | Partial |

### Template Functions

| Function | Cake | Aer | Description |
|----------|------|-----|-------------|
| Variable reference | `{> key }` | `~{# key}` | **Different** |
| `use(path)` | Yes | No | Include partial template files |
| `canon(path)` | Yes | No | Generate canonical URLs |
| `now(format)` | Yes | No | Current timestamp with formatting |
| Custom functions | Yes | No | Register arbitrary template functions |

### Context Value Types

| Type | Cake | Aer | Notes |
|------|------|-----|-------|
| String | Yes | Yes (as `Text`) | |
| Sequence/List | Yes | Yes (as `List`) | |
| Map (nested) | Yes | No | Cake supports nested contexts |

### Implementation Needed

1. **Add negated conditionals** (`~{if !key}`)
2. **Implement `use` function** - Include and compile partial templates
3. **Implement `canon` function** - URL canonicalization
4. **Implement `now` function** - Timestamp formatting (uses `chrono`)
5. **Add nested Map context values** - For hierarchical data
6. **Add custom function registration** - `TemplateFunction` trait pattern

---

## 4. Asset System

### Asset Features

| Feature | Cake | Aer | Notes |
|--------|------|-----|-------|
| Binary content | Yes | Yes | |
| Text content | Yes | Yes | |
| Frontmatter extraction | Yes | No | `***`-delimited TOML metadata |
| Part detection (`_` prefix) | Yes | No | Internal-only assets |
| URL canonicalization | Yes | No | Resolves relative URLs |
| Extension handling | Yes | Yes | |
| Media type detection | Yes | Yes | |

### Implementation Needed

1. **Add frontmatter parsing** - Extract `***`-delimited TOML from file headers
2. **Add part detection** - Skip `_`-prefixed files from final output
3. **Add URL canonicalization method** - For links/images in content
4. **Add canonical root tracking** - For generating absolute URLs

---

## 5. Asset Processors

### Comparison Table

| Processor | Cake | Aer | Gap |
|-----------|------|-----|-----|
| **Markdown → HTML** | Yes | Yes | Minor differences (see below) |
| **SCSS → CSS** | Yes | Yes | Feature complete |
| **Image Resize** | Yes | Yes | Feature complete |
| **HTML Minification** | Yes | No | **Missing** |
| **JS Minification** | Yes | No | **Missing** |
| **PNG → ICO (Favicon)** | Yes | No | **Missing** |
| **JS Bundling** | No | Yes | Aer has extra feature |
| **Template Processing** | Yes | Yes | Functions missing (see above) |

### Markdown Processor Details

| Feature | Cake | Aer |
|---------|------|-----|
| Headers with anchor IDs | Yes | Yes |
| Paragraphs, blockquotes | Yes | Yes |
| Lists (ordered/unordered) | Yes | Yes |
| Bold, italic, strikethrough | Yes | Yes |
| Links with URL canonicalization | Yes | No |
| Images with URL canonicalization | Yes | No |
| Inline/fenced code blocks | Yes | Yes |
| Em-dash conversion (`--` → `—`) | Yes | Yes |
| Tables | No | No |

### Implementation Needed

1. **Add HTML minification processor** - Use `minify-html-onepass` crate
2. **Add JS minification processor** - Use `minify-js` crate
3. **Add PNG → ICO converter** - For favicon generation
4. **Add URL canonicalization to Markdown** - Resolve relative links/images

---

## 6. Build Pipeline

### Cake Pipeline Stages (Missing in Aer)

1. **Context Setup**
   - Clone base context
   - Merge frontmatter for top-level assets
   - Register template functions (`use`, `now`, `canon`)

2. **Format-Specific Compilation**
   - Markdown files processed first
   - SCSS compiled, then templates evaluated on output
   - PNG favicons converted to ICO

3. **HTML Processing**
   - Templates compiled
   - Layout patterns applied (if specified in frontmatter)
   - Compiled content inserted into pattern context

4. **Optimization** (skipped for partials)
   - HTML minification
   - JavaScript minification
   - Image resizing for JPG/PNG

5. **Output**
   - Write compiled assets to target directory
   - Skip partial assets (`_` prefix)

### Layout Pattern System

Cake supports a **layout pattern** feature where frontmatter can specify a layout template:

```markdown
***
layout = "_layout.html"
title = "My Page"
***

Page content here...
```

The compiled content is inserted into the layout's context as a variable.

### Implementation Needed

1. **Create `Pipeline` struct** orchestrating processor sequence
2. **Implement frontmatter-driven context merging**
3. **Implement layout pattern system**
4. **Add conditional processing** (skip partials from final output)
5. **Add filesystem traversal** with part detection
6. **Add target directory writing**

---

## 7. Development Server

### Cake Features (Missing in Aer)

| Feature | Description |
|---------|-------------|
| HTTP server | Serves compiled assets on port 1337 |
| File watching | Monitors source directory for changes |
| Auto-rebuild | Recompiles on file modification/deletion |
| Debouncing | 1-second delay to batch rapid changes |

### Implementation Needed

1. **Add async HTTP server** - Consider `warp`, `axum`, or `hyper`
2. **Add filesystem watcher** - Use `notify` crate
3. **Implement debounced rebuild** - Batch rapid file changes
4. **Add graceful error handling** - Don't crash on compile errors

---

## 8. Error Handling

### Cake Patterns (Improvements for Aer)

| Area | Cake | Aer |
|------|------|-----|
| Missing partials | Panic with message | N/A (no partials) |
| Missing patterns | Panic with message | N/A (no patterns) |
| Minification failures | Graceful fallback (HTML), panic (JS) | N/A |
| Binary vs text mismatch | Return typed error | Returns `ProcessingError` |

### Implementation Needed

1. **Add structured error types** - Use `snafu` or `thiserror`
2. **Add graceful degradation** - Don't crash on recoverable errors
3. **Improve error messages** - Include file paths and context

---

## 9. Logging & Debugging

### Cake Features

| Feature | Description | Aer Status |
|---------|-------------|------------|
| Tracing integration | Structured logging | Partial |
| Debug logging | Detailed operation tracking | Partial |
| `--troubles` mode | Verbose output, skip optimizations | Missing |
| Environment filter | Control log levels via env vars | Missing |

### Implementation Needed

1. **Add `tracing-subscriber`** with environment filter
2. **Implement troubles/debug mode** - Skip minification, verbose logging
3. **Add operation logging** - Track each pipeline stage

---

## 10. Dependencies to Add

Based on Cake's `Cargo.toml`, Aer needs these additional dependencies:

| Crate | Purpose | Priority |
|-------|---------|----------|
| `clap` | CLI argument parsing | **Critical** |
| `serde` + `toml` | Configuration parsing | **Critical** |
| `warp` or `axum` | HTTP server | **Critical** |
| `notify-debouncer-full` | File watching | **Critical** |
| `chrono` | Timestamp formatting | High |
| `minify-html-onepass` | HTML minification | High |
| `minify-js` | JavaScript minification | High |
| `url` | URL canonicalization | High |
| `snafu` | Error handling | Medium |

---

## 11. Implementation Priority

### Phase 1: Core CLI (Critical)
1. Add `clap` for CLI structure
2. Implement `Config.toml` parsing
3. Create `site build` command
4. Add filesystem traversal with part detection
5. Implement frontmatter parsing

### Phase 2: Template Completion (Critical)
1. Add `use` function for partials
2. Add `canon` function for URLs
3. Add `now` function for timestamps
4. Add negated conditionals
5. Implement layout pattern system

### Phase 3: Optimization (High)
1. Add HTML minification processor
2. Add JS minification processor
3. Add PNG → ICO converter
4. Add URL canonicalization to Markdown

### Phase 4: Development Server (High)
1. Add HTTP server for `site serve`
2. Add file watching
3. Implement debounced auto-rebuild

### Phase 5: Polish (Medium)
1. Improve error handling
2. Add debug/troubles mode
3. Add default config generation
4. Improve logging

---

## 12. Architectural Differences

### Positive Differences in Aer

1. **JS Bundling** - Aer has `brk_rolldown` integration that Cake lacks
2. **Color System** - Sophisticated Oklch color tools not in Cake
3. **Modern Rust** - Edition 2024, newer patterns

### Template Syntax Change

Aer uses `~{ }` while Cake uses `{> }`. This is a deliberate design choice but means templates are **not backwards compatible**. Consider:

- Document migration path
- Possibly support legacy syntax via feature flag

---

## Conclusion

Aer has solid foundations for asset processing but needs significant work on:

1. **CLI interface** - No user-facing commands exist
2. **Configuration** - No TOML config parsing
3. **Template functions** - Missing `use`, `canon`, `now`
4. **Build pipeline** - No orchestration of processors
5. **Development server** - No HTTP serving or file watching
6. **Minification** - No HTML/JS minification

Estimated implementation scope: **Medium-Large** (core processors exist, but integration layer is missing).
