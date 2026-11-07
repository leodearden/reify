//! Opt-in size control for the debug `engine_state` read.
//!
//! The full payload inlines every source file, which can run to megabytes on a
//! large design, so a caller may ask for a summary or for chosen top-level keys
//! instead. Both are projections of the full payload: the valid key names and
//! the summarised arrays are whatever it carries, so a key added to it is
//! selectable and summarised with nothing here to update.

use serde_json::{Map, Value, json};

/// Which view of the `engine_state` payload a caller asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineStateView {
    /// The whole payload, unchanged.
    Full,
    /// Only these top-level keys.
    Fields(Vec<String>),
    /// A count per array-valued key, every other key verbatim, and the files
    /// listed without their content.
    Summary,
}

impl EngineStateView {
    /// Read `summary_only` and `fields` from the tool's params. They are
    /// mutually exclusive; neither means the full view.
    pub fn from_params(params: &Value) -> Result<Self, String> {
        let summary_only = match params.get("summary_only") {
            None => false,
            Some(value) => value.as_bool().ok_or("summary_only must be a boolean")?,
        };
        let fields = params.get("fields").map(field_names).transpose()?;
        match (summary_only, fields) {
            (true, Some(_)) => Err("summary_only and fields are mutually exclusive".to_string()),
            (true, None) => Ok(Self::Summary),
            (false, Some(names)) => Ok(Self::Fields(names)),
            (false, None) => Ok(Self::Full),
        }
    }

    /// Project `full`, the whole `engine_state` payload. A field name the
    /// payload does not carry is refused with the valid names.
    pub fn apply(&self, full: Value) -> Result<Value, String> {
        match self {
            Self::Full => Ok(full),
            Self::Fields(names) => select_fields(into_object(full)?, names),
            Self::Summary => Ok(summarise(into_object(full)?)),
        }
    }
}

fn into_object(full: Value) -> Result<Map<String, Value>, String> {
    match full {
        Value::Object(map) => Ok(map),
        _ => Err("the engine_state payload is not a JSON object".to_string()),
    }
}

fn field_names(value: &Value) -> Result<Vec<String>, String> {
    let items = value
        .as_array()
        .ok_or("fields must be an array of top-level key names")?;
    if items.is_empty() {
        return Err("fields must not be empty".to_string());
    }
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("fields must contain only strings; got {item}"))
        })
        .collect()
}

fn select_fields(mut full: Map<String, Value>, names: &[String]) -> Result<Value, String> {
    let unknown: Vec<&str> = names
        .iter()
        .filter(|name| !full.contains_key(name.as_str()))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        let valid: Vec<&str> = full.keys().map(String::as_str).collect();
        return Err(format!(
            "unknown engine_state field(s) {unknown:?}; valid fields: {valid:?}"
        ));
    }
    Ok(Value::Object(
        names
            .iter()
            .filter_map(|name| full.remove_entry(name))
            .collect(),
    ))
}

fn summarise(full: Map<String, Value>) -> Value {
    let mut counts = Map::new();
    let mut summary = Map::new();
    for (key, value) in full {
        match value {
            Value::Array(items) => {
                counts.insert(key.clone(), json!(items.len()));
                if key == "files" {
                    summary.insert(key, items.iter().map(file_listing).collect());
                }
            }
            other => {
                summary.insert(key, other);
            }
        }
    }
    summary.insert("counts".to_string(), Value::Object(counts));
    Value::Object(summary)
}

/// A `files` entry without its content: its path, size and line count.
fn file_listing(file: &Value) -> Value {
    let content = file
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    json!({
        "path": file.get("path"),
        "bytes": content.len(),
        "lines": content.lines().count(),
    })
}
