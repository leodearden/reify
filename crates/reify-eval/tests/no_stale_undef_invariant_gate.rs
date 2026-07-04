//! Debug-gate integration suite for the no-stale-Undef invariant checker
//! (task α, PRD docs/prds/v0_6/eval-uniform-dependency-handling.md §6.1).
//!
//! Runs `reify_eval::invariants::check_no_stale_undef` — and the
//! `Engine::check_no_stale_undef` convenience wrapper — over the eval
//! fixture corpus + examples/, proving the invariant holds post-eval.
//!
//! Step-1 (RED): the mandatory anti-silent-accept seeded-violation
//! self-test. Fabricates a minimal post-eval state (NOT a real
//! compile+eval) containing one genuine stale-Undef consumer and asserts
//! the checker actually fires — a checker that always returns `vec![]`
//! would otherwise make every downstream corpus test in this suite
//! vacuously green.

use std::collections::HashMap;

use reify_core::{ContentHash, Type, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::deps::DependencyTrace;
use reify_eval::graph::{EvaluationGraph, ValueCellNode};
use reify_ir::{CompiledExpr, DeterminacyState, PersistentMap, Value};

/// Seeded state: `producer` is resolved (non-Undef); `consumer`'s
/// `default_expr` is a `ValueRef(producer)` — NOT an undef literal — and its
/// stored value is `Undef` even though its one static dependency is fully
/// resolved. This is precisely the causeless staleness §6.1 exists to catch:
/// no exclusion (auto, missing/Undef dep, @optimized, guard-inactive,
/// undef-literal) applies, so the checker MUST report it.
#[test]
fn seeded_stale_undef_violation_is_reported() {
    let producer_id = ValueCellId::new("SeededDemo", "producer");
    let consumer_id = ValueCellId::new("SeededDemo", "consumer");

    let mut graph = EvaluationGraph::default();

    let producer_expr = CompiledExpr::literal(Value::length(1.0), Type::length());
    graph.value_cells.insert(
        producer_id.clone(),
        ValueCellNode {
            id: producer_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: Type::length(),
            default_expr: Some(producer_expr),
            content_hash: ContentHash::of_str("seeded-producer"),
        },
    );

    let consumer_expr = CompiledExpr::value_ref(producer_id.clone(), Type::length());
    graph.value_cells.insert(
        consumer_id.clone(),
        ValueCellNode {
            id: consumer_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: Type::length(),
            default_expr: Some(consumer_expr),
            content_hash: ContentHash::of_str("seeded-consumer"),
        },
    );

    let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> = PersistentMap::new();
    values.insert(
        producer_id.clone(),
        (Value::length(1.0), DeterminacyState::Determined),
    );
    values.insert(
        consumer_id.clone(),
        (Value::Undef, DeterminacyState::Undetermined),
    );

    let mut trace_map: HashMap<NodeId, DependencyTrace> = HashMap::new();
    trace_map.insert(
        NodeId::Value(consumer_id.clone()),
        DependencyTrace {
            reads: vec![producer_id.clone()],
            realization_reads: Vec::new(),
        },
    );

    let violations =
        reify_eval::invariants::check_no_stale_undef(&graph, &values, &trace_map, &[]);

    assert!(
        !violations.is_empty(),
        "expected the checker to report the seeded stale-Undef consumer, got zero \
         violations — a checker that never fires would make the corpus sweep \
         vacuously green"
    );
    assert!(
        violations.iter().any(|v| v.cell == consumer_id),
        "expected a violation naming consumer cell {:?}, got {:?}",
        consumer_id,
        violations.iter().map(|v| &v.cell).collect::<Vec<_>>()
    );
}

// ── Step-7: Engine-path corpus test over the deliberately-undef fixtures ────

/// The four fixtures purpose-built for the undef-self-describing PRD family
/// (tasks 4321/4322/4323/4326, α/β/γ/η) — each deliberately packed with
/// non-solver Undef origins (Unbound, propagated, UserUndef, AwaitingSolve,
/// and an op cell reading an Undef input). None of these origins may be
/// reported by `check_no_stale_undef`: every one is excluded by clause 1
/// (auto), clause 2 (no `default_expr`), clause 4 (Undef/missing dep), or
/// clause 6 (undef-literal) — see `docs/prds/v0_6/eval-uniform-dependency-handling.md`
/// §6.1. `undef_cause_solve_failed.ri` is deliberately NOT in this list (it
/// needs a solver-attached engine, `MockConstraintSolver::new_infeasible`,
/// to exercise its SolveFailed classification); it's still covered by the
/// broad corpus sweep (step 9), where its lone cell is an Auto param exempt
/// via clause 1 regardless of solver wiring.
const DELIBERATELY_UNDEF_FIXTURES: &[&str] = &[
    "undef_causes_layer1",
    "undef_trace",
    "undef_boundary_representative",
    "undef_cause_op_contract",
];

/// RED until step-8: `Engine::check_no_stale_undef` does not exist yet.
#[test]
fn deliberately_undef_fixtures_report_zero_violations() {
    for name in DELIBERATELY_UNDEF_FIXTURES {
        let path = format!(
            "{}/tests/fixtures/{name}.ri",
            env!("CARGO_MANIFEST_DIR")
        );
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading fixture {name}.ri at {path}: {e}"));

        let compiled = reify_test_support::compile_source_with_stdlib(&source);
        let errors = reify_test_support::collect_errors(&compiled.diagnostics);
        assert!(
            errors.is_empty(),
            "{name}.ri should compile without errors: {errors:#?}"
        );

        let mut engine = reify_eval::Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            Some(Box::new(reify_test_support::MockGeometryKernel::new())),
        );
        engine.eval(&compiled);

        let violations = engine.check_no_stale_undef();
        assert!(
            violations.is_empty(),
            "{name}.ri: expected zero stale-Undef violations (every Undef here \
             is a deliberate, excluded origin), got {violations:?}"
        );
    }
}

// ── Step-9/10: broad debug-gate corpus sweep ─────────────────────────────────

/// Recursively collect every `.ri` file under `dir` (including subdirectories).
/// Unreadable entries/directories are silently skipped — this only ever walks
/// our own repo directories, which are expected to be readable.
fn collect_ri_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ri_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ri") {
            out.push(path);
        }
    }
}

/// The user-observable debug-gate signal (task α, PRD §6.1 row 6 + §9): every
/// `.ri` fixture under `crates/reify-eval/tests/fixtures/` and `examples/`,
/// plus the explicit #4946 R3f-bridge premise fixture
/// `tests/prd-gate/fixtures/geometry_let_selector_consumer.ri` (a geometry
/// LET consumed by a selector, consumed in turn by a plain let — the
/// let-backed shape γ will later give a first-class value cell), must
/// produce ZERO stale-Undef violations.
///
/// A file whose compile emits any Error-severity diagnostic is SKIPPED
/// (negative/malformed fixtures can't be evaluated) — every skip is PRINTED
/// (via `eprintln!`) so bounded coverage never silently reads as full
/// coverage.
#[test]
fn broad_corpus_sweep_reports_zero_violations() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixtures_dir = std::path::Path::new(manifest_dir).join("tests/fixtures");
    let examples_dir = std::path::Path::new(manifest_dir).join("../../examples");
    let selector_consumer_path = std::path::Path::new(manifest_dir)
        .join("../../tests/prd-gate/fixtures/geometry_let_selector_consumer.ri");

    let mut files = Vec::new();
    collect_ri_files(&fixtures_dir, &mut files);
    collect_ri_files(&examples_dir, &mut files);
    files.push(selector_consumer_path.clone());
    files.sort();

    let mut skipped: Vec<String> = Vec::new();
    let mut offenders: Vec<(String, Vec<reify_eval::StaleUndefViolation>)> = Vec::new();
    let mut selector_consumer_result: Option<usize> = None;

    for path in &files {
        let display = path.display().to_string();
        let source =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {display}: {e}"));

        let compiled = reify_test_support::compile_source_with_stdlib(&source);
        let errors = reify_test_support::collect_errors(&compiled.diagnostics);
        if !errors.is_empty() {
            skipped.push(display);
            continue;
        }

        let mut engine = reify_eval::Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            Some(Box::new(reify_test_support::MockGeometryKernel::new())),
        );
        engine.eval(&compiled);
        let violations = engine.check_no_stale_undef();

        if *path == selector_consumer_path {
            selector_consumer_result = Some(violations.len());
        }
        if !violations.is_empty() {
            offenders.push((display, violations));
        }
    }

    eprintln!(
        "broad_corpus_sweep: {} files evaluated, {} skipped (compile errors)",
        files.len() - skipped.len(),
        skipped.len()
    );
    for s in &skipped {
        eprintln!("  SKIP (compile error): {s}");
    }

    assert!(
        offenders.is_empty(),
        "expected zero stale-Undef violations across the corpus; offending file(s):\n{}",
        offenders
            .iter()
            .map(|(f, vs)| {
                let detail = vs
                    .iter()
                    .map(|v| format!("    {:?}: {}", v.cell, v.detail))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("  {f}:\n{detail}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    );

    assert_eq!(
        selector_consumer_result,
        Some(0),
        "geometry_let_selector_consumer.ri must be present, evaluated (not skipped due \
         to a compile error), and produce zero violations — the #4946 R3f-bridge premise"
    );
}
