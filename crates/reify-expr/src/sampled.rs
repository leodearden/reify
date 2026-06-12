//! Runtime sample-dispatch for `Value::SampledField`.
//!
//! v0.2 sampled-field semantics (task 2341):
//! * Out-of-bounds queries return `Value::Undef` and emit
//!   `W_FIELD_OUT_OF_BOUNDS` once per field per session, suppressed by an
//!   `AtomicBool` on the `SampledField` itself.
//! * Linear / NearestNeighbor / Cubic methods dispatch to
//!   [`crate::interp::interpolate_1d`]/`_2d`/`_3d`. RBF / Kriging fall back
//!   to Linear and emit `W_INTERPOLATION_DEFERRED` (delegated to interp's
//!   own resolve-method path).
//!
//! `EvalContext` carries an optional `RefCell<Vec<Diagnostic>>` sink. When
//! present, OOB and interpolation-deferred warnings are pushed into it for
//! the surrounding `Engine::eval` to drain. When absent, warnings are
//! silently dropped — preserving the old `EvalContext::simple` semantics
//! used by ad-hoc tests.
//!
//! End-to-end behaviour of these contracts is pinned by the integration
//! tests in `crates/reify-eval/tests/field_eval_tests.rs`:
//! * `sample_sampled_field_out_of_bounds_returns_undef_and_emits_warning_once`
//!   — OOB → `Value::Undef` and exactly one `W_FIELD_OUT_OF_BOUNDS` across
//!   N OOB calls on the same field.
//! * `sample_sampled_field_with_rbf_emits_interpolation_deferred_warning_and_falls_back_to_linear`
//!   — RBF / Kriging emit `W_INTERPOLATION_DEFERRED` and the value matches
//!   the Linear-fallback baseline.
//! * 1D / 2D / 3D positive-path tests confirm the fully-supported methods
//!   (Linear / NearestNeighbor / Cubic) leave `result.diagnostics` empty.

use std::sync::atomic::Ordering;

use reify_core::{Diagnostic, DiagnosticCode, Type};
use reify_ir::{InterpolationKind, SampledField, SampledGridKind, Value};

use crate::EvalContext;
use crate::interp::{
    InterpolationMethod, InterpolationResult, interpolate_1d, interpolate_2d, interpolate_3d,
};

/// Map a language-level [`InterpolationKind`] to the algorithmic-core
/// [`InterpolationMethod`]. RBF and Kriging map directly so `interp::resolve_method`
/// triggers the deferred-method fallback and emits `W_INTERPOLATION_DEFERRED`.
impl From<InterpolationKind> for InterpolationMethod {
    fn from(kind: InterpolationKind) -> Self {
        match kind {
            InterpolationKind::Linear => InterpolationMethod::Linear,
            InterpolationKind::NearestNeighbor => InterpolationMethod::NearestNeighbor,
            InterpolationKind::Cubic => InterpolationMethod::Cubic,
            InterpolationKind::Rbf => InterpolationMethod::Rbf,
            InterpolationKind::Kriging => InterpolationMethod::Kriging,
        }
    }
}

/// Sample a `SampledField` at the given query point.
///
/// `point` is the user-facing sample arg (whatever the user passed as the
/// second arg to `sample(field, point)`). For `Regular1D`, accepts
/// `Value::Real`, `Value::Int`, or `Value::Scalar` (any dimension; we
/// extract `si_value`). For `Regular2D`/`3D`, accepts `Value::Point`
/// or `Value::Vector` of matching arity, with each component a Real / Int
/// / Scalar.
///
/// `codomain_type` is the field's declared codomain type (from
/// `Value::Field.codomain_type`). Dimensionless codomain → return
/// `Value::Real`; dimensioned codomain → return `Value::Scalar`.
///
/// Behaviour:
/// 1. Coord-extraction failure → `Value::Undef` (no diagnostic; matches
///    the existing analytical-field arm's silent-Undef on shape mismatch).
/// 2. OOB on any axis → atomically swap `oob_emitted` from `false` →
///    `true`; on the winning swap (we set it from false to true), push a
///    `W_FIELD_OUT_OF_BOUNDS` warning to `ctx.diagnostics` if present.
///    Return `Value::Undef`.
/// 3. Else dispatch to `interpolate_1d`/`2d`/`3d` based on `field.kind`,
///    forward the call's `result.diagnostics` to `ctx.diagnostics`, and
///    wrap the f64 in `Value::Real` (dimensionless codomain) or
///    `Value::Scalar` (dimensioned codomain).
pub fn sample_at_point(
    field: &SampledField,
    point: &Value,
    codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    let coords = match extract_coords(point, field.kind) {
        Some(c) => c,
        None => return Value::Undef,
    };

    if is_out_of_bounds(&coords, &field.bounds_min, &field.bounds_max) {
        if field
            .oob_emitted
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            && let Some(sink) = ctx.diagnostics
        {
            let diag = Diagnostic::warning(format!(
                "sampled field '{}' query is out of bounds; returning Undef",
                field.name
            ))
            .with_code(DiagnosticCode::FieldOutOfBounds);
            sink.borrow_mut().push(diag);
        }
        return Value::Undef;
    }

    // Compute grid node count and data stride.
    //
    // For elaborator-validated scalar fields, `data.len() == grid_count` (stride 1).
    // For ε eager-lowered gradient/laplacian Sampled outputs, `data.len() == grid_count * n_axes`
    // (stride = n_axes).  The stride is derived directly from the data buffer so this dispatch
    // is robust to any multi-component output without re-parsing the codomain arity.
    //
    // `grid_count == 0` → degenerate; fall through to scalar path which will surface the
    // invariant violation via the interpolate_Nd assert (same as before this change).
    let grid_count: usize = field.axis_grids.iter().map(|g| g.len()).product();
    // Invariant: data.len() must be an exact multiple of grid_count for elaborator-validated
    // and ε-produced fields.  A debug_assert catches mis-constructed fields in debug builds
    // rather than silently sampling with a truncated stride.
    debug_assert!(
        grid_count == 0 || field.data.len().is_multiple_of(grid_count),
        "SampledField data length {} is not a multiple of grid_count {}; field layout is corrupt",
        field.data.len(),
        grid_count,
    );
    // stride = data.len() / grid_count, clamped to ≥ 1.
    // checked_div returns None when grid_count == 0 (degenerate); .unwrap_or(1) keeps the scalar
    // path so the interpolate_Nd assert surfaces the invariant violation unchanged.
    // For data.len() < grid_count, result is 0; .max(1) also keeps the scalar path.
    let stride = field.data.len().checked_div(grid_count).unwrap_or(1).max(1);

    let method: InterpolationMethod = field.interpolation.into();

    // ── stride > 1: multi-component path (ε) ────────────────────────────────
    // Deinterleave each component from `data[g * stride + c]`, interpolate with
    // the same kernels as the scalar path, assemble Value::Vector.
    // Diagnostics are forwarded from component 0 only (not once per component).
    if stride > 1 {
        // Extract the per-component type from the codomain.
        // Vector{n, quantity} → quantity; any other codomain → codomain itself (fallback).
        let component_type: &Type = match codomain_type {
            Type::Vector { quantity, .. } => quantity.as_ref(),
            _ => codomain_type,
        };

        let mut components = Vec::with_capacity(stride);
        let mut first_diagnostics: Option<Vec<Diagnostic>> = None;

        for c in 0..stride {
            // Deinterleave: collect every c-th element from the interleaved buffer.
            let values_c: Vec<f64> =
                (0..grid_count).map(|g| field.data[g * stride + c]).collect();

            let result_c: InterpolationResult = match field.kind {
                SampledGridKind::Regular1D => {
                    interpolate_1d(method, &field.axis_grids[0], &values_c, coords[0])
                }
                SampledGridKind::Regular2D => interpolate_2d(
                    method,
                    &field.axis_grids[0],
                    &field.axis_grids[1],
                    &values_c,
                    (coords[0], coords[1]),
                ),
                SampledGridKind::Regular3D => interpolate_3d(
                    method,
                    &field.axis_grids[0],
                    &field.axis_grids[1],
                    &field.axis_grids[2],
                    &values_c,
                    (coords[0], coords[1], coords[2]),
                ),
            };

            let comp_f64 = result_c.value;
            // Capture diagnostics from the first component only (RBF/Kriging fallback etc.).
            if c == 0 {
                first_diagnostics = Some(result_c.diagnostics);
            }
            components.push(wrap_result(comp_f64, component_type));
        }

        // Forward first-component diagnostics once to the runtime sink.
        if let Some(diags) = first_diagnostics
            && !diags.is_empty()
            && let Some(sink) = ctx.diagnostics
        {
            let mut borrow = sink.borrow_mut();
            for d in diags {
                borrow.push(d);
            }
        }

        return Value::Vector(components);
    }

    // ── stride ≤ 1: existing scalar path — bit-identical to the original ────
    //
    // The elaborator (`engine_eval::build_sampled_field`) enforces:
    // 1. each axis spacing is strictly positive and finite,
    // 2. each axis grid has at least 2 nodes,
    // 3. `data.len() == product(axis_grids[i].len())`.
    // Any violation poisons the field to Value::Undef at elaboration time, so the
    // interpolate_Nd asserts below cannot fire from elaborator-validated input.
    let result: InterpolationResult = match field.kind {
        SampledGridKind::Regular1D => {
            interpolate_1d(method, &field.axis_grids[0], &field.data, coords[0])
        }
        SampledGridKind::Regular2D => interpolate_2d(
            method,
            &field.axis_grids[0],
            &field.axis_grids[1],
            &field.data,
            (coords[0], coords[1]),
        ),
        SampledGridKind::Regular3D => interpolate_3d(
            method,
            &field.axis_grids[0],
            &field.axis_grids[1],
            &field.axis_grids[2],
            &field.data,
            (coords[0], coords[1], coords[2]),
        ),
    };

    // Forward any interpolation diagnostics (e.g. RBF/Kriging deferral) to
    // the runtime sink. Like OOB above, silent-drop when no sink is wired.
    if !result.diagnostics.is_empty()
        && let Some(sink) = ctx.diagnostics
    {
        let mut borrow = sink.borrow_mut();
        for d in result.diagnostics {
            borrow.push(d);
        }
    }

    wrap_result(result.value, codomain_type)
}

/// Extract per-axis SI scalar coordinates from a sample-point `Value`,
/// projecting to the arity required by `kind`.
fn extract_coords(point: &Value, kind: SampledGridKind) -> Option<Vec<f64>> {
    match kind {
        SampledGridKind::Regular1D => {
            // 1D: accept a single scalar (Real / Int / Scalar). Reject
            // Point/Vector — those are ambiguous for a 1D field.
            scalar_si(point).map(|v| vec![v])
        }
        SampledGridKind::Regular2D => extract_arity_n(point, 2),
        SampledGridKind::Regular3D => extract_arity_n(point, 3),
    }
}

/// Extract `n` per-axis SI scalars from a `Value::Point` or `Value::Vector`
/// whose components are each Real / Int / Scalar.
fn extract_arity_n(point: &Value, n: usize) -> Option<Vec<f64>> {
    let items = match point {
        Value::Point(items) | Value::Vector(items) => items,
        _ => return None,
    };
    if items.len() != n {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for item in items {
        out.push(scalar_si(item)?);
    }
    Some(out)
}

/// Extract the SI scalar value from a `Value::Real`, `Value::Int`, or
/// `Value::Scalar`. Any other shape returns `None`.
fn scalar_si(v: &Value) -> Option<f64> {
    match v {
        Value::Real(r) => Some(*r),
        Value::Int(n) => Some(*n as f64),
        Value::Scalar { si_value, .. } => Some(*si_value),
        _ => None,
    }
}

/// True if any axis coord is strictly outside the field's `[min, max]`
/// bounds. Inclusive on both endpoints (matches `interp::locate_cell`'s
/// right-edge-inclusive contract).
fn is_out_of_bounds(coords: &[f64], min: &[f64], max: &[f64]) -> bool {
    debug_assert_eq!(coords.len(), min.len());
    debug_assert_eq!(coords.len(), max.len());
    coords
        .iter()
        .zip(min.iter())
        .zip(max.iter())
        .any(|((q, lo), hi)| q.is_nan() || q < lo || q > hi)
}

/// Wrap an interpolated f64 in the field's codomain shape. Dimensionless
/// codomain (`Type::dimensionless_scalar()` or `Type::Int`) → `Value::Real`; otherwise
/// `Value::Scalar { si_value, dimension: codomain.dim }`.
fn wrap_result(v: f64, codomain_type: &Type) -> Value {
    match codomain_type {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Value::Scalar {
            si_value: v,
            dimension: *dimension,
        },
        _ => Value::Real(v),
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use reify_core::Type;
    use reify_ir::{InterpolationKind, SampledField, SampledGridKind, Value, ValueMap};

    use crate::EvalContext;
    use super::sample_at_point;

    // ── fixture helpers ──────────────────────────────────────────────────────

    /// Build a Regular1D scalar (stride-1) field.
    fn make_1d_scalar(n: usize, h: f64, f: impl Fn(f64) -> f64) -> SampledField {
        let axis: Vec<f64> = (0..n).map(|i| i as f64 * h).collect();
        let data: Vec<f64> = axis.iter().map(|&x| f(x)).collect();
        SampledField {
            name: "test-1d".to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![(n - 1) as f64 * h],
            spacing: vec![h],
            axis_grids: vec![axis],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Build a Regular2D stride-2 field (interleaved node-major: data[g*2+0]=comp0, [g*2+1]=comp1).
    fn make_2d_stride2(
        nx: usize,
        ny: usize,
        hx: f64,
        hy: f64,
        f: impl Fn(f64, f64) -> [f64; 2],
    ) -> SampledField {
        let xs: Vec<f64> = (0..nx).map(|i| i as f64 * hx).collect();
        let ys: Vec<f64> = (0..ny).map(|j| j as f64 * hy).collect();
        let mut data = Vec::with_capacity(nx * ny * 2);
        for &x in &xs {
            for &y in &ys {
                let v = f(x, y);
                data.push(v[0]);
                data.push(v[1]);
            }
        }
        SampledField {
            name: "test-2d-stride2".to_string(),
            kind: SampledGridKind::Regular2D,
            bounds_min: vec![0.0, 0.0],
            bounds_max: vec![(nx - 1) as f64 * hx, (ny - 1) as f64 * hy],
            spacing: vec![hx, hy],
            axis_grids: vec![xs, ys],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    // ── ε step-5a: stride-1 regression ──────────────────────────────────────

    /// sample_at_point on a stride-1 Regular1D scalar field returns the interpolated
    /// Value::Real — regression pin for the existing scalar path.
    ///
    /// f(x) = 2x + 3 sampled at x=2.0 → 2*2+3 = 7.0 (exact at grid node).
    /// Must be GREEN before and after step-6 (scalar path is branch-guarded, bit-identical).
    #[test]
    fn sample_at_point_stride1_scalar_returns_real() {
        let sf = make_1d_scalar(5, 1.0, |x| 2.0 * x + 3.0);
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);
        let result = sample_at_point(&sf, &Value::Real(2.0), &Type::dimensionless_scalar(), &ctx);
        assert_eq!(result, Value::Real(7.0), "stride-1 scalar sample must return Real(7.0)");
    }

    // ── ε step-5b: stride-2 constant 2D field — currently RED ───────────────

    /// sample_at_point on a stride-2 Regular2D field with constant components (comp0=3.0,
    /// comp1=5.0) at an in-bounds point returns Value::Vector([Real(3.0), Real(5.0)]).
    /// Pins the stride-n deinterleave + interpolate path introduced in ε step-6.
    #[test]
    fn sample_at_point_stride2_constant_returns_vector() {
        // 3×3 grid, constant components: comp0 = 3.0, comp1 = 5.0 everywhere.
        let sf = make_2d_stride2(3, 3, 1.0, 1.0, |_x, _y| [3.0, 5.0]);
        // codomain = Vector{2, Real} — the stride-2 type produced by 2D gradient
        let codomain = Type::Vector {
            n: 2,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);
        // Sample at an in-bounds interior point (not necessarily a grid node)
        let point = Value::Vector(vec![Value::Real(0.5), Value::Real(0.5)]);
        let result = sample_at_point(&sf, &point, &codomain, &ctx);
        match &result {
            Value::Vector(comps) => {
                assert_eq!(comps.len(), 2, "stride-2 result must have 2 components");
                assert_eq!(comps[0], Value::Real(3.0), "comp0 must be Real(3.0)");
                assert_eq!(comps[1], Value::Real(5.0), "comp1 must be Real(5.0)");
            }
            other => panic!("expected Value::Vector, got {:?}", other),
        }
    }

    /// Build a Regular3D stride-3 field (interleaved node-major:
    /// data[g*3+0]=comp0, [g*3+1]=comp1, [g*3+2]=comp2).
    fn make_3d_stride3(
        nx: usize,
        ny: usize,
        nz: usize,
        h: f64,
        f: impl Fn(f64, f64, f64) -> [f64; 3],
    ) -> SampledField {
        let xs: Vec<f64> = (0..nx).map(|i| i as f64 * h).collect();
        let ys: Vec<f64> = (0..ny).map(|j| j as f64 * h).collect();
        let zs: Vec<f64> = (0..nz).map(|k| k as f64 * h).collect();
        let mut data = Vec::with_capacity(nx * ny * nz * 3);
        for &x in &xs {
            for &y in &ys {
                for &z in &zs {
                    let v = f(x, y, z);
                    data.push(v[0]);
                    data.push(v[1]);
                    data.push(v[2]);
                }
            }
        }
        SampledField {
            name: "test-3d-stride3".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![(nx - 1) as f64 * h, (ny - 1) as f64 * h, (nz - 1) as f64 * h],
            spacing: vec![h, h, h],
            axis_grids: vec![xs, ys, zs],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    // ── ζ step-6: stride-3 Regular3D regression pin ──────────────────────────

    /// sample_at_point on a stride-3 Regular3D field with constant components
    /// (comp0=1.0, comp1=2.0, comp2=3.0) at an in-bounds interior point returns
    /// Value::Vector([Real(1.0), Real(2.0), Real(3.0)]).
    ///
    /// Pins the stride-n deinterleave + interpolate path for 3-component output
    /// (ε only tested stride-2 via Regular2D; ζ uses stride-3 via Regular3D curl).
    #[test]
    fn sample_at_point_stride3_regular3d_constant_returns_vector() {
        // 3×3×3 grid, constant components everywhere.
        let sf = make_3d_stride3(3, 3, 3, 1.0, |_x, _y, _z| [1.0, 2.0, 3.0]);
        let codomain = Type::Vector {
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);
        // Interior non-node point — trilinear interpolation of constants is exact.
        let point = Value::Vector(vec![
            Value::Real(0.5),
            Value::Real(0.5),
            Value::Real(0.5),
        ]);
        let result = sample_at_point(&sf, &point, &codomain, &ctx);
        match &result {
            Value::Vector(comps) => {
                assert_eq!(comps.len(), 3, "stride-3 result must have 3 components");
                assert_eq!(comps[0], Value::Real(1.0), "comp0 must be Real(1.0)");
                assert_eq!(comps[1], Value::Real(2.0), "comp1 must be Real(2.0)");
                assert_eq!(comps[2], Value::Real(3.0), "comp2 must be Real(3.0)");
            }
            other => panic!("expected Value::Vector, got {:?}", other),
        }
    }

    /// sample_at_point on a stride-3 Regular3D field with linearly-varying components
    /// (comp0=x, comp1=y, comp2=z) at a grid node returns the exact per-component values.
    /// Linear interpolation on a grid node is exact by construction.
    ///
    /// Locks the stride-3 deinterleave path that curl output relies on.
    #[test]
    fn sample_at_point_stride3_regular3d_linear_at_grid_node_is_exact() {
        // 3×3×3 grid: comp0=x, comp1=y, comp2=z at each node.
        let sf = make_3d_stride3(3, 3, 3, 1.0, |x, y, z| [x, y, z]);
        let codomain = Type::Vector {
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);
        // Grid node (2.0, 1.0, 0.0) — node g = 2*3*3 + 1*3 + 0 = 21
        let point = Value::Vector(vec![
            Value::Real(2.0),
            Value::Real(1.0),
            Value::Real(0.0),
        ]);
        let result = sample_at_point(&sf, &point, &codomain, &ctx);
        match &result {
            Value::Vector(comps) => {
                assert_eq!(comps.len(), 3, "stride-3 result must have 3 components");
                let c0 = match &comps[0] {
                    Value::Real(v) => *v,
                    other => panic!("comp0 must be Real, got {:?}", other),
                };
                let c1 = match &comps[1] {
                    Value::Real(v) => *v,
                    other => panic!("comp1 must be Real, got {:?}", other),
                };
                let c2 = match &comps[2] {
                    Value::Real(v) => *v,
                    other => panic!("comp2 must be Real, got {:?}", other),
                };
                assert!((c0 - 2.0).abs() < 1e-12, "comp0 at node (2,1,0) must be 2.0, got {c0}");
                assert!((c1 - 1.0).abs() < 1e-12, "comp1 at node (2,1,0) must be 1.0, got {c1}");
                assert!((c2 - 0.0).abs() < 1e-12, "comp2 at node (2,1,0) must be 0.0, got {c2}");
            }
            other => panic!("expected Value::Vector, got {:?}", other),
        }
    }

    /// sample_at_point on a stride-2 Regular2D field with linearly-varying components
    /// (comp0=x, comp1=y) at a grid node returns the exact per-component values.
    /// Linear interpolation on a grid node is exact by construction.
    #[test]
    fn sample_at_point_stride2_linear_at_grid_node_is_exact() {
        // 3×3 grid: comp0(i,j) = x_i, comp1(i,j) = y_j.
        let sf = make_2d_stride2(3, 3, 1.0, 1.0, |x, y| [x, y]);
        let codomain = Type::Vector {
            n: 2,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);
        // Sample at grid node (1.0, 2.0) — node g = 1*3+2 = 5
        let point = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0)]);
        let result = sample_at_point(&sf, &point, &codomain, &ctx);
        match &result {
            Value::Vector(comps) => {
                assert_eq!(comps.len(), 2, "stride-2 result must have 2 components");
                let c0 = match &comps[0] {
                    Value::Real(v) => *v,
                    other => panic!("comp0 must be Real, got {:?}", other),
                };
                let c1 = match &comps[1] {
                    Value::Real(v) => *v,
                    other => panic!("comp1 must be Real, got {:?}", other),
                };
                assert!((c0 - 1.0).abs() < 1e-12, "comp0 at grid node (1,2) must be 1.0, got {c0}");
                assert!((c1 - 2.0).abs() < 1e-12, "comp1 at grid node (1,2) must be 2.0, got {c1}");
            }
            other => panic!("expected Value::Vector, got {:?}", other),
        }
    }
}
