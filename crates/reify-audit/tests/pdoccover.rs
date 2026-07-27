//! Integration tests for the PDOCCOVER registry↔chunk name-drift detector
//! (`pdoccover::check`).
//!
//! Two test families:
//!
//! - **Hermetic fixture trees** — a `tempfile` tempdir as `project_root` with
//!   synthetic `units.rs` / `chunks/*.md` / `pdoccover-baseline.txt` files, a
//!   `MockGitOps::set_ls_files`, an in-memory rusqlite and a
//!   `MockJCodemunchOps` (the `tests/pdssentinel.rs` harness). Only the file
//!   *list* is mocked; `check()` reads real content from disk. Every
//!   disposition assertion — including the `offset_surface` fabrication
//!   fixture — lives here, never against the real tree, so a sibling task
//!   editing chunk content cannot flip these RED.
//!
//! - **Brittle-parse floor guards** — run the pure scanners over the REAL
//!   `crates/reify-compiler/src/units.rs` and `crates/reify-mcp/src/tools/
//!   chunks/*.md` and assert conservative floors. Their job is to fail RED
//!   when a source refactor breaks extraction, instead of letting PDOCCOVER
//!   silently pass clean on an empty census. They freeze no exact count.

mod common;

use std::path::{Path, PathBuf};

/// Write `content` to relative `path` inside `root`, creating parent dirs.
#[allow(dead_code)]
fn write_file(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write_file");
}

/// Repo root, resolved from `CARGO_MANIFEST_DIR` (= `crates/reify-audit`).
#[allow(dead_code)]
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

// ─────────────────────────────────────────────────────────────────────────────
// step-3: brittle-parse floor guard — REGISTRY scan path
// ─────────────────────────────────────────────────────────────────────────────

/// The eight `*_NAMES` registries the PRD names as the omission census corpus.
///
/// A **floor**, not an exact set: fifteen are discovered on main today and
/// more may be added. What this guard defends is that a `units.rs` refactor
/// cannot make `extract_registries` silently stop finding them — which would
/// leave PDOCCOVER passing clean on an empty census, the worst possible
/// failure mode for a coverage detector.
///
/// The PRD's line numbers (`:21`, `:102`, …) are provenance against main
/// `92551d18d6`, NOT assertions — nothing here pins a line.
const PRD_NAMED_REGISTRIES: &[&str] = &[
    "GEOMETRY_FUNCTION_NAMES",
    "GEOMETRY_QUERY_HELPER_NAMES",
    "GEOMETRY_KINEMATIC_QUERY_NAMES",
    "GEOMETRY_TOPOLOGY_SELECTOR_NAMES",
    "AFFINE_MAP_CONSTRUCTOR_NAMES",
    "TOLERANCING_MARKER_NAMES",
    "GEOMETRY_QUERY_NAMES",
    "DYNAMICS_QUERY_NAMES",
];

/// Conservative floor on the total distinct name census, set well below the
/// count on main (151 distinct at the time of writing; the PRD cites ~133 for
/// its 8-registry subset) so ordinary additions and removals never flip this
/// RED — only a parse regression that collapses the census does.
const REGISTRY_NAME_FLOOR: usize = 60;

/// `extract_registries` over the REAL `crates/reify-compiler/src/units.rs`
/// must (i) find every PRD-named registry, (ii) find a non-empty entry list
/// for every registry it discovers, (iii) clear a conservative distinct-name
/// floor, and (iv) yield only identifier-shaped names.
///
/// Purpose: a `units.rs` refactor (a new declaration shape, a re-indent, a
/// macro, an attribute) that breaks extraction fails RED here rather than
/// yielding a silently false-clean coverage result.
#[test]
fn registry_extraction_floor_guard_against_real_units_rs() {
    let units_path = repo_root().join("crates/reify-compiler/src/units.rs");
    let src = std::fs::read_to_string(&units_path).unwrap_or_else(|e| {
        panic!(
            "the real units.rs must be readable at {}; got: {e}. \
             (If this file moved, the PDOCCOVER omission census moved with it \
             and pdoccover::UNITS_PATH needs updating.)",
            units_path.display()
        )
    });

    let regs = reify_audit::pdoccover::extract_registries(&src);

    // (i) Every PRD-named registry is present — a FLOOR, extras are fine.
    let found: Vec<&str> = regs.iter().map(|r| r.const_name.as_str()).collect();
    let missing: Vec<&str> = PRD_NAMED_REGISTRIES
        .iter()
        .copied()
        .filter(|want| !found.contains(want))
        .collect();
    assert!(
        missing.is_empty(),
        "extract_registries missed {} PRD-named registr{} in the real units.rs: \
         {:?}. Found {} registr{}: {:?}. A declaration-shape change in units.rs \
         broke extraction — fix the parser, do NOT relax this floor.",
        missing.len(),
        if missing.len() == 1 { "y" } else { "ies" },
        missing,
        found.len(),
        if found.len() == 1 { "y" } else { "ies" },
        found,
    );

    // (ii) No discovered registry may be empty — an empty entry list is the
    // signature of a header that matched but whose body did not parse.
    let empty: Vec<&str> = regs
        .iter()
        .filter(|r| r.entries.is_empty())
        .map(|r| r.const_name.as_str())
        .collect();
    assert!(
        empty.is_empty(),
        "these registries were discovered but parsed to ZERO entries: {empty:?}. \
         The header matched but the body did not — extraction is broken."
    );

    // (iii) Distinct-name census clears a conservative floor.
    let distinct: std::collections::BTreeSet<&str> = regs
        .iter()
        .flat_map(|r| r.entries.iter())
        .map(|e| e.name.as_str())
        .collect();
    assert!(
        distinct.len() >= REGISTRY_NAME_FLOOR,
        "distinct registry-name census is {}, below the conservative floor of \
         {REGISTRY_NAME_FLOOR}. Extraction has regressed. Registries found: \
         {found:?}",
        distinct.len(),
    );

    // (iv) Every extracted name must be identifier-shaped. A stray
    // non-identifier token means a type annotation, doc string, attribute or
    // comment leaked into the census — it would surface as an undocumented
    // "name" that no chunk edit could ever satisfy.
    let malformed: Vec<&str> = distinct
        .iter()
        .copied()
        .filter(|n| {
            n.is_empty() || !n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        .collect();
    assert!(
        malformed.is_empty(),
        "these extracted 'names' are not identifier-shaped: {malformed:?}. \
         A non-entry token (type annotation, doc string, attribute, comment) \
         leaked into the census and would be reported as an undocumented name."
    );
}
