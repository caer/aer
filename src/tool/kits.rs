//! Kit resolution and pre-canonicalization.
//!
//! Kits are reusable asset packages fetched from git repositories
//! and made available to the processing pipeline under a namespace.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::process::Command;

use crate::proc::canonicalize::CanonicalizeProcessor;
use crate::proc::{MediaType, ProcessingError};
use crate::tool::KitConfig;

/// The directory within `.aer/` where kits are cached.
const KITS_DIR: &str = ".aer/kits";

/// Dummy root used during pre-canonicalization.
/// Replaced with `/` after processing.
const PRECANON_ROOT: &str = "http://KITPRECANON/";

/// A resolved kit ready for use in the build pipeline.
#[derive(Debug, Clone)]
pub struct ResolvedKit {
    pub name: String,
    pub local_path: PathBuf,
    pub dest: String,
}

/// Resolves all declared kits, returning their local paths.
///
/// Each kit repository must contain a `kit/` subdirectory. Only its
/// contents are treated as assets.
///
/// For each kit:
/// 1. If `path` is set and exists, use it directly.
/// 2. If cached and the git ref matches, reuse the cache.
/// 3. Otherwise, remove the stale clone and re-clone from git.
pub async fn resolve_kits(
    kits: &BTreeMap<String, KitConfig>,
    config_dir: &Path,
) -> io::Result<Vec<ResolvedKit>> {
    if kits.is_empty() {
        return Ok(Vec::new());
    }

    let kits_dir = config_dir.join(KITS_DIR);
    fs::create_dir_all(&kits_dir).await?;

    let mut resolved = Vec::with_capacity(kits.len());

    for (name, kit) in kits {
        let kit_dir = kits_dir.join(name);
        let dest = kit
            .dest
            .clone()
            .unwrap_or_else(|| format!("/vendor/kits/{}", name));

        // Local path override.
        if let Some(local_path) = &kit.path {
            let local = config_dir.join(local_path);
            if fs::try_exists(&local).await? {
                let kit_assets_dir = local.join("kit");
                if !fs::try_exists(&kit_assets_dir).await? {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("Kit `{}` has no `kit/` directory", name),
                    ));
                }
                tracing::info!("Kit `{}`: using local path {}", name, local.display());
                resolved.push(ResolvedKit {
                    name: name.clone(),
                    local_path: kit_assets_dir,
                    dest,
                });
                continue;
            }
            tracing::debug!(
                "Kit `{}`: local path {} not found, falling back to git",
                name,
                local.display()
            );
        }

        // Check cache: compare git state against configured ref.
        if fs::try_exists(&kit_dir).await? && !is_symlink(&kit_dir).await {
            if let Some(current) = git_current_ref(&kit_dir).await?
                && (current == kit.git_ref
                    || current.starts_with(&kit.git_ref)
                    || kit.git_ref.starts_with(&current))
            {
                tracing::info!("Kit `{}`: cached at ref {}", name, kit.git_ref);
                let kit_assets_dir = kit_dir.join("kit");
                if !fs::try_exists(&kit_assets_dir).await? {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("Kit `{}` has no `kit/` directory", name),
                    ));
                }
                resolved.push(ResolvedKit {
                    name: name.clone(),
                    local_path: kit_assets_dir,
                    dest,
                });
                continue;
            }
            // Ref changed or unreadable — remove stale clone.
            tracing::info!("Kit `{}`: ref changed, re-cloning", name);
            fs::remove_dir_all(&kit_dir).await?;
        } else if is_symlink(&kit_dir).await {
            // Symlink from a previous local override — remove it.
            fs::remove_file(&kit_dir).await?;
        }

        // Fresh clone.
        git_clone(&kit.git_url, &kit.git_ref, &kit_dir).await?;
        tracing::info!("Kit `{}`: resolved at ref {}", name, kit.git_ref);

        let kit_assets_dir = kit_dir.join("kit");
        if !fs::try_exists(&kit_assets_dir).await? {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Kit `{}` has no `kit/` directory", name),
            ));
        }

        resolved.push(ResolvedKit {
            name: name.clone(),
            local_path: kit_assets_dir,
            dest,
        });
    }

    Ok(resolved)
}

/// Clones a git repository at the given ref.
async fn git_clone(url: &str, git_ref: &str, dest: &Path) -> io::Result<()> {
    let output = Command::new("git")
        .args(["clone", "--depth", "1", "--branch", git_ref, url])
        .arg(dest)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "git clone failed: {}",
            stderr.trim()
        )));
    }

    Ok(())
}

/// Returns the current ref of a git repository by inspecting its state.
/// Tries branch name, then tag name, then commit hash.
async fn git_current_ref(repo: &Path) -> io::Result<Option<String>> {
    // Try branch name first.
    let branch = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .await?;
    if branch.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&branch.stdout).trim().to_string(),
        ));
    }

    // Try exact tag name.
    let tag = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["describe", "--tags", "--exact-match", "HEAD"])
        .output()
        .await?;
    if tag.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&tag.stdout).trim().to_string(),
        ));
    }

    // Fall back to commit hash.
    let hash = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .await?;
    if hash.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&hash.stdout).trim().to_string(),
        ));
    }

    Ok(None)
}

/// Returns true if the path is a symlink.
async fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .await
        .map(|m| m.is_symlink())
        .unwrap_or(false)
}

/// Pre-canonicalizes kit assets by rewriting relative URLs
/// to absolute paths under the kit's `dest`.
///
/// Returns a list of `(relative_output_path, content_bytes)` pairs.
pub fn pre_canonicalize_kit_assets(
    assets: &[(String, Vec<u8>)],
    dest: &str,
) -> Vec<(String, Vec<u8>)> {
    let processor = match CanonicalizeProcessor::new(PRECANON_ROOT) {
        Some(p) => p,
        None => return assets.to_vec(),
    };

    let dest_trimmed = dest.trim_start_matches('/');

    assets
        .iter()
        .map(|(relative_path, content)| {
            let prefixed_path = if dest_trimmed.is_empty() {
                relative_path.clone()
            } else {
                format!("{}/{}", dest_trimmed, relative_path)
            };

            let media_type = relative_path
                .rsplit('.')
                .next()
                .map(MediaType::from_extension)
                .unwrap_or(MediaType::Unknown {
                    extension: [String::new().into()],
                });

            let new_content = match media_type {
                MediaType::Css | MediaType::Scss => {
                    if let Ok(text) = std::str::from_utf8(content) {
                        let processed = processor.process_css(text, &prefixed_path);
                        let result = processed.replace(PRECANON_ROOT, "/");
                        result.into_bytes()
                    } else {
                        content.clone()
                    }
                }
                MediaType::Html => {
                    if let Ok(text) = std::str::from_utf8(content) {
                        match processor.process_html(text, &prefixed_path.into()) {
                            Ok(processed) => {
                                let result = processed.replace(PRECANON_ROOT, "/");
                                result.into_bytes()
                            }
                            Err(ProcessingError::Malformed { message }) => {
                                tracing::warn!(
                                    "Pre-canonicalization failed for {}: {}",
                                    relative_path,
                                    message
                                );
                                content.clone()
                            }
                            Err(_) => content.clone(),
                        }
                    } else {
                        content.clone()
                    }
                }
                _ => content.clone(),
            };

            (relative_path.clone(), new_content)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_canonicalizes_css_urls() {
        let css = r#"@font-face { src: url("../fonts/font.ttf"); }"#;
        let assets = vec![("styles/main.css".to_string(), css.as_bytes().to_vec())];

        let result = pre_canonicalize_kit_assets(&assets, "/vendor/kits/base");
        assert_eq!(result.len(), 1);

        let output = std::str::from_utf8(&result[0].1).unwrap();
        assert!(
            output.contains("/vendor/kits/base/fonts/font.ttf"),
            "Expected canonicalized URL, got: {}",
            output
        );
    }

    #[test]
    fn pre_canonicalizes_css_with_root_dest() {
        let css = r#"body { background: url("images/bg.png"); }"#;
        let assets = vec![("styles/main.css".to_string(), css.as_bytes().to_vec())];

        let result = pre_canonicalize_kit_assets(&assets, "/");
        let output = std::str::from_utf8(&result[0].1).unwrap();
        assert!(
            output.contains("/styles/images/bg.png"),
            "Expected canonicalized URL, got: {}",
            output
        );
    }

    #[test]
    fn pre_canonicalizes_html_hrefs() {
        let html = r#"<a href="../page.html">Link</a>"#;
        let assets = vec![("pages/index.html".to_string(), html.as_bytes().to_vec())];

        let result = pre_canonicalize_kit_assets(&assets, "/vendor/kits/base");
        let output = std::str::from_utf8(&result[0].1).unwrap();
        assert!(
            output.contains("/vendor/kits/base/page.html"),
            "Expected canonicalized URL, got: {}",
            output
        );
    }

    #[test]
    fn preserves_binary_assets() {
        let binary = vec![0u8, 1, 2, 3, 255];
        let assets = vec![("image.png".to_string(), binary.clone())];

        let result = pre_canonicalize_kit_assets(&assets, "/vendor/kits/base");
        assert_eq!(result[0].1, binary);
    }

    #[test]
    fn preserves_absolute_urls_in_css() {
        let css = r#"@font-face { src: url("/absolute/font.ttf"); }"#;
        let assets = vec![("styles/main.css".to_string(), css.as_bytes().to_vec())];

        let result = pre_canonicalize_kit_assets(&assets, "/vendor/kits/base");
        let output = std::str::from_utf8(&result[0].1).unwrap();
        // Absolute URLs get canonicalized against the PRECANON root,
        // then the root is stripped to /, leaving the original path.
        assert!(
            output.contains("/absolute/font.ttf"),
            "Expected preserved absolute URL, got: {}",
            output
        );
    }
}
