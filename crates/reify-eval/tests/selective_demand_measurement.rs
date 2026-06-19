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

use reify_core::RealizationNodeId;
use reify_eval::DemandPruneMeasurement;
use reify_eval::cache::NodeId;
use reify_ir::Value;
// `sorted_values` / `bracket_engine` / `two_body_engine` are shared from
// reify-test-support (a single definition shared with
// `observed_demand_measurement.rs`) so the byte-identity comparison logic
// cannot drift between the two test files.
use reify_test_support::{bracket_engine, sorted_values, two_body_engine, vcid};

// ---------------------------------------------------------------------------
// Step-13 GREEN: scenario helpers + summarizer.
// ---------------------------------------------------------------------------

/// Scenario A: bracket with only `thickness` property observed (no visible
/// realization).  Returns per-edit `DemandPruneMeasurement` for the scripted
/// slider session below.
///
/// Scripted edits: thickness → 0.003, 0.004, 0.005, 0.006, 0.004 m.
/// All edits dirty the thickness cone {volume, C0, C1, C2, R0}.
fn run_scenario_a() -> Vec<DemandPruneMeasurement> {
    let thickness_id = vcid("Bracket", "thickness");

    // Observed demand: only the `thickness` value cell is registered.
    // No realization is in the observed cone, so R0 will always appear in
    // would_prune when the dirty cone touches thickness.
    let mut engine = bracket_engine();
    engine.add_observed_demand(NodeId::Value(thickness_id.clone()));
    engine.rebuild_observed_cone();

    let edits: &[f64] = &[0.003, 0.004, 0.005, 0.006, 0.004];
    let mut measurements = Vec::with_capacity(edits.len());

    for &metres in edits {
        engine
            .edit_param(thickness_id.clone(), Value::length(metres))
            .unwrap_or_else(|e| panic!("scenario A: edit_param({metres}) failed: {e:?}"));
        let m = engine
            .last_demand_prune_measurement()
            .expect("measurement must be Some after edit_param")
            .clone();
        measurements.push(m);
    }

    measurements
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
    let drive_id = vcid("TwoBody", "drive");
    let edits: &[f64] = &[0.011, 0.012, 0.010, 0.013];

    // Observed engine: body_a (realization[0]) is visible.
    let mut engine_obs = two_body_engine();
    engine_obs.add_observed_demand(NodeId::Realization(RealizationNodeId::new("TwoBody", 0)));
    engine_obs.rebuild_observed_cone();

    // Control engine: no observed registration.
    let mut engine_ctrl = two_body_engine();

    let mut measurements_obs = Vec::with_capacity(edits.len());
    let mut measurements_ctrl = Vec::with_capacity(edits.len());

    for &metres in edits {
        let val = Value::length(metres);

        engine_obs
            .edit_param(drive_id.clone(), val.clone())
            .unwrap_or_else(|e| panic!("scenario B obs: edit_param({metres}) failed: {e:?}"));
        let m_obs = engine_obs
            .last_demand_prune_measurement()
            .expect("obs measurement must be Some after edit_param")
            .clone();
        measurements_obs.push(m_obs);

        engine_ctrl
            .edit_param(drive_id.clone(), val.clone())
            .unwrap_or_else(|e| panic!("scenario B ctrl: edit_param({metres}) failed: {e:?}"));
        let m_ctrl = engine_ctrl
            .last_demand_prune_measurement()
            .expect("ctrl measurement must be Some after edit_param")
            .clone();
        measurements_ctrl.push(m_ctrl);
    }

    (measurements_obs, measurements_ctrl)
}

// ---------------------------------------------------------------------------
// Summarizer (step-13): aggregate per-edit measurements → distribution table.
// ---------------------------------------------------------------------------

/// Aggregated distribution statistics for a set of measurements.
struct Distribution {
    min: usize,
    /// True median: the middle value for an odd-length run, the **mean of the
    /// two central values** for an even-length run. Held as `f64` so an
    /// even-length split (e.g. 4 edits) is not silently biased toward the
    /// upper-of-two-middle element, which the old `vals[len/2]` form returned.
    median: f64,
    max: usize,
}

impl Distribution {
    fn of<F: Fn(&DemandPruneMeasurement) -> usize>(ms: &[DemandPruneMeasurement], f: F) -> Self {
        let mut vals: Vec<usize> = ms.iter().map(&f).collect();
        vals.sort_unstable();
        let n = vals.len();
        let min = vals.first().copied().unwrap_or(0);
        let max = vals.last().copied().unwrap_or(0);
        // True median. Empty input -> 0.0, matching the `.first()`/`.last()`
        // siblings (a bare `vals[n/2]` would panic on an empty slice).
        // Currently every caller passes a non-empty edit list, but the guard
        // keeps the summarizer total.
        let median = if n == 0 {
            0.0
        } else if n % 2 == 1 {
            vals[n / 2] as f64
        } else {
            (vals[n / 2 - 1] + vals[n / 2]) as f64 / 2.0
        };
        Distribution { min, median, max }
    }

    fn fmt(&self) -> String {
        // Render an integral median without a trailing ".0" so a constant
        // distribution stays integer-clean in the documented table, while a
        // genuine even-length split still shows its ".5".
        let med = if self.median.fract() == 0.0 {
            format!("{}", self.median as i64)
        } else {
            format!("{}", self.median)
        };
        format!("{}/{}/{}", self.min, med, self.max)
    }
}

/// Print the distribution table to stdout so it is visible under
/// `cargo test ... -- --nocapture`.
///
/// Columns: scenario | eval_set_size | observed_retained | would_prune.value
///          | would_prune.constraint | would_prune.realization | total_would_prune
/// Rows show min/median/max over the scripted edit session.
fn print_distribution_table(scenario: &str, measurements: &[DemandPruneMeasurement]) {
    let eval_sz = Distribution::of(measurements, |m| m.eval_set_size);
    let retained = Distribution::of(measurements, |m| m.observed_retained);
    let wp_val = Distribution::of(measurements, |m| m.would_prune.value);
    let wp_con = Distribution::of(measurements, |m| m.would_prune.constraint);
    let wp_real = Distribution::of(measurements, |m| m.would_prune.realization);
    let wp_total = Distribution::of(measurements, |m| m.would_prune.total());

    println!();
    println!(
        "=== {} (min/median/max over {} edits) ===",
        scenario,
        measurements.len()
    );
    println!("{:<26} {}", "eval_set_size:", eval_sz.fmt());
    println!("{:<26} {}", "observed_retained:", retained.fmt());
    println!("{:<26} {}", "would_prune.value:", wp_val.fmt());
    println!("{:<26} {}", "would_prune.constraint:", wp_con.fmt());
    println!("{:<26} {}", "would_prune.realization:", wp_real.fmt());
    println!("{:<26} {}", "would_prune.total:", wp_total.fmt());
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
    assert!(
        any_prune,
        "scenario B: at least one edit must show would_prune > 0"
    );
}

/// Emit the full distribution table for both scenarios.
///
/// Visible under `cargo test -p reify-eval --test selective_demand_measurement
///     emit_distribution_table -- --nocapture`.
///
/// The numbers from this output are tabulated in
/// `docs/design/selective-demand-measurement.md`.
#[test]
fn emit_distribution_table() {
    let a_measurements = run_scenario_a();
    let (b_measurements_obs, b_measurements_ctrl) = run_scenario_b();

    print_distribution_table(
        "Scenario A: bracket, body hidden (thickness observed only)",
        &a_measurements,
    );
    print_distribution_table(
        "Scenario B observed: two-body, body_a visible",
        &b_measurements_obs,
    );
    print_distribution_table(
        "Scenario B control:  two-body, no observed registration",
        &b_measurements_ctrl,
    );

    println!();
    println!("G6 finding:");
    let a_prune_total: usize = a_measurements.iter().map(|m| m.would_prune.total()).sum();
    let a_eval_total: usize = a_measurements.iter().map(|m| m.eval_set_size).sum();
    let b_prune_total: usize = b_measurements_obs
        .iter()
        .map(|m| m.would_prune.total())
        .sum();
    let b_eval_total: usize = b_measurements_obs.iter().map(|m| m.eval_set_size).sum();
    println!(
        "  Scenario A: {}/{} nodes would be pruned across session ({:.0}%)",
        a_prune_total,
        a_eval_total,
        100.0 * a_prune_total as f64 / a_eval_total.max(1) as f64
    );
    println!(
        "  Scenario B: {}/{} nodes would be pruned across session ({:.0}%)",
        b_prune_total,
        b_eval_total,
        100.0 * b_prune_total as f64 / b_eval_total.max(1) as f64
    );
    println!();
    println!("Coarse-per-realization vs fine-per-cell:");
    let b_real_prune: usize = b_measurements_obs
        .iter()
        .map(|m| m.would_prune.realization)
        .sum();
    let b_val_prune: usize = b_measurements_obs.iter().map(|m| m.would_prune.value).sum();
    println!(
        "  realization nodes pruned: {}  (coarse grain — one per body)",
        b_real_prune
    );
    println!(
        "  value nodes pruned:       {}  (fine grain — one per property cell)",
        b_val_prune
    );

    // Lock the "win is real" premise this table documents — without these the
    // test only prints and can never fail (no regression protection).
    assert!(
        a_prune_total > 0,
        "Scenario A: the observed cone must prune at least one node across the \
         session (G6 'win is real')"
    );
    assert!(
        b_prune_total > 0,
        "Scenario B: the observed cone must prune at least one node across the \
         session (G6 'win is real')"
    );
    assert!(
        b_real_prune > 0,
        "Scenario B: body_b's realization must appear in would_prune \
         (coarse per-realization grain)"
    );
}
