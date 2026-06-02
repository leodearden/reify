//! Tests for §6.4 electrical material trait parameter optionality (task δ).
//!
//! Validates the Kleene-degradation semantics of `dielectric_constant`,
//! `dielectric_strength`, and `magnetic_permeability` after they become
//! optional (`= undef`) on `ElectricallyCharacterized` in step-2:
//!
//! - Conductive conformers omitting all three optionals check clean.
//! - Insulating conformers omitting `dielectric_strength` degrade the
//!   `> 0.0V/m` constraint to `Satisfaction::Indeterminate` with a
//!   `ConstraintIndeterminate` Warning (Kleene undef-does-not-falsify).
//! - Insulating conformers supplying `dielectric_strength = 0.0V/m` still
//!   produce `Satisfaction::Violated` + `ConstraintViolated` Error.
//!
//! All tests use `check_source_with_stdlib` (gated behind `eval-helpers`,
//! which IS enabled in reify-compiler dev-deps via Cargo.toml:28) so that
//! real constraint evaluation runs via `SimpleConstraintChecker`.
//!
//! **RED on base (tests 1 and 2):** Before step-2 lands, `dielectric_constant`,
//! `dielectric_strength`, and `magnetic_permeability` are *required* members of
//! `ElectricallyCharacterized`. Omitting them produces a "missing required member"
//! compile error; `check_source_with_stdlib` panics via `parse_and_compile_with_stdlib`
//! which asserts no compile errors.
//!
//! **Preservation guard (test 3):** Green on base. The conformer supplies all
//! four required params including `dielectric_strength = 0.0V/m`, which already
//! violates the pre-existing `> 0.0V/m` constraint — so the `Violated` assertion
//! passes before and after the optionality change.

use reify_core::{DiagnosticCode, Severity};
use reify_ir::Satisfaction;
use reify_test_support::{assert_no_check_errors, check_source_with_stdlib};

// ─── (1) Conductive conformer omitting all three optionals → clean ────────────

/// A Conductive conformer omitting all three optional params (`dielectric_constant`,
/// `dielectric_strength`, `magnetic_permeability`) compiles and checks clean after
/// step-2 makes them optional:
///
/// - No Error-severity diagnostics.
/// - No `ConstraintIndeterminate` warning (Conductive has no `dielectric_strength`
///   constraint; the only injected constraint is `resistivity < 0.0001ohm*m`).
/// - Every `constraint_result` entry is `Satisfaction::Satisfied`.
///
/// RED on base: omitting the three still-required params causes a
/// "missing required member" compile error; `check_source_with_stdlib` panics.
#[test]
fn conductive_conformer_omitting_optionals_checks_clean() {
    // Omits dielectric_constant, dielectric_strength, magnetic_permeability.
    // resistivity = 1.7e-8 Ω·m < 0.0001 Ω·m → satisfies Conductive constraint.
    let source = r#"
structure def CopperLike : Conductive {
    param density : Real = 8960.0
    param name : String = "copper"
    param resistivity : ElectricResistivity = 0.000000017 * 1ohm * 1m
}
"#;
    let result = check_source_with_stdlib(source);

    // (a) No Error-severity diagnostics.
    assert_no_check_errors(&result);

    // (b) No ConstraintIndeterminate warning — Conductive has no dielectric_strength
    //     constraint, so omitting the optional params causes no Undef propagation
    //     into any constraint expression.
    let indeterminate_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::ConstraintIndeterminate)
        })
        .collect();
    assert!(
        indeterminate_warnings.is_empty(),
        "CopperLike : Conductive omitting optionals must not produce \
         ConstraintIndeterminate warnings; diagnostics: {:?}",
        indeterminate_warnings
    );

    // (c) All constraint_results are Satisfied — the only constraint is
    //     `resistivity < 0.0001ohm*m`, and 1.7e-8 < 1e-4 is true.
    assert!(
        !result.constraint_results.is_empty(),
        "expected at least one constraint_result for CopperLike : Conductive"
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "all constraint_results for CopperLike : Conductive should be Satisfied; \
             constraint {:?} is {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ─── (2) Insulating conformer omitting dielectric_strength → Indeterminate ────

/// An Insulating conformer omitting `dielectric_strength` (along with the other
/// two optional params) degrades the `> 0.0V/m` constraint to
/// `Satisfaction::Indeterminate` + a `ConstraintIndeterminate` Warning.
///
/// Expected outcome after step-2:
/// - No `ConstraintViolated` Error (Undef propagation is not a hard failure).
/// - At least one `ConstraintIndeterminate` Warning whose message contains
///   `"indeterminate: undefined inputs"`.
/// - At least one `constraint_results` entry is `Satisfaction::Indeterminate`.
/// - At least one `constraint_results` entry is `Satisfaction::Satisfied`
///   (the resistivity constraint, which guards the "constraints unchanged" clause).
///
/// RED on base: omitting the three still-required params causes a
/// "missing required member" compile error; `check_source_with_stdlib` panics.
#[test]
fn insulating_conformer_omitting_dielectric_strength_warns_indeterminate() {
    // Omits dielectric_strength, dielectric_constant, magnetic_permeability.
    // After step-2 these are optional (= undef); the injected
    // `dielectric_strength > 0.0V/m` constraint evaluates
    // Undef > 0.0V/m → Undef → Satisfaction::Indeterminate.
    // resistivity = 1e9 Ω·m > 1e6 Ω·m → Satisfied.
    let source = r#"
structure def GlassLike : Insulating {
    param density : Real = 2500.0
    param name : String = "glass"
    param resistivity : ElectricResistivity = 1000000000.0 * 1ohm * 1m
}
"#;
    let result = check_source_with_stdlib(source);

    // (a) No ConstraintViolated Error — Undef degrades to Indeterminate, not Violated.
    assert_no_check_errors(&result);

    // (b) At least one ConstraintIndeterminate Warning with the expected message text.
    let indeterminate_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::ConstraintIndeterminate)
        })
        .collect();
    assert!(
        !indeterminate_warnings.is_empty(),
        "GlassLike : Insulating omitting dielectric_strength must produce a \
         ConstraintIndeterminate warning; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        indeterminate_warnings
            .iter()
            .any(|d| d.message.contains("indeterminate: undefined inputs")),
        "ConstraintIndeterminate warning message must contain \
         \"indeterminate: undefined inputs\"; got: {:?}",
        indeterminate_warnings
    );

    // (c) At least one constraint_results entry is Indeterminate (dielectric_strength).
    assert!(
        result
            .constraint_results
            .iter()
            .any(|r| r.satisfaction == Satisfaction::Indeterminate),
        "at least one constraint must be Indeterminate for GlassLike : Insulating \
         omitting dielectric_strength; constraint_results: {:?}",
        result.constraint_results
    );

    // (d) At least one constraint_results entry is Satisfied (resistivity > 1e6).
    assert!(
        result
            .constraint_results
            .iter()
            .any(|r| r.satisfaction == Satisfaction::Satisfied),
        "the resistivity constraint must be Satisfied for GlassLike : Insulating; \
         constraint_results: {:?}",
        result.constraint_results
    );
}

// ─── (3) Insulating conformer zero dielectric_strength → Violated ─────────────

/// An Insulating conformer supplying `dielectric_strength = 0.0V/m` produces
/// `Satisfaction::Violated` + a `ConstraintViolated` Error diagnostic, because
/// `0.0V/m > 0.0V/m` is definitively false (not Indeterminate).
///
/// The resistivity constraint (`resistivity > 1000000ohm*m`) remains Satisfied.
///
/// Preservation guard — GREEN on base. On base all four params are required and
/// the conformer supplies them all. The `> 0.0V/m` constraint already fires on
/// the supplied zero value, so this assertion passes before and after the
/// optionality change in step-2.
#[test]
fn insulating_conformer_zero_dielectric_strength_violates() {
    // Supplies all params; dielectric_strength = 0.0V/m violates > 0.0V/m.
    // resistivity = 1e9 Ω·m > 1e6 Ω·m → Satisfied.
    let source = r#"
structure def GlassLike : Insulating {
    param density : Real = 2500.0
    param name : String = "glass"
    param resistivity : ElectricResistivity = 1000000000.0 * 1ohm * 1m
    param dielectric_constant : Real = 7.0
    param dielectric_strength : DielectricStrength = 0.0V/m
    param magnetic_permeability : Real = 1.0
}
"#;
    let result = check_source_with_stdlib(source);

    // (a) At least one ConstraintViolated Error diagnostic.
    let violated_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::ConstraintViolated)
        })
        .collect();
    assert!(
        !violated_errors.is_empty(),
        "GlassLike : Insulating with dielectric_strength = 0.0V/m must produce \
         a ConstraintViolated Error; diagnostics: {:?}",
        result.diagnostics
    );

    // (b) At least one constraint_results entry is Violated (dielectric_strength).
    assert!(
        result
            .constraint_results
            .iter()
            .any(|r| r.satisfaction == Satisfaction::Violated),
        "at least one constraint must be Violated for GlassLike : Insulating \
         with dielectric_strength = 0.0V/m; constraint_results: {:?}",
        result.constraint_results
    );

    // (c) At least one constraint_results entry is Satisfied (resistivity > 1e6).
    assert!(
        result
            .constraint_results
            .iter()
            .any(|r| r.satisfaction == Satisfaction::Satisfied),
        "the resistivity constraint must be Satisfied for GlassLike : Insulating; \
         constraint_results: {:?}",
        result.constraint_results
    );
}
