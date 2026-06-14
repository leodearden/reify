use super::*;

/// Compile a curve constructor call into CompiledGeometryOps.
///
/// Takes pre-accumulated sub_ops (from any geometry-arg dependencies resolved
/// by the caller). Each arm validates arg count, builds a
/// CompiledGeometryOp::Curve, pushes it to sub_ops, and returns Some(sub_ops).
///
/// Today all curve constructors return `&[]` from `geometry_arg_indices()` so
/// sub_ops will always be empty at the call site; the parameter is accepted for
/// forward-compatibility and to match the signature of `compile_transform_op`
/// and `compile_modify_op`.
pub(crate) fn compile_curve_op(
    name: &str,
    compiled_args: Vec<CompiledExpr>,
    expr_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
    mut sub_ops: Vec<CompiledGeometryOp>,
) -> Option<Vec<CompiledGeometryOp>> {
    match name {
        // line_segment(x1, y1, z1, x2, y2, z2)
        "line_segment" => {
            if !check_arg_count_exact(
                "line_segment",
                compiled_args.len(),
                6,
                expr_span,
                diagnostics,
            ) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            sub_ops.push(CompiledGeometryOp::Curve {
                kind: CurveKind::LineSegment,
                args: vec![
                    ("x1".to_string(), it.next().unwrap()),
                    ("y1".to_string(), it.next().unwrap()),
                    ("z1".to_string(), it.next().unwrap()),
                    ("x2".to_string(), it.next().unwrap()),
                    ("y2".to_string(), it.next().unwrap()),
                    ("z2".to_string(), it.next().unwrap()),
                ],
            });
            Some(sub_ops)
        }
        // arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)
        "arc" => {
            if !check_arg_count_exact("arc", compiled_args.len(), 9, expr_span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            sub_ops.push(CompiledGeometryOp::Curve {
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
            });
            Some(sub_ops)
        }
        // helix(radius, pitch, height)
        "helix" => {
            if !check_arg_count_exact("helix", compiled_args.len(), 3, expr_span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            sub_ops.push(CompiledGeometryOp::Curve {
                kind: CurveKind::Helix,
                args: vec![
                    ("radius".to_string(), it.next().unwrap()),
                    ("pitch".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                ],
            });
            Some(sub_ops)
        }
        // interp(x1,y1,z1, x2,y2,z2, ...) — variadic, triples of coordinates
        "interp" => {
            if compiled_args.len() < 6 || !compiled_args.len().is_multiple_of(3) {
                push_labeled_arg_count_error(
                    format!(
                        "interp() expects coordinate triples (at least 6 args for 2 points), got {}",
                        compiled_args.len()
                    ),
                    expr_span,
                    diagnostics,
                );
                return None;
            }
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("c{}", i), expr))
                .collect();
            sub_ops.push(CompiledGeometryOp::Curve {
                kind: CurveKind::InterpCurve,
                args,
            });
            Some(sub_ops)
        }
        // bezier(x1,y1,z1, x2,y2,z2, ...) — variadic, triples of coordinates
        "bezier" => {
            if compiled_args.len() < 6 || !compiled_args.len().is_multiple_of(3) {
                push_labeled_arg_count_error(
                    format!(
                        "bezier() expects coordinate triples (at least 6 args for 2 points), got {}",
                        compiled_args.len()
                    ),
                    expr_span,
                    diagnostics,
                );
                return None;
            }
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("c{}", i), expr))
                .collect();
            sub_ops.push(CompiledGeometryOp::Curve {
                kind: CurveKind::BezierCurve,
                args,
            });
            Some(sub_ops)
        }
        // nurbs — minimum: degree(1) + n_points(1) + 2×3 coords(6) + 2 weights(2) = 10
        "nurbs" => {
            if !check_arg_count_at_least("nurbs", compiled_args.len(), 10, expr_span, diagnostics) {
                return None;
            }
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("c{}", i), expr))
                .collect();
            sub_ops.push(CompiledGeometryOp::Curve {
                kind: CurveKind::NurbsCurve,
                args,
            });
            Some(sub_ops)
        }
        _ => unreachable!("compile_curve_op called with non-curve name: {}", name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar_literal(v: f64) -> CompiledExpr {
        CompiledExpr::literal(Value::Real(v), Type::dimensionless_scalar())
    }

    #[test]
    fn compile_curve_op_line_segment_direct() {
        let args: Vec<CompiledExpr> = (1..=6).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_curve_op(
            "line_segment",
            args.clone(),
            SourceSpan::new(0, 0),
            &mut diagnostics,
            vec![],
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );
        let ops = result.expect("compile_curve_op line_segment should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Curve {
                kind: CurveKind::LineSegment,
                args: op_args,
            } => {
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["x1", "y1", "z1", "x2", "y2", "z2"]);
                assert_eq!(op_args.len(), 6);
            }
            other => panic!("expected Curve(LineSegment), got {:?}", other),
        }
    }

    #[test]
    fn compile_curve_op_wrong_arg_count_with_label() {
        // line_segment expects 6 args; pass 3 — span must appear on the diagnostic label
        let args: Vec<CompiledExpr> = (1..=3).map(|i| scalar_literal(i as f64)).collect();
        let span = SourceSpan::new(10, 20);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_curve_op("line_segment", args, span, &mut diagnostics, vec![]);
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

    #[test]
    fn compile_curve_op_prepends_sub_ops() {
        let marker_sub_ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Sphere,
            args: vec![("radius".to_string(), scalar_literal(1.0))],
        }];
        let args: Vec<CompiledExpr> = (1..=6).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_curve_op(
            "line_segment",
            args,
            SourceSpan::new(0, 0),
            &mut diagnostics,
            marker_sub_ops,
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );
        let ops = result.expect("compile_curve_op line_segment should return Some");
        assert_eq!(ops.len(), 2);
        match &ops[0] {
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Sphere,
                ..
            } => {}
            other => panic!("expected Primitive(Sphere) at index 0, got {:?}", other),
        }
        match &ops[1] {
            CompiledGeometryOp::Curve {
                kind: CurveKind::LineSegment,
                ..
            } => {}
            other => panic!("expected Curve(LineSegment) at index 1, got {:?}", other),
        }
    }
}
