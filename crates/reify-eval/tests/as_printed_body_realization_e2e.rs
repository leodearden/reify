// SPDX-License-Identifier: AGPL-3.0-or-later

//! End-to-end tests for post-hydration re-dispatch of @optimized ComputeNodes
//! consuming a Solid body (task #4726 — esc-3787-23 root cause).
//!
//! Root cause summary (confirmed against main post-#4651/#4652/#4653):
//!
//! In `Engine::build()` → `build_with_geometry_output()` → `check()` → `eval()`
//! (engine_build.rs), the `@optimized` ComputeNode dispatch happens INSIDE
//! `eval()`'s call to `evaluate_params_and_lets_unified`. At that dispatch the
//! `body` arg evaluates to `Value::Undef` because:
//!   1. The compiler creates NO value cell for geometry lets (`let body = box(...)`).
//!   2. The `mint_symbolic_geometry_handles_into_values` pass (#4652) runs AFTER
//!      `evaluate_params_and_lets_unified` returns, and is explicitly skipped for
//!      already-realized cells.
//! So `build_compute_realization_inputs` yields EMPTY `realization_inputs` at
//! dispatch time → `body_aabb()` returns `None` → `degraded_field()` (lambda=Undef).
//!
//! `build()` later executes `post_process_geometry_handle_cells` (hydrates the
//! body's value cell with the realized handle) but there is no post-hydration
//! re-dispatch.
//!
//! All tests are gated on `reify_kernel_occt::OCCT_AVAILABLE` — OCCT is present
//! in this environment via `/opt/reify-deps`, so tests execute for real.

mod common;

use reify_core::{Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, ExportFormat, FieldSourceKind, ResolvedFunction, Value, ValueMap};
use reify_eval::compute_targets::register_compute_fns;
use reify_expr::{EvalContext, eval_expr};
use reify_core::ContentHash;

// ── shared helpers ─────────────────────────────────────────────────────────────

/// Build a fresh `Engine` backed by a real OCCT kernel (direct
/// `OcctKernelHandle`, not wrapped in `SingleKernelHolder`).
///
/// Mirrors `achieved_repr_tol.rs::make_occt_engine`.
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Inline FDM box structure: a Solid body + `as_printed_material` bound to it.
/// Mirrors the `esc-3787-23` repro shape.
const FDM_BOX_SOURCE: &str = r#"
structure FdmBox {
    let body = box(40mm, 40mm, 10mm)
    let mat = as_printed_material(body, FDMProcess())
}
"#;

// ── sample() round-trip machinery (copied from as_printed_material_e2e.rs) ───

/// A `Point3<Length>` from SI-metre coordinates.
fn point3(p: [f64; 3]) -> Value {
    use common::as_printed::length;
    Value::Point(vec![length(p[0]), length(p[1]), length(p[2])])
}

/// Build a `sample(field, at)` FunctionCall expression.
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
    eval_expr(&sample_call(field.clone(), point3(at)), &ctx)
}

/// Read a `law` constant (e.g. `e_in_plane`) from a sampled `AnisotropicMaterial`.
fn law_constant(aniso: &Value, key: &str) -> f64 {
    let Value::StructureInstance(data) = aniso else {
        panic!("expected AnisotropicMaterial StructureInstance, got {aniso:?}");
    };
    assert_eq!(
        data.type_name, "AnisotropicMaterial",
        "sampled zone material must be an AnisotropicMaterial"
    );
    let law = data.fields.get("law").expect("AnisotropicMaterial.law");
    let Value::StructureInstance(law) = law else {
        panic!("expected law StructureInstance, got {law:?}");
    };
    match law.fields.get(key) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected law.{key} to be a Scalar, got {other:?}"),
    }
}

// ── step-1: RED — realization_inputs NON-empty ────────────────────────────────
//
// After a real `build(ExportFormat::Step)` the snapshot ComputeNode for
// "fdm::as_printed_material_r_fast" must carry a NON-empty `realization_inputs`
// vec (contains the body realization id).
//
// RED today: body=Undef at the only dispatch → `build_compute_realization_inputs`
// yields EMPTY `realization_inputs`; `build()` does NOT re-dispatch after
// `post_process_geometry_handle_cells`.

/// Build the FdmBox through a real OCCT engine and assert the
/// `as_printed_material_r_fast` ComputeNode has a non-empty `realization_inputs`.
#[test]
fn redispatched_optimized_body_node_has_nonempty_realization_inputs() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping redispatched_optimized_body_node_has_nonempty_realization_inputs: \
             OCCT not available"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(FDM_BOX_SOURCE);

    let mut engine = make_occt_engine();
    register_compute_fns(&mut engine);
    engine.build(&compiled, ExportFormat::Step);

    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after build()")
        .snapshot
        .clone();

    // Find the compute node for the as-printed material trampoline.
    let as_printed_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, data)| data.target == "fdm::as_printed_material_r_fast");

    let (_, node_data) = as_printed_node.unwrap_or_else(|| {
        panic!(
            "expected a ComputeNode with target==\"fdm::as_printed_material_r_fast\" \
             in the snapshot graph; present targets: {:?}",
            snapshot
                .graph
                .compute_nodes
                .iter()
                .map(|(_, d)| &d.target)
                .collect::<Vec<_>>()
        )
    });

    assert!(
        !node_data.realization_inputs.is_empty(),
        "ComputeNode for fdm::as_printed_material_r_fast must have NON-empty \
         realization_inputs after post-hydration re-dispatch; got: {:?}",
        node_data.realization_inputs
    );
}

// ── step-3: RED — mat field is non-degraded ───────────────────────────────────
//
// After step-2 (impl) the realization_inputs become non-empty, but the body
// realizes as ReprKind::BRep → project_realization_read_handle returns None
// content → body_aabb None → degraded_field (lambda Undef). This assertion
// checks the NEXT level: the produced `mat` field must be non-degraded (lambda
// NOT Undef). RED until step-4 tessellates the BRep body into the projection
// store.

/// After build the `mat` cell must hold a non-degraded AsPrintedZones field.
///
/// Non-degraded = `Value::Field { source: AsPrintedZones, lambda: NOT Undef }`.
/// Degraded (the today baseline) = lambda is `Value::Undef`.
#[test]
fn redispatched_body_field_is_non_degraded_for_box_body() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping redispatched_body_field_is_non_degraded_for_box_body: \
             OCCT not available"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(FDM_BOX_SOURCE);

    let mut engine = make_occt_engine();
    register_compute_fns(&mut engine);
    engine.build(&compiled, ExportFormat::Step);

    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after build()")
        .snapshot
        .clone();

    let mat_cell = ValueCellId::new("FdmBox", "mat");
    let (mat_value, _det) = snapshot
        .values
        .get(&mat_cell)
        .unwrap_or_else(|| panic!("FdmBox.mat value cell not found in snapshot"));

    match mat_value {
        Value::Field { source, lambda, .. } => {
            assert!(
                matches!(source, FieldSourceKind::AsPrintedZones),
                "FdmBox.mat must be an AsPrintedZones field, got source={source:?}"
            );
            assert!(
                !matches!(lambda.as_ref(), Value::Undef),
                "FdmBox.mat field lambda must NOT be Undef after post-hydration \
                 re-dispatch + BRep tessellation; lambda={lambda:?}"
            );
        }
        other => panic!("FdmBox.mat must be a Value::Field, got {other:?}"),
    }
}

// ── step-5: RED — fdm_bracket.ri produces a non-degraded heterogeneous field ──
//
// User-observable done-gate: a real `examples/fdm_bracket.ri` file parsed,
// compiled, and built through a real OCCT engine produces a non-degraded
// `material` field, and sampling it at a wall vs. deep-interior point yields
// distinct AnisotropicMaterials (heterogeneous: wall in-plane > infill in-plane).
//
// RED until step-6 creates `examples/fdm_bracket.ri` AND the impl from step-4
// makes the field non-degraded.

/// Full done-gate: `examples/fdm_bracket.ri` builds cleanly through OCCT and
/// delivers a heterogeneous `material` field (wall e_in_plane > infill e_in_plane).
#[test]
fn fdm_bracket_example_produces_non_degraded_heterogeneous_material_field() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping fdm_bracket_example_produces_non_degraded_heterogeneous_material_field: \
             OCCT not available"
        );
        return;
    }

    // Use read_to_string (not include_str!) so the test panics at runtime when
    // the file is missing (step-6 creates it), rather than failing at compile time.
    let path = format!(
        "{}/../../examples/fdm_bracket.ri",
        env!("CARGO_MANIFEST_DIR")
    );
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read examples/fdm_bracket.ri: {e}"));

    let compiled = reify_test_support::parse_and_compile_with_stdlib(&source);

    // (a) zero Error-severity diagnostics
    {
        use reify_core::Severity;
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
    engine.build(&compiled, ExportFormat::Step);

    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after build()")
        .snapshot
        .clone();

    // (b) `material` cell is non-degraded AsPrintedZones field.
    // The structure name in fdm_bracket.ri is expected to be "FdmBracket".
    let mat_cell = ValueCellId::new("FdmBracket", "material");
    let (mat_value, _det) = snapshot
        .values
        .get(&mat_cell)
        .unwrap_or_else(|| panic!("FdmBracket.material value cell not found in snapshot"));

    let material_field = mat_value.clone();
    match &material_field {
        Value::Field { source, lambda, .. } => {
            assert!(
                matches!(source, FieldSourceKind::AsPrintedZones),
                "FdmBracket.material must be an AsPrintedZones field, got source={source:?}"
            );
            assert!(
                !matches!(lambda.as_ref(), Value::Undef),
                "FdmBracket.material field lambda must NOT be Undef; lambda={lambda:?}"
            );
        }
        other => panic!("FdmBracket.material must be a Value::Field, got {other:?}"),
    }

    // (c) Heterogeneous: wall in-plane modulus strictly > infill in-plane modulus.
    // Wall point: 0.3 mm from the −X side face (inside the 3-wall × 0.4 mm band).
    // Interior point: deep centre of the bracket body.
    let wall = sample_at(&material_field, [0.0003, 0.020, 0.005]);
    let infill = sample_at(&material_field, [0.020, 0.020, 0.005]);

    let wall_e = law_constant(&wall, "e_in_plane");
    let infill_e = law_constant(&infill, "e_in_plane");
    assert!(
        wall_e > infill_e,
        "FdmBracket.material: wall in-plane modulus ({wall_e}) must strictly exceed \
         infill ({infill_e}) — heterogeneous non-constant field"
    );
}
