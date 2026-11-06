//! The definition environment: a read-only view of every template a compiled
//! program can name.
//!
//! This is the P1 skeleton (PRD docs/prds/v0_6/resolution-unification.md §3.2,
//! D-3); θ #5522 makes it authoritative for the engine. Resolution order is the
//! entry, then its imports in declaration order, then the stdlib (I6), and every
//! scope is keyed internally by `(module_key, name)` (I7) — the seam
//! stdlib-namespace ν #5505 consumes for qualified lookup.

use std::collections::HashMap;
use std::sync::OnceLock;

use reify_compiler::program::CompiledProgram;
use reify_compiler::stdlib_loader::load_stdlib;
use reify_compiler::{CompiledModule, TopologyTemplate, Visibility};

/// Every template a [`CompiledProgram`] can name, in precedence order.
///
/// The entry's scope holds its whole template list — in P1 the merged list,
/// which already carries the direct imports' pub templates (interim I5 until
/// κ #5524). Each import's scope holds its pub templates only; each stdlib
/// module's scope holds all of its templates, as eval's prelude fallback does.
pub struct DefEnv<'p> {
    declared: &'p CompiledModule,
    /// The entry's scope, then each import's.
    program_scopes: Vec<ModuleScope<'p>>,
    /// Each stdlib module's scope, shared by every environment.
    stdlib_scopes: &'p [ModuleScope<'p>],
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

/// Every stdlib module's scope, in load order, built once over the static
/// stdlib.
fn stdlib_scopes() -> &'static [ModuleScope<'static>] {
    static SCOPES: OnceLock<Vec<ModuleScope<'static>>> = OnceLock::new();
    SCOPES.get_or_init(|| {
        load_stdlib()
            .iter()
            .map(|module| ModuleScope::new(module.path.0.join("."), &module.templates))
            .collect()
    })
}

impl<'p> DefEnv<'p> {
    /// Build the environment for `program`: the entry, then each import unit in
    /// declaration order, then each stdlib module in load order.
    // G-allow: P1 DefEnv skeleton (resolution-unification D-3); first production consumer is θ #5522
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
        Self {
            declared: &program.entry,
            program_scopes: std::iter::once(entry).chain(imports).collect(),
            stdlib_scopes: stdlib_scopes(),
        }
    }

    /// Every scope, in precedence order.
    fn scopes(&self) -> impl Iterator<Item = &ModuleScope<'p>> {
        self.program_scopes.iter().chain(self.stdlib_scopes)
    }

    /// The template `name` resolves to: the first scope, in precedence order,
    /// that declares it.
    // G-allow: P1 DefEnv skeleton (resolution-unification D-3); first production consumer is θ #5522
    pub fn resolve_template(&self, name: &str) -> Option<&'p TopologyTemplate> {
        self.scopes()
            .find_map(|scope| scope.templates.get(name).copied())
    }

    /// The template `name` as seen through module `module_key` alone (its pub
    /// surface, for an import).
    // G-allow: P1 DefEnv skeleton (resolution-unification D-3); first production consumer is θ #5522
    pub fn resolve_template_in(
        &self,
        module_key: &str,
        name: &str,
    ) -> Option<&'p TopologyTemplate> {
        self.scopes()
            .find(|scope| scope.module_key == module_key)
            .and_then(|scope| scope.templates.get(name).copied())
    }

    /// The program's entry module.
    pub fn declared(&self) -> &'p CompiledModule {
        self.declared
    }
}
