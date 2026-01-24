# Implementation Progress

## Processors: All Complete (8/8)

| Processor | Status | File |
|-----------|--------|------|
| canonicalize | ✓ | `src/proc/canonicalize.rs` |
| image | ✓ | `src/proc/image.rs` |
| js_bundle | ✓ | `src/proc/js_bundle.rs` |
| markdown | ✓ | `src/proc/markdown.rs` |
| minify_html | ✓ | `src/proc/minify_html.rs` |
| minify_js | ✓ | `src/proc/minify_js.rs` |
| scss | ✓ | `src/proc/scss.rs` |
| template | ✓ | `src/proc/template.rs` (includes frontmatter extraction) |

## Commands: 3/5 Implemented

| Command | Status | Description |
|---------|--------|-------------|
| `aer init` | ✓ | Create default Aer.toml |
| `aer palette` | ✓ | Interactive color palette TUI |
| `aer procs` | ✓ | TOML pipeline with profiles |
| `aer proc` | ❌ | Single processor CLI with glob input |
| `aer serve` | ❌ | Dev server with file watching |

## Remaining Work

### CLI Layer
- [x] Add `clap` for argument parsing
- [x] Implement `aer init` subcommand
- [x] Implement `aer palette` subcommand (TUI in `src/tool/palette.rs`)
- [x] Implement `aer procs <procs_file> [-p profile]`
- [ ] Implement `aer proc <processor> <input> <target_path> [--options]`
- [ ] Implement `aer serve [-p profile]`

### File I/O Layer
- [x] Read source assets from filesystem
- [x] Write processed assets to target directory
- [x] Preserve directory structure
- [ ] Skip writes when content is unchanged (incremental builds)

### Glob Pattern Support
- [ ] Match files like `**/*.scss`, `**/*.html`
- [ ] Integrate with `aer proc` command

### TOML Config Parsing
- [x] Parse `[default.context]` for shared context values
- [x] Parse `[default.procs]` for processor definitions
- [x] Parse `paths.source` and `paths.target`

### Profile System
- [x] Load `[default]` as base configuration
- [x] Merge custom profiles (e.g., `[production]`) over default
- [x] Support `-p` / `--profile` flag

### Parts System (Template Includes)
- [x] Cache assets with `_` prefix without writing to target
- [x] Implement `~{use "path"}` template expression to include parts
- [x] Extract frontmatter from parts when included

### File Watcher (for `aer serve`)
- [ ] Watch `paths.source` for changes
- [ ] Re-run processors on file changes
- [ ] Auto-create default `Aer.toml` if missing

### HTTP Server (for `aer serve`)
- [ ] Serve processed assets on port 1337
- [ ] Serve from `paths.target` directory

### Processor Pipeline
- [x] Execute processors in TOML-defined order
- [x] Match processors to assets by media type
- [x] Re-evaluate processors when media type changes (e.g., `.md` → `.html`)
- [x] Continue processing other assets when one processor fails

---

## Notes

### 2026-01-23: CLI Refactoring

Refactored `main.rs` to use clap for subcommand routing:

- Moved palette TUI code (~340 lines) from `main.rs` to `src/tool/palette.rs`
- `main.rs` is now a minimal CLI dispatcher using clap derive macros
- Pattern: each subcommand calls a `run()` function from its module

**Structure:**
```
src/main.rs          - CLI entry point (clap Parser + Subcommand)
src/tool/palette.rs  - palette::run() launches TUI
src/tool/color.rs    - Color types used by palette (now private to tool)
```

**Key learnings:**
- ratatui's `Color` conflicts with our `Color` type; aliased as `TermColor`
- Internal module imports use `crate::` instead of the crate name
- clap derive with `#[command(subcommand)]` makes routing clean

### 2026-01-23: Implementation Review

Updated TODO to reflect actual implementation state:

- Added `aer init` command to tracking (was implemented but not listed)
- Added **Parts System** section for template includes feature
- Added incremental builds task (skip unchanged writes)
- Corrected command count: 3/5 implemented (init, palette, procs)

**Key gaps identified:**

1. **Parts not implemented**: The `~{use "path"}` template expression is
   documented in README.md but not implemented in `template.rs`. Assets
   with `_` prefix are currently written to target like any other asset.

2. **No incremental writes**: `tool/procs.rs` writes every asset
   unconditionally. The README claims unchanged content skips writes,
   but this check doesn't exist yet.

3. **Template expressions**: `#`, `if`, `for`, `end`, and `use` are implemented.
   The `date` expression shown in docstrings is not yet functional.

### 2026-01-23: Frontmatter Merged into Template

Per concept/README.md, frontmatter extraction was merged into the template
processor. The standalone `frontmatter` processor was removed.

- Deleted `src/proc/frontmatter.rs`
- Added frontmatter extraction to `TemplateProcessor::process()`
- Updated default `Aer.toml` to remove `frontmatter = {}` entry
- All frontmatter tests moved to `template.rs`

### 2026-01-23: Parts System Implemented

Implemented template includes via the Parts system:

- Assets with `_` prefix in any path component are cached without processing
- Parts are stored in context with `_part:` prefix (e.g., `_part:_header.html`)
- `~{use "path"}` expression includes cached parts
- Frontmatter in parts is extracted and available within the part itself
- Added `is_part()` helper in `tool/procs.rs`
- Added `PART_PREFIX` constant and `extract_frontmatter_from_content()` in `template.rs`
