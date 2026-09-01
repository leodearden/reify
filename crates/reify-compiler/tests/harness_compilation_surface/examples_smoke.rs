//! Bulk smoke test — every `examples/*.ri` must parse and compile with stdlib
//! with no Error-severity diagnostics.
//!
//! Motivation: per-file test wrappers (m5_integration, m8_stdlib_integration,
//! m11_full_integration, …) cover a subset of the example files, but files
//! without a wrapper drift silently.  This test walks the directory and catches
//! every file at once.

use std::path::{Path, PathBuf};

use reify_test_support::missing_paths_under;

/// Absolute path to the workspace `examples/` directory, resolved at compile
/// time from this crate's manifest directory (two levels up).
const EXAMPLES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");

/// Discovery-regression TRIPWIRE, NOT a corpus-size target: catches a walk
/// bug, a bad path resolution, or a refactor that stops [`discover_ri_files`]
/// from recursing. Derived from [`MIN_EXERCISED_RI_FILES`] plus
/// [`SKIP_SET`]'s size so the two floors cannot drift apart from each other;
/// [`discovery_floor_tracks_the_live_corpus`] is what keeps
/// `MIN_EXERCISED_RI_FILES` itself fresh against the live corpus. Lower
/// `MIN_EXERCISED_RI_FILES` if the corpus is ever intentionally trimmed
/// below this floor.
const MIN_DISCOVERED_RI_FILES: usize = MIN_EXERCISED_RI_FILES + SKIP_SET.len();

/// Discovery-regression TRIPWIRE, NOT a corpus-size target, for the
/// SKIP_SET-filtered `exercised` count in
/// [`no_example_emits_ctor_field_conformance_diagnostics`]. Deliberately
/// absolute rather than derived from the live count, which would shrink in
/// lockstep with a discovery regression and never fire.
/// [`discovery_floor_tracks_the_live_corpus`] is the freshness ratchet that
/// keeps this constant from going stale.
const MIN_EXERCISED_RI_FILES: usize = 200;

/// Files to skip in the bulk smoke test.  Each entry is `(relative_path, reason)`
/// where `relative_path` is the forward-slash-separated path rooted at `examples/`
/// (e.g. `"bracket.ri"`, `"fields/composed_stiffness.ri"`).  Using the full
/// relative path rather than a bare basename means that same-basename files in
/// different subdirectories can be skipped independently without ambiguity.
/// The reason is mandatory — the `(&str, &str)` tuple shape forces every entry
/// to carry a one-line human-readable justification, making skips auditable at
/// review time.
///
/// Entries here are files that cannot yet reach a clean `compile_with_stdlib`
/// run, or are covered instead by a dedicated gated test elsewhere; every
/// other file discovered under `examples/` is expected to compile clean.
/// Deliberately does not pin a corpus count or set size here: the live
/// corpus size is enforced by [`discovery_floor_tracks_the_live_corpus`],
/// whose failure message reports the current count.
// NOTE: `topology_selectors/fillet_top_edges.ri` used to be skipped here for a
// missing 3-arg `fillet(solid, edges, radius)` stdlib binding.  That binding
// landed (#3205/#4360/#4362) and #5208 made curated 3-arg fillet reachable
// through the production pipeline, so the entry was removed and the file is now
// compiled by the bulk walker like every other example.
const SKIP_SET: &[(&str, &str)] = &[
    (
        "auto/bearing_constraint_select.ri",
        "strict `auto: Seal` with two stub-feasible candidates (ThinSeal, ThickSeal) \
         resolves Ambiguous under the compile-time stub checker → E_AUTO_TYPE_PARAM_AMBIGUOUS \
         Error; the zero-Error gate cannot pass. The unique-survivor selection \
         (ThinSeal, whose thickness=1mm satisfies the `seal.thickness < bore_radius` \
         constraint) is a REAL-checker behaviour exercised by task ζ's reify-eval \
         auto_type_param_completion_e2e harness under SimpleConstraintChecker. \
         Per-candidate ValueMap setup is delivered by task 4433 β \
         (seed_candidate_value_map); loop wiring by γ.",
    ),
    (
        "auto/bounded_fallback_unsound.ri",
        "7 strict `auto: Layer` params (> max_depth=6 → depth-bound BFS fallback) with a \
         joint constraint `l1.thickness + … + l7.thickness < max_stack` coupling all param \
         member fields. Under the compile-time stub checker the TypeParam member reads \
         emit code:None \"member access not yet supported\" Errors at structure-compile time; \
         the zero-Error gate cannot pass. The joint-infeasibility hard error \
         (E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE) is a REAL-checker behaviour — γ's \
         joint-recheck seeds the full ValueMap (7×LayerA.thickness=2mm → sum=14mm > \
         max_stack=10mm → Violated) and is exercised by task ζ's reify-eval e2e. \
         Task 4434 (γ) delivers the joint-recheck; task 4433 β delivers \
         seed_candidate_value_map.",
    ),
    (
        "auto/bearing_unsat.ri",
        "ζ negative fixture: strict `auto: Seal` with TWO candidates that BOTH violate the \
         member constraint `seal.thickness < bore_radius=3mm` (ThickSeal=5mm, HugeSeal=8mm). \
         Under the real checker (SimpleConstraintChecker) Phase B finds zero feasible candidates \
         → FeasibilityResult::Empty → E_AUTO_TYPE_PARAM_NO_CANDIDATE Error naming each \
         candidate's violated constraint. Under the stub checker every constraint is \
         Indeterminate → all candidates stub-feasible → ≥2 feasible → E_AUTO_TYPE_PARAM_AMBIGUOUS. \
         Either way the fixture emits an Error under any checker and cannot pass the zero-Error \
         gate. Exercised by task ζ's reify-eval auto_type_param_completion_e2e harness \
         (bearing_unsat_emits_no_candidate_naming_constraint). \
         Mirrored into auto_type_param_determinism_tests.rs::SKIP_SET (task 4437 ζ).",
    ),
    (
        "auto/bearing_computed_default_unevaluated.ri",
        "Gap-C fixture (task #4616): strict `auto: Seal` with a computed-default template \
         cell (`clearance = bore_radius - 0.5mm`) whose default is a non-literal BinOp. \
         The literal-only seeder skips `clearance`, so the constraint `seal.thickness < \
         clearance` evaluates to Indeterminate for every candidate. Under any checker \
         (stub or real) both ThinSeal and ThickSeal are feasible → ≥2 feasible → \
         E_AUTO_TYPE_PARAM_AMBIGUOUS Error. The fixture additionally emits the new \
         W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED Warning (naming 'clearance') under \
         any non-stub checker (task #4616 Gap-C deliverable). Either way the zero-Error \
         gate cannot pass. Exercised by task #4616's reify-eval e2e regression gate \
         (gap_c_computed_default_unevaluated_emits_warning_literal_does_not).",
    ),
    (
        "conditional_compilation/main.ri",
        "Multi-file cfg-gated entry: `param p : Platform` in type position resolves only \
         through the #cfg(target)-gated import (platform_linux or platform_wasm), using the \
         reify check cfg DAG (compile_entry_with_stdlib_cfg_checked). The single-file \
         compile_with_stdlib bulk smoke path cannot follow gated imports, so `Platform` is an \
         unresolved-type Error there. The two-way symmetric behaviour (both --cfg target=linux \
         and --cfg target=wasm exit 0, each resolving the platform-correct Platform variant) \
         is exercised end-to-end by crates/reify-cli/tests/harness_cli/cli_check_cfg_example.rs. \
         The siblings (platform_linux.ri, platform_wasm.ri) define their own types, compile \
         clean single-file, and are intentionally NOT skipped.",
    ),
    (
        "module_visibility/consumer.ri",
        "Cross-module consumer that fails the single-file smoke for two independent reasons: \
         (1) its `import producer` edge cannot be followed by compile_with_stdlib (Motor is \
         unresolved → unresolved-type Error on `sub m = Motor()`); \
         (2) the `let hidden = m.rated_torque` dot-access is a by-design E_PRIV_MEMBER_ACCESS \
         Error (rated_torque is priv on Motor). The priv-param-hidden / visible-param-resolves \
         two-way signal is exercised end-to-end by \
         crates/reify-cli/tests/harness_cli/cli_module_visibility_example.rs via the real `reify check` \
         binary. The siblings (producer.ri, mismatch_variant.ri) are self-contained / \
         CLI-only-diagnostic and are intentionally NOT skipped.",
    ),
    (
        "multi_aspect_objective_mixed.ri",
        "PRD δ (#5020) BT1 negative fixture — intentionally emits \
         E_OBJECTIVE_MIXED_DIMENSION (ObjectiveDimensionIncoherent) at compile: two \
         same-sense minimize decls over incommensurable dimensions (Money cost, Mass \
         mass) lower to a 2-term WeightedSum that fails α's units-coherence guard. \
         Positive (coherent) coverage lives in the sibling multi_aspect_objective.ri \
         (NOT skipped) and both are exercised end-to-end by \
         crates/reify-eval/tests/harness_fea_solver_e2e/multi_aspect_objective_example_e2e.rs.",
    ),
];

/// Per-SITE waivers for ctor-conformance diagnostics that a shipped example
/// still emits because its call site has not been migrated yet, and cannot be
/// migrated by the task that promoted the family.
///
/// Each entry is `(relative_path, param_name, owning_task)`:
/// * `relative_path` is the same forward-slash `relative_to_examples_dir` key
///   form `SKIP_SET` uses (`"trajectory/printer_print_envelope.ri"`, never the
///   repo-relative `"examples/trajectory/..."` spelling);
/// * `param_name` is the offending ctor param, parsed back out of the
///   diagnostic by [`param_name_from_ctor_diagnostic`];
/// * `owning_task` is the live task that owns retiring the entry, in the
///   canonical `#NNNN` cite form required by the repo's citation convention.
///
/// # This is NOT `SKIP_SET`, and must never be merged into it
///
/// `SKIP_SET` is for files that cannot reach a clean compile AT ALL — the file
/// is dropped from the walk entirely, so it gets no coverage of any kind.
/// `printer_print_envelope.ri` compiles cleanly; it merely carries two
/// un-migrated call sites. It stays fully walked, and every OTHER diagnostic it
/// emits still fails the gate.
///
/// # The waiver is per-SITE, never per-file
///
/// Matching is on the `(file, param)` PAIR. A future diagnostic in the same file
/// at a different param is unwaived and fails the gate, as does a diagnostic at
/// one of these params that carries a different, non-`argument '<name>'`
/// wording.
///
/// # Retirement
///
/// Task #5847 owns deleting BOTH entries in the same diff that dimensions
/// `trajectory/printer_print_envelope.ri:154` / `:155` (esc-5627-5 option A).
/// The sites cannot be dimensioned in isolation without collapsing the TOTS
/// solve, which is the whole reason the debt exists rather than the migration
/// simply having been done. Leaving the entries behind after that lands is
/// caught by [`ctor_conformance_migration_debt_entries_are_all_live`].
const CTOR_CONFORMANCE_MIGRATION_DEBT: &[(&str, &str, &str)] = &[
    (
        "trajectory/printer_print_envelope.ri",
        "velocity_limit",
        "#5847",
    ),
    (
        "trajectory/printer_print_envelope.ri",
        "acceleration_limit",
        "#5847",
    ),
];

/// Bulk smoke: walk `examples/*.ri`, parse each file and compile it with the
/// stdlib prelude, accumulate every file that produces an Error-severity
/// diagnostic, and panic once at the end with a report covering ALL failures.
///
/// A single test run therefore surfaces every broken file rather than stopping
/// at the first one.  Files listed in `SKIP_SET` are excluded from the walk.
#[test]
fn all_examples_parse_and_compile_with_stdlib() {
    let mut failures: Vec<(String, String)> = Vec::new();

    let paths = discover_ri_files();
    let total = paths.len();
    assert!(
        total >= MIN_DISCOVERED_RI_FILES,
        "examples_smoke discovered only {} .ri files, below the \
         MIN_DISCOVERED_RI_FILES floor of {} — did the examples/ directory \
         move or get renamed, or did discover_ri_files() stop recursing?",
        total,
        MIN_DISCOVERED_RI_FILES
    );

    let exercised_list = exercised_paths(&paths);
    let exercised = exercised_list.len();
    for (path, rel_key) in &exercised_list {
        smoke_one(path, rel_key, &mut failures);
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

/// Corpus gate (task 5302 α): NO example file may emit a struct-ctor
/// field-conformance diagnostic, at ANY severity.
///
/// This is a deliberate sibling to [`all_examples_parse_and_compile_with_stdlib`]
/// rather than an extension of it, because the two gates filter on different
/// axes and only the pair is sufficient:
///
/// * the bulk test filters on `Severity::Error`, so a *Warning*-severity
///   regression across the whole corpus passes it silently.  That is exactly
///   how task 5302's first cut shipped five families of false-positive ctor
///   warnings (Point, Matrix, Field, generic-enum, dimensioned-Scalar) across
///   15+ previously-clean shipped examples without tripping the branch's own
///   gate;
/// * this test filters on the ctor-conformance diagnostic *code* set and
///   ignores severity entirely, so it stays meaningful after the planned δ
///   flip of `CTOR_FIELD_CONFORMANCE_SEVERITY` from Warning to Error.
///
/// Every violation across the whole corpus is accumulated and reported in one
/// panic — the gate exists for corpus-wide visibility, so it must NOT fail fast.
///
/// Sites listed in [`CTOR_CONFORMANCE_MIGRATION_DEBT`] are waived per `(file,
/// param)` pair; see [`CTOR_CONFORMANCE_GATE_REMEDY`] for what a firing
/// diagnostic means and which of the three remedies applies.
///
/// Since task 5303 (ε) the gate ALSO covers the two structural ctor codes,
/// `CtorUnknownField` and `CtorArity` — i.e. no shipped example may carry a
/// typo'd constructor field name or a silently-dropped surplus positional
/// argument either. Extending it was conditional on a measurement, because ε
/// was explicitly not allowed to fix corpus sites (γ owns corpus fix-forward)
/// and the waiver list is migration debt, not an escape hatch for new codes:
/// the corpus was measured CLEAN under both new codes, so the tightening was
/// free, added no `CTOR_CONFORMANCE_MIGRATION_DEBT` entry, and δ inherits it.
#[test]
fn no_example_emits_ctor_field_conformance_diagnostics() {
    // Fail fast on the discovery floor BEFORE the corpus walk. This pre-check
    // is a directory walk only, whereas ctor_conformance_corpus_walk() compiles
    // every exercised file inside its OnceLock — so reading walk.exercised here
    // instead would pay for the whole corpus before reporting a misconfigured
    // discover_ri_files()/SKIP_SET. Deliberately symmetric with the eval-side
    // gate in
    // auto_type_param_determinism_tests.rs::v0_1_example_corpus_compile_and_check_time_is_bounded,
    // which is fail-fast for the same reason.
    let paths = discover_ri_files();
    let exercised = exercised_paths(&paths).len();
    assert!(
        exercised >= MIN_EXERCISED_RI_FILES,
        "ctor-conformance corpus gate exercised only {} .ri files, below the \
         MIN_EXERCISED_RI_FILES floor of {} (SKIP_SET has {} entries) — did \
         the examples/ directory move or get renamed, did discover_ri_files() \
         stop recursing, or did SKIP_SET grow unexpectedly?",
        exercised,
        MIN_EXERCISED_RI_FILES,
        SKIP_SET.len()
    );

    let walk = ctor_conformance_corpus_walk();

    let unwaived: Vec<&CtorConformanceViolation> = walk
        .violations
        .iter()
        .filter(|v| !violation_is_waived(v))
        .collect();

    if !unwaived.is_empty() {
        let n = unwaived.len();
        let waived = walk.violations.len() - n;
        let lines: Vec<String> = unwaived
            .iter()
            .map(|v| format!("  {} [{}] {}", v.file, v.code, v.message))
            .collect();
        panic!(
            "ctor-conformance corpus gate: {} unwaived diagnostic(s) across {} exercised \
             example files ({} waived by CTOR_CONFORMANCE_MIGRATION_DEBT).\n\n{}\n\n{}",
            n,
            walk.exercised,
            waived,
            lines.join("\n"),
            CTOR_CONFORMANCE_GATE_REMEDY,
        );
    }
}

/// Appended to every ctor-conformance corpus-gate failure.
///
/// The pre-γ text asserted that every firing diagnostic was "a false positive
/// from the conformance walker, not a broken example" and directed the reader
/// straight at the `general_leaf_param_family_is_validated` allowlist. That was
/// true while the walker only judged families the corpus was already written
/// for; after task 5627 promoted the dimensioned-`Scalar` family it is no longer
/// the common case, and an implementer following it verbatim would REVERT that
/// promotion to make the gate green. So the remedy has to name all three
/// mechanisms and let the reader pick, rather than presume one.
const CTOR_CONFORMANCE_GATE_REMEDY: &str = "\
Each line above is a struct-ctor field-conformance diagnostic fired against a \
shipped example. There are three possible causes; diagnose before fixing.\n\
\n\
  1. TRUE POSITIVE (now the common case) — the example is genuinely \
un-migrated. Fix the EXAMPLE: dimension the call site (`300mm/s`, not `300.0`), \
or, when the arg is right and the DECLARATION is wrong, correct the annotation \
— `examples/bearing_auto_seal.ri`'s `param durometer : Length = 70.0` was a \
dimensionless hardness number mis-annotated as a Length, and the fix was \
`: Real`, NOT adding a unit to the literal.\n\
\n\
  2. FALSE POSITIVE from the walker — still possible, and still fixed at \
crates/reify-compiler/src/conformance/mod.rs: either the family's dedicated \
shape-based arm in `walk_param_against_arg_type` (Vector / Point / Field / \
Matrix / Tensor) or the `general_leaf_param_family_is_validated` allowlist that \
gates the general concrete-leaf arm. Do not reach for this one first: a \
promoted family firing on an un-migrated example is cause 1, and narrowing the \
walker to silence it would undo a landed decision.\n\
\n\
  3. BLOCKED ON ANOTHER TASK — the site is a true positive but cannot be fixed \
here (dimensioning it in isolation breaks something else). Add a per-SITE \
`CTOR_CONFORMANCE_MIGRATION_DEBT` entry naming the file, the param and the LIVE \
task that owns retiring it. Per-site, never per-file, and never without an \
owner.\n\
\n\
A SKIP_SET entry is NEVER the answer for a file that compiles cleanly: it drops \
the file from the walk entirely and removes all of its coverage, not just the \
one diagnostic.";

/// Sanity guard: every entry in SKIP_SET must name a relative path that actually
/// exists under `examples/`.  Catches mis-typed or stale skip entries before they
/// silently disable coverage.
///
/// Existence filter: [`reify_test_support::missing_paths_under`], where its
/// contract is documented and unit-tested.
///
/// Every stale entry is reported in one panic rather than failing fast on the
/// first, matching the corpus-wide-visibility principle this file applies to its
/// other bulk guards.
#[test]
fn skip_set_entries_exist_under_examples_dir() {
    let missing = missing_paths_under(
        Path::new(EXAMPLES_DIR),
        SKIP_SET.iter().map(|(rel, _)| *rel),
    );
    if missing.is_empty() {
        return;
    }
    // Build the report by walking SKIP_SET and keeping the flagged entries,
    // rather than looking each flagged path back up in SKIP_SET: the reason
    // string stays in hand, so there is no lookup that cannot fail and hence no
    // unreachable "reason missing" branch to justify.
    let lines: Vec<String> = SKIP_SET
        .iter()
        .filter(|(rel, _)| missing.contains(rel))
        .map(|(rel, reason)| format!("  '{rel}' (reason: {reason})"))
        .collect();
    panic!(
        "SKIP_SET entry/entries name a relative path that does not exist under {}:\n{}\n\
         A stale key silently disables coverage for a file that is no longer skipped — \
         delete the entry or fix the path.",
        EXAMPLES_DIR,
        lines.join("\n")
    );
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
///
/// # Panics
///
/// Panics if `path` does not begin with the lexical `EXAMPLES_DIR` prefix.
/// **Callers must pass paths produced by [`discover_ri_files`]** — i.e. paths
/// that are constructed by walking `EXAMPLES_DIR` without canonicalization.
/// Canonicalized paths (which resolve `..` components) will not match the
/// lexical prefix string and will panic.
fn relative_to_examples_dir(path: &Path) -> String {
    let rel = path.strip_prefix(EXAMPLES_DIR).unwrap_or_else(|e| {
        panic!(
            "examples_smoke: '{}' is not under EXAMPLES_DIR ({}): {}",
            path.display(),
            EXAMPLES_DIR,
            e
        )
    });
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

/// The subset of `paths` not present in [`SKIP_SET`] (keyed by
/// [`relative_to_examples_dir`]), each paired with its precomputed relative
/// key. The single source of the SKIP_SET-filtered "exercised" quantity —
/// every consumer (both corpus-walking `#[test]`s and
/// [`discovery_floor_tracks_the_live_corpus`]) calls this instead of
/// re-deriving the filter, so they can never disagree about what "exercised"
/// means.
fn exercised_paths(paths: &[PathBuf]) -> Vec<(&PathBuf, String)> {
    use std::collections::HashSet;

    let skip: HashSet<&str> = SKIP_SET.iter().map(|(name, _)| *name).collect();
    paths
        .iter()
        .filter_map(|p| {
            let rel = relative_to_examples_dir(p);
            if skip.contains(rel.as_str()) {
                None
            } else {
                Some((p, rel))
            }
        })
        .collect()
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

/// Verify two invariants for every path returned by `discover_ri_files()`:
///
/// (a) `relative_to_examples_dir` accepts the path without panicking (i.e. the
///     path is lexically rooted under `EXAMPLES_DIR`, as `discover_ri_files`
///     guarantees).  If `discover_ri_files` ever starts canonicalizing paths
///     (resolving `..`), the `strip_prefix` inside `relative_to_examples_dir`
///     would break and this test would surface the regression before it silently
///     corrupts SKIP_SET lookups or failure reports.
///
/// (b) The relative form round-trips back to the original absolute path when
///     joined onto `EXAMPLES_DIR`: `Path::new(EXAMPLES_DIR).join(rel) == path`.
///     This locks the SKIP_SET-key join-compatibility contract across the full
///     corpus — both top-level (`bracket.ri`-style) and nested
///     (`fields/composed_stiffness.ri`-style) entries.
#[test]
fn relative_to_examples_dir_accepts_all_discovered_paths() {
    for path in discover_ri_files() {
        // Will panic if path is not lexically rooted under EXAMPLES_DIR.
        let rel = relative_to_examples_dir(&path);
        assert_eq!(
            Path::new(EXAMPLES_DIR).join(&rel),
            path,
            "round-trip failed: EXAMPLES_DIR.join({:?}) != original {:?}",
            rel,
            path
        );
    }
}

/// Freshness ratchet for [`MIN_EXERCISED_RI_FILES`]: this floor is a
/// discovery-regression TRIPWIRE, not a corpus-size target, so it only
/// stays useful while it tracks the live corpus size. Checking this
/// constant alone is enough: [`MIN_DISCOVERED_RI_FILES`] is derived from
/// it, so it cannot go stale independently.
///
/// One-directional by construction: a discovery regression only SHRINKS
/// `exercised`, which makes the assertion below easier to satisfy, while
/// the absolute gate in
/// [`no_example_emits_ctor_field_conformance_diagnostics`] is what actually
/// fires on that regression. So this ratchet can never mask it — it only
/// fires once the corpus has grown enough that the floor has lost its
/// tripwire sensitivity; see the assertion message for the current bound.
///
/// Measures `exercised` via the same [`exercised_paths`] helper the gate
/// calls, so this test can never disagree with the gate it ratchets.
/// Directory walk only — no compile, no check — so it stays as cheap as the
/// other sanity guards here.
///
/// Also asserts that `exercised_paths` excluded exactly `SKIP_SET.len()`
/// files: `total - exercised` must equal `SKIP_SET.len()`, or a SKIP_SET key
/// no longer matches `relative_to_examples_dir()`'s output (e.g. a
/// path-separator change or a stray prefix) and a skip has silently stopped
/// taking effect — `skip_set_entries_exist_under_examples_dir` alone cannot
/// catch this, since it only proves each key *joins* onto a real file, not
/// that the key string equals the one `exercised_paths` filters on.
#[test]
fn discovery_floor_tracks_the_live_corpus() {
    let paths = discover_ri_files();
    let total = paths.len();
    let exercised = exercised_paths(&paths).len();

    assert_eq!(
        total - exercised,
        SKIP_SET.len(),
        "exercised_paths excluded {} of {} SKIP_SET entries — SKIP_SET keys \
         no longer match relative_to_examples_dir() output",
        total - exercised,
        SKIP_SET.len()
    );

    assert!(
        MIN_EXERCISED_RI_FILES * 2 >= exercised,
        "MIN_EXERCISED_RI_FILES ({}) has drifted stale: the live examples/ \
         corpus now exercises {} .ri files ({} discovered, {} in SKIP_SET), \
         more than 2x the floor. Raise MIN_EXERCISED_RI_FILES to ~{} (its \
         derived sibling MIN_DISCOVERED_RI_FILES will follow automatically) \
         and re-review both constants' tripwire doc comments.",
        MIN_EXERCISED_RI_FILES,
        exercised,
        total,
        SKIP_SET.len(),
        exercised * 3 / 4
    );
}

/// Pins the single-source-of-`exercised` invariant that [`exercised_paths`]'s
/// doc comment claims: the memoized [`ctor_conformance_corpus_walk`] must take
/// its exercised set FROM that helper rather than re-deriving the `SKIP_SET`
/// filter itself.
///
/// Without this the same quantity has two independent derivations — the gate's
/// floor in [`no_example_emits_ctor_field_conformance_diagnostics`] reads the
/// walk's count, while [`discovery_floor_tracks_the_live_corpus`] measures
/// `exercised_paths`. They agree today, so this is a characterization test; it
/// exists so that if a later edit reintroduces a second filter the divergence
/// fails HERE, naming the invariant, instead of silently invalidating the
/// ratchet's freshness claim about the floor the gate actually enforces.
///
/// Free to run: [`ctor_conformance_corpus_walk`] is memoized behind a
/// `OnceLock`, so this reuses the corpus pass the sibling gate already paid
/// for rather than compiling anything a second time.
///
/// Deliberately does NOT re-assert the floor — that is the gate's job. This
/// test is about the two derivations AGREEING, not about how large they are.
#[test]
fn ctor_conformance_walk_exercises_exactly_the_exercised_paths_set() {
    let walk_exercised = ctor_conformance_corpus_walk().exercised;
    let helper_exercised = exercised_paths(&discover_ri_files()).len();

    assert_eq!(
        walk_exercised, helper_exercised,
        "ctor_conformance_corpus_walk() reported {} exercised .ri files but \
         exercised_paths() yields {} — the walk must derive its exercised set \
         from exercised_paths() instead of re-deriving the SKIP_SET filter, or \
         the gate's MIN_EXERCISED_RI_FILES floor and \
         discovery_floor_tracks_the_live_corpus's ratchet are measuring two \
         different quantities.",
        walk_exercised, helper_exercised
    );
}

/// Parse `path`, compile it with the stdlib prelude, and append an entry to
/// `failures` if either parse errors or Error-severity compile diagnostics are
/// found.  Returns without appending when the file is clean.
///
/// `rel_key` is the `relative_to_examples_dir()` string computed by the caller;
/// it is used as the failure-tuple key and in error messages so that nested
/// files are unambiguous in failure reports.
fn smoke_one(path: &Path, rel_key: &str, failures: &mut Vec<(String, String)>) {
    use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
    use reify_core::{ModulePath, Severity};

    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("examples_smoke: cannot read '{}': {}", rel_key, e));

    // Derive a module name from the file stem (e.g. "m5_geometry_flange").
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let module_path = ModulePath::single(&stem);

    // Parse phase — accumulate, do NOT panic on errors.
    // Use prelude-aware parsing so `Type.Variant` references against stdlib
    // enums (e.g. `CorrosionClass.C5`) resolve as `EnumAccess` nodes — see
    // `parse_with_stdlib` for details.  This matches the `compile_with_stdlib`
    // companion below.
    let parsed = parse_with_stdlib(&source, module_path);
    if !parsed.errors.is_empty() {
        let msgs: Vec<String> = parsed.errors.iter().map(|e| e.message.clone()).collect();
        failures.push((rel_key.to_owned(), msgs.join("\n")));
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
        failures.push((rel_key.to_owned(), errors.join("\n")));
    }
}

/// True when `code` is one of the diagnostic codes emitted by the struct-ctor
/// field-conformance surface (tasks 5302 / 5303 / 4584 / 4598 / 4622 / 4444).
///
/// Kept deliberately in sync with the identically-named helper in
/// `struct_ctor_field_conformance_tests.rs`; integration tests are separate
/// binaries and cannot share a private helper without a support-crate hop, and
/// the set is small enough that duplication is cheaper than the indirection.
fn is_ctor_conformance_code(code: Option<reify_core::diagnostics::DiagnosticCode>) -> bool {
    use reify_core::diagnostics::DiagnosticCode;
    matches!(
        code,
        Some(
            DiagnosticCode::ArgTypeMismatch
                | DiagnosticCode::SelectorKindMismatch
                | DiagnosticCode::TypeNotConformingToTrait
                | DiagnosticCode::TypeNotConformingToStructureRef
                | DiagnosticCode::TypeNotConformingToVector
                | DiagnosticCode::CtorUnknownField
                | DiagnosticCode::CtorArity
        )
    )
}

/// One ctor-conformance diagnostic observed during the corpus walk.
///
/// Carries the offending param name alongside file / code / message so the gate
/// can waive per SITE rather than per file — a bare `(file, code, message)`
/// triple can only be matched on the whole message, which would make a waiver
/// hostage to diagnostic wording.
#[derive(Debug)]
struct CtorConformanceViolation {
    /// `relative_to_examples_dir` key of the emitting file.
    file: String,
    /// Offending param name, when the message carries one in the
    /// `argument '<name>'` shape. `None` for the ctor-conformance codes whose
    /// wording names no param — those can never be waived, which is the
    /// intended conservative default.
    param: Option<String>,
    /// `Debug` rendering of the `DiagnosticCode`, for the failure report.
    code: String,
    message: String,
}

/// The result of one full pass over the example corpus.
struct CtorConformanceWalk {
    /// Number of files actually compiled (i.e. discovered minus `SKIP_SET`).
    exercised: usize,
    violations: Vec<CtorConformanceViolation>,
}

/// The corpus walk, computed once per test binary and shared by every gate that
/// needs it.
///
/// Compiling all ~250 examples is the single most expensive thing this binary
/// does. `no_example_emits_ctor_field_conformance_diagnostics` and
/// `ctor_conformance_migration_debt_entries_are_all_live` need exactly the same
/// data, so memoizing keeps the second guard free rather than doubling the
/// gate's wall-clock.
fn ctor_conformance_corpus_walk() -> &'static CtorConformanceWalk {
    use std::sync::OnceLock;

    static WALK: OnceLock<CtorConformanceWalk> = OnceLock::new();
    WALK.get_or_init(|| {
        let mut violations: Vec<CtorConformanceViolation> = Vec::new();
        let paths = discover_ri_files();
        let exercised_list = exercised_paths(&paths);
        let exercised = exercised_list.len();

        for (path, rel_key) in &exercised_list {
            ctor_conformance_one(path, rel_key, &mut violations);
        }

        CtorConformanceWalk {
            exercised,
            violations,
        }
    })
}

/// The `emit_arg_type_mismatch` message prefix that introduces the offending
/// param name (`crates/reify-compiler/src/conformance/mod.rs`).
const CTOR_DIAGNOSTIC_ARG_PREFIX: &str = "argument '";

/// Recover the offending param name from a ctor-conformance diagnostic message.
///
/// A `Diagnostic` carries no structured param field, so the only handle the
/// per-site waiver has is the wording: the text between the first pair of single
/// quotes following the `argument '` prefix. Returns `None` for any message that
/// does not have that shape (the non-`ArgTypeMismatch` ctor-conformance codes),
/// which makes such a diagnostic unwaivable rather than silently waived.
///
/// This is a real coupling to diagnostic prose, and it is deliberately guarded
/// rather than merely commented: if the wording ever drifts so extraction stops
/// matching, [`ctor_conformance_migration_debt_entries_are_all_live`] goes red
/// naming the entry that stopped matching.
fn param_name_from_ctor_diagnostic(message: &str) -> Option<String> {
    let start = message.find(CTOR_DIAGNOSTIC_ARG_PREFIX)? + CTOR_DIAGNOSTIC_ARG_PREFIX.len();
    let rest = &message[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_owned())
}

/// Whether `entry` (a [`CTOR_CONFORMANCE_MIGRATION_DEBT`] row) waives `v`.
///
/// Both halves of the key must match: the file AND the param. An entry whose
/// param does not match — including because extraction returned `None` — waives
/// nothing.
fn debt_entry_matches(entry: &(&str, &str, &str), v: &CtorConformanceViolation) -> bool {
    entry.0 == v.file && v.param.as_deref() == Some(entry.1)
}

/// Whether any debt entry waives `v`.
fn violation_is_waived(v: &CtorConformanceViolation) -> bool {
    CTOR_CONFORMANCE_MIGRATION_DEBT
        .iter()
        .any(|entry| debt_entry_matches(entry, v))
}

/// Expiry guard: every [`CTOR_CONFORMANCE_MIGRATION_DEBT`] entry must still
/// waive at least one live diagnostic.
///
/// A waiver that outlives the thing it waives is worse than no waiver: it is a
/// silent, permanent hole in the gate at a `(file, param)` pair nobody is
/// looking at any more. Liveness is the cheapest predicate with exactly the
/// right shape — the instant #5847 dimensions the two printer sites, these
/// entries match nothing and this test goes red naming them.
///
/// It doubles as the drift guard on [`param_name_from_ctor_diagnostic`]'s
/// coupling to diagnostic wording: if extraction stops matching, every entry
/// goes stale at once and this fails loudly, instead of the gate silently
/// waiving nothing (noisy but visible) or — after a careless "fix" — everything.
///
/// A separate `#[test]` rather than extra assertions inside the gate, so waiver
/// ROT and a corpus REGRESSION are distinguishable by test name.
#[test]
fn ctor_conformance_migration_debt_entries_are_all_live() {
    let walk = ctor_conformance_corpus_walk();

    let stale: Vec<String> = CTOR_CONFORMANCE_MIGRATION_DEBT
        .iter()
        .filter(|entry| !walk.violations.iter().any(|v| debt_entry_matches(entry, v)))
        .map(|(file, param, owner)| format!("  {} :: param '{}'  (owner {})", file, param, owner))
        .collect();

    assert!(
        stale.is_empty(),
        "CTOR_CONFORMANCE_MIGRATION_DEBT has {} stale entry/entries — each waives no live \
         diagnostic:\n{}\n\n\
         Either the site was migrated (the expected case: DELETE the entry, in the same diff \
         that migrated it), or `param_name_from_ctor_diagnostic` no longer matches the \
         `emit_arg_type_mismatch` wording in \
         crates/reify-compiler/src/conformance/mod.rs (then FIX the extraction — do not \
         delete the entries, the waiver would be silently waiving nothing).",
        stale.len(),
        stale.join("\n"),
    );
}

/// Sanity guard mirroring [`skip_set_entries_exist_under_examples_dir`]: every
/// debt entry must name a file that actually exists under `examples/`.
///
/// Cheap, and it separates "mis-typed path" from "already migrated" — both of
/// which would otherwise surface only as a stale-entry failure above.
#[test]
fn ctor_conformance_migration_debt_entries_exist_under_examples_dir() {
    for (rel_path, param, owner) in CTOR_CONFORMANCE_MIGRATION_DEBT {
        let path = Path::new(EXAMPLES_DIR).join(rel_path);
        assert!(
            path.exists(),
            "CTOR_CONFORMANCE_MIGRATION_DEBT entry '{}' (param '{}', owner {}) does not exist \
             under {}",
            rel_path,
            param,
            owner,
            EXAMPLES_DIR,
        );
    }
}

/// The two waiver lists must name disjoint files.
///
/// A `SKIP_SET` file is never walked, so a debt entry for one could never waive
/// anything — it would be dead on arrival, and the only symptom would be the
/// liveness failure above, which reads as "already migrated" and invites exactly
/// the wrong fix. Asserting disjointness directly names the real problem.
#[test]
fn ctor_conformance_migration_debt_is_disjoint_from_skip_set() {
    use std::collections::HashSet;

    let skip: HashSet<&str> = SKIP_SET.iter().map(|(name, _)| *name).collect();
    let overlap: Vec<&str> = CTOR_CONFORMANCE_MIGRATION_DEBT
        .iter()
        .map(|(file, _, _)| *file)
        .filter(|file| skip.contains(file))
        .collect();

    assert!(
        overlap.is_empty(),
        "these files are in BOTH SKIP_SET and CTOR_CONFORMANCE_MIGRATION_DEBT: {:?}.\n\
         A skipped file is never compiled by the corpus walk, so its debt entries waive \
         nothing. Decide which list the file belongs in: SKIP_SET if it cannot reach a clean \
         compile at all, CTOR_CONFORMANCE_MIGRATION_DEBT if it compiles cleanly and merely \
         carries un-migrated ctor call sites.",
        overlap,
    );
}

/// Parse `path`, compile it with the stdlib prelude, and append one entry to
/// `violations` for EVERY ctor-conformance-coded diagnostic found, regardless of
/// severity.
///
/// Files whose parse phase fails contribute nothing: they cannot produce
/// meaningful compile diagnostics, and their parse breakage is already the
/// business of [`all_examples_parse_and_compile_with_stdlib`].  Splitting the
/// concerns this way keeps a parse regression from being reported twice under
/// two different failure headings.
fn ctor_conformance_one(
    path: &Path,
    rel_key: &str,
    violations: &mut Vec<CtorConformanceViolation>,
) {
    use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
    use reify_core::ModulePath;

    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("examples_smoke: cannot read '{}': {}", rel_key, e));

    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let parsed = parse_with_stdlib(&source, ModulePath::single(&stem));
    if !parsed.errors.is_empty() {
        return;
    }

    let compiled = compile_with_stdlib(&parsed);
    for d in compiled
        .diagnostics
        .iter()
        .filter(|d| is_ctor_conformance_code(d.code))
    {
        violations.push(CtorConformanceViolation {
            file: rel_key.to_owned(),
            param: param_name_from_ctor_diagnostic(&d.message),
            code: format!("{:?}", d.code.expect("filtered to Some(code) above")),
            message: d.message.clone(),
        });
    }
}

// ─── best_practices/ corpus catalogue guard (task #5397) ─────────────────────
//
// The `examples/best_practices/` exemplar corpus is *compile*-gated for free by
// this file: `discover_ri_files` walks `examples/` recursively, so every
// exemplar is already covered by `all_examples_parse_and_compile_with_stdlib`
// and `no_example_emits_ctor_field_conformance_diagnostics` with no new infra.
//
// What is NOT covered for free is the corpus's hand-maintained catalogue. The
// `reify-design` skill's session-wrap probe-graduation hook tells agents to grep
// `examples/best_practices/INDEX.md` *first* to decide whether an idiom is
// already captured. That grep is only trustworthy if the index and the
// directory cannot silently diverge: a graduated probe with no index entry is
// invisible to exactly the lookup the hook prescribes, which reintroduces the
// capability≠use drift the corpus exists to fight.
//
// The guard lives HERE rather than in its own `tests/*.rs` binary on purpose.
// It is the same class of catalogue-drift sanity check as
// `skip_set_entries_exist_under_examples_dir` above, over the same directory
// tree, and it reuses `EXAMPLES_DIR`. A new standalone integration binary in
// `reify-compiler` would also be a re-accretion violation of the C1 harness
// layout contract (PRD docs/prds/merge-gate-compile-cost.md §5; gated by
// scripts/check-harness-baseline-registration.sh) — folding it into this
// existing compile unit adds no link to the merge gate at all, which is what
// that ratchet is for.

/// Subdirectory of `examples/` holding the best-practices exemplar corpus.
const CORPUS_SUBDIR: &str = "best_practices";

/// Basename of the hand-maintained catalogue that must stay in step with the
/// corpus directory.
const CORPUS_INDEX_NAME: &str = "INDEX.md";

/// Repo-relative prefix accepted on a path-qualified index reference, so an
/// entry may be written either bare (`negation.ri`) or fully qualified
/// (`examples/best_practices/negation.ri`) without changing its meaning.
const CORPUS_PREFIX: &str = "examples/best_practices/";

/// Appended to every corpus-drift failure report.  The guard fires most often on
/// a half-finished probe graduation, so the message has to state the remedy
/// rather than merely the symptom — a future agent that lands an exemplar and
/// forgets its index entry gets told exactly what to do, with no need to read
/// this file or the skill.
const GRADUATION_HOOK_HINT: &str = "\
The `examples/best_practices/` corpus and its INDEX.md are maintained together \
by the probe-graduation hook in `.claude/skills/reify-design/SKILL.md` \
(\"Session wrap — graduate your probes\"): landing an exemplar .ri file and \
adding its INDEX.md entry are ONE step, never two.\n\
  * missing index entry  -> add a one-line INDEX.md entry naming the file and \
the idiom it demonstrates (do NOT delete the exemplar to silence this).\n\
  * missing file         -> the index names an exemplar that is not there; \
restore the file, or drop/repair the stale entry.\n\
Re-verify with: cargo test -p reify-compiler --test harness_compilation_surface examples_smoke::";

/// Bidirectional filename correspondence between the `examples/best_practices/`
/// corpus directory and its `INDEX.md`:
///
/// 1. every `*.ri` file in the corpus directory is named somewhere in
///    `INDEX.md`; and
/// 2. every bare `*.ri` filename named in `INDEX.md` is a file that actually
///    exists in the corpus directory.
///
/// It deliberately asserts nothing about prose, wording, ordering, or entry
/// format — those are documentation, and pinning them would just make the index
/// expensive to edit.  Structural correspondence is the whole contract.
///
/// Every violation is accumulated and reported in one panic: the guard exists
/// for catalogue-wide visibility, so it must not fail fast and hide the second
/// through Nth drifted entry behind the first.
#[test]
fn best_practices_index_matches_corpus_directory() {
    let dir = Path::new(EXAMPLES_DIR).join(CORPUS_SUBDIR);
    let index_path = dir.join(CORPUS_INDEX_NAME);
    let mut violations: Vec<String> = Vec::new();

    // ── Preconditions ────────────────────────────────────────────────────
    // A missing directory or index makes every downstream check vacuous, so
    // report and stop rather than emit a cascade of derived noise.
    if !dir.is_dir() {
        violations.push(format!(
            "corpus directory '{}' does not exist (or is not a directory)",
            dir.display()
        ));
        report_corpus_violations(violations);
        return;
    }
    if !index_path.is_file() {
        violations.push(format!(
            "'{}' is missing — the corpus directory exists but has no catalogue",
            index_path.display()
        ));
        report_corpus_violations(violations);
        return;
    }

    let ri_files = corpus_ri_files(&dir);
    if ri_files.is_empty() {
        violations.push(format!(
            "corpus directory '{}' contains no *.ri exemplars — an empty \
             best-practices corpus is drift, not a valid state",
            dir.display()
        ));
    }

    let index_text = std::fs::read_to_string(&index_path).unwrap_or_else(|e| {
        panic!(
            "examples_smoke: cannot read '{}': {}",
            index_path.display(),
            e
        )
    });
    let referenced = index_ri_references(&index_text);

    // ── Direction 1: file on disk -> entry in INDEX.md ───────────────────
    for name in &ri_files {
        if !referenced.iter().any(|r| r == name) {
            violations.push(format!(
                "'{}' exists in the corpus but is never named in {}",
                name, CORPUS_INDEX_NAME
            ));
        }
    }

    // ── Direction 2: entry in INDEX.md -> file on disk ───────────────────
    for name in &referenced {
        if !ri_files.iter().any(|f| f == name) {
            violations.push(format!(
                "{} names '{}' but no such file exists in the corpus directory",
                CORPUS_INDEX_NAME, name
            ));
        }
    }

    report_corpus_violations(violations);
}

/// Panic once with a combined, actionable report if `violations` is non-empty.
fn report_corpus_violations(violations: Vec<String>) {
    if violations.is_empty() {
        return;
    }
    let n = violations.len();
    let lines: Vec<String> = violations.iter().map(|v| format!("  - {}", v)).collect();
    panic!(
        "best_practices corpus/INDEX.md drift: {} violation(s).\n\n{}\n\n{}",
        n,
        lines.join("\n"),
        GRADUATION_HOOK_HINT
    );
}

/// Basenames of the `*.ri` files directly inside `dir`, sorted for deterministic
/// reporting.
///
/// Deliberately a flat (non-recursive) read: the corpus is a single flat drawer
/// of idiom exemplars by design, and a nested subdirectory appearing here is a
/// structural change that should be reviewed rather than silently absorbed by
/// the guard.
fn corpus_ri_files(dir: &Path) -> Vec<String> {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "examples_smoke: cannot read directory '{}': {}",
            dir.display(),
            e
        )
    });
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.expect("IO error reading best_practices dir entry");
        let path: PathBuf = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ri") {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            names.push(name.to_string());
        }
    }
    names.sort();
    names
}

/// Corpus-local `*.ri` filenames referenced anywhere in `text`, deduplicated and
/// sorted.
///
/// A reference counts when it is either a bare basename (`negation.ri`) or is
/// qualified with the corpus's own repo-relative prefix
/// (`examples/best_practices/negation.ri`).  A `*.ri` token pointing anywhere
/// else in the repo — e.g. the `examples/kernel_queries/intersects_smoke.ri`
/// cross-references the exemplars are expected to carry — is NOT a corpus
/// membership claim and is ignored, so cross-referencing a proven-green file
/// elsewhere in `examples/` never trips this guard.
fn index_ri_references(text: &str) -> Vec<String> {
    let mut refs: Vec<String> = Vec::new();
    let mut current = String::new();

    // Markdown decoration (backticks, brackets, parens, commas, whitespace) all
    // fall outside this set, so it splits `` `negation.ri` `` and
    // `[negation.ri](negation.ri)` into clean tokens without special-casing any
    // particular link style.
    let is_token_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/');

    for ch in text.chars() {
        if is_token_char(ch) {
            current.push(ch);
        } else {
            push_corpus_ref(&current, &mut refs);
            current.clear();
        }
    }
    push_corpus_ref(&current, &mut refs);

    refs.sort();
    refs.dedup();
    refs
}

/// Normalize one raw token and, if it names a corpus-local `*.ri` file, push its
/// basename onto `refs`.
fn push_corpus_ref(raw: &str, refs: &mut Vec<String>) {
    // Trailing sentence punctuation rides along with the token ("see
    // negation.ri.") — strip it before the extension test.
    let token = raw.trim_end_matches('.');
    if !token.ends_with(".ri") {
        return;
    }
    // Reject a bare ".ri" and glob fragments like "*.ri": no stem, no claim.
    let name = match token.strip_prefix(CORPUS_PREFIX) {
        Some(rest) => rest,
        // Any other path-qualified token points outside the corpus.
        None if token.contains('/') => return,
        None => token,
    };
    if name.contains('/') || name.len() <= ".ri".len() {
        return;
    }
    refs.push(name.to_string());
}
