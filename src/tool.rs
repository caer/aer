//! This module contains implementations for the interactive tools.

mod color;
pub mod kits;
pub mod palette;
pub mod procs;
pub mod serve;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio::fs;

use crate::tool::procs::ProcessorConfig;

/// Default configuration profile.
const DEFAULT_CONFIG_PROFILE: &str = "default";

/// Default configuration file name.
pub const DEFAULT_CONFIG_FILE: &str = "Aer.toml";

/// Default configuration file contents.
pub const DEFAULT_CONFIG_TOML: &str = r#"# Aer asset processing configuration
# See: https://github.com/caer/aer

[default.paths]
source = "site/"
target = "public/"
# If true, `text/html` files will be emitted with clean URLs.
# For example, "about.html" becomes "about/index.html".
clean_urls = true

[default.context]
title = "Aer Site"

[default.procs]
markdown = {}
template = {}
pattern = {}
canonicalize = { root = "http://localhost:1337/" }
scss = {}
minify_html = {}
minify_js = {}
image = { max_width = 1920, max_height = 1920 }
favicon = {}

[production.procs]
canonicalize = { root = "https://www.example.com/" }
"#;

/// Configuration for a single kit.
#[derive(Debug, Deserialize, Clone)]
pub struct KitConfig {
    #[serde(rename = "git")]
    pub git_url: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    #[serde(default)]
    pub dest: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

/// Raw TOML structure of an `Aer.toml` file.
///
/// This is an internal representation used during deserialization.
/// External consumers should use [Config] (returned by [load_config]).
#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    kits: BTreeMap<String, KitConfig>,
    #[serde(flatten)]
    profiles: BTreeMap<String, ConfigProfile>,
}

/// Profile-level configuration in a [Config].
#[derive(Clone, Debug, Deserialize)]
pub struct ConfigProfile {
    #[serde(default)]
    procs: BTreeMap<String, ProcessorConfig>,
    #[serde(default)]
    context: toml::Table,
    #[serde(default)]
    paths: PathsConfig,
}

/// Path configuration in a [ConfigProfile].
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PathsConfig {
    pub source: Option<String>,
    pub target: Option<String>,
    #[serde(default)]
    pub clean_urls: Option<bool>,
}

impl ConfigProfile {
    /// Merges this profile with another, with `other`
    /// taking precedence, and returning the merged profile.
    fn merge(&self, other: &ConfigProfile) -> ConfigProfile {
        let mut merged = self.clone();

        // Merge paths
        if other.paths.source.is_some() {
            merged.paths.source = other.paths.source.clone();
        }
        if other.paths.target.is_some() {
            merged.paths.target = other.paths.target.clone();
        }
        if other.paths.clean_urls.is_some() {
            merged.paths.clean_urls = other.paths.clean_urls;
        }

        // Merge context
        for (key, value) in &other.context {
            merged.context.insert(key.clone(), value.clone());
        }

        // Merge procs
        for (key, value) in &other.procs {
            merged.procs.insert(key.clone(), value.clone());
        }

        merged
    }
}

/// A loaded and resolved `Aer.toml` configuration.
#[derive(Debug)]
pub struct Config {
    pub profile: ConfigProfile,
    pub kits: BTreeMap<String, KitConfig>,
    pub config_dir: PathBuf,
}

/// Loads, validates, and merges an `Aer.toml` configuration file.
///
/// Reads the file at `config_path`, then delegates to [load_config_from_str].
pub async fn load_config(config_path: &Path, profile: Option<&str>) -> io::Result<Config> {
    let config_dir = config_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let toml_str = fs::read_to_string(config_path).await?;
    load_config_from_str(&toml_str, config_dir, profile)
}

/// Parses, validates, and merges an `Aer.toml` configuration string.
///
/// Validates that no reserved top-level keys are used as profile names,
/// and merges the selected profile over the default.
fn load_config_from_str(
    toml_str: &str,
    config_dir: PathBuf,
    profile: Option<&str>,
) -> io::Result<Config> {
    let profile_name = profile.unwrap_or(DEFAULT_CONFIG_PROFILE);

    let raw: RawConfig = toml::from_str(toml_str)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid TOML: {}", e)))?;

    let default_profile = raw.profiles.get(DEFAULT_CONFIG_PROFILE).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("missing default profile: {}", DEFAULT_CONFIG_PROFILE),
        )
    })?;

    let merged = if profile_name == DEFAULT_CONFIG_PROFILE {
        default_profile.clone()
    } else {
        let selected = raw.profiles.get(profile_name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("missing selected profile: {}", profile_name),
            )
        })?;
        default_profile.merge(selected)
    };

    Ok(Config {
        profile: merged,
        kits: raw.kits,
        config_dir,
    })
}

/// Creates a default configuration file in the current directory if one doesn't exist.
pub async fn init() -> std::io::Result<()> {
    let config_path = Path::new(DEFAULT_CONFIG_FILE);

    if fs::try_exists(config_path).await? {
        tracing::warn!("{} already exists", DEFAULT_CONFIG_FILE);
        return Ok(());
    }

    fs::write(config_path, DEFAULT_CONFIG_TOML).await?;
    tracing::info!("Created {}", DEFAULT_CONFIG_FILE);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_profiles() {
        let toml = r#"
[default.paths]
source = "site/"
target = "public/"
clean_urls = false

[default.procs]
canonicalize = { root = "http://localhost/" }
js_bundle = { minify = false }

[production.paths]
target = "dist/"
clean_urls = true

[production.procs]
canonicalize = { root = "https://prod.example.com/" }
js_bundle = { minify = true }
"#;
        let config = load_config_from_str(toml, PathBuf::from("."), Some("production")).unwrap();

        // Paths should be merged (source from default, target from production).
        assert_eq!(config.profile.paths.source.as_deref(), Some("site/"));
        assert_eq!(config.profile.paths.target.as_deref(), Some("dist/"));
        assert_eq!(config.profile.paths.clean_urls, Some(true));
        // Procs from both profiles should be present.
        assert!(config.profile.procs.contains_key("canonicalize"));
        assert!(config.profile.procs.contains_key("js_bundle"));
    }

    #[test]
    fn uses_default_profile() {
        let toml = r#"
[default.paths]
source = "site/"
target = "public/"
"#;
        let config = load_config_from_str(toml, PathBuf::from("."), None).unwrap();
        assert_eq!(config.profile.paths.source.as_deref(), Some("site/"));
    }

    #[test]
    fn parses_kits_separately_from_profiles() {
        let toml = r#"
[kits.base]
git = "git@github.com:example/kit.git"
ref = "v1.0.0"

[default.paths]
source = "site/"
"#;
        let config = load_config_from_str(toml, PathBuf::from("."), None).unwrap();
        assert_eq!(config.kits.len(), 1);
        assert!(config.kits.contains_key("base"));
        assert_eq!(
            config.kits["base"].git_url,
            "git@github.com:example/kit.git"
        );
    }

    #[test]
    fn rejects_missing_default_profile() {
        let toml = r#"
[production.paths]
source = "site/"
"#;
        let result = load_config_from_str(toml, PathBuf::from("."), None);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_missing_selected_profile() {
        let toml = r#"
[default.paths]
source = "site/"
"#;
        let result = load_config_from_str(toml, PathBuf::from("."), Some("staging"));
        assert!(result.is_err());
    }
}
