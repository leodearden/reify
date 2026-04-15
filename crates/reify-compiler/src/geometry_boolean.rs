use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_boolean_op(
    _name: &str,
    _args: &[reify_syntax::Expr],
    _scope: &CompilationScope,
    _enum_defs: &[reify_types::EnumDef],
    _functions: &[CompiledFunction],
    _diagnostics: &mut Vec<Diagnostic>,
    _step_offset: usize,
    _geometry_lets: &HashMap<&str, &reify_syntax::Expr>,
    _visiting: &mut HashSet<String>,
) -> Option<Vec<CompiledGeometryOp>> {
    todo!()
}
