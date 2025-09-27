use codas::types::Text;

use super::AssetError;

// Definitions for all media types explicitly supported by this
// crate, in alphabetical order by their "logical" names
// (e.g., "Css" comes before "Markdown").
//
// Each media type is a tuple of `(name, mime_type, [extensions])`.
// Extensions should be ordered, roughly, in terms of how common the
// extension is (i.e., more common extensions come first).
//
// See: https://www.iana.org/assignments/media-types/media-types.xhtml
macros::media_types! {
    (Css, "text/css", ["css"]),
    (Gif, "image/gif", ["gif"]),
    (Html, "text/html", ["html", "htm", "hxt", "shtml"]),
    (Ico, "image/x-icon", ["ico"]),
    (Jpeg, "image/jpeg", ["jpeg", "jpg"]),
    (Markdown, "text/markdown", ["md", "markdown"]),
    (Png, "image/png", ["png"]),
    (Scss, "text/x-scss", ["scss"]),
    (Webp, "image/webp", ["webp"]),
}

/// An in-memory representation of any asset meant for processing.
#[derive(Clone, Debug)]
pub struct Asset {
    /// The asset's logical path, including the asset's name.
    path: Text,

    /// The asset's media type.
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

/// Categories ("registries") of media types, as enumerated
/// by the [IANA](https://www.iana.org/assignments/media-types/media-types.xhtml).
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum MediaCategory {
    Application,
    Audio,
    Example,
    Font,
    Haptics,
    Image,
    Message,
    Model,
    Multipart,
    Text,
    Video,
}

impl From<&MediaType> for MediaCategory {
    fn from(value: &MediaType) -> Self {
        match value
            .name()
            .split("/")
            .next()
            .expect("split will always return at least one item")
        {
            "application" => Self::Application,
            "audio" => Self::Audio,

            // @caer: note: The IANA specifies that it's an error for media
            //        within the "example" category to appear outside of examples,
            //        but I'm not sure there's a reason to check that here...or even a way.
            "example" => Self::Example,

            "font" => Self::Font,
            "haptics" => Self::Haptics,
            "image" => Self::Image,
            "message" => Self::Message,
            "model" => Self::Model,
            "multipart" => Self::Multipart,
            "text" => Self::Text,
            "video" => Self::Video,

            // The default category for media of an unknown type
            // is application/octet-stream, AKA application.
            _ => Self::Application,
        }
    }
}

mod macros {

    /// Creates the [super::MediaType] enum.
    macro_rules! media_types {
        (
            $(
                ($variant:ident, $mime:expr, [$($ext:expr),+ $(,)?])
            ),+ $(,)?
        ) => {

            /// Media or "MIME" types of an asset.
            ///
            /// This enumeration of types is not a complete list of all
            /// media types: Only those types explicitly supported by this
            /// crate are listed.
            #[non_exhaustive]
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub enum MediaType {
                $($variant,)+

                /// An unknown media type
                Unknown {
                    extension: [Text; 1],
                },
            }

            impl MediaType {
                /// Returns the MIME type of this media type.
                pub fn name(&self) -> Text {
                    match self {
                        $(MediaType::$variant => Text::from($mime),)+
                        MediaType::Unknown { .. } => Text::from("application/octet-stream"),
                    }
                }

                /// Returns the category of this media type.
                pub fn category(&self) -> MediaCategory {
                    MediaCategory::from(self)
                }

                /// Returns the known extensions of this media type.
                pub fn extensions(&self) -> &[Text] {
                    match self {
                        $(
                            MediaType::$variant => &[
                                $(Text::Static($ext),)+
                            ],
                        )+
                        MediaType::Unknown { extension } => extension,
                    }
                }

                /// Returns the media type corresponding to `extension`,
                /// or [MediaType::Unknown] if the extension is unrecognized.
                pub fn from_extension(extension: &str) -> MediaType {
                    match extension {
                        $(
                            $(
                                $ext => MediaType::$variant,
                            )+
                        )+
                        _ => MediaType::Unknown {
                            extension: [extension.into()],
                        },
                    }
                }
            }
        };
    }

    // Re-export macros for use in outer module.
    pub(crate) use media_types;
}
