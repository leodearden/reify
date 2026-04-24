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
        // fillet(target, radius)
        "fillet" => compile_modify_2arg(
            "fillet",
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
        CompiledExpr::literal(Value::Real(v), Type::Real)
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

    /// Assert that `fn_name(target, arg_name)` with a scalar `target` param falls back
    /// to `GeomRef::Step(0)` (the step_offset when there are no prior sub-ops).
    fn assert_non_geometry_target_fallback(kind: ModifyKind, fn_name: &str, arg_name: &str) {
        let source = format!(
            "structure S {{\n    param target: Scalar = 5mm\n    param {a}: Scalar = 2mm\n    let result = {f}(target, {a})\n}}",
            f = fn_name,
            a = arg_name
        );
        let parsed = reify_syntax::parse(
            &source,
            reify_types::ModulePath::single("test_fallback"),
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
        assert_eq!(ops.len(), 1, "{}(): expected 1 op, got {}", fn_name, ops.len());
        match &ops[0] {
            CompiledGeometryOp::Modify {
                kind: op_kind,
                target: op_target,
                ..
            } => {
                assert_eq!(
                    *op_kind,
                    kind,
                    "{}(): expected {:?}, got {:?}",
                    fn_name,
                    kind,
                    op_kind
                );
                assert_eq!(
                    *op_target,
                    GeomRef::Step(0),
                    "{}(): non-geometry target should fall back to GeomRef::Step(0), got {:?}",
                    fn_name,
                    op_target
                );
            }
            other => panic!("{}(): expected Modify({:?}), got {:?}", fn_name, kind, other),
        }
    }

    #[test]
    fn compile_modify_op_non_geometry_target_fallback_all_2arg_kinds() {
        let cases: &[(ModifyKind, &str, &str)] = &[
            (ModifyKind::Chamfer, "chamfer", "distance"),
            (ModifyKind::Fillet, "fillet", "radius"),
            (ModifyKind::Thicken, "thicken", "offset"),
            (ModifyKind::Shell, "shell", "thickness"),
        ];
        for &(kind, fn_name, arg_name) in cases {
            assert_non_geometry_target_fallback(kind, fn_name, arg_name);
        }
    }

    #[test]
    fn compile_modify_op_chamfer_non_geometry_target_fallback_step_offset_nonzero() {
        // Nest chamfer inside union(sphere(1mm), chamfer(target, dist)) to force
        // step_offset > 0 when the chamfer is compiled.  The sphere compiles at
        // step_offset=0 and emits 1 op; the chamfer then compiles at step_offset=1.
        // Expected ops: [Primitive(Sphere), Modify(Chamfer, target=Step(1)), Boolean(Union)]
        let source = "structure S {\n    param target: Scalar = 5mm\n    param dist: Scalar = 2mm\n    let result = union(sphere(1mm), chamfer(target, dist))\n}";
        let parsed = reify_syntax::parse(
            source,
            reify_types::ModulePath::single("test_chamfer_step_offset_nonzero"),
        );
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = crate::compile(&parsed);
        assert!(
            compiled.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            compiled.diagnostics
        );
        let ops = &compiled.templates[0].realizations[0].operations;
        assert_eq!(
            ops.len(),
            3,
            "expected 3 ops [Primitive(Sphere), Modify(Chamfer), Boolean(Union)], got {}",
            ops.len()
        );
        // ops[1] must be the chamfer with target=Step(1), NOT Step(0).
        // Step(1) proves the fallback uses step_offset (== 1 here), not a hardcoded 0
        // (the pre-fix behaviour from task-612/task-1732).
        match &ops[1] {
            CompiledGeometryOp::Modify {
                kind: op_kind,
                target: op_target,
                ..
            } => {
                assert_eq!(*op_kind, ModifyKind::Chamfer, "expected Chamfer at ops[1]");
                assert_eq!(
                    *op_target,
                    GeomRef::Step(1),
                    "chamfer non-geometry target should fall back to GeomRef::Step(step_offset=1), \
                     not Step(0); Step(0) would indicate the pre-fix hardcoded-zero bug \
                     (task-612/task-1732) has regressed"
                );
            }
            other => panic!("expected Modify(Chamfer) at ops[1], got {:?}", other),
        }
        // Sanity-check ops[2]: union with left=Step(0) right=Step(1), confirming
        // the step_offset assignment that the chamfer naturally fell back to.
        match &ops[2] {
            CompiledGeometryOp::Boolean { op, left, right } => {
                assert_eq!(*op, BooleanOp::Union, "expected Union at ops[2]");
                assert_eq!(*left, GeomRef::Step(0), "union left should be Step(0)");
                assert_eq!(*right, GeomRef::Step(1), "union right should be Step(1)");
            }
            other => panic!("expected Boolean(Union) at ops[2], got {:?}", other),
        }
    }
}
