use codas::types::Text;

// Definitions for all asset media types explicitly supported for
// processing, in alphabetical order by their "logical" names
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
