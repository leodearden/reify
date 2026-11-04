//! Import-header derivation (stdlib-namespace ε, PRD
//! `docs/prds/v0_6/stdlib-namespace.md` §4 D-9): which `import std.X` lines a
//! module needs, derived from the names it writes.
//!
//! A purely syntactic walk ([`name_uses`]) finds every name the module writes
//! and the position it writes it in; [`attribution`] ranks the definitions of
//! each name through `name_env`, so only names whose winning definition lives
//! in a std module need an import.

mod attribution;
mod name_uses;

use std::collections::{BTreeMap, BTreeSet};

use reify_ast::ParsedModule;
use reify_core::ModulePath;

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

    pub fn required(&self) -> &[RequiredImport] {
        &self.required
    }
}

/// Derive the std imports `parsed` needs, attributing each written name to
/// its winning definition over `parsed`'s own definitions and `universe`.
/// A module of `universe` is std-tier exactly when its path starts with `std`.
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
