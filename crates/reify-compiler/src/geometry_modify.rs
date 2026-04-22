use super::*;

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
            if compiled_args.len() < 2 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "shell() expects at least 2 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr_span, "wrong number of arguments")),
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
                    .with_label(DiagnosticLabel::new(expr_span, "wrong number of arguments")),
                );
                return None;
            }
            let mut it = compiled_args.into_iter();
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
                    .with_label(DiagnosticLabel::new(expr_span, "wrong number of arguments")),
                );
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
        "chamfer" => {
            if compiled_args.len() != 2 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "chamfer() expects 2 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr_span, "wrong number of arguments")),
                );
                return None;
            }
            let mut it = compiled_args.into_iter();
            let op = CompiledGeometryOp::Modify {
                kind: ModifyKind::Chamfer,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("distance".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // fillet(target, radius)
        "fillet" => {
            if compiled_args.len() != 2 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "fillet() expects 2 arguments, got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr_span, "wrong number of arguments")),
                );
                return None;
            }
            let mut it = compiled_args.into_iter();
            let op = CompiledGeometryOp::Modify {
                kind: ModifyKind::Fillet,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("radius".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
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
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("compile_modify_2arg should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify { kind: ModifyKind::Chamfer, target: op_target, args: op_args } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "distance"]);
            }
            other => panic!("expected Modify(Chamfer), got {:?}", other),
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
            diagnostics[0].message.contains("chamfer() expects 2 arguments, got 1"),
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
        let result = compile_modify_op("shell", args, target.clone(), span, &mut diagnostics, vec![]);
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("compile_modify_op shell should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify { kind: ModifyKind::Shell, target: op_target, args: op_args } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "thickness", "face_0"]);
            }
            other => panic!("expected Modify(Shell), got {:?}", other),
        }
    }

    #[test]
    fn compile_modify_op_chamfer_non_geometry_target_fallback() {
        // chamfer is registered in geometry_arg_indices() — so geom_ref(0) is used.
        // When the first arg is a scalar param (not a geometry let), the resolution
        // block finds no ops for it, so geom_ref(0) falls back to GeomRef::Step(step_offset).
        // With no sub-ops, step_offset == 0, so the target is GeomRef::Step(0).
        let source = r#"structure S {
    param target: Scalar = 5mm
    param dist: Scalar = 2mm
    let result = chamfer(target, dist)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_chamfer_step0"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = crate::compile(&parsed);
        assert!(compiled.diagnostics.is_empty(), "unexpected diagnostics: {:?}", compiled.diagnostics);
        let ops = &compiled.templates[0].realizations[0].operations;
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify { kind: ModifyKind::Chamfer, target: op_target, .. } => {
                // Non-geometry target → geom_ref(0) falls back to GeomRef::Step(0)
                assert_eq!(*op_target, GeomRef::Step(0),
                    "chamfer with non-geometry target should fall back to GeomRef::Step(0), got {:?}", op_target);
            }
            other => panic!("expected Modify(Chamfer), got {:?}", other),
        }
    }
}
