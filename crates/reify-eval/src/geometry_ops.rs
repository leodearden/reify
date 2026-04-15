// Geometry operation compilation: evaluates CompiledGeometryOp into runtime GeometryOp.
//
// Free functions with no Engine coupling — they take values, functions, meta_map
// as plain arguments.

use std::collections::HashMap;

use reify_types::{CompiledFunction, Diagnostic, GeometryHandleId, ValueMap};

/// Look up a named argument in `args`, evaluate it, and return the resulting
/// `Value`.  If the argument is absent, push a `Warning` diagnostic and return
/// `None`.  Callers that need a finite `f64` should use [`eval_named_arg_f64`],
/// which also emits a `Warning` when the value is non-numeric or non-finite.
pub(crate) fn eval_named_arg(
    name: &str,
    kind_label: impl std::fmt::Debug,
    args: &[(String, reify_types::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::Value> {
    match args.iter().find(|(n, _)| n == name) {
        Some((_, expr)) => Some(reify_expr::eval_expr(
            expr,
            &reify_expr::EvalContext::new(values, functions).with_meta(meta_map),
        )),
        None => {
            diagnostics.push(Diagnostic::warning(format!(
                "missing required geometry argument '{}' for {:?}",
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
/// pushed with the message `"argument '{name}' for {kind:?} evaluated to
/// non-numeric/non-finite value"`.
pub(crate) fn eval_named_arg_f64(
    name: &str,
    kind_label: impl std::fmt::Debug + Copy,
    args: &[(String, reify_types::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    let value = eval_named_arg(name, kind_label, args, values, functions, meta_map, diagnostics)?;
    match value.as_f64() {
        Some(v) if v.is_finite() => Some(v),
        _ => {
            diagnostics.push(Diagnostic::warning(format!(
                "argument '{}' for {:?} evaluated to non-numeric/non-finite value",
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
            let v = reify_expr::eval_expr(
                expr,
                &reify_expr::EvalContext::new(values, functions)
                    .with_meta(meta_map),
            );
            match v.as_f64() {
                Some(f) if f.is_finite() => Some(f),
                _ => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "{} arg '{}' is non-finite", label, name
                    )));
                    None
                }
            }
        })
        .collect()
}

/// Compile a CompiledGeometryOp into a GeometryOp by evaluating expressions.
/// Translate a compiled geometry operation into a runtime `GeometryOp`.
///
/// Returns `None` when a required argument is missing, non-finite, or invalid
/// (e.g. negative scale factor), which signals the caller to skip this op and
/// emit a diagnostic.
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
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_types::GeometryOp> {
    use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};

    match op {
        CompiledGeometryOp::Primitive { kind, args } => {
            let mut eval_arg = |name: &str| -> Option<reify_types::Value> {
                eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            };

            match kind {
                PrimitiveKind::Box => Some(reify_types::GeometryOp::Box {
                    width: eval_arg("width")?,
                    height: eval_arg("height")?,
                    depth: eval_arg("depth")?,
                }),
                PrimitiveKind::Cylinder => Some(reify_types::GeometryOp::Cylinder {
                    radius: eval_arg("radius")?,
                    height: eval_arg("height")?,
                }),
                PrimitiveKind::Sphere => Some(reify_types::GeometryOp::Sphere {
                    radius: eval_arg("radius")?,
                }),
            }
        }
        CompiledGeometryOp::Boolean { op, left, right } => {
            let resolve_ref = |r: &GeomRef| -> Option<GeometryHandleId> {
                match r {
                    GeomRef::Step(idx) => step_handles.get(*idx).copied().filter(|h| *h != GeometryHandleId::INVALID),
                    GeomRef::Sub(_name) => step_handles.last().copied().filter(|h| *h != GeometryHandleId::INVALID),
                }
            };
            let left_id = resolve_ref(left)?;
            let right_id = resolve_ref(right)?;
            match op {
                BooleanOp::Union => Some(reify_types::GeometryOp::Union {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Difference => Some(reify_types::GeometryOp::Difference {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Intersection => Some(reify_types::GeometryOp::Intersection {
                    left: left_id,
                    right: right_id,
                }),
            }
        }
        CompiledGeometryOp::Modify { kind, target, args } => {
            let target_id = match target {
                GeomRef::Step(idx) => step_handles.get(*idx).copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                GeomRef::Sub(_) => step_handles.last().copied().filter(|h| *h != GeometryHandleId::INVALID)?,
            };
            let mut eval_arg = |name: &str| -> Option<reify_types::Value> {
                eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            };
            match kind {
                reify_compiler::ModifyKind::Fillet => Some(reify_types::GeometryOp::Fillet {
                    target: target_id,
                    radius: eval_arg("radius")?,
                }),
                reify_compiler::ModifyKind::Chamfer => Some(reify_types::GeometryOp::Chamfer {
                    target: target_id,
                    distance: eval_arg("distance")?,
                }),
                reify_compiler::ModifyKind::Shell => {
                    let thickness = eval_arg("thickness")?;
                    // Collect face indices from face_0, face_1, ...
                    let faces_to_remove: Vec<usize> = args
                        .iter()
                        .filter(|(n, _)| n.starts_with("face_"))
                        .filter_map(|(_, expr)| {
                            reify_expr::eval_expr(
                                expr,
                                &reify_expr::EvalContext::new(values, functions)
                                    .with_meta(meta_map),
                            )
                            .as_f64()
                            .map(|v| v as usize)
                        })
                        .collect();
                    Some(reify_types::GeometryOp::Shell {
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
                    // as None here rather than forwarding INVALID to the kernel.
                    let plane_id = step_handles.last().copied().filter(|h| *h != GeometryHandleId::INVALID);
                    Some(reify_types::GeometryOp::Draft {
                        target: target_id,
                        angle,
                        plane: plane_id?,
                    })
                }
                reify_compiler::ModifyKind::Thicken => {
                    let offset = eval_arg("offset")?;
                    Some(reify_types::GeometryOp::Thicken {
                        target: target_id,
                        offset,
                    })
                }
            }
        }
        CompiledGeometryOp::Transform { kind, target, args } => {
            let target_id = match target {
                GeomRef::Step(idx) => step_handles.get(*idx).copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                GeomRef::Sub(_) => step_handles.last().copied().filter(|h| *h != GeometryHandleId::INVALID)?,
            };
            let mut f64_arg = |name: &str| {
                eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
            };
            match kind {
                reify_compiler::TransformKind::Translate => {
                    Some(reify_types::GeometryOp::Translate {
                        target: target_id,
                        dx: f64_arg("dx")?,
                        dy: f64_arg("dy")?,
                        dz: f64_arg("dz")?,
                    })
                }
                reify_compiler::TransformKind::Rotate => Some(reify_types::GeometryOp::Rotate {
                    target: target_id,
                    axis: [
                        f64_arg("axis_x")?,
                        f64_arg("axis_y")?,
                        f64_arg("axis_z")?,
                    ],
                    // NOTE: bare numeric angle is passed through as-is (radians).
                    // circular_pattern converts bare numbers as degrees; aligning
                    // rotate/rotate_around/revolve is tracked as a follow-up task.
                    angle_rad: f64_arg("angle")?,
                }),
                reify_compiler::TransformKind::Scale => {
                    let factor = f64_arg("factor")?;
                    // Reject negative scale: OCCT SetScale with negative factor
                    // produces inside-out geometry (point-symmetry), not mirroring.
                    if factor < 0.0 {
                        diagnostics.push(Diagnostic::warning(format!(
                            "scale dropped: factor={} is negative (must be non-negative)",
                            factor
                        )));
                        return None;
                    }
                    Some(reify_types::GeometryOp::Scale {
                        target: target_id,
                        factor,
                    })
                }
                reify_compiler::TransformKind::RotateAround => {
                    Some(reify_types::GeometryOp::RotateAround {
                        target: target_id,
                        point: [
                            f64_arg("px")?,
                            f64_arg("py")?,
                            f64_arg("pz")?,
                        ],
                        axis: [
                            f64_arg("axis_x")?,
                            f64_arg("axis_y")?,
                            f64_arg("axis_z")?,
                        ],
                        // NOTE: bare numeric angle is passed through as-is (radians).
                        // circular_pattern converts bare numbers as degrees; aligning
                        // rotate/rotate_around/revolve is tracked as a follow-up task.
                        angle_rad: f64_arg("angle")?,
                    })
                }
            }
        }
        CompiledGeometryOp::Pattern { kind, target, args } => {
            // Pattern operations resolve target via step index
            let target_id = match target {
                GeomRef::Step(idx) => step_handles.get(*idx).copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                GeomRef::Sub(_) => step_handles.last().copied().filter(|h| *h != GeometryHandleId::INVALID)?,
            };
            match kind {
                reify_compiler::PatternKind::Linear => {
                    let mut f64_arg = |name: &str| {
                        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                    };
                    let direction = [f64_arg("dx")?, f64_arg("dy")?, f64_arg("dz")?];
                    let count = f64_arg("count")? as usize;
                    let spacing = eval_named_arg("spacing", kind, args, values, functions, meta_map, diagnostics)?;
                    Some(reify_types::GeometryOp::LinearPattern {
                        target: target_id,
                        direction,
                        count,
                        spacing,
                    })
                }
                reify_compiler::PatternKind::Circular => {
                    let mut f64_arg = |name: &str| {
                        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                    };
                    let axis_origin = [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?];
                    let axis_dir = [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?];
                    let count = f64_arg("count")? as usize;
                    let raw_angle = eval_named_arg("angle", kind, args, values, functions, meta_map, diagnostics)?;
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
                    Some(reify_types::GeometryOp::CircularPattern {
                        target: target_id,
                        axis_origin,
                        axis_dir,
                        count,
                        angle,
                    })
                }
                reify_compiler::PatternKind::Mirror => {
                    let mut f64_arg = |name: &str| {
                        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                    };
                    Some(reify_types::GeometryOp::Mirror {
                        target: target_id,
                        plane_origin: [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?],
                        plane_normal: [f64_arg("nx")?, f64_arg("ny")?, f64_arg("nz")?],
                    })
                }
                reify_compiler::PatternKind::Linear2D => {
                    let mut f64_arg = |name: &str| {
                        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                    };
                    let direction1 = [f64_arg("dx1")?, f64_arg("dy1")?, f64_arg("dz1")?];
                    let count1 = f64_arg("count1")? as usize;
                    let spacing1 = eval_named_arg("spacing1", kind, args, values, functions, meta_map, diagnostics)?;
                    let mut f64_arg = |name: &str| {
                        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                    };
                    let direction2 = [f64_arg("dx2")?, f64_arg("dy2")?, f64_arg("dz2")?];
                    let count2 = f64_arg("count2")? as usize;
                    let spacing2 = eval_named_arg("spacing2", kind, args, values, functions, meta_map, diagnostics)?;
                    Some(reify_types::GeometryOp::LinearPattern2D {
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
                        let mut f64_arg = |name: &str| {
                            eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                        };
                        let dx = f64_arg(&format!("t{}_dx", idx))?;
                        let dy = f64_arg(&format!("t{}_dy", idx))?;
                        let dz = f64_arg(&format!("t{}_dz", idx))?;
                        transforms.push([dx, dy, dz]);
                        idx += 1;
                    }
                    if transforms.is_empty() {
                        return None;
                    }
                    Some(reify_types::GeometryOp::ArbitraryPattern {
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
                    let resolved: Option<Vec<GeometryHandleId>> = profiles
                        .iter()
                        .map(|r| match r {
                            GeomRef::Step(idx) => step_handles.get(*idx).copied().filter(|h| *h != GeometryHandleId::INVALID),
                            GeomRef::Sub(_) => step_handles.last().copied().filter(|h| *h != GeometryHandleId::INVALID),
                        })
                        .collect();
                    Some(reify_types::GeometryOp::Loft {
                        profiles: resolved?,
                    })
                }
                reify_compiler::SweepKind::Extrude => {
                    let profile_handle = match profiles.first()? {
                        GeomRef::Step(idx) => step_handles.get(*idx).copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                        GeomRef::Sub(_) => step_handles.last().copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                    };
                    let distance = eval_named_arg(
                        "distance",
                        kind,
                        args,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                    )?;
                    // Reject sub-picometer magnitudes as degenerate geometry: a
                    // distance near the f64 rounding floor cannot produce a
                    // meaningful solid. Emit a warning so model authors see why
                    // the op was dropped instead of only the caller's generic
                    // "failed to compile geometry operation" error.
                    match distance.as_f64() {
                        Some(v) if v.is_finite() && v.abs() >= 1e-12 => {}
                        Some(v) => {
                            diagnostics.push(Diagnostic::warning(format!(
                                "extrude dropped: distance={} is degenerate \
                                 (|distance| must be finite and >= 1e-12 m)",
                                v
                            )));
                            return None;
                        }
                        None => return None,
                    }
                    Some(reify_types::GeometryOp::Extrude {
                        profile: profile_handle,
                        distance,
                    })
                }
                reify_compiler::SweepKind::Revolve => {
                    let profile_handle = match profiles.first()? {
                        GeomRef::Step(idx) => step_handles.get(*idx).copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                        GeomRef::Sub(_) => step_handles.last().copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                    };
                    let mut f64_arg = |name: &str| {
                        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                    };
                    let axis_dir = [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?];
                    let mag = axis_dir.iter().map(|x| x * x).sum::<f64>().sqrt();
                    // Reject sub-picometer axis magnitudes as degenerate: a
                    // zero-length (or effectively zero) rotation axis cannot
                    // define a revolve. Warn so model authors see a specific
                    // explanation instead of only the caller's generic error.
                    if !mag.is_finite() || mag < 1e-12 {
                        diagnostics.push(Diagnostic::warning(format!(
                            "revolve dropped: rotation axis [{}, {}, {}] has \
                             degenerate magnitude={} (must be finite and >= 1e-12)",
                            axis_dir[0], axis_dir[1], axis_dir[2], mag
                        )));
                        return None;
                    }
                    // NOTE: bare numeric angle is passed through as-is (radians).
                    // circular_pattern converts bare numbers as degrees; aligning
                    // rotate/rotate_around/revolve is tracked as a follow-up task.
                    let angle_rad = f64_arg("angle")?;
                    // Reject sub-picoradian angles as degenerate: an angle at
                    // the f64 rounding floor cannot produce a meaningful
                    // revolve. Warn so model authors see a specific explanation.
                    if angle_rad.abs() < 1e-12 {
                        diagnostics.push(Diagnostic::warning(format!(
                            "revolve dropped: angle={} rad is degenerate \
                             (|angle| must be >= 1e-12 rad)",
                            angle_rad
                        )));
                        return None;
                    }
                    let axis_origin = [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?];
                    Some(reify_types::GeometryOp::Revolve {
                        profile: profile_handle,
                        axis_origin,
                        axis_dir,
                        angle_rad,
                    })
                }
                reify_compiler::SweepKind::Sweep => {
                    // Resolve profile GeomRef (first entry in profiles) to a handle
                    let profile_handle = match profiles.first()? {
                        GeomRef::Step(idx) => step_handles.get(*idx).copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                        GeomRef::Sub(_) => step_handles.last().copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                    };
                    // Resolve path GeomRef (second entry in profiles) to a handle
                    let path_handle = match profiles.get(1)? {
                        GeomRef::Step(idx) => step_handles.get(*idx).copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                        GeomRef::Sub(_) => step_handles.last().copied().filter(|h| *h != GeometryHandleId::INVALID)?,
                    };
                    Some(reify_types::GeometryOp::Sweep {
                        profile: profile_handle,
                        path: path_handle,
                    })
                }
            }
        }
        CompiledGeometryOp::Curve { kind, args } => {
            use reify_compiler::CurveKind;
            match kind {
                CurveKind::LineSegment => {
                    let mut f64_arg = |name: &str| {
                        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                    };
                    Some(reify_types::GeometryOp::LineSegment {
                        x1: f64_arg("x1")?,
                        y1: f64_arg("y1")?,
                        z1: f64_arg("z1")?,
                        x2: f64_arg("x2")?,
                        y2: f64_arg("y2")?,
                        z2: f64_arg("z2")?,
                    })
                }
                CurveKind::Arc => {
                    let mut f64_arg = |name: &str| {
                        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                    };
                    Some(reify_types::GeometryOp::Arc {
                        center: [f64_arg("cx")?, f64_arg("cy")?, f64_arg("cz")?],
                        radius: f64_arg("radius")?,
                        start_angle: f64_arg("start_angle")?,
                        end_angle: f64_arg("end_angle")?,
                        axis: [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?],
                    })
                }
                CurveKind::Helix => {
                    let mut f64_arg = |name: &str| {
                        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                    };
                    Some(reify_types::GeometryOp::Helix {
                        radius: f64_arg("radius")?,
                        pitch: f64_arg("pitch")?,
                        height: f64_arg("height")?,
                    })
                }
                CurveKind::InterpCurve => {
                    let coords = eval_all_args_to_f64("interp", args, values, functions, meta_map, diagnostics)?;
                    let points: Vec<[f64; 3]> = coords
                        .chunks_exact(3)
                        .map(|c| [c[0], c[1], c[2]])
                        .collect();
                    Some(reify_types::GeometryOp::InterpCurve { points })
                }
                CurveKind::BezierCurve => {
                    let coords = eval_all_args_to_f64("bezier", args, values, functions, meta_map, diagnostics)?;
                    let control_points: Vec<[f64; 3]> = coords
                        .chunks_exact(3)
                        .map(|c| [c[0], c[1], c[2]])
                        .collect();
                    Some(reify_types::GeometryOp::BezierCurve { control_points })
                }
                CurveKind::NurbsCurve => {
                    // For NURBS, all args are passed positionally as c0,c1,...
                    // Format: first arg = degree, second = n_points, then
                    // n_points*3 pole coords, n_points weights, remaining knots.
                    let vals = eval_all_args_to_f64("nurbs", args, values, functions, meta_map, diagnostics)?;
                    if vals.len() < 2 {
                        diagnostics.push(Diagnostic::error(
                            "nurbs() requires at least degree and n_points arguments".to_string(),
                        ));
                        return None;
                    }
                    // Validate degree is a positive integer
                    if vals[0] < 1.0 || vals[0] != vals[0].trunc() || vals[0] > 25.0 {
                        diagnostics.push(Diagnostic::error(
                            format!("nurbs() degree must be a positive integer (1..25), got {}", vals[0]),
                        ));
                        return None;
                    }
                    let degree = vals[0] as usize;
                    // Remaining: need to know n_points to split.
                    // Convention: second val is n_points.
                    // Validate n_points is a positive integer within a sensible range
                    if vals[1] < 2.0 || vals[1] != vals[1].trunc() || vals[1] > (vals.len() as f64) {
                        diagnostics.push(Diagnostic::error(
                            format!(
                                "nurbs() n_points must be a positive integer >= 2 and consistent with argument count, got {}",
                                vals[1]
                            ),
                        ));
                        return None;
                    }
                    let n_points = vals[1] as usize;
                    let expected_min = 2 + n_points * 3 + n_points; // degree + n + poles + weights
                    if vals.len() < expected_min {
                        diagnostics.push(Diagnostic::error(format!(
                            "nurbs() got fewer arguments than expected for {} control points",
                            n_points,
                        )));
                        return None;
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
                        return None;
                    }
                    let expected_knots = n_points + degree + 1;
                    if knots.len() != expected_knots {
                        diagnostics.push(Diagnostic::error(format!(
                            "nurbs() expected {} knots (n_points + degree + 1 = {} + {} + 1), got {}",
                            expected_knots, n_points, degree, knots.len(),
                        )));
                        return None;
                    }
                    Some(reify_types::GeometryOp::NurbsCurve {
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

#[cfg(test)]
mod tests {
    use super::compile_geometry_op;
    use reify_types::{GeometryHandleId, ValueMap};
    use std::collections::HashMap;

    /// Smoke test: compile_geometry_op is accessible from this module and can
    /// evaluate a trivial Box primitive.
    #[test]
    fn smoke_compile_geometry_op_box() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};

        let op = CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".to_string(), reify_types::CompiledExpr::literal(reify_types::Value::Real(1.0), reify_types::Type::Real)),
                ("height".to_string(), reify_types::CompiledExpr::literal(reify_types::Value::Real(2.0), reify_types::Type::Real)),
                ("depth".to_string(), reify_types::CompiledExpr::literal(reify_types::Value::Real(3.0), reify_types::Type::Real)),
            ],
        };
        let values = ValueMap::new();
        let step_handles: Vec<GeometryHandleId> = vec![];
        let functions = vec![];
        let meta_map = HashMap::new();
        let mut diagnostics = Vec::new();

        let result = compile_geometry_op(&op, &values, &step_handles, &functions, &meta_map, &mut diagnostics);
        assert!(result.is_some(), "Box with valid args should compile to a GeometryOp");
    }
}
