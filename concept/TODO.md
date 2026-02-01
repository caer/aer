# TODO

Planned enhancements, roughly ordered by implementation dependency.

## Arrays of Objects

Allow arrays to contain tables, not just scalars.

**Where:**
- `src/proc.rs`, `ContextValue::List`. Change from `Vec<Text>` to
  `Vec<ContextValue>`.
- `src/proc.rs`, `ContextValue::from_toml`. Accept `toml::Value::Table`
  inside arrays.
- `src/proc/template.rs`, `for` loop. Insert each item as its actual
  `ContextValue` variant (not always `Text`).
- `src/proc/template.rs`, `get` handler for lists. Handle non-text items
  when rendering a list directly.

## Table Iteration

Support `{~ for key, val in table}` to iterate key-value pairs.

**Where:**
- `src/proc/template.rs`, `for` match arm. Detect the 4-arg form
  (`key`, `val`, `in`, `table`). When the resolved collection is a
  `ContextValue::Table`, iterate its entries and insert `key` as
  `Text` and `val` as the entry's `ContextValue`.
- Iteration order follows `BTreeMap` (alphabetical).

## Processor Deferral

Allow any processor to signal that it cannot complete until other
assets in the current batch have finished processing.

**Where:**
- `src/proc.rs`. Add a `Deferred` variant to `ProcessingError`.
- `src/tool/procs.rs`, `process_asset`. Propagate `Deferred` so the
  batch loop can distinguish it from real errors.
- `src/tool/procs.rs`, `run`. After each parallel batch, collect final
  contexts and output paths from completed assets and add them to the
  shared context. Reprocess deferred assets with the enriched context.
  Track how many times each asset has deferred. If any asset has
  deferred more times than the total number of deferred assets in the
  batch, it is stuck in a cycle — log an error naming the stuck
  assets and stop retrying.

## Directory Queries

Support `{~ for item in assets "path"}` to iterate assets in a directory
with each item's compiled context accessible as fields.

**Where:**
- `src/proc/template.rs`, `for` match arm. When the collection source
  is an `assets` function call, resolve it against completed asset data
  in the context. If the data isn't available yet, return `Deferred`.
- `src/tool/procs.rs`. After each batch, store completed asset metadata
  in the context as a list of tables keyed by directory path (e.g.,
  `_assets:builds/raids`). Each entry should include `slug` (filename
  without extension), the final output path, and the asset's full
  final processing context.
- Depends on arrays-of-objects and processor deferral.

## Comparison via `is`

Support `is` and `is not` in `if` expressions:
`{~ if var is "value"}`, `{~ if var is not "value"}`.

**Where:**
- `src/proc/template.rs`, `if` match arm. After resolving the left
  operand, check for an `is` keyword followed by an optional `not` and
  a string or identifier operand. Compare resolved values as strings.
- No tokenizer changes needed — `is` and `not` are already valid
  identifiers.

## Parameterized Partials

Support injecting values into a part's context via `with...as`:
`{~ use "path", with "Title" as label, with author as byline}`.

**Where:**
- `src/proc/template.rs`, `use` match arm. After the path string, parse
  comma-separated `with <value> as <key>` clauses. The value side may be
  a quoted string literal or a variable identifier resolved against the
  current context. Insert each result into the cloned part context before
  compiling.
- No tokenizer changes needed — `with`, `as`, and `,` can be matched
  as identifiers and punctuation the lexer already handles.
