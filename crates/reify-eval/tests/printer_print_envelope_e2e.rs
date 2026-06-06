//! End-to-end integration tests for the trajectory ρ printer print-envelope dogfood
//! (`docs/prds/v0_3/trajectory-input-shaping.md §1, §10.2, §11 Phase 6` task ρ — 3878).
//!
//! TERMINAL user-observable dogfood for the four-PRD stack:
//!   kinematics (λ) → RBD (ο) → modal (π) → trajectory (ρ).
//!
//! Drives `examples/trajectory/printer_print_envelope.ri` through the full
//! parse → `parse_and_compile_with_stdlib` → `make_simple_engine` +
//! `register_compute_fns` → `Engine::eval` pipeline and asserts:
//!
//!   1. No Error-severity diagnostics after eval.
//!   2. `peak_unshaped`, `peak_impulse`, `peak_tots` cells are finite and ≥ 0.
//!   3. `budget` cell is finite and > 0.
//!   4. `imported_count` cell is ≥ 1.
//!   5. The eval graph contains ComputeNodes with targets
//!      "trajectory::simulate" AND "trajectory::input_shape".
//!
//! ## Numeric posture
//!
//! No magic numeric threshold (e.g. `peak_shaped < peak_unshaped`) gates CI.
//! No validated achievability basis exists for ordering assertions at the e2e
//! layer (no Value-layer test verifies shaped-reduces-deviation). The posture
//! mirrors `modal_analysis_e2e.rs::e2e_printer_gantry_prints_five_modes`:
//! "no analytic tolerance; structural assertion only."
//!
//! ## Release gate
//!
//! The main eval test (`printer_print_envelope_eval_e2e`) is release-gated:
//! it drives a full modal solve (heavy FEA eigenproblem) followed by
//! simulate_trajectory (ODE integration) for three trajectory variants.
//! The registration pin and fixture sub-test run always.
//!
//! ## Fixture sub-test
//!
//! `printer_print_envelope_fixture_multi_segment` lowers the bundled
//! `examples/trajectory/test_data/printer_print_envelope.gcode` fixture through
//! `gcode_import` via `reify_stdlib::eval_builtin` under MarlinDialect and asserts
//! the result has ≥ 2 motion-profile segments (the fixture has one M-code split
//! between two motion runs).
//!
//! The fixture file ships alongside this test under
//! `examples/trajectory/test_data/printer_print_envelope.gcode`.

use reify_core::{Severity, ValueCellId};
use reify_eval::compute_targets::register_compute_fns;
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Path constants ────────────────────────────────────────────────────────────

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/trajectory/printer_print_envelope.ri"
);

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/trajectory/test_data/printer_print_envelope.gcode"
);

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Read an `f64` out of a numeric value cell (`Real` / `Int` / dimensioned `Scalar`).
/// Panics on a non-numeric cell so a shape regression fails loudly.
/// Mirrors `toolhead_motor_sizing_e2e.rs::num` and `rigid_body_dynamics_e2e.rs::num`.
fn num(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Int(n) => *n as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a numeric cell, got {other:?}"),
    }
}

/// Build a `MarlinDialect` value as the `gcode_import` eval path expects:
/// a `Value::StructureInstance` whose `type_name` is `"MarlinDialect"`.
/// The gcode_import arm dispatches on this name without a StructureRegistry.
/// Mirrors `reify_stdlib::trajectory::mod::tests::marlin_dialect_value`.
fn marlin_dialect_value() -> Value {
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "MarlinDialect".to_string(),
        version: 0,
        fields: PersistentMap::default(),
    }))
}

// ── Seam pin (always-run) ─────────────────────────────────────────────────────
//
// Coerce trajectory trampolines to ComputeFn — compile-time proof that the
// cross-crate trampoline signatures are compatible. Mirrors the pattern in
// `zv_shaped_ramp_db_reduction.rs::_seam_pin` and
// `modal_analysis_e2e.rs::_seam_pin`.

#[allow(dead_code)]
fn _seam_pin() {
    let _sim: reify_eval::ComputeFn =
        reify_eval::trajectory_ops::simulate_trajectory_trampoline;
    let _shp: reify_eval::ComputeFn = reify_eval::trajectory_ops::input_shape_trampoline;
}

// ── Registration pin (always-run) ─────────────────────────────────────────────

/// `register_compute_fns` installs both trajectory trampolines.
///
/// Mirrors `zv_shaped_ramp_db_reduction.rs::register_compute_fns_installs_trajectory_trampolines`.
/// This always-run guard catches registration regressions independent of the
/// heavy numerical acceptance test below.
#[test]
fn register_compute_fns_installs_trajectory_trampolines() {
    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);

    assert!(
        engine.compute_dispatch("trajectory::simulate").is_some(),
        "register_compute_fns must install a trampoline under 'trajectory::simulate'"
    );
    assert!(
        engine.compute_dispatch("trajectory::input_shape").is_some(),
        "register_compute_fns must install a trampoline under 'trajectory::input_shape'"
    );
}

// ── Main eval e2e test (release-gated) ───────────────────────────────────────
//
// Drives the full printer print-envelope dogfood through eval and asserts the
// peak-deviation / budget / imported_count cells and ComputeNode presence.
// Release-gated because it drives a full modal solve (heavy FEA eigenproblem)
// followed by simulate_trajectory (ODE integration) — too slow in debug mode.

/// Full eval of `printer_print_envelope.ri`: zero Error diagnostics, finite
/// peak-deviation cells (≥ 0), positive budget, gcode imported, ComputeNode presence.
///
/// Demonstrates the "print-path end-effector error envelope under input-shaped
/// and TOTS-optimal motion" workflow from PRD §10.2:
///   - peak_unshaped / peak_impulse / peak_tots are finite and ≥ 0
///     (peak_deviation_at maxes Euclidean distances → always ≥ 0)
///   - budget is finite and > 0 (0.5 mm tolerance)
///   - ComputeNode "trajectory::simulate" and "trajectory::input_shape" are present
#[cfg_attr(debug_assertions, ignore = "heavy modal + trajectory solve; release-only")]
#[test]
fn printer_print_envelope_eval_e2e() {
    let source = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "examples/trajectory/printer_print_envelope.ri should exist (authored by step-2)",
    );

    let compiled = parse_and_compile_with_stdlib(&source);

    // ── (1) Compile-clean pre-condition ──────────────────────────────────────
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "printer_print_envelope.ri should compile with no Error diagnostics; got:\n{:#?}",
        compile_errors
    );

    // ── Engine setup + eval ───────────────────────────────────────────────────
    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // ── (1) No eval-time Error diagnostics ───────────────────────────────────
    let eval_errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "expected no Error diagnostics after eval of printer_print_envelope.ri; got:\n{:#?}",
        eval_errors
    );

    // ── (2) peak_unshaped / peak_impulse / peak_tots — finite and ≥ 0 ────────
    //
    // peak_deviation_at maxes Euclidean distances → always ≥ 0.
    // Asserting specific ordering (shaped < unshaped) is deliberately avoided —
    // no validated achievability basis exists at this eval layer.
    for cell_name in &["peak_unshaped", "peak_impulse", "peak_tots"] {
        let cell = ValueCellId::new("PrinterPrintEnvelope", *cell_name);
        let val = eval_result
            .values
            .get(&cell)
            .unwrap_or_else(|| {
                panic!(
                    "PrinterPrintEnvelope.{} cell missing from eval result \
                     (all diagnostics: {:#?})",
                    cell_name, eval_result.diagnostics
                )
            });
        let n = num(val);
        assert!(
            n.is_finite(),
            "PrinterPrintEnvelope.{} must be finite, got {}",
            cell_name,
            n
        );
        assert!(
            n >= 0.0,
            "PrinterPrintEnvelope.{} must be ≥ 0 (Euclidean distance), got {}",
            cell_name,
            n
        );
    }

    // ── (3) budget — finite and > 0 ──────────────────────────────────────────
    let budget_cell = ValueCellId::new("PrinterPrintEnvelope", "budget");
    let budget_val = eval_result
        .values
        .get(&budget_cell)
        .unwrap_or_else(|| {
            panic!(
                "PrinterPrintEnvelope.budget cell missing from eval result \
                 (all diagnostics: {:#?})",
                eval_result.diagnostics
            )
        });
    let budget = num(budget_val);
    assert!(
        budget.is_finite() && budget > 0.0,
        "PrinterPrintEnvelope.budget must be finite and > 0 (tolerance is physically meaningful), \
         got {}",
        budget
    );

    // ── (4) imported_count — ≥ 1 ─────────────────────────────────────────────
    let imported_cell = ValueCellId::new("PrinterPrintEnvelope", "imported_count");
    let imported_val = eval_result
        .values
        .get(&imported_cell)
        .unwrap_or_else(|| {
            panic!(
                "PrinterPrintEnvelope.imported_count cell missing from eval result \
                 (all diagnostics: {:#?})",
                eval_result.diagnostics
            )
        });
    let imported_count = num(imported_val) as i64;
    assert!(
        imported_count >= 1,
        "PrinterPrintEnvelope.imported_count must be ≥ 1 \
         (the G1 X10 Y10 move lowers to one profile); got {}",
        imported_count
    );

    // ── (5) ComputeNode presence for trajectory trampolines ───────────────────
    //
    // Mirrors `modal_analysis_e2e.rs` ComputeNode-presence check.
    // Both "trajectory::simulate" and "trajectory::input_shape" must appear
    // in the graph because the .ri calls simulate_trajectory (×3) and
    // input_shape (×2, one ZV + one TOTS).
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();

    let has_simulate = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "trajectory::simulate");
    assert!(
        has_simulate,
        "expected a ComputeNode with target==\"trajectory::simulate\"; found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    let has_input_shape = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "trajectory::input_shape");
    assert!(
        has_input_shape,
        "expected a ComputeNode with target==\"trajectory::input_shape\"; found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );
}

// ── Fixture sub-test ─────────────────────────────────────────────────────────
//
// Lowers `examples/trajectory/test_data/printer_print_envelope.gcode` through
// gcode_import under MarlinDialect via `reify_stdlib::eval_builtin` and asserts
// ≥ 2 motion-profile segments (M-code split between two motion runs).
//
// The fixture is a small Marlin program with one non-motion M-command (e.g.
// M104 temp set) separating two groups of G1 moves, so lower_gcode produces
// ≥ 2 contiguous motion segments. This is a structural consequence of the
// M-code split, not a guessed numeric threshold.

/// Multi-segment bolt-on G-code fixture: asserts ≥ 2 motion-profile segments.
///
/// Follows `gcode_import_eval_e2e.rs`'s eval-path entry: lower the fixture
/// string through `gcode_import` via `reify_stdlib::eval_builtin` under
/// MarlinDialect and assert the resulting `Value::List` has ≥ 2 elements.
#[test]
fn printer_print_envelope_fixture_multi_segment() {
    let fixture_gcode = std::fs::read_to_string(FIXTURE_PATH).expect(
        "examples/trajectory/test_data/printer_print_envelope.gcode should exist \
         (authored by step-6)",
    );

    // Drive gcode_import through the reify_stdlib eval path directly.
    // Passes `Value::String(fixture_gcode)` + MarlinDialect via eval_builtin —
    // the same path that `gcode_import_smoke.ri` exercises at the .ri layer.
    let result = reify_stdlib::eval_builtin(
        "gcode_import",
        &[Value::String(fixture_gcode), marlin_dialect_value()],
    );

    match result {
        Value::List(segments) => {
            assert!(
                segments.len() >= 2,
                "printer_print_envelope.gcode should lower to ≥ 2 motion segments \
                 (one M-code command between two G1 motion runs), got {} segment(s)",
                segments.len()
            );
        }
        other => panic!(
            "gcode_import result should be Value::List, got {other:?}"
        ),
    }
}
