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
/// ultimate_tensile_strength=400.0 >= yield_strength=250.0 keeps the constraint satisfied.
///
/// RED: compressive_strength is still required (no default) → MissingRequiredMember error.
#[test]
fn strong_omits_compressive_strength_is_clean() {
    let src = r#"
        structure def StrongNoCompr : Strong {
            param yield_strength            : Real = 250.0
            param ultimate_tensile_strength : Real = 400.0
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
            param elongation_at_break : Real = 0.2
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

// ── §6.2 Ductile — elongation_at_break rename (β task #4240) ─────────────────

/// Ductile conformer declaring only old name `elongation` (no elongation_at_break)
/// must produce a MissingRequiredMember error for `elongation_at_break`.
///
/// RED: stdlib Ductile still declares `elongation`, so supplying `elongation` succeeds
/// rather than emitting a MissingRequiredMember error for `elongation_at_break`.
#[test]
fn ductile_old_elongation_name_is_missing_required_member() {
    let src = r#"
        structure def DuctileOldName : Ductile {
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
        !errors.is_empty(),
        "expected MissingRequiredMember error for 'elongation_at_break' when \
         only old 'elongation' is supplied, but got no errors"
    );
    assert!(
        errors.iter().any(|d| {
            d.code == Some(DiagnosticCode::MissingRequiredMember)
                && d.message.contains("elongation_at_break")
        }),
        "expected MissingRequiredMember error mentioning 'elongation_at_break', \
         got errors: {:?}",
        errors
    );
}

// ── §6.2 Strong — ultimate_tensile_strength rename (β task #4240) ─────────────

/// Strong conformer declaring ultimate_tensile_strength >= yield_strength compiles clean.
///
/// RED: stdlib Strong still declares `uts` (not `ultimate_tensile_strength`), so
/// `ultimate_tensile_strength` is unknown and `uts` is missing → MissingRequiredMember.
#[test]
fn strong_ultimate_tensile_strength_valid_is_clean() {
    let src = r#"
        structure def StrongRenameOk : Strong {
            param yield_strength             : Real = 250.0
            param ultimate_tensile_strength  : Real = 400.0
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
        "expected no Severity::Error diagnostics for Strong with ultimate_tensile_strength \
         >= yield_strength, got: {:?}",
        errors
    );
}

/// Strong conformer where ultimate_tensile_strength (250) < yield_strength (400)
/// should produce a Violated constraint result and a ConstraintViolated diagnostic.
///
/// RED: stdlib Strong still declares `uts`; the structure misses `uts` → compile error,
/// not a constraint-Violated, so the assertion for Violated fails.
#[test]
fn strong_ultimate_tensile_strength_below_yield_is_violated() {
    let src = r#"
        structure def StrongRenameViolated : Strong {
            param yield_strength             : Real = 400.0
            param ultimate_tensile_strength  : Real = 250.0
        }
    "#;
    let result: CheckResult = check_source_with_stdlib(src);
    let has_violated = result
        .constraint_results
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Violated);
    assert!(
        has_violated,
        "expected at least one Violated constraint for ultimate_tensile_strength=250 < \
         yield_strength=400, got: {:?}",
        result.constraint_results
    );
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
         ultimate_tensile_strength < yield_strength, got diagnostics: {:?}",
        result.diagnostics
    );
}

/// Strong conformer declaring only the old name `uts` (no ultimate_tensile_strength)
/// must produce a MissingRequiredMember error for `ultimate_tensile_strength`.
///
/// RED: stdlib Strong still declares `uts`, so supplying `uts` succeeds rather than
/// emitting a MissingRequiredMember error for `ultimate_tensile_strength`.
#[test]
fn strong_old_uts_name_is_missing_required_member() {
    let src = r#"
        structure def StrongOldName : Strong {
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
        !errors.is_empty(),
        "expected MissingRequiredMember error for 'ultimate_tensile_strength' when \
         only old 'uts' is supplied, but got no errors"
    );
    assert!(
        errors.iter().any(|d| {
            d.code == Some(DiagnosticCode::MissingRequiredMember)
                && d.message.contains("ultimate_tensile_strength")
        }),
        "expected MissingRequiredMember error mentioning 'ultimate_tensile_strength', \
         got errors: {:?}",
        errors
    );
}

// ── §6.2 FatigueRated — endurance_limit → optional params (β task #4240) ───────

/// FatigueRated conformer supplying only inherited MaterialSpec params (density + name)
/// with no fatigue-specific params at all should compile cleanly (all new params optional).
///
/// RED: stdlib FatigueRated still has required `endurance_limit` → MissingRequiredMember.
#[test]
fn fatigue_rated_no_fatigue_params_is_clean() {
    let src = r#"
        structure def FatigueNone : FatigueRated {
            param density : Real = 7850.0
            param name    : String = "steel"
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
        "expected no Severity::Error diagnostics when supplying no optional fatigue params \
         (all should default to undef), got: {:?}",
        errors
    );
}

/// FatigueRated conformer supplying a subset of the optional params (only fatigue_limit)
/// should compile cleanly.
///
/// RED: stdlib FatigueRated still has required `endurance_limit` → MissingRequiredMember.
#[test]
fn fatigue_rated_subset_fatigue_limit_only_is_clean() {
    let src = r#"
        structure def FatigueSubset : FatigueRated {
            param density      : Real   = 7850.0
            param name         : String = "steel"
            param fatigue_limit : Real  = 300.0
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
        "expected no Severity::Error diagnostics when supplying only fatigue_limit \
         (subset of optional params), got: {:?}",
        errors
    );
}

/// FatigueRated trait shape: fatigue_limit and fatigue_strength_at must live in
/// defaults with type Pressure; fatigue_cycles must live in defaults with type Int;
/// none of the three appear in required_members.
/// (task #3111: fatigue_limit and fatigue_strength_at tightened from Real to Pressure.)
///
/// RED: stdlib FatigueRated still has required `endurance_limit` — new params absent.
#[test]
fn fatigue_rated_optional_params_in_defaults() {
    let module = load_stdlib_module();

    let fatigue = module
        .trait_defs
        .iter()
        .find(|t| t.name == "FatigueRated")
        .expect("expected 'FatigueRated' trait in compiled module");

    // None of the three new params should be required.
    for param_name in &["fatigue_limit", "fatigue_strength_at", "fatigue_cycles"] {
        assert!(
            !fatigue.required_members.iter().any(|r| r.name == *param_name),
            "'{}' should be optional (= undef), not a required member",
            param_name
        );
    }

    // fatigue_limit must be in defaults with Pressure type (task #3111).
    let fl = fatigue
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("fatigue_limit"))
        .expect("expected 'fatigue_limit' in FatigueRated.defaults");
    match &fl.kind {
        DefaultKind::Param { cell_type, .. } => assert_eq!(
            *cell_type,
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
            "fatigue_limit should have Pressure type, got {:?}",
            cell_type
        ),
        other => panic!("expected DefaultKind::Param for fatigue_limit, got {:?}", other),
    }

    // fatigue_strength_at must be in defaults with Pressure type (task #3111).
    let fsa = fatigue
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("fatigue_strength_at"))
        .expect("expected 'fatigue_strength_at' in FatigueRated.defaults");
    match &fsa.kind {
        DefaultKind::Param { cell_type, .. } => assert_eq!(
            *cell_type,
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
            "fatigue_strength_at should have Pressure type, got {:?}",
            cell_type
        ),
        other => panic!(
            "expected DefaultKind::Param for fatigue_strength_at, got {:?}",
            other
        ),
    }

    // fatigue_cycles must remain in defaults with Type::Int (not a Real placeholder).
    let fc = fatigue
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("fatigue_cycles"))
        .expect("expected 'fatigue_cycles' in FatigueRated.defaults");
    match &fc.kind {
        DefaultKind::Param { cell_type, .. } => assert_eq!(
            *cell_type,
            Type::Int,
            "fatigue_cycles should have type Int, got {:?}",
            cell_type
        ),
        other => panic!("expected DefaultKind::Param for fatigue_cycles, got {:?}", other),
    }
}

// ── §6.2 ImpactResistant — impact_energy → optional params (β task #4240) ───────

/// ImpactResistant conformer supplying only inherited MaterialSpec params (density + name)
/// with neither impact param compiles cleanly (both new params optional).
///
/// RED: stdlib ImpactResistant still has required `impact_energy` → MissingRequiredMember.
#[test]
fn impact_resistant_no_impact_params_is_clean() {
    let source = r#"
        structure def ImpactNone : ImpactResistant {
            param density : Real = 7850.0
            param name : String = "steel"
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics when supplying no optional impact params \
         (both charpy_impact and izod_impact are optional), got: {:?}",
        errors
    );
}

/// ImpactResistant conformer supplying both optional params compiles cleanly.
///
/// RED: stdlib ImpactResistant still has required `impact_energy` — charpy_impact/izod_impact unknown.
#[test]
fn impact_resistant_both_impact_params_is_clean() {
    let source = r#"
        structure def ImpactBoth : ImpactResistant {
            param density : Real = 7850.0
            param name : String = "steel"
            param charpy_impact : Real = 80.0
            param izod_impact : Real = 60.0
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics when supplying both charpy_impact and izod_impact, \
         got: {:?}",
        errors
    );
}

/// ImpactResistant trait shape: charpy_impact and izod_impact must live in defaults
/// with type Energy; neither in required_members; impact_energy absent.
/// (task #3111: charpy_impact and izod_impact tightened from Real to Energy.)
///
/// RED: stdlib ImpactResistant still has required `impact_energy` — new params absent.
#[test]
fn impact_resistant_optional_params_in_defaults() {
    let module = load_stdlib_module();
    let impact = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ImpactResistant")
        .expect("expected 'ImpactResistant' trait in compiled module");

    for param_name in &["charpy_impact", "izod_impact"] {
        assert!(
            !impact.required_members.iter().any(|r| r.name == *param_name),
            "'{}' must NOT be a required member of ImpactResistant (optional = undef)",
            param_name
        );
    }

    // charpy_impact must be in defaults with Energy type (task #3111).
    let ci = impact
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("charpy_impact"))
        .expect("expected 'charpy_impact' in ImpactResistant.defaults");
    match &ci.kind {
        DefaultKind::Param { cell_type, .. } => assert_eq!(
            *cell_type,
            Type::Scalar {
                dimension: DimensionVector::ENERGY,
            },
            "charpy_impact should have Energy type, got {:?}",
            cell_type
        ),
        other => panic!("expected DefaultKind::Param for charpy_impact, got {:?}", other),
    }

    // izod_impact must be in defaults with Energy type (task #3111).
    let ii = impact
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("izod_impact"))
        .expect("expected 'izod_impact' in ImpactResistant.defaults");
    match &ii.kind {
        DefaultKind::Param { cell_type, .. } => assert_eq!(
            *cell_type,
            Type::Scalar {
                dimension: DimensionVector::ENERGY,
            },
            "izod_impact should have Energy type, got {:?}",
            cell_type
        ),
        other => panic!("expected DefaultKind::Param for izod_impact, got {:?}", other),
    }
}

// ── §6.2 Optional param type pins — Elastic.shear_modulus and Strong.compressive_strength ──

/// Pin that Elastic.shear_modulus optional default has Pressure type after task #3111.
/// Mirrors the temperature_dependent_has_reference_temperature_default_with_temperature_type
/// pattern (Type::Scalar{DimensionVector::TEMPERATURE}).
///
/// RED: stdlib Elastic still has shear_modulus : Real → cell_type is Real, not PRESSURE.
#[test]
fn elastic_shear_modulus_default_is_pressure_type() {
    let module = load_stdlib_module();

    let elastic = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Elastic")
        .expect("expected 'Elastic' trait in compiled module");

    // shear_modulus must be in defaults (optional = undef), not required_members.
    assert!(
        !elastic
            .required_members
            .iter()
            .any(|r| r.name == "shear_modulus"),
        "shear_modulus should be optional (= undef), not a required member"
    );

    let shear = elastic
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("shear_modulus"))
        .expect("expected 'shear_modulus' in Elastic.defaults");
    match &shear.kind {
        DefaultKind::Param { cell_type, .. } => assert_eq!(
            *cell_type,
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
            "shear_modulus should have Pressure type (task #3111), got {:?}",
            cell_type
        ),
        other => panic!(
            "expected DefaultKind::Param for shear_modulus, got {:?}",
            other
        ),
    }
}

/// Pin that Strong.compressive_strength optional default has Pressure type after task #3111.
/// Mirrors the temperature_dependent_has_reference_temperature_default_with_temperature_type
/// pattern (Type::Scalar{DimensionVector::TEMPERATURE}).
///
/// RED: stdlib Strong still has compressive_strength : Real → cell_type is Real, not PRESSURE.
#[test]
fn strong_compressive_strength_default_is_pressure_type() {
    let module = load_stdlib_module();

    let strong = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Strong")
        .expect("expected 'Strong' trait in compiled module");

    // compressive_strength must be in defaults (optional = undef), not required_members.
    assert!(
        !strong
            .required_members
            .iter()
            .any(|r| r.name == "compressive_strength"),
        "compressive_strength should be optional (= undef), not a required member"
    );

    let compr = strong
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("compressive_strength"))
        .expect("expected 'compressive_strength' in Strong.defaults");
    match &compr.kind {
        DefaultKind::Param { cell_type, .. } => assert_eq!(
            *cell_type,
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
            "compressive_strength should have Pressure type (task #3111), got {:?}",
            cell_type
        ),
        other => panic!(
            "expected DefaultKind::Param for compressive_strength, got {:?}",
            other
        ),
    }
}
