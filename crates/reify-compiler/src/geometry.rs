use super::*;

pub(crate) fn is_geometry_let(expr: &reify_syntax::Expr, functions: &[CompiledFunction]) -> bool {
    matches!(
        &expr.kind,
        reify_syntax::ExprKind::FunctionCall { name, .. }
            if is_geometry_function(name) && !functions.iter().any(|f| f.name == *name)
    )
}

/// Compile a geometry function call expression into CompiledGeometryOps.
///
/// Maps positional arguments to the named parameters expected by each primitive:
/// - `box(width, height, depth)`
/// - `cylinder(radius, height)`
/// - `sphere(radius)`
///
/// Boolean operations (union, intersection, difference) take nested geometry
/// call expressions as arguments. Each arg is recursively compiled into ops,
/// and GeomRef::Step indices are assigned globally using `step_offset` (the
/// index of the first op this call will emit in the flat step_handles array).
pub(crate) fn compile_geometry_call(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    step_offset: usize,
) -> Option<Vec<CompiledGeometryOp>> {
    let (name, args) = match &expr.kind {
        reify_syntax::ExprKind::FunctionCall { name, args } => (name.as_str(), args),
        _ => return None,
    };

    // Boolean ops: args are nested geometry calls, NOT scalars.
    // Handle before scalar arg compilation below.
    match name {
        "union" | "intersection" | "difference" => {
            if args.len() != 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "{}() expects 2 arguments, got {}",
                    name,
                    args.len()
                )));
                return None;
            }
            let bool_op = match name {
                "union" => BooleanOp::Union,
                "intersection" => BooleanOp::Intersection,
                "difference" => BooleanOp::Difference,
                _ => unreachable!(),
            };
            // Compile left arg recursively.
            let left_ops = match compile_geometry_call(
                &args[0],
                scope,
                enum_defs,
                functions,
                diagnostics,
                step_offset,
            ) {
                Some(ops) => ops,
                None => {
                    // Only emit extra diagnostic if no FunctionCall was detected
                    // (i.e., arg is a literal or ident — not a geometry expression).
                    if !matches!(args[0].kind, reify_syntax::ExprKind::FunctionCall { .. }) {
                        diagnostics.push(Diagnostic::error(format!(
                            "{}() argument 1 must be a geometry expression",
                            name
                        )));
                    }
                    return None;
                }
            };
            let left_result_step = step_offset + left_ops.len() - 1;
            let right_offset = step_offset + left_ops.len();
            // Compile right arg recursively.
            let right_ops = match compile_geometry_call(
                &args[1],
                scope,
                enum_defs,
                functions,
                diagnostics,
                right_offset,
            ) {
                Some(ops) => ops,
                None => {
                    if !matches!(args[1].kind, reify_syntax::ExprKind::FunctionCall { .. }) {
                        diagnostics.push(Diagnostic::error(format!(
                            "{}() argument 2 must be a geometry expression",
                            name
                        )));
                    }
                    return None;
                }
            };
            let right_result_step = right_offset + right_ops.len() - 1;
            let mut all_ops = left_ops;
            all_ops.extend(right_ops);
            all_ops.push(CompiledGeometryOp::Boolean {
                op: bool_op,
                left: GeomRef::Step(left_result_step),
                right: GeomRef::Step(right_result_step),
            });
            return Some(all_ops);
        }
        "union_all" | "intersection_all" => {
            if args.len() < 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "{}() expects at least 2 arguments, got {}",
                    name,
                    args.len()
                )));
                return None;
            }
            let bool_op = match name {
                "union_all" => BooleanOp::Union,
                "intersection_all" => BooleanOp::Intersection,
                _ => unreachable!(),
            };
            // Left-fold: compile all args, interleaving binary Boolean ops.
            // After each pair (accumulator, next_arg), emit a Boolean op whose
            // result becomes the next accumulator.
            let mut all_ops: Vec<CompiledGeometryOp> = Vec::new();
            let mut current_offset = step_offset;

            // Compile first arg.
            let first_ops = match compile_geometry_call(
                &args[0],
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_offset,
            ) {
                Some(ops) => ops,
                None => {
                    if !matches!(args[0].kind, reify_syntax::ExprKind::FunctionCall { .. }) {
                        diagnostics.push(Diagnostic::error(format!(
                            "{}() argument 1 must be a geometry expression",
                            name
                        )));
                    }
                    return None;
                }
            };
            let mut accumulator_step = current_offset + first_ops.len() - 1;
            current_offset += first_ops.len();
            all_ops.extend(first_ops);

            // Fold remaining args left-to-right.
            for (i, arg) in args.iter().enumerate().skip(1) {
                let arg_ops = match compile_geometry_call(
                    arg,
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_offset,
                ) {
                    Some(ops) => ops,
                    None => {
                        if !matches!(arg.kind, reify_syntax::ExprKind::FunctionCall { .. }) {
                            diagnostics.push(Diagnostic::error(format!(
                                "{}() argument {} must be a geometry expression",
                                name,
                                i + 1
                            )));
                        }
                        return None;
                    }
                };
                let arg_result_step = current_offset + arg_ops.len() - 1;
                current_offset += arg_ops.len();
                all_ops.extend(arg_ops);
                // Emit binary op: (accumulator, arg) → new accumulator at current_offset.
                all_ops.push(CompiledGeometryOp::Boolean {
                    op: bool_op,
                    left: GeomRef::Step(accumulator_step),
                    right: GeomRef::Step(arg_result_step),
                });
                accumulator_step = current_offset;
                current_offset += 1;
            }
            return Some(all_ops);
        }
        _ => {}
    }

    let compiled_args: Vec<CompiledExpr> = args
        .iter()
        .map(|arg| compile_expr(arg, scope, enum_defs, functions, diagnostics))
        .collect();

    match name {
        // --- Primitives ---
        "box" => {
            if compiled_args.len() != 3 {
                diagnostics.push(Diagnostic::error(format!(
                    "box() expects 3 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                    ("depth".to_string(), it.next().unwrap()),
                ],
            }])
        }
        "cylinder" => {
            if compiled_args.len() != 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "cylinder() expects 2 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Cylinder,
                args: vec![
                    ("radius".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                ],
            }])
        }
        "sphere" => {
            if compiled_args.len() != 1 {
                diagnostics.push(Diagnostic::error(format!(
                    "sphere() expects 1 argument, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Sphere,
                args: vec![(
                    "radius".to_string(),
                    compiled_args.into_iter().next().unwrap(),
                )],
            }])
        }
        // --- Patterns ---
        // linear_pattern(target, dx, dy, dz, count, spacing)
        "linear_pattern" => {
            if compiled_args.len() != 6 {
                diagnostics.push(Diagnostic::error(format!(
                    "linear_pattern() expects 6 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear,
                target: GeomRef::Step(0), // target is first arg (evaluated at runtime)
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("dx".to_string(), it.next().unwrap()),
                    ("dy".to_string(), it.next().unwrap()),
                    ("dz".to_string(), it.next().unwrap()),
                    ("count".to_string(), it.next().unwrap()),
                    ("spacing".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // circular_pattern(target, ox, oy, oz, ax, ay, az, count, angle)
        "circular_pattern" => {
            if compiled_args.len() != 9 {
                diagnostics.push(Diagnostic::error(format!(
                    "circular_pattern() expects 9 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Circular,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("ox".to_string(), it.next().unwrap()),
                    ("oy".to_string(), it.next().unwrap()),
                    ("oz".to_string(), it.next().unwrap()),
                    ("ax".to_string(), it.next().unwrap()),
                    ("ay".to_string(), it.next().unwrap()),
                    ("az".to_string(), it.next().unwrap()),
                    ("count".to_string(), it.next().unwrap()),
                    ("angle".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // mirror(target, ox, oy, oz, nx, ny, nz)
        "mirror" => {
            if compiled_args.len() != 7 {
                diagnostics.push(Diagnostic::error(format!(
                    "mirror() expects 7 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Mirror,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("ox".to_string(), it.next().unwrap()),
                    ("oy".to_string(), it.next().unwrap()),
                    ("oz".to_string(), it.next().unwrap()),
                    ("nx".to_string(), it.next().unwrap()),
                    ("ny".to_string(), it.next().unwrap()),
                    ("nz".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // --- Sweeps ---
        // loft(profile1, profile2, ...)
        "loft" => {
            if compiled_args.len() < 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "loft() expects at least 2 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let profiles: Vec<GeomRef> = (0..compiled_args.len()).map(GeomRef::Step).collect();
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("profile_{}", i), expr))
                .collect();
            Some(vec![CompiledGeometryOp::Sweep {
                kind: SweepKind::Loft,
                profiles,
                args,
            }])
        }
        // extrude(profile, distance)
        "extrude" => {
            if compiled_args.len() != 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "extrude() expects exactly 2 arguments (profile, distance), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            let profile_expr = it.next().unwrap();
            let distance_expr = it.next().unwrap();
            Some(vec![CompiledGeometryOp::Sweep {
                kind: SweepKind::Extrude,
                profiles: vec![GeomRef::Step(0)],
                args: vec![
                    ("profile".to_string(), profile_expr),
                    ("distance".to_string(), distance_expr),
                ],
            }])
        }
        // revolve(profile, ox, oy, oz, ax, ay, az, angle)
        "revolve" => {
            if compiled_args.len() != 8 {
                diagnostics.push(Diagnostic::error(format!(
                    "revolve() expects exactly 8 arguments (profile, ox, oy, oz, ax, ay, az, angle), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            let profile_expr = it.next().unwrap();
            let ox = it.next().unwrap();
            let oy = it.next().unwrap();
            let oz = it.next().unwrap();
            let ax = it.next().unwrap();
            let ay = it.next().unwrap();
            let az = it.next().unwrap();
            let angle = it.next().unwrap();
            Some(vec![CompiledGeometryOp::Sweep {
                kind: SweepKind::Revolve,
                profiles: vec![GeomRef::Step(0)],
                args: vec![
                    ("profile".to_string(), profile_expr),
                    ("ox".to_string(), ox),
                    ("oy".to_string(), oy),
                    ("oz".to_string(), oz),
                    ("ax".to_string(), ax),
                    ("ay".to_string(), ay),
                    ("az".to_string(), az),
                    ("angle".to_string(), angle),
                ],
            }])
        }
        // revolve_full(profile, ox, oy, oz, ax, ay, az) — injects 2π for angle
        "revolve_full" => {
            if compiled_args.len() != 7 {
                diagnostics.push(Diagnostic::error(format!(
                    "revolve_full() expects exactly 7 arguments (profile, ox, oy, oz, ax, ay, az), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            let profile_expr = it.next().unwrap();
            let ox = it.next().unwrap();
            let oy = it.next().unwrap();
            let oz = it.next().unwrap();
            let ax = it.next().unwrap();
            let ay = it.next().unwrap();
            let az = it.next().unwrap();
            // Inject literal 2π for the angle
            let tau_expr = CompiledExpr::literal(
                Value::Real(std::f64::consts::TAU),
                reify_types::Type::Real,
            );
            Some(vec![CompiledGeometryOp::Sweep {
                kind: SweepKind::Revolve,
                profiles: vec![GeomRef::Step(0)],
                args: vec![
                    ("profile".to_string(), profile_expr),
                    ("ox".to_string(), ox),
                    ("oy".to_string(), oy),
                    ("oz".to_string(), oz),
                    ("ax".to_string(), ax),
                    ("ay".to_string(), ay),
                    ("az".to_string(), az),
                    ("angle".to_string(), tau_expr),
                ],
            }])
        }
        // sweep(profile, path)
        "sweep" => {
            if compiled_args.len() != 2 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "sweep() expects exactly 2 arguments (profile, path), got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
                return None;
            }
            let profiles: Vec<GeomRef> = vec![GeomRef::Step(0), GeomRef::Step(1)];
            let mut it = compiled_args.into_iter();
            let args: Vec<(String, CompiledExpr)> = vec![
                ("profile".to_string(), it.next().unwrap()),
                ("path".to_string(), it.next().unwrap()),
            ];
            Some(vec![CompiledGeometryOp::Sweep {
                kind: SweepKind::Sweep,
                profiles,
                args,
            }])
        }
        // --- Transforms ---
        // translate(target, dx, dy, dz)
        "translate" => {
            if compiled_args.len() != 4 {
                diagnostics.push(Diagnostic::error(format!(
                    "translate() expects 4 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Transform {
                kind: TransformKind::Translate,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("dx".to_string(), it.next().unwrap()),
                    ("dy".to_string(), it.next().unwrap()),
                    ("dz".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // rotate(target, ax, ay, az, angle)
        "rotate" => {
            if compiled_args.len() != 5 {
                diagnostics.push(Diagnostic::error(format!(
                    "rotate() expects 5 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Transform {
                kind: TransformKind::Rotate,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("ax".to_string(), it.next().unwrap()),
                    ("ay".to_string(), it.next().unwrap()),
                    ("az".to_string(), it.next().unwrap()),
                    ("angle".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // scale(target, factor)
        "scale" => {
            if compiled_args.len() != 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "scale() expects 2 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Transform {
                kind: TransformKind::Scale,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("factor".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // rotate_around(target, px, py, pz, ax, ay, az, angle)
        "rotate_around" => {
            if compiled_args.len() != 8 {
                diagnostics.push(Diagnostic::error(format!(
                    "rotate_around() expects 8 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Transform {
                kind: TransformKind::RotateAround,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("px".to_string(), it.next().unwrap()),
                    ("py".to_string(), it.next().unwrap()),
                    ("pz".to_string(), it.next().unwrap()),
                    ("ax".to_string(), it.next().unwrap()),
                    ("ay".to_string(), it.next().unwrap()),
                    ("az".to_string(), it.next().unwrap()),
                    ("angle".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // --- Modify extensions ---
        // shell(target, thickness, ...)
        "shell" => {
            if compiled_args.len() < 2 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "shell() expects at least 2 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
                return None;
            }
            let mut it = compiled_args.into_iter();
            let mut args = vec![
                ("target".to_string(), it.next().unwrap()),
                ("thickness".to_string(), it.next().unwrap()),
            ];
            // Remaining args are face indices to remove
            for (i, expr) in it.enumerate() {
                args.push((format!("face_{}", i), expr));
            }
            Some(vec![CompiledGeometryOp::Modify {
                kind: ModifyKind::Shell,
                target: GeomRef::Step(0),
                args,
            }])
        }
        // thicken(target, offset)
        "thicken" => {
            if compiled_args.len() != 2 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "thicken() expects 2 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Modify {
                kind: ModifyKind::Thicken,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("offset".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // draft(target, angle, plane)
        "draft" => {
            if compiled_args.len() != 3 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "draft() expects 3 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Modify {
                kind: ModifyKind::Draft,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("angle".to_string(), it.next().unwrap()),
                    ("plane".to_string(), it.next().unwrap()),
                ],
            }])
        }
        _ => {
            diagnostics.push(Diagnostic::error(format!(
                "unsupported geometry function: {}",
                name
            )));
            None
        }
    }
}

/// Detect if a constraint expression matches the count constraint pattern:
///   `collection_name.count == expr`  or  `expr == collection_name.count`
/// Returns `(collection_name, count_expr)` where count_expr is the non-.count side.
pub(crate) fn extract_count_constraint<'a>(
    expr: &'a reify_syntax::Expr,
    collection_sub_names: &HashSet<String>,
) -> Option<(String, &'a reify_syntax::Expr)> {
    if let reify_syntax::ExprKind::BinOp { op, left, right } = &expr.kind {
        if op != "==" {
            return None;
        }
        // Check LHS: collection_name.count == expr
        if let Some(name) = extract_collection_count(left, collection_sub_names) {
            return Some((name, right));
        }
        // Check RHS: expr == collection_name.count
        if let Some(name) = extract_collection_count(right, collection_sub_names) {
            return Some((name, left));
        }
    }
    None
}

/// Check if an expression is `collection_name.count` for a known collection sub.
pub(crate) fn extract_collection_count(
    expr: &reify_syntax::Expr,
    collection_sub_names: &HashSet<String>,
) -> Option<String> {
    if let reify_syntax::ExprKind::MemberAccess { object, member } = &expr.kind
        && member == "count"
        && let reify_syntax::ExprKind::Ident(name) = &object.kind
        && collection_sub_names.contains(name.as_str())
    {
        return Some(name.clone());
    }
    None
}

