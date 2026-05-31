//! Regression test for `examples/anisotropic_bar.ri` (task ζ — 3782).
//!
//! Pins four leaf signals from
//! `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` task ζ:
//!
//!   1. The file parses with zero errors.
//!   2. It compiles under the stdlib prelude with zero Error-severity diagnostics.
//!   3. The compiled module exposes an `AnisotropicBar` structure template.
//!   4. The compiled `AnisotropicBar` template carries both `iso_peak_von_mises`
//!      and `ti_peak_von_mises` value cells (presence of two distinct named cells
//!      fed by two separate solves is the compile-level proxy for "two arms with
//!      different constitutive laws"; numeric values are Undef at compile altitude
//!      — see design decision in plan.json §design_decisions[1]).
//!
//! Path resolution uses `CARGO_MANIFEST_DIR` so it works in any worktree.

use reify_compiler::CompiledModule;
use reify_core::Severity;

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/anisotropic_bar.ri"
);

/// Read, parse (asserting zero parse errors), and compile (asserting zero
/// Error-severity diagnostics) `examples/anisotropic_bar.ri`.
///
/// Returns `(src, module)` so callers can inspect both the raw source and the
/// compiled result without repeating the parse/compile pipeline.
fn compile_example() -> (String, CompiledModule) {
    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/anisotropic_bar.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists",
    );

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

    (src, module)
}

/// `examples/anisotropic_bar.ri` must parse and compile under the stdlib
/// prelude with zero Error-severity diagnostics and expose an `AnisotropicBar`
/// structure template.
///
/// This test is the baseline compile-clean gate.  The transverse-isotropic
/// arm cell-presence pins are checked separately in
/// `anisotropic_bar_example_pins_ti_arm_cell_presence`.
#[test]
fn anisotropic_bar_example_compiles_under_stdlib_with_zero_errors() {
    let (_src, module) = compile_example();

    assert!(
        module.templates.iter().any(|t| t.name == "AnisotropicBar"),
        "expected an 'AnisotropicBar' structure template in compiled anisotropic_bar.ri; \
         found templates: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

/// `examples/anisotropic_bar.ri` must expose two named result cells —
/// `iso_peak_von_mises` and `ti_peak_von_mises` — in the compiled
/// `AnisotropicBar` template.
///
/// This pins **cell presence**, not value distinctness: at compile altitude
/// both cells are Undef (the FEA solver runs only at runtime via the
/// `@optimized("solver::elastic_static")` trampoline), so asserting
/// `iso_peak_von_mises != ti_peak_von_mises` is not possible here.  Presence
/// of two distinct named cells fed by two separate `solve_elastic_static` calls
/// with different constitutive laws is the strongest faithful compile-level
/// proxy for "two arms with different constitutive laws".  Runtime numeric
/// distinctness is proven by dependency ε/3781's reify-eval integration tests;
/// see design decision in plan.json §design_decisions[1].
#[test]
fn anisotropic_bar_example_pins_ti_arm_cell_presence() {
    let (_src, module) = compile_example();

    // ── Value-cell presence pins ───────────────────────────────────────────────
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

    for cell_name in &["iso_peak_von_mises", "ti_peak_von_mises"] {
        assert!(
            anisotropic_bar
                .value_cells
                .iter()
                .any(|c| c.id.member == *cell_name),
            "expected AnisotropicBar to carry a '{}' value cell (from the {} arm); \
             found cells: {:?}",
            cell_name,
            if cell_name.starts_with("iso") {
                "isotropic"
            } else {
                "transverse-isotropic"
            },
            anisotropic_bar
                .value_cells
                .iter()
                .map(|c| &c.id.member)
                .collect::<Vec<_>>()
        );
    }
}
