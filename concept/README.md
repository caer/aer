# Concepts

`aer` is a CLI that enables the entire creative process, with an initial focus on web development.

## Asset Processing

### `canonicalize` Processor

Transforms URL paths in HTML assets to fully-qualified
URLs based on a `root` parameter.

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
  
- `url()` values in inline `style` attributes. 

URLs _within_ `<script>` tags are not processed. Fully-qualified URLs
(like `https://localhost`) and special URLs (`data:`, `javascript:`,
`mailto:`, `#anchor`) are not processed.

### `image` Processor

Resizes JPG, PNG, or GIF assets to fit within `max_height` and `max_width`
parameters (in pixels) while maintaining aspect ratio. Resizes if either
dimension exceeds its limit.

### `js_bundle` Processor

Bundles JavaScript modules into a single file using Rolldown. Resolves
`import` statements and consolidates dependencies. Optionally minifies
output via the `minify` parameter.

### `markdown` Processor

Compiles Markdown assets to HTML body fragments (no boilerplate), following
the CommonMark specification.

### `minify_html` Processor

Minifies and strips comments from HTML assets.

### `minify_js` Processor

Minifies and strips comments from JS assets.

Assets with paths ending in `.min.js` will not be minified.

### `scss` Processor

Compiles SCSS assets to CSS.

### `template` Processor

Compiles templates in text assets, drawing values from the processing context.

#### Frontmatter

Before processing template expressions, the processor extracts TOML
frontmatter from the asset and merges it into the processing context.

Text contains valid TOML frontmatter if it _begins_ with valid TOML
content followed by `***` on a newline. The frontmatter is removed
from the asset after extraction.

Example of an HTML asset containing frontmatter:

```html
title = "Example Page"

***

<h1>Hello, world!</h1>
```

#### Template Expressions

Template expressions are wrapped in `~{ }`. The following expressions are supported:

- `~{# variable_name}` outputs the value of a variable
- `~{if condition}...~{end}` renders content if the condition is truthy (non-empty and not `"false"` or `"0"`)
- `~{for item in items}...~{end}` iterates over a list
- `~{use "path"}` includes a part by its path (see Asset Writing)

Example template:

```html
<title>~{# title}</title>
~{use "_header.html"}
~{if show_greeting}
    <p>Hello, ~{# name}!</p>
~{end}
<ul>
~{for item in items}
    <li>~{# item}</li>
~{end}
</ul>
```

### `aer proc` Command

Accepts a `processor` name, an `input` (file, directory, or glob pattern like
`**/*.scss`), and a `target_path` directory. The processor runs against all
matching assets, writing results to `target_path`. Directory structure from
glob patterns is preserved in the target.

Processor options are passed as long CLI arguments:

```sh
aer proc canonicalize **/*.html public/ --root https://www.example.com/
aer proc image **/*.png public/ --max-width 800 --max-height 600
```

### `aer procs` Command

Accepts an optional `procs_file` path to a TOML configuration file.
If not specified, looks for `Aer.toml` in the current directory.

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

# The "context" processor sets values on the
# global context shared by all processors.
[default.context]
title = "Aer Site"

# Asset processors to run in all environments
[default.procs]
markdown = {}
template = {}
canonicalize = { root = "http://localhost:1337/" }
scss = {}
js_bundle = { minify = false }
minify_html = {}
minify_js = {}
image = { max_width = 1920, max_height = 1920 }

# Asset processors to run in production.
[production.procs]
canonicalize = { root = "https://www.example.com/" }
js_bundle = { minify = true }
```

Processors execute in the order they appear in the TOML file. Processor-to-asset
matching is determined by hardcoded media type support.

If the media type of an asset changes as a result of a given processor
executing against it, all other processors will be re-evaluated. For example,
if a `.md` asset is compiled by the `markdown` processor, the `minify_html`
processor would be run against the resulting compiled `.html`.

#### Asset Writing

For every asset in `paths.source`, the command executes each processor
in the profile's `procs` with a media type matching the asset. Processed assets
are written to `paths.target` with the same relative path they have in `paths.source`.

If the processed asset's contents are identical to what already exists at the target path, no write is performed.

When a processor fails, other processors will still run against the last
successfully processed contents of the asset.

Assets with a path containing a component starting with `_` (e.g.,
`_header.html` or `_parts/footer.html`) are _not_ written to
`paths.target`. Instead, they're cached _without processing_ and made
available as **Parts**. Parts are included via `~{use "path"}` in the
template processor, which extracts any frontmatter from the part
and inserts the remaining content.

#### Profiles

Profiles are specified via `-p` or `--profile`. Custom profiles (like
`[production]`) merge on top of `[default]`. Paths can vary per profile to
support diverse deployment environments.

### `aer serve` Command

Starts a local HTTP server on port `1337` that watches an asset
path for changes, running the same logic as `aer procs` whenever any asset
changes.

Attempts to load an `Aer.toml` from the current directory to use as a `procs_file`.
A default TOML with all processors enabled will be created if one does not already exist.

Profiles are specified the same way as `aer procs` (`-p` or `--profile`).

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