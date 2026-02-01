use std::path::Path;

use grass::{Options, from_string};

use crate::tool::procs::ASSET_SOURCE_ROOT_CONTEXT_KEY;

use super::{Asset, Context, ContextValue, MediaType, ProcessesAssets, ProcessingError};

impl From<Box<grass::Error>> for ProcessingError {
    fn from(error: Box<grass::Error>) -> Self {
        ProcessingError::Compilation {
            message: error.to_string().into(),
        }
    }
}

pub struct ScssProcessor {}

impl ProcessesAssets for ScssProcessor {
    fn process(&self, context: &mut Context, asset: &mut Asset) -> Result<(), ProcessingError> {
        if *asset.media_type() != MediaType::Scss {
            return Ok(());
        }

        tracing::trace!("scss: {}", asset.path());

        // Build load paths for import resolution, using the asset's
        // path relative to the source root as the load path.
        let mut options = Options::default();
        if let Some(ContextValue::Text(source)) = context.get(&ASSET_SOURCE_ROOT_CONTEXT_KEY.into())
        {
            let full_path = Path::new(source.as_str()).join(asset.path().as_str());
            if let Some(parent) = full_path.parent() {
                options = options.load_path(parent);
            }
        }

        // Compile SCSS content to CSS.
        let css = from_string(asset.as_text()?.to_string(), &options)?;

        // Update the asset's contents and media type.
        asset.replace_with_text(css.into(), MediaType::Css);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            .process(&mut Context::default(), &mut asset)
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
            .process(&mut Context::default(), &mut asset)
            .unwrap();

        assert_eq!(
            "nav ul {\n  margin: 0;\n  padding: 0;\n  list-style: none;\n}\nnav li {\n  display: inline-block;\n}\nnav a {\n  display: block;\n  padding: 6px 12px;\n  text-decoration: none;\n}\n",
            asset.as_text().unwrap()
        );
    }
}
