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
use reify_types::{
    FIELD_ENTITY_PREFIX, DiagnosticCode, FieldSourceKind, ModulePath, Value, ValueCellId,
};

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
/// 1. The source string parses without errors.
/// 2. Compiling emits at least one `Severity::Error` with
///    `DiagnosticCode::FieldImportedV02`, whose message contains `"v0.2"` and
///    `"imported"`.
/// 3. `compiled.fields` has exactly one entry whose `source` is
///    `CompiledFieldSource::Imported`.
/// 4. `Engine::eval` produces a `Value::Field { source: FieldSourceKind::Imported,
///    lambda }` where `*lambda == Value::Undef` (the placeholder lowered by
///    `engine_eval::elaborate_field`).
///
/// When the production glue task lands and the deferral lifts, update this
/// test as described in the file-level rustdoc.
#[test]
fn imported_field_smoke_pins_v02_deferral_pipeline() {
    // 1. Parse — must succeed with no parse errors.
    let parsed = reify_syntax::parse(IMPORTED_FIELD_SOURCE, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // 2. Compile — intentionally uses compile_source_with_stdlib, NOT
    //    parse_and_compile_with_stdlib, because the FieldImportedV02 deferral
    //    is a Severity::Error by design; parse_and_compile_with_stdlib would
    //    panic here.
    let compiled = compile_source_with_stdlib(IMPORTED_FIELD_SOURCE);

    // 2a. Expect at least one FieldImportedV02 error.
    let errors = errors_only(&compiled);
    assert!(
        !errors.is_empty(),
        "expected at least one Severity::Error for imported field source, got no errors"
    );

    let has_v02_deferral = errors.iter().any(|d| {
        d.code == Some(DiagnosticCode::FieldImportedV02)
            && d.message.contains("v0.2")
            && d.message.contains("imported")
    });
    assert!(
        has_v02_deferral,
        "expected DiagnosticCode::FieldImportedV02 with message containing 'v0.2' and 'imported', \
         got: {:?}",
        errors
            .iter()
            .map(|d| (d.code, d.severity, &d.message))
            .collect::<Vec<_>>()
    );

    // 2b. The deferral diagnostic must carry at least one label.
    let deferral = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FieldImportedV02))
        .unwrap();
    assert!(
        !deferral.labels.is_empty(),
        "FieldImportedV02 diagnostic should carry at least one label"
    );

    // 3. Exactly one compiled field, with CompiledFieldSource::Imported.
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

    // 4. Eval — FieldSourceKind::Imported + lambda == Value::Undef.
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
        other => panic!(
            "expected Value::Field for imported field, got: {:?}",
            other
        ),
    }
}
