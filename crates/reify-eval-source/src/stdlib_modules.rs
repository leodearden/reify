//! The standard library's part of every definition list, lowered once per
//! process from the embedded sources and the cached stdlib build.

use std::sync::OnceLock;

use reify_ast::ParsedModule;
use reify_compiler::CompiledModule;
use reify_compiler::stdlib_loader::{load_stdlib, stdlib_sources};
use reify_core::{Diagnostic, ModulePath, Severity};

use crate::definition_list::{self, Definition};
use crate::module_context::{ModuleContext, ModuleSet, module_name};
use crate::refusal::SourceRefusal;

/// The standard library's definitions, in registration order, and the
/// errors their lowering raised. Its warnings are not kept: they would repeat
/// on every entry and say nothing about it.
pub(crate) struct StdlibPart {
    pub(crate) definitions: Vec<Definition>,
    pub(crate) errors: Vec<Diagnostic>,
}

/// The modules every user module's compile binds against, unless it opts out.
pub(crate) fn compiled() -> &'static [CompiledModule] {
    load_stdlib()
}

/// Every stdlib module as the second output lowers it, in registration
/// order: what a user definition's traits can be declared in.
pub(crate) fn contexts() -> Result<&'static [ModuleContext<'static>], SourceRefusal> {
    static CONTEXTS: OnceLock<Vec<ModuleContext<'static>>> = OnceLock::new();
    let parsed = parsed().as_ref().map_err(Clone::clone)?;
    Ok(CONTEXTS.get_or_init(|| {
        parsed
            .iter()
            .map(|(module, compiled)| {
                ModuleContext::new(
                    Some(module_name(&module.path)),
                    module,
                    compiled,
                    imports_of(compiled),
                )
            })
            .collect()
    }))
}

/// The standard library, lowered on the first call.
pub(crate) fn lowered() -> &'static Result<StdlibPart, SourceRefusal> {
    static LOWERED: OnceLock<Result<StdlibPart, SourceRefusal>> = OnceLock::new();
    LOWERED.get_or_init(lower)
}

fn lower() -> Result<StdlibPart, SourceRefusal> {
    let contexts = contexts()?;
    let modules = ModuleSet::new(contexts.iter().collect());
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut definitions = Vec::new();
    for context in contexts {
        definitions.extend(definition_list::lower_module(
            context,
            &modules,
            &mut diagnostics,
        )?);
    }
    diagnostics.retain(|diagnostic| diagnostic.severity == Severity::Error);
    Ok(StdlibPart {
        definitions,
        errors: diagnostics,
    })
}

type ParsedStdlib = Vec<(ParsedModule, &'static CompiledModule)>;

/// Every stdlib module parsed, the way the stdlib build parses it, beside
/// that build's compile of it; parsed on the first call.
fn parsed() -> &'static Result<ParsedStdlib, SourceRefusal> {
    static PARSED: OnceLock<Result<ParsedStdlib, SourceRefusal>> = OnceLock::new();
    PARSED.get_or_init(|| stdlib_sources().into_iter().map(parse_module).collect())
}

fn parse_module(
    (registered, source): (&str, String),
) -> Result<(ParsedModule, &'static CompiledModule), SourceRefusal> {
    let path = ModulePath::from_dotted(registered)
        .unwrap_or_else(|e| panic!("a stdlib module is registered by its path: {e:?}"));
    let module = compiled()
        .iter()
        .find(|module| module.path == path)
        .unwrap_or_else(|| panic!("the stdlib build compiles every registered module: {path}"));
    let enum_names: Vec<&str> = imports_of(module)
        .iter()
        .flat_map(|import| import.enum_defs.iter().map(|e| e.name.as_str()))
        .collect();
    let parsed = reify_syntax::parse_with_prelude_enums(&source, path.clone(), &enum_names);
    if parsed.errors.is_empty() {
        Ok((parsed, module))
    } else {
        Err(SourceRefusal::NotParsed {
            module: path,
            errors: parsed.errors,
        })
    }
}

/// The stdlib modules `module` imports, in declaration order: what the stdlib
/// build compiled it against.
fn imports_of(module: &CompiledModule) -> Vec<&'static CompiledModule> {
    let mut imports: Vec<&'static CompiledModule> = Vec::new();
    for import in &module.imports {
        let target = compiled()
            .iter()
            .find(|candidate| candidate.path.to_dotted() == import.path)
            .unwrap_or_else(|| panic!("the stdlib build compiles every import: {}", import.path));
        if !imports.iter().any(|seen| std::ptr::eq(*seen, target)) {
            imports.push(target);
        }
    }
    imports
}
