# Concepts

`aer` is a toolkit for people who create interactive media,
with an initial focus on web development.

## Asset Processing

`aer procs` runs a pipeline of asset processors defined in an `Aer.toml` configuration file.
Processors transform source assets (like Markdown, SCSS, images, templates) into
production-ready output with support for profile-based configuration
(e.g., development vs production settings).

### `aer procs` Command

Accepts an optional `procs_file` path to a TOML configuration file. If not specified,
looks for `Aer.toml` in the current directory.

#### Configuration

Use `aer init` to create a new `Aer.toml` with the recommended
default processors in the current directory. Existing files won't
be overwritten.

Example TOML structure:

```toml
# Paths to read and write assets from during processing.
[default.paths]
source = "site/"
target = "public/"

# The "context" table sets values on the
# global context shared by all processors.
# Nested tables are supported and accessed
# via dotted identifiers in templates.
[default.context]
title = "Aer Site"

[default.context.author]
name = "Alice"

# Asset processors to run in all environments
[default.procs]
markdown = {}
template = {}
pattern = {}
canonicalize = { root = "http://localhost:1337/" }
scss = {}
minify_html = {}
minify_js = {}
image = { max_width = 1920, max_height = 1920 }
favicon = {}

# Asset processors to run in production.
[production.procs]
canonicalize = { root = "https://www.example.com/" }
```

Every processor specified in the TOML file will be run against
every compatible asset in `paths.source`, with compatibility
determined by hard-coded media type support.

If the media type of an asset changes as a result of a given processor
executing against it, all other processors will be re-evaluated. For example,
if a `.md` asset is compiled by the `markdown` processor, the `minify_html`
processor would be run against the resulting compiled `.html`.

#### Asset Writing

For every asset in `paths.source`, the command executes each processor
in the profile's `procs` with a media type matching the asset. Processed assets
are written to `paths.target` with the same relative path they have in `paths.source`.

If the processed asset's contents are identical to what already exists
at the target path, no write is performed.

When a processor fails, other processors will still run against the last
successfully processed contents of the asset.

Assets with a path containing a component starting with `_` (e.g.,
`_header.html` or `_parts/footer.html`) are _not_ written to
`paths.target`. Instead, they're cached _without processing_ and made
available as **Parts**. Parts are included via `{~ use "path"}` in the
template processor, which extracts any frontmatter from the part
and inserts the remaining content.

When `clean_urls` is enabled, `text/html` assets other than `index.html`
are written as `slug/index.html` instead of `slug.html` so that links
can omit the `.html` file extension.

#### Profiles

Profiles are specified via `-p` or `--profile`. Custom profiles (like
`[production]`) merge on top of `[default]`.

### `aer serve` Command

Starts a local HTTP server on port `1337` that watches an asset
path for changes, running the same logic as `aer procs` whenever
any asset changes.

### `canonicalize` Processor

Transforms URL paths in `HTML` and `CSS` assets to
fully-qualified URLs based on a `root` parameter.

Absolute paths are canonicalized relative to `root`:
`/path/to/file` becomes `{root}/path/to/file`.

Relative paths (e.g., `./file`, `../file`, or `file`)
are canonicalized relative to `root` _and_ their source
asset's declared path.

For example, given an asset `/path/to/file.html` containing
a URL `../styles.css`, the final canonicalized URL would be
`{root}/path/styles.css`.

The following URLs within HTML assets are processed:

- URL-containing attributes like `href`, `src`, `action`, 
  `poster`, `data`, `cite`, `formaction`. 
  
- URL values in `meta` tags.

- `url()` values in inline `style` attributes.

The `src` attribute on `<script>` tags is processed, but URL strings
within script content are not. Fully-qualified URLs (like `https://localhost`)
and special URLs (`data:`, `javascript:`, `mailto:`, `#anchor`) are not processed.

### `favicon` Processor

Converts any `favicon.png` file in the root of the
source directory into an appropriately-sized `favicon.ico`.

### `image` Processor

Resizes JPG, PNG, or GIF assets to fit within `max_height` and `max_width`
parameters (in pixels) while maintaining aspect ratio. Resizes if either
dimension exceeds its limit.

### `js_bundle` Processor

Bundles JavaScript modules into a single file using Rolldown. Resolves
`import` statements and consolidates dependencies. Optionally minifies
output via the `minify` parameter.

### `markdown` Processor

Compiles Markdown assets to HTML body fragments, following the CommonMark specification.

### `minify_html` Processor

Minifies and strips comments from HTML assets.

### `minify_js` Processor

Minifies and strips comments from JS assets.

Assets with target paths ending in `.min.js` will _not_ be minified.

### `scss` Processor

Compiles SCSS assets to CSS.

### `template` Processor

Compiles templates in text assets, drawing values from the processing context.

#### Frontmatter

Before processing template expressions, the processor extracts TOML
frontmatter from the asset and merges it into the processing context.

Text contains valid TOML frontmatter if it _begins_ with valid TOML
content followed by `***` on a newline. The frontmatter is removed
from the asset after extraction. Frontmatter values are scoped to the
asset being processed and do not affect other assets.

Example of an HTML asset containing frontmatter:

```html
title = "Example Page"

***

<h1>Hello, world!</h1>
```

#### Template Expressions

Template expressions are wrapped in `{~ }`. The following expressions are supported:

- `{~ get variable_name}` outputs the value of a variable.
    - An arbitrary number of fallbacks may be specified with `or`: `{~ get title or name or headline}`.
- `{~ if variable_name}...{~ end}` renders content if the variable is truthy (non-empty and not `"false"` or `"0"`)
    - `{~ if not variable_name}...{~ end}` renders content if the variable is _not_ truthy.
- `{~ if variable_name is "value"}...{~ end}` renders content if the variable equals a specific value.
    - `{~ if variable_name is not "value"}...{~ end}` renders content if the variable doesn't equal a specific value.
- `{~ for item in items}...{~ end}` iterates over a list of variables.
    - Each `item` may be a scalar, a table, or another list.
- `{~ for key, val in table}...{~ end}` iterates over a table's key-value pairs.
    - Each `key` will be text, but each `val` may be a scalar, a table, or a list.
- `{~ for item in assets "path"}...{~ end}` iterates over assets in a directory, with each item's compiled context accessible as fields.
- `{~ use "path"}` includes a part by its path (see Asset Writing).
    - Values (including variables) can be injected into the part's context using `with`.
    - This example sets `label` to `"Title"` and `byline` to the value of `author`: `{~ use "path", with "Title" as label, with author as byline}`

Example template:

```html
<title>{~ get title}</title>
{~ use "_header.html"}
{~ if show_greeting}
    <p>Hello, {~ get name}!</p>
{~ end}
<ul>
{~ for item in items}
    <li>{~ get item}</li>
{~ end}
</ul>
```

Templates support nested variable access from the context. For example, `{~ get user.name}` would render the `name` property on a `user` table.

### Patterns

Template frontmatter may optionally contain a `pattern` field, which can
be set to the path of an existing part (see Asset Writing). If set, the
processor will save the rendered asset contents onto the processing context
in the `content` variable, and replace the asset with the rendered contents
of the part.

## Color Palettes

### `aer palette` Command

Generates color palettes using the Oklch color space.

A `Color` is represented by:

- `l` (lightness): `0.0` (darkest) to `1.0` (brightest)
- `c` (chroma): `0.0` (desaturated) to `0.4` (saturated)
- `h` (hue): `0.0` to `360.0` degrees

A `Neutrals` palette derives seven shades from a base color:

| Shade     | Lightness |
|-----------|-----------|
| darkest   | 0.19      |
| darker    | 0.24      |
| dark      | 0.41      |
| neutral   | 0.58      |
| light     | 0.75      |
| lighter   | 0.92      |
| lightest  | 0.97      |

A `ColorSystem` provides 9 base colors (neutral, magenta, red, orange, yellow,
green, cyan, blue, purple) all at lightness `0.58`, with additional derived shades.

### `--troubles` Flag

Every `aer` command accepts a `--troubles` flag. By default, `aer` logs
high-level information during command execution. When `--troubles` is passed,
extended logging is enabled to assist with troubleshooting.