use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use grass::{Options, from_string};

use super::{Asset, Context, Environment, MediaType, ProcessesAssets, ProcessingError};

impl From<Box<grass::Error>> for ProcessingError {
    fn from(error: Box<grass::Error>) -> Self {
        ProcessingError::Compilation {
            message: error.to_string().into(),
        }
    }
}

/// Virtual load path prefix used for kit imports.
/// Grass will prepend this to `@use "kit-name/module"`, then
/// [`KitFs`] remaps the resulting path to the real kit directory.
const KITS_VIRTUAL_ROOT: &str = "/__aer_kits__";

pub struct ScssProcessor {}

impl ProcessesAssets for ScssProcessor {
    fn process(
        &self,
        env: &Environment,
        _context: &mut Context,
        asset: &mut Asset,
    ) -> Result<(), ProcessingError> {
        if *asset.media_type() != MediaType::Scss {
            return Ok(());
        }

        tracing::trace!("scss: {}", asset.path());

        // Build load paths for import resolution, using the asset's
        // path relative to the source root as the load path.
        let full_path = env.source_root.join(asset.path().as_str());

        let kit_fs = KitFs {
            kits: env.kit_imports.clone(),
        };

        let mut options = Options::default().fs(&kit_fs);
        if let Some(parent) = full_path.parent() {
            options = options.load_path(parent);
        }

        // Add virtual kit load path so @use "kit-name/module" resolves.
        if !env.kit_imports.is_empty() {
            options = options.load_path(KITS_VIRTUAL_ROOT);
        }

        // Compile SCSS content to CSS.
        let css = from_string(asset.as_text()?.to_string(), &options)?;

        // Update the asset's contents and media type.
        asset.replace_with_text(css.into(), MediaType::Css);

        Ok(())
    }
}

/// A virtual filesystem that remaps `{KITS_VIRTUAL_ROOT}/{kit-name}/…`
/// to the kit's actual asset directory, delegating everything else to
/// the real filesystem.
#[derive(Debug)]
struct KitFs {
    kits: BTreeMap<String, PathBuf>,
}

impl KitFs {
    /// Maps a virtual kit path to its real location on disk.
    fn remap(&self, path: &Path) -> Option<PathBuf> {
        let suffix = path.to_string_lossy();
        let suffix = suffix.strip_prefix(KITS_VIRTUAL_ROOT)?.strip_prefix('/')?;
        if let Some((kit_name, rest)) = suffix.split_once('/') {
            let kit_dir = self.kits.get(kit_name)?;
            Some(kit_dir.join(rest))
        } else {
            // Just the kit name, e.g. "/__aer_kits__/my-kit"
            self.kits.get(suffix).cloned()
        }
    }
}

impl grass::Fs for KitFs {
    fn is_dir(&self, path: &Path) -> bool {
        if path == Path::new(KITS_VIRTUAL_ROOT) {
            return !self.kits.is_empty();
        }
        if let Some(real) = self.remap(path) {
            real.is_dir()
        } else {
            path.is_dir()
        }
    }

    fn is_file(&self, path: &Path) -> bool {
        if let Some(real) = self.remap(path) {
            real.is_file()
        } else {
            path.is_file()
        }
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        if let Some(real) = self.remap(path) {
            std::fs::read(&real)
        } else {
            std::fs::read(path)
        }
    }

    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        if let Some(real) = self.remap(path) {
            std::fs::canonicalize(&real)
        } else {
            std::fs::canonicalize(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn test_env() -> Environment {
        Environment {
            source_root: PathBuf::from("."),
            kit_imports: BTreeMap::new(),
        }
    }

    #[test]
    fn processes_scss() {
        let scss = r#"
$font-stack: Helvetica, sans-serif;
$primary-color: #333;

body {
  font: 100% $font-stack;
  color: $primary-color;
}
"#;
        let mut asset = Asset::new("styles.scss".into(), scss.as_bytes().to_vec());
        ScssProcessor {}
            .process(&test_env(), &mut Context::default(), &mut asset)
            .unwrap();

        assert_eq!(
            "body {\n  font: 100% Helvetica, sans-serif;\n  color: #333;\n}\n",
            asset.as_text().unwrap()
        );
    }

    #[test]
    fn processes_nested_scss() {
        let scss = r#"
nav {
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li { display: inline-block; }
  a {
    display: block;
    padding: 6px 12px;
    text-decoration: none;
  }
}
"#;
        let mut asset = Asset::new("nav.scss".into(), scss.as_bytes().to_vec());
        ScssProcessor {}
            .process(&test_env(), &mut Context::default(), &mut asset)
            .unwrap();

        assert_eq!(
            "nav ul {\n  margin: 0;\n  padding: 0;\n  list-style: none;\n}\nnav li {\n  display: inline-block;\n}\nnav a {\n  display: block;\n  padding: 6px 12px;\n  text-decoration: none;\n}\n",
            asset.as_text().unwrap()
        );
    }
}
