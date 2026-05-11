use super::*;

/// Compile a boolean geometry operation into CompiledGeometryOps.
///
/// Boolean ops (union, intersection, difference, union_all, intersection_all)
/// recursively compile their sub-expressions and need the full compilation context.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_boolean_op(
    name: &str,
    args: &[reify_syntax::Expr],
    expr_span: SourceSpan,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    step_offset: usize,
    geometry_lets: &HashMap<&str, &reify_syntax::Expr>,
    visiting: &mut HashSet<String>,
) -> Option<Vec<CompiledGeometryOp>> {
    match name {
        "union" | "intersection" | "difference" => {
            if !check_arg_count_exact(name, args.len(), 2, expr_span, diagnostics) {
                return None;
            }
            let bool_op = match name {
                "union" => BooleanOp::Union,
                "intersection" => BooleanOp::Intersection,
                "difference" => BooleanOp::Difference,
                _ => unreachable!(),
            };
            // Resolve left arg.  Task 3441: cross-sub geometry pre-check —
            // if the arg is `self.<sub>.<member>` for a non-collection sub's
            // realised geometry member, lower it to a `GeomRef::Sub` with no
            // sub-op accumulation (the eval side seeds `named_steps` with the
            // compound key).  Otherwise fall through to the recursive
            // `compile_geometry_call` path.
            let (left_geom_ref, left_ops): (GeomRef, Vec<CompiledGeometryOp>) =
                if let Some(sub_ref) = try_resolve_cross_sub_geom_ref(&args[0], scope) {
                    (sub_ref, Vec::new())
                } else {
                    let ops = match compile_geometry_call(
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
                    let step = step_offset + ops.len() - 1;
                    (GeomRef::Step(step), ops)
                };
            let right_offset = step_offset + left_ops.len();
            // Resolve right arg with the same cross-sub pre-check.
            let (right_geom_ref, right_ops): (GeomRef, Vec<CompiledGeometryOp>) =
                if let Some(sub_ref) = try_resolve_cross_sub_geom_ref(&args[1], scope) {
                    (sub_ref, Vec::new())
                } else {
                    let ops = match compile_geometry_call(
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
                    let step = right_offset + ops.len() - 1;
                    (GeomRef::Step(step), ops)
                };
            let mut all_ops = left_ops;
            all_ops.extend(right_ops);
            all_ops.push(CompiledGeometryOp::Boolean {
                op: bool_op,
                left: left_geom_ref,
                right: right_geom_ref,
            });
            Some(all_ops)
        }
        "union_all" | "intersection_all" => {
            if !check_arg_count_at_least(name, args.len(), 2, expr_span, diagnostics) {
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
            //
            // Task 3441: each arg first goes through the cross-sub pre-check;
            // when it matches `self.<sub>.<member>`, we record a `GeomRef::Sub`
            // and emit zero sub-ops (so `current_offset` is unchanged for that
            // arg).  Only on the binary Boolean op emission does the
            // accumulator advance by 1.
            let mut all_ops: Vec<CompiledGeometryOp> = Vec::new();
            let mut current_offset = step_offset;

            // Resolve first arg.
            let first_geom_ref: GeomRef = if let Some(sub_ref) =
                try_resolve_cross_sub_geom_ref(&args[0], scope)
            {
                sub_ref
            } else {
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
                let step = current_offset + first_ops.len() - 1;
                current_offset += first_ops.len();
                all_ops.extend(first_ops);
                GeomRef::Step(step)
            };
            let mut accumulator_ref: GeomRef = first_geom_ref;

            // Fold remaining args left-to-right.
            for (i, arg) in args.iter().enumerate().skip(1) {
                let arg_geom_ref: GeomRef = if let Some(sub_ref) =
                    try_resolve_cross_sub_geom_ref(arg, scope)
                {
                    sub_ref
                } else {
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
                    let step = current_offset + arg_ops.len() - 1;
                    current_offset += arg_ops.len();
                    all_ops.extend(arg_ops);
                    GeomRef::Step(step)
                };
                // Emit binary op: (accumulator, arg) → new accumulator at current_offset.
                all_ops.push(CompiledGeometryOp::Boolean {
                    op: bool_op,
                    left: accumulator_ref,
                    right: arg_geom_ref,
                });
                accumulator_ref = GeomRef::Step(current_offset);
                current_offset += 1;
            }
            Some(all_ops)
        }
        _ => unreachable!("compile_boolean_op called with non-boolean name: {}", name),
    }
}
