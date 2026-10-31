//! Task 6922 (PRD `dimension-checked-readers.md` leaf ε): the `MassProperties`
//! reader gates the dimension of `mass` / `com` / `inertia`, and the
//! inverse-dynamics consumer refuses (and diagnoses) a body whose mass is not a
//! mass. Driven through public seams only: `mass_properties_from_value`,
//! `eval_builtin` and `dynamics_diagnose`, plus an engine eval of an inline copy
//! of `examples/dynamics/pendulum_idyn.ri`.

use reify_core::{DiagnosticCode, DimensionVector, Severity, ValueCellId};
use reify_ir::{StructureInstanceData, StructureTypeId, Value};
use reify_stdlib::{dynamics_diagnose, eval_builtin, mass_properties_from_value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Inline copy of `examples/dynamics/pendulum_idyn.ri` (a 1 kg point mass
/// 100 mm below a revolute joint, held at θ = −30°).
const PENDULUM_SOURCE: &str = "structure def PendulumIdyn {
    let mp = MassProperties(mass: 1kg, com: point3(0mm, 0mm, -100mm), inertia: [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]], origin: Frame3(origin: vec3(0mm, 0mm, 0mm), x_axis: vec3(0, 0, 0), y_axis: vec3(0, 0, 0), z_axis: vec3(0, 0, 0)))
    let axis_y = vec3(0, 1, 0)
    let joint = revolute(axis_y, -180deg .. 180deg)
    let m0 = mechanism()
    let m = body(m0, mp, joint)
    let snap = snapshot(m, [bind(joint, -30deg)])
    let forces = inverse_dynamics_at_snapshot(m, snap, [0.0], [0.0])
}";

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn scalar(si_value: f64, dimension: DimensionVector) -> Value {
    Value::Scalar {
        si_value,
        dimension,
    }
}

fn moment_of_inertia(si_value: f64) -> Value {
    scalar(si_value, DimensionVector::MOMENT_OF_INERTIA)
}

fn mass_properties(mass: Value, com: Value, inertia: Value) -> Value {
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "MassProperties".to_string(),
        version: 1,
        fields: [("mass", mass), ("com", com), ("inertia", inertia)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    }))
}

fn point(components: [Value; 3]) -> Value {
    Value::Point(components.to_vec())
}

fn origin() -> Value {
    point([Value::length(0.0), Value::length(0.0), Value::length(0.0)])
}

fn inertia(cells: [[f64; 3]; 3], cell: fn(f64) -> Value) -> Value {
    Value::Matrix(
        cells
            .iter()
            .map(|row| row.iter().map(|&x| cell(x)).collect())
            .collect(),
    )
}

#[test]
fn mass_properties_reader_rejects_a_wrong_dimension_field() {
    let identity = || inertia(IDENTITY, moment_of_inertia);

    let length_mass = mass_properties(scalar(2.0, DimensionVector::LENGTH), origin(), identity());
    assert_eq!(
        mass_properties_from_value(&length_mass),
        None,
        "a 2 m mass is not a mass"
    );
    let kilograms = mass_properties(scalar(2.0, DimensionVector::MASS), origin(), identity());
    assert_eq!(
        mass_properties_from_value(&kilograms),
        Some((2.0, [0.0; 3], IDENTITY)),
        "the 2 kg twin reads unchanged"
    );

    let mass_com = point([
        scalar(0.0, DimensionVector::MASS),
        Value::length(0.0),
        Value::length(0.0),
    ]);
    let wrong_com = mass_properties(scalar(2.0, DimensionVector::MASS), mass_com, identity());
    assert_eq!(mass_properties_from_value(&wrong_com), None);
    let length_inertia = mass_properties(
        scalar(2.0, DimensionVector::MASS),
        origin(),
        inertia(IDENTITY, Value::length),
    );
    assert_eq!(mass_properties_from_value(&length_inertia), None);

    let bare = mass_properties(Value::Real(2.0), origin(), inertia(IDENTITY, Value::Real));
    assert_eq!(
        mass_properties_from_value(&bare),
        Some((2.0, [0.0; 3], IDENTITY)),
        "bare mass and bare inertia cells stay the transitional legacy spelling"
    );
}

/// `(m, snap, forces)` of the inline pendulum.
fn pendulum_cells() -> (Value, Value, Value) {
    let compiled = parse_and_compile_with_stdlib(PENDULUM_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let cell = |name: &str| {
        result
            .values
            .get(&ValueCellId::new("PendulumIdyn", name))
            .cloned()
            .unwrap_or_else(|| panic!("PendulumIdyn.{name} not found"))
    };
    (cell("m"), cell("snap"), cell("forces"))
}

/// `mechanism` with every body's `solid` replaced by `solid`.
fn with_body_solid(mechanism: &Value, solid: &Value) -> Value {
    let Value::Map(mut mech) = mechanism.clone() else {
        panic!("a mechanism is a Map, got {mechanism:?}");
    };
    let bodies_key = Value::String("bodies".to_string());
    let Some(Value::List(bodies)) = mech.get(&bodies_key).cloned() else {
        panic!("a mechanism carries a bodies List");
    };
    let bodies = bodies
        .into_iter()
        .map(|body| match body {
            Value::Map(mut b) => {
                b.insert(Value::String("solid".to_string()), solid.clone());
                Value::Map(b)
            }
            other => other,
        })
        .collect();
    mech.insert(bodies_key, Value::List(bodies));
    Value::Map(mech)
}

#[test]
fn inverse_dynamics_refuses_and_diagnoses_a_length_mass_body() {
    const LOWER: &str = "inverse_dynamics_at_snapshot_lower";
    let (m, snap, forces) = pendulum_cells();
    assert!(
        !matches!(forces, Value::Undef),
        "control: the pendulum's forces must resolve"
    );

    let pendulum_body = |mass: Value| {
        mass_properties(
            mass,
            point([Value::length(0.0), Value::length(0.0), Value::length(-0.1)]),
            inertia([[0.0; 3]; 3], Value::Real),
        )
    };
    let args = |solid: &Value| {
        [
            with_body_solid(&m, solid),
            snap.clone(),
            Value::List(vec![Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0)]),
        ]
    };

    let metre_body = pendulum_body(scalar(1.0, DimensionVector::LENGTH));
    assert_eq!(
        eval_builtin(LOWER, &args(&metre_body)),
        Value::Undef,
        "a 1 m mass must not be read as 1 kg"
    );
    let diagnostic = dynamics_diagnose(LOWER, &args(&metre_body))
        .expect("the rejected body mass must be diagnosed");
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(
        diagnostic.code,
        Some(DiagnosticCode::DimensionedArgRejected)
    );
    assert!(
        diagnostic.message.contains("MassProperties.mass"),
        "got {:?}",
        diagnostic.message
    );

    let kilogram_body = pendulum_body(scalar(1.0, DimensionVector::MASS));
    assert_ne!(eval_builtin(LOWER, &args(&kilogram_body)), Value::Undef);
    assert!(dynamics_diagnose(LOWER, &args(&kilogram_body)).is_none());
}
