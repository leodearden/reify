//! Compile one LSP document: as a program when it is a file-backed entry that
//! follows an import, otherwise exactly as the single-file stdlib compile.

use std::path::Path;

use reify_ast::ParsedModule;
use reify_compiler::CompiledModule;
use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::program;
use reify_constraints::SimpleConstraintChecker;
use tower_lsp::lsp_types::Url;

/// A compiled document, immutable once built.
pub(super) struct CompiledDocument {
    module: CompiledModule,
}

impl CompiledDocument {
    pub(super) fn single_file(module: CompiledModule) -> Self {
        Self { module }
    }

    pub(super) fn module(&self) -> &CompiledModule {
        &self.module
    }
}

/// Compile `parsed`, the parse of `source` addressed by `uri`, under `cfg`.
pub(super) fn compile_document(
    parsed: &ParsedModule,
    source: &str,
    uri: &Url,
    cfg: &CfgSet,
) -> CompiledDocument {
    match uri.to_file_path() {
        Ok(entry_path) if program::entry_follows_imports(parsed, cfg) => {
            let compiled = program::compile_program_with_parsed_entry(
                &entry_path,
                source,
                parsed,
                &entry_dir_resolver(&entry_path),
                cfg,
                &SimpleConstraintChecker,
            );
            CompiledDocument {
                module: compiled.entry,
            }
        }
        _ => CompiledDocument::single_file(reify_compiler::compile_with_stdlib_checked(
            parsed,
            &SimpleConstraintChecker,
        )),
    }
}

/// User imports resolve from the entry file's directory, as `reify check` and
/// the GUI resolve them.
fn entry_dir_resolver(entry_path: &Path) -> ModuleResolver {
    let dir = entry_path.parent().unwrap_or_else(|| Path::new("."));
    ModuleResolver::new(dir, dir.join("crates/reify-compiler/stdlib"))
}
