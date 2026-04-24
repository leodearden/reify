//! Bulk smoke test — every `examples/*.ri` must parse and compile with stdlib
//! with no Error-severity diagnostics.
//!
//! Motivation: per-file test wrappers (m5_integration, m8_stdlib_integration,
//! m11_full_integration, …) cover a subset of the 42 example files, but files
//! without a wrapper drift silently.  This test walks the directory and catches
//! every file at once.

use std::path::{Path, PathBuf};

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

/// Bulk smoke: walk `examples/*.ri`, parse each file and compile it with the
/// stdlib prelude, accumulate every file that produces an Error-severity
/// diagnostic, and panic once at the end with a report covering ALL failures.
///
/// A single test run therefore surfaces every broken file rather than stopping
/// at the first one.  Files listed in `SKIP_SET` are excluded from the walk.
#[test]
fn all_examples_parse_and_compile_with_stdlib() {
    use std::collections::HashSet;

    let skip: HashSet<&str> = SKIP_SET.iter().map(|(name, _)| *name).collect();
    let mut failures: Vec<(String, String)> = Vec::new();

    let paths = discover_ri_files();
    let total = paths.len();

    for path in &paths {
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if skip.contains(filename.as_str()) {
            continue;
        }
        smoke_one(path, &mut failures);
    }

    if !failures.is_empty() {
        let n = failures.len();
        let blocks: Vec<String> = failures
            .into_iter()
            .map(|(name, errors)| format!("=== {} ===\n{}", name, errors))
            .collect();
        panic!(
            "examples_smoke: {} of {} files failed:\n\n{}",
            n,
            total,
            blocks.join("\n\n")
        );
    }
}

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

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Return all `*.ri` files under `EXAMPLES_DIR`, sorted by filename for
/// deterministic output.
fn discover_ri_files() -> Vec<PathBuf> {
    let dir = std::fs::read_dir(EXAMPLES_DIR).unwrap_or_else(|e| {
        panic!(
            "examples_smoke: cannot read examples directory '{}': {}",
            EXAMPLES_DIR, e
        )
    });

    let mut paths: Vec<PathBuf> = dir
        .filter_map(|entry| {
            let entry = entry.expect("IO error reading examples dir entry");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("ri") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    paths.sort_by(|a, b| {
        a.file_name()
            .unwrap_or_default()
            .cmp(b.file_name().unwrap_or_default())
    });
    paths
}

/// Parse `path`, compile it with the stdlib prelude, and append an entry to
/// `failures` if either parse errors or Error-severity compile diagnostics are
/// found.  Returns without appending when the file is clean.
fn smoke_one(path: &Path, failures: &mut Vec<(String, String)>) {
    use reify_compiler::compile_with_stdlib;
    use reify_types::{ModulePath, Severity};

    let filename = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("examples_smoke: cannot read '{}': {}", filename, e)
    });

    // Derive a module name from the file stem (e.g. "m5_geometry_flange").
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let module_path = ModulePath::single(&stem);

    // Parse phase — accumulate, do NOT panic on errors.
    let parsed = reify_syntax::parse(&source, module_path);
    if !parsed.errors.is_empty() {
        let msgs: Vec<String> = parsed
            .errors
            .iter()
            .map(|e| format!("{:?}", e))
            .collect();
        failures.push((filename, msgs.join("\n")));
        return;
    }

    // Compile phase — filter to Error severity only.
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<String> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();

    if !errors.is_empty() {
        failures.push((filename, errors.join("\n")));
    }
}
