//! Which std module, if any, a written name needs.
//!
//! [`role_rule`] is the one table from a name's position to the definition
//! kinds it can refer to, the builtins that position resolves first, and the
//! module-local binders that shadow it. Definitions are ranked by
//! `name_env::bind`, so a local or user-import definition beats a std one
//! exactly as NS-P1 ranks them during compilation.

use std::collections::HashMap;

use reify_core::ModulePath;
use reify_core::units::unit_symbol_to_si;

use super::name_uses::{NameRole, NameUse, NameUses};
use crate::name_env::{self, Definition, Origin, Tier};
use crate::type_resolution::{is_parameterized_builtin_name, resolve_type_name};
use crate::types::CompiledModule;

/// A namespace a written name can be looked up in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ExportKind {
    Template,
    Enum,
    EnumVariant,
    Trait,
    TypeAlias,
    Function,
    ConstraintDef,
    Field,
    Unit,
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
    /// `None` where every definition outranks a local binding, as in a call
    /// head, which resolves through templates and functions before any local.
    shadowed_by: Option<Binders>,
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
            shadowed_by: Some(Binders::Type),
        },
        NameRole::Trait => RoleRule {
            searches: &[ExportKind::Trait],
            is_builtin: no_builtin,
            shadowed_by: Some(Binders::Type),
        },
        NameRole::Callee => RoleRule {
            searches: &[
                ExportKind::Function,
                ExportKind::Template,
                ExportKind::ConstraintDef,
                ExportKind::EnumVariant,
                ExportKind::Field,
            ],
            is_builtin: no_builtin,
            shadowed_by: None,
        },
        NameRole::Value => RoleRule {
            searches: &[
                ExportKind::Function,
                ExportKind::Enum,
                ExportKind::EnumVariant,
                ExportKind::Field,
            ],
            is_builtin: no_builtin,
            shadowed_by: Some(Binders::Value),
        },
        NameRole::Enum => RoleRule {
            searches: &[ExportKind::Enum],
            is_builtin: no_builtin,
            shadowed_by: None,
        },
        NameRole::Variant => RoleRule {
            searches: &[ExportKind::EnumVariant],
            is_builtin: no_builtin,
            shadowed_by: None,
        },
        NameRole::BareUnit => RoleRule {
            searches: &[ExportKind::Unit],
            is_builtin: is_builtin_unit_symbol,
            shadowed_by: None,
        },
        NameRole::UnitFactor => RoleRule {
            searches: &[ExportKind::Unit],
            is_builtin: no_builtin,
            shadowed_by: None,
        },
    }
}

/// Type position consults the builtin type names before any prelude
/// definition (`resolve_type_with_params`), so `param g : Solid` binds the
/// builtin geometry type even where a std trait is named `Solid`.
fn is_builtin_type_name(name: &str) -> bool {
    resolve_type_name(name).is_some() || is_parameterized_builtin_name(name)
}

/// A bare quantity unit missing from the unit registry falls back to the
/// builtin unit table, so `2mm` resolves with no unit module at all.
fn is_builtin_unit_symbol(name: &str) -> bool {
    unit_symbol_to_si(name).is_some()
}

fn no_builtin(_: &str) -> bool {
    false
}

/// The definitions every name can bind to, per kind, over one module's own
/// definitions and a universe of compiled modules: the `name_env` winners for
/// its kinds, and every exporter for units and fields, which are deliberately
/// not `name_env` kinds.
pub(crate) struct ExportIndex<'a> {
    origins: HashMap<ExportKind, HashMap<&'a str, Vec<Origin<'a>>>>,
}

impl<'a> ExportIndex<'a> {
    pub(crate) fn new(local: &'a CompiledModule, universe: &[&'a CompiledModule]) -> Self {
        let mut index = Self {
            origins: HashMap::new(),
        };
        index.add_winners(ExportKind::Template, &local.templates, universe, |t| {
            &t.name
        });
        index.add_winners(ExportKind::Trait, &local.trait_defs, universe, |t| &t.name);
        index.add_winners(ExportKind::TypeAlias, &local.type_aliases, universe, |a| {
            &a.name
        });
        index.add_winners(ExportKind::Function, &local.functions, universe, |f| {
            &f.name
        });
        index.add_winners(
            ExportKind::ConstraintDef,
            &local.constraint_defs,
            universe,
            |c| &c.name,
        );

        // A variant belongs to whichever enum wins the enum's name.
        let enums = name_env::bind(&local.enum_defs, universe.iter().copied());
        for candidate in enums.winning_candidates() {
            index.add(
                ExportKind::Enum,
                &candidate.definition.name,
                candidate.origin,
            );
            for variant in &candidate.definition.variants {
                index.add(ExportKind::EnumVariant, &variant.name, candidate.origin);
            }
        }

        for unit in &local.units {
            index.add(ExportKind::Unit, &unit.name, Origin::Local);
        }
        for field in &local.fields {
            index.add(ExportKind::Field, &field.name, Origin::Local);
        }
        for module in universe {
            let origin = Origin::Module(&module.path);
            for unit in module.units.iter().filter(|unit| unit.is_pub) {
                index.add(ExportKind::Unit, &unit.name, origin);
            }
            for field in module.fields.iter().filter(|field| field.is_pub) {
                index.add(ExportKind::Field, &field.name, origin);
            }
        }
        index
    }

    fn add(&mut self, kind: ExportKind, name: &'a str, origin: Origin<'a>) {
        self.origins
            .entry(kind)
            .or_default()
            .entry(name)
            .or_default()
            .push(origin);
    }

    fn add_winners<D: Definition>(
        &mut self,
        kind: ExportKind,
        local: &'a [D],
        universe: &[&'a CompiledModule],
        name_of: fn(&'a D) -> &'a str,
    ) {
        let bindings = name_env::bind(local, universe.iter().copied());
        for candidate in bindings.winning_candidates() {
            self.add(kind, name_of(candidate.definition), candidate.origin);
        }
    }

    fn origins(&self, kind: ExportKind, name: &str) -> &[Origin<'a>] {
        self.origins
            .get(&kind)
            .and_then(|by_name| by_name.get(name))
            .map_or(&[], Vec::as_slice)
    }
}

/// The std modules `name_use` needs imported: none when a builtin, a module
/// binder, a local definition or a user-import definition supplies the name;
/// otherwise every std module whose definition can bind it.
pub(crate) fn defining_std_modules<'a>(
    name_use: &NameUse,
    uses: &NameUses,
    index: &ExportIndex<'a>,
) -> Vec<&'a ModulePath> {
    let rule = role_rule(name_use.role);
    let name = name_use.name.as_str();
    let shadowed = match rule.shadowed_by {
        Some(Binders::Value) => uses.value_binders.contains(name),
        Some(Binders::Type) => uses.type_binders.contains(name),
        None => false,
    };
    if (rule.is_builtin)(name) || shadowed {
        return Vec::new();
    }
    let origins: Vec<Origin<'a>> = rule
        .searches
        .iter()
        .flat_map(|&kind| index.origins(kind, name).iter().copied())
        .collect();
    if origins.iter().any(|origin| origin.tier() != Tier::Stdlib) {
        return Vec::new();
    }
    origins.iter().filter_map(Origin::module).collect()
}
