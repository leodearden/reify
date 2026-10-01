//! Eval round-trip tests for the compliant-joint fields on the std.kinematic
//! joint structures: free-DOF compliance (task 3849, Phase-1 of
//! docs/prds/v0_3/compliant-joints-flexures.md) and constrained-DOF compliance
//! (task 7163, docs/prds/v0_6/assembly-modal-connection-graph.md §7 C2).
//!
//! Asserts that the structure-def constructors carry their optional compliance
//! fields through the eval pipeline:
//!   - a field supplied via `some(...)` → Value::Option(Some(Scalar{...}))
//!   - a field omitted (defaults to `= none`) → Value::Option(None)

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{PersistentMap, Value};
use reify_test_support::{
    collect_errors, compile_source_with_stdlib, make_simple_engine, parse_and_compile_with_stdlib,
};

fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ─── Revolute with spring_rate supplied ──────────────────────────────────────

#[test]
fn revolute_spring_rate_some_round_trips() {
    const SOURCE: &str = r#"
structure def Probe {
    let r = Revolute(axis: vec3(0.0, 0.0, 1.0), spring_rate: some(1N*m/rad^2))
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

            // spring_rate = some(1 N·m/rad²) → Value::Option(Some(Scalar))
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

// ─── Constrained-DOF compliance (task 7163) ──────────────────────────────────

/// Read the committed fixture. Path resolved from `CARGO_MANIFEST_DIR` so the
/// test is location-independent.
fn fixture_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/prd-gate/fixtures/joint_constrained_dof_compliance.ri");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e))
}

fn eval_fixture() -> reify_eval::EvalResult {
    let compiled = parse_and_compile_with_stdlib(&fixture_source());
    make_simple_engine().eval(&compiled)
}

/// The fields of `JointConstrainedDofCompliance.<cell>`, which must have
/// evaluated to a `kind` structure instance.
fn joint_fields<'a>(
    result: &'a reify_eval::EvalResult,
    cell: &str,
    kind: &str,
) -> &'a PersistentMap<String, Value> {
    let id = ValueCellId::new("JointConstrainedDofCompliance", cell);
    match result.values.get(&id) {
        Some(Value::StructureInstance(data)) => {
            assert_eq!(data.type_name, kind, "{cell} should be a {kind} instance");
            &data.fields
        }
        other => panic!("expected {cell} to evaluate to a {kind} instance, got {other:?}"),
    }
}

fn expect_some_scalar(
    fields: &PersistentMap<String, Value>,
    key: &str,
    si: f64,
    dim: DimensionVector,
) {
    match field(fields, key) {
        Some(Value::Option(Some(inner))) => match inner.as_ref() {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (*si_value - si).abs() <= 1e-9 * si.abs(),
                    "{key} should read back {si} (SI), got {si_value}"
                );
                assert_eq!(*dimension, dim, "{key} has the wrong dimension");
            }
            other => panic!("{key} should wrap a Scalar, got {other:?}"),
        },
        other => panic!("{key} should be Value::Option(Some(Scalar)), got {other:?}"),
    }
}

fn expect_none(fields: &PersistentMap<String, Value>, key: &str) {
    assert_eq!(
        field(fields, key),
        Some(&Value::Option(None)),
        "omitted {key} must read back as none (ideal-rigid)"
    );
}

const TRANSLATIONAL: DimensionVector = DimensionVector::TRANSLATIONAL_STIFFNESS;
const ROTATIONAL: DimensionVector = DimensionVector::ROTATIONAL_STIFFNESS;

/// The `reify check` signal: compliance on every kind's constrained groups,
/// written in constructor form, compiles with no errors. Pre-task this drew one
/// `E_CTOR_UNKNOWN_FIELD` per compliance argument.
#[test]
fn constrained_dof_compliance_fixture_compiles_clean() {
    let compiled = compile_source_with_stdlib(&fixture_source());
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile with no Error-severity diagnostics; got {errors:#?}"
    );
}

/// Expected values are SI: the fixture's `210N/um` reads back as 2.1e8 N/m, and
/// its rotational literals are already SI (N·m/rad²).
#[test]
fn constrained_dof_compliance_round_trips_through_ctor_form() {
    let result = eval_fixture();

    let air_bearing = joint_fields(&result, "air_bearing", "Prismatic");
    expect_some_scalar(air_bearing, "radial_stiffness", 2.1e8, TRANSLATIONAL);
    expect_some_scalar(air_bearing, "tilt_stiffness", 5100.0, ROTATIONAL);
    expect_none(air_bearing, "torsional_stiffness");

    let spindle = joint_fields(&result, "spindle", "Revolute");
    expect_some_scalar(spindle, "axial_stiffness", 2.2e8, TRANSLATIONAL);
    expect_some_scalar(spindle, "radial_stiffness", 2.3e8, TRANSLATIONAL);
    expect_some_scalar(spindle, "tilt_stiffness", 5200.0, ROTATIONAL);

    let quill = joint_fields(&result, "quill", "Cylindrical");
    expect_some_scalar(quill, "radial_stiffness", 2.4e8, TRANSLATIONAL);
    expect_some_scalar(quill, "tilt_stiffness", 5300.0, ROTATIONAL);

    let puck = joint_fields(&result, "puck", "Planar");
    expect_some_scalar(puck, "normal_stiffness", 2.5e8, TRANSLATIONAL);
    expect_some_scalar(puck, "tilt_stiffness", 5400.0, ROTATIONAL);

    let ball = joint_fields(&result, "ball", "Spherical");
    expect_some_scalar(ball, "translational_stiffness", 2.6e8, TRANSLATIONAL);

    let foot = joint_fields(&result, "foot", "Fixed");
    expect_some_scalar(foot, "translational_stiffness", 2.7e8, TRANSLATIONAL);
    expect_some_scalar(foot, "rotational_stiffness", 5500.0, ROTATIONAL);

    let ideal = joint_fields(&result, "ideal", "Prismatic");
    for key in ["radial_stiffness", "tilt_stiffness", "torsional_stiffness"] {
        expect_none(ideal, key);
    }
}

/// The connector form compiles to the synthetic `__connector_0` instance, which
/// is the channel assembly-modal α extracts and δ reads (PRD C1). `reify check`
/// cannot discriminate here (pre-task, an unknown connector-block param was
/// dropped silently), so reading the value back is the signal.
#[test]
fn constrained_dof_compliance_round_trips_through_connector_form() {
    let result = eval_fixture();
    let connector = joint_fields(&result, "__connector_0", "Prismatic");
    expect_some_scalar(connector, "radial_stiffness", 2.8e8, TRANSLATIONAL);
    expect_some_scalar(connector, "tilt_stiffness", 5600.0, ROTATIONAL);
    expect_none(connector, "torsional_stiffness");
}
