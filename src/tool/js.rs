//! JavaScript Module Management
//!
//! This module provides comprehensive functionality for working with JavaScript modules
//! from the NPM registry. It handles downloading packages and their dependencies,
//! extracting them into a proper node_modules structure, and bundling JavaScript
//! applications for direct use in web apps.
//!
//! # Basic Usage
//!
//! ## Fetching JavaScript Modules
//!
//! ```no_run
//! use aer::tool::js::JsModuleManager;
//!
//! let mut manager = JsModuleManager::new("./packages");
//!
//! // Fetch a package with latest version
//! manager.fetch("lodash", Some("latest")).unwrap();
//!
//! // Fetch a scoped package
//! manager.fetch("@lexical/rich-text", Some("latest")).unwrap();
//!
//! // Fetch a specific version
//! manager.fetch("react", Some("18.2.0")).unwrap();
//! ```
//!
//! ## Bundling Applications
//!
//! After downloading modules, bundle a JavaScript application that uses them:
//!
//! ```no_run
//! use aer::tool::js::JsModuleManager;
//!
//! // Download modules
//! let mut manager = JsModuleManager::new("./cache");
//! manager.fetch("lodash", Some("latest")).unwrap();
//! manager.fetch("react", Some("latest")).unwrap();
//!
//! // Bundle your application
//! let bundled_code = manager.bundle(
//!     "./src/app.js",  // Your entry point
//!     "./output"       // Where node_modules will be created
//! ).unwrap();
//!
//! // Save the bundled output
//! std::fs::write("./output/bundle.js", bundled_code).unwrap();
//! ```
//!
//! Your `app.js` entry point can import modules normally:
//!
//! ```javascript
//! import _ from 'lodash';
//! import React from 'react';
//!
//! export function myApp() {
//!     const data = _.chunk(['a', 'b', 'c', 'd'], 2);
//!     return React.createElement('div', null, 'Hello!');
//! }
//! ```
//!
//! ## Extracting Modules Only
//!
//! If you just want to extract modules without bundling:
//!
//! ```no_run
//! use aer::tool::js::JsModuleManager;
//!
//! let manager = JsModuleManager::new("./packages");
//! manager.extract_modules("./output").unwrap();
//! ```
//!
//! This creates a `./output/node_modules/` directory with all downloaded modules.
//!
//! # Features
//!
//! - **NPM Registry Integration**: Downloads JavaScript modules as tarballs from NPM
//! - **Dependency Resolution**: Recursively fetches all module dependencies
//! - **Scoped Packages**: Full support for scoped packages (e.g., `@lexical/rich-text`)
//! - **Version Management**: Supports version specifiers (`latest`, `1.0.0`, `^1.0.0`, `~1.2.3`)
//! - **Module Extraction**: Extracts modules into standard node_modules structure
//! - **Application Bundling**: Bundles JavaScript apps with module resolution for web deployment
//!
//! # Directory Structure
//!
//! ## Download Cache
//!
//! Each module is initially cached as a tarball:
//!
//! ```text
//! ./cache/
//!   ├── lodash-4.17.21/
//!   │   └── package.tgz
//!   ├── at_lexical_rich-text-0.17.1/
//!   │   └── package.tgz
//!   └── react-18.2.0/
//!       └── package.tgz
//! ```
//!
//! ## Extracted Modules
//!
//! After extraction, modules follow the standard Node.js structure:
//!
//! ```text
//! ./output/
//!   └── node_modules/
//!       ├── lodash/
//!       │   ├── package.json
//!       │   └── ...
//!       ├── @lexical/
//!       │   └── rich-text/
//!       │       ├── package.json
//!       │       └── ...
//!       └── react/
//!           ├── package.json
//!           └── ...
//! ```
//!
//! Note: Scoped packages (with `@`) maintain their scope directory structure.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Error types for JavaScript module operations
#[derive(Debug)]
pub enum JsModuleError {
    /// HTTP request failed
    HttpError(String),
    /// JSON parsing failed
    JsonError(String),
    /// File system operation failed
    IoError(String),
    /// Module not found
    ModuleNotFound(String),
    /// Invalid module name or version
    InvalidModule(String),
}

impl std::fmt::Display for JsModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsModuleError::HttpError(msg) => write!(f, "HTTP error: {}", msg),
            JsModuleError::JsonError(msg) => write!(f, "JSON error: {}", msg),
            JsModuleError::IoError(msg) => write!(f, "IO error: {}", msg),
            JsModuleError::ModuleNotFound(pkg) => write!(f, "Module not found: {}", pkg),
            JsModuleError::InvalidModule(msg) => write!(f, "Invalid module: {}", msg),
        }
    }
}

impl std::error::Error for JsModuleError {}

/// Response from NPM registry API for package metadata
#[derive(Debug, Deserialize)]
struct NpmPackageMetadata {
    versions: HashMap<String, NpmVersionMetadata>,
    #[serde(rename = "dist-tags")]
    dist_tags: HashMap<String, String>,
}

/// Metadata for a specific version of a package
#[derive(Debug, Deserialize)]
struct NpmVersionMetadata {
    dist: NpmDist,
    dependencies: Option<HashMap<String, String>>,
}

/// Distribution information for a package version
#[derive(Debug, Deserialize)]
struct NpmDist {
    tarball: String,
}

/// Manages JavaScript modules from NPM for web application bundling
pub struct JsModuleManager {
    /// Base URL for the NPM registry
    registry_url: String,
    /// Cache directory to download modules to
    cache_dir: PathBuf,
    /// Set of already fetched modules to avoid duplicates
    fetched: HashSet<String>,
}

impl JsModuleManager {
    /// Creates a new JavaScript module manager with the default NPM registry
    pub fn new<P: AsRef<Path>>(cache_dir: P) -> Self {
        Self {
            registry_url: "https://registry.npmjs.org".to_string(),
            cache_dir: cache_dir.as_ref().to_path_buf(),
            fetched: HashSet::new(),
        }
    }

    /// Fetches a JavaScript module and all its dependencies recursively from NPM
    ///
    /// # Arguments
    /// * `module_name` - Name of the module (e.g., "@lexical/rich-text", "lodash")
    /// * `version_spec` - Version specifier (e.g., "latest", "1.0.0", "^1.0.0")
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(JsModuleError)` otherwise
    pub fn fetch(
        &mut self,
        module_name: &str,
        version_spec: Option<&str>,
    ) -> Result<(), JsModuleError> {
        let version_spec = version_spec.unwrap_or("latest");

        tracing::info!("Fetching module: {} @ {}", module_name, version_spec);

        self.fetch_recursive(module_name, version_spec)
    }

    fn fetch_recursive(
        &mut self,
        module_name: &str,
        version_spec: &str,
    ) -> Result<(), JsModuleError> {
        // Fetch module metadata from registry
        let metadata = self.fetch_module_metadata(module_name)?;

        // Resolve version
        let version = self.resolve_version(&metadata, version_spec)?;

        // Check if we've already fetched this module at this exact version
        let module_key = format!("{}@{}", module_name, version);
        if self.fetched.contains(&module_key) {
            tracing::debug!("Module already fetched: {}", module_key);
            return Ok(());
        }

        // Get version metadata
        let version_metadata = metadata.versions.get(&version).ok_or_else(|| {
            JsModuleError::ModuleNotFound(format!("{} @ {}", module_name, version))
        })?;

        // Download the tarball
        self.download_tarball(module_name, &version, &version_metadata.dist.tarball)?;

        // Mark as fetched
        self.fetched.insert(module_key);

        // Fetch dependencies recursively
        if let Some(dependencies) = &version_metadata.dependencies {
            for (dep_name, dep_version) in dependencies {
                // Skip optional dependencies and handle version ranges
                let cleaned_version = self.clean_version_spec(dep_version);

                match self.fetch_recursive(dep_name, &cleaned_version) {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(
                            "Failed to fetch dependency {} @ {}: {}",
                            dep_name,
                            cleaned_version,
                            e
                        );
                        // Continue with other dependencies even if one fails
                    }
                }
            }
        }

        Ok(())
    }

    fn fetch_module_metadata(
        &self,
        module_name: &str,
    ) -> Result<NpmPackageMetadata, JsModuleError> {
        // Encode module name for URL (handle scoped packages)
        let encoded_name = module_name.replace('/', "%2F");
        let url = format!("{}/{}", self.registry_url, encoded_name);

        tracing::debug!("Fetching metadata from: {}", url);

        let mut response = ureq::get(&url)
            .call()
            .map_err(|e| JsModuleError::HttpError(format!("Failed to fetch {}: {}", url, e)))?;

        let body = response.body_mut().read_to_string().map_err(|e| {
            JsModuleError::HttpError(format!("Failed to read response body: {}", e))
        })?;

        serde_json::from_str(&body).map_err(|e| {
            JsModuleError::JsonError(format!("Failed to parse JSON for {}: {}", module_name, e))
        })
    }

    fn resolve_version(
        &self,
        metadata: &NpmPackageMetadata,
        version_spec: &str,
    ) -> Result<String, JsModuleError> {
        // Handle "latest" tag
        if version_spec == "latest"
            && let Some(latest) = metadata.dist_tags.get("latest")
        {
            return Ok(latest.clone());
        }

        // Handle other dist-tags
        if let Some(version) = metadata.dist_tags.get(version_spec) {
            return Ok(version.clone());
        }

        // If it's an exact version, check if it exists
        if metadata.versions.contains_key(version_spec) {
            return Ok(version_spec.to_string());
        }

        // For now, simple version matching - could be enhanced with semver
        // Just use the latest version if we can't resolve
        metadata.dist_tags.get("latest").cloned().ok_or_else(|| {
            JsModuleError::InvalidModule(format!(
                "Could not resolve version {} for module",
                version_spec
            ))
        })
    }

    fn clean_version_spec(&self, version_spec: &str) -> String {
        // Remove common version prefixes
        let trimmed = version_spec.trim();
        if trimmed.starts_with(">=") || trimmed.starts_with("<=") {
            trimmed[2..].trim().to_string()
        } else if trimmed.starts_with('^')
            || trimmed.starts_with('~')
            || trimmed.starts_with('>')
            || trimmed.starts_with('<')
            || trimmed.starts_with('=')
        {
            trimmed[1..].trim().to_string()
        } else {
            trimmed.to_string()
        }
    }

    fn download_tarball(
        &self,
        module_name: &str,
        version: &str,
        tarball_url: &str,
    ) -> Result<(), JsModuleError> {
        tracing::info!(
            "Downloading {} @ {} from {}",
            module_name,
            version,
            tarball_url
        );

        // Create module directory
        // Replace '@' and '/' to create safe filesystem names
        // Using 'at_' for '@' preserves the scoped package indicator
        let safe_module_name = module_name.replace('@', "at_").replace('/', "_");
        let module_dir = self
            .cache_dir
            .join(format!("{}-{}", safe_module_name, version));

        fs::create_dir_all(&module_dir).map_err(|e| {
            JsModuleError::IoError(format!(
                "Failed to create directory {}: {}",
                module_dir.display(),
                e
            ))
        })?;

        // Download tarball
        let mut response = ureq::get(tarball_url).call().map_err(|e| {
            JsModuleError::HttpError(format!("Failed to download {}: {}", tarball_url, e))
        })?;

        // Save tarball to file
        let tarball_path = module_dir.join("package.tgz");
        let mut file = fs::File::create(&tarball_path).map_err(|e| {
            JsModuleError::IoError(format!(
                "Failed to create file {}: {}",
                tarball_path.display(),
                e
            ))
        })?;

        // Use as_reader() to get a reader from the body
        let mut reader = response.body_mut().as_reader();
        std::io::copy(&mut reader, &mut file)
            .map_err(|e| JsModuleError::IoError(format!("Failed to write tarball: {}", e)))?;

        tracing::info!(
            "Saved {} @ {} to {}",
            module_name,
            version,
            tarball_path.display()
        );

        Ok(())
    }

    /// Extracts all downloaded modules into a node_modules structure
    ///
    /// # Arguments
    /// * `output_dir` - Directory where the node_modules structure should be created
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(JsModuleError)` otherwise
    pub fn extract_modules<P: AsRef<Path>>(&self, output_dir: P) -> Result<(), JsModuleError> {
        let output_dir = output_dir.as_ref();
        let node_modules_dir = output_dir.join("node_modules");

        tracing::info!("Extracting modules to {}", node_modules_dir.display());

        // Iterate through all subdirectories in cache_dir
        let entries = fs::read_dir(&self.cache_dir).map_err(|e| {
            JsModuleError::IoError(format!("Failed to read cache directory: {}", e))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                JsModuleError::IoError(format!("Failed to read directory entry: {}", e))
            })?;
            let path = entry.path();

            if path.is_dir() {
                let tarball_path = path.join("package.tgz");
                if tarball_path.exists() {
                    // Extract this tarball
                    self.extract_tarball(&tarball_path, &node_modules_dir)?;
                }
            }
        }

        tracing::info!("Extraction complete");
        Ok(())
    }

    fn extract_tarball(
        &self,
        tarball_path: &Path,
        node_modules_dir: &Path,
    ) -> Result<(), JsModuleError> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        tracing::debug!("Extracting {}", tarball_path.display());

        let file = fs::File::open(tarball_path)
            .map_err(|e| JsModuleError::IoError(format!("Failed to open tarball: {}", e)))?;

        let decompressor = GzDecoder::new(file);
        let mut archive = Archive::new(decompressor);

        // Extract to a temporary location first
        let temp_extract = node_modules_dir.join(".temp_extract");
        fs::create_dir_all(&temp_extract).map_err(|e| {
            JsModuleError::IoError(format!("Failed to create temp directory: {}", e))
        })?;

        archive
            .unpack(&temp_extract)
            .map_err(|e| JsModuleError::IoError(format!("Failed to extract tarball: {}", e)))?;

        // NPM tarballs contain a 'package' directory, move its contents to node_modules
        let package_dir = temp_extract.join("package");
        if package_dir.exists() {
            // Read package.json to get the real module name
            let package_json_path = package_dir.join("package.json");
            if package_json_path.exists() {
                let package_json_content = fs::read_to_string(&package_json_path).map_err(|e| {
                    JsModuleError::IoError(format!("Failed to read package.json: {}", e))
                })?;

                let package_info: serde_json::Value = serde_json::from_str(&package_json_content)
                    .map_err(|e| {
                    JsModuleError::JsonError(format!("Failed to parse package.json: {}", e))
                })?;

                if let Some(name) = package_info.get("name").and_then(|n| n.as_str()) {
                    // Handle scoped packages
                    let target_path = if name.starts_with('@') {
                        // For @scope/package, create @scope directory first
                        if let Some(slash_pos) = name.find('/') {
                            let scope = &name[..slash_pos];
                            let pkg_name = &name[slash_pos + 1..];
                            let scope_dir = node_modules_dir.join(scope);
                            fs::create_dir_all(&scope_dir).map_err(|e| {
                                JsModuleError::IoError(format!(
                                    "Failed to create scope directory: {}",
                                    e
                                ))
                            })?;
                            scope_dir.join(pkg_name)
                        } else {
                            node_modules_dir.join(name)
                        }
                    } else {
                        node_modules_dir.join(name)
                    };

                    // Remove existing module if it exists
                    if target_path.exists() {
                        fs::remove_dir_all(&target_path).map_err(|e| {
                            JsModuleError::IoError(format!(
                                "Failed to remove existing module: {}",
                                e
                            ))
                        })?;
                    }

                    // Move the module to node_modules
                    fs::rename(&package_dir, &target_path).map_err(|e| {
                        JsModuleError::IoError(format!(
                            "Failed to move module to node_modules: {}",
                            e
                        ))
                    })?;

                    tracing::debug!("Extracted {} to {}", name, target_path.display());
                }
            }
        }

        // Clean up temp directory
        if let Err(e) = fs::remove_dir_all(&temp_extract) {
            tracing::warn!("Failed to clean up temporary extraction directory: {}", e);
        }

        Ok(())
    }

    /// Bundles a JavaScript application that uses the downloaded modules
    ///
    /// This method:
    /// 1. Extracts all downloaded modules into `output_dir/node_modules`
    /// 2. Copies the entry script to the output directory
    /// 3. Bundles the script with access to the node_modules
    /// 4. Returns the bundled JavaScript code ready for web deployment
    ///
    /// # Arguments
    /// * `entry_script` - Path to the JavaScript entry point file
    /// * `output_dir` - Directory where the node_modules structure will be created
    ///
    /// # Returns
    /// `Ok(String)` containing the bundled JavaScript code, or `Err(JsModuleError)`
    ///
    /// # Example
    /// ```no_run
    /// # use aer::tool::js::JsModuleManager;
    /// let mut manager = JsModuleManager::new("./cache");
    /// manager.fetch("lodash", Some("latest")).unwrap();
    ///
    /// let bundled = manager.bundle(
    ///     "./my-app.js",
    ///     "./output"
    /// ).unwrap();
    /// std::fs::write("./output/bundle.js", bundled).unwrap();
    /// ```
    pub fn bundle<P: AsRef<Path>>(
        &self,
        entry_script: P,
        output_dir: P,
    ) -> Result<String, JsModuleError> {
        use crate::proc::js_bundle::JsBundleProcessor;
        use crate::proc::{Asset, ProcessesAssets};

        let entry_script = entry_script.as_ref();
        let output_dir = output_dir.as_ref();

        // First, ensure modules are extracted
        self.extract_modules(output_dir)?;

        // Copy the entry script to the output directory so node_modules can be resolved
        // This is necessary because the JS bundler uses the entry script's parent directory
        // as the working directory for module resolution
        let entry_filename = entry_script.file_name().ok_or_else(|| {
            JsModuleError::InvalidModule("Entry script must be a file".to_string())
        })?;
        let temp_entry = output_dir.join(entry_filename);

        fs::copy(entry_script, &temp_entry)
            .map_err(|e| JsModuleError::IoError(format!("Failed to copy entry script: {}", e)))?;

        // Create an asset for the entry script (using the temp location)
        let entry_content = fs::read(&temp_entry)
            .map_err(|e| JsModuleError::IoError(format!("Failed to read entry script: {}", e)))?;

        let mut asset = Asset::new(
            temp_entry.to_string_lossy().to_string().into(),
            entry_content,
        );

        // Use the JS bundle processor
        let processor = JsBundleProcessor { minify: false };
        let result = processor
            .process(&mut asset)
            .map_err(|e| JsModuleError::InvalidModule(format!("Bundling failed: {:?}", e)));

        // Clean up the temporary entry script
        if let Err(e) = fs::remove_file(&temp_entry) {
            tracing::warn!("Failed to clean up temporary entry script: {}", e);
        }

        result?;

        // Get the bundled code
        let bundled_code = asset.as_text().map_err(|e| {
            JsModuleError::InvalidModule(format!("Failed to get bundled code: {:?}", e))
        })?;

        Ok(bundled_code.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_version_spec() {
        let manager = JsModuleManager::new(std::env::temp_dir());

        assert_eq!(manager.clean_version_spec("^1.0.0"), "1.0.0");
        assert_eq!(manager.clean_version_spec("~1.2.3"), "1.2.3");
        assert_eq!(manager.clean_version_spec(">=2.0.0"), "2.0.0");
        assert_eq!(manager.clean_version_spec("1.0.0"), "1.0.0");
    }
}
