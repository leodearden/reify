//! End-to-end integration tests for the `fn transient_response` /
//! `fn displacement_at` @optimized → ComputeNode → trampoline pipeline
//! (task ι, docs/prds/v0_3/modal-analysis.md §1 / §5.2 / §9.1).
//!
//! Steps:
//!   step-9/10  — trampoline registration + seam pin (always-run)
//!   step-17/18 — cantilever step-response decay-envelope e2e (release-gated)

use reify_core::{Severity, ValueCellId};
use reify_eval::ComputeFn;
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── step-9: RED — trampoline registration + seam pin ──────────────────────────
//
// Compile-time seam pin: coerce both
//   `reify_eval::modal_ops::solve_transient_response_trampoline`
//   `reify_eval::modal_ops::displacement_at_trampoline`
// to `ComputeFn`, pinning the cross-crate trampoline signatures. Compile success
// is the signal (no runtime assertion). Paired with a runtime check that
// `register_compute_fns` installs both trampolines under their target keys.
//
// Mirrors modal_analysis_e2e.rs:82-103 (the modal::free_vibration seam pin).
//
// RED until step-10 adds the two trampolines + their registration: the seam pin
// references symbols that do not yet exist (compile-fail RED).

#[allow(dead_code)]
fn _seam_pin() {
    let _t: ComputeFn = reify_eval::modal_ops::solve_transient_response_trampoline;
    let _d: ComputeFn = reify_eval::modal_ops::displacement_at_trampoline;
}

/// Step-9: `register_compute_fns` installs both transient trampolines.
///
/// Constructs `make_simple_engine()`, calls
/// `reify_eval::compute_targets::register_compute_fns(&mut engine)`, and asserts
/// `engine.compute_dispatch("modal::transient_response").is_some()` AND
/// `engine.compute_dispatch("modal::displacement_at").is_some()`.
///
/// Expected to fail (compile error) until step-10 creates the trampolines and
/// registers them.
#[test]
fn register_compute_fns_installs_transient_trampolines() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    assert!(
        engine
            .compute_dispatch("modal::transient_response")
            .is_some(),
        "register_compute_fns must install a trampoline under 'modal::transient_response'"
    );
    assert!(
        engine.compute_dispatch("modal::displacement_at").is_some(),
        "register_compute_fns must install a trampoline under 'modal::displacement_at'"
    );
}

// ── step-17: RED — cantilever step-response decay-envelope e2e ─────────────────
//
// Drives examples/modal/transient_step_response.ri end-to-end through the
// transient pipeline (modal_analysis → transient_response → displacement_at) and
// checks four observable signals (PRD §1 / §5.2 / §9.1):
//   (a) no Error-severity diagnostics after parse + eval
//   (b) ComputeNodes with target == "modal::transient_response" AND
//       "modal::displacement_at" are present in the graph
//   (c) the `tip` cell is a non-empty Value::List of finite Reals (the lazily
//       reconstructed Φ-projected displacement series, not Undef)
//   (d) the tip oscillation envelope decays as exp(-ζ₁·ω₁·t) within 5%, where
//       ζ₁ and ω₁ = 2π·f₁ are read from the result's OWN fundamental mode — a
//       self-referential check, so absolute frequency accuracy is irrelevant and
//       the fixture can run at the lighter ElementOrder.P1 (plan design-dec-3/4).
//
// The decay constant is measured from the sequence of *swings* between
// consecutive local extrema of the tip series: |u(eₖ) − u(eₖ₊₁)| decays as
// e^{−σ·t} INDEPENDENT of the (non-zero) step-response steady-state offset (the
// offset cancels in the difference), so no steady-state estimate is needed. A
// least-squares line through (t, ln swing) has slope −σ; the first two swings are
// dropped (initial step + fast higher-mode transient — the 2nd Z-bending mode
// decays ∝ ω² ≈ 39× faster, gone within one fundamental period) and near-floor
// swings (< 5 % of the max) filtered, leaving the clean fundamental-mode tail.
// Duhamel is exact for each mode's homogeneous decay, so σ_measured matches
// ζ₁·ω₁ to well within the PRD-ratified 5 % (residual = discrete-peak sampling +
// any multi-mode contamination).
//
// Release-gated (like the modal e2e): the modal solve assembles K + M and runs a
// generalized eigensolve — heavy in debug. The registration pin above runs
// always; this e2e gate runs release-only.
//
// RED until step-18 authors examples/modal/transient_step_response.ri: the
// include_str! below references a file that does not yet exist (compile-fail RED).

/// Read a scalar magnitude from a numeric `Value`, tolerating `Real`,
/// dimensioned `Scalar`, and `Int` spellings.
fn read_real(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        Value::Int(n) => *n as f64,
        _ => f64::NAN,
    }
}

/// Field accessor over a `StructureInstance` / `Map` value (mirrors the
/// modal_analysis_e2e.rs navigation helpers).
fn struct_field<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    match v {
        Value::StructureInstance(d) => d.fields.get(&name.to_string()),
        Value::Map(m) => m.get(&Value::String(name.to_string())),
        _ => None,
    }
}

/// A `List<Real/Scalar>` value (the `tip` series, or a history's `t_samples`)
/// read as `Vec<f64>`.
fn read_real_list(v: &Value) -> Vec<f64> {
    match v {
        Value::List(items) => items.iter().map(read_real).collect(),
        _ => Vec::new(),
    }
}

/// `(frequency_hz, damping_ratio)` of the fundamental mode (`modes[0]`) of a
/// `ModalResult` value.
fn mode0_freq_damping(result: &Value) -> (f64, f64) {
    let modes = match struct_field(result, "modes") {
        Some(Value::List(items)) => items,
        _ => return (f64::NAN, f64::NAN),
    };
    let Some(mode0) = modes.first() else {
        return (f64::NAN, f64::NAN);
    };
    let f = struct_field(mode0, "frequency").map(read_real).unwrap_or(f64::NAN);
    let z = struct_field(mode0, "damping_ratio").map(read_real).unwrap_or(f64::NAN);
    (f, z)
}

/// Measure the exponential decay constant σ (1/s) of a damped oscillation from
/// the time series `(times, u)`, plus the time span and point count of the fit.
///
/// Steady-state-offset-free: fits ln|swing| vs time over consecutive-extrema
/// swings (see the module note above). Returns `(sigma, span_seconds, n_points)`.
fn measure_decay_constant(times: &[f64], u: &[f64]) -> (f64, f64, usize) {
    // (time, value) at each interior local extremum (maximum or minimum).
    let mut extrema: Vec<(f64, f64)> = Vec::new();
    for j in 1..u.len().saturating_sub(1) {
        let is_max = u[j] > u[j - 1] && u[j] >= u[j + 1];
        let is_min = u[j] < u[j - 1] && u[j] <= u[j + 1];
        if is_max || is_min {
            extrema.push((times[j], u[j]));
        }
    }
    // Swing magnitude between consecutive extrema, timestamped at the first.
    let swings: Vec<(f64, f64)> =
        extrema.windows(2).map(|w| (w[0].0, (w[0].1 - w[1].1).abs())).collect();
    let max_swing = swings.iter().map(|&(_, d)| d).fold(0.0_f64, f64::max);
    let pts: Vec<(f64, f64)> = swings
        .iter()
        .skip(2) // drop the initial step + fast higher-mode transient
        .filter(|&&(_, d)| d > 0.05 * max_swing && d > 0.0)
        .map(|&(t, d)| (t, d.ln()))
        .collect();
    if pts.len() < 2 {
        return (f64::NAN, 0.0, pts.len());
    }
    // Least-squares slope of ln(swing) vs time.
    let n = pts.len() as f64;
    let sx: f64 = pts.iter().map(|&(t, _)| t).sum();
    let sy: f64 = pts.iter().map(|&(_, y)| y).sum();
    let sxx: f64 = pts.iter().map(|&(t, _)| t * t).sum();
    let sxy: f64 = pts.iter().map(|&(t, y)| t * y).sum();
    let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    let span = pts.last().unwrap().0 - pts.first().unwrap().0;
    (-slope, span, pts.len())
}

/// Cantilever step response: the tip displacement series decays at the modal
/// damping rate ζ₁·ω₁ (RED until step-18 authors the example fixture).
#[cfg_attr(debug_assertions, ignore = "heavy modal solve; release-only")]
#[test]
fn e2e_cantilever_step_response_decay_matches_modal_damping() {
    use std::f64::consts::PI;

    let source = include_str!("../../../examples/modal/transient_step_response.ri");
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
    let errors: Vec<_> =
        eval_result.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "expected no Error diagnostics, got: {:?}", errors);

    // (b) ComputeNodes for BOTH transient targets must be present.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let targets: Vec<&str> =
        snapshot.graph.compute_nodes.iter().map(|(_, d)| d.target.as_str()).collect();
    for want in ["modal::transient_response", "modal::displacement_at"] {
        assert!(
            targets.contains(&want),
            "expected a ComputeNode with target==\"{want}\"; found targets: {:?}",
            targets
        );
    }

    // (c) The `tip` cell is a non-empty List of finite Reals.
    let tip_cell = ValueCellId::new("CantileverStepResponse", "tip");
    let tip_val = eval_result
        .values
        .get(&tip_cell)
        .unwrap_or_else(|| panic!("cell CantileverStepResponse.tip not found in eval result"));
    let tip = read_real_list(tip_val);
    assert!(!tip.is_empty(), "tip series must be a non-empty List, got: {:?}", tip_val);
    assert!(
        tip.iter().all(|x| x.is_finite()),
        "tip series must be all-finite Reals, got: {:?}",
        tip_val
    );

    // The uniform time grid the response was sampled on (from the history echo).
    let response_cell = ValueCellId::new("CantileverStepResponse", "response");
    let response_val = eval_result
        .values
        .get(&response_cell)
        .unwrap_or_else(|| panic!("cell CantileverStepResponse.response not found in eval result"));
    let times = match struct_field(response_val, "t_samples") {
        Some(v) => read_real_list(v),
        None => Vec::new(),
    };
    assert_eq!(
        times.len(),
        tip.len(),
        "history t_samples length ({}) must match tip series length ({})",
        times.len(),
        tip.len()
    );

    // (d) Read ζ₁ and ω₁ = 2π·f₁ from the result's OWN fundamental mode and assert
    //     the tip oscillation decays as exp(-ζ₁·ω₁·t) within the 5% PRD band.
    let result_cell = ValueCellId::new("CantileverStepResponse", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell CantileverStepResponse.result not found in eval result"));
    let (f1, zeta1) = mode0_freq_damping(result_val);
    assert!(f1.is_finite() && f1 > 0.0, "f1 must be finite and positive, got: {}", f1);
    assert!(
        zeta1.is_finite() && zeta1 > 0.0 && zeta1 < 1.0,
        "ζ₁ must be a finite underdamped ratio in (0,1) — the fixture must use \
         RayleighDamping(β≠0) so a decay envelope exists; got: {}",
        zeta1
    );
    let omega1 = 2.0 * PI * f1;
    let sigma_theory = zeta1 * omega1;
    let period1 = 2.0 * PI / omega1;

    let (sigma_measured, span, npts) = measure_decay_constant(&times, &tip);
    eprintln!(
        "[modal transient] f1={:.3} Hz  ω1={:.2} rad/s  ζ1={:.5}  σ_theory={:.4}/s  \
         σ_meas={:.4}/s  span={:.4}s ({:.1} periods, {} pts)",
        f1,
        omega1,
        zeta1,
        sigma_theory,
        sigma_measured,
        span,
        span / period1,
        npts
    );

    assert!(npts >= 4, "need ≥4 clean fundamental-mode swings for a decay fit, got {}", npts);
    assert!(
        span >= 3.0 * period1,
        "decay fit must span ≥3 fundamental periods ({:.4}s), got {:.4}s",
        3.0 * period1,
        span
    );
    assert!(sigma_measured.is_finite() && sigma_measured > 0.0, "σ_measured must be finite and positive, got: {}", sigma_measured);
    let rel_err = (sigma_measured - sigma_theory).abs() / sigma_theory;
    assert!(
        rel_err < 0.05,
        "tip decay σ_measured = {:.4}/s vs σ_theory = ζ₁·ω₁ = {:.4}/s, rel_err = {:.2}% > 5%",
        sigma_measured,
        sigma_theory,
        rel_err * 100.0
    );
}
