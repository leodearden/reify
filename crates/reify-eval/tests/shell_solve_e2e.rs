//! End-to-end integration tests for the task 3594/δ shell-route bridge.
//!
//! `examples/fea_shell_flexure.ri` (a 50 mm × 10 mm × 1 mm steel flexure, bare
//! `ElasticOptions()` → `shell_force` default `Auto`, thickness/extent = 0.02 <
//! `shell_threshold` 0.2 → auto-classified SHELL) is evaluated end-to-end through
//! the `@optimized("solver::elastic_static")` lowering. On the shell route the
//! engine inserts an upstream `shell-extract::extract` ComputeNode feeding the
//! `solver::elastic_static` FEA trampoline, which routes assembly through the
//! MITC3 shell kernel and populates `result.shell_channels` (a `ShellStress`
//! StructureInstance).
//!
//! PRD: docs/prds/v0_4/shell-extract-engine-bridge.md task δ (§9 Phase 3).
//!
//! Steps:
//!   step-11 — graph-wiring assertion (both ComputeNodes + the feeding edge)
//!   step-13 — full user-observable signal (shell_channels Some, alias, von Mises band)

use reify_core::Severity;

// ── helpers ────────────────────────────────────────────────────────────────────

/// The shell-classified flexure fixture, carried into the test binary at compile
/// time via `include_str!` so the test stays in sync with the user-facing example
/// file (the "single source of truth" idiom from `solve_elastic_static_e2e.rs`).
fn shell_source() -> &'static str {
    include_str!("../../../examples/fea_shell_flexure.ri")
}

/// Build an engine with BOTH the elastic-static trampoline and the shell-extract
/// trampoline registered — the shell route's graph contract requires both.
fn shell_engine() -> reify_eval::Engine {
    let mut engine = reify_test_support::make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}

// ── step-11: RED — graph-wiring assertion ──────────────────────────────────────
//
// Three observable signals on the eval snapshot graph:
//   (a) no Error-severity diagnostics after parse + eval
//   (b) a ComputeNode with target == "solver::elastic_static" exists (the FEA
//       trampoline; @optimized lowering fired, not body-inlined)
//   (c) a ComputeNode with target == "shell-extract::extract" exists (the upstream
//       segmentation dependency inserted by the engine_eval lowering)
//   (d) the extract node's `output_value_cells` intersect the elastic node's
//       `value_inputs` — the "former feeds the latter" graph edge
//
// Expected to FAIL on (c)/(d) until step-12 teaches the @optimized lowering to
// insert the upstream shell-extract node on the Shell route.

/// The shell fixture lowers to an upstream `shell-extract::extract` ComputeNode
/// feeding the `solver::elastic_static` FEA node.
#[test]
fn shell_fixture_wires_extract_node_into_elastic_static() {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(shell_source());
    let mut engine = shell_engine();
    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics — a clean solve is required before
    //     asserting on graph structure.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // Snapshot the eval graph.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let nodes = &snapshot.graph.compute_nodes;
    let targets: Vec<&str> = nodes.iter().map(|(_, d)| d.target.as_str()).collect();

    // (b) The FEA trampoline node.
    let elastic = nodes
        .iter()
        .find(|(_, d)| d.target == "solver::elastic_static")
        .map(|(_, d)| d);
    assert!(
        elastic.is_some(),
        "expected a ComputeNode target==\"solver::elastic_static\" in the graph; \
         found targets: {:?}",
        targets
    );

    // (c) The upstream segmentation node (inserted by step-12).
    let extract = nodes
        .iter()
        .find(|(_, d)| d.target == "shell-extract::extract")
        .map(|(_, d)| d);
    assert!(
        extract.is_some(),
        "expected an upstream ComputeNode target==\"shell-extract::extract\" in the \
         graph (shell route); found targets: {:?}",
        targets
    );

    // (d) The "former feeds the latter" edge: at least one of the extract node's
    //     output value cells appears in the elastic node's value_inputs.
    let elastic = elastic.unwrap();
    let extract = extract.unwrap();
    let feeds = extract
        .output_value_cells
        .iter()
        .any(|out| elastic.value_inputs.contains(out));
    assert!(
        feeds,
        "expected shell-extract::extract output_value_cells {:?} to intersect \
         solver::elastic_static value_inputs {:?} (the upstream→downstream edge)",
        extract.output_value_cells, elastic.value_inputs
    );
}
