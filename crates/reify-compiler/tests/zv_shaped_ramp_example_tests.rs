//! Regression test for `examples/trajectory/zv_shaped_ramp.ri` (task ε — 3866).
//!
//! Pins four leaf signals (pattern: constants_example_tests.rs):
//!
//!   1. The file parses with zero errors.
//!   2. It compiles under the stdlib prelude with zero Error-severity diagnostics.
//!   3. The compiled module exposes a `ZvShapedRamp` structure template
//!      (distinguishes this test from the bulk examples_smoke gate, which only
//!      checks compile-clean without inspecting the resulting template set).
//!   4. Positive source-text pins: `ZVShaper` and `PiecewisePolynomialProfile`
//!      must appear in the source (guards against the wrong file being read and
//!      ensures the key constructs from task ε are actually exercised).
//!
//! The example is a construction-only smoke test (ZVShaper + ramp
//! PiecewisePolynomialProfile).  `input_shape` (ζ) and `simulate_trajectory`
//! (θ) are intentionally NOT called — they are not yet stdlib signatures and
//! would break the examples_smoke gate.
//!
//! Path resolution uses `CARGO_MANIFEST_DIR` so it works in any worktree.

use reify_core::Severity;

/// `examples/trajectory/zv_shaped_ramp.ri` must parse and compile under
/// the stdlib prelude with zero Error-severity diagnostics, expose a
/// `ZvShapedRamp` structure template, and reference both key constructs
/// (`ZVShaper`, `PiecewisePolynomialProfile`) in the source text.
///
/// The template-presence and source-text assertions distinguish this test from
/// `examples_smoke.rs::all_examples_parse_and_compile_with_stdlib`, which only
/// checks compile-clean across all examples without inspecting the resulting
/// template set or source content.
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

    // ── Template presence ──────────────────────────────────────────────────────
    //
    // The compiled module must expose a `ZvShapedRamp` structure template.
    // This assertion distinguishes the test from the bulk examples_smoke gate,
    // which checks compile-clean but does not inspect the resulting template set.

    assert!(
        module.templates.iter().any(|t| t.name == "ZvShapedRamp"),
        "expected a 'ZvShapedRamp' structure template in compiled zv_shaped_ramp.ri; \
         found templates: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    // ── Positive source-text leaf-signal pins ──────────────────────────────────
    //
    // Both key constructs must appear in the source so a reader can discover them.
    // Guards against the wrong file being resolved via CARGO_MANIFEST_DIR.

    assert!(
        src.contains("ZVShaper"),
        "zv_shaped_ramp.ri must reference ZVShaper"
    );
    assert!(
        src.contains("PiecewisePolynomialProfile"),
        "zv_shaped_ramp.ri must reference PiecewisePolynomialProfile"
    );
}
