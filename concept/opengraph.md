# OpenGraph Tool

The OpenGraph tool resolves metadata from external URLs and injects it into the asset processing context. This enables mixing external article links (e.g., Substack posts) alongside local content in template loops.

## Setup

Add `opengraph = {}` to `[default.tools]` in `Aer.toml`:

```toml
[default.tools]
opengraph = {}
```

Optional: set a custom cache TTL (in seconds, default 24 hours):

```toml
[default.tools]
opengraph = { cache_ttl = 3600 }
```

## File Convention

Create an `opengraph.aer.toml` file in any asset directory. Entries are injected into `_assets:{dir}` for that directory.

For example, `site/logs/opengraph.aer.toml` injects entries into `_assets:logs`, alongside any existing assets in `site/logs/`.

## Format

Each link entry requires a `url`. All other fields are optional — if omitted, they're resolved from the page's OpenGraph meta tags.

```toml
# Minimal: all metadata resolved from OG tags
[[link]]
url = "https://example.com/article"

# With overrides: explicit values take precedence over fetched OG values
[[link]]
url = "https://example.com/another-article"
title = "Custom Title"
description = "Custom description"
image = "https://example.com/custom-image.png"
date = "2025-04-17"
```

## OG Property Mapping

| OG Property | Context Key | Notes |
|---|---|---|
| `og:title` | `title` | |
| `og:description` | `description` | |
| `og:image` | `image` | |
| `article:published_time` | `date` | Maps to `date` for consistency with article frontmatter |

The `path` context key is set to the link's `url`.

## Caching

Fetched metadata is cached at `.aer/opengraph-cache.toml`. Entries are fresh for `cache_ttl` seconds (default 86400 / 24 hours).

On fetch failure with a stale cache entry, the stale data is used with a warning. On fetch failure with no cache, any explicit TOML values are used.

## Template Usage

Resolved entries appear in `_assets:` queries like any other asset:

```html
{~ for item in assets "logs" sort date desc }
<a href="{~ get item.path }">
    <h3>{~ get item.title }</h3>
    <p>{~ get item.description }</p>
</a>
{~ end }
```

In this example, both internal assets (with frontmatter) and external links (from `opengraph.aer.toml`) are iterated together, sorted by date.
