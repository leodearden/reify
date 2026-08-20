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

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;

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

// ── step-13: the full user-observable signal, end-to-end ────────────────────────
//
// Mirrors the unit-level contract pinned by
// `compute_targets::elastic_static::tests::shell_route_trampoline_populates_shell_channels`,
// but exercised through the *complete* `engine.eval` path on the fixture (parse +
// compile-with-stdlib + @optimized lowering + upstream shell-extract wiring +
// FEA trampoline). May already pass once step-10 (trampoline shell branch) and
// step-12 (upstream-node wiring) land — this step locks the exact assertions:
// shell_channels `Some(_)`, the I-2 stress alias, and the one-OOM von Mises band.

/// Extract the `SampledField.data` vec from a `Value::Field { Sampled }`,
/// panicking on any other shape (mirrors the `shell9_field_data` idiom).
fn field_data(v: &Value) -> Vec<f64> {
    match v {
        Value::Field { lambda, .. } => match lambda.as_ref() {
            Value::SampledField(sf) => sf.data.clone(),
            other => panic!("field lambda must be Value::SampledField, got {other:?}"),
        },
        other => panic!("expected Value::Field, got {other:?}"),
    }
}

/// von Mises of a row-major 3×3 stress window
/// (`[σxx,σxy,σxz, σyx,σyy,σyz, σzx,σzy,σzz]`) — rotation-invariant, so a
/// local-frame channel window gives the correct value with no global rotation.
fn vm9(w: &[f64]) -> f64 {
    let (sxx, syy, szz) = (w[0], w[4], w[8]);
    let (sxy, syz, szx) = (w[1], w[5], w[6]);
    (0.5 * ((sxx - syy).powi(2)
        + (syy - szz).powi(2)
        + (szz - sxx).powi(2)
        + 6.0 * (sxy * sxy + syz * syz + szx * szx)))
        .sqrt()
}

/// One-level field extraction from a `StructureInstance` (returns `None` for any
/// other Value shape or a missing field).
fn struct_field(val: &Value, key: &str) -> Option<Value> {
    match val {
        Value::StructureInstance(data) => data.fields.get(&key.to_string()).cloned(),
        _ => None,
    }
}

/// The shell fixture solves end-to-end through `engine.eval` and surfaces a real
/// `ShellStress` `shell_channels` with the I-2 stress alias and an in-band
/// top-channel von Mises.
#[test]
fn shell_fixture_surfaces_shell_channels_with_in_band_von_mises() {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(shell_source());
    let mut engine = shell_engine();
    let eval_result = engine.eval(&compiled);

    // No Error-severity diagnostics — a clean end-to-end solve.
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

    // The user-observable result cell.
    let result_cell = ValueCellId::new("FeaShellFlexure", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaShellFlexure.result not found in eval result"));

    // shell_channels must be a "ShellStress" StructureInstance (NOT Undef).
    let shell_channels = struct_field(result_val, "shell_channels")
        .expect("ElasticResult must carry a shell_channels field");
    let sc_data = match &shell_channels {
        Value::StructureInstance(d) => {
            assert_eq!(
                d.type_name.as_str(),
                "ShellStress",
                "shell_channels must be a ShellStress instance on the shell route"
            );
            d
        }
        other => panic!(
            "result.shell_channels must be a ShellStress StructureInstance (NOT Undef) \
             on the shell route, got {other:?}"
        ),
    };

    // top / mid / bottom present and all-finite.
    let top = field_data(
        sc_data
            .fields
            .get(&"top".to_string())
            .expect("ShellStress.top"),
    );
    let mid = field_data(
        sc_data
            .fields
            .get(&"mid".to_string())
            .expect("ShellStress.mid"),
    );
    let bottom = field_data(
        sc_data
            .fields
            .get(&"bottom".to_string())
            .expect("ShellStress.bottom"),
    );
    for (name, ch) in [("top", &top), ("mid", &mid), ("bottom", &bottom)] {
        assert!(
            !ch.is_empty() && ch.iter().all(|x| x.is_finite()),
            "{name} channel must be non-empty and all-finite"
        );
    }

    // I-2 alias: result.stress (Field) bit-equals shell_channels.mid.
    let stress =
        struct_field(result_val, "stress").expect("ElasticResult must carry a stress field");
    assert!(
        !matches!(&stress, Value::Undef),
        "stress must be a populated field on the shell route (the I-2 alias source)"
    );
    assert_eq!(
        field_data(&stress),
        mid,
        "I-2 alias: result.stress data must equal shell_channels.mid data element-wise"
    );

    // max-over-elements top-channel von Mises in the one-OOM band around
    // σ=6PL/(bh²)=3e8 Pa (the bare-MITC3 honest-accuracy contract, esc-3594-10).
    assert_eq!(
        top.len() % 9,
        0,
        "top must hold a row-major 3×3 per element (len % 9 == 0)"
    );
    let max_vm = top.chunks_exact(9).map(vm9).fold(0.0_f64, f64::max);
    assert!(
        max_vm.is_finite() && max_vm > 0.0,
        "max top von Mises must be finite and > 0, got {max_vm}"
    );
    assert!(
        (3e7..=3e9).contains(&max_vm),
        "max top von Mises {max_vm:.4e} Pa outside one-OOM band [3e7, 3e9] Pa \
         around σ=6PL/(bh²)=3e8"
    );
}
