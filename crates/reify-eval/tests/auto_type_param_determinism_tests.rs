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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use reify_compiler::auto_type_param::{
    CandidateEnumeration, FeasibilityResult, SelectionResult, enumerate_candidates,
    filter_feasible_candidates, select_candidate,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_test_support::{MockConstraintChecker, check_source_with_stdlib, parse_and_compile_with_stdlib};
use reify_types::{ContentHash, DiagnosticCode, Satisfaction, Severity, SourceSpan};

// ─── Constants ───────────────────────────────────────────────────────────────

/// Absolute path to the bearing_auto_seal.ri fixture.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/bearing_auto_seal.ri"
);

/// Absolute path to the workspace `examples/` directory.
/// Mirrors `EXAMPLES_DIR` in `crates/reify-compiler/tests/examples_smoke.rs`.
const EXAMPLES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");

/// Files to skip in the corpus perf regression walk. Each entry is
/// `(relative_path, reason)` — the same `(&str, &str)` shape used in
/// `examples_smoke.rs::SKIP_SET`.
///
/// Mirrored from `crates/reify-compiler/tests/examples_smoke.rs` with
/// attribution; update both sets when a new entry is needed.
const SKIP_SET: &[(&str, &str)] = &[
    (
        "topology_selectors/block_inertia.ri",
        "topology-selectors PRD task 7 worked example; \
         compile_with_stdlib gated on task 2699 (moment_of_inertia language-level wiring) \
         and task 2696 (Tensor surface-syntax + MomentOfInertia named dim). \
         Parse-only smoke is in crates/reify-eval/tests/topology_selector_smoke_tests.rs; \
         full coverage will land via task 2691.",
    ),
    (
        "topology_selectors/fillet_top_edges.ri",
        "topology-selectors PRD task 7 worked example; \
         compile_with_stdlib gated on tasks 2698 (single/flat_map list helpers) \
         and 2699 (faces_by_normal/adjacent_faces/shared_edges language-level wiring). \
         Parse-only smoke is in crates/reify-eval/tests/topology_selector_smoke_tests.rs; \
         full coverage will land via task 2691.",
    ),
];

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Read `bearing_auto_seal.ri`, caching the result in an `OnceLock`.
/// Returns a `&'static str` — no allocation on subsequent calls.
fn source() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(EXAMPLE_PATH)
            .expect("examples/bearing_auto_seal.ri should exist")
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
/// `crates/reify-compiler/tests/auto_type_param_phase_a_tests.rs`.
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
/// `(CandidateEnumeration, FeasibilityResult, SelectionResult, sorted_diag_codes)`.
///
/// - Phase B uses `MockConstraintChecker::with_default(default)`.
/// - Phase C uses `strict` mode (`free = false`).
/// - Diagnostic codes are sorted so that Eq comparison is order-independent
///   if Phase B emits them in non-deterministic order.
fn run_pipeline_with_default(
    module: &CompiledModule,
    default: Satisfaction,
) -> (CandidateEnumeration, FeasibilityResult, SelectionResult, Vec<DiagnosticCode>) {
    let (template_registry, trait_registry) = build_registries(module);

    let bearing = template_registry
        .get("Bearing")
        .expect("Bearing template must exist in bearing_auto_seal.ri");

    let functions: &[reify_types::CompiledFunction] = &[];
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
        CandidateEnumeration::Empty => {
            FeasibilityResult::Empty { rejected: vec![] }
        }
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

    let mut codes: Vec<DiagnosticCode> = diagnostics
        .iter()
        .filter_map(|d| d.code)
        .collect();
    codes.sort_by_key(|c| format!("{c:?}"));

    (enumeration, feasibility, selection, codes)
}

/// Run the pipeline with `Satisfaction::Satisfied` (all candidates accepted).
fn run_pipeline(module: &CompiledModule) -> (CandidateEnumeration, FeasibilityResult, SelectionResult, Vec<DiagnosticCode>) {
    run_pipeline_with_default(module, Satisfaction::Satisfied)
}

/// Compute the "resolved snapshot hash" for a pipeline run.
///
/// As specified by PRD task 7 ("assert that the resolved snapshot hash is
/// identical both times"), the hash is computed over a canonical-string
/// rendering of the Phase A→B→C result tuple. Since source-level
/// `Bearing<auto: Seal>` parsing is not yet supported, this is the
/// algorithm-output stability hash — the closest available proxy until
/// the parser learns `auto:` in type-arg position.
///
/// Uses `ContentHash::of_str` (XXH3-128) consistent with the rest of the
/// codebase (`crates/reify-types/src/expr.rs`).
fn pipeline_snapshot_hash(module: &CompiledModule) -> ContentHash {
    let tuple = run_pipeline(module);
    ContentHash::of_str(&format!("{:?}", tuple))
}

/// Strip `EXAMPLES_DIR` prefix and return a portable forward-slash-separated
/// relative path. Mirrors `relative_to_examples_dir` from `examples_smoke.rs`.
fn relative_to_examples_dir(path: &Path) -> String {
    let rel = path.strip_prefix(EXAMPLES_DIR).unwrap_or_else(|e| {
        panic!(
            "auto_type_param_determinism_tests: '{}' is not under EXAMPLES_DIR ({}): {}",
            path.display(),
            EXAMPLES_DIR,
            e
        )
    });
    rel.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

/// Return all `*.ri` files under `EXAMPLES_DIR` (recursively), sorted.
/// Mirrors `discover_ri_files` from `examples_smoke.rs`.
fn discover_ri_files() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_ri_files(Path::new(EXAMPLES_DIR), &mut paths);
    paths.sort();
    paths
}

/// Recursively collect `*.ri` files under `dir` into `out`.
/// Mirrors `collect_ri_files` from `examples_smoke.rs`.
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
                &result,
                r0,
                "enumerate_candidates result differed on iteration {i} (HashMap re-randomization?)"
            ),
        }
    }
}

// ─── step-5: full A→B→C pipeline is byte-stable across two runs ──────────────

/// Drives the full Phase A→B→C pipeline twice in succession on freshly-built
/// registries and asserts both runs' result tuples are equal AND their
/// sorted diagnostic-code vectors are equal.
#[test]
fn full_pipeline_enumerate_filter_select_is_byte_stable_across_runs() {
    let module = compiled();
    let run_1 = run_pipeline(module);
    let run_2 = run_pipeline(module);
    assert_eq!(
        run_1, run_2,
        "Phase A→B→C pipeline must produce identical results on successive runs"
    );
}

// ─── step-7: snapshot hash is stable across two independent compilations ─────

/// Compiles `bearing_auto_seal.ri` twice **independently** (bypassing the
/// `OnceLock` cache), runs `pipeline_snapshot_hash` on each, and asserts
/// both hashes are equal.
///
/// This is the "resolved snapshot hash is identical both times" assertion
/// from PRD task 7. The two independent compilations exercise HashMap
/// re-randomization between separate `parse_and_compile_with_stdlib` calls.
#[test]
fn pipeline_snapshot_hash_is_stable_across_two_independent_compilations() {
    let hash_1 = pipeline_snapshot_hash(&compile_fresh());
    let hash_2 = pipeline_snapshot_hash(&compile_fresh());
    assert_eq!(
        hash_1, hash_2,
        "resolved snapshot hash must be identical across two independent compilations \
         (HashMap re-randomization must not affect the algorithm output)"
    );
}

// ─── step-9: snapshot hash is stable under NoCandidate arm ──────────────────

/// Drives the pipeline with `MockConstraintChecker::with_default(Violated)` so
/// Phase B's `Empty` arm fires and Phase C emits `E_AUTO_TYPE_PARAM_NO_CANDIDATE`.
/// Computes the snapshot hash over two independent compilations and asserts
/// both are equal.
///
/// Pins determinism of the diagnostic path — a regression that rendered
/// rejected candidates in HashMap-iteration order would flip the hash.
#[test]
fn pipeline_snapshot_hash_is_stable_under_no_candidate_arm() {
    let module_1 = compile_fresh();
    let module_2 = compile_fresh();

    let tuple_1 = run_pipeline_with_default(&module_1, Satisfaction::Violated);
    let tuple_2 = run_pipeline_with_default(&module_2, Satisfaction::Violated);

    let hash_1 = ContentHash::of_str(&format!("{:?}", tuple_1));
    let hash_2 = ContentHash::of_str(&format!("{:?}", tuple_2));

    assert_eq!(
        hash_1, hash_2,
        "NoCandidate-arm snapshot hash must be identical across two independent compilations"
    );

    // Also assert the NoCandidate arm was actually reached (sanity guard).
    assert_eq!(
        tuple_1.2,
        SelectionResult::NoCandidate,
        "expected NoCandidate when all candidates are Violated"
    );
}

// ─── step-11: v0.1 corpus compile+check time is bounded ─────────────────────

/// Walk `examples/*.ri` recursively, skipping SKIP_SET entries, and for each
/// file time `parse_and_compile_with_stdlib + check_source_with_stdlib`.
/// Asserts every per-file duration < 10s AND total elapsed < 120s.
///
/// On failure, prints a sorted `(path, duration)` table so the slow file is
/// immediately visible. Pinned by PRD acceptance criterion 12.
#[test]
fn v0_1_example_corpus_compile_and_check_time_is_bounded() {
    use std::collections::HashSet;

    const PER_FILE_BUDGET: Duration = Duration::from_secs(10);
    const TOTAL_BUDGET: Duration = Duration::from_secs(120);

    let skip: HashSet<&str> = SKIP_SET.iter().map(|(name, _)| *name).collect();
    let paths = discover_ri_files();

    let mut violations: Vec<(String, Duration)> = Vec::new();
    let total_start = Instant::now();

    for path in &paths {
        let rel = relative_to_examples_dir(path);
        if skip.contains(rel.as_str()) {
            continue;
        }

        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));

        let t = Instant::now();
        let module = parse_and_compile_with_stdlib(&src);
        let _ = check_source_with_stdlib(&src);
        let elapsed = t.elapsed();

        let _ = module; // suppress unused warning

        if elapsed > PER_FILE_BUDGET {
            violations.push((rel, elapsed));
        }
    }

    let total_elapsed = total_start.elapsed();

    let mut report_parts: Vec<String> = violations
        .iter()
        .map(|(name, dur)| format!("  {name}: {:.2}s", dur.as_secs_f64()))
        .collect();

    if total_elapsed > TOTAL_BUDGET {
        report_parts.push(format!(
            "  TOTAL: {:.2}s (budget {}s)",
            total_elapsed.as_secs_f64(),
            TOTAL_BUDGET.as_secs()
        ));
    }

    assert!(
        violations.is_empty() && total_elapsed <= TOTAL_BUDGET,
        "v0.1 corpus perf regression detected:\n{}",
        report_parts.join("\n")
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
    let rel_paths: Vec<String> = paths
        .iter()
        .map(|p| relative_to_examples_dir(p))
        .collect();

    assert!(
        rel_paths.iter().any(|r| r == "bearing_auto_seal.ri"),
        "bearing_auto_seal.ri must be in the examples/ corpus discovered by discover_ri_files(); \
         found: {rel_paths:#?}"
    );
}
