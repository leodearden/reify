use super::*;

/// Compile a transform operation into CompiledGeometryOps.
///
/// Takes pre-resolved target GeomRef and pre-accumulated sub_ops.
/// Each arm validates arg count, builds a CompiledGeometryOp::Transform,
/// pushes it to sub_ops, and returns Some(sub_ops).
pub(crate) fn compile_transform_op(
    name: &str,
    compiled_args: Vec<CompiledExpr>,
    target: GeomRef,
    expr_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
    mut sub_ops: Vec<CompiledGeometryOp>,
) -> Option<Vec<CompiledGeometryOp>> {
    match name {
        // translate(target, dx, dy, dz)
        "translate" => {
            if !check_arg_count_exact("translate", compiled_args.len(), 4, expr_span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
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
            if !check_arg_count_exact("rotate", compiled_args.len(), 5, expr_span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
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
            if !check_arg_count_exact("scale", compiled_args.len(), 2, expr_span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
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
            if !check_arg_count_exact(
                "rotate_around",
                compiled_args.len(),
                8,
                expr_span,
                diagnostics,
            ) {
                return None;
            }
            let mut it = compiled_args.into_iter();
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
        _ => unreachable!(
            "compile_transform_op called with non-transform name: {}",
            name
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar_literal(v: f64) -> CompiledExpr {
        CompiledExpr::literal(Value::Real(v), Type::Real)
    }

    #[test]
    fn compile_transform_op_apply_transform_2_args() {
        // apply_transform(target, transform) — 2 args
        let args: Vec<CompiledExpr> = vec![scalar_literal(0.0), scalar_literal(0.0)];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(0);
        let result = compile_transform_op(
            "apply_transform",
            args,
            target.clone(),
            SourceSpan::new(0, 0),
            &mut diagnostics,
            vec![],
        );
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("2-arg apply_transform should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Transform {
                kind: TransformKind::ApplyTransform,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "transform"]);
            }
            other => panic!("expected Transform(ApplyTransform), got {:?}", other),
        }
    }

    #[test]
    fn compile_transform_op_apply_transform_wrong_arg_count_1() {
        let args: Vec<CompiledExpr> = vec![scalar_literal(0.0)];
        let span = SourceSpan::new(5, 15);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_transform_op(
            "apply_transform",
            args,
            GeomRef::Step(0),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(result.is_none(), "expected None for 1-arg apply_transform");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(!diagnostics[0].labels.is_empty(), "expected label on diagnostic");
        assert_eq!(diagnostics[0].labels[0].span, span, "label span must match expr_span");
    }

    #[test]
    fn compile_transform_op_apply_transform_wrong_arg_count_3() {
        let args: Vec<CompiledExpr> = (0..3).map(|_| scalar_literal(0.0)).collect();
        let span = SourceSpan::new(5, 15);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_transform_op(
            "apply_transform",
            args,
            GeomRef::Step(0),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(result.is_none(), "expected None for 3-arg apply_transform");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(!diagnostics[0].labels.is_empty(), "expected label on diagnostic");
        assert_eq!(diagnostics[0].labels[0].span, span, "label span must match expr_span");
    }

    #[test]
    fn compile_transform_op_translate_direct() {
        // translate(target, dx, dy, dz) — 4 args
        let args: Vec<CompiledExpr> = (1..=4).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(0);
        let result = compile_transform_op(
            "translate",
            args,
            target.clone(),
            SourceSpan::new(0, 0),
            &mut diagnostics,
            vec![],
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );
        let ops = result.expect("compile_transform_op translate should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Transform {
                kind: TransformKind::Translate,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "dx", "dy", "dz"]);
            }
            other => panic!("expected Transform(Translate), got {:?}", other),
        }
    }

    #[test]
    fn compile_transform_op_wrong_arg_count() {
        // translate expects 4 args; pass 2
        let args: Vec<CompiledExpr> = (1..=2).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_transform_op(
            "translate",
            args,
            GeomRef::Step(0),
            SourceSpan::new(10, 20),
            &mut diagnostics,
            vec![],
        );
        assert!(result.is_none(), "expected None for wrong arg count");
        assert!(
            !diagnostics.is_empty(),
            "expected diagnostic for wrong arg count"
        );
    }

    #[test]
    fn compile_transform_op_wrong_arg_count_with_label() {
        // translate expects 4 args; pass 2 — span must appear on the diagnostic label
        let args: Vec<CompiledExpr> = (1..=2).map(|i| scalar_literal(i as f64)).collect();
        let span = SourceSpan::new(10, 20);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_transform_op(
            "translate",
            args,
            GeomRef::Step(0),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(result.is_none(), "expected None for wrong arg count");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(
            !diagnostics[0].labels.is_empty(),
            "expected at least one label on arg-count diagnostic"
        );
        assert_eq!(
            diagnostics[0].labels[0].span, span,
            "label span must match the expr_span passed in"
        );
    }
}
