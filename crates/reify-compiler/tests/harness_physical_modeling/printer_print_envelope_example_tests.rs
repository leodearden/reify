//! Regression test for `examples/trajectory/printer_print_envelope.ri` (task ρ — 3878).
//!
//! Pins three leaf signals (pattern: tots_optimal_ptp_example_tests.rs):
//!
//!   1. The file parses with zero errors.
//!   2. It compiles under the stdlib prelude with zero Error-severity diagnostics,
//!      less the two per-SITE waivers in `CTOR_CONFORMANCE_MIGRATION_DEBT` (see
//!      the filter in signal 2's body for why they are waived, not fixed).
//!   3. The compiled module exposes a `PrinterPrintEnvelope` structure template
//!      (distinguishes this test from the bulk examples_smoke gate, which only
//!      checks compile-clean without inspecting the resulting template set, and
//!      also proves the correct file was resolved via CARGO_MANIFEST_DIR).
//!
//! The example is the TERMINAL user-observable dogfood for the four-PRD stack
//! (kinematics + RBD + modal + trajectory). "Runs end-to-end" means the full
//! modal → trajectory → input_shape → simulate_trajectory → peak_deviation
//! pipeline compiles with ZERO unwaived Error diagnostics under the
//! examples_smoke gate.
//! The numeric budget decision is verified at the Rust eval layer by
//! `printer_print_envelope_e2e.rs` (eval registers compute fns before eval).
//!
//! Path resolution uses `CARGO_MANIFEST_DIR` so it works in any worktree.

use reify_core::Severity;
use reify_test_support::ctor_conformance_debt::{
    PRINTER_PRINT_ENVELOPE_REL_KEY, is_migration_debt_diagnostic,
};

/// `examples/trajectory/printer_print_envelope.ri` must parse and compile under
/// the stdlib prelude with zero unwaived Error-severity diagnostics, and expose a
/// `PrinterPrintEnvelope` structure template.
///
/// The template-presence assertion distinguishes this test from
/// `examples_smoke.rs::all_examples_parse_and_compile_with_stdlib`, which only
/// checks compile-clean across all examples without inspecting the resulting
/// template set; it also proves the correct file was resolved via
/// `CARGO_MANIFEST_DIR` (a wrong-file resolution would not produce
/// `PrinterPrintEnvelope`).
///
/// Uses `parse_with_stdlib` (the prelude-aware parser) so that stdlib enum
/// variants such as `SplineKind.CubicSpline` and `ElementOrder.P2` are
/// disambiguated as `EnumAccess` nodes rather than member-access chains —
/// identical to how `examples_smoke.rs::smoke_one` parses every example file.
#[test]
fn printer_print_envelope_example_compiles_under_stdlib_with_zero_errors() {
    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/trajectory/printer_print_envelope.ri"
    );

    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/trajectory/printer_print_envelope.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists",
    );

    // ── Parse ──────────────────────────────────────────────────────────────────
    // Use the prelude-aware parser so stdlib enum names (e.g. SplineKind,
    // ElementOrder) are injected into the EnumAccess disambiguation set before
    // parsing.

    let parsed = reify_compiler::parse_with_stdlib(
        &src,
        reify_core::ModulePath::single("printer_print_envelope"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors in examples/trajectory/printer_print_envelope.ri: {:#?}",
        parsed.errors
    );

    // ── Compile ────────────────────────────────────────────────────────────────

    let module = reify_compiler::compile_with_stdlib(&parsed);

    // δ (#5306) flipped CTOR_FIELD_CONFORMANCE_SEVERITY to Error, which put this
    // file's two un-migrated ctor sites (`velocity_limit`, `acceleration_limit`) in
    // front of this gate. They stay WAIVED, not fixed: esc-5305-3 (Leo) ruled against
    // both migrating them here and taking a dependency edge on #5847, which owns
    // dimensioning them — they cannot be dimensioned in isolation without collapsing
    // the TOTS solve. The waiver rule lives in ONE place
    // (`reify_test_support::ctor_conformance_debt`) because three gates in two crates
    // need it; it is keyed on `(file, param)` AND `ArgTypeMismatch` AND Error, so
    // every OTHER Error this file could emit still fails here.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| !is_migration_debt_diagnostic(PRINTER_PRINT_ENVELOPE_REL_KEY, d))
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero unwaived Error diagnostics compiling \
         examples/trajectory/printer_print_envelope.ri under stdlib, got:\n{:#?}",
        errors
    );

    // ── Template presence ──────────────────────────────────────────────────────
    //
    // The compiled module must expose a `PrinterPrintEnvelope` structure template.
    // This assertion distinguishes the test from the bulk examples_smoke gate.

    assert!(
        module
            .templates
            .iter()
            .any(|t| t.name == "PrinterPrintEnvelope"),
        "expected a 'PrinterPrintEnvelope' structure template in compiled \
         printer_print_envelope.ri; found templates: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}
