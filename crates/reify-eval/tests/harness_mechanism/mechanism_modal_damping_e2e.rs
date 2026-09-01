//! Author-surface end-to-end gate for `mechanism_modal_analysis` damping
//! (task #6875, step-1).
//!
//! ## What this pins
//!
//! `mechanism_modal_analysis(body, opts)` accepts a full `ModalOptions`,
//! including a `damping : DampingDescriptor` descriptor.  Before #6875 the
//! lumped generalized-coordinate producer (`run_mechanism_modal` in
//! `crates/reify-eval/src/modal_ops.rs`) built every `Mode` with a hardcoded
//! `("damping_ratio", Value::Real(0.0))` — the caller's explicitly-declared
//! `RayleighDamping(alpha, beta)` was never read, so the declared damping
//! intent was silently dropped (an INV-SF-3 silent-failure shape).  The FEA
//! sibling `modal_analysis` has always honored it via
//! `extract_damping` → `rayleigh_damping_ratio`.
//!
//! This test drives the exact author surface an engineer touches: a `.ri`
//! source that configures `RayleighDamping(alpha: 0.0, beta: 0.0001)` and
//! reads `z_modal.modes[0].damping_ratio` back out.
//!
//! ## Physics ground truth (an identity, not a fitted number)
//!
//! Rayleigh damping gives ζ_i = (α + β·ω_i²)/(2·ω_i).  With α = 0 this
//! collapses to the exact algebraic identity
//!
//!     ζ = β·ω / 2
//!
//! so the expectation below is *recomputed from the same f64 frequency the
//! producer emitted*, never from a transcribed constant.  For this fixture
//! (0.5 kg carriage on a 20×5×0.5 mm parallelogram steel flexure,
//! k_stage = 48·E·I/L³ ≈ 6.41e4 N/m ⇒ f ≈ 57 Hz) that lands at
//! ζ ≈ 1e-4 · 2π · 57 / 2 ≈ 0.0179 — two orders of magnitude above display
//! precision, while the pre-#6875 producer printed exactly 0.
//!
//! Source: the preserved probe from the #6753 study session,
//! `probes/mechanism_damping_probe.ri`, adapted to a `structure def` so its
//! cells are addressable as `ValueCellId::new("MechanismDampingProbe", …)`
//! (the form `printer_z_compliant_mount.ri` uses).  Inlined as a `&str` rather
//! than committed as a fixture: a committed `.ri` read by a Rust test must be
//! registered in `_RUST_COUPLED_RI_FIXTURES` in `scripts/verify.sh`, a
//! verify-pipeline file that escalates this change to the full global gate.

use std::f64::consts::PI;

use reify_core::ValueCellId;
use reify_eval::compute_targets::register_compute_fns;
use reify_ir::Value;
use reify_test_support::{errors_only, make_simple_engine, parse_and_compile_with_stdlib};

/// The Rayleigh β the source below declares.  Kept as a Rust constant so the
/// expected-ζ identity is computed, not transcribed.  (α is 0, which is what
/// makes ζ = β·ω/2 exact.)
const BETA: f64 = 1e-4;

/// Inline author-surface source.  Mirrors the #6753 probe verbatim except for
/// the `structure def` wrapper.
const SOURCE: &str = r#"
structure def MechanismDampingProbe {
    let steel = Steel_AISI_1045()
    let z_carriage_mass = 0.5kg

    let z_flexure = prb_parallelogram_flexure(
        20mm, 5mm, 0.5mm, 10mm, steel, vec3(0, 0, 1), point3(0mm, 0mm, 0mm))

    let m0 = mechanism()
    let carriage = body(m0, point_mass(z_carriage_mass), z_flexure)

    // Explicitly configured RayleighDamping — mechanism_modal_analysis must
    // honor it in Mode.damping_ratio rather than hardcoding 0.0.
    let opts = ModalOptions(
        n_modes: 1,
        boundary_conditions: [],
        damping: RayleighDamping(alpha: 0.0, beta: 0.0001),
        sigma: 0.0,
        tol: 0.000000001,
        max_iters: 200,
        reference_direction: vec3(0.0, 0.0, 1.0),
        element_order: ElementOrder.P2
    )

    let z_modal = mechanism_modal_analysis(carriage, opts)
    let z_first_mode_hz = first_frequency(z_modal)
    let zeta1 : Real = z_modal.modes[0].damping_ratio
}
"#;

/// Read an `f64` out of a numeric value cell (`Real` / `Int` / dimensioned
/// `Scalar`).  Panics on a non-numeric cell so a shape regression fails
/// loudly.  Mirrors `printer_z_compliant_mount_e2e.rs::num`.
fn num(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Int(n) => *n as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a numeric cell, got {other:?}"),
    }
}

/// A `ModalOptions.damping = RayleighDamping(α = 0, β = 1e-4)` declared at the
/// author surface reaches `Mode.damping_ratio` on the `mechanism_modal_analysis`
/// path.
///
/// RED before step-2: ζ reads back as exactly `0.0` (the hardcoded literal),
/// failing both the defect pin and the identity pin.
#[test]
fn mechanism_modal_rayleigh_zeta_visible_at_author_surface() {
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    assert!(
        errors_only(&compiled).is_empty(),
        "the mechanism-damping probe source must compile with no error-severity \
         diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // (a) eval is clean — no Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval must produce no Error diagnostics, got: {errors:#?}"
    );

    let cell = |name: &str| {
        eval_result
            .values
            .get(&ValueCellId::new("MechanismDampingProbe", name))
            .unwrap_or_else(|| {
                panic!(
                    "MechanismDampingProbe.{name} not found in eval result; \
                     cells present: {:#?}\nall diagnostics: {:#?}",
                    eval_result
                        .values
                        .iter()
                        .map(|(id, _)| id)
                        .collect::<Vec<_>>(),
                    eval_result.diagnostics
                )
            })
    };

    // (b) The first mode is in the physical band.  This is NOT the damping
    // claim — it guards against a rigid-mode / zero-frequency regression that
    // would make the ζ identity below vacuously satisfiable at ζ = 0.
    // k_stage = 48·E·I/L³ ≈ 6.41e4 N/m on 0.5 kg ⇒ f = √(k/m)/(2π) ≈ 57 Hz.
    let f = num(cell("z_first_mode_hz"));
    assert!(
        f.is_finite() && f > 1.0 && f < 1000.0,
        "first mode frequency {f} Hz must be finite and in the ~57 Hz physical \
         band (1..1000 Hz) — a 0 Hz rigid mode would make the ζ assertions vacuous"
    );

    // (c) ζ is a real number at all.
    let zeta = num(cell("zeta1"));
    assert!(
        zeta.is_finite(),
        "modes[0].damping_ratio must be finite, got {zeta}"
    );

    // (d) THE DEFECT PIN.  The declared RayleighDamping must actually reach
    // Mode.damping_ratio.  The expected value is ζ ≈ 0.0179; the pre-#6875
    // producer emits exactly 0.0.  1e-3 is a floor two orders of magnitude
    // below the expectation and infinitely above the bug's 0 — it discriminates
    // "honored" from "hardcoded zero" and nothing finer.
    assert!(
        zeta > 1e-3,
        "modes[0].damping_ratio must reflect the declared \
         RayleighDamping(alpha: 0.0, beta: {BETA}) — expected ζ ≈ 0.0179, got \
         {zeta}. A ζ of exactly 0 means mechanism_modal_analysis silently \
         dropped the caller's damping descriptor (task #6875)."
    );

    // (e) THE IDENTITY PIN.  α = 0 ⇒ ζ = (0 + β·ω²)/(2·ω) = β·ω/2 exactly.
    // ω is recomputed from the SAME f64 frequency the producer wrote into
    // Mode.frequency, so both sides evaluate the identical expression on
    // identical bits; only floating-point associativity can separate them.
    // 1e-9 relative is ~7 orders above the ~1e-16 f64 round-off floor and ~9
    // orders below the 100 % error of the defect — it is an associativity
    // guard, not a tuned tolerance.
    let omega = 2.0 * PI * f;
    let expected = BETA * omega / 2.0;
    assert!(
        expected > 0.0,
        "fixture (α = 0, β = {BETA}) at f = {f} Hz must give a nonzero expected ζ"
    );
    let rel_err = (zeta - expected).abs() / expected;
    assert!(
        rel_err < 1e-9,
        "modes[0].damping_ratio {zeta} must equal the Rayleigh identity \
         β·ω/2 = {expected} (β = {BETA}, ω = 2π·{f} = {omega} rad/s) to within \
         fp associativity; relative error {rel_err:.3e} ≥ 1e-9"
    );
}
