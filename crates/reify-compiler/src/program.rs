//! Program assembly: the single program-level compile entry point.
//!
//! [`compile_program`] compiles an entry module against its `#cfg`-satisfied
//! import DAG under strict stdlib visibility and returns a [`CompiledProgram`]:
//! the entry, the units of its import tier (each direct import followed by
//! what it re-exports through `pub import`, D-5 + D-7, std modules included),
//! and the source of every compiled user module (PRD
//! docs/prds/v0_6/resolution-unification.md §3.1). Every unit, the entry
//! included, sees builtins plus its import tier and nothing else, and a
//! `std.*` import resolves from the embedded stdlib only
//! (docs/prds/v0_6/stdlib-namespace.md NS-V1, κ #5503, which supersedes RU
//! I4). A std name a user module writes without importing it draws the NS-V2
//! fix-it Error. It upholds I2 (a broken import surfaces as a diagnostic on
//! the entry, never as a dropped compile) and I3 (a gated-out import is
//! inert). The entry holds only its own declarations; an import's definitions
//! stay in its unit, and `program_scope` binds them.

use std::path::{Path, PathBuf};

use reify_core::{Diagnostic, ModulePath, Severity, SourceSpan, byte_offset_to_line_col};
use reify_ir::ConstraintChecker;

use crate::CompiledModule;
use crate::cfg::CfgSet;
use crate::compile_builder::pre_pass::{ExpectedPathOrigin, attach_module_path_diag};
use crate::module_dag::{
    ModuleDag, ModuleResolver, ModuleSource, entry_module_name, is_std_import_path,
};

/// One module of a program entry's import tier, compiled.
#[derive(Debug, Clone)]
pub struct CompiledImportUnit {
    /// The dotted path that imports it: the entry's own import, or the
    /// re-exporting `pub import`. It equals the module key.
    pub import_path: String,
    /// How the entry reaches it.
    pub reach: ImportReach,
    /// The compiled module.
    pub module: CompiledModule,
    /// The file it was compiled from; `None` for an embedded std module.
    pub origin: Option<PathBuf>,
}

/// How a program's entry reaches one of its import units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportReach {
    /// The entry imports it.
    Direct,
    /// Reached only through a direct import's chain of `pub import` edges
    /// (resolution-unification D-7).
    ReExported,
}

/// A compiled program: an entry module plus everything it was compiled with.
#[derive(Debug, Clone)]
pub struct CompiledProgram {
    /// See [`Self::entry`].
    entry: CompiledModule,
    /// The entry's import tier in precedence order, one unit per module: each
    /// followed, successfully compiled direct import, in declaration order,
    /// followed depth-first by the units it re-exports through `pub import`
    /// edges. Std modules are listed like user ones, with no origin.
    pub imports: Vec<CompiledImportUnit>,
    /// The source of every user module compiled into the program from a
    /// file, in topological order (dependencies first), the entry last. A
    /// detached entry ([`compile_detached_entry`]) has no file and follows no
    /// user import, so its program lists none.
    pub sources: Vec<ModuleSource>,
    /// Where among [`Self::entry`]'s diagnostics the re-homed copies of the
    /// compiled user modules' own diagnostics sit.
    rehomed: RehomedBlock,
}

/// The entry's re-homed copies: one contiguous run of its diagnostics, the
/// copy at `start + i` raised compiling the user module `module_keys[i]`.
#[derive(Debug, Clone)]
struct RehomedBlock {
    start: usize,
    /// Dotted module keys, one per copy, in diagnostics order.
    module_keys: Vec<String>,
}

impl RehomedBlock {
    /// The run's index range in `diagnostics`, which must contain it.
    fn range_in(&self, diagnostics: &[Diagnostic]) -> std::ops::Range<usize> {
        let range = self.start..self.start + self.module_keys.len();
        assert!(
            range.end <= diagnostics.len(),
            "CompiledProgram invariant: the re-homed copies {range:?} must lie within the \
             entry's {} diagnostics",
            diagnostics.len(),
        );
        range
    }
}

impl CompiledProgram {
    /// Assemble a program, checking its invariant once up front; the accessors
    /// check it again where they rely on it.
    fn new(
        entry: CompiledModule,
        imports: Vec<CompiledImportUnit>,
        sources: Vec<ModuleSource>,
        rehomed: RehomedBlock,
    ) -> Self {
        let program = Self {
            entry,
            imports,
            sources,
            rehomed,
        };
        let _ = program.rehomed.range_in(&program.entry.diagnostics);
        program
    }

    /// The entry module, holding only its own declarations, with every import
    /// failure and definition collision attached to its diagnostics.
    pub fn entry(&self) -> &CompiledModule {
        &self.entry
    }

    /// The same module as [`Self::entry`], handed back by value. It holds only
    /// the entry's own declarations, so evaluating it resolves none of the
    /// program's imports.
    pub fn into_entry(self) -> CompiledModule {
        self.entry
    }

    /// The entry's diagnostics re-homed from the user module `module_key` (a
    /// direct or transitive import), in the order they were raised.
    // G-allow: P1 skeleton API for per-file diagnostics; consumer is GUI ο #5529
    pub fn rehomed_from(&self, module_key: &str) -> impl Iterator<Item = &Diagnostic> {
        let copies = &self.entry.diagnostics[self.rehomed.range_in(&self.entry.diagnostics)];
        self.rehomed
            .module_keys
            .iter()
            .zip(copies)
            .filter(move |(key, _)| key.as_str() == module_key)
            .map(|(_, diag)| diag)
    }

    /// The entry's diagnostics that are not re-homed copies: those raised
    /// compiling the entry itself, and the failures of its imports.
    // G-allow: P1 skeleton API for per-file diagnostics; consumer is GUI ο #5529
    pub fn entry_own_diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        let diagnostics = &self.entry.diagnostics;
        let range = self.rehomed.range_in(diagnostics);
        diagnostics[..range.start]
            .iter()
            .chain(&diagnostics[range.end..])
    }
}

/// Compile the program rooted at `entry_path`, taking the entry's text from
/// `entry_source` (an editor's dirty buffer works: `entry_path` need not exist
/// on disk, and only its file stem names the entry module).
///
/// User imports resolve from disk via `resolver`, `std.*` imports from the
/// embedded stdlib (never `resolver.stdlib_root`); imports whose `#cfg(...)`
/// predicates `cfg` does not satisfy are skipped entirely (not resolved, not
/// compiled, not in any prelude). Each user module, the entry included, is
/// compiled against builtins plus its own import tier and carries the NS-V2
/// fix-it for every std name it writes without importing.
///
/// **Diagnostics-embedded contract.** Nothing short-circuits. An entry parse
/// error reaches the diagnostics of [`CompiledProgram::entry`] through the
/// compiler's own parse-error forwarding, and a followed import that is missing
/// or broken surfaces as `Error` diagnostics on the entry while the entry
/// itself still compiles, so a caller's "any Error diagnostic, exit non-zero"
/// policy handles every failure uniformly. Every Error and Warning a compiled
/// user module carries — transitive modules included — is re-homed onto the
/// entry too, attributed to its file (see `rehome_imported`); an embedded std
/// module's diagnostics are never the user's, so none is re-homed;
/// [`CompiledProgram::rehomed_from`] and
/// [`CompiledProgram::entry_own_diagnostics`] tell those copies apart from the
/// entry's own diagnostics. After the copies comes the program's definition
/// collision report ([`crate::program_scope::collision_warnings`]), which is
/// entry-own too.
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
// G-allow: PRD §3.1 canonical program entry; first production consumer is AnalysisContext::from_parsed in reify-lsp (#8778)
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
/// `reify check` parses it for its own module-path gate first, and the GUI's
/// file-backed reload parses it to route the compile and keeps the parse for
/// source write-back). A caller need not seed an import's enum names into the
/// parse: an imported enum's `E.V` resolves in the compile phase
/// (resolution-unification D-9).
pub fn compile_program_with_parsed_entry(
    entry_path: &Path,
    entry_source: &str,
    parsed: &reify_ast::ParsedModule,
    resolver: &ModuleResolver,
    cfg: &CfgSet,
    checker: &dyn ConstraintChecker,
) -> CompiledProgram {
    let site = EntrySite::File {
        path: entry_path,
        source: entry_source,
        resolver,
    };
    compile_entry(parsed, site, cfg, checker)
}

/// Compile `parsed`, a buffer with no file, as a program entry under `cfg`.
///
/// It follows the entry's cfg-satisfied `std.*` imports from the embedded
/// stdlib, exactly as [`compile_program`] does, and never a user import: with
/// no file there is no directory to resolve one from
/// ([`detached_unfollowed_imports`] names those). There is no file location
/// for a module header to be checked against, so none is. Otherwise the
/// entry compiles as [`compile_program`] compiles it.
pub fn compile_detached_entry(
    parsed: &reify_ast::ParsedModule,
    cfg: &CfgSet,
    checker: &dyn ConstraintChecker,
) -> CompiledProgram {
    compile_entry(parsed, EntrySite::Detached, cfg, checker)
}

/// Compile `parsed`, an entry that follows no import under `cfg`, on its own:
/// against builtins only, with its NS-V2 fix-its and no module-header check.
///
/// It is [`compile_detached_entry`]'s entry, so a [`compile_program`] of the
/// same import-free entry differs from it only by that header check (I1).
pub fn compile_single_file(
    parsed: &reify_ast::ParsedModule,
    cfg: &CfgSet,
    checker: &dyn ConstraintChecker,
) -> CompiledModule {
    compile_detached_entry(parsed, cfg, checker).into_entry()
}

/// Where a program entry came from, which decides what it can follow.
#[derive(Clone, Copy)]
enum EntrySite<'a> {
    /// The file `path`, compiled from the text `source`: every cfg-satisfied
    /// import is followed, user ones through `resolver`.
    File {
        path: &'a Path,
        source: &'a str,
        resolver: &'a ModuleResolver,
    },
    /// A buffer with no file: only its `std.*` imports are followed.
    Detached,
}

impl EntrySite<'_> {
    /// `true` iff an entry from this site follows `import`, one its DAG
    /// follows.
    fn follows(self, import: &reify_ast::ImportDecl) -> bool {
        match self {
            Self::File { .. } => true,
            Self::Detached => is_std_import_path(&import.path),
        }
    }

    fn compile_import(self, dag: &mut ModuleDag, import_path: &str) -> Result<(), Vec<Diagnostic>> {
        match self {
            Self::File { resolver, .. } => dag.compile_module(import_path, resolver),
            Self::Detached => dag.follow_std(import_path),
        }
    }
}

/// The one entry compile [`compile_program_with_parsed_entry`] and
/// [`compile_detached_entry`] share.
fn compile_entry(
    parsed: &reify_ast::ParsedModule,
    site: EntrySite<'_>,
    cfg: &CfgSet,
    checker: &dyn ConstraintChecker,
) -> CompiledProgram {
    let mut dag = ModuleDag::with_embedded_std(cfg.clone());

    // Computed once and shared by the compile loop, the prelude and the
    // returned units, so they cannot drift.
    let followed: Vec<&str> = followed_direct_imports(parsed, &dag)
        .into_iter()
        .filter(|import| site.follows(import))
        .map(|import| import.path.as_str())
        .collect();

    // Collect, never early-return on, import failures: a broken import still
    // lets the entry compile, with the failure attached below.
    let mut import_error_diags: Vec<Diagnostic> = Vec::new();
    for import_path in &followed {
        if let Err(diags) = site.compile_import(&mut dag, import_path) {
            push_unreported(&mut import_error_diags, diags);
        }
    }

    // The entry's import tier (D-5, D-7), shared by the prelude and the
    // returned units so check and eval see the same modules. A
    // satisfied-but-failed import is absent from `dag.modules`; `filter_map`
    // skips it (its failure is already collected).
    let visible = dag.visible_imports(&followed);
    let import_refs: Vec<&CompiledModule> = visible
        .iter()
        .filter_map(|path| dag.modules.get(path))
        .collect();
    let mut entry = crate::compile_with_prelude_refs_checked(parsed, &import_refs, checker);
    let fixits = crate::import_headers::std_import_fixits(parsed, &entry, &import_refs, cfg);
    entry.diagnostics.extend(fixits);

    if let EntrySite::File { .. } = site {
        attach_module_path_diag(&mut entry, parsed, ExpectedPathOrigin::FileLocation);
    }
    entry.diagnostics.extend(import_error_diags);
    let rehomed = rehome_unit_diagnostics(&dag, &mut entry.diagnostics);

    let mut sources: Vec<ModuleSource> = dag
        .topo_order
        .iter()
        .filter_map(|key| dag.source(key).cloned())
        .collect();
    if let EntrySite::File { path, source, .. } = site {
        sources.push(ModuleSource {
            module_key: parsed.path.0.join("."),
            path: path.to_path_buf(),
            source: source.to_string(),
        });
    }

    let imports: Vec<CompiledImportUnit> = visible
        .into_iter()
        .filter_map(|path| {
            let origin = dag.source(&path).map(|s| s.path.clone());
            let reach = if followed.contains(&path.as_str()) {
                ImportReach::Direct
            } else {
                ImportReach::ReExported
            };
            dag.modules.remove(&path).map(|module| CompiledImportUnit {
                import_path: path,
                reach,
                module,
                origin,
            })
        })
        .collect();

    let mut program = CompiledProgram::new(entry, imports, sources, rehomed);
    let collisions = crate::program_scope::collision_warnings(&program);
    program.entry.diagnostics.extend(collisions);
    program
}

/// True iff compiling `parsed` as a program entry under `cfg` follows at least
/// one import.
pub fn entry_follows_imports(parsed: &reify_ast::ParsedModule, cfg: &CfgSet) -> bool {
    !followed_imports(parsed, cfg).is_empty()
}

/// The entry imports [`compile_program`] follows under `cfg` (the
/// cfg-satisfied ones, `std.*` included), in declaration order, one per path.
pub fn followed_imports<'p>(
    parsed: &'p reify_ast::ParsedModule,
    cfg: &CfgSet,
) -> Vec<&'p reify_ast::ImportDecl> {
    followed_direct_imports(parsed, &ModuleDag::with_embedded_std(cfg.clone()))
}

/// The imports [`compile_program`] would follow under `cfg` that
/// [`compile_detached_entry`] cannot: the user ones, which need a file to
/// resolve from. In declaration order, one per path.
pub fn detached_unfollowed_imports<'p>(
    parsed: &'p reify_ast::ParsedModule,
    cfg: &CfgSet,
) -> Vec<&'p reify_ast::ImportDecl> {
    followed_imports(parsed, cfg)
        .into_iter()
        .filter(|import| !EntrySite::Detached.follows(import))
        .collect()
}

/// The resolver a program compile of the entry file `entry_path` takes: user
/// imports resolve from the entry's directory. Its stdlib root,
/// `<dir>/crates/reify-compiler/stdlib`, is never read, because a program
/// compile resolves `std.*` from the embedded stdlib only.
pub fn entry_dir_resolver(entry_path: &Path) -> ModuleResolver {
    let dir = entry_path.parent().unwrap_or_else(|| Path::new("."));
    ModuleResolver::new(dir, dir.join("crates/reify-compiler/stdlib"))
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
) -> Vec<&'p reify_ast::ImportDecl> {
    let mut followed: Vec<&reify_ast::ImportDecl> = Vec::new();
    for import in parsed.declarations.iter().filter_map(|decl| match decl {
        reify_ast::Declaration::Import(import) => Some(import),
        _ => None,
    }) {
        let already = followed.iter().any(|seen| seen.path == import.path);
        if dag.follows_import(import) && !already {
            followed.push(import);
        }
    }
    followed
}

/// Append to the entry's `diagnostics`, as one contiguous run, a re-homed copy
/// of every Error and Warning carried by a file-backed unit `dag` compiled,
/// and return where that run sits. Walks the topological order, not just the
/// direct imports, so a transitive unit's diagnostic is not swallowed one
/// level down. An embedded std unit has no source and is skipped: its
/// diagnostics are not the user's to fix.
fn rehome_unit_diagnostics(dag: &ModuleDag, diagnostics: &mut Vec<Diagnostic>) -> RehomedBlock {
    let start = diagnostics.len();
    let mut module_keys = Vec::new();
    for key in &dag.topo_order {
        let (Some(module), Some(origin)) = (dag.modules.get(key), dag.source(key)) else {
            continue;
        };
        for diag in module
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error | Severity::Warning))
        {
            module_keys.push(key.clone());
            diagnostics.push(rehome_imported(diag, key, origin));
        }
    }
    RehomedBlock { start, module_keys }
}

/// A copy of `diag`, raised while compiling the imported module `module_key`,
/// fit to live on the entry.
///
/// `Diagnostic` carries no file, so the file travels in the message: it is
/// prefixed `in imported module '<key>' (<path>:<line>:<col>)`, located by the
/// first label that indexes the imported file, or `(<path>)` when none does.
/// Every label span is
/// moved to the prelude sentinel, because it indexes the imported file and
/// would otherwise mark a wrong offset in the entry. Severity, code and
/// candidates are kept, so an exit policy keyed on them treats the copy exactly
/// like the original.
fn rehome_imported(diag: &Diagnostic, module_key: &str, origin: &ModuleSource) -> Diagnostic {
    let path = origin.path.display();
    let in_file = diag.labels.iter().find(|label| {
        !label.span.is_prelude() && (label.span.start as usize) <= origin.source.len()
    });
    let location = match in_file {
        Some(label) => {
            let (line, col) = byte_offset_to_line_col(&origin.source, label.span.start as usize);
            format!("{path}:{line}:{col}")
        }
        None => path.to_string(),
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

    /// `module main` declaring one template, `Rig`, and importing nothing.
    fn compile_one_template_entry() -> CompiledProgram {
        let dir = tempfile::tempdir().expect("tempdir");
        compile_program(
            &dir.path().join("main.ri"),
            "module main\nstructure def Rig {\n    param x : Length = 1mm\n}\n",
            &ModuleResolver::new(dir.path(), dir.path().join("stdlib")),
            &CfgSet::default(),
            &crate::compile_builder::auto_type_param_phase::CompileTimeIndeterminateChecker,
        )
    }

    fn rehomed_block(start: usize, module_keys: &[&str]) -> RehomedBlock {
        RehomedBlock {
            start,
            module_keys: module_keys.iter().map(|key| key.to_string()).collect(),
        }
    }

    fn warnings(count: usize) -> Vec<Diagnostic> {
        (0..count)
            .map(|i| Diagnostic::warning(format!("warning {i}")))
            .collect()
    }

    #[test]
    fn rehomed_block_range_is_its_run_of_copies() {
        let diagnostics = warnings(4);

        assert_eq!(rehomed_block(1, &["a", "b"]).range_in(&diagnostics), 1..3);
        assert_eq!(rehomed_block(2, &["a", "b"]).range_in(&diagnostics), 2..4);
        assert_eq!(rehomed_block(4, &[]).range_in(&diagnostics), 4..4);
    }

    #[test]
    #[should_panic(
        expected = "CompiledProgram invariant: the re-homed copies 1..3 must lie within \
                    the entry's 2 diagnostics"
    )]
    fn rehomed_block_outside_the_diagnostics_names_the_invariant() {
        let _ = rehomed_block(1, &["a", "b"]).range_in(&warnings(2));
    }

    /// The invariants are checked where a program is built, not only where it
    /// is read.
    #[test]
    #[should_panic(expected = "CompiledProgram invariant: the re-homed copies")]
    fn compiled_program_new_rejects_a_rehomed_block_past_the_diagnostics() {
        let program = compile_one_template_entry();
        let len = program.entry.diagnostics.len();

        let _ = CompiledProgram::new(
            program.entry,
            Vec::new(),
            Vec::new(),
            rehomed_block(len + 1, &["a"]),
        );
    }
}
