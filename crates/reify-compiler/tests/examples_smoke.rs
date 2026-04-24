//! Bulk smoke test — every `examples/*.ri` must parse and compile with stdlib
//! with no Error-severity diagnostics.
//!
//! Motivation: per-file test wrappers (m5_integration, m8_stdlib_integration,
//! m11_full_integration, …) cover a subset of the 42 example files, but files
//! without a wrapper drift silently.  This test walks the directory and catches
//! every file at once.

use std::path::Path;

/// Sanity guard: every entry in SKIP_SET must name a file that actually exists
/// under `examples/`.  Catches mis-typed or stale skip entries before they
/// silently disable coverage.
#[test]
fn skip_set_entries_exist_under_examples_dir() {
    for (filename, reason) in SKIP_SET {
        let path = Path::new(EXAMPLES_DIR).join(filename);
        assert!(
            path.exists(),
            "SKIP_SET entry '{}' (reason: {}) does not exist under {}",
            filename,
            reason,
            EXAMPLES_DIR,
        );
    }
}
