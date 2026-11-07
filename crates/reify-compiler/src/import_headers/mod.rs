//! Import-header derivation (stdlib-namespace ε, PRD
//! `docs/prds/v0_6/stdlib-namespace.md` §4 D-9): which `import std.X` lines a
//! module needs, derived from the names it writes.
//!
//! A purely syntactic walk ([`name_uses`]) finds every name the module writes
//! and the position it writes it in; [`attribution`] ranks the definitions of
//! each name through `name_env`, so only names whose winning definition lives
//! in a std module need an import. [`coverage`] decides which of those names
//! a module's own `import` declarations already make visible.

mod attribution;
mod coverage;
mod header_edit;
mod name_uses;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use reify_ast::ParsedModule;
use reify_core::{Diagnostic, DiagnosticIdentity, ModulePath, Severity};

pub use name_uses::{NameRole, NameUse};

use crate::types::CompiledModule;

/// One std module a module needs, with the written names that need it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequiredImport {
    pub module: ModulePath,
    /// Sorted and deduplicated.
    pub names: Vec<NameUse>,
}

/// The std modules a module needs, sorted by dotted path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportPlan {
    required: Vec<RequiredImport>,
}

impl ImportPlan {
    pub fn modules(&self) -> Vec<ModulePath> {
        self.required
            .iter()
            .map(|required| required.module.clone())
            .collect()
    }

    /// The required imports `parsed` does not yet satisfy, each narrowed to
    /// the names its declared imports leave uncovered.
    pub fn missing_from(&self, parsed: &ParsedModule) -> Vec<RequiredImport> {
        let coverage = coverage::ImportCoverage::of(parsed);
        self.required
            .iter()
            .filter_map(|required| {
                let names: Vec<NameUse> = required
                    .names
                    .iter()
                    .filter(|name_use| !coverage.covers(&required.module, &name_use.name))
                    .cloned()
                    .collect();
                (!names.is_empty()).then(|| RequiredImport {
                    module: required.module.clone(),
                    names,
                })
            })
            .collect()
    }
}

/// Derive the std imports `parsed` needs, attributing each written name to
/// its winning definition over `parsed`'s own definitions and `universe`.
/// Which modules of `universe` are std-tier is
/// [`name_env::Origin::tier`](crate::name_env::Origin::tier)'s decision.
pub fn derive_std_imports(parsed: &ParsedModule, universe: &[&CompiledModule]) -> ImportPlan {
    let local = crate::compile_with_prelude_refs(parsed, universe);
    let index = attribution::ExportIndex::new(&local, universe);
    let uses = name_uses::collect_name_uses(parsed);

    let mut by_module: BTreeMap<String, (ModulePath, BTreeSet<NameUse>)> = BTreeMap::new();
    for name_use in &uses.uses {
        for module in attribution::defining_std_modules(name_use, &uses, &index) {
            by_module
                .entry(module.to_dotted())
                .or_insert_with(|| (module.clone(), BTreeSet::new()))
                .1
                .insert(name_use.clone());
        }
    }
    ImportPlan {
        required: by_module
            .into_values()
            .map(|(module, names)| RequiredImport {
                module,
                names: names.into_iter().collect(),
            })
            .collect(),
    }
}

/// The module path of every `import` `parsed` declares, in any form.
pub fn declared_imports(parsed: &ParsedModule) -> Vec<ModulePath> {
    coverage::imports(parsed)
        .map(|(module, _)| module)
        .collect()
}

/// Every compiled stdlib module except `module`, in load order: the universe
/// a stdlib module's own imports are derived against.
pub fn stdlib_universe_excluding(module: &ModulePath) -> Vec<&'static CompiledModule> {
    crate::stdlib_loader::load_stdlib()
        .iter()
        .filter(|candidate| &candidate.path != module)
        .collect()
}

/// The Error and Warning diagnostics `parsed` gets when compiled against only
/// the `imports` modules of `universe`, less those the whole `universe` gives
/// it too (matched one for one on their [`DiagnosticIdentity`]). Empty when the
/// imports supply everything the module needs from `universe`; a diagnostic
/// the module raises regardless is never counted against them.
pub fn residual_diagnostics(
    parsed: &ParsedModule,
    imports: &[ModulePath],
    universe: &[&CompiledModule],
) -> Vec<Diagnostic> {
    let imported: Vec<&CompiledModule> = universe
        .iter()
        .copied()
        .filter(|module| imports.contains(&module.path))
        .collect();
    let restricted = crate::compile_with_prelude_refs(parsed, &imported);
    let full = crate::compile_with_prelude_refs(parsed, universe);

    let mut unmatched: HashMap<DiagnosticIdentity<'_>, usize> = HashMap::new();
    for diagnostic in full.diagnostics.iter().filter(|d| is_reportable(d)) {
        *unmatched
            .entry(DiagnosticIdentity::of(diagnostic))
            .or_default() += 1;
    }
    restricted
        .diagnostics
        .iter()
        .filter(|d| is_reportable(d))
        .filter(
            |diagnostic| match unmatched.get_mut(&DiagnosticIdentity::of(diagnostic)) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    false
                }
                _ => true,
            },
        )
        .cloned()
        .collect()
}

fn is_reportable(diagnostic: &Diagnostic) -> bool {
    matches!(diagnostic.severity, Severity::Error | Severity::Warning)
}

/// `source` with an `import` line added for every module of `modules` it
/// does not already import whole, through a namespace import (an entity
/// import of the module does not count), in the order given: after the last
/// import, else after the `module` declaration, else after the leading module
/// pragmas, else after the leading comment block's last blank line.
pub fn insert_imports(source: &str, parsed: &ParsedModule, modules: &[ModulePath]) -> String {
    header_edit::insert_imports(source, parsed, modules)
}
