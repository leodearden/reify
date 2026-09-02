//! Determinism smoke test and v0.1 example-corpus perf regression guard for
//! the `auto` type-parameter resolution algorithm.
//!
//! PRD: `docs/prds/auto-type-param-resolution.md` task 7, acceptance criteria
//! 11 (determinism) and 12 (perf regression on v0.1 example corpus).
//!
//! # Design decisions
//!
//! Source-level `Bearing<auto: Seal>` parsing is not yet supported
//! (`tree-sitter-reify/grammar.js` `type_arg_list` only allows `$.type_expr`).
//! Tests instead call the Phase A/B/C algorithm functions directly on
//! compiler-built registries, following the same convention as the
//! `auto_type_param_phase_{a,b,c}_tests.rs` siblings in
//! `crates/reify-compiler/tests/`.
//!
//! The fixture `examples/bearing_auto_seal.ri` declares `trait Seal {}`, three
//! Seal-conformant structures in non-alphabetical source order, a
//! `Bearing<T: Seal>` parameterized template, and a concrete
//! `sub bearing = Bearing<ORingSeal>()` occurrence inside `BearingAssembly` so
//! the file compiles cleanly and is auto-discovered by `examples_smoke.rs`.
//!
//! Known coverage gap: all determinism assertions exercise a single-bound
//! list `&["Seal"]`. Multi-bound enumeration (per-bound candidate set
//! intersection) has its own potential iteration-order hazards but is
//! out of scope for v0.1's auto-type-param PRD; add follow-up coverage if
//! multi-bound resolution lands.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use reify_compiler::auto_type_param::{
    CandidateEnumeration, FeasibilityResult, SelectionResult, enumerate_candidates,
    filter_feasible_candidates, select_candidate,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_core::{DiagnosticCode, Severity, SourceSpan};
use reify_ir::Satisfaction;
use reify_test_support::{
    MockConstraintChecker, check_source_with_stdlib, missing_paths_under,
    parse_and_compile_with_stdlib,
};

// ─── Constants ───────────────────────────────────────────────────────────────

/// Absolute path to the bearing_auto_seal.ri fixture.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/bearing_auto_seal.ri"
);

/// Absolute path to the workspace `examples/` directory.
/// Mirrors `EXAMPLES_DIR` in
/// `crates/reify-compiler/tests/harness_compilation_surface/examples_smoke.rs`.
const EXAMPLES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");

/// Conservative absolute floor, chosen below current corpus size to leave
/// headroom for ordinary SKIP_SET churn without flaking this guard. If the
/// corpus is ever intentionally trimmed below this floor, lower the
/// constant to match — the assertion has no way to distinguish an intended
/// shrink from a regression.
///
/// Deliberately NOT relative to paths.len() (e.g. `paths.len() -
/// skip.len()`, or a fraction like `paths.len() / 2`): a
/// discover_ri_files() regression shrinks paths.len() and exercised.len()
/// in lockstep, so any floor derived from paths.len() — subtractive or
/// proportional — still passes and misses exactly the regression this
/// check exists to catch.
///
/// A discovery-regression TRIPWIRE, not a corpus-size target:
/// [`discovery_floor_tracks_the_live_corpus`] is this file's own freshness
/// ratchet, firing once this constant has drifted too far below the live
/// corpus to still catch a regression, and naming a new value to raise it
/// to.
const MIN_EXERCISED_RI_FILES: usize = 200;

/// Classifies *why* a [`SKIP_SET`] entry is excluded from the v0.1
/// example-corpus perf regression walk
/// (`v0_1_example_corpus_compile_and_check_time_is_bounded`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipKind {
    /// The file emits Error-severity diagnostics, so
    /// `check_source_with_stdlib` would PANIC in the corpus walk (see its
    /// `# Panics` docs in `crates/reify-test-support/src/helpers.rs`).
    /// Verified automatically by
    /// `compile_correctness_skips_still_fail_to_compile`.
    CompileError,
    /// The file compiles CLEAN (zero Error-severity diagnostics) but exceeds
    /// the 10s `PER_FILE_BUDGET`. Deliberately NOT verified by
    /// `compile_correctness_skips_still_fail_to_compile` — compiling it
    /// would re-pay the multi-second cost the skip exists to avoid.
    PerfBudget,
}

/// Files to skip in the corpus perf regression walk. Each entry is
/// `(relative_path, kind, reason)`, where `relative_path` is forward-slash
/// separated and rooted at `examples/` and `kind` is a [`SkipKind`]
/// classifying why the entry exists.
///
/// This set and `examples_smoke.rs::SKIP_SET` serve different contracts —
/// that sibling set is correctness-only (its own header describes it as
/// "reserved for files that do not yet compile"), while this set mixes
/// `SkipKind::CompileError` entries (needed because
/// `check_source_with_stdlib` panics on a parse or compile error — see its
/// `# Panics` docs in `crates/reify-test-support/src/helpers.rs`) with
/// `SkipKind::PerfBudget` entries that compile clean but exceed
/// `PER_FILE_BUDGET` and are deliberately not mirrored into
/// `examples_smoke.rs::SKIP_SET`. No fixed relation (equal, superset, or
/// subset) between the two sets is asserted or enforced anywhere, and none
/// should be assumed from either set's contents at any given moment — each
/// crate's fixtures can gain or lose entries independently of the other.
///
/// Instead, each set validates itself locally. Here,
/// `compile_correctness_skips_still_fail_to_compile` below asserts every
/// `SkipKind::CompileError` entry still fails to compile, and
/// `skip_set_entries_exist_under_examples_dir` asserts every entry names a
/// path that still exists. `examples_smoke.rs` has an analogous existence
/// guard (also named `skip_set_entries_exist_under_examples_dir`) but no
/// stale-compile-error guard — its `SKIP_SET` is correctness-only, so
/// nothing there re-derives whether a skip's reason still holds.
/// There is no cross-crate parity assertion between the two `SKIP_SET`
/// consts — both are private to separate crates' `tests/` integration
/// binaries, so neither can import the other to compare directly.
const SKIP_SET: &[(&str, SkipKind, &str)] = &[
    (
        "trajectory/tots_optimal_ptp.ri",
        SkipKind::PerfBudget,
        "complex TOTS SQP example (task 3872) exceeds the 10s per-file compile \
         budget on loaded CI (~13.4s observed). This file DOES compile cleanly \
         — it is a perf-only skip (SkipKind::PerfBudget) and is deliberately \
         NOT mirrored into examples_smoke.rs::SKIP_SET (which is \
         reserved for files that do not yet compile). Compile-correctness stays \
         covered by examples_smoke.rs::all_examples_parse_and_compile_with_stdlib \
         and crates/reify-compiler/tests/tots_optimal_ptp_example_tests.rs.",
    ),
    (
        "trajectory/printer_print_envelope.ri",
        SkipKind::PerfBudget,
        "complex four-PRD stack dogfood example (task 3878) that exceeds the 10s \
         per-file compile budget on loaded CI (~14.7s observed). Task 4547 step-8 \
         removed ProfileInput/ShaperInput/GcodeDialectInput coercion shims and \
         switched to passing concretes directly to trait-typed params — the \
         entity-scope conformance post-pass now does additional work per call \
         site, systematically raising compile time. This file DOES compile cleanly \
         — it is a perf-only skip and is deliberately NOT mirrored into \
         examples_smoke.rs::SKIP_SET (which is reserved for files that do not yet \
         compile). Compile-correctness stays covered by \
         examples_smoke.rs::all_examples_parse_and_compile_with_stdlib, \
         crates/reify-compiler/tests/printer_print_envelope_example_tests.rs, \
         and crates/reify-eval/tests/printer_print_envelope_e2e.rs.",
    ),
    (
        "auto/bearing_constraint_select.ri",
        SkipKind::CompileError,
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
        "auto/bearing_unsat.ri",
        SkipKind::CompileError,
        "ζ negative fixture: strict `auto: Seal` with TWO candidates that BOTH violate the \
         member constraint `seal.thickness < bore_radius=3mm` (ThickSeal=5mm, HugeSeal=8mm). \
         Emits an Error under any checker (NoCandidate under real checker, Ambiguous under stub) \
         — check_source_with_stdlib panics on compile errors. Exercised by task ζ's reify-eval \
         auto_type_param_completion_e2e harness (bearing_unsat_emits_no_candidate_naming_constraint). \
         Mirrored from examples_smoke.rs::SKIP_SET (task 4437 ζ).",
    ),
    (
        "auto/bearing_computed_default_unevaluated.ri",
        SkipKind::CompileError,
        "Gap-C fixture (task #4616): strict `auto: Seal` with a computed-default template \
         cell (`clearance = bore_radius - 0.5mm`) whose default is a non-literal BinOp. \
         The literal-only seeder skips `clearance`, so the constraint `seal.thickness < \
         clearance` evaluates to Indeterminate for every candidate. Under any checker \
         (stub or real) both ThinSeal and ThickSeal are feasible → ≥2 feasible → \
         E_AUTO_TYPE_PARAM_AMBIGUOUS Error — check_source_with_stdlib panics on compile errors. \
         Exercised by task #4616's reify-eval e2e regression gate \
         (gap_c_computed_default_unevaluated_emits_warning_literal_does_not). \
         Mirrored from examples_smoke.rs::SKIP_SET (task #4616).",
    ),
    (
        "conditional_compilation/main.ri",
        SkipKind::CompileError,
        "Multi-file cfg-gated entry: `param p : Platform` in type position resolves only \
         through the #cfg(target)-gated import (platform_linux or platform_wasm), using the \
         reify check cfg DAG (compile_entry_with_stdlib_cfg_checked). The single-file \
         check_source_with_stdlib path cannot follow gated imports, so `Platform` is an \
         unresolved-type Error — check_source_with_stdlib panics on compile errors. \
         The two-way symmetric behaviour (both --cfg target=linux and --cfg target=wasm \
         exit 0, each resolving the platform-correct Platform variant) is exercised \
         end-to-end by crates/reify-cli/tests/harness_cli/cli_check_cfg_example.rs. \
         The siblings (platform_linux.ri, platform_wasm.ri) compile clean single-file \
         and are intentionally NOT skipped. \
         Mirrored from examples_smoke.rs::SKIP_SET (task 3995 ε).",
    ),
    (
        "module_visibility/consumer.ri",
        SkipKind::CompileError,
        "Cross-module consumer that fails the single-file smoke for two independent reasons: \
         (1) its `import producer` edge cannot be followed by check_source_with_stdlib (Motor is \
         unresolved → unresolved-type Error on `sub m = Motor()`); \
         (2) the `let hidden = m.rated_torque` dot-access is a by-design E_PRIV_MEMBER_ACCESS \
         Error (rated_torque is priv on Motor). The priv-param-hidden / visible-param-resolves \
         two-way signal is exercised end-to-end by \
         crates/reify-cli/tests/harness_cli/cli_module_visibility_example.rs via the real `reify check` \
         binary. The siblings (producer.ri, mismatch_variant.ri) are self-contained / \
         CLI-only-diagnostic and are intentionally NOT skipped. \
         Mirrored from examples_smoke.rs::SKIP_SET (task 3980 ε).",
    ),
    (
        "multi_aspect_objective_mixed.ri",
        SkipKind::CompileError,
        "PRD δ (#5020) BT1 negative fixture — intentionally emits \
         E_OBJECTIVE_MIXED_DIMENSION (ObjectiveDimensionIncoherent) at compile: two \
         same-sense minimize decls over incommensurable dimensions (Money cost, Mass \
         mass) lower to a 2-term WeightedSum that fails α's units-coherence guard — \
         check_source_with_stdlib panics on compile errors. Positive (coherent) \
         coverage lives in the sibling multi_aspect_objective.ri (NOT skipped) and \
         both are exercised end-to-end by \
         crates/reify-eval/tests/harness_fea_solver_e2e/multi_aspect_objective_example_e2e.rs. \
         Mirrored from examples_smoke.rs::SKIP_SET (task δ #5020).",
    ),
];

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Read `bearing_auto_seal.ri`, caching the result in an `OnceLock`.
/// Returns a `&'static str` — no allocation on subsequent calls.
fn source() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/bearing_auto_seal.ri should exist")
    })
    .as_str()
}

/// Parse and compile (with stdlib) the fixture, caching the result.
/// Returns a `&'static CompiledModule` — no clone on subsequent calls.
fn compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(source()))
}

/// Parse and compile (with stdlib) the fixture WITHOUT caching.
/// Two independent calls exercise HashMap re-randomization between
/// separate module-build invocations (the snapshot-hash determinism test).
fn compile_fresh() -> CompiledModule {
    parse_and_compile_with_stdlib(source())
}

/// Build the `(template_registry, trait_registry)` pair from a compiled module.
///
/// Mirrors the construction shape used internally by
/// `compile_builder::entities_phase::phase_pending_bound_checks` (lines
/// 246-253 in entities_phase.rs) and copied from
/// `crates/reify-compiler/tests/harness_auto_binding/auto_type_param_phase_a_tests.rs`.
fn build_registries(
    module: &CompiledModule,
) -> (
    HashMap<String, &TopologyTemplate>,
    HashMap<String, &CompiledTrait>,
) {
    let template_registry: HashMap<String, &TopologyTemplate> = module
        .templates
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect();
    let trait_registry: HashMap<String, &CompiledTrait> = module
        .trait_defs
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect();
    (template_registry, trait_registry)
}

/// Run the full Phase A→B→C pipeline on the given module and return
/// `(CandidateEnumeration, FeasibilityResult, SelectionResult, diag_codes)`.
///
/// - Phase B uses `MockConstraintChecker::with_default(default)`.
/// - Phase C uses `strict` mode (`free = false`).
/// - Diagnostic codes are returned in emission order (NOT sorted), so that
///   non-deterministic emission order is detectable via `assert_eq!`.
fn run_pipeline_with_default(
    module: &CompiledModule,
    default: Satisfaction,
) -> (
    CandidateEnumeration,
    FeasibilityResult,
    SelectionResult,
    Vec<DiagnosticCode>,
) {
    let (template_registry, trait_registry) = build_registries(module);

    let bearing = template_registry
        .get("Bearing")
        .expect("Bearing template must exist in bearing_auto_seal.ri");

    let functions: &[reify_ir::CompiledFunction] = &[];
    let checker = MockConstraintChecker::new().with_default(default);

    let mut diagnostics = Vec::new();

    let enumeration = enumerate_candidates(
        &["Seal".to_string()],
        &template_registry,
        &trait_registry,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    let feasibility = match &enumeration {
        CandidateEnumeration::Found(candidates) => {
            filter_feasible_candidates(candidates, bearing, &checker, functions)
        }
        CandidateEnumeration::Empty => FeasibilityResult::Empty { rejected: vec![] },
        CandidateEnumeration::Overflow(_) => {
            // Overflow is a hard error; skip feasibility as specified.
            FeasibilityResult::Empty { rejected: vec![] }
        }
    };

    let selection = select_candidate(
        feasibility.clone(),
        &["Seal".to_string()],
        false, // strict mode
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    let codes: Vec<DiagnosticCode> = diagnostics.iter().filter_map(|d| d.code).collect();

    (enumeration, feasibility, selection, codes)
}

/// Run the pipeline with `Satisfaction::Satisfied` (all candidates accepted).
fn run_pipeline(
    module: &CompiledModule,
) -> (
    CandidateEnumeration,
    FeasibilityResult,
    SelectionResult,
    Vec<DiagnosticCode>,
) {
    run_pipeline_with_default(module, Satisfaction::Satisfied)
}

/// Strip `EXAMPLES_DIR` prefix and return a portable forward-slash-separated
/// relative path. Mirrors `relative_to_examples_dir` from `examples_smoke.rs`
/// — update both when this changes.
fn relative_to_examples_dir(path: &Path) -> String {
    let rel = path.strip_prefix(EXAMPLES_DIR).unwrap_or_else(|e| {
        panic!(
            "auto_type_param_determinism_tests: '{}' is not under EXAMPLES_DIR ({}): {}",
            path.display(),
            EXAMPLES_DIR,
            e
        )
    });
    rel.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// Return all `*.ri` files under `EXAMPLES_DIR` (recursively), sorted.
/// Mirrors `discover_ri_files` from `examples_smoke.rs` — update both when
/// this changes.
fn discover_ri_files() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_ri_files(Path::new(EXAMPLES_DIR), &mut paths);
    paths.sort();
    paths
}

/// Recursively collect `*.ri` files under `dir` into `out`.
/// Mirrors `collect_ri_files` from `examples_smoke.rs` — update both when
/// this changes.
fn collect_ri_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "auto_type_param_determinism_tests: cannot read directory '{}': {}",
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
/// both `v0_1_example_corpus_compile_and_check_time_is_bounded` and
/// [`discovery_floor_tracks_the_live_corpus`] call this instead of
/// re-deriving the filter, so they can never disagree about what
/// "exercised" means.
fn exercised_paths(paths: &[PathBuf]) -> Vec<(&PathBuf, String)> {
    use std::collections::HashSet;

    let skip: HashSet<&str> = SKIP_SET.iter().map(|(name, _, _)| *name).collect();
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

// ─── step-1: fixture compiles with three Seal candidates ─────────────────────

/// Load `bearing_auto_seal.ri` via `parse_and_compile_with_stdlib`, assert no
/// error-severity diagnostics, build the registry pair, and assert
/// `enumerate_candidates(&["Seal"])` returns the three Seal-conformant
/// structures in alphabetical order (NOT source declaration order).
///
/// The fixture declares them as `NitrileSeal`, `ORingSeal`, `GasketSeal`
/// (non-alphabetical). The expected result is `["GasketSeal", "NitrileSeal",
/// "ORingSeal"]` — alphabetical — which pins Phase A's deterministic sort.
#[test]
fn bearing_auto_seal_fixture_compiles_with_three_seal_candidates() {
    let module = compiled();

    // parse_and_compile_with_stdlib already panics on errors, but assert
    // explicitly for documentation.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "bearing_auto_seal.ri should compile with no errors, got: {errors:#?}"
    );

    let (template_registry, trait_registry) = build_registries(module);

    let mut diagnostics = Vec::new();
    let result = enumerate_candidates(
        &["Seal".to_string()],
        &template_registry,
        &trait_registry,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    assert_eq!(
        result,
        CandidateEnumeration::Found(vec![
            "GasketSeal".to_string(),
            "NitrileSeal".to_string(),
            "ORingSeal".to_string(),
        ]),
        "expected alphabetical order [GasketSeal, NitrileSeal, ORingSeal], NOT source order"
    );
    assert!(
        diagnostics.is_empty(),
        "enumerate_candidates should emit no diagnostics for 3-candidate case, got: {diagnostics:#?}"
    );
}

// ─── step-3: enumerate_candidates is byte-stable across 50 iterations ────────

/// Drives `enumerate_candidates` 50 times in a loop, building fresh
/// registries each iteration (exercising HashMap re-randomization), and
/// asserts all 50 results are byte-identical to the first.
///
/// Pins Phase A's alphabetical sort at the integration level using the
/// example fixture — a regression to source-iteration order or HashMap
/// iteration order would break this test.
///
/// Scope: this exercises only per-iteration registry-HashMap re-randomization
/// (the cached `compiled()` module's templates Vec is identical in source
/// order across iterations). Broader cross-compilation determinism — where
/// independent `parse_and_compile_with_stdlib` calls produce identical output
/// — is covered by `pipeline_output_is_stable_across_two_independent_compilations`.
#[test]
fn enumerate_candidates_pipeline_is_byte_stable_across_50_iterations() {
    let module = compiled();
    let mut result_0: Option<CandidateEnumeration> = None;

    for i in 0..50 {
        // Build fresh registries each iteration to exercise HashMap
        // re-randomization across iterations.
        let (template_registry, trait_registry) = build_registries(module);
        let mut diagnostics = Vec::new();
        let result = enumerate_candidates(
            &["Seal".to_string()],
            &template_registry,
            &trait_registry,
            SourceSpan::empty(0),
            &mut diagnostics,
        );

        match &result_0 {
            None => result_0 = Some(result),
            Some(r0) => assert_eq!(
                &result, r0,
                "enumerate_candidates result differed on iteration {i} (HashMap re-randomization?)"
            ),
        }
    }
}

// ─── step-5: full A→B→C pipeline is byte-stable across two invocations ───────

/// Drives the full Phase A→B→C pipeline twice in succession on freshly-built
/// registries derived from the same cached compiled module, and asserts both
/// runs' result tuples and diagnostic-code sequences are identical.
///
/// Note: both invocations share the same `compiled()` module, so this test
/// exercises algorithm-level determinism (HashMap re-randomization in
/// freshly-built registries). Cross-compilation determinism — where two
/// independent `parse_and_compile_with_stdlib` calls produce the same output —
/// is covered by `pipeline_output_is_stable_across_two_independent_compilations`.
#[test]
fn full_pipeline_is_byte_stable_across_two_invocations_on_same_module() {
    let module = compiled();
    let run_1 = run_pipeline(module);
    let run_2 = run_pipeline(module);
    assert_eq!(
        run_1, run_2,
        "Phase A→B→C pipeline must produce identical results on successive invocations \
         with freshly-built registries (same compiled module)"
    );
}

// ─── step-7: pipeline output is stable across two independent compilations ────

/// Compiles `bearing_auto_seal.ri` twice **independently** (bypassing the
/// `OnceLock` cache via `compile_fresh()`), runs `run_pipeline` on each, and
/// asserts both result tuples are equal via `assert_eq!`.
///
/// This is the "resolved snapshot hash is identical both times" assertion from
/// PRD task 7, implemented as direct tuple equality (the all-derive-`Eq` tuple
/// produces an actionable diff on failure, unlike an opaque hash comparison).
/// The two independent compilations exercise HashMap re-randomization between
/// separate `parse_and_compile_with_stdlib` calls.
#[test]
fn pipeline_output_is_stable_across_two_independent_compilations() {
    let tuple_1 = run_pipeline(&compile_fresh());
    let tuple_2 = run_pipeline(&compile_fresh());
    assert_eq!(
        tuple_1, tuple_2,
        "Phase A→B→C pipeline must produce identical output across two independent compilations \
         (HashMap re-randomization must not affect the algorithm output)"
    );
}

// ─── step-9: pipeline output is stable under NoCandidate arm ────────────────

/// Drives the pipeline with `MockConstraintChecker::with_default(Violated)` so
/// Phase B's `Empty` arm fires and Phase C emits `E_AUTO_TYPE_PARAM_NO_CANDIDATE`.
/// Runs over two independent compilations and asserts both result tuples are
/// equal via `assert_eq!`.
///
/// Pins determinism of the diagnostic path — a regression that rendered
/// rejected candidates in HashMap-iteration order would produce unequal tuples.
/// Using direct `assert_eq!` (rather than hashing) gives an actionable diff
/// when the assertion fails.
#[test]
fn pipeline_output_is_stable_under_no_candidate_arm() {
    let module_1 = compile_fresh();
    let module_2 = compile_fresh();

    let tuple_1 = run_pipeline_with_default(&module_1, Satisfaction::Violated);
    let tuple_2 = run_pipeline_with_default(&module_2, Satisfaction::Violated);

    assert_eq!(
        tuple_1, tuple_2,
        "NoCandidate-arm pipeline output must be identical across two independent compilations"
    );

    // Also assert the NoCandidate arm was actually reached (sanity guard).
    assert_eq!(
        tuple_1.2,
        SelectionResult::NoCandidate,
        "expected NoCandidate when all candidates are Violated"
    );

    // Explicit diagnostic-code guarantee: a regression that swallowed or
    // renamed E_AUTO_TYPE_PARAM_NO_CANDIDATE would still satisfy the tuple
    // equality above, so assert the code's existence directly.
    assert!(
        tuple_1
            .3
            .contains(&DiagnosticCode::AutoTypeParamNoCandidate),
        "expected AutoTypeParamNoCandidate (E_AUTO_TYPE_PARAM_NO_CANDIDATE) in emitted diag codes, got: {:?}",
        tuple_1.3
    );
}

// ─── step-11: v0.1 corpus compile+check time is bounded ─────────────────────

/// Return the subset of `measurements` whose duration exceeds `per_file_budget`.
///
/// Pure and deterministic (no aggregate/total-wall-clock concept) — the
/// single source of truth for the per-file perf gate, unit-tested directly by
/// `per_file_gate_violation_contract` below.
fn per_file_violations(
    measurements: &[(String, Duration)],
    per_file_budget: Duration,
) -> Vec<(String, Duration)> {
    measurements
        .iter()
        .filter(|(_, dur)| *dur > per_file_budget)
        .cloned()
        .collect()
}

/// Walk `examples/*.ri` recursively, skipping SKIP_SET entries, and for each
/// file time `check_source_with_stdlib` (which internally calls
/// `parse_and_compile_with_stdlib`, so the measurement covers the full
/// parse+compile+check pipeline exactly once per file).
/// Asserts every per-file duration <= 10s (strict `>` boundary) via
/// `per_file_violations`.
///
/// On failure, prints a `(path, duration)` table sorted alphabetically by
/// file name at print time (independent of `discover_ri_files`'s internal
/// order), so the offending file can be located by name. Pinned by PRD
/// acceptance criterion 12.
///
/// # Budget rationale
///
/// Per-file only, deliberately no aggregate/total wall-clock budget: a
/// fixed total erodes as the corpus grows and flakes under concurrent-verify
/// CPU oversubscription, while the 10s per-file bound has generous headroom
/// against a sub-second baseline and no such failure mode. Full
/// history/tradeoffs: task 5149.
#[test]
fn v0_1_example_corpus_compile_and_check_time_is_bounded() {
    const PER_FILE_BUDGET: Duration = Duration::from_secs(10);

    let paths = discover_ri_files();

    // Resolve the skip-filtered candidate list — and fail fast on the
    // discovery-floor check below — before paying for the per-file
    // compile+check loop. Otherwise a misconfigured discover_ri_files()/
    // SKIP_SET would still burn time compiling whatever few files it found
    // before reporting the regression.
    let exercised = exercised_paths(&paths);

    assert!(
        exercised.len() >= MIN_EXERCISED_RI_FILES,
        "corpus discovery found only {} .ri file(s) (expected >= {}) — \
         raw discover_ri_files() count: {}, SKIP_SET size: {} — \
         discover_ri_files()/SKIP_SET may be misconfigured (e.g. examples dir \
         moved or most entries skipped), which would silently narrow this \
         perf gate's coverage. If this is instead an intentional corpus \
         reduction, lower MIN_EXERCISED_RI_FILES to match. The raw/SKIP_SET counts \
         above distinguish a discovery regression (raw count also low) from an \
         intentional corpus shrink (raw count normal, exercised below floor).",
        exercised.len(),
        MIN_EXERCISED_RI_FILES,
        paths.len(),
        SKIP_SET.len()
    );

    let mut measurements: Vec<(String, Duration)> = Vec::new();

    for (path, rel) in &exercised {
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));

        let t = Instant::now();
        let _ = check_source_with_stdlib(&src);
        let elapsed = t.elapsed();

        measurements.push((rel.clone(), elapsed));
    }

    let mut violations = per_file_violations(&measurements, PER_FILE_BUDGET);
    violations.sort_by(|a, b| a.0.cmp(&b.0));

    let report_parts: Vec<String> = violations
        .iter()
        .map(|(name, dur)| format!("  {name}: {:.2}s", dur.as_secs_f64()))
        .collect();

    assert!(
        violations.is_empty(),
        "v0.1 corpus per-file perf regression detected:\n{}",
        report_parts.join("\n")
    );
}

// ─── step-11 unit guard: per_file_violations pure-helper contract ─────────

/// `per_file_violations` must flag over-budget files but not sub-budget
/// ones, with the boundary strict (`duration == budget` is NOT a
/// violation). Covers under-budget, on-boundary, and two independent
/// over-budget files, pinning the `>` boundary and that the helper returns
/// the full offending set rather than short-circuiting on the first match.
///
/// No aggregate case: `per_file_violations` is a pure per-file filter with
/// no place to hold that logic — see the corpus test's "# Budget rationale"
/// above (task 5149).
#[test]
fn per_file_gate_violation_contract() {
    let measurements: Vec<(String, Duration)> = vec![
        ("fast.ri".to_string(), Duration::from_millis(500)),
        ("slow.ri".to_string(), Duration::from_secs(11)),
        ("edge.ri".to_string(), Duration::from_secs(9)),
        ("boundary.ri".to_string(), Duration::from_secs(10)),
        ("slow2.ri".to_string(), Duration::from_secs(12)),
    ];

    let violations = per_file_violations(&measurements, Duration::from_secs(10));

    // Sort before comparing: `per_file_violations` is documented as a filter
    // (see its doc comment above) with no order-preservation guarantee, so
    // pin the offending *set* rather than incidental iteration order — the
    // corpus test itself sorts violations right after calling this helper,
    // so a future reordering refactor here must not fail this contract test.
    let mut names: Vec<&str> = violations.iter().map(|(name, _)| name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["slow.ri", "slow2.ri"],
        "expected both over-budget files (11s and 12s, > 10s budget) to be flagged as \
         violations — confirming the helper returns the full offending set rather than \
         short-circuiting on the first match; sub-budget files (500ms, 9s) and the \
         exact-boundary file (10s — strict `>` means duration == budget is NOT a \
         violation) must not be flagged, got: {names:?}"
    );
}

// ─── SKIP_SET classification guard ────────────────────────────────────────

/// Every `SkipKind::CompileError` entry in [`SKIP_SET`] must still emit at
/// least one Error-severity diagnostic when compiled with stdlib — that is
/// the premise that justifies skipping it in
/// `v0_1_example_corpus_compile_and_check_time_is_bounded`, whose
/// `check_source_with_stdlib` call PANICS on a compile error (see its
/// `# Panics` docs in `crates/reify-test-support/src/helpers.rs`). If a fix
/// lands elsewhere and the file starts compiling clean, this guard catches
/// the resulting stale skip mechanically instead of letting it silently
/// narrow the corpus walk's coverage forever.
///
/// `SkipKind::PerfBudget` entries are deliberately NOT verified here:
/// compiling them would re-pay the ~13-15s the skip exists to avoid, which
/// would defeat its purpose and materially slow this test file.
///
/// Uses `compile_source_with_stdlib_allow_parse_errors` rather than
/// `compile_source_with_stdlib`: the latter PANICS on a parse error (see its
/// own `# Panics` docs), which would abort this loop before every later
/// entry is checked and defeat the report-everything design below. The
/// `_allow_parse_errors` variant instead folds parse-layer diagnostics into
/// the returned module's `diagnostics`, so a `CompileError` entry that now
/// fails at the parse layer is still correctly counted as "still fails to
/// compile" by `errors_only` rather than aborting the whole test.
///
/// Every offending entry is accumulated and reported in a single panic,
/// mirroring `examples_smoke.rs::all_examples_parse_and_compile_with_stdlib`'s
/// report-everything style, so one run surfaces every stale entry rather
/// than stopping at the first.
///
/// A `CompileError` entry whose path is missing is skipped here rather than
/// read (which would panic on a raw `io::Error`) — `skip_set_entries_exist_under_examples_dir`
/// owns that failure mode and reports it with an actionable message instead.
#[test]
fn compile_correctness_skips_still_fail_to_compile() {
    let missing = missing_skip_set_paths(SKIP_SET, Path::new(EXAMPLES_DIR));
    let mut stale: Vec<String> = Vec::new();

    for (rel, kind, _reason) in SKIP_SET {
        if *kind != SkipKind::CompileError || missing.contains(rel) {
            continue;
        }

        let path = Path::new(EXAMPLES_DIR).join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));

        let module = reify_test_support::compile_source_with_stdlib_allow_parse_errors(&src);
        if reify_test_support::errors_only(&module).is_empty() {
            stale.push(rel.to_string());
        }
    }

    assert!(
        stale.is_empty(),
        "SKIP_SET entry/entries tagged SkipKind::CompileError have gone stale — \
         each named file below now compiles with zero Error-severity diagnostics, \
         so the corpus walk would NOT panic on it. Delete the entry and re-run \
         v0_1_example_corpus_compile_and_check_time_is_bounded, or re-classify it \
         as SkipKind::PerfBudget with a measured timing:\n{}",
        stale
            .iter()
            .map(|s| format!("  {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ─── SKIP_SET existence guard ─────────────────────────────────────────────

/// Return the subset of `entries`' relative paths that do not exist under
/// `examples_dir`, regardless of [`SkipKind`].
///
/// Existence filter: [`reify_test_support::missing_paths_under`], where its
/// contract is documented and unit-tested.
///
/// Kept as a named adapter rather than inlined so `missing_skip_set_paths_contract`
/// below can pin the one piece of behaviour the shared helper cannot see: that
/// the projection discards [`SkipKind`].
fn missing_skip_set_paths<'a>(
    entries: &'a [(&'a str, SkipKind, &'a str)],
    examples_dir: &Path,
) -> Vec<&'a str> {
    missing_paths_under(examples_dir, entries.iter().map(|(rel, _, _)| *rel))
}

/// Sanity guard: every entry in [`SKIP_SET`] must name a relative path that
/// actually exists under `examples/`, regardless of [`SkipKind`] — see the
/// assertion message below for why this matters most for `SkipKind::PerfBudget`
/// keys specifically.
///
/// A guard of the same name in
/// `crates/reify-compiler/tests/harness_compilation_surface/examples_smoke.rs`
/// covers that crate's own skip list. Only the existence filter is shared —
/// this asserts nothing about that file's contents, and no fixed relation
/// between the two skip lists is claimed (see the `SKIP_SET` doc above).
#[test]
fn skip_set_entries_exist_under_examples_dir() {
    let missing = missing_skip_set_paths(SKIP_SET, Path::new(EXAMPLES_DIR));
    assert!(
        missing.is_empty(),
        "SKIP_SET entry/entries name a relative path that does not exist under {}: {}. \
         A stale key silently narrows this file's perf-gate coverage forever (for a \
         SkipKind::PerfBudget key, nothing else in this file would ever notice) — \
         delete the entry or fix the path.",
        EXAMPLES_DIR,
        missing
            .iter()
            .map(|s| format!("'{s}'"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

// ─── SKIP_SET existence guard unit test: missing_skip_set_paths contract ──

/// `missing_skip_set_paths` must discard [`SkipKind`] when projecting entries
/// into the shared existence filter. That projection is the adapter's whole
/// job, and the one behaviour [`reify_test_support::missing_paths_under`]
/// structurally cannot see — it never receives a `SkipKind`.
///
/// The fixture pairs each path against BOTH kinds, so `SkipKind` is the only
/// dimension that varies within a pair: an implementation that filtered on
/// kind would return one copy of the missing path where two are expected, or
/// flag one copy of the present one. Present/missing semantics, input order
/// and the no-short-circuit contract belong to the shared helper and are
/// unit-tested there rather than restated here.
///
/// Hermetic by construction: the "present" fixture lives in a fresh
/// `tempfile::TempDir` rather than the real `examples/` corpus, so an
/// unrelated example rename/deletion can't fail this pure-helper unit test —
/// that live-corpus signal belongs to `skip_set_entries_exist_under_examples_dir`
/// above.
#[test]
fn missing_skip_set_paths_contract() {
    let dir = tempfile::TempDir::new().expect("tempdir creation should succeed");
    std::fs::write(dir.path().join("present.ri"), "// present fixture\n")
        .expect("write present.ri fixture");

    let entries: &[(&str, SkipKind, &str)] = &[
        ("present.ri", SkipKind::PerfBudget, "present, perf"),
        ("present.ri", SkipKind::CompileError, "present, compile"),
        ("gone.ri", SkipKind::PerfBudget, "absent, perf"),
        ("gone.ri", SkipKind::CompileError, "absent, compile"),
    ];

    let names = missing_skip_set_paths(entries, dir.path());

    assert_eq!(
        names,
        vec!["gone.ri", "gone.ri"],
        "expected both entries naming the absent path to be flagged and both naming the \
         materialised one to be skipped, the two members of each pair differing only in \
         SkipKind — a projection that did not discard SkipKind would split a pair; \
         got: {names:?}"
    );
}

// ─── step-13: fixture is included in corpus ───────────────────────────────────

/// Assert that `bearing_auto_seal.ri` is discovered by the corpus walker.
/// Pins the contract: the determinism fixture also participates in the
/// corpus perf regression guard — a future move out of `examples/` would
/// silently lose this cross-coverage.
#[test]
fn v0_1_corpus_includes_bearing_auto_seal_fixture() {
    let paths = discover_ri_files();
    let rel_paths: Vec<String> = paths.iter().map(|p| relative_to_examples_dir(p)).collect();

    assert!(
        rel_paths.iter().any(|r| r == "bearing_auto_seal.ri"),
        "bearing_auto_seal.ri must be in the examples/ corpus discovered by discover_ri_files(); \
         found: {rel_paths:#?}"
    );
}

// ─── step-13 ratchet: MIN_EXERCISED_RI_FILES tracks the live corpus ───────────────

/// Freshness ratchet for [`MIN_EXERCISED_RI_FILES`]: a second-order check on the
/// constant's own value, which never gates
/// [`v0_1_example_corpus_compile_and_check_time_is_bounded`] itself.
/// One-directional by construction — a discovery regression only SHRINKS
/// `exercised`, which makes the assertion below easier to satisfy, so this
/// ratchet can never mask that regression; it only fires once the corpus
/// has grown enough that the floor has lost its tripwire sensitivity; see
/// the assertion message for the current bound.
///
/// Measures `exercised` via the same [`exercised_paths`] helper that the
/// gate calls (not the raw [`discover_ri_files`] count), so this ratchet
/// can never disagree with the gate it tracks. Directory walk only — no
/// `check_source_with_stdlib` — so it adds no measurable time to this
/// binary.
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
        "MIN_EXERCISED_RI_FILES ({}) has drifted stale: the live examples/ corpus \
         now yields {} exercised .ri files ({} discovered, {} in SKIP_SET), \
         more than 2x the floor. Raise MIN_EXERCISED_RI_FILES to ~{} and \
         re-review its tripwire doc comment.",
        MIN_EXERCISED_RI_FILES,
        exercised,
        total,
        SKIP_SET.len(),
        exercised * 3 / 4
    );
}
