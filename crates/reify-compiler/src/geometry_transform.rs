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
        // rotate(target, ax, ay, az, angle)  OR  rotate(target, orientation)
        "rotate" => {
            match compiled_args.len() {
                5 => {
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
                2 => {
                    let mut it = compiled_args.into_iter();
                    let op = CompiledGeometryOp::Transform {
                        kind: TransformKind::Rotate,
                        target,
                        args: vec![
                            ("target".to_string(), it.next().unwrap()),
                            ("orientation".to_string(), it.next().unwrap()),
                        ],
                    };
                    sub_ops.push(op);
                    Some(sub_ops)
                }
                n => {
                    push_labeled_arg_count_error(
                        format!("rotate() expects 2 or 5 arguments, got {n}"),
                        expr_span,
                        diagnostics,
                    );
                    None
                }
            }
        }
        // scale(target, factor: Real)  OR  scale(target, factors: Vector3<Real>)
        //
        // Dispatch on the second arg's STATIC type (task 4167): `vec3(..)` is
        // typed `Type::Vector{n:3,..}` at compile time (math_signatures.rs),
        // so a Vector3 second arg routes to the dedicated per-axis
        // `ScaleNonUniform` op (arg name "factors"); any other (Real/
        // dimensionless-scalar) second arg keeps the existing uniform `Scale`
        // fast-path (arg name "factor") untouched.
        "scale" => {
            if !check_arg_count_exact("scale", compiled_args.len(), 2, expr_span, diagnostics) {
                return None;
            }
            let is_vector3 = matches!(compiled_args[1].result_type, Type::Vector { n: 3, .. });
            let mut it = compiled_args.into_iter();
            let op = if is_vector3 {
                CompiledGeometryOp::Transform {
                    kind: TransformKind::ScaleNonUniform,
                    target,
                    args: vec![
                        ("target".to_string(), it.next().unwrap()),
                        ("factors".to_string(), it.next().unwrap()),
                    ],
                }
            } else {
                CompiledGeometryOp::Transform {
                    kind: TransformKind::Scale,
                    target,
                    args: vec![
                        ("target".to_string(), it.next().unwrap()),
                        ("factor".to_string(), it.next().unwrap()),
                    ],
                }
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
        // apply_transform(target, transform)
        "apply_transform" => {
            if !check_arg_count_exact("apply_transform", compiled_args.len(), 2, expr_span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            let op = CompiledGeometryOp::Transform {
                kind: TransformKind::ApplyTransform,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("transform".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // affine_apply(target, map)
        "affine_apply" => {
            if !check_arg_count_exact("affine_apply", compiled_args.len(), 2, expr_span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            let op = CompiledGeometryOp::Transform {
                kind: TransformKind::AffineApply,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("map".to_string(), it.next().unwrap()),
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
        CompiledExpr::literal(Value::Real(v), Type::dimensionless_scalar())
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

    // ── affine_apply tests (task 3963 step-3) ────────────────────────────────

    #[test]
    fn compile_transform_op_affine_apply_2_args() {
        // affine_apply(target, map) — 2 args
        let args: Vec<CompiledExpr> = vec![scalar_literal(0.0), scalar_literal(0.0)];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(0);
        let result = compile_transform_op(
            "affine_apply",
            args,
            target.clone(),
            SourceSpan::new(0, 0),
            &mut diagnostics,
            vec![],
        );
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("2-arg affine_apply should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Transform {
                kind: TransformKind::AffineApply,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "map"]);
            }
            other => panic!("expected Transform(AffineApply), got {:?}", other),
        }
    }

    #[test]
    fn compile_transform_op_affine_apply_wrong_arg_count_1() {
        let args: Vec<CompiledExpr> = vec![scalar_literal(0.0)];
        let span = SourceSpan::new(5, 15);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_transform_op(
            "affine_apply",
            args,
            GeomRef::Step(0),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(result.is_none(), "expected None for 1-arg affine_apply");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(!diagnostics[0].labels.is_empty(), "expected label on diagnostic");
        assert_eq!(diagnostics[0].labels[0].span, span, "label span must match expr_span");
    }

    #[test]
    fn compile_transform_op_affine_apply_wrong_arg_count_3() {
        let args: Vec<CompiledExpr> = (0..3).map(|_| scalar_literal(0.0)).collect();
        let span = SourceSpan::new(5, 15);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_transform_op(
            "affine_apply",
            args,
            GeomRef::Step(0),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(result.is_none(), "expected None for 3-arg affine_apply");
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

    // ── rotate orientation overload tests (task γ, #4166) ────────────────────

    /// 2-arg rotate(target, orientation) → Some with arg names ["target","orientation"],
    /// kind Rotate, no diagnostics.
    #[test]
    fn compile_transform_op_rotate_2_arg_orientation() {
        let args: Vec<CompiledExpr> = vec![scalar_literal(0.0), scalar_literal(0.0)];
        let span = SourceSpan::new(0, 0);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(0);
        let result = compile_transform_op(
            "rotate",
            args,
            target.clone(),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("2-arg rotate should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Transform {
                kind: TransformKind::Rotate,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "orientation"]);
            }
            other => panic!("expected Transform(Rotate), got {:?}", other),
        }
    }

    /// 3-arg rotate → None with "rotate() expects 2 or 5 arguments, got 3" diagnostic
    /// whose label span matches expr_span.
    #[test]
    fn compile_transform_op_rotate_3_arg_error() {
        let args: Vec<CompiledExpr> = (0..3).map(|_| scalar_literal(0.0)).collect();
        let span = SourceSpan::new(7, 42);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_transform_op(
            "rotate",
            args,
            GeomRef::Step(0),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(result.is_none(), "expected None for 3-arg rotate");
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        assert!(
            diagnostics[0].message.contains("rotate() expects 2 or 5 arguments, got 3"),
            "diagnostic message should mention 'rotate() expects 2 or 5 arguments, got 3', got: {}",
            diagnostics[0].message,
        );
        assert!(!diagnostics[0].labels.is_empty(), "expected label on diagnostic");
        assert_eq!(diagnostics[0].labels[0].span, span, "label span must match expr_span");
    }

    /// Regression: 5-arg rotate(target, ax, ay, az, angle) still works after the
    /// dispatch change — arg names ["target","ax","ay","az","angle"], kind Rotate,
    /// no diagnostics.
    #[test]
    fn compile_transform_op_rotate_5_arg_regression() {
        let args: Vec<CompiledExpr> = (0..5).map(|_| scalar_literal(0.0)).collect();
        let span = SourceSpan::new(0, 0);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(0);
        let result = compile_transform_op(
            "rotate",
            args,
            target.clone(),
            span,
            &mut diagnostics,
            vec![],
        );
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("5-arg rotate should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Transform {
                kind: TransformKind::Rotate,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "ax", "ay", "az", "angle"]);
            }
            other => panic!("expected Transform(Rotate), got {:?}", other),
        }
    }

    // ── scale(target, factors: Vector3<Real>) dispatch tests (task 4167 step-7) ──

    /// 2-arg scale(target, factors) where `factors` is statically typed
    /// `Type::Vector{n:3,..}` (e.g. `vec3(2.0,1.0,0.5)`) must dispatch to
    /// `TransformKind::ScaleNonUniform` with arg names ["target","factors"].
    ///
    /// RED until the `scale` arm inspects `compiled_args[1].result_type`
    /// (step-8): today it unconditionally builds `TransformKind::Scale` with
    /// arg name "factor", so this test fails.
    #[test]
    fn compile_transform_op_scale_vector3_dispatches_scale_non_uniform() {
        let factors_expr = CompiledExpr::literal(
            Value::Vector(vec![Value::Real(2.0), Value::Real(1.0), Value::Real(0.5)]),
            Type::vec3(Type::dimensionless_scalar()),
        );
        let args: Vec<CompiledExpr> = vec![scalar_literal(0.0), factors_expr];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(0);
        let result = compile_transform_op(
            "scale",
            args,
            target.clone(),
            SourceSpan::new(0, 0),
            &mut diagnostics,
            vec![],
        );
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("2-arg scale(vec3) should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Transform {
                kind: TransformKind::ScaleNonUniform,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "factors"]);
            }
            other => panic!("expected Transform(ScaleNonUniform), got {:?}", other),
        }
    }

    /// Regression: 2-arg scale(target, factor) where `factor` is a plain
    /// Real/dimensionless scalar must still dispatch to the uniform
    /// `TransformKind::Scale` with arg names ["target","factor"] —
    /// unaffected by the Vector3 dispatch added for step-8.
    #[test]
    fn compile_transform_op_scale_scalar_regression() {
        let args: Vec<CompiledExpr> = vec![scalar_literal(0.0), scalar_literal(2.0)];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(0);
        let result = compile_transform_op(
            "scale",
            args,
            target.clone(),
            SourceSpan::new(0, 0),
            &mut diagnostics,
            vec![],
        );
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("2-arg scale(scalar) should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Transform {
                kind: TransformKind::Scale,
                target: op_target,
                args: op_args,
            } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "factor"]);
            }
            other => panic!("expected Transform(Scale), got {:?}", other),
        }
    }
}
