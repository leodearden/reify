//! A [`CompiledProgram`]'s definitions, bound by `name_env`'s rule over the
//! program's three scopes: the entry's own declarations, each direct import's
//! `pub` surface in declaration order, then the ambient prelude.

use std::borrow::Borrow;

use reify_ir::{CompiledFunction, EnumDef};

use crate::name_env::{self, Bindings, Definition, Surface};
use crate::program::CompiledProgram;
use crate::types::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    CompiledUnit, TopologyTemplate,
};

/// A definition kind as a program's entry declares it.
pub trait ProgramDefinition: Definition {
    /// The definitions the program's entry itself declares, never a copy
    /// merged into it from another module.
    fn declared_by_entry(program: &CompiledProgram) -> impl Iterator<Item = &Self>;
}

impl ProgramDefinition for TopologyTemplate {
    fn declared_by_entry(program: &CompiledProgram) -> impl Iterator<Item = &Self> {
        program.entry_declared_templates().iter()
    }
}

impl ProgramDefinition for EnumDef {
    fn declared_by_entry(program: &CompiledProgram) -> impl Iterator<Item = &Self> {
        program.entry().enum_defs.iter()
    }
}

impl ProgramDefinition for CompiledTrait {
    fn declared_by_entry(program: &CompiledProgram) -> impl Iterator<Item = &Self> {
        program.entry().trait_defs.iter()
    }
}

impl ProgramDefinition for CompiledFunction {
    fn declared_by_entry(program: &CompiledProgram) -> impl Iterator<Item = &Self> {
        program.entry().functions.iter()
    }
}

impl ProgramDefinition for CompiledTypeAlias {
    fn declared_by_entry(program: &CompiledProgram) -> impl Iterator<Item = &Self> {
        program.entry().type_aliases.iter()
    }
}

impl ProgramDefinition for CompiledConstraintDef {
    fn declared_by_entry(program: &CompiledProgram) -> impl Iterator<Item = &Self> {
        program.entry().constraint_defs.iter()
    }
}

impl ProgramDefinition for CompiledPurpose {
    fn declared_by_entry(program: &CompiledProgram) -> impl Iterator<Item = &Self> {
        name_env::own_purposes(program.entry())
    }
}

impl ProgramDefinition for CompiledUnit {
    fn declared_by_entry(program: &CompiledProgram) -> impl Iterator<Item = &Self> {
        program.entry().units.iter()
    }
}

fn direct_imports(program: &CompiledProgram) -> impl Iterator<Item = &CompiledModule> + Clone {
    program.imports.iter().map(|unit| &unit.module)
}

/// The definition `key` binds to across `program`, over the ambient
/// `prelude`. Lazy and allocation-free.
pub fn resolve_in_program<'a, D, Q>(
    program: &'a CompiledProgram,
    prelude: &'a [CompiledModule],
    key: &Q,
) -> Option<&'a D>
where
    D: ProgramDefinition,
    D::Key<'a>: Borrow<Q>,
    Q: Eq + ?Sized,
{
    name_env::resolve_in_scopes(
        D::declared_by_entry(program),
        direct_imports(program),
        prelude,
        key,
    )
}

/// The first definition across `program` that `matches` accepts, in
/// [`resolve_in_program`] order: by [`name_env::resolve_in_scopes_by`], a
/// key's binding when `matches` accepts exactly that key's definitions. Lazy
/// and allocation-free.
pub fn resolve_in_program_by<'a, D: ProgramDefinition>(
    program: &'a CompiledProgram,
    prelude: &'a [CompiledModule],
    matches: impl FnMut(&'a D) -> bool,
) -> Option<&'a D> {
    name_env::resolve_in_scopes_by(
        D::declared_by_entry(program),
        direct_imports(program),
        prelude,
        matches,
    )
}

/// Every key's binding plus every shadowing across `program`, by the same rule
/// as [`resolve_in_program`].
pub fn bind_program<'a, D: ProgramDefinition>(
    program: &'a CompiledProgram,
    prelude: &'a [CompiledModule],
) -> Bindings<'a, D> {
    name_env::bind_scopes(
        D::declared_by_entry(program),
        direct_imports(program),
        prelude,
    )
}

/// [`bind_program`] over only the definitions `keep` accepts, with
/// [`name_env::bind_scopes_where`]'s guarantee: a kept key binds as under
/// [`bind_program`] when `keep` accepts or rejects its definitions together.
pub fn bind_program_where<'a, D: ProgramDefinition>(
    program: &'a CompiledProgram,
    prelude: &'a [CompiledModule],
    keep: impl FnMut(&'a D) -> bool,
) -> Bindings<'a, D> {
    name_env::bind_scopes_where(
        D::declared_by_entry(program),
        direct_imports(program),
        prelude,
        keep,
    )
}

/// The definitions one module of `program` offers a qualified lookup
/// (resolution-unification I7), keyed by dotted module path: the entry's own
/// declarations, a direct import's `pub` surface, or a prelude module's
/// exports. `None` for a key naming none of them.
pub fn module_surface<'a, D: ProgramDefinition>(
    program: &'a CompiledProgram,
    prelude: &'a [CompiledModule],
    module_key: &str,
) -> Option<Box<dyn Iterator<Item = &'a D> + 'a>> {
    if program.entry().path.to_dotted() == module_key {
        return Some(Box::new(D::declared_by_entry(program)));
    }
    if let Some(unit) = program
        .imports
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
