//! End-to-end accuracy test for the P2-buckling path through `solve_buckling`
//! (task 4129, PRD §13 task δ).
//!
//! # Purpose
//!
//! Verifies that `examples/buckling_column_p2.ri` (BucklingOptions with
//! `element_order: ElementOrder.P2`) routes through the P2 kernel and achieves
//! a critical load within 5% of the analytical pin-pin Euler value — an accuracy
//! level UNREACHABLE by P1 (documented floor ~9.2%).
//!
//! # Analytical reference (pin-pin Euler column)
//!
//!   P_cr = π² · E · I / L²
//!   E = 205 GPa, I = 0.02 · 0.02³ / 12 = 1.333e-8 m⁴, L = 0.8 m
//!   P_cr ≈ 42.15 kN
//!
//! # Achievability (DD-7)
//!
//! The same P2 kernel achieves 0.06% on this exact 20×20×800 mm geometry
//! (fixed_guided_euler_column_p2_within_five_percent, nx=ny=2, nz=32).
//! Pin-pin (half-sine) is the smoothest mode. The trampoline's P2 mesh
//! (nx=ny=2, nz≈40) is slightly finer axially than the validated nz=32.
//! P1's floor (~9.2%) leaves ~4.2pp of headroom below the 5% bound.
//! A <5% result is well-founded and simultaneously proves the P2 path ran.
//!
//! # RED status (step-4)
//!
//! With element_order now parsed from the .ri file (step-2) but the trampoline
//! dispatch absent (step-5 not yet), the P2 option is silently ignored → P1
//! path (nx=8, nz=160) → ~9.2% error → fails the <5% assertion.
//!
//! GREEN after step-5 implements the P1/P2 dispatch.
//!
//! # Gate
//!
//! Release-only: the P2 buckling solve is expensive (~2s in release, impractical
//! in debug). Gated `#[cfg_attr(debug_assertions, ignore = "heavy buckling solve;
//! release-only")]` matching the P1 smoke pattern.

use std::f64::consts::PI;

use reify_core::{DimensionVector, Severity};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Load the P2 buckling column fixture.
fn buckling_p2_source() -> &'static str {
    include_str!("../../../examples/buckling_column_p2.ri")
}

/// End-to-end P2 accuracy: critical load within 5% of analytical pin-pin P_cr.
///
/// This test:
///   (a) Compiles examples/buckling_column_p2.ri (element_order: ElementOrder.P2)
///   (b) Runs the full eval pipeline via make_simple_engine + register_compute_fns
///   (c) Reads `BucklingColumnP2.crit` (Value::Scalar, dimension FORCE)
///   (d) Asserts (crit - P_cr).abs() / P_cr < 0.05
///
/// RED at step-4: element_order=P2 is ignored → P1 path → ~9.2% error → fails.
/// GREEN after step-5 dispatch: P2 path → <1% error → passes with headroom.
#[cfg_attr(debug_assertions, ignore = "heavy buckling solve; release-only")]
#[test]
fn e2e_buckling_p2_critical_load_within_five_percent() {
    let source = buckling_p2_source();
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
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from buckling_column_p2.ri, got: {:?}",
        errors
    );

    // (b) `crit` cell: Value::Scalar with dimension FORCE.
    let crit_cell = reify_core::ValueCellId::new("BucklingColumnP2", "crit");
    let crit_val = eval_result
        .values
        .get(&crit_cell)
        .unwrap_or_else(|| panic!("cell BucklingColumnP2.crit not found in eval result"));

    let (crit_si, crit_dim) = match crit_val {
        Value::Scalar { si_value, dimension } => (*si_value, *dimension),
        other => panic!(
            "expected BucklingColumnP2.crit to be Value::Scalar, got: {:?}",
            other
        ),
    };
    assert_eq!(
        crit_dim,
        DimensionVector::FORCE,
        "expected crit dimension == FORCE, got: {:?}",
        crit_dim
    );
    assert!(
        crit_si.is_finite() && crit_si > 0.0,
        "crit must be finite and positive, got: {}",
        crit_si
    );

    // (c) Analytical P_cr = π²·E·I / L²  (pin-pin, k=1)
    //   E = 205e9 Pa (Steel AISI 1045)
    //   I = lx · ly³ / 12 = 0.02 · 0.02³ / 12  (square cross-section, I_min)
    //   L = 0.8 m
    let e: f64 = 205.0e9;
    let i_min: f64 = 0.02 * 0.02_f64.powi(3) / 12.0;
    let l: f64 = 0.8;
    let p_cr = PI.powi(2) * e * i_min / (l * l);

    let rel_err = (crit_si - p_cr).abs() / p_cr;
    assert!(
        rel_err < 0.05,
        "P2 critical_load = {:.4e} N, analytical P_cr = {:.4e} N, rel_err = {:.3}% — \
         must be < 5% (DD-7: P2 achieves <1% on this geometry; P1 floors at ~9.2%)",
        crit_si,
        p_cr,
        rel_err * 100.0
    );
}
