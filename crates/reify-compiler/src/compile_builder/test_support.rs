//! Fixture helpers shared by compile_builder's unit tests: parse and compile
//! inline `.ri` sources, and read what a compiled fn body constructs.

use reify_ast::ParsedModule;
use reify_core::ModulePath;
use reify_ir::{CompiledExpr, CompiledExprKind};

use crate::types::CompiledModule;

#[track_caller]
pub(crate) fn parse_at(module: &str, source: &str) -> ParsedModule {
    let parsed = reify_syntax::parse(source, ModulePath::single(module));
    assert!(
        parsed.errors.is_empty(),
        "fixture `{module}` must parse: {:?}",
        parsed.errors
    );
    parsed
}

#[track_caller]
pub(crate) fn compile_src(source: &str) -> CompiledModule {
    crate::compile(&parse_at("test", source))
}

/// `source` compiled as module `test` with module `p`, compiled from
/// `prelude_source`, as its prelude.
#[track_caller]
pub(crate) fn compile_against_prelude(prelude_source: &str, source: &str) -> CompiledModule {
    let prelude = crate::compile(&parse_at("p", prelude_source));
    crate::compile_with_prelude(&parse_at("test", source), &[prelude])
}

#[track_caller]
pub(crate) fn fn_result_expr<'m>(module: &'m CompiledModule, fn_name: &str) -> &'m CompiledExpr {
    let Some(found) = module.functions.iter().find(|f| f.name == fn_name) else {
        panic!(
            "expected fn `{fn_name}`; found functions: {:?}",
            module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
        )
    };
    &found.body.result_expr
}

/// The param names a structure ctor fills with defaults, or `None` when `expr`
/// constructs no structure. `CompiledExprKind` has no `PartialEq`, so callers
/// compare ctor results by the param names they fill.
pub(crate) fn constructed_params(expr: &CompiledExpr) -> Option<Vec<String>> {
    match &expr.kind {
        CompiledExprKind::StructureInstanceCtor { defaults, .. } => {
            Some(defaults.iter().map(|(name, _)| name.clone()).collect())
        }
        _ => None,
    }
}
