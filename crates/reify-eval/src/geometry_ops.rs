// Geometry operation compilation: evaluates CompiledGeometryOp into runtime GeometryOp.
//
// Free functions with no Engine coupling — they take values, functions, meta_map
// as plain arguments.

use std::collections::{BTreeMap, HashMap, HashSet};

use reify_core::Diagnostic;
use reify_ir::{CompiledFunction, GeometryHandleId, GeometryKernel, KernelHandle, ValueMap};

use crate::eval_ctx_with_meta;

/// Minimum meaningful distance in meters (1 picometer).
///
/// Distances with `|v| < DEGENERATE_LENGTH_M` cannot produce a well-defined
/// solid — any kernel attempting to extrude / sweep at sub-picometer lengths
/// is likely to return an opaque error. Named constants (not bare literals)
/// also let future refactors relocate the tolerance without a regex sweep.
/// Boundary semantics are pinned by
/// `build_extrude_distance_{just_below,at}_threshold_*` tests.
pub(crate) const DEGENERATE_LENGTH_M: f64 = 1e-12;

/// Minimum meaningful angle in radians (sub-picoradian).
///
/// Revolve angles with `|a| < DEGENERATE_ANGLE_RAD` cannot produce a
/// well-defined revolved solid. Boundary semantics are pinned by
/// `build_revolve_angle_*_threshold_*` tests.
pub(crate) const DEGENERATE_ANGLE_RAD: f64 = 1e-12;

/// Generic geometry epsilon for axis-magnitude / direction-vector checks
/// (e.g. rejecting near-zero revolve axes).
pub(crate) const GEOMETRY_EPSILON: f64 = 1e-12;

/// Routing outcome returned by [`gate_query_capability`].
///
/// Maps directly to the downstream dispatcher choice:
/// - `Occt` → invoke the OCCT BRep kernel
/// - `Manifold` → invoke the Manifold Mesh kernel
/// - `Unsupported` → fail closed; the caller maps this to `None` so the cell
///   retains `Value::Undef` (the existing fall-through-is-preservation
///   contract invariant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // used in #[cfg(test)] and by downstream dispatcher tasks (KGQ-ο/π/ρ)
pub(crate) enum CapabilityRoute {
    /// Route to the OCCT BRep kernel.
    Occt,
    /// Route to the Manifold Mesh kernel.
    Manifold,
    /// Query is unsupported for this repr; fail closed.
    ///
    /// The gate has already pushed a `Diagnostic::error` carrying
    /// [`reify_types::DiagnosticCode::QueryNotSupportedOnRepr`] onto the
    /// diagnostics vec.
    Unsupported,
}

/// Capability-gating decision + diagnostic helper (PRD §5.4).
///
/// Maps `(produced_repr, query.capability_kind())` to a [`CapabilityRoute`]:
///
/// | repr          | capability    | route       | diagnostic |
/// |---------------|---------------|-------------|------------|
/// | BRep          | BRepOnly      | Occt        | —          |
/// | BRep          | BRepAndMesh   | Occt        | —          |
/// | BRep          | MeshOnly      | Unsupported | Error      |
/// | Mesh          | MeshOnly      | Manifold    | —          |
/// | Mesh          | BRepAndMesh   | Manifold    | —          |
/// | Mesh          | BRepOnly      | Unsupported | Error      |
/// | Sdf/Voxel/VolumeMesh | any    | Unsupported | Error      |
///
/// # Fail-closed contract
///
/// Every `Unsupported` branch pushes exactly one
/// `Diagnostic::error(...).with_code(DiagnosticCode::QueryNotSupportedOnRepr)`
/// onto `diagnostics`, then returns `Unsupported`. The caller must map
/// `Unsupported` → `None` → `Value::Undef` (the existing fall-through-
/// is-preservation contract). This function never panics.
///
/// # Message text
///
/// The 'requires' clause is derived from the query's capability kind:
/// - `BRepOnly` → `'<name>' requires BRep representation; this geometry is realized as <repr>`
/// - `MeshOnly` → `'<name>' requires Mesh representation; this geometry is realized as <repr>`
/// - `BRepAndMesh` → `'<name>' requires BRep or Mesh representation; this geometry is realized as <repr>`
///
/// `query_display_name` is the user-written `.ri` helper name (e.g.
/// `"curvature"`, `"edge_length"`) — thread it like existing `&function.name`
/// callers. `produced_repr` is rendered via `{:?}` (`"Mesh"`, `"BRep"`, …).
///
/// # Exhaustiveness
///
/// The inner `match produced_repr` covers all five [`reify_types::ReprKind`]
/// variants explicitly (no `_` wildcard) so a future repr addition is a
/// compile error at this site.
#[allow(dead_code)] // used in #[cfg(test)] and by downstream dispatcher tasks (KGQ-ο/π/ρ)
pub(crate) fn gate_query_capability(
    query: &reify_ir::GeometryQuery,
    produced_repr: reify_ir::ReprKind,
    query_display_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> CapabilityRoute {
    use reify_core::DiagnosticCode;
    use reify_ir::{QueryCapability, ReprKind};

    let capability = query.capability_kind();

    // Derive the 'requires' clause from capability so the message accurately
    // describes recovery options: a BRepAndMesh query on Voxel can be recovered
    // by switching to either BRep or Mesh, not just BRep.
    let requires_clause = match capability {
        QueryCapability::BRepOnly => "requires BRep representation",
        QueryCapability::MeshOnly => "requires Mesh representation",
        QueryCapability::BRepAndMesh => "requires BRep or Mesh representation",
    };

    let unsupported = |diagnostics: &mut Vec<Diagnostic>| {
        diagnostics.push(
            Diagnostic::error(format!(
                "'{query_display_name}' {requires_clause}; \
                 this geometry is realized as {produced_repr:?}"
            ))
            .with_code(DiagnosticCode::QueryNotSupportedOnRepr),
        );
        CapabilityRoute::Unsupported
    };

    match produced_repr {
        ReprKind::BRep => match capability {
            QueryCapability::BRepOnly | QueryCapability::BRepAndMesh => CapabilityRoute::Occt,
            QueryCapability::MeshOnly => unsupported(diagnostics),
        },
        ReprKind::Mesh => match capability {
            QueryCapability::MeshOnly | QueryCapability::BRepAndMesh => CapabilityRoute::Manifold,
            QueryCapability::BRepOnly => unsupported(diagnostics),
        },
        // Sdf, Voxel, VolumeMesh: no query is currently supported;
        // fail closed for every capability to ensure a future repr addition
        // is consciously classified here (no wildcard).
        ReprKind::Sdf => unsupported(diagnostics),
        ReprKind::Voxel => unsupported(diagnostics),
        ReprKind::VolumeMesh => unsupported(diagnostics),
    }
}

/// Look up a named argument in `args`, evaluate it, and return the resulting
/// `Value`.  If the argument is absent, push a `Warning` diagnostic and return
/// `None`.  Callers that need a finite `f64` should use [`eval_named_arg_f64`],
/// which also emits a `Warning` when the value is non-numeric or non-finite.
///
/// Fail-fast / anti-cascade contract: the caller is expected to propagate the
/// `None` via `.ok_or_else(...)?` so `compile_geometry_op` short-circuits with
/// a single Error before any downstream type-coercion check can fire. This
/// produces exactly one Warning + one Error per missing arg — no
/// "expected Geometry, found Undef" cascade. That invariant is regression-locked
/// by `build_primitive_missing_arg_emits_exactly_one_compile_warning` in
/// `tests/geometry_error_handling.rs`.
pub(crate) fn eval_named_arg(
    name: &str,
    kind_label: impl std::fmt::Display,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match args.iter().find(|(n, _)| n == name) {
        Some((_, expr)) => Some(reify_expr::eval_expr(
            expr,
            &eval_ctx_with_meta(values, functions, meta_map),
        )),
        None => {
            diagnostics.push(Diagnostic::warning(format!(
                "missing required geometry argument '{}' for {}",
                name, kind_label
            )));
            None
        }
    }
}

/// Look up a named argument, evaluate it, and convert to a finite `f64`.
/// Returns `None` with a diagnostic when the argument is absent (delegated
/// to [`eval_named_arg`]) or when the argument is present but evaluates to a
/// non-numeric or non-finite value (NaN, ±Infinity, or a non-`f64` type such
/// as `String` or `Bool`).  In the latter case a `Warning` diagnostic is
/// pushed with the message `"argument '{name}' for {kind} evaluated to
/// non-numeric/non-finite value"`.
///
/// Non-numeric / non-finite path coverage is locked by
/// `eval_named_arg_f64_{undef,nan,infinity}_value_returns_none_with_warning`.
pub(crate) fn eval_named_arg_f64(
    name: &str,
    kind_label: impl std::fmt::Display + Copy,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    let value = eval_named_arg(
        name,
        kind_label,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )?;
    match value.as_f64() {
        Some(v) if v.is_finite() => Some(v),
        _ => {
            diagnostics.push(Diagnostic::warning(format!(
                "argument '{}' for {} evaluated to non-numeric/non-finite value",
                name, kind_label
            )));
            None
        }
    }
}

/// Evaluate all args in a variadic curve constructor to f64 values.
///
/// Returns `None` if any arg evaluates to a non-finite value, pushing a
/// warning diagnostic for each bad arg.  Used by InterpCurve, BezierCurve,
/// and NurbsCurve to avoid duplicating the same eval-and-collect loop.
pub(crate) fn eval_all_args_to_f64(
    label: &str,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<f64>> {
    args.iter()
        .map(|(name, expr)| {
            let v = reify_expr::eval_expr(expr, &eval_ctx_with_meta(values, functions, meta_map));
            match v.as_f64() {
                Some(f) if f.is_finite() => Some(f),
                _ => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "{} arg '{}' is non-finite",
                        label, name
                    )));
                    None
                }
            }
        })
        .collect()
}

/// Canonicalize sub-handle `kernel_handle` ids into the canonical edge/face
/// order: ascending `kernel_handle` id, deduplicated. Single source of truth
/// for the "canonical order + dedup" step, shared by `resolve_subhandle_list`
/// (which layers the cross-solid membership gate on top) and the
/// `compile_geometry_op` `ModifyKind::Fillet` eval arm, so the two never drift
/// on ordering/dedup (task 3205 reviewer note). Ascending id matches
/// `extract_edges`' TopExp mint order, so a curated subset lines up with the
/// kernel's edge map.
fn canonical_subhandle_ids(
    ids: impl IntoIterator<Item = GeometryHandleId>,
) -> Vec<GeometryHandleId> {
    // `BTreeSet` gives dedup (by id) + ascending canonical order in a single
    // structure — `GeometryHandleId` is `Ord`.
    ids.into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Lower a `List<Geometry>` of KGQ topology sub-handles to a canonical
/// `Vec<GeometryHandleId>` (task 3205 — the curated edge/face SELECTION SEAM).
///
/// This helper is **kernel-free** (pure `Value` → `Value`): it never touches
/// the geometry kernel, so it is callable from BOTH [`compile_geometry_op`]
/// (the legacy eval lowering site, which has no kernel parameter and runs in a
/// phase where the parent shape is not yet realized) AND the unified
/// build-DAG driver (engine-unified-build-dag task η).
///
/// The cross-solid gate is `realization_ref` equality: every KGQ sub-handle
/// inherits its parent solid's `realization_ref` unchanged (KGQ-η PRD §4
/// invariant i, see [`crate::topology_selectors::make_sub_handle`]), so a
/// handle minted from a different solid carries a different `realization_ref`
/// and is rejected. The hash domain for these sub-handles is
/// [`crate::topology_selectors::compose_sub_handle_hash`] /
/// [`crate::topology_selectors::SubKind`]; this resolver reads only the
/// already-built `realization_ref` + `kernel_handle`, so it needs no rehash.
///
/// Contract:
///   - `arg` MUST be a `Value::List`; any other shape is a hard `Err`.
///   - `parent` MUST be a `Value::GeometryHandle`; its `realization_ref` is the
///     membership key.
///   - every element MUST be a `Value::GeometryHandle` whose `realization_ref`
///     equals the parent's — an element from a different solid is `Err`
///     (cross-solid).
///   - the resulting ids are **deduped** by `kernel_handle` and returned in
///     **ascending canonical order** (matching `extract_edges`' TopExp mint
///     order, so a curated subset lines up with the kernel's edge map).
///   - an **empty** input list is a legitimate `Ok(vec![])`. The anti-zero-
///     edges (`E_EMPTY_SELECTION`) guard — which distinguishes "selector
///     present but resolved to nothing" from "no selector at all" — is the
///     eval arm's job, NOT this structural resolver's.
// `#[allow(dead_code)]`: forward-looking selection-resolver seam. The legacy
// eval arm (`compile_geometry_op`'s `ModifyKind::Fillet`) cannot call the full
// resolver because the parent solid's `Value::GeometryHandle` is not realized in
// phase P2 (it enters `values` only in P3 — see the task-3205 plan), so that arm
// shares only the structural `canonical_subhandle_ids` canonicalization and skips
// the cross-solid membership gate. The full cross-solid resolver is consumed by
// engine-unified-build-dag η/ε, whose in-loop driver has the realized parent
// handle. Exercised now by the `resolve_subhandle_list_*` unit tests below.
// TODO(tasks 4360/4358): drop this `#[allow(dead_code)]` once η/ε's in-loop
// driver calls `resolve_subhandle_list` from production code.
#[allow(dead_code)]
pub(crate) fn resolve_subhandle_list(
    arg: &reify_ir::Value,
    parent: &reify_ir::Value,
) -> Result<Vec<GeometryHandleId>, String> {
    let parent_ref = match parent {
        reify_ir::Value::GeometryHandle {
            realization_ref, ..
        } => realization_ref,
        other => {
            return Err(format!(
                "resolve_subhandle_list: parent must be a Geometry handle, got {:?}",
                other
            ));
        }
    };

    let elems = match arg {
        reify_ir::Value::List(elems) => elems,
        other => {
            return Err(format!(
                "resolve_subhandle_list: edge selector must be a List<Geometry>, got {:?}",
                other
            ));
        }
    };

    // Validate every element (cross-solid membership gate), collecting raw
    // kernel_handles; `canonical_subhandle_ids` then dedups + sorts them into
    // canonical order — the SAME canonicalization the eval Fillet arm uses, so
    // the two cannot drift on ordering/dedup.
    let mut ids: Vec<GeometryHandleId> = Vec::with_capacity(elems.len());
    for (i, elem) in elems.iter().enumerate() {
        match elem {
            reify_ir::Value::GeometryHandle {
                realization_ref,
                kernel_handle,
                ..
            } => {
                if realization_ref != parent_ref {
                    return Err(format!(
                        "resolve_subhandle_list: edge[{}] belongs to a different solid \
                         ({} != parent {}) — cross-solid edge selection is rejected",
                        i, realization_ref, parent_ref
                    ));
                }
                ids.push(*kernel_handle);
            }
            other => {
                return Err(format!(
                    "resolve_subhandle_list: edge[{}] must be a Geometry sub-handle, got {:?}",
                    i, other
                ));
            }
        }
    }

    Ok(canonical_subhandle_ids(ids))
}

/// Validate and convert a pattern count from f64 to usize.
///
/// Rejects non-positive values, non-integers, and values exceeding
/// a reasonable upper bound. Returns `Err` with a diagnostic when
/// the count is invalid.
fn validate_pattern_count(
    raw: f64,
    arg_name: &str,
    kind_label: impl std::fmt::Display,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<usize, String> {
    if raw < 1.0 {
        diagnostics.push(Diagnostic::warning(format!(
            "pattern {} dropped: {}={} is less than 1 (must be a positive integer)",
            kind_label, arg_name, raw
        )));
        return Err("invalid pattern count: less than 1".to_string());
    }
    if raw != raw.floor() {
        diagnostics.push(Diagnostic::warning(format!(
            "pattern {} dropped: {}={} is not an integer",
            kind_label, arg_name, raw
        )));
        return Err("invalid pattern count: not an integer".to_string());
    }
    if raw > 100_000.0 {
        diagnostics.push(Diagnostic::warning(format!(
            "pattern {} dropped: {}={} exceeds upper bound of 100000",
            kind_label, arg_name, raw
        )));
        return Err("invalid pattern count: exceeds upper bound".to_string());
    }
    Ok(raw as usize)
}

/// Extract three SI-valued `f64` components from a [`reify_ir::Value::Point`]
/// or [`reify_ir::Value::Vector`] with exactly 3 numeric, finite components.
///
/// Returns `None` if:
/// - the value is not a `Point` or `Vector`;
/// - it does not have exactly 3 components;
/// - any component does not yield a finite `f64` via [`reify_ir::Value::as_f64`].
///
/// Both `Point` (with LENGTH-dimensioned `Scalar` components — SI metres) and
/// `Vector` (with dimensionless `Real` components) pass through correctly
/// because `Value::as_f64` extracts `si_value` from `Scalar` and the raw
/// float from `Real`.
fn point3_components(value: &reify_ir::Value) -> Option<[f64; 3]> {
    let comps = match value {
        reify_ir::Value::Point(c) | reify_ir::Value::Vector(c) if c.len() == 3 => c,
        _ => return None,
    };
    let a = comps[0].as_f64().filter(|v| v.is_finite())?;
    let b = comps[1].as_f64().filter(|v| v.is_finite())?;
    let c = comps[2].as_f64().filter(|v| v.is_finite())?;
    Some([a, b, c])
}

/// Normalize a 3-component direction vector to unit length.
///
/// Returns `Err` when the vector magnitude is below [`GEOMETRY_EPSILON`]
/// (zero or near-zero), preventing a degenerate `[0,0,0]` normal from
/// propagating silently to the kernel.  The caller maps `Err(String)` to a
/// `Diagnostic::error` via the standard `Err(String)` → diagnostic idiom
/// (see `engine_build.rs`).
fn unit_vector3(v: [f64; 3]) -> Result<[f64; 3], String> {
    let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if mag < GEOMETRY_EPSILON {
        return Err(format!(
            "zero-magnitude vector [{:.6e}, {:.6e}, {:.6e}] cannot be normalized \
             to a unit direction",
            v[0], v[1], v[2]
        ));
    }
    Ok([v[0] / mag, v[1] / mag, v[2] / mag])
}

/// Decode a [`Value::Plane`] into `(origin, unit_normal)` — a pair of SI
/// metre triples returned as `([f64; 3], [f64; 3])`.
///
/// The normal is normalized to unit length.  Non-unit normals are accepted and
/// normalized silently (the plane equation is invariant to normal scale).
/// Zero-magnitude normals are always rejected.
///
/// # Returns
/// - `Ok((origin, unit_normal))` — origin in metres, normal dimensionless unit vector.
/// - `Err(message)` — for any of:
///   - wrong value variant (not `Value::Plane`), including `Value::Undef`;
///   - origin or normal with non-numeric / non-finite components;
///   - zero-magnitude normal.
///
/// # Visibility
/// `pub(crate)` — co-located with the mirror/circular_pattern eval consumers
/// and available to sibling modules in `reify-eval`.  Widened to `pub` only
/// when a cross-crate consumer lands (task 3465, design open).
pub(crate) fn decode_plane(value: &reify_ir::Value) -> Result<([f64; 3], [f64; 3]), String> {
    let (origin_val, normal_val) = match value {
        reify_ir::Value::Plane { origin, normal } => (origin.as_ref(), normal.as_ref()),
        other => {
            return Err(format!(
                "expected a Plane value, got {}",
                other
            ));
        }
    };
    let origin_arr = point3_components(origin_val).ok_or_else(|| {
        "Plane origin is not a valid 3-component numeric Point/Vector".to_string()
    })?;
    let normal_raw = point3_components(normal_val).ok_or_else(|| {
        "Plane normal is not a valid 3-component numeric Point/Vector".to_string()
    })?;
    let unit_normal = unit_vector3(normal_raw)
        .map_err(|e| format!("Plane has a degenerate normal: {e}"))?;
    Ok((origin_arr, unit_normal))
}

/// Decode a [`Value::Axis`] into `(origin, unit_direction)` — a pair of SI
/// metre triples returned as `([f64; 3], [f64; 3])`.
///
/// The direction vector is normalized to unit length.  Non-unit directions are
/// accepted and normalized silently.  Zero-magnitude directions are rejected.
///
/// # Returns
/// - `Ok((origin, unit_direction))` — origin in metres, direction a
///   dimensionless unit vector.
/// - `Err(message)` — for any of:
///   - wrong value variant (not `Value::Axis`), including `Value::Undef`;
///   - origin or direction with non-numeric / non-finite components;
///   - zero-magnitude direction.
///
/// Reuses the private helpers [`point3_components`] and [`unit_vector3`] from
/// [`decode_plane`] — the single canonical decode surface for Axis values
/// (task η, design decision A).
///
/// # Visibility
/// `pub(crate)` — widened to `pub` only when a cross-crate consumer lands
/// (task 3465, design open).
pub(crate) fn decode_axis(value: &reify_ir::Value) -> Result<([f64; 3], [f64; 3]), String> {
    let (origin_val, dir_val) = match value {
        reify_ir::Value::Axis { origin, direction } => (origin.as_ref(), direction.as_ref()),
        other => {
            return Err(format!(
                "expected an Axis value, got {}",
                other
            ));
        }
    };
    let origin_arr = point3_components(origin_val).ok_or_else(|| {
        "Axis origin is not a valid 3-component numeric Point/Vector".to_string()
    })?;
    let dir_raw = point3_components(dir_val).ok_or_else(|| {
        "Axis direction is not a valid 3-component numeric Point/Vector".to_string()
    })?;
    let unit_dir = unit_vector3(dir_raw)
        .map_err(|e| format!("Axis has a degenerate direction: {e}"))?;
    Ok((origin_arr, unit_dir))
}

/// Convert a bare-numeric angle [`reify_ir::Value`] to radians, emitting a
/// deprecation warning diagnostic.
///
/// CAD convention: a bare `Real` or `Int` angle (no unit suffix in source) is
/// interpreted as degrees and converted to radians.  Values that already carry
/// an `ANGLE` dimension (from `deg` / `rad` suffixes) pass through unchanged.
///
/// Extracted to a shared free function to prevent verbatim duplication between
/// the value-form and scalar-form branches of the `circular_pattern` eval arm.
fn resolve_bare_angle(raw: reify_ir::Value, diagnostics: &mut Vec<Diagnostic>) -> reify_ir::Value {
    let as_deg: Option<f64> = match &raw {
        reify_ir::Value::Real(v) => Some(*v),
        reify_ir::Value::Int(i) => Some(*i as f64),
        _ => None,
    };
    if let Some(deg) = as_deg {
        let rad = deg * std::f64::consts::PI / 180.0;
        diagnostics.push(Diagnostic::warning(format!(
            "circular_pattern: bare numeric angle `{}` interpreted as {}°; \
             use `{}deg` or `{:.6}rad` for explicit units",
            deg, deg, deg, rad
        )));
        reify_ir::Value::angle(rad)
    } else {
        raw
    }
}

/// Translate a compiled geometry operation into a runtime `GeometryOp` by
/// evaluating its argument expressions against the current value environment.
///
/// # Failure semantics and the silent-defaults convention
///
/// Returns `Err(reason)` — rather than `Ok` with a fabricated default — when
/// evaluation is incomplete: a required argument is absent, a value is
/// non-finite, a `GeomRef` cannot be resolved, or an arm-level validation
/// guard fires (e.g. negative scale factor, degenerate extrude distance,
/// zero-length revolve axis).
///
/// This is the intentional, convention-aligned alternative to silent defaults
/// (see `review/briefing.yaml` line 9 and project norm
/// `feedback_silent_defaults_pattern`, which forbids patterns like
/// `unwrap_or(Value::Undef)` or `unwrap_or(0.0)` that silently fabricate a
/// plausible-but-wrong value).  An `Err` propagates "evaluation is
/// incomplete" to the caller without inventing geometry the user never asked for.
///
/// ## Warning-then-propagate discipline
///
/// The error is never *silent* at its origin point.  Before each `Err`
/// escapes, a `Warning`-severity `Diagnostic` is pushed (by the helpers
/// [`eval_named_arg`] / [`eval_named_arg_f64`] for missing or non-finite
/// args, or by the arm-level validation guards for semantic failures).  The
/// `Err(String)` is a short *summary* the caller uses for its one
/// `Error`-severity diagnostic; the `Warning` carries the full, per-argument
/// explanation.
///
/// # Ordering invariant for `functions`
///
/// `functions` is the slice of [`CompiledFunction`]s from the module.  The
/// evaluator passes the *full* module-level slice so that any expression
/// inside an op's args can reference user-defined functions by index.
/// Forward references within the same structure are resolved during
/// compilation (name → index), so the slice must preserve declaration order
/// to keep indices valid.  Callers that construct a partial functions slice
/// (e.g. for testing) must ensure indices in compiled expressions stay
/// in-bounds or the lookup will silently return `Value::Undef`.
pub(crate) fn compile_geometry_op(
    op: &reify_compiler::CompiledGeometryOp,
    values: &ValueMap,
    step_handles: &[GeometryHandleId],
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};

    // Helper: resolve a GeomRef to a handle.
    //
    // GeomRef::Step(idx) — looks up in the per-realization step_handles slice.
    // GeomRef::Sub(name) — looks up in named_steps (name → handle built by the
    //   engine as each named realization completes).  On miss returns Err; no
    //   Warning diagnostic is emitted here — the caller (execute_realization_ops)
    //   emits a single Error-severity diagnostic per failed op.  This follows the
    //   "no Warning at origin, single Error at caller" convention documented in
    //   the `compile_geometry_op` doc-comment above.
    let resolve_geom_ref =
        |r: &GeomRef, step_handles: &[GeometryHandleId]| -> Result<GeometryHandleId, String> {
            match r {
            GeomRef::Step(idx) => step_handles
                .get(*idx)
                .copied()
                .filter(|h| *h != GeometryHandleId::INVALID)
                .ok_or_else(|| {
                    format!(
                        "unresolvable GeomRef::Step({}) — index out of bounds or INVALID handle",
                        idx
                    )
                }),
            // GeomRef::Sub(name) — look up the handle in the caller-supplied
            // named_steps map.  The map is populated by the engine as each
            // named realization completes (see execute_realization_ops).
            //
            // No Warning diagnostic at origin: on miss this arm returns
            // Err(String) and emits NO diagnostic.  The caller
            // (execute_realization_ops) converts the Err into a single
            // Error-severity diagnostic per failed op, consistent with the
            // "no Warning at origin, single Error at caller" convention
            // documented in the `compile_geometry_op` doc-comment.
            // Pinned by compile_geometry_op_sub_ref_unknown_name_returns_err_no_warning.
            GeomRef::Sub(name) => named_steps
                .get(name)
                .map(|kh| kh.id)
                .filter(|h| *h != GeometryHandleId::INVALID)
                .ok_or_else(|| {
                    format!(
                        "unresolvable GeomRef::Sub('{}') — no such named sub-reference in scope",
                        name
                    )
                }),
        }
        };

    match op {
        CompiledGeometryOp::Primitive { kind, args } => {
            let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
                eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
                    .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
            };

            match kind {
                PrimitiveKind::Box => Ok(reify_ir::GeometryOp::Box {
                    width: eval_arg("width")?,
                    height: eval_arg("height")?,
                    depth: eval_arg("depth")?,
                }),
                PrimitiveKind::Cylinder => Ok(reify_ir::GeometryOp::Cylinder {
                    radius: eval_arg("radius")?,
                    height: eval_arg("height")?,
                }),
                PrimitiveKind::Sphere => Ok(reify_ir::GeometryOp::Sphere {
                    radius: eval_arg("radius")?,
                }),
                PrimitiveKind::Tube => Ok(reify_ir::GeometryOp::Tube {
                    outer_r: eval_arg("outer_r")?,
                    inner_r: eval_arg("inner_r")?,
                    height: eval_arg("height")?,
                }),
                PrimitiveKind::Cone => Ok(reify_ir::GeometryOp::Cone {
                    bottom_radius: eval_arg("bottom_radius")?,
                    top_radius: eval_arg("top_radius")?,
                    height: eval_arg("height")?,
                }),
                PrimitiveKind::Wedge => Ok(reify_ir::GeometryOp::Wedge {
                    width: eval_arg("width")?,
                    depth: eval_arg("depth")?,
                    height: eval_arg("height")?,
                    top_width: eval_arg("top_width")?,
                }),
            }
        }
        CompiledGeometryOp::Boolean { op, left, right } => {
            // Fail-fast: `?` on `left` short-circuits before `right` is resolved,
            // so at most one "unresolvable GeomRef::Step" Error surfaces per
            // Boolean op. Pinned by
            // `build_boolean_{union,difference,intersection}_unresolved_*_no_kernel_error`
            // in `tests/geometry_error_handling.rs`.
            let left_id = resolve_geom_ref(left, step_handles)?;
            let right_id = resolve_geom_ref(right, step_handles)?;
            match op {
                BooleanOp::Union => Ok(reify_ir::GeometryOp::Union {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Difference => Ok(reify_ir::GeometryOp::Difference {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Intersection => Ok(reify_ir::GeometryOp::Intersection {
                    left: left_id,
                    right: right_id,
                }),
            }
        }
        CompiledGeometryOp::Modify { kind, target, args } => {
            let target_id = resolve_geom_ref(target, step_handles)?;
            let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
                eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
                    .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
            };
            match kind {
                reify_compiler::ModifyKind::Fillet => {
                    // Evaluate radius FIRST, while the `eval_arg` closure (which
                    // borrows `diagnostics` mutably) is still live — this keeps
                    // the missing-radius Warning behaviour identical to the
                    // 2-arg path.
                    let radius = eval_arg("radius")?;
                    // Is a curated edge selector present? Presence of an "edges"
                    // named arg distinguishes the 3-arg `fillet(solid, edges,
                    // radius)` form from the 2-arg `fillet(solid, radius)`
                    // back-compat form. `args` is shared-borrowed (compatible
                    // with the closure's shared borrow of `args`).
                    let edges_expr = args.iter().find(|(n, _)| n == "edges").map(|(_, e)| e);
                    // No explicit `drop(eval_arg)` is needed to release the
                    // closure's `&mut diagnostics` borrow: `eval_arg` is not used
                    // again on the Fillet path after the `radius` call above, so
                    // NLL ends its borrow here — letting the empty-selection arm
                    // below push its own EmptyEdgeSelection diagnostic. (An
                    // explicit `drop` of the non-Drop closure trips
                    // `clippy::drop_non_drop`.)
                    match edges_expr {
                        // 2-arg form: no selector → empty edges = all-edges
                        // back-compat (legacy `fillet(solid, radius)`).
                        None => Ok(reify_ir::GeometryOp::Fillet {
                            target: target_id,
                            edges: vec![],
                            radius,
                        }),
                        // 3-arg form: evaluate the selector and resolve it.
                        Some(expr) => {
                            let edges_val = reify_expr::eval_expr(
                                expr,
                                &eval_ctx_with_meta(values, functions, meta_map),
                            );
                            match &edges_val {
                                reify_ir::Value::List(elems) => {
                                    // Extract each sub-handle's kernel_handle,
                                    // ERRORING on any element that is NOT a
                                    // Geometry sub-handle — mirroring
                                    // `resolve_subhandle_list`'s strictness so a
                                    // partially-malformed selector (some handles,
                                    // some non-handles) surfaces an error rather
                                    // than silently filleting only the surviving
                                    // subset (the latent trap the task-3205
                                    // reviewer flagged: a `filter_map` here would
                                    // drop the bad elements and only an
                                    // ALL-dropped list would trip
                                    // EmptyEdgeSelection). `resolve_subhandle_list`
                                    // layers a cross-solid membership gate on top;
                                    // this legacy P2 arm cannot run that gate (the
                                    // parent handle is not realized here — full
                                    // parent-membership/cross-solid resolution is
                                    // engine-unified-build-dag η's in-loop job),
                                    // but it SHARES both the reject-non-handle
                                    // policy AND the `canonical_subhandle_ids`
                                    // (ascending order + dedup) canonicalization,
                                    // so the two never drift.
                                    let mut raw_ids: Vec<GeometryHandleId> =
                                        Vec::with_capacity(elems.len());
                                    for (i, e) in elems.iter().enumerate() {
                                        match e {
                                            reify_ir::Value::GeometryHandle {
                                                kernel_handle,
                                                ..
                                            } => raw_ids.push(*kernel_handle),
                                            other => {
                                                return Err(format!(
                                                    "fillet(solid, edges, radius): edge \
                                                     selector element [{}] is not a Geometry \
                                                     sub-handle (got {:?}) — the edge selector \
                                                     must be a List of edge handles",
                                                    i, other
                                                ));
                                            }
                                        }
                                    }
                                    let resolved = canonical_subhandle_ids(raw_ids);
                                    // ANTI-ZERO-EDGES: a present selector that
                                    // resolves to ZERO edges must NEVER silently
                                    // fall through to the all-edges path (the
                                    // task-3295 fake-done trap). Emit a blocking
                                    // E_EMPTY_SELECTION and return Err.
                                    if resolved.is_empty() {
                                        diagnostics.push(
                                            Diagnostic::error(
                                                "fillet(solid, edges, radius): edge selector \
                                                 resolved to zero edges — refusing to silently \
                                                 fillet all edges",
                                            )
                                            .with_code(
                                                reify_core::DiagnosticCode::EmptyEdgeSelection,
                                            ),
                                        );
                                        return Err(
                                            "fillet: edge selector resolved to zero edges"
                                                .to_string(),
                                        );
                                    }
                                    Ok(reify_ir::GeometryOp::Fillet {
                                        target: target_id,
                                        edges: resolved,
                                        radius,
                                    })
                                }
                                // The selector did not resolve to a List — on the
                                // legacy pipeline it is `Undef` (the edges
                                // selector resolves in P4, after this P2 arm).
                                // This is NOT an empty selection, so do NOT emit
                                // E_EMPTY_SELECTION (that would false-positive on
                                // every legacy 3-arg fillet); return a plain Err
                                // so the cell stays Undef and η resolves it
                                // in-loop.
                                //
                                // The message is deliberately USER-ACTIONABLE (not
                                // the old internal "did not resolve to a List"
                                // string): on the current pipeline this `Err` is
                                // surfaced verbatim as `failed to compile geometry
                                // operation: <msg>` (engine_build.rs), so a user
                                // who writes 3-arg `fillet(solid, edges, radius)`
                                // today gets a diagnostic that explains the
                                // staging and points at the 2-arg fallback. Pinned
                                // by `compile_geometry_op_fillet_legacy_selector_
                                // unresolved_is_user_actionable`. The
                                // engine-unified-build-dag η/ε work (tasks
                                // 4360/4358) removes this arm once the in-loop
                                // selector resolution lands.
                                other => Err(format!(
                                    "fillet(solid, edges, radius): curated edge selection is \
                                     not yet available on the current build pipeline — the edge \
                                     selector cannot be resolved at the point this fillet runs. \
                                     Use 2-arg fillet(solid, radius) to fillet all edges, or \
                                     wait for curated edge selection (engine-unified-build-dag \
                                     tasks 4360/4358). [edge selector evaluated to {:?}]",
                                    other
                                )),
                            }
                        }
                    }
                }
                reify_compiler::ModifyKind::Chamfer => Ok(reify_ir::GeometryOp::Chamfer {
                    target: target_id,
                    distance: eval_arg("distance")?,
                }),
                reify_compiler::ModifyKind::Shell => {
                    let thickness = eval_arg("thickness")?;
                    // Collect face indices from face_0, face_1, ...
                    // Non-numeric values (String, Bool, List, etc.) are skipped with a diagnostic.
                    // Non-finite (NaN, ±Infinity) and negative numeric values are also skipped.
                    let mut faces_to_remove: Vec<usize> = Vec::new();
                    for (name, expr) in args.iter().filter(|(n, _)| n.starts_with("face_")) {
                        let val = reify_expr::eval_expr(
                            expr,
                            &eval_ctx_with_meta(values, functions, meta_map),
                        );
                        // Arm ordering matters: -Infinity satisfies both !is_finite() AND < 0.0;
                        // the non-finite arm must come first so -Infinity is classified as
                        // non-finite rather than negative.
                        match val.as_f64() {
                            None => {
                                diagnostics.push(Diagnostic::warning(format!(
                                    "Shell face index '{}' is non-numeric — skipped",
                                    name
                                )));
                            }
                            Some(f) if !f.is_finite() => {
                                diagnostics.push(Diagnostic::warning(format!(
                                    "Shell face index '{}' is non-finite ({}) — skipped",
                                    name, f
                                )));
                            }
                            Some(f) if f < 0.0 => {
                                diagnostics.push(Diagnostic::warning(format!(
                                    "Shell face index '{}' is negative ({}) — skipped",
                                    name, f
                                )));
                            }
                            Some(f) if f != f.floor() => {
                                diagnostics.push(Diagnostic::warning(format!(
                                    "Shell face index '{}' is not an integer ({}) — skipped",
                                    name, f
                                )));
                            }
                            Some(f) if f > 1_000_000.0 => {
                                diagnostics.push(Diagnostic::warning(format!(
                                    "Shell face index '{}' exceeds upper bound of 1000000 ({}) — skipped",
                                    name, f
                                )));
                            }
                            Some(f) => {
                                faces_to_remove.push(f as usize);
                            }
                        }
                    }
                    Ok(reify_ir::GeometryOp::Shell {
                        target: target_id,
                        thickness,
                        faces_to_remove,
                    })
                }
                reify_compiler::ModifyKind::Draft => {
                    // Evaluate angle FIRST, while the `eval_arg` closure (which
                    // borrows `diagnostics` mutably) is still live — this keeps
                    // the missing-angle Warning behaviour identical to the 3-arg
                    // path.
                    let angle = eval_arg("angle")?;
                    // plane is resolved via step_handles.last() (a pre-existing
                    // approximation — plane_xy yields a Value::Plane, not a sub-op;
                    // the full plane-handle plumbing fix is out of scope for δ).
                    // Filter INVALID so a preceding compile failure (sentinel)
                    // propagates as Err here rather than forwarding INVALID to the
                    // kernel.
                    let plane_id = step_handles
                        .last()
                        .copied()
                        .filter(|h| *h != GeometryHandleId::INVALID)
                        .ok_or_else(|| "no valid plane handle available for Draft".to_string())?;
                    // Is a curated face selector present? Presence of a "faces"
                    // named arg distinguishes the 4-arg
                    // `draft(solid, faces, angle, neutral_plane)` form from the
                    // 3-arg `draft(solid, angle, neutral_plane)` back-compat form.
                    // `args` is shared-borrowed (compatible with the closure's
                    // shared borrow of `args`).
                    let faces_expr = args.iter().find(|(n, _)| n == "faces").map(|(_, e)| e);
                    // `eval_arg` is not used again on the Draft path after the
                    // `angle` call above, so NLL ends its `&mut diagnostics`
                    // borrow here — letting the empty-selection arm below push
                    // its own EmptyEdgeSelection diagnostic.
                    match faces_expr {
                        // 3-arg form: no selector → empty faces = all-draftable-
                        // faces back-compat (legacy `draft(solid, angle, plane)`).
                        None => Ok(reify_ir::GeometryOp::Draft {
                            target: target_id,
                            faces: vec![],
                            angle,
                            plane: plane_id,
                        }),
                        // 4-arg form: evaluate the selector and resolve it.
                        Some(expr) => {
                            let faces_val = reify_expr::eval_expr(
                                expr,
                                &eval_ctx_with_meta(values, functions, meta_map),
                            );
                            match &faces_val {
                                reify_ir::Value::List(elems) => {
                                    // Extract each sub-handle's kernel_handle,
                                    // ERRORING on any element that is NOT a
                                    // Geometry sub-handle — mirroring the Fillet
                                    // arm's reject-non-handle strictness so a
                                    // partially-malformed selector surfaces an
                                    // error rather than silently drafting only
                                    // the surviving handle subset (the latent
                                    // `filter_map` trap the task-3205 reviewer
                                    // flagged for Fillet). The cross-solid
                                    // membership gate is deferred to the
                                    // engine-unified-build-dag η/ε work (tasks
                                    // 4360/4358), matching the Fillet arm's
                                    // constraint.
                                    let mut raw_ids: Vec<GeometryHandleId> =
                                        Vec::with_capacity(elems.len());
                                    for (i, e) in elems.iter().enumerate() {
                                        match e {
                                            reify_ir::Value::GeometryHandle {
                                                kernel_handle,
                                                ..
                                            } => raw_ids.push(*kernel_handle),
                                            other => {
                                                return Err(format!(
                                                    "draft(solid, faces, angle, neutral_plane): \
                                                     face selector element [{}] is not a Geometry \
                                                     sub-handle (got {:?}) — the face selector \
                                                     must be a List of face handles",
                                                    i, other
                                                ));
                                            }
                                        }
                                    }
                                    let resolved = canonical_subhandle_ids(raw_ids);
                                    // ANTI-ZERO-FACES: a present selector that
                                    // resolves to ZERO faces must NEVER silently
                                    // fall through to the all-faces path (the
                                    // task-3295 fake-done trap). Emit a blocking
                                    // E_EMPTY_SELECTION and return Err.
                                    if resolved.is_empty() {
                                        diagnostics.push(
                                            Diagnostic::error(
                                                "draft(solid, faces, angle, neutral_plane): \
                                                 face selector resolved to zero faces — refusing \
                                                 to silently draft all faces",
                                            )
                                            .with_code(
                                                reify_core::DiagnosticCode::EmptyEdgeSelection,
                                            ),
                                        );
                                        return Err(
                                            "draft: face selector resolved to zero faces"
                                                .to_string(),
                                        );
                                    }
                                    Ok(reify_ir::GeometryOp::Draft {
                                        target: target_id,
                                        faces: resolved,
                                        angle,
                                        plane: plane_id,
                                    })
                                }
                                // The selector did not resolve to a List — on
                                // the legacy pipeline it is `Undef` (the faces
                                // selector resolves in P4, after this P2 arm).
                                // This is NOT an empty selection, so do NOT emit
                                // E_EMPTY_SELECTION (that would false-positive on
                                // every legacy 4-arg draft); return a plain Err
                                // so the cell stays Undef and future in-loop
                                // resolution can handle it.
                                //
                                // The message is deliberately USER-ACTIONABLE:
                                // names the 4-arg call form and points at the
                                // 3-arg all-faces fallback. Pinned by
                                // `compile_geometry_op_draft_legacy_selector_
                                // unresolved_is_user_actionable`.
                                other => Err(format!(
                                    "draft(solid, faces, angle, neutral_plane): curated \
                                     face selection is not yet available on the current \
                                     build pipeline — the face selector cannot be resolved \
                                     at the point this draft runs. Use 3-arg \
                                     draft(solid, angle, neutral_plane) to draft all \
                                     faces, or wait for curated face selection \
                                     (engine-unified-build-dag tasks 4360/4358). \
                                     [face selector evaluated to {:?}]",
                                    other
                                )),
                            }
                        }
                    }
                }
                reify_compiler::ModifyKind::Thicken => {
                    let offset = eval_arg("offset")?;
                    Ok(reify_ir::GeometryOp::Thicken {
                        target: target_id,
                        offset,
                    })
                }
                reify_compiler::ModifyKind::ZoneSlab => {
                    let width = eval_arg("width")?;
                    Ok(reify_ir::GeometryOp::ZoneSlab {
                        target: target_id,
                        width,
                    })
                }
                reify_compiler::ModifyKind::OffsetSolid => {
                    let distance = eval_arg("distance")?;
                    Ok(reify_ir::GeometryOp::OffsetSolid {
                        target: target_id,
                        distance,
                    })
                }
            }
        }
        CompiledGeometryOp::Transform { kind, target, args } => {
            let target_id = resolve_geom_ref(target, step_handles)?;
            let mut f64_arg = |name: &str| -> Result<f64, String> {
                eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                    .ok_or_else(|| {
                        format!("missing or non-finite argument '{}' for {}", name, kind)
                    })
            };
            match kind {
                reify_compiler::TransformKind::Translate => Ok(reify_ir::GeometryOp::Translate {
                    target: target_id,
                    dx: f64_arg("dx")?,
                    dy: f64_arg("dy")?,
                    dz: f64_arg("dz")?,
                }),
                reify_compiler::TransformKind::Rotate => Ok(reify_ir::GeometryOp::Rotate {
                    target: target_id,
                    axis: [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?],
                    // NOTE: bare numeric angle is passed through as-is (radians).
                    // circular_pattern converts bare numbers as degrees; aligning
                    // rotate/rotate_around/revolve is tracked as a follow-up task.
                    angle_rad: f64_arg("angle")?,
                }),
                reify_compiler::TransformKind::Scale => {
                    let factor = f64_arg("factor")?;
                    // Reject non-positive scale: OCCT SetScale with negative factor
                    // produces inside-out geometry (point-symmetry), not mirroring.
                    // Zero factor produces degenerate (zero-volume) geometry which
                    // can crash or misbehave in downstream OCCT operations.
                    if factor < 0.0 {
                        diagnostics.push(Diagnostic::warning(format!(
                            "scale dropped: factor={} is negative (must be positive)",
                            factor
                        )));
                        return Err("scale factor is negative".into());
                    }
                    if factor == 0.0 {
                        diagnostics.push(Diagnostic::warning(
                            "scale dropped: factor=0 produces degenerate \
                             (zero-volume) geometry (must be > 0)"
                                .to_string(),
                        ));
                        return Err("scale factor is zero (degenerate)".into());
                    }
                    Ok(reify_ir::GeometryOp::Scale {
                        target: target_id,
                        factor,
                    })
                }
                reify_compiler::TransformKind::RotateAround => {
                    Ok(reify_ir::GeometryOp::RotateAround {
                        target: target_id,
                        point: [f64_arg("px")?, f64_arg("py")?, f64_arg("pz")?],
                        axis: [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?],
                        // NOTE: bare numeric angle is passed through as-is (radians).
                        // circular_pattern converts bare numbers as degrees; aligning
                        // rotate/rotate_around/revolve is tracked as a follow-up task.
                        angle_rad: f64_arg("angle")?,
                    })
                }
            }
        }
        CompiledGeometryOp::Pattern { kind, target, args } => {
            let target_id = resolve_geom_ref(target, step_handles)?;
            match kind {
                reify_compiler::PatternKind::Linear => {
                    let mut f64_arg = |name: &str| -> Result<f64, String> {
                        eval_named_arg_f64(
                            name,
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing or non-finite argument '{}' for {}", name, kind)
                        })
                    };
                    let direction = [f64_arg("dx")?, f64_arg("dy")?, f64_arg("dz")?];
                    let count_raw = f64_arg("count")?;
                    let count = validate_pattern_count(count_raw, "count", kind, diagnostics)?;
                    let spacing = eval_named_arg(
                        "spacing",
                        kind,
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )
                    .ok_or_else(|| format!("missing required argument 'spacing' for {}", kind))?;
                    Ok(reify_ir::GeometryOp::LinearPattern {
                        target: target_id,
                        direction,
                        count,
                        spacing,
                    })
                }
                reify_compiler::PatternKind::Circular => {
                    if args.iter().any(|(n, _)| n == "axis") {
                        // Value form: circular_pattern(target, axis_value, count, angle)
                        let axis_val = eval_named_arg(
                            "axis",
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing required argument 'axis' for {}", kind)
                        })?;
                        let (axis_origin, axis_dir) = decode_axis(&axis_val)
                            .map_err(|e| format!("circular_pattern: {}", e))?;
                        let count_raw = eval_named_arg_f64(
                            "count",
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing or non-finite argument 'count' for {}", kind)
                        })?;
                        let count =
                            validate_pattern_count(count_raw, "count", kind, diagnostics)?;
                        let raw_angle = eval_named_arg(
                            "angle",
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing required argument 'angle' for {}", kind)
                        })?;
                        // CAD convention: bare numeric angle → degrees → radians with warning.
                        let angle = resolve_bare_angle(raw_angle, diagnostics);
                        Ok(reify_ir::GeometryOp::CircularPattern {
                            target: target_id,
                            axis_origin,
                            axis_dir,
                            count,
                            angle,
                        })
                    } else {
                        // Scalar form (back-compat): ox, oy, oz, ax, ay, az, count, angle
                        // Missing-arg coverage: build_circular_pattern_missing_{count,axis}_no_kernel_error
                        let mut f64_arg = |name: &str| -> Result<f64, String> {
                            eval_named_arg_f64(
                                name,
                                kind,
                                args,
                                values,
                                functions,
                                meta_map,
                                diagnostics,
                            )
                            .ok_or_else(|| {
                                format!(
                                    "missing or non-finite argument '{}' for {}",
                                    name, kind
                                )
                            })
                        };
                        let axis_origin = [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?];
                        let axis_dir = [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?];
                        let count_raw = f64_arg("count")?;
                        let count =
                            validate_pattern_count(count_raw, "count", kind, diagnostics)?;
                        let raw_angle = eval_named_arg(
                            "angle",
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing required argument 'angle' for {}", kind)
                        })?;
                        // CAD convention: same bare-angle path as the value form above.
                        let angle = resolve_bare_angle(raw_angle, diagnostics);
                        Ok(reify_ir::GeometryOp::CircularPattern {
                            target: target_id,
                            axis_origin,
                            axis_dir,
                            count,
                            angle,
                        })
                    }
                }
                reify_compiler::PatternKind::Mirror => {
                    if args.iter().any(|(n, _)| n == "plane") {
                        // Value form: mirror(target, plane_value)
                        let plane_val = eval_named_arg(
                            "plane",
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing required argument 'plane' for {}", kind)
                        })?;
                        let (plane_origin, plane_normal) = decode_plane(&plane_val)
                            .map_err(|e| format!("mirror: {}", e))?;
                        Ok(reify_ir::GeometryOp::Mirror {
                            target: target_id,
                            plane_origin,
                            plane_normal,
                        })
                    } else {
                        // Scalar form (back-compat): ox, oy, oz, nx, ny, nz
                        // Missing-arg coverage: build_mirror_pattern_missing_plane_origin_no_kernel_error
                        let mut f64_arg = |name: &str| -> Result<f64, String> {
                            eval_named_arg_f64(
                                name,
                                kind,
                                args,
                                values,
                                functions,
                                meta_map,
                                diagnostics,
                            )
                            .ok_or_else(|| {
                                format!(
                                    "missing or non-finite argument '{}' for {}",
                                    name, kind
                                )
                            })
                        };
                        Ok(reify_ir::GeometryOp::Mirror {
                            target: target_id,
                            plane_origin: [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?],
                            plane_normal: [f64_arg("nx")?, f64_arg("ny")?, f64_arg("nz")?],
                        })
                    }
                }
                reify_compiler::PatternKind::Linear2D => {
                    let mut f64_arg = |name: &str| -> Result<f64, String> {
                        eval_named_arg_f64(
                            name,
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing or non-finite argument '{}' for {}", name, kind)
                        })
                    };
                    let direction1 = [f64_arg("dx1")?, f64_arg("dy1")?, f64_arg("dz1")?];
                    let count1_raw = f64_arg("count1")?;
                    let count1 = validate_pattern_count(count1_raw, "count1", kind, diagnostics)?;
                    let spacing1 = eval_named_arg(
                        "spacing1",
                        kind,
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )
                    .ok_or_else(|| format!("missing required argument 'spacing1' for {}", kind))?;
                    let mut f64_arg = |name: &str| -> Result<f64, String> {
                        eval_named_arg_f64(
                            name,
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing or non-finite argument '{}' for {}", name, kind)
                        })
                    };
                    let direction2 = [f64_arg("dx2")?, f64_arg("dy2")?, f64_arg("dz2")?];
                    let count2_raw = f64_arg("count2")?;
                    let count2 = validate_pattern_count(count2_raw, "count2", kind, diagnostics)?;
                    let spacing2 = eval_named_arg(
                        "spacing2",
                        kind,
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )
                    .ok_or_else(|| format!("missing required argument 'spacing2' for {}", kind))?;
                    Ok(reify_ir::GeometryOp::LinearPattern2D {
                        target: target_id,
                        direction1,
                        count1,
                        spacing1,
                        direction2,
                        count2,
                        spacing2,
                    })
                }
                reify_compiler::PatternKind::Arbitrary => {
                    // Iterate named transform args: t0_dx, t0_dy, t0_dz, t1_dx, ...
                    let mut transforms = Vec::new();
                    let mut idx = 0;
                    loop {
                        let dx_name = format!("t{}_dx", idx);
                        // Check if this triple exists by looking for the dx arg
                        if !args.iter().any(|(name, _)| name == &dx_name) {
                            break;
                        }
                        let mut f64_arg = |name: &str| -> Result<f64, String> {
                            eval_named_arg_f64(
                                name,
                                kind,
                                args,
                                values,
                                functions,
                                meta_map,
                                diagnostics,
                            )
                            .ok_or_else(|| {
                                format!("missing or non-finite argument '{}' for {}", name, kind)
                            })
                        };
                        let dx = f64_arg(&format!("t{}_dx", idx))?;
                        let dy = f64_arg(&format!("t{}_dy", idx))?;
                        let dz = f64_arg(&format!("t{}_dz", idx))?;
                        transforms.push([dx, dy, dz]);
                        idx += 1;
                    }
                    if transforms.is_empty() {
                        return Err("ArbitraryPattern has no transforms".into());
                    }
                    Ok(reify_ir::GeometryOp::ArbitraryPattern {
                        target: target_id,
                        transforms,
                    })
                }
            }
        }
        CompiledGeometryOp::Sweep {
            kind,
            profiles,
            args,
        } => {
            match kind {
                reify_compiler::SweepKind::Loft => {
                    // Resolve each profile GeomRef to a handle via step_handles
                    let resolved: Result<Vec<GeometryHandleId>, String> = profiles
                        .iter()
                        .map(|r| resolve_geom_ref(r, step_handles))
                        .collect();
                    Ok(reify_ir::GeometryOp::Loft {
                        profiles: resolved?,
                    })
                }
                reify_compiler::SweepKind::Extrude => {
                    let profile_handle = resolve_geom_ref(
                        profiles
                            .first()
                            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
                        step_handles,
                    )?;
                    let distance = eval_named_arg(
                        "distance",
                        kind,
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )
                    .ok_or_else(|| format!("missing required argument 'distance' for {}", kind))?;
                    // Reject sub-picometer magnitudes as degenerate geometry: a
                    // distance near the f64 rounding floor cannot produce a
                    // meaningful solid. Emit a warning so model authors see why
                    // the op was dropped instead of only the caller's generic
                    // "failed to compile geometry operation" error.
                    //
                    // Boundary semantics: `v.abs() >= DEGENERATE_LENGTH_M` is an
                    // inclusive floor — a distance of exactly 1e-12 m is accepted;
                    // a distance of 1e-13 m is rejected. Pinned by
                    // `build_extrude_distance_{just_below,at}_threshold_*` in
                    // `tests/geometry_error_handling.rs`.
                    match distance.as_f64() {
                        Some(v) if v.is_finite() && v.abs() >= DEGENERATE_LENGTH_M => {}
                        Some(v) => {
                            // Threshold constant (`DEGENERATE_LENGTH_M`) is a source-level
                            // maintenance aid — user-facing diagnostics show only the numeric
                            // floor so model authors aren't distracted by the Rust name.
                            diagnostics.push(Diagnostic::warning(format!(
                                "extrude dropped: distance={} is degenerate \
                                 (|distance| must be finite and >= 1e-12 m)",
                                v
                            )));
                            return Err(format!("extrude distance is degenerate: {}", v));
                        }
                        None => return Err("extrude distance is non-numeric".into()),
                    }
                    Ok(reify_ir::GeometryOp::Extrude {
                        profile: profile_handle,
                        distance,
                    })
                }
                reify_compiler::SweepKind::Revolve => {
                    let profile_handle = resolve_geom_ref(
                        profiles
                            .first()
                            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
                        step_handles,
                    )?;
                    let mut f64_arg = |name: &str| -> Result<f64, String> {
                        eval_named_arg_f64(
                            name,
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing or non-finite argument '{}' for {}", name, kind)
                        })
                    };
                    let axis_dir = [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?];
                    let mag = axis_dir.iter().map(|x| x * x).sum::<f64>().sqrt();
                    // Reject sub-picometer axis magnitudes as degenerate: a
                    // zero-length (or effectively zero) rotation axis cannot
                    // define a revolve. Warn so model authors see a specific
                    // explanation instead of only the caller's generic error.
                    if !mag.is_finite() || mag < GEOMETRY_EPSILON {
                        // Threshold constant (`GEOMETRY_EPSILON`) is a source-level
                        // maintenance aid — user-facing diagnostics show only the numeric
                        // floor so model authors aren't distracted by the Rust name.
                        diagnostics.push(Diagnostic::warning(format!(
                            "revolve dropped: rotation axis [{}, {}, {}] has \
                             degenerate magnitude={} (must be finite and >= 1e-12)",
                            axis_dir[0], axis_dir[1], axis_dir[2], mag
                        )));
                        return Err(format!("revolve axis has degenerate magnitude: {}", mag));
                    }
                    // NOTE: bare numeric angle is passed through as-is (radians).
                    // circular_pattern converts bare numbers as degrees; aligning
                    // rotate/rotate_around/revolve is tracked as a follow-up task.
                    let angle_rad = f64_arg("angle")?;
                    // Reject sub-picoradian angles as degenerate: an angle at
                    // the f64 rounding floor cannot produce a meaningful
                    // revolve. Warn so model authors see a specific explanation.
                    //
                    // `.abs()` pins sign-symmetric semantics — small negative
                    // angles are rejected identically to small positive ones.
                    // See `build_revolve_angle_negative_just_below_threshold_rejected`
                    // in `tests/geometry_error_handling.rs`.
                    if angle_rad.abs() < DEGENERATE_ANGLE_RAD {
                        // Threshold constant (`DEGENERATE_ANGLE_RAD`) is a source-level
                        // maintenance aid — user-facing diagnostics show only the numeric
                        // floor so model authors aren't distracted by the Rust name.
                        diagnostics.push(Diagnostic::warning(format!(
                            "revolve dropped: angle={} rad is degenerate \
                             (|angle| must be >= 1e-12 rad)",
                            angle_rad
                        )));
                        return Err(format!("revolve angle is degenerate: {} rad", angle_rad));
                    }
                    let axis_origin = [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?];
                    Ok(reify_ir::GeometryOp::Revolve {
                        profile: profile_handle,
                        axis_origin,
                        axis_dir,
                        angle_rad,
                    })
                }
                reify_compiler::SweepKind::Sweep => {
                    let profile_handle = resolve_geom_ref(
                        profiles
                            .first()
                            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
                        step_handles,
                    )?;
                    let path_handle = resolve_geom_ref(
                        profiles
                            .get(1)
                            .ok_or_else(|| "no path GeomRef supplied".to_string())?,
                        step_handles,
                    )?;
                    Ok(reify_ir::GeometryOp::Sweep {
                        profile: profile_handle,
                        path: path_handle,
                    })
                }
                reify_compiler::SweepKind::ExtrudeSymmetric => {
                    let profile_handle = resolve_geom_ref(
                        profiles
                            .first()
                            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
                        step_handles,
                    )?;
                    let distance = eval_named_arg(
                        "distance",
                        kind,
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )
                    .ok_or_else(|| format!("missing required argument 'distance' for {}", kind))?;
                    // ExtrudeSymmetric extrudes `distance/2` each way, so the
                    // per-side magnitude must clear DEGENERATE_LENGTH_M; we
                    // require |distance| >= 2 * DEGENERATE_LENGTH_M so OCCT
                    // never receives a sub-picometer per-side length. A
                    // total-distance threshold would admit values like
                    // 1.5e-12 whose per-side half is below the floor and
                    // would still fail at the kernel with a less specific
                    // diagnostic.
                    // See: extrude_symmetric_per_side_just_below_threshold_rejected
                    //      / extrude_symmetric_per_side_at_threshold_accepted.
                    match distance.as_f64() {
                        // `.abs()` preserves sign-symmetric semantics — see
                        // extrude_symmetric_negative_per_side_just_below_threshold_rejected.
                        Some(v) if v.is_finite() && v.abs() >= 2.0 * DEGENERATE_LENGTH_M => {}
                        Some(v) => {
                            // Threshold constant (`DEGENERATE_LENGTH_M`) is a source-level
                            // maintenance aid — user-facing diagnostics show only the numeric
                            // floor so model authors aren't distracted by the Rust name.
                            // The Err string names the specific op (`extrude_symmetric`,
                            // not `extrude`) so the caller's "failed to compile geometry
                            // operation" Error channel matches the Warning — pinned by
                            // `extrude_symmetric_per_side_just_below_threshold_rejected`.
                            diagnostics.push(Diagnostic::warning(format!(
                                "extrude_symmetric dropped: distance={} is \
                                 degenerate (|distance/2| must be finite and >= 1e-12 m \
                                 per-side; i.e. |distance| >= 2e-12 m, half-distance floor)",
                                v
                            )));
                            return Err(format!("extrude_symmetric distance is degenerate: {}", v));
                        }
                        None => return Err("extrude_symmetric distance is non-numeric".into()),
                    }
                    Ok(reify_ir::GeometryOp::ExtrudeSymmetric {
                        profile: profile_handle,
                        distance,
                    })
                }
                reify_compiler::SweepKind::SweepGuided => {
                    let profile_handle = resolve_geom_ref(
                        profiles
                            .first()
                            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
                        step_handles,
                    )?;
                    let path_handle = resolve_geom_ref(
                        profiles
                            .get(1)
                            .ok_or_else(|| "no path GeomRef supplied".to_string())?,
                        step_handles,
                    )?;
                    let guide_handle = resolve_geom_ref(
                        profiles
                            .get(2)
                            .ok_or_else(|| "no guide GeomRef supplied".to_string())?,
                        step_handles,
                    )?;
                    Ok(reify_ir::GeometryOp::SweepGuided {
                        profile: profile_handle,
                        path: path_handle,
                        guide: guide_handle,
                    })
                }
                reify_compiler::SweepKind::LoftGuided => {
                    // The compiler packs the section profiles in
                    // `profiles[..len-1]` and the trailing guide as the
                    // final entry, matching the surface convention
                    // `loft_guided(p1, p2, ..., guide)`. Split here so
                    // GeometryOp::LoftGuided carries separate profiles +
                    // guides vecs.
                    if profiles.len() < 3 {
                        diagnostics.push(Diagnostic::warning(format!(
                            "loft_guided dropped: expected at least 2 \
                             profile refs + 1 guide ref (3 total), got {}",
                            profiles.len()
                        )));
                        return Err(format!(
                            "loft_guided requires at least 3 refs, got {}",
                            profiles.len()
                        ));
                    }
                    let guide_ref = profiles
                        .last()
                        .ok_or_else(|| "no guide GeomRef supplied".to_string())?;
                    let profile_refs = &profiles[..profiles.len() - 1];
                    let resolved_profiles: Result<Vec<GeometryHandleId>, String> = profile_refs
                        .iter()
                        .map(|r| resolve_geom_ref(r, step_handles))
                        .collect();
                    let resolved_profiles = resolved_profiles?;
                    let resolved_guide = resolve_geom_ref(guide_ref, step_handles)?;
                    Ok(reify_ir::GeometryOp::LoftGuided {
                        profiles: resolved_profiles,
                        guides: vec![resolved_guide],
                    })
                }
                reify_compiler::SweepKind::Pipe => {
                    // The path is resolved through `profiles[0]` (a GeomRef).
                    // `args` carries only "radius" (the scalar); no path placeholder
                    // exists here after task-383 S6 removed it from the compiler.
                    let path_handle = resolve_geom_ref(
                        profiles
                            .first()
                            .ok_or_else(|| "no path GeomRef supplied".to_string())?,
                        step_handles,
                    )?;
                    let radius = eval_named_arg(
                        "radius",
                        kind,
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )
                    .ok_or_else(|| format!("missing required argument 'radius' for {}", kind))?;
                    Ok(reify_ir::GeometryOp::Pipe {
                        path: path_handle,
                        radius,
                    })
                }
            }
        }
        CompiledGeometryOp::Curve { kind, args } => {
            use reify_compiler::CurveKind;
            match kind {
                CurveKind::LineSegment => {
                    let mut f64_arg = |name: &str| -> Result<f64, String> {
                        eval_named_arg_f64(
                            name,
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing or non-finite argument '{}' for {}", name, kind)
                        })
                    };
                    Ok(reify_ir::GeometryOp::LineSegment {
                        x1: f64_arg("x1")?,
                        y1: f64_arg("y1")?,
                        z1: f64_arg("z1")?,
                        x2: f64_arg("x2")?,
                        y2: f64_arg("y2")?,
                        z2: f64_arg("z2")?,
                    })
                }
                CurveKind::Arc => {
                    let mut f64_arg = |name: &str| -> Result<f64, String> {
                        eval_named_arg_f64(
                            name,
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing or non-finite argument '{}' for {}", name, kind)
                        })
                    };
                    Ok(reify_ir::GeometryOp::Arc {
                        center: [f64_arg("cx")?, f64_arg("cy")?, f64_arg("cz")?],
                        radius: f64_arg("radius")?,
                        start_angle: f64_arg("start_angle")?,
                        end_angle: f64_arg("end_angle")?,
                        axis: [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?],
                    })
                }
                CurveKind::Helix => {
                    let mut f64_arg = |name: &str| -> Result<f64, String> {
                        eval_named_arg_f64(
                            name,
                            kind,
                            args,
                            values,
                            functions,
                            meta_map,
                            diagnostics,
                        )
                        .ok_or_else(|| {
                            format!("missing or non-finite argument '{}' for {}", name, kind)
                        })
                    };
                    Ok(reify_ir::GeometryOp::Helix {
                        radius: f64_arg("radius")?,
                        pitch: f64_arg("pitch")?,
                        height: f64_arg("height")?,
                    })
                }
                CurveKind::InterpCurve => {
                    let coords = eval_all_args_to_f64(
                        "interp",
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )
                    .ok_or_else(|| "failed to evaluate all interp args to f64".to_string())?;
                    let points: Vec<[f64; 3]> =
                        coords.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
                    Ok(reify_ir::GeometryOp::InterpCurve { points })
                }
                CurveKind::BezierCurve => {
                    let coords = eval_all_args_to_f64(
                        "bezier",
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )
                    .ok_or_else(|| "failed to evaluate all bezier args to f64".to_string())?;
                    let control_points: Vec<[f64; 3]> =
                        coords.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
                    Ok(reify_ir::GeometryOp::BezierCurve { control_points })
                }
                CurveKind::NurbsCurve => {
                    // For NURBS, all args are passed positionally as c0,c1,...
                    // Format: first arg = degree, second = n_points, then
                    // n_points*3 pole coords, n_points weights, remaining knots.
                    let vals = eval_all_args_to_f64(
                        "nurbs",
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )
                    .ok_or_else(|| "failed to evaluate all nurbs args to f64".to_string())?;
                    if vals.len() < 2 {
                        diagnostics.push(Diagnostic::error(
                            "nurbs() requires at least degree and n_points arguments".to_string(),
                        ));
                        return Err("nurbs() requires at least degree and n_points".into());
                    }
                    // Validate degree is a positive integer
                    if vals[0] < 1.0 || vals[0] != vals[0].trunc() || vals[0] > 25.0 {
                        diagnostics.push(Diagnostic::error(format!(
                            "nurbs() degree must be a positive integer (1..25), got {}",
                            vals[0]
                        )));
                        return Err(format!("nurbs() degree invalid: {}", vals[0]));
                    }
                    let degree = vals[0] as usize;
                    // Remaining: need to know n_points to split.
                    // Convention: second val is n_points.
                    // Validate n_points is a positive integer within a sensible range
                    if vals[1] < 2.0 || vals[1] != vals[1].trunc() || vals[1] > (vals.len() as f64)
                    {
                        diagnostics.push(Diagnostic::error(
                            format!(
                                "nurbs() n_points must be a positive integer >= 2 and consistent with argument count, got {}",
                                vals[1]
                            ),
                        ));
                        return Err(format!("nurbs() n_points invalid: {}", vals[1]));
                    }
                    let n_points = vals[1] as usize;
                    let expected_min = 2 + n_points * 3 + n_points; // degree + n + poles + weights
                    if vals.len() < expected_min {
                        diagnostics.push(Diagnostic::error(format!(
                            "nurbs() got fewer arguments than expected for {} control points",
                            n_points,
                        )));
                        return Err(format!(
                            "nurbs() too few arguments for {} control points",
                            n_points
                        ));
                    }
                    let pole_start = 2;
                    let pole_end = pole_start + n_points * 3;
                    let weight_end = pole_end + n_points;
                    let control_points: Vec<[f64; 3]> = vals[pole_start..pole_end]
                        .chunks_exact(3)
                        .map(|c| [c[0], c[1], c[2]])
                        .collect();
                    let weights: Vec<f64> = vals[pole_end..weight_end].to_vec();
                    let knots: Vec<f64> = vals[weight_end..].to_vec();
                    if knots.is_empty() {
                        diagnostics.push(Diagnostic::error(
                            "nurbs() requires at least 1 knot value".to_string(),
                        ));
                        return Err("nurbs() requires at least 1 knot value".into());
                    }
                    let expected_knots = n_points + degree + 1;
                    if knots.len() != expected_knots {
                        diagnostics.push(Diagnostic::error(format!(
                            "nurbs() expected {} knots (n_points + degree + 1 = {} + {} + 1), got {}",
                            expected_knots, n_points, degree, knots.len(),
                        )));
                        return Err(format!(
                            "nurbs() wrong knot count: expected {}, got {}",
                            expected_knots,
                            knots.len()
                        ));
                    }
                    Ok(reify_ir::GeometryOp::NurbsCurve {
                        control_points,
                        weights,
                        knots,
                        degree,
                    })
                }
            }
        }
        CompiledGeometryOp::Profile { kind, args } => {
            use reify_compiler::ProfileKind;
            let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
                eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
                    .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
            };
            match kind {
                ProfileKind::Rectangle => Ok(reify_ir::GeometryOp::RectangleProfile {
                    width: eval_arg("width")?,
                    height: eval_arg("height")?,
                }),
                ProfileKind::Circle => Ok(reify_ir::GeometryOp::CircleProfile {
                    radius: eval_arg("radius")?,
                }),
            }
        }
    }
}

// ── Conformance-query dispatch (task 2320) ──────────────────────────────────
//
// `try_eval_conformance_query` is the kernel-aware eval-time dispatch for the
// stdlib helpers `is_watertight`, `is_manifold`, and `is_orientable`.
//
// Architecture: the helpers cannot be evaluated by the pure-value
// `eval_expr` / `eval_builtin` path because (a) `Type::Geometry` has no
// corresponding `Value` variant, and (b) the kernel — and therefore
// `GeometryHandleId`s — only exists behind `Engine.geometry_kernel`. The
// kernel-aware dispatch must live in the build / check pipeline where the
// engine has both the kernel and the realisation's per-name
// `GeometryHandleId` map (`named_steps`). This free function is invoked
// from `engine_build.rs` after `execute_realization_ops` has populated
// `named_steps` for a template, and patches the resulting `Value::Bool(_)`
// into the per-cell `ValueMap`.
//
// Helper-name → marker-trait pairing for the user-assertion escape hatch:
//   `is_watertight` ↔ `"Watertight"`
//   `is_manifold`   ↔ `"Manifold"`
//   `is_orientable` ↔ `"Orientable"`
// Note the asymmetry: `is_watertight` short-circuits **only** on
// `"Watertight"` — declaring `Closed` or `Manifold` (which `Watertight`
// refines per `geometry_traits.ri`) is not sufficient. Trait-DAG
// propagation is intentionally not done here; the simple name-equivalence
// rule mirrors task 2321's per-bound `W_TRAIT_USER_ASSERTED` warning.
//
// Returns:
//   `Some(Value::Bool(_))` when the dispatch produces a definite answer
//                          (kernel reply OR user-assertion override).
//   `Some(Value::Undef)`   when the kernel returned a non-`Bool` (defensive
//                          downgrade with a Warning diagnostic).
//   `None`                 when the expression is not a recognised
//                          conformance-query helper, or the arg shape is
//                          unsupported (literal, non-`ValueRef`,
//                          unresolvable cell-member name).  Callers fall
//                          through to the cell's compiled default.
pub(crate) fn try_eval_conformance_query(
    expr: &reify_ir::CompiledExpr,
    template_trait_bounds: &[String],
    named_steps: &HashMap<String, KernelHandle>,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // Early-return ordering audit (task 2320 step-8): the kernel is the last
    // step. Each guard below either short-circuits via `return None` (for
    // unsupported shapes) or short-circuits via `return Some(Bool(true))`
    // (for the user-assertion escape hatch). Pinned by the
    // `try_eval_conformance_query_*_returns_none_no_kernel_call` and
    // `try_eval_conformance_query_user_assertion_*` tests.
    //
    //   1. CompiledExprKind::FunctionCall          (cheapest — pattern match)
    //   2. recognised helper name                  (string compare)
    //   3. user-assertion escape hatch             (Vec::any string compare)
    //   4. single-arg ValueRef shape               (pattern match)
    //   5. named_steps cell-member lookup          (HashMap::get)
    //   6. kernel.query(...)                       (the actual round-trip)

    // (1) Must be a FunctionCall — anything else is unsupported.
    let (function, args) = match &expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
        _ => return None,
    };

    // (2) Must be one of the three recognised helper names. The pairing
    // with the matching marker trait is fixed.
    let marker_trait = match function.name.as_str() {
        "is_watertight" => "Watertight",
        "is_manifold" => "Manifold",
        "is_orientable" => "Orientable",
        _ => return None,
    };

    // (3) Escape hatch: if the enclosing structure declared the matching
    // marker trait, skip the kernel query entirely and return Bool(true).
    // This is intentionally checked *before* arg-shape resolution so the
    // user-assertion semantic holds even when the arg is otherwise
    // unresolvable.
    if template_trait_bounds.iter().any(|t| t == marker_trait) {
        return Some(reify_ir::Value::Bool(true));
    }

    // (4) Arg shape: we only resolve `is_watertight(<entity>.<member>)`
    // where `<member>` is a let-bound geometry name in `named_steps`.
    // Anything else (literals, nested expressions, cross-template idents)
    // falls through to `None` so the cell stays at its compiled default
    // (`Value::Undef`) — verified by the integration test
    // `is_watertight_with_literal_int_arg_falls_through_to_undef` in
    // `tests/conformance_runtime.rs` (task 2320 step-13/14).
    if args.len() != 1 {
        return None;
    }
    let cell_id = match &args[0].kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        // Defensive fall-through (task 2320 step-14): literals, nested
        // expressions, and any non-`ValueRef` shape bail to `None` *before*
        // any `named_steps` lookup or `kernel.query(...)` round-trip — so
        // ill-formed conformance-query call sites degrade gracefully rather
        // than panicking the build.
        _ => return None,
    };

    // (5) Resolve the cell-member name to a kernel handle. Absent →
    // `None` (and the kernel is never consulted).
    let handle = match named_steps.get(&cell_id.member) {
        Some(kh) => kh.id,
        None => return None,
    };

    // (6) All guards passed: build the matching kernel query and dispatch.
    let query = match function.name.as_str() {
        "is_watertight" => reify_ir::GeometryQuery::IsWatertight(handle),
        "is_manifold" => reify_ir::GeometryQuery::IsManifold(handle),
        "is_orientable" => reify_ir::GeometryQuery::IsOrientable(handle),
        // Unreachable — the earlier match already filtered to these three names.
        _ => return None,
    };

    match kernel.query(&query) {
        Ok(reify_ir::Value::Bool(b)) => Some(reify_ir::Value::Bool(b)),
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}({}) kernel returned non-Bool value {:?}; treating as undefined",
                function.name, cell_id.member, other
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}({}) kernel query failed: {}",
                function.name, cell_id.member, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

// ── Whole-handle geometry-query dispatch (task 3608, GHR-ζ) ─────────────────
//
// `try_eval_geometry_query` is the kernel-aware eval-time dispatch for the
// stdlib whole-handle geometry queries `volume` / `area` / `centroid` /
// `bounding_box` on a `Value::GeometryHandle` (PRD
// `docs/prds/v0_3/geometry-handle-runtime.md` §8 Phase 6). Sibling to
// `try_eval_conformance_query` / `try_eval_topology_selector`, dispatched from
// `Engine::post_process_geometry_queries`.

/// Tessellation deflection forwarded to `GeometryQuery::MaxDeviation.tolerance`
/// when the `max_deviation(actual, nominal)` callable is evaluated.
///
/// Mirrors `Engine::DEFAULT_TESSELLATION_TOLERANCE` (engine_build.rs:3165 =
/// 0.0001 m). Kept local to confine ζ's eval footprint to geometry_ops.rs and
/// avoid locking the hot engine_build.rs for a const reference (ζ / C4, task
/// 4479). The test `max_deviation_tessellation_tolerance_pins_engine_default_value`
/// pins this value and documents the update procedure (see the test comment).
const MAX_DEVIATION_TESSELLATION_TOLERANCE_M: f64 = 0.0001;
//
// `length` / `perimeter` are deliberately NOT handled here: they are already
// delivered via the edge/face topology-selector path (`dispatch_edge_length` /
// `dispatch_perimeter`), and `GeometryQuery` has no whole-handle
// Length/Perimeter variant — routing them here would double-dispatch.
//
// Kernel reply contract (`reify-kernel-occt/src/lib.rs:2519`): Volume /
// SurfaceArea return `Value::Real` (SI m³ / m²); Centroid / BoundingBox return
// the canonical JSON `Value::String` wire format.
//
// Returns:
//   `Some(Value::Scalar { dimension: VOLUME/AREA, .. })` for volume / area,
//   `Some(Value::Point([length, length, length]))` for centroid,
//   `Some(Value::BoundingBox { min, max })` (two `Point3<Length>`) for bounding_box,
//   `Some(Value::Undef)` (with a Warning) when a handle arg resolves but the
//        kernel errors or replies with an unexpected type (PRD §4
//        defensive-downgrade contract),
//   `Some(_)` for the NESTED case — the folded value of the enclosing
//        expression (e.g. `Scalar<Mass>` for `mass = volume(g) * density`),
//   `None` when the expr neither IS, nor CONTAINS, a recognised whole-handle
//        geometry-query call (or, for the direct case, its single arg is
//        unresolvable) — the caller leaves the cell at its compiled default
//        (`Value::Undef`).
//
// Two shapes are handled (task 3608: step-2 = direct, step-10 = nested):
//   (a) DIRECT — the cell's `default_expr` IS a geometry-query call, e.g.
//       `centroid = centroid(geometry)`. Dispatched straight to the kernel.
//   (b) NESTED — the `default_expr` CONTAINS a geometry-query call inside a
//       larger expression, e.g. `mass = volume(geometry) * material.density`
//       (a `BinOp` whose left leaf is `volume(...)`). Every geometry-query
//       leaf is rewritten to a `Literal` of its dispatched Value, then the
//       enclosing expression is recomputed with the standard pure evaluator
//       (`reify_expr::eval_expr`): `Scalar<Volume> * Scalar<Density>`
//       recombines to `Scalar<Mass>` via the existing units arithmetic, and
//       `material.density` resolves against the already-evaluated `material`
//       StructureInstance cell in `values` (the eval pass that produced
//       `values` runs before this post-process — engine_build.rs:1802). The
//       frozen Physical spec shape (GHR-α) computes `mass` this way, so the
//       nested fold is what produces the terminal user-observable.
//
// Cross-cell factoring: `try_eval_geometry_query` itself does NOT re-evaluate
// dependent cells. If the geometry-query call is NOT lexically in the cell's
// own `default_expr` — e.g.:
//       let v = volume(geometry)       // (a) DIRECT — folds to Scalar<Volume>
//       let m = v * material.density   // BinOp of ValueRef(v) — NO query leaf
// then `m`'s expr contains no geometry-query `FunctionCall`, so
// `expr_contains_geometry_query` is `false`, this pass returns `None` for `m`.
// This post-process inserts ONLY into geometry-query cells. However, the
// subsequent `post_process_derived_lets` pass in `engine_build.rs` (task 4229)
// performs a fixpoint re-eval of Undef Let cells, which resolves cross-cell
// factoring: after `v` folds to `Scalar<Volume>`, `post_process_derived_lets`
// re-evaluates `m` and folds it to the correct value. This is pinned by
// `cross_cell_factored_dependent_folds_via_fixpoint`
// (tests/geometry_query_kernel_dispatch.rs).
//
// GHR-ζ does NOT route through `gate_query_capability` (task 3623): consistent
// with the existing selector-dispatch siblings, and all GHR-ζ fixtures realize
// as BRep so the gate would route `Occt` anyway. Wiring the gate is the KGQ
// dispatcher family's scope.
pub(crate) fn try_eval_geometry_query(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // ── Case (ζ): DIRECT 2-arg — `max_deviation(actual, nominal)`. Kept
    //    SEPARATE from the 1-arg `is_geometry_query_call` invariant (ζ / C4,
    //    task 4479). Returns `None` (cell keeps compiled default Value::Undef)
    //    when either arg is unresolvable (non-ValueRef literal or missing
    //    named_steps entry). This is a **deliberate** design choice matching
    //    the 2-arg sibling convention (min_clearance, distance, contains) —
    //    literal-arg calls are rejected by the type checker before reaching
    //    eval, so a silent None here avoids spurious Warning diagnostics on
    //    type-unsafe call patterns the compiler already surfaces. If usability
    //    concerns outweigh sibling consistency in a future revision, change
    //    the `?` operators to explicit `else { emit Warning; return
    //    Some(Value::Undef); }` guards. Scope: direct-call only;
    //    nested-arithmetic fold is out of scope (matches the
    //    min_clearance/kinematic-sibling convention).
    if let reify_ir::CompiledExprKind::FunctionCall { function, args } = &expr.kind
        && function.name == "max_deviation" && args.len() == 2
    {
        let actual = resolve_geometry_handle_arg(&args[0], named_steps)?;
        let nominal = resolve_geometry_handle_arg(&args[1], named_steps)?;
        let query = reify_ir::GeometryQuery::MaxDeviation {
            actual,
            nominal,
            tolerance: MAX_DEVIATION_TESSELLATION_TOLERANCE_M,
        };
        return dispatch_scalar_query(
            kernel,
            query,
            reify_core::DimensionVector::LENGTH,
            "max_deviation",
            diagnostics,
        );
    }

    // ── Case (a): DIRECT — the expr itself is a whole-handle geometry-query
    //    call. Dispatch and return the typed Value. Returns `None` when the
    //    single arg is unresolvable, so the cell keeps its compiled default —
    //    preserving the pre-step-10 (direct-only) contract exactly.
    if is_geometry_query_call(expr) {
        let (function, args) = match &expr.kind {
            reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
            _ => unreachable!("is_geometry_query_call guarantees a FunctionCall node"),
        };
        return dispatch_geometry_query_call(
            function.name.as_str(),
            args,
            named_steps,
            kernel,
            diagnostics,
        );
    }

    // ── Case (b): NESTED — fold a geometry-query call buried inside a larger
    //    expression. Skip cells with no geometry-query call anywhere (they keep
    //    their compiled default / belong to another pass).
    if !expr_contains_geometry_query(expr) {
        return None;
    }
    // Rewrite each geometry-query leaf to a Literal of its dispatched Value, then
    // recompute the enclosing expression with the standard pure evaluator.
    let rewritten = rewrite_geometry_queries(expr, named_steps, kernel, diagnostics);
    Some(reify_expr::eval_expr(
        &rewritten,
        &eval_ctx_with_meta(values, functions, meta_map),
    ))
}

/// `true` iff `expr` is a recognised whole-handle geometry-query call —
/// `volume` / `area` / `centroid` / `bounding_box` with exactly one arg. The
/// single source of truth for the recognised-name set, used to gate the
/// direct-dispatch path and to locate fold leaves in the nested path.
/// `length` / `perimeter` are intentionally excluded (topology-selector path —
/// see the module note above `try_eval_geometry_query`).
pub(crate) fn is_geometry_query_call(expr: &reify_ir::CompiledExpr) -> bool {
    matches!(
        &expr.kind,
        reify_ir::CompiledExprKind::FunctionCall { function, args }
            if args.len() == 1
                && matches!(
                    function.name.as_str(),
                    "volume" | "area" | "centroid" | "bounding_box"
                )
    )
}

/// `true` iff any node in `expr`'s tree is a geometry-query call (per
/// [`is_geometry_query_call`]). Drives the nested-fold gate: only expressions
/// that actually contain a query are rewritten + re-evaluated. Uses the
/// canonical `CompiledExpr::walk` traversal so new expr variants are covered
/// automatically.
fn expr_contains_geometry_query(expr: &reify_ir::CompiledExpr) -> bool {
    let mut found = false;
    expr.walk(&mut |node| {
        if is_geometry_query_call(node) {
            found = true;
        }
    });
    found
}

/// Dispatch a single recognised geometry-query call (its `function_name` + one
/// handle `args`) to the kernel and convert the reply to a typed Value. Shared
/// by the direct path (returned straight to the caller) and the nested-fold
/// rewrite (wrapped in a `Literal`).
///
/// Returns `None` when the single arg is unresolvable (literal, non-`ValueRef`,
/// missing `named_steps` entry) or the name is unrecognised; `Some(Value::Undef)`
/// (with a Warning) on a kernel error or unexpected reply type (PRD §4
/// defensive downgrade — see `dispatch_scalar_query` / `dispatch_bounding_box`).
fn dispatch_geometry_query_call(
    function_name: &str,
    args: &[reify_ir::CompiledExpr],
    named_steps: &HashMap<String, KernelHandle>,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // Exactly one handle arg, resolved via `named_steps` (hydrated + revalidated
    // by `post_process_geometry_handle_cells` before this pass). Unresolvable
    // (literal, non-`ValueRef`, missing entry) → `None`.
    if args.len() != 1 {
        return None;
    }
    let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
    match function_name {
        "volume" => dispatch_scalar_query(
            kernel,
            reify_ir::GeometryQuery::Volume(handle),
            reify_core::DimensionVector::VOLUME,
            "volume",
            diagnostics,
        ),
        "area" => dispatch_scalar_query(
            kernel,
            reify_ir::GeometryQuery::SurfaceArea(handle),
            reify_core::DimensionVector::AREA,
            "area",
            diagnostics,
        ),
        // Centroid returns the canonical JSON-Point3 (`{"x":_,"y":_,"z":_}`)
        // wire format; `dispatch_point3_length_reply` decodes it to a
        // `Point3<Length>` (shared with closest_point / center_of_mass), with the
        // same Warning + Undef defensive downgrade on kernel error / malformed reply.
        "centroid" => dispatch_point3_length_reply(
            kernel,
            &reify_ir::GeometryQuery::Centroid(handle),
            "centroid",
            diagnostics,
        ),
        // BoundingBox returns the 6-field JSON (`{"xmin":_,..,"zmax":_}`);
        // `dispatch_bounding_box` decodes it (reusing `parse_bbox_axis_extents`
        // per axis) into `Value::BoundingBox` of two `Point3<Length>` corners.
        "bounding_box" => dispatch_bounding_box(kernel, handle, diagnostics),
        // Unrecognised name — `is_geometry_query_call` gates the callers, so this
        // is unreachable in practice; return `None` defensively.
        _ => None,
    }
}

/// Rewrite every geometry-query leaf in `expr` to a `Literal` of its
/// kernel-dispatched Value, returning a fresh expression the standard pure
/// evaluator can fold. Used only by the nested case of
/// [`try_eval_geometry_query`].
///
/// A geometry-query leaf whose handle arg is unresolvable folds to
/// `Literal(Value::Undef)`, so the enclosing arithmetic propagates `Undef`
/// (strict Undef propagation in `eval_expr`).
///
/// Recursion descends through `BinOp` / `UnOp` — the arithmetic wrappers the
/// frozen Physical spec shape uses (`mass = volume(geometry) *
/// material.density`). Every other node kind is cloned unchanged: an off-path
/// subtree (no nested query, e.g. the `material.density` operand) is reproduced
/// exactly, and a query nested inside an un-handled wrapper kind stays unfolded
/// so `eval_expr` yields `Undef` — the same outcome as the cell's compiled
/// default (a conservative downgrade, never a wrong value). Extend this match
/// if a future trait nests a geometry query inside a richer wrapper.
///
/// PERFORMANCE: every geometry-query leaf is dispatched independently, so an
/// expression repeating an identical call (e.g. `volume(g) + volume(g)`) issues
/// one kernel round-trip per occurrence, and the enclosing
/// `post_process_geometry_queries` re-runs this rewrite on every build path
/// (including cache-hit builds). For the frozen Physical spec shape each query
/// cell holds a single call, so this is one cheap round-trip and negligible. If
/// these expressions grow, memoize dispatch results per
/// `(function_name, GeometryHandleId)` within a single rewrite so repeated
/// leaves reuse one round-trip — deliberately NOT done here as it is
/// unobservable at the current single-query scope.
fn rewrite_geometry_queries(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::CompiledExpr {
    match &expr.kind {
        // Geometry-query leaf → Literal of its dispatched Value.
        reify_ir::CompiledExprKind::FunctionCall { function, args }
            if is_geometry_query_call(expr) =>
        {
            let value = dispatch_geometry_query_call(
                function.name.as_str(),
                args,
                named_steps,
                kernel,
                diagnostics,
            )
            .unwrap_or(reify_ir::Value::Undef);
            reify_ir::CompiledExpr::literal(value, expr.result_type.clone())
        }
        reify_ir::CompiledExprKind::BinOp { op, left, right } => reify_ir::CompiledExpr::binop(
            *op,
            rewrite_geometry_queries(left, named_steps, kernel, diagnostics),
            rewrite_geometry_queries(right, named_steps, kernel, diagnostics),
            expr.result_type.clone(),
        ),
        reify_ir::CompiledExprKind::UnOp { op, operand } => reify_ir::CompiledExpr::unop(
            *op,
            rewrite_geometry_queries(operand, named_steps, kernel, diagnostics),
            expr.result_type.clone(),
        ),
        // Off-path subtree or un-handled wrapper kind — clone unchanged (see the
        // function doc for why this is conservative, never wrong).
        _ => expr.clone(),
    }
}

/// Issue a scalar-returning kernel query (`Volume` / `SurfaceArea` /
/// `MaxDeviation`) and wrap the `Value::Real` (or, defensively,
/// `Value::Scalar`) reply through the `Value::from_real_scalar` chokepoint: a
/// dimensioned result becomes `Value::Scalar { si_value, dimension }`, while a
/// dimensionless result collapses to `Value::Real` (Invariant V — no code path
/// constructs a `Value::Scalar { dimension.is_dimensionless() }`).
///
/// Returns `Some(Value::Undef)` + one Warning on:
/// - a kernel error,
/// - an unexpected reply type (PRD §4 defensive downgrade),
/// - a **non-finite or negative** kernel value — a degenerate result (NaN /
///   ±Inf) or a negative measurement (impossible for volume / area /
///   deviation) propagating as a valid `Scalar` would silently corrupt
///   downstream arithmetic; surfacing it as Undef + Warning matches PRD §4.
///
/// Mirrors `dispatch_edge_length`.
fn dispatch_scalar_query(
    kernel: &dyn reify_ir::GeometryKernel,
    query: reify_ir::GeometryQuery,
    dimension: reify_core::DimensionVector,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(&query) {
        // Both reply shapes — a bare `Real` and the defensive `Scalar` — carry a
        // single magnitude that must be finite and non-negative to stand for a
        // volume / area / deviation. Validate them identically (a NaN / ±Inf /
        // negative magnitude in EITHER shape is downgraded to Undef + Warning),
        // then collapse through the `from_real_scalar` chokepoint (dimensionless
        // → Value::Real, dimensioned → Value::Scalar; Invariant V).
        Ok(reify_ir::Value::Real(v)) | Ok(reify_ir::Value::Scalar { si_value: v, .. })
            if v.is_finite() && v >= 0.0 =>
        {
            Some(reify_ir::Value::from_real_scalar(v, dimension))
        }
        Ok(reify_ir::Value::Real(v)) | Ok(reify_ir::Value::Scalar { si_value: v, .. }) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name}(...) kernel returned a non-finite or negative value ({v}); \
                 cell left at Undef",
            )));
            Some(reify_ir::Value::Undef)
        }
        Ok(unexpected) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name}(...) kernel reply has unexpected type (expected Real, \
                 got {unexpected:?}); cell left at Undef",
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name}(...) kernel query failed: {err}",
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Issue a `BoundingBox` query and decode the canonical 6-field JSON reply
/// (`{"xmin":_,"ymin":_,"zmin":_,"xmax":_,"ymax":_,"zmax":_}`) into
/// `Value::BoundingBox { min, max }` — two `Point3<Length>` corners. Reuses
/// `topology_selectors::parse_bbox_axis_extents` once per axis (the same parser
/// the extremal selectors use) rather than introducing a new 6-field decoder.
///
/// Returns `Some(Value::Undef)` + one Warning on a kernel error or malformed
/// reply (PRD §4 defensive downgrade), mirroring the volume/area/centroid arms.
/// The `?`-chain unifies the kernel `query` error and the per-axis parse error
/// (both `reify_ir::QueryError`) into the single `Err` arm.
fn dispatch_bounding_box(
    kernel: &dyn reify_ir::GeometryKernel,
    handle: GeometryHandleId,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    fn decode(
        kernel: &dyn reify_ir::GeometryKernel,
        handle: GeometryHandleId,
    ) -> Result<reify_ir::Value, reify_ir::QueryError> {
        let reply = kernel.query(&reify_ir::GeometryQuery::BoundingBox(handle))?;
        let (xmin, xmax) = crate::topology_selectors::parse_bbox_axis_extents(&reply, b'x')?;
        let (ymin, ymax) = crate::topology_selectors::parse_bbox_axis_extents(&reply, b'y')?;
        let (zmin, zmax) = crate::topology_selectors::parse_bbox_axis_extents(&reply, b'z')?;
        Ok(reify_ir::Value::BoundingBox {
            min: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(xmin),
                reify_ir::Value::length(ymin),
                reify_ir::Value::length(zmin),
            ])),
            max: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(xmax),
                reify_ir::Value::length(ymax),
                reify_ir::Value::length(zmax),
            ])),
        })
    }

    match decode(kernel, handle) {
        Ok(value) => Some(value),
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "bounding_box(...) kernel query/parse failed: {err}; cell left at Undef",
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

// ── Kinematic-query dispatch (task 2531) ────────────────────────────────────
//
// `try_eval_kinematic_query` is the kernel-aware eval-time dispatch for the
// stdlib helpers `interferes`, `interferes_with`, `min_clearance` (kinematic-
// constraints PRD task 8). Sibling to `try_eval_conformance_query`.
//
// Each helper consumes a Snapshot let-cell (the FK-evaluated Map produced by
// `snapshot()`) plus, for the binary forms, two Int let-cells holding body
// ids. The Snapshot's body records carry a `solid: Value::String("name")`
// field — the helper resolves each `name` to a `GeometryHandleId` via the
// per-template `named_steps` map populated by `execute_realization_ops`.
// All three helpers share OCCT's `BRepExtrema_DistShapeShape` primitive
// (`GeometryQuery::Distance`) — `interferes_with` is just `Distance ≤ 0`.
//
// FK placement via ApplyTransform (task 3906 T8): the Snapshot's per-body
// `world_transform` IS applied to the OCCT shape before the distance probe via
// the shared `GeometryOp::ApplyTransform` primitive — the same path T5 static
// `at` placement uses (`decompose_transform_to_arrays` + `surface_subtree`
// identity short-circuit). Each non-identity body transform is applied ONCE
// (O(N) `ApplyTransform` ops) before the O(N²) pairwise `Distance` probes,
// so posed handle ids are reused across all pairs. Identity / missing /
// undecomposable `world_transform` falls back to the raw source handle with
// no kernel op — preserving `fixed()`-joint fixtures unchanged.
//
// Self-pair exclusion: `interferes` iterates pairs as `i < j` upper-triangular
// — excluding both `(a, a)` self-pairs and the duplicate `(b, a)` ordering.
// Same-chain-segment exclusion (parent/child immediate joints sharing a face)
// is not done here — task 8 acceptance only requires self-pair exclusion.
//
// Returns:
//   `Some(Value::List(_))`   for `interferes` — list of pair Maps
//                            `{ "a": Int, "b": Int }`. Empty when no pair
//                            satisfies `Distance ≤ 0`.
//   `Some(Value::Bool(_))`   for `interferes_with`.
//   `Some(Value::Scalar { dimension: LENGTH, .. })` for `min_clearance`.
//   `Some(Value::Undef)`     when arg shapes pass but a runtime resolution
//                            fails (unresolvable `solid` name in
//                            `named_steps`, kernel error, missing body id,
//                            etc.) — defensive downgrade with Warning.
//   `None`                   when the expression is not a recognised
//                            kinematic-query helper, or the arg shape is
//                            unsupported (literal, non-`ValueRef`, missing
//                            snapshot in `values`). Callers fall through to
//                            the cell's compiled default (`Value::Undef`).
pub(crate) fn try_eval_kinematic_query(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
    pose_cache: &mut HashMap<(GeometryHandleId, [u64; 4], [u64; 3]), GeometryHandleId>,
) -> Option<reify_ir::Value> {
    // (1) Must be a FunctionCall — anything else is unsupported.
    let (function, args) = match &expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
        _ => return None,
    };

    // (1b) Swept flat_map branch: flat_map(snaps, |s| [kin_helper(s, a, b)]).
    // Checked before the helper-name match (step 2) so the `flat_map` name
    // does not fall through to the `_ => return None` arm. Non-kinematic
    // flat_map lambdas (e.g. `center_of_mass`) return None from
    // `try_eval_swept_kinematic_query`, preserving the pure-eval value.
    if function.name == "flat_map" {
        return try_eval_swept_kinematic_query(
            args,
            named_steps,
            values,
            kernel,
            diagnostics,
            pose_cache,
        );
    }

    // (2) Must be one of the three recognised helper names.
    let helper = match function.name.as_str() {
        "interferes" => KinematicHelper::Interferes,
        "interferes_with" => KinematicHelper::InterferesWith,
        "min_clearance" => KinematicHelper::MinClearance,
        _ => return None,
    };

    // (3) Per-helper arity guard.
    let expected_args = helper.arity();
    if args.len() != expected_args {
        return None;
    }

    // (4) args[0] must be a `ValueRef` to a let-cell holding the Snapshot
    // Map. Literal / inline-call shapes fall through to None so the cell
    // stays at its compiled default (`Value::Undef`) — mirrors the
    // `try_eval_conformance_query` arg-shape contract.
    let snapshot_cell = match &args[0].kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    let snapshot_value = values.get(snapshot_cell)?;

    // For the binary forms, args[1] / args[2] are the `Value::Int` body ids
    // (evaluate-then-accept, task ε: an inline integer literal now works, and a
    // defined-but-wrong value emits a Warning rather than falling through
    // silently). Pulled out so the unary `interferes` arm doesn't pay for it.
    let body_id_args = if expected_args == 3 {
        let a = resolve_int_value_ref(&args[1], values, &function.name, "body_a", diagnostics)?;
        let b = resolve_int_value_ref(&args[2], values, &function.name, "body_b", diagnostics)?;
        Some((a, b))
    } else {
        None
    };

    // (5–7) Delegate the per-snapshot core (extract bodies → build id→handle
    // with FK ApplyTransform → dispatch per-helper) to
    // `eval_kinematic_on_snapshot`. The swept flat_map branch calls the same
    // function for each element of the snapshot list (task 3844).
    eval_kinematic_on_snapshot(
        helper,
        &function.name,
        snapshot_value,
        body_id_args,
        named_steps,
        kernel,
        diagnostics,
        pose_cache,
    )
}

/// Per-snapshot kinematic dispatch: resolve bodies from a Snapshot Map, apply
/// FK world_transforms via `GeometryOp::ApplyTransform`, and run the
/// per-helper kernel probe (`interferes`, `interferes_with`, `min_clearance`).
///
/// Extracted from `try_eval_kinematic_query` so the swept flat_map branch
/// (`try_eval_swept_kinematic_query`) can invoke the same per-snapshot logic
/// for each element of a snapshot list (task 3844, KCC-epsilon).
///
/// `fn_name` is used only in Warning diagnostics; pass the stdlib function
/// name (e.g. `"min_clearance"`) for readable messages.
///
/// Returns:
///   `Some(Value)` on success — List / Bool / length Scalar per helper.
///   `Some(Value::Undef)` when the snapshot Map is malformed or a kernel
///     operation fails — the caller receives the per-snapshot Undef rather
///     than collapsing the entire swept result.
#[allow(clippy::too_many_arguments)]
fn eval_kinematic_on_snapshot(
    helper: KinematicHelper,
    fn_name: &str,
    snapshot_value: &reify_ir::Value,
    body_id_args: Option<(i64, i64)>,
    named_steps: &HashMap<String, KernelHandle>,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
    pose_cache: &mut HashMap<(GeometryHandleId, [u64; 4], [u64; 3]), GeometryHandleId>,
) -> Option<reify_ir::Value> {
    // (5) Read the Snapshot's bodies list. Returns Some(Value::Undef) (not
    // None) when the cell value isn't a well-formed Snapshot — the stdlib
    // stub already validated this on the value-eval pass, so reaching here
    // with a non-Snapshot indicates a stale / mismatched cell rather than
    // a parser-time shape; surfacing Undef is more visible than silently
    // falling back to the compiled default.
    let bodies = match extract_snapshot_bodies(snapshot_value) {
        Some(b) => b,
        None => return Some(reify_ir::Value::Undef),
    };

    // (6) Build (id → handle) by resolving each body's `solid` String against
    // `named_steps`. Bodies whose `solid` doesn't appear in `named_steps`
    // (e.g. a snapshot of a mechanism whose source let-name was never
    // realised because the structure has no realization for it) are
    // skipped — the helper still works for the realised subset.
    let mut id_to_handle: Vec<(i64, GeometryHandleId)> = Vec::with_capacity(bodies.len());
    for body in bodies {
        let body_map = match body {
            reify_ir::Value::Map(m) => m,
            _ => return Some(reify_ir::Value::Undef),
        };
        let id = match body_map.get(&reify_ir::Value::String("id".to_string())) {
            Some(reify_ir::Value::Int(n)) => *n,
            _ => return Some(reify_ir::Value::Undef),
        };
        // Binary helpers only probe two specific body ids — skip all others to
        // avoid O(N-2) wasted ApplyTransform ops per query (e.g. a 50-body
        // snapshot calling min_clearance(s, a, b) would otherwise pose 48
        // irrelevant bodies). The unary `interferes` helper needs all bodies.
        if let Some((qid_a, qid_b)) = body_id_args
            && id != qid_a
            && id != qid_b
        {
            continue;
        }
        let solid_name = match body_map.get(&reify_ir::Value::String("solid".to_string())) {
            Some(reify_ir::Value::String(s)) => s,
            // Non-string `solid` (e.g. a stale `Value::Undef` from a body whose
            // source-let was a geometry call) is not resolvable here — skip the
            // body silently rather than collapsing the entire query to Undef.
            _ => continue,
        };
        if let Some(handle) = named_steps.get(solid_name) {
            let raw_id = handle.id;
            // Apply the body's FK world_transform (if present and decomposable)
            // via the shared ApplyTransform primitive so the distance probe
            // operates on FK-posed geometry. Identity/missing world_transform
            // falls back to the raw handle (no kernel op).
            let posed_id = if let Some(wt) =
                body_map.get(&reify_ir::Value::String("world_transform".to_string()))
            {
                match decompose_transform_to_arrays(wt) {
                    Some((rotation, translation))
                        if rotation != [1.0, 0.0, 0.0, 0.0] || translation != [0.0, 0.0, 0.0] =>
                    {
                        // Cache posed handles for the duration of the build
                        // pass: a typical structure calls interferes/interferes_with/min_clearance on
                        // the same snapshot, so without a cache each non-identity
                        // body is re-posed once per query (3× the kernel ops
                        // for the same geometry). The key is (source handle,
                        // rotation bits, translation bits) — bit-exact to avoid
                        // float equality pitfalls across calls.
                        let cache_key = (
                            raw_id,
                            rotation.map(f64::to_bits),
                            translation.map(f64::to_bits),
                        );
                        if let Some(&cached_id) = pose_cache.get(&cache_key) {
                            cached_id
                        } else {
                            match kernel.execute(&reify_ir::GeometryOp::ApplyTransform {
                                target: raw_id,
                                rotation,
                                translation,
                            }) {
                                Ok(posed) => {
                                    pose_cache.insert(cache_key, posed.id);
                                    posed.id
                                }
                                Err(e) => {
                                    // A partial pose would mix FK-posed and
                                    // unposed handles in the same pairwise
                                    // probe, yielding a geometrically
                                    // meaningless result. Collapse the whole
                                    // query to Undef (consistent with the
                                    // kernel_distance error arm) so the failure
                                    // is visible rather than silently wrong.
                                    diagnostics.push(Diagnostic::warning(format!(
                                        "{fn_name}: ApplyTransform failed for body '{solid_name}': {e}",
                                    )));
                                    return Some(reify_ir::Value::Undef);
                                }
                            }
                        }
                    }
                    _ => raw_id,
                }
            } else {
                raw_id
            };
            id_to_handle.push((id, posed_id));
        }
        // Bodies whose solid name isn't in named_steps: skipped (see comment
        // above the loop).
    }

    // (7) Dispatch per-helper.
    match helper {
        KinematicHelper::Interferes => {
            let mut pairs = Vec::new();
            for i in 0..id_to_handle.len() {
                for j in (i + 1)..id_to_handle.len() {
                    let (id_a, handle_a) = id_to_handle[i];
                    let (id_b, handle_b) = id_to_handle[j];
                    match kernel_distance(kernel, handle_a, handle_b, diagnostics, fn_name) {
                        Some(d) if d <= 0.0 => {
                            pairs.push(make_pair_map(id_a, id_b));
                        }
                        Some(_) => {}
                        // Kernel error already emitted a Warning diagnostic
                        // — collapse the whole query to Undef so the cell
                        // exposes the failure rather than a partial list.
                        None => return Some(reify_ir::Value::Undef),
                    }
                }
            }
            Some(reify_ir::Value::List(pairs))
        }
        KinematicHelper::InterferesWith => {
            let (id_a, id_b) = body_id_args.expect("3-arg form populated body_id_args");
            // Self-pair: per the PRD acceptance, "a single body's interference
            // with itself is not reported". Returning Bool(false) here is a
            // defensive fallback — typical user-code uses distinct ids.
            if id_a == id_b {
                return Some(reify_ir::Value::Bool(false));
            }
            let handle_a = match handle_for_id(&id_to_handle, id_a) {
                Some(h) => h,
                None => return Some(reify_ir::Value::Undef),
            };
            let handle_b = match handle_for_id(&id_to_handle, id_b) {
                Some(h) => h,
                None => return Some(reify_ir::Value::Undef),
            };
            match kernel_distance(kernel, handle_a, handle_b, diagnostics, fn_name) {
                Some(d) => Some(reify_ir::Value::Bool(d <= 0.0)),
                None => Some(reify_ir::Value::Undef),
            }
        }
        KinematicHelper::MinClearance => {
            let (id_a, id_b) = body_id_args.expect("3-arg form populated body_id_args");
            // Self-pair clearance is undefined — surfacing 0.0 would lie about
            // a degenerate input. Returning Undef pushes the user toward
            // distinct ids; pinned by the smoke-test self-pair arm.
            if id_a == id_b {
                return Some(reify_ir::Value::Undef);
            }
            let handle_a = match handle_for_id(&id_to_handle, id_a) {
                Some(h) => h,
                None => return Some(reify_ir::Value::Undef),
            };
            let handle_b = match handle_for_id(&id_to_handle, id_b) {
                Some(h) => h,
                None => return Some(reify_ir::Value::Undef),
            };
            match kernel_distance(kernel, handle_a, handle_b, diagnostics, fn_name) {
                Some(d) => Some(reify_ir::Value::length(d)),
                None => Some(reify_ir::Value::Undef),
            }
        }
    }
}

/// Swept kinematic-query dispatch for `flat_map(snaps, |s| [kin_helper(s, a, b)])`.
///
/// Called by `try_eval_kinematic_query` when the outer function name is
/// `flat_map`. Validates that:
///   - `args[0]` is a `ValueRef` to a `Value::List` of Snapshot Maps.
///   - `args[1]` is a `Lambda { param_ids: [s_id], body: ListLiteral([inner]) }`.
///   - `inner` is a binary kinematic helper call (`interferes_with` or
///     `min_clearance`) with `args[0] == ValueRef(s_id)` (the lambda param)
///     and `args[1..]` resolving to `Int` body ids in `values`.
///
/// On match: runs `eval_kinematic_on_snapshot` for each snapshot and returns
/// `Some(Value::List(results))` — one result per snapshot (Undef on per-
/// snapshot failure, rather than collapsing the whole list).
///
/// On any mismatch (non-kinematic inner, wrong shape, non-Int captures):
/// returns `None` so the cell keeps the pure-eval value (e.g.
/// `center_of_mass` swept cells computed by the regular eval pass).
///
/// The unary `interferes` swept form is intentionally not supported: it would
/// concatenate pair-lists ambiguously. Falls through to None.
fn try_eval_swept_kinematic_query(
    args: &[reify_ir::CompiledExpr],
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
    pose_cache: &mut HashMap<(GeometryHandleId, [u64; 4], [u64; 3]), GeometryHandleId>,
) -> Option<reify_ir::Value> {
    // flat_map must have exactly 2 args: (list_arg, lambda_arg).
    if args.len() != 2 {
        return None;
    }

    // args[0] must be a ValueRef to a list of Snapshots.
    let list_id = match &args[0].kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    let snapshots = match values.get(list_id) {
        Some(reify_ir::Value::List(snaps)) => snaps,
        _ => return None,
    };

    // args[1] must be a Lambda with exactly one parameter (the snapshot `s`).
    let (s_param_id, body) = match &args[1].kind {
        reify_ir::CompiledExprKind::Lambda { param_ids, body, .. } if param_ids.len() == 1 => {
            (&param_ids[0], body.as_ref())
        }
        _ => return None,
    };

    // Lambda body must be ListLiteral([inner]) — a single-element list.
    let inner = match &body.kind {
        reify_ir::CompiledExprKind::ListLiteral(elems) if elems.len() == 1 => &elems[0],
        _ => return None,
    };

    // inner must be a binary kinematic helper call with 3 args.
    let (inner_fn, inner_args) = match &inner.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => {
            (function, args.as_slice())
        }
        _ => return None,
    };
    let helper = match inner_fn.name.as_str() {
        "interferes_with" => KinematicHelper::InterferesWith,
        "min_clearance" => KinematicHelper::MinClearance,
        // Unary `interferes` and non-kinematic names (e.g. center_of_mass)
        // → fall through so the pure-eval value is preserved.
        _ => return None,
    };
    if inner_args.len() != 3 {
        return None;
    }

    // inner_args[0] must be ValueRef to the lambda parameter (the snapshot `s`).
    let arg0_ref = match &inner_args[0].kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    if arg0_ref != s_param_id {
        return None;
    }

    // inner_args[1] and [2] are the Int body ids (evaluate-then-accept, task ε:
    // an inline integer literal now works, and a defined-but-wrong value emits a
    // Warning rather than falling through silently).
    let id_a =
        resolve_int_value_ref(&inner_args[1], values, &inner_fn.name, "body_a", diagnostics)?;
    let id_b =
        resolve_int_value_ref(&inner_args[2], values, &inner_fn.name, "body_b", diagnostics)?;
    let body_id_args = Some((id_a, id_b));

    // For each snapshot in the list run the per-snapshot dispatch core and
    // collect results. Per-snapshot failures (None) become Value::Undef so
    // the list length is always equal to the snapshot count.
    let fn_name = inner_fn.name.as_str();
    let mut out: Vec<reify_ir::Value> = Vec::with_capacity(snapshots.len());
    for snap in snapshots {
        let result = eval_kinematic_on_snapshot(
            helper,
            fn_name,
            snap,
            body_id_args,
            named_steps,
            kernel,
            diagnostics,
            pose_cache,
        );
        out.push(result.unwrap_or(reify_ir::Value::Undef));
    }
    Some(reify_ir::Value::List(out))
}

#[derive(Clone, Copy)]
enum KinematicHelper {
    Interferes,
    InterferesWith,
    MinClearance,
}

impl KinematicHelper {
    fn arity(self) -> usize {
        match self {
            KinematicHelper::Interferes => 1,
            KinematicHelper::InterferesWith | KinematicHelper::MinClearance => 3,
        }
    }
}

/// Resolve a kinematic body-id arg (the `id_a` / `id_b` positionals of
/// `interferes_with` / `min_clearance`) to its `i64` value, emitting a
/// `Severity::Warning` when the caller passes a defined-but-wrong value.
///
/// Evaluate-then-accept (task ε): the arg expr is EVALUATED against `values`
/// (via [`eval_arg_value`]) and the resulting `Value` classified. A `ValueRef →
/// Value::Int` cell (the common `let id_a = …` form) reads the cell (now an
/// owned clone; see [`eval_arg_value`]) — functionally identical to the prior
/// `values.get(id)` path — while an inline integer expression now EVALUATES
/// rather than falling through to a silent `None`. The
/// γ-style "non-`ValueRef` shape → silent fall-through" contract is gone.
///
/// | evaluated arg value                              | return    | diagnostic?     |
/// |--------------------------------------------------|-----------|-----------------|
/// | `Value::Undef` (missing/Undef cell, user-fn arg) | `None`    | no — quiet      |
/// | `Value::Int(n)`                                  | `Some(n)` | no              |
/// | any other defined value (Real, Scalar, …)        | `None`    | yes — 1 Warning |
fn resolve_int_value_ref(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<i64> {
    use crate::arg_acceptance::ArgRejection;

    let value = eval_arg_value(expr, values);

    match value {
        // Quiet degradation: an Undef value (missing cell, or a user-fn/meta arg
        // the local ctx can't evaluate) returns None with no diagnostic —
        // behaviourally identical to the prior `values.get(id)` fall-through.
        reify_ir::Value::Undef => None,
        reify_ir::Value::Int(n) => Some(n),
        // Defined-but-wrong (non-Int): emit exactly one Warning naming
        // builtin/arg/Int/got (byte-uniform wording with the density / point /
        // vec3 / range paths).
        other => {
            diagnostics.push(Diagnostic::warning(
                ArgRejection {
                    got: int_got_label(&other),
                    expected: "Int",
                    migration_hint: None,
                }
                .message(builtin, arg_name),
            ));
            None
        }
    }
}

/// Dimension-qualified label for a `Value::Scalar`, mirroring
/// `arg_acceptance::value_short_label` so the `got` payload of task ε's
/// non-scalar resolvers (int / point / vec3 / range / string) reports the
/// SAME dimension-qualified Scalar wording as the density / scalar-dim paths
/// that route through `accept_arg` — e.g. `"MASS_DENSITY Scalar"`,
/// `"dimensionless Scalar"`, `"dimensioned Scalar"`. `value_short_label` is
/// module-private to `arg_acceptance` (owned by task δ, not modified here), so
/// the Scalar arm is replicated rather than shared.
fn scalar_got_label(dimension: &reify_core::DimensionVector) -> String {
    if dimension.is_dimensionless() {
        "dimensionless Scalar".to_string()
    } else if let Some(name) = dimension.canonical_name() {
        format!("{name} Scalar")
    } else {
        "dimensioned Scalar".to_string()
    }
}

/// Short human-readable label for a `Value` that failed Int classification,
/// used as the `got` field of the rejection diagnostic (task ε).
fn int_got_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => scalar_got_label(dimension),
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::String(_) => "String".to_string(),
        reify_ir::Value::Vector(_) => "Vector".to_string(),
        reify_ir::Value::Point(_) => "Point".to_string(),
        _ => "non-Int value".to_string(),
    }
}

/// Extract the `bodies` list from a Snapshot Map, validating
/// `kind="snapshot"`. Mirrors `reify_stdlib::snapshot::snapshot_bodies` —
/// duplicated here because the stdlib helper is module-private.
fn extract_snapshot_bodies(snap: &reify_ir::Value) -> Option<Vec<reify_ir::Value>> {
    let map = match snap {
        reify_ir::Value::Map(m) => m,
        _ => return None,
    };
    if map.get(&reify_ir::Value::String("kind".to_string()))
        != Some(&reify_ir::Value::String("snapshot".to_string()))
    {
        return None;
    }
    match map.get(&reify_ir::Value::String("bodies".to_string())) {
        Some(reify_ir::Value::List(b)) => Some(b.clone()),
        _ => None,
    }
}

fn handle_for_id(pairs: &[(i64, GeometryHandleId)], id: i64) -> Option<GeometryHandleId> {
    pairs.iter().find(|(i, _)| *i == id).map(|(_, h)| *h)
}

/// Build the `{ "a": Int, "b": Int }` pair Map returned by `interferes`.
/// Alphabetical key order matches `BTreeMap` iteration so that List
/// equality used in the smoke tests is stable across iterations.
fn make_pair_map(id_a: i64, id_b: i64) -> reify_ir::Value {
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        reify_ir::Value::String("a".to_string()),
        reify_ir::Value::Int(id_a),
    );
    m.insert(
        reify_ir::Value::String("b".to_string()),
        reify_ir::Value::Int(id_b),
    );
    reify_ir::Value::Map(m)
}

/// Issue a `GeometryQuery::Distance` against the kernel and reduce to a raw
/// SI metres f64. Returns `None` (and emits a Warning diagnostic) on kernel
/// error or when the kernel returns a non-numeric `Value` — caller maps
/// `None` to a defensive `Value::Undef`.
///
/// `pub(crate)` so `Engine::distance_between_placed` (engine_build.rs) can
/// reuse the same error-handling convention (T7 task 3905).
pub(crate) fn kernel_distance(
    kernel: &dyn reify_ir::GeometryKernel,
    from: GeometryHandleId,
    to: GeometryHandleId,
    diagnostics: &mut Vec<Diagnostic>,
    helper_name: &str,
) -> Option<f64> {
    let query = reify_ir::GeometryQuery::Distance { from, to };
    match kernel.query(&query) {
        Ok(reify_ir::Value::Real(d)) => Some(d),
        // Some kernels (e.g. test-support `MockGeometryKernel::with_distance_result`)
        // store the value as a length-dimensioned `Scalar` instead of a raw
        // `Real`. Read the SI value either way so the dispatch stays kernel-
        // agnostic; the dimension itself is unused (the helpers' return-side
        // dimension is fixed by the helper, not the kernel reply).
        Ok(reify_ir::Value::Scalar { si_value, .. }) => Some(si_value),
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel Distance({:?}, {:?}) returned non-numeric value {:?}; treating as undefined",
                helper_name, from, to, other
            )));
            None
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel Distance({:?}, {:?}) failed: {}",
                helper_name, from, to, err
            )));
            None
        }
    }
}

// ── Topology-selector dispatch (tasks 2324, 2699) ────────────────────────────
//
// `try_eval_topology_selector` is the kernel-aware eval-time dispatch for the
// topology-selector helper family (PRD `docs/prds/topology-selectors.md`
// §3.9). Sibling to `try_eval_conformance_query` and
// `try_eval_kinematic_query` — same arg-shape / fall-through contract.
//
// ── Which names get eval dispatch here (task 2324) ──────────────────────────
//
// The per-name `match` at step (2) below is the SOURCE OF TRUTH for which
// helpers get a kernel-routed `Value` payload — NOT the compile-time recogniser
// `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` in `reify_compiler::units` (which is the
// broader classification list).
//
// Currently dispatched:
//   `closest_point(point, geometry)` → `GeometryQuery::ClosestPointOnShape`
//   `is_on(point, geometry)`         → `GeometryQuery::PointOnShape`
//   `angle_between_surfaces(a, b)`   → `GeometryQuery::SurfaceAngle`
//   `angle(a, b)`                    → pure-math acos (task 3614, KGQ-ε)
//   `contains(solid, point)`         → `GeometryQuery::Contains` (task 3611, KGQ-β)
//   `geo_equiv(left, right, tol)`   → `GeometryQuery::GeoEquiv`  (task 3613, KGQ-δ)
//
// ── Which names are compile-time typed but NOT eval-dispatched (task 2699) ──
//
// Task 2699 added 11 names to `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` and
// `topology_selector_result_type`, wiring their compile-time cell types.
// They fall through the `_ => return None` arm at step (2) below, so the
// cell stays at the `Value::Undef` set by the regular eval path.
// `value_type_kind_matches` accepts `Value::Undef` for any type
// (`reify_eval::lib:196`), so the cell typechecks until task 2691 wires
// the actual dispatch arms here:
//   `edges` / `faces`                       → List<Geometry>  (task 2691)
//   `edges_by_length` / `faces_by_area`     → List<Geometry>  (task 2691)
//   `faces_by_normal` / `edges_parallel_to` → List<Geometry>  (task 2691)
//   `edges_at_height`                       → List<Geometry>  (task 2691)
//   `adjacent_faces` / `shared_edges`       → List<Geometry>  (task 2691)
//   `center_of_mass`                        → Point3<Length>  (task 2691)
//   `moment_of_inertia`                     → Tensor<2,3,MI>  (task 2691)
//
// Arg-shape contract (applies to all dispatched names):
//   - Both args must be `ValueRef`s — literal / inline-call shapes fall
//     through to `None` so the cell stays at its compiled default
//     (`Value::Undef`). Pinned by the
//     `try_eval_topology_selector_*_literal_args_falls_through_to_none`
//     unit tests.
//   - For `closest_point` / `is_on`: args[0] must resolve in `values` to a
//     `Value::Point` of three Length-dimensioned scalars; args[1] must
//     resolve in `named_steps` to a `GeometryHandleId` (let-bound geometry).
//   - For `angle_between_surfaces`: both args must resolve in `named_steps`
//     to a `GeometryHandleId`.
//
// Returns:
//   `Some(Value::Point(vec![length, length, length]))` for `closest_point`
//                          (parsed from the kernel's JSON-Point3 reply).
//   `Some(Value::Bool(_))` for `is_on`.
//   `Some(Value::Scalar { dimension: ANGLE, .. })` for
//                          `angle_between_surfaces` and `angle`.
//   `Some(Value::Undef)`   on a kernel error or a malformed kernel reply
//                          (defensive downgrade with a Warning diagnostic);
//                          also for `angle` with zero-length / non-finite
//                          input (Warning emitted, no kernel call).
//   `None`                 when the expression is not a recognised
//                          topology-selector helper, or the arg shape is
//                          unsupported. Callers fall through to the cell's
//                          compiled default.
/// Resolve a selector constructor's parent-solid argument (`arg[0]`) to a
/// [`reify_ir::value::GeometryHandleRef`] target for a kernel-FREE
/// `Value::Selector` leaf (task 4118 γ).
///
/// Reuses [`resolve_parent_geometry_handle_arg`] — which reads the realized
/// `Value::GeometryHandle` out of `values` — then repackages its three identity
/// fields as a `GeometryHandleRef`. Falls through to `None` (cell stays at
/// `Value::Undef`) when `arg[0]` is not yet a hydrated `Value::GeometryHandle`
/// (PRD invariant #2: never partial-construct a selector target).
fn resolve_selector_target(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<reify_ir::value::GeometryHandleRef> {
    let (realization_ref, upstream_values_hash, kernel_handle) =
        resolve_parent_geometry_handle_arg(expr, values)?;
    Some(reify_ir::value::GeometryHandleRef {
        realization_ref,
        upstream_values_hash,
        kernel_handle,
    })
}

/// Package a kernel-FREE leaf `Value::Selector` (task 4118 γ): the 7
/// predicate/all selector constructors evaluate to a typed
/// `Value::Selector(kind)` pairing the parent solid handle (`target`) with a
/// `LeafQuery` describing the predicate. NO kernel query is issued here — the
/// `Selector → List<Geometry>` resolution is deferred to the compiler-inserted
/// `ResolveSelector` coercion node, executed by `topology_selectors::resolve`
/// (K2/BT7: zero kernel queries during construction).
///
/// `kind` and `query.required_kind()` are statically matched at every call site
/// below, so the K1 kind-closure check in `SelectorValue::leaf` never fails in
/// practice; the defensive `Err` arm emits a Warning and leaves the cell at
/// `Undef` rather than silently dropping it.
fn build_leaf_selector(
    kind: reify_core::ty::SelectorKind,
    target: reify_ir::value::GeometryHandleRef,
    query: reify_ir::value::LeafQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match reify_ir::value::SelectorValue::leaf(kind, target, query) {
        Ok(sv) => Some(reify_ir::Value::Selector(sv)),
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name}: selector kind-closure violation ({err:?}); cell left at Undef"
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Kernel-bearing evaluation of the compiler-inserted `ResolveSelector`
/// coercion node and `IndexAccess` over a selector (task 4118 γ, step-6).
///
/// `ResolveSelector { selector }` → reconstruct the inner `Value::Selector`
/// (PREFERRED: inline from a nested selector `FunctionCall`, sidestepping
/// value-cell ordering; else a `ValueRef` to an already-patched selector cell),
/// call the single `topology_selectors::resolve` executor, and wrap the
/// canonical-order handle ids as a `Value::List` of `Value::GeometryHandle`
/// sub-handles via `make_sub_handle`.
///
/// `IndexAccess { object: ResolveSelector{..} | <selector FunctionCall>, index }`
/// → resolve the selector to its list then return the indexed element (the
/// `faces(s)[i]` curvature shape).
///
/// Returns `None` for any other expr shape (the geometry_ops `None`-means-skip
/// contract: the cell is left for a sibling pass / the pure eval path).
pub(crate) fn try_eval_resolve_selector(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match &expr.kind {
        reify_ir::CompiledExprKind::ResolveSelector { selector } => {
            resolve_selector_to_list(selector, named_steps, values, kernel, diagnostics)
        }
        reify_ir::CompiledExprKind::IndexAccess { object, index } => {
            // Only handle IndexAccess whose object is a selector / ResolveSelector;
            // ordinary collection indexing is owned by the pure eval_expr path.
            let inner_selector = match &object.kind {
                reify_ir::CompiledExprKind::ResolveSelector { selector } => selector.as_ref(),
                reify_ir::CompiledExprKind::FunctionCall { .. } => object.as_ref(),
                _ => return None,
            };
            match resolve_selector_to_list(
                inner_selector,
                named_steps,
                values,
                kernel,
                diagnostics,
            )? {
                reify_ir::Value::List(elems) => {
                    let idx = resolve_index_usize(index, values)?;
                    match elems.get(idx) {
                        Some(v) => Some(v.clone()),
                        None => {
                            diagnostics.push(Diagnostic::warning(format!(
                                "selector index {idx} out of bounds (len {}); cell left at Undef",
                                elems.len()
                            )));
                            Some(reify_ir::Value::Undef)
                        }
                    }
                }
                // resolve_selector_to_list downgraded to Undef (kernel error) —
                // propagate so the cell is visibly degraded rather than skipped.
                other => Some(other),
            }
        }
        // `single(<selector>)` (task 4118 γ): the single()/list-helper coercion
        // site (compiler step-10) wraps the selector argument in a
        // `ResolveSelector`, so a `single(faces_by_normal(...))` cell compiles to
        // `FunctionCall { "single", [ResolveSelector{..}] }`. The pure eval path
        // cannot resolve the inner `ResolveSelector` (no kernel), so resolve it
        // HERE and unwrap the unique element — yielding the `Geometry` handle that
        // `single`'s `single(List<Geometry>) → Geometry` contract promises. This
        // is the runtime half of the single()/list-helper coercion (the golden
        // `top = single(faces_by_normal(b, +Z, 1deg))` shape).
        //
        // LOCKSTEP with the compiler: the set of coercing list-helpers is named
        // by `reify_compiler::coerce::COERCING_LIST_HELPERS` (currently just
        // `single`). This arm is the runtime counterpart that constant's doc
        // requires — it is intentionally hard-pinned to `"single"` (not the whole
        // set) because the unwrap-the-unique-element logic below is `single`'s
        // specific `single(List<Geometry>) → Geometry` semantics. If a new
        // coercing helper is ever added to `COERCING_LIST_HELPERS`, it needs its
        // OWN arm here implementing that helper's semantics (e.g. `first` → index
        // 0), not a widening of this `== "single"` guard.
        reify_ir::CompiledExprKind::FunctionCall { function, args }
            if function.name == "single" && args.len() == 1 =>
        {
            let selector_expr = match &args[0].kind {
                reify_ir::CompiledExprKind::ResolveSelector { selector } => selector.as_ref(),
                // Defensive: a bare selector FunctionCall (un-coerced) — still ours.
                reify_ir::CompiledExprKind::FunctionCall { .. } => &args[0],
                // Any other arg shape (a real List, a ValueRef to a List, …) is
                // owned by the pure eval_expr path — skip.
                _ => return None,
            };
            match resolve_selector_to_list(
                selector_expr,
                named_steps,
                values,
                kernel,
                diagnostics,
            )? {
                reify_ir::Value::List(mut elems) => {
                    if elems.len() == 1 {
                        Some(elems.remove(0))
                    } else {
                        diagnostics.push(Diagnostic::warning(format!(
                            "single(...) expected exactly 1 element, got {}; cell left at Undef",
                            elems.len()
                        )));
                        Some(reify_ir::Value::Undef)
                    }
                }
                // resolve_selector_to_list downgraded to Undef (kernel error) —
                // propagate so the cell is visibly degraded rather than skipped.
                other => Some(other),
            }
        }
        _ => None,
    }
}

/// Feature → datum projection member names (geometric-relations ε): the four
/// projections a realized feature's trait bundle carries. LOCKSTEP with the
/// compiler typing table (`datum_projection.rs` `Type::Geometry`/`Selector` arm)
/// — these are exactly the members `datum_projection_result_type` resolves for a
/// feature receiver. Used to gate [`try_eval_feature_datum_projection`] so it
/// only intercepts feature→datum projection `MethodCall`s, leaving β's pure
/// datum→datum projections (and any other method call) to the pure eval path.
const FEATURE_DATUM_PROJECTION_MEMBERS: [&str; 4] = ["axis", "plane", "point", "dir"];

/// Kernel-backed evaluation of a feature → datum projection (`feature.axis` /
/// `.plane` / `.point` / `.dir`), geometric-relations ε (design §7.2). The
/// compiler lowers such a projection to a `MethodCall { object: <feature>,
/// method: <proj>, args: [] }` whose object is a realized `Value::GeometryHandle`
/// cell; the pure `eval_datum_projection` cannot evaluate it (it reaches the
/// kernel, the construction history, and the dedup primitive), so it is resolved
/// HERE, mirroring the `ResolveSelector` coercion in
/// [`try_eval_resolve_selector`].
///
/// Resolves the receiver to its feature handle, builds the deduplicated
/// [`feature_datum_bundle`](crate::feature_datum::feature_datum_bundle) from the
/// analytic ∪ construction-history union (the recovered [`SweptKind`] history is
/// looked up in `swept_kinds` by the feature handle), and refines it to the
/// requested projection via
/// [`feature_datum_projection`](crate::feature_datum::feature_datum_projection):
/// a unique datum is returned as its `Value`, a zero/many group emits a
/// select-a-subfeature [`DiagnosticCode::FeatureDatumAmbiguous`] error and yields
/// `Value::Undef`.
///
/// Returns `None` (skip — leave the cell for the pure eval path) when the expr is
/// not a feature→datum projection `MethodCall`, or when its receiver is a β
/// *datum* receiver (e.g. `axis.dir`, owned by `eval_datum_projection`) that does
/// not resolve to a realized `Value::GeometryHandle`.
///
/// A receiver that statically types as a topology *selector*
/// (`Type::Selector(_)` / `Type::AnySelector`) is accepted at compile time
/// (design §2.2 types a selection's feature→datum projection) but its
/// selector→sub-handle resolution is not yet wired on the eval side; rather than
/// leaving the cell a silent `Value::Undef`, it emits a select-a-subfeature
/// [`DiagnosticCode::FeatureDatumAmbiguous`] error and yields `Value::Undef`.
pub(crate) fn try_eval_feature_datum_projection(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    swept_kinds: &crate::sweep_classifier::SweptKindTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let (object, member) = match &expr.kind {
        reify_ir::CompiledExprKind::MethodCall {
            object,
            method,
            args,
        } if args.is_empty()
            && FEATURE_DATUM_PROJECTION_MEMBERS.contains(&method.as_str()) =>
        {
            (object.as_ref(), method.as_str())
        }
        _ => return None,
    };

    // Resolve the receiver to a realized feature handle. Only a feature receiver
    // backed by a realized `Value::GeometryHandle` cell is wired end-to-end; a β
    // datum receiver (`Axis`/…) does not resolve here, so we return None and the
    // pure `eval_datum_projection` path handles it.
    let handle = match resolve_selector_target(object, values) {
        Some(target) => target.kernel_handle,
        None => {
            // The receiver did not resolve to a realized geometry handle. Check
            // whether the receiver cell holds a hydrated `Value::Selector` (the
            // common post-hydration case where the topology-selector pass has
            // already written the cell before this feature-datum pass runs).
            if matches!(
                object.result_type,
                reify_core::ty::Type::Selector(_) | reify_core::ty::Type::AnySelector
            ) {
                // Try to read the hydrated Value::Selector from the values map.
                let maybe_sv = match &object.kind {
                    reify_ir::CompiledExprKind::ValueRef(id) => match values.get(id) {
                        Some(reify_ir::Value::Selector(sv)) => Some(sv.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(sv) = maybe_sv {
                    return Some(eval_selector_feature_datum(
                        &sv,
                        member,
                        kernel,
                        swept_kinds,
                        diagnostics,
                    ));
                }
                // No hydrated Value::Selector in the cell: static-type fallback —
                // emit an explicit select-a-subfeature diagnostic instead of
                // leaving the cell a silent `Value::Undef`.
                diagnostics.push(
                    Diagnostic::error(format!(
                        "feature→datum projection '.{member}' over a topology selector \
                         requires a resolved selector; select a single sub-feature \
                         (e.g. `single(...)`) or project from the realized feature instead"
                    ))
                    .with_code(reify_core::DiagnosticCode::FeatureDatumAmbiguous),
                );
                return Some(reify_ir::Value::Undef);
            }
            // Not a selector receiver — β datum such as `axis.dir`, or a
            // not-yet-hydrated cell. Return None and let the pure
            // `eval_datum_projection` path own it.
            return None;
        }
    };

    let history = swept_kinds.lookup(handle);
    let bundle = crate::feature_datum::feature_datum_bundle(handle, kernel, history);
    Some(crate::feature_datum::feature_datum_projection(
        &bundle,
        member,
        diagnostics,
    ))
}

/// Resolve a hydrated `Value::Selector` to its sub-handle ids via
/// [`crate::topology_selectors::resolve`], build a per-handle
/// [`crate::feature_datum::FeatureDatumBundle`] for each, union the four groups
/// across all handles, re-dedup the union at the confusion-floor tolerance
/// (so coaxial/coplanar/coincident datums from different sub-handles collapse to
/// one), and finally project via [`crate::feature_datum::feature_datum_projection`]
/// — the same select-one-or-diagnose refinement the `GeometryHandle` arm uses.
///
/// On `topology_selectors::resolve` returning `Err`, pushes a `Severity::Warning`
/// and returns `Value::Undef` (mirroring `try_eval_resolve_selector` @3471-3476).
///
/// Called from `try_eval_feature_datum_projection` in the `None` branch of
/// `resolve_selector_target` when a hydrated `Value::Selector` cell is present.
fn eval_selector_feature_datum(
    sv: &reify_ir::value::SelectorValue,
    member: &str,
    kernel: &mut dyn reify_ir::GeometryKernel,
    swept_kinds: &crate::sweep_classifier::SweptKindTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::Value {
    // (a) Resolve the selector to a list of sub-handle ids.
    let ids = match crate::topology_selectors::resolve(sv, kernel, diagnostics) {
        Ok(ids) => ids,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "feature→datum projection over selector: kernel error resolving selector: \
                 {err}; cell left at Undef"
            )));
            return reify_ir::Value::Undef;
        }
    };

    // (b) Union per-handle FeatureDatumBundles into one combined bundle.
    let mut combined = crate::feature_datum::FeatureDatumBundle::default();
    for id in ids {
        let b = crate::feature_datum::feature_datum_bundle(id, kernel, swept_kinds.lookup(id));
        combined.axes.extend(b.axes);
        combined.planes.extend(b.planes);
        combined.points.extend(b.points);
        combined.directions.extend(b.directions);
    }

    // (c) Re-dedup each group at the confusion-floor tolerance so N coaxial /
    // coplanar / coincident sub-handle datums collapse to one.
    //
    // V1 DESIGN NOTE — floor-only tolerance (deliberate):
    // `dedup_tolerance(0.0, 0.0)` uses the geometric confusion floor with no
    // per-sub-shape local modelling tolerance added.  The `Datum` carrier that
    // flows through `feature_datum_bundle` does not retain each sub-shape's
    // local lin_tol, so we cannot fold per-handle tolerances here without a
    // Datum API change.  For clean analytic primitives (all local tols at the
    // floor) this is equivalent to `max(local_tols)`, making it correct for the
    // v1 target.  A coarse/imprecise sub-shape whose local tol exceeds the
    // floor could in theory yield a spurious FeatureDatumAmbiguous where the
    // single-GeometryHandle arm would merge; that narrowing is accepted as a v1
    // limitation and documented here so future readers do not mistake it for a
    // bug.  Threading per-handle lin_tol into the cross-handle re-dedup (e.g.
    // fold the max of per-handle bundle lin_tols) would fix it at the cost of
    // a `FeatureDatumBundle::lin_tol` field — left to a follow-up if coarse
    // models are encountered in practice.
    let tol = crate::feature_datum::dedup_tolerance(0.0, 0.0);
    combined.axes = crate::feature_datum::dedup_datums(combined.axes, tol);
    combined.planes = crate::feature_datum::dedup_datums(combined.planes, tol);
    combined.points = crate::feature_datum::dedup_datums(combined.points, tol);
    combined.directions = crate::feature_datum::dedup_datums(combined.directions, tol);

    // (d) Project: unique → datum Value; zero/many → FeatureDatumAmbiguous + Undef.
    crate::feature_datum::feature_datum_projection(&combined, member, diagnostics)
}

/// Reconstruct a `SelectorValue` from a single compiled arg expression.
///
/// PREFERRED path: inline reconstruction from a nested selector FunctionCall
/// (no value-cell ordering dependency) via a recursive `try_eval_topology_selector`
/// call.  Fallback: a `ValueRef` pointing to an already-patched `Value::Selector`
/// cell in the `values` map.
///
/// Returns `None` for any other expr shape (the cell is not yet hydrated or does
/// not represent a selector) — the composition arm then returns `None`, leaving
/// the cell at `Value::Undef` for a subsequent pass.
///
/// Factored from the step-1 inline reconstruction in `resolve_selector_to_list`
/// (task 4119 δ, step-6) so the composition arms in `try_eval_topology_selector`
/// can reuse the same logic.
fn reconstruct_selector_value(
    arg: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::value::SelectorValue> {
    match &arg.kind {
        reify_ir::CompiledExprKind::FunctionCall { .. } => {
            match try_eval_topology_selector(arg, named_steps, values, kernel, diagnostics)? {
                reify_ir::Value::Selector(sv) => Some(sv),
                // FunctionCall resolved to a non-selector (e.g. adjacent_faces
                // → List, or Undef) — not ours to wrap.
                _ => None,
            }
        }
        reify_ir::CompiledExprKind::ValueRef(id) => match values.get(id) {
            Some(reify_ir::Value::Selector(sv)) => Some(sv.clone()),
            // Cell not yet patched to a selector / not a selector — skip.
            _ => None,
        },
        _ => None,
    }
}

/// Build a variadic selector composition (`union` or `intersect`) from a slice of
/// compiled args by reconstructing each child `SelectorValue` then calling the
/// provided constructor.  Parameterised by `constructor` so Union and Intersect
/// share the same collect+construct+error path and only differ in the fn they pass.
///
/// Returns `None` if any child cannot be reconstructed (cell stays Undef).
/// Returns `Some(Value::Undef)` + a Warning on `SelectorError` (defensive backstop
/// — compile-time `E_SELECTOR_KIND_MISMATCH` should have fired first).
fn eval_variadic_composition(
    op_name: &str,
    args: &[reify_ir::CompiledExpr],
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
    constructor: fn(
        Vec<reify_ir::value::SelectorValue>,
    ) -> Result<reify_ir::value::SelectorValue, reify_ir::value::SelectorError>,
) -> Option<reify_ir::Value> {
    let children: Vec<reify_ir::value::SelectorValue> = args
        .iter()
        .map(|arg| reconstruct_selector_value(arg, named_steps, values, kernel, diagnostics))
        .collect::<Option<Vec<_>>>()?;
    match constructor(children) {
        Ok(sv) => Some(reify_ir::Value::Selector(sv)),
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{op_name}: selector kind-closure violation ({err:?}); cell left at Undef"
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Build a named-leaf selector (`face`, `edge`, or `solid_body`) from two compiled
/// args: `args[0]` is the geometry target (resolved via `resolve_selector_target`)
/// and `args[1]` is the tag string (extracted via `resolve_string_literal_arg`).
/// Parameterised by `kind` so all three ctors share the same resolution path.
fn eval_named_leaf_selector_ctor(
    kind: reify_core::ty::SelectorKind,
    args: &[reify_ir::CompiledExpr],
    values: &reify_ir::ValueMap,
    function_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let target = resolve_selector_target(&args[0], values)?;
    // Evaluate-then-accept (task ε): the named-leaf ctor threads the real
    // `values`, so the tag arg now resolves a `ValueRef → String` cell
    // (`face(body, label_var)`) in addition to an inline string literal; a
    // defined-but-wrong tag emits a Warning instead of falling through silently.
    let name = resolve_string_literal_arg(&args[1], values, function_name, "name", diagnostics)?;
    build_leaf_selector(
        kind,
        target,
        reify_ir::value::LeafQuery::Named(name),
        function_name,
        diagnostics,
    )
}

/// Reconstruct the `Value::Selector` denoted by `selector_expr`, resolve it via
/// `topology_selectors::resolve`, and wrap the canonical-order handle ids as a
/// `Value::List` of `Value::GeometryHandle` sub-handles. Shared by the
/// `ResolveSelector` and `IndexAccess`-over-selector arms of
/// [`try_eval_resolve_selector`].
///
/// Returns `None` when the inner expr is not a selector we can reconstruct (so
/// the caller skips the cell); `Some(Value::Undef)` + a Warning when `resolve()`
/// fails at the kernel.
///
/// NOTE (sub-handle indexing): the resolved ids are enumerated by FILTERED
/// position, so a predicate leaf's `[i]` does not preserve the parent's canonical
/// TopExp index. For the call-site-transparent shapes in scope here — `All`-leaf
/// indexing (`faces(b)[i]`, filtered == canonical) and single-element
/// `single(predicate(...))` — filtered position equals the intended element.
/// Canonical-index recovery for multi-element predicate `[i]` is a follow-up.
fn resolve_selector_to_list(
    selector_expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // (1) Obtain the Value::Selector via the shared helper (task 4119 δ, step-6).
    let sv = reconstruct_selector_value(selector_expr, named_steps, values, kernel, diagnostics)?;

    // (2) Parent identity for sub-handle hashing: the (first) leaf target.
    let target = first_leaf_target(&sv)?;
    let sub_kind = match sv.kind {
        reify_core::ty::SelectorKind::Face => crate::topology_selectors::SubKind::Face,
        reify_core::ty::SelectorKind::Edge => crate::topology_selectors::SubKind::Edge,
        reify_core::ty::SelectorKind::Body => crate::topology_selectors::SubKind::Solid,
    };
    let parent_rr = target.realization_ref.clone();
    let parent_hash = target.upstream_values_hash;

    // (3) Resolve via the single executor — the kernel-bearing query happens HERE,
    // not at construction (K2/BT7).
    match crate::topology_selectors::resolve(&sv, kernel, diagnostics) {
        Ok(ids) => {
            let elements = ids
                .into_iter()
                .enumerate()
                .map(|(i, id)| {
                    crate::topology_selectors::make_sub_handle(
                        &parent_rr,
                        &parent_hash,
                        sub_kind,
                        i as u32,
                        id,
                    )
                })
                .collect();
            Some(reify_ir::Value::List(elements))
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "resolve_selector: kernel error resolving selector: {err}; cell left at Undef"
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// First `Leaf` target reached by a left-most walk of the selector tree — the
/// parent solid handle used for sub-handle identity. The 7 re-typed constructors
/// only build `Leaf` nodes; composites walk to their first child for robustness.
fn first_leaf_target(
    sv: &reify_ir::value::SelectorValue,
) -> Option<&reify_ir::value::GeometryHandleRef> {
    fn walk(node: &reify_ir::value::SelectorNode) -> Option<&reify_ir::value::GeometryHandleRef> {
        match node {
            reify_ir::value::SelectorNode::Leaf { target, .. } => Some(target),
            reify_ir::value::SelectorNode::Union(children)
            | reify_ir::value::SelectorNode::Intersect(children) => {
                children.first().and_then(|c| walk(&c.node))
            }
            reify_ir::value::SelectorNode::Difference(a, _) => walk(&a.node),
        }
    }
    walk(&sv.node)
}

/// Resolve an `IndexAccess` index expr to a `usize`. Accepts an `Int` literal or
/// a `ValueRef` to an `Int` cell; returns `None` for anything else or a negative
/// index (the caller then leaves the cell untouched).
fn resolve_index_usize(
    index: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<usize> {
    let v = match &index.kind {
        reify_ir::CompiledExprKind::Literal(v) => v,
        reify_ir::CompiledExprKind::ValueRef(id) => values.get(id)?,
        _ => return None,
    };
    match v {
        reify_ir::Value::Int(i) if *i >= 0 => Some(*i as usize),
        _ => None,
    }
}

pub(crate) fn try_eval_topology_selector(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // (1) Must be a FunctionCall — anything else is unsupported.
    let (function, args) = match &expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
        _ => return None,
    };

    // (2) Must be one of the recognised helper names.
    let helper = match function.name.as_str() {
        "closest_point" => TopologySelectorHelper::ClosestPoint,
        "is_on" => TopologySelectorHelper::IsOn,
        "angle_between_surfaces" => TopologySelectorHelper::AngleBetweenSurfaces,
        "edges" => TopologySelectorHelper::Edges,
        "faces" => TopologySelectorHelper::Faces,
        "center_of_mass" => TopologySelectorHelper::CenterOfMass,
        "moment_of_inertia" => TopologySelectorHelper::MomentOfInertia,
        "edges_by_length" => TopologySelectorHelper::EdgesByLength,
        "faces_by_area" => TopologySelectorHelper::FacesByArea,
        "faces_by_normal" => TopologySelectorHelper::FacesByNormal,
        "edges_parallel_to" => TopologySelectorHelper::EdgesParallelTo,
        "edges_at_height" => TopologySelectorHelper::EdgesAtHeight,
        "adjacent_faces" => TopologySelectorHelper::AdjacentFaces,
        "shared_edges" => TopologySelectorHelper::SharedEdges,
        "angle" => TopologySelectorHelper::Angle,
        "contains" => TopologySelectorHelper::Contains,
        "geo_equiv" => TopologySelectorHelper::GeoEquiv,
        "normal" => TopologySelectorHelper::Normal,
        "curvature" => TopologySelectorHelper::Curvature,
        "length" => TopologySelectorHelper::Length,
        "perimeter" => TopologySelectorHelper::Perimeter,
        "distance" => TopologySelectorHelper::Distance,
        "intersects" => TopologySelectorHelper::Intersects,
        "split" => TopologySelectorHelper::Split,
        // task 4119 δ — selector-composition algebra
        "union" => TopologySelectorHelper::Union,
        "intersect" => TopologySelectorHelper::Intersect,
        "difference" => TopologySelectorHelper::Difference,
        // task 4119 δ — Named-leaf constructors (PRD §11.1)
        "face" => TopologySelectorHelper::Face,
        "edge" => TopologySelectorHelper::Edge,
        "solid_body" => TopologySelectorHelper::SolidBody,
        _ => return None,
    };

    // (3) Per-helper arity check. Each new selector in task 3560 carries its
    // own arity contract; the legacy 2-arg trio (closest_point, is_on,
    // angle_between_surfaces) shares the arity-2 branch.
    // task 4119 δ: union/intersect are variadic (≥ 2); difference is binary (== 2).
    match helper {
        TopologySelectorHelper::Union | TopologySelectorHelper::Intersect => {
            if args.len() < 2 {
                return None;
            }
        }
        _ => {
            let expected_arity = helper.expected_arity();
            if args.len() != expected_arity {
                return None;
            }
        }
    }

    match helper {
        TopologySelectorHelper::ClosestPoint | TopologySelectorHelper::IsOn => {
            // args[0]: point ValueRef → values map → Value::Point of three Length scalars.
            let point =
                resolve_point3_length_arg(&args[0], values, &function.name, "point", diagnostics)?;
            // args[1]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[1], named_steps)?;

            match helper {
                TopologySelectorHelper::ClosestPoint => {
                    let query = reify_ir::GeometryQuery::ClosestPointOnShape {
                        handle,
                        px: point[0],
                        py: point[1],
                        pz: point[2],
                    };
                    dispatch_point3_length_reply(kernel, &query, &function.name, diagnostics)
                }
                TopologySelectorHelper::IsOn => {
                    // Use `reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` (= OCCT's
                    // `Precision::Confusion()`, ~1e-7) as the default tolerance for the
                    // v0.1 2-arg `is_on(point, geometry)` form.  The constant is the
                    // single source of truth shared between this dispatcher and
                    // `OcctKernel::point_on_shape`
                    // (`crates/reify-kernel-occt/src/lib.rs`).  A future explicit-
                    // tolerance overload `is_on(point, geometry, tol)` will plumb the
                    // user-supplied tolerance through here.
                    let query = reify_ir::GeometryQuery::PointOnShape {
                        handle,
                        px: point[0],
                        py: point[1],
                        pz: point[2],
                        tolerance: reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
                    };
                    dispatch_point_on_shape(kernel, &query, &function.name, diagnostics)
                }
                // Enumerate the complement explicitly (rather than `_`) so that
                // adding a new `TopologySelectorHelper` variant and grouping it
                // into the outer `ClosestPoint | IsOn` or-pattern forces the
                // compiler to error here instead of silently funnelling into
                // `unreachable!()`.
                TopologySelectorHelper::AngleBetweenSurfaces
                | TopologySelectorHelper::Edges
                | TopologySelectorHelper::Faces
                | TopologySelectorHelper::CenterOfMass
                | TopologySelectorHelper::MomentOfInertia
                | TopologySelectorHelper::EdgesByLength
                | TopologySelectorHelper::FacesByArea
                | TopologySelectorHelper::FacesByNormal
                | TopologySelectorHelper::EdgesParallelTo
                | TopologySelectorHelper::EdgesAtHeight
                | TopologySelectorHelper::AdjacentFaces
                | TopologySelectorHelper::SharedEdges
                | TopologySelectorHelper::Angle
                | TopologySelectorHelper::Contains
                | TopologySelectorHelper::GeoEquiv
                | TopologySelectorHelper::Normal
                | TopologySelectorHelper::Curvature
                | TopologySelectorHelper::Length
                | TopologySelectorHelper::Perimeter
                | TopologySelectorHelper::Distance
                | TopologySelectorHelper::Intersects
                | TopologySelectorHelper::Split
                // task 4119 δ — composition + Named-leaf ctors
                | TopologySelectorHelper::Union
                | TopologySelectorHelper::Intersect
                | TopologySelectorHelper::Difference
                | TopologySelectorHelper::Face
                | TopologySelectorHelper::Edge
                | TopologySelectorHelper::SolidBody => {
                    unreachable!("ClosestPoint/IsOn outer match guarantees this")
                }
            }
        }
        TopologySelectorHelper::AngleBetweenSurfaces => {
            // Both args: geometry ValueRefs → named_steps map → GeometryHandleId.
            let face_a = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let face_b = resolve_geometry_handle_arg(&args[1], named_steps)?;
            let query = reify_ir::GeometryQuery::SurfaceAngle { face_a, face_b };
            dispatch_surface_angle(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::Contains => {
            // args[0]: solid geometry ValueRef → named_steps map → GeometryHandleId.
            // args[1]: point ValueRef → values map → Value::Point of three Length scalars.
            // Arg order is solid-then-point (mirror of is_on: point-then-geometry).
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let point =
                resolve_point3_length_arg(&args[1], values, &function.name, "point", diagnostics)?;
            // Use `reify_ir::DEFAULT_CONTAINS_TOLERANCE_M` (= OCCT's
            // `Precision::Confusion()`, ~1e-7) as the default tolerance for the
            // v0.1 2-arg `contains(solid, point)` form, matching the is_on
            // precedent per §5.2. A future explicit-tolerance
            // `contains(solid, point, tol)` overload will plumb the
            // user-supplied tolerance through here.
            let query = reify_ir::GeometryQuery::Contains {
                handle,
                px: point[0],
                py: point[1],
                pz: point[2],
                tolerance: reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
            };
            // Reuse the Bool-unwrap helper from `is_on`: dispatches
            // `kernel.query(&query)` and unwraps `Value::Bool`, downgrading
            // non-Bool / Err replies to `Some(Value::Undef)` + Warning per §4.
            dispatch_point_on_shape(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::GeoEquiv => {
            // geo_equiv(left, right, tol) → Bool (task 3613, KGQ-δ, PRD §5.1).
            // True iff BOTH topology equivalence (canonical TopExp::MapShapes
            // per-kind counts match) AND sampled-vertex tolerance (N=8 uniform
            // parameter points per face/edge; |p_a - p_b| < tol) hold.
            //
            // FUTURE: geo_equiv_strict(a, b, tol) — symmetric Hausdorff distance
            // variant deferred to v0.4 (PRD §5.1, Open Question §10).
            //
            // args[0]: left geometry ValueRef → named_steps → GeometryHandleId.
            // args[1]: right geometry ValueRef → named_steps → GeometryHandleId.
            // args[2]: tolerance ValueRef → values → Value::length(m) → SI metres.
            let left = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let right = resolve_geometry_handle_arg(&args[1], named_steps)?;
            let tolerance =
                resolve_length_scalar_arg(&args[2], values, &function.name, "tolerance", diagnostics)?;
            let query = reify_ir::GeometryQuery::GeoEquiv {
                left,
                right,
                tolerance,
            };
            // Reuse the Bool-unwrap helper: dispatches kernel.query(&query) and
            // unwraps Value::Bool, downgrading non-Bool / Err replies to
            // Some(Value::Undef) + Warning (function.name = "geo_equiv").
            dispatch_point_on_shape(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::Angle => {
            // Both args: value-flow Vec3 ValueRefs → values map → [f64; 3].
            // Pure-math: acos(clamp(dot(a,b)/(|a||b|), -1, 1)). No kernel call.
            //
            // The dot-product and L2-norm are hand-rolled on [f64; 3] rather than
            // reusing `crates/reify-stdlib/src/linalg.rs` because that crate
            // operates on `Value` tensors, not bare [f64; 3] slices.  If the
            // degenerate-input semantics here ever diverge from linalg.rs's
            // `magnitude`/`dot` handling, align them explicitly.  See also the
            // unit tests for `angle` in this module (task 3614, KGQ-ε).
            let a = resolve_vec3_arg(&args[0], values, &function.name, "a", diagnostics)?;
            let b = resolve_vec3_arg(&args[1], values, &function.name, "b", diagnostics)?;
            let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let na = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            let nb = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
            // Primary degenerate guard: zero-length or explicitly non-finite
            // (NaN/inf component → NaN magnitude, overflow → inf magnitude).
            if na == 0.0 || nb == 0.0 || !na.is_finite() || !nb.is_finite() {
                diagnostics.push(Diagnostic::warning(format!(
                    "angle: degenerate input — zero-length or non-finite vector \
                     (|a|={na}, |b|={nb}); cell left at Undef"
                )));
                return Some(reify_ir::Value::Undef);
            }
            let cos_theta = (dot / (na * nb)).clamp(-1.0, 1.0);
            // Secondary degenerate guard: catch NaN from subnormal magnitude
            // underflow (na*nb underflows to 0.0 while both na and nb
            // individually passed the guard above — a rare but possible case
            // with extremely small component values).  clamp() propagates NaN
            // unchanged in IEEE 754, so this must be tested after clamping.
            if !cos_theta.is_finite() {
                diagnostics.push(Diagnostic::warning(format!(
                    "angle: computed cosine is non-finite \
                     (|a|={na}, |b|={nb}, dot={dot}); \
                     possible subnormal magnitude underflow; cell left at Undef"
                )));
                return Some(reify_ir::Value::Undef);
            }
            let theta = cos_theta.acos();
            Some(reify_ir::Value::angle(theta))
        }
        TopologySelectorHelper::Normal => {
            // `normal(surface, point) -> Vector3<Dimensionless>` (task 3615, KGQ-ζ).
            // Arg order mirrors `contains`: Surface=args[0], Point3<Length>=args[1].
            // args[0]: surface geometry ValueRef → named_steps → GeometryHandleId.
            // args[1]: point ValueRef → values → Value::Point of three Length scalars → [f64;3] SI metres.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let point =
                resolve_point3_length_arg(&args[1], values, &function.name, "point", diagnostics)?;
            let query = reify_ir::GeometryQuery::FaceNormalAt {
                handle,
                px: point[0],
                py: point[1],
                pz: point[2],
            };
            dispatch_normal_vector3(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::Curvature => {
            // `curvature(shape, point) -> Scalar<Curvature>|Matrix<2,2,Curvature>` (task 3621, KGQ-μ).
            // Arg order: Shape=args[0], Point3<Length>=args[1].
            // args[0]: geometry ValueRef → named_steps → GeometryHandleId.
            // args[1]: point ValueRef → values → [f64;3] SI metres (px, py, pz).
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let point =
                resolve_point3_length_arg(&args[1], values, &function.name, "point", diagnostics)?;
            dispatch_curvature(kernel, handle, point, &function.name, diagnostics)
        }
        TopologySelectorHelper::Length => {
            // `length(curve) -> Scalar<Length>` (task 3622, KGQ-ν).
            // arg[0]: edge sub-handle ValueRef → values → kernel_handle.
            // Falls through (None) when arg is not a hydrated Value::GeometryHandle
            // (PRD invariant #2).
            let (_, _, kernel_handle) = resolve_parent_geometry_handle_arg(&args[0], values)?;
            dispatch_edge_length(kernel, kernel_handle, &function.name, diagnostics)
        }
        TopologySelectorHelper::Perimeter => {
            // `perimeter(surface) -> Scalar<Length>` (task 3622, KGQ-ν).
            // arg[0]: face sub-handle ValueRef → values → kernel_handle.
            // Falls through (None) when arg is not a hydrated Value::GeometryHandle
            // (PRD invariant #2).
            let (_, _, face_kh) = resolve_parent_geometry_handle_arg(&args[0], values)?;
            dispatch_perimeter(kernel, face_kh, &function.name, diagnostics)
        }
        TopologySelectorHelper::Distance => {
            // `distance(a, b) -> Scalar<Length>` (task 3610, KGQ-α, PRD §9).
            //
            // Resolve each arg as Shape (named_steps) else Point (values).
            // Non-ValueRef args → None on that resolution, fall-through to None.
            //
            // 2×2 dispatch matrix:
            //   (Shape, Point) / (Point, Shape) → ClosestPointOnShape + Euclidean
            //   (Shape, Shape)                  → GeometryQuery::Distance{from,to}
            //   (Point, Point)                  → pure Euclidean, no kernel call
            //
            // Kernel-error-downgrade contract (invariant #3): on Err or malformed
            // ClosestPointOnShape reply, `dispatch_point3_length_reply` returns
            // `Some(Value::Undef)` with exactly one Warning diagnostic (not None),
            // so the cell is visibly degraded rather than silently preserved.

            // Extract the SI-metre f64 from a length-typed Value component.
            // Returns None for non-numeric variants (dead-code guard: in practice
            // dispatch_point3_length_reply always yields Value::Scalar{LENGTH}
            // components; returning None rather than NAN makes misbehaviour
            // visible so the caller can downgrade to Undef + Warning).
            let extract_si = |v: &reify_ir::Value| -> Option<f64> {
                match v {
                    reify_ir::Value::Scalar { si_value, .. } => Some(*si_value),
                    reify_ir::Value::Real(r) => Some(*r),
                    _ => None,
                }
            };

            // Euclidean distance between two SI-metre 3-D points.
            let euclidean_3d = |a: [f64; 3], b: [f64; 3]| -> f64 {
                let dx = a[0] - b[0];
                let dy = a[1] - b[1];
                let dz = a[2] - b[2];
                (dx * dx + dy * dy + dz * dz).sqrt()
            };

            let arg0_shape = resolve_geometry_handle_arg(&args[0], named_steps);
            // The point resolver is the SECOND probe (shape first, else point):
            // when it is reached the arg is already known not to be a resolvable
            // shape, so a defined-but-wrong value is genuinely neither shape nor
            // point — per PRD §7.3 "never silent" it emits one Warning naming
            // `distance` / the positional arg / Point<Length>. A shape arg never
            // reaches the point probe (arg0_shape is Some → probe skipped), so
            // the common Shape×Shape / Shape×Point forms stay diagnostic-free; an
            // Undef arg still degrades quietly.
            let arg0_point = if arg0_shape.is_none() {
                resolve_point3_length_arg(&args[0], values, &function.name, "a", diagnostics)
            } else {
                None
            };
            let arg1_shape = resolve_geometry_handle_arg(&args[1], named_steps);
            let arg1_point = if arg1_shape.is_none() {
                resolve_point3_length_arg(&args[1], values, &function.name, "b", diagnostics)
            } else {
                None
            };

            // Normalise Shape×Point and Point×Shape to a single (handle, point)
            // pair — both cases are symmetric and share one ClosestPointOnShape
            // block, eliminating the 40-line duplication that was a maintenance
            // hazard (reviewer note: each arm was byte-for-byte identical).
            let shape_point_pair = match (arg0_shape, arg0_point, arg1_shape, arg1_point) {
                (Some(h), None, None, Some(p)) | (None, Some(p), Some(h), None) => Some((h, p)),
                _ => None,
            };

            if let Some((handle, point)) = shape_point_pair {
                // Shape × Point / Point × Shape: issue ClosestPointOnShape on the
                // shape then compute Euclidean distance from the query point.
                //
                // `dispatch_point3_length_reply` handles Err/malformed with
                // Some(Value::Undef) + one Warning (invariant #3). On success it
                // returns Some(Value::Point([length, length, length])).
                let query = reify_ir::GeometryQuery::ClosestPointOnShape {
                    handle,
                    px: point[0],
                    py: point[1],
                    pz: point[2],
                };
                match dispatch_point3_length_reply(kernel, &query, &function.name, diagnostics) {
                    Some(reify_ir::Value::Point(comps)) if comps.len() == 3 => {
                        let cx = extract_si(&comps[0]);
                        let cy = extract_si(&comps[1]);
                        let cz = extract_si(&comps[2]);
                        match (cx, cy, cz) {
                            (Some(cx), Some(cy), Some(cz)) => {
                                Some(reify_ir::Value::length(euclidean_3d(point, [cx, cy, cz])))
                            }
                            // Non-numeric component — unexpected but guarded;
                            // downgrade visibly rather than silently emitting NaN
                            // (invariant #3, reviewer note on robustness).
                            _ => {
                                diagnostics.push(Diagnostic::warning(format!(
                                    "{}: ClosestPointOnShape reply contained a \
                                     non-numeric component; treating distance as undefined",
                                    &function.name
                                )));
                                Some(reify_ir::Value::Undef)
                            }
                        }
                    }
                    // Undef reply (error already warned by dispatch helper) →
                    // propagate.
                    Some(reify_ir::Value::Undef) => Some(reify_ir::Value::Undef),
                    // None from dispatch_point3_length_reply (shouldn't happen) → None.
                    _ => None,
                }
            } else {
                match (arg0_shape, arg0_point, arg1_shape, arg1_point) {
                    (Some(from), None, Some(to), None) => {
                        // Shape × Shape: issue GeometryQuery::Distance{from,to} via
                        // kernel_distance. Returns None on Err/non-numeric (already
                        // warned); map None → Some(Value::Undef) per invariant #3.
                        // Exactly one kernel query (invariant #4).
                        match kernel_distance(kernel, from, to, diagnostics, &function.name) {
                            Some(d) => Some(reify_ir::Value::length(d)),
                            None => Some(reify_ir::Value::Undef),
                        }
                    }
                    (None, Some(pa), None, Some(pb)) => {
                        // Point × Point: pure Euclidean, no kernel call (invariant
                        // #4: 0 queries).
                        Some(reify_ir::Value::length(euclidean_3d(pa, pb)))
                    }
                    // Non-ValueRef / unresolvable args — fall through to None
                    // (invariants #1/#2).
                    _ => None,
                }
            }
        }
        TopologySelectorHelper::Intersects => {
            // `intersects(a, b) -> Bool` (task 3612, KGQ-γ, PRD §9).
            //
            // Routes through GeometryQuery::Distance{from,to} via kernel_distance,
            // classifying d <= 0.0 → Bool(true) (shapes touching or overlapping)
            // and d > 0.0 → Bool(false) (shapes apart).
            //
            // This reproduces the shipped shapes_intersect adapter semantics
            // (reify-kernel-occt/src/lib.rs:770: "Ok(true) iff min BREP distance
            // ≤ 0.0") and the kinematic interferes_with precedent
            // (geometry_ops.rs:1601: `Some(d) => Bool(d <= 0.0)`).
            //
            // NOTE: d=0.0 (touching / face-coincident) → Bool(true) here.  The
            // Manifold-side queries::intersects returns false for the same case
            // (CSG boolean yields empty mesh for zero shared volume) — a known
            // parity divergence to be resolved by KGQ-ο (Phase 5).
            //
            // Both args must be Shape ValueRefs. Non-ValueRef/non-geometry args
            // return None from resolve_geometry_handle_arg → fall through to None
            // (invariants #1/#2). Kernel Err/non-numeric already emitted one
            // Warning and returns None → mapped to Some(Undef) (invariant #3).
            // Exactly one kernel query (invariant #4).
            let from = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let to = resolve_geometry_handle_arg(&args[1], named_steps)?;
            match kernel_distance(kernel, from, to, diagnostics, &function.name) {
                Some(d) => Some(reify_ir::Value::Bool(d <= 0.0)),
                None => Some(reify_ir::Value::Undef),
            }
        }
        TopologySelectorHelper::Split => {
            // `split(solid, plane) -> List<Geometry>` (task 4190, PRD ζ).
            //
            // args[0]: solid ValueRef → values map → full parent GeometryHandle.
            //   Resolved via `resolve_parent_geometry_handle_arg` so we get the
            //   parent's realization_ref + upstream_values_hash for sub-handle
            //   construction (PRD §4).  Falls through to None when the arg cell
            //   is not yet a hydrated Value::GeometryHandle (PRD invariant #2:
            //   never partially construct a sub-handle).
            // args[1]: plane ValueRef → values map → Value::Plane.
            //   Decoded via `decode_plane` → (plane_origin, plane_normal [f64;3]).
            //   Falls through to None when args[1] is not a Value::Plane (wrong
            //   variant, unresolved, Undef, etc.) — same fall-through contract.
            // Dispatch: `GeometryKernel::execute_split(&GeometryOp::Split{..})`.
            //   On Ok(ids): build Value::List via make_sub_handle(SubKind::Solid).
            //   On Err: emit Warning diagnostic, return Some(Value::Undef).
            let (parent_rr, parent_hash, parent_kernel_handle) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;

            // Resolve and decode the plane arg.
            let plane_cell_id = match &args[1].kind {
                reify_ir::CompiledExprKind::ValueRef(id) => id,
                _ => return None,
            };
            let plane_val = values.get(plane_cell_id)?;
            let (plane_origin, plane_normal) = match decode_plane(plane_val) {
                Ok(pair) => pair,
                Err(_) => return None,
            };

            let op = reify_ir::GeometryOp::Split {
                target: parent_kernel_handle,
                plane_origin,
                plane_normal,
            };
            match kernel.execute_split(&op) {
                Ok(piece_ids) => {
                    let elements = piece_ids
                        .into_iter()
                        .enumerate()
                        .map(|(i, piece_kernel_id)| {
                            crate::topology_selectors::make_sub_handle(
                                &parent_rr,
                                &parent_hash,
                                crate::topology_selectors::SubKind::Solid,
                                i as u32,
                                piece_kernel_id,
                            )
                        })
                        .collect();
                    Some(reify_ir::Value::List(elements))
                }
                Err(err) => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "{}({:?}): kernel error: {}",
                        function.name, parent_kernel_handle, err
                    )));
                    Some(reify_ir::Value::Undef)
                }
            }
        }
        // Task 4118 (γ): `edges(solid)` / `faces(solid)` build a kernel-FREE
        // typed `Value::Selector(kind)` whose leaf is `All` over the parent
        // solid handle — NOT an eagerly-extracted `Value::List`. The
        // `Selector → List<Geometry>` resolution is deferred to the
        // compiler-inserted `ResolveSelector` coercion node (K2/BT7: zero
        // kernel queries during construction). `arg[0]` resolves from `values`
        // (not `named_steps`) so the leaf target carries the parent's
        // realization_ref + upstream_values_hash; falls through to None when the
        // arg cell is not yet a hydrated Value::GeometryHandle (PRD invariant #2).
        TopologySelectorHelper::Edges => {
            let target = resolve_selector_target(&args[0], values)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Edge,
                target,
                reify_ir::value::LeafQuery::All,
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::Faces => {
            let target = resolve_selector_target(&args[0], values)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::All,
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::CenterOfMass => {
            // args[0]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            // args[1]: density ValueRef → values map → Value::Scalar{MASS_DENSITY}.
            // Uses resolve_density_arg (same as MomentOfInertia) — Contract A
            // (task 4486 γ): only a dimensioned Density is accepted; bare Real
            // and dimensionless Scalar now emit a Severity::Warning.
            let density = resolve_density_arg(&args[1], values, &function.name, diagnostics)?;
            let query = reify_ir::GeometryQuery::CenterOfMass { handle, density };
            dispatch_point3_length_reply(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::MomentOfInertia => {
            // args[0]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            // args[1]: density ValueRef → values map → Value::Scalar{MASS_DENSITY}.
            // Uses resolve_density_arg — Contract A (task 4486 γ): only a
            // dimensioned Density is accepted; bare Real and dimensionless
            // Scalar now emit a Severity::Warning.
            let density = resolve_density_arg(&args[1], values, &function.name, diagnostics)?;
            let query = reify_ir::GeometryQuery::InertiaTensor { handle, density };
            dispatch_inertia_tensor(kernel, &query, &function.name, diagnostics)
        }
        // Task 4118 (γ): build a kernel-FREE `Value::Selector(Edge)` with a
        // `ByLength` leaf. The Range<Length> arg maps directly to the leaf's
        // (min_m, max_m); NO kernel filter runs here — deferred to resolve().
        TopologySelectorHelper::EdgesByLength => {
            let target = resolve_selector_target(&args[0], values)?;
            // args[1]: Range<Length> arg → (min_m, max_m). Evaluate-then-accept
            // (task ε): inline / computed-bound ranges now WORK; a defined-wrong
            // value emits a Severity::Warning.
            let (min_m, max_m) = resolve_range_dim_arg(
                &args[1],
                values,
                reify_core::DimensionVector::LENGTH,
                &function.name,
                "length_range",
                diagnostics,
            )?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Edge,
                target,
                reify_ir::value::LeafQuery::ByLength { min_m, max_m },
                &function.name,
                diagnostics,
            )
        }
        // Task 4118 (γ): build a kernel-FREE `Value::Selector(Face)` with a
        // `ByArea` leaf. `mm*mm` canonicalises to AREA (LENGTH² == AREA per
        // dimension algebra); the range maps directly to (min_m2, max_m2).
        TopologySelectorHelper::FacesByArea => {
            let target = resolve_selector_target(&args[0], values)?;
            // args[1]: Range<Area> arg → (min_m2, max_m2). Evaluate-then-accept
            // (task ε): inline / computed-bound ranges now WORK; a defined-wrong
            // value emits a Severity::Warning.
            let (min_m2, max_m2) = resolve_range_dim_arg(
                &args[1],
                values,
                reify_core::DimensionVector::AREA,
                &function.name,
                "area_range",
                diagnostics,
            )?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::ByArea { min_m2, max_m2 },
                &function.name,
                diagnostics,
            )
        }
        // Task 4118 (γ): build a kernel-FREE `Value::Selector(Face)` with a
        // `ByNormal` leaf (dir + angular tolerance in SI radians).
        TopologySelectorHelper::FacesByNormal => {
            let target = resolve_selector_target(&args[0], values)?;
            // args[1]: Vec3 direction ValueRef → values map → [f64; 3].
            let dir = resolve_vec3_arg(&args[1], values, &function.name, "dir", diagnostics)?;
            // args[2]: angular tolerance ValueRef → values map → ANGLE Scalar (SI rad).
            let tol_rad =
                resolve_angle_scalar_arg(&args[2], values, &function.name, "tol", diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::ByNormal { dir, tol_rad },
                &function.name,
                diagnostics,
            )
        }
        // Task 4118 (γ): build a kernel-FREE `Value::Selector(Edge)` with a
        // `ByParallel` leaf (axis + angular tolerance in SI radians).
        TopologySelectorHelper::EdgesParallelTo => {
            let target = resolve_selector_target(&args[0], values)?;
            // args[1]: Vec3 axis ValueRef → values map → [f64; 3].
            let axis = resolve_vec3_arg(&args[1], values, &function.name, "axis", diagnostics)?;
            // args[2]: angular tolerance ValueRef → values map → ANGLE Scalar (SI rad).
            let tol_rad =
                resolve_angle_scalar_arg(&args[2], values, &function.name, "tol", diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Edge,
                target,
                reify_ir::value::LeafQuery::ByParallel { axis, tol_rad },
                &function.name,
                diagnostics,
            )
        }
        // Task 4118 (γ): build a kernel-FREE `Value::Selector(Edge)` with a
        // `ByHeight` leaf (z-plane + tolerance, both SI metres).
        TopologySelectorHelper::EdgesAtHeight => {
            let target = resolve_selector_target(&args[0], values)?;
            // args[1]: z plane ValueRef → values map → LENGTH Scalar (SI metres).
            let z_m = resolve_length_scalar_arg(&args[1], values, &function.name, "z", diagnostics)?;
            // args[2]: tolerance ValueRef → values map → LENGTH Scalar (SI metres).
            let tol_m =
                resolve_length_scalar_arg(&args[2], values, &function.name, "tol", diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Edge,
                target,
                reify_ir::value::LeafQuery::ByHeight { z_m, tol_m },
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::AdjacentFaces => {
            // args[0]: parent solid ValueRef → values map → full Value::GeometryHandle.
            // Must resolve from `values` (not `named_steps`) so we get the parent's
            // realization_ref + upstream_values_hash for sub-handle construction (PRD §4).
            // Falls through to None when the arg cell is not a hydrated Value::GeometryHandle
            // (PRD invariant #2: never partially construct a sub-handle).
            let (parent_rr, parent_hash, parent_kh) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;
            // args[1]: face sub-handle ValueRef → values map → kernel_handle only.
            // Real face sub-handles (e.g. from faces(b) / single(faces_by_normal(...)))
            // live in `values` as Value::GeometryHandle, not in named_steps (design §4).
            let (_, _, face_kh) = resolve_parent_geometry_handle_arg(&args[1], values)?;
            // `adjacent_to_face` recovers the 0-based face index via
            // `extract_faces(parent)`, dispatches `GeometryQuery::AdjacentFaces`,
            // and maps the reply indices back to face handles.
            // Result saved before dispatch to avoid a double-mutable-borrow on kernel.
            // Output: List<Value::GeometryHandle> sub-handles per PRD §4 (KGQ-κ).
            let filter_result =
                crate::selector_vocabulary_v2::adjacent_to_face(kernel, parent_kh, face_kh);
            dispatch_filtered_subhandles(
                kernel,
                parent_kh,
                crate::topology_selectors::SubKind::Face,
                &parent_rr,
                &parent_hash,
                filter_result,
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::SharedEdges => {
            // args[0]: face_a ValueRef → values map → kernel_handle only.
            // Face sub-handles live in `values` as Value::GeometryHandle.
            // Falls through to None when not hydrated (PRD invariant #2).
            let (_, _, face_a) = resolve_parent_geometry_handle_arg(&args[0], values)?;
            // args[1]: face_b ValueRef → values map → kernel_handle only.
            let (_, _, face_b) = resolve_parent_geometry_handle_arg(&args[1], values)?;
            dispatch_shared_edges(kernel, face_a, face_b, &function.name, diagnostics, values)
        }
        // ── task 4119 δ: selector-composition algebra ────────────────────────
        // union(a, b, …) / intersect(a, b, …) / difference(a, b) build a
        // kernel-FREE composite `Value::Selector(kind)` whose tree is
        // Union/Intersect/Difference of the child SelectorValues.  Child
        // selectors are reconstructed via `reconstruct_selector_value` (either
        // an inline nested selector FunctionCall or a ValueRef to an already-
        // patched selector cell).  The K1 kind-closure check is delegated to
        // the `SelectorValue::{union,intersect,difference}` constructors; on
        // `SelectorError::KindMismatch` (defensive backstop — compile-time
        // E_SELECTOR_KIND_MISMATCH should have fired first) a Warning is emitted
        // and `Some(Value::Undef)` is returned, mirroring `build_leaf_selector`.
        // Zero kernel queries at construction time (K2/BT7).
        TopologySelectorHelper::Union => eval_variadic_composition(
            "union",
            args,
            named_steps,
            values,
            kernel,
            diagnostics,
            reify_ir::value::SelectorValue::union,
        ),
        TopologySelectorHelper::Intersect => eval_variadic_composition(
            "intersect",
            args,
            named_steps,
            values,
            kernel,
            diagnostics,
            reify_ir::value::SelectorValue::intersect,
        ),
        TopologySelectorHelper::Difference => {
            // args[0] and args[1] guaranteed by the == 2 arity gate.
            let a = reconstruct_selector_value(&args[0], named_steps, values, kernel, diagnostics)?;
            let b = reconstruct_selector_value(&args[1], named_steps, values, kernel, diagnostics)?;
            match reify_ir::value::SelectorValue::difference(a, b) {
                Ok(sv) => Some(reify_ir::Value::Selector(sv)),
                Err(err) => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "difference: selector kind-closure violation ({err:?}); cell left at Undef"
                    )));
                    Some(reify_ir::Value::Undef)
                }
            }
        }
        // ── task 4119 δ: Named-leaf constructors ─────────────────────────────
        // face(geometry, name) / edge(geometry, name) / solid_body(geometry, name)
        // resolve the parent GeometryHandleRef from args[0] (via resolve_selector_target,
        // which reads Value::GeometryHandle from the values map) and the name string
        // from args[1] (a Literal(Value::String(s)), extracted via resolve_string_literal_arg
        // which shares the AdHocSelector precedent).  Both must succeed; either
        // falling through yields None (cell left at Undef — PRD invariant #2).
        // Zero kernel queries at construction time (K2/BT7); resolution is the
        // D8 interim (W_TOPOLOGY_TAG_STALE + [] until persistent-naming-v2).
        TopologySelectorHelper::Face => eval_named_leaf_selector_ctor(
            reify_core::ty::SelectorKind::Face,
            args,
            values,
            &function.name,
            diagnostics,
        ),
        TopologySelectorHelper::Edge => eval_named_leaf_selector_ctor(
            reify_core::ty::SelectorKind::Edge,
            args,
            values,
            &function.name,
            diagnostics,
        ),
        TopologySelectorHelper::SolidBody => eval_named_leaf_selector_ctor(
            reify_core::ty::SelectorKind::Body,
            args,
            values,
            &function.name,
            diagnostics,
        ),
    }
}

/// Dispatch the `shared_edges(face_a, face_b)` selector per design-doc §4.3.
///
/// Pipeline:
///   1. Derive each face's parent solid via `selector_vocabulary_v2::owner_body_of`
///      (which issues `GeometryQuery::OwnerBody` and decodes the `Value::Int`
///      reply). On query error → warning + `Value::Undef`.
///   2. If the two parents differ → push a "different parent solids" warning
///      and return `Value::List(vec![])` (silent degrade — empty list is
///      structurally valid as a `List<Geometry>` cell while the warning
///      surfaces the user-actionable issue).
///   3. Recover each face's 0-based index in the parent via
///      `extract_faces(parent)` + `position`. On extract error OR a face not
///      appearing in `extract_faces` → warning + `Value::Undef`.
///   4. Dispatch `GeometryQuery::SharedEdges { shape, face_a, face_b }`. On
///      query error or non-`Value::List` reply → warning + `Value::Undef`.
///   5. Map the reply integer indices back to edge handles via
///      `extract_edges(parent)`. Skip indices that fall outside the edge
///      enumeration (defensive against a kernel bug rather than a hard
///      failure mode — see design-doc §4.3 for the rationale).
///   6. Recover the parent solid's `(realization_ref, upstream_values_hash)` via
///      `resolve_owner_solid_handle(values, parent_a)`. Falls through to `None`
///      when the parent solid is not hydrated in `values` (PRD invariant #2).
///   7. Return `Value::List(Vec<Value::GeometryHandle>)` edge sub-handles per PRD §4 (KGQ-κ).
fn dispatch_shared_edges(
    kernel: &mut dyn reify_ir::GeometryKernel,
    face_a: GeometryHandleId,
    face_b: GeometryHandleId,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
    values: &reify_ir::ValueMap,
) -> Option<reify_ir::Value> {
    // (1) Derive parents via OwnerBody.
    let parent_a = match crate::selector_vocabulary_v2::owner_body_of(kernel, face_a) {
        Ok(p) => p,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} OwnerBody({:?}) failed: {}",
                helper_name, face_a, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let parent_b = match crate::selector_vocabulary_v2::owner_body_of(kernel, face_b) {
        Ok(p) => p,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} OwnerBody({:?}) failed: {}",
                helper_name, face_b, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    // (2) Cross-solid guard rail: empty list + warning when faces span
    //     different parents (design-doc §4.3).
    if parent_a != parent_b {
        diagnostics.push(Diagnostic::warning(format!(
            "{}: faces have different parent solids ({:?} vs {:?}); returning empty list",
            helper_name, parent_a, parent_b
        )));
        return Some(reify_ir::Value::List(Vec::new()));
    }

    // (3) Recover 0-based face indices via extract_faces(parent).
    let faces = match kernel.extract_faces(parent_a) {
        Ok(f) => f,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} extract_faces({:?}) failed: {}",
                helper_name, parent_a, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let idx_a = match faces.iter().position(|h| *h == face_a) {
        Some(i) => i,
        None => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}: face_a {:?} is not a child of parent {:?} (was extract_faces called?)",
                helper_name, face_a, parent_a
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let idx_b = match faces.iter().position(|h| *h == face_b) {
        Some(i) => i,
        None => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}: face_b {:?} is not a child of parent {:?} (was extract_faces called?)",
                helper_name, face_b, parent_a
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    // (4) Dispatch SharedEdges query.
    let reply = match kernel.query(&reify_ir::GeometryQuery::SharedEdges {
        shape: parent_a,
        face_a: idx_a,
        face_b: idx_b,
    }) {
        Ok(v) => v,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} SharedEdges query failed: {}",
                helper_name, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let int_indices = match reply {
        reify_ir::Value::List(items) => items,
        other => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}: expected Value::List from SharedEdges, got {:?}",
                helper_name, other
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    // (5) Map reply indices back to edge handles via extract_edges(parent).
    let edges = match kernel.extract_edges(parent_a) {
        Ok(e) => e,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} extract_edges({:?}) failed: {}",
                helper_name, parent_a, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let mut out: Vec<GeometryHandleId> = Vec::with_capacity(int_indices.len());
    for item in int_indices {
        let idx = match item {
            reify_ir::Value::Int(i) => i,
            other => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{}: expected Value::Int element in SharedEdges list, got {:?}",
                    helper_name, other
                )));
                return Some(reify_ir::Value::Undef);
            }
        };
        let usize_idx: usize = match idx.try_into() {
            Ok(u) => u,
            Err(_) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{}: SharedEdges returned negative index {}",
                    helper_name, idx
                )));
                return Some(reify_ir::Value::Undef);
            }
        };
        // Defensive: silently skip out-of-range indices rather than failing
        // hard — surfaces a malformed kernel reply as a smaller-than-expected
        // list rather than total cell collapse.
        if let Some(h) = edges.get(usize_idx) {
            out.push(*h);
        }
    }

    // (6) Recover the parent solid's realization_ref + upstream_values_hash from
    //     `values`. Edge sub-handles must compose from the parent solid's hash
    //     (PRD §4 cache coherence), not from a face sub-handle hash.
    //     Falls through to None when the parent solid cell is absent (e.g. unnamed
    //     inline solid), per PRD invariant #2 (never partial-construct sub-handles).
    let (parent_rr, parent_hash) = resolve_owner_solid_handle(values, parent_a)?;

    // (7) Emit List<Value::GeometryHandle> edge sub-handles via dispatch_filtered_subhandles,
    //     which re-extracts extract_edges(parent) to map retained ids → TopExp indices and
    //     builds make_sub_handle entries per PRD §4 iii/iv.
    dispatch_filtered_subhandles(
        kernel,
        parent_a,
        crate::topology_selectors::SubKind::Edge,
        &parent_rr,
        &parent_hash,
        Ok(out),
        helper_name,
        diagnostics,
    )
}

/// Run a pre-computed filtered-selector result and emit a `Value::List` of
/// `Value::GeometryHandle` sub-handles whose `upstream_values_hash` encodes the
/// canonical TopExp index of each retained sub-shape (PRD §4 iii/iv).
///
/// After the filter returns the retained `Vec<GeometryHandleId>`, we
/// re-extract the canonical sub-shape list and map each retained id to its
/// 0-based position, so `faces_by_normal(box,+z,1°)[0]` hashes identically to
/// `faces(box)[k]` for the same physical face.  Relies on PRD §4 intra-session
/// handle persistence (extract_* yields stable ids within a session).
///
/// Defensively warns + skips any retained id absent from the canonical list
/// rather than crashing — surfaces a malformed kernel state as a
/// shorter-than-expected list rather than total cell collapse.
#[allow(clippy::too_many_arguments)]
fn dispatch_filtered_subhandles(
    kernel: &mut dyn reify_ir::GeometryKernel,
    parent_kernel_handle: GeometryHandleId,
    sub_kind: crate::topology_selectors::SubKind,
    parent_rr: &reify_core::identity::RealizationNodeId,
    parent_hash: &[u8; 32],
    filter_result: Result<Vec<GeometryHandleId>, reify_ir::QueryError>,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let retained = match filter_result {
        Ok(ids) => ids,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    // Re-extract the canonical list to recover each retained id's TopExp index.
    // The filter helper already called extract_* once; this second call is safe
    // because PRD §4 intra-session handle persistence guarantees stable ids+order.
    let canonical = match sub_kind {
        crate::topology_selectors::SubKind::Edge => kernel.extract_edges(parent_kernel_handle),
        crate::topology_selectors::SubKind::Face => kernel.extract_faces(parent_kernel_handle),
        // SubKind::Solid is only used by the Split dispatch arm, which calls
        // execute_split directly — it never reaches dispatch_filtered_subhandles.
        crate::topology_selectors::SubKind::Solid => {
            unreachable!(
                "dispatch_filtered_subhandles called with SubKind::Solid — \
                 split pieces are handled by the Split arm via execute_split, \
                 not through the filter-subhandle path"
            )
        }
    };
    let canonical = match canonical {
        Ok(ids) => ids,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}: canonical re-extract failed: {}",
                helper_name, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    let canonical_index_map: std::collections::HashMap<GeometryHandleId, usize> = canonical
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();
    let mut elements: Vec<reify_ir::Value> = Vec::with_capacity(retained.len());
    for retained_id in retained {
        match canonical_index_map.get(&retained_id) {
            Some(&canonical_index) => {
                elements.push(crate::topology_selectors::make_sub_handle(
                    parent_rr,
                    parent_hash,
                    sub_kind,
                    canonical_index as u32,
                    retained_id,
                ));
            }
            None => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{}: retained handle {:?} absent from canonical list; skipping",
                    helper_name, retained_id
                )));
            }
        }
    }
    Some(reify_ir::Value::List(elements))
}

#[derive(Clone, Copy)]
enum TopologySelectorHelper {
    ClosestPoint,
    IsOn,
    AngleBetweenSurfaces,
    /// `edges(geometry) -> List<Geometry>` — extract the unique edges of a
    /// shape (task 3560).
    Edges,
    /// `faces(geometry) -> List<Geometry>` — extract the unique faces of a
    /// shape (task 3560).
    Faces,
    /// `center_of_mass(geometry, density) -> Point3<Length>` — uniform-density
    /// center of mass (task 3560).
    CenterOfMass,
    /// `moment_of_inertia(geometry, density) -> Tensor<2,3,MomentOfInertia>` —
    /// mass-weighted 3×3 inertia tensor about the centroid (task 3560).
    MomentOfInertia,
    /// `edges_by_length(geometry, Range<Length>) -> List<Geometry>` — edges
    /// whose length falls in the range (task 3560).
    EdgesByLength,
    /// `faces_by_area(geometry, Range<Area>) -> List<Geometry>` — faces whose
    /// surface area falls in the range (task 3560).
    FacesByArea,
    /// `faces_by_normal(geometry, Vec3, Angle) -> List<Geometry>` — faces
    /// whose outward normal is within an angular tolerance of a target
    /// direction (task 3560).
    FacesByNormal,
    /// `edges_parallel_to(geometry, Vec3, Angle) -> List<Geometry>` — edges
    /// whose midpoint tangent is (anti-)parallel to an axis within an
    /// angular tolerance (task 3560).
    EdgesParallelTo,
    /// `edges_at_height(geometry, Length, Length) -> List<Geometry>` — edges
    /// lying entirely within a tolerance of a horizontal `z = z0` plane
    /// (task 3560).
    EdgesAtHeight,
    /// `adjacent_faces(parent, face) -> List<Geometry>` — faces of `parent`
    /// that share at least one edge with `face` (task 3560).
    AdjacentFaces,
    /// `shared_edges(face_a, face_b) -> List<Geometry>` — edges of the
    /// common parent solid that lie on the boundary of BOTH faces (task 3560).
    /// Derives the parent via `OwnerBody` on both args; silently degrades to
    /// an empty list (with a warning) when the two faces live on different
    /// parent solids (design-doc §4.3).
    SharedEdges,
    /// `angle(a, b) -> Angle` — angle between two 3-D vectors (task 3614,
    /// PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-ε).
    /// Pure-math: `acos(clamp(dot(a,b)/(|a||b|), -1, 1))`. No kernel call.
    /// Args are value-flow `Vector<3>` resolved from `values`; zero-length
    /// or non-finite input emits a Warning and returns `Some(Value::Undef)`.
    Angle,
    /// `contains(solid, point) -> Bool` — test whether `point` is inside or
    /// on the boundary of the closed solid `solid` (task 3611, KGQ-β, PRD §9).
    /// Backed by `GeometryQuery::Contains` → `BRepClass3d_SolidClassifier`
    /// (IN || ON → true, OUT → false). Arg order is solid-then-point, mirroring
    /// `is_on` with args swapped. Default tolerance from
    /// `DEFAULT_CONTAINS_TOLERANCE_M` per §5.2.
    Contains,
    /// `geo_equiv(left, right, tol) -> Bool` — topology hash + N=8 parameter
    /// sample per §5.1 (task 3613, KGQ-δ, PRD §9). True iff BOTH topology
    /// (per-kind shape count) AND sampled-vertex tolerance hold.
    /// Uses `QueryCapability::BRepAndMesh`; sample count from
    /// `DEFAULT_GEO_EQUIV_SAMPLE_COUNT` (§5.2). Tolerance is an explicit
    /// user-supplied Length arg (no default constant per §5.2).
    ///
    /// FUTURE: `geo_equiv_strict(a, b, tol) -> Bool` — symmetric Hausdorff
    /// distance variant deferred to v0.4 (PRD §5.1, Open Question §10).
    GeoEquiv,
    /// `normal(surface, point) -> Vector3<Dimensionless>` — at-point outward
    /// unit surface normal (task 3615, KGQ-ζ, PRD §9). Projects the Cartesian
    /// `point` onto the face's parametric surface via `ShapeAnalysis_Surface::
    /// ValueOfUV` then returns the orientation-aware outward normal at the
    /// projected (u,v) — the REVERSED-flip convention shared with `FaceNormal`
    /// and `surface_normal_at`. Backed by `GeometryQuery::FaceNormalAt` →
    /// `surface_normal_at_point` in crates/reify-kernel-occt/src/lib.rs.
    /// Arg order: Surface=args[0] (named_steps), Point3<Length>=args[1] (values).
    Normal,
    /// `curvature(shape, point) -> Scalar<Curvature>|Matrix<2,2,Curvature>` —
    /// at-point curvature (task 3621, KGQ-μ, PRD §9). For a surface (face),
    /// returns a 2×2 `Value::Matrix` of principal curvatures [[κ_max,0],[0,κ_min]]
    /// as `Value::Scalar{ dimension: 1/Length }` cells; for a curve (edge),
    /// returns `Value::Scalar{ si_value: κ, dimension: 1/Length }`. Dispatches
    /// `GeometryQuery::SurfaceCurvatureAt` first; on error retries as
    /// `GeometryQuery::CurveCurvatureAt`. Backed by `OcctKernel::curvature_at`
    /// (surface) and `OcctKernel::curve_curvature_at` (curve).
    /// Arg order: Shape=args[0] (named_steps), Point3<Length>=args[1] (values).
    Curvature,
    /// `length(curve) -> Scalar<Length>` — arc length of a single-edge
    /// sub-handle (task 3622, KGQ-ν, PRD §9 Phase 4). Backed by
    /// `GeometryQuery::EdgeLength`. Arg: Curve=args[0] (values sub-handle).
    /// Multi-edge Curve composition deferred per PRD Open Question §10.6.
    Length,
    /// `perimeter(surface) -> Scalar<Length>` — sum of all boundary-edge
    /// lengths of a face sub-handle (task 3622, KGQ-ν, PRD §9 Phase 4).
    /// Composes `extract_edges(face)` + per-edge `EdgeLength`. No new FFI.
    /// Arg: Surface=args[0] (values sub-handle).
    Perimeter,
    /// `distance(a, b) -> Scalar<Length>` — Euclidean distance between two
    /// geometry objects (task 3610, KGQ-α, PRD §9).
    ///
    /// Dispatches a 2×2 arg-kind matrix:
    /// - Shape × Shape → `GeometryQuery::Distance{from,to}` via `kernel_distance`.
    /// - Shape × Point / Point × Shape → `GeometryQuery::ClosestPointOnShape`
    ///   on the shape + Euclidean to the query point.
    /// - Point × Point → pure Euclidean, no kernel call.
    ///
    /// Each arg is resolved as Shape (named_steps via `resolve_geometry_handle_arg`)
    /// else Point (`resolve_point3_length_arg` from values). Non-ValueRef args fall
    /// through to `None` (PRD §4 invariant #1 / #2). Kernel errors downgrade to
    /// `Some(Value::Undef)` + one Warning (invariant #3). At most one kernel
    /// query per call (invariant #4).
    Distance,
    /// `intersects(a, b) -> Bool` — test whether two geometry objects intersect
    /// (task 3612, KGQ-γ, PRD §9).
    ///
    /// Routes through `GeometryQuery::Distance{from,to}` via `kernel_distance`,
    /// classifying `d <= 0.0` → `Bool(true)` and `d > 0.0` → `Bool(false)`.
    /// This reproduces the shipped `shapes_intersect` adapter semantics
    /// (`reify-kernel-occt/src/lib.rs:770`: "Ok(true) iff min BREP distance ≤ 0.0")
    /// and the kinematic `interferes_with` precedent (`geometry_ops.rs:1601`).
    ///
    /// Both args must be Shape ValueRefs (resolved via `resolve_geometry_handle_arg`
    /// from `named_steps`). Non-ValueRef/non-geometry args fall through to `None`
    /// (PRD §4 invariants #1/#2). Kernel Err/non-numeric already emits one Warning
    /// and returns `None` → mapped to `Some(Undef)` (invariant #3). Exactly one
    /// kernel query (invariant #4).
    ///
    /// NOTE: A dedicated `GeometryQuery::Intersects` variant + `ManifoldKernel::query()`
    /// wiring + `#kernel(manifold)` cross-kernel parity gate is KGQ-ο (Phase 5).
    /// This task ships the eval dispatch arm only; the Manifold standalone function
    /// ships alongside in `crates/reify-kernel-manifold/src/queries.rs`.
    ///
    /// KNOWN PARITY DIVERGENCE (KGQ-ο concern): The Manifold-side
    /// `queries::intersects` uses strict CSG non-emptiness rather than `d ≤ 0.0`.
    /// Two solids sharing only a coincident face (BRep distance = 0.0, zero shared
    /// volume) return `true` here but `false` in the Manifold function (empty
    /// CSG intersection mesh).  KGQ-ο must resolve canonical boundary semantics
    /// before enabling the parity gate.  See also the "Known parity divergence"
    /// section in `crates/reify-kernel-manifold/src/queries.rs::intersects`.
    Intersects,
    /// `split(solid, plane) -> List<Geometry>` — split a solid into pieces by
    /// an unbounded planar cutting tool (task 4190, PRD ζ).
    ///
    /// Backed by `GeometryKernel::execute_split` →
    /// `BRepAlgoAPI_Splitter` (OCCT kernel). A non-intersecting plane yields a
    /// length-1 list containing the original solid unchanged.
    ///
    /// args[0]: solid `Value::GeometryHandle` (resolved from `values` via
    ///   `resolve_parent_geometry_handle_arg`, providing the parent
    ///   realization_ref + hash for sub-handle construction).
    /// args[1]: cutting plane `Value::Plane` (resolved from `values`, decoded
    ///   via `decode_plane` into (origin, unit_normal)).
    ///
    /// Each result piece is stored as a `Value::GeometryHandle` sub-handle via
    /// `make_sub_handle` with `SubKind::Solid` (0x03) — domain-separated from
    /// edge (0x01) and face (0x02) hashes.  On kernel error emits a Warning
    /// diagnostic and returns `Some(Value::Undef)`.  Non-Plane args[1] or
    /// unhydrated args[0] fall through to `None`.
    Split,
    /// `union(a, b, …) -> Selector(k)` — variadic same-kind selector union (task
    /// 4119 δ).  All operands must already be `Value::Selector(k)` of the SAME
    /// kind (K1); reconstructed via `reconstruct_selector_value` and combined via
    /// `SelectorValue::union`.  Arity ≥ 2 (variadic; bypasses the fixed
    /// `expected_arity` gate — see arity guard in `try_eval_topology_selector`).
    /// On `SelectorError::KindMismatch` from the value-layer: Warning + Undef
    /// (defensive backstop; compile-time E_SELECTOR_KIND_MISMATCH should have
    /// already fired).
    Union,
    /// `intersect(a, b, …) -> Selector(k)` — variadic same-kind selector
    /// intersection (task 4119 δ).  Mirrors `Union` in construction;
    /// `SelectorValue::intersect` enforces K1. Arity ≥ 2.
    Intersect,
    /// `difference(a, b) -> Selector(k)` — binary same-kind selector difference
    /// (task 4119 δ).  Arity exactly 2; `SelectorValue::difference` enforces K1.
    Difference,
    /// `face(geometry, name) -> Selector(Face)` — Named-leaf FaceSelector ctor
    /// (task 4119 δ, PRD §11.1).  Arity 2: args[0] = parent geometry ValueRef,
    /// args[1] = name string Literal.  Builds `LeafQuery::Named(name)` with
    /// `SelectorKind::Face`.  Resolution is the D8 interim (W_TOPOLOGY_TAG_STALE
    /// + [] for any name until persistent-naming-v2 lands).
    Face,
    /// `edge(geometry, name) -> Selector(Edge)` — Named-leaf EdgeSelector ctor
    /// (task 4119 δ, PRD §11.1).  Arity 2: args[0] = parent geometry ValueRef,
    /// args[1] = name string Literal.  Builds `LeafQuery::Named(name)` with
    /// `SelectorKind::Edge`.
    Edge,
    /// `solid_body(geometry, name) -> Selector(Body)` — Named-leaf BodySelector
    /// ctor (task 4119 δ, PRD §11.1).  Arity 2.  `body(...)` is the RBD ctor
    /// (StructureRef("Mechanism")) — `solid_body` is the verified-free alternative.
    SolidBody,
}

impl TopologySelectorHelper {
    /// The exact number of arguments this helper takes. Used by the
    /// per-helper arity gate in `try_eval_topology_selector` before any
    /// arg-shape resolution runs — non-matching arities fall through to
    /// `None` so the cell stays at the `Value::Undef` left by `eval_expr`.
    fn expected_arity(self) -> usize {
        match self {
            TopologySelectorHelper::ClosestPoint
            | TopologySelectorHelper::IsOn
            | TopologySelectorHelper::AngleBetweenSurfaces
            | TopologySelectorHelper::CenterOfMass
            | TopologySelectorHelper::MomentOfInertia
            | TopologySelectorHelper::EdgesByLength
            | TopologySelectorHelper::FacesByArea
            | TopologySelectorHelper::AdjacentFaces
            | TopologySelectorHelper::SharedEdges
            | TopologySelectorHelper::Angle
            | TopologySelectorHelper::Contains
            | TopologySelectorHelper::Normal
            | TopologySelectorHelper::Curvature
            | TopologySelectorHelper::Distance
            | TopologySelectorHelper::Intersects
            | TopologySelectorHelper::Split
            // task 4119 δ: difference is binary; Union/Intersect are variadic
            // (≥ 2) but list 2 here as their minimum arity. The arity gate in
            // try_eval_topology_selector special-cases them to use a ≥2 check
            // rather than the exact equality check, so this value is not used
            // for the Union/Intersect path.
            | TopologySelectorHelper::Difference
            | TopologySelectorHelper::Union
            | TopologySelectorHelper::Intersect
            // task 4119 δ: Named-leaf ctors are arity 2 (geometry, name).
            | TopologySelectorHelper::Face
            | TopologySelectorHelper::Edge
            | TopologySelectorHelper::SolidBody => 2,
            TopologySelectorHelper::Edges
            | TopologySelectorHelper::Faces
            | TopologySelectorHelper::Length
            | TopologySelectorHelper::Perimeter => 1,
            TopologySelectorHelper::FacesByNormal
            | TopologySelectorHelper::EdgesParallelTo
            | TopologySelectorHelper::EdgesAtHeight
            | TopologySelectorHelper::GeoEquiv => 3,
        }
    }
}

/// Issue `extract_edges` (or `extract_faces`, per `sub_kind`) for the given
/// parent kernel handle and return a `Value::List` of `Value::GeometryHandle`
/// sub-handles (PRD §4). Each element carries:
/// - `realization_ref` — cloned from the parent (unchanged per PRD §4).
/// - `upstream_values_hash` — `compose_sub_handle_hash(parent_hash, sub_kind, index)`.
/// - `kernel_handle` — the kernel id returned by `extract_edges`/`extract_faces`.
///
/// Returns `Some(Value::Undef)` (with a Warning diagnostic) on kernel error —
/// preserving the same defensive-downgrade contract as the sibling dispatchers
/// (`dispatch_point3_length_reply`, `dispatch_point_on_shape`, etc.).
// Task 4118 (γ): the `edges`/`faces` All-leaf construction path is now
// kernel-FREE (see `build_leaf_selector`), so eager sub-shape extraction has no
// caller at construction time. Retained (allow dead_code) — the kernel-bearing
// `ResolveSelector` resolution path (step-6) re-realizes selectors and may reuse
// this eager-extraction shape; remove if it stays unused.
#[allow(dead_code)]
fn dispatch_extract_subshapes(
    kernel: &mut dyn reify_ir::GeometryKernel,
    parent_kernel_handle: GeometryHandleId,
    sub_kind: crate::topology_selectors::SubKind,
    parent_realization_ref: &reify_core::identity::RealizationNodeId,
    parent_hash: &[u8; 32],
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let result = match sub_kind {
        crate::topology_selectors::SubKind::Edge => kernel.extract_edges(parent_kernel_handle),
        crate::topology_selectors::SubKind::Face => kernel.extract_faces(parent_kernel_handle),
        // SubKind::Solid is only used by the Split dispatch arm, which calls
        // execute_split directly and does NOT go through dispatch_extract_subshapes.
        crate::topology_selectors::SubKind::Solid => {
            unreachable!(
                "dispatch_extract_subshapes called with SubKind::Solid — \
                 split pieces are produced via execute_split in the Split arm, \
                 not through the extract-subshapes path"
            )
        }
    };
    match result {
        Ok(sub_ids) => {
            let elements = sub_ids
                .into_iter()
                .enumerate()
                .map(|(i, sub_kernel_id)| {
                    crate::topology_selectors::make_sub_handle(
                        parent_realization_ref,
                        parent_hash,
                        sub_kind,
                        i as u32,
                        sub_kernel_id,
                    )
                })
                .collect();
            Some(reify_ir::Value::List(elements))
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}({:?}): kernel error: {}",
                helper_name, parent_kernel_handle, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Short label for a `Value` that failed `Point<Length>` classification, used
/// as the `got` field of the rejection diagnostic (task ε). A `Value::Point` is
/// distinguished by wrong arity vs. carrying a wrong-dimension / non-Scalar
/// component, so the Warning names what actually went wrong; any other value is
/// labelled by its kind.
fn point3_got_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Point(items) if items.len() != 3 => {
            format!("Point of {} components", items.len())
        }
        reify_ir::Value::Point(_) => {
            "Point with a non-Length or non-Scalar component".to_string()
        }
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => scalar_got_label(dimension),
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::Int(_) => "Int".to_string(),
        reify_ir::Value::Vector(_) => "Vector".to_string(),
        _ => "non-Point value".to_string(),
    }
}

/// Resolve a 3-component point arg to its `[f64; 3]` SI-metre components,
/// emitting a `Severity::Warning` when the caller passes a defined-but-wrong
/// value.
///
/// Evaluate-then-accept (task ε): the arg expr is EVALUATED against `values`
/// (via [`eval_arg_value`]) and the resulting `Value` classified. A `ValueRef →
/// Value::Point` cell (the common let-bound `let p = point3(x, y, z)` form)
/// reads the cell (now an owned clone; see [`eval_arg_value`]) — functionally
/// identical to the prior `values.get(id)` path — while an inline point
/// expression now EVALUATES rather than falling through to a silent `None`. The value must be a `Value::Point` of exactly three
/// LENGTH-dimensioned `Value::Scalar` components: the cell type is fixed at
/// `Type::Point<Length>` by the compile-time wiring in `expr.rs`, so a
/// well-formed Scalar component MUST carry `DimensionVector::LENGTH` — a
/// wrong-dimensioned Scalar slipping through would be silently reinterpreted as
/// metres at the kernel boundary, so a `debug_assert` surfaces the violation in
/// tests; in release we fall through to the rejection path rather than feed the
/// kernel garbage.
///
/// | evaluated arg value                                   | return       | diagnostic?     |
/// |-------------------------------------------------------|--------------|-----------------|
/// | `Value::Undef` (missing/Undef cell, user-fn arg)      | `None`       | no — quiet      |
/// | `Value::Point` of 3 LENGTH `Scalar`s                  | `Some([..])` | no              |
/// | non-Point, wrong arity, or non-LENGTH/non-Scalar comp | `None`       | yes — 1 Warning |
fn resolve_point3_length_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<[f64; 3]> {
    use crate::arg_acceptance::ArgRejection;

    let value = eval_arg_value(expr, values);

    // Quiet degradation: an Undef value (missing cell, or a user-fn/meta arg the
    // local ctx can't evaluate) returns None with no diagnostic — behaviourally
    // identical to the prior `values.get(id)?` fall-through for a missing cell.
    if matches!(value, reify_ir::Value::Undef) {
        return None;
    }

    // A Value::Point of exactly three LENGTH-dimensioned Scalars resolves to its
    // SI-metre components (the `debug_assert` + LENGTH check is preserved from
    // the prior ValueRef path).
    let as_point3 = |v: &reify_ir::Value| -> Option<[f64; 3]> {
        let components = match v {
            reify_ir::Value::Point(items) if items.len() == 3 => items,
            _ => return None,
        };
        let mut out = [0.0_f64; 3];
        for (i, comp) in components.iter().enumerate() {
            match comp {
                reify_ir::Value::Scalar {
                    si_value,
                    dimension,
                } => {
                    debug_assert!(
                        *dimension == reify_core::DimensionVector::LENGTH,
                        "resolve_point3_length_arg: expected LENGTH-dimensioned Scalar, \
                         got dimension {:?} (si_value={}); cell type is Point<Length> per \
                         compile-time wiring in expr.rs",
                        dimension,
                        si_value
                    );
                    if *dimension != reify_core::DimensionVector::LENGTH {
                        return None;
                    }
                    out[i] = *si_value;
                }
                _ => return None,
            }
        }
        Some(out)
    };
    if let Some(components) = as_point3(&value) {
        return Some(components);
    }

    // Defined-but-wrong (non-Point, wrong arity, or non-LENGTH/non-Scalar
    // component): emit exactly one Warning naming builtin/arg/Point<Length>/got
    // (byte-uniform wording with the density / vec3 / range paths).
    diagnostics.push(Diagnostic::warning(
        ArgRejection {
            got: point3_got_label(&value),
            expected: "Point<Length>",
            migration_hint: None,
        }
        .message(builtin, arg_name),
    ));
    None
}

/// Evaluate an argument `CompiledExpr` against the `ValueMap` with a LOCAL
/// context (no user-defined functions, no meta block) — the evaluate-then-accept
/// mechanism shared by the task ε (4492) owned-arg resolvers.
///
/// A `ValueRef` resolves via `get_or_undef`, preserving quiet degradation for a
/// missing or `Value::Undef` cell. The resulting `Value` is *behaviourally*
/// identical to what the prior `values.get(id)` shape-match produced, with one
/// nuance: `eval_expr` returns an OWNED `Value` (`get_or_undef` clones the cell)
/// where the prior path borrowed, so a Point/Vector/Range cell now incurs one
/// `Vec<Scalar>` clone per resolve. The cost is negligible against the kernel
/// round-trips these dispatchers perform. Inline literals, field-access, and
/// range/vector/arithmetic constructors now EVALUATE rather than falling through
/// to a silent `None`.
///
/// User-defined-function-call / meta-block args in these positions evaluate to
/// `Value::Undef` → quiet `None`, consistent with the degradation contract: the
/// selector/kinematic/ad-hoc dispatch fns that own these args do not carry
/// `functions`/`meta_map` (only `compile_geometry_op` does), so the local
/// `EvalContext::new(values, &[])` is faithful to PRD decision 10's load-bearing
/// intent ("evaluate the arg expr against the `ValueMap`"). See task ε design
/// decision 1.
///
/// The local `EvalContext` carries no diagnostics sink (`diagnostics: None`), so
/// any RUNTIME diagnostic `eval_expr` might emit while evaluating an inline arg
/// expression (e.g. a field-OOB or undef-builtin warning) is intentionally
/// dropped here — the v0.1 arg shapes these resolvers accept (scalars, points,
/// vec3, ranges, strings, ints) do not trigger such diagnostics. A future arg
/// form that did would need a `with_runtime_diagnostics` sink drained into the
/// caller's `diagnostics` vec.
fn eval_arg_value(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> reify_ir::Value {
    reify_expr::eval_expr(expr, &reify_expr::EvalContext::new(values, &[]))
}

/// Resolve the `density` argument of `center_of_mass` and `moment_of_inertia`
/// to a raw `f64` (SI kg/m³), emitting a `Severity::Warning` when the caller
/// passes a defined-but-wrong type.
///
/// Contract A (task 4486 γ) + evaluate-then-accept (task 4492 ε): the arg expr
/// is EVALUATED against `values` (via [`eval_arg_value`]) and the resulting
/// `Value` classified by [`crate::arg_acceptance::accept_arg`] with
/// [`crate::arg_acceptance::density_spec`]. Inline / computed density
/// expressions (e.g. `moment_of_inertia(b, 7850kg/m^3)`) now WORK — the γ
/// "must be bound to a let / not yet supported" fall-through is gone.
///
/// | evaluated arg value                           | return       | diagnostic pushed?        |
/// |-----------------------------------------------|--------------|---------------------------|
/// | `Value::Undef` (missing/Undef cell, or a      | `None`       | no — quiet degradation    |
/// |   user-fn/meta arg the local ctx can't eval)  |              |                           |
/// | `Value::Scalar{MASS_DENSITY,v}` (inline lit,  | `Some(v)`    | no                        |
/// |   `ValueRef`, field-access, or arithmetic)    |              |                           |
/// | bare `Value::Real`, dimensionless/wrong-dim   | `None`       | yes — `Severity::Warning` |
/// |   `Value::Scalar`, or any non-numeric `Value` |              | naming `density` + hint   |
fn resolve_density_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    use crate::arg_acceptance::{accept_arg, density_spec, Acceptance};

    let value = eval_arg_value(expr, values);

    match accept_arg(&value, &density_spec()) {
        Acceptance::Accepted(si) => Some(si),
        Acceptance::Undefined => None,
        Acceptance::Rejected(rej) => {
            diagnostics.push(Diagnostic::warning(rej.message(helper_name, "density")));
            None
        }
    }
}

/// Read a single Vec3 component: a `Value::Real` or a dimensionless
/// `Value::Scalar`. Returns `None` for any dimensioned Scalar or non-numeric
/// payload — the direction/axis args of `faces_by_normal` / `edges_parallel_to`
/// are pure unit-vector numerics in v0.1.
fn vec3_component_si(value: &reify_ir::Value) -> Option<f64> {
    match value {
        reify_ir::Value::Real(v) => Some(*v),
        reify_ir::Value::Scalar {
            si_value,
            dimension,
        } if *dimension == reify_core::DimensionVector::DIMENSIONLESS => Some(*si_value),
        _ => None,
    }
}

/// Short human-readable label for a `Value` that failed Vec3 classification,
/// used as the `got` field of the rejection diagnostic (task ε). Vec3-aware: a
/// `Value::Vector` of the wrong arity or carrying a dimensioned / non-numeric
/// component is distinguished from a plainly non-Vector value, so the Warning
/// names what actually went wrong.
fn vec3_got_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Vector(items) if items.len() != 3 => {
            format!("Vector of {} components", items.len())
        }
        reify_ir::Value::Vector(_) => {
            "Vector with a dimensioned or non-numeric component".to_string()
        }
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => scalar_got_label(dimension),
        reify_ir::Value::Point(_) => "Point".to_string(),
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::Int(_) => "Int".to_string(),
        _ => "non-Vec3 value".to_string(),
    }
}

/// Resolve a 3-component vector arg to its `[f64; 3]` SI components, emitting a
/// `Severity::Warning` when the caller passes a defined-but-wrong value.
///
/// Evaluate-then-accept (task ε): the arg expr is EVALUATED against `values`
/// (via [`eval_arg_value`]) and the resulting `Value` classified. Inline
/// `vec3(...)` constructor calls now WORK — `eval_expr` lowers the `vec3(...)`
/// `FunctionCall` to a `Value::Vector` via `reify_stdlib::eval_builtin`, so the
/// γ "Literal/`ValueRef` shape-match only → silent fall-through" behaviour is
/// gone. Each component must still be a `Value::Real` or a dimensionless
/// `Value::Scalar` (per [`vec3_component_si`]); the vector must have exactly
/// three components — the direction/axis args of `faces_by_normal` /
/// `edges_parallel_to` / `angle` are pure unit-vector numerics in v0.1.
///
/// | evaluated arg value                                | return       | diagnostic?     |
/// |----------------------------------------------------|--------------|-----------------|
/// | `Value::Undef` (missing/Undef cell, user-fn arg)   | `None`       | no — quiet      |
/// | `Value::Vector` of 3 `Real`/dimensionless `Scalar` | `Some([..])` | no              |
/// | non-Vector, wrong length, or dimensioned component | `None`       | yes — 1 Warning |
fn resolve_vec3_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<[f64; 3]> {
    use crate::arg_acceptance::ArgRejection;

    let value = eval_arg_value(expr, values);

    // Quiet degradation: an Undef value (missing cell, or a user-fn/meta arg the
    // local ctx can't evaluate) returns None with no diagnostic — behaviourally
    // identical to the γ fall-through for missing cells.
    if matches!(value, reify_ir::Value::Undef) {
        return None;
    }

    let as_vec3 = |v: &reify_ir::Value| -> Option<[f64; 3]> {
        match v {
            reify_ir::Value::Vector(items) if items.len() == 3 => Some([
                vec3_component_si(&items[0])?,
                vec3_component_si(&items[1])?,
                vec3_component_si(&items[2])?,
            ]),
            _ => None,
        }
    };
    if let Some(components) = as_vec3(&value) {
        return Some(components);
    }

    // Defined-but-wrong: emit exactly one Warning naming builtin/arg/Vec3/got
    // (byte-uniform wording with the density / scalar-bound paths).
    diagnostics.push(Diagnostic::warning(
        ArgRejection {
            got: vec3_got_label(&value),
            expected: "Vec3",
            migration_hint: None,
        }
        .message(builtin, arg_name),
    ));
    None
}

/// Shared evaluate-then-accept core for the SCALAR-dimension owned args
/// (task ε): EVALUATE `expr` against `values` (via [`eval_arg_value`]) and
/// classify the resulting `Value` against an inline
/// [`crate::arg_acceptance::ArgSpec`] of `expected_dim`. `Value::Undef`
/// degrades quietly to `None`; a defined-but-wrong value pushes exactly one
/// `Severity::Warning` (built from the rejection + `builtin`/`arg_name` labels,
/// byte-uniform with the density path) and returns `None`.
fn resolve_scalar_dim_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    expected_dim: reify_core::DimensionVector,
    type_name: &'static str,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    use crate::arg_acceptance::{accept_arg, Acceptance, ArgSpec};

    let value = eval_arg_value(expr, values);
    let spec = ArgSpec {
        type_name,
        dimension: expected_dim,
        migration_hint: None,
    };
    match accept_arg(&value, &spec) {
        Acceptance::Accepted(si) => Some(si),
        Acceptance::Undefined => None,
        Acceptance::Rejected(rej) => {
            diagnostics.push(Diagnostic::warning(rej.message(builtin, arg_name)));
            None
        }
    }
}

/// Resolve an ANGLE-dimensioned scalar arg to its SI value (radians).
/// EVALUATES the arg expr (task ε): an inline dimensioned-angle literal, a
/// `ValueRef → ANGLE Scalar` (let-bound `let tol = 1deg`), or an angle-typed
/// arithmetic expression all WORK. A `Value::Undef` (missing cell, etc.)
/// degrades quietly; a defined-but-wrong value (wrong dimension, non-Scalar)
/// pushes exactly one `Severity::Warning` naming `builtin`/`arg_name`. Pins the
/// ANGLE dimension for the angular-tolerance args of `faces_by_normal` /
/// `edges_parallel_to`.
fn resolve_angle_scalar_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    resolve_scalar_dim_arg(
        expr,
        values,
        reify_core::DimensionVector::ANGLE,
        "Angle",
        builtin,
        arg_name,
        diagnostics,
    )
}

/// Resolve a LENGTH-dimensioned scalar arg to its SI value (metres).
/// EVALUATES the arg expr (task ε): an inline dimensioned-length literal, a
/// `ValueRef → LENGTH Scalar` (let-bound `let z = 0mm`), or a length-typed
/// arithmetic expression all WORK. A `Value::Undef` degrades quietly; a
/// defined-but-wrong value pushes exactly one `Severity::Warning` naming
/// `builtin`/`arg_name`. Pins the LENGTH dimension for the z-plane / tolerance
/// args of `edges_at_height` and the tolerance arg of `geo_equiv`.
fn resolve_length_scalar_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    resolve_scalar_dim_arg(
        expr,
        values,
        reify_core::DimensionVector::LENGTH,
        "Length",
        builtin,
        arg_name,
        diagnostics,
    )
}

/// Read a `Value::Scalar` whose `dimension` is `expected_dim` and return its
/// SI value. `None` for any other shape (wrong dimension, non-Scalar).
fn scalar_si_with_dim(
    value: &reify_ir::Value,
    expected_dim: reify_core::DimensionVector,
) -> Option<f64> {
    match value {
        reify_ir::Value::Scalar {
            si_value,
            dimension,
        } if *dimension == expected_dim => Some(*si_value),
        _ => None,
    }
}

/// Human-readable expected-type label for a `Range<dim>` rejection diagnostic
/// (task ε). The two real callers pin `LENGTH` (`edges_by_length`) and `AREA`
/// (`faces_by_area`); any other dimension degrades to a bare `"Range"`.
fn range_expected_label(expected_dim: reify_core::DimensionVector) -> &'static str {
    if expected_dim == reify_core::DimensionVector::LENGTH {
        "Range<Length>"
    } else if expected_dim == reify_core::DimensionVector::AREA {
        "Range<Area>"
    } else {
        "Range"
    }
}

/// Short label for a `Value` that failed `Range<dim>` classification, used as
/// the `got` field of the rejection diagnostic (task ε). A `Value::Range` is
/// distinguished as half-open (one bound `None`) vs. carrying a wrong-dimension
/// / non-Scalar bound, so the Warning names what actually went wrong; any other
/// value is labelled by its kind.
fn range_got_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Range { lower, upper, .. } if lower.is_none() || upper.is_none() => {
            "half-open Range".to_string()
        }
        reify_ir::Value::Range { .. } => {
            "Range with a wrong-dimension or non-Scalar bound".to_string()
        }
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => scalar_got_label(dimension),
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::Int(_) => "Int".to_string(),
        reify_ir::Value::Point(_) => "Point".to_string(),
        reify_ir::Value::Vector(_) => "Vector".to_string(),
        _ => "non-Range value".to_string(),
    }
}

/// Resolve a `Range<Quantity>` arg to its `(lower_si, upper_si)` SI bounds,
/// both dimensioned `expected_dim`, emitting a `Severity::Warning` when the
/// caller passes a defined-but-wrong value.
///
/// Evaluate-then-accept (task ε): the arg expr is EVALUATED against `values`
/// (via [`eval_arg_value`]) and the resulting `Value` classified. `eval_expr`
/// lowers an inline `RangeConstructor` — including one with computed bounds
/// such as `0mm..(20mm + 30mm)` — to a `Value::Range`, and a `ValueRef →
/// Value::Range` (the common let-bound `let r = 0mm..50mm` form) reads the
/// cell; so the former Literal/ValueRef/RangeConstructor shape-match COLLAPSES
/// into one `Value::Range` classification, and the γ "inline computed bound →
/// silent fall-through" behaviour is gone. Both bounds must be present (a
/// half-open range is rejected — the v0.1 filtered selectors require a closed
/// `[lo, hi]` window) and dimensioned `expected_dim`.
///
/// | evaluated arg value                                 | return       | diagnostic?     |
/// |-----------------------------------------------------|--------------|-----------------|
/// | `Value::Undef` (missing/Undef cell, user-fn arg)    | `None`       | no — quiet      |
/// | closed `Value::Range` of two `expected_dim` Scalars | `Some((..))` | no              |
/// | non-Range, half-open, or wrong-dimension bound      | `None`       | yes — 1 Warning |
fn resolve_range_dim_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    expected_dim: reify_core::DimensionVector,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(f64, f64)> {
    use crate::arg_acceptance::ArgRejection;

    let value = eval_arg_value(expr, values);

    // Quiet degradation: an Undef value (missing cell, or a user-fn/meta arg the
    // local ctx can't evaluate) returns None with no diagnostic.
    if matches!(value, reify_ir::Value::Undef) {
        return None;
    }

    // A closed Range of two `expected_dim` Scalars resolves to its SI bounds.
    if let reify_ir::Value::Range {
        lower: Some(lo),
        upper: Some(hi),
        ..
    } = &value
        && let (Some(lo_si), Some(hi_si)) = (
            scalar_si_with_dim(lo, expected_dim),
            scalar_si_with_dim(hi, expected_dim),
        )
    {
        return Some((lo_si, hi_si));
    }

    // Defined-but-wrong (non-Range, half-open, or wrong-dimension bound): emit
    // exactly one Warning naming builtin/arg/Range<dim>/got (byte-uniform
    // wording with the density / vec3 / scalar-bound paths).
    diagnostics.push(Diagnostic::warning(
        ArgRejection {
            got: range_got_label(&value),
            expected: range_expected_label(expected_dim),
            migration_hint: None,
        }
        .message(builtin, arg_name),
    ));
    None
}

/// Scan `values` for the `Value::GeometryHandle` whose `kernel_handle ==
/// parent_body_kh` and return its `(realization_ref, upstream_values_hash)`.
///
/// Used by `dispatch_shared_edges` to recover the parent solid's hash for edge
/// sub-handle construction (PRD §4 cache coherence): edge sub-handles must
/// compose from the parent solid's `upstream_values_hash`, not from a face
/// sub-handle's hash. The parent solid cell is hydrated into `values` by
/// `post_process_geometry_handle_cells` (engine_build.rs:3693-3700).
///
/// Returns `None` when no matching cell is found (e.g. unnamed inline solid),
/// causing the caller to fall through per PRD invariant #2 (never
/// partial-construct sub-handles from a non-hydrated geometry cell).
///
/// # Uniqueness assumption
/// Kernel handles are unique per shape within a session (PRD §4 intra-session
/// handle persistence), so at most one `Value::GeometryHandle` in `values`
/// carries any given `kernel_handle`. The linear scan returns on the first
/// match; that match is expected to be the only one.
fn resolve_owner_solid_handle(
    values: &reify_ir::ValueMap,
    parent_body_kh: GeometryHandleId,
) -> Option<(reify_core::identity::RealizationNodeId, [u8; 32])> {
    for (_, value) in values.iter() {
        if let reify_ir::Value::GeometryHandle {
            realization_ref,
            upstream_values_hash,
            kernel_handle,
        } = value
            && *kernel_handle == parent_body_kh
        {
            return Some((realization_ref.clone(), *upstream_values_hash));
        }
    }
    None
}

/// Resolve a `CompiledExprKind::ValueRef` geometry-arg to a `GeometryHandleId`
/// via `named_steps`. Returns `None` for any non-`ValueRef` shape or missing
/// `named_steps` entry — caller maps to the "unsupported arg shape → fall
/// through" behaviour.
fn resolve_geometry_handle_arg(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
) -> Option<GeometryHandleId> {
    let cell_id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    named_steps.get(&cell_id.member).map(|kh| kh.id)
}

/// Resolve a `CompiledExprKind::ValueRef` arg to the full parent
/// `Value::GeometryHandle` fields: `(realization_ref, upstream_values_hash,
/// kernel_handle)`. Returns `None` for any non-`ValueRef` shape, a missing
/// cell, or a cell that is not a `Value::GeometryHandle` — the caller falls
/// through, leaving the selector cell at its compiled default (`Value::Undef`).
///
/// PRD §4 invariant #2: sub-handles must never be partially constructed from
/// a non-hydrated geometry cell. This gate enforces that contract at the
/// dispatch boundary.
fn resolve_parent_geometry_handle_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<(
    reify_core::identity::RealizationNodeId,
    [u8; 32],
    GeometryHandleId,
)> {
    let cell_id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    match values.get(cell_id)? {
        reify_ir::Value::GeometryHandle {
            realization_ref,
            upstream_values_hash,
            kernel_handle,
        } => Some((
            realization_ref.clone(),
            *upstream_values_hash,
            *kernel_handle,
        )),
        _ => None,
    }
}

/// Issue a query whose kernel reply is the canonical JSON-Point3
/// (`{"x":_,"y":_,"z":_}`) wire format and unwrap to a
/// `Value::Point(vec![length, length, length])`. Returns
/// `Some(Value::Undef)` (with a Warning diagnostic) on a kernel error or a
/// malformed reply. Shared by `closest_point` (`ClosestPointOnShape`),
/// `center_of_mass` (`CenterOfMass`), and the whole-handle `centroid`
/// (`Centroid`, task 3608) — all return the identical JSON-Point3 encoding per
/// the `GeometryQuery` doc, so a single decode path serves them.
///
/// Takes `&dyn` (not `&mut dyn`): `GeometryKernel::query` is `&self`, so an
/// immutable borrow suffices, and `&mut dyn` call sites reborrow to `&dyn`
/// automatically.
fn dispatch_point3_length_reply(
    kernel: &dyn reify_ir::GeometryKernel,
    query: &reify_ir::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(query) {
        Ok(value) => match crate::topology_selectors::parse_xyz_value(&value, helper_name) {
            Ok([x, y, z]) => Some(reify_ir::Value::Point(vec![
                reify_ir::Value::length(x),
                reify_ir::Value::length(y),
                reify_ir::Value::length(z),
            ])),
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{} kernel reply parse failed: {}",
                    helper_name, err
                )));
                Some(reify_ir::Value::Undef)
            }
        },
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Issue a `FaceNormalAt` query and unwrap the kernel's JSON-Point3 reply
/// (`{"x":_,"y":_,"z":_}`) into a dimensionless `Value::Vector([Real, Real,
/// Real])` — the canonical unit normal representation (matches `vec3()` output
/// and `resolve_vec3_arg` expectations).
///
/// Models the same defensive-downgrade contract as `dispatch_point3_length_reply`:
/// - Kernel `Err` → one Warning ("`normal` kernel query failed: …") + `Some(Value::Undef)`
/// - Malformed reply (non-String or bad JSON) → one Warning ("`normal` kernel reply parse
///   failed: …") + `Some(Value::Undef)`
///
/// Powers `TopologySelectorHelper::Normal` (task 3615, KGQ-ζ).
fn dispatch_normal_vector3(
    kernel: &mut dyn reify_ir::GeometryKernel,
    query: &reify_ir::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(query) {
        Ok(value) => match crate::topology_selectors::parse_xyz_value(&value, helper_name) {
            Ok([x, y, z]) => Some(reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(x),
                reify_ir::Value::Real(y),
                reify_ir::Value::Real(z),
            ])),
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{} kernel reply parse failed: {}",
                    helper_name, err
                )));
                Some(reify_ir::Value::Undef)
            }
        },
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Curvature dimension constant: 1/Length = Length^-1 (m⁻¹ in SI).
/// Used by `dispatch_curvature` to tag scalar and matrix cells.
/// The curvature dimension is LENGTH^-1. index 0 = LENGTH in DimensionVector.
const CURVATURE_DIM: reify_core::dimension::DimensionVector = {
    let mut d = [reify_core::dimension::Rational::ZERO; 10];
    d[0] = reify_core::dimension::Rational::new(-1, 1);
    reify_core::dimension::DimensionVector(d)
};

/// Dispatch a `length(curve)` query for `TopologySelectorHelper::Length`
/// (task 3622, KGQ-ν).
///
/// Issues `GeometryQuery::EdgeLength(handle)` and wraps the reply as
/// `Value::length(metres)`. Returns `Some(Value::Undef)` + one Warning on
/// Err or an unexpected kernel reply type (PRD §4 defensive-downgrade contract).
fn dispatch_edge_length(
    kernel: &mut dyn reify_ir::GeometryKernel,
    handle: reify_ir::GeometryHandleId,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(&reify_ir::GeometryQuery::EdgeLength(handle)) {
        Ok(reify_ir::Value::Real(l)) => Some(reify_ir::Value::length(l)),
        Ok(reify_ir::Value::Scalar { si_value, .. }) => Some(reify_ir::Value::length(si_value)),
        Ok(unexpected) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} kernel reply has unexpected type (expected Real, got {unexpected:?}); \
                 cell left at Undef",
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} kernel query failed: {err}",
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Dispatch a `perimeter(surface)` query for `TopologySelectorHelper::Perimeter`
/// (task 3622, KGQ-ν).
///
/// Composes `kernel.extract_edges(face_kh)` + per-edge `EdgeLength`. On any
/// extract error or per-edge non-Real reply, emits exactly one Warning and
/// returns `Some(Value::Undef)` (PRD §4 defensive-downgrade contract).
fn dispatch_perimeter(
    kernel: &mut dyn reify_ir::GeometryKernel,
    face_kh: reify_ir::GeometryHandleId,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let edges = match kernel.extract_edges(face_kh) {
        Ok(e) => e,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} extract_edges failed: {err}",
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    // Degenerate face: no boundary edges → silent 0.0 would mask a real kernel
    // problem; downgrade like the other failure modes instead.
    if edges.is_empty() {
        diagnostics.push(Diagnostic::warning(format!(
            "{helper_name} extract_edges returned no boundary edges for face \
             {face_kh:?}; degenerate geometry; cell left at Undef",
        )));
        return Some(reify_ir::Value::Undef);
    }
    let mut total_m = 0.0_f64;
    for edge_id in &edges {
        match kernel.query(&reify_ir::GeometryQuery::EdgeLength(*edge_id)) {
            Ok(reify_ir::Value::Real(l)) => total_m += l,
            Ok(reify_ir::Value::Scalar { si_value, .. }) => total_m += si_value,
            Ok(unexpected) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{helper_name} EdgeLength for edge {edge_id:?} has unexpected type \
                     (expected Real, got {unexpected:?}); cell left at Undef",
                )));
                return Some(reify_ir::Value::Undef);
            }
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{helper_name} EdgeLength for edge {edge_id:?} failed: {err}",
                )));
                return Some(reify_ir::Value::Undef);
            }
        }
    }
    Some(reify_ir::Value::length(total_m))
}

/// Dispatch a `curvature(shape, point)` query for `TopologySelectorHelper::Curvature`
/// (task 3621, KGQ-μ).
///
/// Strategy: try `SurfaceCurvatureAt{handle, u=px, v=py}` first. If the kernel
/// returns Ok, decode the `[[κ_max,0],[0,κ_min]]` nested-List wire value into a
/// `Value::Matrix` of `Value::Scalar{si_value, dimension: 1/Length}` cells. If
/// the kernel returns Err, retry as `CurveCurvatureAt{handle,px,py,pz}` and
/// return `Value::Scalar{si_value: κ, dimension: 1/Length}`. If both fail, emit
/// exactly one Warning naming `helper_name` and return `Some(Value::Undef)`.
///
/// The surface wire note: the kernel encodes the principal-curvature matrix as a
/// diagonal `[[kappa_max, 0.0], [0.0, kappa_min]]` (InertiaTensor wire convention)
/// so trace/2 = mean curvature H and det = Gaussian curvature K.
fn dispatch_curvature(
    kernel: &mut dyn reify_ir::GeometryKernel,
    handle: reify_ir::GeometryHandleId,
    point: [f64; 3],
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // Try surface first: (px, py) → (u, v) per design decision §3.
    let surface_query = reify_ir::GeometryQuery::SurfaceCurvatureAt {
        handle,
        u: point[0],
        v: point[1],
    };
    if let Ok(value) = kernel.query(&surface_query) {
        return Some(parse_curvature_matrix_reply(
            &value,
            helper_name,
            diagnostics,
        ));
    }
    // Err(_): fall through to curve form

    // Retry as curve: full 3D world point.
    let curve_query = reify_ir::GeometryQuery::CurveCurvatureAt {
        handle,
        px: point[0],
        py: point[1],
        pz: point[2],
    };
    match kernel.query(&curve_query) {
        Ok(reify_ir::Value::Real(kappa)) => Some(reify_ir::Value::Scalar {
            si_value: kappa,
            dimension: CURVATURE_DIM,
        }),
        Ok(unexpected) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} kernel reply has unexpected type (expected Real for curve curvature, \
                 got {unexpected:?}); cell left at Undef",
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} kernel query failed: {err}",
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Decode the kernel's `[[κ_max, 0.0], [0.0, κ_min]]` nested-List reply into a
/// `Value::Matrix` of `Value::Scalar{si_value, dimension: 1/Length}` cells.
///
/// On any parse failure emits a Warning and returns `Value::Undef` (same
/// defensive-downgrade contract as `dispatch_normal_vector3`).
fn parse_curvature_matrix_reply(
    value: &reify_ir::Value,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::Value {
    let rows = match value {
        reify_ir::Value::List(rows) if rows.len() == 2 => rows,
        other => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} kernel reply parse failed: expected 2-row List, got {other:?}",
            )));
            return reify_ir::Value::Undef;
        }
    };
    let mut matrix_rows: Vec<Vec<reify_ir::Value>> = Vec::with_capacity(2);
    for (i, row) in rows.iter().enumerate() {
        let cells = match row {
            reify_ir::Value::List(cells) if cells.len() == 2 => cells,
            other => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{helper_name} kernel reply parse failed: row {i} is not a 2-element List, \
                     got {other:?}",
                )));
                return reify_ir::Value::Undef;
            }
        };
        let mut matrix_row: Vec<reify_ir::Value> = Vec::with_capacity(2);
        for (j, cell) in cells.iter().enumerate() {
            let si_value = match cell {
                reify_ir::Value::Real(v) => *v,
                other => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "{helper_name} kernel reply parse failed: cell [{i}][{j}] is not Real, \
                         got {other:?}",
                    )));
                    return reify_ir::Value::Undef;
                }
            };
            matrix_row.push(reify_ir::Value::Scalar {
                si_value,
                dimension: CURVATURE_DIM,
            });
        }
        matrix_rows.push(matrix_row);
    }
    reify_ir::Value::Matrix(matrix_rows)
}

/// Issue an `InertiaTensor` query and re-wrap the kernel's row-of-row
/// `Value::List` reply into a nested `Value::Tensor(rows_of_tensors)` where
/// each element is a `Value::Scalar { si_value, dimension: MOMENT_OF_INERTIA }`.
///
/// The kernel returns raw dimensionless `Value::Real` cell values
/// (`[[m11,m12,m13],[m21,m22,m23],[m31,m32,m33]]`) because
/// `GeometryQuery::InertiaTensor` predates the dimensioned-Scalar wrap; the
/// eval-side owns the MomentOfInertia (kg·m²) tagging so the result matches
/// the compile-time `Tensor<2,3,MomentOfInertia>` cell type from
/// `topology_selector_result_type`.
///
/// Returns `Some(Value::Undef)` (with a Warning diagnostic) on a kernel
/// error or any malformed shape (non-List reply, non-List row, non-numeric
/// element). Same defensive-downgrade contract as
/// `dispatch_point3_length_reply`.
fn dispatch_inertia_tensor(
    kernel: &mut dyn reify_ir::GeometryKernel,
    query: &reify_ir::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let malformed = |diagnostics: &mut Vec<Diagnostic>, detail: String| {
        diagnostics.push(Diagnostic::warning(format!(
            "{} kernel reply malformed: {}",
            helper_name, detail
        )));
        Some(reify_ir::Value::Undef)
    };
    let reply = match kernel.query(query) {
        Ok(v) => v,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let rows = match &reply {
        reify_ir::Value::List(rows) => rows,
        other => {
            return malformed(
                diagnostics,
                format!("expected Value::List, got {:?}", other),
            );
        }
    };
    let mut tensor_rows = Vec::with_capacity(rows.len());
    for row in rows {
        let cols = match row {
            reify_ir::Value::List(cols) => cols,
            other => {
                return malformed(
                    diagnostics,
                    format!("expected Value::List row, got {:?}", other),
                );
            }
        };
        let mut tensor_cols = Vec::with_capacity(cols.len());
        for col in cols {
            // The kernel emits dimensionless Value::Real; accept a
            // dimensionless Scalar too so the dispatch stays kernel-
            // implementation agnostic (mirrors kernel_distance's
            // Real|Scalar leniency).
            let si = match col {
                reify_ir::Value::Real(v) => *v,
                reify_ir::Value::Scalar {
                    si_value,
                    dimension,
                } if *dimension == reify_core::DimensionVector::DIMENSIONLESS
                    || *dimension == reify_core::DimensionVector::MOMENT_OF_INERTIA =>
                {
                    *si_value
                }
                other => {
                    return malformed(
                        diagnostics,
                        format!("expected numeric tensor element, got {:?}", other),
                    );
                }
            };
            tensor_cols.push(reify_ir::Value::Scalar {
                si_value: si,
                dimension: reify_core::DimensionVector::MOMENT_OF_INERTIA,
            });
        }
        tensor_rows.push(reify_ir::Value::Tensor(tensor_cols));
    }
    Some(reify_ir::Value::Tensor(tensor_rows))
}

/// Issue a `PointOnShape` query and unwrap to a `Value::Bool(_)`. Returns
/// `Some(Value::Undef)` (with a Warning diagnostic) on a kernel error or a
/// non-Bool reply.
fn dispatch_point_on_shape(
    kernel: &mut dyn reify_ir::GeometryKernel,
    query: &reify_ir::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(query) {
        Ok(reify_ir::Value::Bool(b)) => Some(reify_ir::Value::Bool(b)),
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel returned non-Bool value {:?}; treating as undefined",
                helper_name, other
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Issue a `SurfaceAngle` query and unwrap to a `Value::angle(rad)`. Returns
/// `Some(Value::Undef)` (with a Warning diagnostic) on a kernel error or a
/// non-numeric reply.
fn dispatch_surface_angle(
    kernel: &mut dyn reify_ir::GeometryKernel,
    query: &reify_ir::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(query) {
        Ok(reify_ir::Value::Real(rad)) => Some(reify_ir::Value::angle(rad)),
        // Some mock kernels store the angle as an angle-dimensioned Scalar
        // — accept either form so the dispatch is kernel-implementation
        // agnostic (mirrors `kernel_distance`'s Real|Scalar leniency).
        // Bind `dimension` (not `..`) so a wrong-dimensioned Scalar (e.g.
        // LENGTH) is caught rather than silently reinterpreted as radians.
        // Mirrors `resolve_point3_length_arg`'s tightened LENGTH check
        // introduced in commit 8c464177db (task 2324): debug_assert FIRST,
        // then if-fall-through in release.
        //
        // DIMENSIONLESS is accepted alongside ANGLE as a deliberate
        // compatibility trade-off: some mock kernels return raw f64 values
        // without attaching a dimension tag (see
        // `MockGeometryKernel::with_surface_angle_result`). A production kernel
        // returning DIMENSIONLESS for an angle would itself violate the type
        // contract — this leniency is intentional test-support compatibility,
        // not because DIMENSIONLESS is a valid angle dimension in real kernels.
        Ok(reify_ir::Value::Scalar {
            si_value,
            dimension,
        }) => {
            debug_assert!(
                dimension == reify_core::DimensionVector::ANGLE
                    || dimension == reify_core::DimensionVector::DIMENSIONLESS,
                "dispatch_surface_angle: expected ANGLE- or DIMENSIONLESS-dimensioned Scalar, \
                 got dimension {:?} (si_value={}); kernel cell type is Type::angle() per \
                 compile-time wiring",
                dimension,
                si_value
            );
            if dimension != reify_core::DimensionVector::ANGLE
                && dimension != reify_core::DimensionVector::DIMENSIONLESS
            {
                diagnostics.push(Diagnostic::warning(format!(
                    "{} kernel returned wrong-dimensioned Scalar \
                     (dimension={}, si_value={}); treating as undefined",
                    helper_name, dimension, si_value
                )));
                return Some(reify_ir::Value::Undef);
            }
            Some(reify_ir::Value::angle(si_value))
        }
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel returned non-numeric value {:?}; treating as undefined",
                helper_name, other
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Ad-hoc selector eval dispatch (task 3463)
//
// Layer 2 of the two-layer @face/@edge evaluation split.  Layer 1 (pure-expr
// @point) lives in `reify-expr/src/lib.rs`.  Here we wire the kernel-aware
// @face/@edge path, mirroring `try_eval_topology_selector` in structure.
//
// Why `pub` (not `pub(crate)`): integration tests in `tests/` are separate
// crates and cannot see `pub(crate)`.  The function is also re-exported from
// `lib.rs` for the same reason, following the `resolve_unique_by_attribute`
// precedent.
// ─────────────────────────────────────────────────────────────────────────────

/// Sub-shape kind that the kernel-aware `@face` / `@edge` dispatch path
/// is willing to accept — a strict subset of `reify_types::SelectorKind`
/// with `Point` excluded by construction.
///
/// Layer 1 (`eval_expr`) resolves `@point` selectors directly from
/// literal coordinates and never reaches this module; Layer 2's
/// `try_eval_ad_hoc_selector` converts the incoming `SelectorKind` via
/// `FrameSubShapeKind::from_selector_kind` and `?`-propagates `None` for
/// `SelectorKind::Point`, so every downstream `match` in this module
/// only needs to handle `Face` and `Edge`. Replaces three previous
/// `unreachable!("Point arm ...")` arms with compile-time exhaustiveness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameSubShapeKind {
    Face,
    Edge,
}

impl FrameSubShapeKind {
    /// Narrow a `SelectorKind` to a kernel-aware sub-shape kind.
    /// Returns `None` for `SelectorKind::Point` so the caller can `?`-
    /// propagate the early-return-None invariant established by Layer-1.
    fn from_selector_kind(k: &reify_ir::SelectorKind) -> Option<Self> {
        match k {
            reify_ir::SelectorKind::Face => Some(FrameSubShapeKind::Face),
            reify_ir::SelectorKind::Edge => Some(FrameSubShapeKind::Edge),
            reify_ir::SelectorKind::Point => None,
        }
    }
}

/// Dispatch a `CompiledExprKind::AdHocSelector` expression through the engine
/// attribute table and geometry kernel, returning the resolved `Value::Frame`
/// (or `Some(Value::Undef)` on a diagnostic failure, or `None` if the
/// expression is not an `AdHocSelector` or the arg shapes are unsupported).
///
/// Called by `Engine::post_process_ad_hoc_selectors` after `eval_expr` has set
/// Face/Edge cells to `Value::Undef`.  Layer-1 (`@point`) was already resolved
/// by `eval_expr` directly, so `Point` arms here return `None` immediately.
///
/// # Arg-shape contract
/// - `expr.kind` must be `AdHocSelector` — anything else yields `None`.
/// - `base` must be a `Literal(Value::String(name))` — other shapes yield `None`
///   (cell stays at Undef).
/// - `args[0]` must be a `Literal(Value::String(label))` — same fall-through.
/// - `name` must exist in `named_steps` — miss yields `None`.
///
/// # Returns
/// - `Some(Value::Frame { origin, basis })` on success.
/// - `Some(Value::Undef)` on any diagnostic failure (resolver emits its own
///   `TopologyAttributeStale` Warning; kernel errors get a new Warning here).
/// - `None` for non-AdHocSelector, Point-kind, or unsupported arg shapes.
pub fn try_eval_ad_hoc_selector(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    kernel: &mut dyn reify_ir::GeometryKernel,
    table: &reify_ir::TopologyAttributeTable,
    selector_span: reify_core::SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // (1) Must be an AdHocSelector — anything else is not applicable.
    let (base, selector_kind, args) = match &expr.kind {
        reify_ir::CompiledExprKind::AdHocSelector {
            base,
            selector_kind,
            args,
        } => (base.as_ref(), selector_kind, args),
        _ => return None,
    };

    // (2) Convert to the narrowed FrameSubShapeKind — Point maps to None and
    //     `?` early-returns here, keeping Point out of all downstream matches.
    //     Layer-1 (eval_expr) already resolved @point from literal coordinates.
    let frame_sub_shape_kind = FrameSubShapeKind::from_selector_kind(selector_kind)?;

    // (3+4) Extract the base name and the face/edge label via evaluate-then-accept
    //       (task ε). `try_eval_ad_hoc_selector` carries no `ValueMap` — its callers
    //       (`post_process_ad_hoc_selectors` in engine_build.rs + the ad_hoc_selector
    //       smoke tests) are outside task ε's module scope — so base/label evaluate
    //       against a LOCAL empty context. Ad-hoc base/label compile to string
    //       literals (reify_compiler expr.rs AdHocSelector), which evaluate
    //       identically against any context; a stray ValueRef degrades to quiet
    //       Undef exactly as before (no regression), while a defined-but-wrong value
    //       now emits a Warning. See resolve_string_literal_arg's doc-comment.
    let ad_hoc_values = reify_ir::ValueMap::new();
    let builtin = match &frame_sub_shape_kind {
        FrameSubShapeKind::Face => "@face",
        FrameSubShapeKind::Edge => "@edge",
    };
    let name = resolve_string_literal_arg(base, &ad_hoc_values, builtin, "base", diagnostics)?;

    let label = match args.first() {
        Some(a) => resolve_string_literal_arg(a, &ad_hoc_values, builtin, "label", diagnostics)?,
        None => return None,
    };

    // (5) Look up the base name in named_steps → GeometryHandleId.
    let handle = match named_steps.get(name.as_str()) {
        Some(kh) => kh.id,
        None => return None,
    };

    // (6) Extract sub-shape handles from the kernel.
    //     Exhaustive over Face/Edge — no Point arm needed (filtered above).
    let candidates: Vec<GeometryHandleId> = match frame_sub_shape_kind {
        FrameSubShapeKind::Face => match kernel.extract_faces(handle) {
            Ok(faces) => faces,
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "@face(\"{label}\"): extract_faces({handle:?}) failed: {err}"
                )));
                return Some(reify_ir::Value::Undef);
            }
        },
        FrameSubShapeKind::Edge => match kernel.extract_edges(handle) {
            Ok(edges) => edges,
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "@edge(\"{label}\"): extract_edges({handle:?}) failed: {err}"
                )));
                return Some(reify_ir::Value::Undef);
            }
        },
    };

    // (7) Build AttributeQuery with dual user_label + canonical-name role translation.
    let query = crate::topology_attribute_resolver::AttributeQuery {
        user_label: Some(label.clone()),
        role_and_index: cap_kind_translation(&label),
        feature_id: None,
    };

    // (8) Resolve via the attribute table.
    let resolution = crate::topology_attribute_resolver::resolve_unique_by_attribute(
        table,
        &candidates,
        &query,
        selector_span,
        diagnostics,
    );

    // (9) On Resolved: query kernel for Frame; on any other outcome: Some(Undef).
    //     The resolver already pushed its TopologyAttributeStale / AmbiguousAfterSplit
    //     Warning, so we only need to patch the cell value here.
    match resolution {
        crate::topology_attribute_resolver::AttributeResolution::Resolved(target_id) => {
            construct_frame_from_kernel(target_id, frame_sub_shape_kind, kernel, diagnostics)
        }
        _ => Some(reify_ir::Value::Undef),
    }
}

/// Resolve a selector name/label string arg (the `face`/`edge`/`solid_body`
/// builtin name and the ad-hoc `@face`/`@edge` base/label) to an OWNED
/// `String`, emitting a `Severity::Warning` when the caller passes a
/// defined-but-wrong value.
///
/// Evaluate-then-accept (task ε): the arg expr is EVALUATED against `values`
/// (via [`eval_arg_value`]) and the resulting `Value` classified. A `ValueRef →
/// Value::String` cell now resolves (the named-leaf `face(body, label_var)`
/// form), while an inline `Literal(Value::String)` evaluates to itself —
/// functionally identical to the prior `Literal`-match. The return type changed
/// from `Option<&str>` to `Option<String>` because the evaluated `Value` is
/// owned by the local eval, not borrowed from `expr`.
///
/// | evaluated arg value                              | return    | diagnostic?     |
/// |--------------------------------------------------|-----------|-----------------|
/// | `Value::Undef` (missing/Undef cell, user-fn arg) | `None`    | no — quiet      |
/// | `Value::String(s)`                               | `Some(s)` | no              |
/// | any other defined value (Int, Real, …)           | `None`    | yes — 1 Warning |
///
/// NOTE (ad-hoc context): `try_eval_ad_hoc_selector` carries no `ValueMap` in
/// its signature — it is a public API whose callers (`post_process_ad_hoc_selectors`
/// in `engine_build.rs` and the `ad_hoc_selector_smoke_tests`) are outside task
/// ε's module scope — so it evaluates base/label against a LOCAL empty
/// `ValueMap`. Ad-hoc base/label compile to string literals in practice (see
/// `reify_compiler` `expr.rs` `AdHocSelector`), which evaluate identically
/// against any context; a stray `ValueRef` there degrades to quiet `Undef`
/// exactly as before (no regression). The named-leaf caller
/// (`eval_named_leaf_selector_ctor`) threads the real `values`, so
/// `face(body, label_var)` resolves a `ValueRef → String` cell.
fn resolve_string_literal_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    use crate::arg_acceptance::ArgRejection;

    let value = eval_arg_value(expr, values);

    match value {
        // Quiet degradation: an Undef value (missing cell, or a user-fn/meta arg
        // the local ctx can't evaluate) returns None with no diagnostic —
        // behaviourally identical to the prior non-`Literal(String)` fall-through.
        reify_ir::Value::Undef => None,
        reify_ir::Value::String(s) => Some(s),
        // Defined-but-wrong (non-String): emit exactly one Warning naming
        // builtin/arg/String/got (byte-uniform wording with the density / point /
        // vec3 / range / int paths).
        other => {
            diagnostics.push(Diagnostic::warning(
                ArgRejection {
                    got: string_got_label(&other),
                    expected: "String",
                    migration_hint: None,
                }
                .message(builtin, arg_name),
            ));
            None
        }
    }
}

/// Short human-readable label for a `Value` that failed String classification,
/// used as the `got` field of the rejection diagnostic (task ε).
fn string_got_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Int(_) => "Int".to_string(),
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => scalar_got_label(dimension),
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::Vector(_) => "Vector".to_string(),
        reify_ir::Value::Point(_) => "Point".to_string(),
        _ => "non-String value".to_string(),
    }
}

/// Translate a canonical face/edge label into a `(Role, local_index)` pair for
/// the `AttributeQuery::role_and_index` field.
///
/// The translation covers canonical labels wired by the Cylinder seeder
/// (`seed_primitive_attributes`), making `@face("top")` / `@face("bottom")`
/// / `@face("start")` / `@face("end")` / `@face("side")` work against
/// Cylinder and Extrude / Revolve primitives without requiring a
/// `name = "..."` source annotation on the face (which is deferred per the
/// PRD).
///
/// **`"side"` note:** `Role::Side` with `local_index = 0` is the entry
/// seeded for the single lateral face of a Cylinder. For primitives with
/// multiple side faces (future Boolean / Loft results) this match selects
/// index 0 only; if the resolver finds multiple `Role::Side` entries it will
/// return `AmbiguousAfterSplit`, surfacing a `TopologyAttributeStale` Warning
/// and leaving the cell at `Value::Undef` — the same graceful-degradation
/// path as all other Unresolved outcomes.
///
/// Any unrecognised label returns `None` — the query then relies entirely on
/// `user_label` and will Unresolve if no `user_label` entry exists in the table.
// G-allow: task #3463 cap/role vocabulary table; consumer is try_eval_ad_hoc_selector @face/@edge dispatch (same-file, task #3463) + ad_hoc_selector smoke tests
pub(crate) fn cap_kind_translation(label: &str) -> Option<(reify_ir::Role, u32)> {
    use reify_ir::{CapKind, Role};
    match label {
        "top" => Some((Role::Cap(CapKind::Top), 0)),
        "bottom" => Some((Role::Cap(CapKind::Bottom), 0)),
        "start" => Some((Role::Cap(CapKind::Start), 0)),
        "end" => Some((Role::Cap(CapKind::End), 0)),
        "side" => Some((Role::Side, 0)),
        _ => None,
    }
}

/// Query the kernel for centroid and normal/tangent of `target_id`, then
/// construct a `Value::Frame { origin, basis }`.
///
/// The `sub_shape_kind` parameter selects the kernel query:
/// - `FrameSubShapeKind::Face` → `GeometryQuery::FaceNormal` (face normal maps
///   to the frame's **+Z** axis — standard CAD convention for planar features).
/// - `FrameSubShapeKind::Edge` → `GeometryQuery::EdgeTangent` (edge tangent
///   maps to the frame's **+Z** axis; downstream consumers that expect the
///   tangent along **+X** should apply a 90° R_Y pre-rotation).
///
/// `FrameSubShapeKind` excludes `Point` by construction, so this function
/// never needs to handle the Point case — the type system enforces the
/// invariant that was previously guarded by `unreachable!()` arms.
///
/// On centroid failure: push a Warning and return `Some(Value::Undef)`.
/// On normal/tangent failure: push a Warning and use identity basis, so the
/// Frame still has a meaningful origin.
fn construct_frame_from_kernel(
    target_id: GeometryHandleId,
    sub_shape_kind: FrameSubShapeKind,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // ── Origin via Centroid ───────────────────────────────────────────────
    // `GeometryQuery::Centroid` is unified — works for faces, edges, AND solids.
    let origin = match kernel.query(&reify_ir::GeometryQuery::Centroid(target_id)) {
        Ok(value) => match crate::topology_selectors::parse_xyz_value(&value, "Centroid") {
            Ok([x, y, z]) => reify_ir::Value::Point(vec![
                reify_ir::Value::length(x),
                reify_ir::Value::length(y),
                reify_ir::Value::length(z),
            ]),
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "@face/@edge centroid parse failed: {err}; cell left as Undef"
                )));
                return Some(reify_ir::Value::Undef);
            }
        },
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "@face/@edge centroid query failed: {err}; cell left as Undef"
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    // ── Basis via FaceNormal (face) or EdgeTangent (edge) ─────────────────
    // Exhaustive over Face/Edge — Point is excluded by the FrameSubShapeKind
    // type, so no unreachable!() arm is needed here.
    let basis_query = match sub_shape_kind {
        FrameSubShapeKind::Face => reify_ir::GeometryQuery::FaceNormal(target_id),
        FrameSubShapeKind::Edge => reify_ir::GeometryQuery::EdgeTangent(target_id),
    };
    let query_label = match sub_shape_kind {
        FrameSubShapeKind::Face => "FaceNormal",
        FrameSubShapeKind::Edge => "EdgeTangent",
    };

    let basis = match kernel.query(&basis_query) {
        Ok(value) => {
            match crate::topology_selectors::parse_xyz_value(&value, query_label) {
                Ok([nx, ny, nz]) => quaternion_from_z_to_axis(nx, ny, nz),
                Err(err) => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "@face/@edge {query_label} parse failed: {err}; using identity basis"
                    )));
                    // Degrade gracefully: return a Frame with correct origin, identity basis.
                    reify_ir::Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }
                }
            }
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "@face/@edge {query_label} query failed: {err}; using identity basis"
            )));
            // Degrade gracefully: origin was obtained, identity basis.
            reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        }
    };

    Some(reify_ir::Value::Frame {
        origin: Box::new(origin),
        basis: Box::new(basis),
    })
}

/// Compute the shortest-arc unit quaternion that rotates the +Z axis `(0, 0, 1)`
/// to the given (approximately unit) axis vector `(nx, ny, nz)`.
///
/// Formula: for unit vectors `a` and `b`,
///   `q_unnorm = (1 + dot(a,b),  cross(a,b))`
/// where `a = +Z = (0,0,1)`, so:
///   `dot(+Z, b) = nz`
///   `cross(+Z, b) = (-ny, nx, 0)`
///   `q_unnorm = (1 + nz, -ny, nx, 0)`
///
/// Special case: `b ≈ -Z` makes `q_unnorm ≈ (0,0,0,0)` — degenerate.
/// Fall back to a 180° rotation around the +X axis.
///
/// **Numerical note:** for an approximately unit input, `len_sq = (1 + nz)² + nx² + ny²`.
/// Since `nx² + ny² = 1 − nz² = (1 − nz)(1 + nz)`, this simplifies to
///   `len_sq = 2·(1 + nz)`.
/// So `len_sq < 1e-12` fires for `nz < −1 + 5e-13` — roughly half a femto-unit from
/// `−Z`. The margin is intentional: it is well above the rounding noise accumulated
/// by the multiply-and-add chain that produces `len_sq`, yet small enough that the
/// fallback only activates for genuinely degenerate inputs. **Do not tighten the
/// threshold further**: reducing it below ~`1e-13` would shrink the safety margin
/// into f64 rounding noise and allow near-degenerate inputs to produce NaN-carrying
/// quaternions.
fn quaternion_from_z_to_axis(nx: f64, ny: f64, nz: f64) -> reify_ir::Value {
    let w_unnorm = 1.0 + nz;
    // Use `0.0 - ny` instead of `-ny` to avoid producing -0.0 when ny = 0.0.
    // In IEEE 754, `0.0 - 0.0 = +0.0` (round-to-nearest), whereas the unary
    // negation `-0.0 = -0.0`.  The bit-exact `PartialEq` on `Value::Orientation`
    // would treat -0.0 and +0.0 as unequal, causing spurious test failures when
    // the input has a zero component.
    let x_unnorm = 0.0 - ny;
    let y_unnorm = nx;
    // z component of cross(+Z, b) is always 0.

    let len_sq = w_unnorm * w_unnorm + x_unnorm * x_unnorm + y_unnorm * y_unnorm;

    if len_sq < 1e-12 {
        // (nx, ny, nz) ≈ -Z: degenerate case. Rotate 180° around +X.
        return reify_ir::Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
    }

    let len = len_sq.sqrt();
    reify_ir::Value::Orientation {
        w: w_unnorm / len,
        x: x_unnorm / len,
        y: y_unnorm / len,
        z: 0.0,
    }
}

/// Evaluate an `at <pose>` sub-component placement expression into a rigid
/// child→parent [`reify_ir::Value::Transform`].
///
/// # Convention (resolves PRD §11 Q1)
///
/// A `Transform { rotation: Q, translation: t }` maps a child-local point `p`
/// to parent-space via `Q·p + t`.  Carrying the child's identity origin-frame
/// onto target `Frame { origin: o, basis: R }` (target in parent coords) forces:
///
/// - child-origin 0 → o  ⇒  t = o (origin components copied as-is, dimension preserved)
/// - child-axes   I → R  ⇒  Q = R (basis copied; no normalization — frame3 guarantees unit basis)
///
/// Hence `Frame { origin: o, basis: R }` → `Transform { rotation: R, translation: o_as_vector }`.
///
/// | `pose` result                       | outcome                                               |
/// |-------------------------------------|-------------------------------------------------------|
/// | `None`                              | identity (Orientation(1,0,0,0), Vector[len 0,0,0])    |
/// | `Some(_)` → `Value::Transform`      | pass through unchanged                                |
/// | `Some(_)` → `Value::Frame`          | lowered per the convention above                      |
/// | anything else (incl. `Value::Undef`)| one `Diagnostic::error`; returns `Value::Undef`       |
#[allow(dead_code)] // used in #[cfg(test)]; consumed by T5 (full-tree composition)
pub(crate) fn eval_sub_pose(
    pose: Option<&reify_ir::CompiledExpr>,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::Value {
    let Some(expr) = pose else {
        return reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
            ])),
        };
    };

    let value = reify_expr::eval_expr(expr, &eval_ctx_with_meta(values, functions, meta_map));
    match value {
        reify_ir::Value::Transform { .. } => value,
        reify_ir::Value::Frame { origin, basis } => {
            let components = match *origin {
                reify_ir::Value::Point(c) => c,
                other => {
                    diagnostics.push(Diagnostic::error(format!(
                        "`at` pose Frame origin must be a Point; got {:?}",
                        other
                    )));
                    return reify_ir::Value::Undef;
                }
            };
            if components.len() != 3
                || !components.iter().all(|c| {
                    if let reify_ir::Value::Scalar {
                        si_value,
                        dimension,
                    } = c
                    {
                        *dimension == reify_core::DimensionVector::LENGTH && si_value.is_finite()
                    } else {
                        false
                    }
                })
            {
                diagnostics.push(Diagnostic::error(
                    "`at` pose Frame origin must be a 3-component LENGTH-dimensioned Point with finite coordinates",
                ));
                return reify_ir::Value::Undef;
            }
            if !matches!(*basis, reify_ir::Value::Orientation { .. }) {
                diagnostics.push(Diagnostic::error(
                    // The check is structural: Value::Orientation variant only.
                    // frame3 guarantees unit basis; non-unit quaternions arriving
                    // via other construction paths are caught by downstream
                    // Transform composition rather than here (keeps lowering exact).
                    "`at` pose Frame basis must be a Value::Orientation",
                ));
                return reify_ir::Value::Undef;
            }
            reify_ir::Value::Transform {
                rotation: basis,
                translation: Box::new(reify_ir::Value::Vector(components)),
            }
        }
        _ => {
            // This arm includes Value::Undef (expression evaluation failed upstream).
            // We emit one error here intentionally: it gives the caller a call-site
            // anchor showing *where* the failed pose affected sub-component placement,
            // complementing whatever diagnostic the upstream expression already emitted.
            // This behavior is pinned by `eval_sub_pose_undef_expr_returns_undef_with_diagnostic`.
            diagnostics.push(Diagnostic::error(
                "`at` pose expression must evaluate to a Transform or Frame",
            ));
            reify_ir::Value::Undef
        }
    }
}

/// Resolve whether the `let` binding backing a realization was declared `aux`
/// (PRD §2.2), i.e. "no external geometric effect" → surfaced hidden.
///
/// The compiler threads `LetDecl.is_aux` directly onto `RealizationDecl.is_aux`
/// at lowering (geometry lets are lowered as realizations only — they create no
/// `ValueCellDecl`, so the flag cannot be recovered via `value_cells`). Geometry
/// params and the guarded-group path carry no source `aux` modifier and so are
/// always `false`.
///
/// Used by `tessellate_from_values` to derive `MeshSurface.default_visible`
/// on the flat (no-composition) path; the Phase-B containment walk additionally
/// ORs in any `aux` ancestor sub (T5 steps 4/6).
///
/// This is intentionally a thin wrapper over the public `RealizationDecl.is_aux`
/// field: its value is *documentary*, not behavioral. It gives the surfacing
/// call site (`surface_subtree`) a self-describing name for the visibility
/// intent and a single anchor for the non-obvious compiler-threading rationale
/// above (the field exists only because escalation esc-3903-220 added it — the
/// `aux` modifier on a geometry `let` would otherwise be dropped at lowering).
/// Keeping it avoids re-deriving that context at the use site; inlining would
/// either lose the doc or scatter it into a call-site comment.
pub(crate) fn realization_is_aux(realization: &reify_compiler::RealizationDecl) -> bool {
    realization.is_aux
}

/// Decompose a `Value::Transform` into raw quaternion + SI-metre translation
/// arrays for building a kernel-agnostic `reify_ir::GeometryOp::ApplyTransform`
/// (T5 step-8/10).
///
/// Accepts `Transform { rotation: Orientation { w, x, y, z }, translation:
/// Vector([s0, s1, s2]) }` where each translation component is a finite LENGTH
/// or dimensionless `Scalar`; returns `Some(([w,x,y,z], [tx,ty,tz]))` with the
/// translation read straight off `Scalar.si_value` (SI metres). Returns `None`
/// for any other shape — non-`Transform`, non-`Orientation` rotation, a
/// translation that is not a 3-component `Vector`, or a component that is not a
/// LENGTH/dimensionless finite `Scalar`. Each component is checked independently,
/// so a mixed-dimension translation (e.g. one ANGLE among LENGTHs) is rejected.
///
/// `reify_stdlib`'s own `decompose_transform` is private, so this local
/// pattern-match keeps the change inside reify-eval while feeding the IR op's
/// raw float arrays (the IR is kernel-agnostic by design).
pub(crate) fn decompose_transform_to_arrays(v: &reify_ir::Value) -> Option<([f64; 4], [f64; 3])> {
    let reify_ir::Value::Transform {
        rotation,
        translation,
    } = v
    else {
        return None;
    };
    let reify_ir::Value::Orientation { w, x, y, z } = rotation.as_ref() else {
        return None;
    };
    let reify_ir::Value::Vector(components) = translation.as_ref() else {
        return None;
    };
    if components.len() != 3 {
        return None;
    }
    let mut t = [0.0_f64; 3];
    for (i, c) in components.iter().enumerate() {
        let reify_ir::Value::Scalar {
            si_value,
            dimension,
        } = c
        else {
            return None;
        };
        let dim_ok = *dimension == reify_core::DimensionVector::LENGTH
            || *dimension == reify_core::DimensionVector::DIMENSIONLESS;
        if !dim_ok || !si_value.is_finite() {
            return None;
        }
        t[i] = *si_value;
    }
    Some(([*w, *x, *y, *z], t))
}

/// Left-fold a chain of pose `Value::Transform`s into a single world transform
/// via the quaternion-correct `transform_compose` builtin (T5 step-8/10).
///
/// Seeds with the identity Transform and folds `transform_compose(acc, next)`
/// left-to-right, so the result is `pose_0 ∘ pose_1 ∘ … ∘ pose_n` (mirrors the
/// proven left-fold in `reify_stdlib::loop_closure::chain_transform`). An empty
/// chain returns the identity Transform unchanged. Reuses the already-tested
/// stdlib builtin rather than hand-rolling quaternion math; `reify-eval` already
/// depends on `reify-stdlib`.
pub(crate) fn compose_pose_chain(poses: &[reify_ir::Value]) -> reify_ir::Value {
    let identity = reify_ir::Value::Transform {
        rotation: Box::new(reify_ir::Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(reify_ir::Value::Vector(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ])),
    };
    poses.iter().fold(identity, |acc, next| {
        reify_stdlib::eval_builtin("transform_compose", &[acc, next.clone()])
    })
}

/// Indices into `module.templates` of the *root* templates for surfacing: those
/// whose `name` is NOT the `structure_name` of any NON-collection sub anywhere
/// in the module (T5 step-4).
///
/// `CompiledModule.templates` is a flat `Vec` with no root marker, and the
/// pre-T5 evaluator surfaced *every* template standalone — so a contained child
/// appeared at un-placed local coords. The containment walk surfaces each
/// non-root descendant exactly once at its composed world pose, so only roots
/// seed the walk; contained templates are suppressed standalone.
///
/// Collection subs are deliberately excluded from the "contained" set: their
/// per-element placement is out of scope (PRD §10), so a template used *only* as
/// a `List<T>` sub still surfaces standalone as a root (unchanged behavior).
pub(crate) fn root_template_indices(module: &reify_compiler::CompiledModule) -> Vec<usize> {
    let contained: std::collections::HashSet<&str> = module
        .templates
        .iter()
        .flat_map(|t| t.sub_components.iter())
        .filter(|sub| !sub.is_collection)
        .map(|sub| sub.structure_name.as_str())
        .collect();
    module
        .templates
        .iter()
        .enumerate()
        .filter(|(_, t)| !contained.contains(t.name.as_str()))
        .map(|(idx, _)| idx)
        .collect()
}

/// Indices of every template reachable from `seeds` by following NON-collection
/// subs, inclusive of the seeds themselves (T5 amendment — cycle-loss guard).
///
/// The Phase-B driver surfaces from each root, but a template that is excluded
/// from the root set (because some sub names it) yet is reachable from NO root
/// can only sit inside a non-collection containment cycle with no acyclic entry
/// point — a self-recursive `sub child : Self`, or a mutual `A -> B -> A`. Pre-T5
/// every template surfaced standalone, so dropping such a template is a silent
/// geometry-loss regression. The driver computes `reachable_template_indices(…,
/// roots)` and surfaces any *unreached* template as a fallback root, so its
/// geometry is preserved (the per-template `surface_subtree` walk stays bounded
/// by the `depth > templates.len()` cycle guard).
///
/// `structure_name -> template index` is resolved by `position` (mirroring
/// `surface_subtree` / `root_template_indices`); collection subs are skipped to
/// match the root-set's containment definition. In an *acyclic* module every
/// non-root is reachable from some root, so this returns the full index set and
/// the fallback loop is a no-op — zero behavior change off the cyclic path.
pub(crate) fn reachable_template_indices(
    module: &reify_compiler::CompiledModule,
    seeds: &[usize],
) -> std::collections::HashSet<usize> {
    let mut reached: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut stack: Vec<usize> = seeds.to_vec();
    while let Some(idx) = stack.pop() {
        if !reached.insert(idx) {
            continue;
        }
        for sub in &module.templates[idx].sub_components {
            if sub.is_collection {
                continue;
            }
            // Push every non-collection child; the `reached.insert` guard above
            // dedups on pop, so re-pushing an already-reached index is harmless.
            if let Some(child) = module
                .templates
                .iter()
                .position(|t| t.name == sub.structure_name)
            {
                stack.push(child);
            }
        }
    }
    reached
}

/// Pre-filter callback type for the placed-realization walk.
///
/// Called before `ApplyTransform` with `(t_idx, r_idx, entity_path)`.
/// Returns `false` to skip both the transform and visitor call for that realization.
/// Use `None` for the default "include all" behavior.
pub(crate) type PlacedPreFilter<'a> = Option<&'a dyn Fn(usize, usize, &str) -> bool>;

/// Placed-product body collected by the T7 export walk (`surface_export_bodies`).
///
/// The `entity_path` is the composed PRD §11.2 path; `handle_id` is the
/// `GeometryHandleId` of the placed BRep (world transform already baked in via
/// `ApplyTransform`); `default_visible` follows the same OR-of-aux rule as
/// `surface_subtree` (`false` iff aux or under-aux-sub).
#[derive(Debug)]
pub(crate) struct ExportBody {
    pub entity_path: String,
    pub handle_id: GeometryHandleId,
    pub default_visible: bool,
}

/// Shared inner walk for T5 (tessellate) and T7 (export) containment-tree surfacing.
///
/// Implements the common cycle guard, placement decomposition, `ApplyTransform`
/// application, `default_visible` derivation, and `entity_path` formatting shared
/// by `surface_subtree` and `surface_export_bodies`.  The terminal action per
/// realization is delegated to `visit_realization`.
///
/// # Visitor
///
/// `visit_realization(kernel, placed_id, entity_path, default_visible, t_idx, r_idx, diagnostics)`
/// — called once per realization that produced a handle, after the composed world
/// transform is applied:
/// - `kernel`: mutable borrow of the default kernel (over the visit call; released before
///   recursion so the sub loop can re-borrow).
/// - `placed_id`: the `GeometryHandleId` after `ApplyTransform` (or the source handle for
///   identity/undecomposable poses — no extra kernel op).
/// - `entity_path`: PRD §11.2 composed path (owned `String`).
/// - `default_visible`: `false` iff aux or under-aux-sub (same OR rule as the callers).
/// - `t_idx`, `r_idx`: template and realization indices (for budget lookup in the
///   tessellate visitor).
/// - `diagnostics`: the walk's diagnostic accumulator; the visitor may push to it.
///
/// Callers (`surface_subtree`, `surface_export_bodies`) preserve their public signatures
/// unchanged — they are thin wrappers that capture their output collection in the closure.
#[allow(clippy::too_many_arguments)]
pub(crate) fn walk_placed_realizations<V>(
    module: &reify_compiler::CompiledModule,
    t_idx: usize,
    path_prefix: &str,
    aux_ancestor: bool,
    composed_world: &reify_ir::Value,
    depth: usize,
    terminal_handles: &[Vec<Option<KernelHandle>>],
    // task-4147: per-instance handle-row override for constructor-arg subs.
    //
    // When `Some(row)`, the realization loop reads handles from `row[r_idx]`
    // instead of `terminal_handles[t_idx][r_idx]`.  `row` is aligned with
    // `terminal_handles[t_idx]` (same length, same r_idx semantics), produced
    // by `crate::engine_build::realize_sub_override_handles` for each sub
    // with `!sub.args.is_empty()`.
    //
    // Pass `None` at roots (roots are never overridden subs) and for arg-free
    // subs (which reuse the Phase-A shared handle, no re-realization needed).
    handle_row_override: Option<&[Option<KernelHandle>]>,
    geometry_kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
    default_kernel_name: &str,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
    // Optional short-circuit filter (T7 amendment, suggestion 3).
    //
    // Called BEFORE `ApplyTransform` with `(t_idx, r_idx, entity_path)`.
    // If the filter returns `false`, BOTH the transform and the visitor call are
    // skipped for that realization — no new kernel handle is minted.
    //
    // Pass `None` for the default "include all" behavior (zero overhead on the
    // tessellation hot path).  Pass `Some(f)` in `distance_between_placed` to
    // skip transforms for realizations not matching the two target paths, avoiding
    // accumulation of transient placed handles in the kernel store on repeated calls.
    pre_filter: PlacedPreFilter<'_>,
    visit_realization: &mut V,
) where
    V: FnMut(
        &mut dyn GeometryKernel,
        GeometryHandleId,
        String,
        bool,
        usize,
        usize,
        &mut Vec<Diagnostic>,
    ),
{
    if depth > module.templates.len() {
        return;
    }
    let template = &module.templates[t_idx];

    // Decompose the inherited world transform once for this template.
    // `Some(non-identity)` → apply via ApplyTransform before visiting;
    // identity / non-decomposable short-circuits to the source handle (no kernel op).
    let placement: Option<([f64; 4], [f64; 3])> =
        match decompose_transform_to_arrays(composed_world) {
            Some((rotation, translation))
                if rotation != [1.0, 0.0, 0.0, 0.0] || translation != [0.0, 0.0, 0.0] =>
            {
                Some((rotation, translation))
            }
            _ => None,
        };

    // task-4147: use per-instance override row when present; fall back to the
    // Phase-A shared row for arg-free subs (or at roots).
    let handles_row: &[Option<KernelHandle>] =
        handle_row_override.unwrap_or(&terminal_handles[t_idx]);

    for (r_idx, realization) in template.realizations.iter().enumerate() {
        let Some(handle) = handles_row[r_idx] else {
            continue;
        };
        // Compute entity_path BEFORE ApplyTransform so `pre_filter` can gate the
        // transform on the path (avoids minting transient handles for unwanted bodies).
        let entity_path = format!("{}#realization[{}]", path_prefix, realization.id.index);
        // Short-circuit: if the caller supplied a pre-filter and this realization is
        // not wanted, skip both the transform and the visitor — no kernel op issued.
        if pre_filter.is_some_and(|f| !f(t_idx, r_idx, &entity_path)) {
            continue;
        }
        let default_kernel = geometry_kernels
            .get_mut(default_kernel_name)
            .expect("default kernel must remain in the map across the surfacing walk");
        let placed_id = match placement {
            Some((rotation, translation)) => {
                match default_kernel.execute(&reify_ir::GeometryOp::ApplyTransform {
                    target: handle.id,
                    rotation,
                    translation,
                }) {
                    Ok(transformed) => transformed.id,
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!(
                            "transform application error: {}",
                            e
                        )));
                        continue;
                    }
                }
            }
            None => handle.id,
        };
        let default_visible = !(aux_ancestor || realization_is_aux(realization));
        // Pass a mutable borrow of the kernel to the visitor; the borrow is
        // released when visit_realization returns, before the sub-component
        // loop re-borrows geometry_kernels for the recursive call.
        // Explicit deref-coercion: `default_kernel` is `&mut Box<dyn GeometryKernel>`;
        // `&mut **default_kernel` gives the `&mut dyn GeometryKernel` the visitor expects.
        visit_realization(
            &mut **default_kernel,
            placed_id,
            entity_path,
            default_visible,
            t_idx,
            r_idx,
            diagnostics,
        );
    }

    // Recurse into each non-collection sub, composing the sub's `at` pose.
    for sub in &template.sub_components {
        if sub.is_collection {
            continue;
        }
        let Some(child_idx) = module
            .templates
            .iter()
            .position(|t| t.name == sub.structure_name)
        else {
            continue;
        };
        let child_prefix = format!("{}.{}", path_prefix, sub.name);
        let sub_pose = eval_sub_pose(sub.pose.as_ref(), values, functions, meta_map, diagnostics);
        let child_world = compose_pose_chain(&[composed_world.clone(), sub_pose]);

        // task-4147: for constructor-arg subs, re-realize the child's handles
        // against the per-instance override value scope BEFORE the recursive
        // call (sequencing avoids overlapping `&mut geometry_kernels` borrows).
        let child_override_row: Option<Vec<Option<KernelHandle>>> = if !sub.args.is_empty() {
            Some(crate::engine_build::realize_sub_override_handles(
                &template.name,
                sub,
                &module.templates[child_idx],
                geometry_kernels,
                default_kernel_name,
                values,
                functions,
                meta_map,
                diagnostics,
            ))
        } else {
            None
        };

        walk_placed_realizations(
            module,
            child_idx,
            &child_prefix,
            aux_ancestor || sub.is_aux,
            &child_world,
            depth + 1,
            terminal_handles,
            child_override_row.as_deref(),
            geometry_kernels,
            default_kernel_name,
            values,
            functions,
            meta_map,
            diagnostics,
            pre_filter,
            visit_realization,
        );
    }
}

/// T7 (task 3905, robustness fix esc-3905-277): for each template, the set of
/// realization indices to **exclude** from the export walk — every
/// geometry-producing realization except the template's *final* one.
///
/// Rationale: a boolean (or modify / sweep / …) whose operands are bound to named
/// lets — e.g. `let a = box(...); let b = box(...); let r = union(a, b)` — compiles
/// to one realization per let, BUT the compiler *inlines* each operand's
/// construction into the consuming realization (`r`'s ops are `[Box, Box,
/// Boolean(Step0, Step1)]`, referencing its operands by intra-realization
/// `GeomRef::Step`, not by cross-realization `GeomRef::Sub`). The `a`/`b`
/// realizations are therefore standalone duplicates of geometry already contained
/// in `r`. Surfacing all three (the pre-fix behavior, filtered only by `aux`)
/// shipped a STEP file with the two consumed input boxes PLUS their union — three
/// overlapping solids.
///
/// The pre-T7 export took `*step_handles.last()` — the terminal handle of the LAST
/// geometry-producing realization in declaration order, i.e. the un-consumed
/// result. This restores that "final realization per template" semantics while
/// preserving T7's multi-body-via-sub-components behavior: each *sub-component* is a
/// distinct template surfaced by the containment walk, so two product subs still
/// yield two bodies — only redundant *intra-template* intermediate lets are pruned.
///
/// `final` is the highest `r_idx` for which `terminal_handles[t][r]` is `Some`
/// (matching `step_handles.last()`); realizations that produced no handle are
/// already skipped by the walk, so including them in the skip set is harmless.
pub(crate) fn non_final_realization_indices(
    module: &reify_compiler::CompiledModule,
    terminal_handles: &[Vec<Option<KernelHandle>>],
) -> Vec<HashSet<usize>> {
    module
        .templates
        .iter()
        .enumerate()
        .map(|(t_idx, template)| {
            // Index of the final geometry-producing realization (highest r_idx
            // with a recorded terminal handle) — equals `step_handles.last()`.
            let final_idx = terminal_handles.get(t_idx).and_then(|handles| {
                handles
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(r_idx, h)| h.is_some().then_some(r_idx))
            });
            // Skip every realization that is not the final one.
            (0..template.realizations.len())
                .filter(|r_idx| Some(*r_idx) != final_idx)
                .collect()
        })
        .collect()
}

/// T7 export walk (task 3905): collect placed-product BRep handles for STEP export.
///
/// Thin wrapper over `walk_placed_realizations` that pushes each placed realization
/// as an `ExportBody` (handle_id + entity_path + default_visible) without tessellating.
///
/// `skip` (indexed by `t_idx`) lists realization indices to exclude — every
/// non-final intra-template realization (see [`non_final_realization_indices`]) —
/// so only the un-consumed final product body of each template is exported.
///
/// The `default_visible` flag uses the same OR-of-aux derivation as `surface_subtree` —
/// `default_visible == false` ⟺ aux or under-aux-sub ⟺ excluded from export by the
/// caller.  Source handles remain valid after `ApplyTransform` (T3 non-destructive).
/// Identity/undecomposable world transforms short-circuit to the source handle (no kernel op).
///
/// Cycle guard: same `depth > module.templates.len()` bound as `walk_placed_realizations`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn surface_export_bodies(
    module: &reify_compiler::CompiledModule,
    t_idx: usize,
    path_prefix: &str,
    // True when any ancestor sub was declared `aux` (inherited down the walk).
    aux_ancestor: bool,
    // Composed world transform inherited from root, accrued down the walk.
    composed_world: &reify_ir::Value,
    depth: usize,
    terminal_handles: &[Vec<Option<KernelHandle>>],
    geometry_kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
    default_kernel_name: &str,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    // T7 robustness fix (esc-3905-277): per-template non-final realization
    // indices. A realization at (t, r) with `skip[t].contains(&r)` is a redundant
    // intra-template intermediate let (its geometry is inlined into the template's
    // final realization) and is excluded from export.
    skip: &[HashSet<usize>],
    // Optional entity-path pre-filter (T7 amendment, suggestion 3).
    // Passed through to `walk_placed_realizations`; checked BEFORE `ApplyTransform`.
    // `None` = include all non-skipped bodies (default for `build()`).
    // `Some(f)` = also skip bodies whose path doesn't satisfy `f` (used by
    // `distance_between_placed` to avoid minting transient handles for non-target paths).
    pre_filter: PlacedPreFilter<'_>,
    export_bodies: &mut Vec<ExportBody>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Combine the caller-supplied path pre-filter (gates ApplyTransform before calling
    // the walk) with the intra-template skip set (excludes boolean operand lets).
    // The combined closure is used as the walk's pre_filter so that BOTH checks happen
    // BEFORE the kernel operation is issued, saving a transient handle per skipped body.
    let combined_filter: &dyn Fn(usize, usize, &str) -> bool = &|t: usize, r: usize, path: &str| {
        !skip.get(t).is_some_and(|set| set.contains(&r)) && pre_filter.is_none_or(|f| f(t, r, path))
    };
    walk_placed_realizations(
        module,
        t_idx,
        path_prefix,
        aux_ancestor,
        composed_world,
        depth,
        terminal_handles,
        None, // handle_row_override: roots are never overridden subs
        geometry_kernels,
        default_kernel_name,
        values,
        functions,
        meta_map,
        diagnostics,
        Some(combined_filter),
        &mut |_kernel, placed_id, entity_path, default_visible, _t, _r, _diag| {
            // The combined pre_filter already excluded non-final / non-target
            // realizations before the transform, so every body reaching this
            // visitor should be collected unconditionally.
            export_bodies.push(ExportBody {
                entity_path,
                handle_id: placed_id,
                default_visible,
            });
        },
    );
}

/// Phase-B containment-tree surfacing (T5 steps 4/6/10).
///
/// Thin wrapper over `walk_placed_realizations` that tessellates each placed
/// realization into a `MeshSurface`.
///
/// Depth-first walk from a root template: surface the current template's
/// realizations under `path_prefix` (the dotted entity-path prefix that precedes
/// the `#realization[i]` suffix), then recurse into each NON-collection sub with
/// `path_prefix` extended by `.<sub-name>`. The realization's terminal handle is
/// looked up positionally from `terminal_handles[t_idx][r_idx]` (recorded by
/// Phase A); `None` (no geometry produced) is skipped. The default kernel is
/// re-borrowed by name to tessellate, mirroring the pre-T5 terminal-handle path.
///
/// Entity-path scheme (PRD §11.2 `parent.sub#realization[i]`): for a ROOT,
/// `path_prefix` is the template name, so the surface path equals
/// `realization.id.to_string()` (`<entity>#realization[<index>]`) — bit-identical
/// to pre-T5. For a DESCENDANT, `path_prefix` is `<root>.<sub>…`, giving e.g.
/// `Assembly.c#realization[0]`.
///
/// `depth` bounds the recursion: a simple path in an acyclic sub-graph visits at
/// most `templates.len()` distinct templates, so a depth past that implies a
/// non-collection sub cycle (e.g. a recursive structure reached via a root). We
/// stop there to avoid unbounded recursion — runtime recursion unfolding is a
/// separate concern (`unfold.rs`) and out of this surfacing path's scope.
///
/// step-10 composes each sub's `at` pose down the walk (`eval_sub_pose` +
/// `compose_pose_chain`) and applies the resulting world transform via
/// `GeometryOp::ApplyTransform` on the default kernel before tessellation;
/// identity / un-placed poses short-circuit and tessellate the handle directly.
/// Step-6 threads `aux` inheritance.
#[allow(clippy::too_many_arguments)]
pub(crate) fn surface_subtree(
    module: &reify_compiler::CompiledModule,
    t_idx: usize,
    path_prefix: &str,
    // True when any ancestor sub on the path to this template was declared
    // `aux` (PRD §3 rule 2: an aux sub means the whole contained subtree has no
    // external geometric effect). Inherited down the walk; ORed with each
    // realization's own `aux` to derive `default_visible`. `false` at roots.
    aux_ancestor: bool,
    // T5 step-10: the composed world transform inherited from the root down to
    // this template (`pose_root ∘ … ∘ pose_parent`). Identity at roots. When
    // non-identity, applied to each realization's terminal geometry (via
    // `GeometryOp::ApplyTransform` on the default kernel) before tessellation so
    // the descendant surfaces at its composed world pose.
    composed_world: &reify_ir::Value,
    depth: usize,
    terminal_handles: &[Vec<Option<KernelHandle>>],
    // `&mut` so a non-identity `composed_world` can `execute` an ApplyTransform
    // on the default kernel before tessellating; the pre-step-10 walk only read
    // the kernel to tessellate.
    geometry_kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
    default_kernel_name: &str,
    tessellation_budgets: &[Vec<f64>],
    // T5 step-10: parent value / function / meta context for evaluating each
    // sub's `at` pose via `eval_sub_pose`.
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    meshes: &mut Vec<crate::MeshSurface>,
    diagnostics: &mut Vec<Diagnostic>,
    // Determinacy β (task 4198): when `true`, call
    // `kernel.measure_mesh_deviation` for each successfully tessellated
    // occurrence and insert the result into `achieved_repr_tol`. `false`
    // by default — zero overhead when γ assertions are not active.
    capture_repr_tol: bool,
    // Determinacy β (task 4198): per-build map from realized-occurrence name
    // ("{entity}#realization[{index}]") to sampled max facet-chord deviation
    // in SI metres. Populated here (the unique site holding kernel + placed_id
    // + fresh mesh + entity_path simultaneously) when `capture_repr_tol` is
    // true. Recording is skip-on-None and skip-on-empty-mesh so the map never
    // contains misleading 0.0 entries (honest absence = missing key, B3).
    achieved_repr_tol: &mut BTreeMap<String, f64>,
) {
    walk_placed_realizations(
        module,
        t_idx,
        path_prefix,
        aux_ancestor,
        composed_world,
        depth,
        terminal_handles,
        None, // handle_row_override: roots are never overridden subs
        geometry_kernels,
        default_kernel_name,
        values,
        functions,
        meta_map,
        diagnostics,
        // Tessellation surfaces all bodies (no path filter needed); pass None to
        // avoid any pre-filter overhead on the hot path.
        None,
        &mut |kernel, placed_id, entity_path, default_visible, t, r, diag| {
            let budget = tessellation_budgets[t][r];
            match kernel.tessellate(placed_id, budget) {
                Ok(mesh) => {
                    // Determinacy β (task 4198): record the sampled max
                    // facet-chord deviation BEFORE moving entity_path into
                    // MeshSurface. Gated on `capture_repr_tol` so the hot
                    // path pays zero overhead (no BRepExtrema projection,
                    // no actor round-trip) when γ assertions are not active.
                    // Only record for non-empty meshes — an empty mesh yields
                    // honest absence (missing key), never 0.0.
                    // measure_mesh_deviation returns None for non-OCCT kernels
                    // (default-absent trait method, B3). Anti-circularity: the
                    // metric takes no tolerance argument and measures actual
                    // facet-chord error, NOT the configured deflection budget.
                    if capture_repr_tol
                        && !mesh.indices.is_empty()
                        && let Some(dev) = kernel.measure_mesh_deviation(placed_id, &mesh)
                    {
                        achieved_repr_tol.insert(entity_path.clone(), dev);
                    }
                    // step-6: hide iff any `aux` ancestor sub OR this realization's
                    // own `aux` let. aux bodies are still tessellated and shipped —
                    // only hidden by default.
                    meshes.push(crate::MeshSurface {
                        entity_path,
                        mesh,
                        default_visible,
                    });
                }
                Err(e) => {
                    diag.push(Diagnostic::error(format!("tessellation error: {}", e)));
                }
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::{CompiledGeometryOp, GeomRef, PatternKind, SweepKind, TransformKind};
    use reify_ir::GeometryHandleId;

    /// Helper: build a CompiledExpr literal from a constant f64.
    fn literal_f64(v: f64) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), reify_core::Type::dimensionless_scalar())
    }

    /// Helper: build a CompiledExpr literal from a Scalar with LENGTH dimension.
    fn literal_length(meters: f64) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Scalar {
                si_value: meters,
                dimension: reify_core::DimensionVector::LENGTH,
            },
            reify_core::Type::length(),
        )
    }

    /// Helper: build a CompiledExpr literal from a Scalar with ANGLE dimension (radians).
    fn literal_angle(radians: f64) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Scalar {
                si_value: radians,
                dimension: reify_core::DimensionVector::ANGLE,
            },
            reify_core::Type::angle(),
        )
    }

    /// Helper: build an inline `CompiledExpr` literal from a `Value::Scalar`
    /// with an arbitrary `DimensionVector`. Used by task ε's inline-arg tests
    /// (the converted resolvers `eval_expr` the arg, so a `Literal` cell now
    /// flows through exactly like a `ValueRef → Scalar`). The `Type` carried by
    /// the literal is irrelevant to `eval_expr` (which clones the `Value`), so
    /// the dimension on the `Type::Scalar` simply mirrors the value's.
    fn literal_scalar(
        si_value: f64,
        dimension: reify_core::DimensionVector,
    ) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Scalar {
                si_value,
                dimension,
            },
            reify_core::Type::Scalar { dimension },
        )
    }

    /// Helper: wrap a bare `GeometryHandleId` in a `KernelHandle` with the
    /// default test kernel (`KernelId::Occt`).
    ///
    /// Bulk test fixtures use `kh(id)` to keep the named_steps map concise;
    /// contract tests that verify `.kernel` is ignored construct inline
    /// `KernelHandle { kernel: KernelId::Manifold/Fidget, id }` instead.
    fn kh(id: GeometryHandleId) -> reify_ir::KernelHandle {
        reify_ir::KernelHandle {
            kernel: reify_ir::KernelId::Occt,
            id,
        }
    }

    /// Bare `Value::Real` components in a `Value::Point` are NOT a valid
    /// production shape for a `Type::Point<Length>` cell.  The function MUST
    /// return `None` (returning `Some([...])` would silently reinterpret the
    /// raw floats as SI metres at the kernel boundary — exactly the hazard this
    /// closes).  All production mocks use `Value::length(...)` components (i.e.
    /// `Value::Scalar { dimension: LENGTH, .. }`).
    ///
    /// FLIP (task ε, evaluate-then-accept): the resolver now EVALUATES the arg
    /// and, on this defined-but-wrong shape, ALSO pushes exactly one
    /// `Severity::Warning` naming the builtin / arg / expected `Point<Length>`,
    /// instead of the prior silent fall-through to `None`.
    #[test]
    fn resolve_point3_length_arg_bare_real_components_return_none() {
        let cell = reify_core::ValueCellId::new("Bracket", "p");
        let expr = reify_ir::CompiledExpr::value_ref(
            cell.clone(),
            reify_core::Type::point3(reify_core::Type::length()),
        );
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            cell,
            reify_ir::Value::Point(vec![
                reify_ir::Value::Real(1.0),
                reify_ir::Value::Real(2.0),
                reify_ir::Value::Real(3.0),
            ]),
        );
        let mut diags: Vec<Diagnostic> = Vec::new();
        assert_eq!(
            super::resolve_point3_length_arg(&expr, &values, "closest_point", "point", &mut diags),
            None,
            "bare Value::Real components must produce None — production cells \
             carry Type::Point<Length> so components must be \
             Value::Scalar {{ dimension: LENGTH, .. }}; a bare Real slipping \
             through would be silently reinterpreted as metres at the kernel \
             boundary, hence the function must return None"
        );
        // FLIP (task ε): the defined-but-wrong shape now emits exactly one
        // Severity::Warning, not a silent None.
        assert_eq!(
            diags.len(),
            1,
            "bare-Real Point must push exactly 1 Warning (FLIP from silent), got: {diags:?}"
        );
        assert_eq!(diags[0].severity, reify_core::Severity::Warning);
        let msg = diags[0].message.to_lowercase();
        assert!(
            msg.contains("closest_point"),
            "warning must name the builtin, got: {:?}",
            diags[0].message
        );
        assert!(
            msg.contains("point<length>"),
            "warning must name expected Point<Length>, got: {:?}",
            diags[0].message
        );
    }

    /// Task ε (evaluate-then-accept): `resolve_point3_length_arg` now EVALUATES
    /// the arg expr (gaining a `diagnostics` sink + builtin/arg labels). A
    /// `Value::Point` of exactly three LENGTH-dimensioned Scalars — whether an
    /// inline `Literal` or a `ValueRef → Point<Length>` cell — resolves to its
    /// `[m, m, m]` SI components with 0 diagnostics; a defined-but-wrong value
    /// (non-Point, or wrong arity) is Rejected with exactly one
    /// `Severity::Warning` naming the builtin, the arg, and the expected
    /// `Point<Length>` type (byte-uniform wording with the density / vec3 /
    /// range paths). A `Value::Undef` (missing cell) degrades quietly.
    ///
    ///   (a) inline `Literal(Point[LENGTH×3])` → `Some([..])`, 0 diags.
    ///   (b) `ValueRef → Point[LENGTH×3]` cell → `Some([..])`, 0 diags.
    ///   (c) non-Point (`Value::Real`) → `None` + 1 Warning.
    ///   (d) wrong arity (`Point` of 2) → `None` + 1 Warning.
    ///   (e) missing-cell `ValueRef` → `Undef` → `None`, 0 diags (quiet).
    ///
    /// Compile-RED until step-10 adds the `(builtin, arg, &mut diags)` signature.
    #[test]
    fn resolve_point3_length_arg_eval_and_diagnostics() {
        // (a) inline Literal(Point[LENGTH×3]) → Some([..]), 0 diags.
        {
            let expr = reify_ir::CompiledExpr::literal(
                point3_length_value(0.01, 0.02, 0.03),
                reify_core::Type::point3(reify_core::Type::length()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_point3_length_arg(
                &expr,
                &values,
                "closest_point",
                "point",
                &mut diags,
            );
            assert_eq!(
                result,
                Some([0.01, 0.02, 0.03]),
                "(a) inline Point<Length> literal must be Accepted"
            );
            assert!(diags.is_empty(), "(a) Point literal must produce no diags, got: {diags:?}");
        }

        // (b) ValueRef → Point[LENGTH×3] cell → Some([..]), 0 diags.
        {
            let cell = reify_core::ValueCellId::new("Bracket", "p");
            let expr = reify_ir::CompiledExpr::value_ref(
                cell.clone(),
                reify_core::Type::point3(reify_core::Type::length()),
            );
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, point3_length_value(0.1, 0.2, 0.3));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_point3_length_arg(&expr, &values, "is_on", "point", &mut diags);
            assert_eq!(
                result,
                Some([0.1, 0.2, 0.3]),
                "(b) ValueRef Point<Length> must be Accepted"
            );
            assert!(diags.is_empty(), "(b) ValueRef Point must produce no diags, got: {diags:?}");
        }

        // (c) non-Point (Value::Real) → None + 1 Warning naming builtin/arg/Point<Length>.
        {
            let expr = literal_f64(1.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_point3_length_arg(&expr, &values, "contains", "point", &mut diags);
            assert_eq!(result, None, "(c) non-Point must return None");
            assert_eq!(diags.len(), 1, "(c) non-Point must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("contains"), "(c) names builtin, got: {:?}", diags[0].message);
            assert!(msg.contains("point"), "(c) names arg, got: {:?}", diags[0].message);
            assert!(
                msg.contains("point<length>"),
                "(c) names expected Point<Length>, got: {:?}",
                diags[0].message
            );
            assert!(msg.contains("got"), "(c) names what it got, got: {:?}", diags[0].message);
        }

        // (d) wrong arity (Point of 2 LENGTH scalars) → None + 1 Warning.
        {
            let expr = reify_ir::CompiledExpr::literal(
                reify_ir::Value::Point(vec![
                    reify_ir::Value::length(0.01),
                    reify_ir::Value::length(0.02),
                ]),
                reify_core::Type::point3(reify_core::Type::length()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_point3_length_arg(&expr, &values, "normal", "point", &mut diags);
            assert_eq!(result, None, "(d) wrong-arity Point must return None");
            assert_eq!(
                diags.len(),
                1,
                "(d) wrong-arity Point must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("point<length>"),
                "(d) names expected Point<Length>, got: {:?}",
                diags[0].message
            );
        }

        // (e) missing-cell ValueRef → Undef → None, 0 diags (quiet).
        {
            let cell = reify_core::ValueCellId::new("Bracket", "missing_point");
            let expr = reify_ir::CompiledExpr::value_ref(
                cell,
                reify_core::Type::point3(reify_core::Type::length()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_point3_length_arg(&expr, &values, "curvature", "point", &mut diags);
            assert_eq!(result, None, "(e) missing cell must return None");
            assert!(diags.is_empty(), "(e) missing cell must be quiet, got: {diags:?}");
        }
    }

    /// Task ε (evaluate-then-accept): `resolve_int_value_ref` (the kinematic
    /// body-id resolver for `interferes_with` / `min_clearance`) now EVALUATES
    /// the arg expr (gaining a `diagnostics` sink + builtin/arg labels) instead
    /// of shape-matching `CompiledExprKind::ValueRef`. A `Value::Int` — whether
    /// an inline `Literal` or a `ValueRef → Int` cell — resolves to its `i64`
    /// with 0 diagnostics; a defined-but-wrong value (non-Int) is Rejected with
    /// exactly one `Severity::Warning` naming the kinematic builtin, the arg,
    /// and the expected `Int` type (byte-uniform wording with the density /
    /// point / vec3 / range paths). A `Value::Undef` (missing cell) degrades
    /// quietly — behaviourally identical to the prior `values.get(id)` fall-through.
    ///
    ///   (a) inline `Literal(Int)` → `Some(n)`, 0 diags.
    ///   (b) `ValueRef → Int` cell → `Some(n)`, 0 diags.
    ///   (c) non-Int (`Value::Real`) → `None` + 1 Warning naming builtin/arg/Int/got.
    ///   (d) non-Int (`Value::Scalar`) → `None` + 1 Warning.
    ///   (e) missing-cell `ValueRef` → `Undef` → `None`, 0 diags (quiet).
    ///
    /// Compile-RED until step-12 adds the `(builtin, arg, &mut diags)` signature
    /// (today `resolve_int_value_ref` is `(expr, values) -> Option<i64>` and
    /// silently returns `None` on a non-Int, with no diagnostic).
    #[test]
    fn resolve_int_value_ref_eval_and_diagnostics() {
        // (a) inline Literal(Int) → Some(n), 0 diags.
        {
            let expr = reify_ir::CompiledExpr::literal(
                reify_ir::Value::Int(7),
                reify_core::Type::Int,
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_int_value_ref(
                &expr,
                &values,
                "interferes_with",
                "body_a",
                &mut diags,
            );
            assert_eq!(result, Some(7), "(a) inline Int literal must be Accepted");
            assert!(diags.is_empty(), "(a) Int literal must produce no diags, got: {diags:?}");
        }

        // (b) ValueRef → Int cell → Some(n), 0 diags.
        {
            let cell = reify_core::ValueCellId::new("Mech", "id_a");
            let expr = reify_ir::CompiledExpr::value_ref(cell.clone(), reify_core::Type::Int);
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, reify_ir::Value::Int(2));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_int_value_ref(
                &expr,
                &values,
                "min_clearance",
                "body_b",
                &mut diags,
            );
            assert_eq!(result, Some(2), "(b) ValueRef Int must be Accepted");
            assert!(diags.is_empty(), "(b) ValueRef Int must produce no diags, got: {diags:?}");
        }

        // (c) non-Int (Value::Real) → None + 1 Warning naming builtin/arg/Int/got.
        {
            let expr = literal_f64(1.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_int_value_ref(
                &expr,
                &values,
                "interferes_with",
                "body_a",
                &mut diags,
            );
            assert_eq!(result, None, "(c) non-Int must return None");
            assert_eq!(diags.len(), 1, "(c) non-Int must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("interferes_with"),
                "(c) names builtin, got: {:?}",
                diags[0].message
            );
            assert!(msg.contains("body_a"), "(c) names arg, got: {:?}", diags[0].message);
            assert!(msg.contains("int"), "(c) names expected Int, got: {:?}", diags[0].message);
            assert!(msg.contains("got"), "(c) names what it got, got: {:?}", diags[0].message);
        }

        // (d) non-Int (Value::Scalar) → None + 1 Warning.
        {
            let expr = literal_length(0.05);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_int_value_ref(
                &expr,
                &values,
                "min_clearance",
                "body_b",
                &mut diags,
            );
            assert_eq!(result, None, "(d) Scalar must return None");
            assert_eq!(diags.len(), 1, "(d) Scalar must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("int"), "(d) names expected Int, got: {:?}", diags[0].message);
        }

        // (e) missing-cell ValueRef → Undef → None, 0 diags (quiet).
        {
            let cell = reify_core::ValueCellId::new("Mech", "missing_id");
            let expr = reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::Int);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_int_value_ref(
                &expr,
                &values,
                "interferes_with",
                "body_a",
                &mut diags,
            );
            assert_eq!(result, None, "(e) missing cell must return None");
            assert!(diags.is_empty(), "(e) missing cell must be quiet, got: {diags:?}");
        }
    }

    /// Task ε (evaluate-then-accept): `resolve_string_literal_arg` (the selector
    /// name/label resolver for `face`/`edge`/`solid_body` and the ad-hoc
    /// `@face`/`@edge` base/label) now EVALUATES the arg expr (gaining a
    /// `diagnostics` sink + builtin/arg labels) and returns an OWNED `String`
    /// (was `Option<&str>` matching only `Literal(Value::String)`). A
    /// `Value::String` — whether an inline `Literal` or a `ValueRef → String`
    /// cell — resolves to its owned `String` with 0 diagnostics; a
    /// defined-but-wrong value (non-String) is Rejected with exactly one
    /// `Severity::Warning` naming the builtin, the arg, and the expected
    /// `String` type (byte-uniform wording with the density / point / vec3 /
    /// range / int paths). A `Value::Undef` (missing cell) degrades quietly.
    ///
    /// Both call contexts are covered via the builtin/arg labels: the named
    /// leaf selector (`face(body,"top")` → builtin `face`, arg `name`) and the
    /// ad-hoc selector (`@face("top")` → builtin `@face`, arg `label`).
    ///
    ///   (a) inline `Literal(String)` → `Some("top")`, 0 diags.
    ///   (b) `ValueRef → String` cell → `Some("side")`, 0 diags.
    ///   (c) non-String (`Value::Int`) → `None` + 1 Warning naming builtin/arg/String/got.
    ///   (d) missing-cell `ValueRef` → `Undef` → `None`, 0 diags (quiet).
    ///
    /// Compile-RED until step-14 changes the signature to
    /// `(expr, values, builtin, arg, &mut diags) -> Option<String>` (today
    /// `resolve_string_literal_arg(expr) -> Option<&str>` matches only an inline
    /// `Literal(Value::String)` and silently returns `None` otherwise).
    #[test]
    fn resolve_string_literal_arg_eval_and_diagnostics() {
        // (a) inline Literal(String) → Some("top"), 0 diags (named-leaf context).
        {
            let expr = reify_ir::CompiledExpr::literal(
                reify_ir::Value::String("top".to_string()),
                reify_core::Type::String,
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_string_literal_arg(&expr, &values, "face", "name", &mut diags);
            assert_eq!(
                result,
                Some("top".to_string()),
                "(a) inline String literal must be Accepted as an owned String"
            );
            assert!(diags.is_empty(), "(a) String literal must produce no diags, got: {diags:?}");
        }

        // (b) ValueRef → String cell → Some("side"), 0 diags (ad-hoc label context).
        {
            let cell = reify_core::ValueCellId::new("Part", "label");
            let expr = reify_ir::CompiledExpr::value_ref(cell.clone(), reify_core::Type::String);
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, reify_ir::Value::String("side".to_string()));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_string_literal_arg(&expr, &values, "@face", "label", &mut diags);
            assert_eq!(
                result,
                Some("side".to_string()),
                "(b) ValueRef String must be Accepted"
            );
            assert!(diags.is_empty(), "(b) ValueRef String must produce no diags, got: {diags:?}");
        }

        // (c) non-String (Value::Int) → None + 1 Warning naming builtin/arg/String/got.
        {
            let expr = reify_ir::CompiledExpr::literal(
                reify_ir::Value::Int(5),
                reify_core::Type::Int,
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_string_literal_arg(&expr, &values, "edge", "name", &mut diags);
            assert_eq!(result, None, "(c) non-String must return None");
            assert_eq!(diags.len(), 1, "(c) non-String must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("edge"), "(c) names builtin, got: {:?}", diags[0].message);
            assert!(msg.contains("name"), "(c) names arg, got: {:?}", diags[0].message);
            assert!(msg.contains("string"), "(c) names expected String, got: {:?}", diags[0].message);
            assert!(msg.contains("got"), "(c) names what it got, got: {:?}", diags[0].message);
        }

        // (d) missing-cell ValueRef → Undef → None, 0 diags (quiet).
        {
            let cell = reify_core::ValueCellId::new("Part", "missing_label");
            let expr = reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::String);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_string_literal_arg(&expr, &values, "@edge", "label", &mut diags);
            assert_eq!(result, None, "(d) missing cell must return None");
            assert!(diags.is_empty(), "(d) missing cell must be quiet, got: {diags:?}");
        }
    }

    /// Tests for `resolve_density_arg`: diagnostic behavior for the NEW
    /// Density-only contract (γ, task 4486).
    ///
    /// NEW contract under test:
    ///   (a) ValueRef → Scalar{MASS_DENSITY, 7850.0} → Some(7850.0), 0 diagnostics
    ///       [NEW accept — was Warning+None under the old contract].
    ///   (b) ValueRef → Value::Real(7850.0) → None + exactly 1 Severity::Warning
    ///       whose lowercased message contains "density" AND "7850kg/m^3"
    ///       [FLIP — was accepted silently].
    ///   (c) ValueRef → dimensionless Scalar → None + 1 Warning [FLIP — was accepted].
    ///   (d) ValueRef → Scalar{LENGTH} → None + 1 Warning [keep reject].
    ///   (e) ValueRef → Value::Bool(true) → None + 1 Warning [keep reject].
    ///   (f) Non-ValueRef expr (literal_f64) → None + exactly 1 Warning
    ///       [LOUD — was 0/silent under old "unsupported arg shape → silent" contract].
    ///
    /// Modelled on `resolve_point3_length_arg_bare_real_components_return_none` above
    /// — build a `value_ref` expr + a `ValueMap`, call the helper directly,
    /// assert the return value and diagnostic side-effect, compiler-independently.
    #[test]
    fn resolve_density_arg_diagnostics() {
        fn make_value_ref(cell: reify_core::ValueCellId) -> reify_ir::CompiledExpr {
            reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::dimensionless_scalar())
        }

        // (a) ValueRef → MASS_DENSITY Scalar → Some(7850.0), 0 diagnostics [NEW accept]
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(
                cell,
                reify_ir::Value::Scalar {
                    si_value: 7850.0,
                    dimension: reify_core::DimensionVector::MASS_DENSITY,
                },
            );
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(
                result,
                Some(7850.0),
                "(a) MASS_DENSITY Scalar must return Some(7850.0)"
            );
            assert!(
                diags.is_empty(),
                "(a) MASS_DENSITY Scalar must produce no diagnostics, got: {:?}",
                diags
            );
        }

        // (b) ValueRef → Value::Real(7850.0) → None + 1 Warning with "density" + "7850kg/m^3" [FLIP]
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho2");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, reify_ir::Value::Real(7850.0));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(b) Value::Real must return None");
            assert_eq!(
                diags.len(),
                1,
                "(b) Value::Real must push exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(b) diagnostic must be Warning severity"
            );
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("density"),
                "(b) warning must name 'density', got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("7850kg/m^3"),
                "(b) warning must contain '7850kg/m^3' migration hint, got: {:?}",
                diags[0].message
            );
        }

        // (c) ValueRef → dimensionless Scalar → None + 1 Warning [FLIP]
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho3");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(
                cell,
                reify_ir::Value::Scalar {
                    si_value: 7850.0,
                    dimension: reify_core::DimensionVector::DIMENSIONLESS,
                },
            );
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(
                result, None,
                "(c) dimensionless Scalar must return None (no longer accepted)"
            );
            assert_eq!(
                diags.len(),
                1,
                "(c) dimensionless Scalar must push exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(c) diagnostic must be Warning severity"
            );
        }

        // (d) ValueRef → Scalar{LENGTH} → None + 1 Warning [keep reject]
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho4");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(
                cell,
                reify_ir::Value::Scalar {
                    si_value: 1.0,
                    dimension: reify_core::DimensionVector::LENGTH,
                },
            );
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(d) LENGTH Scalar must return None");
            assert_eq!(
                diags.len(),
                1,
                "(d) LENGTH Scalar must push exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(d) diagnostic must be Warning severity"
            );
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("density"),
                "(d) warning must name 'density', got: {:?}",
                diags[0].message
            );
        }

        // (e) ValueRef → Value::Bool(true) → None + 1 Warning [keep reject]
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho5");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, reify_ir::Value::Bool(true));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(e) Bool must return None");
            assert_eq!(
                diags.len(),
                1,
                "(e) Bool must push exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(e) diagnostic must be Warning severity"
            );
        }

        // (f) Non-ValueRef (literal_f64) → None + 1 Warning [LOUD — was silent]
        {
            let expr = literal_f64(7850.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(
                result, None,
                "(f) Literal expr must return None"
            );
            assert_eq!(
                diags.len(),
                1,
                "(f) Non-ValueRef literal must push exactly 1 Warning (γ=LOUD), got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(f) diagnostic must be Warning severity"
            );
        }
    }

    /// Task ε (evaluate-then-accept): `resolve_density_arg` now EVALUATES an
    /// inline (non-`ValueRef`) arg expression instead of warning "not yet
    /// supported". The headline `moment_of_inertia(b, 7850kg/m^3)` inline form
    /// must be ACCEPTED; an inline bare `Real` / wrong-dimension `Scalar` must
    /// be REJECTED with exactly one Warning carrying the same wording as the
    /// `ValueRef` path.
    ///
    ///   (a) inline `Literal(Scalar{MASS_DENSITY, 7850})` → `Some(7850.0)`,
    ///       0 diagnostics [RED before ε: the non-`ValueRef` branch warned + None].
    ///   (b) inline `Literal(Real(7850.0))` → `None` + 1 Warning naming
    ///       `density` + `7850kg/m^3`.
    ///   (c) inline `Literal(Scalar{PRESSURE})` → `None` + 1 Warning
    ///       (Pressure-as-density hole stays closed for the inline shape too).
    #[test]
    fn resolve_density_arg_inline_evaluates() {
        // (a) inline MASS_DENSITY literal → Some(7850.0), 0 diagnostics.
        {
            let expr = literal_scalar(7850.0, reify_core::DimensionVector::MASS_DENSITY);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(
                result,
                Some(7850.0),
                "(a) inline MASS_DENSITY literal must evaluate + be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(a) inline MASS_DENSITY literal must produce no diagnostics, got: {:?}",
                diags
            );
        }

        // (b) inline bare Real literal → None + 1 Warning naming density + hint.
        {
            let expr = literal_f64(7850.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(b) inline bare Real must return None");
            assert_eq!(
                diags.len(),
                1,
                "(b) inline bare Real must push exactly 1 Warning, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(b) diagnostic must be Warning severity"
            );
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("density"),
                "(b) warning must name 'density', got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("7850kg/m^3"),
                "(b) warning must contain '7850kg/m^3' migration hint, got: {:?}",
                diags[0].message
            );
        }

        // (c) inline Pressure Scalar literal → None + 1 Warning [closed hole].
        {
            let expr = literal_scalar(2.0e11, reify_core::DimensionVector::PRESSURE);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(c) inline Pressure Scalar must return None");
            assert_eq!(
                diags.len(),
                1,
                "(c) inline Pressure Scalar must push exactly 1 Warning, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(c) diagnostic must be Warning severity"
            );
        }
    }

    /// Helper: build a `CompiledExpr` literal from a `Value::Bool`.
    fn literal_bool(b: bool) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(reify_ir::Value::Bool(b), reify_core::Type::Bool)
    }

    /// Task ε (evaluate-then-accept): the scalar-bound wrappers
    /// `resolve_angle_scalar_arg` / `resolve_length_scalar_arg` now EVALUATE the
    /// arg expr and route the result through `accept_arg`, gaining a
    /// `diagnostics` sink + builtin/arg labels. An inline dimensioned literal of
    /// the expected dimension is Accepted (0 diags); a defined-but-wrong value
    /// (wrong dimension, dimensionless, or non-Scalar) is Rejected with exactly
    /// one Warning naming the builtin, the arg, and the expected type.
    #[test]
    fn resolve_scalar_bound_arg_eval_and_diagnostics() {
        // (a) inline ANGLE literal → Some(rad), 0 diagnostics.
        {
            let expr = literal_scalar(0.25, reify_core::DimensionVector::ANGLE);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_angle_scalar_arg(
                &expr,
                &values,
                "faces_by_normal",
                "tol",
                &mut diags,
            );
            assert_eq!(result, Some(0.25), "(a) inline ANGLE literal must be Accepted");
            assert!(diags.is_empty(), "(a) ANGLE literal must produce no diags, got: {diags:?}");
        }

        // (b) inline LENGTH literal → Some(m), 0 diagnostics.
        {
            let expr = literal_scalar(0.005, reify_core::DimensionVector::LENGTH);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_length_scalar_arg(
                &expr,
                &values,
                "edges_at_height",
                "z",
                &mut diags,
            );
            assert_eq!(result, Some(0.005), "(b) inline LENGTH literal must be Accepted");
            assert!(diags.is_empty(), "(b) LENGTH literal must produce no diags, got: {diags:?}");
        }

        // (c) wrong dimension (ANGLE where LENGTH expected) → None + 1 Warning.
        {
            let expr = literal_scalar(0.25, reify_core::DimensionVector::ANGLE);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_length_scalar_arg(
                &expr,
                &values,
                "edges_at_height",
                "z",
                &mut diags,
            );
            assert_eq!(result, None, "(c) ANGLE where LENGTH expected must return None");
            assert_eq!(diags.len(), 1, "(c) must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("edges_at_height"), "(c) names builtin, got: {:?}", diags[0].message);
            assert!(msg.contains("z"), "(c) names arg, got: {:?}", diags[0].message);
            assert!(msg.contains("length"), "(c) names expected Length, got: {:?}", diags[0].message);
        }

        // (d) non-Scalar (Bool) where ANGLE expected → None + 1 Warning.
        {
            let expr = literal_bool(true);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_angle_scalar_arg(
                &expr,
                &values,
                "faces_by_normal",
                "tol",
                &mut diags,
            );
            assert_eq!(result, None, "(d) Bool where ANGLE expected must return None");
            assert_eq!(diags.len(), 1, "(d) must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("faces_by_normal"), "(d) names builtin, got: {:?}", diags[0].message);
            assert!(msg.contains("angle"), "(d) names expected Angle, got: {:?}", diags[0].message);
        }

        // (e) Undef (missing cell ValueRef) → None, 0 diagnostics (quiet).
        {
            let cell = reify_core::ValueCellId::new("Bracket", "missing");
            let expr = reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::length());
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_length_scalar_arg(
                &expr,
                &values,
                "edges_at_height",
                "z",
                &mut diags,
            );
            assert_eq!(result, None, "(e) missing cell must return None");
            assert!(diags.is_empty(), "(e) missing cell must be quiet, got: {diags:?}");
        }
    }

    /// Task ε (evaluate-then-accept): `resolve_vec3_arg` now EVALUATES the arg
    /// expr (gaining a `diagnostics` sink + builtin/arg labels). An inline
    /// `Literal(Value::Vector)` AND an inline `vec3(..)` FunctionCall both
    /// resolve to `Some([..])` with 0 diagnostics; a defined-but-wrong value
    /// (non-Vector, wrong length, or a dimensioned-Scalar component) is Rejected
    /// with exactly one `Severity::Warning` naming the builtin, the arg, and the
    /// expected `Vec3` type (byte-uniform wording with the density path).
    ///
    ///   (a) inline `Literal(Vector([Real,Real,Real]))` → `Some([..])`, 0 diags.
    ///   (b) inline `vec3(0,0,1)` FunctionCall → `Some([0,0,1])`, 0 diags
    ///       [RED before ε: the FunctionCall hit `resolve_vec3_arg`'s `_ => None`
    ///       arm → silent fall-through].
    ///   (c) inline `Literal(Real)` (non-Vector) → `None` + 1 Warning.
    ///   (d) inline `Literal(Vector)` of length 2 (wrong length) → `None` + 1 Warning.
    ///   (e) inline `Literal(Vector)` with a dimensioned-Scalar component → `None`
    ///       + 1 Warning.
    ///
    /// Compile-RED until step-6 adds the `(builtin, arg, &mut diags)` signature.
    #[test]
    fn resolve_vec3_arg_eval_and_diagnostics() {
        // (a) inline Literal(Vector([Real,Real,Real])) → Some([..]), 0 diags.
        {
            let expr = reify_ir::CompiledExpr::literal(
                vec3_value(0.0, 0.0, 1.0),
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_vec3_arg(&expr, &values, "faces_by_normal", "dir", &mut diags);
            assert_eq!(
                result,
                Some([0.0, 0.0, 1.0]),
                "(a) inline vector literal must be Accepted"
            );
            assert!(diags.is_empty(), "(a) vector literal must produce no diags, got: {diags:?}");
        }

        // (b) inline vec3(0,0,1) FunctionCall → Some([0,0,1]), 0 diags.
        {
            let arg_x = literal_f64(0.0);
            let arg_y = literal_f64(0.0);
            let arg_z = literal_f64(1.0);
            let mut ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
                .combine(reify_core::ContentHash::of_str("vec3"));
            ch = ch
                .combine(arg_x.content_hash)
                .combine(arg_y.content_hash)
                .combine(arg_z.content_hash);
            let expr = reify_ir::CompiledExpr {
                kind: reify_ir::CompiledExprKind::FunctionCall {
                    function: reify_ir::ResolvedFunction {
                        name: "vec3".to_string(),
                        qualified_name: "vec3".to_string(),
                    },
                    args: vec![arg_x, arg_y, arg_z],
                },
                result_type: reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
                content_hash: ch,
            };
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_vec3_arg(&expr, &values, "faces_by_normal", "dir", &mut diags);
            assert_eq!(
                result,
                Some([0.0, 0.0, 1.0]),
                "(b) inline vec3(0,0,1) FunctionCall must evaluate + be Accepted"
            );
            assert!(diags.is_empty(), "(b) inline vec3 call must produce no diags, got: {diags:?}");
        }

        // (c) non-Vector (Value::Real) → None + 1 Warning naming builtin/arg/Vec3.
        {
            let expr = literal_f64(1.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_vec3_arg(&expr, &values, "faces_by_normal", "dir", &mut diags);
            assert_eq!(result, None, "(c) non-Vector must return None");
            assert_eq!(diags.len(), 1, "(c) non-Vector must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("faces_by_normal"), "(c) names builtin, got: {:?}", diags[0].message);
            assert!(msg.contains("dir"), "(c) names arg, got: {:?}", diags[0].message);
            assert!(msg.contains("vec3"), "(c) names expected Vec3, got: {:?}", diags[0].message);
            assert!(msg.contains("got"), "(c) names what it got, got: {:?}", diags[0].message);
        }

        // (d) wrong length (Vector of 2) → None + 1 Warning.
        {
            let expr = reify_ir::CompiledExpr::literal(
                reify_ir::Value::Vector(vec![
                    reify_ir::Value::Real(0.0),
                    reify_ir::Value::Real(1.0),
                ]),
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_vec3_arg(&expr, &values, "edges_parallel_to", "axis", &mut diags);
            assert_eq!(result, None, "(d) wrong-length Vector must return None");
            assert_eq!(diags.len(), 1, "(d) wrong-length Vector must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("edges_parallel_to"), "(d) names builtin, got: {:?}", diags[0].message);
            assert!(msg.contains("axis"), "(d) names arg, got: {:?}", diags[0].message);
            assert!(msg.contains("vec3"), "(d) names expected Vec3, got: {:?}", diags[0].message);
        }

        // (e) dimensioned-Scalar component → None + 1 Warning.
        {
            let expr = reify_ir::CompiledExpr::literal(
                reify_ir::Value::Vector(vec![
                    reify_ir::Value::Scalar {
                        si_value: 1.0,
                        dimension: reify_core::DimensionVector::LENGTH,
                    },
                    reify_ir::Value::Real(0.0),
                    reify_ir::Value::Real(0.0),
                ]),
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_vec3_arg(&expr, &values, "faces_by_normal", "dir", &mut diags);
            assert_eq!(result, None, "(e) dimensioned component must return None");
            assert_eq!(diags.len(), 1, "(e) dimensioned component must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("vec3"), "(e) names expected Vec3, got: {:?}", diags[0].message);
        }
    }

    /// Helper: build an inline `CompiledExpr` literal carrying a `Value::Range`
    /// with the given optional `(lower, upper)` SI bounds, each a
    /// `Value::Scalar` of `dim`. `None` bounds model a half-open range. The
    /// `Type` is irrelevant to `eval_expr` (which clones the `Value`), so a
    /// `dimensionless_scalar` placeholder suffices.
    fn literal_range(
        lower: Option<f64>,
        upper: Option<f64>,
        dim: reify_core::DimensionVector,
    ) -> reify_ir::CompiledExpr {
        let mk = |si: f64| -> Box<reify_ir::Value> {
            Box::new(reify_ir::Value::Scalar {
                si_value: si,
                dimension: dim,
            })
        };
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Range {
                lower: lower.map(mk),
                upper: upper.map(mk),
                lower_inclusive: true,
                upper_inclusive: true,
            },
            reify_core::Type::dimensionless_scalar(),
        )
    }

    /// Task ε (evaluate-then-accept): `resolve_range_dim_arg` now EVALUATES the
    /// arg expr (gaining a `diagnostics` sink + builtin/arg labels). An inline
    /// `Range<dim>` with both bounds present and dimensioned `expected_dim`
    /// resolves to `Some((lo, hi))` with 0 diagnostics; a defined-but-wrong
    /// value — non-Range, half-open (one bound `None`), or bounds of the wrong
    /// dimension — is Rejected with exactly one `Severity::Warning` naming the
    /// builtin, the arg, and the expected `Range<dim>` type (byte-uniform
    /// wording with the density / vec3 paths). A `Value::Undef` (missing cell)
    /// degrades quietly.
    ///
    ///   (a) inline `Literal(Range{Some(LENGTH 0), Some(LENGTH 0.05)})`
    ///       → `Some((0.0, 0.05))`, 0 diags.
    ///   (b) inline `Literal(Real)` (non-Range) → `None` + 1 Warning.
    ///   (c) inline half-open `Range{Some, None}` → `None` + 1 Warning.
    ///   (d) inline wrong-dimension `Range{ANGLE, ANGLE}` where LENGTH expected
    ///       → `None` + 1 Warning.
    ///   (e) missing-cell `ValueRef` → `Value::Undef` → `None`, 0 diags (quiet).
    ///   (f) inline `Range<AREA>` (faces_by_area path) → `Some(..)`, 0 diags.
    ///
    /// Compile-RED until step-8 adds the
    /// `(expected_dim, builtin, arg, &mut diags)` signature.
    #[test]
    fn resolve_range_dim_arg_eval_and_diagnostics() {
        use reify_core::DimensionVector;

        // (a) inline Range<LENGTH> with both bounds → Some((0.0, 0.05)), 0 diags.
        {
            let expr = literal_range(Some(0.0), Some(0.05), DimensionVector::LENGTH);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::LENGTH,
                "edges_by_length",
                "length_range",
                &mut diags,
            );
            assert_eq!(
                result,
                Some((0.0, 0.05)),
                "(a) inline closed Range<Length> must be Accepted"
            );
            assert!(diags.is_empty(), "(a) closed Range must produce no diags, got: {diags:?}");
        }

        // (b) non-Range (Value::Real) → None + 1 Warning naming builtin/arg/Range.
        {
            let expr = literal_f64(1.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::LENGTH,
                "edges_by_length",
                "length_range",
                &mut diags,
            );
            assert_eq!(result, None, "(b) non-Range must return None");
            assert_eq!(diags.len(), 1, "(b) non-Range must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("edges_by_length"), "(b) names builtin, got: {:?}", diags[0].message);
            assert!(msg.contains("length_range"), "(b) names arg, got: {:?}", diags[0].message);
            assert!(msg.contains("range"), "(b) names expected Range, got: {:?}", diags[0].message);
            assert!(msg.contains("got"), "(b) names what it got, got: {:?}", diags[0].message);
        }

        // (c) half-open Range (upper: None) → None + 1 Warning.
        {
            let expr = literal_range(Some(0.0), None, DimensionVector::LENGTH);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::LENGTH,
                "edges_by_length",
                "length_range",
                &mut diags,
            );
            assert_eq!(result, None, "(c) half-open Range must return None");
            assert_eq!(diags.len(), 1, "(c) half-open Range must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("range"), "(c) names expected Range, got: {:?}", diags[0].message);
        }

        // (d) wrong-dimension bounds (ANGLE where LENGTH expected) → None + 1 Warning.
        {
            let expr = literal_range(Some(0.0), Some(0.25), DimensionVector::ANGLE);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::LENGTH,
                "edges_by_length",
                "length_range",
                &mut diags,
            );
            assert_eq!(result, None, "(d) wrong-dimension bounds must return None");
            assert_eq!(diags.len(), 1, "(d) wrong-dimension bounds must push exactly 1 Warning, got: {diags:?}");
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(msg.contains("range"), "(d) names expected Range, got: {:?}", diags[0].message);
        }

        // (e) missing-cell ValueRef → Undef → None, 0 diags (quiet).
        {
            let cell = reify_core::ValueCellId::new("Bracket", "missing_range");
            let expr = reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::dimensionless_scalar());
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::LENGTH,
                "edges_by_length",
                "length_range",
                &mut diags,
            );
            assert_eq!(result, None, "(e) missing cell must return None");
            assert!(diags.is_empty(), "(e) missing cell must be quiet, got: {diags:?}");
        }

        // (f) inline Range<AREA> (faces_by_area path) → Some((0.0, 1.0)), 0 diags.
        {
            let expr = literal_range(Some(0.0), Some(1.0), DimensionVector::AREA);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::AREA,
                "faces_by_area",
                "area_range",
                &mut diags,
            );
            assert_eq!(
                result,
                Some((0.0, 1.0)),
                "(f) inline closed Range<Area> must be Accepted"
            );
            assert!(diags.is_empty(), "(f) closed Range<Area> must produce no diags, got: {diags:?}");
        }
    }

    // Constants `DEGENERATE_LENGTH_M`, `DEGENERATE_ANGLE_RAD`, and
    // `GEOMETRY_EPSILON` (top of file) are not pinned by a standalone unit
    // test — that would just restate the `const` definitions. Their behavior
    // is pinned by the boundary tests that drive the guards they feed:
    //   - `build_extrude_distance_{just_below,at}_threshold_*` (geometry_error_handling.rs)
    //     → DEGENERATE_LENGTH_M (inclusive floor)
    //   - `build_revolve_angle_{just_below,negative_just_below}_threshold_rejected`
    //     → DEGENERATE_ANGLE_RAD (sign-symmetric floor)
    //   - `extrude_symmetric_{per_side,negative_per_side}_{just_below,at}_threshold_*`
    //     (extrude_symmetric_e2e.rs) → 2 * DEGENERATE_LENGTH_M (per-side floor)
    // Any numeric change to the constants will fail those boundary tests.

    #[test]
    fn compile_geometry_op_scale_produces_scale_variant() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(2.0))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for Scale");

        match result {
            reify_ir::GeometryOp::Scale { target, factor } => {
                assert_eq!(target, GeometryHandleId(42));
                assert!((factor - 2.0).abs() < 1e-12);
            }
            other => panic!("expected GeometryOp::Scale, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_rotate_around_produces_rotate_around_variant() {
        let step_handles = vec![GeometryHandleId(99)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::RotateAround,
            target: GeomRef::Step(0),
            args: vec![
                ("px".into(), literal_f64(0.05)),
                ("py".into(), literal_f64(0.0)),
                ("pz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::FRAC_PI_2)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for RotateAround");

        match result {
            reify_ir::GeometryOp::RotateAround {
                target,
                point,
                axis,
                angle_rad,
            } => {
                assert_eq!(target, GeometryHandleId(99));
                assert!((point[0] - 0.05).abs() < 1e-12);
                assert!((point[1]).abs() < 1e-12);
                assert!((point[2]).abs() < 1e-12);
                assert!((axis[0]).abs() < 1e-12);
                assert!((axis[1]).abs() < 1e-12);
                assert!((axis[2] - 1.0).abs() < 1e-12);
                assert!((angle_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
            }
            other => panic!("expected GeometryOp::RotateAround, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_sweep_resolves_distinct_profiles() {
        // Two distinct step handles representing two wire profiles
        let step_handles = vec![GeometryHandleId(100), GeometryHandleId(200)];
        let values = ValueMap::new();

        // Create a Loft sweep that references Step(0) and Step(1)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Loft,
            profiles: vec![GeomRef::Step(0), GeomRef::Step(1)],
            args: vec![],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for Loft");

        match result {
            reify_ir::GeometryOp::Loft { profiles } => {
                assert_eq!(
                    profiles,
                    vec![GeometryHandleId(100), GeometryHandleId(200)],
                    "Loft profiles should resolve Step(0) -> handle 100, Step(1) -> handle 200"
                );
            }
            other => panic!("expected GeometryOp::Loft, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_extrude_preserves_value_type() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_length(0.05))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for Extrude");

        match result {
            reify_ir::GeometryOp::Extrude { profile, distance } => {
                assert_eq!(profile, GeometryHandleId(10));
                // The distance must preserve Scalar type (not be converted to Value::Real)
                match distance {
                    reify_ir::Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!((si_value - 0.05).abs() < 1e-12, "SI value should be 0.05m");
                        assert_eq!(
                            dimension,
                            reify_core::DimensionVector::LENGTH,
                            "dimension should be LENGTH"
                        );
                    }
                    other => panic!(
                        "expected Value::Scalar, got {:?} — Extrude distance must preserve SI unit info",
                        other
                    ),
                }
            }
            other => panic!("expected GeometryOp::Extrude, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_revolve_missing_each_required_arg_returns_none() {
        // Table-driven coverage for all 7 required Revolve args. Revolve reads
        // ax, ay, az, angle, ox, oy, oz via f64_arg?; omitting any of them must
        // yield None (not silently treat as 0.0).
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Each iteration omits exactly one named arg; all other required args
        // remain present so that f64_arg? short-circuits on (and diagnoses)
        // only the omitted arg under test.
        let full_args: Vec<(&'static str, reify_ir::CompiledExpr)> = vec![
            ("ox", literal_f64(0.0)),
            ("oy", literal_f64(0.0)),
            ("oz", literal_f64(0.0)),
            ("ax", literal_f64(0.0)),
            ("ay", literal_f64(0.0)),
            ("az", literal_f64(1.0)),
            ("angle", literal_f64(std::f64::consts::PI)),
        ];

        for omit in ["ox", "oy", "oz", "ax", "ay", "az", "angle"] {
            let args: Vec<(String, reify_ir::CompiledExpr)> = full_args
                .iter()
                .filter(|(name, _)| *name != omit)
                .map(|(name, expr)| ((*name).into(), expr.clone()))
                .collect();

            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Revolve,
                profiles: vec![GeomRef::Step(0)],
                args,
            };

            // Pin the observable contract: each missing required arg
            // must (a) return None and (b) emit exactly one warning
            // diagnostic naming the quoted arg (e.g. `'ox'`) and the
            // 'Revolve' op. Covers all seven required args including `ox`.
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            let result = compile_geometry_op(
                &op,
                &values,
                &step_handles,
                &[],
                &HashMap::new(),
                &HashMap::new(),
                &mut diagnostics,
            );
            assert!(
                result.is_err(),
                "missing '{omit}' should return None, got {:?}",
                result
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "missing '{omit}' should emit exactly one diagnostic, got: {:?}",
                diagnostics
            );
            assert_eq!(
                diagnostics[0].severity,
                reify_core::Severity::Warning,
                "missing '{omit}' should emit a Warning severity"
            );
            assert!(
                diagnostics[0].message.contains(&format!("'{omit}'")),
                "diagnostic for missing '{omit}' should mention \"'{omit}'\", got: {}",
                diagnostics[0].message
            );
            assert!(
                diagnostics[0].message.contains("revolve"),
                "diagnostic for missing '{omit}' should mention 'revolve', got: {}",
                diagnostics[0].message
            );
        }
    }

    #[test]
    fn compile_geometry_op_extrude_missing_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result.is_err(),
            "expected None for missing 'distance' arg, got {:?}",
            result
        );
    }

    #[test]
    fn compile_geometry_op_extrude_nan_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Extrude with NaN distance — should return None (runtime edge case, not invariant)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_f64(f64::NAN))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(result.is_err(), "NaN extrude distance should return None");
    }

    #[test]
    fn compile_geometry_op_extrude_inf_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Extrude with Inf distance — should return None (runtime edge case, not invariant)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_f64(f64::INFINITY))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(result.is_err(), "Inf extrude distance should return None");
    }

    #[test]
    fn compile_geometry_op_extrude_near_zero_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Extrude with a near-zero (1e-15 m) distance — should return None (degenerate geometry)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_length(1e-15))],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "near-zero extrude distance should return None"
        );
        // A warning diagnostic must be emitted so model authors see why the
        // op was dropped rather than only the caller's generic error.
        assert!(
            diagnostics
                .iter()
                .any(|d| matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("extrude dropped")
                    && d.message.contains("degenerate")),
            "expected degenerate-extrude warning, got {:?}",
            diagnostics,
        );
    }

    #[test]
    fn compile_geometry_op_revolve_zero_axis_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // All 7 args present and numeric, but ax=ay=az=0.0 (zero-length rotation axis)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(0.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
            ],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "zero-length rotation axis should return None"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("revolve dropped")
                    && d.message.contains("axis")),
            "expected degenerate-revolve-axis warning, got {:?}",
            diagnostics,
        );
    }

    #[test]
    fn compile_geometry_op_revolve_nan_axis_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // All 7 args present and numeric, but ax=NaN (non-finite rotation axis)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(f64::NAN)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(0.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_err(), "NaN rotation axis should return None");
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("non-numeric/non-finite")
                    && d.message.contains("ax")
                    && d.message.contains("revolve")
            }),
            "expected a Warning mentioning 'non-numeric/non-finite', 'ax', and 'revolve', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_revolve_near_zero_angle_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Revolve with a near-zero (1e-15 rad) angle — should return None (degenerate geometry)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(1e-15)),
            ],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "near-zero revolve angle should return None"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("revolve dropped")
                    && d.message.contains("angle")),
            "expected degenerate-revolve-angle warning, got {:?}",
            diagnostics,
        );
    }

    #[test]
    fn compile_geometry_op_revolve_produces_revolve_variant() {
        let step_handles = vec![GeometryHandleId(55)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::TAU)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result =
            result.expect("compile_geometry_op should return Ok for Revolve with valid axis");

        match result {
            reify_ir::GeometryOp::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle_rad,
            } => {
                assert_eq!(profile, GeometryHandleId(55));
                assert!((axis_origin[0]).abs() < 1e-12);
                assert!((axis_origin[1]).abs() < 1e-12);
                assert!((axis_origin[2]).abs() < 1e-12);
                assert!((axis_dir[0]).abs() < 1e-12);
                assert!((axis_dir[1]).abs() < 1e-12);
                assert!((axis_dir[2] - 1.0).abs() < 1e-12);
                assert!((angle_rad - std::f64::consts::TAU).abs() < 1e-12);
            }
            other => panic!("expected GeometryOp::Revolve, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_extrude_produces_extrude_variant() {
        let step_handles = vec![GeometryHandleId(77)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_length(0.03))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for Extrude");

        match result {
            reify_ir::GeometryOp::Extrude { profile, distance } => {
                assert_eq!(profile, GeometryHandleId(77));
                match distance {
                    reify_ir::Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!(
                            (si_value - 0.03).abs() < 1e-12,
                            "SI value should be 0.03m (30mm)"
                        );
                        assert_eq!(dimension, reify_core::DimensionVector::LENGTH);
                    }
                    other => panic!("expected Value::Scalar for distance, got {:?}", other),
                }
            }
            other => panic!("expected GeometryOp::Extrude, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_scale_negative_factor_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(-1.0))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "negative scale factor should return None (inside-out geometry)"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for negative scale factor, got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("scale dropped")
                    && d.message.contains("negative")
            }),
            "expected a Warning mentioning 'scale dropped' and 'negative', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_scale_zero_factor_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(0.0))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "zero scale factor should return None (degenerate geometry)"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for zero scale factor, got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("scale dropped")
                    && d.message.contains("degenerate")
            }),
            "expected a Warning mentioning 'scale dropped' and 'degenerate', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_translate_missing_arg_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // Translate with only dx — missing dy, dz
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![("dx".into(), literal_f64(1.0))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result.is_err(),
            "missing dy/dz should return None, not silently default to 0.0"
        );
    }

    #[test]
    fn compile_geometry_op_scale_nan_factor_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(f64::NAN))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_err(), "NaN scale factor should return None");
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for NaN scale factor, got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("non-numeric/non-finite")
                    && d.message.contains("factor")
                    && d.message.contains("scale")
            }),
            "expected a Warning mentioning 'non-numeric/non-finite', 'factor', and 'scale', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_rotate_around_missing_axis_returns_none() {
        let step_handles = vec![GeometryHandleId(99)];
        let values = ValueMap::new();

        // RotateAround with missing az
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::RotateAround,
            target: GeomRef::Step(0),
            args: vec![
                ("px".into(), literal_f64(0.0)),
                ("py".into(), literal_f64(0.0)),
                ("pz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(1.0)),
                // az deliberately omitted
                ("angle".into(), literal_f64(1.0)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(result.is_err(), "missing az should return Err");
    }

    #[test]
    fn compile_geometry_op_linear_pattern_missing_spacing_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // LinearPattern with dx/dy/dz/count but OMITS spacing
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                // spacing deliberately omitted
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result.is_err(),
            "missing spacing should return None, not silently default to Value::Undef"
        );
    }

    #[test]
    fn compile_geometry_op_circular_pattern_missing_angle_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // CircularPattern with ox/oy/oz/ax/ay/az/count but OMITS angle
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(4.0)),
                // angle deliberately omitted
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result.is_err(),
            "missing angle should return None, not silently default to Value::Undef"
        );
    }

    #[test]
    fn compile_geometry_op_linear_pattern_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                ("spacing".into(), literal_length(0.02)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::LinearPattern {
                target,
                direction,
                count,
                spacing,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(direction, [10.0, 0.0, 0.0]);
                assert_eq!(count, 3);
                // spacing should be a Scalar value, not Undef
                assert!(
                    !matches!(spacing, reify_ir::Value::Undef),
                    "spacing should not be Undef when arg is present"
                );
            }
            other => panic!("expected Some(LinearPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_circular_pattern_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(4.0)),
                // Use an explicitly-dimensioned angle literal to test the pass-through path.
                // A bare f64 would now trigger the degrees→radians conversion path instead.
                ("angle".into(), literal_angle(std::f64::consts::FRAC_PI_2)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        match result {
            Ok(reify_ir::GeometryOp::CircularPattern {
                target,
                axis_origin,
                axis_dir,
                count,
                angle,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(axis_origin, [0.0, 0.0, 0.0]);
                assert_eq!(axis_dir, [0.0, 0.0, 1.0]);
                assert_eq!(count, 4);
                // angle should be a Scalar value (with ANGLE dimension), not Undef
                assert!(
                    !matches!(angle, reify_ir::Value::Undef),
                    "angle should not be Undef when arg is present"
                );
                // The explicit-unit path must NOT emit a degree-conversion warning
                let has_deg_warning = diagnostics.iter().any(|d| {
                    d.severity == reify_core::Severity::Warning
                        && (d.message.contains("deg") || d.message.contains("degree"))
                });
                assert!(
                    !has_deg_warning,
                    "explicit angle unit should not trigger a degree-conversion warning, got: {:?}",
                    diagnostics
                );
            }
            other => panic!("expected Some(CircularPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_circular_pattern_bare_f64_converts_to_radians() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = reify_compiler::CompiledGeometryOp::Pattern {
            kind: reify_compiler::PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(6.0)),
                // Bare f64 without unit — should be interpreted as degrees and
                // converted to radians: 360° → 2π rad.
                ("angle".into(), literal_f64(360.0)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::CircularPattern { angle, .. }) => {
                let angle_f64 = angle.as_f64().expect("angle should be numeric");
                assert!(
                    (angle_f64 - std::f64::consts::TAU).abs() < 1e-9,
                    "360.0 (bare f64) should convert to 2π radians, got {}",
                    angle_f64
                );
            }
            other => panic!("expected Some(CircularPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_circular_pattern_bare_int_converts_to_radians() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // Bare integer 360 — should be interpreted as 360° and converted to 2π rad.
        let angle_int_expr =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Int(360), reify_core::Type::Int);

        let op = reify_compiler::CompiledGeometryOp::Pattern {
            kind: reify_compiler::PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(6.0)),
                ("angle".into(), angle_int_expr),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::CircularPattern { angle, .. }) => {
                let angle_f64 = angle.as_f64().expect("angle should be numeric");
                assert!(
                    (angle_f64 - std::f64::consts::TAU).abs() < 1e-9,
                    "Int(360) should convert to 2π radians, got {}",
                    angle_f64
                );
            }
            other => panic!("expected Some(CircularPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_circular_pattern_bare_number_emits_deprecation_warning() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let op = reify_compiler::CompiledGeometryOp::Pattern {
            kind: reify_compiler::PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(6.0)),
                ("angle".into(), literal_f64(360.0)),
            ],
        };

        let _result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let has_degree_warning = diagnostics.iter().any(|d| {
            d.severity == reify_core::Severity::Warning
                && (d.message.contains("deg") || d.message.contains("degree"))
        });
        assert!(
            has_degree_warning,
            "expected a Warning diagnostic about implicit degree conversion, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_circular_pattern_angle_scalar_passes_through() {
        // An explicitly-dimensioned angle (Value::Scalar with ANGLE dimension) must
        // pass through the CircularPattern arm unchanged — no double-conversion,
        // no degree-conversion warning.
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let op = reify_compiler::CompiledGeometryOp::Pattern {
            kind: reify_compiler::PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(6.0)),
                // Explicit angle unit: PI radians
                ("angle".into(), literal_angle(std::f64::consts::PI)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        match result {
            Ok(reify_ir::GeometryOp::CircularPattern { angle, .. }) => {
                let angle_f64 = angle.as_f64().expect("angle should be numeric");
                assert!(
                    (angle_f64 - std::f64::consts::PI).abs() < 1e-12,
                    "explicit PI rad angle should pass through as PI, got {}",
                    angle_f64
                );
                // No degree-conversion warning should be emitted for explicit units
                let has_deg_warning = diagnostics.iter().any(|d| {
                    d.severity == reify_core::Severity::Warning
                        && (d.message.contains("deg") || d.message.contains("degree"))
                });
                assert!(
                    !has_deg_warning,
                    "explicit angle unit should not trigger a degree-conversion warning, got: {:?}",
                    diagnostics
                );
            }
            other => panic!("expected Some(CircularPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_mirror_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Mirror,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("nx".into(), literal_f64(1.0)),
                ("ny".into(), literal_f64(0.0)),
                ("nz".into(), literal_f64(0.0)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::Mirror {
                target,
                plane_origin,
                plane_normal,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(plane_origin, [0.0, 0.0, 0.0]);
                assert_eq!(plane_normal, [1.0, 0.0, 0.0]);
            }
            other => panic!("expected Some(Mirror), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_linear_pattern_2d_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear2D,
            target: GeomRef::Step(0),
            args: vec![
                ("dx1".into(), literal_f64(1.0)),
                ("dy1".into(), literal_f64(0.0)),
                ("dz1".into(), literal_f64(0.0)),
                ("count1".into(), literal_f64(3.0)),
                ("spacing1".into(), literal_length(0.02)),
                ("dx2".into(), literal_f64(0.0)),
                ("dy2".into(), literal_f64(1.0)),
                ("dz2".into(), literal_f64(0.0)),
                ("count2".into(), literal_f64(4.0)),
                ("spacing2".into(), literal_length(0.03)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::LinearPattern2D {
                target,
                direction1,
                count1,
                spacing1,
                direction2,
                count2,
                spacing2,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(direction1, [1.0, 0.0, 0.0]);
                assert_eq!(count1, 3);
                assert!(
                    !matches!(spacing1, reify_ir::Value::Undef),
                    "spacing1 should not be Undef"
                );
                assert_eq!(direction2, [0.0, 1.0, 0.0]);
                assert_eq!(count2, 4);
                assert!(
                    !matches!(spacing2, reify_ir::Value::Undef),
                    "spacing2 should not be Undef"
                );
            }
            other => panic!("expected Some(LinearPattern2D), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_arbitrary_pattern_valid_3_transforms() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Arbitrary,
            target: GeomRef::Step(0),
            args: vec![
                ("t0_dx".into(), literal_f64(0.01)),
                ("t0_dy".into(), literal_f64(0.0)),
                ("t0_dz".into(), literal_f64(0.0)),
                ("t1_dx".into(), literal_f64(0.0)),
                ("t1_dy".into(), literal_f64(0.02)),
                ("t1_dz".into(), literal_f64(0.0)),
                ("t2_dx".into(), literal_f64(0.01)),
                ("t2_dy".into(), literal_f64(0.02)),
                ("t2_dz".into(), literal_f64(0.0)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::ArbitraryPattern { target, transforms }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(transforms.len(), 3);
                assert_eq!(transforms[0], [0.01, 0.0, 0.0]);
                assert_eq!(transforms[1], [0.0, 0.02, 0.0]);
                assert_eq!(transforms[2], [0.01, 0.02, 0.0]);
            }
            other => panic!("expected Some(ArbitraryPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_linear_pattern_2d_missing_spacing2_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear2D,
            target: GeomRef::Step(0),
            args: vec![
                ("dx1".into(), literal_f64(1.0)),
                ("dy1".into(), literal_f64(0.0)),
                ("dz1".into(), literal_f64(0.0)),
                ("count1".into(), literal_f64(3.0)),
                ("spacing1".into(), literal_length(0.02)),
                ("dx2".into(), literal_f64(0.0)),
                ("dy2".into(), literal_f64(1.0)),
                ("dz2".into(), literal_f64(0.0)),
                ("count2".into(), literal_f64(4.0)),
                // spacing2 deliberately omitted
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(result.is_err(), "missing spacing2 should return None");
    }

    #[test]
    fn compile_geometry_op_arbitrary_pattern_missing_transform_coord_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // Only 2 coords for what should be a complete triple
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Arbitrary,
            target: GeomRef::Step(0),
            args: vec![
                ("t0_dx".into(), literal_f64(0.01)),
                ("t0_dy".into(), literal_f64(0.0)),
                // t0_dz deliberately omitted
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result.is_err(),
            "missing transform coord should return None"
        );
    }

    // ── compile_geometry_op diagnostic tests ─────────────────────────────────

    #[test]
    fn compile_geometry_op_primitive_missing_arg_returns_none() {
        let step_handles: Vec<GeometryHandleId> = vec![];
        let values = ValueMap::new();

        // Box with height and depth present, but 'width' deliberately omitted
        let op = CompiledGeometryOp::Primitive {
            kind: reify_compiler::PrimitiveKind::Box,
            args: vec![
                ("height".into(), literal_length(0.05)),
                ("depth".into(), literal_length(0.04)),
                // width deliberately omitted
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // When a required arg is missing, compile_geometry_op should short-circuit and return None
        assert!(
            result.is_err(),
            "compile_geometry_op should return None when a required arg is missing"
        );

        // Exactly one diagnostic warning should have been emitted for the missing 'width'
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for missing 'width', got: {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "expected Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("width"),
            "diagnostic message should mention 'width', got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("box"),
            "diagnostic message should mention 'box', got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn compile_geometry_op_modify_missing_arg_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Fillet with target but 'radius' deliberately omitted
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                // radius deliberately omitted
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // When a required arg is missing, compile_geometry_op should short-circuit and return None
        assert!(
            result.is_err(),
            "compile_geometry_op should return None when a required arg is missing"
        );

        // Exactly one diagnostic warning should have been emitted for the missing 'radius'
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for missing 'radius', got: {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "expected Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("radius"),
            "diagnostic message should mention 'radius', got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("fillet"),
            "diagnostic message should mention 'fillet', got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn compile_geometry_op_present_args_emit_no_diagnostics() {
        let step_handles = vec![GeometryHandleId(1)];
        let values = ValueMap::new();

        // Primitive::Box with all required args present
        let box_op = CompiledGeometryOp::Primitive {
            kind: reify_compiler::PrimitiveKind::Box,
            args: vec![
                ("width".into(), literal_length(0.10)),
                ("height".into(), literal_length(0.05)),
                ("depth".into(), literal_length(0.04)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &box_op,
            &values,
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_ok(), "Box with all args should return Some");
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected when all Primitive args are present, got: {:?}",
            diagnostics
        );

        // Modify::Fillet with target and radius present
        let fillet_op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![("radius".into(), literal_length(0.005))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &fillet_op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_ok(), "Fillet with all args should return Some");
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected when all Modify args are present, got: {:?}",
            diagnostics
        );
    }

    // ── Fillet eval-arm: anti-zero-edges + 2-arg back-compat (task 3205 step-9/10) ──

    /// Build a `CompiledExpr` literal that evaluates to an empty `Value::List`
    /// — a present-but-empty edge selector. Drives the anti-zero-edges
    /// (E_EMPTY_SELECTION) eval-arm path.
    fn empty_list_literal() -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(vec![]),
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        )
    }

    /// (a) ANTI-ZERO-EDGES: a 3-arg Fillet whose `edges` arg is PRESENT but
    /// evaluates to an empty `Value::List` must NOT silently fall through to
    /// the all-edges path. `compile_geometry_op` returns `Err`, pushes exactly
    /// one diagnostic carrying `DiagnosticCode::EmptyEdgeSelection`, and
    /// produces NO `GeometryOp::Fillet`. Closes the task-3295 fake-done trap.
    #[test]
    fn compile_geometry_op_fillet_empty_edge_selection_errors_with_code() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // 3-arg form: args carry "target" (the solid expr), an "edges" selector
        // that evaluates to Value::List(vec![]), and "radius".
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("edges".into(), empty_list_literal()),
                ("radius".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "a present edge selector resolving to zero edges must Err (never \
             fall through to all-edges), got {:?}",
            result
        );
        let empty_sel: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.code == Some(reify_core::DiagnosticCode::EmptyEdgeSelection))
            .collect();
        assert_eq!(
            empty_sel.len(),
            1,
            "expected exactly one EmptyEdgeSelection diagnostic, got diagnostics: {:?}",
            diagnostics
        );
    }

    /// (b) 2-arg back-compat: a Fillet with NO `edges` arg lowers to
    /// `GeometryOp::Fillet{edges: vec![], ..}` (the all-edges path) with NO
    /// `EmptyEdgeSelection` diagnostic — "no selector" is legitimately
    /// all-edges, distinct from "selector present but empty".
    #[test]
    fn compile_geometry_op_fillet_2arg_no_edges_arg_is_all_edges_back_compat() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("radius".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Fillet { target, edges, .. }) => {
                assert_eq!(
                    target,
                    GeometryHandleId(10),
                    "target must resolve via Step(0)"
                );
                assert!(
                    edges.is_empty(),
                    "2-arg fillet (no edges arg) must lower to empty edges \
                     (all-edges back-compat), got {:?}",
                    edges
                );
            }
            other => panic!(
                "expected Ok(GeometryOp::Fillet) for 2-arg fillet, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "2-arg fillet must NOT emit an EmptyEdgeSelection diagnostic, got: {:?}",
            diagnostics
        );
    }

    /// (c) INTERMEDIATE UX: on the legacy pipeline a 3-arg `fillet(solid, edges,
    /// radius)` reaches this eval arm with the `edges` selector still UNRESOLVED
    /// (runtime `Value::Undef` — the selector resolves in P4, after this P2 arm).
    /// That is NOT an empty selection, so the arm must NOT emit
    /// `EmptyEdgeSelection`; instead it returns a USER-ACTIONABLE `Err` (surfaced
    /// verbatim as `failed to compile geometry operation: <msg>`), not the old
    /// internal "did not resolve to a List" string. This pins the staging UX
    /// until engine-unified-build-dag η/ε (tasks 4360/4358) make curated
    /// selection reachable end-to-end. (Reviewer test_coverage note, task 3205.)
    #[test]
    fn compile_geometry_op_fillet_legacy_selector_unresolved_is_user_actionable() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // 3-arg form whose "edges" selector evaluates to `Value::Undef` — the
        // legacy-pipeline state where the selector has not yet resolved. Its
        // STATIC type is `List<Geometry>`, but its runtime value is `Undef`.
        let unresolved_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Undef,
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("edges".into(), unresolved_selector),
                ("radius".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "an unresolved legacy edge selector must Err (stays Undef for η \
                 to resolve in-loop), got Ok({:?})",
                other
            ),
        };
        // User-actionable: names the call form, points at the 2-arg fallback,
        // and does NOT leak the old internal "did not resolve to a List" string.
        assert!(
            msg.contains("fillet(solid, edges, radius)"),
            "diagnostic must name the 3-arg call form, got: {msg:?}"
        );
        assert!(
            msg.contains("2-arg fillet(solid, radius)"),
            "diagnostic must point the user at the 2-arg all-edges fallback, got: {msg:?}"
        );
        assert!(
            !msg.contains("did not resolve to a List"),
            "diagnostic must not surface the raw internal 'did not resolve to a \
             List' string, got: {msg:?}"
        );
        // The deferral is preserved: an unresolved selector is NOT an empty
        // selection, so it must NEVER trip the anti-zero-edges guard.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "an unresolved (non-List) selector must NOT emit EmptyEdgeSelection \
             (that would false-positive on every legacy 3-arg fillet), got: {:?}",
            diagnostics
        );
    }

    /// (d) MALFORMED ELEMENT: a 3-arg Fillet whose `edges` selector resolves to a
    /// List containing a NON-handle element must `Err` on the bad element rather
    /// than silently filleting only the surviving handle subset. This mirrors
    /// `resolve_subhandle_list`'s reject-non-handle strictness so the eval arm
    /// and the full resolver share one validation policy. The malformed case is
    /// distinct from an EMPTY selection, so it must NOT trip EmptyEdgeSelection.
    /// (Reviewer robustness note, task 3205.)
    #[test]
    fn compile_geometry_op_fillet_malformed_element_errors_not_empty_selection() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // "edges" resolves to a List with a non-handle element (a bare Real) —
        // a partially-malformed selector. The old `filter_map` would have
        // silently dropped it; the strict arm errors on it.
        let malformed_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(vec![reify_ir::Value::Real(1.0)]),
            reify_core::Type::List(Box::new(reify_core::Type::dimensionless_scalar())),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("edges".into(), malformed_selector),
                ("radius".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "a selector with a non-handle element must Err (never silently \
                 fillet the surviving subset), got Ok({:?})",
                other
            ),
        };
        assert!(
            msg.contains("not a Geometry sub-handle"),
            "diagnostic must flag the non-handle element, got: {msg:?}"
        );
        // A malformed element is NOT an empty selection — it must error on the
        // element, never reach (and so never trip) the anti-zero-edges guard.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a malformed-element selector must NOT emit EmptyEdgeSelection, got: {:?}",
            diagnostics
        );
    }

    // ── Draft eval-arm: faces resolution + anti-zero + 3-arg back-compat ──

    /// Helper: build a `Value::GeometryHandle` sub-handle with the given
    /// kernel handle id, using a fixed test realization_ref and hash.
    fn geometry_handle_value(kernel_handle: GeometryHandleId) -> reify_ir::Value {
        reify_ir::Value::GeometryHandle {
            realization_ref: reify_core::identity::RealizationNodeId::new("test-solid", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle,
        }
    }

    /// Helper: build a `CompiledExpr` literal that evaluates to a
    /// `Value::List` of `Value::GeometryHandle` sub-handles.
    fn geometry_handle_list_literal(handles: Vec<GeometryHandleId>) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(handles.into_iter().map(geometry_handle_value).collect()),
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        )
    }

    /// (a) 4-arg draft: a "faces" selector that evaluates to a List of
    /// `Value::GeometryHandle` sub-handles threads the canonical face ids
    /// (ascending kernel_handle order, deduped) onto
    /// `GeometryOp::Draft.faces`. Supplies two handles in REVERSE order so
    /// the canonical-sort is observable (h7 < h42 → sorted [7, 42]).
    #[test]
    fn compile_geometry_op_draft_4arg_faces_threads_canonical_handles() {
        // step_handles[0] = target solid; step_handles.last() = plane
        // (the Draft eval arm resolves the plane via step_handles.last()).
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                (
                    "faces".into(),
                    geometry_handle_list_literal(vec![
                        GeometryHandleId(42),
                        GeometryHandleId(7),
                    ]),
                ),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Draft { target, faces, .. }) => {
                assert_eq!(
                    target,
                    GeometryHandleId(10),
                    "target must resolve via Step(0)"
                );
                assert_eq!(
                    faces,
                    vec![GeometryHandleId(7), GeometryHandleId(42)],
                    "faces must be canonically sorted (ascending kernel_handle id), \
                     got {:?}",
                    faces
                );
            }
            other => panic!(
                "expected Ok(GeometryOp::Draft) for 4-arg draft with curated faces, \
                 got {:?}",
                other
            ),
        }
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a curated-faces draft must NOT emit EmptyEdgeSelection, got: {:?}",
            diagnostics
        );
    }

    /// (b) ANTI-ZERO-FACES: a 4-arg Draft whose "faces" selector is PRESENT
    /// but evaluates to an empty List must NOT silently fall through to the
    /// all-faces path. `compile_geometry_op` returns `Err` and pushes exactly
    /// one diagnostic carrying `DiagnosticCode::EmptyEdgeSelection`.
    /// Closes the task-3295 fake-done trap for Draft.
    #[test]
    fn compile_geometry_op_draft_empty_face_selection_errors_with_code() {
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("faces".into(), empty_list_literal()),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "a present face selector resolving to zero faces must Err (never \
             fall through to all-faces), got {:?}",
            result
        );
        let empty_sel: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.code == Some(reify_core::DiagnosticCode::EmptyEdgeSelection))
            .collect();
        assert_eq!(
            empty_sel.len(),
            1,
            "expected exactly one EmptyEdgeSelection diagnostic, got diagnostics: {:?}",
            diagnostics
        );
    }

    /// (c) 3-arg back-compat: a Draft with NO "faces" arg lowers to
    /// `GeometryOp::Draft{faces: vec![], ..}` (the all-faces path) with NO
    /// `EmptyEdgeSelection` diagnostic — "no selector" is legitimately
    /// all-draftable-faces, distinct from "selector present but empty".
    #[test]
    fn compile_geometry_op_draft_3arg_no_faces_arg_is_all_faces_back_compat() {
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Draft { target, faces, .. }) => {
                assert_eq!(
                    target,
                    GeometryHandleId(10),
                    "target must resolve via Step(0)"
                );
                assert!(
                    faces.is_empty(),
                    "3-arg draft (no faces arg) must lower to empty faces \
                     (all-faces back-compat), got {:?}",
                    faces
                );
            }
            other => panic!(
                "expected Ok(GeometryOp::Draft) for 3-arg draft, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "3-arg draft must NOT emit an EmptyEdgeSelection diagnostic, got: {:?}",
            diagnostics
        );
    }

    /// (d) MALFORMED ELEMENT: a 4-arg Draft whose "faces" selector resolves
    /// to a List containing a NON-handle element must `Err` on the bad
    /// element rather than silently drafting only the surviving handle
    /// subset. A malformed element is distinct from an empty selection, so
    /// it must NOT trip EmptyEdgeSelection.
    #[test]
    fn compile_geometry_op_draft_malformed_element_errors_not_empty_selection() {
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        // "faces" resolves to a List with a non-handle element (a bare Real)
        let malformed_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(vec![reify_ir::Value::Real(1.0)]),
            reify_core::Type::List(Box::new(reify_core::Type::dimensionless_scalar())),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("faces".into(), malformed_selector),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "a selector with a non-handle element must Err (never silently \
                 draft the surviving subset), got Ok({:?})",
                other
            ),
        };
        assert!(
            msg.contains("not a Geometry sub-handle"),
            "diagnostic must flag the non-handle element, got: {msg:?}"
        );
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a malformed-element selector must NOT emit EmptyEdgeSelection, got: {:?}",
            diagnostics
        );
    }

    /// (e) NON-LIST SELECTOR: a 4-arg Draft whose "faces" selector evaluates
    /// to a non-List value (e.g., `Value::Undef` on the legacy pipeline)
    /// must return a user-actionable `Err` and must NOT emit
    /// `EmptyEdgeSelection` (that would false-positive on every legacy miss).
    #[test]
    fn compile_geometry_op_draft_legacy_selector_unresolved_is_user_actionable() {
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        // "faces" evaluates to `Value::Undef` — the legacy-pipeline state
        // where the selector has not yet resolved.
        let unresolved_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Undef,
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("faces".into(), unresolved_selector),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "an unresolved (non-List) faces selector must Err (stays \
                 Undef for future in-loop resolution), got Ok({:?})",
                other
            ),
        };
        // User-actionable: names the 4-arg call form and points at the
        // 3-arg all-faces fallback.
        assert!(
            msg.contains("draft(solid, faces, angle, neutral_plane)"),
            "diagnostic must name the 4-arg call form, got: {msg:?}"
        );
        assert!(
            msg.contains("3-arg draft(solid, angle, neutral_plane)"),
            "diagnostic must point the user at the 3-arg all-faces fallback, \
             got: {msg:?}"
        );
        // A non-List is NOT an empty selection — must never trip anti-zero
        // guard.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "an unresolved (non-List) selector must NOT emit EmptyEdgeSelection, \
             got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_transform_pattern_sweep_present_args_emit_no_diagnostics() {
        let step_handles = vec![GeometryHandleId(1)];
        let values = ValueMap::new();

        // Transform::Translate — all three required args present
        let translate_op = CompiledGeometryOp::Transform {
            kind: reify_compiler::TransformKind::Translate,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(1.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
            ],
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &translate_op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_ok(), "Translate with all args should return Some");
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for Translate with all args, got: {:?}",
            diagnostics
        );

        // Pattern::LinearPattern — all required args present
        let linear_op = CompiledGeometryOp::Pattern {
            kind: reify_compiler::PatternKind::Linear,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                ("spacing".into(), literal_length(0.02)),
            ],
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &linear_op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_ok(),
            "LinearPattern with all args should return Some"
        );
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for LinearPattern with all args, got: {:?}",
            diagnostics
        );

        // Sweep::Extrude — distance present
        let extrude_op = CompiledGeometryOp::Sweep {
            kind: reify_compiler::SweepKind::Extrude,
            profiles: vec![reify_compiler::GeomRef::Step(0)],
            args: vec![("distance".into(), literal_length(0.05))],
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &extrude_op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_ok(), "Extrude with all args should return Some");
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for Extrude with all args, got: {:?}",
            diagnostics
        );

        // Sweep::Revolve — all seven args present with a valid axis
        let revolve_op = CompiledGeometryOp::Sweep {
            kind: reify_compiler::SweepKind::Revolve,
            profiles: vec![reify_compiler::GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
            ],
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &revolve_op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_ok(), "Revolve with all args should return Some");
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for Revolve with all args, got: {:?}",
            diagnostics
        );
    }

    // ── missing-arg diagnostic tests for Transform/Pattern/Sweep ─────────────

    #[test]
    fn compile_geometry_op_sweep_extrude_missing_distance_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Extrude with no args at all — 'distance' is missing
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![reify_compiler::GeomRef::Step(0)],
            args: vec![],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // Still returns None
        assert!(
            result.is_err(),
            "missing 'distance' should still return None, got {:?}",
            result
        );

        // Exactly one diagnostic warning for the missing 'distance' arg
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for missing 'distance', got: {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "expected Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("distance"),
            "diagnostic message should mention 'distance', got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("extrude")
                && !diagnostics[0].message.contains("extrude_"),
            "diagnostic message should mention 'extrude' but not any underscore-suffixed sibling (extrude_*), got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn compile_geometry_op_pattern_linear_missing_spacing_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // LinearPattern with dx/dy/dz/count but OMITS spacing
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                // spacing deliberately omitted
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // Still returns None (Pattern short-circuits on missing args)
        assert!(
            result.is_err(),
            "missing spacing should still return None, got {:?}",
            result
        );

        // Exactly one diagnostic warning for the missing 'spacing' arg
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for missing 'spacing', got: {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "expected Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("spacing"),
            "diagnostic message should mention 'spacing', got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("linear")
                && !diagnostics[0].message.contains("linear_"),
            "diagnostic message should mention 'linear' but not any underscore-suffixed sibling (linear_*), got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn compile_geometry_op_transform_translate_missing_arg_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // Translate with only dx — missing dy, dz
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![("dx".into(), literal_f64(1.0))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // Still returns None (Transform short-circuits on missing f64 args)
        assert!(
            result.is_err(),
            "missing dy/dz should still return None, got {:?}",
            result
        );

        // But now exactly one diagnostic warning should be emitted for the first missing arg 'dy'
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for missing 'dy', got: {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "expected Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("dy"),
            "diagnostic message should mention 'dy', got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("translate"),
            "diagnostic message should mention 'translate', got: {}",
            diagnostics[0].message
        );
    }

    // ── non-numeric/non-finite diagnostic tests ──────────────────────────────

    #[test]
    fn compile_geometry_op_translate_wrong_type_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // dx is a String value, not a numeric f64 — should trigger a non-numeric diagnostic
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![
                (
                    "dx".into(),
                    reify_ir::CompiledExpr::literal(
                        reify_ir::Value::String("oops".into()),
                        reify_core::Type::String,
                    ),
                ),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "wrong-type dx should return None, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("non-numeric/non-finite")
                    && d.message.contains("dx")
                    && d.message.contains("translate")
            }),
            "expected a Warning mentioning 'non-numeric/non-finite', 'dx', and 'translate', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_translate_nan_dx_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // dx is NaN — non-finite, should trigger a diagnostic
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(f64::NAN)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "NaN dx should return None, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("non-numeric/non-finite")
                    && d.message.contains("dx")
                    && d.message.contains("translate")
            }),
            "expected a Warning mentioning 'non-numeric/non-finite', 'dx', and 'translate', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_translate_infinity_dx_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // dx is +Infinity — non-finite, should trigger a diagnostic
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(f64::INFINITY)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "Infinity dx should return None, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("non-numeric/non-finite")
                    && d.message.contains("dx")
                    && d.message.contains("translate")
            }),
            "expected a Warning mentioning 'non-numeric/non-finite', 'dx', and 'translate', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_translate_finite_args_no_false_positive_warning() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // All finite args — should succeed with no non-numeric/non-finite warning
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(1.0)),
                ("dy".into(), literal_f64(2.0)),
                ("dz".into(), literal_f64(3.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "finite Translate args should return Some, got None; diagnostics: {:?}",
            diagnostics
        );
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("non-numeric/non-finite")),
            "no 'non-numeric/non-finite' warning expected for finite args, got: {:?}",
            diagnostics
        );
    }

    // ---------------------------------------------------------------------------
    // Tests: INVALID sentinel preserves step index alignment (task-612, step-9)
    // ---------------------------------------------------------------------------

    /// Verifies that an INVALID sentinel at step index 1 does not shift subsequent
    /// valid handles. With step_handles = [42, INVALID, 100]:
    /// - Boolean(Step(0), Step(2)) → Some(Union { left: 42, right: 100 })
    ///   Step(0) resolves to 42 and Step(2) resolves to 100, both correct.
    ///   The INVALID at index 1 is skipped; indices ≥ 2 are unaffected.
    /// - Boolean(Step(0), Step(1)) → None
    ///   Step(1) is INVALID, filtered out by the sentinel check, so the op fails.
    ///
    /// Together these two assertions confirm that:
    /// (a) the sentinel at index 1 maintains index alignment for subsequent handles,
    /// (b) the INVALID value correctly blocks resolution of its own index.
    #[test]
    fn compile_geometry_op_invalid_sentinel_preserves_index_alignment() {
        use reify_compiler::BooleanOp;
        let values = ValueMap::new();

        // step_handles[0] = 42 (valid sphere handle)
        // step_handles[1] = INVALID (sentinel for a failed op)
        // step_handles[2] = 100 (valid handle — must remain at index 2)
        let step_handles = vec![
            GeometryHandleId(42),
            GeometryHandleId::INVALID,
            GeometryHandleId(100),
        ];

        // (a) Union(Step(0), Step(2)): both resolve correctly despite sentinel at index 1
        let op_ok = CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(0),
            right: GeomRef::Step(2),
        };
        let result_ok = compile_geometry_op(
            &op_ok,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result_ok = result_ok
            .expect("Boolean(Step(0), Step(2)) should succeed: both indices hold valid handles");
        match result_ok {
            reify_ir::GeometryOp::Union { left, right } => {
                assert_eq!(
                    left,
                    GeometryHandleId(42),
                    "Step(0) should resolve to handle 42 (not shifted by sentinel at index 1)"
                );
                assert_eq!(
                    right,
                    GeometryHandleId(100),
                    "Step(2) should resolve to handle 100 (aligned correctly despite sentinel at 1)"
                );
            }
            other => panic!(
                "expected GeometryOp::Union from Boolean(Step(0), Step(2)), got {:?}",
                other
            ),
        }

        // (b) Union(Step(0), Step(1)): Step(1) is INVALID → filtered out → returns None
        let op_fail = CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(0),
            right: GeomRef::Step(1),
        };
        let result_fail = compile_geometry_op(
            &op_fail,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result_fail.is_err(),
            "Boolean(Step(0), Step(1)) should return Err: Step(1) is INVALID and filtered out"
        );
    }

    // ── Shell face index validation tests ────────────────────────────────────

    #[test]
    fn compile_geometry_op_shell_non_numeric_face_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 is a String value — should trigger a non-numeric diagnostic
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                (
                    "face_0".into(),
                    reify_ir::CompiledExpr::literal(
                        reify_ir::Value::String("oops".into()),
                        reify_core::Type::String,
                    ),
                ),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // Shell op itself should still succeed (non-numeric face is skipped)
        assert!(
            result.is_ok(),
            "Shell should return Some even when face_0 is non-numeric, got {:?}",
            result
        );
        // The bad face should produce a diagnostic mentioning 'non-numeric'
        // (precision assertion — that it does NOT say 'non-finite' — lives in the dedicated
        // compile_geometry_op_shell_string_face_diagnostic_excludes_non_finite test)
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("non-numeric")
            }),
            "expected a Warning mentioning 'face_0' and 'non-numeric', got: {:?}",
            diagnostics
        );
        // The resulting faces_to_remove should be empty (bad face skipped)
        match result.unwrap() {
            reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            } => {
                assert!(
                    faces_to_remove.is_empty(),
                    "faces_to_remove should be empty when face_0 is non-numeric, got {:?}",
                    faces_to_remove
                );
            }
            other => panic!("expected GeometryOp::Shell, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_shell_bool_face_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_1 is a Bool value — should trigger a non-numeric diagnostic
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                (
                    "face_1".into(),
                    reify_ir::CompiledExpr::literal(
                        reify_ir::Value::Bool(true),
                        reify_core::Type::Bool,
                    ),
                ),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell should return Some even when face_1 is Bool, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_1")
                    && d.message.contains("non-numeric")
                    && !d.message.contains("non-finite")
            }),
            "expected a Warning mentioning 'face_1' and 'non-numeric' (not 'non-finite'), got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_shell_negative_face_index_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = -1.0 — would wrap to usize::MAX without the guard
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(-1.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell should return Some even when face_0 is negative, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("negative")
                    && !d.message.contains("non-finite")
            }),
            "expected a Warning mentioning 'face_0' and 'negative' (not 'non-finite'), got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            } => {
                assert!(
                    faces_to_remove.is_empty(),
                    "faces_to_remove should be empty when face_0 is -1.0, got {:?}",
                    faces_to_remove
                );
            }
            other => panic!("expected GeometryOp::Shell, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_shell_nan_face_index_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = NaN — non-finite
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(f64::NAN)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with NaN face_0 should return Some, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("non-finite")
                    && !d.message.contains("negative")
            }),
            "expected a Warning mentioning 'non-finite' (not 'negative') for NaN face_0, got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            } => {
                assert!(
                    faces_to_remove.is_empty(),
                    "faces_to_remove should be empty for NaN face_0, got {:?}",
                    faces_to_remove
                );
            }
            other => panic!("expected GeometryOp::Shell, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_shell_infinity_face_index_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = +Infinity — non-finite
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(f64::INFINITY)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with INFINITY face_0 should return Some, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("non-finite")
                    && !d.message.contains("negative")
            }),
            "expected a Warning mentioning 'non-finite' (not 'negative') for INFINITY face_0, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_shell_valid_faces_no_false_positive() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // All three faces are valid non-negative integers — no diagnostics should be emitted
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(0.0)),
                ("face_1".into(), literal_f64(2.0)),
                ("face_2".into(), literal_f64(5.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            }) => {
                assert_eq!(
                    faces_to_remove,
                    vec![0usize, 2, 5],
                    "valid faces should all be collected correctly"
                );
            }
            other => panic!("expected Some(Shell), got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "valid faces should produce no diagnostics, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_shell_fractional_face_index_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = 2.7 — non-integer, should emit diagnostic and be skipped (not truncated to 2)
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(2.7)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with fractional face_0 should return Some, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && (d.message.contains("integer") || d.message.contains("fractional"))
            }),
            "expected a Warning about non-integer face_0, got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            } => {
                assert!(
                    faces_to_remove.is_empty(),
                    "fractional face index should be skipped (not truncated), got {:?}",
                    faces_to_remove
                );
            }
            other => panic!("expected GeometryOp::Shell, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_shell_huge_face_index_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = 2e18 — far exceeds upper bound; Rust saturates f64→usize to usize::MAX
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(2e18)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with huge face_0 should return Some, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && (d.message.contains("upper bound") || d.message.contains("exceeds"))
            }),
            "expected a Warning about face_0 exceeding upper bound, got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            } => {
                assert!(
                    faces_to_remove.is_empty(),
                    "huge face index should be skipped, got {:?}",
                    faces_to_remove
                );
            }
            other => panic!("expected GeometryOp::Shell, got {:?}", other),
        }
    }

    // ── Shell face index diagnostic precision tests ───────────────────────────

    #[test]
    fn compile_geometry_op_shell_string_face_diagnostic_excludes_non_finite() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 is a String — as_f64() returns None (non-numeric type, NOT non-finite)
        // Diagnostic should say 'non-numeric' only, NOT 'non-finite'
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                (
                    "face_0".into(),
                    reify_ir::CompiledExpr::literal(
                        reify_ir::Value::String("bad".into()),
                        reify_core::Type::String,
                    ),
                ),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell should return Some even when face_0 is String, got {:?}",
            result
        );
        let face_0_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                matches!(d.severity, reify_core::Severity::Warning) && d.message.contains("face_0")
            })
            .collect();
        assert_eq!(
            face_0_warnings.len(),
            1,
            "expected exactly one Warning mentioning 'face_0', got: {:?}",
            face_0_warnings
        );
        let diag = face_0_warnings[0];
        assert!(
            diag.message.contains("non-numeric"),
            "diagnostic should mention 'non-numeric', got: {:?}",
            diag.message
        );
        assert!(
            !diag.message.contains("non-finite"),
            "diagnostic should NOT mention 'non-finite' for a non-numeric type, got: {:?}",
            diag.message
        );
    }

    #[test]
    fn compile_geometry_op_shell_nan_face_diagnostic_excludes_negative() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = NaN — non-finite value; diagnostic should say 'non-finite', NOT 'negative'
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(f64::NAN)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with NaN face_0 should return Some, got {:?}",
            result
        );
        let face_0_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                matches!(d.severity, reify_core::Severity::Warning) && d.message.contains("face_0")
            })
            .collect();
        assert_eq!(
            face_0_warnings.len(),
            1,
            "expected exactly one Warning mentioning 'face_0', got: {:?}",
            face_0_warnings
        );
        let diag = face_0_warnings[0];
        assert!(
            diag.message.contains("non-finite"),
            "NaN diagnostic should mention 'non-finite', got: {:?}",
            diag.message
        );
        assert!(
            !diag.message.contains("negative"),
            "NaN diagnostic should NOT mention 'negative', got: {:?}",
            diag.message
        );
    }

    #[test]
    fn compile_geometry_op_shell_negative_face_diagnostic_excludes_non_finite() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = -1.0 — negative value; diagnostic should say 'negative', NOT 'non-finite'
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(-1.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with -1.0 face_0 should return Some, got {:?}",
            result
        );
        let face_0_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                matches!(d.severity, reify_core::Severity::Warning) && d.message.contains("face_0")
            })
            .collect();
        assert_eq!(
            face_0_warnings.len(),
            1,
            "expected exactly one Warning mentioning 'face_0', got: {:?}",
            face_0_warnings
        );
        let diag = face_0_warnings[0];
        assert!(
            diag.message.contains("negative"),
            "negative face diagnostic should mention 'negative', got: {:?}",
            diag.message
        );
        assert!(
            !diag.message.contains("non-finite"),
            "negative face diagnostic should NOT mention 'non-finite', got: {:?}",
            diag.message
        );
    }

    #[test]
    fn compile_geometry_op_shell_neg_infinity_face_diagnostic_says_non_finite() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = -Infinity — satisfies both !is_finite() and < 0.0; should be classified
        // as 'non-finite' (not 'negative'), so the is_finite() arm must come first.
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(f64::NEG_INFINITY)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with -Infinity face_0 should return Some, got {:?}",
            result
        );
        let face_0_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                matches!(d.severity, reify_core::Severity::Warning) && d.message.contains("face_0")
            })
            .collect();
        assert_eq!(
            face_0_warnings.len(),
            1,
            "expected exactly one Warning mentioning 'face_0', got: {:?}",
            face_0_warnings
        );
        let diag = face_0_warnings[0];
        assert!(
            diag.message.contains("non-finite"),
            "-Infinity diagnostic should mention 'non-finite', got: {:?}",
            diag.message
        );
        assert!(
            !diag.message.contains("negative"),
            "-Infinity diagnostic should NOT mention 'negative' (it is non-finite, not negative), got: {:?}",
            diag.message
        );
    }

    // ── validate_pattern_count upper-bound tests ──────────────────────────────

    #[test]
    fn validate_pattern_count_rejects_huge_count() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // count=1e15 is way above the upper bound and should be rejected
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(1.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(1e15)),
                ("spacing".into(), literal_length(0.01)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "count=1e15 should return None, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && (d.message.contains("upper bound") || d.message.contains("exceeds"))
            }),
            "expected a Warning mentioning 'upper bound' or 'exceeds', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn validate_pattern_count_boundary_100000_succeeds() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // count=100_000 is exactly at the upper bound and should be accepted
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(1.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(100_000.0)),
                ("spacing".into(), literal_length(0.01)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::LinearPattern { count, .. }) => {
                assert_eq!(count, 100_000, "count=100_000 should be accepted");
            }
            other => panic!(
                "expected Some(LinearPattern) for count=100_000, got {:?}",
                other
            ),
        }
        // No upper-bound diagnostic should be emitted for a valid boundary value
        assert!(
            !diagnostics
                .iter()
                .any(|d| { d.message.contains("upper bound") || d.message.contains("exceeds") }),
            "count=100_000 should not emit an upper-bound diagnostic, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn validate_pattern_count_boundary_100001_rejected() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // count=100_001 exceeds the upper bound by one and should be rejected
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(1.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(100_001.0)),
                ("spacing".into(), literal_length(0.01)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "count=100_001 should return None, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && (d.message.contains("upper bound") || d.message.contains("exceeds"))
            }),
            "expected a Warning for count=100_001, got: {:?}",
            diagnostics
        );
    }

    /// Drives the `Result<GeometryOp, String>` API: a missing required arg must
    /// cause `compile_geometry_op` to return `Err(msg)` where `msg` names both
    /// the missing argument and the op kind, so callers can emit a specific
    /// Error diagnostic instead of a generic one.
    ///
    /// Uses Revolve missing `ox` as the representative case because Revolve has
    /// the most required f64 args (7) and `ox` is the last one resolved, making
    /// it easy to isolate without triggering other validation guards.
    #[test]
    fn compile_geometry_op_missing_arg_returns_err_with_arg_name() {
        let step_handles = vec![GeometryHandleId(1)];
        let values = ValueMap::new();

        // Revolve with all required args EXCEPT ox — drives the Result API.
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
                // "ox" deliberately omitted — drives Result<_, String> API
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // The Result-typed API must return Err containing the arg name and op kind.
        // This assertion fails to compile with the current Option<_> return type.
        assert!(
            result.is_err(),
            "missing 'ox' should return Err, got: {:?}",
            result
        );
        let err_msg = result.unwrap_err();
        assert!(
            err_msg.contains("ox"),
            "error message should mention the missing arg 'ox', got: {:?}",
            err_msg
        );
        assert!(
            err_msg.contains("revolve"),
            "error message should mention the op kind 'revolve', got: {:?}",
            err_msg
        );
    }

    // -----------------------------------------------------------------
    // eval_named_arg_f64: non-numeric / non-finite value coverage
    // -----------------------------------------------------------------
    //
    // These close the gap left by existing coverage (which only exercises
    // numeric paths through compile_geometry_op): the three branches of
    // `match value.as_f64() { Some(v) if v.is_finite() => ..., _ => warn; None }`
    // must all emit a Warning diagnostic naming the arg and kind.

    #[test]
    fn eval_named_arg_f64_undef_value_returns_none_with_warning() {
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        // Value::Undef is the universal no-value sentinel — `as_f64()` returns None.
        let undef_expr =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Undef, reify_core::Type::dimensionless_scalar());
        let args = vec![("width".to_string(), undef_expr)];

        let result = eval_named_arg_f64(
            "width",
            reify_compiler::PrimitiveKind::Box,
            &args,
            &values,
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(result.is_none(), "Undef value should return None");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == reify_core::Severity::Warning
                    && d.message.contains("width")
                    && d.message.contains("box")
                    && d.message.contains("non-numeric/non-finite")),
            "expected Warning mentioning 'width', 'box', and 'non-numeric/non-finite', \
             got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn eval_named_arg_f64_nan_value_returns_none_with_warning() {
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let nan_expr = literal_f64(f64::NAN);
        let args = vec![("width".to_string(), nan_expr)];

        let result = eval_named_arg_f64(
            "width",
            reify_compiler::PrimitiveKind::Box,
            &args,
            &values,
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(result.is_none(), "NaN value should return None");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == reify_core::Severity::Warning
                    && d.message.contains("width")
                    && d.message.contains("box")
                    && d.message.contains("non-numeric/non-finite")),
            "expected Warning mentioning 'width', 'box', and 'non-numeric/non-finite', \
             got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn eval_named_arg_f64_infinity_value_returns_none_with_warning() {
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let inf_expr = literal_f64(f64::INFINITY);
        let args = vec![("width".to_string(), inf_expr)];

        let result = eval_named_arg_f64(
            "width",
            reify_compiler::PrimitiveKind::Box,
            &args,
            &values,
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(result.is_none(), "infinity should return None");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == reify_core::Severity::Warning
                    && d.message.contains("width")
                    && d.message.contains("box")
                    && d.message.contains("non-numeric/non-finite")),
            "expected Warning mentioning 'width', 'box', and 'non-numeric/non-finite', \
             got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ── named_steps / GeomRef::Sub resolution tests ───────────────────────────

    /// Happy path: compile_geometry_op resolves GeomRef::Sub("body") and
    /// GeomRef::Sub("hole") from the named_steps map and produces the correct
    /// Difference op.
    ///
    /// This test intentionally fails to compile until step-2 adds the
    /// `named_steps` parameter to `compile_geometry_op`.
    #[test]
    fn compile_geometry_op_sub_ref_resolved_via_named_steps() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};

        let handle_a = GeometryHandleId(10);
        let handle_b = GeometryHandleId(20);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".into(), kh(handle_a));
        named_steps.insert("hole".into(), kh(handle_b));

        let op = CompiledGeometryOp::Boolean {
            op: BooleanOp::Difference,
            left: GeomRef::Sub("body".into()),
            right: GeomRef::Sub("hole".into()),
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &ValueMap::new(),
            &[], // no step handles — Sub refs must resolve via named_steps
            &[],
            &HashMap::new(),
            &named_steps,
            &mut diagnostics,
        );

        let geom_op = result.expect("Sub refs with known names should resolve successfully");
        match geom_op {
            reify_ir::GeometryOp::Difference { left, right } => {
                assert_eq!(left, handle_a, "left should be body handle");
                assert_eq!(right, handle_b, "right should be hole handle");
            }
            other => panic!("expected Difference, got {:?}", other),
        }

        // No warnings should be emitted — named_steps lookup is silent-success
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Warning)
            .collect();
        assert!(
            warnings.is_empty(),
            "no Warning diagnostics expected for successful Sub resolution, got: {:?}",
            warnings
        );
    }

    /// Unknown-name error path: compile_geometry_op with GeomRef::Sub("unknown")
    /// and an empty named_steps map must return Err whose message contains
    /// "unresolvable GeomRef::Sub('unknown')", and MUST NOT push any
    /// Warning-severity diagnostics (regression guard against the old
    /// warning+last()-fallback behavior).
    #[test]
    fn compile_geometry_op_sub_ref_unknown_name_returns_err_no_warning() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};

        let op = CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Sub("unknown".into()),
            right: GeomRef::Step(0),
        };

        let step_handles = vec![GeometryHandleId(5)];
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new(); // empty — "unknown" not present

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &ValueMap::new(),
            &step_handles,
            &[],
            &HashMap::new(),
            &named_steps,
            &mut diagnostics,
        );

        // Must return Err (not Ok with a fabricated default)
        let err_msg = result.expect_err("Sub ref with unknown name should return Err");
        assert!(
            err_msg.contains("unresolvable GeomRef::Sub('unknown')"),
            "error message should contain \"unresolvable GeomRef::Sub('unknown')\", got: {:?}",
            err_msg
        );

        // Must NOT emit any Warning-severity diagnostic — the old fallback
        // emitted a Warning before returning the last handle; that pattern is
        // explicitly forbidden by the feedback_silent_defaults_pattern norm.
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Warning)
            .collect();
        assert!(
            warnings.is_empty(),
            "no Warning diagnostics expected on unknown-name Sub resolution, got: {:?}",
            warnings
        );
    }

    /// Contract test (task 4142, Cluster A RED): compile_geometry_op resolves
    /// GeomRef::Sub via `KernelHandle.id`, ignoring `KernelHandle.kernel`.
    ///
    /// Uses deliberately non-default `KernelId` values (Manifold for "body",
    /// Fidget for "hole") to prove the GeomRef::Sub arm (geometry_ops.rs:368)
    /// keys only off `.id` and never consults `.kernel`.
    ///
    /// RED on current main: `compile_geometry_op` still takes
    /// `&HashMap<String, GeometryHandleId>`, so passing a
    /// `HashMap<String, KernelHandle>` causes a compile-time type mismatch.
    /// GREEN after step-2: signature changed + leaf projection updated.
    ///
    /// NOTE: Pins the leaf-projection contract only (`.kernel` unused in the current
    /// single-kernel-per-build design). When cross-kernel handle resolution lands,
    /// update to assert per-kernel dispatch rather than treating `.kernel` as ignored.
    #[test]
    fn compile_geometry_op_sub_ref_resolves_via_kernel_handle_id_ignoring_kernel_field() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body".into(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Manifold, // deliberately non-default
                id: GeometryHandleId(10),
            },
        );
        named_steps.insert(
            "hole".into(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Fidget, // deliberately non-default
                id: GeometryHandleId(20),
            },
        );

        let op = CompiledGeometryOp::Boolean {
            op: BooleanOp::Difference,
            left: GeomRef::Sub("body".into()),
            right: GeomRef::Sub("hole".into()),
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &ValueMap::new(),
            &[], // no step handles — Sub refs resolve via named_steps
            &[],
            &HashMap::new(),
            &named_steps,
            &mut diagnostics,
        );

        let geom_op =
            result.expect("Sub refs with known KernelHandle values should resolve successfully");
        match geom_op {
            reify_ir::GeometryOp::Difference { left, right } => {
                assert_eq!(
                    left,
                    GeometryHandleId(10),
                    "left must be body's .id (10), not influenced by .kernel (Manifold)"
                );
                assert_eq!(
                    right,
                    GeometryHandleId(20),
                    "right must be hole's .id (20), not influenced by .kernel (Fidget)"
                );
            }
            other => panic!("expected Difference, got {:?}", other),
        }

        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for successful KernelHandle Sub resolution, \
             got: {:?}",
            diagnostics
        );
    }

    // ── try_eval_conformance_query unit tests (task 2320) ────────────────────
    //
    // These tests pin the contract of `try_eval_conformance_query`, the
    // kernel-aware eval-time dispatch surface for the `is_watertight`,
    // `is_manifold`, `is_orientable` stdlib helpers. Architecture rationale
    // is captured in the task 2320 plan; the function lives in this module
    // (rather than `eval_expr`) because the build pipeline owns both the
    // kernel and the per-realization name → handle map (`named_steps`).

    /// Build a `CompiledExpr` for `is_watertight(<entity>.<member>)`.
    fn conformance_call(helper_name: &str, entity: &str, member: &str) -> reify_ir::CompiledExpr {
        let arg = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member),
            reify_core::Type::Geometry,
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg],
            },
            result_type: reify_core::Type::Bool,
            content_hash,
        }
    }

    #[test]
    fn try_eval_conformance_query_kernel_reply_true() {
        use reify_test_support::mocks::MockGeometryKernel;
        let handle_id = reify_ir::GeometryHandleId(7);
        let kernel =
            MockGeometryKernel::new().with_query_result(handle_id, reify_ir::Value::Bool(true));

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "is_watertight(body) with kernel returning Bool(true) must produce Some(Bool(true))"
        );
    }

    /// Build a `CompiledExpr` for `is_watertight(<literal_real>)`.
    fn conformance_call_literal_arg(helper_name: &str) -> reify_ir::CompiledExpr {
        let arg =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Real(1.0), reify_core::Type::dimensionless_scalar());
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg],
            },
            result_type: reify_core::Type::Bool,
            content_hash,
        }
    }

    #[test]
    fn try_eval_conformance_query_non_helper_name_returns_none_no_kernel_call() {
        let handle_id = reify_ir::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        // `volume` is a real stdlib function name but NOT one of the three
        // recognised conformance helpers. The dispatch must return None.
        let expr = conformance_call("volume", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert!(
            result.is_none(),
            "non-helper name 'volume' must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-helper names"
        );
    }

    #[test]
    fn try_eval_conformance_query_literal_arg_returns_none_no_kernel_call() {
        let handle_id = reify_ir::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();

        // `is_watertight(1.0)` — recognised helper name but the arg is a
        // literal, not a `ValueRef`. The dispatch must return None *and*
        // never consult the kernel.
        let expr = conformance_call_literal_arg("is_watertight");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert!(
            result.is_none(),
            "is_watertight(<literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_watertight_short_circuits() {
        // Kernel is configured to return Bool(false) — but the structure
        // declares `: Watertight`, so the dispatch must short-circuit to
        // Bool(true) WITHOUT consulting the kernel.
        let handle_id = reify_ir::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_watertight", "TrustedShell", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Watertight".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "user-asserted Watertight must override kernel reply"
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted when the structure asserts Watertight"
        );
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_manifold_short_circuits() {
        let handle_id = reify_ir::GeometryHandleId(11);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_manifold", "TrustedShell", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Manifold".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Bool(true)));
        assert_eq!(kernel.total_query_count(), 0);
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_orientable_short_circuits() {
        let handle_id = reify_ir::GeometryHandleId(13);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_orientable", "TrustedShell", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Orientable".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Bool(true)));
        assert_eq!(kernel.total_query_count(), 0);
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_closed_does_not_short_circuit_is_watertight() {
        // Asymmetry per task 2320 design decision: `is_watertight` short-
        // circuits ONLY on `Watertight` — declaring the (refined) `Closed`
        // bound is not sufficient. The kernel must be consulted and its
        // Bool(false) reply honoured.
        let handle_id = reify_ir::GeometryHandleId(17);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Closed".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(false)),
            "is_watertight must NOT be short-circuited by ': Closed'"
        );
        assert_eq!(
            kernel.total_query_count(),
            1,
            "kernel must be consulted exactly once when no matching marker trait is declared"
        );
    }

    #[test]
    fn try_eval_conformance_query_unresolvable_member_returns_none_no_kernel_call() {
        let handle_id = reify_ir::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        // `named_steps` contains "body" but the call references "ghost",
        // which is not present. The dispatch must return None and never
        // consult the kernel.
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_watertight", "Bracket", "ghost");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert!(
            result.is_none(),
            "unresolvable cell-member 'ghost' must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted when the cell-member name is absent"
        );
    }

    /// Failure-mode contract (amend, task 2320): when the kernel returns
    /// `Ok(value)` with a non-`Bool` value (e.g. a stray `Value::Real`),
    /// `try_eval_conformance_query` must defensively downgrade to
    /// `Some(Value::Undef)` and emit exactly one Warning diagnostic naming
    /// the helper. Pins the `Ok(other)` arm in the source so a regression
    /// that swaps the downgrade for a panic (or drops the diagnostic)
    /// would be caught.
    #[test]
    fn try_eval_conformance_query_kernel_returns_non_bool_downgrades_with_warning() {
        let handle_id = reify_ir::GeometryHandleId(23);
        // Seed a non-Bool kernel reply for the IsWatertight query.
        let kernel = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Real(1.0));

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "non-Bool kernel reply must downgrade to Some(Value::Undef), got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Bool kernel reply must emit exactly one diagnostic, got {}",
            diagnostics.len()
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "non-Bool kernel reply must emit a Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("is_watertight"),
            "diagnostic must mention the helper name, got: {}",
            diag.message
        );
    }

    /// Failure-mode contract (amend, task 2320): when the kernel returns
    /// `Err(QueryError)`, `try_eval_conformance_query` must defensively
    /// downgrade to `Some(Value::Undef)` and emit exactly one Warning
    /// diagnostic naming the helper and surfacing the error message. Pins
    /// the `Err(err)` arm so a regression swapping the downgrade for a
    /// panic (or losing the error context in the diagnostic) would fail.
    #[test]
    fn try_eval_conformance_query_kernel_query_error_downgrades_with_warning() {
        let handle_id = reify_ir::GeometryHandleId(29);
        // No `with_query_result` seeding → MockGeometryKernel.query() returns
        // `Err(QueryError::QueryFailed("no mock result for …"))` for any handle.
        let kernel = reify_test_support::mocks::MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_manifold", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "kernel Err must downgrade to Some(Value::Undef), got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one diagnostic, got {}",
            diagnostics.len()
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "kernel Err must emit a Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("is_manifold"),
            "diagnostic must mention the helper name, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("kernel query failed"),
            "diagnostic must indicate the kernel failure, got: {}",
            diag.message
        );
    }

    // ── task 4142 Cluster B RED contract tests ───────────────────────────────
    //
    // These three tests pin the "resolve via KernelHandle.id, ignore
    // KernelHandle.kernel" contract on the three remaining leaf families.
    // They fail to compile on current main (Cluster B helpers still take
    // `&HashMap<String, GeometryHandleId>`), and go GREEN when step-4 lands.

    /// Contract test (task 4142, Cluster B RED — conformance leaf):
    /// `try_eval_conformance_query` resolves the geometry handle via
    /// `KernelHandle.id`, ignoring `KernelHandle.kernel`.
    ///
    /// Uses `KernelId::Manifold` (non-default) to prove the leaf at
    /// geometry_ops.rs:1399 keys only off `.id`.
    ///
    /// RED on current main: `try_eval_conformance_query` still takes
    /// `&HashMap<String, GeometryHandleId>` → E0308 type mismatch.
    ///
    /// NOTE: Pins the leaf-projection contract only (`.kernel` unused in the current
    /// single-kernel-per-build design). When cross-kernel handle resolution lands,
    /// update to assert per-kernel dispatch rather than treating `.kernel` as ignored.
    #[test]
    fn try_eval_conformance_query_resolves_via_kernel_handle_id() {
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_id = reify_ir::GeometryHandleId(7);
        let kernel =
            MockGeometryKernel::new().with_query_result(handle_id, reify_ir::Value::Bool(true));

        // Map "body" to a KernelHandle with deliberately non-default kernel.
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Manifold, // non-default: must be ignored
                id: handle_id,
            },
        );

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "is_watertight with KernelHandle{{Manifold, 7}} must produce Some(Bool(true)); \
             kernel was keyed on .id (7), not .kernel",
        );
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for successful conformance resolution, got: {:?}",
            diagnostics
        );
    }

    /// Contract test (task 4142, Cluster B RED — kinematic leaf):
    /// `try_eval_kinematic_query` resolves solid names via `KernelHandle.id`,
    /// ignoring `KernelHandle.kernel`.
    ///
    /// Uses `KernelId::Manifold`/"base" and `KernelId::Fidget`/"hole" (both
    /// non-default) to prove the leaf at geometry_ops.rs:1909 keys only off
    /// `.id`.
    ///
    /// RED on current main: `try_eval_kinematic_query` still takes
    /// `&HashMap<String, GeometryHandleId>` → E0308 type mismatch.
    ///
    /// NOTE: Pins the leaf-projection contract only (`.kernel` unused in the current
    /// single-kernel-per-build design). When cross-kernel handle resolution lands,
    /// update to assert per-kernel dispatch rather than treating `.kernel` as ignored.
    #[test]
    fn try_eval_kinematic_query_resolves_via_kernel_handle_id() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let base_id = reify_ir::GeometryHandleId(10);
        let hole_id = reify_ir::GeometryHandleId(20);

        // Distance <= 0.0 → interference.
        let mut kernel = MockGeometryKernel::new().with_distance_result(
            base_id,
            hole_id,
            reify_ir::Value::Real(-1.0),
        );

        // Map solid names to KernelHandle with deliberately non-default kernels.
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "base".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Manifold, // non-default
                id: base_id,
            },
        );
        named_steps.insert(
            "hole".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Fidget, // non-default
                id: hole_id,
            },
        );

        // Build a Snapshot value: { kind: "snapshot", bodies: [{id:1, solid:"base"}, {id:2, solid:"hole"}] }
        let make_body = |id: i64, solid: &str| -> reify_ir::Value {
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                reify_ir::Value::String("id".to_string()),
                reify_ir::Value::Int(id),
            );
            m.insert(
                reify_ir::Value::String("solid".to_string()),
                reify_ir::Value::String(solid.to_string()),
            );
            reify_ir::Value::Map(m)
        };
        let mut snap_map = std::collections::BTreeMap::new();
        snap_map.insert(
            reify_ir::Value::String("kind".to_string()),
            reify_ir::Value::String("snapshot".to_string()),
        );
        snap_map.insert(
            reify_ir::Value::String("bodies".to_string()),
            reify_ir::Value::List(vec![make_body(1, "base"), make_body(2, "hole")]),
        );
        let snapshot = reify_ir::Value::Map(snap_map);

        let snap_cell = ValueCellId::new("Mech", "snap");
        let snap_arg = reify_ir::CompiledExpr::value_ref(snap_cell.clone(), Type::Geometry);
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("interferes"))
            .combine(snap_arg.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "interferes".to_string(),
                    qualified_name: "interferes".to_string(),
                },
                args: vec![snap_arg],
            },
            result_type: Type::List(Box::new(Type::Map(
                Box::new(Type::String),
                Box::new(Type::Int),
            ))),
            content_hash,
        };

        let mut values = reify_ir::ValueMap::new();
        values.insert(snap_cell, snapshot);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        // Distance(base_id=10, hole_id=20) = -1.0 ≤ 0.0 → the pair (1,2) interferes.
        // Result must be Some(List([{a:1, b:2}])).
        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "interferes with overlapping bodies must return Some(List([..])), \
                 got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "exactly one interfering pair expected, got {} entries: {:?}",
            list.len(),
            list
        );
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for successful kinematic resolution, got: {:?}",
            diagnostics
        );
    }

    /// Contract test (task 3906 T8, step-1 RED): `try_eval_kinematic_query` applies each
    /// body's `world_transform` via `kernel.execute(ApplyTransform{…})` before the pairwise
    /// `Distance` probe, so FK-posed geometry is what actually determines interference.
    ///
    /// Fixture: two bodies whose SOURCE handles are disjoint (distance 5.0 > 0) but whose
    /// FK-POSED handles interfere (distance -1.0 ≤ 0). Each body carries a NON-identity
    /// `world_transform` (pure translation: body_a +10mm, body_b +15mm along X).
    ///
    /// After step-2's impl:
    ///   (a) result is `Some(List([{a:1, b:2}]))` — posed geometry interferes.
    ///   (b) `kernel.operations()` has exactly 2 `ApplyTransform` records whose
    ///       `(target, rotation, translation)` match each body's source handle and
    ///       `decompose_transform_to_arrays` output.
    ///
    /// RED on main: no ApplyTransform ops emitted → probe uses source handles (10, 20)
    /// → distance 5.0 > 0 → empty list; both (a) and (b) fail.
    #[test]
    fn try_eval_kinematic_query_applies_world_transform_before_distance() {
        use reify_core::{Type, ValueCellId};
        use reify_ir::GeometryOp;
        use reify_test_support::mocks::MockGeometryKernel;

        let src_a = reify_ir::GeometryHandleId(10);
        let src_b = reify_ir::GeometryHandleId(20);
        // MockGeometryKernel::new() initialises next_id = 1 with no prior
        // operations (see mocks.rs: `next_id: 1`). Each execute() call
        // auto-increments: first call → GeometryHandleId(1), second → (2).
        // This test issues no other execute() calls before try_eval_kinematic_query,
        // so body A's ApplyTransform → id 1, body B's → id 2. If the mock ever
        // pre-allocates a handle or changes its seed, update these constants and
        // the with_distance_result fixture below to match.
        let posed_a = reify_ir::GeometryHandleId(1);
        let posed_b = reify_ir::GeometryHandleId(2);

        // Source handles disjoint; posed handles interfere.
        let mut kernel = MockGeometryKernel::new()
            .with_distance_result(src_a, src_b, reify_ir::Value::Real(5.0))
            .with_distance_result(posed_a, posed_b, reify_ir::Value::Real(-1.0));

        // Non-identity world_transforms: pure translations along X.
        // Both are non-identity (translation != [0,0,0]).
        let tx_a = 0.010_f64; // 10 mm
        let tx_b = 0.015_f64; // 15 mm

        let make_transform = |tx: f64| -> reify_ir::Value {
            reify_ir::Value::Transform {
                rotation: Box::new(reify_ir::Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                translation: Box::new(reify_ir::Value::Vector(vec![
                    reify_ir::Value::length(tx),
                    reify_ir::Value::length(0.0),
                    reify_ir::Value::length(0.0),
                ])),
            }
        };

        // Map solid names to KernelHandle.
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body_a".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_a,
            },
        );
        named_steps.insert(
            "body_b".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_b,
            },
        );

        // Snapshot with body records that carry a `world_transform` key.
        let make_body = |id: i64, solid: &str, wt: reify_ir::Value| -> reify_ir::Value {
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                reify_ir::Value::String("id".to_string()),
                reify_ir::Value::Int(id),
            );
            m.insert(
                reify_ir::Value::String("solid".to_string()),
                reify_ir::Value::String(solid.to_string()),
            );
            m.insert(reify_ir::Value::String("world_transform".to_string()), wt);
            reify_ir::Value::Map(m)
        };
        let mut snap_map = std::collections::BTreeMap::new();
        snap_map.insert(
            reify_ir::Value::String("kind".to_string()),
            reify_ir::Value::String("snapshot".to_string()),
        );
        snap_map.insert(
            reify_ir::Value::String("bodies".to_string()),
            reify_ir::Value::List(vec![
                make_body(1, "body_a", make_transform(tx_a)),
                make_body(2, "body_b", make_transform(tx_b)),
            ]),
        );
        let snapshot = reify_ir::Value::Map(snap_map);

        // Build interferes(s) call expr.
        let snap_cell = ValueCellId::new("Mech", "snap");
        let snap_arg = reify_ir::CompiledExpr::value_ref(snap_cell.clone(), Type::Geometry);
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("interferes"))
            .combine(snap_arg.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "interferes".to_string(),
                    qualified_name: "interferes".to_string(),
                },
                args: vec![snap_arg],
            },
            result_type: Type::List(Box::new(Type::Map(
                Box::new(Type::String),
                Box::new(Type::Int),
            ))),
            content_hash,
        };

        let mut values = reify_ir::ValueMap::new();
        values.insert(snap_cell, snapshot);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        // (a) Posed geometry interferes — exactly one pair {a:1, b:2} (body ids, not handle ids).
        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "FK-posed interfering bodies must return Some(List([..])), \
                 got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "exactly one interfering pair expected from FK-posed geometry, got {} entries: {:?}",
            list.len(),
            list
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );

        // (b) Exactly two ApplyTransform ops (one per non-identity body).
        let ops = kernel.operations();
        let apply_ops: Vec<_> = ops
            .iter()
            .filter(|rec| matches!(&rec.op, GeometryOp::ApplyTransform { .. }))
            .collect();
        assert_eq!(
            apply_ops.len(),
            2,
            "expected exactly 2 ApplyTransform ops (one per body), got {}: {:?}",
            apply_ops.len(),
            apply_ops
        );
        // body_a: first op targets src_a with its decomposed transform.
        match &apply_ops[0].op {
            GeometryOp::ApplyTransform {
                target,
                rotation,
                translation,
            } => {
                assert_eq!(
                    *target, src_a,
                    "first ApplyTransform must target body_a source handle"
                );
                assert_eq!(
                    *rotation,
                    [1.0_f64, 0.0, 0.0, 0.0],
                    "body_a rotation must be identity quaternion"
                );
                assert!(
                    (translation[0] - tx_a).abs() < 1e-12,
                    "body_a tx[0]: expected {tx_a}, got {}",
                    translation[0]
                );
                assert_eq!(translation[1], 0.0, "body_a tx[1] must be zero");
                assert_eq!(translation[2], 0.0, "body_a tx[2] must be zero");
            }
            other => panic!("expected ApplyTransform, got {:?}", other),
        }
        // body_b: second op targets src_b with its decomposed transform.
        match &apply_ops[1].op {
            GeometryOp::ApplyTransform {
                target,
                rotation,
                translation,
            } => {
                assert_eq!(
                    *target, src_b,
                    "second ApplyTransform must target body_b source handle"
                );
                assert_eq!(
                    *rotation,
                    [1.0_f64, 0.0, 0.0, 0.0],
                    "body_b rotation must be identity quaternion"
                );
                assert!(
                    (translation[0] - tx_b).abs() < 1e-12,
                    "body_b tx[0]: expected {tx_b}, got {}",
                    translation[0]
                );
                assert_eq!(translation[1], 0.0, "body_b tx[1] must be zero");
                assert_eq!(translation[2], 0.0, "body_b tx[2] must be zero");
            }
            other => panic!("expected ApplyTransform, got {:?}", other),
        }
    }

    /// Contract test (task 3906 T8, step-3 RED): bodies with an IDENTITY `world_transform`
    /// must NOT emit an `ApplyTransform` op — only bodies with a non-identity transform do.
    ///
    /// Fixture: body A has an identity world_transform (rotation [1,0,0,0], translation
    /// [0,0,0]); body B has a non-identity world_transform (translation +20mm along X).
    /// After step-4's identity short-circuit, only body B gets an ApplyTransform op.
    ///
    /// RED states:
    ///   - on main (before step-2): ZERO ApplyTransform ops (≠ 1)
    ///   - after step-2's apply-unconditionally impl: TWO ops (identity applied too, ≠ 1)
    ///
    /// Either way "exactly one" fails until the short-circuit lands in step-4.
    #[test]
    fn try_eval_kinematic_query_skips_identity_world_transform() {
        use reify_core::{Type, ValueCellId};
        use reify_ir::GeometryOp;
        use reify_test_support::mocks::MockGeometryKernel;

        let src_a = reify_ir::GeometryHandleId(100);
        let src_b = reify_ir::GeometryHandleId(200);
        // Only body B (non-identity) gets an ApplyTransform; body A (identity)
        // stays at its raw handle. MockGeometryKernel::new() initialises
        // next_id = 1, and no execute() calls precede try_eval_kinematic_query
        // in this test, so body B's single ApplyTransform → GeometryHandleId(1).
        // If the mock ever changes its seeding, update posed_b accordingly.
        let posed_b = reify_ir::GeometryHandleId(1);

        // Probe (src_a=100, posed_b=1) interferes; body A stays at raw handle.
        let mut kernel = MockGeometryKernel::new().with_distance_result(
            src_a,
            posed_b,
            reify_ir::Value::Real(-1.0),
        );

        let make_transform = |tx: f64, ty: f64, tz: f64| -> reify_ir::Value {
            reify_ir::Value::Transform {
                rotation: Box::new(reify_ir::Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                translation: Box::new(reify_ir::Value::Vector(vec![
                    reify_ir::Value::length(tx),
                    reify_ir::Value::length(ty),
                    reify_ir::Value::length(tz),
                ])),
            }
        };

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body_a".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_a,
            },
        );
        named_steps.insert(
            "body_b".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_b,
            },
        );

        let make_body = |id: i64, solid: &str, wt: reify_ir::Value| -> reify_ir::Value {
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                reify_ir::Value::String("id".to_string()),
                reify_ir::Value::Int(id),
            );
            m.insert(
                reify_ir::Value::String("solid".to_string()),
                reify_ir::Value::String(solid.to_string()),
            );
            m.insert(reify_ir::Value::String("world_transform".to_string()), wt);
            reify_ir::Value::Map(m)
        };
        let mut snap_map = std::collections::BTreeMap::new();
        snap_map.insert(
            reify_ir::Value::String("kind".to_string()),
            reify_ir::Value::String("snapshot".to_string()),
        );
        snap_map.insert(
            reify_ir::Value::String("bodies".to_string()),
            reify_ir::Value::List(vec![
                // body A: identity transform — must NOT emit ApplyTransform.
                make_body(100, "body_a", make_transform(0.0, 0.0, 0.0)),
                // body B: non-identity (20mm along X) — MUST emit ApplyTransform.
                make_body(200, "body_b", make_transform(0.020, 0.0, 0.0)),
            ]),
        );
        let snapshot = reify_ir::Value::Map(snap_map);

        let snap_cell = ValueCellId::new("Mech", "snap");
        let snap_arg = reify_ir::CompiledExpr::value_ref(snap_cell.clone(), Type::Geometry);
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("interferes"))
            .combine(snap_arg.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "interferes".to_string(),
                    qualified_name: "interferes".to_string(),
                },
                args: vec![snap_arg],
            },
            result_type: Type::List(Box::new(Type::Map(
                Box::new(Type::String),
                Box::new(Type::Int),
            ))),
            content_hash,
        };

        let mut values = reify_ir::ValueMap::new();
        values.insert(snap_cell, snapshot);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let _ = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        // Exactly ONE ApplyTransform op: only body B's non-identity transform.
        // Body A's identity transform must short-circuit to its raw handle.
        let ops = kernel.operations();
        let apply_ops: Vec<_> = ops
            .iter()
            .filter(|rec| matches!(&rec.op, GeometryOp::ApplyTransform { .. }))
            .collect();
        assert_eq!(
            apply_ops.len(),
            1,
            "expected exactly 1 ApplyTransform (body B only, not identity body A), \
             got {}: {:?}",
            apply_ops.len(),
            apply_ops
        );
        // Verify the single op targets body B's source handle.
        match &apply_ops[0].op {
            GeometryOp::ApplyTransform {
                target,
                translation,
                ..
            } => {
                assert_eq!(
                    *target, src_b,
                    "ApplyTransform must target body_b (non-identity)"
                );
                assert!(
                    (translation[0] - 0.020).abs() < 1e-12,
                    "body_b tx[0]: expected 0.020, got {}",
                    translation[0]
                );
            }
            other => panic!("expected ApplyTransform, got {:?}", other),
        }
    }

    /// Contract test (task 3844 KCC-ε): a `flat_map(snaps, |s| [center_of_mass(s)])`
    /// cell must return `None` from `try_eval_kinematic_query` so the pure-eval value
    /// (set by the regular eval pass) is preserved.
    ///
    /// The swept dispatch intercepts ALL `flat_map` calls in the kinematic post-process;
    /// this test locks the fall-through contract for non-kinematic inner functions,
    /// independent of OCCT availability.
    #[test]
    fn try_eval_swept_kinematic_query_non_kinematic_inner_falls_through_to_none() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        // snaps cell → a single-element list.  The snapshot content is never
        // accessed because the non-kinematic inner fn name triggers the
        // fall-through before any snapshot body processing.
        let snaps_cell = ValueCellId::new("Swept", "snaps");
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            snaps_cell.clone(),
            reify_ir::Value::List(vec![reify_ir::Value::Undef]),
        );

        // Lambda param cell.
        let s_param = ValueCellId::new("Swept", "s");

        // Inner: FunctionCall("center_of_mass", [ValueRef(s_param)])
        let s_ref = reify_ir::CompiledExpr::value_ref(s_param.clone(), Type::Geometry);
        let inner = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "center_of_mass".to_string(),
                    qualified_name: "center_of_mass".to_string(),
                },
                args: vec![s_ref],
            },
            result_type: Type::dimensionless_scalar(),
            content_hash: reify_core::ContentHash(0),
        };

        // Lambda body: ListLiteral([inner])
        let lambda_body = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::ListLiteral(vec![inner]),
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        // Lambda arg: Lambda { params, param_ids: [s_param], body, captures: [] }
        let lambda_arg = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::Lambda {
                params: vec![("s".to_string(), None)],
                param_ids: vec![s_param],
                body: Box::new(lambda_body),
                captures: vec![],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        // flat_map(snaps, lambda_arg)
        let snaps_ref =
            reify_ir::CompiledExpr::value_ref(snaps_cell, Type::List(Box::new(Type::Geometry)));
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "flat_map".to_string(),
                    qualified_name: "flat_map".to_string(),
                },
                args: vec![snaps_ref, lambda_arg],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();

        let result = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        assert!(
            result.is_none(),
            "flat_map with non-kinematic inner (center_of_mass) must return None \
             so the pure-eval value is preserved; got {result:?}"
        );
        assert!(
            diagnostics.is_empty(),
            "fall-through must emit no diagnostics; got {diagnostics:?}"
        );
    }

    /// Contract test (task 3844 KCC-ε): a well-formed
    /// `flat_map(snaps, |s| [min_clearance(s, id_a, id_b)])` over a 2-element
    /// snapshot list must return `Some(Value::List(len=2))` from
    /// `try_eval_kinematic_query`.
    ///
    /// Uses identity world_transforms (no `ApplyTransform` ops emitted) so the
    /// test can run without OCCT — only `MockGeometryKernel::with_distance_result`
    /// is required.  Locks the happy-path list-length contract independent of OCCT
    /// availability.
    #[test]
    fn try_eval_swept_kinematic_query_min_clearance_returns_list_of_correct_length() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let src_a = reify_ir::GeometryHandleId(10);
        let src_b = reify_ir::GeometryHandleId(20);

        // Identity world_transforms: no ApplyTransform ops, probe uses raw handles.
        let mut kernel = MockGeometryKernel::new()
            .with_distance_result(src_a, src_b, reify_ir::Value::Real(0.050)); // 50 mm

        let make_transform_identity = || -> reify_ir::Value {
            reify_ir::Value::Transform {
                rotation: Box::new(reify_ir::Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                translation: Box::new(reify_ir::Value::Vector(vec![
                    reify_ir::Value::length(0.0),
                    reify_ir::Value::length(0.0),
                    reify_ir::Value::length(0.0),
                ])),
            }
        };

        let make_snapshot = |transform_a: reify_ir::Value,
                              transform_b: reify_ir::Value|
         -> reify_ir::Value {
            let make_body = |id: i64, solid: &str, wt: reify_ir::Value| -> reify_ir::Value {
                let mut m = std::collections::BTreeMap::new();
                m.insert(
                    reify_ir::Value::String("id".to_string()),
                    reify_ir::Value::Int(id),
                );
                m.insert(
                    reify_ir::Value::String("solid".to_string()),
                    reify_ir::Value::String(solid.to_string()),
                );
                m.insert(reify_ir::Value::String("world_transform".to_string()), wt);
                reify_ir::Value::Map(m)
            };
            let mut snap_map = std::collections::BTreeMap::new();
            snap_map.insert(
                reify_ir::Value::String("kind".to_string()),
                reify_ir::Value::String("snapshot".to_string()),
            );
            snap_map.insert(
                reify_ir::Value::String("bodies".to_string()),
                reify_ir::Value::List(vec![
                    make_body(1, "body_a", transform_a),
                    make_body(2, "body_b", transform_b),
                ]),
            );
            reify_ir::Value::Map(snap_map)
        };

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body_a".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_a,
            },
        );
        named_steps.insert(
            "body_b".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_b,
            },
        );

        // Two snapshots, each with identity transforms.
        let snaps_cell = ValueCellId::new("Swept", "snaps");
        let id_a_cell = ValueCellId::new("Swept", "id_a");
        let id_b_cell = ValueCellId::new("Swept", "id_b");
        let s_param = ValueCellId::new("Swept", "s");

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            snaps_cell.clone(),
            reify_ir::Value::List(vec![
                make_snapshot(make_transform_identity(), make_transform_identity()),
                make_snapshot(make_transform_identity(), make_transform_identity()),
            ]),
        );
        values.insert(id_a_cell.clone(), reify_ir::Value::Int(1));
        values.insert(id_b_cell.clone(), reify_ir::Value::Int(2));

        // Build flat_map(snaps, |s| [min_clearance(s, id_a, id_b)])
        let s_ref = reify_ir::CompiledExpr::value_ref(s_param.clone(), Type::Geometry);
        let id_a_ref = reify_ir::CompiledExpr::value_ref(id_a_cell.clone(), Type::Int);
        let id_b_ref = reify_ir::CompiledExpr::value_ref(id_b_cell.clone(), Type::Int);

        let inner = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "min_clearance".to_string(),
                    qualified_name: "min_clearance".to_string(),
                },
                args: vec![s_ref, id_a_ref, id_b_ref],
            },
            result_type: Type::dimensionless_scalar(),
            content_hash: reify_core::ContentHash(0),
        };

        let lambda_body = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::ListLiteral(vec![inner]),
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        let lambda_arg = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::Lambda {
                params: vec![("s".to_string(), None)],
                param_ids: vec![s_param],
                body: Box::new(lambda_body),
                captures: vec![id_a_cell, id_b_cell],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        let snaps_ref =
            reify_ir::CompiledExpr::value_ref(snaps_cell, Type::List(Box::new(Type::Geometry)));
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "flat_map".to_string(),
                    qualified_name: "flat_map".to_string(),
                },
                args: vec![snaps_ref, lambda_arg],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        // Must return Some(Value::List) of length 2 (one result per snapshot).
        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "swept min_clearance flat_map over a 2-snapshot list must return \
                 Some(Value::List(len=2)), got {other:?}; diagnostics: {diagnostics:?}"
            ),
        };
        assert_eq!(
            list.len(),
            2,
            "list length must equal snapshot count (2), got {}: {list:?}",
            list.len()
        );
        // Each element must be a length Scalar with si_value ≈ 0.050 m.
        // Without this check, a regression that returns Value::Undef or dispatches
        // to the wrong helper would still pass the length-only assertion above.
        for (i, elem) in list.iter().enumerate() {
            match elem {
                reify_ir::Value::Scalar { si_value, .. } => {
                    let diff = (si_value - 0.050_f64).abs();
                    assert!(
                        diff < 1e-9,
                        "clearances[{i}] expected ≈ 0.050 m, got {si_value:.9} m (delta {diff:.2e})"
                    );
                }
                other => panic!(
                    "clearances[{i}] expected Value::Scalar (length ≈ 0.050 m), got {other:?}"
                ),
            }
        }
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
    }

    /// Swept kinematic query: per-snapshot failures (malformed snapshot missing
    /// `bodies`) become `Value::Undef` in the output list, while other snapshots
    /// still resolve.  List length is always equal to the snapshot count.
    ///
    /// Pins the most subtle invariant documented in `try_eval_swept_kinematic_query`:
    /// "Per-snapshot failures (None) become Value::Undef so the list length is
    /// always equal to the snapshot count."
    #[test]
    fn try_eval_swept_kinematic_query_malformed_snapshot_yields_undef_element() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let src_a = reify_ir::GeometryHandleId(10);
        let src_b = reify_ir::GeometryHandleId(20);

        let mut kernel = MockGeometryKernel::new()
            .with_distance_result(src_a, src_b, reify_ir::Value::Real(0.030)); // 30 mm

        // Well-formed snapshot with identity transforms.
        let make_body = |id: i64, solid: &str| -> reify_ir::Value {
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                reify_ir::Value::String("id".to_string()),
                reify_ir::Value::Int(id),
            );
            m.insert(
                reify_ir::Value::String("solid".to_string()),
                reify_ir::Value::String(solid.to_string()),
            );
            let identity_transform = reify_ir::Value::Transform {
                rotation: Box::new(reify_ir::Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                translation: Box::new(reify_ir::Value::Vector(vec![
                    reify_ir::Value::length(0.0),
                    reify_ir::Value::length(0.0),
                    reify_ir::Value::length(0.0),
                ])),
            };
            m.insert(
                reify_ir::Value::String("world_transform".to_string()),
                identity_transform,
            );
            reify_ir::Value::Map(m)
        };

        // snapshot_good: valid Snapshot Map with a `bodies` list.
        let mut good_map = std::collections::BTreeMap::new();
        good_map.insert(
            reify_ir::Value::String("kind".to_string()),
            reify_ir::Value::String("snapshot".to_string()),
        );
        good_map.insert(
            reify_ir::Value::String("bodies".to_string()),
            reify_ir::Value::List(vec![make_body(1, "body_a"), make_body(2, "body_b")]),
        );
        let snapshot_good = reify_ir::Value::Map(good_map);

        // snapshot_malformed: Map has `kind="snapshot"` but is missing `bodies`.
        // `extract_snapshot_bodies` returns None → `eval_kinematic_on_snapshot`
        // returns Some(Value::Undef).
        let mut bad_map = std::collections::BTreeMap::new();
        bad_map.insert(
            reify_ir::Value::String("kind".to_string()),
            reify_ir::Value::String("snapshot".to_string()),
        );
        let snapshot_malformed = reify_ir::Value::Map(bad_map);

        let snaps_cell = ValueCellId::new("Swept", "snaps");
        let id_a_cell = ValueCellId::new("Swept", "id_a");
        let id_b_cell = ValueCellId::new("Swept", "id_b");
        let s_param = ValueCellId::new("Swept", "s");

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body_a".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_a,
            },
        );
        named_steps.insert(
            "body_b".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_b,
            },
        );

        // List: [snapshot_good, snapshot_malformed].
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            snaps_cell.clone(),
            reify_ir::Value::List(vec![snapshot_good, snapshot_malformed]),
        );
        values.insert(id_a_cell.clone(), reify_ir::Value::Int(1));
        values.insert(id_b_cell.clone(), reify_ir::Value::Int(2));

        // Build flat_map(snaps, |s| [min_clearance(s, id_a, id_b)])
        let s_ref = reify_ir::CompiledExpr::value_ref(s_param.clone(), Type::Geometry);
        let id_a_ref = reify_ir::CompiledExpr::value_ref(id_a_cell.clone(), Type::Int);
        let id_b_ref = reify_ir::CompiledExpr::value_ref(id_b_cell.clone(), Type::Int);

        let inner = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "min_clearance".to_string(),
                    qualified_name: "min_clearance".to_string(),
                },
                args: vec![s_ref, id_a_ref, id_b_ref],
            },
            result_type: Type::dimensionless_scalar(),
            content_hash: reify_core::ContentHash(0),
        };
        let lambda_body = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::ListLiteral(vec![inner]),
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };
        let lambda_arg = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::Lambda {
                params: vec![("s".to_string(), None)],
                param_ids: vec![s_param],
                body: Box::new(lambda_body),
                captures: vec![id_a_cell, id_b_cell],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };
        let snaps_ref =
            reify_ir::CompiledExpr::value_ref(snaps_cell, Type::List(Box::new(Type::Geometry)));
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "flat_map".to_string(),
                    qualified_name: "flat_map".to_string(),
                },
                args: vec![snaps_ref, lambda_arg],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        // Must return Some(Value::List) of length 2 (one per snapshot, not one per resolution).
        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "malformed-snapshot test must return Some(Value::List(len=2)), \
                 got {other:?}; diagnostics: {diagnostics:?}"
            ),
        };
        assert_eq!(
            list.len(),
            2,
            "list length must equal snapshot count (2) even with a malformed element, \
             got {}: {list:?}",
            list.len()
        );

        // Element 0 (good snapshot): resolved to a length Scalar ≈ 0.030 m.
        match &list[0] {
            reify_ir::Value::Scalar { si_value, .. } => {
                let diff = (si_value - 0.030_f64).abs();
                assert!(
                    diff < 1e-9,
                    "list[0] (good snapshot) expected ≈ 0.030 m, got {si_value:.9} m"
                );
            }
            other => panic!("list[0] (good snapshot) expected Value::Scalar, got {other:?}"),
        }

        // Element 1 (malformed snapshot): must be Value::Undef.
        assert_eq!(
            list[1],
            reify_ir::Value::Undef,
            "list[1] (malformed snapshot missing 'bodies') must be Value::Undef, got {:?}",
            list[1]
        );
    }

    /// Contract test (task 4142, Cluster B RED — topology/resolve_geometry_handle_arg leaf):
    /// `try_eval_topology_selector` resolves the geometry handle via
    /// `KernelHandle.id`, ignoring `KernelHandle.kernel`.
    ///
    /// The `edges` path exercises the shared `resolve_geometry_handle_arg` leaf
    /// (geometry_ops.rs:3620), which is the single leaf covering ALL topology
    /// selectors AND the new ghr-zeta geometry-query path.  Proves `.kernel`
    /// is never consulted.
    ///
    /// RED on current main: `try_eval_topology_selector` still takes
    /// `&HashMap<String, GeometryHandleId>` → E0308 type mismatch.
    ///
    /// NOTE: Pins the leaf-projection contract only (`.kernel` unused in the current
    /// single-kernel-per-build design). When cross-kernel handle resolution lands,
    /// update to assert per-kernel dispatch rather than treating `.kernel` as ignored.
    #[test]
    fn try_eval_topology_selector_resolves_via_kernel_handle_id() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_id = reify_ir::GeometryHandleId(1);
        let edge_a = reify_ir::GeometryHandleId(2);
        let edge_b = reify_ir::GeometryHandleId(3);
        let parent_rr = RealizationNodeId::new("EdgeBody", 0);
        let parent_hash: [u8; 32] = [0x42; 32];

        let mut kernel =
            MockGeometryKernel::new().with_extracted_edges(parent_id, vec![edge_a, edge_b]);

        // Map "s" to a KernelHandle with deliberately non-default kernel.
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "s".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Manifold, // non-default: must be ignored
                id: parent_id,
            },
        );

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("EdgeBody", "s"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_id,
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "edges",
            "EdgeBody",
            "s",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Task 4118 (γ): `edges` now constructs a kernel-FREE typed
        // `Value::Selector(Edge)` (All leaf) over the parent solid. The arm
        // resolves its target from `values` (the hydrated Value::GeometryHandle),
        // so `named_steps` — and thus `KernelHandle.kernel` — is not consulted at
        // all; the leaf target carries `parent_id` as its kernel_handle regardless
        // of the deliberately-non-default `KernelId::Manifold` staged in named_steps.
        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "edges with KernelHandle{{Manifold, 1}} must return Some(Value::Selector(..)), \
                 got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Edge,
            "edges → Edge kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, parent_id,
                    "leaf target kernel_handle must be parent_id (resolved from values, \
                     KernelHandle.kernel ignored)"
                );
                assert_eq!(*query, reify_ir::value::LeafQuery::All, "edges → All leaf");
            }
            other => panic!("edges must be a Leaf selector node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for successful edge construction, got: {:?}",
            diagnostics
        );
    }

    // ── try_eval_topology_selector unit tests (task 2324) ────────────────────
    //
    // These tests pin the contract of `try_eval_topology_selector`, the
    // kernel-aware eval-time dispatch surface for the `closest_point`, `is_on`,
    // and `angle_between_surfaces` stdlib helpers. Sibling to the
    // `try_eval_conformance_query_*` and (integration-only) kinematic-query
    // tests above. The function lives in this module (rather than
    // `eval_expr`) because the build pipeline owns both the kernel and the
    // per-realization name → handle map (`named_steps`).

    /// Build a `CompiledExpr` for a stdlib call `helper(<entity>.<member>)` with
    /// a single `ValueRef` arg. Used for the `edges(b)` / `faces(b)` dispatch
    /// unit tests (task 3616 step-5).
    fn topology_selector_call_one_value_ref(
        helper_name: &str,
        entity: &str,
        member: &str,
        arg_type: reify_core::Type,
        result_type: reify_core::Type,
    ) -> reify_ir::CompiledExpr {
        let arg = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member),
            arg_type,
        );
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name))
            .combine(arg.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg],
            },
            result_type,
            content_hash,
        }
    }

    // ── step-5 (task 3616): edges/faces dispatch unit tests ─────────────────
    //
    // These tests verify that the arm emits Value::List(Value::GeometryHandle)
    // via dispatch_filtered_subhandles.

    /// `edges` dispatch returns `Value::List` of three `Value::GeometryHandle`
    /// elements when the mock kernel returns [GHId(2),GHId(3),GHId(4)] and the
    /// `values` map carries the parent `Value::GeometryHandle`. Each element
    /// must carry the parent's `realization_ref`, and the three
    /// `upstream_values_hash` fields must be pairwise distinct (PRD §4 iii).
    #[test]
    fn edges_dispatch_returns_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("BoxEdges", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new().with_extracted_edges(
            parent_handle,
            vec![
                GeometryHandleId(2),
                GeometryHandleId(3),
                GeometryHandleId(4),
            ],
        );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("BoxEdges", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "edges",
            "BoxEdges",
            "b",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Task 4118 (γ): construction is kernel-FREE — `edges(b)` builds a typed
        // `Value::Selector(Edge)` with an `All` leaf over the parent solid handle,
        // NOT an eagerly-extracted `Value::List` of sub-handles. The staged
        // `with_extracted_edges` data is intentionally unused (zero kernel queries
        // during construction, K2/BT7); the `Selector → List<Geometry>` resolution
        // (extraction + per-element canonical hashing) is the ResolveSelector
        // coercion node's job, covered by the try_eval_resolve_selector tests.
        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "expected Some(Value::Selector(..)), got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Edge,
            "edges(b) → Edge kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, parent_handle,
                    "leaf target must be the parent solid handle"
                );
                assert_eq!(
                    *query,
                    reify_ir::value::LeafQuery::All,
                    "edges(b) → All leaf"
                );
            }
            other => panic!("edges(b) must be a Leaf selector node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "kernel-free construction must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// When the `values` map does not carry a `Value::GeometryHandle` for the
    /// arg cell, the `edges` arm must fall through to `None` (cell stays Undef)
    /// rather than partially constructing a sub-handle (PRD invariant #2).
    /// RED: current arm dispatches via `named_steps` regardless of `values` and
    /// returns `Some(Value::List(Value::Int))`.
    #[test]
    fn edges_dispatch_falls_through_to_none_when_parent_not_hydrated() {
        use reify_core::Type;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let mut kernel = MockGeometryKernel::new().with_extracted_edges(
            parent_handle,
            vec![GeometryHandleId(2), GeometryHandleId(3)],
        );

        // named_steps has the handle so the kernel could serve the call …
        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        // … but values has NO Value::GeometryHandle for the arg cell.
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_one_value_ref(
            "edges",
            "BoxEdges",
            "b",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "must fall through to None when parent is not a hydrated Value::GeometryHandle, \
             got {:?}",
            result
        );
    }

    /// Build a `CompiledExpr` for a stdlib call `helper(<entity>.<member_a>,
    /// <entity>.<member_b>)` with two `ValueRef` args resolving to let-bound
    /// cells. Mirrors `conformance_call` above.
    fn topology_selector_call_two_value_refs(
        helper_name: &str,
        entity: &str,
        member_a: &str,
        type_a: reify_core::Type,
        member_b: &str,
        type_b: reify_core::Type,
        result_type: reify_core::Type,
    ) -> reify_ir::CompiledExpr {
        let arg_a = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_a),
            type_a,
        );
        let arg_b = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_b),
            type_b,
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_a, arg_b],
            },
            result_type,
            content_hash,
        }
    }

    /// Build a `CompiledExpr` for `helper(<literal_real>, <literal_real>)` —
    /// used for the literal-arg fall-through defensive tests. Mirrors
    /// `conformance_call_literal_arg` above.
    fn topology_selector_call_literal_args(helper_name: &str) -> reify_ir::CompiledExpr {
        let arg_a =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Real(1.0), reify_core::Type::dimensionless_scalar());
        let arg_b =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Real(2.0), reify_core::Type::dimensionless_scalar());
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_a, arg_b],
            },
            // result_type is unused on the dispatch path — set to a
            // representative value to keep the literal hand-built expression
            // structurally well-formed.
            result_type: reify_core::Type::Bool,
            content_hash,
        }
    }

    /// Build a Value::Point with three Length scalars, mirroring how a
    /// let-bound `point3(x_mm, y_mm, z_mm)` realises in the `values` map.
    fn point3_length_value(x_m: f64, y_m: f64, z_m: f64) -> reify_ir::Value {
        reify_ir::Value::Point(vec![
            reify_ir::Value::length(x_m),
            reify_ir::Value::length(y_m),
            reify_ir::Value::length(z_m),
        ])
    }

    /// Build a Value::Vector with three dimensionless Real components, mirroring
    /// how a let-bound `vec3(x, y, z)` realises in the `values` map.
    /// Analogous to `point3_length_value` above. Used by the `angle` dispatch
    /// unit tests (task 3614, KGQ-ε).
    fn vec3_value(x: f64, y: f64, z: f64) -> reify_ir::Value {
        reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(x),
            reify_ir::Value::Real(y),
            reify_ir::Value::Real(z),
        ])
    }

    #[test]
    fn try_eval_topology_selector_closest_point_kernel_reply_parses_to_point3_length() {
        use reify_test_support::mocks::MockGeometryKernel;
        let body_handle = reify_ir::GeometryHandleId(7);
        // The kernel reply mirrors the `OcctKernel::query()` arm for
        // `ClosestPointOnShape` (lib.rs JSON-Point3 encoding). The dispatcher
        // is expected to parse it and produce a `Value::Point(vec![length(...),
        // length(...), length(...)])`.
        let mut kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            body_handle,
            [10.0, 0.0, 0.0],
            reify_ir::Value::String("{\"x\":5.0,\"y\":0.0,\"z\":0.0}".to_string()),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Bracket", "p"),
            point3_length_value(10.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "closest_point",
            "Bracket",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::point3(reify_core::Type::length()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(point3_length_value(5.0, 0.0, 0.0)),
            "closest_point(p, body) with kernel JSON-Point3 reply must \
             produce Some(Value::Point(vec![length, length, length])) parsed \
             from the JSON; got {:?}",
            result
        );
    }

    #[test]
    fn try_eval_topology_selector_is_on_kernel_reply_returns_bool_with_default_tolerance() {
        use reify_test_support::mocks::MockGeometryKernel;
        let body_handle = reify_ir::GeometryHandleId(11);
        // The dispatcher must use `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` (≈ OCCT's
        // `Precision::Confusion()`, ~1e-7) for the 2-arg `is_on(point, geometry)`
        // form. Recording the mock under exactly this tolerance pins the contract —
        // if the dispatcher ever changes the default, the recorded reply would not
        // be served and the test would fail with `None`.
        let mut kernel = MockGeometryKernel::new().with_point_on_shape_result(
            body_handle,
            [5.0, 0.0, 0.0],
            reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            reify_ir::Value::Bool(true),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Bracket", "p"),
            point3_length_value(5.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "is_on",
            "Bracket",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "is_on(p, body) with kernel reply Bool(true) must produce \
             Some(Value::Bool(true)) (default tolerance DEFAULT_POINT_ON_SHAPE_TOLERANCE_M); got {:?}",
            result
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_returns_angle_scalar() {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_a = reify_ir::GeometryHandleId(31);
        let face_b = reify_ir::GeometryHandleId(37);
        // Kernel returns a raw f64 (radians) — the dispatcher is expected to
        // wrap as `Value::angle(rad)` to match the cell type
        // `Type::angle()`.
        let mut kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_ir::Value::Real(std::f64::consts::FRAC_PI_2),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face_a".to_string(), kh(face_a));
        named_steps.insert("face_b".to_string(), kh(face_b));

        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_core::Type::Geometry,
            "face_b",
            reify_core::Type::Geometry,
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::FRAC_PI_2)),
            "angle_between_surfaces(face_a, face_b) with kernel reply \
             Real(PI/2) must produce Some(Value::angle(PI/2)); got {:?}",
            result
        );
    }

    #[test]
    fn try_eval_topology_selector_closest_point_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        // `closest_point(<literal>, <literal>)` — literal args, no `let`
        // bindings to resolve. The dispatcher must return None *and* never
        // consult the kernel, mirroring `try_eval_conformance_query`'s
        // literal-arg-fall-through contract.
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("closest_point");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "closest_point(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_is_on_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("is_on");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "is_on(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("angle_between_surfaces");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "angle_between_surfaces(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_non_helper_name_returns_none_no_kernel_call() {
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(reify_ir::GeometryHandleId(7)));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Bracket", "p"),
            point3_length_value(0.0, 0.0, 0.0),
        );

        // `volume` is a real stdlib function name but NOT one of the three
        // recognised topology-selector helpers. The dispatch must return
        // None, mirroring the conformance-query contract.
        let expr = topology_selector_call_two_value_refs(
            "volume",
            "Bracket",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::dimensionless_scalar(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "non-helper name 'volume' must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-helper names"
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_scalar_resolves_identically_to_real()
     {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the dispatch's `Real | Scalar` leniency for `dispatch_surface_angle`:
        // a kernel reply of `Value::Scalar { dimension: ANGLE, si_value: PI/2 }`
        // must resolve to `Value::angle(PI/2)`, identically to the `Value::Real(PI/2)`
        // reply pinned by the sibling `..._returns_angle_scalar` test above. Mirrors
        // `kernel_distance`'s Real|Scalar leniency so a future kernel returning a
        // dimensioned Scalar does not regress silently.
        let face_a = reify_ir::GeometryHandleId(31);
        let face_b = reify_ir::GeometryHandleId(37);
        let mut kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_ir::Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_2,
                dimension: reify_core::DimensionVector::ANGLE,
            },
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face_a".to_string(), kh(face_a));
        named_steps.insert("face_b".to_string(), kh(face_b));

        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_core::Type::Geometry,
            "face_b",
            reify_core::Type::Geometry,
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::FRAC_PI_2)),
            "angle_between_surfaces with kernel Scalar(ANGLE, PI/2) reply must \
             resolve identically to a Real(PI/2) reply; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "Scalar reply on the happy path must NOT emit diagnostics, got: {:?}",
            diagnostics
        );
    }

    /// Pins the DIMENSIONLESS leniency documented at the `dispatch_surface_angle`
    /// Scalar arm (see comment block around line 1902). A mock kernel that returns
    /// `Value::Scalar { dimension: DIMENSIONLESS, si_value: x }` must be accepted
    /// alongside ANGLE without emitting any diagnostic, and must resolve to
    /// `Value::angle(x)`. Without this test, tightening the guard to ANGLE-only
    /// would not be caught by the existing ANGLE or Real fixtures.
    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_scalar_dimensionless_resolves_as_angle()
     {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_a = reify_ir::GeometryHandleId(31);
        let face_b = reify_ir::GeometryHandleId(37);
        let mut kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_ir::Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_2,
                dimension: reify_core::DimensionVector::DIMENSIONLESS,
            },
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face_a".to_string(), kh(face_a));
        named_steps.insert("face_b".to_string(), kh(face_b));

        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_core::Type::Geometry,
            "face_b",
            reify_core::Type::Geometry,
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::FRAC_PI_2)),
            "angle_between_surfaces with kernel Scalar(DIMENSIONLESS, PI/2) reply must \
             resolve to Some(Value::angle(PI/2)); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "DIMENSIONLESS Scalar reply must NOT emit diagnostics (intentional leniency), \
             got: {:?}",
            diagnostics
        );
    }

    /// Shared fixture for the two wrong-dim-Scalar tests below. Builds a
    /// `MockGeometryKernel` wired to return a LENGTH-dimensioned Scalar for the
    /// `angle_between_surfaces(face_a, face_b)` call, together with the
    /// `named_steps` map, empty `ValueMap`, and the compiled `expr`. Each test
    /// owns its own `diagnostics` Vec and call site, which is all that differs
    /// between the debug-panic and release-warn cases.
    fn wrong_dim_scalar_fixture() -> (
        reify_ir::CompiledExpr,
        HashMap<String, reify_ir::KernelHandle>,
        reify_ir::ValueMap,
        reify_test_support::mocks::MockGeometryKernel,
    ) {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_a = reify_ir::GeometryHandleId(31);
        let face_b = reify_ir::GeometryHandleId(37);
        // LENGTH is the real-world bug class: metres silently reinterpreted as
        // radians. Using LENGTH (not e.g. MASS) ties the fixture to the actual
        // failure mode described in the task analysis.
        let kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_ir::Value::Scalar {
                si_value: 1.0,
                dimension: reify_core::DimensionVector::LENGTH,
            },
        );
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face_a".to_string(), kh(face_a));
        named_steps.insert("face_b".to_string(), kh(face_b));
        let values = reify_ir::ValueMap::new();
        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_core::Type::Geometry,
            "face_b",
            reify_core::Type::Geometry,
            reify_core::Type::angle(),
        );
        (expr, named_steps, values, kernel)
    }

    /// Pins the defensive dim-check in `dispatch_surface_angle`'s Scalar arm —
    /// mirrors `resolve_point3_length_arg`'s tightened LENGTH check from commit
    /// 8c464177db. A LENGTH-dimensioned Scalar reply must NOT be silently
    /// reinterpreted as radians; the dispatcher must emit a Warning naming the
    /// helper and return Undef. Gated `#[cfg(not(debug_assertions))]` because in
    /// debug builds the same fixture trips the sibling
    /// `..._panics_in_debug_build` test's debug_assert.
    #[cfg(not(debug_assertions))]
    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_scalar_wrong_dimension_emits_warning_and_returns_undef()
     {
        let (expr, named_steps, values, mut kernel) = wrong_dim_scalar_fixture();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "angle_between_surfaces with LENGTH-dimensioned Scalar reply must yield \
             Some(Value::Undef), NOT Some(Value::angle(1.0)); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "wrong-dim Scalar reply must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("angle_between_surfaces"),
            "diagnostic must mention the helper name 'angle_between_surfaces', got: {}",
            diag.message
        );
        // DimensionVector::LENGTH displays as "m" (via its fmt::Display impl).
        // The format string is `"(dimension={}, si_value={})"`  so the rendered
        // fragment is `"dimension=m, si_value="`.  Anchoring past the trailing
        // comma prevents false positives from dimensions that also start with
        // "m" (e.g. m^2, m·s^-1, mol, …).
        assert!(
            diag.message.contains("dimension=m, si_value="),
            "diagnostic must mention the offending dimension anchored by the trailing \
             ', si_value=' (LENGTH displays as 'm'; bare 'dimension=m' would also \
             match m^2, m·… etc.); got: {}",
            diag.message
        );
    }

    /// Pins the debug-mode panic in `dispatch_surface_angle`'s Scalar arm.
    /// Uses the same LENGTH-dimensioned Scalar fixture as the sibling release
    /// test; in debug builds the `debug_assert!` panics before the if-fall-through
    /// runs, so the `#[should_panic]` attribute is the only assertion needed.
    ///
    /// Follows the dual-test pattern from `crates/reify-eval/src/kernel_registry.rs`
    /// (see `emit_kernel_selection_panics_when_total_is_zero` at line 665 and
    /// `warn_if_duplicate_op_repr_pairs_always_emits_warn_on_duplicate` at 685):
    /// pair a `#[cfg(debug_assertions)] #[should_panic]` test with a
    /// `#[cfg(not(debug_assertions))]` test for the release fall-through.
    /// The `#[cfg(debug_assertions)]` guard is required because `debug_assert!`
    /// compiles to a no-op in release builds — `#[should_panic]` would falsely
    /// "pass" in a release build where the panic never fires.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "expected ANGLE")]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_scalar_wrong_dimension_panics_in_debug_build()
     {
        let (expr, named_steps, values, mut kernel) = wrong_dim_scalar_fixture();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        // The debug_assert! in dispatch_surface_angle's Scalar arm must panic
        // with a message containing "expected ANGLE". No assert_eq! after this
        // call — the #[should_panic] attribute drives the assertion.
        super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );
    }

    #[test]
    fn try_eval_topology_selector_is_on_non_bool_kernel_reply_emits_warning_and_returns_undef() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the `Ok(other)` warning arm of `dispatch_point_on_shape`: a kernel
        // reply that is neither `Value::Bool(_)` nor an Err must produce
        // `Some(Value::Undef)` with a Warning diagnostic naming the helper. Defends
        // the contract against a future kernel that mistakenly returns the
        // wrong-typed Value.
        let body_handle = reify_ir::GeometryHandleId(11);
        let mut kernel = MockGeometryKernel::new().with_point_on_shape_result(
            body_handle,
            [5.0, 0.0, 0.0],
            reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            // Wrong type — should trigger the non-Bool warning arm.
            reify_ir::Value::Real(0.5),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Bracket", "p"),
            point3_length_value(5.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "is_on",
            "Bracket",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "is_on(...) with non-Bool kernel reply must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Bool reply must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("is_on"),
            "diagnostic must mention the helper name 'is_on', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("non-Bool"),
            "diagnostic must indicate the non-Bool reply, got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_closest_point_malformed_json_reply_emits_warning_and_returns_undef()
     {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the `Err(err)` parse-failure arm of `dispatch_point3_length_reply`: a
        // kernel reply whose `Value::String(_)` payload is not parseable as a
        // JSON-Point3 must produce `Some(Value::Undef)` with a Warning
        // diagnostic naming the helper. Defends the contract against a future
        // kernel that emits a malformed JSON string.
        let body_handle = reify_ir::GeometryHandleId(7);
        let mut kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            body_handle,
            [10.0, 0.0, 0.0],
            // Not a JSON-Point3 payload — should trigger the parse-failure
            // warning arm.
            reify_ir::Value::String("not a valid json point".to_string()),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Bracket", "p"),
            point3_length_value(10.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "closest_point",
            "Bracket",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::point3(reify_core::Type::length()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "closest_point with malformed JSON reply must yield \
             Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "malformed reply must emit exactly one Warning, got {} \
             diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("closest_point"),
            "diagnostic must mention the helper name 'closest_point', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("parse failed"),
            "diagnostic must indicate the parse failure, got: {}",
            diag.message
        );
    }

    // ── try_eval_topology_selector: angle dispatch (task 3614, KGQ-ε) ────────
    //
    // These tests pin the pure-math `angle(Vec3, Vec3) -> Angle` dispatch arm.
    // No kernel calls — acos(clamp(dot/(|a||b|), -1, 1)).  Modelled on the
    // `try_eval_topology_selector_angle_between_surfaces_*` tests above.

    #[test]
    fn try_eval_topology_selector_angle_two_vec3_value_refs_returns_angle_scalar() {
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "a"),
            vec3_value(1.0, 0.0, 0.0),
        );
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "b"),
            vec3_value(0.0, 1.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "angle",
            "AngleSmoke",
            "a",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "b",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::FRAC_PI_2)),
            "angle(vec3(1,0,0), vec3(0,1,0)) must return Some(Value::angle(PI/2)); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "perpendicular vectors must NOT emit diagnostics, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_parallel_vectors_returns_zero_angle() {
        // vec3(1,0,0) · vec3(2,0,0): cos=1.0 → clamp → acos(1.0) = 0.0.
        // Proves the acos-domain upper-bound clamp for parallel vectors.
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "a"),
            vec3_value(1.0, 0.0, 0.0),
        );
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "b"),
            vec3_value(2.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "angle",
            "AngleSmoke",
            "a",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "b",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(0.0)),
            "angle(vec3(1,0,0), vec3(2,0,0)) (parallel) must return Some(Value::angle(0.0)); \
             got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "parallel vectors must NOT emit diagnostics, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_antiparallel_vectors_returns_pi() {
        // vec3(1,0,0) · vec3(-1,0,0): cos=-1.0 → clamp → acos(-1.0) = π.
        // Proves the acos-domain lower-bound clamp for antiparallel vectors.
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "a"),
            vec3_value(1.0, 0.0, 0.0),
        );
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "b"),
            vec3_value(-1.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "angle",
            "AngleSmoke",
            "a",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "b",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::PI)),
            "angle(vec3(1,0,0), vec3(-1,0,0)) (antiparallel) must return \
             Some(Value::angle(PI)); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "antiparallel vectors must NOT emit diagnostics, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_nonvec3_scalar_literal_args_falls_through_to_none() {
        // angle(<literal_real>, <literal_real>) — scalar Real literals evaluate
        // (task ε) to Value::Real, which resolve_vec3_arg rejects as
        // defined-but-wrong: it pushes a Warning and returns None for args[0],
        // so the `?` short-circuits and the dispatcher returns None WITHOUT
        // consulting the kernel.  Note: an inline expr that EVALUATES to a
        // Value::Vector (e.g. a vec3(...) constructor or a Value::Vector literal)
        // DOES resolve and compute an angle — see
        // `try_eval_topology_selector_angle_literal_vec3_args_resolves_and_returns_angle`.
        // This test pins the non-Vec3 scalar literal case (result None; kernel
        // untouched).
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("angle");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "angle(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_literal_vec3_args_resolves_and_returns_angle() {
        // angle(Literal(vec3(1,0,0)), Literal(vec3(0,1,0))) — resolve_vec3_arg
        // EVALUATES the arg (task ε); a Literal(Value::Vector) evaluates to that
        // Value::Vector and is accepted, so literal vec3 args DO resolve and
        // produce an angle, unlike the scalar-literal case above.  Pins the
        // actually-distinct contract for literal-typed Vec3 args.
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new(); // empty — args come from literals

        // Build angle(Literal(vec3(1,0,0)), Literal(vec3(0,1,0))).
        let arg_a = reify_ir::CompiledExpr::literal(
            vec3_value(1.0, 0.0, 0.0),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let arg_b = reify_ir::CompiledExpr::literal(
            vec3_value(0.0, 1.0, 0.0),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let mut ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("angle"));
        ch = ch.combine(arg_a.content_hash).combine(arg_b.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "angle".to_string(),
                    qualified_name: "angle".to_string(),
                },
                args: vec![arg_a, arg_b],
            },
            result_type: reify_core::Type::angle(),
            content_hash: ch,
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::FRAC_PI_2)),
            "angle(literal vec3(1,0,0), literal vec3(0,1,0)) must resolve to \
             Some(Value::angle(π/2)); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "orthogonal literal vec3 args must NOT emit diagnostics, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_zero_length_vector_returns_undef() {
        // Degenerate input: zero-length vec3(0,0,0) causes |a|=0, division by
        // zero → the dispatcher must emit exactly one Warning and return
        // Some(Value::Undef) rather than propagating NaN.
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "a"),
            vec3_value(0.0, 0.0, 0.0),
        );
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "b"),
            vec3_value(0.0, 1.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "angle",
            "AngleSmoke",
            "a",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "b",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "angle(vec3(0,0,0), ...) with zero-length input must return \
             Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "zero-length vector must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("angle"),
            "diagnostic must mention the helper name 'angle', got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_nonfinite_vector_component_returns_undef() {
        // Degenerate input: a vector with a NaN component causes na = NaN
        // (NaN*NaN + 0 + 0 = NaN, sqrt(NaN) = NaN).  The primary guard
        // `!na.is_finite()` at the dispatch arm must catch this and return
        // Some(Value::Undef) with exactly one Warning — no panic, no NaN-poison.
        // Also tested: f64::INFINITY component → na = inf → same guard fires.
        use reify_test_support::mocks::MockGeometryKernel;

        for (label, ax, ay, az) in [
            ("NaN", f64::NAN, 0.0_f64, 0.0_f64),
            ("INFINITY", f64::INFINITY, 0.0, 0.0),
        ] {
            let mut kernel = MockGeometryKernel::new();
            let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
            let mut values = reify_ir::ValueMap::new();
            values.insert(
                reify_core::ValueCellId::new("T", "a"),
                vec3_value(ax, ay, az),
            );
            values.insert(
                reify_core::ValueCellId::new("T", "b"),
                vec3_value(0.0, 1.0, 0.0),
            );
            let expr = topology_selector_call_two_value_refs(
                "angle",
                "T",
                "a",
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
                "b",
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
                reify_core::Type::angle(),
            );
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            let result = super::try_eval_topology_selector(
                &expr,
                &named_steps,
                &values,
                &mut kernel,
                &mut diagnostics,
            );
            assert_eq!(
                result,
                Some(reify_ir::Value::Undef),
                "angle(vec3({label},...), ...) must return Some(Value::Undef); got {result:?}"
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "non-finite component must emit exactly one Warning \
                 (label={label}), got {} diagnostics: {diagnostics:?}",
                diagnostics.len()
            );
            assert_eq!(
                diagnostics[0].severity,
                reify_core::Severity::Warning,
                "diagnostic severity must be Warning (label={label}), got {:?}",
                diagnostics[0].severity
            );
        }
    }

    // ── try_eval_topology_selector `contains` unit tests (task 3611, KGQ-β) ────
    //
    // These tests pin the `contains(solid, point) -> Bool` dispatch contract
    // (PRD §9 KGQ-β). Arg order is solid-then-point (args[0]=geometry,
    // args[1]=point3<Length>), mirroring `is_on` with args swapped. The
    // dispatcher reuses `dispatch_point_on_shape` (Bool unwrapper) and
    // `DEFAULT_CONTAINS_TOLERANCE_M` per §5.2.
    //
    // Three contracts:
    //   (a) happy path: kernel Bool(true) reply → Some(Value::Bool(true))
    //   (b) literal-arg fall-through: non-ValueRef args → None, no kernel call
    //   (c) non-Bool kernel reply → Some(Value::Undef) + exactly-one Warning
    //       naming "contains"
    //
    // All three FAIL (RED) until step-6 wires the `contains` arm in
    // try_eval_topology_selector / TopologySelectorHelper.

    #[test]
    fn try_eval_topology_selector_contains_kernel_reply_returns_bool_with_default_tolerance() {
        use reify_test_support::mocks::MockGeometryKernel;
        let body_handle = reify_ir::GeometryHandleId(42);
        // Record the mock under DEFAULT_CONTAINS_TOLERANCE_M — pins that the
        // dispatcher uses this constant and not some ad-hoc value.
        let mut kernel = MockGeometryKernel::new().with_contains_result(
            body_handle,
            [0.0, 0.0, 0.0],
            reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
            reify_ir::Value::Bool(true),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // args[0] = solid → resolved via named_steps by member name "solid"
        named_steps.insert("solid".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[1] = point → resolved via values by ValueCellId
        values.insert(
            reify_core::ValueCellId::new("ContainsBox", "center"),
            point3_length_value(0.0, 0.0, 0.0),
        );

        // contains(solid, center): args[0]=solid (Geometry), args[1]=center (Point3<Length>)
        let expr = topology_selector_call_two_value_refs(
            "contains",
            "ContainsBox",
            "solid",
            reify_core::Type::Geometry,
            "center",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "contains(solid, center) with kernel reply Bool(true) must produce \
             Some(Value::Bool(true)) (default tolerance DEFAULT_CONTAINS_TOLERANCE_M); \
             got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path contains must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_contains_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        // `contains(<literal>, <literal>)` — non-ValueRef args, so both
        // resolve_geometry_handle_arg and resolve_point3_length_arg return None,
        // and the dispatcher must return None without consulting the kernel.
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("contains");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "contains(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_contains_non_bool_kernel_reply_emits_warning_and_returns_undef() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the `Ok(other)` warning arm of `dispatch_point_on_shape` (reused for
        // `contains`): a kernel reply that is not `Value::Bool(_)` must produce
        // `Some(Value::Undef)` with a Warning diagnostic naming "contains".
        let body_handle = reify_ir::GeometryHandleId(42);
        let mut kernel = MockGeometryKernel::new().with_contains_result(
            body_handle,
            [0.0, 0.0, 0.0],
            reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
            // Wrong type — should trigger the non-Bool warning arm.
            reify_ir::Value::Real(0.5),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("solid".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("ContainsBox", "center"),
            point3_length_value(0.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "contains",
            "ContainsBox",
            "solid",
            reify_core::Type::Geometry,
            "center",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "contains(...) with non-Bool kernel reply must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Bool reply must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("contains"),
            "diagnostic must mention the helper name 'contains', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("non-Bool"),
            "diagnostic must indicate the non-Bool reply, got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_contains_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        // No `with_contains_result` seeding — MockGeometryKernel.query() falls
        // through to the generic handle-only map which also has no entry for
        // this handle, so it returns `Err(QueryError::QueryFailed(...))`.
        // `dispatch_point_on_shape` must downgrade this to `Some(Value::Undef)`
        // and emit exactly one Warning diagnostic naming "contains" and
        // "kernel query failed". Pins the `Err(err)` arm of that helper.
        let body_handle = reify_ir::GeometryHandleId(42);
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("solid".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("ContainsBox", "center"),
            point3_length_value(0.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "contains",
            "ContainsBox",
            "solid",
            reify_core::Type::Geometry,
            "center",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "contains(...) with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("contains"),
            "diagnostic must mention the helper name 'contains', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("kernel query failed"),
            "diagnostic must indicate the kernel failure, got: {}",
            diag.message
        );
    }

    // ── distance unit tests (task 3610, KGQ-α) ──────────────────────────────
    //
    // Tests for `try_eval_topology_selector` with the `distance` helper (step-1
    // RED driver and step-3/5 contract pins).
    //
    // Step-1a (RED driver): Shape×Point happy path. Asserts that
    // `distance(shape_ref, point_ref)` with a canned ClosestPointOnShape
    // mock reply returns `Some(Value::Scalar{LENGTH, si_value ≈ 0.015})`.
    // RED before step-2 because `distance` is absent from the name-match
    // (the function returns `None` immediately at the `_ => return None` arm).

    #[test]
    fn try_eval_topology_selector_distance_shape_point_happy_path() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Box handle inserted into named_steps under member "b".
        let box_handle = reify_ir::GeometryHandleId(99);
        // Mock: ClosestPointOnShape query for (box_handle, point=(0.02,0,0))
        // replies with the closest surface point (0.005, 0.0, 0.0) as JSON.
        // Euclidean distance = |(0.02,0,0) - (0.005,0,0)| = 0.015 m.
        let mut kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            box_handle,
            [0.02, 0.0, 0.0],
            reify_ir::Value::String("{\"x\":0.005,\"y\":0.0,\"z\":0.0}".to_string()),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // args[0] = b (Shape) → named_steps by member name "b"
        named_steps.insert("b".to_string(), kh(box_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[1] = p (Point3<Length>) → values by ValueCellId
        // 20mm = 0.02m in SI
        values.insert(
            reify_core::ValueCellId::new("DistanceBoxPoint", "p"),
            point3_length_value(0.02, 0.0, 0.0),
        );

        // distance(b, p): args[0]=b (Geometry), args[1]=p (Point3<Length>)
        let expr = topology_selector_call_two_value_refs(
            "distance",
            "DistanceBoxPoint",
            "b",
            reify_core::Type::Geometry,
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Expected: Some(Value::Scalar{ dimension: LENGTH, si_value ≈ 0.015 })
        match result {
            Some(reify_ir::Value::Scalar {
                si_value,
                dimension,
            }) if dimension == reify_core::DimensionVector::LENGTH => {
                let expected = 0.015_f64;
                let epsilon = 1e-12;
                assert!(
                    (si_value - expected).abs() < epsilon,
                    "distance(box, point) si_value should be 0.015 (≈{expected:.15}), \
                     got {si_value:.15} (delta {delta:.3e})",
                    delta = (si_value - expected).abs()
                );
            }
            other => panic!(
                "distance(shape, point) with canned ClosestPointOnShape reply must return \
                 Some(Value::Scalar{{LENGTH, ≈0.015}}); got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "happy-path distance must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    // Step-3 RED tests: error-downgrade contract (invariant #3) and
    // non-ValueRef fall-through (invariant #1).
    //
    // (a) Invariant #3 (error downgrade): distance(shape_ref, point_ref) whose
    // ClosestPointOnShape query returns Err must produce Some(Value::Undef) AND
    // exactly one Severity::Warning diagnostic — NOT None.
    // RED against step-2's naive `.ok()? → None` path.
    //
    // (b) Invariant #1 (ValueRef contract): distance(<literal>, <literal>)
    // must return None without consulting the kernel.

    #[test]
    fn try_eval_topology_selector_distance_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        // No with_closest_point_on_shape_result seeding — the mock returns
        // Err(QueryError::QueryFailed(...)) for unregistered queries.
        // The step-2 naive `.ok()?` path returns None (RED); step-4 replaces
        // it with dispatch_point3_length_reply which downgrades to Undef+Warning.
        let box_handle = reify_ir::GeometryHandleId(55);
        let mut kernel = MockGeometryKernel::new(); // no canned reply for this handle+point

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("b".to_string(), kh(box_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("DistanceBoxPoint", "p"),
            point3_length_value(0.02, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "distance",
            "DistanceBoxPoint",
            "b",
            reify_core::Type::Geometry,
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "distance(shape, point) with kernel Err must yield Some(Value::Undef) \
             (not None); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("distance"),
            "diagnostic must mention the helper name 'distance'; got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_distance_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        // distance(Real(1.0), Real(2.0)) — each arg is a defined-but-unusable
        // value (not a shape, not a Point<Length>). The dispatcher returns None
        // without consulting the kernel, but under task ε (evaluate-then-accept)
        // it is no longer SILENT: the point probe emits one Severity::Warning per
        // arg naming `distance` (FLIP from the prior silent fall-through).
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("distance");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "distance(<literal>, <literal>) must return None; got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
        // FLIP (task ε): one Warning per defined-but-unusable arg, naming the
        // `distance` builtin. The result still degrades to None.
        assert_eq!(
            diagnostics.len(),
            2,
            "distance(<literal>, <literal>) must emit one Warning per arg \
             (FLIP from silent), got: {:?}",
            diagnostics
        );
        for d in &diagnostics {
            assert_eq!(d.severity, reify_core::Severity::Warning);
            assert!(
                d.message.to_lowercase().contains("distance"),
                "warning must name the distance builtin, got: {:?}",
                d.message
            );
        }
    }

    // Step-5 RED tests: remaining arg combinations (Shape×Shape, Point×Point).
    //
    // (a) Shape×Shape: both members in named_steps, kernel seeded with
    // with_distance_result(handle_a, handle_b, meters(0.04)) → must produce
    // Some(Value::Scalar{LENGTH, ≈0.04}). RED: step-4 only handles Shape×Point;
    // (Some(shapeA), None, Some(shapeB), None) hits `_ => None`.
    //
    // (b) Point×Point: both in values, no kernel call → pure Euclidean result.
    // (0,0,0) to (0.03,0.04,0.0) = 0.05 (3-4-5). RED: same placeholder.

    #[test]
    fn try_eval_topology_selector_distance_shape_shape_uses_kernel_distance() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_test_support::values::meters;
        let handle_a = reify_ir::GeometryHandleId(10);
        let handle_b = reify_ir::GeometryHandleId(11);
        // kernel_distance reads GeometryQuery::Distance{from, to} and accepts
        // Real or Scalar{LENGTH} reply. Use meters(0.04) (Value::Scalar{LENGTH}).
        let mut kernel =
            MockGeometryKernel::new().with_distance_result(handle_a, handle_b, meters(0.04));

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("a".to_string(), kh(handle_a));
        named_steps.insert("b".to_string(), kh(handle_b));

        let values = reify_ir::ValueMap::new();

        // distance(a, b): both args are Geometry (named_steps).
        let expr = topology_selector_call_two_value_refs(
            "distance",
            "ShapeShape",
            "a",
            reify_core::Type::Geometry,
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        match result {
            Some(reify_ir::Value::Scalar {
                si_value,
                dimension,
            }) if dimension == reify_core::DimensionVector::LENGTH => {
                let expected = 0.04_f64;
                let epsilon = 1e-12;
                assert!(
                    (si_value - expected).abs() < epsilon,
                    "distance(shapeA, shapeB) si_value should be 0.04; \
                     got {si_value:.15} (delta {delta:.3e})",
                    delta = (si_value - expected).abs()
                );
            }
            other => panic!(
                "distance(shapeA, shapeB) with kernel Distance reply must return \
                 Some(Value::Scalar{{LENGTH, ≈0.04}}); got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "happy-path Shape×Shape distance must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_distance_point_point_pure_euclidean() {
        use reify_test_support::mocks::CountingMockKernel;
        // Point×Point: both args resolved from values; pure Euclidean, no kernel.
        // (0,0,0) to (0.03,0.04,0) = 0.05 (3-4-5 right triangle).
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("PointPoint", "pa"),
            point3_length_value(0.0, 0.0, 0.0),
        );
        values.insert(
            reify_core::ValueCellId::new("PointPoint", "pb"),
            point3_length_value(0.03, 0.04, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "distance",
            "PointPoint",
            "pa",
            reify_core::Type::point3(reify_core::Type::length()),
            "pb",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        match result {
            Some(reify_ir::Value::Scalar {
                si_value,
                dimension,
            }) if dimension == reify_core::DimensionVector::LENGTH => {
                let expected = 0.05_f64; // |(0.03, 0.04, 0)| = 0.05 exactly
                let epsilon = 1e-12;
                assert!(
                    (si_value - expected).abs() < epsilon,
                    "distance(pointA, pointB) pure Euclidean should be 0.05; \
                     got {si_value:.15} (delta {delta:.3e})",
                    delta = (si_value - expected).abs()
                );
            }
            other => panic!(
                "distance(pointA, pointB) must return Some(Value::Scalar{{LENGTH, ≈0.05}}); \
                 got {:?}",
                other
            ),
        }
        assert_eq!(
            kernel.total_query_count(),
            0,
            "Point×Point distance must not consult the kernel; got {} queries",
            kernel.total_query_count()
        );
        assert!(
            diagnostics.is_empty(),
            "Point×Point distance must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    // ── Amendment tests for distance dispatch (reviewer suggestions) ─────────
    //
    // These tests were added as part of the code-review amendment pass to
    // address three reviewer observations:
    //
    //   1. Point×Shape happy-path (symmetric arm — could regress independently
    //      of Shape×Point after the deduplication refactor).
    //   2. Shape×Shape error-downgrade (invariant #3 for the S×S branch: kernel
    //      Err → Some(Value::Undef) + one Warning, not None).
    //   3. Invariant #4 (exactly one kernel query) for Shape×Point and Shape×Shape
    //      success paths (previously only the zero-query Point cases were pinned).

    #[test]
    fn try_eval_topology_selector_distance_point_shape_happy_path() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Point × Shape: args swapped versus the Shape×Point test; the
        // normalised dispatch should route to the same ClosestPointOnShape block.
        let box_handle = reify_ir::GeometryHandleId(99);
        let mut kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            box_handle,
            [0.02, 0.0, 0.0],
            reify_ir::Value::String("{\"x\":0.005,\"y\":0.0,\"z\":0.0}".to_string()),
        );

        // arg0 = p (Point3<Length>) → values map
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("PointShape", "p"),
            point3_length_value(0.02, 0.0, 0.0),
        );
        // arg1 = b (Shape) → named_steps
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("b".to_string(), kh(box_handle));

        // distance(p, b): args[0]=p (Point3), args[1]=b (Geometry)
        let expr = topology_selector_call_two_value_refs(
            "distance",
            "PointShape",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        match result {
            Some(reify_ir::Value::Scalar {
                si_value,
                dimension,
            }) if dimension == reify_core::DimensionVector::LENGTH => {
                let expected = 0.015_f64;
                let epsilon = 1e-12;
                assert!(
                    (si_value - expected).abs() < epsilon,
                    "distance(point, shape) si_value should be 0.015 (≈{expected:.15}), \
                     got {si_value:.15} (delta {delta:.3e})",
                    delta = (si_value - expected).abs()
                );
            }
            other => panic!(
                "distance(point, shape) with canned ClosestPointOnShape reply must return \
                 Some(Value::Scalar{{LENGTH, ≈0.015}}); got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "happy-path Point×Shape distance must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_distance_shape_shape_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Shape × Shape with NO seeded Distance result → mock returns Err for
        // the unregistered (handle_a, handle_b) pair.  Invariant #3 contract:
        // kernel_distance maps Err → None → Some(Value::Undef) + one Warning.
        let handle_a = reify_ir::GeometryHandleId(10);
        let handle_b = reify_ir::GeometryHandleId(11);
        let mut kernel = MockGeometryKernel::new(); // no Distance reply seeded

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("a".to_string(), kh(handle_a));
        named_steps.insert("b".to_string(), kh(handle_b));
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "distance",
            "ShapeShape",
            "a",
            reify_core::Type::Geometry,
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "distance(shapeA, shapeB) with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "Shape×Shape kernel Err must emit exactly one Warning; got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("distance"),
            "diagnostic must mention the helper name 'distance'; got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_distance_shape_point_query_count_is_one() {
        use reify_test_support::mocks::{CountingMockKernel, MockGeometryKernel};
        // Invariant #4: Shape×Point success path issues exactly one kernel query.
        let box_handle = reify_ir::GeometryHandleId(99);
        let inner = MockGeometryKernel::new().with_closest_point_on_shape_result(
            box_handle,
            [0.02, 0.0, 0.0],
            reify_ir::Value::String("{\"x\":0.005,\"y\":0.0,\"z\":0.0}".to_string()),
        );
        let mut kernel = CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("b".to_string(), kh(box_handle));
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("DistanceBoxPoint", "p"),
            point3_length_value(0.02, 0.0, 0.0),
        );
        let expr = topology_selector_call_two_value_refs(
            "distance",
            "DistanceBoxPoint",
            "b",
            reify_core::Type::Geometry,
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );
        assert!(
            result.is_some(),
            "Shape×Point happy path must return Some; got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            1,
            "Shape×Point distance must issue exactly one kernel query (invariant #4); got {}",
            kernel.total_query_count()
        );
    }

    #[test]
    fn try_eval_topology_selector_distance_shape_shape_query_count_is_one() {
        use reify_test_support::mocks::{CountingMockKernel, MockGeometryKernel};
        use reify_test_support::values::meters;
        // Invariant #4: Shape×Shape success path issues exactly one kernel query.
        let handle_a = reify_ir::GeometryHandleId(10);
        let handle_b = reify_ir::GeometryHandleId(11);
        let inner =
            MockGeometryKernel::new().with_distance_result(handle_a, handle_b, meters(0.04));
        let mut kernel = CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("a".to_string(), kh(handle_a));
        named_steps.insert("b".to_string(), kh(handle_b));
        let values = reify_ir::ValueMap::new();
        let expr = topology_selector_call_two_value_refs(
            "distance",
            "ShapeShape",
            "a",
            reify_core::Type::Geometry,
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );
        assert!(
            result.is_some(),
            "Shape×Shape happy path must return Some; got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            1,
            "Shape×Shape distance must issue exactly one kernel query (invariant #4); got {}",
            kernel.total_query_count()
        );
    }

    // ── gate_query_capability unit tests (task 3623) ─────────────────────────
    //
    // These tests pin the §5.4 four-branch policy contract of
    // `gate_query_capability`. The function lives in this module (pub(crate))
    // and is tested here following the established in-module pattern for
    // `try_eval_conformance_query_*` / `try_eval_topology_selector_*`.
    //
    // Coverage map (PRD §8.1):
    //  branch-a: BRepOnly + BRep    → Occt,        zero diagnostics
    //  branch-b: BRepAndMesh + BRep → Occt,        zero diagnostics
    //  branch-c: BRepAndMesh + Mesh → Manifold,    zero diagnostics
    //  branch-d: BRepOnly + Mesh    → Unsupported, exactly-one Error
    //            with code QueryNotSupportedOnRepr, message contains
    //            helper name + repr token
    //  branch-e: any capability + Voxel/Sdf/VolumeMesh → Unsupported + diag
    //  branch-f: exhaustive no-panic loop over all 5 ReprKind values for both
    //            BRepOnly and BRepAndMesh; Unsupported ⟺ exactly-one-diagnostic

    #[test]
    fn gate_query_capability_brep_only_on_brep_routes_occt_no_diag() {
        // branch-a: BRepOnly + BRep → Occt
        let query = reify_ir::GeometryQuery::EdgeLength(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_ir::ReprKind::BRep,
            "edge_length",
            &mut diags,
        );
        assert_eq!(
            route,
            super::CapabilityRoute::Occt,
            "BRepOnly query on BRep repr must route to Occt"
        );
        assert!(
            diags.is_empty(),
            "BRepOnly on BRep must emit zero diagnostics; got: {:?}",
            diags
        );
    }

    #[test]
    fn gate_query_capability_brep_and_mesh_on_brep_routes_occt_no_diag() {
        // branch-b: BRepAndMesh + BRep → Occt
        let query = reify_ir::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route =
            super::gate_query_capability(&query, reify_ir::ReprKind::BRep, "distance", &mut diags);
        assert_eq!(
            route,
            super::CapabilityRoute::Occt,
            "BRepAndMesh query on BRep repr must route to Occt"
        );
        assert!(
            diags.is_empty(),
            "BRepAndMesh on BRep must emit zero diagnostics; got: {:?}",
            diags
        );
    }

    #[test]
    fn gate_query_capability_brep_and_mesh_on_mesh_routes_manifold_no_diag() {
        // branch-c: BRepAndMesh + Mesh → Manifold
        let query = reify_ir::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route =
            super::gate_query_capability(&query, reify_ir::ReprKind::Mesh, "distance", &mut diags);
        assert_eq!(
            route,
            super::CapabilityRoute::Manifold,
            "BRepAndMesh query on Mesh repr must route to Manifold"
        );
        assert!(
            diags.is_empty(),
            "BRepAndMesh on Mesh must emit zero diagnostics; got: {:?}",
            diags
        );
    }

    #[test]
    fn gate_query_capability_brep_only_on_mesh_fails_closed_with_diag() {
        // branch-d: BRepOnly + Mesh → Unsupported + exactly-one Error diag
        let query = reify_ir::GeometryQuery::EdgeLength(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route =
            super::gate_query_capability(&query, reify_ir::ReprKind::Mesh, "curvature", &mut diags);
        assert_eq!(
            route,
            super::CapabilityRoute::Unsupported,
            "BRepOnly query on Mesh repr must route to Unsupported (fail closed)"
        );
        assert_eq!(
            diags.len(),
            1,
            "BRepOnly on Mesh must emit exactly one diagnostic; got {} ({:?})",
            diags.len(),
            diags
        );
        let diag = &diags[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Error,
            "diagnostic severity must be Error, got {:?}",
            diag.severity
        );
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::QueryNotSupportedOnRepr),
            "diagnostic code must be QueryNotSupportedOnRepr, got {:?}",
            diag.code
        );
        assert!(
            diag.message.contains("curvature"),
            "diagnostic must contain the helper name 'curvature'; got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("Mesh"),
            "diagnostic must contain the repr token 'Mesh'; got: {}",
            diag.message
        );
    }

    #[test]
    fn gate_query_capability_any_query_on_voxel_fails_closed() {
        // branch-e (Voxel): BRepAndMesh query + Voxel → Unsupported + one diag
        // Message must say "BRep or Mesh" (not just "BRep") because Distance is BRepAndMesh.
        let query = reify_ir::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route =
            super::gate_query_capability(&query, reify_ir::ReprKind::Voxel, "distance", &mut diags);
        assert_eq!(route, super::CapabilityRoute::Unsupported);
        assert_eq!(diags.len(), 1, "Voxel repr must emit one diag: {:?}", diags);
        assert_eq!(
            diags[0].code,
            Some(reify_core::DiagnosticCode::QueryNotSupportedOnRepr)
        );
        assert!(
            diags[0].message.contains("BRep or Mesh"),
            "BRepAndMesh query on Voxel must say 'BRep or Mesh', not just 'BRep'; \
             got: {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("Voxel"),
            "diagnostic must contain repr token 'Voxel'; got: {}",
            diags[0].message
        );
    }

    #[test]
    fn gate_query_capability_any_query_on_sdf_fails_closed() {
        // branch-e (Sdf): BRepAndMesh query + Sdf → Unsupported + one diag
        // Message must say "BRep or Mesh" because Volume is BRepAndMesh.
        let query = reify_ir::GeometryQuery::Volume(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route =
            super::gate_query_capability(&query, reify_ir::ReprKind::Sdf, "volume", &mut diags);
        assert_eq!(route, super::CapabilityRoute::Unsupported);
        assert_eq!(diags.len(), 1, "Sdf repr must emit one diag: {:?}", diags);
        assert_eq!(
            diags[0].code,
            Some(reify_core::DiagnosticCode::QueryNotSupportedOnRepr)
        );
        assert!(
            diags[0].message.contains("BRep or Mesh"),
            "BRepAndMesh query on Sdf must say 'BRep or Mesh'; got: {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("Sdf"),
            "diagnostic must contain repr token 'Sdf'; got: {}",
            diags[0].message
        );
    }

    #[test]
    fn gate_query_capability_any_query_on_volume_mesh_fails_closed() {
        // branch-e (VolumeMesh): BRepAndMesh query + VolumeMesh → Unsupported + one diag
        // Message must say "BRep or Mesh" because BoundingBox is BRepAndMesh.
        let query = reify_ir::GeometryQuery::BoundingBox(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_ir::ReprKind::VolumeMesh,
            "bounding_box",
            &mut diags,
        );
        assert_eq!(route, super::CapabilityRoute::Unsupported);
        assert_eq!(
            diags.len(),
            1,
            "VolumeMesh repr must emit one diag: {:?}",
            diags
        );
        assert_eq!(
            diags[0].code,
            Some(reify_core::DiagnosticCode::QueryNotSupportedOnRepr)
        );
        assert!(
            diags[0].message.contains("BRep or Mesh"),
            "BRepAndMesh query on VolumeMesh must say 'BRep or Mesh'; got: {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("VolumeMesh"),
            "diagnostic must contain repr token 'VolumeMesh'; got: {}",
            diags[0].message
        );
    }

    #[test]
    fn gate_query_capability_exhaustive_no_panic_unsupported_iff_one_diag() {
        // branch-f: no-panic loop over all 5 ReprKind values for two queries
        // (one BRepOnly, one BRepAndMesh); invariant: Unsupported ⟺ exactly
        // one diagnostic with code QueryNotSupportedOnRepr.
        let all_reprs = [
            reify_ir::ReprKind::BRep,
            reify_ir::ReprKind::Mesh,
            reify_ir::ReprKind::Sdf,
            reify_ir::ReprKind::Voxel,
            reify_ir::ReprKind::VolumeMesh,
        ];
        let brep_only_query = reify_ir::GeometryQuery::EdgeLength(GeometryHandleId(1));
        let brep_and_mesh_query = reify_ir::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        for repr in all_reprs {
            for (query, label) in [
                (&brep_only_query, "edge_length"),
                (&brep_and_mesh_query, "distance"),
            ] {
                let mut diags: Vec<Diagnostic> = Vec::new();
                let route = super::gate_query_capability(query, repr, label, &mut diags);
                if matches!(route, super::CapabilityRoute::Unsupported) {
                    assert_eq!(
                        diags.len(),
                        1,
                        "Unsupported route for {label}/{repr:?} must emit exactly one diag; got {} ({:?})",
                        diags.len(),
                        diags
                    );
                    assert_eq!(
                        diags[0].code,
                        Some(reify_core::DiagnosticCode::QueryNotSupportedOnRepr),
                        "Unsupported diag must carry QueryNotSupportedOnRepr code"
                    );
                } else {
                    assert!(
                        diags.is_empty(),
                        "non-Unsupported route for {label}/{repr:?} must emit zero diagnostics; got: {:?}",
                        diags
                    );
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Unit tests for `quaternion_from_z_to_axis` (task 3463 amend pass)
    //
    // Covers the four canonical axis directions and the degenerate (-Z) fallback.
    // Each test verifies unit norm AND correct component values.
    // ─────────────────────────────────────────────────────────────────────────

    /// Helper: apply quaternion `(w, qx, qy, qz)` to a pure vector `(vx, vy, vz)`.
    /// Returns the rotated vector `[rx, ry, rz]`.
    ///
    /// Uses the Rodrigues-style formula:
    ///   `v' = v + 2w*(q_vec × v) + 2*(q_vec × (q_vec × v))`
    fn quat_rotate(w: f64, qx: f64, qy: f64, qz: f64, vx: f64, vy: f64, vz: f64) -> [f64; 3] {
        let cx = qy * vz - qz * vy;
        let cy = qz * vx - qx * vz;
        let cz = qx * vy - qy * vx;
        let dx = qy * cz - qz * cy;
        let dy = qz * cx - qx * cz;
        let dz = qx * cy - qy * cx;
        [
            vx + 2.0 * w * cx + 2.0 * dx,
            vy + 2.0 * w * cy + 2.0 * dy,
            vz + 2.0 * w * cz + 2.0 * dz,
        ]
    }

    /// Helper: extract `(w, x, y, z)` from a `Value::Orientation`.
    fn orientation_components(v: reify_ir::Value) -> (f64, f64, f64, f64) {
        match v {
            reify_ir::Value::Orientation { w, x, y, z } => (w, x, y, z),
            other => panic!("expected Value::Orientation, got {other:?}"),
        }
    }

    /// `+Z → +Z`: shortest arc is zero rotation → identity quaternion.
    ///
    /// All arithmetic is exact (w_unnorm = 2.0, len = 2.0, components are
    /// integer multiples of 0.5), so `assert_eq!` with bit-exact comparison
    /// is appropriate here.
    #[test]
    fn quaternion_from_z_to_axis_z_plus_is_identity() {
        let q = super::quaternion_from_z_to_axis(0.0, 0.0, 1.0);
        assert_eq!(
            q,
            reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            "+Z → +Z should yield identity quaternion"
        );
    }

    /// `(0, 0, -1)` is the degenerate case (anti-parallel). The function
    /// falls back to a 180° rotation around +X: `{w:0, x:1, y:0, z:0}`.
    #[test]
    fn quaternion_from_z_to_axis_z_minus_gives_180_around_x() {
        let q = super::quaternion_from_z_to_axis(0.0, 0.0, -1.0);
        assert_eq!(
            q,
            reify_ir::Value::Orientation {
                w: 0.0,
                x: 1.0,
                y: 0.0,
                z: 0.0
            },
            "-Z degenerate case should fall back to 180° around +X"
        );
        // Round-trip: applying the quaternion to (0,0,1) should give (0,0,-1).
        let (w, x, y, z) = orientation_components(q);
        let rotated = quat_rotate(w, x, y, z, 0.0, 0.0, 1.0);
        assert!(
            rotated[0].abs() < 1e-12
                && rotated[1].abs() < 1e-12
                && (rotated[2] + 1.0).abs() < 1e-12,
            "180°/+X applied to +Z should give -Z, got {rotated:?}"
        );
    }

    /// `+X axis`: shortest arc from +Z to +X is 90° around +Y.
    /// `quaternion_from_z_to_axis(1,0,0)` → `{w: 1/√2, x: 0, y: 1/√2, z: 0}`.
    #[test]
    fn quaternion_from_z_to_axis_x_plus_unit_norm_and_round_trip() {
        let q = super::quaternion_from_z_to_axis(1.0, 0.0, 0.0);
        let (w, x, y, z) = orientation_components(q);
        let norm = (w * w + x * x + y * y + z * z).sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-12,
            "+X axis: quaternion should be unit-norm; norm={norm}"
        );
        let sqrt2_inv = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (w - sqrt2_inv).abs() < 1e-12,
            "+X axis: w should be 1/√2; got {w}"
        );
        assert!(x.abs() < 1e-12, "+X axis: x should be 0; got {x}");
        assert!(
            (y - sqrt2_inv).abs() < 1e-12,
            "+X axis: y should be 1/√2; got {y}"
        );
        assert!(z.abs() < 1e-12, "+X axis: z should be 0; got {z}");
        // Round-trip: q applied to (0,0,1) should give (1,0,0).
        let rotated = quat_rotate(w, x, y, z, 0.0, 0.0, 1.0);
        assert!(
            (rotated[0] - 1.0).abs() < 1e-12
                && rotated[1].abs() < 1e-12
                && rotated[2].abs() < 1e-12,
            "+X round-trip: expected (1,0,0), got {rotated:?}"
        );
    }

    /// `+Y axis`: shortest arc from +Z to +Y is 90° around -X.
    /// `quaternion_from_z_to_axis(0,1,0)` → `{w: 1/√2, x: -1/√2, y: 0, z: 0}`.
    #[test]
    fn quaternion_from_z_to_axis_y_plus_unit_norm_and_round_trip() {
        let q = super::quaternion_from_z_to_axis(0.0, 1.0, 0.0);
        let (w, x, y, z) = orientation_components(q);
        let norm = (w * w + x * x + y * y + z * z).sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-12,
            "+Y axis: quaternion should be unit-norm; norm={norm}"
        );
        let sqrt2_inv = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (w - sqrt2_inv).abs() < 1e-12,
            "+Y axis: w should be 1/√2; got {w}"
        );
        assert!(
            (x + sqrt2_inv).abs() < 1e-12,
            "+Y axis: x should be -1/√2; got {x}"
        );
        assert!(y.abs() < 1e-12, "+Y axis: y should be 0; got {y}");
        assert!(z.abs() < 1e-12, "+Y axis: z should be 0; got {z}");
        // Round-trip: q applied to (0,0,1) should give (0,1,0).
        let rotated = quat_rotate(w, x, y, z, 0.0, 0.0, 1.0);
        assert!(
            rotated[0].abs() < 1e-12
                && (rotated[1] - 1.0).abs() < 1e-12
                && rotated[2].abs() < 1e-12,
            "+Y round-trip: expected (0,1,0), got {rotated:?}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // FrameSubShapeKind::from_selector_kind conversion contract
    //
    // These tests pin the Face→Some(Face), Edge→Some(Edge), Point→None contract
    // for the narrowed enum that eliminates `unreachable!()` arms in the
    // kernel-aware dispatch path.  Face and Edge are the only sub-shape kinds
    // that reach `construct_frame_from_kernel`; Point is filtered to `None` so
    // the dispatcher's `?` early-returns without ever reaching kernel queries.
    // ─────────────────────────────────────────────────────────────────────────

    /// Pins the full `FrameSubShapeKind::from_selector_kind` conversion contract:
    /// - `Face`  → `Some(Face)` — kernel path handles @face via FaceNormal query
    /// - `Edge`  → `Some(Edge)` — kernel path handles @edge via EdgeTangent query
    /// - `Point` → `None`       — @point is resolved by Layer-1; `?` propagates
    ///   None without ever reaching kernel dispatch
    #[test]
    fn frame_sub_shape_kind_from_selector_kind_contract() {
        assert_eq!(
            super::FrameSubShapeKind::from_selector_kind(&reify_ir::SelectorKind::Face),
            Some(super::FrameSubShapeKind::Face),
            "SelectorKind::Face should convert to Some(FrameSubShapeKind::Face)"
        );
        assert_eq!(
            super::FrameSubShapeKind::from_selector_kind(&reify_ir::SelectorKind::Edge),
            Some(super::FrameSubShapeKind::Edge),
            "SelectorKind::Edge should convert to Some(FrameSubShapeKind::Edge)"
        );
        assert!(
            super::FrameSubShapeKind::from_selector_kind(&reify_ir::SelectorKind::Point).is_none(),
            "SelectorKind::Point should convert to None — point selectors are \
             handled by Layer-1 eval_expr and must not reach kernel dispatch"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // construct_frame_from_kernel with narrowed FrameSubShapeKind signature
    //
    // These tests exercise the two match arms inside construct_frame_from_kernel
    // directly, locking the Face↔FaceNormal and Edge↔EdgeTangent dispatch.
    // They are RED until step 4 changes the function signature from
    // `selector_kind: &SelectorKind` to `sub_shape_kind: FrameSubShapeKind`.
    // ─────────────────────────────────────────────────────────────────────────

    /// `construct_frame_from_kernel` with `FrameSubShapeKind::Face` must query
    /// `GeometryQuery::FaceNormal` for the basis and return a `Value::Frame`
    /// whose origin is the centroid and whose basis is the identity quaternion
    /// when the face normal is +Z (the CAD standard orientation for a top cap).
    ///
    /// Pins the Face↔FaceNormal dispatch: the Face arm must use FaceNormal, not
    /// EdgeTangent.  With centroid (0, 0, 0.01) and normal (0, 0, 1) (both +Z),
    /// `quaternion_from_z_to_axis(0, 0, 1)` produces the identity quaternion
    /// (w=1, x=0, y=0, z=0) — exact IEEE 754.
    #[test]
    fn construct_frame_from_kernel_face_returns_frame_from_centroid_and_face_normal() {
        use reify_test_support::mocks::MockGeometryKernel;

        let target = reify_ir::GeometryHandleId(10);
        let centroid_json = reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":0.01}"#.to_string());
        let normal_json = reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
        let mut kernel = MockGeometryKernel::new()
            .with_centroid_result(target, centroid_json)
            .with_face_normal_result(target, normal_json);
        let mut diagnostics = Vec::new();

        let result = super::construct_frame_from_kernel(
            target,
            super::FrameSubShapeKind::Face,
            &mut kernel,
            &mut diagnostics,
        );

        let Some(reify_ir::Value::Frame {
            ref origin,
            ref basis,
        }) = result
        else {
            panic!(
                "construct_frame_from_kernel(Face) should return Some(Value::Frame {{ .. }}); got {:?}",
                result
            );
        };
        assert_eq!(
            **origin,
            reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.01),
            ]),
            "Face: origin should be centroid (0m, 0m, 0.01m)"
        );
        assert_eq!(
            **basis,
            reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            "Face: basis should be identity (FaceNormal +Z → +Z = zero rotation)"
        );
        assert!(
            diagnostics.is_empty(),
            "Face: no diagnostics expected on clean kernel results; got {:?}",
            diagnostics
        );
    }

    /// `construct_frame_from_kernel` with `FrameSubShapeKind::Edge` must query
    /// `GeometryQuery::EdgeTangent` for the basis and return a `Value::Frame`
    /// whose origin is the centroid and whose basis is the identity quaternion
    /// when the edge tangent is +Z.
    ///
    /// Pins the Edge↔EdgeTangent dispatch: the Edge arm must use EdgeTangent,
    /// not FaceNormal.  With centroid (0, 0, 0.005) and tangent (0, 0, 1),
    /// `quaternion_from_z_to_axis(0, 0, 1)` produces identity — exact IEEE 754.
    #[test]
    fn construct_frame_from_kernel_edge_returns_frame_from_centroid_and_edge_tangent() {
        use reify_test_support::mocks::MockGeometryKernel;

        let target = reify_ir::GeometryHandleId(20);
        let centroid_json = reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":0.005}"#.to_string());
        let tangent_json = reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
        let mut kernel = MockGeometryKernel::new()
            .with_centroid_result(target, centroid_json)
            .with_edge_tangent_result(target, tangent_json);
        let mut diagnostics = Vec::new();

        let result = super::construct_frame_from_kernel(
            target,
            super::FrameSubShapeKind::Edge,
            &mut kernel,
            &mut diagnostics,
        );

        let Some(reify_ir::Value::Frame {
            ref origin,
            ref basis,
        }) = result
        else {
            panic!(
                "construct_frame_from_kernel(Edge) should return Some(Value::Frame {{ .. }}); got {:?}",
                result
            );
        };
        assert_eq!(
            **origin,
            reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.005),
            ]),
            "Edge: origin should be centroid (0m, 0m, 0.005m)"
        );
        assert_eq!(
            **basis,
            reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            "Edge: basis should be identity (EdgeTangent +Z → +Z = zero rotation)"
        );
        assert!(
            diagnostics.is_empty(),
            "Edge: no diagnostics expected on clean kernel results; got {:?}",
            diagnostics
        );
    }

    #[test]
    fn cap_kind_translation_maps_all_canonical_labels_and_returns_none_for_unknown() {
        use reify_ir::{CapKind, Role};
        let cases: &[(&str, Option<(Role, u32)>)] = &[
            ("top", Some((Role::Cap(CapKind::Top), 0))),
            ("bottom", Some((Role::Cap(CapKind::Bottom), 0))),
            ("start", Some((Role::Cap(CapKind::Start), 0))),
            ("end", Some((Role::Cap(CapKind::End), 0))),
            ("side", Some((Role::Side, 0))),
            ("nonexistent", None),
        ];
        for (label, expected) in cases {
            assert_eq!(
                cap_kind_translation(label),
                *expected,
                "label {:?} should map to {:?}",
                label,
                expected
            );
        }
    }

    // ── try_eval_topology_selector `geo_equiv` unit tests (task 3613, KGQ-δ) ──
    //
    // These tests pin the `geo_equiv(left, right, tol) -> Bool` dispatch
    // contract (PRD §9 KGQ-δ). Args[0]/args[1] are Geometry ValueRefs resolved
    // via named_steps; args[2] is a Length scalar ValueRef resolved via values
    // to SI metres. The dispatcher reuses `dispatch_point_on_shape`
    // (Bool unwrapper), threading `DEFAULT_GEO_EQUIV_SAMPLE_COUNT` to the FFI.
    //
    // Four contracts (mirror the four `try_eval_topology_selector_contains_*`):
    //   (a) happy path: kernel Bool(true) reply → Some(Value::Bool(true)), no diags
    //   (b) literal-arg fall-through: 3 literal args → None, zero kernel calls
    //   (c) non-Bool kernel reply → Some(Value::Undef) + exactly-one Warning
    //       naming "geo_equiv" and "non-Bool"
    //   (d) kernel-Err: no seeding → Some(Value::Undef) + exactly-one Warning
    //       naming "geo_equiv" and "kernel query failed"
    //
    // All four FAIL (RED) until step-6 wires the `geo_equiv` arm in
    // try_eval_topology_selector / TopologySelectorHelper.

    /// Build a `CompiledExpr` for `helper(member_a, member_b, member_c)` where
    /// all three args are ValueRefs. Mirrors `topology_selector_call_two_value_refs`
    /// but with a third arg — used by the geo_equiv 3-arg dispatch tests.
    #[allow(clippy::too_many_arguments)]
    fn topology_selector_call_three_value_refs(
        helper_name: &str,
        entity: &str,
        member_a: &str,
        type_a: reify_core::Type,
        member_b: &str,
        type_b: reify_core::Type,
        member_c: &str,
        type_c: reify_core::Type,
        result_type: reify_core::Type,
    ) -> reify_ir::CompiledExpr {
        let arg_a = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_a),
            type_a,
        );
        let arg_b = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_b),
            type_b,
        );
        let arg_c = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_c),
            type_c,
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        content_hash = content_hash.combine(arg_c.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_a, arg_b, arg_c],
            },
            result_type,
            content_hash,
        }
    }

    /// Build a `CompiledExpr` for `helper(<literal>, <literal>, <literal>)` —
    /// used for 3-arg literal fall-through defensive tests. Mirrors
    /// `topology_selector_call_literal_args` but with three args so the arity
    /// gate for arity-3 helpers (like `geo_equiv`) passes.
    fn topology_selector_call_three_literal_args(helper_name: &str) -> reify_ir::CompiledExpr {
        let arg_a =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Real(1.0), reify_core::Type::dimensionless_scalar());
        let arg_b =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Real(2.0), reify_core::Type::dimensionless_scalar());
        let arg_c =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Real(3.0), reify_core::Type::dimensionless_scalar());
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        content_hash = content_hash.combine(arg_c.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_a, arg_b, arg_c],
            },
            result_type: reify_core::Type::Bool,
            content_hash,
        }
    }

    #[test]
    fn try_eval_topology_selector_geo_equiv_kernel_reply_returns_bool() {
        use reify_test_support::mocks::MockGeometryKernel;
        let left_handle = reify_ir::GeometryHandleId(41);
        let right_handle = reify_ir::GeometryHandleId(42);
        let tol = 1e-6_f64;
        // Record the mock using `with_geo_equiv_result` — pins that the
        // dispatcher builds `GeometryQuery::GeoEquiv { left, right, tolerance }`.
        let mut kernel = MockGeometryKernel::new().with_geo_equiv_result(
            left_handle,
            right_handle,
            tol,
            reify_ir::Value::Bool(true),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // args[0] = left geometry → resolved via named_steps by member "left"
        named_steps.insert("left".to_string(), kh(left_handle));
        // args[1] = right geometry → resolved via named_steps by member "right"
        named_steps.insert("right".to_string(), kh(right_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[2] = tolerance → resolved via values by ValueCellId
        values.insert(
            reify_core::ValueCellId::new("GeoEquivSmoke", "tol"),
            reify_ir::Value::length(tol),
        );

        // geo_equiv(left, right, tol): args[0]=left (Geometry), args[1]=right (Geometry),
        //                              args[2]=tol (Scalar<Length>)
        let expr = topology_selector_call_three_value_refs(
            "geo_equiv",
            "GeoEquivSmoke",
            "left",
            reify_core::Type::Geometry,
            "right",
            reify_core::Type::Geometry,
            "tol",
            reify_core::Type::length(),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "geo_equiv(left, right, tol) with kernel reply Bool(true) must produce \
             Some(Value::Bool(true)); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path geo_equiv must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_geo_equiv_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        // `geo_equiv(<literal>, <literal>, <literal>)` — non-ValueRef args, so
        // resolve_geometry_handle_arg returns None on args[0], and the dispatcher
        // must return None without consulting the kernel.
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_three_literal_args("geo_equiv");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "geo_equiv(<literal>, <literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_geo_equiv_non_bool_kernel_reply_emits_warning_and_returns_undef()
    {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the `Ok(other)` warning arm of `dispatch_point_on_shape` (reused
        // for `geo_equiv`): a kernel reply that is not `Value::Bool(_)` must
        // produce `Some(Value::Undef)` with a Warning diagnostic naming
        // "geo_equiv" and "non-Bool".
        let left_handle = reify_ir::GeometryHandleId(41);
        let right_handle = reify_ir::GeometryHandleId(42);
        let tol = 1e-6_f64;
        let mut kernel = MockGeometryKernel::new().with_geo_equiv_result(
            left_handle,
            right_handle,
            tol,
            reify_ir::Value::Real(0.5), // Wrong type — triggers non-Bool warning arm
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("left".to_string(), kh(left_handle));
        named_steps.insert("right".to_string(), kh(right_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("GeoEquivSmoke", "tol"),
            reify_ir::Value::length(tol),
        );

        let expr = topology_selector_call_three_value_refs(
            "geo_equiv",
            "GeoEquivSmoke",
            "left",
            reify_core::Type::Geometry,
            "right",
            reify_core::Type::Geometry,
            "tol",
            reify_core::Type::length(),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "geo_equiv(...) with non-Bool kernel reply must yield Some(Value::Undef); \
             got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Bool reply must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("geo_equiv"),
            "diagnostic must mention the helper name 'geo_equiv', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("non-Bool"),
            "diagnostic must indicate the non-Bool reply, got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_geo_equiv_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        // No `with_geo_equiv_result` seeding — MockGeometryKernel.query() falls
        // through to the generic handle-only map which also has no entry for
        // this handle, so it returns `Err(QueryError::QueryFailed(...))`.
        // `dispatch_point_on_shape` must downgrade this to `Some(Value::Undef)`
        // and emit exactly one Warning diagnostic naming "geo_equiv" and
        // "kernel query failed". Pins the `Err(err)` arm of that helper.
        let left_handle = reify_ir::GeometryHandleId(41);
        let right_handle = reify_ir::GeometryHandleId(42);
        let tol = 1e-6_f64;
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("left".to_string(), kh(left_handle));
        named_steps.insert("right".to_string(), kh(right_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("GeoEquivSmoke", "tol"),
            reify_ir::Value::length(tol),
        );

        let expr = topology_selector_call_three_value_refs(
            "geo_equiv",
            "GeoEquivSmoke",
            "left",
            reify_core::Type::Geometry,
            "right",
            reify_core::Type::Geometry,
            "tol",
            reify_core::Type::length(),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "geo_equiv(...) with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("geo_equiv"),
            "diagnostic must mention the helper name 'geo_equiv', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("kernel query failed"),
            "diagnostic must indicate the kernel failure, got: {}",
            diag.message
        );
    }

    // ── try_eval_topology_selector — `normal` dispatch unit tests (task 3615, KGQ-ζ) ─────────
    //
    // Four contracts (mirrors the four `try_eval_topology_selector_contains_*` tests):
    //   (a) HAPPY        — kernel reply `Value::String("{\"x\":0,\"y\":0,\"z\":1}")` → `Some(Value::Vector([Real(0), Real(0), Real(1)]))`
    //   (b) FALL-THROUGH — non-ValueRef literal args → `None` with zero kernel calls
    //   (c) ERROR        — kernel `Err` → `Some(Value::Undef)` + exactly one Warning mentioning "normal" + "kernel query failed"
    //   (d) MALFORMED    — non-`Value::String` kernel reply → `Some(Value::Undef)` + exactly one Warning
    //
    // Arg order: Surface = args[0] (resolved via named_steps["surface"]),
    //            Point3  = args[1] (resolved via values[(entity, "pt")]).
    // This mirrors the `contains(solid, point)` precedent (KGQ-β), NOT closest_point.
    //
    // Depends on `MockGeometryKernel::with_face_normal_at_result` (pre-1, task 3615).

    #[test]
    fn try_eval_topology_selector_normal_kernel_reply_returns_vec3_real() {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_handle = reify_ir::GeometryHandleId(55);
        // Stage the mock: point (0m, 0m, 0.005m) ≈ (0, 0, 5mm) in SI.
        // The kernel wire format for FaceNormalAt is the same JSON-Point3 encoding
        // as FaceNormal / surface_normal_at: {"x":_,"y":_,"z":_}.
        let mut kernel = MockGeometryKernel::new().with_face_normal_at_result(
            face_handle,
            [0.0, 0.0, 0.005],
            reify_ir::Value::String("{\"x\":0,\"y\":0,\"z\":1}".to_string()),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // args[0] = surface → resolved via named_steps by member name "surface"
        named_steps.insert("surface".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[1] = point3 → resolved via values by ValueCellId
        values.insert(
            reify_core::ValueCellId::new("NormalSmoke", "pt"),
            point3_length_value(0.0, 0.0, 0.005),
        );

        // normal(surface_ref, point3_ref) — Surface=args[0], Point3=args[1]
        let expr = topology_selector_call_two_value_refs(
            "normal",
            "NormalSmoke",
            "surface",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ])),
            "normal(surface, point3) with kernel reply {{x:0,y:0,z:1}} must produce \
             Some(Value::Vector([Real(0),Real(0),Real(1)])); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path normal must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_normal_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        // `normal(<literal>, <literal>)` — non-ValueRef args: both
        // resolve_geometry_handle_arg (args[0]) and resolve_point3_length_arg (args[1])
        // return None, so the dispatcher must return None without consulting the kernel.
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("normal");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "normal(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    /// Guard the point-arg LENGTH-qualification contract end-to-end through the
    /// dispatcher.  Even when args[0] (surface) resolves successfully via
    /// `named_steps`, a non-LENGTH-qualified point arg[1] must cause
    /// `resolve_point3_length_arg` to return `None`, which propagates as a
    /// silent fall-through (`None`) — zero kernel calls, zero diagnostics.
    ///
    /// Complements `try_eval_topology_selector_normal_literal_args_falls_through_to_none`
    /// (which tests BOTH args failing at the arg-shape level) by exercising the
    /// case where the SURFACE resolves but the POINT fails its unit-qualification
    /// check.  Locks the split-arg fall-through path: the result still degrades
    /// to None, but under task ε the point failure now emits exactly one
    /// Severity::Warning (no longer silent).
    #[test]
    fn try_eval_topology_selector_normal_dimensionless_point_falls_through_to_none() {
        use reify_test_support::mocks::{CountingMockKernel, MockGeometryKernel};
        let face_handle = reify_ir::GeometryHandleId(55);

        // Wrap in a counting mock so we can assert zero kernel queries.
        let inner = MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // args[0] = surface → present in named_steps, so resolve_geometry_handle_arg
        // returns Some(face_handle).
        named_steps.insert("surface".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[1] = point → bare Value::Real components, NOT Value::Scalar with
        // DimensionVector::LENGTH.  resolve_point3_length_arg returns None for
        // this shape (the `_ => return None` arm on the component match) AND, under
        // task ε (evaluate-then-accept), pushes exactly one Severity::Warning
        // naming the builtin / arg / expected Point<Length> — the surface resolves,
        // so the point probe IS reached and the defined-but-wrong value is no
        // longer silent.
        values.insert(
            reify_core::ValueCellId::new("NormalSmoke", "pt"),
            reify_ir::Value::Point(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(5.0),
            ]),
        );

        // normal(surface_ref, pt_ref) — same arg shape as the happy-path test,
        // but with a dimensionless-Real point value.
        let expr = topology_selector_call_two_value_refs(
            "normal",
            "NormalSmoke",
            "surface",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "normal(surface, dimensionless_point) must return None (fall-through); \
             got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted when point arg is not LENGTH-qualified; \
             got {} query calls",
            kernel.total_query_count()
        );
        // FLIP (task ε): the defined-but-wrong point (bare-Real components) is no
        // longer a silent fall-through — the point probe pushes exactly one
        // Severity::Warning naming the `normal` builtin and the expected
        // Point<Length>. The result still degrades to None with no kernel call.
        assert_eq!(
            diagnostics.len(),
            1,
            "dimensionless-point fall-through must emit exactly 1 Warning (FLIP \
             from silent); got: {:?}",
            diagnostics
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);
        let msg = diagnostics[0].message.to_lowercase();
        assert!(
            msg.contains("normal"),
            "warning must name the normal builtin; got: {:?}",
            diagnostics[0].message
        );
        assert!(
            msg.contains("point<length>"),
            "warning must name expected Point<Length>; got: {:?}",
            diagnostics[0].message
        );
    }

    #[test]
    fn try_eval_topology_selector_normal_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        // No `with_face_normal_at_result` staging — MockGeometryKernel.query() falls
        // through to the generic handle-only map which also has no entry for this
        // handle, so it returns `Err(QueryError::QueryFailed(...))`.
        // `dispatch_normal_vector3` must downgrade this to `Some(Value::Undef)`
        // and emit exactly one Warning diagnostic naming "normal" + "kernel query failed".
        let face_handle = reify_ir::GeometryHandleId(55);
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("surface".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("NormalSmoke", "pt"),
            point3_length_value(0.0, 0.0, 0.005),
        );

        let expr = topology_selector_call_two_value_refs(
            "normal",
            "NormalSmoke",
            "surface",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "normal(...) with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("normal"),
            "diagnostic must mention the helper name 'normal', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("kernel query failed"),
            "diagnostic must indicate the kernel failure, got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_normal_malformed_kernel_reply_emits_warning_and_returns_undef() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Stage a non-`Value::String` reply (Value::Real) — parse_xyz_value rejects
        // non-String replies, so dispatch_normal_vector3 must produce
        // `Some(Value::Undef)` with a Warning diagnostic naming "normal".
        let face_handle = reify_ir::GeometryHandleId(55);
        let mut kernel = MockGeometryKernel::new().with_face_normal_at_result(
            face_handle,
            [0.0, 0.0, 0.005],
            // Wrong type: a Real, not the expected JSON String.
            reify_ir::Value::Real(42.0),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("surface".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("NormalSmoke", "pt"),
            point3_length_value(0.0, 0.0, 0.005),
        );

        let expr = topology_selector_call_two_value_refs(
            "normal",
            "NormalSmoke",
            "surface",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "normal(...) with malformed kernel reply must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "malformed reply must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("normal"),
            "diagnostic must mention the helper name 'normal', got: {}",
            diag.message
        );
    }

    // ── step-5 (task 4118 γ): ResolveSelector kernel-bearing eval tests ──────
    //
    // These pin `try_eval_resolve_selector`, the kernel-bearing dispatch for the
    // compiler-inserted `ResolveSelector` coercion node (and `IndexAccess` over a
    // selector). It reconstructs the inner `Value::Selector` INLINE from the
    // nested selector FunctionCall (sidestepping value-cell ordering), calls the
    // single `topology_selectors::resolve` executor, and wraps the resulting
    // canonical-order handle ids as `Value::List(Value::GeometryHandle)`
    // sub-handles via `make_sub_handle`. RED until step-6 adds the function.

    /// `ResolveSelector { faces(b) }` resolves the All-face leaf via the kernel
    /// and yields a `Value::List` of three `Value::GeometryHandle` sub-handles,
    /// matching a direct `resolve()` + `make_sub_handle`: canonical TopExp order,
    /// per-element hashing, parent realization_ref inherited.
    #[test]
    fn resolve_selector_faces_all_yields_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("BoxFaces", 0);
        let parent_hash: [u8; 32] = [0x55; 32];

        let mut kernel = MockGeometryKernel::new().with_extracted_faces(
            parent_handle,
            vec![
                GeometryHandleId(2),
                GeometryHandleId(3),
                GeometryHandleId(4),
            ],
        );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("BoxFaces", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );

        // ResolveSelector { faces(b) } — inner selector is a nested FunctionCall,
        // reconstructed inline (no value-cell ordering dependency).
        let inner = topology_selector_call_one_value_ref(
            "faces",
            "BoxFaces",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let expr = reify_ir::CompiledExpr::resolve_selector(inner);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "ResolveSelector{{faces(b)}} must yield Some(Value::List(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(list.len(), 3, "expected 3 resolved face sub-handles");

        let expected_ids = [
            GeometryHandleId(2),
            GeometryHandleId(3),
            GeometryHandleId(4),
        ];
        for (i, (elem, expected_id)) in list.iter().zip(&expected_ids).enumerate() {
            let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
                &parent_hash,
                crate::topology_selectors::SubKind::Face,
                i as u32,
            );
            match elem {
                reify_ir::Value::GeometryHandle {
                    realization_ref,
                    upstream_values_hash,
                    kernel_handle,
                } => {
                    assert_eq!(
                        realization_ref, &parent_rr,
                        "elem[{i}] realization_ref must inherit parent"
                    );
                    assert_eq!(kernel_handle, expected_id, "elem[{i}] kernel_handle");
                    assert_eq!(
                        upstream_values_hash, &expected_hash,
                        "elem[{i}] hash must be compose_sub_handle_hash(parent, Face, {i})"
                    );
                }
                other => panic!("elem[{i}] must be Value::GeometryHandle, got {:?}", other),
            }
        }
        assert!(
            diagnostics.is_empty(),
            "successful resolve must emit zero diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `IndexAccess { object: ResolveSelector { faces(b) }, index: 0 }` recomputes
    /// to the indexed sub-handle (the curvature_smoke `faces(s)[0]` shape): resolve
    /// the selector to its list then index — element 0 is the canonical first face.
    #[test]
    fn resolve_selector_index_access_returns_indexed_handle() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("BoxFaces", 0);
        let parent_hash: [u8; 32] = [0x55; 32];

        let mut kernel = MockGeometryKernel::new().with_extracted_faces(
            parent_handle,
            vec![
                GeometryHandleId(2),
                GeometryHandleId(3),
                GeometryHandleId(4),
            ],
        );
        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("BoxFaces", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );

        let inner = topology_selector_call_one_value_ref(
            "faces",
            "BoxFaces",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let object = reify_ir::CompiledExpr::resolve_selector(inner);
        let index = reify_ir::CompiledExpr::literal(reify_ir::Value::Int(0), Type::Int);
        let expr = reify_ir::CompiledExpr::index_access(object, index, Type::Geometry);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Face,
            0,
        );
        match result {
            Some(reify_ir::Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            }) => {
                assert_eq!(realization_ref, parent_rr, "indexed handle realization_ref");
                assert_eq!(
                    kernel_handle,
                    GeometryHandleId(2),
                    "faces(b)[0] → canonical first face GHId(2)"
                );
                assert_eq!(
                    upstream_values_hash, expected_hash,
                    "indexed handle hash == compose_sub_handle_hash(parent, Face, 0)"
                );
            }
            other => panic!(
                "faces(b)[0] must yield Some(Value::GeometryHandle(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "successful index must emit zero diagnostics; got {:?}",
            diagnostics
        );
    }

    // ── step-5 (task 4119 δ): composition-algebra eval + resolve tests ─────────
    //
    // These tests pin `try_eval_topology_selector` for the three combinator names
    // (union/intersect/difference) and `topology_selectors::resolve` for their
    // K3 set semantics. RED until step-6 adds the composition arms to
    // try_eval_topology_selector; the resolve-semantics assertions are only
    // reachable once the eval arm returns Some(Value::Selector(..)).
    //
    // BT2: union = canonical-order set-union of children (no duplicates).
    // BT3: intersect of disjoint children = []; difference = a minus b.

    /// Build a two-arg composition FunctionCall (`union(arg_a, arg_b)` etc.)
    /// from two pre-compiled selector exprs.
    fn topology_selector_composition_call(
        combinator: &str,
        arg_a: reify_ir::CompiledExpr,
        arg_b: reify_ir::CompiledExpr,
    ) -> reify_ir::CompiledExpr {
        // Result type mirrors arg_a (same kind for valid same-kind composition).
        let result_type = arg_a.result_type.clone();
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(combinator))
            .combine(arg_a.content_hash)
            .combine(arg_b.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: combinator.to_string(),
                    qualified_name: combinator.to_string(),
                },
                args: vec![arg_a, arg_b],
            },
            result_type,
            content_hash,
        }
    }

    /// `union(faces(b), faces(c))` evaluates to `Value::Selector(Union([sv_b, sv_c]))`
    /// of kind Face, and resolving via `topology_selectors::resolve` yields the
    /// canonical-order set-union of all face handles. BT2.
    ///
    /// RED until step-6 adds the `union` arm to `try_eval_topology_selector`.
    #[test]
    fn union_eval_produces_union_selector_and_resolve_yields_set_union() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let handle_c = GeometryHandleId(2);
        let rr = RealizationNodeId::new("UnionTest", 0);
        let hash_b: [u8; 32] = [0x11; 32];
        let hash_c: [u8; 32] = [0x22; 32];

        // faces(b) = [GHId(10), GHId(11)]; faces(c) = [GHId(12), GHId(13)].
        // Union = [GHId(10), GHId(11), GHId(12), GHId(13)] (first-seen order, no dups).
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(handle_b, vec![GeometryHandleId(10), GeometryHandleId(11)])
            .with_extracted_faces(handle_c, vec![GeometryHandleId(12), GeometryHandleId(13)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(handle_b));
        named_steps.insert("c".to_string(), kh(handle_c));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("UnionTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: handle_b,
            },
        );
        values.insert(
            ValueCellId::new("UnionTest", "c"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_c,
                kernel_handle: handle_c,
            },
        );

        let faces_b = topology_selector_call_one_value_ref(
            "faces",
            "UnionTest",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_c = topology_selector_call_one_value_ref(
            "faces",
            "UnionTest",
            "c",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let union_expr = topology_selector_composition_call("union", faces_b, faces_c);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &union_expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // RED until step-6 adds the union arm.
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "union(faces(b), faces(c)) must yield Some(Value::Selector(..)), \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "union of face selectors → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Union(children) => {
                assert_eq!(children.len(), 2, "union of 2 operands → 2 children");
            }
            other => panic!("expected Union node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean composition must emit no diagnostics; got: {:?}",
            diagnostics
        );

        // Resolve set semantics: union = set-union of children (BT2).
        let resolved = crate::topology_selectors::resolve(&sv, &mut kernel, &mut diagnostics)
            .expect("union resolve must not error");
        assert_eq!(
            resolved,
            vec![
                GeometryHandleId(10),
                GeometryHandleId(11),
                GeometryHandleId(12),
                GeometryHandleId(13),
            ],
            "union resolves to canonical-order set-union of all child handles"
        );
    }

    /// `intersect(faces(b), faces(c))` where b and c are disjoint evaluates to
    /// `Value::Selector(Intersect([sv_b, sv_c]))` of kind Face, and resolving
    /// yields [] (empty intersection of disjoint sets). BT3.
    ///
    /// RED until step-6 adds the `intersect` arm to `try_eval_topology_selector`.
    #[test]
    fn intersect_eval_produces_intersect_selector_and_disjoint_resolves_empty() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let handle_c = GeometryHandleId(2);
        let rr = RealizationNodeId::new("IntersectTest", 0);
        let hash_b: [u8; 32] = [0x33; 32];
        let hash_c: [u8; 32] = [0x44; 32];

        // faces(b) = [GHId(10), GHId(11)]; faces(c) = [GHId(12), GHId(13)] (disjoint).
        // Intersect of disjoint sets = [].
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(handle_b, vec![GeometryHandleId(10), GeometryHandleId(11)])
            .with_extracted_faces(handle_c, vec![GeometryHandleId(12), GeometryHandleId(13)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(handle_b));
        named_steps.insert("c".to_string(), kh(handle_c));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("IntersectTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: handle_b,
            },
        );
        values.insert(
            ValueCellId::new("IntersectTest", "c"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_c,
                kernel_handle: handle_c,
            },
        );

        let faces_b = topology_selector_call_one_value_ref(
            "faces",
            "IntersectTest",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_c = topology_selector_call_one_value_ref(
            "faces",
            "IntersectTest",
            "c",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let intersect_expr = topology_selector_composition_call("intersect", faces_b, faces_c);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &intersect_expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // RED until step-6 adds the intersect arm.
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "intersect(faces(b), faces(c)) must yield Some(Value::Selector(..)), \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "intersect of face selectors → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Intersect(children) => {
                assert_eq!(children.len(), 2, "intersect of 2 operands → 2 children");
            }
            other => panic!("expected Intersect node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean intersect composition must emit no diagnostics; got: {:?}",
            diagnostics
        );

        // Resolve set semantics: intersect of disjoint sets = [] (BT3).
        let resolved = crate::topology_selectors::resolve(&sv, &mut kernel, &mut diagnostics)
            .expect("intersect resolve must not error");
        assert!(
            resolved.is_empty(),
            "intersect of disjoint face sets must resolve to []; got {:?}",
            resolved
        );
    }

    /// `difference(faces(b), faces(c))` evaluates to `Value::Selector(Difference(sv_b, sv_c))`
    /// of kind Face, and resolving yields faces in b but not in c. BT3.
    ///
    /// RED until step-6 adds the `difference` arm to `try_eval_topology_selector`.
    #[test]
    fn difference_eval_produces_difference_selector_and_resolve_yields_set_difference() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let handle_c = GeometryHandleId(2);
        let rr = RealizationNodeId::new("DiffTest", 0);
        let hash_b: [u8; 32] = [0x55; 32];
        let hash_c: [u8; 32] = [0x66; 32];

        // faces(b) = [GHId(10), GHId(11), GHId(12)]; faces(c) = [GHId(11)].
        // Difference = b \ c = [GHId(10), GHId(12)] (GHId(11) excluded).
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(
                handle_b,
                vec![GeometryHandleId(10), GeometryHandleId(11), GeometryHandleId(12)],
            )
            .with_extracted_faces(handle_c, vec![GeometryHandleId(11)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(handle_b));
        named_steps.insert("c".to_string(), kh(handle_c));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("DiffTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: handle_b,
            },
        );
        values.insert(
            ValueCellId::new("DiffTest", "c"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_c,
                kernel_handle: handle_c,
            },
        );

        let faces_b = topology_selector_call_one_value_ref(
            "faces",
            "DiffTest",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_c = topology_selector_call_one_value_ref(
            "faces",
            "DiffTest",
            "c",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let diff_expr = topology_selector_composition_call("difference", faces_b, faces_c);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &diff_expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // RED until step-6 adds the difference arm.
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "difference(faces(b), faces(c)) must yield Some(Value::Selector(..)), \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "difference of face selectors → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Difference(a, _b) => {
                assert_eq!(
                    a.kind,
                    reify_core::ty::SelectorKind::Face,
                    "difference left operand → Face kind"
                );
            }
            other => panic!("expected Difference node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean difference composition must emit no diagnostics; got: {:?}",
            diagnostics
        );

        // Resolve set semantics: difference = a \ b (BT3).
        let resolved = crate::topology_selectors::resolve(&sv, &mut kernel, &mut diagnostics)
            .expect("difference resolve must not error");
        assert_eq!(
            resolved,
            vec![GeometryHandleId(10), GeometryHandleId(12)],
            "difference(faces(b), faces(c)) resolves to b \\ c = [GHId(10), GHId(12)]"
        );
    }

    // ── eval-side composition coverage (task 4119 δ amendment) ─────────────
    //
    // Tests added in the amendment pass to cover paths not exercised by the
    // compile-time tests in selector_composition_tests.rs:
    //   1. Variadic 3-arg union at eval level.
    //   2. SelectorError::KindMismatch → Warning + Value::Undef backstop in
    //      eval_variadic_composition (defensive path; compile-time
    //      E_SELECTOR_KIND_MISMATCH should fire first in normal use).

    /// `union(faces(b), faces(c), faces(d))` — 3 operands, all Face — evaluates
    /// to `Value::Selector(Union([sv_b, sv_c, sv_d]))` of kind Face.
    /// Covers the variadic path in `eval_variadic_composition`.
    #[test]
    fn union_three_operands_eval_produces_union_with_three_children() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let handle_c = GeometryHandleId(2);
        let handle_d = GeometryHandleId(3);
        let rr = RealizationNodeId::new("Union3Test", 0);
        let hash_b: [u8; 32] = [0x11; 32];
        let hash_c: [u8; 32] = [0x22; 32];
        let hash_d: [u8; 32] = [0x33; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(handle_b, vec![GeometryHandleId(10)])
            .with_extracted_faces(handle_c, vec![GeometryHandleId(11)])
            .with_extracted_faces(handle_d, vec![GeometryHandleId(12)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(handle_b));
        named_steps.insert("c".to_string(), kh(handle_c));
        named_steps.insert("d".to_string(), kh(handle_d));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Union3Test", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: handle_b,
            },
        );
        values.insert(
            ValueCellId::new("Union3Test", "c"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_c,
                kernel_handle: handle_c,
            },
        );
        values.insert(
            ValueCellId::new("Union3Test", "d"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_d,
                kernel_handle: handle_d,
            },
        );

        let faces_b = topology_selector_call_one_value_ref(
            "faces",
            "Union3Test",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_c = topology_selector_call_one_value_ref(
            "faces",
            "Union3Test",
            "c",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_d = topology_selector_call_one_value_ref(
            "faces",
            "Union3Test",
            "d",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );

        // Build union(faces_b, faces_c, faces_d) — three-arg FunctionCall.
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("union"))
            .combine(faces_b.content_hash)
            .combine(faces_c.content_hash)
            .combine(faces_d.content_hash);
        let union3_expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "union".to_string(),
                    qualified_name: "union".to_string(),
                },
                args: vec![faces_b, faces_c, faces_d],
            },
            result_type: Type::Selector(reify_core::ty::SelectorKind::Face),
            content_hash,
        };

        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &union3_expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "union(faces(b), faces(c), faces(d)) must yield Some(Value::Selector(..)), \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Face, "3-arg union → Face kind");
        match &sv.node {
            reify_ir::value::SelectorNode::Union(children) => {
                assert_eq!(children.len(), 3, "3-arg union → 3 children in Union node");
            }
            other => panic!("expected Union node with 3 children, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean 3-arg union must emit no diagnostics; got: {:?}",
            diagnostics
        );

        // Resolve: union of 3 disjoint single-face sets = all three handles.
        let resolved = crate::topology_selectors::resolve(&sv, &mut kernel, &mut diagnostics)
            .expect("3-arg union resolve must not error");
        assert_eq!(
            resolved,
            vec![GeometryHandleId(10), GeometryHandleId(11), GeometryHandleId(12)],
            "3-arg union resolves to set-union of all three child face sets"
        );
    }

    /// Defensive backstop: when `eval_variadic_composition` receives children of
    /// mismatched `SelectorKind` (bypassing the compile-time
    /// `E_SELECTOR_KIND_MISMATCH`), `SelectorValue::union` returns
    /// `SelectorError::KindMismatch` and the result is `Some(Value::Undef)` with
    /// exactly one Warning diagnostic.
    ///
    /// This path is not reachable from valid .ri source (the compiler catches it
    /// first) but is reachable from hand-crafted IR, so we pin the defensive
    /// behaviour here.
    #[test]
    fn eval_variadic_composition_kind_mismatch_yields_undef_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let handle_c = GeometryHandleId(2);
        let rr = RealizationNodeId::new("KindMismatchTest", 0);
        let hash_b: [u8; 32] = [0xAA; 32];
        let hash_c: [u8; 32] = [0xBB; 32];

        // Kernel needs no mock data: `faces`/`edges` construction is kernel-free
        // (LeafQuery::All build via build_leaf_selector, no extract_* calls).
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(handle_b));
        named_steps.insert("c".to_string(), kh(handle_c));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("KindMismatchTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: handle_b,
            },
        );
        values.insert(
            ValueCellId::new("KindMismatchTest", "c"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_c,
                kernel_handle: handle_c,
            },
        );

        // Build union(faces(b), edges(c)) at IR level — mixed kinds, bypasses
        // the compiler's kind-mismatch check.  result_type is deliberately Face
        // (as if the compiler did anti-cascade), so the FunctionCall arm in
        // try_eval_topology_selector routes to the Union handler.
        let faces_b = topology_selector_call_one_value_ref(
            "faces",
            "KindMismatchTest",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let edges_c = topology_selector_call_one_value_ref(
            "edges",
            "KindMismatchTest",
            "c",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Edge),
        );
        let union_mixed = topology_selector_composition_call("union", faces_b, edges_c);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &union_mixed,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Defensive backstop: kind-mismatch at eval level → Some(Undef) + Warning.
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "kind-mismatch union at eval level must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kind-mismatch must emit exactly 1 Warning diagnostic; got {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "backstop diagnostic must be Warning severity"
        );
    }

    // ── step-9 (task 4119 δ): Named-leaf constructor eval tests ─────────────
    //
    // These tests pin `try_eval_topology_selector` for the three named-leaf
    // constructors (face/edge/solid_body) and the BT8 resolve-to-empty path
    // for an unresolvable name.  RED until step-10 adds the face/edge/solid_body
    // arms to try_eval_topology_selector.
    //
    // BT8: resolving a face(b,"nope") Named leaf (no matching tag) returns []
    // and pushes EXACTLY ONE DiagnosticCode::TopologyTagStale — exercising the
    // already-landed resolve_leaf Named interim now reachable from the .ri surface.

    /// Build a two-arg `face`/`edge`/`solid_body` FunctionCall: arg[0] is a
    /// ValueRef to the parent geometry cell, arg[1] is a string Literal.
    fn named_selector_call(
        helper_name: &str,
        entity: &str,
        member: &str,
        result_kind: reify_core::ty::SelectorKind,
        name_str: &str,
    ) -> reify_ir::CompiledExpr {
        let arg_geom = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member),
            reify_core::Type::Geometry,
        );
        let arg_name = reify_ir::CompiledExpr::literal(
            reify_ir::Value::String(name_str.to_string()),
            reify_core::Type::String,
        );
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name))
            .combine(arg_geom.content_hash)
            .combine(arg_name.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_geom, arg_name],
            },
            result_type: reify_core::Type::Selector(result_kind),
            content_hash,
        }
    }

    /// `face(b, "top")` evaluates to `Value::Selector(Face)` with a
    /// `SelectorNode::Leaf { query: LeafQuery::Named("top") }`. Zero kernel
    /// queries at construction time (K2/BT7). RED until step-10.
    #[test]
    fn face_named_ctor_yields_named_leaf_selector_of_face_kind() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ValueCellId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("NamedFaceCtorTest", 0);
        let hash_b: [u8; 32] = [0xAA; 32];

        let named_steps = HashMap::new(); // no kernel queries at construction
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("NamedFaceCtorTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: handle_b,
            },
        );

        let expr = named_selector_call(
            "face",
            "NamedFaceCtorTest",
            "b",
            reify_core::ty::SelectorKind::Face,
            "top",
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "face(b, \"top\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "face() → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                ..
            } => {
                assert_eq!(n, "top", "face(b, \"top\") → Named(\"top\") leaf");
            }
            other => panic!("expected Leaf{{ Named }}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `edge(b, "rim")` evaluates to `Value::Selector(Edge)` with
    /// `LeafQuery::Named("rim")`. RED until step-10.
    #[test]
    fn edge_named_ctor_yields_named_leaf_selector_of_edge_kind() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ValueCellId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("NamedEdgeCtorTest", 0);
        let hash_b: [u8; 32] = [0xBB; 32];

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("NamedEdgeCtorTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: handle_b,
            },
        );

        let expr = named_selector_call(
            "edge",
            "NamedEdgeCtorTest",
            "b",
            reify_core::ty::SelectorKind::Edge,
            "rim",
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "edge(b, \"rim\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Edge,
            "edge() → Edge kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                ..
            } => {
                assert_eq!(n, "rim", "edge(b, \"rim\") → Named(\"rim\") leaf");
            }
            other => panic!("expected Leaf{{ Named }}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `solid_body(b, "core")` evaluates to `Value::Selector(Body)` with
    /// `LeafQuery::Named("core")`. RED until step-10.
    #[test]
    fn solid_body_named_ctor_yields_named_leaf_selector_of_body_kind() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ValueCellId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("NamedBodyCtorTest", 0);
        let hash_b: [u8; 32] = [0xCC; 32];

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("NamedBodyCtorTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: handle_b,
            },
        );

        let expr = named_selector_call(
            "solid_body",
            "NamedBodyCtorTest",
            "b",
            reify_core::ty::SelectorKind::Body,
            "core",
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "solid_body(b, \"core\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Body,
            "solid_body() → Body kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                ..
            } => {
                assert_eq!(n, "core", "solid_body(b, \"core\") → Named(\"core\") leaf");
            }
            other => panic!("expected Leaf{{ Named }}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// BT8: resolving `face(b, "nope")` (unknown name) returns the empty list
    /// and pushes exactly ONE `DiagnosticCode::TopologyTagStale` warning — the
    /// already-landed resolve_leaf Named interim, now reachable from .ri surface.
    /// RED until step-10 wires the face arm; the resolve assertion is only
    /// reachable once construction succeeds.
    #[test]
    fn face_named_ctor_resolve_unknown_name_yields_empty_and_topology_tag_stale() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ValueCellId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("NamedBT8Test", 0);
        let hash_b: [u8; 32] = [0xDD; 32];

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("NamedBT8Test", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: handle_b,
            },
        );

        let expr = named_selector_call(
            "face",
            "NamedBT8Test",
            "b",
            reify_core::ty::SelectorKind::Face,
            "nope",
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();

        // Step 1: construction produces Value::Selector with no diagnostics.
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "face(b, \"nope\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );

        // Step 2: resolve the Named leaf — resolves to [] + exactly one TopologyTagStale.
        let resolved = crate::topology_selectors::resolve(&sv, &mut kernel, &mut diagnostics)
            .expect("resolve must not return QueryError for Named leaf");
        assert_eq!(
            resolved,
            vec![],
            "Named(\"nope\"): resolve must return empty list (D8 interim)"
        );
        let stale_count = diagnostics
            .iter()
            .filter(|d| d.code == Some(reify_core::DiagnosticCode::TopologyTagStale))
            .count();
        assert_eq!(
            stale_count,
            1,
            "resolve of Named leaf with no matching tag must emit exactly ONE \
             W_TOPOLOGY_TAG_STALE; got {stale_count}: {:?}",
            diagnostics
        );
    }

    // ── try_eval_topology_selector directional-selector dispatch unit tests ───
    // (task 3618, KGQ-ι: faces_by_normal + edges_parallel_to)
    //
    // These tests pin that the rewired arms resolve arg[0] via `values` (not
    // `named_steps`), run the filter helper, recover canonical TopExp indices,
    // and emit Value::List([Value::GeometryHandle]) — not Value::Int.
    //
    // The canonical-index correctness requirement: faces_by_normal(box,+z,1°)[0]
    // must hash identically to faces(box)[k] for the same physical face.
    // Each test places the retained face/edge at a NON-ZERO canonical index to
    // exercise the index recovery (if the arm hardcoded index 0 it would pass a
    // trivial index-0-only case but fail these).

    /// `faces_by_normal` dispatch emits `Value::List([Value::GeometryHandle])`
    /// with the retained face's canonical TopExp index (not filtered position).
    ///
    /// Setup: canonical list [GHId(2), GHId(3), GHId(4)]; only GHId(3) (index 1)
    /// has a +z normal within 1°; GHId(2) (+x) and GHId(4) (−z, sign-sensitive)
    /// are rejected. The result must carry canonical index 1, not position 0.
    #[test]
    fn faces_by_normal_dispatch_returns_geometry_handle_sub_handles() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Directional", 0);
        let parent_hash: [u8; 32] = [0xAA; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(
                parent_handle,
                vec![
                    GeometryHandleId(2),
                    GeometryHandleId(3),
                    GeometryHandleId(4),
                ],
            )
            .with_face_normal_result(
                GeometryHandleId(2),
                reify_ir::Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".to_string()),
            )
            .with_face_normal_result(
                GeometryHandleId(3),
                reify_ir::Value::String("{\"x\":0.0,\"y\":0.0,\"z\":1.0}".to_string()),
            )
            .with_face_normal_result(
                GeometryHandleId(4),
                reify_ir::Value::String("{\"x\":0.0,\"y\":0.0,\"z\":-1.0}".to_string()),
            );

        // named_steps carries a different handle id to prove the arm reads from
        // values, not named_steps.
        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(GeometryHandleId(99)));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Directional", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );
        values.insert(
            ValueCellId::new("Directional", "dir"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        let tol_rad = std::f64::consts::PI / 180.0; // 1°
        values.insert(
            ValueCellId::new("Directional", "tol"),
            reify_ir::Value::Scalar {
                si_value: tol_rad,
                dimension: DimensionVector::ANGLE,
            },
        );

        let expr = topology_selector_call_three_value_refs(
            "faces_by_normal",
            "Directional",
            "b",
            Type::Geometry,
            "dir",
            Type::vec3(Type::dimensionless_scalar()),
            "tol",
            Type::angle(),
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Task 4118 (γ): construction is kernel-FREE — `faces_by_normal(b, dir, tol)`
        // builds a typed `Value::Selector(Face)` with a `ByNormal` leaf carrying the
        // direction + angular tolerance, NOT an eagerly-filtered `Value::List`. The
        // staged extract_faces / face-normal kernel data is intentionally unused
        // (zero kernel queries during construction, K2/BT7); the predicate filter and
        // canonical sub-handle indexing now run on the ResolveSelector / resolve()
        // path (see the try_eval_resolve_selector tests).
        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "faces_by_normal(..) must yield Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "faces_by_normal → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, parent_handle,
                    "leaf target must be the parent solid handle"
                );
                assert_eq!(
                    *query,
                    reify_ir::value::LeafQuery::ByNormal { dir: [0.0, 0.0, 1.0], tol_rad },
                    "faces_by_normal → ByNormal leaf (dir +z, tol 1°)"
                );
            }
            other => panic!(
                "faces_by_normal must be a Leaf selector node, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "kernel-free construction must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `faces_by_normal` falls through to `None` when the parent arg is not a
    /// hydrated `Value::GeometryHandle` in `values` (PRD §4 invariant #2).
    #[test]
    fn faces_by_normal_dispatch_falls_through_when_parent_not_hydrated() {
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let mut kernel = MockGeometryKernel::new();
        let named_steps = HashMap::new();

        // values has NO Value::GeometryHandle for the arg cell.
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Directional", "dir"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        values.insert(
            ValueCellId::new("Directional", "tol"),
            reify_ir::Value::Scalar {
                si_value: std::f64::consts::PI / 180.0,
                dimension: DimensionVector::ANGLE,
            },
        );

        let expr = topology_selector_call_three_value_refs(
            "faces_by_normal",
            "Directional",
            "b",
            Type::Geometry,
            "dir",
            Type::vec3(Type::dimensionless_scalar()),
            "tol",
            Type::angle(),
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "must fall through to None when parent is not a hydrated Value::GeometryHandle; \
             got {:?}",
            result
        );
    }

    /// `edges_parallel_to` dispatch emits `Value::List([Value::GeometryHandle])`
    /// with the retained edge's canonical TopExp index (not filtered position).
    ///
    /// Setup: canonical list [GHId(2), GHId(3), GHId(4)]; only GHId(4) (index 2)
    /// is (anti-)parallel to +z within 1° (tangent = −z; sign-tolerant predicate).
    /// GHId(2) and GHId(3) have +x tangents and are rejected. The result must
    /// carry canonical index 2, not position 0.
    #[test]
    fn edges_parallel_to_dispatch_returns_geometry_handle_sub_handles() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Directional", 0);
        let parent_hash: [u8; 32] = [0xBB; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(
                parent_handle,
                vec![
                    GeometryHandleId(2),
                    GeometryHandleId(3),
                    GeometryHandleId(4),
                ],
            )
            .with_edge_tangent_result(
                GeometryHandleId(2),
                reify_ir::Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".to_string()),
            )
            .with_edge_tangent_result(
                GeometryHandleId(3),
                reify_ir::Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".to_string()),
            )
            // GHId(4) has tangent −z: sign-tolerant |dot(−z, +z)| = 1 ≥ cos(1°), retained.
            .with_edge_tangent_result(
                GeometryHandleId(4),
                reify_ir::Value::String("{\"x\":0.0,\"y\":0.0,\"z\":-1.0}".to_string()),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(GeometryHandleId(99)));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Directional", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );
        values.insert(
            ValueCellId::new("Directional", "axis"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        let tol_rad = std::f64::consts::PI / 180.0; // 1°
        values.insert(
            ValueCellId::new("Directional", "tol"),
            reify_ir::Value::Scalar {
                si_value: tol_rad,
                dimension: DimensionVector::ANGLE,
            },
        );

        let expr = topology_selector_call_three_value_refs(
            "edges_parallel_to",
            "Directional",
            "b",
            Type::Geometry,
            "axis",
            Type::vec3(Type::dimensionless_scalar()),
            "tol",
            Type::angle(),
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Task 4118 (γ): construction is kernel-FREE — `edges_parallel_to(b, axis, tol)`
        // builds a typed `Value::Selector(Edge)` with a `ByParallel` leaf carrying the
        // axis + angular tolerance, NOT an eagerly-filtered `Value::List`. The staged
        // extract_edges / edge-tangent kernel data is intentionally unused (zero kernel
        // queries during construction, K2/BT7); the predicate filter and canonical
        // sub-handle indexing now run on the ResolveSelector / resolve() path.
        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "edges_parallel_to(..) must yield Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Edge,
            "edges_parallel_to → Edge kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, parent_handle,
                    "leaf target must be the parent solid handle"
                );
                assert_eq!(
                    *query,
                    reify_ir::value::LeafQuery::ByParallel { axis: [0.0, 0.0, 1.0], tol_rad },
                    "edges_parallel_to → ByParallel leaf (axis +z, tol 1°)"
                );
            }
            other => panic!(
                "edges_parallel_to must be a Leaf selector node, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "kernel-free construction must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    // --- dispatch_filtered_subhandles defensive-branch tests ---

    /// Branch (a): filter_result is Err → dispatch emits a Warning and returns Value::Undef.
    #[test]
    fn dispatch_filtered_subhandles_filter_error_yields_undef_and_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_rr = RealizationNodeId::new("Def", 0);
        let parent_hash: [u8; 32] = [0x01; 32];
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();

        let result = super::dispatch_filtered_subhandles(
            &mut kernel,
            GeometryHandleId(1),
            crate::topology_selectors::SubKind::Face,
            &parent_rr,
            &parent_hash,
            Err(reify_ir::QueryError::QueryFailed(
                "mock filter failure".to_string(),
            )),
            "faces_by_normal",
            &mut diagnostics,
        );

        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "filter Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(diagnostics.len(), 1, "must emit exactly one warning");
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "diagnostic must be Warning severity, got {:?}",
            diagnostics[0]
        );
    }

    /// Branch (b): filter_result is Ok but canonical re-extract fails → Warning + Value::Undef.
    #[test]
    fn dispatch_filtered_subhandles_canonical_reextract_error_yields_undef_and_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_rr = RealizationNodeId::new("Def", 0);
        let parent_hash: [u8; 32] = [0x02; 32];
        // Kernel has no extract_faces entry for the parent → extract_faces returns QueryError.
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();

        // Filter returned Ok with a retained id, but the re-extract below will fail.
        let result = super::dispatch_filtered_subhandles(
            &mut kernel,
            GeometryHandleId(1),
            crate::topology_selectors::SubKind::Face,
            &parent_rr,
            &parent_hash,
            Ok(vec![GeometryHandleId(2)]),
            "faces_by_normal",
            &mut diagnostics,
        );

        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "canonical re-extract Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(diagnostics.len(), 1, "must emit exactly one warning");
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "diagnostic must be Warning severity, got {:?}",
            diagnostics[0]
        );
    }

    /// Branch (c): a retained id is absent from the canonical list → that element is silently
    /// skipped (list is shorter than retained), and a Warning is emitted for the missing id.
    #[test]
    fn dispatch_filtered_subhandles_absent_retained_id_is_skipped_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_rr = RealizationNodeId::new("Def", 0);
        let parent_hash: [u8; 32] = [0x03; 32];
        let parent_handle = GeometryHandleId(1);
        // Canonical list: [GHId(2), GHId(3)] — GHId(99) is NOT present.
        let mut kernel = MockGeometryKernel::new().with_extracted_faces(
            parent_handle,
            vec![GeometryHandleId(2), GeometryHandleId(3)],
        );
        let mut diagnostics = Vec::new();

        // Retained contains one present id (GHId(2)) and one absent id (GHId(99)).
        let result = super::dispatch_filtered_subhandles(
            &mut kernel,
            parent_handle,
            crate::topology_selectors::SubKind::Face,
            &parent_rr,
            &parent_hash,
            Ok(vec![GeometryHandleId(2), GeometryHandleId(99)]),
            "faces_by_normal",
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "must yield Some(Value::List(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        // GHId(2) at canonical index 0 is included; GHId(99) is absent → skipped.
        assert_eq!(
            list.len(),
            1,
            "absent retained id must be skipped; expected 1 element, got {}; diags: {:?}",
            list.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "must emit one warning for the absent id"
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "diagnostic must be Warning severity, got {:?}",
            diagnostics[0]
        );
        assert!(
            diagnostics[0]
                .message
                .contains("absent from canonical list"),
            "warning must mention 'absent from canonical list'; got: {}",
            diagnostics[0].message
        );
    }

    // ── GHR-ζ (task 3608): whole-handle geometry-query defensive-downgrade ──
    //
    // The volume/area/centroid/bounding_box dispatch helpers each promise the
    // PRD §4 defensive-downgrade contract: on a kernel error OR an unexpected/
    // malformed reply, return `Some(Value::Undef)` and push EXACTLY ONE Warning
    // (never `None`, never a panic). These unit tests drive each arm with a
    // `MockGeometryKernel` that (a) has no registered result → `query` returns
    // `Err`, and (b) returns a wrong-typed reply, asserting the Undef + single-
    // Warning contract. They live here, not in the OCCT integration test,
    // because the dispatch helpers are crate-private and a real kernel cannot be
    // coerced into erroring for a valid primitive.

    /// `volume` arm (`dispatch_scalar_query`, VOLUME): kernel `Err` and an
    /// unexpected (non-Real/Scalar) reply each yield `Some(Value::Undef)` + one
    /// Warning.
    #[test]
    fn dispatch_volume_query_error_and_unexpected_reply_yield_undef_and_one_warning() {
        use reify_test_support::mocks::MockGeometryKernel;

        // (a) kernel Err — no registered result.
        let kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::Volume(GeometryHandleId(1)),
            reify_core::DimensionVector::VOLUME,
            "volume",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "volume (kernel Err) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "volume (kernel Err) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);

        // (b) unexpected reply type — Bool is neither Real nor Scalar.
        let kernel = MockGeometryKernel::new()
            .with_volume_result(GeometryHandleId(1), reify_ir::Value::Bool(true));
        let mut diagnostics = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::Volume(GeometryHandleId(1)),
            reify_core::DimensionVector::VOLUME,
            "volume",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "volume (unexpected reply) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "volume (unexpected reply) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);
    }

    /// `area` arm (`dispatch_scalar_query`, AREA): kernel `Err` and an
    /// unexpected reply each yield `Some(Value::Undef)` + one Warning.
    #[test]
    fn dispatch_area_query_error_and_unexpected_reply_yield_undef_and_one_warning() {
        use reify_test_support::mocks::MockGeometryKernel;

        // (a) kernel Err.
        let kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::SurfaceArea(GeometryHandleId(1)),
            reify_core::DimensionVector::AREA,
            "area",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "area (kernel Err) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "area (kernel Err) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);

        // (b) unexpected reply type.
        let kernel = MockGeometryKernel::new()
            .with_surface_area_result(GeometryHandleId(1), reify_ir::Value::Bool(true));
        let mut diagnostics = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::SurfaceArea(GeometryHandleId(1)),
            reify_core::DimensionVector::AREA,
            "area",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "area (unexpected reply) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "area (unexpected reply) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);
    }

    /// `centroid` arm (`dispatch_point3_length_reply`): kernel `Err` and a
    /// malformed (non-String) reply each yield `Some(Value::Undef)` + one
    /// Warning.
    #[test]
    fn dispatch_centroid_query_error_and_malformed_reply_yield_undef_and_one_warning() {
        use reify_test_support::mocks::MockGeometryKernel;

        // (a) kernel Err.
        let kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::dispatch_point3_length_reply(
            &kernel,
            &reify_ir::GeometryQuery::Centroid(GeometryHandleId(1)),
            "centroid",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "centroid (kernel Err) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "centroid (kernel Err) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);

        // (b) malformed reply — a non-String value fails parse_xyz_value.
        let kernel = MockGeometryKernel::new()
            .with_centroid_result(GeometryHandleId(1), reify_ir::Value::Bool(true));
        let mut diagnostics = Vec::new();
        let result = super::dispatch_point3_length_reply(
            &kernel,
            &reify_ir::GeometryQuery::Centroid(GeometryHandleId(1)),
            "centroid",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "centroid (malformed reply) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "centroid (malformed reply) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);
    }

    /// `bounding_box` arm (`dispatch_bounding_box`): kernel `Err` and a
    /// malformed (non-String) reply each yield `Some(Value::Undef)` + one
    /// Warning.
    #[test]
    fn dispatch_bounding_box_query_error_and_malformed_reply_yield_undef_and_one_warning() {
        use reify_test_support::mocks::MockGeometryKernel;

        // (a) kernel Err.
        let kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::dispatch_bounding_box(&kernel, GeometryHandleId(1), &mut diagnostics);
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "bounding_box (kernel Err) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "bounding_box (kernel Err) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);

        // (b) malformed reply — a non-String value fails parse_bbox_axis_extents.
        let kernel = MockGeometryKernel::new()
            .with_bbox_result(GeometryHandleId(1), reify_ir::Value::Bool(true));
        let mut diagnostics = Vec::new();
        let result = super::dispatch_bounding_box(&kernel, GeometryHandleId(1), &mut diagnostics);
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "bounding_box (malformed reply) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "bounding_box (malformed reply) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);
    }

    // ── step-1 (task 3619): adjacent_faces dispatch unit tests ──────────────
    //
    // These tests verify that the arm emits Value::List(Value::GeometryHandle)
    // via dispatch_filtered_subhandles.

    /// `adjacent_faces` dispatch returns `Value::List` of one
    /// `Value::GeometryHandle` when the mock kernel returns the adjacent face
    /// at index 0. The element must carry the parent's `realization_ref` and
    /// an `upstream_values_hash` equal to
    /// `compose_sub_handle_hash(parent_hash, SubKind::Face, 0)`.
    #[test]
    fn adjacent_faces_dispatch_returns_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // args[0]: parent solid; args[1]: face sub-handle (same handle in mock)
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent_handle, vec![GeometryHandleId(1)])
            .with_adjacent_faces_result(
                parent_handle,
                0,
                reify_ir::Value::List(vec![reify_ir::Value::Int(0)]),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        // Seed parent solid (args[0])
        values.insert(
            ValueCellId::new("Solid", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );
        // Seed face arg (args[1]) — same kernel handle for the mock
        values.insert(
            ValueCellId::new("Solid", "face"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );

        let expr = topology_selector_call_two_value_refs(
            "adjacent_faces",
            "Solid",
            "b",
            Type::Geometry,
            "face",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "expected Some(Value::List(..)), got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "expected 1 adjacent face sub-handle; diags: {:?}",
            diagnostics
        );

        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Face,
            0,
        );
        match &list[0] {
            reify_ir::Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            } => {
                assert_eq!(
                    realization_ref.entity, parent_rr.entity,
                    "realization_ref.entity must match parent"
                );
                assert_eq!(
                    realization_ref.index, parent_rr.index,
                    "realization_ref.index must match parent"
                );
                assert_eq!(
                    *kernel_handle,
                    GeometryHandleId(1),
                    "kernel_handle must be GHId(1)"
                );
                assert_eq!(
                    *upstream_values_hash, expected_hash,
                    "upstream_values_hash must be compose_sub_handle_hash(parent_hash, Face, 0)"
                );
            }
            other => panic!("elem[0] is not Value::GeometryHandle: {:?}", other),
        }
    }

    /// When args[1]'s cell is absent from `values`, the `adjacent_faces` arm
    /// must fall through to `None` (PRD invariant #2: never partial-construct).
    #[test]
    fn adjacent_faces_dispatch_falls_through_when_face_arg_not_hydrated() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent_handle, vec![GeometryHandleId(1)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        // Only the parent is seeded; the face cell is absent
        values.insert(
            ValueCellId::new("Solid", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );
        // "face" cell intentionally absent from values

        let expr = topology_selector_call_two_value_refs(
            "adjacent_faces",
            "Solid",
            "b",
            Type::Geometry,
            "face",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "must fall through to None when face arg is not a hydrated Value::GeometryHandle, \
             got {:?}",
            result
        );
    }

    // ── step-3 (task 3619): shared_edges dispatch unit tests ─────────────────
    //
    // These tests verify that the arm emits Value::List(Value::GeometryHandle)
    // via dispatch_filtered_subhandles.

    /// `shared_edges` dispatch returns `Value::List` of one
    /// `Value::GeometryHandle` (kernel_handle GHId(4)) when the mock kernel
    /// stages two faces (GHId(2), GHId(3)) sharing one edge (GHId(4)).
    /// The element must carry the parent solid's `realization_ref` and an
    /// `upstream_values_hash` equal to
    /// `compose_sub_handle_hash(parent_hash, SubKind::Edge, 0)`.
    #[test]
    fn shared_edges_dispatch_returns_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_a_handle = GeometryHandleId(2);
        let face_b_handle = GeometryHandleId(3);
        let edge_handle = GeometryHandleId(4);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_owner_body_result(face_a_handle, parent_handle)
            .with_owner_body_result(face_b_handle, parent_handle)
            .with_extracted_faces(parent_handle, vec![face_a_handle, face_b_handle])
            .with_extracted_edges(parent_handle, vec![edge_handle])
            .with_shared_edges_result(
                parent_handle,
                0,
                1,
                reify_ir::Value::List(vec![reify_ir::Value::Int(0)]),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("fa".to_string(), kh(face_a_handle));
        named_steps.insert("fb".to_string(), kh(face_b_handle));

        let mut values = reify_ir::ValueMap::new();
        // Parent solid — found by resolve_owner_solid_handle scanning values
        values.insert(
            ValueCellId::new("Solid", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );
        // Face args — resolved by resolve_parent_geometry_handle_arg for the arm
        values.insert(
            ValueCellId::new("Solid", "fa"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: face_a_handle,
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fb"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: face_b_handle,
            },
        );

        let expr = topology_selector_call_two_value_refs(
            "shared_edges",
            "Solid",
            "fa",
            Type::Geometry,
            "fb",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "expected Some(Value::List(..)), got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "expected 1 shared edge sub-handle; diags: {:?}",
            diagnostics
        );

        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Edge,
            0,
        );
        match &list[0] {
            reify_ir::Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            } => {
                assert_eq!(
                    realization_ref.entity, parent_rr.entity,
                    "realization_ref.entity must match parent solid"
                );
                assert_eq!(
                    realization_ref.index, parent_rr.index,
                    "realization_ref.index must match parent solid"
                );
                assert_eq!(
                    *kernel_handle, edge_handle,
                    "kernel_handle must be the edge GHId(4)"
                );
                assert_eq!(
                    *upstream_values_hash, expected_hash,
                    "upstream_values_hash must be compose_sub_handle_hash(parent_hash, Edge, 0)"
                );
            }
            other => panic!("elem[0] is not Value::GeometryHandle: {:?}", other),
        }
    }

    /// When the parent solid is not hydrated in `values`, the `shared_edges`
    /// arm must fall through to `None` (PRD invariant #2: never partial-construct).
    #[test]
    fn shared_edges_dispatch_falls_through_when_parent_not_hydrated() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_a_handle = GeometryHandleId(2);
        let face_b_handle = GeometryHandleId(3);
        let edge_handle = GeometryHandleId(4);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_owner_body_result(face_a_handle, parent_handle)
            .with_owner_body_result(face_b_handle, parent_handle)
            .with_extracted_faces(parent_handle, vec![face_a_handle, face_b_handle])
            .with_extracted_edges(parent_handle, vec![edge_handle])
            .with_shared_edges_result(
                parent_handle,
                0,
                1,
                reify_ir::Value::List(vec![reify_ir::Value::Int(0)]),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("fa".to_string(), kh(face_a_handle));
        named_steps.insert("fb".to_string(), kh(face_b_handle));

        let mut values = reify_ir::ValueMap::new();
        // Face args present — arm resolves them, then hits dispatch_shared_edges
        values.insert(
            ValueCellId::new("Solid", "fa"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: face_a_handle,
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fb"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: face_b_handle,
            },
        );
        // Parent solid (kernel_handle=GHId(1)) intentionally absent from values
        // so resolve_owner_solid_handle returns None → arm falls through.

        let expr = topology_selector_call_two_value_refs(
            "shared_edges",
            "Solid",
            "fa",
            Type::Geometry,
            "fb",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "must fall through to None when parent solid is not hydrated in values, got {:?}",
            result
        );
    }

    // ── error-path coverage for dispatch_filtered_subhandles (suggestion 3) ──

    /// When the `AdjacentFaces` kernel query is not staged, `adjacent_to_face`
    /// returns `Err`; `dispatch_filtered_subhandles` receives `filter_result = Err`
    /// and must return `Some(Value::Undef)` with a Warning diagnostic.
    #[test]
    fn adjacent_faces_dispatch_emits_warning_and_undef_on_kernel_query_failure() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // Stage extract_faces so adjacent_to_face can find the face index (0),
        // but omit the AdjacentFaces query result → kernel.query(...) returns Err
        // → adjacent_to_face propagates Err → filter_result = Err in
        // dispatch_filtered_subhandles → Warning + Value::Undef.
        let mut kernel =
            MockGeometryKernel::new().with_extracted_faces(parent_handle, vec![face_handle]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Solid", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );
        values.insert(
            ValueCellId::new("Solid", "face"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: face_handle,
            },
        );

        let expr = topology_selector_call_two_value_refs(
            "adjacent_faces",
            "Solid",
            "b",
            Type::Geometry,
            "face",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "kernel query failure must yield Value::Undef; got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            !diagnostics.is_empty(),
            "a Warning diagnostic must be emitted on kernel query failure"
        );
    }

    /// When the `SharedEdges` kernel query is not staged, `dispatch_shared_edges`
    /// hits `Err` at step 4 (SharedEdges query) and must return
    /// `Some(Value::Undef)` with a Warning diagnostic.
    #[test]
    fn shared_edges_dispatch_emits_warning_and_undef_on_shared_edges_query_failure() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_a_handle = GeometryHandleId(2);
        let face_b_handle = GeometryHandleId(3);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // Stage OwnerBody + extract_faces so the arm passes the cross-solid guard
        // and face-index recovery, but omit the SharedEdges query result →
        // kernel.query(SharedEdges { ... }) returns Err → Warning + Value::Undef.
        let mut kernel = MockGeometryKernel::new()
            .with_owner_body_result(face_a_handle, parent_handle)
            .with_owner_body_result(face_b_handle, parent_handle)
            .with_extracted_faces(parent_handle, vec![face_a_handle, face_b_handle]);

        let mut named_steps = HashMap::new();
        named_steps.insert("fa".to_string(), kh(face_a_handle));
        named_steps.insert("fb".to_string(), kh(face_b_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Solid", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fa"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: face_a_handle,
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fb"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: face_b_handle,
            },
        );

        let expr = topology_selector_call_two_value_refs(
            "shared_edges",
            "Solid",
            "fa",
            Type::Geometry,
            "fb",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "SharedEdges query failure must yield Value::Undef; got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            !diagnostics.is_empty(),
            "a Warning diagnostic must be emitted on SharedEdges query failure"
        );
    }

    // ── try_eval_topology_selector curvature dispatch unit tests ─────────────
    // (task 3621, KGQ-μ: curvature(Curve) + curvature(Surface))
    //
    // Step-5 RED: these tests compile but FAIL until step-6 adds the
    // "curvature" → TopologySelectorHelper::Curvature arm to the dispatcher.
    // Modelled on the `normal` tests above (lines ~10006-10319).

    // DimensionVector for curvature = 1/Length = Length^-1.
    // Constructed directly (from_exps is private); index-0 is the LENGTH basis.
    const CURVATURE_DIM: reify_core::dimension::DimensionVector = {
        let mut d = [reify_core::dimension::Rational::ZERO; 10];
        d[0] = reify_core::dimension::Rational::new(-1, 1);
        reify_core::dimension::DimensionVector(d)
    };

    /// `curvature(surface, point)` with a fake kernel staging a 2×2 nested-List
    /// [[kappa_max, 0], [0, kappa_min]] must yield `Some(Value::Matrix(...))` where
    /// every cell is a `Value::Scalar` with dimension = 1/Length (Curvature), and
    /// the matrix diagonal mean (trace/2) equals the expected curvature.
    #[test]
    fn try_eval_topology_selector_curvature_surface_returns_matrix() {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_handle = reify_ir::GeometryHandleId(55);
        let kappa = 200.0_f64; // 1/(0.005 m) — sphere radius 5 mm
        // Kernel wire: [[kappa, 0.0], [0.0, kappa]] (diagonal: kappa_max == kappa_min for sphere).
        let row0 = reify_ir::Value::List(vec![
            reify_ir::Value::Real(kappa),
            reify_ir::Value::Real(0.0),
        ]);
        let row1 = reify_ir::Value::List(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(kappa),
        ]);
        // u = px = 0.005 m, v = py = 0.0 m  (eval maps DSL point3 coords → (u,v))
        let mut kernel = MockGeometryKernel::new().with_surface_curvature_at_result(
            face_handle,
            [0.005, 0.0],
            reify_ir::Value::List(vec![row0, row1]),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("CurvatureSmoke", "pt"),
            point3_length_value(0.005, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "curvature",
            "CurvatureSmoke",
            "face",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::dimensionless_scalar(), // placeholder result type — unused on dispatch path
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Expect a 2×2 Value::Matrix of curvature-dimensioned scalars.
        let expected_cell = reify_ir::Value::Scalar {
            si_value: kappa,
            dimension: CURVATURE_DIM,
        };
        let expected_zero = reify_ir::Value::Scalar {
            si_value: 0.0,
            dimension: CURVATURE_DIM,
        };
        let expected = Some(reify_ir::Value::Matrix(vec![
            vec![expected_cell.clone(), expected_zero.clone()],
            vec![expected_zero, expected_cell],
        ]));
        assert_eq!(
            result, expected,
            "curvature(surface, point) must return Some(Value::Matrix([[κ,0],[0,κ]])); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path surface curvature must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `curvature(curve, point)` with a fake kernel staging `Value::Real(κ)` must
    /// yield `Some(Value::Scalar{ si_value: κ, dimension: 1/Length })`.
    #[test]
    fn try_eval_topology_selector_curvature_curve_returns_scalar() {
        use reify_test_support::mocks::MockGeometryKernel;
        let edge_handle = reify_ir::GeometryHandleId(77);
        let kappa = 100.0_f64; // 1/(0.01 m) — circle radius 10 mm
        // Kernel wire: Value::Real(κ).  Staged for CurveCurvatureAt at point (0.01, 0.0, 0.0).
        let mut kernel = MockGeometryKernel::new().with_curve_curvature_at_result(
            edge_handle,
            [0.01, 0.0, 0.0],
            reify_ir::Value::Real(kappa),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("edge".to_string(), kh(edge_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("CurvatureSmoke", "pt"),
            point3_length_value(0.01, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "curvature",
            "CurvatureSmoke",
            "edge",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::dimensionless_scalar(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let expected = Some(reify_ir::Value::Scalar {
            si_value: kappa,
            dimension: CURVATURE_DIM,
        });
        assert_eq!(
            result, expected,
            "curvature(curve, point) must return Some(Value::Scalar{{κ, 1/m}}); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path curve curvature must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `curvature(surface, point)` with no staged kernel result must yield
    /// `Some(Value::Undef)` + exactly one Warning diagnostic naming "curvature".
    #[test]
    fn try_eval_topology_selector_curvature_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_handle = reify_ir::GeometryHandleId(55);
        // No staging — both SurfaceCurvatureAt and CurveCurvatureAt fall through to
        // the generic no-match error in the mock kernel, yielding QueryFailed.
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("CurvatureSmoke", "pt"),
            point3_length_value(0.005, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "curvature",
            "CurvatureSmoke",
            "face",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::dimensionless_scalar(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "curvature(...) with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning; got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("curvature"),
            "diagnostic must mention the helper name 'curvature', got: {}",
            diag.message
        );
    }

    /// `curvature(<literal>, <literal>)` must fall through to `None` without
    /// consulting the kernel — both arg-shape guards reject non-ValueRef args.
    #[test]
    fn try_eval_topology_selector_curvature_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("curvature");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "curvature(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args; got {} queries",
            kernel.total_query_count()
        );
        assert!(
            diagnostics.is_empty(),
            "literal-arg fall-through must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    // ── try_eval_topology_selector length dispatch unit tests ──────────────
    // (task 3622, KGQ-ν: length(Curve) + perimeter(Surface))
    //
    // Step-1 RED: tests compile but FAIL until step-2 wires the
    // "length" → TopologySelectorHelper::Length arm to the dispatcher.

    /// Build a `CompiledExpr` for `helper(<literal_real>)` with a single
    /// literal arg. Used for 1-arg literal fall-through tests.
    fn topology_selector_call_one_literal_arg(helper_name: &str) -> reify_ir::CompiledExpr {
        let arg =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Real(1.0), reify_core::Type::dimensionless_scalar());
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name))
            .combine(arg.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg],
            },
            result_type: reify_core::Type::dimensionless_scalar(),
            content_hash,
        }
    }

    /// `length(edge_sub_handle)` with a staged `Value::Real(0.02)` EdgeLength
    /// result must yield `Some(Value::length(0.02))` and zero diagnostics.
    ///
    /// PRIMARY RED assertion — pre-impl `length` hits the `_ => return None` arm.
    #[test]
    fn try_eval_topology_selector_length_edge_subhandle_returns_scalar_length() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let edge_kh = reify_ir::GeometryHandleId(10);
        let parent_rr = RealizationNodeId::new("LengthTest", 0);
        let parent_hash: [u8; 32] = [0x42; 32];
        let mut kernel =
            MockGeometryKernel::new().with_edge_length_result(edge_kh, reify_ir::Value::Real(0.02));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("LengthTest", "e"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: edge_kh,
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "length",
            "LengthTest",
            "e",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::length(0.02)),
            "length(edge) must return Some(Value::length(0.02 m)); got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path length must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `length(<literal>)` must fall through to `None` without consulting the
    /// kernel — the `resolve_parent_geometry_handle_arg` guard rejects non-ValueRef.
    #[test]
    fn try_eval_topology_selector_length_literal_arg_falls_through_to_none() {
        use reify_test_support::mocks::MockGeometryKernel;

        let mut kernel = MockGeometryKernel::new();
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_one_literal_arg("length");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "length(<literal>) must return None; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "literal-arg fall-through must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `length(edge_sub_handle)` with no staged EdgeLength result (mock returns
    /// error) must yield `Some(Value::Undef)` + exactly one Warning mentioning
    /// "length".
    #[test]
    fn try_eval_topology_selector_length_kernel_err_returns_undef_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let edge_kh = reify_ir::GeometryHandleId(11);
        let parent_rr = RealizationNodeId::new("LengthTest", 0);
        let parent_hash: [u8; 32] = [0x43; 32];
        // No EdgeLength staged → mock returns QueryFailed.
        let mut kernel = MockGeometryKernel::new();

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("LengthTest", "e"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: edge_kh,
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "length",
            "LengthTest",
            "e",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "length with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning; got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("length"),
            "diagnostic must mention 'length'; got: {}",
            diag.message
        );
    }

    // ── try_eval_topology_selector perimeter dispatch unit tests ───────────
    // (task 3622, KGQ-ν)
    //
    // Step-3 RED: tests compile but FAIL until step-4 wires the
    // "perimeter" → TopologySelectorHelper::Perimeter arm to the dispatcher.

    /// `perimeter(face_sub_handle)` where the mock kernel returns 4 edges with
    /// exactly-representable lengths 1.0+2.0+3.0+4.0=10.0 must yield
    /// `Some(Value::length(10.0))` and zero diagnostics.
    ///
    /// PRIMARY RED assertion — pre-impl `perimeter` hits the `_ => return None` arm.
    #[test]
    fn try_eval_topology_selector_perimeter_face_subhandle_sums_edge_lengths() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let face_kh = reify_ir::GeometryHandleId(20);
        let e1 = reify_ir::GeometryHandleId(21);
        let e2 = reify_ir::GeometryHandleId(22);
        let e3 = reify_ir::GeometryHandleId(23);
        let e4 = reify_ir::GeometryHandleId(24);
        let parent_rr = RealizationNodeId::new("PerimTest", 0);
        let parent_hash: [u8; 32] = [0x50; 32];
        // Use exactly-representable lengths so summation is bit-exact.
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(face_kh, vec![e1, e2, e3, e4])
            .with_edge_length_result(e1, reify_ir::Value::Real(1.0))
            .with_edge_length_result(e2, reify_ir::Value::Real(2.0))
            .with_edge_length_result(e3, reify_ir::Value::Real(3.0))
            .with_edge_length_result(e4, reify_ir::Value::Real(4.0));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("PerimTest", "f"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: face_kh,
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "perimeter",
            "PerimTest",
            "f",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::length(10.0)),
            "perimeter(face) must return Some(Value::length(10.0 m)); got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path perimeter must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `perimeter(<literal>)` must fall through to `None` — the
    /// `resolve_parent_geometry_handle_arg` guard rejects non-ValueRef args.
    #[test]
    fn try_eval_topology_selector_perimeter_literal_arg_falls_through_to_none() {
        use reify_test_support::mocks::MockGeometryKernel;

        let mut kernel = MockGeometryKernel::new();
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_one_literal_arg("perimeter");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "perimeter(<literal>) must return None; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "literal-arg fall-through must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `perimeter(face_sub_handle)` when `extract_edges` returns an error must
    /// yield `Some(Value::Undef)` + exactly one Warning mentioning "perimeter".
    #[test]
    fn try_eval_topology_selector_perimeter_extract_edges_error_returns_undef_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let face_kh = reify_ir::GeometryHandleId(25);
        let parent_rr = RealizationNodeId::new("PerimTest", 0);
        let parent_hash: [u8; 32] = [0x51; 32];
        let mut kernel = MockGeometryKernel::new()
            .with_extract_edges_error(face_kh, reify_ir::QueryError::InvalidHandle(face_kh));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("PerimTest", "f"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: face_kh,
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "perimeter",
            "PerimTest",
            "f",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "perimeter with extract_edges error must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "extract_edges error must emit exactly one Warning; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("perimeter"),
            "diagnostic must mention 'perimeter'; got: {}",
            diag.message
        );
    }

    /// `perimeter(face_sub_handle)` when edges are staged but one `EdgeLength`
    /// query returns a non-Real value must yield `Some(Value::Undef)` + one Warning.
    #[test]
    fn try_eval_topology_selector_perimeter_non_real_edge_length_returns_undef_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let face_kh = reify_ir::GeometryHandleId(26);
        let e1 = reify_ir::GeometryHandleId(27);
        let e2 = reify_ir::GeometryHandleId(28);
        let parent_rr = RealizationNodeId::new("PerimTest", 0);
        let parent_hash: [u8; 32] = [0x52; 32];
        // e1 returns Real(1.0) ok, e2 returns a non-Real value → should degrade.
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(face_kh, vec![e1, e2])
            .with_edge_length_result(e1, reify_ir::Value::Real(1.0))
            .with_edge_length_result(e2, reify_ir::Value::Bool(true)); // unexpected type

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("PerimTest", "f"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: face_kh,
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "perimeter",
            "PerimTest",
            "f",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "perimeter with non-Real EdgeLength must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Real EdgeLength must emit exactly one Warning; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diagnostics[0].severity
        );
    }

    // ── feature→datum projection over a SELECTOR receiver (review amend) ────
    //
    // The compiler types a selection's feature→datum projection (`s.axis` where
    // `s : FaceSelector` → Axis, design §2.2) but the selector→sub-handle
    // resolution is not yet wired on the eval side. These pin that the eval emits
    // an honest diagnostic instead of a silent `Value::Undef`, and that the β
    // datum→datum path is NOT captured by the new branch.

    /// A feature→datum projection whose receiver STATICALLY types as a topology
    /// selector (`Type::Selector(_)`) but does not resolve to a realized
    /// `Value::GeometryHandle` must emit exactly one select-a-subfeature
    /// `FeatureDatumAmbiguous` error and evaluate to `Value::Undef` — NOT leave the
    /// cell a silent `Value::Undef` with no diagnostic (the
    /// clean-compile-then-silent-runtime failure mode).
    #[test]
    fn feature_datum_projection_over_selector_receiver_emits_diagnostic_not_silent_undef() {
        use reify_core::ty::SelectorKind;
        use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        // `s.axis` where `s : FaceSelector`. The receiver cell is unhydrated (and a
        // Selector value would not resolve to a GeometryHandle anyway), so
        // resolve_selector_target → None and the Selector static type drives the
        // not-yet-supported diagnostic.
        let object = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("S", "s"),
            Type::Selector(SelectorKind::Face),
        );
        let expr =
            reify_ir::CompiledExpr::method_call(object, "axis".to_string(), vec![], Type::Axis);

        let values = reify_ir::ValueMap::new();
        let mut kernel = MockGeometryKernel::new();
        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "a selector-receiver feature→datum projection must yield Some(Value::Undef); \
             got {result:?}"
        );
        let errs: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Error
                    && d.code == Some(DiagnosticCode::FeatureDatumAmbiguous)
            })
            .collect();
        assert_eq!(
            errs.len(),
            1,
            "a selector-receiver projection must emit exactly one FeatureDatumAmbiguous \
             (not-yet-supported / select-a-subfeature) error rather than a silent Undef; \
             got {diagnostics:?}"
        );
    }

    /// A β *datum* receiver (`axis.dir` — receiver statically types as `Axis`, not a
    /// feature/selector) must make the kernel-backed feature-datum path DECLINE
    /// (`None`, no diagnostic) so the pure `eval_datum_projection` owns it. Guards
    /// that the selector not-yet-supported branch does not capture β's datum→datum
    /// projections.
    #[test]
    fn feature_datum_projection_over_datum_receiver_declines_to_pure_path() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let object = reify_ir::CompiledExpr::value_ref(ValueCellId::new("S", "a"), Type::Axis);
        let expr =
            reify_ir::CompiledExpr::method_call(object, "dir".to_string(), vec![], Type::Direction);

        let values = reify_ir::ValueMap::new();
        let mut kernel = MockGeometryKernel::new();
        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "a β datum receiver (`axis.dir`) must decline (None) to the pure projection \
             path; got {result:?}"
        );
        assert!(
            diagnostics.is_empty(),
            "declining to the pure path must emit no diagnostic; got {diagnostics:?}"
        );
    }

    // ── feature→datum projection over a HYDRATED Selector receiver (task 4594) ─
    //
    // These tests verify the new `eval_selector_feature_datum` arm added to
    // `try_eval_feature_datum_projection`: when the receiver cell holds a hydrated
    // `Value::Selector`, the arm resolves it to sub-handles, unions the per-handle
    // `FeatureDatumBundle`s, re-dedups across handles at the confusion-floor
    // tolerance, and calls `feature_datum_projection` — the same select-one-or-
    // diagnose refinement the GeometryHandle arm uses.

    /// Assert that a `Value` is a `Value::Axis` lying on the world Z line
    /// (origin x ≈ y ≈ 0, direction parallel to ±Z, |z| ≈ 1).
    /// Mirrors `assert_value_axis_is_z_line` from feature_datum_tests.rs.
    fn assert_value_axis_is_z_line(v: &reify_ir::Value) {
        match v {
            reify_ir::Value::Axis { origin, direction } => {
                let o = match origin.as_ref() {
                    reify_ir::Value::Point(c) if c.len() == 3 => [
                        c[0].as_f64().expect("axis origin x is numeric"),
                        c[1].as_f64().expect("axis origin y is numeric"),
                        c[2].as_f64().expect("axis origin z is numeric"),
                    ],
                    other => panic!("axis origin must be a 3-component Point; got {other:?}"),
                };
                let d = match direction.as_ref() {
                    reify_ir::Value::Direction { x, y, z } => [*x, *y, *z],
                    other => panic!("axis direction must be a Direction; got {other:?}"),
                };
                assert!(
                    o[0].abs() < 1e-9 && o[1].abs() < 1e-9,
                    "axis origin must lie on the world Z line; got {o:?}"
                );
                assert!(
                    d[0].abs() < 1e-9 && d[1].abs() < 1e-9 && (d[2].abs() - 1.0).abs() < 1e-9,
                    "axis direction must be parallel to ±Z; got {d:?}"
                );
            }
            other => panic!("expected Value::Axis; got {other:?}"),
        }
    }

    /// Build a `Value::Axis` along the world Z line at the given z-origin offset,
    /// direction +Z.  Mirrors `axis_value` from feature_datum_tests.rs.
    fn z_axis_value_at(z_origin: f64) -> reify_ir::Value {
        reify_ir::Value::Axis {
            origin: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(z_origin),
            ])),
            direction: Box::new(reify_ir::Value::Direction {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            }),
        }
    }

    /// `s.axis` where `s : FaceSelector` is backed by a hydrated `Value::Selector`
    /// that `topology_selectors::resolve` expands to a single cylindrical face,
    /// whose `FaceAnalyticDatum` is an Axis on the world-Z line, must evaluate to
    /// `Some(Value::Axis{..})` on Z with zero `FeatureDatumAmbiguous` errors.
    ///
    /// RED today: the existing stub returns `Some(Value::Undef)` + one
    /// `FeatureDatumAmbiguous` error for ANY selector receiver — hydrated or not.
    #[test]
    fn feature_datum_projection_over_selector_receiver_resolves_single_cyl_face_to_axis() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ty::SelectorKind;
        use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent = reify_ir::GeometryHandleId(1);
        let cyl_face = reify_ir::GeometryHandleId(10);

        let sv = reify_ir::value::SelectorValue::leaf(
            SelectorKind::Face,
            reify_ir::value::GeometryHandleRef {
                realization_ref: RealizationNodeId::new("S", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: parent,
            },
            reify_ir::value::LeafQuery::All,
        )
        .expect("SelectorValue::leaf for Face/All must succeed");

        // selector resolve:        extract_faces(parent)   → [cyl_face]
        // feature_datum_bundle:    extract_faces(cyl_face) → [cyl_face]
        // FaceAnalyticDatum(cyl_face)                      → Axis at z=0, dir +Z
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent, vec![cyl_face])
            .with_extracted_faces(cyl_face, vec![cyl_face])
            .with_face_analytic_datum_result(cyl_face, z_axis_value_at(0.0));

        let mut values = reify_ir::ValueMap::new();
        values.insert(ValueCellId::new("S", "s"), reify_ir::Value::Selector(sv));

        let object = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("S", "s"),
            Type::Selector(SelectorKind::Face),
        );
        let expr =
            reify_ir::CompiledExpr::method_call(object, "axis".to_string(), vec![], Type::Axis);

        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        let value = result.expect(
            "hydrated-selector s.axis (single cyl face) must yield Some(..), not None",
        );
        assert_value_axis_is_z_line(&value);

        let ambiguous_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Error
                    && d.code == Some(DiagnosticCode::FeatureDatumAmbiguous)
            })
            .collect();
        assert!(
            ambiguous_errors.is_empty(),
            "a hydrated-selector s.axis over a single coaxial face must emit zero \
             FeatureDatumAmbiguous errors; got {diagnostics:?}"
        );
    }

    /// `s.axis` where the selector resolves to TWO coaxial cylindrical faces whose
    /// analytic axes share the world-Z line at different origins must deduplicate
    /// across sub-handles and return `Some(Value::Axis{..})` on Z with zero
    /// `FeatureDatumAmbiguous` errors.
    ///
    /// RED after step-2 (before step-4 dedup): without cross-handle
    /// `dedup_datums` the combined bundle has `axes = [Z@0, Z@5]` (len 2), so
    /// `feature_datum_projection` emits FeatureDatumAmbiguous + Undef.
    #[test]
    fn feature_datum_projection_over_selector_receiver_dedups_coaxial_faces_to_single_axis() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ty::SelectorKind;
        use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent = reify_ir::GeometryHandleId(1);
        let face_a = reify_ir::GeometryHandleId(10);
        let face_b = reify_ir::GeometryHandleId(11);

        let sv = reify_ir::value::SelectorValue::leaf(
            SelectorKind::Face,
            reify_ir::value::GeometryHandleRef {
                realization_ref: RealizationNodeId::new("S", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: parent,
            },
            reify_ir::value::LeafQuery::All,
        )
        .expect("SelectorValue::leaf for Face/All must succeed");

        // selector resolve:     extract_faces(parent) → [face_a, face_b]
        // bundle(face_a):       extract_faces(face_a) → [face_a]
        //                       FaceAnalyticDatum(face_a) → Axis at z=0, dir +Z
        // bundle(face_b):       extract_faces(face_b) → [face_b]
        //                       FaceAnalyticDatum(face_b) → Axis at z=5, dir +Z
        // → two coaxial Z axes, perpendicular distance = 0 → dedup_datums merges to 1
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent, vec![face_a, face_b])
            .with_extracted_faces(face_a, vec![face_a])
            .with_extracted_faces(face_b, vec![face_b])
            .with_face_analytic_datum_result(face_a, z_axis_value_at(0.0))
            .with_face_analytic_datum_result(face_b, z_axis_value_at(5.0));

        let mut values = reify_ir::ValueMap::new();
        values.insert(ValueCellId::new("S", "s"), reify_ir::Value::Selector(sv));

        let object = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("S", "s"),
            Type::Selector(SelectorKind::Face),
        );
        let expr =
            reify_ir::CompiledExpr::method_call(object, "axis".to_string(), vec![], Type::Axis);

        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        let value = result.expect(
            "hydrated-selector s.axis (two coaxial cyl faces) must yield Some(..), not None",
        );
        assert_value_axis_is_z_line(&value);

        let ambiguous_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Error
                    && d.code == Some(DiagnosticCode::FeatureDatumAmbiguous)
            })
            .collect();
        assert!(
            ambiguous_errors.is_empty(),
            "a hydrated-selector s.axis over two coaxial faces must dedup to one axis \
             and emit zero FeatureDatumAmbiguous errors; got {diagnostics:?}"
        );
    }

    /// `s.axis` where the selector resolves to TWO genuinely non-coaxial cylindrical
    /// faces (one on Z, one on X) must NOT merge the axes and must emit exactly one
    /// `FeatureDatumAmbiguous` error and return `Some(Value::Undef)`.
    ///
    /// This guards the most important regression: real ambiguity is still surfaced
    /// even after the cross-handle dedup pass.  The dedup step merges *only* datums
    /// that are geometrically equivalent; distinct axes must survive and produce a
    /// diagnostic rather than silently picking one.
    #[test]
    fn feature_datum_projection_over_selector_receiver_ambiguous_non_coaxial_faces_emit_diagnostic()
    {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ty::SelectorKind;
        use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent = reify_ir::GeometryHandleId(1);
        let face_z = reify_ir::GeometryHandleId(10); // axis on +Z
        let face_x = reify_ir::GeometryHandleId(11); // axis on +X (perpendicular to face_z)

        let sv = reify_ir::value::SelectorValue::leaf(
            SelectorKind::Face,
            reify_ir::value::GeometryHandleRef {
                realization_ref: RealizationNodeId::new("S", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: parent,
            },
            reify_ir::value::LeafQuery::All,
        )
        .expect("SelectorValue::leaf for Face/All must succeed");

        // An axis on +X, perpendicular to Z — not coaxial with the Z axis so
        // dedup_datums will NOT merge the two.
        let x_axis_value = reify_ir::Value::Axis {
            origin: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
            ])),
            direction: Box::new(reify_ir::Value::Direction {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            }),
        };

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent, vec![face_z, face_x])
            .with_extracted_faces(face_z, vec![face_z])
            .with_extracted_faces(face_x, vec![face_x])
            .with_face_analytic_datum_result(face_z, z_axis_value_at(0.0))
            .with_face_analytic_datum_result(face_x, x_axis_value);

        let mut values = reify_ir::ValueMap::new();
        values.insert(ValueCellId::new("S", "s"), reify_ir::Value::Selector(sv));

        let object = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("S", "s"),
            Type::Selector(SelectorKind::Face),
        );
        let expr =
            reify_ir::CompiledExpr::method_call(object, "axis".to_string(), vec![], Type::Axis);

        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        let value = result.expect("non-coaxial s.axis must yield Some(Value::Undef), not None");
        assert!(
            matches!(value, reify_ir::Value::Undef),
            "non-coaxial s.axis must return Value::Undef; got {value:?}"
        );

        let ambiguous_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Error
                    && d.code == Some(DiagnosticCode::FeatureDatumAmbiguous)
            })
            .collect();
        assert_eq!(
            ambiguous_errors.len(),
            1,
            "non-coaxial s.axis must emit exactly one FeatureDatumAmbiguous error; \
             got {diagnostics:?}"
        );
    }

    /// `s.axis` where the selector's `topology_selectors::resolve` returns `Err`
    /// (e.g. `extract_faces` on the parent handle fails) must push a
    /// `Severity::Warning` and return `Some(Value::Undef)` — not a hard error, not
    /// `None`.
    ///
    /// Mirrors the `try_eval_resolve_selector` Err handling precedent
    /// (geometry_ops.rs `@try_eval_resolve_selector` Warning arm).
    #[test]
    fn feature_datum_projection_over_selector_receiver_resolve_error_emits_warning_and_undef() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ty::SelectorKind;
        use reify_core::{Severity, Type, ValueCellId};
        use reify_ir::QueryError;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent = reify_ir::GeometryHandleId(1);

        let sv = reify_ir::value::SelectorValue::leaf(
            SelectorKind::Face,
            reify_ir::value::GeometryHandleRef {
                realization_ref: RealizationNodeId::new("S", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: parent,
            },
            reify_ir::value::LeafQuery::All,
        )
        .expect("SelectorValue::leaf for Face/All must succeed");

        // Inject an error so that extract_faces(parent) fails → resolve returns Err.
        let mut kernel = MockGeometryKernel::new().with_extract_faces_error(
            parent,
            QueryError::QueryFailed("mock extract_faces failure for test".to_string()),
        );

        let mut values = reify_ir::ValueMap::new();
        values.insert(ValueCellId::new("S", "s"), reify_ir::Value::Selector(sv));

        let object = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("S", "s"),
            Type::Selector(SelectorKind::Face),
        );
        let expr =
            reify_ir::CompiledExpr::method_call(object, "axis".to_string(), vec![], Type::Axis);

        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        let value = result.expect(
            "resolve-error s.axis must yield Some(Value::Undef), not None",
        );
        assert!(
            matches!(value, reify_ir::Value::Undef),
            "resolve-error s.axis must return Value::Undef; got {value:?}"
        );

        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert!(
            !warnings.is_empty(),
            "resolve-error s.axis must push at least one Severity::Warning diagnostic; \
             got {diagnostics:?}"
        );
    }

    // ── Scalar-branch coverage (suggestion from review, task 3622 amend) ────
    //
    // Both dispatch_edge_length and dispatch_perimeter accept
    // `Ok(Value::Scalar { si_value, .. })` in addition to `Ok(Value::Real(_))`
    // (following the kernel_distance Real-or-Scalar precedent). These tests
    // verify that the Scalar arm accumulates correctly and is not dead code.

    /// `length(edge_sub_handle)` when the kernel returns
    /// `Value::Scalar{si_value: 0.03, dimension: LENGTH}` must accept it and
    /// return `Some(Value::length(0.03))`.
    #[test]
    fn try_eval_topology_selector_length_scalar_reply_accepted_as_length() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let edge_kh = reify_ir::GeometryHandleId(30);
        let parent_rr = RealizationNodeId::new("LengthScalarTest", 0);
        let parent_hash: [u8; 32] = [0x60; 32];
        // Stage a Scalar{LENGTH} reply instead of a plain Real.
        let scalar_reply = reify_ir::Value::Scalar {
            si_value: 0.03,
            dimension: reify_core::DimensionVector::LENGTH,
        };
        let mut kernel = MockGeometryKernel::new().with_edge_length_result(edge_kh, scalar_reply);

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("LengthScalarTest", "e"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: edge_kh,
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "length",
            "LengthScalarTest",
            "e",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::length(0.03)),
            "length() with Scalar reply must return Some(Value::length(0.03)); \
             got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            diagnostics.is_empty(),
            "Scalar-reply length must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `perimeter(face_sub_handle)` where one edge returns `Value::Scalar{LENGTH}`
    /// instead of `Value::Real` must accumulate `si_value` correctly and return
    /// `Some(Value::length(total))` with zero diagnostics.
    #[test]
    fn try_eval_topology_selector_perimeter_scalar_edge_length_accepted_in_sum() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let face_kh = reify_ir::GeometryHandleId(31);
        let e1 = reify_ir::GeometryHandleId(32);
        let e2 = reify_ir::GeometryHandleId(33);
        let parent_rr = RealizationNodeId::new("PerimScalarTest", 0);
        let parent_hash: [u8; 32] = [0x61; 32];
        // e1 returns Real(3.0); e2 returns Scalar{si_value: 7.0, LENGTH}.
        // Sum = 10.0 exactly.
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(face_kh, vec![e1, e2])
            .with_edge_length_result(e1, reify_ir::Value::Real(3.0))
            .with_edge_length_result(
                e2,
                reify_ir::Value::Scalar {
                    si_value: 7.0,
                    dimension: reify_core::DimensionVector::LENGTH,
                },
            );

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("PerimScalarTest", "f"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: face_kh,
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "perimeter",
            "PerimScalarTest",
            "f",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::length(10.0)),
            "perimeter() with mixed Real/Scalar edge lengths must return \
             Some(Value::length(10.0)); got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            diagnostics.is_empty(),
            "Scalar-reply perimeter must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `perimeter(face_sub_handle)` when `extract_edges` returns an empty list
    /// must yield `Some(Value::Undef)` + exactly one Warning (degenerate face
    /// guard, task 3622 amend).
    #[test]
    fn try_eval_topology_selector_perimeter_empty_edges_returns_undef_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let face_kh = reify_ir::GeometryHandleId(34);
        let parent_rr = RealizationNodeId::new("PerimEmptyTest", 0);
        let parent_hash: [u8; 32] = [0x62; 32];
        // Stage an empty edge list.
        let mut kernel = MockGeometryKernel::new().with_extracted_edges(face_kh, vec![]);

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("PerimEmptyTest", "f"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: face_kh,
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "perimeter",
            "PerimEmptyTest",
            "f",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "perimeter() with empty edge list must yield Some(Value::Undef); \
             got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "empty edge list must emit exactly one Warning; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("perimeter"),
            "diagnostic must mention 'perimeter'; got: {}",
            diag.message
        );
    }

    // -------------------------------------------------------------------------
    // eval_sub_pose tests (T4: sub placement pose evaluation)
    // -------------------------------------------------------------------------

    /// `eval_sub_pose(None, ...)` must return an identity child→parent Transform
    /// (Orientation(1,0,0,0) rotation; Vector[length(0), length(0), length(0)] translation)
    /// and push no diagnostics.
    ///
    /// RED: fails to compile until `eval_sub_pose` is defined (step-2).
    #[test]
    fn eval_sub_pose_none_returns_identity_transform() {
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            None,
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "None pose must not push any diagnostics; got: {:?}",
            diagnostics
        );

        match result {
            reify_ir::Value::Transform {
                rotation,
                translation,
            } => {
                assert_eq!(
                    *rotation,
                    reify_ir::Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0
                    },
                    "identity rotation must be Orientation(1,0,0,0); got {:?}",
                    rotation
                );
                match *translation {
                    reify_ir::Value::Vector(ref components) => {
                        assert_eq!(
                            components.len(),
                            3,
                            "identity translation must have 3 components; got {}",
                            components.len()
                        );
                        for (i, c) in components.iter().enumerate() {
                            assert_eq!(
                                c,
                                &reify_ir::Value::length(0.0),
                                "identity translation component {} must be length(0.0); got {:?}",
                                i,
                                c
                            );
                        }
                    }
                    ref other => panic!("identity translation must be a Vector; got {:?}", other),
                }
            }
            other => panic!("expected Value::Transform for None pose; got {:?}", other),
        }
    }

    /// `eval_sub_pose(Some(&transform_expr), ...)` must return the Transform unchanged
    /// (passthrough) and push no diagnostics.
    ///
    /// Pins the step-3/4 contract: a pose that is already a Transform is not altered.
    #[test]
    fn eval_sub_pose_transform_passthrough() {
        let s = std::f64::consts::FRAC_1_SQRT_2; // 90° about Z: (s, 0, 0, s)
        let input_transform = reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Orientation {
                w: s,
                x: 0.0,
                y: 0.0,
                z: s,
            }),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(10.0),
                reify_ir::Value::length(20.0),
                reify_ir::Value::length(30.0),
            ])),
        };
        let expr = reify_ir::CompiledExpr::literal(
            input_transform.clone(),
            reify_core::Type::transform(3),
        );

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Transform passthrough must not push diagnostics; got: {:?}",
            diagnostics
        );
        assert_eq!(
            result, input_transform,
            "Transform passthrough must return the input unchanged"
        );
    }

    /// `eval_sub_pose` with a `Frame { origin: Point([1m, 2m, 3m]), basis: 90°Z }` must
    /// lower to `Transform { rotation: 90°Z, translation: Vector([1m, 2m, 3m]) }`.
    ///
    /// This is the PRD §11 Q1 convention-pinning numeric test (step-5/6).
    /// Derivation: Transform{Q,t} maps child-local p to parent Q·p + t.
    /// Carrying identity frame onto Frame{o, R} forces t = o and Q = R.
    #[test]
    fn eval_sub_pose_frame_lowers_to_transform_convention() {
        let s = std::f64::consts::FRAC_1_SQRT_2; // 90° about Z
        let input_frame = reify_ir::Value::Frame {
            origin: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(1.0),
                reify_ir::Value::length(2.0),
                reify_ir::Value::length(3.0),
            ])),
            basis: Box::new(reify_ir::Value::Orientation {
                w: s,
                x: 0.0,
                y: 0.0,
                z: s,
            }),
        };
        let expr = reify_ir::CompiledExpr::literal(input_frame, reify_core::Type::frame(3));

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Frame lowering must not push diagnostics; got: {:?}",
            diagnostics
        );

        match result {
            reify_ir::Value::Transform {
                rotation,
                translation,
            } => {
                // Convention: rotation == Frame.basis (exact copy, no normalization)
                assert_eq!(
                    *rotation,
                    reify_ir::Value::Orientation {
                        w: s,
                        x: 0.0,
                        y: 0.0,
                        z: s
                    },
                    "lowered rotation must equal Frame basis; got {:?}",
                    rotation
                );
                // Convention: translation == Frame.origin components as Vector
                match *translation {
                    reify_ir::Value::Vector(ref components) => {
                        assert_eq!(components.len(), 3);
                        assert_eq!(components[0], reify_ir::Value::length(1.0));
                        assert_eq!(components[1], reify_ir::Value::length(2.0));
                        assert_eq!(components[2], reify_ir::Value::length(3.0));
                    }
                    ref other => panic!("lowered translation must be a Vector; got {:?}", other),
                }
            }
            other => panic!(
                "expected Value::Transform after Frame lowering; got {:?}",
                other
            ),
        }
    }

    /// `eval_sub_pose(Some(&non_pose_expr), ...)` must return `Value::Undef` and
    /// push exactly one `Diagnostic::error`.
    ///
    /// T4 owns pose type-validation (T2 deferred it). Pins the step-7/8 contract.
    #[test]
    fn eval_sub_pose_non_pose_value_returns_undef_with_diagnostic() {
        let expr =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Real(5.0), reify_core::Type::dimensionless_scalar());

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_undef(),
            "non-pose value must return Value::Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-pose value must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error severity; got {:?}",
            diagnostics[0].severity
        );
    }

    // -------------------------------------------------------------------------
    // Frame validation branch tests (Suggestion 1: test_coverage)
    // Each of the four Frame-specific guard clauses must individually produce
    // exactly one Diagnostic::error and return Value::Undef.
    // -------------------------------------------------------------------------

    /// Helper: a valid unit Orientation (identity).
    fn identity_orientation() -> reify_ir::Value {
        reify_ir::Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// Helper: a valid 3-component LENGTH Point.
    fn valid_origin() -> reify_ir::Value {
        reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ])
    }

    /// Frame origin is not a `Value::Point` at all (e.g. a bare `Value::Real`).
    /// The first guard in the Frame arm must fire.
    #[test]
    fn eval_sub_pose_frame_non_point_origin_errors() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Frame {
                origin: Box::new(reify_ir::Value::Real(1.0)),
                basis: Box::new(identity_orientation()),
            },
            reify_core::Type::frame(3),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "non-Point origin must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Point origin must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    /// Frame origin is a `Value::Point` but with only 2 components (not 3).
    /// The component-count guard must fire.
    #[test]
    fn eval_sub_pose_frame_origin_two_components_errors() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Frame {
                origin: Box::new(reify_ir::Value::Point(vec![
                    reify_ir::Value::length(1.0),
                    reify_ir::Value::length(2.0),
                    // missing third component
                ])),
                basis: Box::new(identity_orientation()),
            },
            reify_core::Type::frame(3),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "2-component origin must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "2-component origin must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    /// Frame origin has a component with ANGLE dimension instead of LENGTH.
    /// The dimension guard must fire.
    #[test]
    fn eval_sub_pose_frame_origin_non_length_dimension_errors() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Frame {
                origin: Box::new(reify_ir::Value::Point(vec![
                    // First component is ANGLE-dimensioned — invalid for a Point origin
                    reify_ir::Value::Scalar {
                        si_value: 1.0,
                        dimension: reify_core::DimensionVector::ANGLE,
                    },
                    reify_ir::Value::length(2.0),
                    reify_ir::Value::length(3.0),
                ])),
                basis: Box::new(identity_orientation()),
            },
            reify_core::Type::frame(3),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "non-LENGTH origin component must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-LENGTH origin must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    /// Frame origin has a non-finite (NaN) coordinate.
    /// The `si_value.is_finite()` guard must fire.
    #[test]
    fn eval_sub_pose_frame_origin_nan_coordinate_errors() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Frame {
                origin: Box::new(reify_ir::Value::Point(vec![
                    reify_ir::Value::Scalar {
                        si_value: f64::NAN,
                        dimension: reify_core::DimensionVector::LENGTH,
                    },
                    reify_ir::Value::length(2.0),
                    reify_ir::Value::length(3.0),
                ])),
                basis: Box::new(identity_orientation()),
            },
            reify_core::Type::frame(3),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "NaN coordinate must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "NaN coordinate must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    /// Frame basis is not a `Value::Orientation` (e.g. a bare `Value::Real`).
    /// The basis-variant guard must fire.
    #[test]
    fn eval_sub_pose_frame_non_orientation_basis_errors() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Frame {
                origin: Box::new(valid_origin()),
                basis: Box::new(reify_ir::Value::Real(1.0)),
            },
            reify_core::Type::frame(3),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "non-Orientation basis must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Orientation basis must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    // -------------------------------------------------------------------------
    // Undef behavior test (Suggestion 3: robustness — pins the chosen behavior)
    // -------------------------------------------------------------------------

    /// A pose expression that evaluates to `Value::Undef` (simulating an upstream
    /// evaluation failure) must produce exactly one `Diagnostic::error`.
    ///
    /// This pins the intentional design choice: we emit a call-site error even when
    /// the expression already produced Undef, giving the consumer a placement-site
    /// anchor in addition to whatever upstream diagnostic the expression emitted.
    /// See the comment in the `_` catch-all arm of `eval_sub_pose` for the rationale.
    #[test]
    fn eval_sub_pose_undef_expr_returns_undef_with_diagnostic() {
        // Build an expression that evaluates to Value::Undef directly.
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Undef,
            reify_core::Type::dimensionless_scalar(), // type doesn't matter; the value is Undef
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "Undef-evaluating pose must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "Undef pose must push exactly one call-site diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "call-site diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    // ── T5 step-7: pose decompose / compose helpers ──────────────────────────

    /// Build a `Value::Transform` from a raw quaternion `[w,x,y,z]` and a
    /// LENGTH-dimensioned translation `[tx,ty,tz]` (metres).
    fn transform_of(q: [f64; 4], t: [f64; 3]) -> reify_ir::Value {
        reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Orientation {
                w: q[0],
                x: q[1],
                y: q[2],
                z: q[3],
            }),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(t[0]),
                reify_ir::Value::length(t[1]),
                reify_ir::Value::length(t[2]),
            ])),
        }
    }

    /// The canonical identity `Value::Transform` (mirrors `eval_sub_pose`'s
    /// `None` arm and step-8's `compose_pose_chain` seed).
    fn identity_transform() -> reify_ir::Value {
        transform_of([1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0])
    }

    #[test]
    fn decompose_transform_to_arrays_extracts_quat_and_si_translation() {
        let v = transform_of([0.5, 0.5, 0.5, 0.5], [0.03, -0.01, 0.2]);
        let (q, t) = decompose_transform_to_arrays(&v).expect("valid Transform must decompose");
        assert_eq!(q, [0.5, 0.5, 0.5, 0.5], "quaternion [w,x,y,z]");
        assert_eq!(t, [0.03, -0.01, 0.2], "translation in SI metres");
    }

    #[test]
    fn decompose_transform_to_arrays_rejects_non_transform() {
        assert!(decompose_transform_to_arrays(&reify_ir::Value::Real(1.0)).is_none());
        assert!(decompose_transform_to_arrays(&reify_ir::Value::Undef).is_none());
    }

    #[test]
    fn decompose_transform_to_arrays_rejects_non_orientation_rotation() {
        // rotation is a Vector, not an Orientation → reject.
        let v = reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(1.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
            ])),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
            ])),
        };
        assert!(decompose_transform_to_arrays(&v).is_none());
    }

    #[test]
    fn decompose_transform_to_arrays_rejects_wrong_length_translation() {
        let v = reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
            ])),
        };
        assert!(decompose_transform_to_arrays(&v).is_none());
    }

    #[test]
    fn decompose_transform_to_arrays_rejects_mixed_dimension_translation() {
        // one ANGLE component among LENGTHs → reject (mixed dimensions).
        let v = reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::angle(0.0),
                reify_ir::Value::length(0.0),
            ])),
        };
        assert!(decompose_transform_to_arrays(&v).is_none());
    }

    #[test]
    fn compose_pose_chain_two_equals_eval_builtin() {
        // Identity-quaternion (translation-only) transforms compose exactly
        // (quat_mul / quat_rotate by identity are bit-exact), so the left-fold
        // from identity collapses to a plain transform_compose of the pair.
        let t1 = transform_of([1.0, 0.0, 0.0, 0.0], [0.01, 0.0, 0.0]);
        let t2 = transform_of([1.0, 0.0, 0.0, 0.0], [0.0, 0.02, 0.0]);
        let got = compose_pose_chain(&[t1.clone(), t2.clone()]);
        let expected = reify_stdlib::eval_builtin("transform_compose", &[t1, t2]);
        assert_eq!(
            got, expected,
            "compose_pose_chain([t1,t2]) must equal transform_compose(t1,t2)"
        );
    }

    #[test]
    fn compose_pose_chain_empty_is_identity() {
        assert_eq!(
            compose_pose_chain(&[]),
            identity_transform(),
            "empty chain must be the identity Transform"
        );
    }

    #[test]
    fn compose_pose_chain_single_equals_compose_onto_identity() {
        let t = transform_of([1.0, 0.0, 0.0, 0.0], [0.05, 0.0, 0.0]);
        let got = compose_pose_chain(std::slice::from_ref(&t));
        let expected = reify_stdlib::eval_builtin("transform_compose", &[identity_transform(), t]);
        assert_eq!(
            got, expected,
            "single-element chain == transform_compose(identity, t)"
        );
    }

    // ── decode_plane unit tests (task η, step-1) ─────────────────────────────

    /// True producer→decode round-trip for plane_xy: the real stdlib producer
    /// is used so the test exercises the full Plane value shape that consumers
    /// will encounter at eval time.
    #[test]
    fn decode_plane_producer_round_trip_plane_xy() {
        // plane_xy(3mm) → Plane at z=0.003 m, normal=[0,0,1]
        let z_si = 0.003_f64;
        let val = reify_stdlib::eval_builtin("plane_xy", &[reify_ir::Value::length(z_si)]);
        let (origin, normal) = decode_plane(&val).expect("plane_xy should decode cleanly");
        assert!((origin[0] - 0.0).abs() < 1e-12, "ox must be 0.0, got {}", origin[0]);
        assert!((origin[1] - 0.0).abs() < 1e-12, "oy must be 0.0, got {}", origin[1]);
        assert!((origin[2] - z_si).abs() < 1e-12, "oz must be {z_si}, got {}", origin[2]);
        assert!((normal[0] - 0.0).abs() < 1e-12, "nx must be 0.0, got {}", normal[0]);
        assert!((normal[1] - 0.0).abs() < 1e-12, "ny must be 0.0, got {}", normal[1]);
        assert!((normal[2] - 1.0).abs() < 1e-12, "nz must be 1.0, got {}", normal[2]);
    }

    /// True producer→decode round-trip for plane_xz: offset lands in Y
    /// (index 1) and the normal is [0,1,0].
    #[test]
    fn decode_plane_producer_round_trip_plane_xz() {
        // plane_xz(5mm) → Plane at y=0.005 m, normal=[0,1,0]
        let z_si = 0.005_f64;
        let val = reify_stdlib::eval_builtin("plane_xz", &[reify_ir::Value::length(z_si)]);
        let (origin, normal) = decode_plane(&val).expect("plane_xz should decode cleanly");
        assert!((origin[0] - 0.0).abs() < 1e-12, "ox must be 0.0, got {}", origin[0]);
        assert!((origin[1] - z_si).abs() < 1e-12, "oy must be {z_si}, got {}", origin[1]);
        assert!((origin[2] - 0.0).abs() < 1e-12, "oz must be 0.0, got {}", origin[2]);
        assert!((normal[0] - 0.0).abs() < 1e-12, "nx must be 0.0, got {}", normal[0]);
        assert!((normal[1] - 1.0).abs() < 1e-12, "ny must be 1.0, got {}", normal[1]);
        assert!((normal[2] - 0.0).abs() < 1e-12, "nz must be 0.0, got {}", normal[2]);
    }

    /// True producer→decode round-trip for plane_yz: offset lands in X
    /// (index 0) and the normal is [1,0,0].
    #[test]
    fn decode_plane_producer_round_trip_plane_yz() {
        // plane_yz(7mm) → Plane at x=0.007 m, normal=[1,0,0]
        let z_si = 0.007_f64;
        let val = reify_stdlib::eval_builtin("plane_yz", &[reify_ir::Value::length(z_si)]);
        let (origin, normal) = decode_plane(&val).expect("plane_yz should decode cleanly");
        assert!((origin[0] - z_si).abs() < 1e-12, "ox must be {z_si}, got {}", origin[0]);
        assert!((origin[1] - 0.0).abs() < 1e-12, "oy must be 0.0, got {}", origin[1]);
        assert!((origin[2] - 0.0).abs() < 1e-12, "oz must be 0.0, got {}", origin[2]);
        assert!((normal[0] - 1.0).abs() < 1e-12, "nx must be 1.0, got {}", normal[0]);
        assert!((normal[1] - 0.0).abs() < 1e-12, "ny must be 0.0, got {}", normal[1]);
        assert!((normal[2] - 0.0).abs() < 1e-12, "nz must be 0.0, got {}", normal[2]);
    }

    /// A Plane whose normal vector has magnitude 2 (non-unit) must be
    /// normalized to a unit normal by decode_plane — never returned as-is.
    #[test]
    fn decode_plane_normalizes_non_unit_normal() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let non_unit_normal = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(2.0),
        ]);
        let plane = reify_ir::Value::Plane {
            origin: Box::new(origin),
            normal: Box::new(non_unit_normal),
        };
        let (_, normal) =
            decode_plane(&plane).expect("non-unit normal [0,0,2] should normalize without error");
        assert!((normal[0] - 0.0).abs() < 1e-12, "nx must be 0.0, got {}", normal[0]);
        assert!((normal[1] - 0.0).abs() < 1e-12, "ny must be 0.0, got {}", normal[1]);
        assert!((normal[2] - 1.0).abs() < 1e-12, "nz must be 1.0 after normalization, got {}", normal[2]);
    }

    /// Value::Axis must be rejected by decode_plane — wrong variant.
    #[test]
    fn decode_plane_rejects_axis_value() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let dir = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(1.0),
        ]);
        let axis = reify_ir::Value::Axis {
            origin: Box::new(origin),
            direction: Box::new(dir),
        };
        assert!(
            decode_plane(&axis).is_err(),
            "Value::Axis must be rejected by decode_plane (wrong variant)"
        );
    }

    /// Value::Undef must be rejected by decode_plane — never silently pass through.
    #[test]
    fn decode_plane_rejects_undef() {
        assert!(
            decode_plane(&reify_ir::Value::Undef).is_err(),
            "Value::Undef must be rejected by decode_plane"
        );
    }

    /// A Plane with a zero-magnitude normal must be rejected — the decoder
    /// must never return (0,0,0) as the unit normal.
    #[test]
    fn decode_plane_rejects_zero_magnitude_normal() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let zero_normal = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
        ]);
        let plane = reify_ir::Value::Plane {
            origin: Box::new(origin),
            normal: Box::new(zero_normal),
        };
        assert!(
            decode_plane(&plane).is_err(),
            "zero-magnitude normal must be rejected by decode_plane (never pass through as [0,0,0])"
        );
    }

    // ── decode_axis unit tests (task η, step-3) ─────────────────────────────

    /// Helper: build a Value::Point with three LENGTH-dimensioned components
    /// (metres), as produced by point3() in the stdlib.
    fn make_point3_length_val(x: f64, y: f64, z: f64) -> reify_ir::Value {
        reify_ir::Value::Point(vec![
            reify_ir::Value::length(x),
            reify_ir::Value::length(y),
            reify_ir::Value::length(z),
        ])
    }

    /// True producer→decode round-trip for axis_z with origin at (0,0,0).
    /// decode_axis must return origin=[0,0,0] and direction=[0,0,1].
    #[test]
    fn decode_axis_producer_round_trip_axis_z_origin() {
        let origin = make_point3_length_val(0.0, 0.0, 0.0);
        let val = reify_stdlib::eval_builtin("axis_z", std::slice::from_ref(&origin));
        let (got_origin, got_dir) = decode_axis(&val).expect("axis_z should decode cleanly");
        assert!((got_origin[0] - 0.0).abs() < 1e-12, "ox must be 0.0, got {}", got_origin[0]);
        assert!((got_origin[1] - 0.0).abs() < 1e-12, "oy must be 0.0, got {}", got_origin[1]);
        assert!((got_origin[2] - 0.0).abs() < 1e-12, "oz must be 0.0, got {}", got_origin[2]);
        assert!((got_dir[0] - 0.0).abs() < 1e-12, "dx must be 0.0, got {}", got_dir[0]);
        assert!((got_dir[1] - 0.0).abs() < 1e-12, "dy must be 0.0, got {}", got_dir[1]);
        assert!((got_dir[2] - 1.0).abs() < 1e-12, "dz must be 1.0, got {}", got_dir[2]);
    }

    /// axis_x round-trip: direction=[1,0,0], origin passes through in SI metres.
    #[test]
    fn decode_axis_producer_round_trip_axis_x_with_offset_origin() {
        // 1mm=0.001m, 2mm=0.002m, 3mm=0.003m
        let origin = make_point3_length_val(0.001, 0.002, 0.003);
        let val = reify_stdlib::eval_builtin("axis_x", std::slice::from_ref(&origin));
        let (got_origin, got_dir) = decode_axis(&val).expect("axis_x with offset origin should decode");
        assert!((got_origin[0] - 0.001).abs() < 1e-12, "ox must be 0.001, got {}", got_origin[0]);
        assert!((got_origin[1] - 0.002).abs() < 1e-12, "oy must be 0.002, got {}", got_origin[1]);
        assert!((got_origin[2] - 0.003).abs() < 1e-12, "oz must be 0.003, got {}", got_origin[2]);
        assert!((got_dir[0] - 1.0).abs() < 1e-12, "dx must be 1.0, got {}", got_dir[0]);
        assert!((got_dir[1] - 0.0).abs() < 1e-12, "dy must be 0.0, got {}", got_dir[1]);
        assert!((got_dir[2] - 0.0).abs() < 1e-12, "dz must be 0.0, got {}", got_dir[2]);
    }

    /// axis_y round-trip: direction=[0,1,0].
    #[test]
    fn decode_axis_producer_round_trip_axis_y() {
        let origin = make_point3_length_val(0.0, 0.0, 0.0);
        let val = reify_stdlib::eval_builtin("axis_y", std::slice::from_ref(&origin));
        let (got_origin, got_dir) = decode_axis(&val).expect("axis_y should decode cleanly");
        assert!((got_dir[0] - 0.0).abs() < 1e-12, "dx must be 0.0, got {}", got_dir[0]);
        assert!((got_dir[1] - 1.0).abs() < 1e-12, "dy must be 1.0, got {}", got_dir[1]);
        assert!((got_dir[2] - 0.0).abs() < 1e-12, "dz must be 0.0, got {}", got_dir[2]);
        // origin must be [0,0,0]
        assert!((got_origin[0] - 0.0).abs() < 1e-12, "ox");
        assert!((got_origin[1] - 0.0).abs() < 1e-12, "oy");
        assert!((got_origin[2] - 0.0).abs() < 1e-12, "oz");
    }

    /// A non-unit direction (magnitude 2) must be normalized to unit length.
    #[test]
    fn decode_axis_normalizes_non_unit_direction() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let non_unit_dir = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(2.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
        ]);
        let axis = reify_ir::Value::Axis {
            origin: Box::new(origin),
            direction: Box::new(non_unit_dir),
        };
        let (_, got_dir) =
            decode_axis(&axis).expect("non-unit direction [2,0,0] should normalize without error");
        assert!((got_dir[0] - 1.0).abs() < 1e-12, "dx must be 1.0 after normalization, got {}", got_dir[0]);
        assert!((got_dir[1] - 0.0).abs() < 1e-12, "dy");
        assert!((got_dir[2] - 0.0).abs() < 1e-12, "dz");
    }

    /// Value::Plane must be rejected by decode_axis — wrong variant.
    #[test]
    fn decode_axis_rejects_plane_value() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let normal = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(1.0),
        ]);
        let plane = reify_ir::Value::Plane {
            origin: Box::new(origin),
            normal: Box::new(normal),
        };
        assert!(
            decode_axis(&plane).is_err(),
            "Value::Plane must be rejected by decode_axis (wrong variant)"
        );
    }

    /// Value::Undef must be rejected by decode_axis.
    #[test]
    fn decode_axis_rejects_undef() {
        assert!(
            decode_axis(&reify_ir::Value::Undef).is_err(),
            "Value::Undef must be rejected by decode_axis"
        );
    }

    /// An Axis with a zero-magnitude direction must be rejected.
    #[test]
    fn decode_axis_rejects_zero_magnitude_direction() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let zero_dir = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
        ]);
        let axis = reify_ir::Value::Axis {
            origin: Box::new(origin),
            direction: Box::new(zero_dir),
        };
        assert!(
            decode_axis(&axis).is_err(),
            "zero-magnitude direction must be rejected by decode_axis"
        );
    }

    // ── step-7 (task 4190): split dispatch unit tests ────────────────────────
    //
    // Tests for the `split(solid, plane) -> List<Geometry>` dispatch arm in
    // `try_eval_topology_selector`.  These tests reference
    // `crate::topology_selectors::SubKind::Solid` (added in step-8) and
    // `TopologySelectorHelper::Split` (added in step-8), so the crate fails
    // to compile until step-8 is done → RED.

    /// Thin wrapper around `MockGeometryKernel` that overrides `execute_split`
    /// to return a configurable success result.  All other trait methods
    /// delegate to the inner mock.
    ///
    /// Required because `MockGeometryKernel` does not expose `execute_split`
    /// configuration (it is not in the mock's in-scope file list for this
    /// task), so we define a minimal delegating wrapper inline.
    struct SplitMockKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        /// Returned by every `execute_split` call (cloned on each call).
        split_ids: Vec<GeometryHandleId>,
    }

    impl SplitMockKernel {
        fn new(
            inner: reify_test_support::mocks::MockGeometryKernel,
            split_ids: Vec<GeometryHandleId>,
        ) -> Self {
            Self { inner, split_ids }
        }
    }

    impl reify_ir::GeometryKernel for SplitMockKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            query: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(query)
        }

        fn export(
            &self,
            handle: GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.inner.tessellate(handle, tolerance)
        }

        fn execute_split(
            &mut self,
            _op: &reify_ir::GeometryOp,
        ) -> Result<Vec<GeometryHandleId>, reify_ir::GeometryError> {
            Ok(self.split_ids.clone())
        }
    }

    /// Build a `Value::Plane` with a z=0 normal (z-axis cutting plane) for use
    /// as the plane argument in split dispatch tests.
    fn z_plane_value() -> reify_ir::Value {
        reify_ir::Value::Plane {
            origin: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
            ])),
            normal: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ])),
        }
    }

    /// `split(solid, plane)` dispatch returns `Value::List` of two
    /// `Value::GeometryHandle` elements when the mock kernel returns
    /// [GHId(5), GHId(6)] from `execute_split`.
    ///
    /// Each element must:
    ///   (i)  carry the parent solid's `realization_ref` (unchanged, PRD §4 i);
    ///   (ii) have a `upstream_values_hash` distinct from the other piece
    ///        (PRD §4 iii) — derived from `SubKind::Solid` discriminant (0x03)
    ///        via `compose_sub_handle_hash`.
    ///
    /// RED: `SubKind::Solid` does not exist yet → compile error.
    #[test]
    fn split_dispatch_returns_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("MySolid", 0);
        let parent_hash: [u8; 32] = [0xAB; 32];

        let piece_ids = vec![GeometryHandleId(5), GeometryHandleId(6)];
        let mut kernel = SplitMockKernel::new(
            reify_test_support::mocks::MockGeometryKernel::new(),
            piece_ids.clone(),
        );

        let mut named_steps = HashMap::new();
        named_steps.insert("solid".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[0]: parent solid as hydrated GeometryHandle in the values map.
        values.insert(
            ValueCellId::new("MySolid", "solid"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_handle,
            },
        );
        // args[1]: cutting plane as Value::Plane in the values map.
        values.insert(ValueCellId::new("MySolid", "plane"), z_plane_value());

        let expr = topology_selector_call_two_value_refs(
            "split",
            "MySolid",
            "solid",
            Type::Geometry,
            "plane",
            Type::Plane,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "split dispatch must return Some(Value::List(..)), got {:?}; \
                 diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(list.len(), 2, "expected 2 split pieces, got {}", list.len());
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for successful split, got: {:?}",
            diagnostics
        );

        // Verify each piece: correct realization_ref, correct kernel_handle,
        // distinct upstream_values_hash (SubKind::Solid domain-separation).
        let expected_kernel_ids = [GeometryHandleId(5), GeometryHandleId(6)];
        let mut hashes: Vec<[u8; 32]> = Vec::new();
        for (i, (elem, expected_id)) in list.iter().zip(&expected_kernel_ids).enumerate() {
            match elem {
                reify_ir::Value::GeometryHandle {
                    realization_ref,
                    upstream_values_hash,
                    kernel_handle,
                } => {
                    assert_eq!(
                        realization_ref.entity, parent_rr.entity,
                        "piece[{i}] realization_ref.entity must match parent"
                    );
                    assert_eq!(
                        realization_ref.index, parent_rr.index,
                        "piece[{i}] realization_ref.index must match parent"
                    );
                    assert_eq!(
                        kernel_handle, expected_id,
                        "piece[{i}] kernel_handle must be {expected_id:?}"
                    );
                    // Verify the hash uses SubKind::Solid (0x03) domain separator.
                    let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
                        &parent_hash,
                        crate::topology_selectors::SubKind::Solid, // RED: not yet defined
                        i as u32,
                    );
                    assert_eq!(
                        *upstream_values_hash, expected_hash,
                        "piece[{i}] upstream_values_hash must use SubKind::Solid"
                    );
                    hashes.push(*upstream_values_hash);
                }
                other => panic!("piece[{i}] is not Value::GeometryHandle: {:?}", other),
            }
        }
        // PRD §4 iii: per-index hashes must be distinct.
        assert_ne!(
            hashes[0], hashes[1],
            "split piece 0 and piece 1 hashes must differ (PRD §4 iii)"
        );
    }

    /// When args[1] is not a `Value::Plane` (e.g. a bare `Value::Real`),
    /// `split` dispatch must fall through to `None` so the cell retains its
    /// compiled default (`Value::Undef`).
    #[test]
    fn split_dispatch_falls_through_when_plane_arg_not_a_plane() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps = HashMap::new();
        named_steps.insert("solid".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[0]: valid parent solid.
        values.insert(
            ValueCellId::new("MySolid", "solid"),
            reify_ir::Value::GeometryHandle {
                realization_ref: reify_core::identity::RealizationNodeId::new("MySolid", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: parent_handle,
            },
        );
        // args[1]: NOT a Plane — should cause decode_plane to fail → fall through.
        values.insert(
            ValueCellId::new("MySolid", "plane"),
            reify_ir::Value::Real(0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "split",
            "MySolid",
            "solid",
            Type::Geometry,
            "plane",
            Type::dimensionless_scalar(),
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "non-Plane args[1] must fall through to None (cell stays Undef), got {:?}",
            result
        );
    }

    /// When `execute_split` returns an error, `split` dispatch must emit a
    /// `Warning` diagnostic and return `Some(Value::Undef)` — the same
    /// defensive-downgrade contract as other topology-selector dispatch arms.
    #[test]
    fn split_dispatch_emits_warning_and_undef_on_kernel_error() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        // Default MockGeometryKernel inherits the trait default for execute_split:
        // Err(GeometryError::OperationFailed("execute_split not supported by this kernel")).
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps = HashMap::new();
        named_steps.insert("solid".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("MySolid", "solid"),
            reify_ir::Value::GeometryHandle {
                realization_ref: reify_core::identity::RealizationNodeId::new("MySolid", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: parent_handle,
            },
        );
        values.insert(ValueCellId::new("MySolid", "plane"), z_plane_value());

        let expr = topology_selector_call_two_value_refs(
            "split",
            "MySolid",
            "solid",
            Type::Geometry,
            "plane",
            Type::Plane,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "kernel Err must produce Some(Value::Undef), got {:?}; \
             diagnostics: {:?}",
            result,
            diagnostics
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 Warning diagnostic on kernel error, \
             got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert!(
            matches!(diagnostics[0].severity, reify_core::Severity::Warning),
            "diagnostic must be a Warning, got {:?}",
            diagnostics[0].severity
        );
    }

    // ── resolve_subhandle_list (task 3205 step-7/8) ───────────────────────
    //
    // `resolve_subhandle_list(arg, parent)` is the KERNEL-FREE (pure
    // Value→Value) helper that lowers a List<Geometry> of KGQ sub-handles to a
    // canonical `Vec<GeometryHandleId>`: it requires a List of `GeometryHandle`
    // elements, rejects any element whose `realization_ref` differs from the
    // parent's (cross-solid gate), dedups by `kernel_handle`, and returns the
    // ids in ascending canonical order (matching extract_edges' mint order).
    // These cases are built from DIRECTLY-CONSTRUCTED handles via
    // `make_sub_handle` — no live build / scheduling required.

    /// (a) Happy path: a List of N edge sub-handles all sharing the parent's
    /// `realization_ref` resolves to their `kernel_handle` ids in ascending-id
    /// canonical order. The sub-handles are constructed OUT of ascending order
    /// to prove the resolver sorts (canonical = ascending kernel_handle id,
    /// matching extract_edges' TopExp mint order).
    #[test]
    fn resolve_subhandle_list_happy_path_canonical_order() {
        let ra = reify_core::identity::RealizationNodeId::new("PartA", 0);
        let parent_hash = [7u8; 32];
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra.clone(),
            upstream_values_hash: parent_hash,
            kernel_handle: GeometryHandleId(1),
        };
        // kernel handles deliberately scrambled to prove ascending canonical sort.
        let scrambled = [103u64, 101, 102, 100];
        let edges: Vec<reify_ir::Value> = scrambled
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                crate::topology_selectors::make_sub_handle(
                    &ra,
                    &parent_hash,
                    crate::topology_selectors::SubKind::Edge,
                    i as u32,
                    GeometryHandleId(id),
                )
            })
            .collect();
        let arg = reify_ir::Value::List(edges);
        let result = super::resolve_subhandle_list(&arg, &parent);
        assert_eq!(
            result,
            Ok(vec![
                GeometryHandleId(100),
                GeometryHandleId(101),
                GeometryHandleId(102),
                GeometryHandleId(103),
            ]),
            "happy path must resolve sub-handles to kernel_handle ids in \
             ascending canonical order"
        );
    }

    /// (b) Dedup: the same sub-handle listed twice collapses to one entry.
    #[test]
    fn resolve_subhandle_list_dedups_repeated_handle() {
        let ra = reify_core::identity::RealizationNodeId::new("PartA", 0);
        let parent_hash = [9u8; 32];
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra.clone(),
            upstream_values_hash: parent_hash,
            kernel_handle: GeometryHandleId(1),
        };
        let edge = crate::topology_selectors::make_sub_handle(
            &ra,
            &parent_hash,
            crate::topology_selectors::SubKind::Edge,
            0,
            GeometryHandleId(200),
        );
        let arg = reify_ir::Value::List(vec![edge.clone(), edge]);
        let result = super::resolve_subhandle_list(&arg, &parent);
        assert_eq!(
            result,
            Ok(vec![GeometryHandleId(200)]),
            "a repeated sub-handle must dedup to a single kernel_handle"
        );
    }

    /// (c) Cross-solid rejection: a sub-handle whose `realization_ref` differs
    /// from the parent's is rejected (a handle minted from a different solid).
    #[test]
    fn resolve_subhandle_list_rejects_cross_solid_handle() {
        let ra = reify_core::identity::RealizationNodeId::new("PartA", 0);
        let rb = reify_core::identity::RealizationNodeId::new("PartB", 0);
        let parent_hash = [1u8; 32];
        let other_hash = [2u8; 32];
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra.clone(),
            upstream_values_hash: parent_hash,
            kernel_handle: GeometryHandleId(1),
        };
        // One legit edge from PartA, one foreign edge from PartB.
        let good = crate::topology_selectors::make_sub_handle(
            &ra,
            &parent_hash,
            crate::topology_selectors::SubKind::Edge,
            0,
            GeometryHandleId(100),
        );
        let foreign = crate::topology_selectors::make_sub_handle(
            &rb,
            &other_hash,
            crate::topology_selectors::SubKind::Edge,
            0,
            GeometryHandleId(101),
        );
        let arg = reify_ir::Value::List(vec![good, foreign]);
        let result = super::resolve_subhandle_list(&arg, &parent);
        assert!(
            result.is_err(),
            "a sub-handle from a different realization_ref must be rejected \
             (cross-solid), got {:?}",
            result
        );
    }

    /// (d) Non-List arg: a non-List `Value` (e.g. `Real`) is rejected — the
    /// resolver requires a `List<Geometry>`.
    #[test]
    fn resolve_subhandle_list_rejects_non_list_arg() {
        let ra = reify_core::identity::RealizationNodeId::new("PartA", 0);
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra,
            upstream_values_hash: [3u8; 32],
            kernel_handle: GeometryHandleId(1),
        };
        let arg = reify_ir::Value::Real(2.0);
        let result = super::resolve_subhandle_list(&arg, &parent);
        assert!(
            result.is_err(),
            "a non-List arg must be rejected, got {:?}",
            result
        );
    }

    /// (e) Empty List: an empty selector list resolves to `Ok(vec![])`. The
    /// anti-zero-edges (E_EMPTY_SELECTION) guard lives in the eval arm, NOT in
    /// this kernel-free resolver — the resolver's job is purely structural.
    #[test]
    fn resolve_subhandle_list_empty_list_is_ok_empty() {
        let ra = reify_core::identity::RealizationNodeId::new("PartA", 0);
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra,
            upstream_values_hash: [4u8; 32],
            kernel_handle: GeometryHandleId(1),
        };
        let arg = reify_ir::Value::List(vec![]);
        let result = super::resolve_subhandle_list(&arg, &parent);
        assert_eq!(
            result,
            Ok(Vec::<GeometryHandleId>::new()),
            "an empty selector List must resolve to Ok(empty) — the \
             anti-zero-edges guard lives in the eval arm, not the resolver"
        );
    }

    // ── MaxDeviation via try_eval_geometry_query (ζ / C4) ───────────────────

    /// ζ / C4 (step-7 RED → step-8 GREEN): `max_deviation(actual, nominal)`
    /// direct call folds to `Value::Scalar<LENGTH>` via
    /// `try_eval_geometry_query`. The seeded kernel returns `Value::Real(5e-4)`
    /// (0.5 mm); the dispatch wraps it as
    /// `Scalar { dimension: LENGTH, si_value: 5e-4 }`.
    ///
    /// RED until step-8 wires the 2-arg `max_deviation` recognizer into
    /// `try_eval_geometry_query` (the current 1-arg gate returns `None` for a
    /// 2-arg `max_deviation` call).
    #[test]
    fn try_eval_geometry_query_max_deviation_direct_happy_path() {
        use reify_test_support::mocks::MockGeometryKernel;
        let actual = reify_ir::GeometryHandleId(20);
        let nominal = reify_ir::GeometryHandleId(21);
        // Tolerance matches the `MAX_DEVIATION_TESSELLATION_TOLERANCE_M` const
        // that step-8 will define in geometry_ops.rs.
        const TOL: f64 = 0.0001;
        let kernel = MockGeometryKernel::new()
            .with_max_deviation_result(actual, nominal, TOL, reify_ir::Value::Real(5e-4));

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("a".to_string(), kh(actual));
        named_steps.insert("b".to_string(), kh(nominal));

        let values = reify_ir::ValueMap::new();
        let functions: Vec<reify_ir::CompiledFunction> = Vec::new();
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();

        // max_deviation(a, b): 2-arg call, both args resolved from named_steps.
        let expr = topology_selector_call_two_value_refs(
            "max_deviation",
            "MaxDevTest",
            "a",
            reify_core::Type::Geometry,
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_geometry_query(
            &expr,
            &named_steps,
            &values,
            &functions,
            &meta_map,
            &kernel,
            &mut diagnostics,
        );

        match result {
            Some(reify_ir::Value::Scalar { si_value, dimension })
                if dimension == reify_core::DimensionVector::LENGTH =>
            {
                let expected = 5e-4_f64;
                let epsilon = 1e-12_f64;
                assert!(
                    (si_value - expected).abs() < epsilon,
                    "max_deviation direct call must produce si_value ≈ 5e-4; \
                     got {si_value:.15} (delta {delta:.3e})",
                    delta = (si_value - expected).abs()
                );
            }
            other => panic!(
                "max_deviation(actual, nominal) must return \
                 Some(Value::Scalar{{LENGTH, ≈5e-4}}); got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "happy-path max_deviation must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// ζ / C4: `max_deviation` with non-ValueRef (literal) args returns `None`
    /// — the cell stays at its compiled default (Value::Undef). Mirrors the
    /// defensive fall-through contract of the other 2-arg selectors.
    #[test]
    fn try_eval_geometry_query_max_deviation_literal_args_returns_none() {
        use reify_test_support::mocks::MockGeometryKernel;
        let kernel = MockGeometryKernel::new();
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();
        let functions: Vec<reify_ir::CompiledFunction> = Vec::new();
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();

        // literal args (non-ValueRef) — dispatch must return None
        let expr = topology_selector_call_literal_args("max_deviation");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_geometry_query(
            &expr,
            &named_steps,
            &values,
            &functions,
            &meta_map,
            &kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "max_deviation with literal (non-ValueRef) args must return None; \
             got {:?}",
            result
        );
    }

    /// Drift-pin: `MAX_DEVIATION_TESSELLATION_TOLERANCE_M` must equal
    /// `Engine::DEFAULT_TESSELLATION_TOLERANCE` (engine_build.rs:3165 = 0.0001).
    ///
    /// `Engine::DEFAULT_TESSELLATION_TOLERANCE` is a private associated const, so
    /// this test pins its numeric value. **If the engine default ever changes**,
    /// update `MAX_DEVIATION_TESSELLATION_TOLERANCE_M` in this file to match and
    /// also update the `const TOL` literals in the tests above (they mirror the
    /// same value).
    #[test]
    fn max_deviation_tessellation_tolerance_pins_engine_default_value() {
        assert_eq!(
            super::MAX_DEVIATION_TESSELLATION_TOLERANCE_M,
            0.0001_f64,
            "MAX_DEVIATION_TESSELLATION_TOLERANCE_M must equal \
             Engine::DEFAULT_TESSELLATION_TOLERANCE (engine_build.rs:3165); \
             update both if the engine default changes"
        );
    }

    /// Pins the finite/non-negative guard in `dispatch_scalar_query` (amend task
    /// 4479 — reviewer suggestion 3). Kernels that return NaN, ±Inf, or a negative
    /// deviation must produce `Some(Value::Undef)` + exactly one Warning rather
    /// than silently propagating a bogus `Scalar<LENGTH>` into downstream
    /// arithmetic.
    #[test]
    fn dispatch_scalar_query_non_finite_or_negative_emits_warning_and_undef() {
        use reify_test_support::mocks::MockGeometryKernel;
        let actual = reify_ir::GeometryHandleId(40);
        let nominal = reify_ir::GeometryHandleId(41);
        const TOL: f64 = 0.0001;

        for bad_value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e-4_f64] {
            let kernel = MockGeometryKernel::new().with_max_deviation_result(
                actual,
                nominal,
                TOL,
                reify_ir::Value::Real(bad_value),
            );
            let query = reify_ir::GeometryQuery::MaxDeviation {
                actual,
                nominal,
                tolerance: TOL,
            };
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            let result = super::dispatch_scalar_query(
                &kernel,
                query,
                reify_core::DimensionVector::LENGTH,
                "max_deviation",
                &mut diagnostics,
            );
            assert_eq!(
                result,
                Some(reify_ir::Value::Undef),
                "dispatch_scalar_query with bad_value={bad_value:?} must return Some(Undef)"
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "dispatch_scalar_query with bad_value={bad_value:?} must emit exactly \
                 one Warning; got: {:?}",
                diagnostics
            );
        }
    }

    /// β/step-12 — Invariant V at the kernel-reply boundary. A scalar query whose
    /// caller-supplied dimension is DIMENSIONLESS must collapse the finite,
    /// non-negative `Value::Real` reply to `Value::Real` (via the
    /// `from_real_scalar` chokepoint), NOT leak a
    /// `Value::Scalar { dimension.is_dimensionless() }`. A dimensioned (LENGTH)
    /// query still yields a `Value::Scalar`.
    #[test]
    fn dispatch_scalar_query_dimensionless_collapses_to_real() {
        use reify_test_support::mocks::MockGeometryKernel;
        let actual = reify_ir::GeometryHandleId(50);
        let nominal = reify_ir::GeometryHandleId(51);
        const TOL: f64 = 0.0001;

        let kernel = MockGeometryKernel::new().with_max_deviation_result(
            actual,
            nominal,
            TOL,
            reify_ir::Value::Real(2.5),
        );

        // DIMENSIONLESS caller dimension → result must be Value::Real(2.5),
        // never Value::Scalar { dimension.is_dimensionless() }.
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::MaxDeviation {
                actual,
                nominal,
                tolerance: TOL,
            },
            reify_core::DimensionVector::DIMENSIONLESS,
            "leak_guard",
            &mut diagnostics,
        );
        assert_eq!(
            result,
            Some(reify_ir::Value::Real(2.5)),
            "a DIMENSIONLESS dispatch_scalar_query must collapse to Value::Real, \
             not leak Value::Scalar{{DIMENSIONLESS}}"
        );
        assert!(
            diagnostics.is_empty(),
            "a finite, non-negative reply must emit no warning; got: {diagnostics:?}"
        );

        // Guard: a dimensioned (LENGTH) query still yields a Value::Scalar.
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::MaxDeviation {
                actual,
                nominal,
                tolerance: TOL,
            },
            reify_core::DimensionVector::LENGTH,
            "leak_guard",
            &mut diagnostics,
        );
        assert_eq!(
            result,
            Some(reify_ir::Value::Scalar {
                si_value: 2.5,
                dimension: reify_core::DimensionVector::LENGTH,
            }),
            "a dimensioned (LENGTH) query must still yield Value::Scalar{{LENGTH}}"
        );
        assert!(diagnostics.is_empty());
    }

    /// Amendment (task 4374/β, reviewer suggestion 2): the defensive `Scalar`
    /// reply arm of `dispatch_scalar_query` must validate finiteness /
    /// non-negativity *identically* to the `Real` arm. A `Scalar` reply carrying
    /// NaN / ±Inf / a negative magnitude is downgraded to `Some(Value::Undef)` +
    /// exactly one Warning (never silently wrapped); a finite, non-negative
    /// `Scalar` reply collapses through `from_real_scalar` like a `Real` reply.
    #[test]
    fn dispatch_scalar_query_scalar_reply_validates_finite_non_negative() {
        use reify_test_support::mocks::MockGeometryKernel;
        let actual = reify_ir::GeometryHandleId(60);
        let nominal = reify_ir::GeometryHandleId(61);
        const TOL: f64 = 0.0001;

        // Bad Scalar replies → Undef + exactly one Warning, just like Real.
        for bad_value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e-4_f64] {
            let kernel = MockGeometryKernel::new().with_max_deviation_result(
                actual,
                nominal,
                TOL,
                reify_ir::Value::Scalar {
                    si_value: bad_value,
                    dimension: reify_core::DimensionVector::LENGTH,
                },
            );
            let query = reify_ir::GeometryQuery::MaxDeviation {
                actual,
                nominal,
                tolerance: TOL,
            };
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            let result = super::dispatch_scalar_query(
                &kernel,
                query,
                reify_core::DimensionVector::LENGTH,
                "max_deviation",
                &mut diagnostics,
            );
            assert_eq!(
                result,
                Some(reify_ir::Value::Undef),
                "Scalar reply with bad_value={bad_value:?} must return Some(Undef)"
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "Scalar reply with bad_value={bad_value:?} must emit exactly one \
                 Warning; got: {diagnostics:?}"
            );
        }

        // A finite, non-negative Scalar reply collapses through the chokepoint:
        // a DIMENSIONLESS caller dimension yields Value::Real (Invariant V).
        let kernel = MockGeometryKernel::new().with_max_deviation_result(
            actual,
            nominal,
            TOL,
            reify_ir::Value::Scalar {
                si_value: 2.5,
                dimension: reify_core::DimensionVector::LENGTH,
            },
        );
        let query = reify_ir::GeometryQuery::MaxDeviation {
            actual,
            nominal,
            tolerance: TOL,
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            query,
            reify_core::DimensionVector::DIMENSIONLESS,
            "leak_guard",
            &mut diagnostics,
        );
        assert_eq!(
            result,
            Some(reify_ir::Value::Real(2.5)),
            "a finite, non-negative Scalar reply with a DIMENSIONLESS caller \
             dimension must collapse to Value::Real"
        );
        assert!(diagnostics.is_empty());
    }
}
