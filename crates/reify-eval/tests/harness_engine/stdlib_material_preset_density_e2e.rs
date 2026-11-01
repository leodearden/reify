//! A defaulted stdlib FEA material preset exposes `density` to evaluation as a
//! dimensioned `Value::Scalar`, on the solver-free path and under both solving
//! engine wirings (task #9572).
//!
//! The only other reader of a defaulted preset's `density` on the solver path
//! is the heavy, gate-excluded bracket e2e (`harness_fea_solver_e2e`), and the
//! goldens that print `density` are rewritten wholesale by
//! `REIFY_REGENERATE_GOLDEN=1`. These pins assert exact values instead: a
//! preset that stops carrying `density` fails the merge gate and names the
//! field, and an FEA-free twin of the bracket's `minimize mass` loop reports an
//! Undef density as an unresolved `thickness` without running the FEA solve.
//!
//! Lives under `harness_engine/` for the anti-re-accretion reason recorded in
//! `dimensioned_ctor_migration_si_values.rs`.

use reify_constraints::{DimensionalSolver, SimpleConstraintChecker};
use reify_core::{DimensionVector, ValueCellId};
use reify_eval::{Engine, EvalResult, SolverPosture};
use reify_ir::{ResolutionProfile, StructureInstanceData, Value, ValueMap};
use reify_test_support::{
    MockConstraintChecker, assert_dimensioned, make_simple_engine, parse_and_compile_with_stdlib,
    scalar_si,
};

/// `Steel_AISI_1045`'s `param density` default in `materials_fea.ri`, in SI kg/m^3.
const STEEL_DENSITY_SI: f64 = 7850.0;

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
        density_si: STEEL_DENSITY_SI,
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

const DENSITY_OBJECTIVE_STRUCTURE: &str = "PresetDensityObjective";

/// The bracket design loop with the FEA constraint replaced by a plain bound.
const DENSITY_OBJECTIVE_SRC: &str = r#"structure PresetDensityObjective {
    param length : Length = 50mm
    param width : Length = 30mm
    param thickness : Length = auto(free)
    let material = Steel_AISI_1045()
    let rho = material.density
    let mass = material.density * length * width * thickness
    constraint thickness > 3mm
    minimize mass
}"#;

const MASS_COHERENCE_REL_TOL: f64 = 1e-9;

const SOLVE_FRESH: SolverPosture = SolverPosture::Solve(ResolutionProfile::FRESH);

/// `reify eval`'s non-geometry engine (`reify-cli`'s `cmd/eval.rs`), minus its
/// cache and compute-trampoline configuration.
fn cli_eval_engine() -> Engine {
    Engine::new(SOLVE_FRESH, Box::new(SimpleConstraintChecker), None)
}

/// The engine shape of `harness_fea_solver_e2e`'s `fea_loop_engine` (another
/// test binary, so not importable) minus its FEA trampolines, which this
/// fixture has no `@optimized` call to use.
fn fea_design_loop_engine() -> Engine {
    Engine::new(SOLVE_FRESH, Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver))
}

type NamedEngine = (&'static str, fn() -> Engine);

const SOLVING_ENGINES: [NamedEngine; 2] = [
    ("reify-cli eval wiring", cli_eval_engine),
    ("FEA design-loop harness wiring", fea_design_loop_engine),
];

#[test]
fn minimize_objective_reading_defaulted_preset_density_resolves_auto_thickness() {
    let compiled = parse_and_compile_with_stdlib(DENSITY_OBJECTIVE_SRC);

    for (arm, build_engine) in SOLVING_ENGINES {
        let mut engine = build_engine();
        let result = engine.eval(&compiled);
        assert_auto_thickness_resolved_through_preset_density(arm, &result);
    }
}

fn assert_auto_thickness_resolved_through_preset_density(arm: &str, result: &EvalResult) {
    let cell = |member: &str| -> Value {
        result
            .values
            .get(&ValueCellId::new(DENSITY_OBJECTIVE_STRUCTURE, member))
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "[{arm}] {DENSITY_OBJECTIVE_STRUCTURE}.{member} cell missing from eval result"
                )
            })
    };
    let resolved_si = |member: &str, dimension: DimensionVector| -> f64 {
        let value = cell(member);
        let si = scalar_si(
            &value,
            &format!(
                "[{arm}] {DENSITY_OBJECTIVE_STRUCTURE}.{member} is not a resolved Scalar (an \
                 Undef objective, which leaves `thickness` unresolved, is the #9572 signature); \
                 eval diagnostics: {:#?}",
                result.diagnostics
            ),
        );
        assert_eq!(
            value.dimension(),
            dimension,
            "[{arm}] {DENSITY_OBJECTIVE_STRUCTURE}.{member} has the wrong dimension"
        );
        si
    };

    assert_dimensioned(
        &cell("rho"),
        STEEL_DENSITY_SI,
        DimensionVector::MASS_DENSITY,
        &format!("[{arm}] {DENSITY_OBJECTIVE_STRUCTURE}.rho (`material.density` member access)"),
    );

    let thickness_si = resolved_si("thickness", DimensionVector::LENGTH);
    assert!(
        thickness_si.is_finite(),
        "[{arm}] resolved thickness must be finite, got {thickness_si:?}"
    );

    let mass_si = resolved_si("mass", DimensionVector::MASS);
    assert!(
        mass_si.is_finite() && mass_si > 0.0,
        "[{arm}] mass must be finite and positive, got {mass_si:?}"
    );

    let length_si = resolved_si("length", DimensionVector::LENGTH);
    let width_si = resolved_si("width", DimensionVector::LENGTH);
    let expected_mass_si = STEEL_DENSITY_SI * length_si * width_si * thickness_si;
    let rel_err = (mass_si - expected_mass_si).abs() / expected_mass_si;
    assert!(
        rel_err < MASS_COHERENCE_REL_TOL,
        "[{arm}] mass ({mass_si:.9e} kg) should equal density*length*width*thickness \
         ({expected_mass_si:.9e} kg) at the resolved length {length_si:.6e} m, width \
         {width_si:.6e} m and thickness {thickness_si:.6e} m; relative error {rel_err:.3e}. \
         Every factor but the stdlib density is read back from the resolved cells, so a \
         mismatch means the objective is not the closed-form mass the fixture declares."
    );
}
