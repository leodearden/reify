// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration gate: as-printed vs homogeneous FEA solve on `examples/fdm_bracket.ri`
//! (task #3787 ε — FDM ε done-gate).
//!
//! Proves the full parse → compile → build → eval pipeline delivers the
//! user-observable headline signal: the as-printed bracket deflects strictly MORE
//! than a homogeneous solid-ABS baseline, and the build-Z direction is materially
//! weaker than in-plane.
//!
//! This file is the sibling integration gate to `differential_field_ops_e2e.rs`
//! (the G2 gate) and mirrors its structure.
//!
//! All tests are gated on `reify_kernel_occt::OCCT_AVAILABLE`.

mod common;

use reify_core::{ContentHash, Severity, Type, ValueCellId};
use reify_ir::{
    CompiledExpr, CompiledExprKind, ExportFormat, FieldSourceKind, ResolvedFunction, Satisfaction,
    Value, ValueMap,
};
use reify_eval::compute_targets::register_compute_fns;
use reify_expr::{EvalContext, eval_expr};

// ── shared helpers ────────────────────────────────────────────────────────────

fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

fn point3_val(p: [f64; 3]) -> Value {
    Value::Point(vec![
        common::as_printed::length(p[0]),
        common::as_printed::length(p[1]),
        common::as_printed::length(p[2]),
    ])
}

fn sample_call(field: Value, at: Value) -> CompiledExpr {
    let field_type = Type::Field {
        domain: Box::new(Type::point3(Type::length())),
        codomain: Box::new(Type::StructureRef("AnisotropicMaterial".to_string())),
    };
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "sample".to_string(),
                qualified_name: "std::sample".to_string(),
            },
            args: vec![
                CompiledExpr::literal(field, field_type),
                CompiledExpr::literal(at, Type::point3(Type::length())),
            ],
        },
        result_type: Type::StructureRef("AnisotropicMaterial".to_string()),
        content_hash: ContentHash::of(b"sample"),
    }
}

/// Sample `field` at SI-metre coordinates `at` through the public evaluator.
fn sample_at(field: &Value, at: [f64; 3]) -> Value {
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    eval_expr(&sample_call(field.clone(), point3_val(at)), &ctx)
}

/// Read a modulus constant from a sampled `AnisotropicMaterial`.
fn law_constant(aniso: &Value, key: &str) -> f64 {
    let Value::StructureInstance(data) = aniso else {
        panic!("expected AnisotropicMaterial StructureInstance, got {aniso:?}");
    };
    let law = data.fields.get("law").expect("AnisotropicMaterial.law");
    let Value::StructureInstance(law) = law else {
        panic!("expected law StructureInstance, got {law:?}");
    };
    match law.fields.get(key) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected law.{key} to be a Scalar, got {other:?}"),
    }
}

/// Extract stride-3 displacement data from a `Value::Field { source: Sampled }`.
fn extract_displacement_data(disp_field: &Value) -> Vec<f64> {
    match disp_field {
        Value::Field { source: FieldSourceKind::Sampled, lambda, .. } => match lambda.as_ref() {
            Value::SampledField(sf) => sf.data.clone(),
            other => panic!("expected SampledField lambda, got {other:?}"),
        },
        other => panic!("expected sampled Value::Field for displacement, got {other:?}"),
    }
}

// ── test 1: as-printed deflects more than homogeneous ─────────────────────────

/// Main deflection-Δ gate: as-printed bracket must deflect strictly more than a
/// homogeneous solid-ABS baseline.
///
/// RED: `fdm_bracket.ri` has no `r_print`/`r_solid` bindings → value-cell
///      lookup panics.
/// GREEN (step-2): example extended with both solve calls.
#[test]
fn as_printed_deflects_more_than_homogeneous() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping as_printed_deflects_more_than_homogeneous: OCCT not available");
        return;
    }

    let path = format!("{}/../../examples/fdm_bracket.ri", env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read examples/fdm_bracket.ri: {e}"));

    let compiled = reify_test_support::parse_and_compile_with_stdlib(&source);

    // (a) zero Error-severity diagnostics
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "fdm_bracket.ri must compile with zero error diagnostics; got: {errors:#?}"
    );

    let mut engine = make_occt_engine();
    register_compute_fns(&mut engine);
    engine.build(&compiled, ExportFormat::Step);

    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after build()")
        .snapshot
        .clone();

    // (b) r_print and r_solid are converged ElasticResult StructureInstances
    let get_elastic_result = |cell_name: &str| -> Value {
        let cell = ValueCellId::new("FdmBracket", cell_name);
        let (value, _) = snapshot
            .values
            .get(&cell)
            .unwrap_or_else(|| panic!("FdmBracket.{cell_name} not found in snapshot — \
                add `let {cell_name} = solve_elastic_static(...)` to fdm_bracket.ri"));
        match value {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.type_name, "ElasticResult",
                    "FdmBracket.{cell_name} must be an ElasticResult, got {:?}",
                    data.type_name
                );
                assert_eq!(
                    data.fields.get("converged"),
                    Some(&Value::Bool(true)),
                    "FdmBracket.{cell_name} must have converged=true"
                );
                value.clone()
            }
            other => panic!(
                "FdmBracket.{cell_name} must be a StructureInstance, got {other:?}"
            ),
        }
    };

    let r_print = get_elastic_result("r_print");
    let r_solid = get_elastic_result("r_solid");

    // (c) as-printed deflects strictly more than homogeneous, by > 0.1% relative
    let get_displacement = |result: &Value, label: &str| -> Value {
        match result {
            Value::StructureInstance(data) => data
                .fields
                .get("displacement")
                .unwrap_or_else(|| panic!("{label}: missing displacement field"))
                .clone(),
            other => panic!("{label}: expected StructureInstance, got {other:?}"),
        }
    };

    let disp_print = get_displacement(&r_print, "r_print");
    let disp_solid = get_displacement(&r_solid, "r_solid");

    assert!(
        matches!(&disp_print, Value::Field { source: FieldSourceKind::Sampled, .. }),
        "r_print.displacement must be a Sampled field, got {disp_print:?}"
    );
    assert!(
        matches!(&disp_solid, Value::Field { source: FieldSourceKind::Sampled, .. }),
        "r_solid.displacement must be a Sampled field, got {disp_solid:?}"
    );

    let data_print = extract_displacement_data(&disp_print);
    let data_solid = extract_displacement_data(&disp_solid);

    let defl_print = reify_eval::persistent_cache::max_deflection_magnitude(&data_print);
    let defl_solid = reify_eval::persistent_cache::max_deflection_magnitude(&data_solid);

    assert!(
        defl_print.is_finite() && defl_print > 0.0,
        "as-printed max deflection must be finite and > 0, got {defl_print}"
    );
    assert!(
        defl_solid.is_finite() && defl_solid > 0.0,
        "homogeneous max deflection must be finite and > 0, got {defl_solid}"
    );

    assert!(
        defl_print > defl_solid,
        "as-printed deflection ({defl_print:.6e}) must be strictly LARGER than homogeneous \
         ({defl_solid:.6e}): infill knockdown increases compliance"
    );

    let relative_diff = (defl_print - defl_solid) / defl_solid.max(1e-30);
    assert!(
        relative_diff > 1e-3,
        "as-printed deflection ({defl_print:.6e}) should exceed homogeneous ({defl_solid:.6e}) \
         by > 0.1% relative; got {relative_diff:.2e}"
    );

    // (d) build-Z material weakness: e_axial < e_in_plane at deep interior point
    let mat_cell = ValueCellId::new("FdmBracket", "material");
    let (mat_value, _) = snapshot
        .values
        .get(&mat_cell)
        .unwrap_or_else(|| panic!("FdmBracket.material not found in snapshot"));

    // Interior point: deep centre of the bracket body (20mm, 20mm, 5mm) in SI metres.
    // Same coordinate as `infill_query_pt` in fdm_bracket.ri.
    let interior = sample_at(mat_value, [0.020, 0.020, 0.005]);
    let e_in_plane = law_constant(&interior, "e_in_plane");
    let e_axial = law_constant(&interior, "e_axial");

    assert!(
        e_axial < e_in_plane,
        "build-Z is weaker: e_axial ({e_axial:.3e} Pa) must be < e_in_plane \
         ({e_in_plane:.3e} Pa) — transverse-iso knockdown from inter-layer bonding"
    );
}

// ── test 2: .ri-level scalar deflection values ────────────────────────────────

/// Scalar deflection gate: `defl_print` and `defl_solid` must be finite positive
/// Length values in the snapshot, and `defl_print > defl_solid`.
///
/// RED: fdm_bracket.ri has no `defl_print`/`defl_solid` lets yet.
/// GREEN (step-4): example extended with scalar reduction + constraint.
#[test]
fn ri_scalar_deflection_values_and_constraint() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping ri_scalar_deflection_values_and_constraint: OCCT not available"
        );
        return;
    }

    let path = format!("{}/../../examples/fdm_bracket.ri", env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read examples/fdm_bracket.ri: {e}"));

    let compiled = reify_test_support::parse_and_compile_with_stdlib(&source);

    // Zero Error diagnostics
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "fdm_bracket.ri must compile with zero error diagnostics; got: {errors:#?}"
    );

    let mut engine = make_occt_engine();
    register_compute_fns(&mut engine);
    // Capture build_result: post_process_derived_lets populates pure-let cells
    // (defl_print, defl_solid) in BuildResult.values but NOT in snapshot.values.
    let build_result = engine.build(&compiled, ExportFormat::Step);

    // (a) defl_print and defl_solid are finite positive Length scalars.
    // Use build_result.values (not snapshot.values) because defl_print/defl_solid
    // are pure-let reductions evaluated by post_process_derived_lets, which only
    // updates BuildResult.values, not the engine's internal snapshot.
    let get_length_scalar = |cell_name: &str| -> f64 {
        let cell = ValueCellId::new("FdmBracket", cell_name);
        match build_result.values.get(&cell) {
            Some(Value::Scalar { si_value, .. }) => {
                assert!(
                    si_value.is_finite() && *si_value > 0.0,
                    "FdmBracket.{cell_name} must be finite and > 0, got {si_value}"
                );
                *si_value
            }
            Some(other) => panic!(
                "FdmBracket.{cell_name} must be a Scalar, got {other:?}"
            ),
            None => panic!(
                "FdmBracket.{cell_name} not found — \
                add `let {cell_name} = max(r_*.displacement)` to fdm_bracket.ri"
            ),
        }
    };

    let defl_print = get_length_scalar("defl_print");
    let defl_solid = get_length_scalar("defl_solid");

    assert!(
        defl_print > defl_solid,
        "FdmBracket.defl_print ({defl_print:.6e}) must exceed \
         FdmBracket.defl_solid ({defl_solid:.6e})"
    );

    // (b) the `.ri` constraint `defl_print > defl_solid` is Satisfied
    let check_result = engine.check(&compiled);
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} must be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── test 3: committed golden ──────────────────────────────────────────────────

/// Golden gate: deterministic material-zone moduli match the committed golden file.
///
/// The golden captures ONLY the bit-stable, deterministic signal:
///   - wall zone e_in_plane, e_axial
///   - infill zone e_in_plane, e_axial
///   - qualitative `defl_print > defl_solid = true`
///
/// Raw deflection floats are NOT golden-matched (FEA is non-deterministic).
/// Run with `REIFY_UPDATE_GOLDEN=1` to regenerate.
///
/// RED: `golden/fdm_bracket.txt` does not exist yet.
/// GREEN (step-6): golden file created with `REIFY_UPDATE_GOLDEN=1` run.
#[test]
fn fdm_bracket_golden() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping fdm_bracket_golden: OCCT not available");
        return;
    }

    let path = format!("{}/../../examples/fdm_bracket.ri", env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read examples/fdm_bracket.ri: {e}"));

    let compiled = reify_test_support::parse_and_compile_with_stdlib(&source);

    {
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "fdm_bracket.ri must compile with zero error diagnostics; got: {errors:#?}"
        );
    }

    let mut engine = make_occt_engine();
    register_compute_fns(&mut engine);
    // Capture build_result for post-processed values (defl_print/defl_solid);
    // snapshot is needed for material (a compute-node output already in snapshot).
    let build_result = engine.build(&compiled, ExportFormat::Step);

    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after build()")
        .snapshot
        .clone();

    // ── sample material at wall and infill points (deterministic zone moduli) ──
    let mat_cell = ValueCellId::new("FdmBracket", "material");
    let (mat_value, _) = snapshot
        .values
        .get(&mat_cell)
        .unwrap_or_else(|| panic!("FdmBracket.material not found in snapshot"));

    // Wall point: 0.3 mm from -X face (wall zone).
    let wall = sample_at(mat_value, [0.0003, 0.020, 0.005]);
    let wall_e_in_plane = law_constant(&wall, "e_in_plane");
    let wall_e_axial = law_constant(&wall, "e_axial");

    // Infill point: deep centre of the bracket body (infill zone).
    let infill = sample_at(mat_value, [0.020, 0.020, 0.005]);
    let infill_e_in_plane = law_constant(&infill, "e_in_plane");
    let infill_e_axial = law_constant(&infill, "e_axial");

    // ── deflection comparison (qualitative boolean only — not the raw float) ──
    // Use build_result.values: defl_print/defl_solid are pure-let reductions
    // populated by post_process_derived_lets (BuildResult only, not snapshot).
    let get_deflection_scalar = |cell_name: &str| -> f64 {
        let cell = ValueCellId::new("FdmBracket", cell_name);
        match build_result.values.get(&cell) {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            Some(other) => panic!("FdmBracket.{cell_name}: expected Scalar, got {other:?}"),
            None => panic!("FdmBracket.{cell_name} not found in build_result.values"),
        }
    };
    let defl_print = get_deflection_scalar("defl_print");
    let defl_solid = get_deflection_scalar("defl_solid");
    let defl_gt = defl_print > defl_solid;

    // ── build the golden string ───────────────────────────────────────────────
    let actual = format!(
        "wall_e_in_plane = {wall_e_in_plane:.6e}\n\
         wall_e_axial = {wall_e_axial:.6e}\n\
         infill_e_in_plane = {infill_e_in_plane:.6e}\n\
         infill_e_axial = {infill_e_axial:.6e}\n\
         defl_print > defl_solid = {defl_gt}\n"
    );

    let golden_path = format!(
        "{}/tests/golden/fdm_bracket.txt",
        env!("CARGO_MANIFEST_DIR")
    );

    if std::env::var("REIFY_UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::write(&golden_path, &actual)
            .unwrap_or_else(|e| panic!("failed to write golden {golden_path}: {e}"));
        eprintln!("updated golden: {golden_path}");
        return;
    }

    let expected = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!(
            "golden file missing ({golden_path}): {e}\n\
             Run with REIFY_UPDATE_GOLDEN=1 to create it."
        ));

    assert_eq!(
        actual, expected,
        "fdm_bracket golden mismatch\n\
         --- expected (golden/fdm_bracket.txt) ---\n{expected}\
         --- actual ---\n{actual}"
    );
}
