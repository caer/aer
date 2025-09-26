use codas::types::Text;

use crate::process::{AssetError, media_type::MediaType};

/// An asset meant to be processed by anything that [ProcessesAssets].
#[derive(Clone, Debug)]
pub struct Asset {
    /// The asset's logical path, including the asset's name.
    path: Text,

    /// The asset's _current_ media type.
    media_type: MediaType,

    /// The asset's raw contents
    contents: Option<AssetContents>,
}

impl Asset {
    /// Returns a new asset with `path` and `contents`.
    pub fn new(path: Text, contents: Vec<u8>) -> Self {
        let contents = if contents.is_empty() {
            None
        } else {
            // Try to convert the vector to UTF-8 bytes.
            Some(match String::from_utf8(contents) {
                Ok(text) => AssetContents::Textual(text.into()),
                Err(e) => AssetContents::Binary(e.into_bytes()),
            })
        };

        // Extract the media type from the path.
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

    /// Returns the asset's media type.
    pub fn media_type(&self) -> &MediaType {
        &self.media_type
    }

    /// Sets the asset's media type.
    pub fn set_media_type(&mut self, media_type: MediaType) {
        self.media_type = media_type;
    }

    /// Replaces the assets contents with `bytes` and `media_type`.
    pub fn replace_with_bytes(&mut self, bytes: Vec<u8>, media_type: MediaType) {
        self.contents = Some(AssetContents::Binary(bytes));
        self.media_type = media_type;
    }

    /// Replaces the assets contents with `text` and `media_type`.
    pub fn replace_with_text(&mut self, text: Text, media_type: MediaType) {
        self.contents = Some(AssetContents::Textual(text));
        self.media_type = media_type;
    }

    /// Returns the asset's contents as immutable bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self.contents.as_ref() {
            Some(AssetContents::Binary(bytes)) => bytes,
            Some(AssetContents::Textual(text)) => text.as_bytes(),
            None => &[],
        }
    }

    /// Returns the assets contents as immutable text.
    ///
    /// If the asset is empty or contains non-textual data,
    /// this function will fail.
    pub fn as_text(&self) -> Result<&Text, AssetError> {
        match self.contents.as_ref() {
            Some(AssetContents::Textual(text)) => Ok(text),
            _ => Err(AssetError::NonTextual),
        }
    }

    /// Returns the asset's contents as mutable bytes.
    ///
    /// If the asset is empty, this function will fail.
    ///
    /// If the asset contains text, this function will fail:
    /// All assets can be _represented_ [as bytes](Self::as_bytes),
    /// but it would be unsafe to modify a textual asset's bytes
    /// in place, since the resulting bytes may no longer
    /// represent valid text.
    pub fn as_mut_bytes(&mut self) -> Result<&mut Vec<u8>, AssetError> {
        match &mut self.contents {
            Some(AssetContents::Binary(bytes)) => Ok(bytes),
            _ => Err(AssetError::NonBinary),
        }
    }
}

/// Raw contents of an [Asset].
#[derive(Clone, Debug)]
enum AssetContents {
    Binary(Vec<u8>),
    Textual(Text),
}
