//! FEA-2 (task 2881) — `TractionLoad` and `BodyForce` structure-def smoke tests,
//! plus the typed `PointLoad.point` round trip (task 5312).
//!
//! Pins the wave-3 migration of `traction_load` and `body_force` from
//! name-dispatched builtins to stdlib `structure def`s declared in
//! `crates/reify-compiler/stdlib/fea_multi_case.ri`.  Each test verifies that a
//! source-level `TractionLoad(…)` / `BodyForce(…)` / `PointLoad(…)` constructor
//! lowers to a `Value::StructureInstance` with the expected `type_name` and
//! field values.

#![allow(clippy::mutable_key_type)]

use reify_core::ValueCellId;
use reify_core::ty::SelectorKind;
use reify_ir::value::LeafQuery;
use reify_ir::{PersistentMap, Value};
use reify_test_support::{assert_selector_leaf, make_simple_engine, parse_and_compile_with_stdlib};

/// `PersistentMap<String, Value>::get` is keyed by `&String`; this lets the
/// scenarios index `StructureInstance.fields` with a string literal.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ── step-1 (RED) → step-2 (GREEN) : TractionLoad ────────────────────────────

/// A `TractionLoad` supplying only its required `face` lowers to a
/// `Value::StructureInstance` whose `type_name` is `"TractionLoad"` and whose
/// `traction` carries the declared default `0.0`.
#[test]
fn traction_load_in_source_lowers_to_structure_instance() {
    const SOURCE: &str = r#"
structure def TractionLoadFixture {
    let body = box(1m, 0.1m, 0.1m)
    let load = TractionLoad(face: face(body, "top"))
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

/// `TractionLoad(face: face(body, "top"), traction: 5.0)` constructor
/// round-trips the caller-supplied field values through the structure
/// instance; `face` is a typed Face selector naming "top" (task 5312).
#[test]
fn traction_load_ctor_field_override_round_trips() {
    const SOURCE: &str = r#"
structure def TractionLoadFixture2 {
    let body = box(1m, 0.1m, 0.1m)
    let load = TractionLoad(face: face(body, "top"), traction: 5.0)
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
            assert_selector_leaf(
                field(&data.fields, "face"),
                "TractionLoad.face",
                SelectorKind::Face,
                |q| assert_eq!(q, &LeafQuery::Named("top".into())),
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

/// A `BodyForce` supplying only its required `body` lowers to a
/// `Value::StructureInstance` whose `type_name` is `"BodyForce"` and whose
/// `force_density` carries the declared default `0.0`.
#[test]
fn body_force_in_source_lowers_to_structure_instance() {
    const SOURCE: &str = r#"
structure def BodyForceFixture {
    let body = box(1m, 0.1m, 0.1m)
    let load = BodyForce(body: solid_body(body, "all"))
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

/// `BodyForce(body: solid_body(body, "all"), force_density: -77000.0)`
/// constructor round-trips the caller-supplied field values through the
/// structure instance; `body` is a typed Body selector naming "all" (task 5312).
#[test]
fn body_force_ctor_field_override_round_trips() {
    const SOURCE: &str = r#"
structure def BodyForceFixture2 {
    let body = box(1m, 0.1m, 0.1m)
    let load = BodyForce(body: solid_body(body, "all"), force_density: -77000.0)
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
            assert_selector_leaf(
                field(&data.fields, "body"),
                "BodyForce.body",
                SelectorKind::Body,
                |q| assert_eq!(q, &LeafQuery::Named("all".into())),
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

// ── task 5312 step-9 (RED) → step-10 (GREEN) : PointLoad.point ──────────────

/// `PointLoad(point: vertex(body, "tip"), force: 1000.0)` round-trips through
/// the structure instance; `point` is a typed Vertex selector naming "tip".
#[test]
fn point_load_ctor_point_round_trips_as_a_vertex_selector() {
    const SOURCE: &str = r#"
structure def PointLoadFixture {
    let body = box(1m, 0.1m, 0.1m)
    let load = PointLoad(point: vertex(body, "tip"), force: 1000.0)
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("PointLoadFixture", "load");
    let load = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("PointLoadFixture.load cell missing from eval result"));

    match load {
        Value::StructureInstance(data) => {
            assert_selector_leaf(
                field(&data.fields, "point"),
                "PointLoad.point",
                SelectorKind::Vertex,
                |q| assert_eq!(q, &LeafQuery::Named("tip".into())),
            );
            assert_eq!(
                field(&data.fields, "force"),
                Some(&Value::Real(1000.0)),
                "PointLoad.force override must be 1000.0; fields: {:?}",
                data.fields
            );
        }
        other => {
            panic!("expected Value::StructureInstance for PointLoadFixture.load — got {other:?}")
        }
    }
}
