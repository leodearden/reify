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

use reify_audit::{
    AuditContext, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps, Pattern, Severity,
};
use rusqlite::Connection;
use std::collections::HashMap;
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

/// Owns the mock seams so an [`AuditContext`] (which borrows all three) can be
/// handed out without every test repeating the construction.
///
/// Only the file *list* is mocked — `check()` reads real content from disk
/// under `project_root`, exactly as `pdssentinel::check` does.
#[allow(dead_code)]
struct Harness {
    conn: Connection,
    git: MockGitOps,
    jc: MockJCodemunchOps,
}

#[allow(dead_code)]
impl Harness {
    fn new(tracked: &[&str]) -> Self {
        let mut git = MockGitOps::new();
        git.set_ls_files(tracked.iter().map(|p| p.to_string()).collect());
        Self {
            conn: Connection::open_in_memory().expect("in-memory sqlite"),
            git,
            jc: MockJCodemunchOps::new(),
        }
    }

    fn ctx(&self, root: &Path) -> AuditContext<'_> {
        AuditContext {
            project_root: root.to_path_buf(),
            conn: &self.conn,
            git: &self.git,
            jcodemunch: &self.jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        }
    }
}

/// Fixture paths — the three real paths PDOCCOVER's omission lane reads,
/// reproduced inside the tempdir so nothing here depends on the real tree.
#[allow(dead_code)]
const FIX_UNITS: &str = "crates/reify-compiler/src/units.rs";
#[allow(dead_code)]
const FIX_CHUNK: &str = "crates/reify-mcp/src/tools/chunks/geometry.md";
#[allow(dead_code)]
const FIX_BASELINE: &str = "crates/reify-audit/pdoccover-baseline.txt";

/// The summary body after a `<category>: ` prefix, up to the first space —
/// i.e. the offending name a finding is about.
#[allow(dead_code)]
fn finding_name(f: &Finding) -> &str {
    let after = f
        .summary
        .split_once(": ")
        .map(|(_, rest)| rest)
        .unwrap_or(&f.summary);
    after.split_whitespace().next().unwrap_or(after)
}

/// The `<category>` prefix of a finding summary.
#[allow(dead_code)]
fn finding_category(f: &Finding) -> &str {
    f.summary.split_once(':').map(|(c, _)| c).unwrap_or("")
}

// ─────────────────────────────────────────────────────────────────────────────
// step-7: OMISSION-direction `check()` integration — the three-way disposition
// ─────────────────────────────────────────────────────────────────────────────

/// A registry whose four names exercise all three compliant dispositions plus
/// the offender:
///
/// - `alpha_op` — documented in a chunk;
/// - `beta_op`  — `// pdoccover:allow — <reason>` on its entry line;
/// - `gamma_op` — listed in `pdoccover-baseline.txt`;
/// - `delta_op` — bare, and therefore the ONLY offender.
#[allow(dead_code)]
const FOUR_WAY_UNITS: &str = "\
//! Fixture registry for the PDOCCOVER omission lane.

pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    \"alpha_op\",
    \"beta_op\", // pdoccover:allow — internal lowering shim
    \"gamma_op\",
    \"delta_op\",
];
";

/// The chunk that documents `alpha_op` — and nothing else.
#[allow(dead_code)]
const ALPHA_CHUNK: &str = "\
# Geometry

- `alpha_op(shape, amount)` — the alpha operation.
";

/// With all three exemption channels populated, `check()` reports exactly the
/// one bare name, as a High `undocumented-name:` finding whose evidence points
/// at the registry source.
#[test]
fn omission_lane_reports_only_the_bare_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(root, FIX_UNITS, FOUR_WAY_UNITS);
    write_file(root, FIX_CHUNK, ALPHA_CHUNK);
    write_file(
        root,
        FIX_BASELINE,
        "# PDOCCOVER ratchet baseline — one name per line.\n\ngamma_op\n",
    );

    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK, FIX_BASELINE]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    assert_eq!(
        findings.len(),
        1,
        "expected exactly 1 finding (the bare `delta_op`); documented/allowed/\
         baselined names must all be exempt. Got {}: {:?}",
        findings.len(),
        findings
    );

    let f = &findings[0];
    assert_eq!(
        f.pattern,
        Pattern::PDocCover,
        "finding must carry the PDocCover pattern; got {:?}",
        f.pattern
    );
    assert_eq!(
        f.severity,
        Severity::High,
        "every PDOCCOVER finding rides at High severity; got {:?}",
        f.severity
    );
    assert!(
        f.summary.starts_with("undocumented-name:"),
        "summary must carry the stable `undocumented-name:` category prefix; \
         got {:?}",
        f.summary
    );
    assert_eq!(
        finding_name(f),
        "delta_op",
        "the offender must be `delta_op`; got summary {:?}",
        f.summary
    );
    assert!(
        f.evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::File { path } if path == FIX_UNITS)),
        "evidence must point at the registry source {FIX_UNITS:?}; got {:?}",
        f.evidence
    );
}

/// PRD leaf γ contract: the baseline file "may be empty/absent at this stage".
/// Absent → no error, no panic, and the previously-baselined name simply joins
/// the offender set.
#[test]
fn omission_lane_tolerates_an_absent_baseline_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(root, FIX_UNITS, FOUR_WAY_UNITS);
    write_file(root, FIX_CHUNK, ALPHA_CHUNK);
    // No baseline file written, and none tracked.

    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    let names: Vec<&str> = findings.iter().map(finding_name).collect();
    assert_eq!(
        names,
        vec!["delta_op", "gamma_op"],
        "with the baseline absent, `gamma_op` loses its exemption and joins \
         `delta_op`; the documented and allow-marked names stay exempt. \
         Got {findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|f| f.summary.starts_with("undocumented-name:")),
        "both findings are omission offenders; got {findings:?}"
    );
}

/// An EMPTY baseline file is the other half of the same contract — it must
/// behave exactly like an absent one, not like a parse error.
#[test]
fn omission_lane_tolerates_an_empty_baseline_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(root, FIX_UNITS, FOUR_WAY_UNITS);
    write_file(root, FIX_CHUNK, ALPHA_CHUNK);
    write_file(root, FIX_BASELINE, "");

    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK, FIX_BASELINE]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    let names: Vec<&str> = findings.iter().map(finding_name).collect();
    assert_eq!(
        names,
        vec!["delta_op", "gamma_op"],
        "an empty baseline exempts nothing; got {findings:?}"
    );
}

/// Findings are deterministically ordered by (category, name) — not by
/// declaration order, and not by whatever order the registry walk happened to
/// visit. A detector whose output reorders between runs cannot be diffed, and
/// #5480's baseline regenerator consumes this ordering directly.
#[test]
fn omission_findings_are_deterministically_ordered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Declaration order is deliberately anti-sorted, and spans two registries
    // so cross-registry ordering is exercised too.
    write_file(
        root,
        FIX_UNITS,
        "\
pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    \"zulu_op\",
    \"mike_op\",
];

pub const DYNAMICS_QUERY_NAMES: &[&str] = &[
    \"alpha_op\",
    \"bravo_op\",
];
",
    );

    let h = Harness::new(&[FIX_UNITS]);
    let ctx = h.ctx(root);
    let findings = reify_audit::pdoccover::check(&ctx);

    let names: Vec<&str> = findings.iter().map(finding_name).collect();
    assert_eq!(
        names,
        vec!["alpha_op", "bravo_op", "mike_op", "zulu_op"],
        "findings must be name-sorted within a category, across registries and \
         irrespective of declaration order; got {findings:?}"
    );

    // (category, name) is the full key: the category prefixes must be
    // non-decreasing, so a future category never interleaves with this one.
    let cats: Vec<&str> = findings.iter().map(finding_category).collect();
    let mut sorted_cats = cats.clone();
    sorted_cats.sort_unstable();
    assert_eq!(
        cats, sorted_cats,
        "category prefixes must be non-decreasing across the finding list; \
         got {cats:?}"
    );

    // Re-running over the same tree must reproduce the list byte-for-byte.
    let again = reify_audit::pdoccover::check(&ctx);
    assert_eq!(
        again.iter().map(|f| f.summary.clone()).collect::<Vec<_>>(),
        findings.iter().map(|f| f.summary.clone()).collect::<Vec<_>>(),
        "two runs over an unchanged tree must produce identical output"
    );
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
