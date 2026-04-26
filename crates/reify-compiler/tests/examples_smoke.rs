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
    assert!(
        total >= 40,
        "examples_smoke discovered only {} .ri files — expected ~42; \
         did the examples/ directory move or get renamed?",
        total
    );

    let mut exercised = 0usize;
    for path in &paths {
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if skip.contains(filename.as_str()) {
            continue;
        }
        exercised += 1;
        smoke_one(path, &filename, &mut failures);
    }

    if !failures.is_empty() {
        let n = failures.len();
        let skipped = total - exercised;
        let blocks: Vec<String> = failures
            .into_iter()
            .map(|(name, errors)| format!("=== {} ===\n{}", name, errors))
            .collect();
        panic!(
            "examples_smoke: {} of {} exercised files failed ({} skipped):\n\n{}",
            n,
            exercised,
            skipped,
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

/// Strip the `EXAMPLES_DIR` prefix from `path` and return a portable,
/// forward-slash-separated relative path string.
///
/// For example:
/// - `<EXAMPLES_DIR>/bracket.ri`                   → `"bracket.ri"`
/// - `<EXAMPLES_DIR>/fields/composed_stiffness.ri` → `"fields/composed_stiffness.ri"`
///
/// This is the canonical form used as SKIP_SET keys and in failure reports,
/// so that same-basename files in different subdirectories are unambiguous.
fn relative_to_examples_dir(path: &Path) -> String {
    let rel = path
        .strip_prefix(EXAMPLES_DIR)
        .unwrap_or_else(|e| panic!(
            "examples_smoke: '{}' is not under EXAMPLES_DIR ({}): {}",
            path.display(), EXAMPLES_DIR, e
        ));
    rel.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// Return all `*.ri` files under `EXAMPLES_DIR` (recursively), sorted by
/// their full path for deterministic output.
fn discover_ri_files() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_ri_files(std::path::Path::new(EXAMPLES_DIR), &mut paths);
    paths.sort();
    paths
}

/// Recursively collect `*.ri` files under `dir` into `out`.
fn collect_ri_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "examples_smoke: cannot read directory '{}': {}",
            dir.display(),
            e
        )
    });
    for entry in entries {
        let entry = entry.expect("IO error reading examples dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_ri_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ri") {
            out.push(path);
        }
    }
}

/// Verify that `relative_to_examples_dir` strips the `EXAMPLES_DIR` prefix and
/// returns a portable forward-slash-separated relative path for both top-level
/// and nested `.ri` files.
#[test]
fn relative_to_examples_dir_strips_prefix_for_top_level_and_nested_files() {
    let top_level = Path::new(EXAMPLES_DIR).join("bracket.ri");
    let nested = Path::new(EXAMPLES_DIR).join("fields/composed_stiffness.ri");

    assert_eq!(relative_to_examples_dir(&top_level), "bracket.ri");
    assert_eq!(
        relative_to_examples_dir(&nested),
        "fields/composed_stiffness.ri"
    );
}

/// Verify that the string produced by `relative_to_examples_dir` for the only
/// nested example on HEAD round-trips back to an existing file when joined onto
/// `EXAMPLES_DIR`.
///
/// This locks the SKIP_SET-key contract: entries must be join-compatible
/// (no leading slash, portable forward-slash separators, no absolute path).
#[test]
fn relative_to_examples_dir_output_round_trips_to_existing_file() {
    let abs = Path::new(EXAMPLES_DIR).join("fields/composed_stiffness.ri");
    let rel = relative_to_examples_dir(&abs);
    let joined = Path::new(EXAMPLES_DIR).join(&rel);
    assert!(
        joined.exists() && joined.is_file(),
        "round-trip failed: EXAMPLES_DIR.join({:?}) = {:?} does not exist",
        rel,
        joined
    );
}

/// Parse `path`, compile it with the stdlib prelude, and append an entry to
/// `failures` if either parse errors or Error-severity compile diagnostics are
/// found.  Returns without appending when the file is clean.
///
/// `filename` is the `file_name()` string computed by the caller; passing it
/// in avoids a second extraction of the same data.
fn smoke_one(path: &Path, filename: &str, failures: &mut Vec<(String, String)>) {
    use reify_compiler::compile_with_stdlib;
    use reify_types::{ModulePath, Severity};

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
            .map(|e| e.message.clone())
            .collect();
        failures.push((filename.to_owned(), msgs.join("\n")));
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
        failures.push((filename.to_owned(), errors.join("\n")));
    }
}
