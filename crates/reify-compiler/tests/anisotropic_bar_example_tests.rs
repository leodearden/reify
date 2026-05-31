//! Regression test for `examples/anisotropic_bar.ri` (task ζ — 3782).
//!
//! Pins four leaf signals from
//! `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` task ζ:
//!
//!   1. The file parses with zero errors.
//!   2. It compiles under the stdlib prelude with zero Error-severity diagnostics.
//!   3. The compiled module exposes an `AnisotropicBar` structure template.
//!   4. Source-text pins: `TransverseIsotropicMaterial`, `MaterialFrame`, and
//!      `e_axial` (weak-build-axis marker) appear in the source; exactly two
//!      `solve_elastic_static(` call sites are present; and the compiled
//!      `AnisotropicBar` template carries both `iso_tip_deflection` and
//!      `ti_tip_deflection` value cells (compile-level proxy for the two distinct
//!      tip deflections — numeric values are Undef at compile altitude; see design
//!      decision in plan.json).
//!
//! Path resolution uses `CARGO_MANIFEST_DIR` so it works in any worktree.

use reify_core::Severity;

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/anisotropic_bar.ri"
);

/// `examples/anisotropic_bar.ri` must parse and compile under the stdlib
/// prelude with zero Error-severity diagnostics and expose an `AnisotropicBar`
/// structure template.
///
/// This test is the baseline compile-clean gate.  The transverse-isotropic
/// leaf-signal pins are checked separately in
/// `anisotropic_bar_example_pins_transverse_isotropic_leaf_signals`.
#[test]
fn anisotropic_bar_example_compiles_under_stdlib_with_zero_errors() {
    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/anisotropic_bar.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists",
    );

    // ── Parse ──────────────────────────────────────────────────────────────────
    // Use the prelude-aware parser so stdlib enum names are injected before
    // parsing — mirrors examples_smoke.rs::smoke_one.
    let parsed = reify_compiler::parse_with_stdlib(
        &src,
        reify_core::ModulePath::single("anisotropic_bar"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors in examples/anisotropic_bar.ri: {:#?}",
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
        "expected zero Error diagnostics compiling examples/anisotropic_bar.ri \
         under stdlib, got:\n{:#?}",
        errors
    );

    // ── Template presence ──────────────────────────────────────────────────────
    assert!(
        module.templates.iter().any(|t| t.name == "AnisotropicBar"),
        "expected an 'AnisotropicBar' structure template in compiled anisotropic_bar.ri; \
         found templates: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

/// `examples/anisotropic_bar.ri` must contain the transverse-isotropic arm
/// leaf signals: `TransverseIsotropicMaterial`, `MaterialFrame`, a weak
/// `e_axial`, exactly two `solve_elastic_static(` call sites, and the compiled
/// `AnisotropicBar` template must carry both `iso_tip_deflection` and
/// `ti_tip_deflection` value cells (compile-level proxy for "two distinct tip
/// deflections" — see design decision in plan.json §design_decisions[1]).
#[test]
fn anisotropic_bar_example_pins_transverse_isotropic_leaf_signals() {
    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/anisotropic_bar.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists",
    );

    // ── Source-text pins ───────────────────────────────────────────────────────
    // (a) TransverseIsotropicMaterial, MaterialFrame, and e_axial must appear.
    assert!(
        src.contains("TransverseIsotropicMaterial"),
        "anisotropic_bar.ri must reference TransverseIsotropicMaterial"
    );
    assert!(
        src.contains("MaterialFrame"),
        "anisotropic_bar.ri must reference MaterialFrame"
    );
    assert!(
        src.contains("e_axial"),
        "anisotropic_bar.ri must reference e_axial (weak-build-axis param)"
    );

    // (b) Exactly two solve_elastic_static( call sites (iso arm + TI arm).
    let solve_count = src.matches("solve_elastic_static(").count();
    assert_eq!(
        solve_count,
        2,
        "anisotropic_bar.ri must contain exactly 2 'solve_elastic_static(' call sites \
         (one isotropic, one transverse-isotropic); found {}",
        solve_count
    );

    // ── Compile ────────────────────────────────────────────────────────────────
    let parsed = reify_compiler::parse_with_stdlib(
        &src,
        reify_core::ModulePath::single("anisotropic_bar"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors in examples/anisotropic_bar.ri: {:#?}",
        parsed.errors
    );

    let module = reify_compiler::compile_with_stdlib(&parsed);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics: {:#?}",
        errors
    );

    // ── Value-cell pins ────────────────────────────────────────────────────────
    // (c) Both tip-deflection value cells must compile.  Presence of two distinct
    //     named cells fed by two separate solves with different constitutive laws
    //     is the strongest faithful compile-level proxy for "two distinct tip
    //     deflections" (numeric Undef at compile altitude — solver runs only at
    //     runtime via the @optimized trampoline).
    let anisotropic_bar = module
        .templates
        .iter()
        .find(|t| t.name == "AnisotropicBar")
        .unwrap_or_else(|| {
            panic!(
                "AnisotropicBar template not found; got: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });

    for cell_name in &["iso_tip_deflection", "ti_tip_deflection"] {
        assert!(
            anisotropic_bar
                .value_cells
                .iter()
                .any(|c| c.id.member == *cell_name),
            "leaf signal 'two distinct tip deflections': expected AnisotropicBar to carry \
             a '{}' value cell; found cells: {:?}",
            cell_name,
            anisotropic_bar
                .value_cells
                .iter()
                .map(|c| &c.id.member)
                .collect::<Vec<_>>()
        );
    }
}
