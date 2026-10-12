//! Program assembly: the single program-level compile entry point.
//!
//! [`compile_program`] compiles an entry module against the full stdlib plus
//! its `#cfg`-satisfied user-import DAG and returns a [`CompiledProgram`]: the
//! entry (with the direct imports' pub templates merged in), the direct import
//! units, and the source of every compiled user module (PRD
//! docs/prds/v0_6/resolution-unification.md §3.1). It upholds I1 (an
//! import-free entry compiles exactly as the single-file stdlib compile does),
//! I2 (a broken import surfaces as a diagnostic on the entry, never as a
//! dropped compile), I3 (a gated-out import is inert), I4 (`std.*` is never
//! walked through the DAG) and I5 (the entry's template list is the 1-hop pub
//! merge).

use std::path::{Path, PathBuf};

use reify_core::{Diagnostic, ModulePath};
use reify_ir::ConstraintChecker;

use crate::CompiledModule;
use crate::cfg::CfgSet;
use crate::module_dag::{
    ModuleDag, ModuleResolver, ModuleSource, attach_module_path_diag, entry_module_name,
    merge_imported_pub_templates,
};

/// One direct import of a program's entry, compiled.
#[derive(Debug, Clone)]
pub struct CompiledImportUnit {
    /// The dotted path the entry imported it by.
    pub import_path: String,
    /// The compiled module.
    pub module: CompiledModule,
    /// The file it was compiled from; `None` for an embedded-stdlib module.
    pub origin: Option<PathBuf>,
}

/// A compiled program: an entry module plus everything it was compiled with.
#[derive(Debug, Clone)]
pub struct CompiledProgram {
    /// The entry module, with the direct imports' pub templates merged into its
    /// template list and every import failure attached to its diagnostics.
    pub entry: CompiledModule,
    /// The entry's followed, successfully compiled direct imports, in
    /// declaration order, one per distinct import path.
    pub imports: Vec<CompiledImportUnit>,
    /// The source of every user module compiled into the program, in
    /// topological order (dependencies first), entry last.
    pub sources: Vec<ModuleSource>,
}

/// Compile the program rooted at `entry_path`, taking the entry's text from
/// `entry_source` (an editor's dirty buffer works: `entry_path` need not exist
/// on disk, and only its file stem names the entry module).
///
/// The full stdlib prelude is always seeded. User imports resolve from disk via
/// `resolver`; imports whose `#cfg(...)` predicates `cfg` does not satisfy are
/// skipped entirely (not resolved, not compiled, not in any prelude), and
/// `std.*` imports are skipped because the stdlib is already present.
///
/// **Diagnostics-embedded contract.** Nothing short-circuits. An entry parse
/// error reaches `entry.diagnostics` through the compiler's own parse-error
/// forwarding, and a followed import that is missing or broken surfaces as
/// `Error` diagnostics on the entry while the entry itself still compiles, so a
/// caller's "any Error diagnostic, exit non-zero" policy handles every failure
/// uniformly.
///
/// **Entry/import checker asymmetry.** Only the entry's compile receives
/// `checker`; import units are compiled with the compile-time stub checker,
/// which bounds the blast radius of compile-time `auto:` feasibility checks.
/// The boundary is visible for constant-foldable constraints (e.g.
/// `constraint 0 > 1`): such a module has its `auto:` candidates rejected when
/// compiled as the entry under a real checker, but accepted when compiled as an
/// import (stub, `Indeterminate`). For cell-dependent constraints the
/// compile-time value map is empty, so both paths return `Indeterminate` and no
/// divergence is observable. The integration test
/// `compile_program_import_uses_stub` pins this.
pub fn compile_program(
    entry_path: &Path,
    entry_source: &str,
    resolver: &ModuleResolver,
    cfg: &CfgSet,
    checker: &dyn ConstraintChecker,
) -> CompiledProgram {
    let entry_key = entry_module_name(entry_path);
    let parsed = crate::parse_with_stdlib(entry_source, ModulePath::single(entry_key));
    // Ambient stdlib visibility: every unit is parsed and compiled against the
    // full cached stdlib, and no `std.*` import is walked at any depth.
    let mut dag = ModuleDag::with_ambient_stdlib(cfg.clone());

    // The FOLLOWED direct imports, through the same gate the DAG walk applies
    // inside each unit, computed once and shared by the compile loop, the
    // prelude, the merge and the returned units so they cannot drift.
    let mut followed: Vec<&str> = Vec::new();
    for import in parsed.declarations.iter().filter_map(|decl| match decl {
        reify_ast::Declaration::Import(import) => Some(import),
        _ => None,
    }) {
        let path = import.path.as_str();
        if dag.follows_import(import) && !followed.contains(&path) {
            followed.push(path);
        }
    }

    // Collect, never early-return on, import failures: a broken import still
    // lets the entry compile, with the failure attached below.
    let mut import_error_diags: Vec<Diagnostic> = Vec::new();
    for import_path in &followed {
        if let Err(diags) = dag.compile_module(import_path, resolver) {
            import_error_diags.extend(diags);
        }
    }

    // A satisfied-but-failed import is absent from `dag.modules`; `filter_map`
    // skips it (its failure is already collected).
    let compiled_imports: Vec<(&str, &CompiledModule)> = followed
        .iter()
        .filter_map(|path| dag.modules.get(*path).map(|module| (*path, module)))
        .collect();
    let import_refs: Vec<&CompiledModule> =
        compiled_imports.iter().map(|(_, module)| *module).collect();
    let mut entry = crate::compile_with_stdlib_and_imports_checked(&parsed, &import_refs, checker);

    attach_module_path_diag(&mut entry, &parsed);
    entry.diagnostics.extend(import_error_diags);
    let entry_origin = entry.path.0.join(".");
    merge_imported_pub_templates(&mut entry, &entry_origin, &compiled_imports);

    let mut sources: Vec<ModuleSource> = dag
        .topo_order
        .iter()
        .filter_map(|key| dag.source(key).cloned())
        .collect();
    sources.push(ModuleSource {
        module_key: entry_key.to_string(),
        path: entry_path.to_path_buf(),
        source: entry_source.to_string(),
    });

    let imports: Vec<CompiledImportUnit> = followed
        .iter()
        .filter_map(|path| {
            let origin = dag.source(path).map(|s| s.path.clone());
            dag.modules.remove(*path).map(|module| CompiledImportUnit {
                import_path: path.to_string(),
                module,
                origin,
            })
        })
        .collect();

    CompiledProgram {
        entry,
        imports,
        sources,
    }
}
