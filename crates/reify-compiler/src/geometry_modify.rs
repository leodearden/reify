use super::*;

pub(crate) fn compile_modify_op(
    _name: &str,
    _compiled_args: Vec<CompiledExpr>,
    _target: GeomRef,
    _expr_span: SourceSpan,
    _diagnostics: &mut Vec<Diagnostic>,
    _sub_ops: Vec<CompiledGeometryOp>,
) -> Option<Vec<CompiledGeometryOp>> {
    todo!()
}
