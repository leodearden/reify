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
//!   2. `peak_unshaped`, `peak_impulse`, `peak_tots` cells are finite and ≥ 0
//!      (selector-valued locations resolved via R3c/4655: faces_by_normal over a
//!      kernel-free box → FaceSelector → trampoline index-0 → finite peak;
//!      step-4's Undef-loud guard makes an upstream R2a/R2b regression surface
//!      as num(Undef) panic rather than a false-green degenerate 0.0).
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

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_eval::compute_targets::register_compute_fns;
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value, ValueMap};
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

/// Read an intermediate `EndEffectorTrack` let cell from the eval values map and
/// return the number of trajectory samples (`t_samples` list length).
///
/// **Panics loudly** if the cell is missing or not an `EndEffectorTrack`
/// `StructureInstance`, so that a regression where `simulate_trajectory` or
/// `input_shape` returns `Undef` fails the test immediately — instead of silently
/// reducing to `peak_* = 0.0` via `peak_deviation_at`'s lossy `f64::max` fold.
///
/// Returns 0 if `t_samples` is absent or its list is empty (a degenerate but non-Undef
/// track); panics if `t_samples` is present but not a `Value::List` (structural corruption).
/// `assert!(nonempty_track(...) >= 2, ...)` catches both empty-track and corrupt-track
/// regressions.
/// A valid 4-waypoint cubic spline yields ≥ 2 samples from `simulate_trajectory_core`.
///
/// Follows the same `StructureInstance` field-walk pattern as `vibration_at_loc` in
/// `zv_shaped_ramp_db_reduction.rs` (lines 211-221), but reads `t_samples` and panics
/// (rather than returning empty) on a structural mismatch so regressions fail loudly.
fn nonempty_track(values: &ValueMap, member: &str) -> usize {
    let cell = ValueCellId::new("PrinterPrintEnvelope", member);
    let val = values.get(&cell).unwrap_or_else(|| {
        panic!(
            "PrinterPrintEnvelope.{member} cell missing from eval result \
             — simulate_trajectory or input_shape likely returned Undef (regression?)"
        )
    });
    let Value::StructureInstance(data) = val else {
        panic!(
            "PrinterPrintEnvelope.{member} must be an EndEffectorTrack StructureInstance; \
             got {val:?} — simulate_trajectory or input_shape returned Undef or a non-track \
             value (regression?)"
        )
    };
    assert_eq!(
        data.type_name, "EndEffectorTrack",
        "PrinterPrintEnvelope.{member} has wrong type_name: expected \"EndEffectorTrack\", \
         got \"{}\"",
        data.type_name
    );
    match data.fields.get(&"t_samples".to_string()) {
        Some(Value::List(samples)) => samples.len(),
        None => 0,
        Some(other) => panic!(
            "PrinterPrintEnvelope.{member}: t_samples field has unexpected type {other:?}; \
             expected Value::List — structural corruption in the EndEffectorTrack (regression?)"
        ),
    }
}

// ── Seam pin (always-run) ─────────────────────────────────────────────────────
//
// Coerce trajectory trampolines to ComputeFn — compile-time proof that the
// cross-crate trampoline signatures are compatible. Mirrors the pattern in
// `zv_shaped_ramp_db_reduction.rs::_seam_pin` and
// `modal_analysis_e2e.rs::_seam_pin`.

#[allow(dead_code)]
fn _seam_pin() {
    let _sim: reify_eval::ComputeFn = reify_eval::trajectory_ops::simulate_trajectory_trampoline;
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
#[cfg_attr(
    debug_assertions,
    ignore = "heavy modal + trajectory solve; release-only"
)]
#[test]
fn printer_print_envelope_eval_e2e() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/trajectory/printer_print_envelope.ri should exist (authored by step-2)");

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
        let val = eval_result.values.get(&cell).unwrap_or_else(|| {
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

    // ── (2b) track_unshaped — non-empty EndEffectorTrack (≥ 2 t_samples) ────────
    //
    // peak_deviation_at folds with f64::max starting at 0.0 (trampoline.rs:1084-1088):
    // it collapses BOTH "broken track (Undef/empty)" AND "live track with zero modal
    // deviation" to Real(0.0). The peak_* >= 0 gate above therefore cannot distinguish
    // a genuine regression from a healthy-but-zero-deviation run. The discriminating
    // signal exists only at the TRACK level, before peak_deviation_at's reduction.
    //
    // A valid 4-waypoint cubic spline yields >= 2 t_samples from simulate_trajectory_core
    // (simulate.rs:470-487). Undef returns 0 samples because Undef is not an
    // EndEffectorTrack — the helper panics with a clear message on type mismatch.
    //
    // This assertion is a regression guard: GREEN on current main (the subsystem
    // returns a well-formed non-empty track), RED only when simulate_trajectory
    // returns Undef (Undef-track regression) or t_samples collapses to empty.
    assert!(
        nonempty_track(&eval_result.values, "track_unshaped") >= 2,
        "PrinterPrintEnvelope.track_unshaped must have >= 2 t_samples: \
         a valid 4-waypoint cubic spline always produces >= 2 trajectory samples \
         (simulate_trajectory_core), while Undef/degenerate has 0 — \
         this catches the documented \"simulate_trajectory returns Undef -> \
         peak_unshaped silently = 0.0\" regression"
    );

    // ── (2c) track_impulse / track_tots — non-empty EndEffectorTrack (≥ 2 t_samples) ─
    //
    // Covers the `input_shape` regression surface for both shaped variants:
    //
    //   track_impulse: ZV input_shape arm. build_train_for_shaper resolves ZVShaper
    //   deterministically from the example's ZVShaper struct; the impulse arm re-fits
    //   a PiecewisePolynomialProfile from a valid input, so simulate always yields a
    //   non-empty track. Catches: ZV input_shape -> Undef -> simulate(Undef) -> Undef
    //   -> peak_impulse silently = 0.0.
    //
    //   track_tots: TOTS input_shape arm. input_shape returns Undef only on
    //   ConstraintInfeasible; the example's kinematic limits (velocity_limit 300,
    //   acceleration_limit 5000 over a 0->200 mm / 3 s move) are generously feasible,
    //   and a NonConvergence outcome still returns the best-feasible re-timed profile.
    //   Catches: TOTS ConstraintInfeasible -> Undef -> peak_tots silently = 0.0.
    assert!(
        nonempty_track(&eval_result.values, "track_impulse") >= 2,
        "PrinterPrintEnvelope.track_impulse must have >= 2 t_samples: \
         the ZV input_shape arm re-fits a valid PiecewisePolynomialProfile and \
         simulate always produces a non-empty track — \
         0 samples means input_shape returned Undef (ZV regression?)"
    );
    assert!(
        nonempty_track(&eval_result.values, "track_tots") >= 2,
        "PrinterPrintEnvelope.track_tots must have >= 2 t_samples: \
         the TOTS kinematic limits (v=300, a=5000, 0->200 mm / 3 s) are feasible \
         and NonConvergence still returns a best-feasible profile — \
         0 samples means input_shape returned Undef (ConstraintInfeasible? regression?)"
    );

    // ── (3) budget — finite and > 0 ──────────────────────────────────────────
    let budget_cell = ValueCellId::new("PrinterPrintEnvelope", "budget");
    let budget_val = eval_result.values.get(&budget_cell).unwrap_or_else(|| {
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
    let imported_val = eval_result.values.get(&imported_cell).unwrap_or_else(|| {
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

    // ── (6) tots_shaper ctor args — BOTH halves of the 5758 split state ───────
    //
    // Task 5758 (PRD docs/prds/v0_6/dimensioned-construction-strictness.md §11 β)
    // migrates this file's bare ctor args at dimensioned param slots to unit
    // literals — but only ONE of the three, by Leo's esc-5758-4 option-D1
    // amendment. This section pins both halves so the split cannot drift in
    // either direction.
    //
    // NOTE: deliberately NOT routed through `num()` (line 70). That helper folds
    // `Value::Real` and `Value::Scalar` into one f64, which would make (a) pass
    // while the arg is still bare AND make (b) pass after a stray migration —
    // blind to exactly what this section exists to catch.
    let tots_shaper_cell = ValueCellId::new("PrinterPrintEnvelope", "tots_shaper");
    let tots_shaper_val = eval_result.values.get(&tots_shaper_cell).unwrap_or_else(|| {
        panic!(
            "PrinterPrintEnvelope.tots_shaper cell missing from eval result \
             (all diagnostics: {:#?})",
            eval_result.diagnostics
        )
    });
    let Value::StructureInstance(tots_shaper) = tots_shaper_val else {
        panic!(
            "PrinterPrintEnvelope.tots_shaper must be a TOTSShaper StructureInstance; \
             got {tots_shaper_val:?}"
        )
    };

    // (a) MIGRATED — actuator_limits[0].max_force is a dimensioned Scalar<Force>.
    //
    // `1000N` is 1000 N in SI, exactly what the previous bare `1000.0` was
    // already being read as, so this migration is magnitude-preserving: all four
    // of budget / peak_impulse / peak_tots / peak_unshaped are unchanged by it.
    let Some(Value::List(actuator_limits)) = tots_shaper.fields.get(&"actuator_limits".to_string())
    else {
        panic!(
            "PrinterPrintEnvelope.tots_shaper.actuator_limits must be a Value::List; got {:?}",
            tots_shaper.fields.get(&"actuator_limits".to_string())
        )
    };
    assert_eq!(
        actuator_limits.len(),
        1,
        "PrinterPrintEnvelope.tots_shaper.actuator_limits should hold exactly one \
         JointLimit, got {}",
        actuator_limits.len()
    );
    let Value::StructureInstance(joint_limit) = &actuator_limits[0] else {
        panic!(
            "PrinterPrintEnvelope.tots_shaper.actuator_limits[0] must be a JointLimit \
             StructureInstance; got {:?}",
            actuator_limits[0]
        )
    };
    match joint_limit.fields.get(&"max_force".to_string()) {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension,
                DimensionVector::FORCE,
                "tots_shaper.actuator_limits[0].max_force has the wrong dimension: \
                 expected FORCE (kg·m·s⁻²), got {dimension:?}"
            );
            assert_eq!(
                *si_value, 1000.0,
                "tots_shaper.actuator_limits[0].max_force must be 1000 N in SI \
                 (the `1000N` literal is magnitude-preserving vs the old bare `1000.0`); \
                 got {si_value}"
            );
        }
        other => panic!(
            "tots_shaper.actuator_limits[0].max_force must be a dimensioned \
             Value::Scalar {{ si_value: 1000.0, dimension: FORCE }}, got {other:?}. \
             If this is a bare Value::Real(1000.0), printer_print_envelope.ri:153 has not \
             been migrated to the `1000N` unit literal (task 5758 / PRD §11 β)."
        ),
    }

    // (b) DEFERRED — velocity_limit / acceleration_limit are STILL bare Reals.
    //
    // This is a deliberate tripwire, not a bug. Both assertions are GREEN today
    // and must STAY green through task 5758.
    for (field_name, expected) in [("velocity_limit", 300.0_f64), ("acceleration_limit", 5000.0)] {
        match tots_shaper.fields.get(&field_name.to_string()) {
            Some(Value::Real(r)) => assert_eq!(
                *r, expected,
                "PrinterPrintEnvelope.tots_shaper.{field_name} must still be the bare \
                 Value::Real({expected}); got Value::Real({r})"
            ),
            other => panic!(
                "PrinterPrintEnvelope.tots_shaper.{field_name} must STILL be a bare \
                 Value::Real({expected}), got {other:?}.\n\
                 \n\
                 These two ctor args (printer_print_envelope.ri:154/:155) are OUT OF SCOPE \
                 for task 5758, per Leo's esc-5758-4 option-D1 amendment. Dimensioning them \
                 to `300mm/s` / `5000mm/s^2` makes the TOTS solve ConstraintInfeasible: \
                 input_shape returns Undef, track_tots is empty, and peak_tots becomes \
                 exactly 0 — with exit 0 and ZERO diagnostics, which is how this defect \
                 class hides.\n\
                 \n\
                 The cause is that this file's `Waypoint.values` are dimensionless \
                 (`JointValue = Real`, stdlib/trajectory.ri:77, kinematic.ri:306) and demand \
                 ~200 units/s across printer_print_envelope.ri:115-118. The limits and the \
                 waypoints must be corrected TOGETHER.\n\
                 \n\
                 Task #5847 (dependency-gated on #5412) owns that migration and must update \
                 this assertion in the same change. Do not simply delete it."
            ),
        }
    }
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
        other => panic!("gcode_import result should be Value::List, got {other:?}"),
    }
}
