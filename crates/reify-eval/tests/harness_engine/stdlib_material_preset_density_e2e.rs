//! A defaulted stdlib FEA material preset exposes `density` to evaluation as a
//! dimensioned `Value::Scalar` (task #9572).
//!
//! The only other reader of a defaulted preset's `density` on the solver path
//! is the heavy, gate-excluded bracket e2e (`harness_fea_solver_e2e`), and the
//! goldens that print `density` are rewritten wholesale by
//! `REIFY_REGENERATE_GOLDEN=1`. These pins assert the exact `Value::Scalar`
//! instead, so a preset that stops carrying `density` fails the merge gate and
//! names the field.
//!
//! Lives under `harness_engine/` for the anti-re-accretion reason recorded in
//! `dimensioned_ctor_migration_si_values.rs`.

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{StructureInstanceData, Value, ValueMap};
use reify_test_support::{assert_dimensioned, make_simple_engine, parse_and_compile_with_stdlib};

const PRESET_DENSITIES_STRUCTURE: &str = "DefaultedPresetDensities";

const PRESET_DENSITIES_SRC: &str = r#"structure def DefaultedPresetDensities {
    let steel = Steel_AISI_1045()
    let aluminium = Aluminium_6061_T6()
    let titanium = Titanium_Ti6Al4V()
    let abs = ABS_Plastic()
    let steel_rho = steel.density
    let aluminium_rho = aluminium.density
    let titanium_rho = titanium.density
    let abs_rho = abs.density
}"#;

/// One stdlib FEA preset, constructed with `()` so every param takes its default.
struct Preset {
    /// The `let` member holding the instance; `<cell>_rho` holds `<cell>.density`.
    cell: &'static str,
    type_name: &'static str,
    /// The `param density` default in `materials_fea.ri`, in SI kg/m^3.
    density_si: f64,
}

const PRESETS: [Preset; 4] = [
    Preset {
        cell: "steel",
        type_name: "Steel_AISI_1045",
        density_si: 7850.0,
    },
    Preset {
        cell: "aluminium",
        type_name: "Aluminium_6061_T6",
        density_si: 2700.0,
    },
    Preset {
        cell: "titanium",
        type_name: "Titanium_Ti6Al4V",
        density_si: 4430.0,
    },
    Preset {
        cell: "abs",
        type_name: "ABS_Plastic",
        density_si: 1050.0,
    },
];

fn structure_cell<'a>(values: &'a ValueMap, member: &str) -> &'a Value {
    values
        .get(&ValueCellId::new(PRESET_DENSITIES_STRUCTURE, member))
        .unwrap_or_else(|| {
            panic!("{PRESET_DENSITIES_STRUCTURE}.{member} cell missing from eval result")
        })
}

fn preset_instance<'a>(values: &'a ValueMap, preset: &Preset) -> &'a StructureInstanceData {
    let cell = structure_cell(values, preset.cell);
    let Value::StructureInstance(instance) = cell else {
        panic!(
            "{PRESET_DENSITIES_STRUCTURE}.{} must be a {} StructureInstance; got {cell:?}",
            preset.cell, preset.type_name
        )
    };
    assert_eq!(
        instance.type_name, preset.type_name,
        "{PRESET_DENSITIES_STRUCTURE}.{} has the wrong type_name",
        preset.cell
    );
    instance
}

/// A named field of a `StructureInstance`, panicking with the sorted names that
/// ARE present when it is absent: an instance carrying `density_provenance`
/// but no `density` is the signature of a preset that lost its density.
fn field<'a>(instance: &'a StructureInstanceData, name: &str) -> &'a Value {
    instance.fields.get(&name.to_string()).unwrap_or_else(|| {
        let mut present: Vec<&String> = instance.fields.iter().map(|(k, _)| k).collect();
        present.sort();
        panic!(
            "{}.{name} field missing; present fields: {present:?}",
            instance.type_name
        )
    })
}

#[test]
fn defaulted_stdlib_fea_presets_expose_density_as_dimensioned_scalar() {
    let compiled = parse_and_compile_with_stdlib(PRESET_DENSITIES_SRC);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    for preset in &PRESETS {
        let instance = preset_instance(&result.values, preset);
        assert_dimensioned(
            field(instance, "density"),
            preset.density_si,
            DimensionVector::MASS_DENSITY,
            &format!("{}.density field", preset.type_name),
        );

        let member_access = format!("{}_rho", preset.cell);
        assert_dimensioned(
            structure_cell(&result.values, &member_access),
            preset.density_si,
            DimensionVector::MASS_DENSITY,
            &format!(
                "{PRESET_DENSITIES_STRUCTURE}.{member_access} (`{}.density` member access)",
                preset.cell
            ),
        );
    }
}
