//! OpenGraph tool resolves metadata from external URLs
//! and injects it into the asset processing context.

use std::collections::BTreeMap;
use std::path::Path;

use lol_html::{RewriteStrSettings, element, rewrite_str};
use serde::Deserialize;

use crate::proc::{Context, ContextValue};

/// Default cache TTL in seconds (24 hours).
const DEFAULT_CACHE_TTL: u64 = 86400;

/// Cache file location relative to project root.
const CACHE_FILE: &str = ".aer/opengraph-cache.toml";

/// Configuration for the opengraph tool in `Aer.toml`.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct OpenGraphConfig {
    pub cache_ttl: Option<u64>,
}

/// A single link entry from the TOML file.
#[derive(Debug, Deserialize)]
struct LinkEntry {
    url: String,
    title: Option<String>,
    description: Option<String>,
    image: Option<String>,
    date: Option<String>,
}

/// The TOML structure of an opengraph.aer.toml file.
#[derive(Debug, Deserialize)]
struct OpenGraphToml {
    link: Vec<LinkEntry>,
}

/// A cached OG entry.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
struct CachedEntry {
    title: Option<String>,
    description: Option<String>,
    image: Option<String>,
    date: Option<String>,
    fetched_at: u64,
}

type CacheMap = BTreeMap<String, CachedEntry>;

/// Resolved OG metadata (may be partially empty).
#[derive(Debug, Default)]
struct OgMetadata {
    title: Option<String>,
    description: Option<String>,
    image: Option<String>,
    date: Option<String>,
}

impl From<&CachedEntry> for OgMetadata {
    fn from(cached: &CachedEntry) -> Self {
        Self {
            title: cached.title.clone(),
            description: cached.description.clone(),
            image: cached.image.clone(),
            date: cached.date.clone(),
        }
    }
}

/// Resolves OpenGraph metadata from an `opengraph.aer.toml` file
/// and returns context entries for injection into the asset pipeline.
pub async fn resolve(
    content: &str,
    config: &OpenGraphConfig,
    project_root: &Path,
) -> Result<Vec<Context>, String> {
    let parsed: OpenGraphToml =
        toml::from_str(content).map_err(|e| format!("malformed opengraph data: {}", e))?;

    let cache_ttl = config.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL);
    let cache_path = project_root.join(CACHE_FILE);
    let mut cache = load_cache(&cache_path).await;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Phase 1: check cache, resolve hits, collect misses.
    let mut resolved: Vec<OgMetadata> = Vec::with_capacity(parsed.link.len());
    let mut misses: Vec<(usize, &str)> = Vec::new();

    for (i, link) in parsed.link.iter().enumerate() {
        if let Some(cached) = cache.get(&link.url)
            && cached.fetched_at + cache_ttl > now
        {
            tracing::debug!("OG cache hit for {}", link.url);
            resolved.push(OgMetadata::from(cached));
        } else {
            misses.push((i, &link.url));
            resolved.push(OgMetadata::default()); // placeholder
        }
    }

    // Phase 2: fetch all misses concurrently.
    if !misses.is_empty() {
        let handles: Vec<_> = misses
            .iter()
            .map(|(_, url)| {
                let url = url.to_string();
                tokio::spawn(async move { fetch_og_metadata(&url).await })
            })
            .collect();
        let fetch_results: Vec<_> = {
            let mut results = Vec::with_capacity(handles.len());
            for handle in handles {
                results.push(handle.await.unwrap_or(Err("task panicked".into())));
            }
            results
        };

        for ((i, _), result) in misses.iter().zip(fetch_results) {
            let link = &parsed.link[*i];
            match result {
                Ok(og) => {
                    tracing::info!("Fetched OG metadata for {}", link.url);
                    cache.insert(
                        link.url.clone(),
                        CachedEntry {
                            title: og.title.clone(),
                            description: og.description.clone(),
                            image: og.image.clone(),
                            date: og.date.clone(),
                            fetched_at: now,
                        },
                    );
                    resolved[*i] = og;
                }
                Err(e) => {
                    // On failure, try stale cache.
                    if let Some(cached) = cache.get(&link.url) {
                        tracing::warn!(
                            "Failed to fetch OG for {}, using stale cache: {}",
                            link.url,
                            e
                        );
                        resolved[*i] = OgMetadata::from(cached);
                    } else {
                        tracing::error!("Failed to fetch OG for {}: {}", link.url, e);
                    }
                }
            }
        }
    }

    // Phase 3: build context entries and save cache.
    let mut results = Vec::with_capacity(parsed.link.len());

    for (link, og) in parsed.link.iter().zip(resolved) {
        let mut ctx = Context::new();

        if let Some(title) = link.title.as_ref().or(og.title.as_ref()) {
            ctx.insert("title".into(), ContextValue::Text(title.clone().into()));
        }
        if let Some(description) = link.description.as_ref().or(og.description.as_ref()) {
            ctx.insert(
                "description".into(),
                ContextValue::Text(description.clone().into()),
            );
        }
        if let Some(image) = link.image.as_ref().or(og.image.as_ref()) {
            ctx.insert("image".into(), ContextValue::Text(image.clone().into()));
        }
        if let Some(date) = link.date.as_ref().or(og.date.as_ref()) {
            ctx.insert("date".into(), ContextValue::Text(date.clone().into()));
        }

        ctx.insert("path".into(), ContextValue::Text(link.url.clone().into()));
        ctx.insert("feed".into(), ContextValue::Text("true".into()));

        results.push(ctx);
    }

    save_cache(&cache_path, &cache).await;

    Ok(results)
}

/// Fetches and parses OG metadata from a URL.
async fn fetch_og_metadata(url: &str) -> Result<OgMetadata, String> {
    let response = reqwest::get(url).await.map_err(|e| e.to_string())?;
    let html = response.text().await.map_err(|e| e.to_string())?;
    parse_og_metadata(&html)
}

/// Parses OG metadata from an HTML string using lol_html.
fn parse_og_metadata(html: &str) -> Result<OgMetadata, String> {
    use std::cell::RefCell;

    let result = RefCell::new(OgMetadata::default());

    let _ = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![element!("meta[property]", |el| {
                if let (Some(prop), Some(content)) =
                    (el.get_attribute("property"), el.get_attribute("content"))
                {
                    let mut r = result.borrow_mut();
                    match prop.as_str() {
                        "og:title" => r.title = Some(content),
                        "og:description" => r.description = Some(content),
                        "og:image" => r.image = Some(content),
                        "article:published_time" => r.date = Some(content),
                        _ => {}
                    }
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    )
    .map_err(|e| e.to_string())?;

    Ok(result.into_inner())
}

/// Loads the OG cache from disk.
async fn load_cache(path: &Path) -> CacheMap {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    }
}

/// Saves the OG cache to disk.
async fn save_cache(path: &Path, cache: &CacheMap) {
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    match toml::to_string_pretty(cache) {
        Ok(content) => {
            if let Err(e) = tokio::fs::write(path, content).await {
                tracing::warn!("Failed to write OG cache: {}", e);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to serialize OG cache: {}", e);
        }
    }
}

/// Dispatches a `.aer.toml` file to the appropriate tool.
/// Returns `None` if the filename is not recognized.
pub fn tool_for_filename(filename: &str) -> Option<&'static str> {
    match filename {
        "opengraph.aer.toml" => Some("opengraph"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_og_metadata_from_html() {
        let html = r#"
            <html><head>
                <meta property="og:title" content="Test Article" />
                <meta property="og:description" content="A test description" />
                <meta property="og:image" content="https://example.com/image.png" />
                <meta property="article:published_time" content="2025-04-17T00:00:00Z" />
            </head><body></body></html>
        "#;

        let og = parse_og_metadata(html).unwrap();
        assert_eq!(og.title.as_deref(), Some("Test Article"));
        assert_eq!(og.description.as_deref(), Some("A test description"));
        assert_eq!(og.image.as_deref(), Some("https://example.com/image.png"));
        assert_eq!(og.date.as_deref(), Some("2025-04-17T00:00:00Z"));
    }

    #[test]
    fn parses_partial_og_metadata() {
        let html = r#"
            <html><head>
                <meta property="og:title" content="Only Title" />
            </head><body></body></html>
        "#;

        let og = parse_og_metadata(html).unwrap();
        assert_eq!(og.title.as_deref(), Some("Only Title"));
        assert!(og.description.is_none());
        assert!(og.image.is_none());
        assert!(og.date.is_none());
    }

    #[test]
    fn toml_overrides_og_values() {
        let link = LinkEntry {
            url: "https://example.com".into(),
            title: Some("Override Title".into()),
            description: None,
            image: None,
            date: Some("2025-04-17".into()),
        };

        let og = OgMetadata {
            title: Some("OG Title".into()),
            description: Some("OG Description".into()),
            image: Some("https://example.com/og.png".into()),
            date: Some("2025-04-17T00:00:00Z".into()),
        };

        let title = link.title.as_ref().or(og.title.as_ref());
        let description = link.description.as_ref().or(og.description.as_ref());
        let image = link.image.as_ref().or(og.image.as_ref());
        let date = link.date.as_ref().or(og.date.as_ref());

        assert_eq!(title.unwrap(), "Override Title");
        assert_eq!(description.unwrap(), "OG Description");
        assert_eq!(image.unwrap(), "https://example.com/og.png");
        assert_eq!(date.unwrap(), "2025-04-17");
    }
}
