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
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

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
#[test]
fn gcode_dialect_param_admits_both_dialects() {
    const SOURCE: &str = r#"
structure def ChooseDialect {
    param d : GcodeDialect = MarlinDialect()
    let chosen = self.d
}
structure def DialectHolder {
    sub marlin  = ChooseDialect(d: MarlinDialect())
    sub klipper = ChooseDialect(d: KlipperDialect())
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let marlin_id = ValueCellId::new("DialectHolder/marlin", "chosen");
    let marlin_val = result
        .values
        .get(&marlin_id)
        .unwrap_or_else(|| panic!("DialectHolder/marlin.chosen missing from eval result"));

    match marlin_val {
        Value::StructureInstance(data) => assert_eq!(
            data.type_name, "MarlinDialect",
            "DialectHolder/marlin.chosen should be MarlinDialect; got {:?}",
            data.type_name
        ),
        other => panic!(
            "expected StructureInstance for DialectHolder/marlin.chosen; got {other:?}"
        ),
    }

    let klipper_id = ValueCellId::new("DialectHolder/klipper", "chosen");
    let klipper_val = result
        .values
        .get(&klipper_id)
        .unwrap_or_else(|| panic!("DialectHolder/klipper.chosen missing from eval result"));

    match klipper_val {
        Value::StructureInstance(data) => assert_eq!(
            data.type_name, "KlipperDialect",
            "DialectHolder/klipper.chosen should be KlipperDialect; got {:?}",
            data.type_name
        ),
        other => panic!(
            "expected StructureInstance for DialectHolder/klipper.chosen; got {other:?}"
        ),
    }
}
