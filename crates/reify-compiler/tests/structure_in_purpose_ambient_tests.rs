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
use reify_test_support::{compile_source_with_stdlib, parse_and_compile_with_stdlib};

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

// ── Negative test: no ambient default → injection must NOT fire ───────────────

/// When no `default Material` is declared at any scope (neither file-level nor
/// purpose-level), ambient injection does not fire, and `InPurpose : Physical`
/// must produce at least one `MissingRequiredMember` error for the missing
/// `material` param.
///
/// This guards against a regression where injection fires unconditionally — if
/// injection were unconditional, this test would pass with zero errors and the
/// positive tests would no longer discriminate.
#[test]
fn purpose_nested_structure_without_ambient_emits_missing_required_member() {
    // Source with a purpose-nested Physical structure but NO `default Material`
    // at any scope (no file-level default, no purpose-level default).
    const NO_AMBIENT_SRC: &str = r#"
purpose Exploration() {
    structure def InPurpose : Physical {
        param geometry : Solid = box(20mm, 20mm, 20mm)
    }
}
"#;

    // Use compile_source_with_stdlib (non-panicking on errors) because this
    // test intentionally expects Error-severity diagnostics.
    let compiled = compile_source_with_stdlib(NO_AMBIENT_SRC);

    // At least one MissingRequiredMember error expected: the required
    // `param material : Material` is absent because no ambient default is in scope.
    let missing_member_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::MissingRequiredMember)
        })
        .collect();

    assert!(
        !missing_member_errors.is_empty(),
        "expected at least one MissingRequiredMember error when no ambient Material \
         is in scope; injection must be conditional; \
         got zero such errors (all diagnostics: {:?})",
        compiled.diagnostics
    );
}

// ── Duplicate-name tests: nested structure shares entity namespace ─────────────

/// A purpose-nested structure that has the same name as a top-level structure
/// must produce exactly one `duplicate entity definition` diagnostic (via
/// `record_or_report_duplicate`), and only the first definition must appear in
/// `templates`.
///
/// This verifies that the Purpose arm's `record_or_report_duplicate` call (in
/// `pre_pass::collect_decl_refs`) correctly detects cross-kind collisions, not
/// just within-purpose ones.
#[test]
fn purpose_nested_structure_name_collides_with_top_level_structure() {
    const SRC: &str = r#"
structure InPurpose {
    param width : Length = 10mm
}

purpose Exploration() {
    structure def InPurpose {
        param height : Length = 20mm
    }
}
"#;

    // compile_source_with_stdlib: non-panicking — we expect a duplicate error.
    let compiled = compile_source_with_stdlib(SRC);

    let dup_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.contains("duplicate entity definition")
                && d.message.contains("InPurpose")
        })
        .collect();

    assert_eq!(
        dup_errors.len(),
        1,
        "expected exactly one 'duplicate entity definition' error when a top-level \
         structure and a purpose-nested structure share the name 'InPurpose'; \
         got: {:?}",
        dup_errors
    );

    // Only the first (top-level) InPurpose must be compiled.
    let in_purpose_count = compiled.templates.iter().filter(|t| t.name == "InPurpose").count();
    assert_eq!(
        in_purpose_count,
        1,
        "only the first InPurpose definition should appear in templates; \
         got {} templates named InPurpose",
        in_purpose_count
    );
}

/// Two purposes each defining a structure with the same name must produce
/// exactly one `duplicate entity definition` diagnostic; the second definition
/// is skipped.
///
/// This covers the cross-purpose collision path (both nested structures go
/// through the same flat `record_or_report_duplicate` namespace check).
#[test]
fn purpose_nested_structure_name_collides_across_purposes() {
    const SRC: &str = r#"
purpose Alpha() {
    structure def Widget {
        param width : Length = 10mm
    }
}

purpose Beta() {
    structure def Widget {
        param height : Length = 20mm
    }
}
"#;

    // compile_source_with_stdlib: non-panicking — we expect a duplicate error.
    let compiled = compile_source_with_stdlib(SRC);

    let dup_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.contains("duplicate entity definition")
                && d.message.contains("Widget")
        })
        .collect();

    assert_eq!(
        dup_errors.len(),
        1,
        "expected exactly one 'duplicate entity definition' error when two purposes \
         each define a structure named 'Widget'; got: {:?}",
        dup_errors
    );

    // Only one Widget template (the first definition wins).
    let widget_count = compiled.templates.iter().filter(|t| t.name == "Widget").count();
    assert_eq!(
        widget_count,
        1,
        "only the first Widget definition should appear in templates; \
         got {} templates named Widget",
        widget_count
    );
}
