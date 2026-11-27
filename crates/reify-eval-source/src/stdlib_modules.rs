//! The standard library's part of every definition list, lowered once per
//! process from the embedded sources and the cached stdlib build.

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_compiler::stdlib_loader::{load_stdlib, stdlib_sources};
use reify_core::{Diagnostic, ModulePath};

use crate::SourceRefusal;
use crate::definition_list::{self, Definition};
use crate::module_context::{ModuleContext, module_name};

/// The standard library's definitions, in registration order.
pub(crate) struct StdlibPart {
    pub(crate) definitions: Vec<Definition>,
}

/// The modules every user module's compile binds against, unless it opts out.
pub(crate) fn compiled() -> &'static [CompiledModule] {
    load_stdlib()
}

/// The standard library, lowered on the first call.
pub(crate) fn lowered() -> &'static Result<StdlibPart, SourceRefusal> {
    static LOWERED: OnceLock<Result<StdlibPart, SourceRefusal>> = OnceLock::new();
    LOWERED.get_or_init(lower)
}

fn lower() -> Result<StdlibPart, SourceRefusal> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut definitions = Vec::new();
    for (registered, source) in stdlib_sources() {
        let path = ModulePath::from_dotted(registered)
            .unwrap_or_else(|e| panic!("a stdlib module is registered by its path: {e:?}"));
        let module = compiled()
            .iter()
            .find(|module| module.path == path)
            .unwrap_or_else(|| panic!("the stdlib build compiles every registered module: {path}"));
        let imports = imports_of(module);
        let enum_names: Vec<&str> = imports
            .iter()
            .flat_map(|import| import.enum_defs.iter().map(|e| e.name.as_str()))
            .collect();
        let parsed = reify_syntax::parse_with_prelude_enums(&source, path.clone(), &enum_names);
        if !parsed.errors.is_empty() {
            return Err(SourceRefusal::NotParsed {
                module: path,
                errors: parsed.errors,
            });
        }
        let context = ModuleContext::new(Some(module_name(&path)), &parsed, module, imports);
        definitions.extend(definition_list::lower_module(&context, &mut diagnostics)?);
    }
    Ok(StdlibPart { definitions })
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
