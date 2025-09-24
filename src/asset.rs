//! This module contains things that [ProcessesAssets],
//! like SCSS compilers, Markdown transpilers, and image
//! minifiers.
use codas::types::Text;

use crate::asset::media_type::MediaType;

pub mod markdown;
pub mod scss;
pub mod media_type;

/// An asset meant to be processed by anything that [ProcessesAssets].
#[derive(Clone, Debug)]
pub struct Asset {
    /// The asset's logical path, including the asset's name.
    path: Text,

    /// The asset's _current_ media type.
    ///
    /// When the asset is exported as a file, its extension
    /// will match this media type's preferred extension.
    pub media_type: MediaType,

    /// The asset's raw contents
    pub contents: AssetContents,
}

impl Asset {
    /// Returns a new asset at `path` with `contents`.
    pub fn new(path: Text, contents: Vec<u8>) -> Self {
        // Try to convert the vector to UTF-8 bytes.
        let contents = if let Ok(text) = str::from_utf8(&contents) {
            AssetContents::Text(text.into())
        } else {
            AssetContents::Binary(contents)
        };

        // Extract the media type fro the path.
        let media_type = MediaType::from_extension(path.split('.').next_back().unwrap_or_default());

        Self {
            path,
            media_type,
            contents,
        }
    }

    /// Returns the asset's logical path, including its name.
    pub fn path(&self) -> &Text {
        &self.path
    }
}

/// Raw contents of an [Asset].
#[derive(Clone, Debug)]
pub enum AssetContents {
    Binary(Vec<u8>),
    Text(Text),
}

impl AssetContents {
    /// Returns the contents as raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match &self {
            AssetContents::Binary(data) => data,
            AssetContents::Text(text) => text.as_bytes(),
        }
    }

    /// Returns the contents as mutable text.
    pub fn try_as_mut_text(&mut self) -> Result<&mut Text, Error> {
        match self {
            AssetContents::Text(text) => Ok(text),
            _ => Err(Error::NotText),
        }
    }
}

/// A thing that processes [Asset]s.
pub trait ProcessesAssets {
    /// Processes `asset`.
    fn process(&self, asset: &mut Asset);
}

#[derive(Debug)]
pub enum Error {
    /// An asset contained data that wasn't text.
    NotText,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_assets() {
        let mut markdown_asset = Asset::new("story.md".into(), "Hello, world!".as_bytes().to_vec());
        assert_eq!("story.md", markdown_asset.path());
        assert_eq!(MediaType::Markdown, markdown_asset.media_type);
        assert_eq!(
            "Hello, world!".as_bytes(),
            markdown_asset.contents.as_bytes()
        );
        assert_eq!(
            "Hello, world!",
            markdown_asset.contents.try_as_mut_text().unwrap()
        );

        let binary_asset = Asset::new("data.dat".into(), (-1337i16).to_le_bytes().to_vec());
        assert_eq!("data.dat", binary_asset.path());
        assert_eq!(
            MediaType::Unknown {
                extension: ["dat".into()]
            },
            binary_asset.media_type
        );
        assert_eq!(
            &(-1337i16).to_le_bytes().to_vec(),
            binary_asset.contents.as_bytes()
        );
    }
}
