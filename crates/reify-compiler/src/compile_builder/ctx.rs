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

use reify_types::{Diagnostic, SourceSpan};

use crate::entity::PendingBoundCheck;
use crate::type_resolution::TypeAliasRegistry;
use crate::types::{
    CompiledConstraintDef, CompiledField, CompiledImport, CompiledTrait, CompiledUnit,
    TopologyTemplate,
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
}
