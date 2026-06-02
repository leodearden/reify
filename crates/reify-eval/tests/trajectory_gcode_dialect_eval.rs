//! Runtime evaluation tests for the G-code dialect marker types declared in
//! `crates/reify-compiler/stdlib/trajectory.ri` (PRD §7.2, task ξ 3861).
//!
//! Verifies the "runtime values produced" signal: constructing `MarlinDialect()`
//! and `KlipperDialect()` via the eval engine yields `Value::StructureInstance`
//! with the correct `type_name`. Also proves a `GcodeDialect`-typed param
//! admits both dialect values and preserves them as runtime `StructureInstance`s,
//! directly exercising the consumer-ο dispatch premise.
//!
//! Mirrors `point_load_in_source_lowers_to_structure_instance` in
//! `structure_instance_e2e.rs` (task 3540 step-19).

use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::{
    collect_errors, compile_source_with_stdlib, make_simple_engine, parse_and_compile_with_stdlib,
};

// ─── step-51: MarlinDialect ───────────────────────────────────────────────────

/// Constructing `MarlinDialect()` in a `.ri` source fixture evaluates to a
/// `Value::StructureInstance` whose `type_name` is `"MarlinDialect"`.
///
/// This exercises the SIR-α zero-param structure constructor path (no fields
/// map entries expected for a marker structure).
#[test]
fn marlin_dialect_constructs_to_structure_instance() {
    const SOURCE: &str = r#"
structure def MarlinDialectFixture {
    let d = MarlinDialect()
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("MarlinDialectFixture", "d");
    let value = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("MarlinDialectFixture.d cell missing from eval result"));

    match value {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "MarlinDialect",
                "expected type_name=\"MarlinDialect\" (zero-DOF G-code dialect marker), \
                 got {:?}",
                data.type_name
            );
        }
        other => panic!(
            "expected Value::StructureInstance for MarlinDialectFixture.d — \
             got {other:?}"
        ),
    }
}

// ─── step-53: KlipperDialect ──────────────────────────────────────────────────

/// Constructing `KlipperDialect()` in a `.ri` source fixture evaluates to a
/// `Value::StructureInstance` whose `type_name` is `"KlipperDialect"`.
#[test]
fn klipper_dialect_constructs_to_structure_instance() {
    const SOURCE: &str = r#"
structure def KlipperDialectFixture {
    let d = KlipperDialect()
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("KlipperDialectFixture", "d");
    let value = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("KlipperDialectFixture.d cell missing from eval result"));

    match value {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "KlipperDialect",
                "expected type_name=\"KlipperDialect\" (zero-DOF G-code dialect marker), \
                 got {:?}",
                data.type_name
            );
        }
        other => panic!(
            "expected Value::StructureInstance for KlipperDialectFixture.d — \
             got {other:?}"
        ),
    }
}

/// A `GcodeDialect`-typed param admits both `MarlinDialect()` and
/// `KlipperDialect()` values and preserves them as runtime `StructureInstance`s.
///
/// This exercises the consumer-ο dispatch premise: `value_type_kind_matches`
/// routes `StructureInstance ↔ TraitObject("GcodeDialect")` conformance
/// through the trait registry, so both dialect values materialise correctly
/// when passed through a `GcodeDialect`-typed param.
///
/// Pattern: `sub marlin = ChooseDialect(d: MarlinDialect())` → the sub cell
/// is at `ValueCellId::new("DialectHolder", "marlin")` (parent scope, sub
/// name), giving a ChooseDialect StructureInstance whose `d` field holds the
/// dialect value. Mirrors `trait_typed_param_admits_conforming_structure` in
/// structure_instance_e2e.rs.
#[test]
fn gcode_dialect_param_admits_both_dialects() {
    const SOURCE: &str = r#"
structure def ChooseDialect {
    param d : GcodeDialect = MarlinDialect()
}
structure def DialectHolder {
    sub marlin  = ChooseDialect(d: MarlinDialect())
    sub klipper = ChooseDialect(d: KlipperDialect())
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // The sub `marlin` is a value cell on DialectHolder; it holds a
    // ChooseDialect StructureInstance whose `d` field is the dialect value.
    let marlin_sub = result
        .values
        .get(&ValueCellId::new("DialectHolder", "marlin"))
        .unwrap_or_else(|| panic!("DialectHolder.marlin sub cell missing from eval result"));

    match marlin_sub {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "ChooseDialect");
            match data.fields.get(&"d".to_string()) {
                Some(Value::StructureInstance(d)) => assert_eq!(
                    d.type_name, "MarlinDialect",
                    "DialectHolder.marlin.d should be MarlinDialect; got {:?}",
                    d.type_name
                ),
                other => {
                    panic!("expected StructureInstance for DialectHolder.marlin.d; got {other:?}")
                }
            }
        }
        other => panic!("expected StructureInstance for DialectHolder.marlin sub; got {other:?}"),
    }

    let klipper_sub = result
        .values
        .get(&ValueCellId::new("DialectHolder", "klipper"))
        .unwrap_or_else(|| panic!("DialectHolder.klipper sub cell missing from eval result"));

    match klipper_sub {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "ChooseDialect");
            match data.fields.get(&"d".to_string()) {
                Some(Value::StructureInstance(d)) => assert_eq!(
                    d.type_name, "KlipperDialect",
                    "DialectHolder.klipper.d should be KlipperDialect; got {:?}",
                    d.type_name
                ),
                other => {
                    panic!("expected StructureInstance for DialectHolder.klipper.d; got {other:?}")
                }
            }
        }
        other => panic!("expected StructureInstance for DialectHolder.klipper sub; got {other:?}"),
    }
}

// ─── amend: negative-conformance ─────────────────────────────────────────────

/// A non-`GcodeDialect` structure passed to a `GcodeDialect`-typed param must
/// be rejected at compile time with a trait-conformance diagnostic.
///
/// This confirms the `GcodeDialect` bound is actually enforced by the trait
/// registry (not vacuous) — the positive `gcode_dialect_param_admits_both_dialects`
/// test would still pass if the bound had no enforcement at all (e.g. if any
/// `StructureInstance` were admitted). Mirrors `nominal_conformance_enforcement_negative`
/// (scenario 5) in `structure_instance_e2e.rs`.
#[test]
fn gcode_dialect_param_rejects_non_conforming_structure() {
    // NotADialect declares no trait bound, so it does NOT conform to GcodeDialect.
    // Passing it into a `param d : GcodeDialect` must produce a diagnostic.
    const SOURCE: &str = r#"
structure def NotADialect {}

structure def ChooseDialectStrict {
    param d : GcodeDialect = MarlinDialect()
}
structure def BadDialectHolder {
    sub bad = ChooseDialectStrict(d: NotADialect())
}
"#;
    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("does not conform to trait")
                && d.message.contains("GcodeDialect")),
        "passing a non-conforming `NotADialect()` to a GcodeDialect-typed param \
         must produce a trait-conformance error; got: {errors:?}"
    );
}
