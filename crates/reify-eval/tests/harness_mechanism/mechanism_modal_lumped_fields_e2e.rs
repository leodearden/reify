//! Author-surface gate for the `ModalResult` fields of
//! `mechanism_modal_analysis` (task #7012).
//!
//! The lumped generalized-coordinate producer (`run_mechanism_modal` in
//! `crates/reify-eval/src/modal_ops.rs`) has no per-node mode shape, no
//! direction to project a participation mass onto, and applies no `Support`.
//! It used to report those as `[]`, `0` and `[]`: plausible values that
//! downstream arithmetic such as `pm + 1.0` silently consumed. This pins them,
//! and what is computed from them, as `undef`. It also pins the matrix norms,
//! which the lumped model does have, as the solve's own `‖M‖` / `‖K‖` rather
//! than the `0` they used to be.
//!
//! The source is inlined rather than committed as a fixture for the reason
//! `mechanism_modal_damping_e2e.rs` gives: a committed `.ri` read by a Rust
//! test must be registered in `_RUST_COUPLED_RI_FIXTURES` in
//! `scripts/verify.sh`.

use std::sync::OnceLock;

use reify_core::{Severity, ValueCellId};
use reify_eval::EvalResult;
use reify_eval::compute_targets::register_compute_fns;
use reify_ir::Value;
use reify_test_support::{errors_only, make_simple_engine, parse_and_compile_with_stdlib};

use crate::numeric_cell::as_f64;

const PROBE: &str = "MechanismLumpedFieldsProbe";

const SOURCE: &str = r#"
structure def MechanismLumpedFieldsProbe {
    let steel = Steel_AISI_1045()
    let z_carriage_mass = 0.5kg
    let z_flexure = prb_parallelogram_flexure(20mm, 5mm, 0.5mm, 10mm, steel, vec3(0, 0, 1), point3(0mm, 0mm, 0mm))
    let z_effective_stiffness = flexure_compliance(z_flexure).effective_stiffness
    let m0 = mechanism()
    let carriage = body(m0, point_mass(z_carriage_mass), z_flexure)
    let base = FixedSupport(target: "base")
    let opts = ModalOptions(n_modes: 1, boundary_conditions: [base], damping: NoDamping(),
        sigma: 0.0, tol: 0.000000001, max_iters: 200,
        reference_direction: vec3(1.0, 0.0, 0.0), element_order: ElementOrder.P1)
    let z_modal = mechanism_modal_analysis(carriage, opts)
    let z_first_mode_hz = first_frequency(z_modal)
    let pm = z_modal.modes[0].participation_mass
    let pm_plus_one = pm + 1.0
    let shape = z_modal.modes[0].shape
    let bcs = z_modal.boundary_conditions
    let mass_norm = z_modal.mass_matrix_norm
    let stiffness_norm = z_modal.stiffness_matrix_norm
}
"#;

/// [`SOURCE`] compiled and evaluated once for every test in this module.
fn probe_result() -> &'static EvalResult {
    static PROBE_RESULT: OnceLock<EvalResult> = OnceLock::new();
    PROBE_RESULT.get_or_init(eval_probe)
}

/// Compile and evaluate [`SOURCE`], asserting neither step reports an error.
fn eval_probe() -> EvalResult {
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    assert!(
        errors_only(&compiled).is_empty(),
        "the probe source must compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval must produce no Error diagnostics, got: {errors:#?}"
    );
    eval_result
}

fn cell<'a>(eval_result: &'a EvalResult, name: &str) -> &'a Value {
    eval_result
        .values
        .get(&ValueCellId::new(PROBE, name))
        .unwrap_or_else(|| {
            panic!(
                "{PROBE}.{name} not found in eval result; all diagnostics: {:#?}",
                eval_result.diagnostics
            )
        })
}

#[test]
fn mechanism_modal_lumped_fields_are_honest_undef_at_author_surface() {
    let cell = |name: &str| cell(probe_result(), name);

    let f = as_f64(cell("z_first_mode_hz"));
    assert!(
        f.is_finite() && f > 1.0 && f < 1000.0,
        "first mode frequency {f} Hz must be finite and in 1..1000 Hz, or the \
         solve did not run and the undef checks below are vacuous"
    );
    let caller_bcs = match cell("opts") {
        Value::StructureInstance(d) => d.fields.get("boundary_conditions"),
        other => panic!("opts must be a ModalOptions instance, got {other:?}"),
    };
    assert!(
        matches!(caller_bcs, Some(Value::List(l)) if l.len() == 1),
        "the caller must supply a one-element support list, or `bcs` being undef \
         rather than an echo proves nothing; got {caller_bcs:?}"
    );

    let not_undef: Vec<(&str, &Value)> = ["pm", "pm_plus_one", "shape", "bcs"]
        .into_iter()
        .map(|name| (name, cell(name)))
        .filter(|(_, value)| **value != Value::Undef)
        .collect();
    assert!(
        not_undef.is_empty(),
        "participation_mass, shape and boundary_conditions are undefined for the \
         lumped model, so they and anything computed from them must be undef; \
         got {not_undef:?}"
    );
}

/// The one-body lumped model is M = [m], K = [k], so ‖M‖_F = m and
/// ‖K‖_F = √(k²) = k. `prb_parallelogram_flexure`'s effective stiffness equals
/// the joint's `spring_rate` to 1e-12 (pinned in
/// `crates/reify-stdlib/src/flexures/compound.rs`), hence the 1e-9 band on ‖K‖.
#[test]
fn mechanism_modal_lumped_matrix_norms_are_the_physical_m_and_k_at_author_surface() {
    let num_cell = |name: &str| as_f64(cell(probe_result(), name));
    let rel_err = |got: f64, expected: f64| (got - expected).abs() / expected;

    let mass = num_cell("z_carriage_mass");
    let mass_norm = num_cell("mass_norm");
    assert!(
        rel_err(mass_norm, mass) <= 1e-12,
        "mass_matrix_norm {mass_norm} must equal the carriage mass {mass} kg"
    );

    let stiffness = num_cell("z_effective_stiffness");
    let stiffness_norm = num_cell("stiffness_norm");
    assert!(
        rel_err(stiffness_norm, stiffness) <= 1e-9,
        "stiffness_matrix_norm {stiffness_norm} must equal the flexure's effective \
         stiffness {stiffness} N/m"
    );
}
