use super::*;

pub(crate) fn is_geometry_let(
    expr: &reify_syntax::Expr,
    functions: &[CompiledFunction],
    known_geometry_lets: &HashSet<&str>,
) -> bool {
    match &expr.kind {
        reify_syntax::ExprKind::FunctionCall { name, .. } => {
            is_geometry_function(name) && !functions.iter().any(|f| f.name == *name)
        }
        // No `!functions.iter().any(...)` guard needed: `known_geometry_lets` is
        // populated only from let-binding names (never function names), and an Ident
        // expression is syntactically distinct from FunctionCall, so a user-defined
        // function cannot collide with a geometry let via this branch.
        reify_syntax::ExprKind::Ident(name) => known_geometry_lets.contains(name.as_str()),
        _ => false,
    }
}

/// Returns the arg indices that are geometry refs for each non-boolean geometry function.
/// Empty slice means no geometry args (primitives, curves).
/// Boolean ops are excluded — they handle geometry args with their own recursive block.
fn geometry_arg_indices(name: &str) -> &'static [usize] {
    match name {
        "translate" | "rotate" | "scale" | "rotate_around"
        | "circular_pattern" | "linear_pattern" | "mirror"
        | "extrude" | "revolve" | "revolve_full"
        | "shell" | "thicken" | "draft" => &[0],
        "sweep" => &[0, 1],
        // NOTE: `loft` is handled specially (variadic geometry args) in the resolution block.
        // IMPORTANT: New geometry functions that take geometry args MUST be registered here
        // (or handled like loft for variadic cases). Missing entries are silently treated as
        // having no geometry args, breaking let-bound geometry references for those functions.
        _ => &[],
    }
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_geometry_call(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    step_offset: usize,
    geometry_lets: &HashMap<&str, &reify_syntax::Expr>,
    visiting: &mut HashSet<String>,
) -> Option<Vec<CompiledGeometryOp>> {
    // Resolve let-bound geometry variable references: when the expression is an
    // Ident that names a geometry let, recursively compile the initializer.
    // Guard against cycles (e.g. `let a = difference(b, x); let b = difference(a, y);`)
    // by tracking which names are currently being resolved.
    if let reify_syntax::ExprKind::Ident(name) = &expr.kind {
        if let Some(init_expr) = geometry_lets.get(name.as_str()) {
            if !visiting.insert(name.clone()) {
                diagnostics.push(Diagnostic::error(format!(
                    "cyclic geometry let reference: '{}' references itself (directly or indirectly)",
                    name
                )));
                return None;
            }
            let result = compile_geometry_call(
                init_expr,
                scope,
                enum_defs,
                functions,
                diagnostics,
                step_offset,
                geometry_lets,
                visiting,
            );
            visiting.remove(name.as_str());
            return result;
        }
        return None;
    }

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
                geometry_lets,
                visiting,
            ) {
                Some(ops) => ops,
                None => {
                    // Only emit extra diagnostic if the arg is not a geometry expression
                    // (neither a FunctionCall nor a geometry-let Ident).
                    if !matches!(args[0].kind, reify_syntax::ExprKind::FunctionCall { .. })
                        && !matches!(&args[0].kind, reify_syntax::ExprKind::Ident(n) if geometry_lets.contains_key(n.as_str()))
                    {
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
                geometry_lets,
                visiting,
            ) {
                Some(ops) => ops,
                None => {
                    if !matches!(args[1].kind, reify_syntax::ExprKind::FunctionCall { .. })
                        && !matches!(&args[1].kind, reify_syntax::ExprKind::Ident(n) if geometry_lets.contains_key(n.as_str()))
                    {
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
                geometry_lets,
                visiting,
            ) {
                Some(ops) => ops,
                None => {
                    if !matches!(args[0].kind, reify_syntax::ExprKind::FunctionCall { .. })
                        && !matches!(&args[0].kind, reify_syntax::ExprKind::Ident(n) if geometry_lets.contains_key(n.as_str()))
                    {
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
                    geometry_lets,
                    visiting,
                ) {
                    Some(ops) => ops,
                    None => {
                        if !matches!(arg.kind, reify_syntax::ExprKind::FunctionCall { .. })
                            && !matches!(&arg.kind, reify_syntax::ExprKind::Ident(n) if geometry_lets.contains_key(n.as_str()))
                        {
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

    // Generic geometry-arg resolution: for each arg index that is a geometry ref,
    // recursively compile the geometry expression, collect sub-ops, and record the
    // result step in geom_refs. Boolean ops are handled above and excluded here.
    // Short-circuit for primitives and curves (no geometry args) to avoid
    // unnecessary allocations on the hot path for the majority of calls.
    let static_indices = geometry_arg_indices(name);
    let needs_geom_resolution = name == "loft" || !static_indices.is_empty();

    let mut sub_ops: Vec<CompiledGeometryOp> = Vec::new();
    let mut geom_refs: HashMap<usize, GeomRef> = HashMap::new();
    let mut current_offset = step_offset;

    if needs_geom_resolution {
        let effective_indices: Vec<usize> = if name == "loft" {
            (0..args.len()).collect()
        } else {
            static_indices.to_vec()
        };
        for idx in &effective_indices {
            if *idx < args.len()
                && let Some(ops) = compile_geometry_call(
                    &args[*idx],
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_offset,
                    geometry_lets,
                    visiting,
                )
            {
                let result_step = current_offset + ops.len() - 1;
                current_offset += ops.len();
                geom_refs.insert(*idx, GeomRef::Step(result_step));
                sub_ops.extend(ops);
            }
        }
    }

    let compiled_args: Vec<CompiledExpr> = args
        .iter()
        .map(|arg| compile_expr(arg, scope, enum_defs, functions, diagnostics))
        .collect();

    // Helper: look up resolved geometry ref or fall back to step_offset.
    // Used by single-geometry-arg functions (translate, rotate, etc.).
    // For loft and sweep the fallback is handled with explicit diagnostics below.
    let geom_ref = |idx: usize| -> GeomRef {
        geom_refs.get(&idx).cloned().unwrap_or(GeomRef::Step(step_offset))
    };

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
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("dx".to_string(), it.next().unwrap()),
                    ("dy".to_string(), it.next().unwrap()),
                    ("dz".to_string(), it.next().unwrap()),
                    ("count".to_string(), it.next().unwrap()),
                    ("spacing".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Pattern {
                kind: PatternKind::Circular,
                target,
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
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Pattern {
                kind: PatternKind::Mirror,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("ox".to_string(), it.next().unwrap()),
                    ("oy".to_string(), it.next().unwrap()),
                    ("oz".to_string(), it.next().unwrap()),
                    ("nx".to_string(), it.next().unwrap()),
                    ("ny".to_string(), it.next().unwrap()),
                    ("nz".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // linear_pattern_2d(target, dx1, dy1, dz1, count1, spacing1, dx2, dy2, dz2, count2, spacing2)
        "linear_pattern_2d" => {
            if compiled_args.len() != 11 {
                diagnostics.push(Diagnostic::error(format!(
                    "linear_pattern_2d() expects 11 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear2D,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("dx1".to_string(), it.next().unwrap()),
                    ("dy1".to_string(), it.next().unwrap()),
                    ("dz1".to_string(), it.next().unwrap()),
                    ("count1".to_string(), it.next().unwrap()),
                    ("spacing1".to_string(), it.next().unwrap()),
                    ("dx2".to_string(), it.next().unwrap()),
                    ("dy2".to_string(), it.next().unwrap()),
                    ("dz2".to_string(), it.next().unwrap()),
                    ("count2".to_string(), it.next().unwrap()),
                    ("spacing2".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // arbitrary_pattern(target, dx1, dy1, dz1, dx2, dy2, dz2, ...)
        "arbitrary_pattern" => {
            if compiled_args.len() < 4 || !(compiled_args.len() - 1).is_multiple_of(3) {
                diagnostics.push(Diagnostic::error(format!(
                    "arbitrary_pattern() expects target + N*(dx,dy,dz) triples (>= 4 args, (len-1) % 3 == 0), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            let mut args = vec![("target".to_string(), it.next().unwrap())];
            let coords: Vec<_> = it.collect();
            for (idx, chunk) in coords.chunks_exact(3).enumerate() {
                args.push((format!("t{}_dx", idx), chunk[0].clone()));
                args.push((format!("t{}_dy", idx), chunk[1].clone()));
                args.push((format!("t{}_dz", idx), chunk[2].clone()));
            }
            Some(vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Arbitrary,
                target: GeomRef::Step(0),
                args,
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
            let mut profiles = Vec::with_capacity(args.len());
            for i in 0..args.len() {
                // Silent fallback — consistent with extrude/revolve_full which use
                // geom_ref() and never emit an error for non-geometry profiles.
                let r = geom_refs
                    .get(&i)
                    .cloned()
                    .unwrap_or(GeomRef::Step(step_offset + i));
                profiles.push(r);
            }
            let loft_args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("profile_{}", i), expr))
                .collect();
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Loft,
                profiles,
                args: loft_args,
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let profile = geom_ref(0);
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Extrude,
                profiles: vec![profile],
                args: vec![
                    ("profile".to_string(), profile_expr),
                    ("distance".to_string(), distance_expr),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let profile = geom_ref(0);
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Revolve,
                profiles: vec![profile],
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
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let profile = geom_ref(0);
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Revolve,
                profiles: vec![profile],
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
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            // Note: sweep() emits an error diagnostic when profile or path are not
            // geometry expressions. This is intentionally asymmetric with loft(), which
            // silently falls back to GeomRef::Step(step_offset) for any non-geometry
            // profile (matching the geom_ref() helper convention used by extrude/revolve_full).
            // sweep() is stricter because its two arguments have specific, named roles
            // (profile and path) that are clearly communicated to the user.
            let profile = if let Some(r) = geom_refs.get(&0).cloned() {
                r
            } else {
                diagnostics.push(Diagnostic::error(
                    "sweep() profile (argument 1) must be a geometry expression".to_string(),
                ));
                GeomRef::Step(step_offset)
            };
            let path = if let Some(r) = geom_refs.get(&1).cloned() {
                r
            } else {
                diagnostics.push(Diagnostic::error(
                    "sweep() path (argument 2) must be a geometry expression".to_string(),
                ));
                GeomRef::Step(step_offset + 1)
            };
            let mut it = compiled_args.into_iter();
            let sweep_args: Vec<(String, CompiledExpr)> = vec![
                ("profile".to_string(), it.next().unwrap()),
                ("path".to_string(), it.next().unwrap()),
            ];
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Sweep,
                profiles: vec![profile, path],
                args: sweep_args,
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Transform {
                kind: TransformKind::Translate,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("dx".to_string(), it.next().unwrap()),
                    ("dy".to_string(), it.next().unwrap()),
                    ("dz".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Transform {
                kind: TransformKind::Rotate,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("ax".to_string(), it.next().unwrap()),
                    ("ay".to_string(), it.next().unwrap()),
                    ("az".to_string(), it.next().unwrap()),
                    ("angle".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Transform {
                kind: TransformKind::Scale,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("factor".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Transform {
                kind: TransformKind::RotateAround,
                target,
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
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let mut shell_args = vec![
                ("target".to_string(), it.next().unwrap()),
                ("thickness".to_string(), it.next().unwrap()),
            ];
            // Remaining args are face indices to remove
            for (i, expr) in it.enumerate() {
                shell_args.push((format!("face_{}", i), expr));
            }
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Modify {
                kind: ModifyKind::Shell,
                target,
                args: shell_args,
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Modify {
                kind: ModifyKind::Thicken,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("offset".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
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
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Modify {
                kind: ModifyKind::Draft,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("angle".to_string(), it.next().unwrap()),
                    ("plane".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // --- Curve constructors ---
        // line_segment(x1, y1, z1, x2, y2, z2)
        "line_segment" => {
            if compiled_args.len() != 6 {
                diagnostics.push(Diagnostic::error(format!(
                    "line_segment() expects 6 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::LineSegment,
                args: vec![
                    ("x1".to_string(), it.next().unwrap()),
                    ("y1".to_string(), it.next().unwrap()),
                    ("z1".to_string(), it.next().unwrap()),
                    ("x2".to_string(), it.next().unwrap()),
                    ("y2".to_string(), it.next().unwrap()),
                    ("z2".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)
        "arc" => {
            if compiled_args.len() != 9 {
                diagnostics.push(Diagnostic::error(format!(
                    "arc() expects 9 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::Arc,
                args: vec![
                    ("cx".to_string(), it.next().unwrap()),
                    ("cy".to_string(), it.next().unwrap()),
                    ("cz".to_string(), it.next().unwrap()),
                    ("radius".to_string(), it.next().unwrap()),
                    ("start_angle".to_string(), it.next().unwrap()),
                    ("end_angle".to_string(), it.next().unwrap()),
                    ("ax".to_string(), it.next().unwrap()),
                    ("ay".to_string(), it.next().unwrap()),
                    ("az".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // helix(radius, pitch, height)
        "helix" => {
            if compiled_args.len() != 3 {
                diagnostics.push(Diagnostic::error(format!(
                    "helix() expects 3 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::Helix,
                args: vec![
                    ("radius".to_string(), it.next().unwrap()),
                    ("pitch".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // interp(x1,y1,z1, x2,y2,z2, ...) — variadic, triples of coordinates
        "interp" => {
            if compiled_args.len() < 6 || !compiled_args.len().is_multiple_of(3) {
                diagnostics.push(Diagnostic::error(format!(
                    "interp() expects coordinate triples (at least 6 args for 2 points), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("c{}", i), expr))
                .collect();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::InterpCurve,
                args,
            }])
        }
        // bezier(x1,y1,z1, x2,y2,z2, ...) — variadic, triples of coordinates
        "bezier" => {
            if compiled_args.len() < 6 || !compiled_args.len().is_multiple_of(3) {
                diagnostics.push(Diagnostic::error(format!(
                    "bezier() expects coordinate triples (at least 6 args for 2 points), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("c{}", i), expr))
                .collect();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::BezierCurve,
                args,
            }])
        }
        // nurbs(degree, n_points, x1,y1,z1,..., w1,..., k1,...)
        // For simplicity: nurbs(degree, x1,y1,z1,...xn,yn,zn, w1,...wn, k1,...km)
        // Actually, let's use a flat encoding: first arg is degree, then groups.
        // Simpler: nurbs takes named-style flat args similar to other constructors.
        // We'll pass all args as positional and decode in eval.
        "nurbs" => {
            // Minimum: degree(1) + n_points(1) + 2×3 coords(6) + 2 weights(2) = 10
            // (knots are also required but their count depends on degree, so we
            // defer full validation to the eval layer)
            if compiled_args.len() < 10 {
                diagnostics.push(Diagnostic::error(format!(
                    "nurbs() expects at least 10 arguments (degree + n_points + coords + weights), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("c{}", i), expr))
                .collect();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::NurbsCurve,
                args,
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

