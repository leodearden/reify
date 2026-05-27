// Tests for path_key::canonicalize_document_key.
//
// Several tests mutate the process-wide current directory. A static Mutex
// serialises those tests so parallel test threads don't interfere with each
// other's CWD state.

use std::sync::Mutex;

/// Lock to serialise tests that call `std::env::set_current_dir`.
static CWD_LOCK: Mutex<()> = Mutex::new(());

/// (a) Given a CWD-relative path that exists on disk, returns the absolute
/// realpath (same as `fs::canonicalize`).
#[test]
fn canonicalize_returns_absolute_realpath_for_cwd_relative() {
    use crate::path_key::canonicalize_document_key;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("myfile.ri");
    std::fs::write(&file, "").unwrap();
    // Compute expected BEFORE changing CWD so it's safe regardless of
    // what happens inside the lock.
    let expected = std::fs::canonicalize(&file)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let _guard = CWD_LOCK.lock().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let result = canonicalize_document_key("myfile.ri");

    std::env::set_current_dir(&original).unwrap();

    assert_eq!(result, expected, "relative path should resolve to canonical absolute path");
}

/// (b) `./foo.ri`, `foo.ri`, and the absolute path to the same on-disk file
/// all produce the SAME canonical string.
#[test]
fn canonicalize_is_same_for_different_spellings() {
    use crate::path_key::canonicalize_document_key;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("foo.ri");
    std::fs::write(&file, "").unwrap();
    let abs_path = std::fs::canonicalize(&file)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let _guard = CWD_LOCK.lock().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let by_bare = canonicalize_document_key("foo.ri");
    let by_dotslash = canonicalize_document_key("./foo.ri");
    let by_abs = canonicalize_document_key(&abs_path);

    std::env::set_current_dir(&original).unwrap();

    assert_eq!(
        by_bare, by_dotslash,
        "foo.ri and ./foo.ri should produce the same canonical key"
    );
    assert_eq!(
        by_bare, by_abs,
        "foo.ri and the absolute path should produce the same canonical key"
    );
}

/// (c) A symlink is resolved to its target's realpath.
#[cfg(unix)]
#[test]
fn canonicalize_resolves_symlink_to_real_target() {
    use crate::path_key::canonicalize_document_key;

    let dir = tempfile::tempdir().unwrap();
    let real_file = dir.path().join("real.ri");
    let link_file = dir.path().join("link.ri");
    std::fs::write(&real_file, "").unwrap();
    std::os::unix::fs::symlink(&real_file, &link_file).unwrap();

    let expected = std::fs::canonicalize(&real_file)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let result = canonicalize_document_key(link_file.to_str().unwrap());

    assert_eq!(
        result, expected,
        "symlink should resolve to the real file's canonical path"
    );
}

/// (d) When the path does not exist on disk, falls back to the input string
/// (no panic, no Err propagation).
#[test]
fn canonicalize_falls_back_to_input_for_nonexistent_path() {
    use crate::path_key::canonicalize_document_key;

    let nonexistent = "/tmp/__reify_test_nonexistent_xyzzy_12345/missing.ri";
    let result = canonicalize_document_key(nonexistent);

    assert_eq!(
        result, nonexistent,
        "nonexistent path should fall back to the input string without panicking"
    );
}

// --- debug_server path canonicalisation coverage (step-13) ---
//
// `handle_open_file` in `debug_server.rs` is async and requires a full Tauri
// runtime to test end-to-end.  The sync helper `canonicalize_debug_open_path`
// is extracted from that path so we can verify the canonicalisation logic in
// isolation.  These tests carry the `debug_server` naming in their identifiers
// so the coverage link to the bug-source is immediately clear.

/// A `./foo.ri` relative spelling and the absolute path to the same on-disk
/// file should produce the SAME key via `canonicalize_debug_open_path` —
/// mirrors `handle_open_file`'s call site for the debug_server path.
#[test]
fn canonicalize_debug_open_path_relative_matches_absolute() {
    use crate::path_key::canonicalize_debug_open_path;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("design.ri");
    std::fs::write(&file, "structure Debug {}").unwrap();
    let abs_str = std::fs::canonicalize(&file)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let _guard = CWD_LOCK.lock().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let by_dotslash = canonicalize_debug_open_path("./design.ri");
    let by_abs = canonicalize_debug_open_path(&abs_str);

    std::env::set_current_dir(&original).unwrap();

    assert_eq!(
        by_dotslash, by_abs,
        "./design.ri and the absolute path should produce the same canonical key \
         (debug_server handle_open_file path)"
    );
}

/// Non-existent path falls back to the input string (no panic) — same
/// contract as `canonicalize_document_key`.
#[test]
fn canonicalize_debug_open_path_fallback_for_nonexistent() {
    use crate::path_key::canonicalize_debug_open_path;

    let nonexistent = "/tmp/__reify_debug_open_path_nonexistent_xyzzy.ri";
    let result = canonicalize_debug_open_path(nonexistent);
    assert_eq!(
        result, nonexistent,
        "nonexistent path should fall back to input without panicking"
    );
}
