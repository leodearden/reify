//! End-to-end signal for leaf δ (#7261) of
//! `docs/prds/v0_6/shift-invert-eigensolve.md`: the modal trampoline honors the
//! `ModalOptions.sigma` shift in λ-space, and SAYS SO when the shifted result is
//! a window rather than the bottom of the spectrum.
//!
//! The vehicle is the committed fixture PAIR
//! `tests/prd-gate/fixtures/shift_invert_modal_{unshifted,shifted}.ri` —
//! byte-identical except for their `sigma:` literal and header. So the primary
//! assertion is a MODE-SET DIFFERENCE between two files that differ in exactly
//! one number, not "a warning stopped firing" (PRD §G2).
//!
//! WHAT IS RED AND WHAT IS A REGRESSION FLOOR. Only assertion (3) — the
//! `W_ShiftSkippedModes` warning — is RED when this file lands: that code is
//! minted by leaf α but emitted nowhere, and wiring the emit is δ's deliverable.
//! Assertions (1), (2), (5) and (6) are expected GREEN on arrival and are NOT
//! dead weight: they promote β's landed numerics to δ's committed,
//! user-observable signal, so a later change that silences the warning by
//! quietly un-honoring the shift cannot pass by deleting one assertion.
//!
//! Every band below is transcribed from a release `reify eval` measurement of
//! these exact two files on 2026-09-18 (recorded in each fixture's header), not
//! from an analytic estimate.

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── the fixture pair ─────────────────────────────────────────────────────────

fn unshifted_source() -> &'static str {
    include_str!("../../../tests/prd-gate/fixtures/shift_invert_modal_unshifted.ri")
}

fn shifted_source() -> &'static str {
    include_str!("../../../tests/prd-gate/fixtures/shift_invert_modal_shifted.ri")
}

/// The shifted fixture's `sigma:` literal, in EIGENVALUE (λ) space — transcribed
/// from `shift_invert_modal_shifted.ri` exactly once, so the band below and the
/// file it describes cannot drift apart. (Same one-constant-per-fixture
/// discipline as `modal_analysis_e2e.rs`'s `BeamSection`.)
const SIGMA: f64 = 150000000000.0;

/// The frequency σ corresponds to: λ = ω² = (2π·f)², so f_σ = √σ / 2π.
/// Measured 61640.4444 Hz.
fn f_sigma() -> f64 {
    SIGMA.sqrt() / (2.0 * std::f64::consts::PI)
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Read a frequency cell (Hz) as `f64`, tolerating the `Real` placeholder
/// (`Mode.frequency : Real`, modal_analysis.ri) or a dimensioned `Scalar` —
/// the same idiom as `modal_analysis_e2e.rs::read_frequency`.
fn read_frequency(val: &Value) -> f64 {
    match val {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a frequency Real/Scalar, got: {:?}", other),
    }
}

/// The two frequency cells and every diagnostic produced by evaluating one
/// fixture.
struct ModalRun {
    f1: f64,
    f2: f64,
    diagnostics: Vec<reify_core::Diagnostic>,
}

impl ModalRun {
    fn min_frequency(&self) -> f64 {
        self.f1.min(self.f2)
    }

    fn max_frequency(&self) -> f64 {
        self.f1.max(self.f2)
    }

    fn with_code(&self, code: DiagnosticCode) -> Vec<&reify_core::Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.code == Some(code))
            .collect()
    }

    /// Diagnostics whose message carries the given `W_…`/`E_…` prefix. Keyed on
    /// PREFIX, never on full prose, so a wording edit does not move this test.
    fn with_prefix(&self, prefix: &str) -> Vec<&reify_core::Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.message.starts_with(prefix))
            .collect()
    }
}

/// Compile + eval one fixture and collect its `f1`/`f2` cells and diagnostics.
fn run_fixture(source: &str, structure: &str) -> ModalRun {
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    let cell = |name: &str| {
        read_frequency(
            eval_result
                .values
                .get(&ValueCellId::new(structure, name))
                .unwrap_or_else(|| panic!("cell {structure}.{name} not found in eval result")),
        )
    };

    ModalRun {
        f1: cell("f1"),
        f2: cell("f2"),
        diagnostics: eval_result.diagnostics.clone(),
    }
}

// ── the signal ───────────────────────────────────────────────────────────────

/// Leaf δ's end-to-end deliverable, asserted over the committed fixture pair.
///
/// Both files in one test because the load-bearing claim is a COMPARISON: they
/// differ in exactly one literal, so any difference in the frequency pairs is
/// attributable to the shift and to nothing else. Splitting them would let each
/// half pass while the pair says nothing.
/// NOT release-gated: both evals were MEASURED at 2.08 s each in debug on the
/// 504-DOF fixture mesh, so this stays gate-resident and needs no
/// `.config/nextest.toml` slow-timeout override or heavy-test-filter entry.
#[test]
fn shift_changes_the_mode_set_and_warns_once() {
    let unshifted = run_fixture(unshifted_source(), "ShiftInvertModalUnshifted");
    let shifted = run_fixture(shifted_source(), "ShiftInvertModalShifted");

    // (1) MODE-SET DIFFERENCE — the signal. Measured ratio 1.9975 (unshifted
    //     {18829.10, 28285.08} Hz vs shifted {56498.80, 65303.56} Hz): a
    //     genuinely DISJOINT mode set, not a numerical wobble, so the 1.5 floor
    //     has ~33% of headroom under the measurement.
    assert!(
        shifted.min_frequency() > 1.5 * unshifted.max_frequency(),
        "the shifted solve must return a DIFFERENT mode set, not a perturbed one: \
         min(shifted) = {} must exceed 1.5 · max(unshifted) = {} \
         (measured ratio 1.9975); shifted = {:?}, unshifted = {:?}",
        shifted.min_frequency(),
        1.5 * unshifted.max_frequency(),
        (shifted.f1, shifted.f2),
        (unshifted.f1, unshifted.f2),
    );

    // (2) CLUSTERING — the shifted pair brackets f_σ, which is what "a window
    //     around sigma" means. Measured 0.9166× and 1.0594×, so the ±15% band
    //     holds both with margin.
    let f_sigma = f_sigma();
    for f in [shifted.f1, shifted.f2] {
        let rel = (f - f_sigma).abs() / f_sigma;
        assert!(
            rel <= 0.15,
            "shifted frequency {f} Hz must sit within ±15% of f_sigma = {f_sigma} Hz \
             (measured 0.9166× and 1.0594×), got {:.1}% off",
            rel * 100.0,
        );
    }

    // (3) WARNING PRESENT — RED until δ wires the emit. Keyed on the typed code
    //     AND the message prefix; the prose in between is free to be edited.
    let skipped = shifted.with_code(DiagnosticCode::ShiftSkippedModes);
    assert_eq!(
        skipped.len(),
        1,
        "the shifted run must carry EXACTLY ONE ShiftSkippedModes diagnostic, got {:?}; \
         all diagnostics: {:?}",
        skipped,
        shifted.diagnostics,
    );
    let warning = skipped[0];
    assert_eq!(
        warning.severity,
        Severity::Warning,
        "W_ShiftSkippedModes is advisory — inspecting a band around sigma is a legitimate \
         use — so it must be a Warning, got {:?}",
        warning.severity,
    );
    assert!(
        warning.message.starts_with("W_ShiftSkippedModes:"),
        "message must carry the canonical prefix, got: {}",
        warning.message,
    );
    assert!(
        warning.message.contains(&format!("{SIGMA}")),
        "the warning must NAME the sigma that was applied ({SIGMA}) so a reader never has \
         to infer which shift produced the window, got: {}",
        warning.message,
    );

    // (4) WARNING ABSENT on the control. sigma = 0 makes
    //     `shift_provenance_from_factorization` return false unconditionally, so
    //     this can never fire on an unshifted solve.
    assert!(
        unshifted.with_code(DiagnosticCode::ShiftSkippedModes).is_empty(),
        "the unshifted run must carry NO ShiftSkippedModes diagnostic, got: {:?}",
        unshifted.with_code(DiagnosticCode::ShiftSkippedModes),
    );

    // (5) INV-SF-2 severity-hygiene corollary. This is `cmd_eval`'s exact
    //     predicate (`crates/reify-cli/src/main.rs`, the general severity fold —
    //     INV-SF-2 forbids a per-code bolt-on), so asserting it here is
    //     asserting that `reify eval` still exits 0 on a legitimate band
    //     inspection.
    for (label, run) in [("unshifted", &unshifted), ("shifted", &shifted)] {
        let errors: Vec<_> = run
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "{label}: a healthy shifted/unshifted solve must carry ZERO Error-severity \
             diagnostics (cmd_eval folds any Error into a FAILURE exit), got: {:?}",
            errors,
        );
    }

    // (6) NEGATIVE HALF (β's discipline). Each of these names a remedy that is
    //     WRONG for these fixtures — their supports are fine, their iteration
    //     budget is fine, and both solves returned modes. A diagnostic that
    //     sends the author to fix the wrong thing is the defect class this leaf
    //     is forbidden to create, so it is asserted absent rather than assumed.
    for (label, run) in [("unshifted", &unshifted), ("shifted", &shifted)] {
        for prefix in [
            "W_ModalRigidBodyMode",
            "E_ModalNoModesComputed",
            "W_ModalConvergence",
        ] {
            assert!(
                run.with_prefix(prefix).is_empty(),
                "{label}: must not emit {prefix} — its remedy is wrong for this fixture; got: {:?}",
                run.with_prefix(prefix),
            );
        }
    }
}
