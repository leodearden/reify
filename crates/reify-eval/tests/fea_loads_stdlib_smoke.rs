//! FEA-2 (task 2881) — `TractionLoad` and `BodyForce` structure-def smoke tests.
//!
//! Pins the wave-3 migration of `traction_load` and `body_force` from
//! name-dispatched builtins to stdlib `structure def`s declared in
//! `crates/reify-compiler/stdlib/fea_multi_case.ri`.  Each test verifies that a
//! source-level `TractionLoad(…)` / `BodyForce(…)` constructor lowers to a
//! `Value::StructureInstance` with the expected `type_name` and field values.

#![allow(clippy::mutable_key_type)]

use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_core::ValueCellId;
use reify_ir::{PersistentMap, Value};

/// `PersistentMap<String, Value>::get` is keyed by `&String`; this lets the
/// scenarios index `StructureInstance.fields` with a string literal.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ── step-1 (RED) → step-2 (GREEN) : TractionLoad ────────────────────────────

/// Bare `TractionLoad()` constructor lowers to a `Value::StructureInstance`
/// whose `type_name` is `"TractionLoad"` and whose fields carry the two
/// declared defaults: `face = ""`, `traction = 0.0`.
#[test]
fn traction_load_in_source_lowers_to_structure_instance() {
    const SOURCE: &str = r#"
structure def TractionLoadFixture {
    let load = TractionLoad()
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("TractionLoadFixture", "load");
    let load = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("TractionLoadFixture.load cell missing from eval result"));

    match load {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "TractionLoad",
                "expected type_name=\"TractionLoad\" (the wave-3 FEA-2 stdlib \
                 structure_def), got {:?}",
                data.type_name
            );
            // face default = ""
            assert_eq!(
                field(&data.fields, "face"),
                Some(&Value::String(String::new())),
                "TractionLoad.face default must be \"\"; fields: {:?}",
                data.fields
            );
            // traction default = 0.0
            assert_eq!(
                field(&data.fields, "traction"),
                Some(&Value::Real(0.0)),
                "TractionLoad.traction default must be 0.0; fields: {:?}",
                data.fields
            );
        }
        other => panic!(
            "expected Value::StructureInstance for TractionLoadFixture.load — \
             got {other:?}"
        ),
    }
}

/// `TractionLoad(face: "top", traction: 5.0)` constructor round-trips the
/// caller-supplied field values through the structure instance.
#[test]
fn traction_load_ctor_field_override_round_trips() {
    const SOURCE: &str = r#"
structure def TractionLoadFixture2 {
    let load = TractionLoad(face: "top", traction: 5.0)
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("TractionLoadFixture2", "load");
    let load = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("TractionLoadFixture2.load cell missing from eval result"));

    match load {
        Value::StructureInstance(data) => {
            assert_eq!(
                field(&data.fields, "face"),
                Some(&Value::String("top".to_string())),
                "TractionLoad.face override must be \"top\"; fields: {:?}",
                data.fields
            );
            assert_eq!(
                field(&data.fields, "traction"),
                Some(&Value::Real(5.0)),
                "TractionLoad.traction override must be 5.0; fields: {:?}",
                data.fields
            );
        }
        other => panic!(
            "expected Value::StructureInstance for TractionLoadFixture2.load — \
             got {other:?}"
        ),
    }
}

// ── step-5 (RED) → step-6 (GREEN) : BodyForce ───────────────────────────────

/// Bare `BodyForce()` constructor lowers to a `Value::StructureInstance` whose
/// `type_name` is `"BodyForce"` and whose fields carry the two declared
/// defaults: `body = ""`, `force_density = 0.0`.
#[test]
fn body_force_in_source_lowers_to_structure_instance() {
    const SOURCE: &str = r#"
structure def BodyForceFixture {
    let load = BodyForce()
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("BodyForceFixture", "load");
    let load = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("BodyForceFixture.load cell missing from eval result"));

    match load {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "BodyForce",
                "expected type_name=\"BodyForce\" (the wave-3 FEA-2 stdlib \
                 structure_def), got {:?}",
                data.type_name
            );
            // body default = ""
            assert_eq!(
                field(&data.fields, "body"),
                Some(&Value::String(String::new())),
                "BodyForce.body default must be \"\"; fields: {:?}",
                data.fields
            );
            // force_density default = 0.0
            assert_eq!(
                field(&data.fields, "force_density"),
                Some(&Value::Real(0.0)),
                "BodyForce.force_density default must be 0.0; fields: {:?}",
                data.fields
            );
        }
        other => panic!(
            "expected Value::StructureInstance for BodyForceFixture.load — \
             got {other:?}"
        ),
    }
}

/// `BodyForce(body: "all", force_density: -77000.0)` constructor round-trips
/// the caller-supplied field values through the structure instance.
#[test]
fn body_force_ctor_field_override_round_trips() {
    const SOURCE: &str = r#"
structure def BodyForceFixture2 {
    let load = BodyForce(body: "all", force_density: -77000.0)
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("BodyForceFixture2", "load");
    let load = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("BodyForceFixture2.load cell missing from eval result"));

    match load {
        Value::StructureInstance(data) => {
            assert_eq!(
                field(&data.fields, "body"),
                Some(&Value::String("all".to_string())),
                "BodyForce.body override must be \"all\"; fields: {:?}",
                data.fields
            );
            assert_eq!(
                field(&data.fields, "force_density"),
                Some(&Value::Real(-77000.0)),
                "BodyForce.force_density override must be -77000.0; fields: {:?}",
                data.fields
            );
        }
        other => panic!(
            "expected Value::StructureInstance for BodyForceFixture2.load — \
             got {other:?}"
        ),
    }
}
