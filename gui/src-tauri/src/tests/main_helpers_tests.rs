// Tests for the resolve_initial_file_path helper used by main.rs to
// canonicalise the argv path before loading it into the engine.
//
// CWD-mutating tests are serialised via the shared process-global Mutex at
// `crate::tests::test_helpers::cwd_lock`.  These tests live in a separate
// file so the module boundary keeps main-helper concerns out of the general
// command tests.

use crate::tests::test_helpers::cwd_lock;

/// (a) Given a CWD-relative argv path that exists on disk, returns
/// `Some(canonical_absolute_pathbuf)`.
#[test]
fn resolve_initial_file_path_relative_existing_returns_canonical() {
    use crate::commands::resolve_initial_file_path;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mydesign.ri");
    std::fs::write(&file, "structure Foo {}").unwrap();
    let expected = std::fs::canonicalize(&file)
        .unwrap();

    let _guard = cwd_lock().lock().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let result = resolve_initial_file_path("mydesign.ri");

    std::env::set_current_dir(&original).unwrap();

    assert_eq!(
        result,
        Some(expected),
        "CWD-relative .ri path that exists should return Some(canonical)"
    );
}

/// (b) Given an already-absolute path, returns the same canonical path
/// (idempotent).
#[test]
fn resolve_initial_file_path_absolute_is_idempotent() {
    use crate::commands::resolve_initial_file_path;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("design.ri");
    std::fs::write(&file, "structure Bar {}").unwrap();
    let abs_str = file.to_str().unwrap().to_string();
    let expected = std::fs::canonicalize(&file).unwrap();

    let result = resolve_initial_file_path(&abs_str);
    assert_eq!(
        result,
        Some(expected),
        "Absolute .ri path should return Some(canonical) idempotently"
    );
}

/// (c) Returns `None` when the path is empty.
#[test]
fn resolve_initial_file_path_empty_returns_none() {
    use crate::commands::resolve_initial_file_path;

    assert_eq!(
        resolve_initial_file_path(""),
        None,
        "Empty path should return None"
    );
}

/// (c) Returns `None` when the extension is not `.ri`.
#[test]
fn resolve_initial_file_path_non_ri_extension_returns_none() {
    use crate::commands::resolve_initial_file_path;

    // .step, .stl, no-extension — all should return None
    assert_eq!(
        resolve_initial_file_path("model.step"),
        None,
        ".step extension should return None"
    );
    assert_eq!(
        resolve_initial_file_path("model.stl"),
        None,
        ".stl extension should return None"
    );
    assert_eq!(
        resolve_initial_file_path("/absolute/model.step"),
        None,
        "absolute .step path should return None"
    );
}

/// (d) Returns `Some(path)` even when the file does not exist on disk:
/// `canonicalize_document_key` falls back to the original string, so the
/// caller can attempt `load_file` and receive the actionable IO error.
#[test]
fn resolve_initial_file_path_nonexistent_ri_returns_some_fallback() {
    use crate::commands::resolve_initial_file_path;

    let nonexistent = "/tmp/__reify_test_nonexistent_xyzzy_3892/missing.ri";
    let result = resolve_initial_file_path(nonexistent);
    assert!(
        result.is_some(),
        "Nonexistent .ri path should still return Some (fallback to input)"
    );
    assert_eq!(
        result.unwrap(),
        std::path::PathBuf::from(nonexistent),
        "Fallback should be the original path string"
    );
}

// --- dispatch_file_event tests (step-11) ---
//
// A fake `EmitCapture` test double records (event_name, json_value) pairs so
// dispatch_file_event can be tested without a Tauri runtime.

use crate::watcher::FileEvent;

/// Minimal FileEventEmitter stand-in that captures emitted events.
struct EmitCapture {
    events: std::sync::Mutex<Vec<(String, serde_json::Value)>>,
}

impl EmitCapture {
    fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(vec![]),
        }
    }

    fn emitted(&self) -> Vec<(String, serde_json::Value)> {
        self.events.lock().unwrap().clone()
    }
}

impl crate::commands::FileEventEmitter for EmitCapture {
    fn emit_changed(&self, payload: crate::types::FileData) {
        let json = serde_json::json!({
            "path": payload.path,
            "content": payload.content,
        });
        self.events
            .lock()
            .unwrap()
            .push(("file-changed".to_string(), json));
    }

    fn emit_removed(&self, payload: serde_json::Value) {
        self.events
            .lock()
            .unwrap()
            .push(("file-removed".to_string(), payload));
    }
}

/// (a) FileEvent::Changed with readable file → emits ("file-changed", FileData) once.
#[test]
fn dispatch_file_event_changed_emits_file_changed() {
    use crate::commands::dispatch_file_event;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.ri");
    std::fs::write(&file, "structure Test {}").unwrap();

    let capture = EmitCapture::new();
    dispatch_file_event(&capture, FileEvent::Changed(file.clone()));

    let emitted = capture.emitted();
    assert_eq!(emitted.len(), 1, "should emit exactly one event");
    assert_eq!(emitted[0].0, "file-changed");
    let path_val = emitted[0].1.get("path").and_then(|v| v.as_str()).unwrap();
    assert!(
        path_val.ends_with("test.ri"),
        "emitted path should end with test.ri, got: {path_val}"
    );
    let content_val = emitted[0]
        .1
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(
        content_val.contains("structure Test"),
        "emitted content should contain file contents"
    );
}

/// (b) FileEvent::Removed → emits ("file-removed", {{ path }}) without reading content.
#[test]
fn dispatch_file_event_removed_emits_file_removed_without_reading() {
    use crate::commands::dispatch_file_event;

    // The file does NOT need to exist on disk — Removed events don't read content.
    let removed_path = std::path::PathBuf::from("/tmp/__reify_nonexistent_dispatch_test.ri");

    let capture = EmitCapture::new();
    dispatch_file_event(&capture, FileEvent::Removed(removed_path.clone()));

    let emitted = capture.emitted();
    assert_eq!(emitted.len(), 1, "should emit exactly one event");
    assert_eq!(emitted[0].0, "file-removed");
    let path_val = emitted[0].1.get("path").and_then(|v| v.as_str()).unwrap();
    assert!(
        path_val.contains("reify_nonexistent_dispatch_test.ri"),
        "emitted path should match the removed path, got: {path_val}"
    );
}

/// (c) FileEvent::Changed but content read fails → emitter NOT called.
#[test]
fn dispatch_file_event_changed_read_error_skips_emit() {
    use crate::commands::dispatch_file_event;

    // Point at a path that does not exist — read_to_string will Err.
    let nonexistent = std::path::PathBuf::from("/tmp/__reify_nonexistent_changed_dispatch.ri");

    let capture = EmitCapture::new();
    dispatch_file_event(&capture, FileEvent::Changed(nonexistent));

    let emitted = capture.emitted();
    assert!(
        emitted.is_empty(),
        "emitter should NOT be called when content read fails, got: {:?}",
        emitted
    );
}
