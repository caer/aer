use codas::types::Text;

use crate::proc::asset::Asset;

pub mod asset;
pub mod image;
pub mod markdown;
pub mod scss;

/// A thing that processes [Asset]s.
pub trait ProcessesAssets {
    /// Processes `asset`.
    fn process(&self, asset: &mut Asset) -> Result<(), AssetError>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum AssetError {
    /// An asset contained data that wasn't text.
    NonTextual,

    /// An asset contained data that wasn't binary.
    NonBinary,

    /// An asset contained data that was malformed.
    Malformed { message: Text },

    /// An error occurred while compiling an asset
    /// via a processor.
    Compilation { message: Text },
}

#[cfg(test)]
mod tests {
    use asset::MediaType;

    use super::*;

    #[test]
    fn creates_assets() {
        let markdown_asset = Asset::new("story.md".into(), "Hello, world!".as_bytes().to_vec());
        assert_eq!("story.md", markdown_asset.path());
        assert_eq!(&MediaType::Markdown, markdown_asset.media_type());
        assert_eq!(b"Hello, world!", markdown_asset.as_bytes());
        assert_eq!("Hello, world!", markdown_asset.as_text().unwrap());

        let binary_asset = Asset::new("data.dat".into(), (-1337i16).to_le_bytes().to_vec());
        assert_eq!("data.dat", binary_asset.path());
        assert_eq!(
            &MediaType::Unknown {
                extension: ["dat".into()]
            },
            binary_asset.media_type()
        );
        assert_eq!(&(-1337i16).to_le_bytes().to_vec(), binary_asset.as_bytes(),);
        assert_eq!(Err(AssetError::NonTextual), binary_asset.as_text());
    }
}
