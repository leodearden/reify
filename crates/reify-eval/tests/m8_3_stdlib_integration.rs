//! M8.3 stdlib integration tests.
//!
//! Exercises four stdlib modules — materials, ports, tolerancing, units —
//! through the full parse → compile_with_stdlib → eval pipeline using
//! .ri fixture files in examples/.
//!
//! Unlike m8_stdlib_integration.rs (M8.1: math/linalg), this file uses
//! `parse_and_compile_with_stdlib` + `SimpleConstraintChecker` because the
//! fixtures depend on stdlib traits (Material, Physical, Elastic, Strong),
//! enums (MaterialCondition, SurfaceParameter), structures (Position,
//! DimensionalTolerance), and unit aliases (1nm, 1Mm, 1in, 1psi).
//! Mirrors the m10_combined.rs / m11_field_calculus.rs eval pattern.

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, ModulePath, Severity, Type, ValueCellId};
use reify_ir::{CompiledExprKind, ExportFormat, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── File paths (resolved at compile time from this crate's root) ─────────────

const PATH_MATERIALS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m8_materials.ri"
);
const PATH_PORTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/m8_ports.ri");
const PATH_TOLERANCING: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m8_tolerancing.ri"
);
const PATH_UNITS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/m8_units.ri");

// ── Expected SI constants for imperial units (from imperial_units_tests.rs) ──

const LBF_SI: f64 = 4.4482216152605;
const PSI_SI: f64 = 6894.757293168361;
const GAL_SI: f64 = 0.003785411784;
const LB_SI: f64 = 0.45359237;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Read a .ri fixture file, parse, compile with stdlib (asserting no
/// Severity::Error diagnostics at each stage), eval with
/// `SimpleConstraintChecker`, and assert no eval errors.
/// Returns the full `EvalResult` for per-test assertions.
fn eval_ri_file(path: &str, module_name: &str) -> reify_eval::EvalResult {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{} should exist: {}", path, e));

    let parsed = reify_syntax::parse(&source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {}: {:?}",
        path,
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors in {}: {:?}",
        path,
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in {}: {:?}",
        path,
        eval_errors
    );

    result
}

/// Read a .ri fixture file, parse, and compile with stdlib.
/// Panics if there are any Severity::Error diagnostics.
/// Returns the CompiledModule for compile-level assertions.
fn compiled_ri(path: &str) -> CompiledModule {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
    parse_and_compile_with_stdlib(&source)
}

// ── OCCT-build helpers (GHR-ζ mass-revival) ──────────────────────────────────

/// Compile `source` with stdlib (asserting no error-severity diagnostics), then —
/// if OCCT is available — build through a real-OCCT `Engine` and return the
/// `BuildResult`. Returns `None` when OCCT is unavailable (caller skips numeric
/// assertions). Mirrors `compile_and_build_occt` in geometry_query_kernel_dispatch.rs.
fn compile_and_build_occt(source: &str) -> Option<reify_eval::BuildResult> {
    let compiled = parse_and_compile_with_stdlib(source);
    {
        let errs: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errs.is_empty(),
            "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
            errs
        );
    }
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return None;
    }
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(&compiled, ExportFormat::Step))
}

/// Assert `value` is a `Value::Scalar` of dimension `dim` whose `si_value` is
/// within 1e-6 relative of `expected`.
fn assert_scalar_rel(value: Option<&Value>, dim: DimensionVector, expected: f64, what: &str) {
    match value {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension, dim,
                "{what}: expected dimension {dim:?}, got {dimension:?}"
            );
            let rel = (si_value - expected).abs() / expected.abs();
            assert!(
                rel < 1e-6,
                "{what}: si_value {si_value:.12} not within 1e-6 relative of \
                 {expected:.12} (rel={rel:.3e})"
            );
        }
        other => panic!("{what}: expected Value::Scalar{{{dim:?}}}, got {other:?}"),
    }
}

/// Read the runtime `density` (SI kg·m⁻³) from a structure's evaluated
/// `material` StructureInstance cell. Lets expected mass track the actual
/// material constant rather than a hardcoded literal.
fn material_density_si(result: &reify_eval::BuildResult, structure: &str) -> f64 {
    match result.values.get(&ValueCellId::new(structure, "material")) {
        Some(Value::StructureInstance(data)) => match data.fields.get("density") {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("{structure}.material.density should be Scalar, got {other:?}"),
        },
        other => panic!("{structure}.material should be StructureInstance, got {other:?}"),
    }
}

// ── Section 1: m8_materials.ri ───────────────────────────────────────────────

/// Smoke test: m8_materials.ri parses, compiles (stdlib), evals without errors,
/// and produces non-empty values.
#[test]
fn m8_materials_smoke() {
    let result = eval_ri_file(PATH_MATERIALS, "m8_materials");
    assert!(
        !result.values.is_empty(),
        "m8_materials.ri eval should produce non-empty values"
    );
}

// ── step-3: materials_bracket_mass_computed ───────────────────────────────────

/// `AluminumBracket.mass` folds via the landed GHR-ζ dispatch (task 3608):
/// `mass = volume(geometry) * material.density`, where
///   - `geometry = box(20mm, 20mm, 10mm)` → analytic volume 4e-6 m³
///   - `material.density ≈ 2700 kg/m³` (runtime-read from the `material` StructureInstance slot)
///     → `mass ≈ 0.0108 kg` (`Value::Scalar<MASS>`, rel < 1e-6).
///
/// Requires the real-OCCT `Engine::build()` path — `post_process_geometry_queries` runs
/// only on the build path with a registered kernel. Skips cleanly when OCCT is unavailable.
#[test]
fn materials_bracket_mass_computed() {
    let source = std::fs::read_to_string(PATH_MATERIALS)
        .unwrap_or_else(|e| panic!("{} should exist: {}", PATH_MATERIALS, e));
    let Some(result) = compile_and_build_occt(&source) else {
        return;
    };
    let box_v = 0.020 * 0.020 * 0.010; // 4e-6 m³: box(20mm, 20mm, 10mm)
    let density = material_density_si(&result, "AluminumBracket");
    assert_scalar_rel(
        result.values.get(&ValueCellId::new("AluminumBracket", "mass")),
        DimensionVector::MASS,
        box_v * density,
        "AluminumBracket.mass",
    );
}

// ── step-5: materials_trait_conformance_checks ───────────────────────────────

/// Asserts the compiled AluminumBracket template has trait_bounds including
/// Physical, Elastic, and Strong (from the `: Physical + Elastic + Strong`
/// declaration), and at least 2 constraints injected by trait refinement
/// (Physical.volume > 0, Strong.ultimate_tensile_strength >= yield_strength).
#[test]
fn materials_trait_conformance_checks() {
    let compiled: CompiledModule = compiled_ri(PATH_MATERIALS);

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "AluminumBracket")
        .expect("AluminumBracket template should exist in compiled module");

    // trait_bounds from the `: Physical + Elastic + Strong` header
    assert!(
        template.trait_bounds.contains(&"Physical".to_string()),
        "AluminumBracket should have 'Physical' trait bound, got: {:?}",
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"Elastic".to_string()),
        "AluminumBracket should have 'Elastic' trait bound, got: {:?}",
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"Strong".to_string()),
        "AluminumBracket should have 'Strong' trait bound, got: {:?}",
        template.trait_bounds
    );

    // At least 2 constraints injected by trait refinement:
    //   Physical: constraint volume > 0
    //   Strong:   constraint ultimate_tensile_strength >= yield_strength
    assert!(
        template.constraints.len() >= 2,
        "AluminumBracket should have >= 2 constraints (from Physical + Strong trait refinements), got: {}",
        template.constraints.len()
    );
}

// ── Section 2: m8_ports.ri ───────────────────────────────────────────────────

/// Smoke test: m8_ports.ri parses, compiles (stdlib), evals without errors,
/// and produces non-empty values.
#[test]
fn m8_ports_smoke() {
    let result = eval_ri_file(PATH_PORTS, "m8_ports");
    assert!(
        !result.values.is_empty(),
        "m8_ports.ri eval should produce non-empty values"
    );
}

// ── step-7: ports_rotary_connection_compiles ─────────────────────────────────

/// Asserts the compiled DriveTrain assembly has >= 1 connection linking
/// motor.shaft → gearbox.input. Also asserts:
///   - Motor template has a "shaft" port with type_name "RotaryPort"
///   - Motor template has a "mount" port with type_name "ThreadedPort"
///   - Gearbox template has an "input" port with type_name "RotaryPort"
#[test]
fn ports_rotary_connection_compiles() {
    let compiled: CompiledModule = compiled_ri(PATH_PORTS);

    // --- DriveTrain assembly: connection motor.shaft → gearbox.input ---
    let drivetrain = compiled
        .templates
        .iter()
        .find(|t| t.name == "DriveTrain")
        .expect("DriveTrain template should exist in compiled m8_ports module");

    assert!(
        !drivetrain.connections.is_empty(),
        "DriveTrain should have >= 1 connection"
    );

    assert!(
        drivetrain
            .connections
            .iter()
            .any(|c| c.left_port == "motor.shaft" && c.right_port == "gearbox.input"),
        "DriveTrain should have a connection from motor.shaft to gearbox.input"
    );

    // --- Motor template: shaft port (RotaryPort) + mount port (ThreadedPort) ---
    let motor = compiled
        .templates
        .iter()
        .find(|t| t.name == "Motor")
        .expect("Motor template should exist");

    let shaft_port = motor
        .ports
        .iter()
        .find(|p| p.name == "shaft")
        .expect("Motor should have a 'shaft' port");
    assert_eq!(
        shaft_port.type_name, "RotaryPort",
        "Motor.shaft port type_name should be 'RotaryPort', got '{}'",
        shaft_port.type_name
    );

    let mount_port = motor
        .ports
        .iter()
        .find(|p| p.name == "mount")
        .expect("Motor should have a 'mount' port");
    assert_eq!(
        mount_port.type_name, "ThreadedPort",
        "Motor.mount port type_name should be 'ThreadedPort', got '{}'",
        mount_port.type_name
    );

    // --- Gearbox template: input port (RotaryPort) ---
    let gearbox = compiled
        .templates
        .iter()
        .find(|t| t.name == "Gearbox")
        .expect("Gearbox template should exist");

    let input_port = gearbox
        .ports
        .iter()
        .find(|p| p.name == "input")
        .expect("Gearbox should have an 'input' port");
    assert_eq!(
        input_port.type_name, "RotaryPort",
        "Gearbox.input port type_name should be 'RotaryPort', got '{}'",
        input_port.type_name
    );
}

// ── step-9: ports_threaded_m8_port_values ────────────────────────────────────

/// Asserts the compiled Motor template's mount port has the correct M8 thread
/// dimensions in its port member default_exprs (compile-level assertion):
///   - thread_major_dia : default_expr Literal(Scalar(0.008 m, LENGTH))  = 8mm
///   - thread_pitch     : default_expr Literal(Scalar(0.00125 m, LENGTH)) = 1.25mm
#[test]
fn ports_threaded_m8_port_values() {
    let compiled = compiled_ri(PATH_PORTS);

    let motor = compiled
        .templates
        .iter()
        .find(|t| t.name == "Motor")
        .expect("Motor template should exist");

    let mount_port = motor
        .ports
        .iter()
        .find(|p| p.name == "mount")
        .expect("Motor should have a 'mount' port");

    // ── thread_major_dia (8mm = 0.008 m) ──────────────────────────────────────
    let major_dia_member = mount_port
        .members
        .iter()
        .find(|m| m.id.member.contains("thread_major_dia"))
        .expect("mount port should have a 'thread_major_dia' member");

    let major_expr = major_dia_member
        .default_expr
        .as_ref()
        .expect("thread_major_dia should have a default_expr");

    match &major_expr.kind {
        CompiledExprKind::Literal(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert!(
                (si_value - 0.008).abs() < 1e-9,
                "thread_major_dia should be 0.008m (8mm), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "thread_major_dia should have LENGTH dimension"
            );
        }
        other => panic!(
            "thread_major_dia default_expr should be Scalar literal, got {:?}",
            other
        ),
    }

    // ── thread_pitch (1.25mm = 0.00125 m) ────────────────────────────────────
    let pitch_member = mount_port
        .members
        .iter()
        .find(|m| m.id.member.contains("thread_pitch"))
        .expect("mount port should have a 'thread_pitch' member");

    let pitch_expr = pitch_member
        .default_expr
        .as_ref()
        .expect("thread_pitch should have a default_expr");

    match &pitch_expr.kind {
        CompiledExprKind::Literal(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert!(
                (si_value - 0.00125).abs() < 1e-9,
                "thread_pitch should be 0.00125m (1.25mm), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "thread_pitch should have LENGTH dimension"
            );
        }
        other => panic!(
            "thread_pitch default_expr should be Scalar literal, got {:?}",
            other
        ),
    }
}

// ── Section 3: m8_tolerancing.ri ─────────────────────────────────────────────

/// Smoke test: m8_tolerancing.ri parses, compiles (stdlib), evals without errors,
/// and produces non-empty values.
#[test]
fn m8_tolerancing_smoke() {
    let result = eval_ri_file(PATH_TOLERANCING, "m8_tolerancing");
    assert!(
        !result.values.is_empty(),
        "m8_tolerancing.ri eval should produce non-empty values"
    );
}

// ── task #3116 step-7: tolerancing_m8_feature_type_is_geometry (RED) ─────────

/// RED (step-7): m8_tolerancing.ri's local `Position` and `Flatness` struct
/// definitions must declare `feature` with `Type::Geometry` (not `Type::Real`).
///
/// Assertions:
///   - `Position.feature`  cell_type == `Type::Geometry`
///   - `Flatness.feature`  cell_type == `Type::Geometry`
///
/// Fails before step-8 because both local structs still bind
/// `param feature : Real = 0.0` → cell_type is `Type::Real`.
/// Passes after step-8 flips them to `param feature : Geometry`.
#[test]
fn tolerancing_m8_feature_type_is_geometry() {
    let module = compiled_ri(PATH_TOLERANCING);

    // ── Position.feature must be Type::Geometry ───────────────────────────────
    let position = module
        .templates
        .iter()
        .find(|t| t.name == "Position")
        .expect("Position template should exist in compiled m8_tolerancing");
    let pos_feature = position
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "feature")
        .expect("Position must have a 'feature' value cell");
    assert_eq!(
        pos_feature.cell_type,
        Type::Geometry,
        "Position.feature must be Type::Geometry (not {:?}) — step-8 flips m8_tolerancing.ri",
        pos_feature.cell_type
    );

    // ── Flatness.feature must be Type::Geometry ───────────────────────────────
    let flatness = module
        .templates
        .iter()
        .find(|t| t.name == "Flatness")
        .expect("Flatness template should exist in compiled m8_tolerancing");
    let flat_feature = flatness
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "feature")
        .expect("Flatness must have a 'feature' value cell");
    assert_eq!(
        flat_feature.cell_type,
        Type::Geometry,
        "Flatness.feature must be Type::Geometry (not {:?}) — step-8 flips m8_tolerancing.ri",
        flat_feature.cell_type
    );
}

// ── step-11: tolerancing_position_mmc_flatness_ra ────────────────────────────

/// Asserts the compiled Flange template from m8_tolerancing.ri has:
///   - sub pos (Position): tolerance_value=0.1mm (0.0001m), material_condition=MMC
///   - sub flat (Flatness): tolerance_value=0.05mm (0.00005m)
///   - sub finish (SurfaceFinish): parameter=Ra, value=1.6μm (1.6e-6m)
///     Tests both compile-level (template exists) and eval-level (values resolved).
#[test]
fn tolerancing_position_mmc_flatness_ra() {
    let result = eval_ri_file(PATH_TOLERANCING, "m8_tolerancing");

    // ── Flange.pos.tolerance_value (0.1mm = 0.0001m, LENGTH) ─────────────────
    let pos_tol_id = ValueCellId::new("Flange.pos", "tolerance_value");
    let pos_tol_val = result
        .values
        .get(&pos_tol_id)
        .unwrap_or_else(|| panic!("Flange.pos.tolerance_value not found in eval result"));
    match pos_tol_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.0001).abs() < 1e-9,
                "Flange.pos.tolerance_value should be 0.0001m (0.1mm), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "Flange.pos.tolerance_value should have LENGTH dimension"
            );
        }
        other => panic!(
            "Flange.pos.tolerance_value should be Value::Scalar, got {:?}",
            other
        ),
    }

    // ── Flange.pos.material_condition (MaterialCondition.MMC) ─────────────────
    let pos_mc_id = ValueCellId::new("Flange.pos", "material_condition");
    let pos_mc_val = result
        .values
        .get(&pos_mc_id)
        .unwrap_or_else(|| panic!("Flange.pos.material_condition not found"));
    match pos_mc_val {
        Value::Enum { variant, .. } => {
            assert_eq!(
                variant, "MMC",
                "Flange.pos.material_condition should be MMC, got {}",
                variant
            );
        }
        other => panic!(
            "Flange.pos.material_condition should be Value::Enum, got {:?}",
            other
        ),
    }

    // ── Flange.flat.tolerance_value (0.05mm = 0.00005m, LENGTH) ──────────────
    let flat_tol_id = ValueCellId::new("Flange.flat", "tolerance_value");
    let flat_tol_val = result
        .values
        .get(&flat_tol_id)
        .unwrap_or_else(|| panic!("Flange.flat.tolerance_value not found"));
    match flat_tol_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.00005).abs() < 1e-12,
                "Flange.flat.tolerance_value should be 0.00005m (0.05mm), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "Flange.flat.tolerance_value should have LENGTH dimension"
            );
        }
        other => panic!(
            "Flange.flat.tolerance_value should be Value::Scalar, got {:?}",
            other
        ),
    }

    // ── Flange.finish.parameter (SurfaceParameter.Ra) ────────────────────────
    let finish_param_id = ValueCellId::new("Flange.finish", "parameter");
    let finish_param_val = result
        .values
        .get(&finish_param_id)
        .unwrap_or_else(|| panic!("Flange.finish.parameter not found"));
    match finish_param_val {
        Value::Enum { variant, .. } => {
            assert_eq!(
                variant, "Ra",
                "Flange.finish.parameter should be Ra, got {}",
                variant
            );
        }
        other => panic!(
            "Flange.finish.parameter should be Value::Enum, got {:?}",
            other
        ),
    }

    // ── Flange.finish.value (1.6μm = 1.6e-6m, LENGTH) ────────────────────────
    let finish_val_id = ValueCellId::new("Flange.finish", "value");
    let finish_val = result
        .values
        .get(&finish_val_id)
        .unwrap_or_else(|| panic!("Flange.finish.value not found"));
    match finish_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 1.6e-6).abs() < 1e-15,
                "Flange.finish.value should be 1.6e-6m (1.6μm), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "Flange.finish.value should have LENGTH dimension"
            );
        }
        other => panic!(
            "Flange.finish.value should be Value::Scalar, got {:?}",
            other
        ),
    }
}

// ── step-13: tolerancing_dimensional_bounds_computed ─────────────────────────

/// Asserts the computed lets in the Flange.dim_tol sub (DimensionalTolerance)
/// evaluate to the expected Scalar si_values:
///   nominal=50mm, upper_deviation=+0.1mm, lower_deviation=-0.1mm
///   → upper_limit  = 50mm + 0.1mm  = 50.1mm  (0.0501m)
///   → lower_limit  = 50mm - 0.1mm  = 49.9mm  (0.0499m)
///   → tolerance_band = 0.1mm - (-0.1mm) = 0.2mm (0.0002m)
/// All dimension = LENGTH.  Confirms sub-component computed lets are correctly
/// elaborated in child scope "Flange.dim_tol".
#[test]
fn tolerancing_dimensional_bounds_computed() {
    let result = eval_ri_file(PATH_TOLERANCING, "m8_tolerancing");

    // ── upper_limit (50mm + 0.1mm = 0.0501m) ─────────────────────────────────
    let ul_id = ValueCellId::new("Flange.dim_tol", "upper_limit");
    let ul_val = result
        .values
        .get(&ul_id)
        .unwrap_or_else(|| panic!("Flange.dim_tol.upper_limit not found in eval result"));
    match ul_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.0501).abs() < 1e-9,
                "Flange.dim_tol.upper_limit should be ≈0.0501m, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "Flange.dim_tol.upper_limit should have LENGTH dimension"
            );
        }
        other => panic!(
            "Flange.dim_tol.upper_limit should be Value::Scalar, got {:?}",
            other
        ),
    }

    // ── lower_limit (50mm - 0.1mm = 0.0499m) ─────────────────────────────────
    let ll_id = ValueCellId::new("Flange.dim_tol", "lower_limit");
    let ll_val = result
        .values
        .get(&ll_id)
        .unwrap_or_else(|| panic!("Flange.dim_tol.lower_limit not found in eval result"));
    match ll_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.0499).abs() < 1e-9,
                "Flange.dim_tol.lower_limit should be ≈0.0499m, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "Flange.dim_tol.lower_limit should have LENGTH dimension"
            );
        }
        other => panic!(
            "Flange.dim_tol.lower_limit should be Value::Scalar, got {:?}",
            other
        ),
    }

    // ── tolerance_band (0.1mm - (-0.1mm) = 0.2mm = 0.0002m) ─────────────────
    let tb_id = ValueCellId::new("Flange.dim_tol", "tolerance_band");
    let tb_val = result
        .values
        .get(&tb_id)
        .unwrap_or_else(|| panic!("Flange.dim_tol.tolerance_band not found in eval result"));
    match tb_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.0002).abs() < 1e-9,
                "Flange.dim_tol.tolerance_band should be ≈0.0002m (0.2mm), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "Flange.dim_tol.tolerance_band should have LENGTH dimension"
            );
        }
        other => panic!(
            "Flange.dim_tol.tolerance_band should be Value::Scalar, got {:?}",
            other
        ),
    }
}

// ── Section 4: m8_units.ri ───────────────────────────────────────────────────

/// Smoke test: m8_units.ri parses, compiles (stdlib), evals without errors,
/// and produces non-empty values.
#[test]
fn m8_units_smoke() {
    let result = eval_ri_file(PATH_UNITS, "m8_units");
    assert!(
        !result.values.is_empty(),
        "m8_units.ri eval should produce non-empty values"
    );
}

// ── step-15: units_si_prefix_coverage ────────────────────────────────────────

/// Helper: fetch a Scalar cell from the eval result and assert si_value + dimension.
fn assert_scalar_cell(
    result: &reify_eval::EvalResult,
    entity: &str,
    member: &str,
    expected_si: f64,
    tolerance: f64,
    expected_dim: DimensionVector,
) {
    let id = ValueCellId::new(entity, member);
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("{}.{} not found in eval result", entity, member));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - expected_si).abs() < tolerance,
                "{}.{}: expected si_value ≈{}, got {}",
                entity,
                member,
                expected_si,
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "{}.{}: wrong dimension",
                entity, member
            );
        }
        other => panic!(
            "{}.{} should be Value::Scalar, got {:?}",
            entity, member, other
        ),
    }
}

/// Asserts that SI-prefix literals for length, mass, and time all resolve to
/// the correct si_value (in the SI base unit) and dimension in the eval result.
///
/// Length prefix coverage (SI base = metre):
///   1nm → 1e-9 m, 1um → 1e-6 m, 1mm → 1e-3 m, 1km → 1e3 m, 1Mm → 1e6 m
///
/// Mass prefix coverage (SI base = kilogram):
///   1mg → 1e-6 kg, 1kg → 1.0 kg, 1Mg → 1e3 kg
///
/// Time prefix coverage (SI base = second):
///   1ns → 1e-9 s, 1us → 1e-6 s, 1ms → 1e-3 s, 1ks → 1e3 s
#[test]
fn units_si_prefix_coverage() {
    let result = eval_ri_file(PATH_UNITS, "m8_units");
    let e = "UnitShowcase";
    let l = DimensionVector::LENGTH;
    let m = DimensionVector::MASS;
    let t = DimensionVector::TIME;

    // ── Length ────────────────────────────────────────────────────────────────
    assert_scalar_cell(&result, e, "len_nm", 1e-9, 1e-20, l);
    assert_scalar_cell(&result, e, "len_um", 1e-6, 1e-17, l);
    assert_scalar_cell(&result, e, "len_mm", 1e-3, 1e-14, l);
    assert_scalar_cell(&result, e, "len_km", 1e3, 1e-6, l);
    assert_scalar_cell(&result, e, "len_Mm", 1e6, 1e-3, l);

    // ── Mass ──────────────────────────────────────────────────────────────────
    assert_scalar_cell(&result, e, "mass_mg", 1e-6, 1e-17, m);
    assert_scalar_cell(&result, e, "mass_kg", 1.0, 1e-9, m);
    assert_scalar_cell(&result, e, "mass_Mg", 1e3, 1e-6, m);

    // ── Time ──────────────────────────────────────────────────────────────────
    assert_scalar_cell(&result, e, "time_ns", 1e-9, 1e-20, t);
    assert_scalar_cell(&result, e, "time_us", 1e-6, 1e-17, t);
    assert_scalar_cell(&result, e, "time_ms", 1e-3, 1e-14, t);
    assert_scalar_cell(&result, e, "time_ks", 1e3, 1e-6, t);
}

// ── step-17: units_imperial_conversions ──────────────────────────────────────

/// Asserts that imperial unit literals resolve to the expected SI values and
/// dimensions in the eval result. Expected conversion factors:
///   1in  → LENGTH  0.0254 m
///   1ft  → LENGTH  0.3048 m
///   1yd  → LENGTH  0.9144 m
///   1lb  → MASS    0.45359237 kg
///   1lbf → FORCE   4.4482216152605 N
///   1psi → PRESSURE 6894.757293168361 Pa
///   1gal → VOLUME   0.003785411784 m³
///
/// Values live in UnitShowcase as `let len_in = 1in`, `let mass_lb = 1lb`, etc.
/// Cross-checks with the exact factors from `crates/reify-compiler/stdlib/units.ri`.
#[test]
fn units_imperial_conversions() {
    let result = eval_ri_file(PATH_UNITS, "m8_units");
    let e = "UnitShowcase";
    let l = DimensionVector::LENGTH;
    let m = DimensionVector::MASS;
    let f = DimensionVector::FORCE;
    let p = DimensionVector::PRESSURE;
    let v = DimensionVector::VOLUME;

    // ── Length ────────────────────────────────────────────────────────────────
    assert_scalar_cell(&result, e, "len_in", 0.0254, 1e-13, l);
    assert_scalar_cell(&result, e, "len_ft", 0.3048, 1e-12, l);
    assert_scalar_cell(&result, e, "len_yd", 0.9144, 1e-12, l);

    // ── Mass ──────────────────────────────────────────────────────────────────
    assert_scalar_cell(&result, e, "mass_lb", LB_SI, 1e-12, m);

    // ── Force ─────────────────────────────────────────────────────────────────
    assert_scalar_cell(&result, e, "force_lbf", LBF_SI, 1e-10, f);

    // ── Pressure ──────────────────────────────────────────────────────────────
    assert_scalar_cell(&result, e, "pressure_psi", PSI_SI, 1e-6, p);

    // ── Volume ────────────────────────────────────────────────────────────────
    assert_scalar_cell(&result, e, "volume_gal", GAL_SI, 1e-13, v);
}

// ── step-19: units_cross_system_arithmetic ───────────────────────────────────

/// Asserts three mixed-unit arithmetic expressions in UnitShowcase evaluate
/// correctly across the SI/imperial boundary:
///
///   mixed_len      = 1in + 25.4mm   → LENGTH ≈ 0.0508m
///     (1in = 0.0254m, 25.4mm = 0.0254m, sum = 0.0508m)
///
///   energy_imperial = 2lbf * 3mm    → ENERGY ≈ 0.026689329691563 J
///     (2 × 4.4482216152605 N × 0.003 m = 0.026689329691563 J)
///
///   pressure_ratio = (1psi) / (1Pa) → DIMENSIONLESS ≈ 6894.757293168361
///     (Pressure/Pressure = dimensionless; 6894.757.../1.0 = 6894.757...)
///
/// Exercises that dimension promotion is correct across the imperial/SI boundary.
#[test]
fn units_cross_system_arithmetic() {
    let result = eval_ri_file(PATH_UNITS, "m8_units");
    let e = "UnitShowcase";

    // ── mixed_len: 1in + 25.4mm = 0.0508 m (LENGTH) ──────────────────────────
    assert_scalar_cell(
        &result,
        e,
        "mixed_len",
        0.0508,
        1e-12,
        DimensionVector::LENGTH,
    );

    // ── energy_imperial: 2lbf * 3mm = 2 * LBF_SI * 0.003 J (ENERGY) ─────────
    let expected_energy = 2.0 * LBF_SI * 0.003; // ≈ 0.026689329691563 J
    assert_scalar_cell(
        &result,
        e,
        "energy_imperial",
        expected_energy,
        1e-11,
        DimensionVector::ENERGY,
    );

    // ── pressure_ratio: (1psi) / (1Pa) = dimensionless ≈ 6894.757 ────────────
    assert_scalar_cell(
        &result,
        e,
        "pressure_ratio",
        PSI_SI,
        1e-6,
        DimensionVector::DIMENSIONLESS,
    );
}
