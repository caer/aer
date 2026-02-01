//! This module contains implementations for the interactive tools.

mod color;
pub mod palette;
pub mod procs;
pub mod serve;

use std::{collections::BTreeMap, path::Path};

use serde::Deserialize;
use tokio::fs;

use crate::tool::procs::ProcessorConfig;

/// Default configuration profile.
pub const DEFAULT_CONFIG_PROFILE: &str = "default";

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

/// Global configuration.
///
/// This is a top-level configuration containing
/// a named table for each profile.
#[derive(Debug, Deserialize)]
pub struct Config {
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
