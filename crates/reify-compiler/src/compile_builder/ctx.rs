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
use reify_types::{ContentHash, Diagnostic, DiagnosticLabel, SourceSpan};

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
    /// module-local `enum_defs`. Populated by `enums_phase::build_resolution_enums_from_cache`.
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

    /// Returns `true` when `name` is either absent from `seen_entity_names` or
    /// its stored first-seen span equals `span` (i.e. this is the first-seen
    /// definition for this name). Returns `false` when a prior entry exists with
    /// a *different* span — a genuine duplicate definition in the entity namespace.
    ///
    /// Centralises the `(name, span) → "first def?"` contract so a future
    /// change to the tracker's value shape only needs to update this predicate.
    pub(crate) fn is_first_entity_def(&self, name: &str, span: SourceSpan) -> bool {
        self.seen_entity_names
            .get(name)
            .is_none_or(|(first_span, _)| *first_span == span)
    }

    /// Attempt to record `name` (kind `kind`) at `span` in the unified entity
    /// namespace.  Returns `true` if this is the first definition (entry
    /// inserted); returns `false` if a prior entry with a *different* span
    /// already exists, in which case a `duplicate entity definition` diagnostic
    /// is pushed to `self.diagnostics`.
    ///
    /// Callers that receive `true` should proceed with compilation of the
    /// declaration.  Callers that receive `false` should skip it.
    pub(crate) fn record_or_report_duplicate(
        &mut self,
        name: &str,
        span: SourceSpan,
        kind: &'static str,
    ) -> bool {
        if self.is_first_entity_def(name, span) {
            self.seen_entity_names.insert(name.to_string(), (span, kind));
            true
        } else {
            let (first_span, first_kind) = *self
                .seen_entity_names
                .get(name)
                .expect("duplicate path implies prior entry");
            self.diagnostics.push(
                Diagnostic::error(format!("duplicate entity definition '{}'", name))
                    .with_label(DiagnosticLabel::new(span, format!("{} defined here", kind)))
                    .with_label(DiagnosticLabel::new(
                        first_span,
                        format!("first defined as {} here", first_kind),
                    )),
            );
            false
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
            // Filled in by `module_pragmas::apply_module_pragmas` after assembly.
            default_tolerance: None,
            // Filled in by `module_pragmas::apply_module_pragmas` after assembly.
            declared_version: None,
            // Filled in by `module_pragmas::apply_module_pragmas` after assembly.
            solver_pragma: None,
            // Filled in by `module_pragmas::apply_module_pragmas` after assembly.
            kernel_pragma: None,
            diagnostics: self.diagnostics,
            content_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_first_entity_def` returns true when the name is absent from the
    /// tracker, true when the name is present with a matching span (same
    /// definition site, e.g. re-visiting the same decl), and false when the
    /// name is present with a different span (genuine duplicate).
    ///
    /// Anchors the `(name, span) → "first def?"` contract so a future
    /// tracker-shape change only needs to update `is_first_entity_def`.
    #[test]
    fn is_first_entity_def_absent_same_span_different_span() {
        let mut ctx = CompilationCtx::new();
        let span_a = SourceSpan::new(0, 10);
        let span_b = SourceSpan::new(20, 30);

        // (a) name absent → first def
        assert!(
            ctx.is_first_entity_def("Widget", span_a),
            "absent name should be treated as first def"
        );

        // Seed the tracker as pre_pass would.
        ctx.seen_entity_names
            .insert("Widget".to_string(), (span_a, "structure"));

        // (b) same span → still the first def (same definition revisited)
        assert!(
            ctx.is_first_entity_def("Widget", span_a),
            "matching span should be treated as first def"
        );

        // (c) different span → duplicate
        assert!(
            !ctx.is_first_entity_def("Widget", span_b),
            "different span should not be treated as first def"
        );
    }

    /// `record_or_report_duplicate` inserts on first call, is idempotent for
    /// same-span revisits, and emits a duplicate diagnostic on a different span.
    #[test]
    fn record_or_report_duplicate_inserts_and_deduplicates() {
        let mut ctx = CompilationCtx::new();
        let span_a = SourceSpan::new(0, 10);
        let span_b = SourceSpan::new(20, 30);

        // First insertion: new name → true, entry stored, no diagnostic.
        assert!(
            ctx.record_or_report_duplicate("Widget", span_a, "structure"),
            "first insertion should succeed"
        );
        assert_eq!(
            ctx.seen_entity_names.get("Widget"),
            Some(&(span_a, "structure")),
            "entry should be present after insertion"
        );
        assert!(ctx.diagnostics.is_empty(), "no diagnostic on first insertion");

        // Same name + same span is idempotent → true, still no diagnostic.
        assert!(
            ctx.record_or_report_duplicate("Widget", span_a, "structure"),
            "re-inserting with same span should return true"
        );
        assert!(ctx.diagnostics.is_empty(), "no diagnostic on same-span revisit");

        // Same name + different span → false, duplicate diagnostic emitted.
        assert!(
            !ctx.record_or_report_duplicate("Widget", span_b, "structure"),
            "duplicate span should return false"
        );
        assert_eq!(ctx.diagnostics.len(), 1, "exactly one diagnostic on duplicate");

        // Anchor the shape of the duplicate diagnostic — label count, span
        // ordering, and `{kind}` interpolation — so structural regressions
        // (swapped label order, missing second label, un-interpolated `{kind}`)
        // are caught at the unit level.
        let diag = &ctx.diagnostics[0];

        // 1. Top-level message contains the stable substring.
        assert!(
            diag.message.contains("duplicate entity definition 'Widget'"),
            "message should contain the stable duplicate-diagnostic substring, got: {:?}",
            diag.message,
        );

        // 2. Exactly two labels (duplicate site + first-seen site).
        assert_eq!(diag.labels.len(), 2, "duplicate diagnostic must have exactly two labels");

        // 3. labels[0] = duplicate site (span_b, "structure defined here").
        assert_eq!(
            diag.labels[0].span, span_b,
            "labels[0] should point to the duplicate site (span_b)"
        );
        assert!(
            diag.labels[0].message.contains("structure defined here"),
            "labels[0] message should interpolate the `{{kind}}` token into the '... defined here' template, got: {:?}",
            diag.labels[0].message,
        );

        // 4. labels[1] = first-seen site (span_a, "first defined as structure here").
        assert_eq!(
            diag.labels[1].span, span_a,
            "labels[1] should point to the first-seen site (span_a)"
        );
        assert!(
            diag.labels[1].message.contains("first defined as structure"),
            "labels[1] message should interpolate the `{{first_kind}}` token into the 'first defined as ... here' template, got: {:?}",
            diag.labels[1].message,
        );
    }

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
