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
use reify_types::{CompiledFunction, Diagnostic, DiagnosticLabel, EnumDef, SourceSpan};

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

    // Constraint-def registry: prelude pub defs first (without re-warning;
    // the shadow-warning pass ran in phase-10), then local overrides.
    // `.entry().or_insert()` encodes the first-imported-wins policy without
    // a separate sentinel set.
    let mut constraint_def_registry: HashMap<String, &CompiledConstraintDef> = HashMap::new();
    for m in prelude {
        for cd in m.constraint_defs.iter().filter(|c| c.is_pub) {
            constraint_def_registry.entry(cd.name.clone()).or_insert(cd);
        }
    }
    for cd in &ctx.constraint_defs {
        constraint_def_registry.insert(cd.name.clone(), cd);
    }

    for decl in &parsed.declarations {
        match decl {
            reify_syntax::Declaration::Structure(structure) => {
                compile_entity_decl(
                    EntityDefRef::from(structure),
                    EntityKind::Structure,
                    &ctx.seen_entity_names,
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
            reify_syntax::Declaration::Enum(_) => {
                // Already collected in pre-pass above.
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
                // Already compiled in pre-pass above.
            }
            reify_syntax::Declaration::Trait(_) => {
                // Already compiled in trait pre-pass above.
            }
            reify_syntax::Declaration::Occurrence(occurrence) => {
                compile_entity_decl(
                    EntityDefRef::from(occurrence),
                    EntityKind::Occurrence,
                    &ctx.seen_entity_names,
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
            reify_syntax::Declaration::Field(_) => {
                // Already compiled in field pre-pass above.
            }
            reify_syntax::Declaration::Purpose(_) => {
                // Compiled in dedicated purpose pass below.
            }
            reify_syntax::Declaration::Constraint(_) => {
                // Already compiled by the constraint_defs pre-pass above;
                // annotation/pragma validation ran there too.
            }
            reify_syntax::Declaration::Unit(_) => {
                // Already compiled in unit pre-pass above.
            }
            reify_syntax::Declaration::TypeAlias(_) => {
                // Already compiled in type alias pre-pass above.
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
/// the whole ctx.
#[allow(clippy::too_many_arguments)]
fn compile_entity_decl(
    entity_ref: EntityDefRef<'_>,
    kind: EntityKind,
    seen_entity_names: &HashMap<String, (SourceSpan, &'static str)>,
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
    let is_first_def = seen_entity_names
        .get(entity_ref.name)
        .is_none_or(|(first_span, _)| *first_span == entity_ref.span);
    if !is_first_def {
        return;
    }
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
