use codas::types::Text;

mod media_type;
pub use media_type::{MediaCategory, MediaType};

use crate::proc::ProcessingError;

/// An in-memory representation of any asset meant for processing.
#[derive(Clone, Debug)]
pub struct Asset {
    /// The asset's logical path, including the asset's name
    /// and _original_ file extension (if any).
    ///
    /// This path is assumed to be relative to a common root
    /// directory for all assets being processed.
    path: Text,
    content: Option<AssetContent>,
    content_media_type: MediaType,
}

impl Asset {
    /// Returns a new asset with `path` and `content`.
    pub fn new(path: Text, content: Vec<u8>) -> Self {
        let contents = if content.is_empty() {
            None
        } else {
            // Try to convert the vector to UTF-8 bytes.
            Some(match String::from_utf8(content) {
                Ok(text) => AssetContent::Textual(text.into()),
                Err(e) => AssetContent::Binary(e.into_bytes()),
            })
        };

        // Extract the media type from the path.
        let media_type = MediaType::from_extension(path.split('.').next_back().unwrap_or_default());

        Self {
            path,
            content_media_type: media_type,
            content: contents,
        }
    }

    /// Returns the asset's logical path, including its name.
    pub fn path(&self) -> &Text {
        &self.path
    }

    /// Returns the asset's media type.
    pub fn media_type(&self) -> &MediaType {
        &self.content_media_type
    }

    /// Sets the asset's media type.
    pub fn set_media_type(&mut self, media_type: MediaType) {
        self.content_media_type = media_type;
    }

    /// Replaces the assets content with `bytes` and `media_type`.
    pub fn replace_with_bytes(&mut self, bytes: Vec<u8>, media_type: MediaType) {
        self.content = Some(AssetContent::Binary(bytes));
        self.content_media_type = media_type;
    }

    /// Replaces the assets content with `text` and `media_type`.
    pub fn replace_with_text(&mut self, text: Text, media_type: MediaType) {
        self.content = Some(AssetContent::Textual(text));
        self.content_media_type = media_type;
    }

    /// Returns the asset's content as immutable bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self.content.as_ref() {
            Some(AssetContent::Binary(bytes)) => bytes,
            Some(AssetContent::Textual(text)) => text.as_bytes(),
            None => &[],
        }
    }

    /// Returns the assets content as immutable text.
    ///
    /// If the asset is empty or contains non-textual data,
    /// this function will fail.
    pub fn as_text(&self) -> Result<&Text, ProcessingError> {
        match self.content.as_ref() {
            Some(AssetContent::Textual(text)) => Ok(text),
            _ => Err(ProcessingError::NonTextual),
        }
    }

    /// Returns the asset's content as mutable bytes.
    ///
    /// If the asset is empty, this function will fail.
    ///
    /// If the asset contains text, this function will fail:
    /// All assets can be _represented_ [as bytes](Self::as_bytes),
    /// but it would be unsafe to modify a textual asset's bytes
    /// in place, since the resulting bytes may no longer
    /// represent valid text.
    pub fn as_mut_bytes(&mut self) -> Result<&mut Vec<u8>, ProcessingError> {
        match &mut self.content {
            Some(AssetContent::Binary(bytes)) => Ok(bytes),
            _ => Err(ProcessingError::NonBinary),
        }
    }
}

/// Raw content of an [Asset].
#[derive(Clone, Debug)]
enum AssetContent {
    Binary(Vec<u8>),
    Textual(Text),
}

#[cfg(test)]
mod tests {
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
        assert_eq!(Err(ProcessingError::NonTextual), binary_asset.as_text());
    }
}
