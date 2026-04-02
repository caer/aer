use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

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

/// Build-time system state shared across all processors.
#[derive(Debug)]
pub struct Environment {
    pub source_root: PathBuf,
    pub kit_imports: BTreeMap<String, PathBuf>,
    /// Maps asset input paths to their final output paths.
    /// Populated during processing; used to resolve [ContextValue::AssetRef].
    pub asset_outputs: RwLock<BTreeMap<String, String>>,
}

#[cfg(test)]
impl Environment {
    pub fn test() -> Self {
        Self {
            source_root: std::path::PathBuf::from("."),
            kit_imports: Default::default(),
            asset_outputs: RwLock::new(BTreeMap::new()),
        }
    }
}

/// A thing that processes [Asset]s.
pub trait ProcessesAssets {
    /// Processes `asset` with access to a shared `context`.
    fn process(
        &self,
        env: &Environment,
        context: &LayeredContext,
        asset: &mut Asset,
    ) -> Result<(), ProcessingError>;
}

/// A flat context map used for overlays, table internals, and base construction.
pub type Context = BTreeMap<Text, ContextValue>;

/// Metadata extracted from a processed asset, used to populate `_assets:` entries.
pub type AssetMetadata = Context;

/// A layered context that resolves keys by checking overlay layers top-down,
/// then falling back to a shared immutable base.
///
/// The base contains global config, parts, tool entries, and `_assets:` metadata.
/// Overlays contain per-asset values (frontmatter, pattern defaults, loop variables).
/// Priority is structural: higher layers shadow lower ones.
pub struct LayeredContext {
    base: Arc<Context>,
    overlays: Vec<Context>,
}

impl LayeredContext {
    /// Creates a layered context from a shared base with no overlays.
    pub fn new(base: Arc<Context>) -> Self {
        Self {
            base,
            overlays: Vec::new(),
        }
    }

    /// Creates a layered context from a flat map (wraps it in an Arc).
    /// Useful for tests.
    pub fn from_flat(context: Context) -> Self {
        Self::new(Arc::new(context))
    }

    pub fn get(&self, key: &Text) -> Option<&ContextValue> {
        for overlay in self.overlays.iter().rev() {
            if let Some(v) = overlay.get(key) {
                return Some(v);
            }
        }
        self.base.get(key)
    }

    pub fn push_layer(&mut self) {
        self.overlays.push(Context::new());
    }

    pub fn pop_layer(&mut self) -> Option<Context> {
        self.overlays.pop()
    }

    /// Panics if there are no overlays.
    pub fn insert(&mut self, key: Text, value: ContextValue) {
        self.overlays
            .last_mut()
            .expect("insert requires at least one overlay")
            .insert(key, value);
    }

    /// Removes a key from the first overlay that contains it (top-down).
    /// Does not touch the base layer.
    pub fn remove(&mut self, key: &Text) -> Option<ContextValue> {
        for overlay in self.overlays.iter_mut().rev() {
            if let Some(v) = overlay.remove(key) {
                return Some(v);
            }
        }
        None
    }

    pub fn extend_top(&mut self, entries: Context) {
        self.overlays
            .last_mut()
            .expect("extend_top requires at least one overlay")
            .extend(entries);
    }

    /// Inserts entries only if the key is not present in any layer.
    pub fn fill(&mut self, entries: Context) {
        for (key, value) in entries {
            if self.get(&key).is_none() {
                self.insert(key, value);
            }
        }
    }

    /// Inserts a layer below the topmost overlay.
    /// Used for pattern defaults that should sit between global and page context.
    pub fn insert_layer_below_top(&mut self, layer: Context) {
        let top = self.overlays.pop();
        self.overlays.push(layer);
        if let Some(top) = top {
            self.overlays.push(top);
        }
    }

    /// Creates an independent child scope for loop bodies and part
    /// rendering. Inherits all current layers but can diverge without
    /// affecting the parent.
    pub fn child_scope(&self) -> Self {
        let mut child = Self {
            base: Arc::clone(&self.base),
            overlays: self.overlays.clone(),
        };
        child.push_layer();
        child
    }

    /// Iterates base-layer entries whose keys start with `prefix`.
    /// Only scans the base; overlay entries are not included.
    pub fn iter_by_prefix<'a>(
        &'a self,
        prefix: &'a str,
    ) -> impl Iterator<Item = (&'a Text, &'a ContextValue)> + 'a {
        let start: Text = prefix.into();
        self.base
            .range(start..)
            .take_while(move |(k, _)| k.as_str().starts_with(prefix))
    }

    /// Resolves a possibly-dotted identifier against the layered context.
    /// The first segment is resolved through layers; remaining segments
    /// walk into nested [ContextValue::Table]s.
    pub fn resolve(&self, identifier: &str) -> Option<&ContextValue> {
        if !identifier.contains('.') {
            let key: Text = identifier.into();
            return self.get(&key);
        }

        let mut segments = identifier.split('.');
        let first: Text = segments.next()?.into();
        let mut current = self.get(&first)?;

        for segment in segments {
            match current {
                ContextValue::Table(table) => {
                    let key: Text = segment.into();
                    current = table.get(&key)?;
                }
                _ => return None,
            }
        }

        Some(current)
    }
}

/// Value types used in processing [Context].
#[derive(Debug, Clone)]
pub enum ContextValue {
    Text(Text),
    List(Vec<ContextValue>),
    Table(Context),
    /// A reference to another asset by its input path.
    /// Resolves to the asset's final output path during template rendering.
    AssetRef(Text),
}

/// Converts a TOML table into a processing [Context].
pub fn context_from_toml(table: toml::Table) -> Result<Context, ProcessingError> {
    let mut context = Context::default();
    for (key, value) in table {
        context.insert(key.into(), ContextValue::from_toml(value)?);
    }
    Ok(context)
}

/// Delimiter separating TOML frontmatter from content.
pub const FRONTMATTER_DELIMITER: &str = "***";
const FRONTMATTER_DELIMITER_LINE: &str = "\n***\n";
const FRONTMATTER_DELIMITER_START: &str = "***\n";

/// Extracts TOML frontmatter from some text, returning the
/// remaining content and any parsed context values.
///
/// If the content before the delimiter is not valid TOML, returns the
/// original content unchanged (the delimiter might just be regular content).
pub fn extract_frontmatter(text: &Text) -> (&str, Option<Context>) {
    let split_pos = if text.starts_with(FRONTMATTER_DELIMITER_START) {
        Some(0)
    } else {
        text.find(FRONTMATTER_DELIMITER_LINE)
    };

    let Some(pos) = split_pos else {
        return (text.as_str(), None);
    };

    let frontmatter = &text[..pos];
    let body_start = if pos == 0 {
        FRONTMATTER_DELIMITER_START.len()
    } else {
        pos + FRONTMATTER_DELIMITER_LINE.len() - 1
    };
    let body = &text[body_start..];

    let table: toml::Table = match toml::from_str(frontmatter) {
        Ok(t) => t,
        Err(_) => return (text, None),
    };
    match context_from_toml(table) {
        Ok(parsed) => (body, Some(parsed)),
        Err(_) => (text, None),
    }
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
}
