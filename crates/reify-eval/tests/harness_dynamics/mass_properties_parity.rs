//! PRD `trampoline-decode-and-result-build.md` §4 C4 / §5 T9 (task 8697): every
//! production `MassProperties` entry writes exactly the fields `std.dynamics`
//! declares, the layout read from the Engine's structure registry.

use reify_core::DimensionVector;
use reify_ir::Value;
use reify_stdlib::dynamics::mass_props::MassProperties;
use reify_stdlib::eval_builtin;
use reify_test_support::make_simple_engine;
use reify_test_support::result_parity::key_set_parity;

fn scalar(si_value: f64, dimension: DimensionVector) -> Value {
    Value::Scalar {
        si_value,
        dimension,
    }
}

#[test]
fn mass_properties_written_equals_declared() {
    let engine = make_simple_engine();
    let registry = engine.structure_registry();

    let meta = registry
        .id_for("MassProperties")
        .and_then(|id| registry.meta(id))
        .expect("the std.dynamics prelude interns MassProperties");
    let mut declared: Vec<&str> = meta
        .field_layout
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    declared.sort_unstable();
    assert_eq!(declared, ["com", "inertia", "mass"]);

    let diagonal = [[0.1, 0.0, 0.0], [0.0, 0.2, 0.0], [0.0, 0.0, 0.3]];
    let two_kg = || scalar(2.0, DimensionVector::MASS);
    let com = Value::Point([0.1, 0.2, 0.3].map(Value::length).to_vec());
    let inertia = Value::Matrix(
        diagonal
            .iter()
            .map(|row| {
                row.iter()
                    .map(|&x| scalar(x, DimensionVector::MOMENT_OF_INERTIA))
                    .collect()
            })
            .collect(),
    );

    let entries = [
        (
            "MassProperties::to_value",
            MassProperties {
                mass: 2.0,
                com: [0.1, 0.2, 0.3],
                inertia: diagonal,
            }
            .to_value(),
        ),
        (
            "MassProperties::unresolved_value",
            MassProperties::unresolved_value(),
        ),
        ("point_mass", eval_builtin("point_mass", &[two_kg()])),
        (
            "mass_properties",
            eval_builtin("mass_properties", &[two_kg(), com, inertia]),
        ),
    ];
    for (entry, value) in &entries {
        assert!(
            matches!(value, Value::StructureInstance(_)),
            "{entry} must mint a MassProperties instance, got {value:?}"
        );
        assert_eq!(key_set_parity(registry, value), Ok(()), "{entry}");
    }
}
