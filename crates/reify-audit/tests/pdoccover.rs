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

use reify_audit::pdoccover::{BASELINE_PATH, UNITS_PATH};
use reify_audit::{
    AuditContext, EvidenceRef, Finding, MockGitOps, MockJCodemunchOps, Pattern, Severity,
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Write `content` to relative `path` inside `root`, creating parent dirs.
fn write_file(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write_file");
}

/// Repo root, resolved from `CARGO_MANIFEST_DIR` (= `crates/reify-audit`).
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
struct Harness {
    conn: Connection,
    git: MockGitOps,
    jc: MockJCodemunchOps,
}

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
const FIX_UNITS: &str = "crates/reify-compiler/src/units.rs";
const FIX_CHUNK: &str = "crates/reify-mcp/src/tools/chunks/geometry.md";
const FIX_BASELINE: &str = "crates/reify-audit/pdoccover-baseline.txt";

/// The summary body after a `<category>: ` prefix, up to the first space —
/// i.e. the offending name a finding is about.
fn finding_name(f: &Finding) -> &str {
    let after = f
        .summary
        .split_once(": ")
        .map(|(_, rest)| rest)
        .unwrap_or(&f.summary);
    after.split_whitespace().next().unwrap_or(after)
}

/// The `<category>` prefix of a finding summary.
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

/// A units file whose registry mixes identifier names with the unit SYMBOLS a
/// units file plausibly carries — one of which occurs non-boundary-delimited in
/// the chunk below, which is the shape that used to take the whole detector
/// down with `byte index is not a char boundary`.
const NON_ASCII_UNITS: &str = "\
//! Fixture registry mixing builtin names with non-identifier unit symbols.

pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    \"alpha_op\",
    \"µm\",
    \"°C\",
    \"m/s\",
    \"delta_op\",
];
";

/// Prose that mentions the multibyte tokens ONLY without word boundaries — the
/// retry path — plus the `§`/`→` a real chunk carries.
const NON_ASCII_CHUNK: &str = "\
# Geometry §3 — tolerances

- `alpha_op(shape, amount)` — the alpha operation.
- Machining tolerance is 5µm at 20°C → tighter than cast.
";

/// A non-ASCII corpus must be scanned, not crashed on, and must not put an
/// unsatisfiable entry into the ratchet.
///
/// Two defects met here: `contains_word`'s boundary-rejected retry stepped one
/// BYTE, so a multibyte census name plus one non-boundary occurrence panicked
/// mid-scan; and the census admitted non-identifier tokens at all, so `µm`
/// would have become an `undocumented-name:` that no chunk edit could satisfy
/// (the ASCII boundary alphabet can never report such a token documented).
/// `check()`'s contract is "unreadable files are skipped fail-safe (no finding,
/// no panic)" — a panic reachable from corpus CONTENT breaks it just as badly.
#[test]
fn a_non_ascii_registry_and_chunk_are_scanned_not_panicked_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(root, FIX_UNITS, NON_ASCII_UNITS);
    write_file(root, FIX_CHUNK, NON_ASCII_CHUNK);

    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    let names: Vec<&str> = findings.iter().map(finding_name).collect();
    assert_eq!(
        names,
        vec!["delta_op"],
        "only the identifier-shaped undocumented name may be reported: the \
         multibyte and punctuated tokens are not documentable builtin call \
         names and must never enter the census. Got {findings:?}"
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
    // so cross-registry ordering is exercised too. `kilo_op` carries a
    // reasonless marker and `eta_op` is documented-but-still-baselined, so the
    // finding list spans THREE categories — without them the category half of
    // the (category, name) sort key would be unexercised, and the assertion
    // below would compare a constant vector to itself.
    write_file(
        root,
        FIX_UNITS,
        "\
pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    \"zulu_op\",
    \"kilo_op\", // pdoccover:allow
    \"mike_op\",
    \"eta_op\",
];

pub const DYNAMICS_QUERY_NAMES: &[&str] = &[
    \"alpha_op\",
    \"bravo_op\",
];
",
    );
    write_file(root, FIX_CHUNK, "# Geometry\n\n- `eta_op(shape)` — documented.\n");
    write_file(root, FIX_BASELINE, "eta_op\n");

    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK, FIX_BASELINE]);
    let ctx = h.ctx(root);
    let findings = reify_audit::pdoccover::check(&ctx);

    // (category, name), fully: `allow-missing-reason` < `stale-baseline-entry`
    // < `undocumented-name` lexicographically, and names sort within each.
    let pairs: Vec<(&str, &str)> = findings
        .iter()
        .map(|f| (finding_category(f), finding_name(f)))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("allow-missing-reason", "kilo_op"),
            ("stale-baseline-entry", "eta_op"),
            ("undocumented-name", "alpha_op"),
            ("undocumented-name", "bravo_op"),
            ("undocumented-name", "mike_op"),
            ("undocumented-name", "zulu_op"),
        ],
        "findings must sort by (category, name) — categories grouped in \
         lexicographic order, names sorted within each group, across \
         registries and irrespective of declaration order; got {findings:?}"
    );

    // The category half of the key, asserted against a fixture that actually
    // spans several categories rather than a single-category constant.
    let cats: Vec<&str> = findings.iter().map(finding_category).collect();
    let distinct: std::collections::BTreeSet<&str> = cats.iter().copied().collect();
    assert!(
        distinct.len() >= 3,
        "fixture sanity — the ordering assertion is only meaningful if the \
         finding list spans several categories; got {distinct:?}"
    );
    let mut sorted_cats = cats.clone();
    sorted_cats.sort_unstable();
    assert_eq!(
        cats, sorted_cats,
        "category prefixes must be non-decreasing across the finding list, so \
         a future category never interleaves with an existing one; got {cats:?}"
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
// step-15: FABRICATION direction — a chunk that documents a call the compiler
// does not provide
//
// This is the direction that costs a design-authoring agent the most: it reads
// the chunk, writes the call, and the compile fails on something the reference
// promised. Every disposition here is pinned in a TEMPDIR, never against the
// real tree — `offset_surface` has since left the real stdlib.md, and a test
// that pinned it there would have flipped RED for an unrelated reason.
// ─────────────────────────────────────────────────────────────────────────────

/// A second chunk path, so the fabrication fixtures do not collide with the
/// omission fixtures' `geometry.md`.
const FIX_STDLIB_CHUNK: &str = "crates/reify-mcp/src/tools/chunks/stdlib.md";
/// `.ri` stdlib source — the oracle arm that has no Rust literal to harvest.
const FIX_RI: &str = "crates/reify-compiler/stdlib/geometry.ri";

/// The chunk under test. Line numbers are load-bearing for the assertions:
/// 3 `extrude`, 4 `revolve`, 5 `offset_surface`, 6 `planned_op`,
/// 7 `sketchy_op`.
const FABRICATION_CHUNK: &str = "\
# Stdlib

- `extrude(profile, height)` — real: declared in a `*_NAMES` registry.
- `revolve(profile, axis, angle)` — real: declared ONLY in the `.ri` stdlib.
- `offset_surface(surface, distance)` — fabricated: exists nowhere.
- `planned_op(x)` <!-- pdoccover:allow — planned, see #5434 -->
- `sketchy_op(x)` <!-- pdoccover:allow -->
";

/// The fabrication lane over a tree where every disposition is represented.
///
/// Exactly two findings survive: the bare fabrication, and the phantom whose
/// escape hatch has no reason. The oracle-backed names and the properly
/// allow-marked phantom produce nothing.
#[test]
fn fabrication_lane_reports_names_that_exist_nowhere() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Oracle arm 1: a Rust registry literal.
    write_file(
        root,
        FIX_UNITS,
        "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n    \"extrude\",\n];\n",
    );
    // Oracle arm 2: a `.ri` declaration with no Rust literal anywhere.
    write_file(
        root,
        FIX_RI,
        "pub fn revolve(profile: Profile, axis: Axis, angle: Angle) -> Solid { x }\n",
    );
    write_file(root, FIX_STDLIB_CHUNK, FABRICATION_CHUNK);

    let h = Harness::new(&[FIX_UNITS, FIX_RI, FIX_STDLIB_CHUNK]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    let names: Vec<&str> = findings.iter().map(finding_name).collect();
    assert_eq!(
        names,
        vec!["sketchy_op", "offset_surface"],
        "expected exactly the reasonless-marker phantom and the bare \
         fabrication, in (category, name) order. `extrude` is oracle-backed by \
         a registry literal, `revolve` ONLY by the `.ri` stdlib arm, and \
         `planned_op` carries a well-formed allow marker. Got {findings:?}"
    );

    // (b) the fabrication itself
    let fab = findings
        .iter()
        .find(|f| finding_name(f) == "offset_surface")
        .expect("offset_surface finding");
    assert!(
        fab.summary.starts_with("fabricated-name:"),
        "got {:?}",
        fab.summary
    );
    assert_eq!(fab.severity, Severity::High, "got {:?}", fab.severity);
    assert!(
        fab.summary.contains(&format!("{FIX_STDLIB_CHUNK}:5")),
        "the summary must locate the claim precisely — chunk path AND line, so \
         the fix is one jump away; got {:?}",
        fab.summary
    );
    assert!(
        fab.evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::File { path } if path == FIX_STDLIB_CHUNK)),
        "evidence must point at the CHUNK (the file that lies), not at the \
         compiler; got {:?}",
        fab.evidence
    );

    // (d) the reasonless chunk-side marker
    let bad_marker = findings
        .iter()
        .find(|f| finding_name(f) == "sketchy_op")
        .expect("sketchy_op finding");
    assert!(
        bad_marker.summary.starts_with("allow-missing-reason:"),
        "a reasonless chunk-side marker suppresses nothing and is itself the \
         finding; got {:?}",
        bad_marker.summary
    );
    assert!(
        bad_marker
            .evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::File { path } if path == FIX_STDLIB_CHUNK)),
        "got {:?}",
        bad_marker.evidence
    );
}

/// The `allow-missing-reason:` verdict must not be narrowed by any
/// MENTION-SIDE filter.
///
/// `fabrication_findings` reports a malformed marker keyed on a name, and the
/// obvious way to find that name is to read `chunk_call_mentions`' output for
/// the marked line. That coupling is a trap: every filter #5647 added there
/// (`.`/`@` left delimiter, `RI_KEYWORDS`, file-scoped `ri_declared_name`,
/// elided `(...)`) can drop the ONLY call-shaped token on a marked line, and a
/// marker that yields no name yields no finding. The erosion is silent — the
/// lane reports clean, nothing goes RED — and `allow-missing-reason:` is one of
/// the four categories #5480 hard-gates, so PRD design decision 7's guarantee
/// that "the escape hatch can never become un-reviewable" would quietly stop
/// holding as the mention side got more precise.
///
/// One case per filter. Each chunk also documents `extrude`, which is the only
/// name in the fixture's registry, so the omission lane stays quiet and the
/// assertion below sees the marker verdict alone.
#[test]
fn reasonless_marker_survives_every_mention_side_filter() {
    for (filter, chunk, expected_name) in [
        (
            "RI_KEYWORDS membership (`auto` is a value literal, spec §2.10:207)",
            "# Params\n\n- `auto(free)` <!-- pdoccover:allow -->\n\
             - `extrude(profile, height)` — real.\n",
            "auto",
        ),
        (
            "`@` left delimiter (ad-hoc port/region designator, spec §D5)",
            "# Connect\n\n- `pipe@region(outer)` <!-- pdoccover:allow -->\n\
             - `extrude(profile, height)` — real.\n",
            "region",
        ),
        (
            "`.` left delimiter (member access)",
            "# Query\n\n- `solid.volume()` <!-- pdoccover:allow -->\n\
             - `extrude(profile, height)` — real.\n",
            "volume",
        ),
        (
            "elided `(...)` argument list (metavariable)",
            "# Geometry\n\n- `primitive(...)` <!-- pdoccover:allow -->\n\
             - `extrude(profile, height)` — real.\n",
            "primitive",
        ),
        (
            "file-scoped `ri_declared_name` (declared elsewhere in this chunk)",
            "# Traits\n\n```\nfn make_default() -> Self\n```\n\
             - `make_default(x)` <!-- pdoccover:allow -->\n\
             - `extrude(profile, height)` — real.\n",
            "make_default",
        ),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_file(
            root,
            FIX_UNITS,
            "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n    \"extrude\",\n];\n",
        );
        write_file(root, FIX_STDLIB_CHUNK, chunk);

        let h = Harness::new(&[FIX_UNITS, FIX_STDLIB_CHUNK]);
        let findings = reify_audit::pdoccover::check(&h.ctx(root));

        let got: Vec<(&str, &str)> = findings
            .iter()
            .map(|f| (finding_category(f), finding_name(f)))
            .collect();
        assert_eq!(
            got,
            vec![("allow-missing-reason", expected_name)],
            "[{filter}] a reasonless marker must stay reportable even though \
             this filter drops the only call-shaped token on its line. If this \
             is RED, `fabrication_findings`' pre-pass has been re-coupled to \
             `chunk_call_mentions`' narrowed output and the escape hatch is \
             now un-reviewable on those lines. Got {findings:?}"
        );
    }
}

/// A reasonless marker on a line the mention side drops must still SUBSUME the
/// fabrication verdict for that name elsewhere in the same file — the
/// precedence rule and the filter-independence rule have to hold together, not
/// just one at a time.
#[test]
fn reasonless_marker_on_a_filtered_line_still_subsumes_the_fabrication() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_file(
        root,
        FIX_UNITS,
        "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n    \"extrude\",\n];\n",
    );
    // `ghost_op(...)` is elided on the MARKED line (filter 5 drops it) and
    // plain two lines below, where it is a live fabrication.
    write_file(
        root,
        FIX_STDLIB_CHUNK,
        "# Stdlib\n\n- `ghost_op(...)` <!-- pdoccover:allow -->\n\
         - `ghost_op(x)` — plain mention, no marker.\n\
         - `extrude(profile, height)` — real.\n",
    );

    let h = Harness::new(&[FIX_UNITS, FIX_STDLIB_CHUNK]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    let got: Vec<(&str, &str)> = findings
        .iter()
        .map(|f| (finding_category(f), finding_name(f)))
        .collect();
    assert_eq!(
        got,
        vec![("allow-missing-reason", "ghost_op")],
        "exactly one finding, and it must be the malformed marker: the marker \
         is the defect to fix, and reporting the fabrication as well would \
         charge one mistake twice. Got {findings:?}"
    );
    assert!(
        findings[0].summary.contains(&format!("{FIX_STDLIB_CHUNK}:3")),
        "the summary must cite the MARKER's line (3), not the plain mention's \
         (4) — line 3 is the one the reader has to edit; got {:?}",
        findings[0].summary
    );
}

/// The `allow-missing-reason:` verdict SUBSUMES the fabrication verdict for the
/// same name in the same file REGARDLESS OF LINE ORDER.
///
/// `fabrication_findings` dedupes per (file, name), so a category-blind dedup
/// set would let whichever finding came first win. Mentions arrive in line
/// order, so with the marker BELOW the plain mention the fabrication would be
/// emitted first and the malformed marker would then be swallowed by the dedup
/// set and never reported at all — a hole in PRD design decision 7's guarantee
/// that the escape hatch can never become un-reviewable, opened purely by where
/// the marker sits in the file.
///
/// `fabrication_lane_reports_names_that_exist_nowhere` only exercises the
/// favourable order (its `sketchy_op` is mentioned once, on the marker line).
/// This pins BOTH orders, and pins that the reported line is the MARKER's —
/// the line the reader has to edit.
#[test]
fn reasonless_marker_wins_over_fabrication_in_either_line_order() {
    for (label, chunk, plain_line, marker_line) in [
        (
            "marker BELOW the plain mention",
            "# Stdlib\n\n- `ghost_op(x)` — plain mention, no marker.\n\n\
             - `ghost_op(y)` <!-- pdoccover:allow -->\n\
             - `extrude(profile, height)` — real; keeps the omission lane quiet.\n",
            3usize,
            5usize,
        ),
        (
            "marker ABOVE the plain mention",
            "# Stdlib\n\n- `ghost_op(y)` <!-- pdoccover:allow -->\n\n\
             - `ghost_op(x)` — plain mention, no marker.\n\
             - `extrude(profile, height)` — real; keeps the omission lane quiet.\n",
            5usize,
            3usize,
        ),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_file(
            root,
            FIX_UNITS,
            "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n    \"extrude\",\n];\n",
        );
        write_file(root, FIX_STDLIB_CHUNK, chunk);

        let h = Harness::new(&[FIX_UNITS, FIX_STDLIB_CHUNK]);
        let findings = reify_audit::pdoccover::check(&h.ctx(root));

        let cats: Vec<&str> = findings.iter().map(finding_category).collect();
        assert_eq!(
            cats,
            vec!["allow-missing-reason"],
            "[{label}] exactly one finding, and it must be the malformed \
             marker — the marker is the defect to fix and must never be \
             invisible because a plain mention happened to come first. Got \
             {findings:?}"
        );
        assert!(
            findings[0]
                .summary
                .contains(&format!("{FIX_STDLIB_CHUNK}:{marker_line}")),
            "[{label}] the reported line must be the MARKER's ({marker_line}), \
             not the plain mention's ({plain_line}) — that is the line the \
             reader has to edit. Got {:?}",
            findings[0].summary
        );
    }
}

/// The same fabricated name mentioned repeatedly in one chunk is ONE defect
/// and one finding — otherwise a name documented in a table, a fence and a
/// heading would cost three, and the ratchet count would track prose volume
/// instead of drift.
#[test]
fn fabrication_lane_dedupes_repeat_mentions_within_a_chunk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        FIX_UNITS,
        "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[];\n",
    );
    write_file(
        root,
        FIX_STDLIB_CHUNK,
        "\
## `offset_surface(surface, distance)`

| Call | Meaning |
|---|---|
| `offset_surface(s, d)` | offsets |

```reify
let x = offset_surface(s, 1mm)
```
",
    );

    let h = Harness::new(&[FIX_UNITS, FIX_STDLIB_CHUNK]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    assert_eq!(
        findings.len(),
        1,
        "three mentions of one fabricated name in one chunk are one finding; \
         got {}: {:?}",
        findings.len(),
        findings
    );
    assert!(
        findings[0].summary.contains(&format!("{FIX_STDLIB_CHUNK}:1")),
        "the finding must report the FIRST occurrence, so the reported line is \
         stable as later mentions come and go; got {:?}",
        findings[0].summary
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-9: ratchet honesty — a suppression channel that is no longer needed is
// itself a finding
//
// A ratchet that lets exemptions outlive their justification decays into a
// permanent allowlist: the count never moves, nobody notices, and the detector
// reports clean while the debt is untouched. These four cases are the fail-safe
// against that — the same lesson `ptodo-baseline-gen` (PRD §6.6) paid for.
// ─────────────────────────────────────────────────────────────────────────────

/// (a) A baselined name that has since been documented must be reported so the
/// stale entry gets deleted — otherwise the baseline silently accumulates
/// names that no longer need it and stops meaning "residual debt".
///
/// Evidence points at the BASELINE file, because that is the file to edit.
#[test]
fn baselined_name_that_is_documented_is_a_stale_baseline_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        FIX_UNITS,
        "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n    \"alpha_op\",\n];\n",
    );
    write_file(root, FIX_CHUNK, ALPHA_CHUNK);
    write_file(root, FIX_BASELINE, "alpha_op\n");

    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK, FIX_BASELINE]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    assert_eq!(
        findings.len(),
        1,
        "expected exactly 1 finding (the now-redundant baseline entry); \
         got {}: {:?}",
        findings.len(),
        findings
    );
    let f = &findings[0];
    assert!(
        f.summary.starts_with("stale-baseline-entry:"),
        "a documented name that is still baselined must be reported under the \
         `stale-baseline-entry:` category; got {:?}",
        f.summary
    );
    assert_eq!(finding_name(f), "alpha_op", "got summary {:?}", f.summary);
    assert_eq!(
        f.severity,
        Severity::High,
        "ratchet-honesty findings ride at High like the rest; got {:?}",
        f.severity
    );
    assert!(
        f.evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::File { path } if path == FIX_BASELINE)),
        "evidence must point at the baseline file {FIX_BASELINE:?} — the file \
         that needs the edit; got {:?}",
        f.evidence
    );
}

/// (b) The allow-marker half of the same contract: an escape hatch on a name
/// that is now documented is dead weight and must be reported.
///
/// Evidence points at the REGISTRY source, where the marker lives.
#[test]
fn allow_marked_name_that_is_documented_is_a_stale_allow_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        FIX_UNITS,
        "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n    \
         \"alpha_op\", // pdoccover:allow — internal lowering shim\n];\n",
    );
    write_file(root, FIX_CHUNK, ALPHA_CHUNK);

    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    assert_eq!(
        findings.len(),
        1,
        "expected exactly 1 finding (the now-redundant allow marker); got {}: {:?}",
        findings.len(),
        findings
    );
    let f = &findings[0];
    assert!(
        f.summary.starts_with("stale-allow-entry:"),
        "a documented name that still carries `pdoccover:allow` must be \
         reported under the `stale-allow-entry:` category; got {:?}",
        f.summary
    );
    assert_eq!(finding_name(f), "alpha_op", "got summary {:?}", f.summary);
    assert_eq!(f.severity, Severity::High, "got {:?}", f.severity);
    assert!(
        f.evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::File { path } if path == FIX_UNITS)),
        "evidence must point at the registry source {FIX_UNITS:?}, where the \
         marker lives; got {:?}",
        f.evidence
    );
}

/// (c) PRD design decision 7 — the escape hatch may never become
/// un-reviewable. A `pdoccover:allow` with no reason body (bare token, or a
/// separator with nothing after it) is itself the finding, and confers NO
/// exemption.
///
/// Exactly one finding per name: `allow-missing-reason:` SUBSUMES the
/// undocumented report rather than duplicating it, so fixing the marker is one
/// edit against one finding.
#[test]
fn reasonless_allow_marker_is_reported_and_confers_no_exemption() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        FIX_UNITS,
        "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n    \
         \"epsilon_op\", // pdoccover:allow\n    \
         \"zeta_op\", // pdoccover:allow —\n];\n",
    );
    // No chunks at all: both names are undocumented on the merits.

    let h = Harness::new(&[FIX_UNITS]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    let cats: Vec<&str> = findings.iter().map(finding_category).collect();
    let names: Vec<&str> = findings.iter().map(finding_name).collect();

    assert_eq!(
        names,
        vec!["epsilon_op", "zeta_op"],
        "both reasonless markers must be reported, each exactly once — a bare \
         token and an empty body after the separator are the same defect. \
         Got {findings:?}"
    );
    assert!(
        cats.iter().all(|c| *c == "allow-missing-reason"),
        "a reasonless marker is an `allow-missing-reason:` finding; got \
         categories {cats:?} in {findings:?}"
    );
    assert!(
        !findings
            .iter()
            .any(|f| f.summary.starts_with("undocumented-name:")),
        "the `allow-missing-reason:` finding SUBSUMES the undocumented report \
         for the same name — a duplicate would make one defect cost two \
         findings and inflate the ratchet count. Got {findings:?}"
    );
    assert!(
        findings.iter().all(|f| f.severity == Severity::High),
        "got {findings:?}"
    );
}

/// (d) The 2026-07-25 rename is one-way: the legacy unprefixed
/// `doccover:allow` is consumed by NO code path.
///
/// If it silently kept working, a stale marker written against an earlier PRD
/// draft would suppress a real gap forever with nothing to show it.
#[test]
fn legacy_unprefixed_doccover_allow_confers_no_exemption() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        FIX_UNITS,
        "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n    \
         \"theta_op\", // doccover:allow — legacy unprefixed token\n];\n",
    );

    let h = Harness::new(&[FIX_UNITS]);
    let findings = reify_audit::pdoccover::check(&h.ctx(root));

    assert_eq!(
        findings.len(),
        1,
        "expected exactly 1 finding — the legacy token grants nothing; \
         got {}: {:?}",
        findings.len(),
        findings
    );
    let f = &findings[0];
    assert!(
        f.summary.starts_with("undocumented-name:"),
        "an undocumented name carrying only the legacy unprefixed \
         `doccover:allow` must surface as `undocumented-name:` — not be \
         exempted, and not be reported as a malformed prefixed marker. \
         Got {:?}",
        f.summary
    );
    assert_eq!(finding_name(f), "theta_op", "got summary {:?}", f.summary);
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

/// `*_NAMES` declarations in the real `units.rs` that `extract_registries`
/// legitimately does not model, each with the reason it is excluded.
///
/// EMPTY today: all 12 production registries are plain `pub const X_NAMES:
/// &[&str]`, and the three `#[cfg(test)]` fixtures are outside the code view.
/// An entry belongs here only when a human has classified the declaration as
/// something other than a builtin-name registry (a tuple slice, an alias of
/// another registry whose names are already censused). Adding an entry to
/// silence a shape the grammar SHOULD read is the false-clean move assertion
/// (v) exists to make impossible — widen `registry_header` instead.
const KNOWN_UNDISCOVERED_REGISTRY_IDENTS: &[&str] = &[];

/// `extract_registries` over the REAL `crates/reify-compiler/src/units.rs`
/// must (i) find every PRD-named registry, (ii) find a non-empty entry list
/// for every registry it discovers, (iii) clear a conservative distinct-name
/// floor, (iv) yield only identifier-shaped names, and (v) leave no
/// declaration-shaped `*_NAMES` ident undiscovered and unclassified.
///
/// Purpose: a `units.rs` refactor (a new declaration shape, a re-indent, a
/// macro, an attribute) that breaks extraction fails RED here rather than
/// yielding a silently false-clean coverage result.
///
/// (i)–(iv) all defend registries that ARE discovered; only (v) defends
/// against a registry that is never discovered at all — which is invisible to
/// the others, since a missing registry makes nothing empty and nothing
/// malformed, it just shrinks the census.
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

    // (v) Completeness: every declaration-shaped `*_NAMES` ident in production
    // scope is either discovered or explicitly classified. Without this, a
    // registry written in a shape `registry_header` does not model — a
    // `static`, an unusual element type, anything future — leaves the census
    // silently and PDOCCOVER reports clean on every name in it forever.
    let declared = reify_audit::pdoccover::declared_registry_idents(&src);
    // Non-vacuity FIRST: a cross-check that scanned nothing would otherwise
    // report nothing missing and pass silently — the same shape of
    // false-clean it exists to catch.
    let unseen: Vec<&str> = found
        .iter()
        .copied()
        .filter(|f| !declared.iter().any(|d| d == f))
        .collect();
    assert!(
        unseen.is_empty(),
        "the completeness oracle did not see {} registr{} that \
         `extract_registries` DID discover: {unseen:?}. \
         `declared_registry_idents` has gone blind, so assertion (v) below \
         would pass vacuously.",
        unseen.len(),
        if unseen.len() == 1 { "y" } else { "ies" },
    );

    let undiscovered: Vec<String> = declared
        .into_iter()
        .filter(|ident| !found.contains(&ident.as_str()))
        .filter(|ident| !KNOWN_UNDISCOVERED_REGISTRY_IDENTS.contains(&ident.as_str()))
        .collect();
    assert!(
        undiscovered.is_empty(),
        "these `*_NAMES` declarations in the real units.rs were NOT discovered \
         as registries: {undiscovered:?}. Every name in them has silently left \
         the omission census. Either widen `registry_header` to read the shape \
         (the usual answer), or — if a human has classified the declaration as \
         something other than a builtin-name registry — record it in \
         KNOWN_UNDISCOVERED_REGISTRY_IDENTS with the reason."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-17: brittle-parse floor guard — CHUNK scan path
// ─────────────────────────────────────────────────────────────────────────────

/// Call-shaped names whose presence in the chunk corpus is structural to the
/// language reference rather than incidental to any one task's content edits.
///
/// Chosen deliberately: `union` (boolean CSG), `extrude` and `fillet` are core
/// modelling verbs — a language reference that documented none of them would
/// not be a language reference. Nothing here names `offset_surface` or any
/// other name a concurrent chunk edit (#5434, #5347, #5389) is touching, so a
/// sibling merge can never flip this guard RED.
const CHUNK_MENTION_ANCHORS: &[&str] = &["union", "extrude", "fillet"];

/// Conservative floor on the distinct call-shaped census over the real chunk
/// corpus. 93 distinct names, drawn from 12 of the 17 chunk files, are
/// extracted at HEAD=4fdfd18513 — an ancestor of this commit, so the run is
/// reproducible from branch history. That is a DATED MEASUREMENT, not an
/// invariant, and nothing asserts it: the count moves whenever chunk content
/// or mention-side precision changes, so cite a commit that is actually on
/// this branch whenever it is re-taken, or state no number at all. The floor
/// sits far below it, so ordinary chunk edits — including whole-file rewrites
/// of the smaller chunks — never flip this RED. Only an extraction regression
/// that collapses the census does.
const CHUNK_MENTION_FLOOR: usize = 30;

/// The census must span more than one chunk file. A single-file census is the
/// signature of an extractor that only survives one file's formatting.
const CHUNK_MENTION_FILE_FLOOR: usize = 3;

/// `chunk_call_mentions` over the REAL `crates/reify-mcp/src/tools/chunks/*.md`
/// must (i) clear a conservative distinct-name floor, (ii) draw those names
/// from several distinct chunk files, (iii) contain the structural anchors, and
/// (iv) report only identifier-shaped names at in-range 1-based line numbers.
///
/// Purpose — the fabrication half of task requirement 3. The fabrication lane
/// reports a name only when it IS mentioned in a chunk, so an extractor that
/// goes blind reports *clean*: a heading, fence, table or list reformat that
/// broke extraction would look exactly like a chunk corpus with no
/// fabrications left. This guard converts that silent false-clean into a RED
/// test. Freeze no exact count here — a count assertion would be flipped by
/// every ordinary chunk edit and would be relaxed away within a week.
#[test]
fn chunk_call_mention_floor_guard_against_real_chunks() {
    let dir = repo_root().join(reify_audit::pdoccover::CHUNKS_PREFIX);
    let read = std::fs::read_dir(&dir).unwrap_or_else(|e| {
        panic!(
            "the real chunk corpus must be readable at {}; got: {e}. \
             (If the chunks moved, PDOCCOVER's fabrication census moved with \
             them and pdoccover::CHUNKS_PREFIX needs updating.)",
            dir.display()
        )
    });
    let mut chunk_files: Vec<PathBuf> = read
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    chunk_files.sort();
    assert!(
        !chunk_files.is_empty(),
        "no `*.md` chunk files under {} — the fabrication lane has no corpus \
         to scan and would report clean unconditionally.",
        dir.display()
    );

    // name -> the chunk file names that mention it.
    let mut census: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    let mut bad_lines: Vec<String> = Vec::new();
    let mut malformed: Vec<String> = Vec::new();

    for path in &chunk_files {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("chunk {} must be readable; got: {e}", path.display()));
        let total_lines = content.lines().count();
        let file = path
            .file_name()
            .expect("chunk path has a file name")
            .to_string_lossy()
            .to_string();
        for (name, line) in reify_audit::pdoccover::chunk_call_mentions(&content) {
            if line == 0 || line > total_lines {
                bad_lines.push(format!("{file}:{line} (file has {total_lines} lines)"));
            }
            if name.is_empty()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || name.as_bytes()[0].is_ascii_digit()
            {
                malformed.push(format!("{file}:{line} {name:?}"));
            }
            census.entry(name).or_default().insert(file.clone());
        }
    }

    let files_with_mentions: std::collections::BTreeSet<&String> =
        census.values().flatten().collect();

    // (iii) computed first so every failure message below can name it.
    let missing: Vec<&str> = CHUNK_MENTION_ANCHORS
        .iter()
        .copied()
        .filter(|a| !census.contains_key(*a))
        .collect();

    // (i) Distinct census clears the floor.
    assert!(
        census.len() >= CHUNK_MENTION_FLOOR,
        "distinct call-shaped chunk-mention census is {} over {} chunk file(s), \
         below the conservative floor of {CHUNK_MENTION_FLOOR}; missing \
         anchors: {missing:?}. Chunk extraction has regressed — a reformat \
         (headings, fences, tables, bold-prefixed lists) broke \
         `chunk_call_mentions`. A blind extractor makes the fabrication lane \
         report CLEAN, so fix the extractor; do NOT relax this floor.",
        census.len(),
        chunk_files.len(),
    );

    // (ii) Mentions come from several files, not just the one whose shape the
    // extractor happens to survive.
    assert!(
        files_with_mentions.len() >= CHUNK_MENTION_FILE_FLOOR,
        "call-shaped mentions were found in only {} chunk file(s) \
         ({files_with_mentions:?}) out of {}; census size {}, missing anchors \
         {missing:?}. Extraction survives one file's formatting and not the \
         rest — that is a partial blind spot in the fabrication lane.",
        files_with_mentions.len(),
        chunk_files.len(),
        census.len(),
    );

    // (iii) Structural anchors present.
    assert!(
        missing.is_empty(),
        "the chunk-mention census is missing {} structural anchor(s): \
         {missing:?}. Observed census size {} across {} chunk file(s). These \
         are core modelling verbs the language reference cannot plausibly have \
         dropped, so their absence means `chunk_call_mentions` stopped seeing \
         the shape they are written in.",
        missing.len(),
        census.len(),
        files_with_mentions.len(),
    );

    // (iv) Well-formed output: 1-based in-range lines, identifier-shaped names.
    assert!(
        bad_lines.is_empty(),
        "these mentions carry out-of-range line numbers: {bad_lines:?}. \
         Findings would cite lines that do not exist."
    );
    assert!(
        malformed.is_empty(),
        "these extracted 'names' are not identifier-shaped: {malformed:?}. \
         A non-call token leaked into the census and would be reported as a \
         fabricated name no chunk edit could ever satisfy."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// #5647 step-3(d): keyword-vs-builtin collision guard
// ─────────────────────────────────────────────────────────────────────────────

/// Every `*_NAMES` registry file this guard reads. Not the fabrication lane's
/// full oracle — see the guard's doc comment for why that is deliberate.
///
/// `units.rs` is the omission census's own source. `math_signatures.rs` is the
/// other half of the registered-builtin population and is NOT in the omission
/// census at all: `clamp`, `lerp` and `dot` are real, documented builtins
/// declared only there (module header, "The existence oracle is deliberately
/// asymmetric"), so a keyword colliding with one of them would be invisible to
/// a units.rs-only guard.
const REGISTRY_CENSUS_SOURCES: &[&str] = &[
    "crates/reify-compiler/src/units.rs",
    "crates/reify-compiler/src/math_signatures.rs",
];

/// `RI_KEYWORDS` must never intersect the registered-builtin census drawn from
/// [`REGISTRY_CENSUS_SOURCES`]. A token that is BOTH a reserved word and a
/// registered builtin name would be silently dropped from every chunk claim by
/// `chunk_call_mentions`'s keyword filter, blinding the fabrication lane to
/// that builtin — a false negative with no bound, which is exactly the risk of
/// widening the mention-side filter from `RI_DECL_KEYWORDS` to the full
/// reserved-word set. Measured empty today (47 keywords against both files'
/// `*_NAMES` registries). If this ever fails, remove the colliding token from
/// `RI_KEYWORDS` — do NOT delete this guard.
///
/// ## Scope: the `*_NAMES` registries, deliberately NOT the whole oracle
///
/// This is a REGISTRY census, not `known_name_index` over
/// `load_oracle_sources`. Widening it to the latter was measured and rejected:
/// the oracle deliberately harvests every identifier-shaped quoted literal
/// under `crates/reify-compiler/src/**` and `crates/reify-stdlib/src/**`, so
/// keyword STRINGS in the parser's own tables register as "builtins" and the
/// guard reports ~29 spurious collisions — noise that would get the guard
/// deleted rather than a keyword removed. The `*_NAMES` registries are the
/// population where a collision is genuinely actionable, so they are what this
/// guard covers; a builtin declared outside them is out of its reach and is
/// covered instead by `ri_keywords_excludes_the_spec_carve_outs` on the
/// spec-facing side.
#[test]
fn ri_keywords_never_collides_with_the_real_registry_census() {
    let mut census: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for rel in REGISTRY_CENSUS_SOURCES {
        let path = repo_root().join(rel);
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "the real {rel} must be readable at {}; got: {e}. (If it moved, \
                 the registered-builtin population moved with it and this \
                 guard's source list needs updating — do not just drop the \
                 file from the list.)",
                path.display()
            )
        });
        let regs = reify_audit::pdoccover::extract_registries(&src);
        assert!(
            !regs.is_empty(),
            "no `*_NAMES` registry was parsed out of {rel}. This guard would \
             then pass vacuously for that file, so a keyword/builtin collision \
             in it would go unnoticed. Fix the parse; do not relax this."
        );
        census.extend(regs.iter().flat_map(|r| r.entries.iter()).map(|e| e.name.clone()));
    }

    let collisions: Vec<&str> = reify_audit::pdoccover::RI_KEYWORDS
        .iter()
        .copied()
        .filter(|kw| census.contains(*kw))
        .collect();
    assert!(
        collisions.is_empty(),
        "{collisions:?} are BOTH RI_KEYWORDS members AND registered builtin \
         names in the real {REGISTRY_CENSUS_SOURCES:?} census. Every chunk \
         claim naming one of them is silently dropped by the mention-side \
         keyword filter, blinding the fabrication lane to that builtin. Remove \
         the colliding token(s) from RI_KEYWORDS — do NOT delete this guard."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-19: shared-derivation contract — baseline_candidates() vs check()
// ─────────────────────────────────────────────────────────────────────────────

/// A registry exercising every disposition at once, so the contract is proved
/// against a mixed tree rather than an all-offenders one:
///
/// - `alpha_op` — documented in a chunk → not a candidate;
/// - `beta_op`  — well-formed allow marker → not a candidate;
/// - `gamma_op` — baselined already → not a candidate;
/// - `delta_op`, `epsilon_op` — bare → candidates;
/// - `zeta_op`  — reasonless marker → an `allow-missing-reason:` finding, and
///   deliberately NOT a candidate: adding it to the baseline would freeze a
///   malformed marker into the ratchet instead of fixing it;
/// - `eta_op`   — documented but still allow-marked → `stale-allow-entry:`,
///   also not a candidate.
///
/// Declared deliberately out of alphabetical order so a `check()` that happens
/// to emit in declaration order cannot pass the same-order assertion by luck.
const MIXED_UNITS: &str = "\
//! Fixture registry for the PDOCCOVER shared-derivation contract.

pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    \"epsilon_op\",
    \"alpha_op\",
    \"zeta_op\", // pdoccover:allow
    \"delta_op\",
    \"eta_op\", // pdoccover:allow — documented, so this marker is stale
    \"beta_op\", // pdoccover:allow — internal lowering shim
    \"gamma_op\",
];
";

/// Documents `alpha_op` and `eta_op` only.
const MIXED_CHUNK: &str = "\
# Geometry

- `alpha_op(shape, amount)` — the alpha operation.
- `eta_op(shape)` — the eta operation.
";

/// `baseline_candidates()` must return EXACTLY the names `check()` reports as
/// `undocumented-name:` — same set, same order, no duplicates.
///
/// This is the seam #5480 consumes: its regenerator writes
/// `pdoccover-baseline.txt` from this list, and the ratchet compares that file
/// against `check()`'s findings. If the two derivations could disagree by even
/// one name, a freshly regenerated baseline would immediately fail its own
/// ratchet — the failure mode PRD §6.6 records from `ptodo-baseline-gen`. One
/// derivation, two callers.
#[test]
fn baseline_candidates_match_the_undocumented_name_findings_exactly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(root, FIX_UNITS, MIXED_UNITS);
    write_file(root, FIX_CHUNK, MIXED_CHUNK);
    write_file(root, FIX_BASELINE, "# ratchet baseline\n\ngamma_op\n");

    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK, FIX_BASELINE]);
    let ctx = h.ctx(root);

    let findings = reify_audit::pdoccover::check(&ctx);
    let from_check: Vec<String> = findings
        .iter()
        .filter(|f| f.summary.starts_with("undocumented-name:"))
        .map(|f| finding_name(f).to_string())
        .collect();
    let candidates = reify_audit::pdoccover::baseline_candidates(&ctx);

    // Sanity: the fixture must actually exercise the lane, or the equality
    // below would be trivially satisfied by two empty vectors.
    assert_eq!(
        from_check,
        vec!["delta_op".to_string(), "epsilon_op".to_string()],
        "fixture sanity — `check()` must report exactly the two bare names as \
         `undocumented-name:`. Got {from_check:?} from findings {:?}",
        findings
            .iter()
            .map(|f| f.summary.as_str())
            .collect::<Vec<_>>()
    );

    // The contract: identical content AND identical order.
    assert_eq!(
        candidates, from_check,
        "`baseline_candidates()` and `check()`'s `undocumented-name:` findings \
         must be the same list in the same order — they are one derivation with \
         two callers. #5480's regenerator writes the former and the ratchet \
         compares the latter; any divergence makes a freshly generated baseline \
         fail its own ratchet."
    );

    // Sorted and deduped — a baseline file is line-diffed, so order must not
    // depend on `units.rs` declaration order (the fixture declares them
    // out of order on purpose).
    let mut sorted = candidates.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        candidates, sorted,
        "`baseline_candidates()` must be sorted and deduped so the generated \
         baseline file diffs cleanly; got {candidates:?}"
    );
}

/// The exclusions, asserted by name rather than by count, so a future
/// disposition change cannot quietly leak a name into the generated baseline.
///
/// A baseline is residual debt. Anything documented is not debt; anything
/// already suppressed by another channel is already accounted for; and a
/// malformed marker is a defect to fix, not debt to freeze.
#[test]
fn baseline_candidates_exclude_documented_allowed_and_baselined_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(root, FIX_UNITS, MIXED_UNITS);
    write_file(root, FIX_CHUNK, MIXED_CHUNK);
    write_file(root, FIX_BASELINE, "# ratchet baseline\n\ngamma_op\n");

    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK, FIX_BASELINE]);
    let candidates = reify_audit::pdoccover::baseline_candidates(&h.ctx(root));

    for (name, why) in [
        ("alpha_op", "documented in a chunk"),
        ("eta_op", "documented (its allow marker is merely stale)"),
        ("beta_op", "carries a well-formed allow marker"),
        ("gamma_op", "already listed in the baseline file"),
        (
            "zeta_op",
            "carries a REASONLESS marker — a defect to fix, not debt to freeze",
        ),
    ] {
        assert!(
            !candidates.contains(&name.to_string()),
            "`{name}` must NOT be a baseline candidate: it {why}. Got \
             {candidates:?}"
        );
    }

    for name in ["delta_op", "epsilon_op"] {
        assert!(
            candidates.contains(&name.to_string()),
            "`{name}` is undocumented with no exemption channel, so it MUST be \
             a baseline candidate. Got {candidates:?}"
        );
    }
}

/// The derivation is over the omission lane only. Fabrications are a chunk
/// defect to fix, not registry debt to ratchet, and #5480's baseline file is
/// keyed by bare name — a fabricated name has no registry entry to key on.
#[test]
fn baseline_candidates_exclude_fabricated_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(root, FIX_UNITS, MIXED_UNITS);
    write_file(
        root,
        FIX_CHUNK,
        "# Geometry\n\n- `alpha_op(shape, amount)` — real.\n\
         - `eta_op(shape)` — real.\n- `phantom_op(x)` — not real.\n",
    );
    write_file(root, FIX_BASELINE, "# ratchet baseline\n\ngamma_op\n");

    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK, FIX_BASELINE]);
    let ctx = h.ctx(root);

    let findings = reify_audit::pdoccover::check(&ctx);
    assert!(
        findings
            .iter()
            .any(|f| f.summary.starts_with("fabricated-name:")
                && finding_name(f) == "phantom_op"),
        "fixture sanity — `phantom_op` must be reported as a fabrication. Got \
         {:?}",
        findings
            .iter()
            .map(|f| f.summary.as_str())
            .collect::<Vec<_>>()
    );

    let candidates = reify_audit::pdoccover::baseline_candidates(&ctx);
    assert!(
        !candidates.contains(&"phantom_op".to_string()),
        "a fabricated name must never enter the generated baseline — the \
         baseline is the omission ratchet, keyed by registry name. Got \
         {candidates:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-23: real-repo smoke — PRD leaf γ's observable signal
// ─────────────────────────────────────────────────────────────────────────────

/// The five stable category prefixes. A summary carrying anything else means
/// a category was added without updating the module header's contract table.
const KNOWN_CATEGORIES: &[&str] = &[
    "undocumented-name",
    "fabricated-name",
    "stale-baseline-entry",
    "stale-allow-entry",
    "allow-missing-reason",
];

/// `check()` over the REAL repo, via `RealGitOps`.
///
/// Deliberately NOT a zero-on-main guard — the inverse of its
/// `tests/pdssentinel.rs` sibling. PDOCCOVER is expected non-zero until #5480
/// seeds the baseline; that residual IS the signal PRD leaf γ asks for.
///
/// So this asserts only invariants that no concurrent chunk edit can flip:
/// findings exist, every one is well-formed, and the order is deterministic.
/// It names no specific name and freezes no count — #5434 (owns
/// `chunks/stdlib.md`), #5347 and #5389 are all editing chunk content, and any
/// count or name assertion here would flip RED on their merge rather than on a
/// real defect.
#[test]
fn real_repo_smoke_findings_are_well_formed_and_deterministic() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by cargo test");
    let repo_root = Path::new(&manifest_dir)
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root");

    let git = reify_audit::RealGitOps::new(&repo_root);
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: repo_root.clone(),
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };

    // Guard against a vacuous pass: an empty index (a vendored/exported tree
    // with no git work-tree) would make every loop in check() a no-op and the
    // assertions below meaningless.
    let tracked = ctx.git.ls_files();
    assert!(
        !tracked.is_empty(),
        "ls_files() returned an empty file list — the test is likely running \
         outside a git work-tree. Fail rather than pass vacuously."
    );
    for required in [
        "crates/reify-compiler/src/units.rs",
        "crates/reify-mcp/src/tools/chunks/stdlib.md",
    ] {
        assert!(
            tracked.iter().any(|p| p == required),
            "{required} must be tracked — it is a PDOCCOVER input, and without \
             it the corresponding lane silently scans nothing"
        );
    }

    // (iv) Completes without panicking on the real tree.
    let findings = reify_audit::pdoccover::check(&ctx);

    // (i) The omission lane is not blind. The property this test actually
    // wants is NON-VACUITY — that the lane built a census at all — not that
    // the census still has a residual.
    //
    // Asserting `undocumented >= 1` unconditionally would couple this test to
    // the ABSENCE of `pdoccover-baseline.txt`, which #5480 is chartered to
    // seed. Once it does, every census name resolves to Exempt, the residual
    // drops to zero and this test would flip RED on the intended improvement —
    // a hidden dependency forcing #5480 to edit this file as part of its own
    // landing. So: the census must be non-empty always, and the residual must
    // be non-empty only while nothing has been baselined yet.
    let units_src = std::fs::read_to_string(repo_root.join(UNITS_PATH))
        .expect("the real units.rs must be readable");
    let census: std::collections::BTreeSet<String> =
        reify_audit::pdoccover::extract_registries(&units_src)
            .iter()
            .flat_map(|r| r.entries.iter())
            .map(|e| e.name.clone())
            .collect();
    assert!(
        !census.is_empty(),
        "the omission lane extracted an EMPTY census from the real units.rs — \
         it is scanning nothing, and would report clean no matter how far the \
         chunks drifted. This is the silent-false-clean mode the floor guards \
         exist to prevent."
    );

    let undocumented = findings
        .iter()
        .filter(|f| f.summary.starts_with("undocumented-name:"))
        .count();
    let baselined = repo_root.join(BASELINE_PATH).exists();
    assert!(
        undocumented >= 1 || baselined,
        "no `undocumented-name:` finding, and no baseline file at \
         {BASELINE_PATH} to explain it. With {} names in the census and \
         nothing suppressing them, zero residual means the omission lane went \
         blind (a chunk-path change, or a documented-name matcher that now \
         matches everything) — NOT that the corpus was completed. Total \
         findings: {}",
        census.len(),
        findings.len()
    );

    // (ii) Every finding is well-formed.
    for f in &findings {
        assert_eq!(
            f.pattern,
            Pattern::PDocCover,
            "every finding must carry Pattern::PDocCover; got {:?} for {:?}",
            f.pattern,
            f.summary
        );
        assert_eq!(
            f.severity,
            Severity::High,
            "every PDOCCOVER finding rides at High; got {:?} for {:?}",
            f.severity,
            f.summary
        );
        let category = finding_category(f);
        assert!(
            KNOWN_CATEGORIES.contains(&category),
            "finding carries unknown category prefix {category:?}; expected one \
             of {KNOWN_CATEGORIES:?}. A new category needs a row in the module \
             header's contract table. Summary: {:?}",
            f.summary
        );
        assert!(
            !finding_name(f).is_empty(),
            "finding must name the offending symbol after its category prefix; \
             got {:?}",
            f.summary
        );
        let files: Vec<&String> = f
            .evidence
            .iter()
            .filter_map(|e| match e {
                EvidenceRef::File { path } => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(
            f.evidence.len(),
            1,
            "each finding carries exactly one evidence ref; got {:?} for {:?}",
            f.evidence,
            f.summary
        );
        assert_eq!(
            files.len(),
            1,
            "that evidence ref must be an EvidenceRef::File — PDOCCOVER is a \
             working-tree detector with no commit, runs-db or task-metadata \
             evidence to offer; got {:?} for {:?}",
            f.evidence,
            f.summary
        );
        assert!(
            !files[0].is_empty(),
            "evidence path must be non-empty for {:?}",
            f.summary
        );
    }

    // (iii) Deterministic across two consecutive runs over an unchanged tree.
    // Sort order is the reporting contract: /audit diffs consecutive runs, and
    // an unstable order would show spurious churn on every sweep.
    let again = reify_audit::pdoccover::check(&ctx);
    let first: Vec<&str> = findings.iter().map(|f| f.summary.as_str()).collect();
    let second: Vec<&str> = again.iter().map(|f| f.summary.as_str()).collect();
    assert_eq!(
        first, second,
        "two consecutive check() runs over an unchanged tree must produce a \
         byte-identical finding sequence"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// amendments: contracts the plan's fixtures left unexercised
//
// Every fixture above declares each name exactly once and writes every tracked
// file it lists, so two load-bearing code paths had no test: the census merge
// arm (a name declared in several registries) and load_inputs' fail-safe
// branches. Both are live on the real tree — `single` and `flat_map` are each
// declared twice in units.rs — and both encode a stated contract.
// ─────────────────────────────────────────────────────────────────────────────

/// `delta_op` is declared in TWO registries — once bare, once with a
/// well-formed allow marker. The census is keyed by NAME, so it must yield
/// exactly ONE finding, not one per declaration site.
///
/// The contract: "reporting the same undocumented name twice would make the
/// ratchet count depend on an internal `units.rs` factoring detail". Splitting
/// or merging a registry must not move the number.
#[test]
fn a_name_declared_in_two_registries_yields_one_finding() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(
        root,
        FIX_UNITS,
        "\
pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    \"delta_op\",
];

pub const DYNAMICS_QUERY_NAMES: &[&str] = &[
    \"delta_op\", // pdoccover:allow — declared in both families
];
",
    );

    let h = Harness::new(&[FIX_UNITS]);
    let ctx = h.ctx(root);
    let findings = reify_audit::pdoccover::check(&ctx);

    // First-well-formed-reason-wins: the second declaration's marker exempts
    // the name, so the merged entry is Exempt and nothing is reported.
    assert!(
        findings.is_empty(),
        "a well-formed allow marker on ANY declaring line exempts the merged \
         census name; got {:?}",
        findings
            .iter()
            .map(|f| f.summary.as_str())
            .collect::<Vec<_>>()
    );

    // And it is not a baseline candidate either — one derivation, one verdict.
    assert!(
        reify_audit::pdoccover::baseline_candidates(&ctx).is_empty(),
        "an exempted name must not be offered as a baseline candidate"
    );
}

/// A reasonless marker on EITHER declaring line wins over a bare sibling
/// declaration: `allow_missing_reason` is OR-folded across declarations, and
/// the malformed marker is reported once.
///
/// Asserted in BOTH declaration orders. A merge that simply let the LAST
/// declaration overwrite the first would satisfy one order and fail the other,
/// so testing a single order would not distinguish an OR-fold from a
/// last-wins overwrite.
#[test]
fn a_reasonless_marker_on_either_declaration_wins() {
    for (label, src) in [
        (
            "marker second",
            "\
pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    \"delta_op\",
];

pub const DYNAMICS_QUERY_NAMES: &[&str] = &[
    \"delta_op\", // pdoccover:allow
];
",
        ),
        (
            "marker first",
            "\
pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    \"delta_op\", // pdoccover:allow
];

pub const DYNAMICS_QUERY_NAMES: &[&str] = &[
    \"delta_op\",
];
",
        ),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_file(root, FIX_UNITS, src);

        let h = Harness::new(&[FIX_UNITS]);
        let ctx = h.ctx(root);
        let findings = reify_audit::pdoccover::check(&ctx);

        let summaries: Vec<&str> = findings.iter().map(|f| f.summary.as_str()).collect();
        assert_eq!(
            findings.len(),
            1,
            "[{label}] a name declared twice yields at most ONE finding; got \
             {summaries:?}"
        );
        assert_eq!(
            finding_category(&findings[0]),
            "allow-missing-reason",
            "[{label}] a reasonless marker on ANY declaring line is OR-folded \
             into the merged census entry and wins over the bare declaration; \
             got {summaries:?}"
        );
        assert_eq!(finding_name(&findings[0]), "delta_op");

        // Provenance is FIRST-declaration-wins, so the reported line does not
        // depend on which registry happens to carry the marker.
        assert!(
            findings[0].summary.contains(&format!("{UNITS_PATH}:2")),
            "[{label}] the finding must cite the FIRST declaration \
             ({UNITS_PATH}:2); got {summaries:?}"
        );

        // Deliberately not a baseline candidate: baselining a malformed marker
        // would freeze it into the ratchet instead of prompting the fix.
        assert!(
            reify_audit::pdoccover::baseline_candidates(&ctx).is_empty(),
            "[{label}] a reasonless marker is a defect to fix, not debt to freeze"
        );
    }
}

/// Only TRACKED files participate. An untracked chunk sitting on disk must not
/// document anything.
///
/// The module header claims "a stray untracked `units.rs.orig` or a scratch
/// chunk never perturbs the census" — this is what verifies it. Without the
/// `ls_files()` filter, an agent's scratch markdown in the chunks directory
/// would silently satisfy the ratchet.
#[test]
fn an_untracked_chunk_documents_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    write_file(root, FIX_UNITS, FOUR_WAY_UNITS);
    // On disk, and it DOES mention every name — but it is not tracked.
    write_file(
        root,
        "crates/reify-mcp/src/tools/chunks/scratch.md",
        "# Scratch\n\n`alpha_op(x)` `beta_op(x)` `gamma_op(x)` `delta_op(x)`\n",
    );

    // FIX_CHUNK is deliberately absent from both disk and the tracked list.
    let h = Harness::new(&[FIX_UNITS]);
    let ctx = h.ctx(root);
    let findings = reify_audit::pdoccover::check(&ctx);

    let names: Vec<&str> = findings.iter().map(finding_name).collect();
    assert_eq!(
        names,
        vec!["alpha_op", "delta_op", "gamma_op"],
        "an untracked chunk must document nothing — only `beta_op`'s allow \
         marker exempts it. Had the scratch file counted, all four names would \
         be documented and the sole finding would be a `stale-allow-entry:` \
         for `beta_op`. Got {findings:?}"
    );
}

/// A file that is TRACKED but unreadable (listed in the index, absent from the
/// work tree) is skipped fail-safe: no panic, no finding.
///
/// This is the detector's core safety property — "a missing census reports
/// nothing, it does not report everything". The opposite behaviour (treating an
/// unreadable units.rs as an empty chunk corpus, or vice versa) would turn a
/// mid-rebase working tree into a wall of false findings.
#[test]
fn a_tracked_but_missing_file_is_skipped_fail_safe() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Nothing is written to disk at all; every path below is index-only.
    let h = Harness::new(&[FIX_UNITS, FIX_CHUNK, FIX_BASELINE]);
    let ctx = h.ctx(root);

    let findings = reify_audit::pdoccover::check(&ctx);
    assert!(
        findings.is_empty(),
        "an unreadable census source must yield NO findings rather than \
         panicking or reporting everything; got {findings:?}"
    );
    assert!(
        reify_audit::pdoccover::baseline_candidates(&ctx).is_empty(),
        "the shared derivation must be fail-safe on the same terms as check()"
    );

    // The inverse half: a readable units.rs with an unreadable chunk corpus
    // must report the census as undocumented, not silently exempt it.
    write_file(root, FIX_UNITS, FOUR_WAY_UNITS);
    let findings = reify_audit::pdoccover::check(&ctx);
    let names: Vec<&str> = findings.iter().map(finding_name).collect();
    assert_eq!(
        names,
        vec!["alpha_op", "delta_op", "gamma_op"],
        "with the chunk corpus unreadable, every non-exempt name is \
         undocumented — the lane fails LOUD on a readable census, and silent \
         only when it has no census at all; got {findings:?}"
    );
}
