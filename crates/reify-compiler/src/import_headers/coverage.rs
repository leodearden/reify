//! What a module's `import` declarations make visible, per imported module.
//! A namespace import (`import std.x`, `import std.x as m`) covers the
//! module's whole pub surface; an entity import covers exactly the names it
//! binds (`docs/prds/v0_6/resolution-unification.md` D-8).

use std::collections::{HashMap, HashSet};

use reify_ast::{Declaration, ImportKind, ParsedModule};
use reify_core::ModulePath;

/// Every `import` `parsed` declares, in declaration order, with the module it
/// names. Panics on an import path that is not a module path, which a
/// parse-clean module never has.
pub(crate) fn imports(parsed: &ParsedModule) -> impl Iterator<Item = (ModulePath, &ImportKind)> {
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
            (module, &import.kind)
        })
}

/// The names of its module one `import` declaration binds.
enum Binding<'p> {
    WholeSurface,
    Names(Vec<&'p str>),
}

/// The same namespace/entity split as reify-syntax's
/// `collect_import_bindings` (`ts_parser.rs`); change the two together.
fn binding(kind: &ImportKind) -> Binding<'_> {
    match kind {
        ImportKind::Module | ImportKind::Aliased { .. } => Binding::WholeSurface,
        ImportKind::Entity(entity) => Binding::Names(vec![entity]),
        ImportKind::Destructured(names) => {
            Binding::Names(names.iter().map(String::as_str).collect())
        }
        ImportKind::EntityAliased { alias, .. } => Binding::Names(vec![alias]),
    }
}

/// Which names of each imported module a module's imports, together, bind.
pub(crate) struct ImportCoverage<'p> {
    by_module: HashMap<ModulePath, Coverage<'p>>,
}

enum Coverage<'p> {
    WholeSurface,
    Names(HashSet<&'p str>),
}

impl<'p> ImportCoverage<'p> {
    pub(crate) fn of(parsed: &'p ParsedModule) -> Self {
        let mut by_module = HashMap::new();
        for (module, kind) in imports(parsed) {
            match binding(kind) {
                Binding::WholeSurface => {
                    by_module.insert(module, Coverage::WholeSurface);
                }
                Binding::Names(names) => {
                    if let Coverage::Names(covered) = by_module
                        .entry(module)
                        .or_insert_with(|| Coverage::Names(HashSet::new()))
                    {
                        covered.extend(names);
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
