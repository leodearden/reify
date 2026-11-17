//! What a module's `import` declarations make visible, per imported module.
//! A namespace import (`import std.x`, `import std.x as m`) covers the
//! module's whole pub surface; an entity import covers exactly the names it
//! binds (`docs/prds/v0_6/resolution-unification.md` D-8). An import whose
//! `#cfg(...)` predicates the active cfg does not satisfy makes nothing
//! visible (`docs/prds/v0_6/conditional-compilation.md` D-3;
//! `docs/prds/v0_6/stdlib-namespace.md` boundary #14).

use std::collections::{HashMap, HashSet};

use reify_ast::{Declaration, EntityBinding, ImportBinding, ImportKind, ParsedModule};
use reify_core::ModulePath;

use crate::cfg::CfgSet;
use crate::module_dag::import_cfg_satisfied;

/// Every `import` `parsed` declares whose `#cfg` predicates `cfg` satisfies,
/// in declaration order, with the module it names. Panics on an import path
/// that is not a module path, which a parse-clean module never has, whether
/// or not `cfg` gates that import off.
pub(crate) fn imports<'p>(
    parsed: &'p ParsedModule,
    cfg: &CfgSet,
) -> impl Iterator<Item = (ModulePath, &'p ImportKind)> {
    parsed
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Import(import) => Some(import),
            _ => None,
        })
        .map(|import| {
            let module = ModulePath::from_dotted(&import.path).unwrap_or_else(|error| {
                panic!(
                    "import_headers requires a parse-clean module, but `{}` imports \
                     `{}`, which is not a module path: {error}",
                    parsed.path.to_dotted(),
                    import.path
                )
            });
            (module, import)
        })
        .filter(|(_, import)| import_cfg_satisfied(import, cfg))
        .map(|(module, import)| (module, &import.kind))
}

/// Which names of each imported module a module's imports in effect under
/// the active cfg, together, bind.
pub(crate) struct ImportCoverage<'p> {
    by_module: HashMap<ModulePath, Coverage<'p>>,
}

enum Coverage<'p> {
    WholeSurface,
    Names(HashSet<&'p str>),
}

impl<'p> ImportCoverage<'p> {
    pub(crate) fn of(parsed: &'p ParsedModule, cfg: &CfgSet) -> Self {
        let mut by_module = HashMap::new();
        for (module, kind) in imports(parsed, cfg) {
            match kind.binding() {
                ImportBinding::Namespace { .. } => {
                    by_module.insert(module, Coverage::WholeSurface);
                }
                ImportBinding::Entities(entities) => {
                    if let Coverage::Names(covered) = by_module
                        .entry(module)
                        .or_insert_with(|| Coverage::Names(HashSet::new()))
                    {
                        covered.extend(entities.iter().map(EntityBinding::local_name));
                    }
                }
            }
        }
        Self { by_module }
    }

    pub(crate) fn covers(&self, module: &ModulePath, name: &str) -> bool {
        match self.by_module.get(module) {
            Some(Coverage::WholeSurface) => true,
            Some(Coverage::Names(covered)) => covered.contains(name),
            None => false,
        }
    }

    pub(crate) fn covers_whole_surface(&self, module: &ModulePath) -> bool {
        matches!(self.by_module.get(module), Some(Coverage::WholeSurface))
    }
}
