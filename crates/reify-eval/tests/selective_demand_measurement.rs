//! Reproducible measurement harness for the selective-demand precondition
//! (task 4532, step-12 / step-13).
//!
//! Two scenarios capture G6 "win is real" evidence:
//!
//! **Scenario A — bracket, body hidden.**
//! Observed demand = `{Value(thickness)}` only (property panel shows
//! `thickness`; no realization registered).  A scripted slider session edits
//! `thickness` through a range.  On every such edit the dirty cone of
//! `thickness` includes `R0` (the box realization), so the measurement must
//! report `would_prune.realization >= 1` — the hidden body's realization is
//! provably pruneable.
//!
//! **Scenario B — two-body module, one body visible.**
//! Observed demand = `{Realization(TwoBody#realization[0])}` (body_a visible;
//! body_b hidden).  Editing `drive` dirties BOTH realizations.  The
//! measurement must retain body_a and report body_b in `would_prune.realization`.
//! Byte-identity vs a no-observed-registration control run proves zero behavior
//! change on the multi-body path.
//!
//! The helpers `run_scenario_a()` and `run_scenario_b()` (implemented in
//! step-13) return `Vec<DemandPruneMeasurement>` for use in the doc table and
//! as the data source for the summarizer that prints distributions under
//! `--nocapture`.

use reify_core::{RealizationNodeId, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::{DemandPruneMeasurement, Engine, EvalResult};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{bracket_compiled_module, two_body_module, vcid};

// ---------------------------------------------------------------------------
// Helpers shared across tests
// ---------------------------------------------------------------------------

/// Collect an `EvalResult`'s values into a deterministically-ordered
/// `Vec<(cell-id-string, Value)>` for byte-identity comparison.
fn sorted_values(r: &EvalResult) -> Vec<(String, Value)> {
    let mut v: Vec<(String, Value)> = r
        .values
        .iter()
        .map(|(id, val)| (id.to_string(), val.clone()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Build a freshly-eval'd bracket engine.
fn bracket_engine() -> Engine {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.eval(&module);
    engine
}

/// Build a freshly-eval'd two-body engine.
fn two_body_engine() -> Engine {
    let module = two_body_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.eval(&module);
    engine
}

// ---------------------------------------------------------------------------
// Step-12 RED: helper stubs — implemented in step-13.
// ---------------------------------------------------------------------------

/// Scenario A: bracket with only `thickness` property observed (no visible
/// realization).  Returns per-edit `DemandPruneMeasurement` for the scripted
/// slider session below.
///
/// Scripted edits: thickness → 0.003, 0.004, 0.005, 0.006, 0.004 m.
/// All edits dirty the thickness cone {volume, C0, C1, C2, R0}.
fn run_scenario_a() -> Vec<DemandPruneMeasurement> {
    todo!("implement in step-13")
}

/// Scenario B: two-body module with body_a (realization[0]) registered as
/// observed demand.  Returns `(measurements_observed, measurements_control)`.
///
/// `measurements_observed` — engine with `{Realization[0]}` registered.
/// `measurements_control`  — engine with NO observed registration (control).
///
/// Scripted edits: drive → 0.011, 0.012, 0.010, 0.013 m — each dirtying
/// both realizations.
fn run_scenario_b() -> (Vec<DemandPruneMeasurement>, Vec<DemandPruneMeasurement>) {
    todo!("implement in step-13")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Scenario A: every thickness edit must show `would_prune.realization >= 1`
/// (R0 is in the dirty cone but NOT in the observed cone → it would be pruned).
#[test]
fn scenario_a_bracket_hidden_body_shows_realization_in_would_prune() {
    let measurements = run_scenario_a();
    assert!(
        !measurements.is_empty(),
        "scenario A must produce at least one measurement"
    );

    for (i, m) in measurements.iter().enumerate() {
        // Conservation law must hold on every edit.
        assert_eq!(
            m.observed_retained + m.would_prune.total(),
            m.eval_set_size,
            "edit {i}: conservation law observed_retained + Σwould_prune == eval_set_size"
        );

        // R0 is always in the dirty cone of `thickness` (it reads thickness
        // directly); since no realization is in the observed cone, every edit
        // must report at least one prunable realization.
        assert!(
            m.would_prune.realization >= 1,
            "edit {i}: R0 must appear in would_prune.realization (hidden body)"
        );
    }
}

/// Scenario A: every measurement should show a positive would_prune total —
/// the "win is real" premise from the graph structure.
#[test]
fn scenario_a_win_is_real() {
    let measurements = run_scenario_a();
    assert!(
        !measurements.is_empty(),
        "scenario A must produce at least one measurement"
    );
    let total_would_prune: usize = measurements.iter().map(|m| m.would_prune.total()).sum();
    assert!(
        total_would_prune > 0,
        "scenario A: at least one node would be pruned across the session (win is real)"
    );
}

/// Scenario B: with body_a observed, editing `drive` retains body_a's
/// realization and reports body_b's realization in `would_prune`.
#[test]
fn scenario_b_two_body_registered_body_retained_other_pruned() {
    let (measurements, _control) = run_scenario_b();
    assert!(
        !measurements.is_empty(),
        "scenario B must produce at least one measurement"
    );

    for (i, m) in measurements.iter().enumerate() {
        // Conservation law.
        assert_eq!(
            m.observed_retained + m.would_prune.total(),
            m.eval_set_size,
            "edit {i}: conservation law"
        );

        // `drive` dirties BOTH realizations — so eval_set_size must be >= 2.
        assert!(
            m.eval_set_size >= 2,
            "edit {i}: eval_set must contain at least both realizations"
        );

        // body_a is observed → it must be RETAINED (not appear in would_prune.realization).
        // body_b is NOT observed → at least one realization must be in would_prune.
        assert!(
            m.would_prune.realization >= 1,
            "edit {i}: body_b's realization must appear in would_prune"
        );

        // At least body_a is retained.
        assert!(
            m.observed_retained >= 1,
            "edit {i}: body_a (observed) must be retained"
        );
    }
}

/// Scenario B: byte-identity — observed registration must not change the
/// production eval results (values + last_eval_set) vs the control run.
#[test]
fn scenario_b_byte_identity_observed_vs_control() {
    // We need a scripted session comparing the two engines directly so we can
    // compare EvalResult, not just measurements.  This test drives both engines
    // through the same edits and asserts equivalence.
    let drive_id = vcid("TwoBody", "drive");
    let edits: Vec<Value> = vec![
        Value::length(0.011),
        Value::length(0.012),
        Value::length(0.010),
        Value::length(0.013),
    ];

    let mut engine_ctrl = two_body_engine();
    let mut engine_obs = two_body_engine();

    // Register body_a on engine_obs BEFORE the first edit.
    engine_obs.add_observed_demand(NodeId::Realization(RealizationNodeId::new("TwoBody", 0)));
    engine_obs.rebuild_observed_cone();

    for (i, val) in edits.iter().enumerate() {
        let r_ctrl = engine_ctrl
            .edit_param(drive_id.clone(), val.clone())
            .unwrap_or_else(|e| panic!("control engine edit {i} failed: {e:?}"));
        let r_obs = engine_obs
            .edit_param(drive_id.clone(), val.clone())
            .unwrap_or_else(|e| panic!("observed engine edit {i} failed: {e:?}"));

        assert_eq!(
            sorted_values(&r_ctrl),
            sorted_values(&r_obs),
            "edit {i}: EvalResult.values must be byte-identical"
        );
        assert_eq!(
            r_ctrl.resolved_params, r_obs.resolved_params,
            "edit {i}: resolved_params must match"
        );
        assert_eq!(
            r_ctrl.diagnostics.len(),
            r_obs.diagnostics.len(),
            "edit {i}: diagnostics count must match"
        );
        assert_eq!(
            engine_ctrl.last_eval_set(),
            engine_obs.last_eval_set(),
            "edit {i}: last_eval_set must be byte-identical"
        );
    }
}

/// Scenario B: the observed engine's `last_demand_prune_measurement()` must be
/// `Some` on every edit and show real pruning (body_b would be pruned).
#[test]
fn scenario_b_measurement_is_populated_and_shows_pruning() {
    let (measurements, _control) = run_scenario_b();
    assert!(
        !measurements.is_empty(),
        "scenario B must produce at least one measurement"
    );
    let any_prune: bool = measurements.iter().any(|m| m.would_prune.total() > 0);
    assert!(any_prune, "scenario B: at least one edit must show would_prune > 0");
}
