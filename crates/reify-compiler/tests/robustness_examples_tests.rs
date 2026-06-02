//! Regression tests for the input-shaping robustness examples
//! (task ζ — 3867): `examples/trajectory/zvd_robustness.ri` and
//! `examples/trajectory/ei_robustness.ri`.
//!
//! Mirrors `zv_shaped_ramp_example_tests.rs` (task ε). For each example the
//! test pins four leaf signals:
//!
//!   1. The file parses with zero errors (prelude-aware `parse_with_stdlib`,
//!      so stdlib enum variants like `SplineKind.CubicSpline` disambiguate as
//!      `EnumAccess`).
//!   2. It compiles under the stdlib prelude with zero Error-severity
//!      diagnostics.
//!   3. The compiled module exposes its top-level structure template
//!      (`ZvdRobustness` / `EiRobustness`) — distinguishes this test from the
//!      bulk examples_smoke gate, which only checks compile-clean without
//!      inspecting the resulting template set.
//!   4. Positive source-text pins: the shaper construct + `input_shape` must
//!      appear in the source (`ZVDShaper` for the first, `EIShaper` for the
//!      second), guarding against the wrong file being resolved and ensuring
//!      the ζ `input_shape` call is actually exercised.
//!
//! The examples are construction + `input_shape` compile-smoke fixtures: the
//! ±10 % / ±15 % robustness property is verified at the Rust eval layer (the
//! impulse-train residual band sweep in
//! `reify-eval/src/trajectory_ops.rs::worst_case_residual_fraction`, steps 7–8),
//! NOT via `simulate_trajectory` — that signature is not yet in the stdlib and
//! calling it would break the examples_smoke gate (design decision D3). The
//! `.ri` files carry NO numeric robustness assertion.
//!
//! Path resolution uses `CARGO_MANIFEST_DIR` so it works in any worktree.

use reify_core::Severity;

/// Parse + compile a robustness example under the stdlib prelude and assert the
/// four leaf signals: zero parse errors, zero Error-severity compile
/// diagnostics, the expected top-level structure template is present, and every
/// `required_construct` appears in the source text.
fn assert_robustness_example_compiles(
    path: &str,
    module_name: &str,
    template_name: &str,
    required_constructs: &[&str],
) {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "failed to read {path}: {e} — check CARGO_MANIFEST_DIR resolution \
             and that the example file exists"
        )
    });

    // ── Parse (prelude-aware so SplineKind.* disambiguates as EnumAccess) ────────
    let parsed =
        reify_compiler::parse_with_stdlib(&src, reify_core::ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {path}: {:#?}",
        parsed.errors
    );

    // ── Compile (zero Error-severity diagnostics under the stdlib) ───────────────
    let module = reify_compiler::compile_with_stdlib(&parsed);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling {path} under stdlib, got:\n{:#?}",
        errors
    );

    // ── Template presence (distinguishes this from the bulk smoke gate) ──────────
    assert!(
        module.templates.iter().any(|t| t.name == template_name),
        "expected a '{template_name}' structure template in compiled {path}; \
         found templates: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    // ── Positive source-text leaf-signal pins ───────────────────────────────────
    for construct in required_constructs {
        assert!(
            src.contains(construct),
            "{path} must reference {construct}"
        );
    }
}

/// `examples/trajectory/zvd_robustness.ri` must parse and compile under the
/// stdlib prelude with zero Error diagnostics, expose a `ZvdRobustness`
/// structure template, and reference both `ZVDShaper` and `input_shape`.
#[test]
fn zvd_robustness_example_compiles_under_stdlib_with_zero_errors() {
    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/trajectory/zvd_robustness.ri"
    );
    assert_robustness_example_compiles(
        EXAMPLE_PATH,
        "zvd_robustness",
        "ZvdRobustness",
        &["ZVDShaper", "input_shape"],
    );
}

/// `examples/trajectory/ei_robustness.ri` must parse and compile under the
/// stdlib prelude with zero Error diagnostics, expose an `EiRobustness`
/// structure template, and reference both `EIShaper` and `input_shape`.
#[test]
fn ei_robustness_example_compiles_under_stdlib_with_zero_errors() {
    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/trajectory/ei_robustness.ri"
    );
    assert_robustness_example_compiles(
        EXAMPLE_PATH,
        "ei_robustness",
        "EiRobustness",
        &["EIShaper", "input_shape"],
    );
}
