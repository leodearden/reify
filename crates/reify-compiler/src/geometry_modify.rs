use super::*;

/// Compile a modify operation into CompiledGeometryOps.
///
/// Takes pre-resolved target GeomRef, expr_span for diagnostics, and pre-accumulated sub_ops.
///
/// Shell/thicken/draft use the passed `target`, push to sub_ops, and return Some(sub_ops).
///
/// Chamfer/fillet use hardcoded GeomRef::Step(0) and return Some(vec![op]).
/// NOTE: This preserves a known bug where chamfer/fillet are NOT registered in
/// geometry_arg_indices(), so sub_ops is always empty for them and geom_ref(0)
/// resolves to GeomRef::Step(step_offset). The caller passes GeomRef::Step(0)
/// explicitly to maintain bug-for-bug compatibility.
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
        // NOTE: Preserves known bug — uses hardcoded GeomRef::Step(0) regardless of passed target.
        // Chamfer is not registered in geometry_arg_indices(), so sub_ops is always empty
        // and geom_ref(0) in the caller resolves to GeomRef::Step(step_offset), not the
        // geometry argument. The caller passes GeomRef::Step(0) explicitly for chamfer/fillet
        // to make the bug preservation explicit and testable.
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
            Some(vec![CompiledGeometryOp::Modify {
                kind: ModifyKind::Chamfer,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("distance".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // fillet(target, radius)
        // NOTE: Same bug preservation as chamfer above.
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
            Some(vec![CompiledGeometryOp::Modify {
                kind: ModifyKind::Fillet,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("radius".to_string(), it.next().unwrap()),
                ],
            }])
        }
        _ => unreachable!("compile_modify_op called with non-modify name: {}", name),
    }
}
