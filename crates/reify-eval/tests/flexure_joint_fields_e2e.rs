//! Eval round-trip tests for the flexure joint fields added in task 3849
//! (Phase-1 of docs/prds/v0_3/compliant-joints-flexures.md).
//!
//! Asserts that Revolute and Prismatic structure-def constructors carry the
//! spring_rate / damping / neutral optional fields through the eval pipeline:
//!   - a field supplied via `some(...)` → Value::Option(Some(Scalar{...}))
//!   - a field omitted (defaults to `= none`) → Value::Option(None)

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{PersistentMap, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ─── Revolute with spring_rate supplied ──────────────────────────────────────

#[test]
fn revolute_spring_rate_some_round_trips() {
    const SOURCE: &str = r#"
structure def Probe {
    let r = Revolute(axis: vec3(0.0, 0.0, 1.0), spring_rate: some(1N*m/rad))
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("Probe", "r");
    let r = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Probe.r cell missing from eval result"));

    match r {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "Revolute");

            // spring_rate = some(1 N·m/rad) → Value::Option(Some(Scalar))
            match field(&data.fields, "spring_rate") {
                Some(Value::Option(Some(inner))) => match inner.as_ref() {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!(
                            (*si_value - 1.0).abs() < 1e-12,
                            "Revolute.spring_rate si_value should be 1.0, got {si_value}"
                        );
                        assert_eq!(
                            *dimension,
                            DimensionVector::ROTATIONAL_STIFFNESS,
                            "Revolute.spring_rate dimension should be ROTATIONAL_STIFFNESS"
                        );
                    }
                    other => panic!("Revolute.spring_rate inner should be Scalar, got {other:?}"),
                },
                other => panic!(
                    "Revolute.spring_rate should be Value::Option(Some(Scalar)), got {other:?}"
                ),
            }

            // damping omitted → Value::Option(None)
            assert_eq!(
                field(&data.fields, "damping"),
                Some(&Value::Option(None)),
                "Revolute.damping default must be Value::Option(None)"
            );

            // neutral omitted → Value::Option(None)
            assert_eq!(
                field(&data.fields, "neutral"),
                Some(&Value::Option(None)),
                "Revolute.neutral default must be Value::Option(None)"
            );
        }
        other => panic!("expected Value::StructureInstance for Probe.r, got {other:?}"),
    }
}

// ─── Revolute with all fields omitted ────────────────────────────────────────

#[test]
fn revolute_all_optional_fields_default_to_none() {
    const SOURCE: &str = r#"
structure def Probe {
    let r = Revolute(axis: vec3(0.0, 0.0, 1.0))
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("Probe", "r");
    let r = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Probe.r cell missing from eval result"));

    match r {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "Revolute");
            assert_eq!(
                field(&data.fields, "spring_rate"),
                Some(&Value::Option(None))
            );
            assert_eq!(field(&data.fields, "damping"), Some(&Value::Option(None)));
            assert_eq!(field(&data.fields, "neutral"), Some(&Value::Option(None)));
        }
        other => panic!("expected Value::StructureInstance for Probe.r, got {other:?}"),
    }
}

// ─── Prismatic with spring_rate supplied ─────────────────────────────────────

#[test]
fn prismatic_spring_rate_some_round_trips() {
    const SOURCE: &str = r#"
structure def Probe {
    let p = Prismatic(axis: vec3(0.0, 0.0, 1.0), spring_rate: some(1N/m))
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("Probe", "p");
    let p = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Probe.p cell missing from eval result"));

    match p {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "Prismatic");

            // spring_rate = some(1 N/m) → Value::Option(Some(Scalar{TRANSLATIONAL_STIFFNESS}))
            match field(&data.fields, "spring_rate") {
                Some(Value::Option(Some(inner))) => match inner.as_ref() {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!(
                            (*si_value - 1.0).abs() < 1e-12,
                            "Prismatic.spring_rate si_value should be 1.0, got {si_value}"
                        );
                        assert_eq!(
                            *dimension,
                            DimensionVector::TRANSLATIONAL_STIFFNESS,
                            "Prismatic.spring_rate dimension should be TRANSLATIONAL_STIFFNESS"
                        );
                    }
                    other => panic!("Prismatic.spring_rate inner should be Scalar, got {other:?}"),
                },
                other => panic!(
                    "Prismatic.spring_rate should be Value::Option(Some(Scalar)), got {other:?}"
                ),
            }

            // damping omitted → Value::Option(None)
            assert_eq!(
                field(&data.fields, "damping"),
                Some(&Value::Option(None)),
                "Prismatic.damping default must be Value::Option(None)"
            );

            // neutral omitted → Value::Option(None)
            assert_eq!(
                field(&data.fields, "neutral"),
                Some(&Value::Option(None)),
                "Prismatic.neutral default must be Value::Option(None)"
            );
        }
        other => panic!("expected Value::StructureInstance for Probe.p, got {other:?}"),
    }
}

// ─── Prismatic with neutral supplied ─────────────────────────────────────────

#[test]
fn prismatic_neutral_some_round_trips_as_length() {
    // Use a typed intermediate param to sidestep the unit-suffix ambiguity
    // that `some(1m)` / `some(1000mm)` encounters in an untyped some() context.
    // `ref_len` carries the Length annotation so its value is unambiguously
    // resolved as LENGTH before being wrapped by some().
    const SOURCE: &str = r#"
structure def Probe {
    param ref_len : Length = 1000mm
    let p = Prismatic(axis: vec3(0.0, 0.0, 1.0), spring_rate: none, damping: none, neutral: some(ref_len))
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("Probe", "p");
    let p = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Probe.p cell missing from eval result"));

    match p {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "Prismatic");

            // neutral = some(ref_len) where ref_len=1000mm=1.0m SI → Value::Option(Some(Scalar{LENGTH}))
            match field(&data.fields, "neutral") {
                Some(Value::Option(Some(inner))) => match inner.as_ref() {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!(
                            (*si_value - 1.0).abs() < 1e-12,
                            "Prismatic.neutral si_value should be 1.0 (ref_len=1000mm), got {si_value}"
                        );
                        assert_eq!(
                            *dimension,
                            DimensionVector::LENGTH,
                            "Prismatic.neutral dimension should be LENGTH"
                        );
                    }
                    other => panic!("Prismatic.neutral inner should be Scalar, got {other:?}"),
                },
                other => {
                    panic!("Prismatic.neutral should be Value::Option(Some(Scalar)), got {other:?}")
                }
            }
        }
        other => panic!("expected Value::StructureInstance for Probe.p, got {other:?}"),
    }
}
