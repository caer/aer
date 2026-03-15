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
const CACHE_FILE: &str = ".aer/tools/opengraph/cache.toml";

/// Directory for cached OG images, relative to project root.
const IMAGE_CACHE_DIR: &str = ".aer/tools/opengraph/images";

/// Default output directory for vendored images in the target.
const DEFAULT_IMAGE_DIR: &str = "opengraph";

/// Configuration for the opengraph tool in `Aer.toml`.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct OpenGraphConfig {
    pub cache_ttl: Option<u64>,
    pub images_target: Option<String>,
}

/// Result of resolving an `opengraph.aer.toml` file.
pub struct ResolveResult {
    /// Context entries to inject into `_assets:`.
    pub entries: Vec<Context>,
    /// Image assets to inject into the processing pipeline as `(relative_path, bytes)`.
    pub images: Vec<(String, Vec<u8>)>,
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

/// Returns a stable hash of a URL for use as a cache filename.
fn url_hash(url: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Derives an image file extension from a Content-Type header value.
fn ext_from_content_type(ct: &str) -> Option<&str> {
    match ct.split(';').next()?.trim() {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/avif" => Some("avif"),
        "image/svg+xml" => Some("svg"),
        _ => None,
    }
}

/// Derives an image file extension from a URL path.
fn ext_from_url(url: &str) -> Option<&'static str> {
    let path = url.split('?').next()?;
    match path.rsplit('.').next()? {
        "jpg" | "jpeg" => Some("jpg"),
        "png" => Some("png"),
        "gif" => Some("gif"),
        "webp" => Some("webp"),
        "avif" => Some("avif"),
        "svg" => Some("svg"),
        _ => None,
    }
}

/// Checks if a cached image file exists for the given hash.
async fn find_cached_image(hash: &str, cache_dir: &Path) -> Option<String> {
    let prefix = format!("{}.", hash);
    let mut entries = tokio::fs::read_dir(cache_dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(&prefix) {
            return Some(name.to_string_lossy().into_owned());
        }
    }
    None
}

/// Downloads an image and caches it to disk. Returns the filename and bytes.
async fn cache_image(url: &str, cache_dir: &Path) -> Result<(String, Vec<u8>), String> {
    let hash = url_hash(url);

    // Return from disk cache if available.
    if let Some(filename) = find_cached_image(&hash, cache_dir).await {
        let bytes = tokio::fs::read(cache_dir.join(&filename))
            .await
            .map_err(|e| e.to_string())?;
        return Ok((filename, bytes));
    }

    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("image fetch failed: {}", e))?;

    let ext = response
        .headers()
        .get("content-type")
        .and_then(|ct| ct.to_str().ok())
        .and_then(ext_from_content_type)
        .or_else(|| ext_from_url(url))
        .unwrap_or("jpg");

    let filename = format!("{}.{}", hash, ext);
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("image read failed: {}", e))?;

    tokio::fs::create_dir_all(cache_dir)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::write(cache_dir.join(&filename), &bytes)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("Cached OG image: {} -> {}", url, filename);
    Ok((filename, bytes.to_vec()))
}

/// Resolves OpenGraph metadata from an `opengraph.aer.toml` file
/// and returns context entries for injection into the asset pipeline.
pub async fn resolve(
    content: &str,
    config: &OpenGraphConfig,
    project_root: &Path,
) -> Result<ResolveResult, String> {
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

    // Phase 3: cache images concurrently.
    let image_dir = config.images_target.as_deref().unwrap_or(DEFAULT_IMAGE_DIR);
    let image_cache_dir = project_root.join(IMAGE_CACHE_DIR);
    let image_urls: Vec<(usize, String)> = parsed
        .link
        .iter()
        .enumerate()
        .zip(resolved.iter())
        .filter_map(|((i, link), og)| {
            let url = link.image.as_ref().or(og.image.as_ref())?;
            if url.starts_with("http://") || url.starts_with("https://") {
                Some((i, url.clone()))
            } else {
                None
            }
        })
        .collect();

    let mut image_results: Vec<Option<(String, Vec<u8>)>> = vec![None; parsed.link.len()];

    if !image_urls.is_empty() {
        let handles: Vec<_> = image_urls
            .iter()
            .map(|(_, url)| {
                let url = url.clone();
                let dir = image_cache_dir.clone();
                tokio::spawn(async move { cache_image(&url, &dir).await })
            })
            .collect();

        for (handle, (i, url)) in handles.into_iter().zip(image_urls.iter()) {
            match handle.await.unwrap_or(Err("task panicked".into())) {
                Ok(result) => {
                    image_results[*i] = Some(result);
                }
                Err(e) => {
                    tracing::warn!("Failed to cache image for {}: {}", url, e);
                }
            }
        }
    }

    // Phase 4: build context entries, collect image assets, and save cache.
    let mut entries = Vec::with_capacity(parsed.link.len());
    let mut images = Vec::new();

    for (i, (link, og)) in parsed.link.iter().zip(resolved).enumerate() {
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

        // Vendor remote images as pipeline assets; pass through local paths.
        if let Some((filename, bytes)) = image_results[i].take() {
            let asset_path = format!("{}/{}", image_dir, filename);
            ctx.insert(
                "image".into(),
                ContextValue::AssetRef(asset_path.clone().into()),
            );
            images.push((asset_path, bytes));
        } else if let Some(image) = link.image.as_ref().or(og.image.as_ref()) {
            ctx.insert("image".into(), ContextValue::Text(image.clone().into()));
        }

        if let Some(date) = link.date.as_ref().or(og.date.as_ref()) {
            ctx.insert("date".into(), ContextValue::Text(date.clone().into()));
        }

        ctx.insert("path".into(), ContextValue::Text(link.url.clone().into()));
        ctx.insert("feed".into(), ContextValue::Text("true".into()));

        entries.push(ctx);
    }

    save_cache(&cache_path, &cache).await;

    Ok(ResolveResult { entries, images })
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
    fn url_hash_is_deterministic() {
        let h1 = url_hash("https://example.com/article");
        let h2 = url_hash("https://example.com/article");
        let h3 = url_hash("https://example.com/other");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 16);
    }

    #[test]
    fn ext_from_content_type_maps_correctly() {
        assert_eq!(ext_from_content_type("image/jpeg"), Some("jpg"));
        assert_eq!(ext_from_content_type("image/png"), Some("png"));
        assert_eq!(ext_from_content_type("image/webp"), Some("webp"));
        assert_eq!(
            ext_from_content_type("image/jpeg; charset=utf-8"),
            Some("jpg")
        );
        assert_eq!(ext_from_content_type("text/html"), None);
    }

    #[test]
    fn ext_from_url_extracts_extension() {
        assert_eq!(ext_from_url("https://example.com/img.png"), Some("png"));
        assert_eq!(
            ext_from_url("https://example.com/img.jpg?w=100"),
            Some("jpg")
        );
        assert_eq!(ext_from_url("https://example.com/img.jpeg"), Some("jpg"));
        assert_eq!(ext_from_url("https://cdn.example.com/fetch/image"), None);
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
