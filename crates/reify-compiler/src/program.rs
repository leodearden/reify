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

use reify_core::{Diagnostic, ModulePath, Severity, SourceSpan, byte_offset_to_line_col};
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
    /// Which of `entry.diagnostics` are re-homed copies of a compiled user
    /// module's own diagnostics, in `entry.diagnostics` order.
    rehomed: Vec<RehomedDiagnostic>,
}

/// One of the entry's diagnostics that is a re-homed copy.
#[derive(Debug, Clone)]
struct RehomedDiagnostic {
    /// The dotted key of the user module whose compile raised the original.
    module_key: String,
    /// The copy's index in the entry's diagnostics as compiled; appending to
    /// them keeps it valid.
    entry_index: usize,
}

impl CompiledProgram {
    /// The entry's diagnostics re-homed from the user module `module_key` (a
    /// direct or transitive import), in the order they were raised.
    // G-allow: P1 skeleton API for per-file diagnostics; consumers are LSP ζ #5520 and GUI ε #5519
    pub fn rehomed_from(&self, module_key: &str) -> impl Iterator<Item = &Diagnostic> {
        self.rehomed
            .iter()
            .filter(move |rehomed| rehomed.module_key == module_key)
            .map(|rehomed| &self.entry.diagnostics[rehomed.entry_index])
    }

    /// The entry's diagnostics that are not re-homed copies: those raised
    /// compiling the entry itself, and the failures of its imports.
    // G-allow: P1 skeleton API for per-file diagnostics; consumers are LSP ζ #5520 and GUI ε #5519
    pub fn entry_own_diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.entry
            .diagnostics
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                !self
                    .rehomed
                    .iter()
                    .any(|rehomed| rehomed.entry_index == *index)
            })
            .map(|(_, diag)| diag)
    }
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
/// uniformly. Every Error and Warning a compiled user module carries —
/// transitive modules included — is re-homed onto the entry too, attributed to
/// its file (see `rehome_imported`); [`CompiledProgram::rehomed_from`] and
/// [`CompiledProgram::entry_own_diagnostics`] tell those copies apart from the
/// entry's own diagnostics.
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
// G-allow: PRD §3.1 canonical program entry; production consumers are P1 γ #5517 and δ #5518
pub fn compile_program(
    entry_path: &Path,
    entry_source: &str,
    resolver: &ModuleResolver,
    cfg: &CfgSet,
    checker: &dyn ConstraintChecker,
) -> CompiledProgram {
    let parsed = crate::parse_with_stdlib(
        entry_source,
        ModulePath::single(entry_module_name(entry_path)),
    );
    compile_program_with_parsed_entry(entry_path, entry_source, &parsed, resolver, cfg, checker)
}

/// [`compile_program`] over an entry the caller has already parsed.
///
/// Precondition: `parsed` is the parse of `entry_source`. The entry's module
/// key is `parsed.path`, dotted.
///
/// The seam exists for callers that already hold the entry's parse (the CLI's
/// `reify check` parses it for its own module-path gate first), and for callers
/// that must seed extra enum names into the parse — an import's enums, which
/// `parse_with_stdlib` does not know — until task #5522 (θ) moves that fixup
/// into the resolution phase.
pub fn compile_program_with_parsed_entry(
    entry_path: &Path,
    entry_source: &str,
    parsed: &reify_ast::ParsedModule,
    resolver: &ModuleResolver,
    cfg: &CfgSet,
    checker: &dyn ConstraintChecker,
) -> CompiledProgram {
    // Ambient stdlib visibility: every unit is parsed and compiled against the
    // full cached stdlib, and no `std.*` import is walked at any depth.
    let mut dag = ModuleDag::with_ambient_stdlib(cfg.clone());

    // Computed once and shared by the compile loop, the prelude, the merge and
    // the returned units, so they cannot drift.
    let followed = followed_direct_imports(parsed, &dag);

    // Collect, never early-return on, import failures: a broken import still
    // lets the entry compile, with the failure attached below.
    let mut import_error_diags: Vec<Diagnostic> = Vec::new();
    for import_path in &followed {
        if let Err(diags) = dag.compile_module(import_path, resolver) {
            push_unreported(&mut import_error_diags, diags);
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
    let mut entry = crate::compile_with_stdlib_and_imports_checked(parsed, &import_refs, checker);

    attach_module_path_diag(&mut entry, parsed);
    entry.diagnostics.extend(import_error_diags);
    let rehomed = rehome_unit_diagnostics(&dag, &mut entry.diagnostics);
    let entry_origin = entry.path.0.join(".");
    merge_imported_pub_templates(&mut entry, &entry_origin, &compiled_imports);

    let mut sources: Vec<ModuleSource> = dag
        .topo_order
        .iter()
        .filter_map(|key| dag.source(key).cloned())
        .collect();
    sources.push(ModuleSource {
        module_key: parsed.path.0.join("."),
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
        rehomed,
    }
}

/// Append each of `diags` that `reported` does not already hold.
///
/// A DAG does not remember a module that failed (a retry re-reads it), so a
/// broken module shared by two direct imports fails once per importer, with the
/// same diagnostics each time; the entry reports each failure once.
fn push_unreported(reported: &mut Vec<Diagnostic>, diags: Vec<Diagnostic>) {
    for diag in diags {
        let already = reported.iter().any(|seen| {
            seen.severity == diag.severity && seen.code == diag.code && seen.message == diag.message
        });
        if !already {
            reported.push(diag);
        }
    }
}

/// The entry's followed direct imports, in declaration order, one per path:
/// those `dag` follows, through the same gate it applies inside every unit.
fn followed_direct_imports<'p>(
    parsed: &'p reify_ast::ParsedModule,
    dag: &ModuleDag,
) -> Vec<&'p str> {
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
    followed
}

/// Append to the entry's `diagnostics` a re-homed copy of every Error and
/// Warning carried by a unit `dag` compiled, and record where each copy landed.
/// Walks the topological order, not just the direct imports, so a transitive
/// unit's diagnostic is not swallowed one level down.
fn rehome_unit_diagnostics(
    dag: &ModuleDag,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<RehomedDiagnostic> {
    let mut rehomed = Vec::new();
    for key in &dag.topo_order {
        let Some(module) = dag.modules.get(key) else {
            continue;
        };
        for diag in module
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error | Severity::Warning))
        {
            rehomed.push(RehomedDiagnostic {
                module_key: key.clone(),
                entry_index: diagnostics.len(),
            });
            diagnostics.push(rehome_imported(diag, key, dag.source(key)));
        }
    }
    rehomed
}

/// A copy of `diag`, raised while compiling the imported module `module_key`,
/// fit to live on the entry.
///
/// `Diagnostic` carries no file, so the file travels in the message: it is
/// prefixed `in imported module '<key>' (<path>:<line>:<col>)`, located by the
/// first label that indexes the imported file, or `(<path>)` when none does,
/// or `(embedded stdlib)` when the module has no source. Every label span is
/// moved to the prelude sentinel, because it indexes the imported file and
/// would otherwise mark a wrong offset in the entry. Severity, code and
/// candidates are kept, so an exit policy keyed on them treats the copy exactly
/// like the original.
fn rehome_imported(
    diag: &Diagnostic,
    module_key: &str,
    origin: Option<&ModuleSource>,
) -> Diagnostic {
    let location = match origin {
        None => "embedded stdlib".to_string(),
        Some(origin) => {
            let path = origin.path.display();
            let in_file = diag.labels.iter().find(|label| {
                !label.span.is_prelude() && (label.span.start as usize) <= origin.source.len()
            });
            match in_file {
                Some(label) => {
                    let (line, col) =
                        byte_offset_to_line_col(&origin.source, label.span.start as usize);
                    format!("{path}:{line}:{col}")
                }
                None => path.to_string(),
            }
        }
    };
    let mut rehomed = diag.clone();
    rehomed.message = format!(
        "in imported module '{module_key}' ({location}): {}",
        diag.message
    );
    for label in &mut rehomed.labels {
        label.span = SourceSpan::prelude();
    }
    rehomed
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{DiagnosticCode, DiagnosticLabel};

    /// While I4 holds only `std.*` resolves embedded, so `compile_program` never
    /// reaches the no-source branch; this pins it directly.
    #[test]
    fn rehome_without_source_names_the_embedded_stdlib() {
        let original = Diagnostic::warning("something odd")
            .with_code(DiagnosticCode::ImportedModuleParseError)
            .with_label(DiagnosticLabel::new(SourceSpan::new(3, 7), "here"));

        let rehomed = rehome_imported(&original, "std.units", None);

        assert_eq!(
            rehomed.message,
            "in imported module 'std.units' (embedded stdlib): something odd"
        );
        assert_eq!(rehomed.severity, original.severity);
        assert_eq!(rehomed.code, original.code);
        assert_eq!(rehomed.labels.len(), 1);
        assert_eq!(rehomed.labels[0].span, SourceSpan::prelude());
        assert_eq!(rehomed.labels[0].message, "here");
    }
}
