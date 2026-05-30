//! Regression test for `examples/trajectory/zv_shaped_ramp.ri` (task ε — 3866).
//!
//! Pins two leaf signals (pattern: constants_example_tests.rs):
//!
//!   1. The file parses with zero errors.
//!   2. It compiles under the stdlib prelude with zero Error-severity diagnostics.
//!
//! The example is a construction-only smoke test (ZVShaper + ramp
//! PiecewisePolynomialProfile).  `input_shape` (ζ) and `simulate_trajectory`
//! (θ) are intentionally NOT called — they are not yet stdlib signatures and
//! would break the examples_smoke gate.
//!
//! Path resolution uses `CARGO_MANIFEST_DIR` so it works in any worktree.

use reify_core::Severity;

/// `examples/trajectory/zv_shaped_ramp.ri` must parse and compile under
/// the stdlib prelude with zero Error-severity diagnostics.
///
/// Uses `parse_with_stdlib` (the prelude-aware parser) so that stdlib enum
/// variants such as `SplineKind.CubicSpline` are disambiguated as
/// `EnumAccess` nodes rather than member-access chains — identical to how
/// `examples_smoke.rs::smoke_one` parses every example file.
#[test]
fn zv_shaped_ramp_example_compiles_under_stdlib_with_zero_errors() {
    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/trajectory/zv_shaped_ramp.ri"
    );

    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/trajectory/zv_shaped_ramp.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists",
    );

    // ── Parse ──────────────────────────────────────────────────────────────────
    // Use the prelude-aware parser so stdlib enum names (e.g. SplineKind) are
    // injected into the EnumAccess disambiguation set before parsing.

    let parsed = reify_compiler::parse_with_stdlib(
        &src,
        reify_core::ModulePath::single("zv_shaped_ramp"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors in examples/trajectory/zv_shaped_ramp.ri: {:#?}",
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
        "expected zero Error diagnostics compiling examples/trajectory/zv_shaped_ramp.ri \
         under stdlib, got:\n{:#?}",
        errors
    );
}
