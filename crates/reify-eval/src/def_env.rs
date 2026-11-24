//! The definition environment: every definition, of every kind, a compiled
//! program can name (PRD docs/prds/v0_6/resolution-unification.md §3.2).
//!
//! Per kind, a bare name binds the entry's own declaration, then each direct
//! import's `pub` surface in declaration order (D-5), then the ambient prelude
//! (I6). A qualified lookup sees one module's surface alone, keyed by
//! `(module_key, name)` (I7). The entry's own declarations exclude every copy
//! merged into it: its templates are `entry_declared_templates()` and its
//! purposes are those it declares. The rule itself lives in
//! `reify_compiler::name_env`, which the compiler binds by too, and the
//! program's scopes come from `reify_compiler::program_scope`; this is a view
//! over them.
//!
//! A lone [`CompiledModule`] has the environment of the import-free program
//! whose entry it is (I1): its own templates, all of them declared, then the
//! prelude.
//!
//! The engine evaluates a loaded program as one module, its
//! [`evaluation_module`]: eval walks `module.templates` index by index, so the
//! program's user templates are that module's. [`EnvScope`] is the
//! environment the engine resolves a module in: the loaded program's for
//! exactly that module, the module's own for any other.

use std::borrow::Borrow;
use std::sync::Arc;

use reify_compiler::name_env::Tier;
use reify_compiler::program::CompiledProgram;
use reify_compiler::program_scope::{self, ProgramDefinition, ProgramRoot};
use reify_compiler::stdlib_loader::load_stdlib;
use reify_compiler::{
    CompiledModule, CompiledTrait, CompiledTypeAlias, CompiledUnit, TopologyTemplate,
};
use reify_core::Type;
use reify_ir::{CompiledFunction, EnumDef};

/// Every definition a [`CompiledProgram`] (or a lone module) can name,
/// resolved on demand over it and a prelude.
#[derive(Debug, Clone, Copy)]
pub struct DefEnv<'p> {
    root: ProgramRoot<'p>,
    prelude: &'p [CompiledModule],
}

impl<'p> DefEnv<'p> {
    /// The environment of `program` over the stdlib.
    pub fn from_program(program: &'p CompiledProgram) -> Self {
        Self::with_prelude(program, load_stdlib())
    }

    /// The environment of `program` over `prelude`.
    pub fn with_prelude(program: &'p CompiledProgram, prelude: &'p [CompiledModule]) -> Self {
        Self {
            root: program.into(),
            prelude,
        }
    }

    /// The environment of `module` over `prelude`: that of the import-free
    /// program whose entry `module` is (I1).
    pub fn of_module(module: &'p CompiledModule, prelude: &'p [CompiledModule]) -> Self {
        Self::with_root(module.into(), prelude)
    }

    /// The environment of `root` over `prelude`.
    pub fn with_root(root: ProgramRoot<'p>, prelude: &'p [CompiledModule]) -> Self {
        Self { root, prelude }
    }

    /// The program's user templates: every template the entry declares, then
    /// each direct import's `pub` templates whose name nothing before them
    /// binds, in declaration order. The prelude's are not among them.
    pub fn user_templates(&self) -> impl Iterator<Item = &'p TopologyTemplate> {
        let imported: Vec<&'p TopologyTemplate> =
            program_scope::bind_program::<TopologyTemplate>(self.root, &[])
                .winning_candidates()
                .iter()
                .filter(|candidate| candidate.origin.tier() == Tier::Import)
                .map(|candidate| candidate.definition)
                .collect();
        self.root.entry_declared_templates().iter().chain(imported)
    }

    /// The bound traits, in precedence order: the trait lattice every
    /// conformance check walks.
    pub fn traits(&self) -> impl Iterator<Item = &'p CompiledTrait> {
        program_scope::bind_program::<CompiledTrait>(self.root, self.prelude)
            .winners()
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn resolve<D>(&self, name: &str) -> Option<&'p D>
    where
        D: ProgramDefinition,
        D::Key<'p>: Borrow<str>,
    {
        program_scope::resolve_in_program(self.root, self.prelude, name)
    }

    pub fn resolve_template(&self, name: &str) -> Option<&'p TopologyTemplate> {
        self.resolve(name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is λ #5525
    pub fn resolve_enum(&self, name: &str) -> Option<&'p EnumDef> {
        self.resolve(name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is λ #5525
    pub fn resolve_unit(&self, name: &str) -> Option<&'p CompiledUnit> {
        self.resolve(name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is λ #5525
    pub fn resolve_alias(&self, name: &str) -> Option<&'p CompiledTypeAlias> {
        self.resolve(name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is λ #5525
    pub fn resolve_trait(&self, name: &str) -> Option<&'p CompiledTrait> {
        self.resolve(name)
    }

    /// The bound functions in precedence order, one per signature (name and
    /// parameter types): the program's dispatch table (§10 Q4).
    pub fn function_table(&self) -> Arc<[CompiledFunction]> {
        program_scope::bind_program::<CompiledFunction>(self.root, self.prelude)
            .winners()
            .cloned()
            .collect()
    }

    /// The function `name(param_types)` binds to: functions are keyed by name
    /// and parameter types. Lazy and allocation-free.
    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is λ #5525
    pub fn resolve_function(
        &self,
        name: &str,
        param_types: &[Type],
    ) -> Option<&'p CompiledFunction> {
        program_scope::resolve_in_program_by(
            self.root,
            self.prelude,
            |function: &CompiledFunction| {
                function.name == name && function.params.iter().map(|(_, ty)| ty).eq(param_types)
            },
        )
    }

    /// The bound functions named `name`, one per signature, in precedence
    /// order. Only `name`'s candidates are bound.
    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is λ #5525
    pub fn overloads(&self, name: &str) -> Vec<&'p CompiledFunction> {
        program_scope::bind_program_where(self.root, self.prelude, |function: &CompiledFunction| {
            function.name == name
        })
        .winners()
        .collect()
    }

    /// The `D` named `name` as seen through module `module_key` alone: the
    /// entry's own declarations, a direct import's `pub` surface, or a prelude
    /// module's exports. Matches by name alone, so for a function it is the
    /// module's first overload of `name`; [`Self::resolve_function`] matches
    /// a signature.
    // G-allow: resolution-unification I7 qualified lookup; first production consumer is stdlib-namespace ν #5505
    pub fn resolve_in<D: ProgramDefinition>(&self, module_key: &str, name: &str) -> Option<&'p D> {
        program_scope::module_surface::<D>(self.root, self.prelude, module_key)?
            .find(|definition| definition.name() == name)
    }

    // G-allow: resolution-unification I7 qualified lookup; first production consumer is stdlib-namespace ν #5505
    pub fn resolve_template_in(
        &self,
        module_key: &str,
        name: &str,
    ) -> Option<&'p TopologyTemplate> {
        self.resolve_in(module_key, name)
    }

    /// The entry module. A program's entry's `templates` include the copies
    /// the I5 merge took from its imports; the entry's own are
    /// `CompiledProgram::entry_declared_templates`.
    pub fn declared(&self) -> &'p CompiledModule {
        self.root.entry()
    }
}

/// The module the engine evaluates for `program`: its entry, with the
/// program's [`DefEnv::user_templates`] as its templates.
pub fn evaluation_module(program: &CompiledProgram) -> CompiledModule {
    let templates = DefEnv::from_program(program)
        .user_templates()
        .cloned()
        .collect();
    CompiledModule {
        templates,
        ..program.entry().clone()
    }
}

/// A program installed by `Engine::load`, with the module the engine evaluates
/// for it.
#[derive(Debug)]
pub(crate) struct LoadedProgram {
    program: Arc<CompiledProgram>,
    evaluated: Arc<CompiledModule>,
}

impl LoadedProgram {
    pub(crate) fn new(program: Arc<CompiledProgram>) -> Self {
        let evaluated = Arc::new(evaluation_module(&program));
        Self { program, evaluated }
    }

    pub(crate) fn evaluated(&self) -> &Arc<CompiledModule> {
        &self.evaluated
    }
}

/// The definition environment the engine resolves one module in. It owns what
/// it shares with the engine, so it borrows the module alone, never the engine.
#[derive(Debug, Clone)]
pub struct EnvScope<'m> {
    module: &'m CompiledModule,
    /// The engine's loaded program, iff `module` is its evaluation module.
    loaded: Option<Arc<LoadedProgram>>,
    prelude: &'m [CompiledModule],
}

impl<'m> EnvScope<'m> {
    pub(crate) fn new(
        module: &'m CompiledModule,
        loaded: Option<&Arc<LoadedProgram>>,
        prelude: &'m [CompiledModule],
    ) -> Self {
        let loaded = loaded
            .filter(|loaded| std::ptr::eq(module, &*loaded.evaluated))
            .cloned();
        Self {
            module,
            loaded,
            prelude,
        }
    }

    /// The loaded program's environment, rooted at its evaluation module, when
    /// the scope's module is that module; otherwise the module's own (I1).
    pub fn env(&self) -> DefEnv<'_> {
        match &self.loaded {
            Some(loaded) => DefEnv::with_root(
                ProgramRoot::new(&loaded.evaluated, &loaded.program.imports),
                self.prelude,
            ),
            None => DefEnv::of_module(self.module, self.prelude),
        }
    }
}
