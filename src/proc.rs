use codas::types::Text;

mod asset;
pub use asset::{Asset, MediaType, MediaCategory};
pub mod image;
pub mod markdown;
pub mod scss;

/// A thing that processes [Asset]s.
pub trait ProcessesAssets {
    /// Processes `asset`.
    fn process(&self, asset: &mut Asset) -> Result<(), ProcessingError>;
}

/// An error that occurs while procesing assets.
#[derive(Debug, PartialEq, Eq)]
pub enum ProcessingError {
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
