//! End-to-end smoke test + diagnostic coverage for `imported` field sources.
//!
//! # PRD task 5 scope
//!
//! This file pins the public API surface and pipeline state for v0.2
//! imported-field support.  The goal is **testing, not wiring**: the
//! production glue that calls the ingestion / provenance / cache helpers
//! inside `elaborate_field` is scheduled for a future task.  These tests
//! document the *current* contract so the future task has a clear before/after
//! diff.
//!
//! # Test structure
//!
//! Three strata:
//!
//! - **Stratum A (1 test)** — End-to-end smoke: pins the v0.2 deferral
//!   pipeline (`FieldImportedV02` diagnostic + `Value::Undef` lambda
//!   placeholder).
//!
//! - **Stratum B (3 tests)** — Diagnostic surface: pins cross-crate
//!   reachability of representative `IngestError` variants from the
//!   eval-crate vantage.  Detailed per-variant coverage lives in
//!   `crates/reify-kernel-openvdb/tests/ingest_tests.rs`; we deliberately
//!   do NOT duplicate it here.
//!
//! - **Stratum C (2 tests)** — Provenance + cache integration: exercises the
//!   cross-cutting helpers (`build_field_import_provenance` and
//!   `CacheStore::imported_file_hash_changed`) from this crate's vantage.
//!
//! # Embedded source fixture
//!
//! The `imported` source kind currently emits a `Severity::Error`
//! `FieldImportedV02` deferral for any `imported` source, which makes it
//! incompatible with the `all_examples_parse_and_compile_with_stdlib` sweep
//! in `crates/reify-eval/tests/e2e_meta.rs`.  The source string is therefore
//! embedded directly in this file rather than stored under
//! `examples/fields/`.  When the production glue task lifts the deferral,
//! only the embedded string and the smoke-test assertions need updating —
//! no `examples/` migration required.
//!
//! # What to update when the glue task lands
//!
//! `imported_field_smoke_pins_v02_deferral_pipeline` will need:
//!   1. The deferral assertion replaced by a positive ingestion assertion
//!      (expect no `FieldImportedV02` error).
//!   2. The `Value::Undef` lambda assertion replaced by an assertion that
//!      the lambda is a populated `SampledField`.
//!   3. A provenance assertion checking the `FieldImportProvenance` record
//!      that `elaborate_field` will write.
//! The Stratum B and Stratum C tests remain valid through that transition.

// ── Stratum A imports ─────────────────────────────────────────────────────
use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{compile_source_with_stdlib, errors_only};
use reify_types::{
    FIELD_ENTITY_PREFIX, DiagnosticCode, FieldSourceKind, ModulePath, Value, ValueCellId,
};

// ─────────────────────────────────────────────────────────────────────────────
// Stratum A — End-to-end smoke
// ─────────────────────────────────────────────────────────────────────────────

/// Embedded source fixture — an `imported` field in a minimal module.
///
/// Uses `compile_source_with_stdlib` (not `parse_and_compile_with_stdlib`) so
/// the test survives the expected `FieldImportedV02` error without panicking.
/// See the design decision in the file-level rustdoc.
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
