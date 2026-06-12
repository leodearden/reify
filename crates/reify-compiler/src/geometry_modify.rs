use super::*;

/// Shared helper for 2-argument modify operations (thicken, chamfer, fillet).
///
/// Validates that exactly 2 arguments were provided, emits a labeled diagnostic if not,
/// then builds `CompiledGeometryOp::Modify` with args `[("target", ...), (arg2_name, ...)]`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_modify_2arg(
    name: &str,
    kind: ModifyKind,
    arg2_name: &str,
    compiled_args: Vec<CompiledExpr>,
    target: GeomRef,
    expr_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
    mut sub_ops: Vec<CompiledGeometryOp>,
) -> Option<Vec<CompiledGeometryOp>> {
    if !check_arg_count_exact(name, compiled_args.len(), 2, expr_span, diagnostics) {
        return None;
    }
    let mut it = compiled_args.into_iter();
    let op = CompiledGeometryOp::Modify {
        kind,
        target,
        args: vec![
            ("target".to_string(), it.next().unwrap()),
            (arg2_name.to_string(), it.next().unwrap()),
        ],
    };
    sub_ops.push(op);
    Some(sub_ops)
}

/// Compile a modify operation into CompiledGeometryOps.
///
/// Takes pre-resolved target GeomRef, expr_span for diagnostics, and pre-accumulated sub_ops.
///
/// All arms use the passed `target`, push to sub_ops, and return Some(sub_ops).
///
/// chamfer/fillet are registered in geometry_arg_indices() alongside
/// shell/thicken/draft, so their target is resolved via geom_ref(0) in the caller
/// (geometry.rs). This function honours whatever target it receives.
pub(crate) fn compile_modify_op(
    name: &str,
    compiled_args: Vec<CompiledExpr>,
    target: GeomRef,
    expr_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
    mut sub_ops: Vec<CompiledGeometryOp>,
) -> Option<Vec<CompiledGeometryOp>> {
    match name {
        // shell(target, thickness, ...)
        "shell" => {
            if !check_arg_count_at_least("shell", compiled_args.len(), 2, expr_span, diagnostics) {
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
            let op = CompiledGeometryOp::Modify {
                kind: ModifyKind::Shell,
                target,
                args: shell_args,
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // thicken(target, offset)
        "thicken" => compile_modify_2arg(
            "thicken",
            ModifyKind::Thicken,
            "offset",
            compiled_args,
            target,
            expr_span,
            diagnostics,
            sub_ops,
        ),
        // zone_slab(face, width) — offset face ±w/2 and cap into a centered slab solid
        "zone_slab" => compile_modify_2arg(
            "zone_slab",
            ModifyKind::ZoneSlab,
            "width",
            compiled_args,
            target,
            expr_span,
            diagnostics,
            sub_ops,
        ),
        // offset_solid(target, distance)
        "offset_solid" => compile_modify_2arg(
            "offset_solid",
            ModifyKind::OffsetSolid,
            "distance",
            compiled_args,
            target,
            expr_span,
            diagnostics,
            sub_ops,
        ),
        // draft(target, angle, plane)
        "draft" => {
            if !check_arg_count_exact("draft", compiled_args.len(), 3, expr_span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
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
        // chamfer(target, distance)
        "chamfer" => compile_modify_2arg(
            "chamfer",
            ModifyKind::Chamfer,
            "distance",
            compiled_args,
            target,
            expr_span,
            diagnostics,
            sub_ops,
        ),
        // fillet(target, radius)             — 2-arg all-edges back-compat
        // fillet(target, edges, radius)      — 3-arg curated edge selection
        "fillet" => match compiled_args.len() {
            2 => compile_modify_2arg(
                "fillet",
                ModifyKind::Fillet,
                "radius",
                compiled_args,
                target,
                expr_span,
                diagnostics,
                sub_ops,
            ),
            3 => {
                let mut it = compiled_args.into_iter();
                let op = CompiledGeometryOp::Modify {
                    kind: ModifyKind::Fillet,
                    target,
                    args: vec![
                        ("target".to_string(), it.next().unwrap()),
                        ("edges".to_string(), it.next().unwrap()),
                        ("radius".to_string(), it.next().unwrap()),
                    ],
                };
                sub_ops.push(op);
                Some(sub_ops)
            }
            // No range-arity helper exists (only exact/at_least), so emit a labeled
            // diagnostic mirroring check_arg_count_*'s format. fillet accepts only the
            // 2-arg all-edges form or the 3-arg curated-edges form.
            got => {
                diagnostics.push(
                    Diagnostic::error(format!("fillet() expects 2 or 3 arguments, got {got}"))
                        .with_label(DiagnosticLabel::new(expr_span, "wrong number of arguments")),
                );
                None
            }
        },
        // fillet_all(target, radius) — all-edges alias: identical to 2-arg fillet.
        // Uses compile_modify_2arg with ModifyKind::Fillet → CompiledGeometryOp::Modify{Fillet}
        // with NO "edges" arg, so it reaches the same eval None-edges branch as 2-arg fillet.
        "fillet_all" => compile_modify_2arg(
            "fillet_all",
            ModifyKind::Fillet,
            "radius",
            compiled_args,
            target,
            expr_span,
            diagnostics,
            sub_ops,
        ),
        _ => unreachable!("compile_modify_op called with non-modify name: {}", name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar_literal(v: f64) -> CompiledExpr {
        CompiledExpr::literal(Value::Real(v), Type::dimensionless_scalar())
    }

    #[test]
    fn compile_modify_2arg_chamfer_builds_correct_args() {
        let args: Vec<CompiledExpr> = vec![scalar_literal(1.0), scalar_literal(2.0)];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(5);
        let span = SourceSpan::new(0, 0);
        let result = compile_modify_2arg(
            "chamfer",
            ModifyKind::Chamfer,
            "distance",
            args,
            target.clone(),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );
        let ops = result.expect("compile_modify_2arg should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Chamfer,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "distance"]);
            }
            other => panic!("expected Modify(Chamfer), got {:?}", other),
        }
    }

    #[test]
    fn compile_modify_2arg_thicken_builds_correct_args() {
        let args: Vec<CompiledExpr> = vec![scalar_literal(1.0), scalar_literal(2.0)];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(3);
        let span = SourceSpan::new(0, 0);
        let result = compile_modify_2arg(
            "thicken",
            ModifyKind::Thicken,
            "offset",
            args,
            target.clone(),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );
        let ops = result.expect("compile_modify_2arg should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Thicken,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "offset"]);
            }
            other => panic!("expected Modify(Thicken), got {:?}", other),
        }
    }

    #[test]
    fn compile_modify_2arg_fillet_builds_correct_args() {
        let args: Vec<CompiledExpr> = vec![scalar_literal(1.0), scalar_literal(2.0)];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(7);
        let span = SourceSpan::new(0, 0);
        let result = compile_modify_2arg(
            "fillet",
            ModifyKind::Fillet,
            "radius",
            args,
            target.clone(),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );
        let ops = result.expect("compile_modify_2arg should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Fillet,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "radius"]);
            }
            other => panic!("expected Modify(Fillet), got {:?}", other),
        }
    }

    /// 3-arg `fillet(solid, edges, radius)` is recognised by `compile_modify_op`
    /// and lowered to named args `[target, edges, radius]` (curated edge selection).
    #[test]
    fn compile_modify_op_fillet_3arg_builds_curated_edge_args() {
        let args: Vec<CompiledExpr> =
            vec![scalar_literal(1.0), scalar_literal(2.0), scalar_literal(3.0)];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(7);
        let span = SourceSpan::new(0, 0);
        let result =
            compile_modify_op("fillet", args, target.clone(), span, &mut diagnostics, vec![]);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );
        let ops = result.expect("compile_modify_op fillet (3-arg) should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Fillet,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "edges", "radius"]);
            }
            other => panic!("expected Modify(Fillet) with 3 args, got {:?}", other),
        }
    }

    /// 2-arg `fillet(solid, radius)` through `compile_modify_op` is unchanged
    /// (back-compat): named args `[target, radius]`, no `edges` slot.
    #[test]
    fn compile_modify_op_fillet_2arg_back_compat_through_dispatcher() {
        let args: Vec<CompiledExpr> = vec![scalar_literal(1.0), scalar_literal(2.0)];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(7);
        let span = SourceSpan::new(0, 0);
        let result =
            compile_modify_op("fillet", args, target.clone(), span, &mut diagnostics, vec![]);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );
        let ops = result.expect("compile_modify_op fillet (2-arg) should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Fillet,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "radius"]);
            }
            other => panic!("expected Modify(Fillet) with 2 args, got {:?}", other),
        }
    }

    /// `fillet` accepts only 2 or 3 args: a 1-arg and a 4-arg call each return
    /// None and emit ≥1 arity diagnostic.
    #[test]
    fn compile_modify_op_fillet_rejects_1arg_and_4arg() {
        let span = SourceSpan::new(10, 20);
        // 1 arg → None + ≥1 diagnostic
        {
            let args: Vec<CompiledExpr> = vec![scalar_literal(1.0)];
            let mut diagnostics: Vec<Diagnostic> = vec![];
            let result =
                compile_modify_op("fillet", args, GeomRef::Step(0), span, &mut diagnostics, vec![]);
            assert!(result.is_none(), "expected None for 1-arg fillet");
            assert!(
                !diagnostics.is_empty(),
                "expected at least one diagnostic for 1-arg fillet"
            );
        }
        // 4 args → None + ≥1 diagnostic
        {
            let args: Vec<CompiledExpr> = vec![
                scalar_literal(1.0),
                scalar_literal(2.0),
                scalar_literal(3.0),
                scalar_literal(4.0),
            ];
            let mut diagnostics: Vec<Diagnostic> = vec![];
            let result =
                compile_modify_op("fillet", args, GeomRef::Step(0), span, &mut diagnostics, vec![]);
            assert!(result.is_none(), "expected None for 4-arg fillet");
            assert!(
                !diagnostics.is_empty(),
                "expected at least one diagnostic for 4-arg fillet"
            );
        }
    }

    #[test]
    fn compile_modify_2arg_rejects_wrong_arg_count_with_label() {
        let args: Vec<CompiledExpr> = vec![scalar_literal(1.0)]; // only 1 arg, need 2
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(5);
        let span = SourceSpan::new(10, 20);
        let result = compile_modify_2arg(
            "chamfer",
            ModifyKind::Chamfer,
            "distance",
            args,
            target,
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(result.is_none(), "expected None for wrong arg count");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(
            diagnostics[0]
                .message
                .contains("chamfer() expects 2 arguments, got 1"),
            "unexpected message: {:?}",
            diagnostics[0].message
        );
        assert!(
            !diagnostics[0].labels.is_empty(),
            "expected at least one label on arg-count diagnostic"
        );
        assert!(
            !diagnostics[0].labels[0].span.is_empty(),
            "expected non-empty span on arg-count label"
        );
    }

    #[test]
    fn compile_modify_op_shell_direct() {
        // shell(target, thickness, face_0) — 3 args, target = GeomRef::Step(5)
        let args: Vec<CompiledExpr> = (1..=3).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(5);
        let span = SourceSpan::new(0, 0);
        let result = compile_modify_op(
            "shell",
            args,
            target.clone(),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );
        let ops = result.expect("compile_modify_op shell should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Shell,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "thickness", "face_0"]);
            }
            other => panic!("expected Modify(Shell), got {:?}", other),
        }
    }

    /// Assert that `fn_name(target, tail_arg_names[0], tail_arg_names[1], ...)` with a scalar
    /// `target` param falls back to `GeomRef::Step(0)` (the step_offset when there are no
    /// prior sub-ops). Each name in `tail_arg_names` becomes a `param <name>: Length` in the
    /// generated source; the call is `fn_name(target, name0, name1, ...)`.
    ///
    /// Tail arg values are assigned uniform `Scalar = (i+2)mm` literals regardless of
    /// real-world semantics (e.g. `draft`'s `angle` would normally be dimensionless and
    /// `plane` would be a geometry).  This is intentional: the fallback path under test is
    /// triggered by `target` not being a geometry ref; arg types for tail parameters do not
    /// affect that path.
    fn assert_non_geometry_target_fallback(
        kind: ModifyKind,
        fn_name: &str,
        tail_arg_names: &[&str],
    ) {
        let param_decls: String = tail_arg_names
            .iter()
            .enumerate()
            .map(|(i, name)| format!("    param {name}: Length = {}mm\n", i + 2))
            .collect();
        let tail_call = tail_arg_names.join(", ");
        let source = format!(
            "structure S {{\n    param target: Length = 5mm\n{decls}    let result = {f}(target, {tail})\n}}",
            f = fn_name,
            decls = param_decls,
            tail = tail_call,
        );
        let parsed = reify_syntax::parse(
            &source,
            reify_core::ModulePath::single(format!("test_fallback_{}", fn_name)),
        );
        assert!(
            parsed.errors.is_empty(),
            "{}(): parse errors: {:?}",
            fn_name,
            parsed.errors
        );
        let compiled = crate::compile(&parsed);
        assert!(
            compiled.diagnostics.is_empty(),
            "{}(): unexpected diagnostics: {:?}",
            fn_name,
            compiled.diagnostics
        );
        let ops = &compiled.templates[0].realizations[0].operations;
        assert_eq!(
            ops.len(),
            1,
            "{}(): expected 1 op, got {}",
            fn_name,
            ops.len()
        );
        match &ops[0] {
            CompiledGeometryOp::Modify {
                kind: op_kind,
                target: op_target,
                ..
            } => {
                assert_eq!(
                    *op_kind, kind,
                    "{}(): expected {:?}, got {:?}",
                    fn_name, kind, op_kind
                );
                assert_eq!(
                    *op_target,
                    GeomRef::Step(0),
                    "{}(): non-geometry target should fall back to GeomRef::Step(0), got {:?}",
                    fn_name,
                    op_target
                );
            }
            other => panic!(
                "{}(): expected Modify({:?}), got {:?}",
                fn_name, kind, other
            ),
        }
    }

    /// Returns the canonical test-table for all single-geometry-target modify kinds:
    /// `(ModifyKind, fn_name, tail_arg_names)`.
    ///
    /// ## Exhaustiveness guarantees
    ///
    /// **First-line tripwire** — the no-wildcard closure below has no wildcard arm.  Adding a new
    /// `ModifyKind` variant causes a compile error here, drawing the author's attention before
    /// the new kind can silently escape regression coverage.
    ///
    /// **Compile-time coverage lock** — `const _: () = assert!(CASES.len() ==
    /// ModifyKind::VARIANT_COUNT, ...)` immediately after the `CASES` declaration makes it a hard
    /// compile error (caught at `cargo check`) to add a variant without also extending `CASES`.
    /// The pattern follows `crates/reify-kernel-occt/src/lib.rs:36` which uses the same idiom to
    /// pin a Rust/C++ floor-constant invariant.
    fn single_geom_target_kinds() -> &'static [(ModifyKind, &'static str, &'static [&'static str])]
    {
        static CASES: &[(ModifyKind, &str, &[&str])] = &[
            (ModifyKind::Chamfer, "chamfer", &["distance"]),
            (ModifyKind::Fillet, "fillet", &["radius"]),
            (ModifyKind::Thicken, "thicken", &["offset"]),
            (ModifyKind::Shell, "shell", &["thickness"]),
            (ModifyKind::Draft, "draft", &["angle", "plane"]),
            (ModifyKind::ZoneSlab, "zone_slab", &["width"]),
            (ModifyKind::OffsetSolid, "offset_solid", &["distance"]),
        ];
        // Compile-time coverage lock: if CASES.len() ever falls out of step with
        // ModifyKind::VARIANT_COUNT, `cargo check` fails here before any test runs.
        const _: () = assert!(
            CASES.len() == ModifyKind::VARIANT_COUNT,
            "CASES table must cover every ModifyKind variant \
             — bump ModifyKind::VARIANT_COUNT and add the new entry to CASES together"
        );
        // Exhaustiveness sentinel (first-line tripwire): no wildcard arm ensures a new
        // ModifyKind variant causes a compile error here, requiring an explicit update.
        let _ = |k: ModifyKind| match k {
            ModifyKind::Chamfer
            | ModifyKind::Fillet
            | ModifyKind::Thicken
            | ModifyKind::Shell
            | ModifyKind::Draft
            | ModifyKind::ZoneSlab
            | ModifyKind::OffsetSolid => (),
        };
        CASES
    }

    #[test]
    fn compile_modify_op_non_geometry_target_fallback_all_single_geom_target_kinds() {
        for &(kind, fn_name, tail) in single_geom_target_kinds() {
            assert_non_geometry_target_fallback(kind, fn_name, tail);
        }
    }

    /// Regression-lock helper: verify that `fn_name(target, tail_arg_names[0], ...)` with a
    /// scalar `target` param nested inside `union(sphere(1mm), fn_name(target, ...))` falls back
    /// to `GeomRef::Step(1)` (the step_offset after the sphere occupies step 0), NOT a
    /// hardcoded `Step(0)` as was the pre-fix bug (task-612/task-1732).
    ///
    /// Each name in `tail_arg_names` becomes a `param <name>: Length` in the generated source.
    /// Tail arg values are uniform `Scalar = (i+2)mm` literals — see
    /// `assert_non_geometry_target_fallback` for the rationale (the fallback path is independent
    /// of tail arg types).
    /// The sphere compiles at step_offset=0 and emits 1 op; the modify call then compiles
    /// at step_offset=1.  Expected ops: [Primitive(Sphere), Modify(<kind>, target=Step(1)),
    /// Boolean(Union, left=Step(0), right=Step(1))].
    fn assert_non_geometry_target_fallback_step_offset_nonzero(
        kind: ModifyKind,
        fn_name: &str,
        tail_arg_names: &[&str],
    ) {
        let param_decls: String = tail_arg_names
            .iter()
            .enumerate()
            .map(|(i, name)| format!("    param {name}: Length = {}mm\n", i + 2))
            .collect();
        let tail_call = tail_arg_names.join(", ");
        let source = format!(
            "structure S {{\n    param target: Length = 5mm\n{decls}    let result = union(sphere(1mm), {f}(target, {tail}))\n}}",
            f = fn_name,
            decls = param_decls,
            tail = tail_call,
        );
        let parsed = reify_syntax::parse(
            &source,
            reify_core::ModulePath::single(format!("test_{}_step_offset_nonzero", fn_name)),
        );
        assert!(
            parsed.errors.is_empty(),
            "{}(): parse errors: {:?}",
            fn_name,
            parsed.errors
        );
        let compiled = crate::compile(&parsed);
        assert!(
            compiled.diagnostics.is_empty(),
            "{}(): unexpected diagnostics: {:?}",
            fn_name,
            compiled.diagnostics
        );
        let ops = &compiled.templates[0].realizations[0].operations;
        assert_eq!(
            ops.len(),
            3,
            "{}(): expected 3 ops [Primitive(Sphere), Modify({:?}), Boolean(Union)], got {}",
            fn_name,
            kind,
            ops.len()
        );
        // ops[1] must be the modify op with target=Step(1), NOT Step(0).
        // Step(1) proves the fallback uses step_offset (== 1 here), not a hardcoded 0.
        match &ops[1] {
            CompiledGeometryOp::Modify {
                kind: op_kind,
                target: op_target,
                ..
            } => {
                assert_eq!(
                    *op_kind, kind,
                    "{}(): expected {:?} at ops[1], got {:?}",
                    fn_name, kind, op_kind
                );
                assert_eq!(
                    *op_target,
                    GeomRef::Step(1),
                    "{}(): non-geometry target should fall back to GeomRef::Step(step_offset=1), \
                     not Step(0); Step(0) would indicate the pre-fix hardcoded-zero bug \
                     (task-612/task-1732) has regressed",
                    fn_name
                );
            }
            other => panic!(
                "{}(): expected Modify({:?}) at ops[1], got {:?}",
                fn_name, kind, other
            ),
        }
        // Sanity-check ops[2]: union with left=Step(0) right=Step(1), confirming
        // the step_offset assignment that the modify call naturally fell back to.
        match &ops[2] {
            CompiledGeometryOp::Boolean { op, left, right } => {
                assert_eq!(
                    *op,
                    BooleanOp::Union,
                    "{}(): expected Union at ops[2]",
                    fn_name
                );
                assert_eq!(
                    *left,
                    GeomRef::Step(0),
                    "{}(): union left should be Step(0)",
                    fn_name
                );
                assert_eq!(
                    *right,
                    GeomRef::Step(1),
                    "{}(): union right should be Step(1)",
                    fn_name
                );
            }
            other => panic!(
                "{}(): expected Boolean(Union) at ops[2], got {:?}",
                fn_name, other
            ),
        }
    }

    #[test]
    fn compile_modify_op_non_geometry_target_fallback_step_offset_nonzero_all_single_geom_target_kinds()
     {
        // Regression-lock for all 5 single-geometry-target modify kinds — proves each
        // uses step_offset from context rather than a hardcoded 0 (task-612/task-1732).
        for &(kind, fn_name, tail) in single_geom_target_kinds() {
            assert_non_geometry_target_fallback_step_offset_nonzero(kind, fn_name, tail);
        }
    }

    /// Assert that the `CASES` table in `single_geom_target_kinds()` contains exactly one entry
    /// per `ModifyKind` variant — i.e., that the set of variants in the table has the same
    /// cardinality as `ModifyKind::VARIANT_COUNT`.
    ///
    /// ## Gap this closes
    ///
    /// The two existing exhaustiveness tripwires in `single_geom_target_kinds()` protect against
    /// *missing* entries but not against *duplicate-with-omission* edits:
    ///
    /// 1. **Compile-time count assert** (`geometry_modify.rs` — the `const _: () = assert!(
    ///    CASES.len() == ModifyKind::VARIANT_COUNT, ...)` immediately after the `CASES`
    ///    declaration): fires at `cargo check` if the table length drifts from the variant count,
    ///    but passes for any table of exactly 5 rows — including one with two `Chamfer` rows and
    ///    zero `Draft` rows.
    ///
    /// 2. **No-wildcard sentinel closure** (`geometry_modify.rs` — the `let _ = |k: ModifyKind|
    ///    match k { ModifyKind::Chamfer | ... => () }` below `CASES`): fails to compile when a
    ///    new variant is added without updating the closure, but only enumerates variants; it does
    ///    not cross-check the `CASES` rows against that enumeration.
    ///
    /// Neither tripwire catches a routine table edit that silently swaps one variant for a
    /// duplicate. This test closes that gap by collecting the `ModifyKind` keys from `CASES` into
    /// a `HashSet` and asserting the set's length equals `ModifyKind::VARIANT_COUNT`. A
    /// duplicate row shrinks the set size below the count, failing the assertion.
    #[test]
    fn single_geom_target_kinds_cases_table_unique_variant_set() {
        use std::collections::HashSet;
        let variants: HashSet<ModifyKind> = single_geom_target_kinds()
            .iter()
            .map(|&(k, _, _)| k)
            .collect();
        assert_eq!(
            variants.len(),
            ModifyKind::VARIANT_COUNT,
            "CASES table has duplicate variants — every ModifyKind must appear exactly once \
             (got {:?}, expected {} unique entries); a duplicate-with-omission edit (e.g., two \
             `Chamfer` rows and zero `Draft`) would slip past the count-only `const _: () = \
             assert!(CASES.len() == ModifyKind::VARIANT_COUNT, ...)` check above",
            variants,
            ModifyKind::VARIANT_COUNT,
        );
    }
}
