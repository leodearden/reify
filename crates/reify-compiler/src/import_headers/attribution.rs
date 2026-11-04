//! Which std module, if any, a written name needs.
//!
//! [`role_rule`] is the one table from a name's position to the definition
//! kinds it can refer to, the builtins that position resolves first, and the
//! module-local binders that shadow it. Definitions are ranked by
//! `name_env::bind`, so a local or user-import definition beats a std one
//! exactly as NS-P1 ranks them during compilation.

use std::collections::HashMap;

use reify_core::ModulePath;

use super::name_uses::{NameRole, NameUse, NameUses};
use crate::name_env::{self, Definition, Origin, Tier};
use crate::type_resolution::{is_parameterized_builtin_name, resolve_type_name};
use crate::types::CompiledModule;

/// A namespace a written name can be looked up in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ExportKind {
    Template,
    Enum,
    Trait,
    TypeAlias,
}

/// Which module-local binder set shadows a role's names.
#[derive(Clone, Copy, Debug)]
enum Binders {
    Value,
    Type,
}

struct RoleRule {
    searches: &'static [ExportKind],
    /// Names this position resolves to a builtin before any definition.
    is_builtin: fn(&str) -> bool,
    shadowed_by: Binders,
}

fn role_rule(role: NameRole) -> RoleRule {
    match role {
        NameRole::Type => RoleRule {
            searches: &[
                ExportKind::Template,
                ExportKind::Enum,
                ExportKind::Trait,
                ExportKind::TypeAlias,
            ],
            is_builtin: is_builtin_type_name,
            shadowed_by: Binders::Type,
        },
        NameRole::Trait => RoleRule {
            searches: &[ExportKind::Trait],
            is_builtin: no_builtin,
            shadowed_by: Binders::Type,
        },
        NameRole::Callee
        | NameRole::Value
        | NameRole::Enum
        | NameRole::Variant
        | NameRole::BareUnit
        | NameRole::UnitFactor => RoleRule {
            searches: &[],
            is_builtin: no_builtin,
            shadowed_by: Binders::Value,
        },
    }
}

/// Type position consults the builtin type names before any prelude
/// definition (`resolve_type_with_params`), so `param g : Solid` binds the
/// builtin geometry type even where a std trait is named `Solid`.
fn is_builtin_type_name(name: &str) -> bool {
    resolve_type_name(name).is_some() || is_parameterized_builtin_name(name)
}

fn no_builtin(_: &str) -> bool {
    false
}

/// The winning definition of every name, per kind, over one module's own
/// definitions and a universe of compiled modules.
pub(crate) struct ExportIndex<'a> {
    winners: HashMap<ExportKind, HashMap<&'a str, Vec<Origin<'a>>>>,
}

impl<'a> ExportIndex<'a> {
    pub(crate) fn new(local: &'a CompiledModule, universe: &[&'a CompiledModule]) -> Self {
        let mut index = Self {
            winners: HashMap::new(),
        };
        index.add_bound(ExportKind::Template, &local.templates, universe, |t| {
            &t.name
        });
        index.add_bound(ExportKind::Enum, &local.enum_defs, universe, |e| &e.name);
        index.add_bound(ExportKind::Trait, &local.trait_defs, universe, |t| &t.name);
        index.add_bound(ExportKind::TypeAlias, &local.type_aliases, universe, |a| {
            &a.name
        });
        index
    }

    fn add_bound<D: Definition>(
        &mut self,
        kind: ExportKind,
        local: &'a [D],
        universe: &[&'a CompiledModule],
        name_of: fn(&'a D) -> &'a str,
    ) {
        let bindings = name_env::bind(local, universe.iter().copied());
        for candidate in bindings.winning_candidates() {
            self.winners
                .entry(kind)
                .or_default()
                .entry(name_of(candidate.definition))
                .or_default()
                .push(candidate.origin);
        }
    }

    fn winners(&self, kind: ExportKind, name: &str) -> &[Origin<'a>] {
        self.winners
            .get(&kind)
            .and_then(|by_name| by_name.get(name))
            .map_or(&[], Vec::as_slice)
    }
}

/// The std modules `name_use` needs imported: none when a builtin, a module
/// binder, a local definition or a user-import definition supplies the name;
/// otherwise every std module whose definition wins the name.
pub(crate) fn defining_std_modules<'a>(
    name_use: &NameUse,
    uses: &NameUses,
    index: &ExportIndex<'a>,
) -> Vec<&'a ModulePath> {
    let rule = role_rule(name_use.role);
    let name = name_use.name.as_str();
    let binders = match rule.shadowed_by {
        Binders::Value => &uses.value_binders,
        Binders::Type => &uses.type_binders,
    };
    if (rule.is_builtin)(name) || binders.contains(name) {
        return Vec::new();
    }
    let origins: Vec<Origin<'a>> = rule
        .searches
        .iter()
        .flat_map(|&kind| index.winners(kind, name).iter().copied())
        .collect();
    if origins.iter().any(|origin| origin.tier() != Tier::Stdlib) {
        return Vec::new();
    }
    origins.iter().filter_map(Origin::module).collect()
}
