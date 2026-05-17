// Geometry operation compilation: evaluates CompiledGeometryOp into runtime GeometryOp.
//
// Free functions with no Engine coupling — they take values, functions, meta_map
// as plain arguments.

use std::collections::HashMap;

use reify_types::{CompiledFunction, Diagnostic, GeometryHandleId, ValueMap};

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
    query: &reify_types::GeometryQuery,
    produced_repr: reify_types::ReprKind,
    query_display_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> CapabilityRoute {
    use reify_types::{DiagnosticCode, QueryCapability, ReprKind};

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
    args: &[(String, reify_types::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::Value> {
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
    args: &[(String, reify_types::CompiledExpr)],
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
    args: &[(String, reify_types::CompiledExpr)],
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
) -> Result<reify_types::GeometryOp, String> {
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
            let mut eval_arg = |name: &str| -> Result<reify_types::Value, String> {
                eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
                    .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
            };

            match kind {
                PrimitiveKind::Box => Ok(reify_types::GeometryOp::Box {
                    width: eval_arg("width")?,
                    height: eval_arg("height")?,
                    depth: eval_arg("depth")?,
                }),
                PrimitiveKind::Cylinder => Ok(reify_types::GeometryOp::Cylinder {
                    radius: eval_arg("radius")?,
                    height: eval_arg("height")?,
                }),
                PrimitiveKind::Sphere => Ok(reify_types::GeometryOp::Sphere {
                    radius: eval_arg("radius")?,
                }),
                PrimitiveKind::Tube => Ok(reify_types::GeometryOp::Tube {
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
                BooleanOp::Union => Ok(reify_types::GeometryOp::Union {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Difference => Ok(reify_types::GeometryOp::Difference {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Intersection => Ok(reify_types::GeometryOp::Intersection {
                    left: left_id,
                    right: right_id,
                }),
            }
        }
        CompiledGeometryOp::Modify { kind, target, args } => {
            let target_id = resolve_geom_ref(target, step_handles)?;
            let mut eval_arg = |name: &str| -> Result<reify_types::Value, String> {
                eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
                    .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
            };
            match kind {
                reify_compiler::ModifyKind::Fillet => Ok(reify_types::GeometryOp::Fillet {
                    target: target_id,
                    radius: eval_arg("radius")?,
                }),
                reify_compiler::ModifyKind::Chamfer => Ok(reify_types::GeometryOp::Chamfer {
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
                    Ok(reify_types::GeometryOp::Shell {
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
                    Ok(reify_types::GeometryOp::Draft {
                        target: target_id,
                        angle,
                        plane: plane_id,
                    })
                }
                reify_compiler::ModifyKind::Thicken => {
                    let offset = eval_arg("offset")?;
                    Ok(reify_types::GeometryOp::Thicken {
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
                    Ok(reify_types::GeometryOp::Translate {
                        target: target_id,
                        dx: f64_arg("dx")?,
                        dy: f64_arg("dy")?,
                        dz: f64_arg("dz")?,
                    })
                }
                reify_compiler::TransformKind::Rotate => Ok(reify_types::GeometryOp::Rotate {
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
                    Ok(reify_types::GeometryOp::Scale {
                        target: target_id,
                        factor,
                    })
                }
                reify_compiler::TransformKind::RotateAround => {
                    Ok(reify_types::GeometryOp::RotateAround {
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
                    Ok(reify_types::GeometryOp::LinearPattern {
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
                    let mut convert_bare_angle = |deg: f64| -> reify_types::Value {
                        let rad = deg * std::f64::consts::PI / 180.0;
                        diagnostics.push(Diagnostic::warning(format!(
                            "circular_pattern: bare numeric angle `{}` interpreted as {}°; \
                             use `{}deg` or `{:.6}rad` for explicit units",
                            deg, deg, deg, rad
                        )));
                        reify_types::Value::angle(rad)
                    };
                    let angle = match raw_angle {
                        reify_types::Value::Real(v) => convert_bare_angle(v),
                        reify_types::Value::Int(i) => convert_bare_angle(i as f64),
                        other => other,
                    };
                    Ok(reify_types::GeometryOp::CircularPattern {
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
                    Ok(reify_types::GeometryOp::Mirror {
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
                    Ok(reify_types::GeometryOp::LinearPattern2D {
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
                    Ok(reify_types::GeometryOp::ArbitraryPattern {
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
                    Ok(reify_types::GeometryOp::Loft {
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
                    Ok(reify_types::GeometryOp::Extrude {
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
                    Ok(reify_types::GeometryOp::Revolve {
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
                    Ok(reify_types::GeometryOp::Sweep {
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
                    Ok(reify_types::GeometryOp::ExtrudeSymmetric {
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
                    Ok(reify_types::GeometryOp::SweepGuided {
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
                    Ok(reify_types::GeometryOp::LoftGuided {
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
                    Ok(reify_types::GeometryOp::Pipe {
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
                    Ok(reify_types::GeometryOp::LineSegment {
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
                    Ok(reify_types::GeometryOp::Arc {
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
                    Ok(reify_types::GeometryOp::Helix {
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
                    Ok(reify_types::GeometryOp::InterpCurve { points })
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
                    Ok(reify_types::GeometryOp::BezierCurve { control_points })
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
                    Ok(reify_types::GeometryOp::NurbsCurve {
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
    expr: &reify_types::CompiledExpr,
    template_trait_bounds: &[String],
    named_steps: &HashMap<String, GeometryHandleId>,
    kernel: &dyn reify_types::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::Value> {
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
        reify_types::CompiledExprKind::FunctionCall { function, args } => (function, args),
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
        return Some(reify_types::Value::Bool(true));
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
        reify_types::CompiledExprKind::ValueRef(id) => id,
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
        "is_watertight" => reify_types::GeometryQuery::IsWatertight(handle),
        "is_manifold" => reify_types::GeometryQuery::IsManifold(handle),
        "is_orientable" => reify_types::GeometryQuery::IsOrientable(handle),
        // Unreachable — the earlier match already filtered to these three names.
        _ => return None,
    };

    match kernel.query(&query) {
        Ok(reify_types::Value::Bool(b)) => Some(reify_types::Value::Bool(b)),
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}({}) kernel returned non-Bool value {:?}; treating as undefined",
                function.name, cell_id.member, other
            )));
            Some(reify_types::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}({}) kernel query failed: {}",
                function.name, cell_id.member, err
            )));
            Some(reify_types::Value::Undef)
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
    expr: &reify_types::CompiledExpr,
    named_steps: &HashMap<String, GeometryHandleId>,
    values: &reify_types::ValueMap,
    kernel: &dyn reify_types::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::Value> {
    // (1) Must be a FunctionCall — anything else is unsupported.
    let (function, args) = match &expr.kind {
        reify_types::CompiledExprKind::FunctionCall { function, args } => (function, args),
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
        reify_types::CompiledExprKind::ValueRef(id) => id,
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
        None => return Some(reify_types::Value::Undef),
    };

    // (6) Build (id → handle) by resolving each body's `solid` String against
    // `named_steps`. Bodies whose `solid` doesn't appear in `named_steps`
    // (e.g. a snapshot of a mechanism whose source let-name was never
    // realised because the structure has no realization for it) are
    // skipped — the helper still works for the realised subset.
    let mut id_to_handle: Vec<(i64, GeometryHandleId)> = Vec::with_capacity(bodies.len());
    for body in bodies {
        let body_map = match body {
            reify_types::Value::Map(m) => m,
            _ => return Some(reify_types::Value::Undef),
        };
        let id = match body_map.get(&reify_types::Value::String("id".to_string())) {
            Some(reify_types::Value::Int(n)) => *n,
            _ => return Some(reify_types::Value::Undef),
        };
        let solid_name = match body_map.get(&reify_types::Value::String("solid".to_string())) {
            Some(reify_types::Value::String(s)) => s,
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
                        None => return Some(reify_types::Value::Undef),
                    }
                }
            }
            Some(reify_types::Value::List(pairs))
        }
        KinematicHelper::InterferesWith => {
            let (id_a, id_b) = body_id_args.expect("3-arg form populated body_id_args");
            // Self-pair: per the PRD acceptance, "a single body's interference
            // with itself is not reported". Returning Bool(false) here is a
            // defensive fallback — typical user-code uses distinct ids.
            if id_a == id_b {
                return Some(reify_types::Value::Bool(false));
            }
            let handle_a = match handle_for_id(&id_to_handle, id_a) {
                Some(h) => h,
                None => return Some(reify_types::Value::Undef),
            };
            let handle_b = match handle_for_id(&id_to_handle, id_b) {
                Some(h) => h,
                None => return Some(reify_types::Value::Undef),
            };
            match kernel_distance(kernel, handle_a, handle_b, diagnostics, &function.name) {
                Some(d) => Some(reify_types::Value::Bool(d <= 0.0)),
                None => Some(reify_types::Value::Undef),
            }
        }
        KinematicHelper::MinClearance => {
            let (id_a, id_b) = body_id_args.expect("3-arg form populated body_id_args");
            // Self-pair clearance is undefined — surfacing 0.0 would lie about
            // a degenerate input. Returning Undef pushes the user toward
            // distinct ids; pinned by the smoke-test self-pair arm.
            if id_a == id_b {
                return Some(reify_types::Value::Undef);
            }
            let handle_a = match handle_for_id(&id_to_handle, id_a) {
                Some(h) => h,
                None => return Some(reify_types::Value::Undef),
            };
            let handle_b = match handle_for_id(&id_to_handle, id_b) {
                Some(h) => h,
                None => return Some(reify_types::Value::Undef),
            };
            match kernel_distance(kernel, handle_a, handle_b, diagnostics, &function.name) {
                Some(d) => Some(reify_types::Value::length(d)),
                None => Some(reify_types::Value::Undef),
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
    expr: &reify_types::CompiledExpr,
    values: &reify_types::ValueMap,
) -> Option<i64> {
    let id = match &expr.kind {
        reify_types::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    match values.get(id) {
        Some(reify_types::Value::Int(n)) => Some(*n),
        _ => None,
    }
}

/// Extract the `bodies` list from a Snapshot Map, validating
/// `kind="snapshot"`. Mirrors `reify_stdlib::snapshot::snapshot_bodies` —
/// duplicated here because the stdlib helper is module-private.
fn extract_snapshot_bodies(snap: &reify_types::Value) -> Option<Vec<reify_types::Value>> {
    let map = match snap {
        reify_types::Value::Map(m) => m,
        _ => return None,
    };
    if map.get(&reify_types::Value::String("kind".to_string()))
        != Some(&reify_types::Value::String("snapshot".to_string()))
    {
        return None;
    }
    match map.get(&reify_types::Value::String("bodies".to_string())) {
        Some(reify_types::Value::List(b)) => Some(b.clone()),
        _ => None,
    }
}

fn handle_for_id(pairs: &[(i64, GeometryHandleId)], id: i64) -> Option<GeometryHandleId> {
    pairs.iter().find(|(i, _)| *i == id).map(|(_, h)| *h)
}

/// Build the `{ "a": Int, "b": Int }` pair Map returned by `interferes`.
/// Alphabetical key order matches `BTreeMap` iteration so that List
/// equality used in the smoke tests is stable across iterations.
fn make_pair_map(id_a: i64, id_b: i64) -> reify_types::Value {
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        reify_types::Value::String("a".to_string()),
        reify_types::Value::Int(id_a),
    );
    m.insert(
        reify_types::Value::String("b".to_string()),
        reify_types::Value::Int(id_b),
    );
    reify_types::Value::Map(m)
}

/// Issue a `GeometryQuery::Distance` against the kernel and reduce to a raw
/// SI metres f64. Returns `None` (and emits a Warning diagnostic) on kernel
/// error or when the kernel returns a non-numeric `Value` — caller maps
/// `None` to a defensive `Value::Undef`.
fn kernel_distance(
    kernel: &dyn reify_types::GeometryKernel,
    from: GeometryHandleId,
    to: GeometryHandleId,
    diagnostics: &mut Vec<Diagnostic>,
    helper_name: &str,
) -> Option<f64> {
    let query = reify_types::GeometryQuery::Distance { from, to };
    match kernel.query(&query) {
        Ok(reify_types::Value::Real(d)) => Some(d),
        // Some kernels (e.g. test-support `MockGeometryKernel::with_distance_result`)
        // store the value as a length-dimensioned `Scalar` instead of a raw
        // `Real`. Read the SI value either way so the dispatch stays kernel-
        // agnostic; the dimension itself is unused (the helpers' return-side
        // dimension is fixed by the helper, not the kernel reply).
        Ok(reify_types::Value::Scalar { si_value, .. }) => Some(si_value),
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
//                          `angle_between_surfaces`.
//   `Some(Value::Undef)`   on a kernel error or a malformed kernel reply
//                          (defensive downgrade with a Warning diagnostic).
//   `None`                 when the expression is not a recognised
//                          topology-selector helper, or the arg shape is
//                          unsupported. Callers fall through to the cell's
//                          compiled default.
pub(crate) fn try_eval_topology_selector(
    expr: &reify_types::CompiledExpr,
    named_steps: &HashMap<String, GeometryHandleId>,
    values: &reify_types::ValueMap,
    kernel: &dyn reify_types::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::Value> {
    // (1) Must be a FunctionCall — anything else is unsupported.
    let (function, args) = match &expr.kind {
        reify_types::CompiledExprKind::FunctionCall { function, args } => (function, args),
        _ => return None,
    };

    // (2) Must be one of the three recognised helper names.
    let helper = match function.name.as_str() {
        "closest_point" => TopologySelectorHelper::ClosestPoint,
        "is_on" => TopologySelectorHelper::IsOn,
        "angle_between_surfaces" => TopologySelectorHelper::AngleBetweenSurfaces,
        _ => return None,
    };

    // (3) All v0.1 helpers are 2-arg; non-2-arg call sites fall through.
    if args.len() != 2 {
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
                    let query = reify_types::GeometryQuery::ClosestPointOnShape {
                        handle,
                        px: point[0],
                        py: point[1],
                        pz: point[2],
                    };
                    dispatch_closest_point(kernel, &query, &function.name, diagnostics)
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
                    let query = reify_types::GeometryQuery::PointOnShape {
                        handle,
                        px: point[0],
                        py: point[1],
                        pz: point[2],
                        tolerance: reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
                    };
                    dispatch_point_on_shape(kernel, &query, &function.name, diagnostics)
                }
                TopologySelectorHelper::AngleBetweenSurfaces => {
                    unreachable!("angle_between_surfaces is handled in the outer match")
                }
            }
        }
        TopologySelectorHelper::AngleBetweenSurfaces => {
            // Both args: geometry ValueRefs → named_steps map → GeometryHandleId.
            let face_a = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let face_b = resolve_geometry_handle_arg(&args[1], named_steps)?;
            let query = reify_types::GeometryQuery::SurfaceAngle { face_a, face_b };
            dispatch_surface_angle(kernel, &query, &function.name, diagnostics)
        }
    }
}

#[derive(Clone, Copy)]
enum TopologySelectorHelper {
    ClosestPoint,
    IsOn,
    AngleBetweenSurfaces,
}

/// Resolve a `CompiledExprKind::ValueRef` arg to a `Value::Point` of three
/// Length-dimensioned scalars and return their SI-metres components.
/// Returns `None` for any non-`ValueRef` shape, missing cell, non-Point
/// payload, wrong length, or non-scalar component — caller maps to the
/// "unsupported arg shape → fall through" behaviour.
fn resolve_point3_length_arg(
    expr: &reify_types::CompiledExpr,
    values: &reify_types::ValueMap,
) -> Option<[f64; 3]> {
    let id = match &expr.kind {
        reify_types::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    let value = values.get(id)?;
    let components = match value {
        reify_types::Value::Point(items) => items,
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
            reify_types::Value::Scalar {
                si_value,
                dimension,
            } => {
                debug_assert!(
                    *dimension == reify_types::DimensionVector::LENGTH,
                    "resolve_point3_length_arg: expected LENGTH-dimensioned Scalar, \
                     got dimension {:?} (si_value={}); cell type is Point<Length> per \
                     compile-time wiring in expr.rs",
                    dimension,
                    si_value
                );
                if *dimension != reify_types::DimensionVector::LENGTH {
                    return None;
                }
                out[i] = *si_value;
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Resolve a `CompiledExprKind::ValueRef` geometry-arg to a `GeometryHandleId`
/// via `named_steps`. Returns `None` for any non-`ValueRef` shape or missing
/// `named_steps` entry — caller maps to the "unsupported arg shape → fall
/// through" behaviour.
fn resolve_geometry_handle_arg(
    expr: &reify_types::CompiledExpr,
    named_steps: &HashMap<String, GeometryHandleId>,
) -> Option<GeometryHandleId> {
    let cell_id = match &expr.kind {
        reify_types::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    named_steps.get(&cell_id.member).copied()
}

/// Issue a `ClosestPointOnShape` query and unwrap to a
/// `Value::Point(vec![length, length, length])`. Returns
/// `Some(Value::Undef)` (with a Warning diagnostic) on a kernel error or a
/// malformed JSON-Point3 reply.
fn dispatch_closest_point(
    kernel: &dyn reify_types::GeometryKernel,
    query: &reify_types::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::Value> {
    match kernel.query(query) {
        Ok(value) => match crate::topology_selectors::parse_xyz_value(&value, helper_name) {
            Ok([x, y, z]) => Some(reify_types::Value::Point(vec![
                reify_types::Value::length(x),
                reify_types::Value::length(y),
                reify_types::Value::length(z),
            ])),
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{} kernel reply parse failed: {}",
                    helper_name, err
                )));
                Some(reify_types::Value::Undef)
            }
        },
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_types::Value::Undef)
        }
    }
}

/// Issue a `PointOnShape` query and unwrap to a `Value::Bool(_)`. Returns
/// `Some(Value::Undef)` (with a Warning diagnostic) on a kernel error or a
/// non-Bool reply.
fn dispatch_point_on_shape(
    kernel: &dyn reify_types::GeometryKernel,
    query: &reify_types::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::Value> {
    match kernel.query(query) {
        Ok(reify_types::Value::Bool(b)) => Some(reify_types::Value::Bool(b)),
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel returned non-Bool value {:?}; treating as undefined",
                helper_name, other
            )));
            Some(reify_types::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_types::Value::Undef)
        }
    }
}

/// Issue a `SurfaceAngle` query and unwrap to a `Value::angle(rad)`. Returns
/// `Some(Value::Undef)` (with a Warning diagnostic) on a kernel error or a
/// non-numeric reply.
fn dispatch_surface_angle(
    kernel: &dyn reify_types::GeometryKernel,
    query: &reify_types::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::Value> {
    match kernel.query(query) {
        Ok(reify_types::Value::Real(rad)) => Some(reify_types::Value::angle(rad)),
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
        Ok(reify_types::Value::Scalar {
            si_value,
            dimension,
        }) => {
            debug_assert!(
                dimension == reify_types::DimensionVector::ANGLE
                    || dimension == reify_types::DimensionVector::DIMENSIONLESS,
                "dispatch_surface_angle: expected ANGLE- or DIMENSIONLESS-dimensioned Scalar, \
                 got dimension {:?} (si_value={}); kernel cell type is Type::angle() per \
                 compile-time wiring",
                dimension,
                si_value
            );
            if dimension != reify_types::DimensionVector::ANGLE
                && dimension != reify_types::DimensionVector::DIMENSIONLESS
            {
                diagnostics.push(Diagnostic::warning(format!(
                    "{} kernel returned wrong-dimensioned Scalar \
                     (dimension={}, si_value={}); treating as undefined",
                    helper_name, dimension, si_value
                )));
                return Some(reify_types::Value::Undef);
            }
            Some(reify_types::Value::angle(si_value))
        }
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel returned non-numeric value {:?}; treating as undefined",
                helper_name, other
            )));
            Some(reify_types::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_types::Value::Undef)
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
    fn from_selector_kind(k: &reify_types::SelectorKind) -> Option<Self> {
        match k {
            reify_types::SelectorKind::Face => Some(FrameSubShapeKind::Face),
            reify_types::SelectorKind::Edge => Some(FrameSubShapeKind::Edge),
            reify_types::SelectorKind::Point => None,
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
    expr: &reify_types::CompiledExpr,
    named_steps: &HashMap<String, GeometryHandleId>,
    kernel: &mut dyn reify_types::GeometryKernel,
    table: &reify_types::TopologyAttributeTable,
    selector_span: reify_types::SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::Value> {
    // (1) Must be an AdHocSelector — anything else is not applicable.
    let (base, selector_kind, args) = match &expr.kind {
        reify_types::CompiledExprKind::AdHocSelector {
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
                return Some(reify_types::Value::Undef);
            }
        },
        FrameSubShapeKind::Edge => match kernel.extract_edges(handle) {
            Ok(edges) => edges,
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "@edge(\"{label}\"): extract_edges({handle:?}) failed: {err}"
                )));
                return Some(reify_types::Value::Undef);
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
        _ => Some(reify_types::Value::Undef),
    }
}

/// Helper: extract a `&str` from a `CompiledExprKind::Literal(Value::String(s))`
/// expression.  Returns `None` for any other expression kind or value payload.
///
/// Used by `try_eval_ad_hoc_selector` to extract the base name and the label
/// from an `AdHocSelector`'s `base` and `args[0]` respectively.
fn resolve_string_literal_arg(expr: &reify_types::CompiledExpr) -> Option<&str> {
    match &expr.kind {
        reify_types::CompiledExprKind::Literal(reify_types::Value::String(s)) => Some(s.as_str()),
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
pub fn cap_kind_translation(label: &str) -> Option<(reify_types::Role, u32)> {
    use reify_types::{CapKind, Role};
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
    kernel: &mut dyn reify_types::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::Value> {
    // ── Origin via Centroid ───────────────────────────────────────────────
    // `GeometryQuery::Centroid` is unified — works for faces, edges, AND solids.
    let origin = match kernel.query(&reify_types::GeometryQuery::Centroid(target_id)) {
        Ok(value) => {
            match crate::topology_selectors::parse_xyz_value(&value, "Centroid") {
                Ok([x, y, z]) => reify_types::Value::Point(vec![
                    reify_types::Value::length(x),
                    reify_types::Value::length(y),
                    reify_types::Value::length(z),
                ]),
                Err(err) => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "@face/@edge centroid parse failed: {err}; cell left as Undef"
                    )));
                    return Some(reify_types::Value::Undef);
                }
            }
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "@face/@edge centroid query failed: {err}; cell left as Undef"
            )));
            return Some(reify_types::Value::Undef);
        }
    };

    // ── Basis via FaceNormal (face) or EdgeTangent (edge) ─────────────────
    // Exhaustive over Face/Edge — Point is excluded by the FrameSubShapeKind
    // type, so no unreachable!() arm is needed here.
    let basis_query = match sub_shape_kind {
        FrameSubShapeKind::Face => reify_types::GeometryQuery::FaceNormal(target_id),
        FrameSubShapeKind::Edge => reify_types::GeometryQuery::EdgeTangent(target_id),
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
                    reify_types::Value::Orientation {
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
            reify_types::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        }
    };

    Some(reify_types::Value::Frame {
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
fn quaternion_from_z_to_axis(nx: f64, ny: f64, nz: f64) -> reify_types::Value {
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
        return reify_types::Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
    }

    let len = len_sq.sqrt();
    reify_types::Value::Orientation {
        w: w_unnorm / len,
        x: x_unnorm / len,
        y: y_unnorm / len,
        z: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::{CompiledGeometryOp, GeomRef, PatternKind, SweepKind, TransformKind};
    use reify_types::GeometryHandleId;

    /// Helper: build a CompiledExpr literal from a constant f64.
    fn literal_f64(v: f64) -> reify_types::CompiledExpr {
        reify_types::CompiledExpr::literal(reify_types::Value::Real(v), reify_types::Type::Real)
    }

    /// Helper: build a CompiledExpr literal from a Scalar with LENGTH dimension.
    fn literal_length(meters: f64) -> reify_types::CompiledExpr {
        reify_types::CompiledExpr::literal(
            reify_types::Value::Scalar {
                si_value: meters,
                dimension: reify_types::DimensionVector::LENGTH,
            },
            reify_types::Type::length(),
        )
    }

    /// Helper: build a CompiledExpr literal from a Scalar with ANGLE dimension (radians).
    fn literal_angle(radians: f64) -> reify_types::CompiledExpr {
        reify_types::CompiledExpr::literal(
            reify_types::Value::Scalar {
                si_value: radians,
                dimension: reify_types::DimensionVector::ANGLE,
            },
            reify_types::Type::angle(),
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
        let cell = reify_types::ValueCellId::new("Bracket", "p");
        let expr = reify_types::CompiledExpr::value_ref(
            cell.clone(),
            reify_types::Type::point3(reify_types::Type::length()),
        );
        let mut values = reify_types::ValueMap::new();
        values.insert(
            cell,
            reify_types::Value::Point(vec![
                reify_types::Value::Real(1.0),
                reify_types::Value::Real(2.0),
                reify_types::Value::Real(3.0),
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
            reify_types::GeometryOp::Scale { target, factor } => {
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
            reify_types::GeometryOp::RotateAround {
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
            reify_types::GeometryOp::Loft { profiles } => {
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
            reify_types::GeometryOp::Extrude { profile, distance } => {
                assert_eq!(profile, GeometryHandleId(10));
                // The distance must preserve Scalar type (not be converted to Value::Real)
                match distance {
                    reify_types::Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!((si_value - 0.05).abs() < 1e-12, "SI value should be 0.05m");
                        assert_eq!(
                            dimension,
                            reify_types::DimensionVector::LENGTH,
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
        let full_args: Vec<(&'static str, reify_types::CompiledExpr)> = vec![
            ("ox", literal_f64(0.0)),
            ("oy", literal_f64(0.0)),
            ("oz", literal_f64(0.0)),
            ("ax", literal_f64(0.0)),
            ("ay", literal_f64(0.0)),
            ("az", literal_f64(1.0)),
            ("angle", literal_f64(std::f64::consts::PI)),
        ];

        for omit in ["ox", "oy", "oz", "ax", "ay", "az", "angle"] {
            let args: Vec<(String, reify_types::CompiledExpr)> = full_args
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
                reify_types::Severity::Warning,
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
                .any(|d| matches!(d.severity, reify_types::Severity::Warning)
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
                .any(|d| matches!(d.severity, reify_types::Severity::Warning)
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
                matches!(d.severity, reify_types::Severity::Warning)
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
                .any(|d| matches!(d.severity, reify_types::Severity::Warning)
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
            reify_types::GeometryOp::Revolve {
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
            reify_types::GeometryOp::Extrude { profile, distance } => {
                assert_eq!(profile, GeometryHandleId(77));
                match distance {
                    reify_types::Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!(
                            (si_value - 0.03).abs() < 1e-12,
                            "SI value should be 0.03m (30mm)"
                        );
                        assert_eq!(dimension, reify_types::DimensionVector::LENGTH);
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
                matches!(d.severity, reify_types::Severity::Warning)
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
                matches!(d.severity, reify_types::Severity::Warning)
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
                matches!(d.severity, reify_types::Severity::Warning)
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
            Ok(reify_types::GeometryOp::LinearPattern {
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
                    !matches!(spacing, reify_types::Value::Undef),
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
            Ok(reify_types::GeometryOp::CircularPattern {
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
                    !matches!(angle, reify_types::Value::Undef),
                    "angle should not be Undef when arg is present"
                );
                // The explicit-unit path must NOT emit a degree-conversion warning
                let has_deg_warning = diagnostics.iter().any(|d| {
                    d.severity == reify_types::Severity::Warning
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
            Ok(reify_types::GeometryOp::CircularPattern { angle, .. }) => {
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
        let angle_int_expr = reify_types::CompiledExpr::literal(
            reify_types::Value::Int(360),
            reify_types::Type::Int,
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
            Ok(reify_types::GeometryOp::CircularPattern { angle, .. }) => {
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
            d.severity == reify_types::Severity::Warning
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
            Ok(reify_types::GeometryOp::CircularPattern { angle, .. }) => {
                let angle_f64 = angle.as_f64().expect("angle should be numeric");
                assert!(
                    (angle_f64 - std::f64::consts::PI).abs() < 1e-12,
                    "explicit PI rad angle should pass through as PI, got {}",
                    angle_f64
                );
                // No degree-conversion warning should be emitted for explicit units
                let has_deg_warning = diagnostics.iter().any(|d| {
                    d.severity == reify_types::Severity::Warning
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
            Ok(reify_types::GeometryOp::Mirror {
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
            Ok(reify_types::GeometryOp::LinearPattern2D {
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
                    !matches!(spacing1, reify_types::Value::Undef),
                    "spacing1 should not be Undef"
                );
                assert_eq!(direction2, [0.0, 1.0, 0.0]);
                assert_eq!(count2, 4);
                assert!(
                    !matches!(spacing2, reify_types::Value::Undef),
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
            Ok(reify_types::GeometryOp::ArbitraryPattern { target, transforms }) => {
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
            reify_types::Severity::Warning,
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
            reify_types::Severity::Warning,
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
            reify_types::Severity::Warning,
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
            reify_types::Severity::Warning,
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
            reify_types::Severity::Warning,
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
                    reify_types::CompiledExpr::literal(
                        reify_types::Value::String("oops".into()),
                        reify_types::Type::String,
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
                matches!(d.severity, reify_types::Severity::Warning)
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
                matches!(d.severity, reify_types::Severity::Warning)
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
                matches!(d.severity, reify_types::Severity::Warning)
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
            reify_types::GeometryOp::Union { left, right } => {
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
                    reify_types::CompiledExpr::literal(
                        reify_types::Value::String("oops".into()),
                        reify_types::Type::String,
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
                matches!(d.severity, reify_types::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("non-numeric")
            }),
            "expected a Warning mentioning 'face_0' and 'non-numeric', got: {:?}",
            diagnostics
        );
        // The resulting faces_to_remove should be empty (bad face skipped)
        match result.unwrap() {
            reify_types::GeometryOp::Shell {
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
                    reify_types::CompiledExpr::literal(
                        reify_types::Value::Bool(true),
                        reify_types::Type::Bool,
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
                matches!(d.severity, reify_types::Severity::Warning)
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
                matches!(d.severity, reify_types::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("negative")
                    && !d.message.contains("non-finite")
            }),
            "expected a Warning mentioning 'face_0' and 'negative' (not 'non-finite'), got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_types::GeometryOp::Shell {
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
                matches!(d.severity, reify_types::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("non-finite")
                    && !d.message.contains("negative")
            }),
            "expected a Warning mentioning 'non-finite' (not 'negative') for NaN face_0, got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_types::GeometryOp::Shell {
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
                matches!(d.severity, reify_types::Severity::Warning)
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
            Ok(reify_types::GeometryOp::Shell {
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
                matches!(d.severity, reify_types::Severity::Warning)
                    && d.message.contains("face_0")
                    && (d.message.contains("integer") || d.message.contains("fractional"))
            }),
            "expected a Warning about non-integer face_0, got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_types::GeometryOp::Shell {
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
                matches!(d.severity, reify_types::Severity::Warning)
                    && d.message.contains("face_0")
                    && (d.message.contains("upper bound") || d.message.contains("exceeds"))
            }),
            "expected a Warning about face_0 exceeding upper bound, got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_types::GeometryOp::Shell {
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
                    reify_types::CompiledExpr::literal(
                        reify_types::Value::String("bad".into()),
                        reify_types::Type::String,
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
                matches!(d.severity, reify_types::Severity::Warning) && d.message.contains("face_0")
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
                matches!(d.severity, reify_types::Severity::Warning) && d.message.contains("face_0")
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
                matches!(d.severity, reify_types::Severity::Warning) && d.message.contains("face_0")
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
                matches!(d.severity, reify_types::Severity::Warning) && d.message.contains("face_0")
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
                matches!(d.severity, reify_types::Severity::Warning)
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
            Ok(reify_types::GeometryOp::LinearPattern { count, .. }) => {
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
                matches!(d.severity, reify_types::Severity::Warning)
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
            reify_types::CompiledExpr::literal(reify_types::Value::Undef, reify_types::Type::Real);
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
                .any(|d| d.severity == reify_types::Severity::Warning
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
                .any(|d| d.severity == reify_types::Severity::Warning
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
                .any(|d| d.severity == reify_types::Severity::Warning
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
            reify_types::GeometryOp::Difference { left, right } => {
                assert_eq!(left, handle_a, "left should be body handle");
                assert_eq!(right, handle_b, "right should be hole handle");
            }
            other => panic!("expected Difference, got {:?}", other),
        }

        // No warnings should be emitted — named_steps lookup is silent-success
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == reify_types::Severity::Warning)
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
            .filter(|d| d.severity == reify_types::Severity::Warning)
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
    ) -> reify_types::CompiledExpr {
        let arg = reify_types::CompiledExpr::value_ref(
            reify_types::ValueCellId::new(entity, member),
            reify_types::Type::Geometry,
        );
        let mut content_hash = reify_types::ContentHash::of(&[reify_types::TAG_FUNCTION_CALL])
            .combine(reify_types::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg.content_hash);
        reify_types::CompiledExpr {
            kind: reify_types::CompiledExprKind::FunctionCall {
                function: reify_types::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg],
            },
            result_type: reify_types::Type::Bool,
            content_hash,
        }
    }

    #[test]
    fn try_eval_conformance_query_kernel_reply_true() {
        use reify_test_support::mocks::MockGeometryKernel;
        let handle_id = reify_types::GeometryHandleId(7);
        let kernel =
            MockGeometryKernel::new().with_query_result(handle_id, reify_types::Value::Bool(true));

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_types::Value::Bool(true)),
            "is_watertight(body) with kernel returning Bool(true) must produce Some(Bool(true))"
        );
    }

    /// Build a `CompiledExpr` for `is_watertight(<literal_real>)`.
    fn conformance_call_literal_arg(helper_name: &str) -> reify_types::CompiledExpr {
        let arg = reify_types::CompiledExpr::literal(
            reify_types::Value::Real(1.0),
            reify_types::Type::Real,
        );
        let mut content_hash = reify_types::ContentHash::of(&[reify_types::TAG_FUNCTION_CALL])
            .combine(reify_types::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg.content_hash);
        reify_types::CompiledExpr {
            kind: reify_types::CompiledExprKind::FunctionCall {
                function: reify_types::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg],
            },
            result_type: reify_types::Type::Bool,
            content_hash,
        }
    }

    #[test]
    fn try_eval_conformance_query_non_helper_name_returns_none_no_kernel_call() {
        let handle_id = reify_types::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
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
        let handle_id = reify_types::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();

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
        let handle_id = reify_types::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
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
            Some(reify_types::Value::Bool(true)),
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
        let handle_id = reify_types::GeometryHandleId(11);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
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

        assert_eq!(result, Some(reify_types::Value::Bool(true)));
        assert_eq!(kernel.total_query_count(), 0);
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_orientable_short_circuits() {
        let handle_id = reify_types::GeometryHandleId(13);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
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

        assert_eq!(result, Some(reify_types::Value::Bool(true)));
        assert_eq!(kernel.total_query_count(), 0);
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_closed_does_not_short_circuit_is_watertight() {
        // Asymmetry per task 2320 design decision: `is_watertight` short-
        // circuits ONLY on `Watertight` — declaring the (refined) `Closed`
        // bound is not sufficient. The kernel must be consulted and its
        // Bool(false) reply honoured.
        let handle_id = reify_types::GeometryHandleId(17);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
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
            Some(reify_types::Value::Bool(false)),
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
        let handle_id = reify_types::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        // `named_steps` contains "body" but the call references "ghost",
        // which is not present. The dispatch must return None and never
        // consult the kernel.
        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
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
        let handle_id = reify_types::GeometryHandleId(23);
        // Seed a non-Bool kernel reply for the IsWatertight query.
        let kernel = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Real(1.0));

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_types::Value::Undef),
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
            reify_types::Severity::Warning,
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
        let handle_id = reify_types::GeometryHandleId(29);
        // No `with_query_result` seeding → MockGeometryKernel.query() returns
        // `Err(QueryError::QueryFailed("no mock result for …"))` for any handle.
        let kernel = reify_test_support::mocks::MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

        let expr = conformance_call("is_manifold", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_types::Value::Undef),
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
            reify_types::Severity::Warning,
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

    /// Build a `CompiledExpr` for a stdlib call `helper(<entity>.<member_a>,
    /// <entity>.<member_b>)` with two `ValueRef` args resolving to let-bound
    /// cells. Mirrors `conformance_call` above.
    fn topology_selector_call_two_value_refs(
        helper_name: &str,
        entity: &str,
        member_a: &str,
        type_a: reify_types::Type,
        member_b: &str,
        type_b: reify_types::Type,
        result_type: reify_types::Type,
    ) -> reify_types::CompiledExpr {
        let arg_a = reify_types::CompiledExpr::value_ref(
            reify_types::ValueCellId::new(entity, member_a),
            type_a,
        );
        let arg_b = reify_types::CompiledExpr::value_ref(
            reify_types::ValueCellId::new(entity, member_b),
            type_b,
        );
        let mut content_hash = reify_types::ContentHash::of(&[reify_types::TAG_FUNCTION_CALL])
            .combine(reify_types::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        reify_types::CompiledExpr {
            kind: reify_types::CompiledExprKind::FunctionCall {
                function: reify_types::ResolvedFunction {
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
    fn topology_selector_call_literal_args(helper_name: &str) -> reify_types::CompiledExpr {
        let arg_a = reify_types::CompiledExpr::literal(
            reify_types::Value::Real(1.0),
            reify_types::Type::Real,
        );
        let arg_b = reify_types::CompiledExpr::literal(
            reify_types::Value::Real(2.0),
            reify_types::Type::Real,
        );
        let mut content_hash = reify_types::ContentHash::of(&[reify_types::TAG_FUNCTION_CALL])
            .combine(reify_types::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        reify_types::CompiledExpr {
            kind: reify_types::CompiledExprKind::FunctionCall {
                function: reify_types::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_a, arg_b],
            },
            // result_type is unused on the dispatch path — set to a
            // representative value to keep the literal hand-built expression
            // structurally well-formed.
            result_type: reify_types::Type::Bool,
            content_hash,
        }
    }

    /// Build a Value::Point with three Length scalars, mirroring how a
    /// let-bound `point3(x_mm, y_mm, z_mm)` realises in the `values` map.
    fn point3_length_value(x_m: f64, y_m: f64, z_m: f64) -> reify_types::Value {
        reify_types::Value::Point(vec![
            reify_types::Value::length(x_m),
            reify_types::Value::length(y_m),
            reify_types::Value::length(z_m),
        ])
    }

    #[test]
    fn try_eval_topology_selector_closest_point_kernel_reply_parses_to_point3_length() {
        use reify_test_support::mocks::MockGeometryKernel;
        let body_handle = reify_types::GeometryHandleId(7);
        // The kernel reply mirrors the `OcctKernel::query()` arm for
        // `ClosestPointOnShape` (lib.rs JSON-Point3 encoding). The dispatcher
        // is expected to parse it and produce a `Value::Point(vec![length(...),
        // length(...), length(...)])`.
        let kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            body_handle,
            [10.0, 0.0, 0.0],
            reify_types::Value::String("{\"x\":5.0,\"y\":0.0,\"z\":0.0}".to_string()),
        );

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), body_handle);

        let mut values = reify_types::ValueMap::new();
        values.insert(
            reify_types::ValueCellId::new("Bracket", "p"),
            point3_length_value(10.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "closest_point",
            "Bracket",
            "p",
            reify_types::Type::point3(reify_types::Type::length()),
            "body",
            reify_types::Type::Geometry,
            reify_types::Type::point3(reify_types::Type::length()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
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
        let body_handle = reify_types::GeometryHandleId(11);
        // The dispatcher must use `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` (≈ OCCT's
        // `Precision::Confusion()`, ~1e-7) for the 2-arg `is_on(point, geometry)`
        // form. Recording the mock under exactly this tolerance pins the contract —
        // if the dispatcher ever changes the default, the recorded reply would not
        // be served and the test would fail with `None`.
        let kernel = MockGeometryKernel::new().with_point_on_shape_result(
            body_handle,
            [5.0, 0.0, 0.0],
            reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            reify_types::Value::Bool(true),
        );

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), body_handle);

        let mut values = reify_types::ValueMap::new();
        values.insert(
            reify_types::ValueCellId::new("Bracket", "p"),
            point3_length_value(5.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "is_on",
            "Bracket",
            "p",
            reify_types::Type::point3(reify_types::Type::length()),
            "body",
            reify_types::Type::Geometry,
            reify_types::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_types::Value::Bool(true)),
            "is_on(p, body) with kernel reply Bool(true) must produce \
             Some(Value::Bool(true)) (default tolerance DEFAULT_POINT_ON_SHAPE_TOLERANCE_M); got {:?}",
            result
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_returns_angle_scalar() {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_a = reify_types::GeometryHandleId(31);
        let face_b = reify_types::GeometryHandleId(37);
        // Kernel returns a raw f64 (radians) — the dispatcher is expected to
        // wrap as `Value::angle(rad)` to match the cell type
        // `Type::angle()`.
        let kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_types::Value::Real(std::f64::consts::FRAC_PI_2),
        );

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("face_a".to_string(), face_a);
        named_steps.insert("face_b".to_string(), face_b);

        let values = reify_types::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_types::Type::Geometry,
            "face_b",
            reify_types::Type::Geometry,
            reify_types::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_types::Value::angle(std::f64::consts::FRAC_PI_2)),
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
        let kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        let values = reify_types::ValueMap::new();

        let expr = topology_selector_call_literal_args("closest_point");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
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
        let kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        let values = reify_types::ValueMap::new();

        let expr = topology_selector_call_literal_args("is_on");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
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
        let kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        let values = reify_types::ValueMap::new();

        let expr = topology_selector_call_literal_args("angle_between_surfaces");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
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
        let kernel = CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), reify_types::GeometryHandleId(7));

        let mut values = reify_types::ValueMap::new();
        values.insert(
            reify_types::ValueCellId::new("Bracket", "p"),
            point3_length_value(0.0, 0.0, 0.0),
        );

        // `volume` is a real stdlib function name but NOT one of the three
        // recognised topology-selector helpers. The dispatch must return
        // None, mirroring the conformance-query contract.
        let expr = topology_selector_call_two_value_refs(
            "volume",
            "Bracket",
            "p",
            reify_types::Type::point3(reify_types::Type::length()),
            "body",
            reify_types::Type::Geometry,
            reify_types::Type::dimensionless_scalar(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
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
        let face_a = reify_types::GeometryHandleId(31);
        let face_b = reify_types::GeometryHandleId(37);
        let kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_types::Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_2,
                dimension: reify_types::DimensionVector::ANGLE,
            },
        );

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("face_a".to_string(), face_a);
        named_steps.insert("face_b".to_string(), face_b);

        let values = reify_types::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_types::Type::Geometry,
            "face_b",
            reify_types::Type::Geometry,
            reify_types::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_types::Value::angle(std::f64::consts::FRAC_PI_2)),
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
        let face_a = reify_types::GeometryHandleId(31);
        let face_b = reify_types::GeometryHandleId(37);
        let kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_types::Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_2,
                dimension: reify_types::DimensionVector::DIMENSIONLESS,
            },
        );

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("face_a".to_string(), face_a);
        named_steps.insert("face_b".to_string(), face_b);

        let values = reify_types::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_types::Type::Geometry,
            "face_b",
            reify_types::Type::Geometry,
            reify_types::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_types::Value::angle(std::f64::consts::FRAC_PI_2)),
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
        reify_types::CompiledExpr,
        HashMap<String, reify_types::GeometryHandleId>,
        reify_types::ValueMap,
        reify_test_support::mocks::MockGeometryKernel,
    ) {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_a = reify_types::GeometryHandleId(31);
        let face_b = reify_types::GeometryHandleId(37);
        // LENGTH is the real-world bug class: metres silently reinterpreted as
        // radians. Using LENGTH (not e.g. MASS) ties the fixture to the actual
        // failure mode described in the task analysis.
        let kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_types::Value::Scalar {
                si_value: 1.0,
                dimension: reify_types::DimensionVector::LENGTH,
            },
        );
        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("face_a".to_string(), face_a);
        named_steps.insert("face_b".to_string(), face_b);
        let values = reify_types::ValueMap::new();
        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_types::Type::Geometry,
            "face_b",
            reify_types::Type::Geometry,
            reify_types::Type::angle(),
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
        let (expr, named_steps, values, kernel) = wrong_dim_scalar_fixture();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_types::Value::Undef),
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
            reify_types::Severity::Warning,
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
        let (expr, named_steps, values, kernel) = wrong_dim_scalar_fixture();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        // The debug_assert! in dispatch_surface_angle's Scalar arm must panic
        // with a message containing "expected ANGLE". No assert_eq! after this
        // call — the #[should_panic] attribute drives the assertion.
        super::try_eval_topology_selector(&expr, &named_steps, &values, &kernel, &mut diagnostics);
    }

    #[test]
    fn try_eval_topology_selector_is_on_non_bool_kernel_reply_emits_warning_and_returns_undef() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the `Ok(other)` warning arm of `dispatch_point_on_shape`: a kernel
        // reply that is neither `Value::Bool(_)` nor an Err must produce
        // `Some(Value::Undef)` with a Warning diagnostic naming the helper. Defends
        // the contract against a future kernel that mistakenly returns the
        // wrong-typed Value.
        let body_handle = reify_types::GeometryHandleId(11);
        let kernel = MockGeometryKernel::new().with_point_on_shape_result(
            body_handle,
            [5.0, 0.0, 0.0],
            reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            // Wrong type — should trigger the non-Bool warning arm.
            reify_types::Value::Real(0.5),
        );

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), body_handle);

        let mut values = reify_types::ValueMap::new();
        values.insert(
            reify_types::ValueCellId::new("Bracket", "p"),
            point3_length_value(5.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "is_on",
            "Bracket",
            "p",
            reify_types::Type::point3(reify_types::Type::length()),
            "body",
            reify_types::Type::Geometry,
            reify_types::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_types::Value::Undef),
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
            reify_types::Severity::Warning,
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
        // Pin the `Err(err)` parse-failure arm of `dispatch_closest_point`: a
        // kernel reply whose `Value::String(_)` payload is not parseable as a
        // JSON-Point3 must produce `Some(Value::Undef)` with a Warning
        // diagnostic naming the helper. Defends the contract against a future
        // kernel that emits a malformed JSON string.
        let body_handle = reify_types::GeometryHandleId(7);
        let kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            body_handle,
            [10.0, 0.0, 0.0],
            // Not a JSON-Point3 payload — should trigger the parse-failure
            // warning arm.
            reify_types::Value::String("not a valid json point".to_string()),
        );

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), body_handle);

        let mut values = reify_types::ValueMap::new();
        values.insert(
            reify_types::ValueCellId::new("Bracket", "p"),
            point3_length_value(10.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "closest_point",
            "Bracket",
            "p",
            reify_types::Type::point3(reify_types::Type::length()),
            "body",
            reify_types::Type::Geometry,
            reify_types::Type::point3(reify_types::Type::length()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_types::Value::Undef),
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
            reify_types::Severity::Warning,
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
        let query = reify_types::GeometryQuery::EdgeLength(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_types::ReprKind::BRep,
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
        let query = reify_types::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_types::ReprKind::BRep,
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
        let query = reify_types::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_types::ReprKind::Mesh,
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
        let query = reify_types::GeometryQuery::EdgeLength(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_types::ReprKind::Mesh,
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
            reify_types::Severity::Error,
            "diagnostic severity must be Error, got {:?}",
            diag.severity
        );
        assert_eq!(
            diag.code,
            Some(reify_types::DiagnosticCode::QueryNotSupportedOnRepr),
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
        let query = reify_types::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_types::ReprKind::Voxel,
            "distance",
            &mut diags,
        );
        assert_eq!(route, super::CapabilityRoute::Unsupported);
        assert_eq!(diags.len(), 1, "Voxel repr must emit one diag: {:?}", diags);
        assert_eq!(
            diags[0].code,
            Some(reify_types::DiagnosticCode::QueryNotSupportedOnRepr)
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
        let query = reify_types::GeometryQuery::Volume(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_types::ReprKind::Sdf,
            "volume",
            &mut diags,
        );
        assert_eq!(route, super::CapabilityRoute::Unsupported);
        assert_eq!(diags.len(), 1, "Sdf repr must emit one diag: {:?}", diags);
        assert_eq!(
            diags[0].code,
            Some(reify_types::DiagnosticCode::QueryNotSupportedOnRepr)
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
        let query = reify_types::GeometryQuery::BoundingBox(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_types::ReprKind::VolumeMesh,
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
            Some(reify_types::DiagnosticCode::QueryNotSupportedOnRepr)
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
            reify_types::ReprKind::BRep,
            reify_types::ReprKind::Mesh,
            reify_types::ReprKind::Sdf,
            reify_types::ReprKind::Voxel,
            reify_types::ReprKind::VolumeMesh,
        ];
        let brep_only_query = reify_types::GeometryQuery::EdgeLength(GeometryHandleId(1));
        let brep_and_mesh_query = reify_types::GeometryQuery::Distance {
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
                        Some(reify_types::DiagnosticCode::QueryNotSupportedOnRepr),
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
    fn orientation_components(v: reify_types::Value) -> (f64, f64, f64, f64) {
        match v {
            reify_types::Value::Orientation { w, x, y, z } => (w, x, y, z),
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
            reify_types::Value::Orientation {
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
            reify_types::Value::Orientation {
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
    ///                            None without ever reaching kernel dispatch
    #[test]
    fn frame_sub_shape_kind_from_selector_kind_contract() {
        assert_eq!(
            super::FrameSubShapeKind::from_selector_kind(&reify_types::SelectorKind::Face),
            Some(super::FrameSubShapeKind::Face),
            "SelectorKind::Face should convert to Some(FrameSubShapeKind::Face)"
        );
        assert_eq!(
            super::FrameSubShapeKind::from_selector_kind(&reify_types::SelectorKind::Edge),
            Some(super::FrameSubShapeKind::Edge),
            "SelectorKind::Edge should convert to Some(FrameSubShapeKind::Edge)"
        );
        assert!(
            super::FrameSubShapeKind::from_selector_kind(&reify_types::SelectorKind::Point).is_none(),
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

        let target = reify_types::GeometryHandleId(10);
        let centroid_json =
            reify_types::Value::String(r#"{"x":0.0,"y":0.0,"z":0.01}"#.to_string());
        let normal_json =
            reify_types::Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
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

        let Some(reify_types::Value::Frame { ref origin, ref basis }) = result else {
            panic!(
                "construct_frame_from_kernel(Face) should return Some(Value::Frame {{ .. }}); got {:?}",
                result
            );
        };
        assert_eq!(
            **origin,
            reify_types::Value::Point(vec![
                reify_types::Value::length(0.0),
                reify_types::Value::length(0.0),
                reify_types::Value::length(0.01),
            ]),
            "Face: origin should be centroid (0m, 0m, 0.01m)"
        );
        assert_eq!(
            **basis,
            reify_types::Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 },
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

        let target = reify_types::GeometryHandleId(20);
        let centroid_json =
            reify_types::Value::String(r#"{"x":0.0,"y":0.0,"z":0.005}"#.to_string());
        let tangent_json =
            reify_types::Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
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

        let Some(reify_types::Value::Frame { ref origin, ref basis }) = result else {
            panic!(
                "construct_frame_from_kernel(Edge) should return Some(Value::Frame {{ .. }}); got {:?}",
                result
            );
        };
        assert_eq!(
            **origin,
            reify_types::Value::Point(vec![
                reify_types::Value::length(0.0),
                reify_types::Value::length(0.0),
                reify_types::Value::length(0.005),
            ]),
            "Edge: origin should be centroid (0m, 0m, 0.005m)"
        );
        assert_eq!(
            **basis,
            reify_types::Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 },
            "Edge: basis should be identity (EdgeTangent +Z → +Z = zero rotation)"
        );
        assert!(
            diagnostics.is_empty(),
            "Edge: no diagnostics expected on clean kernel results; got {:?}",
            diagnostics
        );
    }
}
