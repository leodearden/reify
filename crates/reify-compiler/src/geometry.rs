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

/// Returns `true` if a param with the given type and default expression is a
/// Solid-typed geometry param — i.e. its type is `Type::Geometry` and its
/// default expression is a geometry call or a reference to a geometry let.
///
/// This predicate is the single source of truth for the "Solid param with
/// geometry-call default" check, which must be consistent across four sites:
/// entity.rs pre-pass, entity.rs main Param loop, guards.rs
/// `register_guarded_names`, and guards.rs `compile_guarded_members`.
pub(crate) fn is_solid_geometry_param(
    ty: &reify_types::Type,
    default: Option<&reify_syntax::Expr>,
    functions: &[CompiledFunction],
    known: &HashSet<&str>,
) -> bool {
    ty == &reify_types::Type::Geometry
        && default
            .map(|e| is_geometry_let(e, functions, known))
            .unwrap_or(false)
}

/// Returns the arg indices that are geometry refs for each non-boolean geometry function.
/// Empty slice means no geometry args (primitives, curves).
/// Boolean ops are excluded — they handle geometry args with their own recursive block.
fn geometry_arg_indices(name: &str) -> &'static [usize] {
    match name {
        "translate" | "rotate" | "scale" | "rotate_around"
        | "circular_pattern" | "linear_pattern" | "mirror"
        | "extrude" | "revolve" | "revolve_full"
        | "shell" | "thicken" | "draft" | "chamfer" | "fillet" => &[0],
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
        "union" | "intersection" | "difference" | "union_all" | "intersection_all" => {
            return compile_boolean_op(
                name,
                args,
                expr.span,
                scope,
                enum_defs,
                functions,
                diagnostics,
                step_offset,
                geometry_lets,
                visiting,
            );
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
    // Used by single-geometry-arg functions (extrude, revolve, revolve_full,
    // translate, rotate, etc.) and by loft (which calls the equivalent
    // step_offset+i form inline to preserve per-profile index uniqueness).
    // These functions intentionally emit no diagnostic when the geometry arg
    // is non-geometry: their callers are responsible for providing a geometry
    // expression, and the silent fallback keeps compilation from
    // short-circuiting while still producing an op for downstream analysis.
    // sweep() is the exception — it emits per-argument diagnostics with span
    // labels so users get a precise numbered error for each bad argument.
    let geom_ref = |idx: usize| -> GeomRef {
        geom_refs.get(&idx).cloned().unwrap_or(GeomRef::Step(step_offset))
    };

    match name {
        // --- Primitives ---
        "box" => {
            if compiled_args.len() != 3 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "box() expects 3 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
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
                diagnostics.push(
                    Diagnostic::error(format!(
                        "cylinder() expects 2 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
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
                diagnostics.push(
                    Diagnostic::error(format!(
                        "sphere() expects 1 argument, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
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
                diagnostics.push(
                    Diagnostic::error(format!(
                        "linear_pattern() expects 6 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
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
                diagnostics.push(
                    Diagnostic::error(format!(
                        "circular_pattern() expects 9 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
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
                diagnostics.push(
                    Diagnostic::error(format!(
                        "mirror() expects 7 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
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
                // Silently fall back to GeomRef::Step(step_offset + i) per profile
                // when an arg is not a geometry expression. This matches the
                // single-geom-arg geom_ref() convention while preserving per-profile
                // index uniqueness (loft requires distinct cross-sections, so each
                // fallback profile needs a distinct step index for downstream
                // analysis).
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
            // sweep() emits per-argument diagnostics when an arg is not a geometry
            // expression, then falls back to GeomRef::Step so the op is still
            // produced for downstream analysis.  (loft() differs: it falls back
            // silently per profile to preserve index uniqueness without
            // duplicating the type-check error category at each profile slot.)
            let profile = if let Some(r) = geom_refs.get(&0).cloned() {
                r
            } else {
                diagnostics.push(
                    Diagnostic::error(
                        "sweep() profile (argument 1) must be a geometry expression".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        args[0].span,
                        "not a geometry expression",
                    )),
                );
                GeomRef::Step(step_offset)
            };
            let path = if let Some(r) = geom_refs.get(&1).cloned() {
                r
            } else {
                diagnostics.push(
                    Diagnostic::error(
                        "sweep() path (argument 2) must be a geometry expression".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        args[1].span,
                        "not a geometry expression",
                    )),
                );
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
        "translate" | "rotate" | "scale" | "rotate_around" => {
            compile_transform_op(name, compiled_args, geom_ref(0), diagnostics, sub_ops)
        }
        // --- Modify extensions ---
        // All five modifiers take a geometry target as their first argument (correctly
        // resolved from geom_refs via geom_ref(0)) and are registered in geometry_arg_indices().
        "shell" | "thicken" | "draft" | "chamfer" | "fillet" => {
            compile_modify_op(name, compiled_args, geom_ref(0), expr.span, diagnostics, sub_ops)
        }
        // --- Curve constructors ---
        "line_segment" | "arc" | "helix" | "interp" | "bezier" | "nurbs" => {
            compile_curve_op(name, compiled_args, diagnostics)
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

// ─── Registry cross-check (task-1733) ────────────────────────────────────────
//
// The tests below cross-check the set of function names handled in
// `geometry_arg_indices` against the names dispatched in `compile_geometry_call`.
// When a new geometry function is added to the dispatch block, it must also be
// added to one of the lists below, ensuring `geometry_arg_indices` is kept in
// sync and geometry-arg resolution is not silently broken.
//
// The `compile_geometry_call_source_arms_match_registry_lists` test closes the
// reverse direction: it scans `compile_geometry_call`'s source text for
// match-arm string-literal keys and verifies they all appear in the four
// registry lists, so an undeclared arm cannot slip through undetected.

#[cfg(test)]
mod tests {
    use super::*;

    /// Every non-boolean, non-loft function dispatched in `compile_geometry_call`
    /// that takes at least one geometry arg (first arg is target/profile/etc.).
    /// These MUST return non-empty from `geometry_arg_indices`.
    const GEOM_ARG_FUNCTIONS: &[&str] = &[
        "translate",
        "rotate",
        "scale",
        "rotate_around",
        "circular_pattern",
        "linear_pattern",
        "mirror",
        "extrude",
        "revolve",
        "revolve_full",
        "shell",
        "thicken",
        "draft",
        "chamfer",
        "fillet",
        "sweep",
    ];

    /// Every non-boolean function dispatched in `compile_geometry_call` that has
    /// NO geometry args (primitives, curves, patterns that don't use geom_ref).
    /// These MUST return empty from `geometry_arg_indices`.
    const NO_GEOM_ARG_FUNCTIONS: &[&str] = &[
        "box",
        "cylinder",
        "sphere",
        "linear_pattern_2d",
        "arbitrary_pattern",
        "line_segment",
        "arc",
        "helix",
        "interp",
        "bezier",
        "nurbs",
    ];

    /// Canary pin: the total number of distinct function names dispatched by
    /// `compile_geometry_call`, spread across the four category lists.
    ///
    /// Breakdown at time of writing:
    /// ```text
    /// GEOM_ARG_FUNCTIONS    16
    /// NO_GEOM_ARG_FUNCTIONS 11
    /// boolean ops            5
    /// loft                   1
    /// Total                 33
    /// ```
    ///
    /// **Maintenance rule:** whenever a new arm is added to `compile_geometry_call`,
    ///   1. Add the function name to exactly one of the four lists in
    ///      `all_dispatch_functions_accounted_for`.
    ///   2. Increment this constant.
    ///   3. Confirm the `assert_eq!` in `all_dispatch_functions_accounted_for` still passes.
    ///
    /// The constant is declared separately from the lists so any mutation of the lists
    /// that omits the corresponding increment will trip the assertion, prompting a
    /// conscious audit.
    const EXPECTED_DISPATCH_COUNT: usize = 33;

    #[test]
    fn geometry_arg_indices_covers_all_geom_arg_functions() {
        for &name in GEOM_ARG_FUNCTIONS {
            assert!(
                !geometry_arg_indices(name).is_empty(),
                "geometry_arg_indices(\"{}\") returned empty — \
                 this function takes geometry args but is not registered in the index",
                name
            );
        }
    }

    #[test]
    fn geometry_arg_indices_empty_for_no_geom_arg_functions() {
        for &name in NO_GEOM_ARG_FUNCTIONS {
            assert!(
                geometry_arg_indices(name).is_empty(),
                "geometry_arg_indices(\"{}\") returned non-empty — \
                 this function should not have geometry args registered",
                name
            );
        }
    }

    #[test]
    fn geometry_arg_indices_loft_is_empty_handled_specially() {
        // loft is variadic — handled with special logic in compile_geometry_call,
        // not via geometry_arg_indices. Verify it returns empty (the wildcard arm)
        // so we know the special path is the only handler.
        assert!(
            geometry_arg_indices("loft").is_empty(),
            "loft should not be in geometry_arg_indices — it uses variadic handling"
        );
    }

    #[test]
    fn all_dispatch_functions_accounted_for() {
        // Ensure the two lists together with loft and the boolean ops cover every
        // arm in compile_geometry_call.  If a new function is added there but not
        // listed here, this test should be updated (the developer will notice
        // because the new function is absent from both lists).
        let boolean_ops: &[&str] =
            &["union", "intersection", "difference", "union_all", "intersection_all"];
        let loft: &[&str] = &["loft"];

        let mut all: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for &name in GEOM_ARG_FUNCTIONS
            .iter()
            .chain(NO_GEOM_ARG_FUNCTIONS.iter())
            .chain(boolean_ops.iter())
            .chain(loft.iter())
        {
            assert!(
                all.insert(name),
                "duplicate function name \"{}\" in cross-check lists",
                name
            );
        }

        // The per-function tests above (`geometry_arg_indices_covers_all_geom_arg_functions`
        // and `geometry_arg_indices_empty_for_no_geom_arg_functions`) are the primary
        // correctness guardrail — they verify each function is in the right list.
        // `EXPECTED_DISPATCH_COUNT` is the canary pin for the four lists above.  If any of
        // GEOM_ARG_FUNCTIONS, NO_GEOM_ARG_FUNCTIONS, boolean_ops, or loft changes,
        // bump that constant and verify that `compile_geometry_call` contains a matching
        // dispatch arm for the new entry.
        // NOTE: the reverse direction (an arm added to `compile_geometry_call` without
        // a corresponding entry in one of the four lists) is now caught by the companion
        // `compile_geometry_call_source_arms_match_registry_lists` test, which scans the
        // source text of the function directly.
        assert_eq!(
            all.len(),
            EXPECTED_DISPATCH_COUNT,
            "total dispatched geometry function count changed — \
             bump EXPECTED_DISPATCH_COUNT and make sure the new function is added to \
             GEOM_ARG_FUNCTIONS, NO_GEOM_ARG_FUNCTIONS, boolean_ops, or loft above"
        );
    }

    /// Scan the body of a named function in `source` for match-arm string-literal
    /// keys and return them as a `HashSet<String>`.
    ///
    /// Algorithm:
    /// 1. Find the first occurrence of `fn_signature_prefix`.
    /// 2. Scan forward to the first `{` — the function-body open brace.
    /// 3. Walk byte-by-byte tracking `{`/`}` depth to locate the body close brace.
    /// 4. Strip `/* ... */` block comments from the body (prevents false positives
    ///    from quoted strings inside block comments).
    /// 5. For each line inside the body: skip `//`-comment-only lines; strip
    ///    trailing inline `//` comments; accumulate pure-pattern lines (string
    ///    literals and `|` operators only) into `pending_lhs` for multi-line
    ///    or-patterns; on any line that contains `=>`, combine `pending_lhs` with
    ///    the current line's LHS and extract every double-quoted token.
    ///
    /// Correctly handles single-arm (`"foo" =>`), multi-arm on one line
    /// (`"a" | "b" | "c" =>`), rustfmt-wrapped multi-arm patterns (continuation
    /// lines beginning with `|`), nested inner `match` blocks, wildcard arms
    /// (`_ =>`), pattern-destructure arms (no string literals on the LHS), and
    /// code outside the target function.
    ///
    /// Known limitation: block-comment stripping does not track string-literal
    /// boundaries, so a `/*` inside a string literal in a match-arm pattern would
    /// be mishandled.  This combination does not occur in `compile_geometry_call`.
    fn extract_dispatch_keys_from_source(
        source: &str,
        fn_signature_prefix: &str,
    ) -> HashSet<String> {
        // Step 1: locate the function signature.
        let sig_pos = source.find(fn_signature_prefix).unwrap_or_else(|| {
            panic!(
                "extract_dispatch_keys_from_source: could not find '{}' in source",
                fn_signature_prefix
            )
        });

        // Step 2: scan forward from the signature to the opening '{'.
        let after_sig = &source[sig_pos..];
        let brace_rel = after_sig.find('{').unwrap_or_else(|| {
            panic!(
                "extract_dispatch_keys_from_source: no opening '{{' found after '{}'",
                fn_signature_prefix
            )
        });
        let body_start = sig_pos + brace_rel + 1; // exclusive of the opening '{'

        // Step 3: track brace depth to find the matching close brace.
        let mut depth: usize = 1;
        let mut body_end = body_start;
        let source_bytes = source.as_bytes();
        for i in body_start..source.len() {
            match source_bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        body_end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &source[body_start..body_end];

        // Step 4a: remove block comments to prevent false positives from quoted
        // strings inside `/* ... */` spans.
        let body_clean = strip_block_comments(body);

        // Step 4b: scan line-by-line, accumulating multi-line or-pattern fragments.
        let mut keys = HashSet::new();
        // Accumulates pattern LHS fragments across physical lines when rustfmt
        // wraps a long `"a" | "b" | "c" =>` arm (continuation lines start with `|`).
        let mut pending_lhs = String::new();

        for raw_line in body_clean.lines() {
            let trimmed = raw_line.trim();

            // Skip `//`-comment-only lines.
            if trimmed.starts_with("//") {
                continue;
            }

            // Strip trailing inline `//` comments (only outside string literals).
            let line = strip_inline_comment(raw_line);
            let line_trimmed = line.trim();

            if let Some(arrow_pos) = line.find("=>") {
                // `=>` found: finalize the logical match-arm LHS.
                let full_lhs = format!("{} {}", pending_lhs, &line[..arrow_pos]);
                pending_lhs.clear();

                // Extract every double-quoted token from the combined LHS.
                let mut chars = full_lhs.chars();
                while let Some(c) = chars.next() {
                    if c == '"' {
                        let token: String =
                            chars.by_ref().take_while(|&ch| ch != '"').collect();
                        if !token.is_empty() {
                            keys.insert(token);
                        }
                    }
                }
            } else if is_or_pattern_line(line_trimmed) {
                // Pure pattern fragment (string literals and `|` operators only):
                // this is a continuation line of a rustfmt-wrapped multi-arm pattern.
                pending_lhs.push(' ');
                pending_lhs.push_str(line_trimmed);
            } else if !line_trimmed.is_empty() {
                // Regular arm-body code line — not a pattern continuation.
                pending_lhs.clear();
            }
            // Empty/whitespace-only lines: leave `pending_lhs` unchanged.
        }

        keys
    }

    /// Returns `true` if `trimmed` consists only of string literals, `|` operators,
    /// and whitespace — i.e. it is a pure match-pattern fragment with no `=>`.
    ///
    /// Used to detect the first and middle lines of a rustfmt-wrapped multi-arm
    /// pattern:
    /// ```text
    ///     "line_segment"      ← is_or_pattern_line → true, accumulate
    ///         | "arc"         ← is_or_pattern_line → true, accumulate
    ///         | "helix" => {  ← contains `=>`, flush + process
    /// ```
    fn is_or_pattern_line(trimmed: &str) -> bool {
        let mut s = trimmed;
        if s.starts_with('|') {
            s = s[1..].trim_start();
        }
        let mut chars = s.chars().peekable();
        let mut found_string = false;
        while let Some(&c) = chars.peek() {
            match c {
                '"' => {
                    chars.next();
                    chars.by_ref().take_while(|&x| x != '"').for_each(drop);
                    found_string = true;
                }
                '|' | ' ' | '\t' => {
                    chars.next();
                }
                _ => return false,
            }
        }
        found_string
    }

    /// Strip a trailing `//` inline comment from `line`, but only outside string
    /// literals.  Returns the portion of the line before the `//`.
    ///
    /// Limitation: does not handle `/*` block comments or char literals.  Use
    /// [`strip_block_comments`] as a pre-pass for block-comment removal.
    fn strip_inline_comment(line: &str) -> &str {
        let bytes = line.as_bytes();
        let mut in_string = false;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => in_string = !in_string,
                b'\\' if in_string => {
                    // Skip the next byte (part of an escape sequence) without
                    // flipping `in_string`.
                    if i + 1 < bytes.len() {
                        i += 1;
                    }
                }
                b'/' if !in_string
                    && i + 1 < bytes.len()
                    && bytes[i + 1] == b'/' =>
                {
                    return &line[..i];
                }
                _ => {}
            }
            i += 1;
        }
        line
    }

    /// Replace all `/* ... */` block comments in `s` with spaces, preserving
    /// newlines so that line numbers remain stable for the subsequent line scan.
    ///
    /// Does not handle `/*` inside string literals (not needed for
    /// `compile_geometry_call` match-arm patterns).
    fn strip_block_comments(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        let mut in_block = false;
        while let Some(c) = chars.next() {
            if in_block {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next(); // consume '/'
                    out.push_str("  ");
                    in_block = false;
                } else {
                    // Preserve newlines; replace everything else with a space.
                    out.push(if c == '\n' { '\n' } else { ' ' });
                }
            } else if c == '/' && chars.peek() == Some(&'*') {
                chars.next(); // consume '*'
                out.push_str("  ");
                in_block = true;
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn extract_dispatch_keys_from_source_parses_match_arms() {
        // Synthetic source exercising the five original shapes plus the three
        // edge cases most likely to regress in practice.
        let source = r#"
fn other_fn(name: &str) {
    match name {
        "should_not_appear" => {}
        _ => {}
    }
}

pub(crate) fn target_fn(x: &str) {
    if let Expr::Ident(name) = x {
        return;
    }
    // Original shapes: single-arm, multi-arm, wildcard, destructure, nested match.
    match x {
        "foo" => { do_something(); }
        "bar" | "baz" | "qux" => { do_other(); }
        _ => { fallback(); }
    }
    let inner = match x {
        "inner_key" => 1,
        _ => 2,
    };
    match complicated {
        Ident(name) => handle(name),
        _ => {}
    }
    // (a) rustfmt-wrapped multi-arm or-pattern: continuation lines begin with `|`.
    // All three keys must be extracted even though only the last line has `=>`.
    match x {
        "wrapped_a"
            | "wrapped_b"
            | "wrapped_c" => { do_wrapped(); }
        _ => {}
    }
    // (b) Trailing `//` comment on an arm-body line containing `"spurious" =>`.
    // The comment must be stripped so `spurious` is NOT extracted as a dispatch key.
    match x {
        "legit" => {
            let _ = (); // "spurious" => should NOT be extracted
        }
        _ => {}
    }
    // (c) `/* ... */` block comment containing `"block_key" =>`.
    // The block comment must be stripped so `block_key` is NOT extracted.
    /* "block_key" => inside a block comment, must not appear */
    match x {
        "after_block_comment" => { after(); }
        _ => {}
    }
}

fn after_fn(name: &str) {
    match name {
        "also_should_not_appear" => {}
        _ => {}
    }
}
"#;
        let extracted = extract_dispatch_keys_from_source(source, "pub(crate) fn target_fn(");
        let expected: HashSet<String> = [
            // original shapes
            "foo", "bar", "baz", "qux", "inner_key",
            // (a) wrapped or-pattern — all three keys must be present
            "wrapped_a", "wrapped_b", "wrapped_c",
            // (b) legit dispatch arm (`spurious` must NOT appear)
            "legit",
            // (c) real arm after block comment (`block_key` must NOT appear)
            "after_block_comment",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            extracted,
            expected,
            "extracted keys did not match expected set.\n\
             False positives (in extracted, not in expected): {:?}\n\
             Missed keys (in expected, not in extracted): {:?}",
            extracted.difference(&expected).collect::<Vec<_>>(),
            expected.difference(&extracted).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn compile_geometry_call_source_arms_match_registry_lists() {
        let source = include_str!("geometry.rs");
        let extracted =
            extract_dispatch_keys_from_source(source, "pub(crate) fn compile_geometry_call(");

        let boolean_ops: &[&str] =
            &["union", "intersection", "difference", "union_all", "intersection_all"];
        let loft: &[&str] = &["loft"];

        let expected: HashSet<String> = GEOM_ARG_FUNCTIONS
            .iter()
            .chain(NO_GEOM_ARG_FUNCTIONS.iter())
            .chain(boolean_ops.iter())
            .chain(loft.iter())
            .map(|s| s.to_string())
            .collect();

        assert_eq!(
            extracted,
            expected,
            "match-arm keys extracted from compile_geometry_call source do not match registry lists.\n\
             In source but not in lists: {:?}\n\
             In lists but not in source: {:?}",
            extracted.difference(&expected).collect::<Vec<_>>(),
            expected.difference(&extracted).collect::<Vec<_>>(),
        );
    }
}

