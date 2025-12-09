# NPM Fetch Tool Examples

## Basic Usage

The `npm_fetch` tool allows you to download NPM packages and their dependencies, extract them into a node_modules structure, and bundle JavaScript applications.

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

## Bundle with Packages

After downloading packages, you can bundle a JavaScript application that uses them:

```rust
use aer::tool::npm_fetch::NpmFetcher;

// Download packages
let mut fetcher = NpmFetcher::new("./npm_cache");
fetcher.fetch("lodash", Some("latest")).unwrap();
fetcher.fetch("react", Some("latest")).unwrap();

// Bundle your application
let bundled_code = fetcher.bundle_with_packages(
    "./src/app.js",  // Your entry point
    "./output"       // Where node_modules will be created
).unwrap();

// Save the bundled output
std::fs::write("./output/bundle.js", bundled_code).unwrap();
```

### Your Entry Point Example

Your `app.js` can import packages normally:

```javascript
import _ from 'lodash';
import React from 'react';

export function myApp() {
    const data = _.chunk(['a', 'b', 'c', 'd'], 2);
    return React.createElement('div', null, 'Hello!');
}
```

## Extract Packages Only

If you just want to extract packages without bundling:

```rust
use aer::tool::npm_fetch::NpmFetcher;

let fetcher = NpmFetcher::new("./packages");
fetcher.extract_packages("./output").unwrap();
```

This creates a `./output/node_modules/` directory with all downloaded packages.

## Output Structure

### Downloaded Tarballs

Each package is initially saved as a tarball:

```
./packages/
  ├── lodash-4.17.21/
  │   └── package.tgz
  ├── at_lexical_rich-text-0.17.1/
  │   └── package.tgz
  └── react-18.2.0/
      └── package.tgz
```

### Extracted node_modules

After extraction, packages are organized in a node_modules structure:

```
./output/
  └── node_modules/
      ├── lodash/
      │   ├── package.json
      │   └── ...
      ├── @lexical/
      │   └── rich-text/
      │       ├── package.json
      │       └── ...
      └── react/
          ├── package.json
          └── ...
```

Note: Scoped packages (with `@`) maintain their scope directory structure.

## Supported Packages

This tool supports:
- Regular packages (e.g., `lodash`, `react`)
- Scoped packages (e.g., `@lexical/rich-text`, `@tiptap/core`)
- Version specifiers (e.g., `latest`, `1.0.0`, `^1.0.0`, `~1.2.3`)

## Dependencies

The tool recursively downloads all dependencies of the specified package.
