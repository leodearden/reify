//! Compiler integration tests for structure definitions nested inside a
//! `purpose` body (task #4639, step-5).
//!
//! Tests that a `structure def` declared directly inside a `purpose` body is
//! compiled as a first-class entity template, and that it conforms to its
//! required traits via ambient-default injection.
//!
//! These tests are intentionally NON-discriminating on *which* material value
//! is injected (file-level steel vs purpose-level aluminum); the discriminating
//! eval test (step-7) pins the aluminum density value.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::parse_and_compile_with_stdlib;

/// A fully-valid `Material(...)` constructor for steel.
const STEEL_CTOR: &str =
    r#"Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)"#;

/// A fully-valid `Material(...)` constructor for aluminum.
const ALUMINUM_CTOR: &str =
    r#"Material(name: "aluminum", density: 2700kg/m^3, youngs_modulus: 69GPa)"#;

/// Source with:
/// - file-level `default Material = steel` (fallback)
/// - `purpose Exploration` with purpose-level `default Material = aluminum` (innermost)
/// - `structure def InPurpose : Physical` with a `let rho` accessing material density
///
/// After step-6 this structure is compiled (template produced, conformance via any
/// ambient Material injection). Step-7 then pins that the PURPOSE-level aluminum
/// wins (rho == 2700).
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

/// A structure defined lexically inside a `purpose` body must be compiled as a
/// first-class entity template (named `InPurpose`), and it must conform to
/// `Physical` (no `MissingRequiredMember` errors) because an ambient Material
/// default is available in scope.
///
/// RED (step-5): the `phase_entities` `Declaration::Purpose` arm ignores
/// `p.structures`, so no `InPurpose` template is produced → assertion on
/// templates inclusion fails.
///
/// GREEN (step-6): purpose-nested structures are compiled with file-scope
/// injection (interim `None`); the `InPurpose` template is produced and
/// `material` is injected from the file-level steel default.
#[test]
fn purpose_nested_structure_compiles_and_conforms() {
    let compiled = parse_and_compile_with_stdlib(&src());

    // (i) The InPurpose template must appear in the compiled module.
    let has_in_purpose = compiled.templates.iter().any(|t| t.name == "InPurpose");
    assert!(
        has_in_purpose,
        "expected an `InPurpose` template in the compiled module; \
         got templates: {:?}; diagnostics: {:?}",
        compiled.templates.iter().map(|t| &t.name).collect::<Vec<_>>(),
        compiled.diagnostics
    );

    // (ii) There must be zero MissingRequiredMember errors — conformance to
    // Physical requires a Material to be injected.
    let missing_member_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::MissingRequiredMember)
        })
        .collect();

    assert!(
        missing_member_errors.is_empty(),
        "purpose-nested `InPurpose : Physical` must have zero MissingRequiredMember \
         errors (ambient Material injection must fire); \
         got: {:?}",
        missing_member_errors
    );
}
