//! The definition environment: every definition, of every kind, a compiled
//! program can name (PRD docs/prds/v0_6/resolution-unification.md §3.2).
//!
//! Per kind, a bare name binds the entry's own declaration, then each direct
//! import's `pub` surface in declaration order (D-5), then the ambient prelude
//! (I6). A qualified lookup sees one module's surface alone, keyed by
//! `(module_key, name)` (I7). The entry's own declarations exclude every copy
//! merged into it: its templates are `entry_declared_templates()` and its
//! purposes are those it declares. The rule itself lives in
//! `reify_compiler::name_env`, which the compiler binds by too; this is a view
//! over it.

use std::borrow::Borrow;

use reify_compiler::name_env::{self, Bindings, Definition};
use reify_compiler::program::CompiledProgram;
use reify_compiler::stdlib_loader::load_stdlib;
use reify_compiler::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    CompiledUnit, TopologyTemplate,
};
use reify_core::Type;
use reify_ir::{CompiledFunction, EnumDef};

/// Every definition a [`CompiledProgram`] can name, resolved on demand over
/// the program and a prelude.
#[derive(Debug, Clone, Copy)]
pub struct DefEnv<'p> {
    program: &'p CompiledProgram,
    prelude: &'p [CompiledModule],
}

impl<'p> DefEnv<'p> {
    /// The environment of `program` over the stdlib.
    // G-allow: resolution-unification §3.2 DefEnv; in-crate consumer Engine::load, flat-slice lookup sites migrate onto it in ι #5523
    pub fn from_program(program: &'p CompiledProgram) -> Self {
        Self::with_prelude(program, load_stdlib())
    }

    /// The environment of `program` over `prelude`.
    pub fn with_prelude(program: &'p CompiledProgram, prelude: &'p [CompiledModule]) -> Self {
        Self { program, prelude }
    }

    fn resolve<D>(&self, name: &str) -> Option<&'p D>
    where
        D: Definition,
        D::Key<'p>: Borrow<str>,
    {
        name_env::resolve_in_program(self.program, self.prelude, name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; in-crate consumer Engine::load, flat-slice lookup sites migrate onto it in ι #5523
    pub fn resolve_template(&self, name: &str) -> Option<&'p TopologyTemplate> {
        self.resolve(name)
    }

    pub fn resolve_enum(&self, name: &str) -> Option<&'p EnumDef> {
        self.resolve(name)
    }

    pub fn resolve_unit(&self, name: &str) -> Option<&'p CompiledUnit> {
        self.resolve(name)
    }

    pub fn resolve_alias(&self, name: &str) -> Option<&'p CompiledTypeAlias> {
        self.resolve(name)
    }

    pub fn resolve_trait(&self, name: &str) -> Option<&'p CompiledTrait> {
        self.resolve(name)
    }

    pub fn resolve_constraint_def(&self, name: &str) -> Option<&'p CompiledConstraintDef> {
        self.resolve(name)
    }

    pub fn resolve_purpose(&self, name: &str) -> Option<&'p CompiledPurpose> {
        self.resolve(name)
    }

    fn function_bindings(&self) -> Bindings<'p, CompiledFunction> {
        name_env::bind_program(self.program, self.prelude)
    }

    /// The function `name(param_types)` binds to: functions are keyed by name
    /// and parameter types.
    pub fn resolve_function(
        &self,
        name: &str,
        param_types: &[Type],
    ) -> Option<&'p CompiledFunction> {
        self.function_bindings().winners().find(|function| {
            function.name == name && function.params.iter().map(|(_, ty)| ty).eq(param_types)
        })
    }

    /// The bound functions named `name`, one per signature, in precedence
    /// order.
    pub fn overloads(&self, name: &str) -> Vec<&'p CompiledFunction> {
        self.function_bindings()
            .winners()
            .filter(|function| function.name == name)
            .collect()
    }

    /// The `D` named `name` as seen through module `module_key` alone: the
    /// entry's own declarations, a direct import's `pub` surface, or a prelude
    /// module's exports.
    pub fn resolve_in<D: Definition>(&self, module_key: &str, name: &str) -> Option<&'p D> {
        name_env::module_surface::<D>(self.program, self.prelude, module_key)?
            .find(|definition| definition.name() == name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; in-crate consumer Engine::load, flat-slice lookup sites migrate onto it in ι #5523
    pub fn resolve_template_in(
        &self,
        module_key: &str,
        name: &str,
    ) -> Option<&'p TopologyTemplate> {
        self.resolve_in(module_key, name)
    }

    /// The program's entry module.
    pub fn declared(&self) -> &'p CompiledModule {
        self.program.entry()
    }
}
