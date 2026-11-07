//! Author-surface gate for the `mechanism_modal_analysis` shift warning
//! (task #6097).
//!
//! ## What this pins
//!
//! `ModalOptions.shift_frequency` is HONORED by the FEA `modal_analysis` path
//! (#7261), but the lumped generalized-coordinate path (`run_mechanism_modal`
//! in `crates/reify-eval/src/modal_ops.rs`) still solves at the default and
//! always returns its lowest modes. A non-zero shift there must not be dropped
//! silently (INV-SF-3): it carries a coded `W_ModalOptionUnsupported` warning
//! naming the author's own value and the owner, #7263, which retires the
//! warning by honoring the shift on this path.
//!
//! The warning lives ONLY on the lumped path. Its negative half — the FEA path
//! never claims an honored shift is un-honored — is pinned in
//! `crates/reify-eval-fea-tests/tests/shift_invert_modal_e2e.rs`.
//!
//! Inlined `.ri` rather than a committed fixture, for the reason
//! `mechanism_modal_damping_e2e.rs`'s header records.

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::compute_targets::register_compute_fns;
use reify_test_support::{errors_only, make_simple_engine, parse_and_compile_with_stdlib};

use crate::numeric_cell::as_f64;

/// The `mechanism_modal_damping_e2e.rs` probe (0.5 kg carriage on a steel
/// parallelogram flexure), undamped, with `shift` as the `shift_frequency:`
/// literal.
fn source(shift: &str) -> String {
    format!(
        r#"
structure def MechanismShiftProbe {{
    let steel = Steel_AISI_1045()
    let z_carriage_mass = 0.5kg

    let z_flexure = prb_parallelogram_flexure(
        20mm, 5mm, 0.5mm, 10mm, steel, vec3(0, 0, 1), point3(0mm, 0mm, 0mm))

    let m0 = mechanism()
    let carriage = body(m0, point_mass(z_carriage_mass), z_flexure)

    let opts = ModalOptions(
        n_modes: 1,
        boundary_conditions: [],
        damping: NoDamping(),
        shift_frequency: {shift},
        tol: 0.000000001,
        max_iters: 200,
        reference_direction: vec3(0.0, 0.0, 1.0),
        element_order: ElementOrder.P2
    )

    let z_modal = mechanism_modal_analysis(carriage, opts)
    let f1 = first_frequency(z_modal)
}}
"#
    )
}

/// The first-mode frequency and every eval diagnostic for one shift literal.
struct ShiftRun {
    f1: f64,
    diagnostics: Vec<reify_core::Diagnostic>,
}

impl ShiftRun {
    fn unsupported(&self) -> Vec<&reify_core::Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::ModalOptionUnsupported))
            .collect()
    }
}

fn run(shift: &str) -> ShiftRun {
    let compiled = parse_and_compile_with_stdlib(&source(shift));
    assert!(
        errors_only(&compiled).is_empty(),
        "shift_frequency: {shift}: the probe must compile with no error-severity \
         diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);
    let f1 = eval_result
        .values
        .get(&ValueCellId::new("MechanismShiftProbe", "f1"))
        .map(as_f64)
        .unwrap_or_else(|| {
            panic!(
                "shift_frequency: {shift}: MechanismShiftProbe.f1 not found; \
                 diagnostics: {:#?}",
                eval_result.diagnostics
            )
        });
    ShiftRun {
        f1,
        diagnostics: eval_result.diagnostics.clone(),
    }
}

/// A non-zero `shift_frequency` on the lumped path warns, names the author's
/// value and the owner, and is honest that the solve fell back; the default
/// `0Hz` is not a dropped intent and stays silent.
#[test]
fn mechanism_modal_nonzero_shift_frequency_warns_and_falls_back() {
    let unshifted = run("0Hz");
    let shifted = run("100Hz");

    // (a) exactly one coded, cited Warning naming the author's own value.
    let warnings = shifted.unsupported();
    assert_eq!(
        warnings.len(),
        1,
        "a non-zero shift_frequency on the lumped path must carry EXACTLY ONE \
         ModalOptionUnsupported diagnostic; all diagnostics: {:#?}",
        shifted.diagnostics
    );
    let warning = warnings[0];
    assert_eq!(warning.severity, Severity::Warning, "got {warning:?}");
    for token in ["shift_frequency", "100 Hz", "#7263"] {
        assert!(
            warning.message.contains(token),
            "the warning must contain {token:?}, got: {}",
            warning.message
        );
    }
    assert!(
        warning.message.starts_with("W_ModalOptionUnsupported:"),
        "the mnemonic must be a message PREFIX, got: {}",
        warning.message
    );

    // (b) the default is not a dropped intent.
    assert!(
        unshifted.unsupported().is_empty(),
        "shift_frequency: 0Hz must not warn, got: {:#?}",
        unshifted.unsupported()
    );

    // (c) the warning is honest about the fallback: the shift was not applied.
    assert_eq!(
        shifted.f1, unshifted.f1,
        "the lumped path does not honor the shift, so f1 must be bit-identical \
         with and without it"
    );

    // (d) advisory only — `reify eval` must still exit 0.
    for (label, run) in [("0Hz", &unshifted), ("100Hz", &shifted)] {
        let errors: Vec<_> = run
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "{label}: must carry no Error-severity diagnostic, got: {errors:#?}"
        );
    }
}
