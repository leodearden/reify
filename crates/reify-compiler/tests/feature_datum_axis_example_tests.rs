//! Compile-clean regression for `examples/geometric_relations/feature_datum_axis.ri`
//! (geometric-relations ε, task 4385, step-17 RED / step-18 GREEN).
//!
//! This is the OCCT-free half of the B8 worked example: it pins that the
//! feature→datum projection example **parses and compiles clean under the
//! stdlib prelude** and that `cyl.axis` types + lowers to an `a` value cell on
//! the `Cyl` structure. It mirrors `anisotropic_bar_example_tests.rs` /
//! `examples_smoke.rs` (the example file is also auto-discovered by
//! `examples_smoke`'s recursive walk, so it cannot rot).
//!
//! The **runtime** B8 signal — that `cyl.axis` over the realized
//! revolved-rectangle cylinder resolves to exactly one `Value::Axis` equal to
//! the revolution axis — is asserted end-to-end against a real OCCT kernel in
//! `crates/reify-eval/tests/feature_datum_tests.rs`
//! (`feature_datum_axis_example_resolves_to_single_revolution_axis`). It lives
//! in the eval crate because realizing the revolve needs an OCCT-backed engine,
//! and `reify-compiler` is intentionally NOT an OCCT-touching crate
//! (`scripts/occt-touching-crates.txt`); see esc-4385-134.
//!
//! RED until step-18 creates the `.ri` fixture (this test panics on the missing
//! file read).

use reify_compiler::CompiledModule;
use reify_core::Severity;

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/geometric_relations/feature_datum_axis.ri"
);

/// Read, parse (asserting zero parse errors), and compile under the stdlib
/// prelude (asserting zero Error-severity diagnostics) the B8 example.
fn compile_example() -> CompiledModule {
    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/geometric_relations/feature_datum_axis.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists (step-18)",
    );

    let parsed = reify_compiler::parse_with_stdlib(
        &src,
        reify_core::ModulePath::single("feature_datum_axis"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors in feature_datum_axis.ri: {:#?}",
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
        "expected zero Error diagnostics compiling feature_datum_axis.ri under stdlib, got:\n{:#?}",
        errors
    );

    module
}

/// The B8 example parses + compiles clean under the stdlib prelude and exposes
/// a `Cyl` structure template. Baseline compile-clean gate (the `cyl.axis`
/// feature→datum projection must type without any Error diagnostic).
#[test]
fn feature_datum_axis_example_compiles_under_stdlib_with_zero_errors() {
    let module = compile_example();

    assert!(
        module.templates.iter().any(|t| t.name == "Cyl"),
        "expected a 'Cyl' structure template in compiled feature_datum_axis.ri; \
         found templates: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

/// The compiled `Cyl` template must carry an `a` value cell — the
/// `let a : Axis = cyl.axis` binding. Cell presence is the compile-level proxy
/// that the feature→datum `.axis` projection typed (`Type::Geometry` receiver →
/// `Axis`) and lowered to a kernel-backed value cell rather than being dropped.
#[test]
fn feature_datum_axis_example_lowers_cyl_axis_to_value_cell() {
    let module = compile_example();

    let cyl = module
        .templates
        .iter()
        .find(|t| t.name == "Cyl")
        .unwrap_or_else(|| {
            panic!(
                "Cyl template not found; got: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });

    assert!(
        cyl.value_cells.iter().any(|c| c.id.member == "a"),
        "expected Cyl to carry an 'a' value cell (the `let a : Axis = cyl.axis` \
         feature→datum projection); found cells: {:?}",
        cyl.value_cells
            .iter()
            .map(|c| &c.id.member)
            .collect::<Vec<_>>()
    );
}
