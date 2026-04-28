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
                    axis: [f64_arg("axis_x")?, f64_arg("axis_y")?, f64_arg("axis_z")?],
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
                        axis: [f64_arg("axis_x")?, f64_arg("axis_y")?, f64_arg("axis_z")?],
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
                ("axis_x".into(), literal_f64(0.0)),
                ("axis_y".into(), literal_f64(0.0)),
                ("axis_z".into(), literal_f64(1.0)),
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

        // RotateAround with missing axis_z
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::RotateAround,
            target: GeomRef::Step(0),
            args: vec![
                ("px".into(), literal_f64(0.0)),
                ("py".into(), literal_f64(0.0)),
                ("pz".into(), literal_f64(0.0)),
                ("axis_x".into(), literal_f64(0.0)),
                ("axis_y".into(), literal_f64(1.0)),
                // axis_z deliberately omitted
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
        assert!(result.is_err(), "missing axis_z should return None");
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
        let kernel = MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(true));

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &[],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_types::Value::Bool(true)),
            "is_watertight(body) with kernel returning Bool(true) must produce Some(Bool(true))"
        );
    }

    /// Test-local wrapper around `MockGeometryKernel` that increments a
    /// counter on every `query()` call. Used to assert that
    /// `try_eval_conformance_query` short-circuits *before* consulting
    /// the kernel along the early-return paths (non-helper name, bad
    /// arg shape, unresolvable cell name, user-asserted marker trait).
    struct RecordingMockKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        query_count: std::sync::atomic::AtomicUsize,
    }

    impl RecordingMockKernel {
        fn new(inner: reify_test_support::mocks::MockGeometryKernel) -> Self {
            Self {
                inner,
                query_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn query_count(&self) -> usize {
            self.query_count.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl reify_types::GeometryKernel for RecordingMockKernel {
        fn execute(
            &mut self,
            op: &reify_types::GeometryOp,
        ) -> Result<reify_types::GeometryHandle, reify_types::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            query: &reify_types::GeometryQuery,
        ) -> Result<reify_types::Value, reify_types::QueryError> {
            self.query_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.query(query)
        }

        fn export(
            &self,
            handle: reify_types::GeometryHandleId,
            format: reify_types::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_types::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_types::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_types::Mesh, reify_types::TessError> {
            self.inner.tessellate(handle, tolerance)
        }
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
        let kernel = RecordingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

        // `volume` is a real stdlib function name but NOT one of the three
        // recognised conformance helpers. The dispatch must return None.
        let expr = conformance_call("volume", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &[],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "non-helper name 'volume' must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.query_count(),
            0,
            "kernel must NOT be consulted for non-helper names"
        );
    }

    #[test]
    fn try_eval_conformance_query_literal_arg_returns_none_no_kernel_call() {
        let handle_id = reify_types::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(true));
        let kernel = RecordingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();

        // `is_watertight(1.0)` — recognised helper name but the arg is a
        // literal, not a `ValueRef`. The dispatch must return None *and*
        // never consult the kernel.
        let expr = conformance_call_literal_arg("is_watertight");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &[],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "is_watertight(<literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.query_count(),
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
        let kernel = RecordingMockKernel::new(inner);

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
            kernel.query_count(),
            0,
            "kernel must NOT be consulted when the structure asserts Watertight"
        );
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_manifold_short_circuits() {
        let handle_id = reify_types::GeometryHandleId(11);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(false));
        let kernel = RecordingMockKernel::new(inner);

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
        assert_eq!(kernel.query_count(), 0);
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_orientable_short_circuits() {
        let handle_id = reify_types::GeometryHandleId(13);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(false));
        let kernel = RecordingMockKernel::new(inner);

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
        assert_eq!(kernel.query_count(), 0);
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
        let kernel = RecordingMockKernel::new(inner);

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
            kernel.query_count(),
            1,
            "kernel must be consulted exactly once when no matching marker trait is declared"
        );
    }

    #[test]
    fn try_eval_conformance_query_unresolvable_member_returns_none_no_kernel_call() {
        let handle_id = reify_types::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_types::Value::Bool(true));
        let kernel = RecordingMockKernel::new(inner);

        // `named_steps` contains "body" but the call references "ghost",
        // which is not present. The dispatch must return None and never
        // consult the kernel.
        let mut named_steps: HashMap<String, reify_types::GeometryHandleId> = HashMap::new();
        named_steps.insert("body".to_string(), handle_id);

        let expr = conformance_call("is_watertight", "Bracket", "ghost");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &[],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "unresolvable cell-member 'ghost' must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.query_count(),
            0,
            "kernel must NOT be consulted when the cell-member name is absent"
        );
    }
}
