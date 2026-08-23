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
    arg: &reify_ast::Expr,
    op_name: &str,
    arg_idx_for_diag: usize,
    scope: &CompilationScope,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    step_offset: usize,
    geometry_lets: &HashMap<&str, &reify_ast::Expr>,
    visiting: &mut HashSet<String>,
    constraint_sink: &mut GeometryConstraintSink<'_>,
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
        constraint_sink,
    ) {
        Some(ops) => ops,
        None => {
            // Only emit the fallback diagnostic when the arg is not itself a
            // shape `compile_geometry_call` would have flagged (FunctionCall
            // or geometry-let Ident).  Preserves the pre-extraction call-site
            // diagnostic semantics.
            if !matches!(arg.kind, reify_ast::ExprKind::FunctionCall { .. })
                && !matches!(
                    &arg.kind,
                    reify_ast::ExprKind::Ident(n) if geometry_lets.contains_key(n.as_str())
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
    args: &[reify_ast::Expr],
    expr_span: SourceSpan,
    scope: &CompilationScope,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    step_offset: usize,
    geometry_lets: &HashMap<&str, &reify_ast::Expr>,
    visiting: &mut HashSet<String>,
    constraint_sink: &mut GeometryConstraintSink<'_>,
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
                &mut constraint_sink.reborrow(),
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
                &mut constraint_sink.reborrow(),
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
            // Task #5385: a SINGLE `List<Geometry>` argument expands to its
            // element expressions here, BEFORE the arity gate, and then runs
            // the existing left-fold verbatim — no new fold logic, no IR
            // change. Each element compiles inline exactly as it does today
            // for `union(a, b)` over geometry lets.
            //
            // That inline re-compilation means an element's geometry is built
            // twice: once for its own `<list>#k` realization and once inside
            // the fold. Reviewed (esc-5385-3) and kept deliberately, because
            // it is NOT a deviation this task introduced — `compile_geometry_call`'s
            // `Ident` arm (geometry.rs) recursively compiles a geometry let's
            // INITIALIZER, so `union(a, b)` over two geometry lets already
            // duplicates both operands the same way. The zero-op `GeomRef::Sub`
            // fast path in `resolve_boolean_arg` matches only the cross-sub
            // `self.<sub>.<member>` shape, never a sibling let. Referencing the
            // already-emitted `<list>#k` realizations instead is a worthwhile
            // change to the whole boolean-arg path — and only worthwhile there,
            // since scoping it to geometry lists alone would leave the identical
            // duplication in place for every other operand shape.
            let expanded: std::rc::Rc<Vec<reify_ast::Expr>>;
            let mut args = args;
            let mut expanded_from_list = false;
            if args.len() == 1 {
                match resolve_geometry_list_arg(&args[0], scope, functions) {
                    GeometryListArg::Elements(elements) => {
                        if elements.is_empty() {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "{name}() over an empty geometry list has nothing \
                                     to fold; it needs at least one element"
                                ))
                                .with_label(DiagnosticLabel::new(
                                    args[0].span,
                                    "this geometry list is empty",
                                )),
                            );
                            return None;
                        }
                        expanded = elements;
                        args = &expanded;
                        expanded_from_list = true;
                    }
                    GeometryListArg::NotGeometry => {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "{name}()'s single argument must be a geometry list \
                                 (a list literal of geometry, or generate(<literal>, \
                                 |i| <geometry>))"
                            ))
                            .with_label(DiagnosticLabel::new(
                                args[0].span,
                                "this collection's elements are not geometry",
                            )),
                        );
                        return None;
                    }
                    // An INLINE list over the element cap (review esc-5385-6).
                    // It has no declaring let to own the cap Error, so report it
                    // here — and as a cap problem: its elements ARE geometry, so
                    // the `NotGeometry` label above would send the user looking
                    // for a type error that does not exist.
                    GeometryListArg::OverCap { subject, count } => {
                        push_element_cap_error(subject, count, args[0].span, diagnostics);
                        return None;
                    }
                    // The declaring let already reported its own Error; a
                    // second one here would point at the fold rather than at
                    // the real defect (review esc-5385-3).
                    GeometryListArg::AlreadyDiagnosed => return None,
                    // Not a collection at all (e.g. `union_all(box(…))`) — fall
                    // through to the unchanged arity diagnostic below.
                    GeometryListArg::NotAList => {}
                }
            }
            // A geometry list MIXED INTO a multi-argument fold (review
            // esc-5385-7): `union_all(holes, box(1mm,1mm,1mm))` clears the >= 2
            // arity gate, then `resolve_boolean_arg` reaches
            // `compile_geometry_call`'s `Ident` arm, which returns `None` with NO
            // diagnostic for a name absent from `geometry_lets`. The enclosing
            // let then emits no realization and no error whatsoever. The
            // behaviour predates this task, but this task is what makes `holes` a
            // plausible thing to write there, so say so rather than lower nothing
            // silently.
            //
            // Diagnose rather than flatten: splicing a list into a
            // partially-written fold guesses at an ordering the user did not
            // write, and the fold is not commutative for `difference`-shaped
            // future operators. `NotGeometry` / `NotAList` are deliberately NOT
            // caught — a non-geometry collection here keeps its existing
            // behaviour, so this arm can only fire on an argument the
            // single-argument form would have accepted.
            //
            // Cost is bounded: `resolve_geometry_list_arg` returns `NotAList`
            // immediately for anything that is not an `Ident`, a `ListLiteral` or
            // a `generate(…)` call, so an ordinary `union_all(a, b, c)` pays one
            // cheap scope lookup per argument and expands nothing.
            if !expanded_from_list && args.len() > 1 {
                for arg in args {
                    match resolve_geometry_list_arg(arg, scope, functions) {
                        GeometryListArg::Elements(_) | GeometryListArg::OverCap { .. } => {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "{name}() takes a geometry list only as its SOLE \
                                     argument; fold this list on its own, or write out \
                                     its elements alongside the other arguments"
                                ))
                                .with_label(DiagnosticLabel::new(
                                    arg.span,
                                    "this geometry list is mixed with other arguments",
                                )),
                            );
                            return None;
                        }
                        // The declaring let already reported its own Error.
                        GeometryListArg::AlreadyDiagnosed => return None,
                        GeometryListArg::NotGeometry | GeometryListArg::NotAList => {}
                    }
                }
            }
            // A list that expanded to exactly ONE element folds to that element
            // with zero Boolean ops, which is well-defined — so the >= 2 gate
            // applies only to a literally-written argument list. The empty case
            // was already rejected above with its own specific message.
            if !expanded_from_list
                && !check_arg_count_at_least(name, args.len(), 2, expr_span, diagnostics)
            {
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
                &mut constraint_sink.reborrow(),
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
                    &mut constraint_sink.reborrow(),
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
