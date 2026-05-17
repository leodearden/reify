use super::*;

/// Resolve a single boolean-op geometry argument into its `GeomRef` and the
/// sub-ops that must be emitted before the binary `Boolean` op.
///
/// Encapsulates the "cross-sub pre-check OR fall back to recursive
/// `compile_geometry_call`" branch that previously appeared four times in
/// this file (left/right of binary ops; first arg + loop iter of n-ary ops).
/// Extracted (amendment) so the cross-sub fast path lives in one place and
/// future changes — e.g. recognising geometry-let `Ident`s earlier — become
/// a one-line patch.
///
/// Returns:
/// - `Some((GeomRef::Sub(...), vec![]))` when the arg is `self.<sub>.<member>`
///   that the cross-sub pre-check recognises.  No sub-ops are emitted; the
///   eval side seeds `named_steps["<sub>.<member>"]` (task 3441).
/// - `Some((GeomRef::Step(step), ops))` when the arg compiles to a regular
///   sequence of `ops`; `step` indexes the final result inside
///   `step_offset + ops`.
/// - `None` on error.  An "argument N must be a geometry expression"
///   diagnostic is emitted **only when** `compile_geometry_call` did not
///   already emit one (i.e. when the arg is neither a `FunctionCall` nor an
///   `Ident` naming a geometry-let).  Matches the prior call-site semantics.
///
/// `arg_idx_for_diag` is the 1-based position of `arg` in the surrounding
/// boolean op's argument list — used purely for the fallback diagnostic.
#[allow(clippy::too_many_arguments)]
fn resolve_boolean_arg(
    arg: &reify_syntax::Expr,
    op_name: &str,
    arg_idx_for_diag: usize,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    step_offset: usize,
    geometry_lets: &HashMap<&str, &reify_syntax::Expr>,
    visiting: &mut HashSet<String>,
) -> Option<(GeomRef, Vec<CompiledGeometryOp>)> {
    // Task 3441: cross-sub pre-check — `self.<sub>.<member>` for a
    // non-collection sub's realised geometry member lowers to a
    // `GeomRef::Sub` with no sub-op accumulation.
    if let Some(sub_ref) = try_resolve_cross_sub_geom_ref(arg, scope) {
        return Some((sub_ref, Vec::new()));
    }
    // Task 3512: near-miss cross-sub routing — when the working path returned
    // None (e.g. because `<sub>` is a collection sub), pattern-match the
    // `self.<sub>.<member>` shape and route through `try_emit_cross_sub_geometry`
    // to emit the specific v0.1 deferred diagnostic naming the sub and member,
    // rather than falling through to the generic "argument N must be a geometry
    // expression" fallback.
    //
    // Mirrors the value-level call sites at expr.rs:1307 (bare collection sub)
    // and expr.rs:1562 (indexed collection sub) that already use this helper.
    // The returned `Option<CompiledExpr>` is consumed only for its is_some()
    // signal — the CompiledExpr value is discarded because boolean-arg position
    // needs a GeomRef, not a CompiledExpr (task 3512 design decision).
    //
    // Scope note: only the `self.<sub>.<member>` two-level MemberAccess shape is
    // matched here.  Indexed forms such as `self.<sub>[i].<member>` (where the
    // outer object is an IndexAccess rather than a MemberAccess) are intentionally
    // out of scope for task 3512 and fall through to the generic diagnostic.
    // Extending boolean-arg routing to that shape is a post-3512 follow-up.
    if let Some((sub_name, member)) = match_self_sub_member(arg)
        && try_emit_cross_sub_geometry(scope, sub_name, member, arg.span, diagnostics).is_some()
    {
        // Specific deferred diagnostic emitted; skip generic fallback.
        return None;
    }
    // Helper returned None: member is not a geometry realization on this sub
    // (e.g. scalar param), or the arg is not a self.<sub>.<member> shape.
    // Fall through to compile_geometry_call + generic fallback so the existing
    // "must be a geometry expression" message fires correctly for scalar-member
    // shapes.
    let ops = match compile_geometry_call(
        arg,
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
            // Only emit the fallback diagnostic when the arg is not itself a
            // shape `compile_geometry_call` would have flagged (FunctionCall
            // or geometry-let Ident).  Preserves the pre-extraction call-site
            // diagnostic semantics.
            if !matches!(arg.kind, reify_syntax::ExprKind::FunctionCall { .. })
                && !matches!(
                    &arg.kind,
                    reify_syntax::ExprKind::Ident(n) if geometry_lets.contains_key(n.as_str())
                )
            {
                diagnostics.push(Diagnostic::error(format!(
                    "{}() argument {} must be a geometry expression",
                    op_name, arg_idx_for_diag
                )));
            }
            return None;
        }
    };
    let step = step_offset + ops.len() - 1;
    Some((GeomRef::Step(step), ops))
}

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
            // Resolve left arg via the shared helper (task 3441 cross-sub
            // pre-check + recursive compile fallback).
            let (left_geom_ref, left_ops) = resolve_boolean_arg(
                &args[0],
                name,
                1,
                scope,
                enum_defs,
                functions,
                diagnostics,
                step_offset,
                geometry_lets,
                visiting,
            )?;
            let right_offset = step_offset + left_ops.len();
            // Resolve right arg via the same helper.
            let (right_geom_ref, right_ops) = resolve_boolean_arg(
                &args[1],
                name,
                2,
                scope,
                enum_defs,
                functions,
                diagnostics,
                right_offset,
                geometry_lets,
                visiting,
            )?;
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
            // Task 3441: each arg first goes through the cross-sub pre-check
            // inside `resolve_boolean_arg`; when it matches `self.<sub>.<member>`,
            // we record a `GeomRef::Sub` and emit zero sub-ops (so
            // `current_offset` is unchanged for that arg).  Only on the binary
            // Boolean op emission does the accumulator advance by 1.
            let mut all_ops: Vec<CompiledGeometryOp> = Vec::new();
            let mut current_offset = step_offset;

            // Resolve first arg.
            let (first_geom_ref, first_ops) = resolve_boolean_arg(
                &args[0],
                name,
                1,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_offset,
                geometry_lets,
                visiting,
            )?;
            current_offset += first_ops.len();
            all_ops.extend(first_ops);
            let mut accumulator_ref: GeomRef = first_geom_ref;

            // Fold remaining args left-to-right.
            for (i, arg) in args.iter().enumerate().skip(1) {
                let (arg_geom_ref, arg_ops) = resolve_boolean_arg(
                    arg,
                    name,
                    i + 1,
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_offset,
                    geometry_lets,
                    visiting,
                )?;
                current_offset += arg_ops.len();
                all_ops.extend(arg_ops);
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
