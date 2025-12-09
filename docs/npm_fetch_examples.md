# NPM Fetch Tool Examples

## Basic Usage

The `npm_fetch` tool allows you to download NPM packages and their dependencies.

### Fetch a package with latest version

```rust
use aer::tool::npm_fetch::NpmFetcher;

let mut fetcher = NpmFetcher::new("./packages");
fetcher.fetch("lodash", Some("latest")).unwrap();
```

### Fetch a scoped package

```rust
use aer::tool::npm_fetch::NpmFetcher;

let mut fetcher = NpmFetcher::new("./packages");
fetcher.fetch("@lexical/rich-text", Some("latest")).unwrap();
```

### Fetch a specific version

```rust
use aer::tool::npm_fetch::NpmFetcher;

let mut fetcher = NpmFetcher::new("./packages");
fetcher.fetch("react", Some("18.2.0")).unwrap();
```

## Output Structure

Each package is saved as a tarball in a subdirectory:

```
./packages/
  ├── lodash-4.17.21/
  │   └── package.tgz
  ├── _lexical_rich-text-0.17.1/
  │   └── package.tgz
  └── react-18.2.0/
      └── package.tgz
```

## Supported Packages

This tool supports:
- Regular packages (e.g., `lodash`, `react`)
- Scoped packages (e.g., `@lexical/rich-text`, `@tiptap/core`)
- Version specifiers (e.g., `latest`, `1.0.0`, `^1.0.0`, `~1.2.3`)

## Dependencies

The tool recursively downloads all dependencies of the specified package.
