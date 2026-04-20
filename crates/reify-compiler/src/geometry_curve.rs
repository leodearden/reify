use super::*;

/// Compile a curve constructor call into a single CompiledGeometryOp::Curve.
///
/// Curve constructors are pure: they take only scalar args and produce a
/// standalone op with no geometry-arg dependencies (sub_ops is always empty).
pub(crate) fn compile_curve_op(
    name: &str,
    compiled_args: Vec<CompiledExpr>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<CompiledGeometryOp>> {
    match name {
        // line_segment(x1, y1, z1, x2, y2, z2)
        "line_segment" => {
            if compiled_args.len() != 6 {
                diagnostics.push(Diagnostic::error(format!(
                    "line_segment() expects 6 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::LineSegment,
                args: vec![
                    ("x1".to_string(), it.next().unwrap()),
                    ("y1".to_string(), it.next().unwrap()),
                    ("z1".to_string(), it.next().unwrap()),
                    ("x2".to_string(), it.next().unwrap()),
                    ("y2".to_string(), it.next().unwrap()),
                    ("z2".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)
        "arc" => {
            if compiled_args.len() != 9 {
                diagnostics.push(Diagnostic::error(format!(
                    "arc() expects 9 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Curve {
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
            }])
        }
        // helix(radius, pitch, height)
        "helix" => {
            if compiled_args.len() != 3 {
                diagnostics.push(Diagnostic::error(format!(
                    "helix() expects 3 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::Helix,
                args: vec![
                    ("radius".to_string(), it.next().unwrap()),
                    ("pitch".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // interp(x1,y1,z1, x2,y2,z2, ...) — variadic, triples of coordinates
        "interp" => {
            if compiled_args.len() < 6 || !compiled_args.len().is_multiple_of(3) {
                diagnostics.push(Diagnostic::error(format!(
                    "interp() expects coordinate triples (at least 6 args for 2 points), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("c{}", i), expr))
                .collect();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::InterpCurve,
                args,
            }])
        }
        // bezier(x1,y1,z1, x2,y2,z2, ...) — variadic, triples of coordinates
        "bezier" => {
            if compiled_args.len() < 6 || !compiled_args.len().is_multiple_of(3) {
                diagnostics.push(Diagnostic::error(format!(
                    "bezier() expects coordinate triples (at least 6 args for 2 points), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("c{}", i), expr))
                .collect();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::BezierCurve,
                args,
            }])
        }
        // nurbs — minimum: degree(1) + n_points(1) + 2×3 coords(6) + 2 weights(2) = 10
        "nurbs" => {
            if compiled_args.len() < 10 {
                diagnostics.push(Diagnostic::error(format!(
                    "nurbs() expects at least 10 arguments (degree + n_points + coords + weights), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("c{}", i), expr))
                .collect();
            Some(vec![CompiledGeometryOp::Curve {
                kind: CurveKind::NurbsCurve,
                args,
            }])
        }
        _ => unreachable!("compile_curve_op called with non-curve name: {}", name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar_literal(v: f64) -> CompiledExpr {
        CompiledExpr::literal(Value::Real(v), Type::Real)
    }

    #[test]
    fn compile_curve_op_line_segment_direct() {
        let args: Vec<CompiledExpr> = (1..=6).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_curve_op("line_segment", args.clone(), &mut diagnostics);
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("compile_curve_op line_segment should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Curve { kind: CurveKind::LineSegment, args: op_args } => {
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["x1", "y1", "z1", "x2", "y2", "z2"]);
                assert_eq!(op_args.len(), 6);
            }
            other => panic!("expected Curve(LineSegment), got {:?}", other),
        }
    }

    #[test]
    fn compile_curve_op_wrong_arg_count() {
        let args: Vec<CompiledExpr> = (1..=3).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_curve_op("line_segment", args, &mut diagnostics);
        assert!(result.is_none(), "expected None for wrong arg count");
        assert!(!diagnostics.is_empty(), "expected diagnostic for wrong arg count");
    }
}
