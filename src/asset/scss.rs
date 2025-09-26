use std::path::Path;

use grass::{Options, from_path};

use crate::asset::{AssetError, ProcessesAssets, media_type::MediaType};

impl From<Box<grass::Error>> for AssetError {
    fn from(error: Box<grass::Error>) -> Self {
        AssetError::Compilation {
            message: error.to_string().into(),
        }
    }
}
pub struct ScssProcessor {}

impl ProcessesAssets for ScssProcessor {
    fn process(&self, asset: &mut super::Asset) -> Result<(), AssetError> {
        // Get Path Ref
        let path_text = asset.path().clone();
        let path: &str = path_text.as_ref();

        // Compile SCSS file at selected path to CSS
        let css = from_path(Path::new(path), &Options::default())?;

        // Update the asset's contents and target extension.
        let text = asset.contents.try_as_mut_text()?;
        *text.to_mut() = css;
        asset.media_type = MediaType::Css;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::asset::Asset;

    use super::*;

    #[test]
    fn processes_scss() {
        let mut simple_scss_asset =
            Asset::new("test/simple_example.scss".into(), "".as_bytes().to_vec());

        let _ = ScssProcessor {}.process(&mut simple_scss_asset);

        assert_eq!(
            "body {\n  font: 100% Helvetica, sans-serif;\n  color: #333;\n}\n",
            simple_scss_asset.contents.try_as_mut_text().unwrap()
        );

        let mut simple_nested_scss_asset = Asset::new(
            "test/simple_nested_example.scss".into(),
            "".as_bytes().to_vec(),
        );

        let _ = ScssProcessor {}.process(&mut simple_nested_scss_asset);

        assert_eq!(
            "nav ul {\n  margin: 0;\n  padding: 0;\n  list-style: none;\n}\nnav li {\n  display: inline-block;\n}\nnav a {\n  display: block;\n  padding: 6px 12px;\n  text-decoration: none;\n}\n",
            simple_nested_scss_asset.contents.try_as_mut_text().unwrap()
        );
    }
}
