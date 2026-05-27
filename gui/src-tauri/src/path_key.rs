// Shared helper for canonicalising file-path document keys.
//
// The GUI editor uses file paths as document identity keys. A single physical
// file can be referenced by multiple path spellings (relative, absolute, with
// symlinks, with `.`/`..` segments). Canonicalising every path before it is
// used as a key prevents the editor from opening the same file twice under
// different spellings (the root cause of duplicate-tab bug #3892).
//
// `std::fs::canonicalize` is the only standard API that resolves both
// symlinks and CWD-relative paths. The frontend's `canonicalizeKey` TS helper
// adds defence-in-depth for `file://`-prefix and `.`/`..` segments, but cannot
// perform a true `realpath(3)` call from inside the browser/webview sandbox.

/// Return the canonical absolute path string for `path`.
///
/// On success (the filesystem entry exists), returns the result of
/// [`std::fs::canonicalize`] converted to a `String` via
/// `to_string_lossy().into_owned()`.
///
/// On failure (the path does not exist, or any other OS error), falls back
/// to `path.to_string()` without panicking or propagating the error.  The
/// caller's primary intent (read / load the file) will then surface the
/// actionable IO error on the next filesystem operation.
pub fn canonicalize_document_key(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}
