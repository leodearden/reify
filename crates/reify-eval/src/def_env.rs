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

use std::borrow::Borrow;
use std::sync::Arc;

use reify_compiler::name_env::{self, Bindings, Origin, Tier};
use reify_compiler::program::CompiledProgram;
use reify_compiler::program_scope::{self, ProgramDefinition};
use reify_compiler::stdlib_loader::load_stdlib;
use reify_compiler::{
    CompiledConstraintDef, CompiledModule, CompiledPurpose, CompiledTrait, CompiledTypeAlias,
    CompiledUnit, TopologyTemplate,
};
use reify_core::{Diagnostic, Type};
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
    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn from_program(program: &'p CompiledProgram) -> Self {
        Self::with_prelude(program, load_stdlib())
    }

    /// The environment of `program` over `prelude`.
    pub fn with_prelude(program: &'p CompiledProgram, prelude: &'p [CompiledModule]) -> Self {
        Self { program, prelude }
    }

    fn resolve<D>(&self, name: &str) -> Option<&'p D>
    where
        D: ProgramDefinition,
        D::Key<'p>: Borrow<str>,
    {
        program_scope::resolve_in_program(self.program, self.prelude, name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn resolve_template(&self, name: &str) -> Option<&'p TopologyTemplate> {
        self.resolve(name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn resolve_enum(&self, name: &str) -> Option<&'p EnumDef> {
        self.resolve(name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn resolve_unit(&self, name: &str) -> Option<&'p CompiledUnit> {
        self.resolve(name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn resolve_alias(&self, name: &str) -> Option<&'p CompiledTypeAlias> {
        self.resolve(name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn resolve_trait(&self, name: &str) -> Option<&'p CompiledTrait> {
        self.resolve(name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn resolve_constraint_def(&self, name: &str) -> Option<&'p CompiledConstraintDef> {
        self.resolve(name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn resolve_purpose(&self, name: &str) -> Option<&'p CompiledPurpose> {
        self.resolve(name)
    }

    fn function_bindings(&self) -> Bindings<'p, CompiledFunction> {
        program_scope::bind_program(self.program, self.prelude)
    }

    /// The bound functions in precedence order, one per signature (name and
    /// parameter types): the program's dispatch table (§10 Q4).
    // G-allow: resolution-unification §10 Q4 dispatch table; κ #5524 folds merge_functions into it
    pub fn function_table(&self) -> Arc<[CompiledFunction]> {
        self.function_bindings().winners().cloned().collect()
    }

    /// The function `name(param_types)` binds to: functions are keyed by name
    /// and parameter types.
    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
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
    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn overloads(&self, name: &str) -> Vec<&'p CompiledFunction> {
        self.function_bindings()
            .winners()
            .filter(|function| function.name == name)
            .collect()
    }

    /// The `D` named `name` as seen through module `module_key` alone: the
    /// entry's own declarations, a direct import's `pub` surface, or a prelude
    /// module's exports.
    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn resolve_in<D: ProgramDefinition>(&self, module_key: &str, name: &str) -> Option<&'p D> {
        program_scope::module_surface::<D>(self.program, self.prelude, module_key)?
            .find(|definition| definition.name() == name)
    }

    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn resolve_template_in(
        &self,
        module_key: &str,
        name: &str,
    ) -> Option<&'p TopologyTemplate> {
        self.resolve_in(module_key, name)
    }

    /// The program's entry module.
    // G-allow: resolution-unification §3.2 DefEnv; first production consumer is ι #5523
    pub fn declared(&self) -> &'p CompiledModule {
        self.program.entry()
    }

    /// One Warning per first-wins collision whose loser is a user import's
    /// definition (D-6) and that no module's own compile already reports, so
    /// each collision has one reporter. Entry-vs-import is always reported
    /// here. Import-vs-import is reported here too, except for units, type
    /// aliases and constraint defs: the entry's compile binds its imports as
    /// its prelude and reports those three kinds' collisions there. A shadowed
    /// stdlib definition is the shadowing module's own compile's to report.
    pub fn collision_warnings(&self) -> Vec<Diagnostic> {
        use ImportPairs::{LeaveToEntryCompile, Report};
        [
            self.import_tier_collisions::<TopologyTemplate>(Report),
            self.import_tier_collisions::<CompiledFunction>(Report),
            self.import_tier_collisions::<EnumDef>(Report),
            self.import_tier_collisions::<CompiledUnit>(LeaveToEntryCompile),
            self.import_tier_collisions::<CompiledTypeAlias>(LeaveToEntryCompile),
            self.import_tier_collisions::<CompiledTrait>(Report),
            self.import_tier_collisions::<CompiledConstraintDef>(LeaveToEntryCompile),
            self.import_tier_collisions::<CompiledPurpose>(Report),
        ]
        .concat()
    }

    fn import_tier_collisions<D: ProgramDefinition>(
        &self,
        import_pairs: ImportPairs,
    ) -> Vec<Diagnostic> {
        let entry = &self.program.entry().path;
        program_scope::bind_program::<D>(self.program, self.prelude)
            .shadowings()
            .iter()
            .filter(|shadowing| {
                shadowing.shadowed.origin.tier() == Tier::Import
                    && shadowing.winner.origin != shadowing.shadowed.origin
                    && (shadowing.winner.origin == Origin::Local
                        || import_pairs == ImportPairs::Report)
            })
            .map(|shadowing| name_env::collision_warning(shadowing, Some(entry)))
            .collect()
    }
}

/// Who reports a collision between two direct imports' definitions of one
/// kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportPairs {
    /// [`DefEnv::collision_warnings`] does.
    Report,
    /// The entry's own compile does: it binds its imports as its prelude.
    LeaveToEntryCompile,
}
