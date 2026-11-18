//! Import-header derivation (stdlib-namespace ε, PRD
//! `docs/prds/v0_6/stdlib-namespace.md` §4 D-9): which `import std.X` lines a
//! module needs, derived from the names it writes.
//!
//! A purely syntactic walk ([`name_uses`]) finds every name the module writes
//! and the position it writes it in; [`attribution`] ranks the definitions of
//! each name through `name_env`, so only names whose winning definition lives
//! in a std module need an import. [`coverage`] decides which of those names
//! a module's own `import` declarations, those in effect under the active
//! cfg, already make visible.

mod attribution;
mod coverage;
mod header_edit;
mod name_uses;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use reify_ast::ParsedModule;
use reify_core::{ContentHash, Diagnostic, DiagnosticIdentity, ModulePath, Severity};

pub use name_uses::{NameRole, NameUse};

use crate::cfg::CfgSet;
use crate::types::CompiledModule;

/// One std module a module needs, with the written names that need it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequiredImport {
    pub module: ModulePath,
    /// Sorted and deduplicated.
    pub names: Vec<NameUse>,
}

/// The exact module an [`ImportPlan`] was derived for: its path and the hash
/// of its source.
#[derive(Clone, PartialEq, Eq)]
struct Subject {
    path: ModulePath,
    content_hash: ContentHash,
}

impl Subject {
    fn of(parsed: &ParsedModule) -> Self {
        Self {
            path: parsed.path.clone(),
            content_hash: parsed.content_hash,
        }
    }

    /// # Panics
    ///
    /// If `parsed` is not this subject: another path, or the same path with
    /// edited source.
    fn assert_is(&self, parsed: &ParsedModule) {
        let given = Self::of(parsed);
        if *self == given {
            return;
        }
        let source_changed = if self.path == given.path {
            format!(
                ": the paths match but the source changed ({:?} at derive, {:?} given); \
                 re-derive the plan for the edited source",
                self.content_hash, given.content_hash
            )
        } else {
            String::new()
        };
        panic!(
            "ImportPlan::residual_diagnostics judges only the module the plan was derived for: \
             derived for `{}`, given `{}`{source_changed}",
            self.path.to_dotted(),
            given.path.to_dotted()
        );
    }
}

/// The std-import derivation of one module over one universe.
#[derive(Clone)]
pub struct ImportPlan<'u> {
    subject: Subject,
    required: Vec<RequiredImport>,
    universe: Vec<&'u CompiledModule>,
    whole_universe_diagnostics: Vec<Diagnostic>,
}

impl<'u> ImportPlan<'u> {
    /// The std modules the module needs, sorted by dotted path.
    pub fn modules(&self) -> Vec<ModulePath> {
        self.required
            .iter()
            .map(|required| required.module.clone())
            .collect()
    }

    /// The required imports `parsed` does not yet satisfy, each narrowed to
    /// the names its imports in effect under `cfg` leave uncovered.
    ///
    /// # Panics
    ///
    /// If `parsed` declares an import whose path is not a module path, which
    /// only a module with parse errors can.
    pub fn missing_from(&self, parsed: &ParsedModule, cfg: &CfgSet) -> Vec<RequiredImport> {
        let coverage = coverage::ImportCoverage::of(parsed, cfg);
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

    /// The Error and Warning diagnostics `parsed` gets when compiled against
    /// only the `imports` modules of the plan's universe, less those the
    /// whole universe gives it too (matched one for one on their
    /// [`DiagnosticIdentity`]). Empty when the imports supply everything the
    /// module needs from the universe; a diagnostic the module raises
    /// regardless is never counted against them.
    ///
    /// # Panics
    ///
    /// If `parsed` is not the module the plan was derived for, at the same
    /// path with the same source: the whole-universe diagnostics are that
    /// exact module's, so a module whose source was edited, for example by
    /// [`insert_imports`], needs a fresh plan.
    pub fn residual_diagnostics(
        &self,
        parsed: &ParsedModule,
        imports: &[ModulePath],
    ) -> Vec<Diagnostic> {
        self.subject.assert_is(parsed);
        let imported: Vec<&CompiledModule> = self
            .universe
            .iter()
            .copied()
            .filter(|module| imports.contains(&module.path))
            .collect();
        let restricted = crate::compile_with_prelude_refs(parsed, &imported);

        let mut unmatched: HashMap<DiagnosticIdentity<'_>, usize> = HashMap::new();
        for diagnostic in &self.whole_universe_diagnostics {
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
}

/// Derive the std imports `parsed` needs, attributing each written name to
/// its winning definition over `parsed`'s own definitions and `universe`.
/// Which modules of `universe` are std-tier is
/// [`name_env::Origin::tier`](crate::name_env::Origin::tier)'s decision.
pub fn derive_std_imports<'u>(
    parsed: &ParsedModule,
    universe: &[&'u CompiledModule],
) -> ImportPlan<'u> {
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
        subject: Subject::of(parsed),
        required: by_module
            .into_values()
            .map(|(module, names)| RequiredImport {
                module,
                names: names.into_iter().collect(),
            })
            .collect(),
        universe: universe.to_vec(),
        whole_universe_diagnostics: local
            .diagnostics
            .into_iter()
            .filter(is_reportable)
            .collect(),
    }
}

/// The module path of every `import` `parsed` declares whose `#cfg`
/// predicates `cfg` satisfies, in any form.
///
/// # Panics
///
/// If `parsed` declares an import whose path is not a module path, which
/// only a module with parse errors can.
pub fn cfg_satisfied_imports(parsed: &ParsedModule, cfg: &CfgSet) -> Vec<ModulePath> {
    coverage::imports(parsed, cfg)
        .map(|(module, _)| module)
        .collect()
}

/// The module path of every `import` `parsed` declares whose `#cfg`
/// predicates `cfg` does not satisfy, in any form: the imports
/// [`cfg_satisfied_imports`] leaves out, which cover nothing under `cfg`.
///
/// # Panics
///
/// If `parsed` declares an import whose path is not a module path, which
/// only a module with parse errors can.
pub fn cfg_unsatisfied_imports(parsed: &ParsedModule, cfg: &CfgSet) -> Vec<ModulePath> {
    coverage::gated_off(parsed, cfg).collect()
}

/// Every compiled stdlib module except `module`, in load order: the universe
/// a stdlib module's own imports are derived against.
pub fn stdlib_universe_excluding(module: &ModulePath) -> Vec<&'static CompiledModule> {
    crate::stdlib_loader::load_stdlib()
        .iter()
        .filter(|candidate| &candidate.path != module)
        .collect()
}

fn is_reportable(diagnostic: &Diagnostic) -> bool {
    matches!(diagnostic.severity, Severity::Error | Severity::Warning)
}

/// `source` with an `import` line added for every module of `modules` it
/// does not already import whole under `cfg`, through a namespace import (an
/// entity import, or a gated import `cfg` does not satisfy, does not count),
/// in the order given: after the last import, else after the `module`
/// declaration, else after the leading module pragmas above the line holding
/// the first leading `#cfg`, else after the leading comment block's last blank
/// line.
///
/// # Panics
///
/// If `parsed` declares an import whose path is not a module path, which
/// only a module with parse errors can.
pub fn insert_imports(
    source: &str,
    parsed: &ParsedModule,
    cfg: &CfgSet,
    modules: &[ModulePath],
) -> String {
    header_edit::insert_imports(source, parsed, cfg, modules)
}
