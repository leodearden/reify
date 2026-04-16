use super::*;

/// Compile a modify operation into CompiledGeometryOps.
///
/// Takes pre-resolved target GeomRef, expr_span for diagnostics, and pre-accumulated sub_ops.
///
/// All arms use the passed `target`, push to sub_ops, and return Some(sub_ops).
///
/// NOTE: chamfer/fillet are NOT registered in geometry_arg_indices(), so their
/// sub_ops is always empty and geom_ref(0) in the caller would fall back to
/// GeomRef::Step(step_offset) rather than a true geometry ref. The caller
/// (geometry.rs) passes GeomRef::Step(0) explicitly for chamfer/fillet to
/// preserve bug-for-bug compatibility with existing behaviour. This function
/// simply honours whatever target it receives.
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
