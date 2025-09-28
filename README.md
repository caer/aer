[![`aer` on crates.io](https://img.shields.io/crates/v/aer)](https://crates.io/crates/aer)
[![`aer` on docs.rs](https://img.shields.io/docsrs/aer)](https://docs.rs/aer/)
[![Ask DeepWiki about `aer`](https://deepwiki.com/badge.svg)](https://deepwiki.com/caer/aer)

A command-line toolkit for creatives.

## What's Here

> [!NOTE]
> This crate is a work-in-progress toolkit for supporting the entire process of creating static web content, from concept to deployment.
> The way the tools are organized will likely change in future versions of the crate.

### Color Palette Picker

![Picture of the `aer` color palette tool](docs/aer-colors.png)

The default entrypoint (via `cargo run`, or by running `aer` after `cargo install aer`) launches an interactive color palette picker based on [Oklab Colorspace](https://bottosson.github.io/posts/oklab/) relationships.

### Asset Processors

> [!WARNING]
> While these processors _are_ implemented within the `aer` library, they aren't _currently_ exposed via a command-line interface. Work-in-progress!

In addition to the interactive color picker, the `aer` crate exposes a collection of ["asset processors"](src/proc.rs) which can be assembled into a pipeline for compiling static websites.

## License and Contributions 

Copyright © 2025 With Caer, LLC.

Licensed under the Functional Source License, Version 1.1, MIT Future License.
Refer to [the license file](LICENSE.txt) for more info.