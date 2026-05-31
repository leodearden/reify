// Geometry operation compilation: evaluates CompiledGeometryOp into runtime GeometryOp.
//
// Free functions with no Engine coupling — they take values, functions, meta_map
// as plain arguments.

use std::collections::HashMap;

use reify_core::Diagnostic;
use reify_ir::{CompiledFunction, GeometryHandleId, ValueMap};

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
    named_steps: &HashMap<String, GeometryHandleId>,
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
                .copied()
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
                reify_compiler::ModifyKind::Fillet => Ok(reify_ir::GeometryOp::Fillet {
                    target: target_id,
                    radius: eval_arg("radius")?,
                }),
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
                    let angle = eval_arg("angle")?;
                    // plane is passed as an expression that evaluates to a value;
                    // at this level we don't have the geometry handle yet, so we
                    // use step_handles.last() as a placeholder for the plane reference.
                    // Filter INVALID so a preceding compile failure (sentinel) propagates
                    // as Err here rather than forwarding INVALID to the kernel.
                    let plane_id = step_handles
                        .last()
                        .copied()
                        .filter(|h| *h != GeometryHandleId::INVALID)
                        .ok_or_else(|| "no valid plane handle available for Draft".to_string())?;
                    Ok(reify_ir::GeometryOp::Draft {
                        target: target_id,
                        angle,
                        plane: plane_id,
                    })
                }
                reify_compiler::ModifyKind::Thicken => {
                    let offset = eval_arg("offset")?;
                    Ok(reify_ir::GeometryOp::Thicken {
                        target: target_id,
                        offset,
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
                reify_compiler::TransformKind::Translate => {
                    Ok(reify_ir::GeometryOp::Translate {
                        target: target_id,
                        dx: f64_arg("dx")?,
                        dy: f64_arg("dy")?,
                        dz: f64_arg("dz")?,
                    })
                }
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
                            format!("missing or non-finite argument '{}' for {}", name, kind)
                        })
                    };
                    let axis_origin = [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?];
                    let axis_dir = [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?];
                    let count_raw = f64_arg("count")?;
                    let count = validate_pattern_count(count_raw, "count", kind, diagnostics)?;
                    let raw_angle = eval_named_arg(
                        "angle",
                        kind,
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )
                    .ok_or_else(|| format!("missing required argument 'angle' for {}", kind))?;
                    // CAD convention: a bare numeric angle (no unit suffix) is
                    // interpreted as degrees and converted to radians.  Values
                    // that already carry an ANGLE dimension (from `deg`/`rad`
                    // suffixes in source) pass through unchanged.
                    let mut convert_bare_angle = |deg: f64| -> reify_ir::Value {
                        let rad = deg * std::f64::consts::PI / 180.0;
                        diagnostics.push(Diagnostic::warning(format!(
                            "circular_pattern: bare numeric angle `{}` interpreted as {}°; \
                             use `{}deg` or `{:.6}rad` for explicit units",
                            deg, deg, deg, rad
                        )));
                        reify_ir::Value::angle(rad)
                    };
                    let angle = match raw_angle {
                        reify_ir::Value::Real(v) => convert_bare_angle(v),
                        reify_ir::Value::Int(i) => convert_bare_angle(i as f64),
                        other => other,
                    };
                    Ok(reify_ir::GeometryOp::CircularPattern {
                        target: target_id,
                        axis_origin,
                        axis_dir,
                        count,
                        angle,
                    })
                }
                reify_compiler::PatternKind::Mirror => {
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
                            format!("missing or non-finite argument '{}' for {}", name, kind)
                        })
                    };
                    Ok(reify_ir::GeometryOp::Mirror {
                        target: target_id,
                        plane_origin: [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?],
                        plane_normal: [f64_arg("nx")?, f64_arg("ny")?, f64_arg("nz")?],
                    })
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
    named_steps: &HashMap<String, GeometryHandleId>,
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
        Some(h) => *h,
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
// v0.1 simplification: the Snapshot's per-body `world_transform` is **not**
// applied to the OCCT shape before the distance probe; geometry must be
// pre-positioned at the source-let level (e.g. `let a = translate(box(...),
// 30mm, 0mm, 0mm)`). FK-driven OCCT placement requires either a new
// `GeometryOp::ApplyTransform` op + handle bookkeeping or per-pair on-the-
// fly OCCT transforms — both expand scope beyond the PRD task-8 acceptance.
// This matches the existing `bounding_box`/`center_of_mass` v0.1 approach
// (also operates on world-frame body origins only, point-mass approximation).
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
    named_steps: &HashMap<String, GeometryHandleId>,
    values: &reify_ir::ValueMap,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // (1) Must be a FunctionCall — anything else is unsupported.
    let (function, args) = match &expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
        _ => return None,
    };

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

    // For the binary forms, args[1] / args[2] must also be ValueRefs
    // resolving to `Value::Int` body ids. Pulled out as a helper so the
    // unary `interferes` arm doesn't pay for it.
    let body_id_args = if expected_args == 3 {
        let a = resolve_int_value_ref(&args[1], values)?;
        let b = resolve_int_value_ref(&args[2], values)?;
        Some((a, b))
    } else {
        None
    };

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
        let solid_name = match body_map.get(&reify_ir::Value::String("solid".to_string())) {
            Some(reify_ir::Value::String(s)) => s,
            // Non-string `solid` (e.g. a stale `Value::Undef` from a body whose
            // source-let was a geometry call) is not resolvable here — skip the
            // body silently rather than collapsing the entire query to Undef.
            _ => continue,
        };
        if let Some(handle) = named_steps.get(solid_name) {
            id_to_handle.push((id, *handle));
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
                    match kernel_distance(kernel, handle_a, handle_b, diagnostics, &function.name) {
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
            match kernel_distance(kernel, handle_a, handle_b, diagnostics, &function.name) {
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
            match kernel_distance(kernel, handle_a, handle_b, diagnostics, &function.name) {
                Some(d) => Some(reify_ir::Value::length(d)),
                None => Some(reify_ir::Value::Undef),
            }
        }
    }
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

/// Resolve a `CompiledExprKind::ValueRef` arg to its `Value::Int` body id.
/// Returns `None` for any non-`ValueRef` shape, missing cell, or non-Int
/// payload — caller maps this to the "unsupported arg shape → fall through"
/// behaviour of `try_eval_kinematic_query`.
fn resolve_int_value_ref(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<i64> {
    let id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    match values.get(id) {
        Some(reify_ir::Value::Int(n)) => Some(*n),
        _ => None,
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
fn kernel_distance(
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
pub(crate) fn try_eval_topology_selector(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, GeometryHandleId>,
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
        _ => return None,
    };

    // (3) Per-helper arity check. Each new selector in task 3560 carries its
    // own arity contract; the legacy 2-arg trio (closest_point, is_on,
    // angle_between_surfaces) shares the arity-2 branch.
    let expected_arity = helper.expected_arity();
    if args.len() != expected_arity {
        return None;
    }

    match helper {
        TopologySelectorHelper::ClosestPoint | TopologySelectorHelper::IsOn => {
            // args[0]: point ValueRef → values map → Value::Point of three Length scalars.
            let point = resolve_point3_length_arg(&args[0], values)?;
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
                | TopologySelectorHelper::Perimeter => {
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
            let point = resolve_point3_length_arg(&args[1], values)?;
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
            let tolerance = resolve_length_scalar_arg(&args[2], values)?;
            let query = reify_ir::GeometryQuery::GeoEquiv { left, right, tolerance };
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
            let a = resolve_vec3_arg(&args[0], values)?;
            let b = resolve_vec3_arg(&args[1], values)?;
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
            let point = resolve_point3_length_arg(&args[1], values)?;
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
            let point = resolve_point3_length_arg(&args[1], values)?;
            dispatch_curvature(kernel, handle, point, &function.name, diagnostics)
        }
        TopologySelectorHelper::Length => {
            // `length(curve) -> Scalar<Length>` (task 3622, KGQ-ν).
            // arg[0]: edge sub-handle ValueRef → values → kernel_handle.
            // Falls through (None) when arg is not a hydrated Value::GeometryHandle
            // (PRD invariant #2).
            let (_, _, kernel_handle) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;
            dispatch_edge_length(kernel, kernel_handle, &function.name, diagnostics)
        }
        TopologySelectorHelper::Perimeter => {
            // `perimeter(surface) -> Scalar<Length>` (task 3622, KGQ-ν).
            // arg[0]: face sub-handle ValueRef → values → kernel_handle.
            // Falls through (None) when arg is not a hydrated Value::GeometryHandle
            // (PRD invariant #2).
            let (_, _, face_kh) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;
            dispatch_perimeter(kernel, face_kh, &function.name, diagnostics)
        }
        TopologySelectorHelper::Edges | TopologySelectorHelper::Faces => {
            // args[0]: geometry ValueRef → values map → full parent GeometryHandle.
            // The parent's realization_ref + upstream_values_hash are needed to
            // construct well-formed sub-handle values (PRD §4). Fall through when
            // the arg cell is not yet a hydrated Value::GeometryHandle (PRD invariant
            // #2: do not partially construct a sub-handle; cell retains its compiled
            // default, i.e. stays at Value::Undef).
            let (parent_rr, parent_hash, parent_kernel_handle) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;
            let sub_kind = match helper {
                TopologySelectorHelper::Edges => crate::topology_selectors::SubKind::Edge,
                TopologySelectorHelper::Faces => crate::topology_selectors::SubKind::Face,
                // Enumerate the complement explicitly (rather than `_`) so that
                // adding a new `TopologySelectorHelper` variant and grouping it
                // into the outer `Edges | Faces` or-pattern forces the compiler
                // to error here instead of silently funnelling into
                // `unreachable!()`.
                TopologySelectorHelper::ClosestPoint
                | TopologySelectorHelper::IsOn
                | TopologySelectorHelper::AngleBetweenSurfaces
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
                | TopologySelectorHelper::Perimeter => {
                    unreachable!("Edges/Faces outer match guarantees this")
                }
            };
            dispatch_extract_subshapes(
                kernel,
                parent_kernel_handle,
                sub_kind,
                &parent_rr,
                &parent_hash,
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::CenterOfMass => {
            // args[0]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            // args[1]: density ValueRef → values map → Real / dimensionless Scalar.
            // Uses resolve_density_arg (same as MomentOfInertia) so a dimensioned
            // density arg emits a Severity::Warning instead of silently resolving
            // to undefined — consistent diagnostic experience for both density-taking
            // queries.
            let density =
                resolve_density_arg(&args[1], values, &function.name, diagnostics)?;
            let query = reify_ir::GeometryQuery::CenterOfMass { handle, density };
            dispatch_point3_length_reply(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::MomentOfInertia => {
            // args[0]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            // args[1]: density ValueRef → values map → Real / dimensionless Scalar.
            // Uses resolve_density_arg (not resolve_real_scalar_arg) to emit a
            // Severity::Warning when the caller passes a dimensioned value
            // (e.g. kg/m³ literal) — the v0.3 grammar does not yet support
            // compound-unit density literals, so bare-numeric Real is required.
            let density =
                resolve_density_arg(&args[1], values, &function.name, diagnostics)?;
            let query = reify_ir::GeometryQuery::InertiaTensor { handle, density };
            dispatch_inertia_tensor(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::EdgesByLength => {
            // args[0]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            // args[1]: Range<Length> ValueRef/Literal → (lo_m, hi_m).
            let (lo, hi) =
                resolve_range_dim_arg(&args[1], values, reify_core::DimensionVector::LENGTH)?;
            dispatch_filtered_list(
                crate::topology_selectors::edges_by_length(kernel, handle, lo, hi),
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::FacesByArea => {
            // args[0]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            // args[1]: Range<Area> ValueRef/Literal → (lo_m2, hi_m2). `mm*mm`
            // canonicalises to AREA (LENGTH² == AREA per dimension algebra).
            let (lo, hi) =
                resolve_range_dim_arg(&args[1], values, reify_core::DimensionVector::AREA)?;
            dispatch_filtered_list(
                crate::topology_selectors::faces_by_area(kernel, handle, lo, hi),
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::FacesByNormal => {
            // args[0]: parent geometry ValueRef → values map → full Value::GeometryHandle.
            // Must resolve from `values` (not `named_steps`) so we get the parent's
            // realization_ref + upstream_values_hash for sub-handle construction (PRD §4).
            // Falls through to None when the arg cell is not a hydrated Value::GeometryHandle
            // (PRD invariant #2: never partially construct a sub-handle).
            let (parent_rr, parent_hash, parent_kernel_handle) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;
            // args[1]: Vec3 direction ValueRef → values map → [f64; 3].
            let dir = resolve_vec3_arg(&args[1], values)?;
            // args[2]: angular tolerance ValueRef → values map → ANGLE Scalar
            // (SI radians — `topology_selectors::faces_by_normal` expects rad).
            let tol = resolve_angle_scalar_arg(&args[2], values)?;
            let filter_result =
                crate::topology_selectors::faces_by_normal(kernel, parent_kernel_handle, dir, tol);
            dispatch_filtered_subhandles(
                kernel,
                parent_kernel_handle,
                crate::topology_selectors::SubKind::Face,
                &parent_rr,
                &parent_hash,
                filter_result,
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::EdgesParallelTo => {
            // args[0]: parent geometry ValueRef → values map → full Value::GeometryHandle.
            // Must resolve from `values` (not `named_steps`) — same rationale as
            // FacesByNormal (PRD §4 invariant #2).
            let (parent_rr, parent_hash, parent_kernel_handle) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;
            // args[1]: Vec3 axis ValueRef → values map → [f64; 3].
            let axis = resolve_vec3_arg(&args[1], values)?;
            // args[2]: angular tolerance ValueRef → values map → ANGLE Scalar
            // (SI radians — `topology_selectors::edges_parallel_to` expects rad).
            let tol = resolve_angle_scalar_arg(&args[2], values)?;
            let filter_result = crate::topology_selectors::edges_parallel_to(
                kernel,
                parent_kernel_handle,
                axis,
                tol,
            );
            dispatch_filtered_subhandles(
                kernel,
                parent_kernel_handle,
                crate::topology_selectors::SubKind::Edge,
                &parent_rr,
                &parent_hash,
                filter_result,
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::EdgesAtHeight => {
            // args[0]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            // args[1]: z plane ValueRef → values map → LENGTH Scalar (SI metres).
            let z_m = resolve_length_scalar_arg(&args[1], values)?;
            // args[2]: tolerance ValueRef → values map → LENGTH Scalar (SI metres).
            let tol_m = resolve_length_scalar_arg(&args[2], values)?;
            dispatch_filtered_list(
                crate::topology_selectors::edges_at_height(kernel, handle, z_m, tol_m),
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

/// Map a filtered-selector helper `Result<Vec<GeometryHandleId>, QueryError>`
/// onto the dispatcher's `Option<Value>` contract: `Ok` → `Value::List` of
/// Int handle ids; `Err` → Warning diagnostic + `Value::Undef`. Shared by all
/// `topology_selectors::*` delegating arms (task 3560).
///
/// NOTE: arms using this helper emit `Value::List([Value::Int])` (raw kernel-handle ids),
/// NOT the canonical `List<Geometry>` = `List<Value::GeometryHandle>` contract required by
/// PRD §4. They must be migrated to `dispatch_filtered_subhandles` in KGQ-θ (edges_by_length,
/// faces_by_area, edges_at_height) and subsequent tasks; do not mistake the Int-list output
/// for the intended final shape.
fn dispatch_filtered_list(
    result: Result<Vec<GeometryHandleId>, reify_ir::QueryError>,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match result {
        Ok(handles) => Some(handle_list_value(handles)),
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
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

    let canonical_index_map: std::collections::HashMap<GeometryHandleId, usize> =
        canonical.iter().enumerate().map(|(i, &id)| (id, i)).collect();
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

/// Wrap a `Vec<GeometryHandleId>` as `Value::List(Vec<Value::Int>)` whose
/// elements are the raw u64 handle ids cast to `i64`. Shared by all
/// list-returning topology selectors (task 3560).
fn handle_list_value(handles: Vec<GeometryHandleId>) -> reify_ir::Value {
    reify_ir::Value::List(
        handles
            .into_iter()
            .map(|h| reify_ir::Value::Int(h.0 as i64))
            .collect(),
    )
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
            | TopologySelectorHelper::Curvature => 2,
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

/// Resolve a `CompiledExprKind::ValueRef` arg to a `Value::Point` of three
/// Length-dimensioned scalars and return their SI-metres components.
/// Returns `None` for any non-`ValueRef` shape, missing cell, non-Point
/// payload, wrong length, or non-scalar component — caller maps to the
/// "unsupported arg shape → fall through" behaviour.
fn resolve_point3_length_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<[f64; 3]> {
    let id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    let value = values.get(id)?;
    let components = match value {
        reify_ir::Value::Point(items) => items,
        _ => return None,
    };
    if components.len() != 3 {
        return None;
    }
    let mut out = [0.0_f64; 3];
    for (i, comp) in components.iter().enumerate() {
        match comp {
            // The cell type is fixed at `Type::Point<Length>` by the
            // compile-time wiring in `expr.rs`, so a well-formed
            // `Value::Scalar` component MUST carry `DimensionVector::LENGTH`.
            // A wrong-dimensioned Scalar slipping through here would silently
            // be reinterpreted as metres at the kernel boundary — debug-assert
            // to surface the violation in tests; in release we still fall
            // through to `None` rather than feeding the kernel garbage.
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
}

/// Resolve the `density` argument of `center_of_mass` and `moment_of_inertia`
/// to a raw `f64`, emitting a `Severity::Warning` when the caller passes a
/// dimensioned or non-numeric `Value`.
///
/// Delegates the accept-logic (Real / dimensionless Scalar) to
/// [`resolve_real_scalar_arg`] and only owns the diagnostic-on-wrong-type
/// behavior, keeping the type-acceptance contract in a single place:
///
/// | arg expr / resolved value        | return       | diagnostic pushed?           |
/// |----------------------------------|--------------|------------------------------|
/// | non-`ValueRef` expr              | `None`       | no (silent fall-through)     |
/// | `ValueRef` → missing cell        | `None`       | no                           |
/// | `ValueRef` → `Value::Real(v)`    | `Some(v)`    | no                           |
/// | `ValueRef` → dimensionless       | `Some(si_v)` | no                           |
/// |   `Value::Scalar`                |              |                              |
/// | `ValueRef` → dimensioned Scalar  | `None`       | yes — `Severity::Warning`    |
/// |   or any non-numeric `Value`     |              | naming `helper_name` +       |
/// |                                  |              | "density" + "dimensionless"  |
fn resolve_density_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    // Delegate accept-logic to the shared resolver (Real / dimensionless Scalar).
    if let Some(v) = resolve_real_scalar_arg(expr, values) {
        return Some(v);
    }
    // resolve_real_scalar_arg returned None. Emit a diagnostic only when expr
    // is a ValueRef pointing to a *present* value of the wrong type —
    // a non-ValueRef shape or a missing cell falls through silently
    // (the established "unsupported arg shape → silent" contract).
    let id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    if let Some(other) = values.get(id) {
        diagnostics.push(Diagnostic::warning(format!(
            "{helper_name}: density argument must be a bare numeric Real or \
             dimensionless value in v0.3 (compound-unit literals are not yet \
             supported as a density arg); got {other:?} — treating as undefined"
        )));
    }
    None
}

/// Shared accept-logic for a density-style argument: resolves a
/// `CompiledExprKind::ValueRef` to a raw `f64` from a `Value::Real` or a
/// dimensionless `Value::Scalar`. Called internally by [`resolve_density_arg`]
/// — not invoked directly from dispatch arms. Returns `None` (no diagnostic)
/// for any non-`ValueRef` shape, a missing cell, or a dimensioned Scalar;
/// callers that need a `Severity::Warning` for the wrong-type case should use
/// [`resolve_density_arg`] instead.
fn resolve_real_scalar_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<f64> {
    let id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    match values.get(id)? {
        reify_ir::Value::Real(v) => Some(*v),
        reify_ir::Value::Scalar {
            si_value,
            dimension,
        } if *dimension == reify_core::DimensionVector::DIMENSIONLESS => Some(*si_value),
        _ => None,
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

/// Resolve a 3-component vector arg to its `[f64; 3]` SI components. Accepts
/// `Literal(Value::Vector(items))` (rare — inline vector literals) or the
/// common `ValueRef → Value::Vector` (let-bound `let dir = vec3(0,0,1)`).
/// Each component must be a `Value::Real` or a dimensionless `Value::Scalar`
/// (per `vec3_component_si`); the vector must have exactly three components.
/// Returns `None` for any other shape — caller maps to the "unsupported arg
/// shape → fall through" behaviour. Inline `vec3(...)` FunctionCall args fall
/// through (the dispatcher has no recursive eval context); test fixtures
/// let-bind the direction so it lands in `values` as a `Value::Vector`.
fn resolve_vec3_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<[f64; 3]> {
    let from_vector_value = |v: &reify_ir::Value| -> Option<[f64; 3]> {
        match v {
            reify_ir::Value::Vector(items) if items.len() == 3 => Some([
                vec3_component_si(&items[0])?,
                vec3_component_si(&items[1])?,
                vec3_component_si(&items[2])?,
            ]),
            _ => None,
        }
    };
    match &expr.kind {
        reify_ir::CompiledExprKind::Literal(v) => from_vector_value(v),
        reify_ir::CompiledExprKind::ValueRef(id) => from_vector_value(values.get(id)?),
        _ => None,
    }
}

/// Resolve an ANGLE-dimensioned scalar arg to its SI value (radians).
/// Accepts `Literal(Value::Scalar { dimension: ANGLE, .. })` or the common
/// `ValueRef → ANGLE Scalar` (let-bound `let tol = 1deg`). Returns `None`
/// for any other shape (wrong dimension, non-Scalar) — caller maps to the
/// "unsupported arg shape → fall through" behaviour. Mirrors
/// `resolve_scalar_bound_expr` but pins the ANGLE dimension for the angular-
/// tolerance args of `faces_by_normal` / `edges_parallel_to`.
fn resolve_angle_scalar_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<f64> {
    resolve_scalar_bound_expr(expr, values, reify_core::DimensionVector::ANGLE)
}

/// Resolve a LENGTH-dimensioned scalar arg to its SI value (metres).
/// Accepts `Literal(Value::Scalar { dimension: LENGTH, .. })` or the common
/// `ValueRef → LENGTH Scalar` (let-bound `let z = 0mm`). Returns `None` for
/// any other shape (wrong dimension, non-Scalar) — caller maps to the
/// "unsupported arg shape → fall through" behaviour. Mirrors
/// `resolve_angle_scalar_arg` but pins the LENGTH dimension for the
/// z-plane / tolerance args of `edges_at_height`.
fn resolve_length_scalar_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<f64> {
    resolve_scalar_bound_expr(expr, values, reify_core::DimensionVector::LENGTH)
}

/// Read a `Value::Scalar` whose `dimension` is `expected_dim` and return its
/// SI value. `None` for any other shape (wrong dimension, non-Scalar).
fn scalar_si_with_dim(value: &reify_ir::Value, expected_dim: reify_core::DimensionVector) -> Option<f64> {
    match value {
        reify_ir::Value::Scalar {
            si_value,
            dimension,
        } if *dimension == expected_dim => Some(*si_value),
        _ => None,
    }
}

/// Resolve a single range-bound `CompiledExpr` (the `lower`/`upper` slot of a
/// `RangeConstructor`) to its SI value, accepting a `Literal(Value::Scalar)`
/// or a `ValueRef → Value::Scalar`, both dimensioned `expected_dim`.
fn resolve_scalar_bound_expr(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    expected_dim: reify_core::DimensionVector,
) -> Option<f64> {
    match &expr.kind {
        reify_ir::CompiledExprKind::Literal(v) => scalar_si_with_dim(v, expected_dim),
        reify_ir::CompiledExprKind::ValueRef(id) => {
            scalar_si_with_dim(values.get(id)?, expected_dim)
        }
        _ => None,
    }
}

/// Resolve a `Range<Quantity>` arg to its `(lower_si, upper_si)` SI bounds,
/// both dimensioned `expected_dim`. Accepts three arg shapes:
///
///  (a) `Literal(Value::Range { lower: Some, upper: Some, .. })`,
///  (b) `ValueRef → Value::Range { lower: Some, upper: Some, .. }` (the
///      common let-bound `let r = 0mm..50mm` form — the regular eval path
///      evaluates the `RangeConstructor` RHS into the cell as a
///      `Value::Range`),
///  (c) `RangeConstructor { lower: Some, upper: Some, .. }` written inline,
///      whose bound exprs each resolve via Literal/ValueRef.
///
/// Both bounds must be present (a half-open range falls through to `None` —
/// the v0.1 filtered selectors require a closed `[lo, hi]` window) and
/// dimensioned `expected_dim`. Returns `None` for any other shape — caller
/// maps to the "unsupported arg shape → fall through" behaviour.
fn resolve_range_dim_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    expected_dim: reify_core::DimensionVector,
) -> Option<(f64, f64)> {
    // Range-from-Value: shared by the Literal and ValueRef arms.
    let from_range_value = |v: &reify_ir::Value| -> Option<(f64, f64)> {
        match v {
            reify_ir::Value::Range {
                lower: Some(lo),
                upper: Some(hi),
                ..
            } => Some((
                scalar_si_with_dim(lo, expected_dim)?,
                scalar_si_with_dim(hi, expected_dim)?,
            )),
            _ => None,
        }
    };
    match &expr.kind {
        reify_ir::CompiledExprKind::Literal(v) => from_range_value(v),
        reify_ir::CompiledExprKind::ValueRef(id) => from_range_value(values.get(id)?),
        reify_ir::CompiledExprKind::RangeConstructor {
            lower: Some(lo),
            upper: Some(hi),
            ..
        } => Some((
            resolve_scalar_bound_expr(lo, values, expected_dim)?,
            resolve_scalar_bound_expr(hi, values, expected_dim)?,
        )),
        _ => None,
    }
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
    named_steps: &HashMap<String, GeometryHandleId>,
) -> Option<GeometryHandleId> {
    let cell_id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    named_steps.get(&cell_id.member).copied()
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
) -> Option<(reify_core::identity::RealizationNodeId, [u8; 32], GeometryHandleId)> {
    let cell_id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    match values.get(cell_id)? {
        reify_ir::Value::GeometryHandle {
            realization_ref,
            upstream_values_hash,
            kernel_handle,
        } => Some((realization_ref.clone(), *upstream_values_hash, *kernel_handle)),
        _ => None,
    }
}

/// Issue a query whose kernel reply is the canonical JSON-Point3
/// (`{"x":_,"y":_,"z":_}`) wire format and unwrap to a
/// `Value::Point(vec![length, length, length])`. Returns
/// `Some(Value::Undef)` (with a Warning diagnostic) on a kernel error or a
/// malformed reply. Shared by `closest_point` (`ClosestPointOnShape`) and
/// `center_of_mass` (`CenterOfMass`) — both return the identical JSON-Point3
/// encoding per the `GeometryQuery` doc, so a single decode path serves
/// both.
fn dispatch_point3_length_reply(
    kernel: &mut dyn reify_ir::GeometryKernel,
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
        return Some(parse_curvature_matrix_reply(&value, helper_name, diagnostics));
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
        other => return malformed(diagnostics, format!("expected Value::List, got {:?}", other)),
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
    named_steps: &HashMap<String, GeometryHandleId>,
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

    // (3) Extract the base name — must be a string literal.
    let name = resolve_string_literal_arg(base)?;

    // (4) Extract the face/edge label — must be args[0] string literal.
    let label = match args.first() {
        Some(a) => resolve_string_literal_arg(a)?,
        None => return None,
    };

    // (5) Look up the base name in named_steps → GeometryHandleId.
    let handle = match named_steps.get(name) {
        Some(&h) => h,
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
        user_label: Some(label.to_string()),
        role_and_index: cap_kind_translation(label),
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

/// Helper: extract a `&str` from a `CompiledExprKind::Literal(Value::String(s))`
/// expression.  Returns `None` for any other expression kind or value payload.
///
/// Used by `try_eval_ad_hoc_selector` to extract the base name and the label
/// from an `AdHocSelector`'s `base` and `args[0]` respectively.
fn resolve_string_literal_arg(expr: &reify_ir::CompiledExpr) -> Option<&str> {
    match &expr.kind {
        reify_ir::CompiledExprKind::Literal(reify_ir::Value::String(s)) => Some(s.as_str()),
        _ => None,
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
        Ok(value) => {
            match crate::topology_selectors::parse_xyz_value(&value, "Centroid") {
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
            }
        }
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
                    if let reify_ir::Value::Scalar { si_value, dimension } = c {
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

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::{CompiledGeometryOp, GeomRef, PatternKind, SweepKind, TransformKind};
    use reify_ir::GeometryHandleId;

    /// Helper: build a CompiledExpr literal from a constant f64.
    fn literal_f64(v: f64) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), reify_core::Type::Real)
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

    /// Bare `Value::Real` components in a `Value::Point` are NOT a valid
    /// production shape for a `Type::Point<Length>` cell.  The function MUST
    /// return `None` so the caller falls through to "unsupported arg shape".
    /// Returning `Some([...])` would silently reinterpret the raw floats as
    /// SI metres at the kernel boundary — exactly the hazard this tightening
    /// closes.  All production mocks use `Value::length(...)` components (i.e.
    /// `Value::Scalar { dimension: LENGTH, .. }`).
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
        assert_eq!(
            super::resolve_point3_length_arg(&expr, &values),
            None,
            "bare Value::Real components must produce None — production cells \
             carry Type::Point<Length> so components must be \
             Value::Scalar {{ dimension: LENGTH, .. }}; a bare Real slipping \
             through would be silently reinterpreted as metres at the kernel \
             boundary, hence the function must return None (caller falls \
             through to 'unsupported arg shape')"
        );
    }

    /// Tests for `resolve_density_arg`: diagnostic behavior for wrong-typed density
    /// arguments to `moment_of_inertia`.
    ///
    /// Contract under test:
    ///   (a) ValueRef → LENGTH-dimensioned Scalar → None + exactly 1 Warning whose
    ///       message names "density" and "real" or "dimensionless" (case-insensitive).
    ///   (b) ValueRef → non-numeric Value (e.g. `Value::Bool`) → None + 1 Warning.
    ///   (c) ValueRef → `Value::Real(7850.0)` → Some(7850.0), empty diagnostics.
    ///       ValueRef → dimensionless `Value::Scalar` → Some(si_value), empty diagnostics.
    ///   (d) Non-ValueRef expr (Literal) → None, empty diagnostics (silent fall-through,
    ///       matching the established "unsupported arg shape → silent fall-through"
    ///       contract that every sibling resolver follows).
    ///
    /// Modelled on `resolve_point3_length_arg_bare_real_components_return_none` above
    /// (line 3254) — build a `value_ref` expr + a `ValueMap`, call the helper directly,
    /// assert the return value and diagnostic side-effect, compiler-independently.
    #[test]
    fn resolve_density_arg_diagnostics() {
        fn make_value_ref(cell: reify_core::ValueCellId) -> reify_ir::CompiledExpr {
            reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::Real)
        }

        // (a) ValueRef → LENGTH Scalar → None + 1 Warning
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho");
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
            assert_eq!(result, None, "(a) LENGTH Scalar must return None");
            assert_eq!(
                diags.len(),
                1,
                "(a) LENGTH Scalar must push exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(a) diagnostic must be Warning severity"
            );
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("density"),
                "(a) warning must name 'density', got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("real") || msg.contains("dimensionless"),
                "(a) warning must mention 'real' or 'dimensionless', got: {:?}",
                diags[0].message
            );
        }

        // (b) ValueRef → Value::Bool(true) → None + 1 Warning
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho2");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, reify_ir::Value::Bool(true));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(b) Bool must return None");
            assert_eq!(
                diags.len(),
                1,
                "(b) Bool must push exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(b) diagnostic must be Warning severity"
            );
        }

        // (c-i) ValueRef → Value::Real(7850.0) → Some(7850.0), empty diagnostics
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho3");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, reify_ir::Value::Real(7850.0));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(
                result,
                Some(7850.0),
                "(c-i) Value::Real(7850.0) must return Some(7850.0)"
            );
            assert!(
                diags.is_empty(),
                "(c-i) Value::Real must produce no diagnostics, got: {:?}",
                diags
            );
        }

        // (c-ii) ValueRef → dimensionless Scalar → Some(si_value), empty diagnostics
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho4");
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
                result,
                Some(7850.0),
                "(c-ii) dimensionless Scalar must return Some(si_value)"
            );
            assert!(
                diags.is_empty(),
                "(c-ii) dimensionless Scalar must produce no diagnostics, got: {:?}",
                diags
            );
        }

        // (d) Non-ValueRef (literal_f64) → None, empty diagnostics (silent fall-through)
        {
            let expr = literal_f64(7850.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(
                result, None,
                "(d) Literal expr must return None (silent fall-through)"
            );
            assert!(
                diags.is_empty(),
                "(d) Literal expr must produce no diagnostics, got: {:?}",
                diags
            );
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
        let angle_int_expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Int(360),
            reify_core::Type::Int,
        );

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
            reify_ir::CompiledExpr::literal(reify_ir::Value::Undef, reify_core::Type::Real);
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

        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
        named_steps.insert("body".into(), handle_a);
        named_steps.insert("hole".into(), handle_b);

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
        let named_steps: HashMap<String, GeometryHandleId> = HashMap::new(); // empty — "unknown" not present

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

    // ── try_eval_conformance_query unit tests (task 2320) ────────────────────
    //
    // These tests pin the contract of `try_eval_conformance_query`, the
    // kernel-aware eval-time dispatch surface for the `is_watertight`,
    // `is_manifold`, `is_orientable` stdlib helpers. Architecture rationale
    // is captured in the task 2320 plan; the function lives in this module
    // (rather than `eval_expr`) because the build pipeline owns both the
    // kernel and the per-realization name → handle map (`named_steps`).

    /// Build a `CompiledExpr` for `is_watertight(<entity>.<member>)`.
    fn conformance_call(
        helper_name: &str,
        entity: &str,
        member: &str,
    ) -> reify_ir::CompiledExpr {
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

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
        let arg = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(1.0),
            reify_core::Type::Real,
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
    fn try_eval_conformance_query_non_helper_name_returns_none_no_kernel_call() {
        let handle_id = reify_ir::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

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
        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

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

    // ── step-5 (task 3616): edges/faces dispatch RED unit tests ─────────────
    //
    // These tests are RED because the current arm returns Value::List(Value::Int)
    // via handle_list_value instead of Value::List(Value::GeometryHandle).

    /// `edges` dispatch returns `Value::List` of three `Value::GeometryHandle`
    /// elements when the mock kernel returns [GHId(2),GHId(3),GHId(4)] and the
    /// `values` map carries the parent `Value::GeometryHandle`. Each element
    /// must carry the parent's `realization_ref`, and the three
    /// `upstream_values_hash` fields must be pairwise distinct (PRD §4 iii).
    /// RED: current arm returns `Value::Int` via `handle_list_value`.
    #[test]
    fn edges_dispatch_returns_geometry_handle_list() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("BoxEdges", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new().with_extracted_edges(
            parent_handle,
            vec![GeometryHandleId(2), GeometryHandleId(3), GeometryHandleId(4)],
        );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), parent_handle);

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

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "expected Some(Value::List(..)), got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(list.len(), 3, "expected 3 edge sub-handles");

        let expected_ids = [GeometryHandleId(2), GeometryHandleId(3), GeometryHandleId(4)];
        let mut hashes: Vec<[u8; 32]> = Vec::new();
        for (i, (elem, expected_id)) in list.iter().zip(&expected_ids).enumerate() {
            match elem {
                reify_ir::Value::GeometryHandle { realization_ref, upstream_values_hash, kernel_handle } => {
                    assert_eq!(
                        realization_ref.entity, parent_rr.entity,
                        "elem[{i}] realization_ref.entity must match parent"
                    );
                    assert_eq!(
                        realization_ref.index, parent_rr.index,
                        "elem[{i}] realization_ref.index must match parent"
                    );
                    assert_eq!(
                        kernel_handle, expected_id,
                        "elem[{i}] kernel_handle must be {expected_id:?}"
                    );
                    hashes.push(*upstream_values_hash);
                }
                other => panic!("elem[{i}] is not Value::GeometryHandle: {:?}", other),
            }
        }
        // All three upstream_values_hashes must be pairwise distinct (PRD §4 iii).
        assert_ne!(hashes[0], hashes[1], "edge 0 and 1 hashes must differ");
        assert_ne!(hashes[1], hashes[2], "edge 1 and 2 hashes must differ");
        assert_ne!(hashes[0], hashes[2], "edge 0 and 2 hashes must differ");
    }

    /// When the `values` map does not carry a `Value::GeometryHandle` for the
    /// arg cell, the `edges` arm must fall through to `None` (cell stays Undef)
    /// rather than partially constructing a sub-handle (PRD invariant #2).
    /// RED: current arm dispatches via `named_steps` regardless of `values` and
    /// returns `Some(Value::List(Value::Int))`.
    #[test]
    fn edges_dispatch_falls_through_to_none_when_parent_not_hydrated() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::Type;

        let parent_handle = GeometryHandleId(1);
        let mut kernel = MockGeometryKernel::new().with_extracted_edges(
            parent_handle,
            vec![GeometryHandleId(2), GeometryHandleId(3)],
        );

        // named_steps has the handle so the kernel could serve the call …
        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), parent_handle);

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
        let arg_a = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(1.0),
            reify_core::Type::Real,
        );
        let arg_b = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(2.0),
            reify_core::Type::Real,
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), body_handle);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), body_handle);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("face_a".to_string(), face_a);
        named_steps.insert("face_b".to_string(), face_b);

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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), reify_ir::GeometryHandleId(7));

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("face_a".to_string(), face_a);
        named_steps.insert("face_b".to_string(), face_b);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("face_a".to_string(), face_a);
        named_steps.insert("face_b".to_string(), face_b);

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
        HashMap<String, reify_ir::GeometryHandleId>,
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
        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("face_a".to_string(), face_a);
        named_steps.insert("face_b".to_string(), face_b);
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
        super::try_eval_topology_selector(&expr, &named_steps, &values, &mut kernel, &mut diagnostics);
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), body_handle);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), body_handle);

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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
            reify_core::Type::vec3(reify_core::Type::Real),
            "b",
            reify_core::Type::vec3(reify_core::Type::Real),
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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
            reify_core::Type::vec3(reify_core::Type::Real),
            "b",
            reify_core::Type::vec3(reify_core::Type::Real),
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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
            reify_core::Type::vec3(reify_core::Type::Real),
            "b",
            reify_core::Type::vec3(reify_core::Type::Real),
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
        // angle(<literal_real>, <literal_real>) — scalar Real literals (not
        // Value::Vector), so resolve_vec3_arg returns None for both args and
        // the dispatcher falls through to None.  Note: resolve_vec3_arg DOES
        // accept Literal(Value::Vector) (see line ~2490), so a literal *vec3*
        // would NOT fall through — it would resolve and compute an angle.
        // This test pins the non-Vec3 scalar literal case only.  See
        // `try_eval_topology_selector_angle_literal_vec3_args_resolves_and_returns_angle`
        // for the literal-Vec3 case.
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        // accepts CompiledExprKind::Literal(Value::Vector) directly (line ~2490),
        // so literal vec3 args DO resolve and produce an angle, unlike the
        // scalar-literal case above.  Pins the actually-distinct contract for
        // literal-typed Vec3 args.
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        let values = reify_ir::ValueMap::new(); // empty — args come from literals

        // Build angle(Literal(vec3(1,0,0)), Literal(vec3(0,1,0))).
        let arg_a = reify_ir::CompiledExpr::literal(
            vec3_value(1.0, 0.0, 0.0),
            reify_core::Type::vec3(reify_core::Type::Real),
        );
        let arg_b = reify_ir::CompiledExpr::literal(
            vec3_value(0.0, 1.0, 0.0),
            reify_core::Type::vec3(reify_core::Type::Real),
        );
        let mut ch =
            reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
            reify_core::Type::vec3(reify_core::Type::Real),
            "b",
            reify_core::Type::vec3(reify_core::Type::Real),
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
            let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
                reify_core::Type::vec3(reify_core::Type::Real),
                "b",
                reify_core::Type::vec3(reify_core::Type::Real),
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        // args[0] = solid → resolved via named_steps by member name "solid"
        named_steps.insert("solid".to_string(), body_handle);

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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
    fn try_eval_topology_selector_contains_non_bool_kernel_reply_emits_warning_and_returns_undef()
    {
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("solid".to_string(), body_handle);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("solid".to_string(), body_handle);

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
        let route = super::gate_query_capability(
            &query,
            reify_ir::ReprKind::BRep,
            "distance",
            &mut diags,
        );
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
        let route = super::gate_query_capability(
            &query,
            reify_ir::ReprKind::Mesh,
            "distance",
            &mut diags,
        );
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
        let route = super::gate_query_capability(
            &query,
            reify_ir::ReprKind::Mesh,
            "curvature",
            &mut diags,
        );
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
        let route = super::gate_query_capability(
            &query,
            reify_ir::ReprKind::Voxel,
            "distance",
            &mut diags,
        );
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
        let route = super::gate_query_capability(
            &query,
            reify_ir::ReprKind::Sdf,
            "volume",
            &mut diags,
        );
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
            rotated[0].abs() < 1e-12 && rotated[1].abs() < 1e-12 && (rotated[2] + 1.0).abs() < 1e-12,
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
        assert!((w - sqrt2_inv).abs() < 1e-12, "+X axis: w should be 1/√2; got {w}");
        assert!(x.abs() < 1e-12, "+X axis: x should be 0; got {x}");
        assert!((y - sqrt2_inv).abs() < 1e-12, "+X axis: y should be 1/√2; got {y}");
        assert!(z.abs() < 1e-12, "+X axis: z should be 0; got {z}");
        // Round-trip: q applied to (0,0,1) should give (1,0,0).
        let rotated = quat_rotate(w, x, y, z, 0.0, 0.0, 1.0);
        assert!(
            (rotated[0] - 1.0).abs() < 1e-12 && rotated[1].abs() < 1e-12 && rotated[2].abs() < 1e-12,
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
        assert!((w - sqrt2_inv).abs() < 1e-12, "+Y axis: w should be 1/√2; got {w}");
        assert!((x + sqrt2_inv).abs() < 1e-12, "+Y axis: x should be -1/√2; got {x}");
        assert!(y.abs() < 1e-12, "+Y axis: y should be 0; got {y}");
        assert!(z.abs() < 1e-12, "+Y axis: z should be 0; got {z}");
        // Round-trip: q applied to (0,0,1) should give (0,1,0).
        let rotated = quat_rotate(w, x, y, z, 0.0, 0.0, 1.0);
        assert!(
            rotated[0].abs() < 1e-12 && (rotated[1] - 1.0).abs() < 1e-12 && rotated[2].abs() < 1e-12,
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
        let centroid_json =
            reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":0.01}"#.to_string());
        let normal_json =
            reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
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

        let Some(reify_ir::Value::Frame { ref origin, ref basis }) = result else {
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
            reify_ir::Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 },
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
        let centroid_json =
            reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":0.005}"#.to_string());
        let tangent_json =
            reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
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

        let Some(reify_ir::Value::Frame { ref origin, ref basis }) = result else {
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
            reify_ir::Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 },
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
            ("top",         Some((Role::Cap(CapKind::Top), 0))),
            ("bottom",      Some((Role::Cap(CapKind::Bottom), 0))),
            ("start",       Some((Role::Cap(CapKind::Start), 0))),
            ("end",         Some((Role::Cap(CapKind::End), 0))),
            ("side",        Some((Role::Side, 0))),
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
        let arg_a = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(1.0),
            reify_core::Type::Real,
        );
        let arg_b = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(2.0),
            reify_core::Type::Real,
        );
        let arg_c = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(3.0),
            reify_core::Type::Real,
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        // args[0] = left geometry → resolved via named_steps by member "left"
        named_steps.insert("left".to_string(), left_handle);
        // args[1] = right geometry → resolved via named_steps by member "right"
        named_steps.insert("right".to_string(), right_handle);

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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("left".to_string(), left_handle);
        named_steps.insert("right".to_string(), right_handle);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("left".to_string(), left_handle);
        named_steps.insert("right".to_string(), right_handle);

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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        // args[0] = surface → resolved via named_steps by member name "surface"
        named_steps.insert("surface".to_string(), face_handle);

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
            reify_core::Type::vec3(reify_core::Type::Real),
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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
    /// check.  Locks the split-arg fall-through path.
    #[test]
    fn try_eval_topology_selector_normal_dimensionless_point_falls_through_to_none() {
        use reify_test_support::mocks::{CountingMockKernel, MockGeometryKernel};
        let face_handle = reify_ir::GeometryHandleId(55);

        // Wrap in a counting mock so we can assert zero kernel queries.
        let inner = MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        // args[0] = surface → present in named_steps, so resolve_geometry_handle_arg
        // returns Some(face_handle).
        named_steps.insert("surface".to_string(), face_handle);

        let mut values = reify_ir::ValueMap::new();
        // args[1] = point → bare Value::Real components, NOT Value::Scalar with
        // DimensionVector::LENGTH.  resolve_point3_length_arg must return None for
        // this shape (the `_ => return None` arm on the component match).
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
            reify_core::Type::vec3(reify_core::Type::Real),
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
            "normal(surface, dimensionless_point) must return None (silent fall-through); \
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
        assert!(
            diagnostics.is_empty(),
            "dimensionless-point fall-through must emit zero diagnostics; \
             got: {:?}",
            diagnostics
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("surface".to_string(), face_handle);

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
            reify_core::Type::vec3(reify_core::Type::Real),
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("surface".to_string(), face_handle);

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
            reify_core::Type::vec3(reify_core::Type::Real),
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{DimensionVector, Type, ValueCellId};

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Directional", 0);
        let parent_hash: [u8; 32] = [0xAA; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(
                parent_handle,
                vec![GeometryHandleId(2), GeometryHandleId(3), GeometryHandleId(4)],
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
        named_steps.insert("b".to_string(), GeometryHandleId(99));

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
            Type::vec3(Type::Real),
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

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "faces_by_normal(..) must yield Some(Value::List(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "exactly 1 face matches +z within 1° (index 1); got {} elements; diags: {:?}",
            list.len(),
            diagnostics
        );

        // Expected hash: canonical index 1 (GHId(3) is at position 1 in the list).
        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Face,
            1,
        );

        match &list[0] {
            reify_ir::Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            } => {
                assert_eq!(
                    realization_ref, &parent_rr,
                    "realization_ref must equal parent (full struct)"
                );
                assert_eq!(
                    *kernel_handle,
                    GeometryHandleId(3),
                    "retained face must be GHId(3)"
                );
                assert_eq!(
                    upstream_values_hash, &expected_hash,
                    "upstream_values_hash must be compose_sub_handle_hash(parent_hash, Face, 1) \
                     — canonical index 1, NOT filtered position 0"
                );
            }
            other => panic!(
                "faces_by_normal result[0] must be Value::GeometryHandle, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "happy-path faces_by_normal must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `faces_by_normal` falls through to `None` when the parent arg is not a
    /// hydrated `Value::GeometryHandle` in `values` (PRD §4 invariant #2).
    #[test]
    fn faces_by_normal_dispatch_falls_through_when_parent_not_hydrated() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::{DimensionVector, Type, ValueCellId};

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
            Type::vec3(Type::Real),
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{DimensionVector, Type, ValueCellId};

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Directional", 0);
        let parent_hash: [u8; 32] = [0xBB; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(
                parent_handle,
                vec![GeometryHandleId(2), GeometryHandleId(3), GeometryHandleId(4)],
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
        named_steps.insert("b".to_string(), GeometryHandleId(99));

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
            Type::vec3(Type::Real),
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

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "edges_parallel_to(..) must yield Some(Value::List(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "exactly 1 edge is (anti-)parallel to +z (index 2, tangent −z); got {}; diags: {:?}",
            list.len(),
            diagnostics
        );

        // Expected hash: canonical index 2 (GHId(4) is at position 2 in the list).
        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Edge,
            2,
        );

        match &list[0] {
            reify_ir::Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            } => {
                assert_eq!(
                    realization_ref, &parent_rr,
                    "realization_ref must equal parent (full struct)"
                );
                assert_eq!(
                    *kernel_handle,
                    GeometryHandleId(4),
                    "retained edge must be GHId(4)"
                );
                assert_eq!(
                    upstream_values_hash, &expected_hash,
                    "upstream_values_hash must be compose_sub_handle_hash(parent_hash, Edge, 2) \
                     — canonical index 2, NOT filtered position 0"
                );
            }
            other => panic!(
                "edges_parallel_to result[0] must be Value::GeometryHandle, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "happy-path edges_parallel_to must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    // --- dispatch_filtered_subhandles defensive-branch tests ---

    /// Branch (a): filter_result is Err → dispatch emits a Warning and returns Value::Undef.
    #[test]
    fn dispatch_filtered_subhandles_filter_error_yields_undef_and_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;

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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;

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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;

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
        assert_eq!(diagnostics.len(), 1, "must emit one warning for the absent id");
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "diagnostic must be Warning severity, got {:?}",
            diagnostics[0]
        );
        assert!(
            diagnostics[0].message.contains("absent from canonical list"),
            "warning must mention 'absent from canonical list'; got: {}",
            diagnostics[0].message
        );
    }

    // ── step-1 (task 3619): adjacent_faces dispatch RED unit tests ───────────
    //
    // These tests are RED because the current arm returns Value::List(Value::Int)
    // via dispatch_filtered_list instead of Value::List(Value::GeometryHandle).

    /// `adjacent_faces` dispatch returns `Value::List` of one
    /// `Value::GeometryHandle` when the mock kernel returns the adjacent face
    /// at index 0. The element must carry the parent's `realization_ref` and
    /// an `upstream_values_hash` equal to
    /// `compose_sub_handle_hash(parent_hash, SubKind::Face, 0)`.
    #[test]
    fn adjacent_faces_dispatch_returns_geometry_handle_list() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

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
        named_steps.insert("b".to_string(), parent_handle);

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
        assert_eq!(list.len(), 1, "expected 1 adjacent face sub-handle; diags: {:?}", diagnostics);

        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Face,
            0,
        );
        match &list[0] {
            reify_ir::Value::GeometryHandle { realization_ref, upstream_values_hash, kernel_handle } => {
                assert_eq!(realization_ref.entity, parent_rr.entity, "realization_ref.entity must match parent");
                assert_eq!(realization_ref.index, parent_rr.index, "realization_ref.index must match parent");
                assert_eq!(*kernel_handle, GeometryHandleId(1), "kernel_handle must be GHId(1)");
                assert_eq!(*upstream_values_hash, expected_hash, "upstream_values_hash must be compose_sub_handle_hash(parent_hash, Face, 0)");
            }
            other => panic!("elem[0] is not Value::GeometryHandle: {:?}", other),
        }
    }

    /// When args[1]'s cell is absent from `values`, the `adjacent_faces` arm
    /// must fall through to `None` (PRD invariant #2: never partial-construct).
    #[test]
    fn adjacent_faces_dispatch_falls_through_when_face_arg_not_hydrated() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent_handle, vec![GeometryHandleId(1)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), parent_handle);

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

    // ── step-3 (task 3619): shared_edges dispatch RED unit tests ─────────────
    //
    // These tests are RED because the current arm returns Value::List(Value::Int)
    // via handle_list_value (inside dispatch_shared_edges) instead of
    // Value::List(Value::GeometryHandle).

    /// `shared_edges` dispatch returns `Value::List` of one
    /// `Value::GeometryHandle` (kernel_handle GHId(4)) when the mock kernel
    /// stages two faces (GHId(2), GHId(3)) sharing one edge (GHId(4)).
    /// The element must carry the parent solid's `realization_ref` and an
    /// `upstream_values_hash` equal to
    /// `compose_sub_handle_hash(parent_hash, SubKind::Edge, 0)`.
    #[test]
    fn shared_edges_dispatch_returns_geometry_handle_list() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

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
        named_steps.insert("fa".to_string(), face_a_handle);
        named_steps.insert("fb".to_string(), face_b_handle);

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
        assert_eq!(list.len(), 1, "expected 1 shared edge sub-handle; diags: {:?}", diagnostics);

        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Edge,
            0,
        );
        match &list[0] {
            reify_ir::Value::GeometryHandle { realization_ref, upstream_values_hash, kernel_handle } => {
                assert_eq!(realization_ref.entity, parent_rr.entity, "realization_ref.entity must match parent solid");
                assert_eq!(realization_ref.index, parent_rr.index, "realization_ref.index must match parent solid");
                assert_eq!(*kernel_handle, edge_handle, "kernel_handle must be the edge GHId(4)");
                assert_eq!(*upstream_values_hash, expected_hash, "upstream_values_hash must be compose_sub_handle_hash(parent_hash, Edge, 0)");
            }
            other => panic!("elem[0] is not Value::GeometryHandle: {:?}", other),
        }
    }

    /// When the parent solid is not hydrated in `values`, the `shared_edges`
    /// arm must fall through to `None` (PRD invariant #2: never partial-construct).
    #[test]
    fn shared_edges_dispatch_falls_through_when_parent_not_hydrated() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

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
        named_steps.insert("fa".to_string(), face_a_handle);
        named_steps.insert("fb".to_string(), face_b_handle);

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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

        let parent_handle = GeometryHandleId(1);
        let face_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // Stage extract_faces so adjacent_to_face can find the face index (0),
        // but omit the AdjacentFaces query result → kernel.query(...) returns Err
        // → adjacent_to_face propagates Err → filter_result = Err in
        // dispatch_filtered_subhandles → Warning + Value::Undef.
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent_handle, vec![face_handle]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), parent_handle);

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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

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
        named_steps.insert("fa".to_string(), face_a_handle);
        named_steps.insert("fb".to_string(), face_b_handle);

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
        let mut kernel = MockGeometryKernel::new()
            .with_surface_curvature_at_result(face_handle, [0.005, 0.0], reify_ir::Value::List(vec![row0, row1]));

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("face".to_string(), face_handle);

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
            reify_core::Type::Real, // placeholder result type — unused on dispatch path
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
        let mut kernel = MockGeometryKernel::new()
            .with_curve_curvature_at_result(edge_handle, [0.01, 0.0, 0.0], reify_ir::Value::Real(kappa));

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("edge".to_string(), edge_handle);

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
            reify_core::Type::Real,
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

        let mut named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
        named_steps.insert("face".to_string(), face_handle);

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
            reify_core::Type::Real,
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

        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        let arg = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(1.0),
            reify_core::Type::Real,
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
            result_type: reify_core::Type::Real,
            content_hash,
        }
    }

    /// `length(edge_sub_handle)` with a staged `Value::Real(0.02)` EdgeLength
    /// result must yield `Some(Value::length(0.02))` and zero diagnostics.
    ///
    /// PRIMARY RED assertion — pre-impl `length` hits the `_ => return None` arm.
    #[test]
    fn try_eval_topology_selector_length_edge_subhandle_returns_scalar_length() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

        let edge_kh = reify_ir::GeometryHandleId(10);
        let parent_rr = RealizationNodeId::new("LengthTest", 0);
        let parent_hash: [u8; 32] = [0x42; 32];
        let mut kernel = MockGeometryKernel::new()
            .with_edge_length_result(edge_kh, reify_ir::Value::Real(0.02));

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
        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

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
        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

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
        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

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
        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

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
        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

        let edge_kh = reify_ir::GeometryHandleId(30);
        let parent_rr = RealizationNodeId::new("LengthScalarTest", 0);
        let parent_hash: [u8; 32] = [0x60; 32];
        // Stage a Scalar{LENGTH} reply instead of a plain Real.
        let scalar_reply = reify_ir::Value::Scalar {
            si_value: 0.03,
            dimension: reify_core::DimensionVector::LENGTH,
        };
        let mut kernel = MockGeometryKernel::new()
            .with_edge_length_result(edge_kh, scalar_reply);

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
        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

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
        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

        let face_kh = reify_ir::GeometryHandleId(34);
        let parent_rr = RealizationNodeId::new("PerimEmptyTest", 0);
        let parent_hash: [u8; 32] = [0x62; 32];
        // Stage an empty edge list.
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(face_kh, vec![]);

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
        let named_steps: HashMap<String, reify_ir::GeometryHandleId> = HashMap::new();
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
            reify_ir::Value::Transform { rotation, translation } => {
                assert_eq!(
                    *rotation,
                    reify_ir::Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 },
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
                    ref other => panic!(
                        "identity translation must be a Vector; got {:?}",
                        other
                    ),
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
        let expr = reify_ir::CompiledExpr::literal(
            input_frame,
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
            diagnostics.is_empty(),
            "Frame lowering must not push diagnostics; got: {:?}",
            diagnostics
        );

        match result {
            reify_ir::Value::Transform { rotation, translation } => {
                // Convention: rotation == Frame.basis (exact copy, no normalization)
                assert_eq!(
                    *rotation,
                    reify_ir::Value::Orientation { w: s, x: 0.0, y: 0.0, z: s },
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
                    ref other => panic!(
                        "lowered translation must be a Vector; got {:?}",
                        other
                    ),
                }
            }
            other => panic!("expected Value::Transform after Frame lowering; got {:?}", other),
        }
    }

    /// `eval_sub_pose(Some(&non_pose_expr), ...)` must return `Value::Undef` and
    /// push exactly one `Diagnostic::error`.
    ///
    /// T4 owns pose type-validation (T2 deferred it). Pins the step-7/8 contract.
    #[test]
    fn eval_sub_pose_non_pose_value_returns_undef_with_diagnostic() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(5.0),
            reify_core::Type::Real,
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
        reify_ir::Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 }
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
        assert!(result.is_undef(), "non-Point origin must return Undef; got {:?}", result);
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
        assert!(result.is_undef(), "2-component origin must return Undef; got {:?}", result);
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
        assert!(result.is_undef(), "NaN coordinate must return Undef; got {:?}", result);
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
            reify_core::Type::Real, // type doesn't matter; the value is Undef
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
}
