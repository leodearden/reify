//! Eval e2e test: innermost-wins ambient-default injection for a structure
//! defined inside a `purpose` body (task #4639, step-7/step-8).
//!
//! Pins ambient-default-material PRD §9 row 4 (positive direction):
//! a `structure def` nested inside a purpose body resolves the PURPOSE-level
//! `default Material = aluminum` over the FILE-level `default Material = steel`
//! (innermost-wins, DD6).
//!
//! # Design
//!
//! Kernel-INDEPENDENT: uses `MockGeometryKernel` and asserts on
//! `material.density` (a scalar field access on the injected `Material`
//! StructureInstance, no geometry op required), NOT on `mass` (which needs
//! OCCT for `volume(geometry)`). Mirrors the pattern of
//! `row_7_no_ambient_no_material_errors_no_density`.
//!
//! # RED / GREEN cadence
//!
//! RED (step-7): step-6 threads `None` (file scope) for purpose-nested
//! structures → the file-level STEEL default is injected → `rho` evaluates to
//! 7850.0 kg/m³ → the assert_approx_eq!(rho, 2700.0) assertion FAILS.
//!
//! GREEN (step-8): the Purpose arm is changed to pass `Some(p.name)` →
//! innermost-wins resolver picks the purpose-level ALUMINUM default →
//! `rho` evaluates to 2700.0 kg/m³ → assertion passes.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, parse_and_compile_with_stdlib};

/// A fully-valid `Material(...)` constructor for steel (7850 kg/m³).
const STEEL_CTOR: &str =
    r#"Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)"#;

/// A fully-valid `Material(...)` constructor for aluminum (2700 kg/m³).
const ALUMINUM_CTOR: &str =
    r#"Material(name: "aluminum", density: 2700kg/m^3, youngs_modulus: 69GPa)"#;

/// Source with:
/// - file-level `default Material = steel` (outer fallback)
/// - `purpose Exploration` with purpose-level `default Material = aluminum`
///   (innermost — must win)
/// - `structure def InPurpose : Physical` with `let rho = material.density`
///   (density accessed via the injected Material, no geometry query needed)
const SRC: &str = r#"
default Material = STEEL_CTOR_PLACEHOLDER

purpose Exploration() {
    default Material = ALUMINUM_CTOR_PLACEHOLDER

    structure def InPurpose : Physical {
        param geometry : Solid = box(20mm, 20mm, 20mm)
        let rho = material.density
    }
}
"#;

fn src() -> String {
    SRC.replace("STEEL_CTOR_PLACEHOLDER", STEEL_CTOR)
        .replace("ALUMINUM_CTOR_PLACEHOLDER", ALUMINUM_CTOR)
}

/// §9 row 4 (positive eval): purpose-nested structure resolves purpose-level
/// aluminum default over file-level steel default (innermost-wins).
///
/// Asserts that `InPurpose.rho` (= `material.density`) evaluates to
/// 2700.0 kg/m³ (aluminum SI value), NOT 7850.0 kg/m³ (steel SI value).
///
/// Kernel-INDEPENDENT: `rho = material.density` is a scalar field access
/// on the injected Material StructureInstance; no geometry op, no OCCT
/// required.
///
/// RED (step-7): step-6 injects file-scope steel → rho == 7850 → FAILS.
/// GREEN (step-8): Purpose arm passes `Some("Exploration")` → purpose-scope
/// aluminum injected → rho == 2700 → PASSES.
#[test]
fn purpose_nested_structure_resolves_purpose_level_aluminum() {
    let compiled = parse_and_compile_with_stdlib(&src());

    // Build with MockGeometryKernel — kernel-independent (density is a
    // scalar field lookup on the injected Material, no geometry query).
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Retrieve InPurpose.rho — must be present (step-6 already ensures the
    // template is compiled and material is injected).
    let rho_id = ValueCellId::new("InPurpose", "rho");
    let rho_val = result
        .values
        .get(&rho_id)
        .unwrap_or_else(|| {
            panic!(
                "ValueCellId {:?} not found in eval result; \
                 templates in compiled module: {:?}; \
                 eval diagnostics: {:?}",
                rho_id,
                compiled.templates.iter().map(|t| &t.name).collect::<Vec<_>>(),
                result.diagnostics
            )
        });

    // Must be a density-dimensioned scalar.
    let si_value = match rho_val {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::MASS_DENSITY,
                "InPurpose.rho must be MASS_DENSITY-dimensioned \
                 (material.density is kg/m³); got dimension {:?}",
                dimension
            );
            *si_value
        }
        other => panic!(
            "InPurpose.rho must be a Value::Scalar; got: {:?}",
            other
        ),
    };

    // Innermost-wins: purpose-level aluminum (2700 kg/m³) must win over
    // file-level steel (7850 kg/m³).
    let aluminum_density_si = 2700.0_f64; // kg/m³
    let tol = 1e-9_f64;
    assert!(
        (si_value - aluminum_density_si).abs() < tol,
        "InPurpose.rho must equal aluminum density ({} kg/m³, innermost-wins) \
         but got {} kg/m³ (delta {:.3e}); \
         if this is 7850.0 the Purpose arm is still passing `None` (file scope) \
         instead of `Some(\"Exploration\")` — step-8 is not yet implemented",
        aluminum_density_si,
        si_value,
        (si_value - aluminum_density_si).abs()
    );
}
