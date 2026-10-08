//! Writing an image tool's PNG to disk instead of returning it inline.
//!
//! The screenshot tools reply `{data: "data:image/png;base64,..."}` from the
//! frontend. With a `save_path` the caller gets `{saved_to, bytes, mimeType}`
//! instead, which keeps a large image out of the tool result. The bytes are
//! decoded and written here, so the webview needs no filesystem access.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

const PNG_DATA_URL_PREFIX: &str = "data:image/png;base64,";

/// The base64 payload of a PNG data URL, or `data` itself when it carries no
/// data-URL prefix.
pub fn png_base64(data: &str) -> &str {
    data.strip_prefix(PNG_DATA_URL_PREFIX).unwrap_or(data)
}

/// Split `save_path` off an image tool's params, returning it with the params
/// to forward to the frontend. The path must be absolute, since the GUI's
/// working directory is arbitrary, and must end in `.png`, since an existing
/// file there is overwritten.
pub fn take_save_path(mut params: Value) -> Result<(Option<PathBuf>, Value), String> {
    let Some(raw) = params.as_object_mut().and_then(|p| p.remove("save_path")) else {
        return Ok((None, params));
    };
    let path = PathBuf::from(raw.as_str().ok_or("save_path must be a string")?);
    if !path.is_absolute() {
        return Err(format!(
            "save_path must be an absolute path; got {}",
            path.display()
        ));
    }
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        return Err(format!(
            "save_path must end in .png; got {}",
            path.display()
        ));
    }
    Ok((Some(path), params))
}

/// Write the PNG in an image tool's `result` to `path` and reply where it went,
/// keeping every other key of the result. A result without string `data` — an
/// error shape — is returned verbatim with nothing written. The parent
/// directory must already exist; it is never created.
pub fn save_image_result(result: Value, path: &Path) -> Result<Value, String> {
    let Value::Object(mut fields) = result else {
        return Ok(result);
    };
    let Some(data) = fields.get("data").and_then(Value::as_str) else {
        return Ok(Value::Object(fields));
    };
    let bytes = STANDARD
        .decode(png_base64(data))
        .map_err(|e| format!("screenshot data is not valid base64: {e}"))?;
    if let Some(parent) = path.parent()
        && !parent.is_dir()
    {
        return Err(format!(
            "save_path's parent directory {} does not exist",
            parent.display()
        ));
    }
    std::fs::write(path, &bytes)
        .map_err(|e| format!("failed to write screenshot to {}: {e}", path.display()))?;
    fields.remove("data");
    fields.insert("saved_to".to_string(), json!(path.to_string_lossy()));
    fields.insert("bytes".to_string(), json!(bytes.len()));
    fields.insert("mimeType".to_string(), json!("image/png"));
    Ok(Value::Object(fields))
}
