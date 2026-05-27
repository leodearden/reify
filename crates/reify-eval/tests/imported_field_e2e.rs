//! End-to-end smoke test for `imported` field sources in the v0.2 deferral state.
//!
//! # Why `compile_source_with_stdlib` instead of `parse_and_compile_with_stdlib`
//!
//! `parse_and_compile_with_stdlib` panics on any `Severity::Error` diagnostic.
//! The `imported` source kind currently emits `DiagnosticCode::FieldImportedV02`
//! (a `Severity::Error`) by design — the whole point of this test is to pin that
//! error.  `compile_source_with_stdlib` only panics on parse errors, so it is the
//! correct helper here.
//!
//! # What to update when the glue task lands
//!
//! When production wiring replaces the v0.2 deferral inside `elaborate_field`,
//! update `imported_field_smoke_pins_v02_deferral_pipeline`:
//!   1. Remove the `FieldImportedV02` error assertion.
//!   2. Replace the `Value::Undef` lambda assertion with an assertion that the
//!      lambda is a populated `SampledField`.
//!   3. Add a provenance assertion checking the `FieldImportProvenance` record
//!      written by `elaborate_field`.
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

/// Pins the currently-shipping v0.2 deferral pipeline for `imported` field
/// sources end-to-end:
///
/// 1. Compiling emits at least one `Severity::Error` with
///    `DiagnosticCode::FieldImportedV02`.  Message wording and label details
///    are pinned by `compile_field_imported_emits_v02_deferral_diagnostic` in
///    `crates/reify-compiler/tests/field_compile_tests.rs`.
/// 2. `compiled.fields` has exactly one entry whose `source` is
///    `CompiledFieldSource::Imported`.
/// 3. `Engine::eval` produces a `Value::Field { source: FieldSourceKind::Imported,
///    lambda }` where `*lambda == Value::Undef` (the placeholder lowered by
///    `engine_eval::elaborate_field`).
///
/// When the production glue task lands and the deferral lifts, update this
/// test as described in the file-level rustdoc.
#[test]
fn imported_field_smoke_pins_v02_deferral_pipeline() {
    // 1. Compile — intentionally uses compile_source_with_stdlib, NOT
    //    parse_and_compile_with_stdlib, because the FieldImportedV02 deferral
    //    is a Severity::Error by design; parse_and_compile_with_stdlib panics
    //    on any Severity::Error.  compile_source_with_stdlib panics only on
    //    parse errors, which also enforces the parse-correctness contract.
    let compiled = compile_source_with_stdlib(IMPORTED_FIELD_SOURCE);

    // Expect at least one FieldImportedV02 error.  Message wording and label
    // details are pinned by the compiler-crate test; pin only the code here
    // so this test doesn't need updating if the wording changes.
    let errors = errors_only(&compiled);
    assert!(
        errors
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::FieldImportedV02)),
        "expected DiagnosticCode::FieldImportedV02, got: {:?}",
        errors.iter().map(|d| d.code).collect::<Vec<_>>()
    );

    // 2. Exactly one compiled field, with CompiledFieldSource::Imported.
    assert_eq!(
        compiled.fields.len(),
        1,
        "expected exactly 1 compiled field, got {}",
        compiled.fields.len()
    );
    let field = &compiled.fields[0];
    assert!(
        matches!(field.source, reify_compiler::CompiledFieldSource::Imported),
        "expected CompiledFieldSource::Imported, got {:?}",
        field.source
    );

    // 3. Eval — FieldSourceKind::Imported + lambda == Value::Undef.
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
                "v0.2 deferral placeholder: expected lambda == Value::Undef, got {:?}",
                **lambda
            );
        }
        other => panic!("expected Value::Field for imported field, got: {:?}", other),
    }
}
