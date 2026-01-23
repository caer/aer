use std::path::Path;

use grass::{Options, from_path};

use super::{Asset, Context, MediaType, ProcessesAssets, ProcessingError};

impl From<Box<grass::Error>> for ProcessingError {
    fn from(error: Box<grass::Error>) -> Self {
        ProcessingError::Compilation {
            message: error.to_string().into(),
        }
    }
}
pub struct ScssProcessor {}

impl ProcessesAssets for ScssProcessor {
    fn process(&self, _context: &mut Context, asset: &mut Asset) -> Result<(), ProcessingError> {
        if *asset.media_type() != MediaType::Scss {
            tracing::debug!(
                "skipping asset {}: not SCSS {}",
                asset.path(),
                asset.media_type().name()
            );
            return Ok(());
        }

        // Get Path Ref
        let path_text = asset.path().clone();
        let path: &str = path_text.as_ref();

        // Compile SCSS file at selected path to CSS
        let css = from_path(Path::new(path), &Options::default())?;

        // Update the asset's contents and target extension.
        asset.replace_with_text(css.into(), MediaType::Scss);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_scss() {
        let mut simple_scss_asset =
            Asset::new("test/simple_example.scss".into(), "".as_bytes().to_vec());

        let _ = ScssProcessor {}.process(&mut Context::default(), &mut simple_scss_asset);

        assert_eq!(
            "body {\n  font: 100% Helvetica, sans-serif;\n  color: #333;\n}\n",
            simple_scss_asset.as_text().unwrap()
        );

        let mut simple_nested_scss_asset = Asset::new(
            "test/simple_nested_example.scss".into(),
            "".as_bytes().to_vec(),
        );

        let _ = ScssProcessor {}.process(&mut Context::default(), &mut simple_nested_scss_asset);

        assert_eq!(
            "nav ul {\n  margin: 0;\n  padding: 0;\n  list-style: none;\n}\nnav li {\n  display: inline-block;\n}\nnav a {\n  display: block;\n  padding: 6px 12px;\n  text-decoration: none;\n}\n",
            simple_nested_scss_asset.as_text().unwrap()
        );
    }
}
