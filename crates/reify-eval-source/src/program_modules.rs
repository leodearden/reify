//! The existing program compile, run unchanged as the second output's
//! resolver, and the user modules it compiled, each paired with its parse.
//!
//! Each user module is compiled as the entry of a program of its own, which
//! hands back the import tier its compile bound names against; the compile of
//! a module is the same whether it is reached as an import or as an entry.

use std::path::Path;

use reify_ast::ParsedModule;
use reify_compiler::cfg::CfgSet;
use reify_compiler::program::{self, CompiledProgram};
use reify_compiler::{CompiledModule, parse_with_stdlib};
use reify_core::{Diagnostic, ModuleName, ModulePath};
use reify_ir::{
    ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction,
};

use crate::definition_list::{self, Definition};
use crate::module_context::{ModuleContext, ModuleSet, has_no_prelude, module_name};
use crate::refusal::SourceRefusal;

/// The user modules of one program: the entry first, then every module it
/// imports, directly or not, in the order the program compiled them.
pub(crate) struct ProgramModules {
    modules: Vec<UserModule>,
}

struct UserModule {
    /// `None` for the entry file.
    name: Option<ModuleName>,
    parsed: ParsedModule,
    compiled: CompiledModule,
    /// The user modules its compile bound names against, in that order.
    imports: Vec<CompiledModule>,
}

impl UserModule {
    fn of_program(
        name: Option<ModuleName>,
        parsed: ParsedModule,
        mut program: CompiledProgram,
    ) -> Self {
        let imports = std::mem::take(&mut program.imports)
            .into_iter()
            .map(|unit| unit.module)
            .collect();
        Self {
            name,
            parsed,
            compiled: program.into_entry(),
            imports,
        }
    }
}

impl ProgramModules {
    /// Compile the program rooted at `entry_path` with the existing compile,
    /// refusing a module that does not parse and an import the program did not
    /// compile.
    pub(crate) fn compile(entry_path: &Path, entry_source: &str) -> Result<Self, SourceRefusal> {
        let cfg = CfgSet::default();
        let resolver = program::entry_dir_resolver(entry_path);
        let compile = |path: &Path, source: &str, parsed: &ParsedModule| {
            program::compile_program_with_parsed_entry(
                path,
                source,
                parsed,
                &resolver,
                &cfg,
                &IndeterminateChecker,
            )
        };

        let entry_program = program::compile_program(
            entry_path,
            entry_source,
            &resolver,
            &cfg,
            &IndeterminateChecker,
        );
        let compiled_keys: Vec<String> = entry_program
            .sources
            .iter()
            .map(|source| source.module_key.clone())
            .collect();
        let entry_key = entry_program.entry().path.to_dotted();
        let imported_sources: Vec<_> = entry_program
            .sources
            .iter()
            .filter(|source| source.module_key != entry_key)
            .cloned()
            .collect();
        let entry_parsed = parsed(entry_source, entry_program.entry().path.clone())?;

        let mut modules = vec![UserModule::of_program(None, entry_parsed, entry_program)];
        for source in imported_sources {
            let path = ModulePath::from_dotted(&source.module_key).unwrap_or_else(|e| {
                panic!(
                    "a compiled module's key `{}` is a dotted path: {e:?}",
                    source.module_key
                )
            });
            let module_parsed = parsed(&source.source, path.clone())?;
            let module_program = compile(&source.path, &source.source, &module_parsed);
            modules.push(UserModule::of_program(
                Some(module_name(&path)),
                module_parsed,
                module_program,
            ));
        }

        for module in &modules {
            for import in program::followed_imports(&module.parsed, &cfg) {
                if !compiled_keys.contains(&import.path) {
                    return Err(SourceRefusal::ImportNotCompiled {
                        module: module.parsed.path.clone(),
                        import: ModulePath::from_dotted(&import.path).unwrap_or_else(|e| {
                            panic!(
                                "the parser accepts `{}` only as a dotted path: {e:?}",
                                import.path
                            )
                        }),
                    });
                }
            }
        }
        Ok(Self { modules })
    }

    /// Whether the entry opts out of every prelude, the standard library's
    /// definitions included.
    pub(crate) fn entry_has_no_prelude(&self) -> bool {
        has_no_prelude(&self.modules[0].parsed)
    }

    /// The definitions of every user module, the entry's first, each module
    /// resolving against `stdlib` and then its own imports; the traits they
    /// conform to may be declared in any user module or in `stdlib_contexts`.
    pub(crate) fn lower(
        &self,
        stdlib: &[CompiledModule],
        stdlib_contexts: &[ModuleContext<'_>],
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Vec<Definition>, SourceRefusal> {
        let contexts: Vec<ModuleContext<'_>> = self
            .modules
            .iter()
            .map(|module| {
                let tier = stdlib.iter().chain(&module.imports).collect();
                ModuleContext::new(module.name.clone(), &module.parsed, &module.compiled, tier)
            })
            .collect();
        let modules = ModuleSet::new(contexts.iter().chain(stdlib_contexts).collect());
        let mut definitions = Vec::new();
        for context in &contexts {
            definitions.extend(definition_list::lower_module(
                context,
                &modules,
                diagnostics,
            )?);
        }
        Ok(definitions)
    }
}

/// `source` parsed as the module at `path`, the way the existing compile
/// parses a user module.
fn parsed(source: &str, path: ModulePath) -> Result<ParsedModule, SourceRefusal> {
    let parsed = parse_with_stdlib(source, path);
    if parsed.errors.is_empty() {
        Ok(parsed)
    } else {
        Err(SourceRefusal::NotParsed {
            module: parsed.path.clone(),
            errors: parsed.errors,
        })
    }
}

/// The checker the program compile is handed: every constraint is
/// indeterminate, as under the compile-time stub the existing compile gives
/// its own import units (`CompileTimeIndeterminateChecker`, crate-private to
/// reify-compiler).
struct IndeterminateChecker;

impl ConstraintChecker for IndeterminateChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: Satisfaction::Indeterminate,
                diagnostics: ConstraintDiagnostics::default(),
                indeterminate_reason: None,
            })
            .collect()
    }

    fn is_compile_time_stub(&self) -> bool {
        true
    }
}
