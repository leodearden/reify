//! Tests for PRD §6 parameter-surface additions to stdlib/materials_mechanical.ri.
//!
//! Split by mechanism:
//! - `compile_source_with_stdlib` → conformance (no Severity::Error) for
//!   TemperatureDependent (§6.1) and optional-param omissions (§6.2).
//! - `check_source_with_stdlib`   → constraint Satisfied/Violated for the
//!   Elastic poissons_ratio constraint (§6.2).

use reify_compiler::{CompiledModule, DefaultKind};
use reify_core::{DiagnosticCode, DimensionVector, ModulePath, Severity, Type};
use reify_eval::CheckResult;
use reify_ir::Satisfaction;
use reify_test_support::{check_source_with_stdlib, compile_source_with_stdlib};
use std::path::PathBuf;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Load and compile `stdlib/materials_mechanical.ri` without prelude context.
///
/// `Temperature` is a named dimension (resolved by the compiler regardless of
/// prelude), and the `K` unit literal has a hardcoded bootstrap entry in
/// `units.rs` so `293.15K` resolves without a seeded unit registry. Used
/// for trait-shape introspection tests that need the raw `CompiledTrait`
/// structure (required_members / defaults).
fn load_stdlib_module() -> CompiledModule {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "stdlib",
        "materials_mechanical.ri",
    ]
    .iter()
    .collect();
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    let parsed = reify_syntax::parse(&source, ModulePath::single("stdlib"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in materials_mechanical.ri: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

// ── §6.1 TemperatureDependent — conformance (compile-time) ───────────────────

/// Conforming structure that omits reference_temperature.
/// The trait provides a default (293.15K) so the param is optional.
/// Expects: compilation with no Severity::Error diagnostics.
///
/// RED: TemperatureDependent does not yet exist → unresolved-trait error.
#[test]
fn temperature_dependent_omits_reference_temperature_is_clean() {
    let src = r#"
        structure def RoomTemp : TemperatureDependent {
            let marker = 1
        }
    "#;
    let compiled = compile_source_with_stdlib(src);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics when omitting reference_temperature \
         (should default to 293.15K), got: {:?}",
        errors
    );
}

/// Conforming structure that explicitly supplies reference_temperature = 350.0K.
/// Expects: compilation with no Severity::Error diagnostics.
///
/// RED: TemperatureDependent does not yet exist → unresolved-trait error.
#[test]
fn temperature_dependent_supplies_350k_is_clean() {
    let src = r#"
        structure def HotEnv : TemperatureDependent {
            param reference_temperature : Temperature = 350.0K
        }
    "#;
    let compiled = compile_source_with_stdlib(src);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics when supplying reference_temperature = 350.0K, \
         got: {:?}",
        errors
    );
}

/// Pin the trait shape of TemperatureDependent: `reference_temperature` must
/// live in `defaults` (not `required_members`) with type `Temperature`.
///
/// This catches regressions that either:
/// (a) accidentally promote the param to required (removing the default), or
/// (b) change the dimension type (e.g. back to `Real`).
/// Conformance-only tests cannot catch (b) because they only check that no
/// Error diagnostics appear — they don't inspect the compiled trait structure.
#[test]
fn temperature_dependent_has_reference_temperature_default_with_temperature_type() {
    let module = load_stdlib_module();

    let td = module
        .trait_defs
        .iter()
        .find(|t| t.name == "TemperatureDependent")
        .expect("expected 'TemperatureDependent' trait in compiled module");

    // Must NOT appear in required_members — the param is optional.
    assert!(
        !td.required_members
            .iter()
            .any(|r| r.name == "reference_temperature"),
        "reference_temperature should not be a required member (it has a default), \
         got required_members: {:?}",
        td.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    // Must appear in defaults with Temperature type.
    let default_entry = td
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("reference_temperature"))
        .expect(
            "expected 'reference_temperature' in TemperatureDependent.defaults \
             (param with default 293.15K must live here)",
        );

    match &default_entry.kind {
        DefaultKind::Param { cell_type, .. } => {
            assert_eq!(
                *cell_type,
                Type::Scalar {
                    dimension: DimensionVector::TEMPERATURE
                },
                "reference_temperature should have the Temperature dimension type \
                 (Type::Scalar {{ dimension: DimensionVector::TEMPERATURE }}), got {:?}",
                cell_type
            );
        }
        other => panic!(
            "expected DefaultKind::Param for reference_temperature, got {:?}",
            other
        ),
    }
}

// ── §6.2 Elastic poissons_ratio constraint — eval-time ───────────────────────

/// poissons_ratio = 0.7 violates the (0, 0.5) physical bound.
/// All three Elastic params supplied to isolate the constraint variable.
///
/// RED: Elastic has no poissons_ratio constraint yet → no Violated entry.
#[test]
fn elastic_poissons_ratio_high_is_violated() {
    let src = r#"
        structure def StiffMat : Elastic {
            param youngs_modulus : Real = 200.0
            param poissons_ratio : Real = 0.7
            param shear_modulus  : Real = 77.0
        }
    "#;
    let result: CheckResult = check_source_with_stdlib(src);
    let has_violated = result
        .constraint_results
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Violated);
    assert!(
        has_violated,
        "expected at least one Violated constraint for poissons_ratio=0.7 (outside (0,0.5)), \
         got: {:?}",
        result.constraint_results
    );
    // Tightened: require the error to carry DiagnosticCode::ConstraintViolated so that
    // an unrelated error doesn't accidentally satisfy this check.
    let constraint_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::ConstraintViolated)
        })
        .collect();
    assert!(
        !constraint_errors.is_empty(),
        "expected at least one Severity::Error with code ConstraintViolated for \
         poissons_ratio=0.7, got diagnostics: {:?}",
        result.diagnostics
    );
}

/// poissons_ratio = -0.1 violates the lower bound (must be > 0, auxetic excluded).
/// All three Elastic params supplied.
///
/// RED: Elastic has no poissons_ratio constraint yet → no Violated entry.
#[test]
fn elastic_poissons_ratio_negative_is_violated() {
    let src = r#"
        structure def AuxeticMat : Elastic {
            param youngs_modulus : Real = 100.0
            param poissons_ratio : Real = -0.1
            param shear_modulus  : Real = 40.0
        }
    "#;
    let result: CheckResult = check_source_with_stdlib(src);
    let has_violated = result
        .constraint_results
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Violated);
    assert!(
        has_violated,
        "expected at least one Violated constraint for poissons_ratio=-0.1 (below 0), \
         got: {:?}",
        result.constraint_results
    );
    // Tightened: require the error to carry DiagnosticCode::ConstraintViolated so that
    // an unrelated error doesn't accidentally satisfy this check.
    let constraint_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::ConstraintViolated)
        })
        .collect();
    assert!(
        !constraint_errors.is_empty(),
        "expected at least one Severity::Error with code ConstraintViolated for \
         poissons_ratio=-0.1, got diagnostics: {:?}",
        result.diagnostics
    );
}

/// poissons_ratio = 0.3 is inside (0, 0.5) — constraint should be Satisfied.
/// Expects: no Violated entry and no Severity::Error diagnostics.
#[test]
fn elastic_poissons_ratio_valid_is_clean() {
    let src = r#"
        structure def NormalMat : Elastic {
            param youngs_modulus : Real = 200.0
            param poissons_ratio : Real = 0.3
            param shear_modulus  : Real = 77.0
        }
    "#;
    let result: CheckResult = check_source_with_stdlib(src);
    let violated: Vec<_> = result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "expected no Violated constraint for poissons_ratio=0.3 (inside (0,0.5)), \
         got: {:?}",
        violated
    );
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics for valid poissons_ratio=0.3, got: {:?}",
        errors
    );
}

// ── §6.2 Optional params — conformance (compile-time) ───────────────────────

/// Conforming structure that omits shear_modulus from Elastic.
/// Once shear_modulus gains `= undef`, this should compile cleanly.
/// poissons_ratio = 0.3 keeps the (0, 0.5) constraint satisfied.
///
/// RED: shear_modulus is still required (no default) → MissingRequiredMember error.
#[test]
fn elastic_omits_shear_modulus_is_clean() {
    let src = r#"
        structure def ElasticNoShear : Elastic {
            param youngs_modulus : Real = 200.0
            param poissons_ratio : Real = 0.3
        }
    "#;
    let compiled = compile_source_with_stdlib(src);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics when omitting shear_modulus \
         (should default to undef), got: {:?}",
        errors
    );
}

/// Conforming structure that omits compressive_strength from Strong.
/// Once compressive_strength gains `= undef`, this should compile cleanly.
/// uts=400.0 >= yield_strength=250.0 keeps the uts>=yield_strength constraint satisfied.
///
/// RED: compressive_strength is still required (no default) → MissingRequiredMember error.
#[test]
fn strong_omits_compressive_strength_is_clean() {
    let src = r#"
        structure def StrongNoCompr : Strong {
            param yield_strength : Real = 250.0
            param uts            : Real = 400.0
        }
    "#;
    let compiled = compile_source_with_stdlib(src);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics when omitting compressive_strength \
         (should default to undef), got: {:?}",
        errors
    );
}

/// Conforming structure that omits reduction_of_area from Ductile.
/// Once reduction_of_area gains `= undef`, this should compile cleanly.
///
/// RED: reduction_of_area is still required (no default) → MissingRequiredMember error.
#[test]
fn ductile_omits_reduction_of_area_is_clean() {
    let src = r#"
        structure def DuctileNoROA : Ductile {
            param elongation : Real = 0.2
        }
    "#;
    let compiled = compile_source_with_stdlib(src);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics when omitting reduction_of_area \
         (should default to undef), got: {:?}",
        errors
    );
}
