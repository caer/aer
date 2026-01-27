[![`aer` on crates.io](https://img.shields.io/crates/v/aer)](https://crates.io/crates/aer)
[![`aer` on docs.rs](https://img.shields.io/docsrs/aer)](https://docs.rs/aer/)
[![Ask DeepWiki about `aer`](https://deepwiki.com/badge.svg)](https://deepwiki.com/caer/aer)

The toolkit for creatives.

## Quickstart

Here's the TL;DR to get going _quick_ with `aer` for web development:

1. `cargo install aer`
2. `cd` to a path containing assets you want to compile and serve over local HTTP
3. `aer init`
4. Configure your source and target ("build") paths in `Aer.toml`
4. `aer serve`
5. Check out your assets on [localhost:1337](http://localhost:1337)

Check out [what's here](#whats-here) to see everything else you can do.

## What's Here

### Asset Processors

`aer procs` runs a pipeline of asset processors defined in an `Aer.toml` configuration file. Processors transform source assets (Markdown, SCSS, images, templates) into production-ready output with support for profile-based configuration (e.g., development vs production settings).

> [!NOTE]
> Use `aer init` to create a starter `Aer.toml` in the current directory.

`aer serve` starts a local development server that watches for file changes and automatically rebuilds assets.

See [concept/README.md](concept/README.md) for detailed documentation.

### Color Palette Picker

![Picture of the `aer` color palette tool](docs/aer-colors.png)

`aer palette` launches an interactive color palette picker based on [Oklab Colorspace](https://bottosson.github.io/posts/oklab/) relationships.

## License and Contributions 

Copyright © 2026 With Caer, LLC.

Licensed under the Functional Source License, Version 1.1, MIT Future License.
Refer to [the license file](LICENSE.md) for more info.