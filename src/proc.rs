use std::collections::BTreeMap;

use codas::types::Text;

mod asset;
pub use asset::{Asset, MediaCategory, MediaType};
pub mod canonicalize;
pub mod favicon;
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
    List(Vec<ContextValue>),
    Table(Context),
}

/// Converts a TOML table into a processing [Context].
pub fn context_from_toml(table: toml::Table) -> Result<Context, ProcessingError> {
    let mut context = Context::default();
    for (key, value) in table {
        context.insert(key.into(), ContextValue::from_toml(value)?);
    }
    Ok(context)
}

impl ContextValue {
    /// Converts a TOML [toml::Value] into a [ContextValue].
    pub fn from_toml(value: toml::Value) -> Result<Self, ProcessingError> {
        match value {
            toml::Value::String(s) => Ok(ContextValue::Text(s.into())),
            toml::Value::Integer(n) => Ok(ContextValue::Text(n.to_string().into())),
            toml::Value::Float(n) => Ok(ContextValue::Text(n.to_string().into())),
            toml::Value::Boolean(b) => Ok(ContextValue::Text(b.to_string().into())),
            toml::Value::Array(arr) => {
                let items: Result<Vec<ContextValue>, _> =
                    arr.into_iter().map(ContextValue::from_toml).collect();
                Ok(ContextValue::List(items?))
            }
            toml::Value::Table(table) => Ok(ContextValue::Table(context_from_toml(table)?)),
            toml::Value::Datetime(dt) => Ok(ContextValue::Text(dt.to_string().into())),
        }
    }
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

    /// The processor cannot complete until other
    /// assets in the current pass have been processed.
    Deferred,
}
