# Concepts

`aer` is a CLI that enables the entire creative process, with an initial focus on web development.

## Asset Processors

### `canonicalize`

This processor parses any CSS, JS, or HTML asset for relative or absolute
URL paths that are _not_ fully-qualified against a domain or hostname,
converting them to fully-qualified ("canonicalized") paths based on 
`canonicalize.root`.

### `image`

This processor resizes any image asset to fit within a `max_height` and
`max_width`, in pixels, while maintaining the original aspect ratio.

### `markdown`

This processor compiles a Markdown asset to HTML.

### `minify_html`

This processor minifies and strips comments from an HTML asset.

### `minify_js`

This processor minifies and strips comments from a JS asset.

### `scss`

This processor compiles a SCSS asset to CSS.

### `template`

This processor compiles a template inside a text asset.

## Creative Tools

### `color`

## CLI: `aer proc`

This command accepts a:

- `processor`, which is the name of an asset processor to execute.
- `input`, which is a file, directory, or glob pattern (like `**/*.scss`).
- `target_path`, which is a directory.

On execution, `processor` will be executed against all files matching `input`,
emitting the results to `target_path`.

## CLI: `aer procs`

This command accepts a `procs_file`, which is the path to a TOML file
containing a structure like:

```toml
[paths]
source = "site/"
target = "public/"

# Asset processors to run in all environments
[procs.default]
template = { title = "Aer Site" }
canonicalize = { root = "http://localhost:1337/" }
image = { max_width = 1920, max_height = 1920 }
scss = {}
markdown = {}
minify_html = {}
minify_js = {}

# Asset processors to run in production.
[procs.production]
canonicalize = { root = "https://www.example.com/" }
```

For _every_ file in `procs_file.target`, the command will execute _each_
processor in `procs_file.procs` with a media type matching the file. The
resulting processed asset(s) will be written to `target/` with the same
relative path they have in `source/`.

If the media type of an asset changes as a result of a given processor
executing against it, all other `procs_file.procs` will be re-evaluated:
For example, if a `.md` asset is compiled by the `markdown` processor, the
`minify_html` proc would be run against the resulting compiled `.html`.

## CLI: `aer serve`

This command starts a local HTTP server that watches an asset path for changes,
running the same logic as `aer procs` whenever any asset changes.

This command will attempt to load an `Aer.toml` from the current directory
to use as a `procs_file`. A default TOML with all processors enabled will
be created if one does not already exist.