/// Collection of all media or "MIME" types that
/// are _explicitly_ supported by asset procesors.
///
/// See: https://www.iana.org/assignments/media-types/media-types.xhtml
use codas::types::Text;

// Definitions for all media types, in alphabetical order
// by their "logical" names (e.g., "Css" comes before "Markdown").
//
// Each media type is a tuple of `(name, mime_type, [extensions])`.
// Extensions should be ordered, roughly, in terms of how common the
// extension is (i.e., more common extensions come first).
macros::media_types! {
    (Css, "text/css", ["css"]),
    (Markdown, "text/markdown", ["md", "markdown"]),
    (Html, "text/html", ["html", "htm", "hxt", "shtml"]),
    (Scss, "text/x-scss", ["scss"]),
}

mod macros {

    /// Creates the [super::MediaType] enum.
    macro_rules! media_types {
        (
            $(
                ($variant:ident, $mime:expr, [$($ext:expr),+ $(,)?])
            ),+ $(,)?
        ) => {

            /// Supported Media or "MIME" types.
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
                /// or [Self::Unknown] if the extension is unrecognized.
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
