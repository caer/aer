[![`aer` on crates.io](https://img.shields.io/crates/v/aer)](https://crates.io/crates/aer)
[![`aer` on docs.rs](https://img.shields.io/docsrs/aer)](https://docs.rs/aer/)
[![Ask DeepWiki about `aer`](https://deepwiki.com/badge.svg)](https://deepwiki.com/caer/aer)

A command-line toolkit for creatives.

## What's Here

> [!NOTE]
> This crate is a work-in-progress toolkit for supporting the entire process of creating static web content, from concept to deployment.
> The way the tools are organized will likely change in future versions of the crate.

### Asset Processors

`aer procs` runs a pipeline of asset processors defined in an `Aer.toml` configuration file. Processors transform source assets (Markdown, SCSS, images, templates) into production-ready output with support for profile-based configuration (e.g., development vs production settings).

> [!NOTE]
> Use `aer init` to create a starter `Aer.toml` in the current directory.

`aer serve` starts a local development server that watches for file changes and automatically rebuilds assets.

#### Available Processors

| Processor | Description |
|-----------|-------------|
| `template` | Compiles `{~ }` template expressions with TOML frontmatter support |
| `markdown` | Converts Markdown to HTML (CommonMark) |
| `scss` | Compiles SCSS to CSS |
| `js_bundle` | Bundles JavaScript modules via Rolldown |
| `image` | Resizes images to fit within bounds |
| `canonicalize` | Converts relative URLs to fully-qualified URLs |
| `minify_html` | Minifies HTML |
| `minify_js` | Minifies JavaScript |

See [concept/README.md](concept/README.md) for detailed documentation.

### Color Palette Picker

![Picture of the `aer` color palette tool](docs/aer-colors.png)

`aer palette` launches an interactive color palette picker based on [Oklab Colorspace](https://bottosson.github.io/posts/oklab/) relationships.

## License and Contributions 

Copyright © 2026 With Caer, LLC.

Licensed under the Functional Source License, Version 1.1, MIT Future License.
Refer to [the license file](LICENSE.md) for more info.