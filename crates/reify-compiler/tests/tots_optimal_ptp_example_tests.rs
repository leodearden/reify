//! Regression test for `examples/trajectory/tots_optimal_ptp.ri` (task λ — 3872).
//!
//! Pins three leaf signals (pattern: zv_shaped_ramp_example_tests.rs):
//!
//!   1. The file parses with zero errors.
//!   2. It compiles under the stdlib prelude with zero Error-severity diagnostics.
//!   3. The compiled module exposes a `TotsOptimalPtp` structure template
//!      (distinguishes this test from the bulk examples_smoke gate, which only
//!      checks compile-clean without inspecting the resulting template set, and
//!      also proves the correct file was resolved via CARGO_MANIFEST_DIR).
//!
//! The example is a construction + input_shape compile-smoke test. There is no
//! numeric `.ri`-level duration assertion — `profile_duration`/`.duration` is
//! a θ-stub returning `0.0 * 1s`, so `optimal.duration < baseline.duration`
//! cannot be asserted at the `.ri` layer. The PRD's duration-improvement
//! property is verified at the Rust solver layer by `tots.rs::sqp_gantry_converges`.
//!
//! Path resolution uses `CARGO_MANIFEST_DIR` so it works in any worktree.

use reify_core::Severity;

/// `examples/trajectory/tots_optimal_ptp.ri` must parse and compile under
/// the stdlib prelude with zero Error-severity diagnostics, and expose a
/// `TotsOptimalPtp` structure template.
///
/// The template-presence assertion distinguishes this test from
/// `examples_smoke.rs::all_examples_parse_and_compile_with_stdlib`, which only
/// checks compile-clean across all examples without inspecting the resulting
/// template set; it also proves the correct file was resolved via
/// `CARGO_MANIFEST_DIR` (a wrong-file resolution would not produce
/// `TotsOptimalPtp`).
///
/// Uses `parse_with_stdlib` (the prelude-aware parser) so that stdlib enum
/// variants such as `SplineKind.CubicSpline` are disambiguated as
/// `EnumAccess` nodes rather than member-access chains — identical to how
/// `examples_smoke.rs::smoke_one` parses every example file.
#[test]
fn tots_optimal_ptp_example_compiles_under_stdlib_with_zero_errors() {
    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/trajectory/tots_optimal_ptp.ri"
    );

    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/trajectory/tots_optimal_ptp.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists",
    );

    // ── Parse ──────────────────────────────────────────────────────────────────
    // Use the prelude-aware parser so stdlib enum names (e.g. SplineKind) are
    // injected into the EnumAccess disambiguation set before parsing.

    let parsed = reify_compiler::parse_with_stdlib(
        &src,
        reify_core::ModulePath::single("tots_optimal_ptp"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors in examples/trajectory/tots_optimal_ptp.ri: {:#?}",
        parsed.errors
    );

    // ── Compile ────────────────────────────────────────────────────────────────

    let module = reify_compiler::compile_with_stdlib(&parsed);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling examples/trajectory/tots_optimal_ptp.ri \
         under stdlib, got:\n{:#?}",
        errors
    );

    // ── Template presence ──────────────────────────────────────────────────────
    //
    // The compiled module must expose a `TotsOptimalPtp` structure template.
    // This assertion distinguishes the test from the bulk examples_smoke gate.

    assert!(
        module.templates.iter().any(|t| t.name == "TotsOptimalPtp"),
        "expected a 'TotsOptimalPtp' structure template in compiled tots_optimal_ptp.ri; \
         found templates: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}
