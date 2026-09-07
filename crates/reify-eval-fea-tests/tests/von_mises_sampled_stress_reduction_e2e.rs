//! Eval-level regression: `max(von_mises(<stress field>))` over a REAL
//! `solve_elastic_static` result (task 7129).
//!
//! `ElasticResult.stress` is a `Value::Field { source: FieldSourceKind::Sampled,
//! lambda: Arc(Value::SampledField(_)) }` — see
//! `reify_eval::compute_targets::sampled_stress_field`. Before task 7129,
//! `reify_expr::analysis::validate_tensor_field` accepted only
//! `Analytical | Composed` sources with a `Value::Lambda` backing, so
//! `von_mises(result.stress)` evaluated to `Value::Undef` and every reduction
//! of it silently followed. Nothing flagged that: `value_type_kind_matches` in
//! `reify-eval` accepts `Value::Undef` for ANY declared type, so the pipeline
//! reported a clean solve with no diagnostics while the analysis chain produced
//! nothing at all.
//!
//! The reduction here is driven through `reify_expr::eval_expr` on hand-built
//! `CompiledExpr`s rather than through `.ri` source. That is deliberate: the
//! `.ri` spelling `max(von_mises(fea.stress))` types via
//! `analysis_signatures::tensor_quantity`, which still has no `Type::Field`
//! arm, so writing it as source would entangle this eval-level regression with
//! the un-landed compile-time typing gap tracked by #6577. The `Value`-level
//! contract asserted here is independent of how the expression is spelled.

use reify_core::{ContentHash, DimensionVector, Severity, Type, ValueCellId};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{
    CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, SampledField, Value,
    ValueMap,
};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── harness (mirrors solve_elastic_static_e2e.rs) ────────────────────────────
//
// `cantilever_source` / `extract_field` / `solve_cantilever` are copied from
// `solve_elastic_static_e2e.rs` rather than shared. That is an accepted
// trade-off, not a language constraint — a `tests/common/mod.rs` module (as
// `crates/reify-eval/tests/common/` does) or the `reify-test-support` crate
// could carry them. Hoisting would mean editing `solve_elastic_static_e2e.rs`,
// which this task does not own.

/// Load and compile the cantilever smoke fixture.
fn cantilever_source() -> &'static str {
    include_str!("../../../examples/fea_cantilever_smoke.ri")
}

/// Extract a named field from an ElasticResult value.
fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        // PersistentMap::get takes &K (= &String), not &str — use owned key.
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// Extract `result.max_von_mises` as a raw SI f64.
fn extract_max_von_mises_si(result: &Value) -> f64 {
    match extract_field(result, "max_von_mises") {
        Some(Value::Scalar { si_value, .. }) => si_value,
        Some(Value::Real(v)) => v,
        other => panic!("max_von_mises must be a numeric scalar, got: {other:?}"),
    }
}

/// Build a FunctionCall expression for stdlib functions.
fn make_function_call(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::{}", name),
            },
            args,
        },
        result_type,
        content_hash: ContentHash::of(name.as_bytes()),
    }
}

fn pressure_scalar_type() -> Type {
    Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    }
}

/// Run the cantilever smoke fixture through a real engine eval and return the
/// `ElasticResult` value, asserting the solve produced no Error diagnostics.
fn solve_cantilever() -> Value {
    let compiled = parse_and_compile_with_stdlib(cantilever_source());
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from the cantilever solve, got: {errors:?}"
    );

    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaCantileverSmoke.result not found in eval result"))
        .clone()
}

/// The stress field value plus its `Type::Field`, ready to feed to `von_mises`.
///
/// Also asserts the production shape this task depends on: a `Sampled` source
/// with a `Value::SampledField` backing.
fn stress_field(result: &Value) -> (Value, Type) {
    let field_val = extract_field(result, "stress")
        .unwrap_or_else(|| panic!("field 'stress' not found in result: {result:?}"));

    let Value::Field {
        domain_type,
        codomain_type,
        source,
        lambda,
    } = &field_val
    else {
        panic!("result.stress must be a Value::Field, got: {field_val:?}");
    };
    assert_eq!(
        *source,
        FieldSourceKind::Sampled,
        "result.stress must carry source: Sampled — this is the shape task 7129 admits"
    );
    assert!(
        matches!(lambda.as_ref(), Value::SampledField(_)),
        "result.stress lambda slot must be a Value::SampledField, got: {lambda:?}"
    );

    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };
    (field_val.clone(), field_type)
}

/// Borrow the backing `SampledField` out of a `Field { source: Sampled }`.
fn backing_sampled_field(field_val: &Value) -> &SampledField {
    match field_val {
        Value::Field { lambda, .. } => match lambda.as_ref() {
            Value::SampledField(sf) => sf,
            other => panic!("expected Value::SampledField in the lambda slot, got: {other:?}"),
        },
        other => panic!("expected Value::Field, got: {other:?}"),
    }
}

/// Evaluate `max(von_mises(<stress field>))` and return the resulting `Value`.
fn max_von_mises_over_field(field: Value, field_type: Type) -> Value {
    let vm_field_type = Type::Field {
        domain: match &field_type {
            Type::Field { domain, .. } => domain.clone(),
            other => panic!("expected a Type::Field, got: {other:?}"),
        },
        codomain: Box::new(pressure_scalar_type()),
    };

    let vm_expr = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        vm_field_type.clone(),
    );
    let max_expr = make_function_call("max", vec![vm_expr], pressure_scalar_type());

    let values = ValueMap::new();
    eval_expr(&max_expr, &EvalContext::simple(&values))
}

/// Recompute the grid-max von Mises directly from a stride-9 stress buffer,
/// using the same `pub` stdlib kernel the reduction uses.
///
/// Out-of-solid grid points carry an all-`f64::NAN` sentinel window; those
/// project to NaN and are dropped by the `is_finite()` filter, exactly as
/// `argmax_argmin_index` does inside the reduction.
fn grid_max_von_mises(sf: &SampledField) -> Option<f64> {
    sf.data
        .chunks_exact(9)
        .map(reify_stdlib::compute_von_mises_3x3)
        .filter(|v| v.is_finite())
        .max_by(|a, b| a.total_cmp(b))
}

/// Linear index of the grid-max von Mises window, replicating
/// `field_reductions::argmax_argmin_index` exactly: skip non-finite windows,
/// compare with `total_cmp`, and keep the FIRST of equal maxima (a strict
/// `is_gt()` takes over). `Iterator::max_by` keeps the LAST, which is why
/// `grid_max_von_mises` above cannot be reused for the index.
fn grid_argmax_von_mises_index(sf: &SampledField) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, window) in sf.data.chunks_exact(9).enumerate() {
        let v = reify_stdlib::compute_von_mises_3x3(window);
        if !v.is_finite() {
            continue;
        }
        match best {
            None => best = Some((i, v)),
            Some((_, b)) if v.total_cmp(&b).is_gt() => best = Some((i, v)),
            _ => {}
        }
    }
    best.map(|(i, _)| i)
}

/// Per-axis SI coords at a row-major linear index, mirroring
/// `field_reductions::decompose_index`: axis-0 outermost, so the LAST axis
/// varies fastest (`i_{N-1} = i % s_{N-1}`, then divide out and repeat).
fn axis_coords_at_index(sf: &SampledField, linear: usize) -> Vec<f64> {
    let n = sf.axis_grids.len();
    let mut per_axis = vec![0usize; n];
    let mut rem = linear;
    for k in (0..n).rev() {
        let len = sf.axis_grids[k].len();
        per_axis[k] = rem % len;
        rem /= len;
    }
    (0..n).map(|k| sf.axis_grids[k][per_axis[k]]).collect()
}

/// Evaluate `argmax(von_mises(<stress field>))` and return the resulting `Value`.
///
/// The reduction's result type is the field's DOMAIN, not its codomain —
/// argmax answers "where", not "how much".
fn argmax_von_mises_over_field(field: Value, field_type: Type) -> Value {
    let domain = match &field_type {
        Type::Field { domain, .. } => domain.clone(),
        other => panic!("expected a Type::Field, got: {other:?}"),
    };
    let vm_field_type = Type::Field {
        domain: domain.clone(),
        codomain: Box::new(pressure_scalar_type()),
    };

    let vm_expr = make_function_call(
        "von_mises",
        vec![CompiledExpr::literal(field, field_type)],
        vm_field_type,
    );
    let argmax_expr = make_function_call("argmax", vec![vm_expr], *domain);

    let values = ValueMap::new();
    eval_expr(&argmax_expr, &EvalContext::simple(&values))
}

// ── tests ────────────────────────────────────────────────────────────────────

/// THE REGRESSION: `max(von_mises(result.stress))` over a real solve must be a
/// finite, positive PRESSURE scalar — not `Value::Undef`.
///
/// This is the assertion nothing else in the pipeline makes.
/// `value_type_kind_matches` (`crates/reify-eval/src/lib.rs`) accepts
/// `Value::Undef` for any declared type, so before task 7129 the whole
/// analysis chain collapsed to `Undef` with a clean diagnostic list — a silent
/// false-green. The explicit non-`Undef` assertion is the guard against that
/// regressing.
#[test]
fn max_von_mises_over_real_solve_elastic_static_stress_field_is_not_undef() {
    let result = solve_cantilever();
    let (field, field_type) = stress_field(&result);

    // Guard the oracle's premise: the buffer must contain at least one finite
    // window, otherwise the reduction is legitimately Undef and the test below
    // would be asserting nothing.
    let sf = backing_sampled_field(&field);
    assert!(
        !sf.data.is_empty() && sf.data.len().is_multiple_of(9),
        "stress buffer must be a non-empty stride-9 tensor buffer, got len {}",
        sf.data.len()
    );
    assert!(
        grid_max_von_mises(sf).is_some(),
        "stress buffer has no finite window — the cantilever fixture would need \
         replacing with one whose grid intersects the mesh"
    );

    let reduced = max_von_mises_over_field(field, field_type);

    assert_ne!(
        reduced,
        Value::Undef,
        "max(von_mises(result.stress)) must not be Undef — before task 7129 the \
         analysis wrapper refused the Sampled backing and this silently \
         evaluated to Undef, which value_type_kind_matches accepts for any type"
    );

    let Value::Scalar {
        si_value,
        dimension,
    } = reduced
    else {
        panic!("max(von_mises(stress)) should be a Value::Scalar, got: {reduced:?}");
    };
    assert_eq!(
        dimension,
        DimensionVector::PRESSURE,
        "the reduction must preserve the stress field's PRESSURE dimension"
    );
    assert!(
        si_value.is_finite() && si_value > 0.0,
        "peak von Mises over a loaded cantilever must be finite and positive, got {si_value:e}"
    );
}

/// EXACTNESS ORACLE: the reduction equals an independent recomputation over
/// the stress field's own backing buffer, bit for bit.
///
/// This is a genuine identity, not a tolerance. `compute_extremum`'s `VonMises`
/// arm IS `project_sampled_tensor_windows(stride 9, compute_von_mises_3x3)`
/// followed by `reduce_sampled_extremum` → `argmax_argmin_index`, which is an
/// `is_finite()`-filtered `total_cmp` scan that selects a buffer element
/// verbatim. `grid_max_von_mises` reproduces the identical f64 operations, in
/// the identical order, over the identical buffer, using the same `pub` kernel
/// — so the two agree exactly and `assert_eq!` on the f64 is correct.
///
/// The oracle also pins the VALUE, not just its shape: a degenerate
/// implementation returning some other finite positive number fails here even
/// though it would pass the not-Undef test above.
#[test]
fn max_von_mises_over_stress_field_equals_independent_grid_recomputation() {
    let result = solve_cantilever();
    let (field, field_type) = stress_field(&result);

    let expected = grid_max_von_mises(backing_sampled_field(&field))
        .expect("stress buffer must contain at least one finite window");

    let reduced = max_von_mises_over_field(field, field_type);
    let Value::Scalar { si_value, .. } = reduced else {
        panic!("max(von_mises(stress)) should be a Value::Scalar, got: {reduced:?}");
    };

    assert_eq!(
        si_value, expected,
        "the reduction must be bit-identical to the same kernel applied to the \
         same buffer: reduction {si_value:e} vs direct recomputation {expected:e}"
    );
}

/// `max(von_mises(result.stress))` must not EXCEED `result.max_von_mises`.
///
/// These are NOT the same quantity, and this test deliberately does not assert
/// that they are. `result.max_von_mises` is the raw ELEMENT-max von Mises,
/// computed by looping over per-element stresses in
/// `reify_eval::compute_targets::elastic_static`. `result.stress` is that same
/// field after TWO lossy resamplings: volume-weighted element-to-node
/// averaging (`reify_solver_elastic::result::recover_nodal_stress_p1`), then P1
/// barycentric node-to-grid interpolation
/// (`reify_solver_elastic::resample`), which also writes `f64::NAN` at grid
/// points outside the mesh.
///
/// What IS guaranteed is an inequality. Von Mises is a CONVEX function of the
/// stress tensor, and both resamplings are convex combinations (positive volume
/// weights summing to 1; barycentric coordinates in [0,1] summing to 1).
/// Therefore, for every grid sample:
///
///   vM(grid sample) ≤ max over incident nodes vM
///                   ≤ max over incident elements vM
///                   ≤ result.max_von_mises
///
/// The `1e-9` below is float-rounding slack on an inequality that holds exactly
/// in exact arithmetic. It is NOT a tuned tolerance, and this must not later be
/// "improved" into an equality or approximate-equality assertion with a guessed
/// threshold — the two quantities genuinely differ, and such a test would be
/// unfixable without weakening it into meaninglessness.
#[test]
fn max_von_mises_over_stress_field_does_not_exceed_element_max_von_mises() {
    let result = solve_cantilever();
    let (field, _field_type) = stress_field(&result);

    let grid_max = grid_max_von_mises(backing_sampled_field(&field))
        .expect("stress buffer must contain at least one finite window");
    let element_max = extract_max_von_mises_si(&result);

    assert!(
        element_max.is_finite() && element_max > 0.0,
        "result.max_von_mises must be finite and positive, got {element_max:e}"
    );
    assert!(
        grid_max <= element_max * (1.0 + 1e-9),
        "convexity bound violated: grid-resampled peak von Mises {grid_max:e} Pa \
         exceeds the element-max {element_max:e} Pa. Both resamplings between them \
         are convex combinations, so the grid peak can only be ≤ the element max."
    );
}

/// `argmax(von_mises(result.stress))` — WHERE the peak stress is — over a real
/// Regular3D stress field.
///
/// This exercises coordinate machinery no other test on this path reaches. The
/// unit-level `argmax` coverage in
/// `reify-expr/tests/field_analysis_tests.rs` uses a Regular1D grid with a
/// dimensionless-scalar domain, so `arg_coord_from_index`'s 3-axis
/// `decompose_index` and `wrap_coord_for_domain`'s `Type::Point` arm are only
/// driven for an analysis wrapper HERE — over the production shape
/// (`Regular3D` grid, `Point3<Length>` domain, stamped by
/// `reify_eval::compute_targets::sampled_stress_field`). Admitting the Sampled
/// backing is what first makes that path reachable at all, and every guard on
/// it fails to `Value::Undef` rather than panicking — the exact silent shape
/// task 7129 exists to close.
///
/// The oracle is exact, not approximate: `argmax_argmin_index` selects a
/// buffer index and `arg_coord_from_index` looks the coord up in
/// `axis_grids` verbatim, so the expected coords are f64s copied out of the
/// same grid — no interpolation, no arithmetic.
#[test]
fn argmax_von_mises_over_real_stress_field_returns_the_peak_grid_coordinate() {
    let result = solve_cantilever();
    let (field, field_type) = stress_field(&result);

    // The production domain: Point3<Length>. `wrap_coord_for_domain` wraps a
    // coord per axis only for a `Type::Point` domain; anything else is Undef.
    let Type::Field { domain, .. } = &field_type else {
        panic!("expected a Type::Field, got: {field_type:?}");
    };
    assert_eq!(
        **domain,
        Type::point3(Type::length()),
        "result.stress must carry a Point3<Length> domain — this is the shape \
         arg_coord_from_index decomposes against"
    );

    // Oracle, computed before `field` is moved into the reduction.
    let (expected_coords, bounds_min, bounds_max) = {
        let sf = backing_sampled_field(&field);
        assert_eq!(
            sf.axis_grids.len(),
            3,
            "the cantilever stress field must be a 3-axis Regular3D grid"
        );
        let grid_count: usize = sf.axis_grids.iter().map(|g| g.len()).product();
        assert_eq!(
            sf.data.len(),
            grid_count * 9,
            "stride-9 buffer must hold one 3x3 tensor per grid point"
        );
        let index = grid_argmax_von_mises_index(sf)
            .expect("stress buffer must contain at least one finite window");
        (
            axis_coords_at_index(sf, index),
            sf.bounds_min.clone(),
            sf.bounds_max.clone(),
        )
    };

    let arg = argmax_von_mises_over_field(field, field_type);

    assert_ne!(
        arg,
        Value::Undef,
        "argmax(von_mises(result.stress)) must not be Undef — locating the peak \
         stress is the user-facing capability admitting the Sampled backing unlocks"
    );
    let Value::Point(coords) = &arg else {
        panic!("argmax over a Point3 domain should return a Value::Point, got: {arg:?}");
    };
    assert_eq!(coords.len(), 3, "a Point3 domain yields three coords");

    for (k, coord) in coords.iter().enumerate() {
        let Value::Scalar {
            si_value,
            dimension,
        } = coord
        else {
            panic!("axis {k} coord should be a Value::Scalar, got: {coord:?}");
        };
        assert_eq!(
            *dimension,
            DimensionVector::LENGTH,
            "axis {k} coord must carry the domain's LENGTH dimension"
        );
        assert!(
            *si_value >= bounds_min[k] && *si_value <= bounds_max[k],
            "axis {k} coord {si_value:e} m falls outside the stress grid bounds \
             [{:e}, {:e}]",
            bounds_min[k],
            bounds_max[k]
        );
        assert_eq!(
            *si_value, expected_coords[k],
            "axis {k}: the reduction must return the grid coord of the peak \
             window (index-matched to the independent recomputation)"
        );
    }
}
