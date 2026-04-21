//! CompilationCtx — the durable mutable state threaded through every phase of
//! [`crate::compile_with_prelude_refs`].
//!
//! Holds ONLY the owned `Vec<T>` / registry state that outlives a single phase.
//! Borrow-based lookup registries (`HashMap<String, &CompiledTrait>`, etc.)
//! stay phase-local because storing `&T` alongside the owning `Vec<T>` in the
//! same struct requires self-referential-lifetime tricks that aren't worth it
//! for a purely mechanical refactor (see task 2035 design decision #2).
//!
//! The `parsed: &ParsedModule` and `prelude: &[&CompiledModule]` slices plus
//! phase-local ref collections (`fn_refs`, `trait_refs`, etc.) flow as explicit
//! args from the orchestrator to the phases that need them.

use std::collections::HashMap;

use reify_syntax::ParsedModule;
use reify_types::{ContentHash, Diagnostic, SourceSpan};

use crate::entity::PendingBoundCheck;
use crate::type_resolution::TypeAliasRegistry;
use crate::types::{
    CompiledConstraintDef, CompiledField, CompiledImport, CompiledModule, CompiledPurpose,
    CompiledTrait, CompiledUnit, TopologyTemplate,
};
use crate::units::UnitRegistry;
use reify_types::CompiledFunction;

/// Durable mutable state threaded through every phase of
/// [`crate::compile_with_prelude_refs`].
///
/// Owns the cumulative `Vec<T>` outputs (diagnostics, imports, functions,
/// fields, etc.) and the two registries (`unit_registry`, `alias_registry`)
/// that are written in early phases and read in later ones.
///
/// Borrow-based lookup registries (`HashMap<String, &CompiledTrait>`, etc.)
/// are NOT owned here — they are rebuilt fresh as phase-local variables inside
/// each phase function that needs them.
pub(crate) struct CompilationCtx {
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) imports: Vec<CompiledImport>,
    pub(crate) functions: Vec<CompiledFunction>,
    pub(crate) fields: Vec<CompiledField>,
    pub(crate) templates: Vec<TopologyTemplate>,
    pub(crate) enum_defs: Vec<reify_types::EnumDef>,
    pub(crate) trait_defs: Vec<CompiledTrait>,
    pub(crate) constraint_defs: Vec<CompiledConstraintDef>,
    pub(crate) compiled_units: Vec<CompiledUnit>,
    pub(crate) pending_bound_checks: Vec<PendingBoundCheck>,
    /// Unified entity namespace tracker (spec §4.2.1): structures, occurrences,
    /// constraints, and fields all share the entity name space. Maps
    /// name → (first_span, first_kind_label).
    pub(crate) seen_entity_names: HashMap<String, (SourceSpan, &'static str)>,
    pub(crate) unit_registry: UnitRegistry,
    pub(crate) alias_registry: TypeAliasRegistry,
    /// Enum defs available for resolution: prelude enum_defs chained with
    /// module-local `enum_defs`. Populated by `enums_phase::build_resolution_enums`.
    pub(crate) resolution_enums: Vec<reify_types::EnumDef>,
    /// Function table available for resolution: user functions merged with
    /// prelude functions via [`crate::merge_prelude_functions`]. Populated by
    /// `functions_phase::phase_functions`.
    pub(crate) resolution_functions: Vec<CompiledFunction>,
}

impl CompilationCtx {
    /// Construct an empty CompilationCtx with every field zero-initialized.
    ///
    /// The two registries (`unit_registry`, `alias_registry`) are empty; no
    /// prelude content is seeded here — seeding is each phase's responsibility.
    pub(crate) fn new() -> Self {
        CompilationCtx {
            diagnostics: Vec::new(),
            imports: Vec::new(),
            functions: Vec::new(),
            fields: Vec::new(),
            templates: Vec::new(),
            enum_defs: Vec::new(),
            trait_defs: Vec::new(),
            constraint_defs: Vec::new(),
            compiled_units: Vec::new(),
            pending_bound_checks: Vec::new(),
            seen_entity_names: HashMap::new(),
            unit_registry: UnitRegistry::new(),
            alias_registry: TypeAliasRegistry::new(),
            resolution_enums: Vec::new(),
            resolution_functions: Vec::new(),
        }
    }

    /// Consume this ctx and assemble the final [`CompiledModule`].
    ///
    /// Combines the owned state accumulated across all phases with the external
    /// `compiled_purposes` (produced by `post_passes::phase_purposes`) and the
    /// `content_hash` (produced by `hash::compute_module_hash`). Calls
    /// `alias_registry.into_compiled()` to finalize type aliases.
    pub(crate) fn into_compiled_module(
        self,
        parsed: &ParsedModule,
        compiled_purposes: Vec<CompiledPurpose>,
        content_hash: ContentHash,
    ) -> CompiledModule {
        let type_aliases = self.alias_registry.into_compiled();
        CompiledModule {
            path: parsed.path.clone(),
            imports: self.imports,
            enum_defs: self.enum_defs,
            functions: self.functions,
            trait_defs: self.trait_defs,
            fields: self.fields,
            compiled_purposes,
            templates: self.templates,
            units: self.compiled_units,
            type_aliases,
            constraint_defs: self.constraint_defs,
            pragmas: parsed.pragmas.clone(),
            diagnostics: self.diagnostics,
            content_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CompilationCtx::new()` produces genuinely zero-state: every owned Vec
    /// is empty, the entity-name tracker is empty, and both registries have no
    /// entries.  Anchors the invariant that construction does no hidden seeding
    /// (prelude/stdlib content enters via the phase functions, not the ctor).
    #[test]
    fn new_produces_empty_state() {
        let ctx = CompilationCtx::new();
        assert!(ctx.diagnostics.is_empty(), "diagnostics should be empty");
        assert!(ctx.imports.is_empty(), "imports should be empty");
        assert!(ctx.functions.is_empty(), "functions should be empty");
        assert!(ctx.fields.is_empty(), "fields should be empty");
        assert!(ctx.templates.is_empty(), "templates should be empty");
        assert!(ctx.enum_defs.is_empty(), "enum_defs should be empty");
        assert!(ctx.trait_defs.is_empty(), "trait_defs should be empty");
        assert!(
            ctx.constraint_defs.is_empty(),
            "constraint_defs should be empty"
        );
        assert!(
            ctx.compiled_units.is_empty(),
            "compiled_units should be empty"
        );
        assert!(
            ctx.pending_bound_checks.is_empty(),
            "pending_bound_checks should be empty"
        );
        assert!(
            ctx.resolution_enums.is_empty(),
            "resolution_enums should be empty"
        );
        assert!(
            ctx.resolution_functions.is_empty(),
            "resolution_functions should be empty"
        );
        assert!(
            ctx.seen_entity_names.is_empty(),
            "seen_entity_names should be empty"
        );
        // A fresh UnitRegistry has no entries — lookup of any common unit
        // name must return None (no hidden seeding happens in new()).
        assert!(
            ctx.unit_registry.lookup("meter").is_none(),
            "fresh unit_registry should have no 'meter' entry"
        );
    }
}
