# Implementation Progress

## Processors: All Complete (9/9)

| Processor | Status | File |
|-----------|--------|------|
| canonicalize | ✓ | `src/proc/canonicalize.rs` |
| frontmatter | ✓ | `src/proc/frontmatter.rs` |
| image | ✓ | `src/proc/image.rs` |
| js_bundle | ✓ | `src/proc/js_bundle.rs` |
| markdown | ✓ | `src/proc/markdown.rs` |
| minify_html | ✓ | `src/proc/minify_html.rs` |
| minify_js | ✓ | `src/proc/minify_js.rs` |
| scss | ✓ | `src/proc/scss.rs` |
| template | ✓ | `src/proc/template.rs` |

## Commands: 1/4 Implemented

| Command | Status | Description |
|---------|--------|-------------|
| `aer proc` | ❌ | Single processor CLI with glob input |
| `aer procs` | ❌ | TOML pipeline with profiles |
| `aer serve` | ❌ | Dev server with file watching |
| `aer palette` | ✓ | Interactive color palette TUI |

## Remaining Work

### CLI Layer
- [x] Add `clap` for argument parsing
- [x] Implement `aer palette` subcommand (TUI in `src/tool/palette.rs`)
- [ ] Implement `aer proc <processor> <input> <target_path> [--options]`
- [ ] Implement `aer procs <procs_file> [-p profile]`
- [ ] Implement `aer serve [-p profile]`

### File I/O Layer
- [ ] Read source assets from filesystem
- [ ] Write processed assets to target directory
- [ ] Preserve directory structure from glob patterns

### Glob Pattern Support
- [ ] Match files like `**/*.scss`, `**/*.html`
- [ ] Integrate with `aer proc` command

### TOML Config Parsing
- [ ] Parse `[default.context]` for shared context values
- [ ] Parse `[default.procs]` for processor definitions
- [ ] Parse `paths.source` and `paths.target`

### Profile System
- [ ] Load `[default]` as base configuration
- [ ] Merge custom profiles (e.g., `[production]`) over default
- [ ] Support `-p` / `--profile` flag

### File Watcher (for `aer serve`)
- [ ] Watch `paths.source` for changes
- [ ] Re-run processors on file changes
- [ ] Auto-create default `Aer.toml` if missing

### HTTP Server (for `aer serve`)
- [ ] Serve processed assets on port 1337
- [ ] Serve from `paths.target` directory

### Processor Pipeline
- [ ] Execute processors in TOML-defined order
- [ ] Match processors to assets by media type
- [ ] Re-evaluate processors when media type changes (e.g., `.md` → `.html`)
- [ ] Continue processing other assets when one processor fails

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
