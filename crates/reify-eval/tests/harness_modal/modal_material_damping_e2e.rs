//! Author-surface end-to-end gate for `MaterialDamping` on the FEA
//! `modal_analysis` path (task #6878, PRD leaf β of
//! `docs/prds/v0_6/damped-modal-bonded-heterogeneous.md`).
//!
//! ## What this pins
//!
//! `ModalOptions.damping` accepts a `DampingDescriptor`. Before #6878 the FEA
//! producer read it through `extract_damping`, whose deliberate `_ => (0.0, 0.0)`
//! catch-all flattens ANY descriptor that is not `RayleighDamping` to the
//! undamped pair. So a `MaterialDamping` descriptor — the author's explicit
//! request for modal-strain-energy damping — evaluated to `damping_ratio = 0`
//! with **exit 0 and zero diagnostics**: the INV-SF-3 silent-failure shape.
//! (The mechanism sibling already degrades loudly via
//! `W_MechanismModalUnsupportedDamping`; the FEA path did not.) Measured on this
//! branch immediately before step-7, on the exact fixture below:
//!
//! ```text
//! MaterialDampingProbe.z_mat_1  = 0        MaterialDampingProbe.z_mat_2  = 0
//! MaterialDampingProbe.z_both_1 = 0        MaterialDampingProbe.z_both_2 = 0
//! EXIT=0, no diagnostics
//! ```
//!
//! ## Physics ground truth (an identity, not a fitted number)
//!
//! `MaterialDamping` selects ζ_i = ½·(Σ_e η_e·SE_e)/(Σ_e SE_e) + ζ_extra(ω_i).
//! With a SINGLE material every η_e is the same η, so the energy ratio collapses
//! to 1 **for any mode shape whatsoever** and
//!
//! ζ_i = η/2  exactly, for every mode, independent of φ_i.
//!
//! That is an algebraic identity (PRD §C5's "degenerate identity"), not a
//! converged number: it needs no eigensolve accuracy at all. `run_modal_analysis`
//! reads exactly ONE material, so this fixture is that degenerate case by
//! construction. η is read from the MODEL (`steel.loss_factor`, its own value
//! cell) and never transcribed into an assertion.
//!
//! `ζ_extra` is the EXISTING Rayleigh closed form applied to the `extra`
//! descriptor. With α = 0 it collapses to the exact identity ζ_extra = β·ω/2.
//!
//! ## Why 1e-9 relative is an associativity guard, not a tuned tolerance
//!
//! Both sides of every comparison below evaluate the SAME expression on the SAME
//! f64 bits: ω is recomputed from the very `Mode.frequency` the producer emitted,
//! and η from the very material cell the solver read. Only floating-point
//! associativity can separate them — a ~1e-16 floor, seven orders below the
//! asserted band and ~nine orders below the 100 % error of the silent-zero
//! defect. The landed `mechanism_modal_damping_e2e.rs` uses exactly this guard
//! on the β·ω/2 half and is green.
//!
//! ## Fixture and source-inlining
//!
//! One shared geometry across all four arms (a 100 × 20 × 5 mm `Steel_AISI_1045`
//! cantilever, `FixedSupport(target: "x_min")`, `n_modes: 2`, P2 elements),
//! measured on this branch at f₁ = 418.0101090358127 Hz and
//! f₂ = 1616.206557442738 Hz — both comfortably inside the physical band the
//! guards below assert. Using ONE source for every arm is what makes the
//! cross-arm additivity claim meaningful: the four solves cannot differ in
//! geometry, material or boundary conditions, only in the damping descriptor.
//!
//! The source is inlined as a `&str` rather than committed as a `.ri` fixture,
//! copying `mechanism_modal_damping_e2e.rs`'s choice and its stated reason: a
//! committed `.ri` read by a Rust test must be registered in
//! `_RUST_COUPLED_RI_FIXTURES` in `scripts/verify.sh`, a verify-pipeline file
//! that escalates this change to the full global gate.

use std::f64::consts::PI;

use reify_core::ValueCellId;
use reify_eval::compute_targets::register_compute_fns;
use reify_ir::Value;
use reify_test_support::{errors_only, make_simple_engine, parse_and_compile_with_stdlib};

/// The Rayleigh β the `_rayl` and `_both` arms declare, in seconds. Kept as a
/// Rust constant so the expected ζ is COMPUTED from the emitted frequency rather
/// than transcribed. α is 0 in both arms, which is what makes ζ_extra = β·ω/2
/// exact rather than approximate.
const BETA: f64 = 1e-4;

/// The one fixture, four damping arms.
///
/// Argument binding for `ModalOptions` is POSITIONAL (the `name:` labels are
/// cosmetic — see the note in `examples/modal/cantilever_beam_modes.ri`), so
/// every param is spelled out in declaration order to reach the trailing
/// `element_order`. The four `ModalOptions` differ ONLY in their `damping:` slot.
const SOURCE: &str = r#"
structure def MaterialDampingProbe {
    param length : Length = 100mm
    param width  : Length = 20mm
    param height : Length = 5mm

    let steel = Steel_AISI_1045()
    let mi = FEAMaterialInput(material: steel)
    let root = FixedSupport(target: "x_min")

    // η read from the MODEL, never transcribed into the assertions.
    let eta : Real = steel.loss_factor

    // ── arm `none`: the undamped baseline (B4) ───────────────────────────────
    let o_none = ModalOptions(
        n_modes: 2,
        boundary_conditions: [root],
        damping: NoDamping(),
        sigma: 0.0,
        tol: 0.000000001,
        max_iters: 200,
        reference_direction: vec3(0.0, 0.0, 1.0),
        element_order: ElementOrder.P2
    )
    let r_none = modal_analysis(mi.material, length, width, height, o_none)
    let f_none_1 : Frequency = r_none.modes[0].frequency
    let f_none_2 : Frequency = r_none.modes[1].frequency
    let z_none_1 : Real = r_none.modes[0].damping_ratio
    let z_none_2 : Real = r_none.modes[1].damping_ratio

    // ── arm `rayl`: pre-existing Rayleigh damping (B4) ───────────────────────
    let o_rayl = ModalOptions(
        n_modes: 2,
        boundary_conditions: [root],
        damping: RayleighDamping(alpha: 0.0Hz, beta: 0.0001s),
        sigma: 0.0,
        tol: 0.000000001,
        max_iters: 200,
        reference_direction: vec3(0.0, 0.0, 1.0),
        element_order: ElementOrder.P2
    )
    let r_rayl = modal_analysis(mi.material, length, width, height, o_rayl)
    let f_rayl_1 : Frequency = r_rayl.modes[0].frequency
    let f_rayl_2 : Frequency = r_rayl.modes[1].frequency
    let z_rayl_1 : Real = r_rayl.modes[0].damping_ratio
    let z_rayl_2 : Real = r_rayl.modes[1].damping_ratio

    // ── arm `mat`: pure modal-strain-energy damping (B5) ─────────────────────
    let o_mat = ModalOptions(
        n_modes: 2,
        boundary_conditions: [root],
        damping: MaterialDamping(),
        sigma: 0.0,
        tol: 0.000000001,
        max_iters: 200,
        reference_direction: vec3(0.0, 0.0, 1.0),
        element_order: ElementOrder.P2
    )
    let r_mat = modal_analysis(mi.material, length, width, height, o_mat)
    let f_mat_1 : Frequency = r_mat.modes[0].frequency
    let f_mat_2 : Frequency = r_mat.modes[1].frequency
    let z_mat_1 : Real = r_mat.modes[0].damping_ratio
    let z_mat_2 : Real = r_mat.modes[1].damping_ratio

    // ── arm `both`: MSE + an additive Rayleigh companion (B7) ────────────────
    let o_both = ModalOptions(
        n_modes: 2,
        boundary_conditions: [root],
        damping: MaterialDamping(extra: RayleighDamping(alpha: 0.0Hz, beta: 0.0001s)),
        sigma: 0.0,
        tol: 0.000000001,
        max_iters: 200,
        reference_direction: vec3(0.0, 0.0, 1.0),
        element_order: ElementOrder.P2
    )
    let r_both = modal_analysis(mi.material, length, width, height, o_both)
    let f_both_1 : Frequency = r_both.modes[0].frequency
    let f_both_2 : Frequency = r_both.modes[1].frequency
    let z_both_1 : Real = r_both.modes[0].damping_ratio
    let z_both_2 : Real = r_both.modes[1].damping_ratio
}
"#;

/// The two mode indices every arm reports, as the `_1` / `_2` cell-name suffixes
/// the source uses. Iterating this rather than hand-unrolling is what makes
/// "for EVERY mode" a real quantifier instead of a spot check.
const MODES: [&str; 2] = ["1", "2"];

/// Read an `f64` out of a numeric value cell (`Real` / `Int` / dimensioned
/// `Scalar`). Panics on a non-numeric cell so a shape regression fails loudly.
/// Mirrors `mechanism_modal_damping_e2e.rs::num`.
fn num(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Int(n) => *n as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a numeric cell, got {other:?}"),
    }
}

/// Compile + evaluate [`SOURCE`], asserting it is clean at BOTH altitudes, and
/// return a reader over its value cells.
///
/// Compiling clean is asserted here rather than in each test because it is a
/// precondition of every arm: a source that failed to type-check would make
/// every ζ assertion below vacuous (the cells would simply be absent, and the
/// panic would name a missing cell rather than the real cause).
fn eval_probe() -> impl Fn(&str) -> f64 {
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    assert!(
        errors_only(&compiled).is_empty(),
        "the MaterialDamping probe source must compile with no error-severity \
         diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval must produce no Error diagnostics for a well-formed damped solve, \
         got: {errors:#?}"
    );

    move |name: &str| {
        let v = eval_result
            .values
            .get(&ValueCellId::new("MaterialDampingProbe", name))
            .unwrap_or_else(|| {
                panic!(
                    "MaterialDampingProbe.{name} not found in eval result; \
                     cells present: {:#?}\nall diagnostics: {:#?}",
                    eval_result
                        .values
                        .iter()
                        .map(|(id, _)| id)
                        .collect::<Vec<_>>(),
                    eval_result.diagnostics
                )
            });
        num(v)
    }
}

/// Assert a frequency is in the physical band this fixture is known to produce.
///
/// This is NOT the damping claim. It guards against a rigid-mode /
/// zero-frequency regression that would make every ζ identity below vacuously
/// satisfiable at ζ = 0 — the same guard clause the landed
/// `mechanism_modal_damping_e2e.rs` carries. Measured band for this fixture:
/// f₁ = 418.01 Hz, f₂ = 1616.21 Hz.
fn assert_physical_band(label: &str, f: f64) {
    assert!(
        f.is_finite() && f > 1.0 && f < 10_000.0,
        "{label} frequency {f} Hz must be finite and in the physical band \
         (1 Hz .. 10 kHz; this fixture measures 418.01 Hz and 1616.21 Hz) — a \
         0 Hz rigid mode would make the ζ assertions vacuous"
    );
}

/// Relative error, for the associativity guards below.
fn rel_err(actual: f64, expected: f64) -> f64 {
    (actual - expected).abs() / expected.abs()
}

/// **B4 — REGRESSION PIN. Expected GREEN from the moment it is written.**
///
/// `NoDamping` and `RayleighDamping` must behave exactly as they did before
/// #6878. This test therefore has no RED phase: it pins pre-existing behaviour
/// so that any later step which perturbs the damping seam goes red HERE, at the
/// author surface, rather than silently changing every existing modal design's
/// numbers.
///
/// Three claims:
///   (a) `NoDamping()` gives ζ == 0.0 EXACTLY (not "small") for every mode;
///   (b) `RayleighDamping(α = 0, β)` gives ζ == β·ω/2 recomputed from the SAME
///       emitted `Mode.frequency`, to within fp associativity;
///   (c) the frequencies are BIT-FOR-BIT equal across the two descriptors —
///       damping is applied after the eigensolve and must never touch it. (c) is
///       the claim that would catch a damping change that accidentally perturbed
///       the assembly or the cache key.
#[test]
fn material_damping_leaves_nodamping_and_rayleigh_byte_identical() {
    let cell = eval_probe();
    let eta = cell("eta");
    assert!(
        eta > 0.0,
        "the fixture material must carry a positive η for the sibling tests to \
         be meaningful; got {eta}"
    );

    for m in MODES {
        let f_none = cell(&format!("f_none_{m}"));
        let f_rayl = cell(&format!("f_rayl_{m}"));
        assert_physical_band(&format!("mode {m} (NoDamping)"), f_none);

        // (a) NoDamping is EXACTLY zero.
        let z_none = cell(&format!("z_none_{m}"));
        assert_eq!(
            z_none, 0.0,
            "mode {m}: NoDamping() must give damping_ratio exactly 0.0, got \
             {z_none} — #6878 must not perturb the undamped path"
        );

        // (b) Rayleigh is the pre-existing closed form, recomputed from the
        // emitted frequency. α = 0 ⇒ ζ = (0 + β·ω²)/(2·ω) = β·ω/2 exactly.
        let omega = 2.0 * PI * f_rayl;
        let expected = BETA * omega / 2.0;
        assert!(
            expected > 0.0,
            "mode {m}: fixture (α = 0, β = {BETA}) at f = {f_rayl} Hz must give \
             a nonzero expected ζ, else this assertion is vacuous"
        );
        let z_rayl = cell(&format!("z_rayl_{m}"));
        let e = rel_err(z_rayl, expected);
        assert!(
            e < 1e-9,
            "mode {m}: RayleighDamping ζ {z_rayl} must equal β·ω/2 = {expected} \
             (β = {BETA}, ω = 2π·{f_rayl}) to within fp associativity; relative \
             error {e:.3e} ≥ 1e-9"
        );

        // (c) The eigensolve is untouched by the descriptor.
        assert_eq!(
            f_none, f_rayl,
            "mode {m}: frequencies must be BIT-FOR-BIT equal across NoDamping \
             and RayleighDamping — damping is applied after the eigensolve and \
             is excluded from the assembly cache key, so a difference here means \
             the damping seam reached the eigensolve"
        );
    }
}

/// **B5 — the degenerate MSE identity, ζ = η/2.**
///
/// RED before step-8: measured on this branch, `damping: MaterialDamping()`
/// yields `z_mat_1 = 0`, `z_mat_2 = 0`, exit 0, zero diagnostics.
///
/// η is read from the model (`steel.loss_factor`), so this test cannot pass by
/// agreeing with a transcribed constant — it can only pass if the producer read
/// the same material the model declares. The `ζ > 0` clause is deliberately
/// separate from the identity clause: it is the one assertion that CANNOT be
/// satisfied by rounding, so it separates today's exact 0 from the identity even
/// if η were somehow tiny.
#[test]
fn material_damping_gives_half_the_loss_factor_for_every_mode() {
    let cell = eval_probe();
    let eta = cell("eta");
    assert!(
        eta.is_finite() && eta > 0.0,
        "η must be read from the model as a positive finite number; got {eta}. \
         Steel_AISI_1045 declares loss_factor = 0.0006"
    );
    let expected = eta / 2.0;

    for m in MODES {
        let f = cell(&format!("f_mat_{m}"));
        assert_physical_band(&format!("mode {m} (MaterialDamping)"), f);

        let zeta = cell(&format!("z_mat_{m}"));

        // THE DEFECT PIN. The pre-#6878 producer emits exactly 0.0 here.
        assert!(
            zeta > 0.0,
            "mode {m}: MaterialDamping() must produce a POSITIVE damping ratio \
             (expected η/2 = {expected} for η = {eta}), got {zeta}. A ζ of \
             exactly 0 means the FEA producer silently dropped the declared \
             MaterialDamping descriptor — the INV-SF-3 shape #6878 exists to \
             eliminate."
        );

        // THE IDENTITY PIN. Single-material MSE energy ratio ≡ 1 by
        // construction (PRD §C5), so ζ = η/2 EXACTLY for every mode,
        // independent of mode shape. Both sides are the same single f64
        // multiply; only fp associativity separates them.
        let e = rel_err(zeta, expected);
        assert!(
            e < 1e-9,
            "mode {m}: MaterialDamping ζ {zeta} must equal η/2 = {expected} \
             (η = {eta}, read from the model) to within fp associativity. This \
             is an algebraic identity, not a converged value: with one material \
             the modal-strain-energy ratio is 1 for ANY mode shape. Relative \
             error {e:.3e} ≥ 1e-9"
        );
    }

    // ζ must be MODE-INDEPENDENT here, which is the sharpest statement of the
    // degenerate identity: it holds for any φ, so the two modes — whose shapes
    // and frequencies differ by a factor of ~4 — must agree bit-for-bit.
    assert_eq!(
        cell("z_mat_1"),
        cell("z_mat_2"),
        "the single-material MSE ratio is ≡ 1 for ANY mode shape, so ζ must be \
         identical across modes even though their frequencies differ ~4×; a \
         difference means the producer made ζ_material depend on the mode"
    );
}

/// **B7 — additive composition, ζ = η/2 + β·ω/2.**
///
/// RED before step-8: measured on this branch,
/// `MaterialDamping(extra: RayleighDamping(...))` yields `z_both_1 = 0`,
/// `z_both_2 = 0`, exit 0, zero diagnostics.
///
/// Two independent statements of the same claim, so a failure localises:
///   (i) the CLOSED FORM — ζ equals η/2 + β·ω/2, with ω recomputed from the
///       emitted `Mode.frequency` and BOTH terms proven individually nonzero, so
///       neither can vanish and let the other alone satisfy the sum;
///  (ii) the STRUCTURAL claim — ζ_both(i) − ζ_mat(i) == ζ_rayl(i), i.e. adding
///       the `extra` descriptor contributes exactly what that descriptor
///       contributes on its own. That is the composition claim itself, and it
///       holds independently of either closed form: it would survive a change to
///       the Rayleigh formula, and it would fail for any non-additive
///       composition (a max, a sum-of-squares, a replacement).
#[test]
fn material_damping_composes_additively_with_its_extra_descriptor() {
    let cell = eval_probe();
    let eta = cell("eta");
    let zeta_material = eta / 2.0;
    assert!(
        zeta_material > 0.0,
        "the MSE half must be nonzero for the additivity claim to have two \
         terms; η = {eta}"
    );

    for m in MODES {
        let f = cell(&format!("f_both_{m}"));
        assert_physical_band(&format!("mode {m} (MaterialDamping + extra)"), f);

        // (i) THE CLOSED FORM. Both halves individually nonzero first.
        let omega = 2.0 * PI * f;
        let zeta_extra = BETA * omega / 2.0;
        assert!(
            zeta_extra > 0.0,
            "mode {m}: the extra (Rayleigh) half must be nonzero — β = {BETA} \
             at ω = 2π·{f} — else the sum could be satisfied by the MSE half \
             alone"
        );
        let expected = zeta_material + zeta_extra;
        let zeta = cell(&format!("z_both_{m}"));
        let e = rel_err(zeta, expected);
        assert!(
            e < 1e-9,
            "mode {m}: MaterialDamping(extra: RayleighDamping(α = 0, β = \
             {BETA})) ζ {zeta} must equal η/2 + β·ω/2 = {zeta_material} + \
             {zeta_extra} = {expected} (η = {eta}, ω = 2π·{f}) to within fp \
             associativity; relative error {e:.3e} ≥ 1e-9"
        );

        // (ii) THE STRUCTURAL CLAIM, independent of both closed forms: the
        // `extra` companion contributes exactly what it contributes alone.
        let delta = zeta - cell(&format!("z_mat_{m}"));
        let z_rayl = cell(&format!("z_rayl_{m}"));
        let e_struct = rel_err(delta, z_rayl);
        assert!(
            e_struct < 1e-9,
            "mode {m}: composition must be ADDITIVE — ζ_both − ζ_mat = {delta} \
             must equal the standalone ζ_rayl = {z_rayl} on the SAME fixture and \
             the SAME mode. This is the composition claim itself and does not \
             depend on either closed form; relative error {e_struct:.3e} ≥ 1e-9"
        );
    }
}

// ─── B8: the loud coded rejection ────────────────────────────────────────────

/// The diagnostic code the FEA producer must emit when `MaterialDamping` is
/// declared over a material that does not conform to `Damped`. A message-prefix
/// code, matching every other modal/transient diagnostic in `modal_ops.rs`
/// (`E_ModalNoMassMatrix`, `E_ModalNoModesComputed`, `E_TransientForcingMissing`,
/// `W_MechanismModalUnsupportedDamping`) rather than a `DiagnosticCode` registry
/// variant — that file's explicit, documented, opposite-to-the-house convention.
const NOT_DAMPED_CODE: &str = "E_ModalDampingMaterialNotDamped";

/// A source declaring a material that type-checks as `ElasticMaterial` but does
/// NOT conform to `Damped`, and asking for `MaterialDamping` over it.
///
/// This is reachable ONLY because `modal_analysis(material : ElasticMaterial, …)`
/// (`crates/reify-compiler/stdlib/modal_analysis_fns.ri`) does not require
/// `Damped` — verified on this branch: this exact source compiles and evaluates
/// CLEAN today, exit 0, f₁ = 418.01 Hz, ζ = 0. That is the silent-zero defect B8
/// converts, and it is why the rejection has to be eval-time.
///
/// `{damping}` is substituted per arm so the rejection and the three silence
/// arms share one material and one geometry.
const NOT_DAMPED_SOURCE_TEMPLATE: &str = r#"
structure def PlainSteel : ElasticMaterial {
    param youngs_modulus : Pressure = 205GPa
    param poisson_ratio : Real = 0.29
    param density : Density = 7850kg/m^3
    param yield_stress : Option<Pressure> = some(310MPa)
}

structure def NotDampedProbe {
    param length : Length = 100mm
    param width  : Length = 20mm
    param height : Length = 5mm

    let plain = PlainSteel()
    let mi = FEAMaterialInput(material: plain)
    let root = FixedSupport(target: "x_min")

    let opts = ModalOptions(
        n_modes: 2,
        boundary_conditions: [root],
        damping: {damping},
        sigma: 0.0,
        tol: 0.000000001,
        max_iters: 200,
        reference_direction: vec3(0.0, 0.0, 1.0),
        element_order: ElementOrder.P2
    )
    let r = modal_analysis(mi.material, length, width, height, opts)
    let n_modes_returned : Int = r.modes.count
}
"#;

/// A `MaterialDamping` solve over a genuinely `Damped` material — the SILENCE
/// arm that proves the rejection keys on conformance and not merely on the
/// descriptor. Reuses the main [`SOURCE`] fixture's material.
const DAMPED_MATERIAL_SOURCE: &str = r#"
structure def DampedProbe {
    param length : Length = 100mm
    param width  : Length = 20mm
    param height : Length = 5mm

    let steel = Steel_AISI_1045()
    let mi = FEAMaterialInput(material: steel)
    let root = FixedSupport(target: "x_min")

    let opts = ModalOptions(
        n_modes: 2,
        boundary_conditions: [root],
        damping: MaterialDamping(),
        sigma: 0.0,
        tol: 0.000000001,
        max_iters: 200,
        reference_direction: vec3(0.0, 0.0, 1.0),
        element_order: ElementOrder.P2
    )
    let r = modal_analysis(mi.material, length, width, height, opts)
    let n_modes_returned : Int = r.modes.count
}
"#;

/// Compile + eval `source`, asserting only that it COMPILES clean, and return
/// `(mode count returned, eval diagnostics)`.
///
/// Deliberately does NOT assert eval is clean — these arms are exactly about
/// what eval says.
fn eval_diagnostics(struct_name: &str, source: &str) -> (i64, Vec<reify_core::Diagnostic>) {
    let compiled = parse_and_compile_with_stdlib(source);
    // (i) The source must COMPILE with no error-severity diagnostics. This is a
    // deliberately EVAL-time rejection, not a check-time one — the compiler
    // cannot catch it, because `modal_analysis`'s param is `ElasticMaterial`.
    // Pinning both halves keeps the two channels from being confused.
    assert!(
        errors_only(&compiled).is_empty(),
        "the probe source must COMPILE with no error-severity diagnostics — the \
         MaterialDamping/`Damped` mismatch is deliberately an eval-time \
         rejection, not a check-time one; got:\n{:#?}",
        errors_only(&compiled)
    );
    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);
    let n = eval_result
        .values
        .get(&ValueCellId::new(struct_name, "n_modes_returned"))
        .map(num)
        .unwrap_or_else(|| {
            panic!(
                "{struct_name}.n_modes_returned not found; diagnostics: {:#?}",
                eval_result.diagnostics
            )
        });
    (n as i64, eval_result.diagnostics.clone())
}

/// **B8 — a `MaterialDamping` solve over a non-`Damped` material is a LOUD,
/// CODED, eval-time Error naming the material and the fix.**
///
/// RED before step-10: measured on this branch, this exact source evaluates
/// silently to `f₁ = 418.0101090358127 Hz`, `ζ = 0`, exit 0, zero diagnostics.
///
/// B8's normative wording is "eval error naming the material; NOT
/// `damping_ratio = 0`", so clause (vi) is not decoration: a result that still
/// hands back modes carrying a zero ζ fails the signal even with the Error
/// present. A downstream consumer reading `ModalResult` without inspecting the
/// diagnostics channel would be back in the silent-zero shape.
///
/// INV-SF-2. Clause (ii) asserts the severity is `Severity::Error`, which is
/// what `cmd_eval`'s severity gate turns into a process exit code of 1 —
/// verified independently on this branch by observing `reify eval` return 1 on
/// an `E_ModalNoModesComputed` Error. Asserting the severity here is asserting
/// the nonzero exit.
#[test]
fn material_damping_over_a_non_damped_material_is_a_coded_eval_error() {
    let (n_modes, diagnostics) = eval_diagnostics(
        "NotDampedProbe",
        &NOT_DAMPED_SOURCE_TEMPLATE.replace("{damping}", "MaterialDamping()"),
    );

    // (ii) EXACTLY ONE Error-severity diagnostic — not zero (the defect), and
    // not several (which would mean the guard fired downstream of itself).
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "a MaterialDamping solve over a material that does not conform to \
         `Damped` must emit EXACTLY ONE Error-severity diagnostic. Zero means \
         the silent ζ = 0 defect is still live (INV-SF-3); more than one means \
         the guard did not short-circuit. Got: {diagnostics:#?}"
    );
    let message = &errors[0].message;

    // (iii) the message-prefix code, at the START of the message.
    assert!(
        message.starts_with(NOT_DAMPED_CODE),
        "the diagnostic must start with the code `{NOT_DAMPED_CODE}` (the \
         message-prefix convention every other modal diagnostic in \
         modal_ops.rs uses); got: {message}"
    );

    // (iv) it NAMES the offending material's runtime type, so the author can
    // locate it — mirroring how `W_MechanismModalUnsupportedDamping` names the
    // offending descriptor.
    assert!(
        message.contains("PlainSteel"),
        "the diagnostic must name the offending material's runtime type so the \
         author can locate it; got: {message}"
    );

    // (v) it NAMES THE FIX in the author's own vocabulary. A diagnostic that
    // reports a problem without naming the one-line remedy just relocates the
    // author's confusion.
    for needle in ["DampedMaterial", "loss_factor"] {
        assert!(
            message.contains(needle),
            "the diagnostic must name the fix — it must mention `{needle}` so \
             the author knows what to add and where; got: {message}"
        );
    }

    // (vi) NO MODES CARRYING ζ = 0. B8's normative wording is "eval error
    // naming the material; NOT damping_ratio = 0", so an Error alongside a full
    // modes list still fails the signal.
    assert_eq!(
        n_modes, 0,
        "the rejection must return the DEGENERATE empty-modes ModalResult, not \
         a full modes list whose damping_ratio is 0 — a consumer reading \
         ModalResult without inspecting diagnostics would otherwise be back in \
         the silent-zero shape (PRD §C6). Got {n_modes} modes."
    );
}

/// **The SILENCE half of B8**, without which the new diagnostic is noise rather
/// than signal.
///
/// Three arms that must emit NO `E_ModalDampingMaterialNotDamped` at all and
/// must still return a full modes list:
///   - the SAME non-`Damped` material under `NoDamping()` and under
///     `RayleighDamping(...)` — the rejection must key on the DESCRIPTOR as well
///     as on conformance, so a material with no loss factor stays perfectly
///     usable for every damping model that does not need one;
///   - `MaterialDamping()` over a genuinely `Damped` material
///     (`Steel_AISI_1045`) — the rejection must key on CONFORMANCE as well as on
///     the descriptor.
///
/// Together those two directions are what make the guard a conjunction rather
/// than either half alone.
#[test]
fn the_not_damped_rejection_stays_silent_for_every_valid_combination() {
    let cases: [(&str, &str, String); 3] = [
        (
            "NoDamping over a non-Damped material",
            "NotDampedProbe",
            NOT_DAMPED_SOURCE_TEMPLATE.replace("{damping}", "NoDamping()"),
        ),
        (
            "RayleighDamping over a non-Damped material",
            "NotDampedProbe",
            NOT_DAMPED_SOURCE_TEMPLATE
                .replace("{damping}", "RayleighDamping(alpha: 0.0Hz, beta: 0.0001s)"),
        ),
        (
            "MaterialDamping over a genuinely Damped material",
            "DampedProbe",
            DAMPED_MATERIAL_SOURCE.to_string(),
        ),
    ];

    for (label, struct_name, source) in cases {
        let (n_modes, diagnostics) = eval_diagnostics(struct_name, &source);
        assert!(
            !diagnostics.iter().any(|d| d.message.contains(NOT_DAMPED_CODE)),
            "{label}: must emit NO {NOT_DAMPED_CODE} diagnostic at any \
             severity; got {diagnostics:#?}"
        );
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == reify_core::Severity::Error),
            "{label}: is a valid solve and must produce no Error diagnostics at \
             all; got {diagnostics:#?}"
        );
        assert!(
            n_modes > 0,
            "{label}: must still return a full modes list, got {n_modes} modes \
             — a guard that degenerates a VALID solve is worse than no guard"
        );
    }
}
