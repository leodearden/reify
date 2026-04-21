//! Phase-11 entities: compile every `structure` / `occurrence` declaration
//! and forward `import` declarations into `ctx.imports` with their
//! placeholder diagnostics; other decl arms are passthroughs (already
//! compiled in earlier phases).
//!
//! Also hosts the post-pass `phase_pending_bound_checks` that drains
//! `ctx.pending_bound_checks` once all entities are compiled and the
//! template registry is complete.
//!
//! Both functions rebuild `trait_registry`, `field_registry`, and
//! `constraint_def_registry` phase-locally from `ctx` + `prelude`. The
//! constraint-def shadow warnings are emitted once in `defs_phase`; this
//! rebuild does NOT re-emit them (design decision in task 2035).

use std::collections::{HashMap, HashSet};

use reify_syntax::ParsedModule;
use reify_types::{CompiledFunction, Diagnostic, DiagnosticLabel, EnumDef};

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::compile_builder::traits_phase::build_trait_registry;
use crate::conformance::check_trait_arg_conformance;
use crate::entity::{EntityDefRef, PendingBoundCheck, check_type_param_bounds, compile_entity};
use crate::type_resolution::TypeAliasRegistry;
use crate::types::{
    CompiledConstraintDef, CompiledField, CompiledImport, CompiledTrait, EntityKind,
    TopologyTemplate,
};
use crate::units::UnitRegistry;

/// Build a combined constraint-def registry: prelude pub defs first
/// (first-imported wins on cross-prelude name collisions), then local defs
/// override on name collision with prelude.
///
/// Borrows from `local` (module-local constraint defs in ctx) and every prelude
/// module's `constraint_defs`. Non-pub prelude defs are excluded — only pub
/// constraint defs are exported. Shadow warnings for cross-prelude name
/// collisions are NOT emitted here — they were already emitted once in
/// `defs_phase::emit_constraint_def_shadow_warnings`.
pub(crate) fn build_constraint_def_registry<'a>(
    local: &'a [CompiledConstraintDef],
    prelude: &[&'a CompiledModule],
) -> HashMap<String, &'a CompiledConstraintDef> {
    let mut registry: HashMap<String, &'a CompiledConstraintDef> = HashMap::new();
    for m in prelude {
        for cd in m.constraint_defs.iter().filter(|c| c.is_pub) {
            registry.entry(cd.name.clone()).or_insert(cd);
        }
    }
    for cd in local {
        registry.insert(cd.name.clone(), cd);
    }
    registry
}

/// Run phase-11 (entity compile) over `parsed.declarations`.
///
/// Constructs phase-local registries (`trait_registry`, `field_registry`,
/// `constraint_def_registry`) and dispatches each declaration to its
/// handler. Structure / Occurrence arms call `compile_entity` and push
/// onto `ctx.templates`. Import arm appends to `ctx.imports` with a
/// placeholder warning diagnostic. Other arms are passthroughs — they were
/// already handled in earlier phases.
pub(crate) fn phase_entities(
    ctx: &mut CompilationCtx,
    parsed: &ParsedModule,
    trait_names: &HashSet<String>,
    prelude: &[&CompiledModule],
) {
    // Rebuild the trait registry fresh: prelude first, then local overrides.
    let trait_registry = build_trait_registry(&ctx.trait_defs, prelude);

    // Field registry built from local ctx.fields only (fields are not
    // re-exported from prelude modules in the current scope resolution).
    let field_registry: HashMap<String, &CompiledField> =
        ctx.fields.iter().map(|f| (f.name.clone(), f)).collect();

    let constraint_def_registry = build_constraint_def_registry(&ctx.constraint_defs, prelude);

    for decl in &parsed.declarations {
        match decl {
            reify_syntax::Declaration::Structure(structure) => {
                if ctx.is_first_entity_def(&structure.name, structure.span) {
                    compile_entity_decl(
                        EntityDefRef::from(structure),
                        EntityKind::Structure,
                        &ctx.resolution_enums,
                        &ctx.resolution_functions,
                        &trait_registry,
                        trait_names,
                        &field_registry,
                        &constraint_def_registry,
                        &ctx.unit_registry,
                        &ctx.alias_registry,
                        &mut ctx.pending_bound_checks,
                        &mut ctx.diagnostics,
                        &mut ctx.templates,
                    );
                }
            }
            reify_syntax::Declaration::Enum(_) => {
                // Already collected by pre_pass::collect_decl_refs.
            }
            reify_syntax::Declaration::Import(import) => {
                ctx.imports.push(CompiledImport {
                    path: import.path.clone(),
                    kind: import.kind.clone(),
                    is_pub: import.is_pub,
                    span: import.span,
                });
                ctx.diagnostics.push(
                    Diagnostic::warning(format!(
                        "import \"{}\" noted; module resolution not yet implemented",
                        import.path
                    ))
                    .with_label(DiagnosticLabel::new(import.span, "import")),
                );
            }
            reify_syntax::Declaration::Function(_) => {
                // Already compiled by functions_phase::phase_functions.
            }
            reify_syntax::Declaration::Trait(_) => {
                // Already compiled by traits_phase::phase_traits.
            }
            reify_syntax::Declaration::Occurrence(occurrence) => {
                if ctx.is_first_entity_def(&occurrence.name, occurrence.span) {
                    compile_entity_decl(
                        EntityDefRef::from(occurrence),
                        EntityKind::Occurrence,
                        &ctx.resolution_enums,
                        &ctx.resolution_functions,
                        &trait_registry,
                        trait_names,
                        &field_registry,
                        &constraint_def_registry,
                        &ctx.unit_registry,
                        &ctx.alias_registry,
                        &mut ctx.pending_bound_checks,
                        &mut ctx.diagnostics,
                        &mut ctx.templates,
                    );
                }
            }
            reify_syntax::Declaration::Field(_) => {
                // Already compiled by fields_phase::phase_fields.
            }
            reify_syntax::Declaration::Purpose(_) => {
                // Handled later by post_passes::phase_purposes.
            }
            reify_syntax::Declaration::Constraint(_) => {
                // Already compiled by defs_phase::phase_constraint_defs; annotation/pragma validation ran there too.
            }
            reify_syntax::Declaration::Unit(_) => {
                // Already compiled by units_phase::phase_units.
            }
            reify_syntax::Declaration::TypeAlias(_) => {
                // Already compiled by aliases_phase::phase_aliases.
            }
        }
    }
}

/// Compile a single `structure` or `occurrence` declaration, pushing the
/// resulting template onto `templates` if this is the first-seen definition
/// for the entity name. Structure and Occurrence arms are otherwise
/// byte-for-byte identical; the caller distinguishes them by passing the
/// appropriate `kind` and `EntityDefRef::from(...)`.
///
/// Takes the `ctx` fields as split `&` / `&mut` borrows rather than
/// `&mut CompilationCtx` because the caller holds shared borrows of
/// `ctx.trait_defs`, `ctx.fields`, and `ctx.constraint_defs` (via the
/// phase-local registries) that would conflict with an exclusive borrow of
/// the whole ctx. The first-def guard (`ctx.is_first_entity_def`) is
/// evaluated at the call site in `phase_entities` before invoking this
/// function.
#[allow(clippy::too_many_arguments)]
fn compile_entity_decl(
    entity_ref: EntityDefRef<'_>,
    kind: EntityKind,
    resolution_enums: &[EnumDef],
    resolution_functions: &[CompiledFunction],
    trait_registry: &HashMap<String, &CompiledTrait>,
    trait_names: &HashSet<String>,
    field_registry: &HashMap<String, &CompiledField>,
    constraint_def_registry: &HashMap<String, &CompiledConstraintDef>,
    unit_registry: &UnitRegistry,
    alias_registry: &TypeAliasRegistry,
    pending_bound_checks: &mut Vec<PendingBoundCheck>,
    diagnostics: &mut Vec<Diagnostic>,
    templates: &mut Vec<TopologyTemplate>,
) {
    let template = compile_entity(
        &entity_ref,
        kind,
        resolution_enums,
        resolution_functions,
        trait_registry,
        trait_names,
        field_registry,
        constraint_def_registry,
        unit_registry,
        alias_registry,
        pending_bound_checks,
        diagnostics,
        templates,
    );
    templates.push(template);
}

/// Post-compilation pass: drain `ctx.pending_bound_checks` now that all
/// structures are compiled and available in the template registry, and
/// dispatch each queued check across its `PendingBoundCheck` variant.
///
/// The `trait_registry` and `template_registry` are rebuilt phase-locally
/// from `ctx.trait_defs` (plus `prelude` trait defs) and `ctx.templates`.
pub(crate) fn phase_pending_bound_checks(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
) {
    let template_registry: HashMap<String, &TopologyTemplate> = ctx
        .templates
        .iter()
        .map(|t: &TopologyTemplate| (t.name.clone(), t))
        .collect();

    // Rebuild the trait registry (same composition as phase_entities).
    let trait_registry = build_trait_registry(&ctx.trait_defs, prelude);

    let pending_bound_checks = std::mem::take(&mut ctx.pending_bound_checks);
    for check in pending_bound_checks {
        match check {
            PendingBoundCheck::SubComponent {
                type_args,
                target_name,
                span,
            } => {
                // Resolve type_params from the template registry now that
                // all structures are compiled.
                let type_params =
                    if let Some(target) = template_registry.get(target_name.as_str()) {
                        if target.type_params.is_empty() {
                            continue; // target has no type params, nothing to check
                        }
                        &target.type_params
                    } else {
                        // Target structure not found — skip (may be an external/unknown structure)
                        continue;
                    };

                check_type_param_bounds(
                    type_params,
                    &type_args,
                    &target_name,
                    &template_registry,
                    &trait_registry,
                    &mut ctx.diagnostics,
                    span,
                );
            }
            PendingBoundCheck::TraitConformance {
                type_params,
                type_args,
                target_name,
                span,
            } => {
                check_type_param_bounds(
                    &type_params,
                    &type_args,
                    &target_name,
                    &template_registry,
                    &trait_registry,
                    &mut ctx.diagnostics,
                    span,
                );
            }
            PendingBoundCheck::TraitArgConformance {
                target_name,
                arg_name,
                arg_type,
                arg_call_name,
                span,
            } => {
                check_trait_arg_conformance(
                    &target_name,
                    &arg_name,
                    &arg_type,
                    arg_call_name.as_deref(),
                    span,
                    &template_registry,
                    &trait_registry,
                    &mut ctx.diagnostics,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_constraint_def_registry;
    use crate::CompiledModule;
    use crate::types::CompiledConstraintDef;
    use reify_types::{ContentHash, ModulePath, SourceSpan};

    fn mk_cd(name: &str, is_pub: bool) -> CompiledConstraintDef {
        CompiledConstraintDef {
            name: name.to_string(),
            is_pub,
            type_params: vec![],
            params: vec![],
            predicates: vec![],
            span: SourceSpan::new(0, 0),
            content_hash: ContentHash::of_str(""),
            pragmas: vec![],
            annotations: vec![],
            annotations_optimized_target: None,
        }
    }

    fn mk_module(path: &str, cds: Vec<CompiledConstraintDef>) -> CompiledModule {
        CompiledModule {
            path: ModulePath::single(path),
            imports: vec![],
            enum_defs: vec![],
            functions: vec![],
            trait_defs: vec![],
            fields: vec![],
            compiled_purposes: vec![],
            templates: vec![],
            units: vec![],
            type_aliases: vec![],
            constraint_defs: cds,
            pragmas: vec![],
            diagnostics: vec![],
            content_hash: ContentHash::of_str(""),
        }
    }

    /// Covers four behaviours of `build_constraint_def_registry`:
    ///
    /// 1. Local-only: empty prelude → local def appears in registry.
    /// 2. Prelude pub inclusion: pub defs included, non-pub filtered.
    /// 3. Local overrides prelude: local def wins on name collision.
    /// 4. First prelude wins: first-imported module wins on cross-prelude collision.
    #[test]
    fn build_constraint_def_registry_prelude_first_local_override() {
        // Case 1: local-only baseline.
        let local_a = vec![mk_cd("A", false)];
        let reg = build_constraint_def_registry(&local_a, &[]);
        assert!(
            std::ptr::eq(reg["A"], &local_a[0]),
            "local def should appear in registry"
        );

        // Case 2: prelude pub inclusion — pub included, non-pub filtered.
        let prelude_bc = mk_module("prelude_bc", vec![mk_cd("B", true), mk_cd("C", false)]);
        let reg = build_constraint_def_registry(&[], &[&prelude_bc]);
        assert!(reg.contains_key("B"), "pub prelude def 'B' should be included");
        assert!(!reg.contains_key("C"), "non-pub prelude def 'C' should be excluded");

        // Case 3: local overrides prelude.
        let local_d = vec![mk_cd("D", false)];
        let prelude_d = mk_module("prelude_d", vec![mk_cd("D", true)]);
        let reg = build_constraint_def_registry(&local_d, &[&prelude_d]);
        assert!(
            std::ptr::eq(reg["D"], &local_d[0]),
            "local def should override prelude def for 'D'"
        );

        // Case 4: first prelude wins on cross-prelude collision.
        let prelude_e1 = mk_module("prelude_e1", vec![mk_cd("E", true)]);
        let prelude_e2 = mk_module("prelude_e2", vec![mk_cd("E", true)]);
        let reg = build_constraint_def_registry(&[], &[&prelude_e1, &prelude_e2]);
        assert!(
            std::ptr::eq(reg["E"], &prelude_e1.constraint_defs[0]),
            "first prelude's def should win on cross-prelude collision"
        );
    }
}
