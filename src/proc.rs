use std::collections::BTreeMap;

use codas::types::Text;

mod asset;
pub use asset::{Asset, MediaCategory, MediaType};
pub mod canonicalize;
pub mod image;
pub mod js_bundle;
pub mod markdown;
pub mod minify_html;
pub mod minify_js;
pub mod scss;
pub mod template;

/// A thing that processes [Asset]s.
pub trait ProcessesAssets {
    /// Processes `asset` with access to a shared `context`.
    fn process(&self, context: &mut Context, asset: &mut Asset) -> Result<(), ProcessingError>;
}

/// A shared processing context passed between processors.
pub type Context = BTreeMap<Text, ContextValue>;

/// Value types used in processing [Context].
#[derive(Debug, Clone)]
pub enum ContextValue {
    Text(Text),
    List(Vec<Text>),
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
