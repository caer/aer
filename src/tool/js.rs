//! JavaScript module downloading (via NPM) and bundling.
//!
//! # Basic Usage
//!
//! ```no_run
//! # use aer::tool::js::JsModuleManager;
//!
//! // Download modules
//! let mut manager = JsModuleManager::new("./packages");
//! manager.fetch("@tiptap/core", Some("latest")).unwrap();
//!
//! // Extract modules
//! manager.extract_modules("./modules").unwrap();
//!
//! // Bundle modules
//! let bundled_code = manager.bundle(
//!     "./src/app.js",  // Your entry point
//!     "./modules"
//! ).unwrap();
//!
//! // Save the bundled output
//! std::fs::write("./modules/bundle.js", bundled_code).unwrap();
//! ```
//!
//! This creates a `./modules/node_modules/` directory with all downloaded modules.
//!
//! # Directory Structure
//!
//! ## Download Cache
//!
//! Each module is initially cached as a tarball:
//!
//! ```text
//! ./packages/
//!   └── node_modules/
//!       ├── @lexical/
//!       │   └── rich-text/
//!       │       └── 0.17.1.tgz
//!       └── react/
//!           └── 18.2.0.tgz
//! ```
//!
//! ## Extracted Modules
//!
//! After extraction, modules follow the standard Node.js structure:
//!
//! ```text
//! ./modules/
//!   └── node_modules/
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

    /// Returns the tarball path inside the cache using a node_modules-like layout
    fn module_tarball_path(
        &self,
        module_name: &str,
        version: &str,
    ) -> Result<PathBuf, JsModuleError> {
        let mut path = self.cache_dir.join("node_modules");
        let mut saw_component = false;

        for part in module_name.split('/') {
            if part.is_empty() || part == "." || part == ".." {
                return Err(JsModuleError::InvalidModule(format!(
                    "Invalid module component: {}",
                    module_name
                )));
            }
            saw_component = true;
            path = path.join(part);
        }

        if !saw_component {
            return Err(JsModuleError::InvalidModule(
                "Module name must not be empty".to_string(),
            ));
        }

        Ok(path.join(format!("{}.tgz", version)))
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

        let tarball_path = self.module_tarball_path(module_name, version)?;
        let module_dir = tarball_path
            .parent()
            .ok_or_else(|| {
                JsModuleError::InvalidModule(format!(
                    "Invalid module path for {} @ {}",
                    module_name, version
                ))
            })?
            .to_path_buf();

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

        fs::create_dir_all(&node_modules_dir).map_err(|e| {
            JsModuleError::IoError(format!("Failed to create node_modules directory: {}", e))
        })?;

        tracing::info!("Extracting modules to {}", node_modules_dir.display());

        let mut tarballs = Vec::new();
        self.collect_tarballs(&self.cache_dir, &mut tarballs)?;
        tarballs.sort();

        for tarball_path in tarballs {
            self.extract_tarball(&tarball_path, &node_modules_dir)?;
        }

        tracing::info!("Extraction complete");
        Ok(())
    }

    fn collect_tarballs(
        &self,
        dir: &Path,
        tarballs: &mut Vec<PathBuf>,
    ) -> Result<(), JsModuleError> {
        for entry in fs::read_dir(dir)
            .map_err(|e| JsModuleError::IoError(format!("Failed to read cache directory: {}", e)))?
        {
            let entry = entry.map_err(|e| {
                JsModuleError::IoError(format!("Failed to read directory entry: {}", e))
            })?;
            let path = entry.path();

            if path.is_dir() {
                self.collect_tarballs(&path, tarballs)?;
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("tgz"))
                .unwrap_or(false)
            {
                tarballs.push(path);
            }
        }

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

        let target_path = self.target_path_from_tarball(tarball_path, node_modules_dir)?;

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
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    JsModuleError::IoError(format!(
                        "Failed to create module parent directory: {}",
                        e
                    ))
                })?;
            }

            if target_path.exists() {
                fs::remove_dir_all(&target_path).map_err(|e| {
                    JsModuleError::IoError(format!(
                        "Failed to remove existing module: {}",
                        e
                    ))
                })?;
            }

            fs::rename(&package_dir, &target_path).map_err(|e| {
                JsModuleError::IoError(format!("Failed to move module to node_modules: {}", e))
            })?;

            tracing::debug!("Extracted to {}", target_path.display());
        }

        // Clean up temp directory
        if let Err(e) = fs::remove_dir_all(&temp_extract) {
            tracing::warn!("Failed to clean up temporary extraction directory: {}", e);
        }

        Ok(())
    }

    fn target_path_from_tarball(
        &self,
        tarball_path: &Path,
        node_modules_dir: &Path,
    ) -> Result<PathBuf, JsModuleError> {
        let cache_node_modules = self.cache_dir.join("node_modules");

        if !tarball_path.starts_with(&cache_node_modules) {
            return Err(JsModuleError::InvalidModule(format!(
                "Tarball not in cache: {}",
                tarball_path.display()
            )));
        }

        if tarball_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("tgz"))
            .unwrap_or(false)
            == false
        {
            return Err(JsModuleError::InvalidModule(format!(
                "Unexpected tarball extension for {}",
                tarball_path.display()
            )));
        }

        let module_dir = tarball_path.parent().ok_or_else(|| {
            JsModuleError::InvalidModule(format!(
                "Invalid tarball path: {}",
                tarball_path.display()
            ))
        })?;

        let relative = module_dir.strip_prefix(&cache_node_modules).map_err(|_| {
            JsModuleError::InvalidModule(format!(
                "Tarball path outside cache: {}",
                tarball_path.display()
            ))
        })?;

        if relative.components().next().is_none() {
            return Err(JsModuleError::InvalidModule(
                "Module path cannot be empty".to_string(),
            ));
        }

        Ok(node_modules_dir.join(relative))
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
