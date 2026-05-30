//! End-to-end tests for `imported` field sources.
//!
//! # Why `compile_source_with_stdlib` instead of `parse_and_compile_with_stdlib`
//!
//! `parse_and_compile_with_stdlib` panics on any `Severity::Error` diagnostic.
//! During initial wiring work `compile_source_with_stdlib` is preferred so tests
//! can assert the absence of errors explicitly. Once the import path is fully
//! wired, `parse_and_compile_with_stdlib` can be used for the success path.
//!
//! The embedded source string (rather than an `examples/fields/*.ri` fixture)
//! keeps the test self-contained and avoids contaminating the
//! `all_examples_parse_and_compile_with_stdlib` sweep in `e2e_meta.rs`, which
//! expects no `Severity::Error` diagnostics.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{compile_source_with_stdlib, errors_only};
use reify_core::{DiagnosticCode, FIELD_ENTITY_PREFIX, ValueCellId};
use reify_ir::{FieldSourceKind, Value};

/// Embedded source fixture — an `imported` field in a minimal module.
///
/// Uses `compile_source_with_stdlib` (not `parse_and_compile_with_stdlib`) so
/// the test survives the expected `FieldImportedV02` error without panicking.
/// See the file-level rustdoc for the rationale.
const IMPORTED_FIELD_SOURCE: &str = r#"
field def pressure_map : Point3 -> Scalar {
    source = imported {
        path = "fea_results.vdb"
        format = OpenVDB
        grid = "pressure"
    }
}
"#;

/// Smoke test: imported field compiles without errors (FieldImportedV02 deferral lifted)
/// and evaluates to a `Value::Field { source: FieldSourceKind::Imported, lambda: Value::Undef }`.
///
/// The `Value::Undef` lambda assertion holds until the elaborate_field Imported arm
/// is wired (PRD task 5 step-8); step-8 will replace it with `Value::SampledField`.
#[test]
fn imported_field_smoke_pins_v02_deferral_pipeline() {
    let compiled = compile_source_with_stdlib(IMPORTED_FIELD_SOURCE);

    // No FieldImportedV02 error (deferral lifted) and no Severity::Error.
    let errors = errors_only(&compiled);
    assert!(
        errors.iter().all(|d| d.code != Some(DiagnosticCode::FieldImportedV02)),
        "unexpected FieldImportedV02, got: {:?}",
        errors.iter().map(|d| d.code).collect::<Vec<_>>()
    );
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics, got: {:?}",
        errors.iter().map(|d| (d.code, &d.message)).collect::<Vec<_>>()
    );

    // Exactly one compiled field, with CompiledFieldSource::Imported { .. }.
    assert_eq!(
        compiled.fields.len(),
        1,
        "expected exactly 1 compiled field, got {}",
        compiled.fields.len()
    );
    let field = &compiled.fields[0];
    assert!(
        matches!(field.source, reify_compiler::CompiledFieldSource::Imported { .. }),
        "expected CompiledFieldSource::Imported, got {:?}",
        field.source
    );

    // Eval — FieldSourceKind::Imported + lambda == Value::Undef (until step-8 wires read_vdb_file).
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let cell_id = ValueCellId::new(FIELD_ENTITY_PREFIX, &field.name);
    let val = result
        .values
        .get(&cell_id)
        .unwrap_or_else(|| panic!("no value for field cell {:?}", cell_id));

    match val {
        Value::Field { source, lambda, .. } => {
            assert_eq!(
                *source,
                FieldSourceKind::Imported,
                "expected FieldSourceKind::Imported, got {:?}",
                source
            );
            assert_eq!(
                **lambda,
                Value::Undef,
                "elaborate_field not yet wired: expected lambda == Value::Undef, got {:?}",
                **lambda
            );
        }
        other => panic!("expected Value::Field for imported field, got: {:?}", other),
    }
}
