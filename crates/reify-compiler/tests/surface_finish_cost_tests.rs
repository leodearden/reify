//! Compile-surface + cell-type pins for `examples/surface_finish_cost.ri` (task 4890 γ).
//!
//! Mirrors `cost_aggregation_tests.rs` (step-9):
//!   - CARGO_MANIFEST_DIR-anchored read of the example
//!   - parse zero errors
//!   - compile_with_stdlib zero Error diagnostics
//!   - cell-type pins on compiled templates:
//!       AssemblyBOM.total_finishing_cost → Type::Scalar<MONEY>
//!       CoatedPlate.coat_cost            → Type::Scalar<MONEY>
//!       CoatedPlate.coat_mass            → Type::Scalar<MASS>
//!
//! Compile-only (no eval, no kernel) — runs everywhere including OCCT-stub
//! builds.  The realized area VALUES (coat_cost/coat_mass) are locked
//! separately by crates/reify-cli/tests/cli_surface_finish_cost.rs (B7 gate).
//!
//! File-stem `surface_finish_cost` matches the
//! `cargo test -p reify-compiler -- surface_finish_cost` filter used in this
//! task's testStrategy.  Every test function name contains `surface_finish_cost`
//! so that filter picks them up.
//!
//! PRD: docs/prds/v0_6/surface-finish-functional.md task γ, boundaries B6+B7.

#[allow(dead_code)]
mod common;

use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
use reify_core::{DimensionVector, ModulePath, Severity, Type};

// ─── Helper: load + compile the example ──────────────────────────────────────

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/surface_finish_cost.ri"
);

fn load_surface_finish_cost_example() -> reify_compiler::CompiledModule {
    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/surface_finish_cost.ri — check CARGO_MANIFEST_DIR resolution",
    );

    // Use parse_with_stdlib (prelude-aware) so that stdlib enum variants like
    // CoatingProcess.Anodize / FinishProcess.Polished / TreatmentProcess.Temper
    // resolve as EnumAccess nodes at parse time.  Plain reify_syntax::parse
    // cannot resolve these Type.Variant references against the stdlib prelude.
    // Mirrors the pattern in examples_smoke.rs::smoke_one and
    // anisotropic_bar_example_tests.rs.
    let parsed = parse_with_stdlib(&src, ModulePath::single("surface_finish_cost"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in surface_finish_cost.ri: {:?}",
        parsed.errors
    );

    let module = compile_with_stdlib(&parsed);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling surface_finish_cost.ri under stdlib, got:\n{:#?}",
        errors
    );

    module
}

// ─── Cell-type pins ───────────────────────────────────────────────────────────

/// `examples/surface_finish_cost.ri` must parse and compile under the stdlib
/// prelude with zero Error diagnostics, and expose:
///
///   - `AssemblyBOM` template with `total_finishing_cost : Scalar<MONEY>`
///   - `CoatedPlate` template with `coat_cost : Scalar<MONEY>`
///   - `CoatedPlate` template with `coat_mass : Scalar<MASS>`
///
/// Mirrors `cost_aggregation_example_compiles_under_stdlib_with_zero_errors`
/// (cost_aggregation_tests.rs:218-283), extended to cover the area-based
/// CoatedPlate cells.
#[test]
fn surface_finish_cost_example_compiles_under_stdlib_with_correct_cell_types() {
    let module = load_surface_finish_cost_example();

    // ── AssemblyBOM.total_finishing_cost : Scalar<MONEY> ─────────────────────

    let assembly = module
        .templates
        .iter()
        .find(|t| t.name == "AssemblyBOM")
        .unwrap_or_else(|| {
            panic!(
                "AssemblyBOM template should be present in compiled surface_finish_cost.ri; \
                 found templates: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });

    let total_finishing_cost_cell = assembly
        .value_cells
        .iter()
        .find(|c| c.id.member == "total_finishing_cost")
        .unwrap_or_else(|| {
            panic!(
                "AssemblyBOM should carry a 'total_finishing_cost' value cell; found cells: {:?}",
                assembly
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        total_finishing_cost_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "AssemblyBOM.total_finishing_cost should have type Scalar<MONEY>, got {:?}",
        total_finishing_cost_cell.cell_type
    );

    // ── CoatedPlate.coat_cost : Scalar<MONEY> ────────────────────────────────

    let coated_plate = module
        .templates
        .iter()
        .find(|t| t.name == "CoatedPlate")
        .unwrap_or_else(|| {
            panic!(
                "CoatedPlate template should be present in compiled surface_finish_cost.ri; \
                 found templates: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });

    let coat_cost_cell = coated_plate
        .value_cells
        .iter()
        .find(|c| c.id.member == "coat_cost")
        .unwrap_or_else(|| {
            panic!(
                "CoatedPlate should carry a 'coat_cost' value cell; found cells: {:?}",
                coated_plate
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        coat_cost_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "CoatedPlate.coat_cost should have type Scalar<MONEY>, got {:?}",
        coat_cost_cell.cell_type
    );

    // ── CoatedPlate.coat_mass : Scalar<MASS> ─────────────────────────────────

    let coat_mass_cell = coated_plate
        .value_cells
        .iter()
        .find(|c| c.id.member == "coat_mass")
        .unwrap_or_else(|| {
            panic!(
                "CoatedPlate should carry a 'coat_mass' value cell; found cells: {:?}",
                coated_plate
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        coat_mass_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::MASS
        },
        "CoatedPlate.coat_mass should have type Scalar<MASS>, got {:?}",
        coat_mass_cell.cell_type
    );
}
