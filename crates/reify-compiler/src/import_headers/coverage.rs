//! What a module's `import` declarations make visible, per imported module.
//! A namespace import (`import std.x`, `import std.x as m`) covers the
//! module's whole pub surface; an entity import covers exactly the names it
//! binds (`docs/prds/v0_6/resolution-unification.md` D-8). Either covers the
//! same on every module the imported one re-exports through `pub import`,
//! transitively (D-7), so `import std.prelude` covers what the facade
//! re-exports. An import whose `#cfg(...)` predicates the active cfg does not
//! satisfy makes nothing visible (`docs/prds/v0_6/conditional-compilation.md`
//! D-3; `docs/prds/v0_6/stdlib-namespace.md` boundary #14).

use std::collections::{HashMap, HashSet};

use reify_ast::{Declaration, EntityBinding, ImportBinding, ImportDecl, ImportKind, ParsedModule};
use reify_core::ModulePath;

use crate::cfg::{CfgSet, import_cfg_satisfied};
use crate::re_exports::ReExports;

/// Every `import` `parsed` declares that is in effect under `cfg` (its `#cfg`
/// predicates are satisfied), in declaration order, with the module it names.
pub(crate) fn in_effect<'p>(
    parsed: &'p ParsedModule,
    cfg: &CfgSet,
) -> impl Iterator<Item = (ModulePath, &'p ImportKind)> {
    declared(parsed)
        .filter(|(_, import)| import_cfg_satisfied(import, cfg))
        .map(|(module, import)| (module, &import.kind))
}

/// Every `import` `parsed` declares whose `#cfg` predicates `cfg` does not
/// satisfy, in declaration order, with the module it names: the imports
/// [`in_effect`] leaves out.
pub(crate) fn gated_off<'p>(
    parsed: &'p ParsedModule,
    cfg: &CfgSet,
) -> impl Iterator<Item = (ModulePath, &'p ImportKind)> {
    declared(parsed)
        .filter(|(_, import)| !import_cfg_satisfied(import, cfg))
        .map(|(module, import)| (module, &import.kind))
}

/// Every `import` `parsed` declares, in declaration order, with the module it
/// names. Panics on an import path that is not a module path, which a
/// parse-clean module never has, whatever its `#cfg` predicates.
fn declared(parsed: &ParsedModule) -> impl Iterator<Item = (ModulePath, &ImportDecl)> {
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
}

/// Which names of each imported module a set of a module's imports, together,
/// bind, along the re-export edges of `re_exports`: [`ImportCoverage::of`]
/// gives those in effect under the active cfg, [`ImportCoverage::of_gated_off`]
/// those the active cfg leaves out.
pub(crate) struct ImportCoverage<'p> {
    by_module: HashMap<ModulePath, Coverage<'p>>,
}

enum Coverage<'p> {
    WholeSurface,
    Names(HashSet<&'p str>),
}

impl<'p> ImportCoverage<'p> {
    pub(crate) fn of(parsed: &'p ParsedModule, cfg: &CfgSet, re_exports: &ReExports) -> Self {
        Self::folding(in_effect(parsed, cfg), re_exports)
    }

    /// What the imports `parsed` declares that `cfg` leaves out would bind,
    /// were their `#cfg` predicates satisfied.
    pub(crate) fn of_gated_off(
        parsed: &'p ParsedModule,
        cfg: &CfgSet,
        re_exports: &ReExports,
    ) -> Self {
        Self::folding(gated_off(parsed, cfg), re_exports)
    }

    fn folding(
        imports: impl Iterator<Item = (ModulePath, &'p ImportKind)>,
        re_exports: &ReExports,
    ) -> Self {
        let mut by_module = HashMap::new();
        for (module, kind) in imports {
            let binding = kind.binding();
            for reached in re_exports.closure(&module.to_dotted()) {
                let reached = ModulePath::from_dotted(&reached).unwrap_or_else(|error| {
                    panic!("re-export edge `{reached}` is not a module path: {error}")
                });
                match &binding {
                    ImportBinding::Namespace { .. } => {
                        by_module.insert(reached, Coverage::WholeSurface);
                    }
                    ImportBinding::Entities(entities) => {
                        if let Coverage::Names(covered) = by_module
                            .entry(reached)
                            .or_insert_with(|| Coverage::Names(HashSet::new()))
                        {
                            covered.extend(entities.iter().map(EntityBinding::local_name));
                        }
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

    /// Every module one of the imports names, once each, in no particular order.
    pub(crate) fn modules(&self) -> impl Iterator<Item = &ModulePath> {
        self.by_module.keys()
    }
}
