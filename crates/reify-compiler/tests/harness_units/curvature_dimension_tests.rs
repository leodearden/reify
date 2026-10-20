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
    assert_param_default_mismatch(
        "structure def S { param x : Curvature = 0.2 / 1mm }",
        DimensionVector::CURVATURE,
        DimensionVector::ABSORPTION_COEFF,
    );
}

/// Control: green before and after α — AbsorptionCoeff stays m⁻¹.
#[test]
fn absorption_coeff_param_rejects_the_curvature_spelling() {
    assert_param_default_mismatch(
        "structure def S { param x : AbsorptionCoeff = 1rad/m }",
        DimensionVector::ABSORPTION_COEFF,
        DimensionVector::CURVATURE,
    );
}

/// Asserts `source` raises an Error `ParamDefaultTypeMismatch` whose message
/// names the `declared` scalar type before the `initializer` one.
fn assert_param_default_mismatch(
    source: &str,
    declared: DimensionVector,
    initializer: DimensionVector,
) {
    let module = compile_source_with_stdlib(source);
    let mismatch = module
        .diagnostics
        .iter()
        .find(|d| {
            d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch)
                && d.severity == Severity::Error
        })
        .unwrap_or_else(|| {
            panic!(
                "expected an Error ParamDefaultTypeMismatch for `{source}`, got: {:?}",
                module.diagnostics
            )
        });

    let quoted = |dimension| format!("`{}`", Type::Scalar { dimension });
    let declared = quoted(declared);
    let initializer = quoted(initializer);
    let declared_at = mismatch.message.find(&declared);
    let initializer_at = mismatch.message.find(&initializer);
    assert!(
        matches!((declared_at, initializer_at), (Some(d), Some(i)) if d < i),
        "mismatch for `{source}` should name declared {declared} then initializer \
         {initializer}; got: {}",
        mismatch.message
    );
}
