//! Pins `Curvature` = rad·m⁻¹ at the compiler's declared-vs-initializer check
//! (docs/prds/v0_6/angle-dimension-completion.md leaf α, task #6179, B1).
//!
//! Cross-file references here name SYMBOLS, never line numbers.

use crate::common::stdlib_param_si_value;
use reify_core::{DiagnosticCode, DimensionVector, Severity, Type};
use reify_test_support::compile_source_with_stdlib;

/// The committed prd-gate fixture is α's user-observable signal: it must check
/// clean, with both the param-default and the ctor-conformance mismatch on
/// `kc` gone (docs/notes/ctor-conformance-flip-leaf-signals.md §α).
#[test]
fn prd_gate_fixture_curvature_rad_literal_compiles_clean() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/prd-gate/fixtures/curvature_rad_literal.ri");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e));

    let module = compile_source_with_stdlib(&src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "fixture {} should compile with no errors, got: {:?}",
        path.display(),
        errors
    );

    let mismatches: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.code,
                Some(DiagnosticCode::ParamDefaultTypeMismatch | DiagnosticCode::ArgTypeMismatch)
            )
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "fixture {} should carry no ParamDefaultTypeMismatch/ArgTypeMismatch of any severity, got: {:?}",
        path.display(),
        mismatches
    );
}

#[test]
fn curvature_param_resolves_rad_per_length_and_folds() {
    let (si, dim) = stdlib_param_si_value("Curvature", "1rad/m");
    assert_eq!(si, 1.0, "1rad/m should fold to exactly 1.0 SI");
    assert_eq!(dim, DimensionVector::CURVATURE);
}

#[test]
fn curvature_param_rejects_the_dimensionless_numerator_spelling() {
    let module = compile_source_with_stdlib("structure def S { param x : Curvature = 0.2 / 1mm }");
    let mismatch = module
        .diagnostics
        .iter()
        .find(|d| {
            d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch)
                && d.severity == Severity::Error
        })
        .unwrap_or_else(|| {
            panic!(
                "expected an Error ParamDefaultTypeMismatch for `0.2 / 1mm`, got: {:?}",
                module.diagnostics
            )
        });

    let declared = format!(
        "`{}`",
        Type::Scalar {
            dimension: DimensionVector::CURVATURE
        }
    );
    let initializer = format!(
        "`{}`",
        Type::Scalar {
            dimension: DimensionVector::ABSORPTION_COEFF
        }
    );
    assert!(
        mismatch.message.contains(&declared) && mismatch.message.contains(&initializer),
        "mismatch message should name declared {declared} and initializer {initializer}; got: {}",
        mismatch.message
    );
}

/// Control: green before and after α — AbsorptionCoeff stays m⁻¹.
#[test]
fn absorption_coeff_param_rejects_the_curvature_spelling() {
    let module =
        compile_source_with_stdlib("structure def S { param x : AbsorptionCoeff = 1rad/m }");
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "`param x : AbsorptionCoeff = 1rad/m` should be rejected, got no errors"
    );
}
