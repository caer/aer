use std::path::Path;

use grass::{Options, from_path};

use crate::asset::{ProcessesAssets, media_type::MediaType};

pub struct ScssProcessor {}

impl ProcessesAssets for ScssProcessor {
    fn process(&self, asset: &mut super::Asset) {
        // Get Path Ref
        let path_text = asset.path().clone();
        let path: &str = path_text.as_ref();

        // Compile SCSS file at selected path to CSS
        let css = from_path(Path::new(path), &Options::default()).unwrap();

        // Update the asset's contents and target extension.
        let text = asset.contents.try_as_mut_text().expect("todo");
        *text.to_mut() = css;
        asset.media_type = MediaType::Css;
    }
}

#[cfg(test)]
mod tests {
    use crate::asset::Asset;

    use super::*;

    #[test]
    fn processes_scss() {
        let mut scss_asset = Asset::new("src/test/test.scss".into(), "".as_bytes().to_vec());

        ScssProcessor {}.process(&mut scss_asset);

        assert_eq!(
            "body {\n  font: 100% Helvetica, sans-serif;\n  color: #333;\n}\n",
            scss_asset.contents.try_as_mut_text().unwrap()
        );
    }
}
