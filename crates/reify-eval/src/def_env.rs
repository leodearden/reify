//! The definition environment: a read-only view of every template a compiled
//! program can name.
//!
//! This is the P1 skeleton (PRD docs/prds/v0_6/resolution-unification.md §3.2,
//! D-3); θ #5522 makes it authoritative for the engine. Resolution order is the
//! entry, then its imports in declaration order, then the stdlib (I6), and every
//! scope is keyed internally by `(module_key, name)` (I7) — the seam
//! stdlib-namespace ν #5505 consumes for qualified lookup.

use std::collections::HashMap;
use std::iter;

use reify_compiler::program::CompiledProgram;
use reify_compiler::stdlib_loader::load_stdlib;
use reify_compiler::{CompiledModule, TopologyTemplate, Visibility};

/// Every template a [`CompiledProgram`] can name, in precedence order.
///
/// The entry's scope holds its whole template list — in P1 the merged list,
/// which already carries the direct imports' pub templates (interim I5 until
/// κ #5524). Each import's scope holds its pub templates only; each stdlib
/// module's scope holds all of its templates, as eval's prelude fallback does.
// G-allow: P1 DefEnv skeleton (resolution-unification D-3); first production consumer is θ #5522
pub struct DefEnv<'p> {
    declared: &'p CompiledModule,
    scopes: Vec<ModuleScope<'p>>,
}

/// One module's visible templates, first declaration winning per name (the
/// same first-match rule as `find_template`).
struct ModuleScope<'p> {
    module_key: String,
    templates: HashMap<&'p str, &'p TopologyTemplate>,
}

impl<'p> ModuleScope<'p> {
    fn new(module_key: String, templates: impl IntoIterator<Item = &'p TopologyTemplate>) -> Self {
        let mut by_name = HashMap::new();
        for template in templates {
            by_name.entry(template.name.as_str()).or_insert(template);
        }
        Self {
            module_key,
            templates: by_name,
        }
    }
}

impl<'p> DefEnv<'p> {
    /// Build the environment for `program`: the entry, then each import unit in
    /// declaration order, then each stdlib module in load order.
    pub fn from_program(program: &'p CompiledProgram) -> Self {
        let entry = ModuleScope::new(program.entry.path.0.join("."), &program.entry.templates);
        let imports = program.imports.iter().map(|unit| {
            ModuleScope::new(
                unit.import_path.clone(),
                unit.module
                    .templates
                    .iter()
                    .filter(|t| t.visibility == Visibility::Public),
            )
        });
        let stdlib = load_stdlib()
            .iter()
            .map(|module| ModuleScope::new(module.path.0.join("."), &module.templates));
        Self {
            declared: &program.entry,
            scopes: iter::once(entry).chain(imports).chain(stdlib).collect(),
        }
    }

    /// The template `name` resolves to: the first scope, in precedence order,
    /// that declares it.
    pub fn resolve_template(&self, name: &str) -> Option<&'p TopologyTemplate> {
        self.scopes
            .iter()
            .find_map(|scope| scope.templates.get(name).copied())
    }

    /// The template `name` as seen through module `module_key` alone (its pub
    /// surface, for an import).
    pub fn resolve_template_in(
        &self,
        module_key: &str,
        name: &str,
    ) -> Option<&'p TopologyTemplate> {
        self.scopes
            .iter()
            .find(|scope| scope.module_key == module_key)
            .and_then(|scope| scope.templates.get(name).copied())
    }

    /// The program's entry module.
    pub fn declared(&self) -> &'p CompiledModule {
        self.declared
    }
}
