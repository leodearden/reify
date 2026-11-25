//! A [`CompiledProgram`]'s definitions, bound by `name_env`'s rule over the
//! program's three scopes: the entry's own declarations, each direct import's
//! `pub` surface in declaration order, then the ambient prelude.

use std::borrow::Borrow;

use reify_core::Diagnostic;
use reify_ir::{CompiledFunction, EnumDef};

use crate::name_env::{self, Bindings, Definition, Origin, Surface, Tier};
use crate::program::{CompiledImportUnit, CompiledProgram};
use crate::types::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    CompiledUnit, TopologyTemplate,
};

/// What resolution starts from: an entry module and its direct imports. A
/// root is built only from a compiled program, whose entry holds only its own
/// declarations (boundary #14), or from a lone module, which stands for the
/// import-free program whose entry it is (resolution-unification I1). So the
/// entry's own declarations are everything it holds.
#[derive(Debug, Clone, Copy)]
pub struct ProgramRoot<'a> {
    entry: &'a CompiledModule,
    imports: &'a [CompiledImportUnit],
}

impl<'a> ProgramRoot<'a> {
    /// The root whose entry is `entry` over the direct `imports`, in
    /// declaration order.
    fn new(entry: &'a CompiledModule, imports: &'a [CompiledImportUnit]) -> Self {
        Self { entry, imports }
    }

    /// The entry module.
    pub fn entry(self) -> &'a CompiledModule {
        self.entry
    }

    /// The entry's direct imports, in declaration order.
    pub fn imports(self) -> &'a [CompiledImportUnit] {
        self.imports
    }
}

impl<'a> From<&'a CompiledProgram> for ProgramRoot<'a> {
    fn from(program: &'a CompiledProgram) -> Self {
        Self::new(program.entry(), &program.imports)
    }
}

impl<'a> From<&'a CompiledModule> for ProgramRoot<'a> {
    fn from(module: &'a CompiledModule) -> Self {
        Self::new(module, &[])
    }
}

/// A definition kind as a program's entry declares it.
pub trait ProgramDefinition: Definition {
    /// The definitions the root's entry itself declares.
    fn declared_by_entry(root: ProgramRoot<'_>) -> impl Iterator<Item = &Self>;
}

impl ProgramDefinition for TopologyTemplate {
    fn declared_by_entry(root: ProgramRoot<'_>) -> impl Iterator<Item = &Self> {
        root.entry().templates.iter()
    }
}

impl ProgramDefinition for EnumDef {
    fn declared_by_entry(root: ProgramRoot<'_>) -> impl Iterator<Item = &Self> {
        root.entry().enum_defs.iter()
    }
}

impl ProgramDefinition for CompiledTrait {
    fn declared_by_entry(root: ProgramRoot<'_>) -> impl Iterator<Item = &Self> {
        root.entry().trait_defs.iter()
    }
}

impl ProgramDefinition for CompiledFunction {
    fn declared_by_entry(root: ProgramRoot<'_>) -> impl Iterator<Item = &Self> {
        root.entry().functions.iter()
    }
}

impl ProgramDefinition for CompiledTypeAlias {
    fn declared_by_entry(root: ProgramRoot<'_>) -> impl Iterator<Item = &Self> {
        root.entry().type_aliases.iter()
    }
}

impl ProgramDefinition for CompiledConstraintDef {
    fn declared_by_entry(root: ProgramRoot<'_>) -> impl Iterator<Item = &Self> {
        root.entry().constraint_defs.iter()
    }
}

impl ProgramDefinition for CompiledPurpose {
    fn declared_by_entry(root: ProgramRoot<'_>) -> impl Iterator<Item = &Self> {
        name_env::own_purposes(root.entry())
    }
}

impl ProgramDefinition for CompiledUnit {
    fn declared_by_entry(root: ProgramRoot<'_>) -> impl Iterator<Item = &Self> {
        root.entry().units.iter()
    }
}

fn direct_imports(root: ProgramRoot<'_>) -> impl Iterator<Item = &CompiledModule> + Clone {
    root.imports().iter().map(|unit| &unit.module)
}

/// The definition `key` binds to across `root`, over the ambient `prelude`.
/// Lazy and allocation-free.
pub fn resolve_in_program<'a, D, Q>(
    root: impl Into<ProgramRoot<'a>>,
    prelude: &'a [CompiledModule],
    key: &Q,
) -> Option<&'a D>
where
    D: ProgramDefinition,
    D::Key<'a>: Borrow<Q>,
    Q: Eq + ?Sized,
{
    let root = root.into();
    name_env::resolve_in_scopes(
        D::declared_by_entry(root),
        direct_imports(root),
        prelude,
        key,
    )
}

/// The first definition across `root` that `matches` accepts, in
/// [`resolve_in_program`] order: by [`name_env::resolve_in_scopes_by`], a
/// key's binding when `matches` accepts exactly that key's definitions. Lazy
/// and allocation-free.
pub fn resolve_in_program_by<'a, D: ProgramDefinition>(
    root: impl Into<ProgramRoot<'a>>,
    prelude: &'a [CompiledModule],
    matches: impl FnMut(&'a D) -> bool,
) -> Option<&'a D> {
    let root = root.into();
    name_env::resolve_in_scopes_by(
        D::declared_by_entry(root),
        direct_imports(root),
        prelude,
        matches,
    )
}

/// Every key's binding plus every shadowing across `root`, by the same rule
/// as [`resolve_in_program`].
pub fn bind_program<'a, D: ProgramDefinition>(
    root: impl Into<ProgramRoot<'a>>,
    prelude: &'a [CompiledModule],
) -> Bindings<'a, D> {
    let root = root.into();
    name_env::bind_scopes(D::declared_by_entry(root), direct_imports(root), prelude)
}

/// [`bind_program`] over only the definitions `keep` accepts, with
/// [`name_env::bind_scopes_where`]'s guarantee: a kept key binds as under
/// [`bind_program`] when `keep` accepts or rejects its definitions together.
pub fn bind_program_where<'a, D: ProgramDefinition>(
    root: impl Into<ProgramRoot<'a>>,
    prelude: &'a [CompiledModule],
    keep: impl FnMut(&'a D) -> bool,
) -> Bindings<'a, D> {
    let root = root.into();
    name_env::bind_scopes_where(
        D::declared_by_entry(root),
        direct_imports(root),
        prelude,
        keep,
    )
}

/// One Warning per first-wins collision across `root` whose loser is a user
/// import's definition (resolution-unification D-6) and that no module's own
/// compile already reports, so each collision has one reporter. Entry-vs-import
/// is always reported here. Import-vs-import is reported here too, except for
/// units, type aliases and constraint defs: the entry's compile binds its
/// imports as its prelude and reports those three kinds' collisions there. A
/// shadowed stdlib definition is the shadowing module's own compile's to
/// report.
pub fn collision_warnings<'a>(
    root: impl Into<ProgramRoot<'a>>,
    prelude: &'a [CompiledModule],
) -> Vec<Diagnostic> {
    use ImportPairs::{LeaveToEntryCompile, Report};
    let root = root.into();
    [
        import_tier_collisions::<TopologyTemplate>(root, prelude, Report),
        import_tier_collisions::<CompiledFunction>(root, prelude, Report),
        import_tier_collisions::<EnumDef>(root, prelude, Report),
        import_tier_collisions::<CompiledUnit>(root, prelude, LeaveToEntryCompile),
        import_tier_collisions::<CompiledTypeAlias>(root, prelude, LeaveToEntryCompile),
        import_tier_collisions::<CompiledTrait>(root, prelude, Report),
        import_tier_collisions::<CompiledConstraintDef>(root, prelude, LeaveToEntryCompile),
        import_tier_collisions::<CompiledPurpose>(root, prelude, Report),
    ]
    .concat()
}

fn import_tier_collisions<D: ProgramDefinition>(
    root: ProgramRoot<'_>,
    prelude: &[CompiledModule],
    import_pairs: ImportPairs,
) -> Vec<Diagnostic> {
    let entry = &root.entry().path;
    bind_program::<D>(root, prelude)
        .shadowings()
        .iter()
        .filter(|shadowing| {
            shadowing.shadowed.origin.tier() == Tier::Import
                && shadowing.winner.origin != shadowing.shadowed.origin
                && (shadowing.winner.origin == Origin::Local || import_pairs == ImportPairs::Report)
        })
        .map(|shadowing| name_env::collision_warning(shadowing, Some(entry)))
        .collect()
}

/// Who reports a collision between two direct imports' definitions of one
/// kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportPairs {
    /// [`collision_warnings`] does.
    Report,
    /// The entry's own compile does: it binds its imports as its prelude.
    LeaveToEntryCompile,
}

/// The definitions one module of `root` offers a qualified lookup
/// (resolution-unification I7), keyed by dotted module path: the entry's own
/// declarations, a direct import's `pub` surface, or a prelude module's
/// exports. `None` for a key naming none of them.
pub fn module_surface<'a, D: ProgramDefinition>(
    root: impl Into<ProgramRoot<'a>>,
    prelude: &'a [CompiledModule],
    module_key: &str,
) -> Option<Box<dyn Iterator<Item = &'a D> + 'a>> {
    let root = root.into();
    if root.entry().path.to_dotted() == module_key {
        return Some(Box::new(D::declared_by_entry(root)));
    }
    if let Some(unit) = root
        .imports()
        .iter()
        .find(|unit| unit.import_path == module_key)
    {
        return Some(Box::new(name_env::surface_of(
            &unit.module,
            Surface::DirectImport,
        )));
    }
    prelude
        .iter()
        .find(|module| module.path.to_dotted() == module_key)
        .map(|module| -> Box<dyn Iterator<Item = &'a D> + 'a> {
            Box::new(name_env::surface_of(module, Surface::Ambient))
        })
}

#[cfg(test)]
mod tests {
    use reify_core::ModulePath;

    use super::*;

    fn compile_at(path: ModulePath, src: &str) -> CompiledModule {
        let parsed = reify_syntax::parse(src, path);
        assert!(
            parsed.errors.is_empty(),
            "fixture must parse: {:?}",
            parsed.errors
        );
        crate::compile(&parsed)
    }

    fn named<'a>(module: &'a CompiledModule, name: &str) -> &'a TopologyTemplate {
        module
            .templates
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("fixture declares {name}"))
    }

    /// A lone module is the entry of the import-free program it stands for
    /// (I1): it declares every template it holds, imports nothing, and binds
    /// its own templates before the prelude's.
    #[test]
    fn a_lone_module_root_declares_every_template_it_holds_and_imports_nothing() {
        let module = compile_at(
            ModulePath::single("main"),
            "structure def Own { param a : Real = 1.0 }\n\
             structure def Shared { param a : Real = 1.0 }\n",
        );
        let prelude = [compile_at(
            ModulePath(vec!["std".into(), "lo".into()]),
            "structure def Shared { param a : Real = 2.0 }\n\
             structure def StdOnly { param a : Real = 2.0 }\n",
        )];
        let root = ProgramRoot::from(&module);

        assert!(std::ptr::eq(root.entry(), &module));
        assert!(
            TopologyTemplate::declared_by_entry(root)
                .map(|t| t as *const TopologyTemplate)
                .eq(module
                    .templates
                    .iter()
                    .map(|t| t as *const TopologyTemplate))
        );
        assert!(root.imports().is_empty());

        let resolve =
            |name: &str| resolve_in_program::<TopologyTemplate, str>(root, &prelude, name);
        assert!(resolve("Shared").is_some_and(|t| std::ptr::eq(t, named(&module, "Shared"))));
        assert!(resolve("StdOnly").is_some_and(|t| std::ptr::eq(t, named(&prelude[0], "StdOnly"))));
        assert!(resolve("Absent").is_none());
    }
}
