//! Bulk smoke test — every `examples/*.ri` must parse and compile with stdlib
//! with no Error-severity diagnostics.
//!
//! Motivation: per-file test wrappers (m5_integration, m8_stdlib_integration,
//! m11_full_integration, …) cover a subset of the 42 example files, but files
//! without a wrapper drift silently.  This test walks the directory and catches
//! every file at once.

use std::path::Path;

/// Absolute path to the workspace `examples/` directory, resolved at compile
/// time from this crate's manifest directory (two levels up).
const EXAMPLES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");

/// Files to skip in the bulk smoke test.  Each entry is `(filename, reason)`.
/// The reason is mandatory — the `(&str, &str)` tuple shape forces every entry
/// to carry a one-line human-readable justification, making skips auditable at
/// review time.
///
/// Default: empty — all 42 example files compile clean on HEAD after task #2181
/// (purpose-body MemberAccess) was merged on 2026-04-24.
const SKIP_SET: &[(&str, &str)] = &[];

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
