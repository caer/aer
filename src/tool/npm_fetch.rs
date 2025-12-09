//! NPM Package Fetcher
//!
//! This module provides functionality to download NPM packages and their dependencies
//! from the NPM registry, extract them into a node_modules structure, and bundle
//! JavaScript applications that use those packages.
//!
//! # Example
//!
//! ```no_run
//! use aer::tool::npm_fetch::NpmFetcher;
//!
//! // Download packages
//! let mut fetcher = NpmFetcher::new("./npm_cache");
//! fetcher.fetch("@lexical/rich-text", Some("latest")).unwrap();
//! fetcher.fetch("react", Some("18.2.0")).unwrap();
//! 
//! // Extract packages to node_modules and bundle your app
//! let bundled_code = fetcher.bundle_with_packages("./my-app.js", "./output").unwrap();
//! std::fs::write("./output/bundle.js", bundled_code).unwrap();
//! ```
//!
//! # Features
//!
//! - Downloads NPM packages as tarballs from the NPM registry
//! - Recursively fetches all dependencies
//! - Handles scoped packages (e.g., `@lexical/rich-text`)
//! - Supports version specifiers like `latest`, `1.0.0`, `^1.0.0`, etc.
//! - Extracts packages into a node_modules structure
//! - Bundles JavaScript applications using the downloaded packages
//!
//! # Output
//!
//! Each package is initially saved as a tarball in a subdirectory named
//! `{package_name}-{version}/package.tgz` under the target directory.
//! When extracted, packages are organized in a node_modules structure.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Error types for npm package fetching
#[derive(Debug)]
pub enum NpmFetchError {
    /// HTTP request failed
    HttpError(String),
    /// JSON parsing failed
    JsonError(String),
    /// File system operation failed
    IoError(String),
    /// Package not found
    PackageNotFound(String),
    /// Invalid package name or version
    InvalidPackage(String),
}

impl std::fmt::Display for NpmFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NpmFetchError::HttpError(msg) => write!(f, "HTTP error: {}", msg),
            NpmFetchError::JsonError(msg) => write!(f, "JSON error: {}", msg),
            NpmFetchError::IoError(msg) => write!(f, "IO error: {}", msg),
            NpmFetchError::PackageNotFound(pkg) => write!(f, "Package not found: {}", pkg),
            NpmFetchError::InvalidPackage(msg) => write!(f, "Invalid package: {}", msg),
        }
    }
}

impl std::error::Error for NpmFetchError {}

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

/// Fetches an NPM package and its dependencies to a target directory
pub struct NpmFetcher {
    /// Base URL for the NPM registry
    registry_url: String,
    /// Target directory to download packages to
    target_dir: PathBuf,
    /// Set of already fetched packages to avoid duplicates
    fetched: HashSet<String>,
}

impl NpmFetcher {
    /// Creates a new NPM fetcher with the default registry
    pub fn new<P: AsRef<Path>>(target_dir: P) -> Self {
        Self {
            registry_url: "https://registry.npmjs.org".to_string(),
            target_dir: target_dir.as_ref().to_path_buf(),
            fetched: HashSet::new(),
        }
    }

    /// Fetches a package and all its dependencies recursively
    ///
    /// # Arguments
    /// * `package_name` - Name of the package (e.g., "@lexical/rich-text")
    /// * `version_spec` - Version specifier (e.g., "latest", "1.0.0", "^1.0.0")
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(NpmFetchError)` otherwise
    pub fn fetch(&mut self, package_name: &str, version_spec: Option<&str>) -> Result<(), NpmFetchError> {
        let version_spec = version_spec.unwrap_or("latest");
        
        tracing::info!("Fetching package: {} @ {}", package_name, version_spec);
        
        self.fetch_recursive(package_name, version_spec)
    }

    fn fetch_recursive(&mut self, package_name: &str, version_spec: &str) -> Result<(), NpmFetchError> {
        // Fetch package metadata from registry
        let metadata = self.fetch_package_metadata(package_name)?;
        
        // Resolve version
        let version = self.resolve_version(&metadata, version_spec)?;
        
        // Check if we've already fetched this package at this exact version
        let package_key = format!("{}@{}", package_name, version);
        if self.fetched.contains(&package_key) {
            tracing::debug!("Package already fetched: {}", package_key);
            return Ok(());
        }
        
        // Get version metadata
        let version_metadata = metadata.versions.get(&version)
            .ok_or_else(|| NpmFetchError::PackageNotFound(
                format!("{} @ {}", package_name, version)
            ))?;

        // Download the tarball
        self.download_tarball(package_name, &version, &version_metadata.dist.tarball)?;

        // Mark as fetched
        self.fetched.insert(package_key);

        // Fetch dependencies recursively
        if let Some(dependencies) = &version_metadata.dependencies {
            for (dep_name, dep_version) in dependencies {
                // Skip optional dependencies and handle version ranges
                let cleaned_version = self.clean_version_spec(dep_version);
                
                match self.fetch_recursive(dep_name, &cleaned_version) {
                    Ok(_) => {},
                    Err(e) => {
                        tracing::warn!("Failed to fetch dependency {} @ {}: {}", dep_name, cleaned_version, e);
                        // Continue with other dependencies even if one fails
                    }
                }
            }
        }

        Ok(())
    }

    fn fetch_package_metadata(&self, package_name: &str) -> Result<NpmPackageMetadata, NpmFetchError> {
        // Encode package name for URL (handle scoped packages)
        let encoded_name = package_name.replace('/', "%2F");
        let url = format!("{}/{}", self.registry_url, encoded_name);

        tracing::debug!("Fetching metadata from: {}", url);

        let mut response = ureq::get(&url)
            .call()
            .map_err(|e| NpmFetchError::HttpError(format!("Failed to fetch {}: {}", url, e)))?;

        let body = response.body_mut().read_to_string()
            .map_err(|e| NpmFetchError::HttpError(format!("Failed to read response body: {}", e)))?;

        serde_json::from_str(&body)
            .map_err(|e| NpmFetchError::JsonError(format!("Failed to parse JSON for {}: {}", package_name, e)))
    }

    fn resolve_version(&self, metadata: &NpmPackageMetadata, version_spec: &str) -> Result<String, NpmFetchError> {
        // Handle "latest" tag
        if version_spec == "latest" {
            if let Some(latest) = metadata.dist_tags.get("latest") {
                return Ok(latest.clone());
            }
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
        metadata.dist_tags.get("latest")
            .cloned()
            .ok_or_else(|| NpmFetchError::InvalidPackage(
                format!("Could not resolve version {} for package", version_spec)
            ))
    }

    fn clean_version_spec(&self, version_spec: &str) -> String {
        // Remove common version prefixes
        let trimmed = version_spec.trim();
        if trimmed.starts_with(">=") || trimmed.starts_with("<=") {
            trimmed[2..].trim().to_string()
        } else if trimmed.starts_with('^') || trimmed.starts_with('~') 
                  || trimmed.starts_with('>') || trimmed.starts_with('<') 
                  || trimmed.starts_with('=') {
            trimmed[1..].trim().to_string()
        } else {
            trimmed.to_string()
        }
    }

    fn download_tarball(&self, package_name: &str, version: &str, tarball_url: &str) -> Result<(), NpmFetchError> {
        tracing::info!("Downloading {} @ {} from {}", package_name, version, tarball_url);

        // Create package directory
        // Replace '@' and '/' to create safe filesystem names
        // Using 'at_' for '@' preserves the scoped package indicator
        let safe_package_name = package_name
            .replace('@', "at_")
            .replace('/', "_");
        let package_dir = self.target_dir.join(format!("{}-{}", safe_package_name, version));
        
        fs::create_dir_all(&package_dir)
            .map_err(|e| NpmFetchError::IoError(format!("Failed to create directory {}: {}", package_dir.display(), e)))?;

        // Download tarball
        let mut response = ureq::get(tarball_url)
            .call()
            .map_err(|e| NpmFetchError::HttpError(format!("Failed to download {}: {}", tarball_url, e)))?;

        // Save tarball to file
        let tarball_path = package_dir.join("package.tgz");
        let mut file = fs::File::create(&tarball_path)
            .map_err(|e| NpmFetchError::IoError(format!("Failed to create file {}: {}", tarball_path.display(), e)))?;

        // Use as_reader() to get a reader from the body
        let mut reader = response.body_mut().as_reader();
        std::io::copy(&mut reader, &mut file)
            .map_err(|e| NpmFetchError::IoError(format!("Failed to write tarball: {}", e)))?;

        tracing::info!("Saved {} @ {} to {}", package_name, version, tarball_path.display());

        Ok(())
    }

    /// Extracts all downloaded packages into a node_modules-like structure
    ///
    /// # Arguments
    /// * `output_dir` - Directory where the node_modules structure should be created
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(NpmFetchError)` otherwise
    pub fn extract_packages<P: AsRef<Path>>(&self, output_dir: P) -> Result<(), NpmFetchError> {
        let output_dir = output_dir.as_ref();
        let node_modules_dir = output_dir.join("node_modules");
        
        tracing::info!("Extracting packages to {}", node_modules_dir.display());

        // Iterate through all subdirectories in target_dir
        let entries = fs::read_dir(&self.target_dir)
            .map_err(|e| NpmFetchError::IoError(format!("Failed to read target directory: {}", e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| NpmFetchError::IoError(format!("Failed to read directory entry: {}", e)))?;
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

    fn extract_tarball(&self, tarball_path: &Path, node_modules_dir: &Path) -> Result<(), NpmFetchError> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        tracing::debug!("Extracting {}", tarball_path.display());

        let file = fs::File::open(tarball_path)
            .map_err(|e| NpmFetchError::IoError(format!("Failed to open tarball: {}", e)))?;

        let decompressor = GzDecoder::new(file);
        let mut archive = Archive::new(decompressor);

        // Extract to a temporary location first
        let temp_extract = node_modules_dir.join(".temp_extract");
        fs::create_dir_all(&temp_extract)
            .map_err(|e| NpmFetchError::IoError(format!("Failed to create temp directory: {}", e)))?;

        archive.unpack(&temp_extract)
            .map_err(|e| NpmFetchError::IoError(format!("Failed to extract tarball: {}", e)))?;

        // NPM tarballs contain a 'package' directory, move its contents to node_modules
        let package_dir = temp_extract.join("package");
        if package_dir.exists() {
            // Read package.json to get the real package name
            let package_json_path = package_dir.join("package.json");
            if package_json_path.exists() {
                let package_json_content = fs::read_to_string(&package_json_path)
                    .map_err(|e| NpmFetchError::IoError(format!("Failed to read package.json: {}", e)))?;
                
                let package_info: serde_json::Value = serde_json::from_str(&package_json_content)
                    .map_err(|e| NpmFetchError::JsonError(format!("Failed to parse package.json: {}", e)))?;

                if let Some(name) = package_info.get("name").and_then(|n| n.as_str()) {
                    // Handle scoped packages
                    let target_path = if name.starts_with('@') {
                        // For @scope/package, create @scope directory first
                        if let Some(slash_pos) = name.find('/') {
                            let scope = &name[..slash_pos];
                            let pkg_name = &name[slash_pos + 1..];
                            let scope_dir = node_modules_dir.join(scope);
                            fs::create_dir_all(&scope_dir)
                                .map_err(|e| NpmFetchError::IoError(format!("Failed to create scope directory: {}", e)))?;
                            scope_dir.join(pkg_name)
                        } else {
                            node_modules_dir.join(name)
                        }
                    } else {
                        node_modules_dir.join(name)
                    };

                    // Remove existing package if it exists
                    if target_path.exists() {
                        fs::remove_dir_all(&target_path)
                            .map_err(|e| NpmFetchError::IoError(format!("Failed to remove existing package: {}", e)))?;
                    }

                    // Move the package to node_modules
                    fs::rename(&package_dir, &target_path)
                        .map_err(|e| NpmFetchError::IoError(format!("Failed to move package to node_modules: {}", e)))?;

                    tracing::debug!("Extracted {} to {}", name, target_path.display());
                }
            }
        }

        // Clean up temp directory
        let _ = fs::remove_dir_all(&temp_extract);

        Ok(())
    }

    /// Bundles a JavaScript entry point that uses the downloaded NPM packages
    ///
    /// # Arguments
    /// * `entry_script` - Path to the JavaScript entry point file
    /// * `output_dir` - Directory where the node_modules structure was created
    ///
    /// # Returns
    /// `Ok(String)` containing the bundled JavaScript code, or `Err(NpmFetchError)`
    pub fn bundle_with_packages<P: AsRef<Path>>(&self, entry_script: P, output_dir: P) -> Result<String, NpmFetchError> {
        use crate::proc::js_bundle::JsBundleProcessor;
        use crate::proc::{Asset, ProcessesAssets};

        let entry_script = entry_script.as_ref();
        let output_dir = output_dir.as_ref();

        // First, ensure packages are extracted
        self.extract_packages(output_dir)?;

        // Create an asset for the entry script
        let entry_content = fs::read(entry_script)
            .map_err(|e| NpmFetchError::IoError(format!("Failed to read entry script: {}", e)))?;

        let mut asset = Asset::new(
            entry_script.to_string_lossy().to_string().into(),
            entry_content
        );

        // Use the JS bundle processor
        let processor = JsBundleProcessor::new();
        processor.process(&mut asset)
            .map_err(|e| NpmFetchError::InvalidPackage(format!("Bundling failed: {:?}", e)))?;

        // Get the bundled code
        let bundled_code = asset.as_text()
            .map_err(|e| NpmFetchError::InvalidPackage(format!("Failed to get bundled code: {:?}", e)))?;

        Ok(bundled_code.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_version_spec() {
        let fetcher = NpmFetcher::new("/tmp");
        
        assert_eq!(fetcher.clean_version_spec("^1.0.0"), "1.0.0");
        assert_eq!(fetcher.clean_version_spec("~1.2.3"), "1.2.3");
        assert_eq!(fetcher.clean_version_spec(">=2.0.0"), "2.0.0");
        assert_eq!(fetcher.clean_version_spec("1.0.0"), "1.0.0");
    }

    #[test]
    fn test_npm_fetcher_creation() {
        let temp_dir = std::env::temp_dir().join("test_npm_fetch");
        let fetcher = NpmFetcher::new(&temp_dir);
        
        assert_eq!(fetcher.target_dir, temp_dir);
        assert_eq!(fetcher.registry_url, "https://registry.npmjs.org");
        assert!(fetcher.fetched.is_empty());
    }

    /// This test requires network access and is ignored by default.
    /// Run with `cargo test -- --ignored` to execute.
    #[test]
    #[ignore]
    fn test_fetch_small_package() {
        let temp_dir = std::env::temp_dir().join("test_npm_fetch_integration");
        
        // Clean up if it exists
        let _ = std::fs::remove_dir_all(&temp_dir);
        
        let mut fetcher = NpmFetcher::new(&temp_dir);
        
        // Fetch a small, stable package (is-odd is a tiny package with minimal dependencies)
        let result = fetcher.fetch("is-odd", Some("latest"));
        
        assert!(result.is_ok(), "Failed to fetch package: {:?}", result.err());
        
        // Verify at least one directory was created
        let entries = std::fs::read_dir(&temp_dir).expect("Failed to read temp dir");
        let count = entries.count();
        assert!(count > 0, "No packages were downloaded");
        
        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
