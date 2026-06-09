//! task 4439 — `Gravity : Load` type-surface boundary tests (typed-fea-authoring α).
//!
//! Mirrors the Load-conformer wave pattern in `pressure_load.rs`.  All tests from
//! (1)–(5) are RED before step-2 declares `structure def Gravity : Load { … }` in
//! `crates/reify-compiler/stdlib/fea_multi_case.ri`; test (6) is the anti-vacuity
//! companion and passes independently.
//!
//! PRD: `docs/prds/v0_6/typed-fea-authoring-surface.md` §4.1, §8.

#![allow(clippy::mutable_key_type)]

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{PersistentMap, Value};
use reify_test_support::{
    collect_errors, compile_source_with_stdlib, make_simple_engine, parse_and_compile_with_stdlib,
};

fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ── task 4439 step-1 (RED) → step-2 (GREEN) tests ────────────────────────────

/// task 4439 step-1: bare `Gravity()` constructor lowers to a
/// `Value::StructureInstance` with `type_name == "Gravity"`, whose `magnitude`
/// field is `Value::Scalar { dimension: ACCELERATION, si_value ≈ 9.80665 }`
/// (from the `STANDARD_GRAVITY()` default), and whose `direction` field is
/// `Value::List([0.0, 0.0, -1.0])` (the canonical −Z unit vector).
///
/// RED: `Gravity(…)` is an unknown constructor today — falls through to
/// `Value::Undef` — so the `StructureInstance` branch is never reached.
#[test]
fn gravity_default_ctor_lowers_to_structure_instance() {
    const SOURCE: &str = r#"
structure def GravityFixture {
    let g = Gravity()
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("GravityFixture", "g");
    let g = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GravityFixture.g cell missing from eval result"));

    match g {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "Gravity",
                "expected type_name=\"Gravity\" (typed-fea-authoring α stdlib structure def); \
                 got {:?}",
                data.type_name
            );
            // magnitude default = STANDARD_GRAVITY() ≈ 9.80665 m/s²
            match field(&data.fields, "magnitude") {
                Some(Value::Scalar { si_value, dimension }) => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::ACCELERATION,
                        "Gravity.magnitude must have ACCELERATION dimension; got {:?}",
                        dimension
                    );
                    assert!(
                        (si_value - 9.80665).abs() < 1e-9,
                        "Gravity.magnitude si_value: expected ≈9.80665, got {}",
                        si_value
                    );
                }
                other => panic!(
                    "Gravity.magnitude must be Value::Scalar{{ACCELERATION, 9.80665}}; \
                     got {:?}",
                    other
                ),
            }
            // direction default = [0.0, 0.0, -1.0]
            match field(&data.fields, "direction") {
                Some(Value::List(items)) => {
                    assert_eq!(
                        items.len(),
                        3,
                        "Gravity().direction must have 3 elements; got {:?}",
                        items
                    );
                    assert_eq!(
                        items[0],
                        Value::Real(0.0),
                        "Gravity().direction[0] must be 0.0"
                    );
                    assert_eq!(
                        items[1],
                        Value::Real(0.0),
                        "Gravity().direction[1] must be 0.0"
                    );
                    assert_eq!(
                        items[2],
                        Value::Real(-1.0),
                        "Gravity().direction[2] must be -1.0 (−Z default)"
                    );
                }
                other => panic!(
                    "Gravity.direction must be Value::List([0.0, 0.0, -1.0]); got {:?}",
                    other
                ),
            }
        }
        other => panic!(
            "expected Value::StructureInstance for GravityFixture.g — got {other:?}"
        ),
    }
}

/// task 4439 step-1: `Gravity(magnitude: 5*STANDARD_GRAVITY())` round-trips the
/// override — `magnitude` must be `Value::Scalar { ACCELERATION, ≈49.03325 }`.
///
/// RED: same unknown-ctor path as above.
#[test]
fn gravity_magnitude_override_round_trips() {
    const SOURCE: &str = r#"
structure def GravityMagOverride {
    let g = Gravity(magnitude: 5*STANDARD_GRAVITY())
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("GravityMagOverride", "g");
    let g = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GravityMagOverride.g cell missing from eval result"));

    match g {
        Value::StructureInstance(data) => {
            match field(&data.fields, "magnitude") {
                Some(Value::Scalar { si_value, dimension }) => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::ACCELERATION,
                        "Gravity override magnitude must have ACCELERATION dimension"
                    );
                    assert!(
                        (si_value - 49.03325).abs() < 1e-9,
                        "5*STANDARD_GRAVITY() si_value: expected ≈49.03325, got {}",
                        si_value
                    );
                }
                other => panic!(
                    "Gravity override magnitude must be Value::Scalar{{ACCELERATION, 49.03325}}; \
                     got {:?}",
                    other
                ),
            }
            // direction must retain its [0,0,-1] default when only magnitude is overridden
            match field(&data.fields, "direction") {
                Some(Value::List(items)) => {
                    assert_eq!(items.len(), 3, "direction must have 3 elements; got {:?}", items);
                    assert_eq!(items[0], Value::Real(0.0), "direction[0] must be 0.0 (default)");
                    assert_eq!(items[1], Value::Real(0.0), "direction[1] must be 0.0 (default)");
                    assert_eq!(
                        items[2],
                        Value::Real(-1.0),
                        "direction[2] must be -1.0 (default −Z) when only magnitude is overridden"
                    );
                }
                other => panic!(
                    "Gravity direction must retain default [0,0,-1] when only magnitude is \
                     overridden; got {:?}",
                    other
                ),
            }
        }
        other => panic!(
            "expected Value::StructureInstance for GravityMagOverride.g — got {other:?}"
        ),
    }
}

/// task 4439 step-1: `Gravity` with an explicit direction override round-trips
/// correctly — `direction` must be `Value::List([1.0, 0.0, 0.0])`.
///
/// Both params are supplied in declaration order (`magnitude` first, `direction`
/// second) because structure-def constructors use positional binding — the same
/// pattern as `PointLoad(point: "", force: 0.0, direction: […])` in
/// `structure_instance_e2e.rs:552`.
///
/// RED: same unknown-ctor path as above.
#[test]
fn gravity_direction_override_round_trips() {
    const SOURCE: &str = r#"
structure def GravityDirOverride {
    let g = Gravity(magnitude: STANDARD_GRAVITY(), direction: [1.0, 0.0, 0.0])
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("GravityDirOverride", "g");
    let g = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GravityDirOverride.g cell missing from eval result"));

    match g {
        Value::StructureInstance(data) => {
            match field(&data.fields, "direction") {
                Some(Value::List(items)) => {
                    assert_eq!(
                        items.len(),
                        3,
                        "Gravity direction override must have 3 elements; got {:?}",
                        items
                    );
                    assert_eq!(
                        items[0],
                        Value::Real(1.0),
                        "Gravity direction override[0] must be 1.0"
                    );
                    assert_eq!(
                        items[1],
                        Value::Real(0.0),
                        "Gravity direction override[1] must be 0.0"
                    );
                    assert_eq!(
                        items[2],
                        Value::Real(0.0),
                        "Gravity direction override[2] must be 0.0"
                    );
                }
                other => panic!(
                    "Gravity direction override must be Value::List([1.0, 0.0, 0.0]); \
                     got {:?}",
                    other
                ),
            }
        }
        other => panic!(
            "expected Value::StructureInstance for GravityDirOverride.g — got {other:?}"
        ),
    }
}

/// Member-access chain `self.g.magnitude` reads through the `Gravity` structure
/// instance and resolves to `Value::Scalar { ACCELERATION, ≈9.80665 }`.
///
/// Mirrors `pressure_load_member_access_magnitude` in `pressure_load.rs` — covers
/// the member-access evaluator path, which is distinct from the ctor round-trip path
/// tested above.
#[test]
fn gravity_member_access_magnitude() {
    const SOURCE: &str = r#"
structure def GravityMagAccess {
    let g         = Gravity()
    let magnitude = self.g.magnitude
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("GravityMagAccess", "magnitude");
    let mag = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GravityMagAccess.magnitude cell missing from eval result"));

    match mag {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::ACCELERATION,
                "self.g.magnitude must have ACCELERATION dimension; got {:?}",
                dimension
            );
            assert!(
                (si_value - 9.80665).abs() < 1e-9,
                "self.g.magnitude si_value: expected ≈9.80665, got {}",
                si_value
            );
        }
        other => panic!(
            "self.g.magnitude must resolve to Value::Scalar{{ACCELERATION, 9.80665}}; \
             got {other:?}"
        ),
    }
}

/// Member-access chain `self.g.direction` reads through the `Gravity` structure
/// instance and resolves to `Value::List([0.0, 0.0, -1.0])` — the canonical
/// −Z unit vector default.
///
/// Covers the member-access evaluator path for the `List<Real>`-valued
/// `direction` field, which is materially distinct from the scalar `magnitude`
/// member-access shape tested above.  `PressureLoad.direction` is a `String`;
/// this is the first Load-conformer whose `direction` is a `List<Real>`, so
/// this test exercises a distinct read-through path.
#[test]
fn gravity_member_access_direction() {
    const SOURCE: &str = r#"
structure def GravityDirAccess {
    let g         = Gravity()
    let direction = self.g.direction
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("GravityDirAccess", "direction");
    let dir = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GravityDirAccess.direction cell missing from eval result"));

    match dir {
        Value::List(items) => {
            assert_eq!(
                items.len(),
                3,
                "self.g.direction must have 3 elements; got {:?}",
                items
            );
            assert_eq!(
                items[0],
                Value::Real(0.0),
                "self.g.direction[0] must be 0.0"
            );
            assert_eq!(
                items[1],
                Value::Real(0.0),
                "self.g.direction[1] must be 0.0"
            );
            assert_eq!(
                items[2],
                Value::Real(-1.0),
                "self.g.direction[2] must be -1.0 (−Z default)"
            );
        }
        other => panic!(
            "self.g.direction must resolve to Value::List([0.0, 0.0, -1.0]); \
             got {other:?}"
        ),
    }
}

/// task 4439 step-1: `param load : Load = Gravity()` compiles without any
/// Error-severity diagnostics — positive conformance test.
///
/// Mirrors `trait_typed_param_admits_pressure_load` in `pressure_load.rs`.
///
/// RED: `Gravity` is unknown before the structure def lands, so the compiler
/// emits an Error diagnostic for the unresolved constructor.
#[test]
fn gravity_conforms_to_load_param() {
    const SOURCE: &str = r#"
structure def LoadHolder {
    param load : Load = Gravity()
}
"#;

    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "Gravity must be admitted for a Load-typed param without Error diagnostics \
         (nominal conformance via `: Load` supertrait declaration); \
         got errors: {errors:?}"
    );
}

/// task 4439 step-1: `[Gravity()]` and `[Gravity(), PointLoad()]` fill a
/// `List<Load>` ctor-arg slot without Error diagnostics — the literal
/// `[g] : List<Load>` user-observable signal.
///
/// Tests both homogeneous (`[Gravity()]`) and heterogeneous
/// (`[Gravity(), PointLoad()]`) list forms.
///
/// RED: same unresolved-constructor path as above.
#[test]
fn gravity_conforms_in_list_load() {
    const SOURCE: &str = r#"
structure def LoadListConsumer {
    param loads : List<Load>
}
structure def Usage {
    sub a = LoadListConsumer(loads: [Gravity()])
    sub b = LoadListConsumer(loads: [Gravity(), PointLoad()])
}
"#;

    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "Gravity must be admitted in a List<Load> ctor-arg slot without Error diagnostics; \
         got errors: {errors:?}"
    );
}

/// task 4439 anti-vacuity companion: a structure that does NOT declare `: Load`
/// must be rejected when placed in a `List<Load>` slot.
///
/// Without this guard, the positive `gravity_conforms_in_list_load` above cannot
/// distinguish "Gravity nominally conforms" from "the List<Load> constraint is
/// silently ignored entirely".
///
/// Mirrors `trait_typed_param_rejects_non_load_structure` in `pressure_load.rs`.
/// This test is expected to PASS even before step-2 (the negative path is
/// independent of the Gravity structure def existing).
#[test]
fn list_load_rejects_non_load() {
    const SOURCE: &str = r#"
structure def NotALoad {
    param value : Real = 0.0
}
structure def LoadListConsumer {
    param loads : List<Load>
}
structure def BadUsage {
    sub consumer = LoadListConsumer(loads: [NotALoad()])
}
"#;

    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("does not conform to trait") && d.message.contains("Load")),
        "NotALoad must be rejected for a List<Load> slot with a \
         'does not conform to trait Load' error; got errors: {errors:?}"
    );
}
