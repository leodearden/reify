//! End-to-end eval tests for `gcode_import` reaching the real eval path
//! (task 4073).
//!
//! Guards that `gcode_import(source, dialect)` in a `.ri` structure actually
//! returns a real `Value::List` of motion profiles — NOT the `{ [] }` empty-list
//! stub the body previously contained.
//!
//! **RED today:** the `{ [] }` body makes `imported` evaluate to an empty list,
//! `profile_count` to 0, and the `profile_count > 0` constraint to Violated.
//! After the dispatch fix (step-2) these assertions go GREEN.
//!
//! Reads the shipped example file via `CARGO_MANIFEST_DIR` so this test is a
//! true guard of `examples/trajectory/gcode_import_smoke.ri`, not an inlined
//! copy. Follows the idiom from `kinematic_examples_e2e.rs`.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use std::sync::OnceLock;

use reify_core::ValueCellId;
use reify_ir::{Satisfaction, Value};
use reify_test_support::{
    check_source_with_stdlib, collect_errors, make_simple_engine, parse_and_compile_with_stdlib,
};

// ── Path constant ──────────────────────────────────────────────────────────────

const SMOKE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/trajectory/gcode_import_smoke.ri"
);

// ── Cached source + compile helpers ───────────────────────────────────────────

/// Read `examples/trajectory/gcode_import_smoke.ri`, caching the result.
fn smoke_source() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(SMOKE_PATH)
            .unwrap_or_else(|e| panic!("{SMOKE_PATH} should exist: {e}"))
    })
    .as_str()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Compile-clean gate (sanity check — should always be green)
// ═══════════════════════════════════════════════════════════════════════════════

/// The example file exists, is non-empty, and compiles with stdlib without any
/// Error-severity diagnostics. This is a pre-condition for the eval assertions.
#[test]
fn gcode_import_smoke_compiles_clean() {
    let source = smoke_source();
    assert!(
        !source.is_empty(),
        "gcode_import_smoke.ri should be non-empty"
    );
    let compiled = parse_and_compile_with_stdlib(source);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "gcode_import_smoke.ri should compile with zero Error diagnostics, got: {errors:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRIMARY: eval-path assertion — imported is a non-empty Value::List
// ═══════════════════════════════════════════════════════════════════════════════

/// **PRIMARY RED→GREEN signal.**
///
/// `GcodeImportSmoke.imported` must be a `Value::List` with at least one
/// element (the single `G1 X10 Y10` move lowers to exactly one profile).
///
/// Before the fix: `imported` is an empty `Value::List` (the `{ [] }` stub body
/// runs instead of `eval_gcode_import`).
/// After the fix: `imported` is a 1-element `Value::List` of profile records.
#[test]
fn gcode_import_smoke_imported_is_nonempty_list() {
    let source = smoke_source();
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("GcodeImportSmoke", "imported");
    let imported = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GcodeImportSmoke.imported cell missing from eval result"));

    match imported {
        Value::List(items) => {
            assert!(
                !items.is_empty(),
                "GcodeImportSmoke.imported should be a non-empty list \
                 (the G1 move lowers to >= 1 profile); got an empty list — \
                 gcode_import is still returning the stub {{ [] }} body"
            );
        }
        other => panic!(
            "expected Value::List for GcodeImportSmoke.imported, got {other:?}"
        ),
    }
}

/// `GcodeImportSmoke.profile_count` must be `Value::Int(n)` with `n >= 1`.
///
/// Before the fix: `profile_count` is `Value::Int(0)` (count of the empty list).
/// After the fix: `profile_count` is `Value::Int(1)` (one profile for the single
/// contiguous `G1` move).
#[test]
fn gcode_import_smoke_profile_count_is_at_least_one() {
    let source = smoke_source();
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("GcodeImportSmoke", "profile_count");
    let count = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GcodeImportSmoke.profile_count cell missing"));

    match count {
        Value::Int(n) => {
            assert!(
                *n >= 1,
                "GcodeImportSmoke.profile_count should be >= 1, got {n} — \
                 gcode_import is still returning the stub {{ [] }} empty list"
            );
        }
        other => panic!(
            "expected Value::Int for GcodeImportSmoke.profile_count, got {other:?}"
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECONDARY: constraint-satisfaction assertion — profile_count > 0 is Satisfied
// ═══════════════════════════════════════════════════════════════════════════════

/// **SECONDARY RED→GREEN signal (reify check path).**
///
/// The `constraint profile_count > 0` in `GcodeImportSmoke` must surface as
/// `Satisfaction::Satisfied` after the dispatch fix.
///
/// Before the fix: `profile_count` is 0 → constraint is `Satisfied::Violated`.
/// After the fix: `profile_count` is 1 → constraint is `Satisfaction::Satisfied`.
///
/// This directly validates the task title: "its profile_count constraint is a
/// real eval-time assertion."
#[test]
fn gcode_import_smoke_profile_count_constraint_is_satisfied() {
    let source = smoke_source();
    let check_result = check_source_with_stdlib(source);

    // The GcodeImportSmoke structure has exactly one inline constraint:
    // `constraint profile_count > 0`.
    let satisfied = check_result
        .constraint_results
        .iter()
        .any(|entry| entry.satisfaction == Satisfaction::Satisfied);

    assert!(
        satisfied,
        "expected at least one Satisfied constraint for GcodeImportSmoke.profile_count > 0; \
         got: {:?}. gcode_import is still returning the stub {{ [] }} body (profile_count == 0).",
        check_result.constraint_results
    );

    // Also assert there are no Violated constraints (the only constraint is
    // profile_count > 0, which should now be Satisfied).
    let violated: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "expected no Violated constraints after gcode_import dispatch fix, \
         got: {violated:?}"
    );
}
