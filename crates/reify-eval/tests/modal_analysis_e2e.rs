//! End-to-end integration tests for the `fn modal_analysis` @optimized →
//! ComputeNode → trampoline pipeline (task ζ, docs/prds/v0_3/modal-analysis.md
//! §10).
//!
//! Steps:
//!   step-13/14 — trampoline registration + seam pin (always-run)
//!   step-15/16 — cantilever first-mode-frequency e2e (release-gated)
//!   step-17/18 — simply-supported first-mode + higher-mode band (release-gated)

use reify_core::{Severity, ValueCellId};
use reify_eval::ComputeFn;
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Load and compile the cantilever modal smoke fixture.
fn cantilever_source() -> &'static str {
    include_str!("../../../examples/modal/cantilever_beam_modes.ri")
}

/// Load and compile the simply-supported modal smoke fixture.
// Consumed by the step-17 simply-supported e2e test (added next step).
#[allow(dead_code)]
fn simply_supported_source() -> &'static str {
    include_str!("../../../examples/modal/simply_supported_beam_modes.ri")
}

/// Read a frequency cell (Hz) as `f64`, tolerating the `Real` placeholder
/// (`Mode.frequency : Real`, modal_analysis.ri) or a dimensioned `Scalar`.
fn read_frequency(val: &Value) -> f64 {
    match val {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a frequency Real/Scalar, got: {:?}", other),
    }
}

/// Analytic Euler–Bernoulli fundamental frequency (Hz) for the shared
/// 200×10×2 mm AISI 1045 steel beam, given the dimensionless eigen-coefficient
/// `beta_l` (β·L): cantilever uses 1.875, simply-supported mode `n` uses `n·π`.
///
///   fₙ = (βL)² / (2π) · √( E·I / (ρ·A·L⁴) )
///
/// E = 205 GPa, ρ = 7850 kg/m³, L = 0.2 m, b = 0.01 m, h = 0.002 m,
/// I = b·h³/12 (bending about Y, deflection in Z), A = b·h.
fn analytic_beam_frequency(beta_l: f64) -> f64 {
    use std::f64::consts::PI;
    let e: f64 = 205.0e9;
    let rho: f64 = 7850.0;
    let l: f64 = 0.2;
    let b: f64 = 0.01;
    let h: f64 = 0.002;
    let i: f64 = b * h.powi(3) / 12.0;
    let a: f64 = b * h;
    beta_l.powi(2) / (2.0 * PI) * (e * i / (rho * a * l.powi(4))).sqrt()
}

// ── step-13: RED — trampoline registration + seam pin ────────────────────────
//
// Compile-time seam pin: coerce
//   `reify_eval::modal_ops::solve_modal_analysis_trampoline`
// to `ComputeFn`, pinning the cross-crate trampoline signature. Compile success
// is the signal (no runtime assertion). Paired with a runtime check that
// `register_compute_fns` installs the trampoline under "modal::free_vibration".
//
// RED until step-14 adds `solve_modal_analysis_trampoline` + its registration:
// the seam pin references a symbol that does not yet exist (compile-fail RED),
// mirroring buckling_smoke.rs's step-1 seam pin.

#[allow(dead_code)]
fn _seam_pin() {
    let _f: ComputeFn = reify_eval::modal_ops::solve_modal_analysis_trampoline;
}

/// Step-13: `register_compute_fns` installs the modal trampoline under the key.
///
/// Constructs `make_simple_engine()`, calls
/// `reify_eval::compute_targets::register_compute_fns(&mut engine)`, and asserts
/// `engine.compute_dispatch("modal::free_vibration").is_some()`.
///
/// Expected to fail (compile error) until step-14 creates the trampoline and
/// registers it.
#[test]
fn register_compute_fns_installs_modal_free_vibration() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    assert!(
        engine.compute_dispatch("modal::free_vibration").is_some(),
        "register_compute_fns must install a trampoline under 'modal::free_vibration'"
    );
}

// ── step-15: RED — cantilever first-mode-frequency e2e ───────────────────────
//
// Four observable signals on the cantilever fixture (examples/modal/
// cantilever_beam_modes.ri):
//   (a) no Error-severity diagnostics after parse + eval
//   (b) a ComputeNode with target == "modal::free_vibration" in the graph
//   (c) the `result` cell is a non-Undef StructureInstance/Map
//   (d) the first-mode frequency `f1` is within 10% of the analytic
//       Euler–Bernoulli cantilever fundamental f₁ = (1.875²/2π)·√(EI/ρAL⁴)
//       ≈ 41.3 Hz — the committed PRD §1/§9.1 bound (NOT the aspirational 2%,
//       moved to P2-tet follow-up 4066).
//
// Gated like buckling: the modal solve assembles K + M on the slender-beam mesh
// (~25k DOFs) and runs a generalized eigensolve — heavy in debug. The
// registration pin above runs always; this e2e gate runs release-only.
//
// ── step-16: MEASURED P1 first-mode floor (the pinned tolerance) ─────────────
//
// BC realization (build_dirichlet_bcs): the single FixedSupport(target:"x_min")
// clamps ALL THREE translational DOFs on every root-face (x ≈ 0) node — the
// cantilever clamped-free configuration.
//
// Mesh (build_beam_mesh, mirroring solve_cantilever_fea): nz = 6 through the
// 2 mm height, nx = round(L/h·nz) = 600 near-cubic bending-plane (XZ) elements,
// ny = 1. This is the CI-practical density: the shear-locking-aware near-cubic
// aspect ratio is the anti-locking lever, not raw nz.
//
// MEASURED on this mesh: f1 = 44.715 Hz vs analytic 41.271 Hz → +8.34% error
// (P1 constant-strain tets lock in bending → overestimate K → bias frequency
// high, since f ∝ √K). 8.34% clears the committed 10% PRD §1/§9.1 gate with
// ~1.66% headroom — no mesh refinement was required. Consistent with the
// buckling Euler-column's validated ~9.2% P1 eigenvalue floor on a comparable
// column (f ∝ √λ halves the eigenvalue error) and the 2026-05-29 achievability
// survey (2% deferred to P2-tet follow-up 4066). The assertion stays at the
// committed 10% bound (mirroring buckling_smoke.rs, which pins 10% while
// documenting its 9.2% measured error): the ~1.66% headroom absorbs minor
// cross-platform / faer-version numerical drift without false-failing CI.

/// Cantilever: first-mode frequency within 10% of the analytic value
/// (MEASURED P1 floor +8.34%; see the step-16 note above).
#[cfg_attr(debug_assertions, ignore = "heavy modal solve; release-only")]
#[test]
fn e2e_cantilever_first_mode_within_ten_percent() {
    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no Error diagnostics, got: {:?}", errors);

    // (b) A ComputeNode with target == "modal::free_vibration" must be present.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "modal::free_vibration");
    assert!(
        has_compute_node,
        "expected a ComputeNode with target==\"modal::free_vibration\"; found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // (c) The `result` cell must hold a non-Undef StructureInstance/Map.
    let result_cell = ValueCellId::new("CantileverBeamModes", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell CantileverBeamModes.result not found in eval result"));
    assert!(
        matches!(result_val, Value::StructureInstance(_) | Value::Map(_)),
        "expected result to be StructureInstance or Map (NOT Undef), got: {:?}",
        result_val
    );

    // (d) `f1` within 10% of the analytic cantilever fundamental (βL = 1.875).
    let f1_cell = ValueCellId::new("CantileverBeamModes", "f1");
    let f1 = read_frequency(
        eval_result
            .values
            .get(&f1_cell)
            .unwrap_or_else(|| panic!("cell CantileverBeamModes.f1 not found in eval result")),
    );
    assert!(f1.is_finite() && f1 > 0.0, "f1 must be finite and positive, got: {}", f1);

    let f1_analytic = analytic_beam_frequency(1.875);
    let rel_err = (f1 - f1_analytic).abs() / f1_analytic;
    assert!(
        rel_err < 0.10,
        "cantilever f1 = {:.3} Hz, analytic = {:.3} Hz, rel_err = {:.2}% > 10%",
        f1,
        f1_analytic,
        rel_err * 100.0
    );
}
