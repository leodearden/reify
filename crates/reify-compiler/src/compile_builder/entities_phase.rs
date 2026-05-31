//! Phase-11 entities: compile every `structure` / `occurrence` declaration
//! and forward `import` declarations into `ctx.imports`; other decl arms are
//! passthroughs (already compiled in earlier phases).
//!
//! ## Import warning policy (task 2226)
//!
//! A "not resolved" warning is emitted **only** when the import path is absent
//! from the prelude supplied to `phase_entities`. When the prelude already
//! contains the module (e.g. because ModuleDag recursively compiled it, or
//! `compile_with_stdlib` seeded the stdlib prelude), the warning is suppressed.
//! The `CompiledImport` push is unconditional — downstream tools rely on it.
//!
//! Also hosts the post-pass `phase_pending_bound_checks` that drains
//! `ctx.pending_bound_checks` once all entities are compiled and the
//! template registry is complete, and `phase_sub_override_autos` that drains
//! `ctx.pending_sub_override_autos` for forward-declared-child sub-override
//! `auto` cells (task 3806, step 10).
//!
//! Both functions rebuild `trait_registry`, `field_registry`, and
//! `constraint_def_registry` phase-locally from `ctx` + `prelude`. The
//! constraint-def shadow warnings are emitted once in `defs_phase`; this
//! rebuild does NOT re-emit them (design decision in task 2035).

use std::collections::{HashMap, HashSet};

use reify_ast::ParsedModule;
use reify_core::{Diagnostic, DiagnosticLabel, SourceSpan, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, CompiledFunction, EnumDef};

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::compile_builder::defs_phase::build_constraint_def_registry;
use crate::compile_builder::traits_phase::build_trait_registry;
use crate::conformance::{check_fn_arg_conformance, check_trait_arg_conformance};
use crate::type_compat::{type_carries_trait_object, resolve_function_overload, OverloadResolution};
use crate::entity::{
    AutoResolutionRequest, EntityDefRef, PendingBoundCheck, PendingSubOverrideAuto,
    check_type_param_bounds, compile_entity,
};
use crate::type_resolution::TypeAliasRegistry;
use reify_core::ValueCellId;
use crate::types::{
    CompiledConstraintDef, CompiledField, CompiledImport, CompiledTrait, EntityKind,
    TopologyTemplate, ValueCellDecl, ValueCellKind, Visibility,
};
use crate::units::UnitRegistry;

/// Run phase-11 (entity compile) over `parsed.declarations`.
///
/// Constructs phase-local registries (`trait_registry`, `field_registry`,
/// `constraint_def_registry`) and dispatches each declaration to its
/// handler. Structure / Occurrence arms call `compile_entity` and push
/// onto `ctx.templates`. Import arm appends to `ctx.imports` unconditionally;
/// a "not resolved" warning is emitted only when the import path is absent from
/// `prelude` (see import warning policy in the module-level doc). Other arms
/// are passthroughs — they were already handled in earlier phases.
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

    // Build the set of structure/occurrence names known at compile time so
    // `param material : Material` (structure name) resolves to
    // Type::StructureRef("Material") rather than falling through to the
    // trait-name fallback.  Collected from local structure/occurrence decls
    // (already in `ctx.seen_entity_names` from pre_pass::collect_decl_refs)
    // and every prelude module's exported templates. See task 1876.
    let structure_names: HashSet<String> = ctx
        .seen_entity_names
        .iter()
        .filter(|(_, (_, kind))| *kind == "structure" || *kind == "occurrence")
        .map(|(name, _)| name.clone())
        .chain(
            prelude
                .iter()
                .flat_map(|m| m.templates.iter().map(|t| t.name.clone())),
        )
        .collect();

    // Lazily build the set of dotted import paths that are already resolved
    // in the prelude (e.g. "std.units", "a", "shapes.bolts"). Initialised on
    // the first Import declaration encountered; not allocated at all when the
    // module has no imports (the common case). Note: ModulePath::to_string()
    // uses '/' separators; use .0.join(".") to get the dotted form that
    // matches ImportDecl.path (see task 2226).
    let mut resolved_import_paths: Option<HashSet<String>> = None;

    // task 3540 (SIR-α): prelude `structure def` templates, keyed by name, so
    // the expression-lowering site can recognise `Foo()` as a structure
    // constructor (esc-3540-177 RULING 1). Built once here (immutable borrow
    // of `prelude` for the loop); `compile_entity` merges in local
    // already-compiled structure-defs. Occurrences are excluded — only
    // structure-defs are constructible via the ctor path.
    let prelude_template_registry: HashMap<String, &TopologyTemplate> = prelude
        .iter()
        .flat_map(|m| m.templates.iter())
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| (t.name.clone(), t))
        .collect();

    for decl in &parsed.declarations {
        match decl {
            reify_ast::Declaration::Structure(structure) => {
                if ctx.is_first_entity_def(&structure.name, structure.span) {
                    compile_entity_decl(
                        EntityDefRef::from(structure),
                        EntityKind::Structure,
                        &ctx.resolution_enums,
                        &ctx.resolution_functions,
                        &trait_registry,
                        &structure_names,
                        trait_names,
                        &field_registry,
                        &constraint_def_registry,
                        &ctx.unit_registry,
                        &ctx.alias_registry,
                        &mut ctx.pending_bound_checks,
                        &mut ctx.pending_auto_resolutions,
                        &mut ctx.pending_sub_override_autos,
                        &mut ctx.diagnostics,
                        &mut ctx.templates,
                        &prelude_template_registry,
                    );
                }
            }
            reify_ast::Declaration::Enum(_) => {
                // Already collected by pre_pass::collect_decl_refs.
            }
            reify_ast::Declaration::Import(import) => {
                ctx.imports.push(CompiledImport {
                    path: import.path.clone(),
                    kind: import.kind.clone(),
                    is_pub: import.is_pub,
                    span: import.span,
                });
                // Emit a "not resolved" warning only when the import path is
                // absent from the prelude (i.e. this entry point did not load
                // the imported file). Resolved imports — ModuleDag user modules
                // and stdlib modules — are already in the set and receive no
                // diagnostic. The set is built lazily: the first Import
                // declaration triggers collection; subsequent ones reuse it.
                // See task 2226 for detection strategy and wording rationale.
                let resolved = resolved_import_paths
                    .get_or_insert_with(|| prelude.iter().map(|m| m.path.0.join(".")).collect());
                if !resolved.contains(&import.path) {
                    ctx.diagnostics.push(
                        Diagnostic::warning(format!(
                            "import \"{}\" not resolved by this entry point",
                            import.path
                        ))
                        .with_label(DiagnosticLabel::new(
                            import.span,
                            "use compile_project (ModuleDag) for cross-file imports",
                        )),
                    );
                }
            }
            reify_ast::Declaration::Function(_) => {
                // Already compiled by functions_phase::phase_functions.
            }
            reify_ast::Declaration::Trait(_) => {
                // Already compiled by traits_phase::phase_traits.
            }
            reify_ast::Declaration::Occurrence(occurrence) => {
                if ctx.is_first_entity_def(&occurrence.name, occurrence.span) {
                    compile_entity_decl(
                        EntityDefRef::from(occurrence),
                        EntityKind::Occurrence,
                        &ctx.resolution_enums,
                        &ctx.resolution_functions,
                        &trait_registry,
                        &structure_names,
                        trait_names,
                        &field_registry,
                        &constraint_def_registry,
                        &ctx.unit_registry,
                        &ctx.alias_registry,
                        &mut ctx.pending_bound_checks,
                        &mut ctx.pending_auto_resolutions,
                        &mut ctx.pending_sub_override_autos,
                        &mut ctx.diagnostics,
                        &mut ctx.templates,
                        &prelude_template_registry,
                    );
                }
            }
            reify_ast::Declaration::Field(_) => {
                // Already compiled by fields_phase::phase_fields.
            }
            reify_ast::Declaration::Purpose(_) => {
                // Handled later by post_passes::phase_purposes.
            }
            reify_ast::Declaration::Constraint(_) => {
                // Already compiled by defs_phase::phase_constraint_defs; annotation/pragma validation ran there too.
            }
            reify_ast::Declaration::Unit(_) => {
                // Already compiled by units_phase::phase_units.
            }
            reify_ast::Declaration::TypeAlias(_) => {
                // Already compiled by aliases_phase::phase_aliases.
            }
            reify_ast::Declaration::Module(_) => {
                // No entity to build from a module declaration.
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
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    field_registry: &HashMap<String, &CompiledField>,
    constraint_def_registry: &HashMap<String, &CompiledConstraintDef>,
    unit_registry: &UnitRegistry,
    alias_registry: &TypeAliasRegistry,
    pending_bound_checks: &mut Vec<PendingBoundCheck>,
    pending_auto_resolutions: &mut Vec<AutoResolutionRequest>,
    pending_sub_override_autos: &mut Vec<PendingSubOverrideAuto>,
    diagnostics: &mut Vec<Diagnostic>,
    templates: &mut Vec<TopologyTemplate>,
    prelude_template_registry: &HashMap<String, &TopologyTemplate>,
) {
    let template = compile_entity(
        &entity_ref,
        kind,
        resolution_enums,
        resolution_functions,
        trait_registry,
        structure_names,
        trait_names,
        field_registry,
        constraint_def_registry,
        unit_registry,
        alias_registry,
        pending_bound_checks,
        pending_auto_resolutions,
        pending_sub_override_autos,
        diagnostics,
        templates,
        prelude_template_registry,
    );
    templates.push(template);
}

/// Post-compilation pass: drain `ctx.pending_bound_checks` now that all
/// structures are compiled and available in the template registry, and
/// dispatch each queued check across its `PendingBoundCheck` variant.
///
/// The `trait_registry` and `template_registry` are rebuilt phase-locally
/// from `ctx.trait_defs` (plus `prelude` trait defs) and `ctx.templates`.
pub(crate) fn phase_pending_bound_checks(ctx: &mut CompilationCtx, prelude: &[&CompiledModule]) {
    // task 3540 (SIR-α): seed with prelude `structure def` templates first,
    // then overlay local `ctx.templates` (local overrides prelude on name
    // collision — same "prelude first, then local" composition as
    // `build_trait_registry` and the `prelude_template_registry` in
    // `phase_entities`). Before SIR-α, `point_load(...)`/`fixed_support(...)`
    // were `Value::Map` stdlib builtins, so trait-arg conformance never needed
    // to resolve a *prelude* structure's `trait_bounds`. Now that those wave-1
    // structures lower to `StructureInstanceCtor` with a
    // `Type::StructureRef("PointLoad")` result type, the post-pass conformance
    // walker must see the prelude `PointLoad`/`FixedSupport` templates to read
    // their declared `trait_bounds` — otherwise
    // `emit_leaf_conformance_for_arg_type` early-returns on a registry miss and
    // a non-conforming ctor arg (e.g. `Beam(mat: PointLoad())` where
    // `mat : ElasticMaterial`) is silently admitted.
    let template_registry: HashMap<String, &TopologyTemplate> = prelude
        .iter()
        .flat_map(|m| m.templates.iter())
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t: &TopologyTemplate| (t.name.clone(), t))
        .chain(ctx.templates.iter().map(|t| (t.name.clone(), t)))
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
                let type_params = if let Some(target) = template_registry.get(target_name.as_str())
                {
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
                compiled_arg,
                span,
            } => {
                check_trait_arg_conformance(
                    &target_name,
                    &arg_name,
                    &compiled_arg,
                    span,
                    &template_registry,
                    &trait_registry,
                    &mut ctx.diagnostics,
                );
            }
        }
    }
}

/// Post-compilation pass: drain `ctx.pending_sub_override_autos` and resolve
/// each deferred sub-instance-override `auto` / `auto(free)` registration now
/// that all templates are compiled and the template registry is complete.
///
/// For each deferred entry:
/// - Look up the child template in the registry.
/// - If the member is found: push a scoped `ValueCellDecl { kind: Auto { free } }`
///   into the PARENT template's `value_cells`.
/// - If the member is absent: emit a "no such param" error (genuine missing member).
/// - If the child template itself is not found: emit a warning and skip
///   (the structure-reference error is handled elsewhere).
///
/// This mirrors the shape of `phase_pending_bound_checks` and runs between
/// `phase_auto_type_param_resolution` and `phase_pending_bound_checks` in lib.rs.
pub(crate) fn phase_sub_override_autos(ctx: &mut CompilationCtx, prelude: &[&CompiledModule]) {
    if ctx.pending_sub_override_autos.is_empty() {
        return;
    }

    // Build a template registry covering prelude + local compiled templates,
    // identical composition to `phase_pending_bound_checks`.
    let template_registry: HashMap<String, &TopologyTemplate> = prelude
        .iter()
        .flat_map(|m| m.templates.iter())
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t: &TopologyTemplate| (t.name.clone(), t))
        .chain(ctx.templates.iter().map(|t| (t.name.clone(), t)))
        .collect();

    let pending = std::mem::take(&mut ctx.pending_sub_override_autos);

    // Collect (parent_entity_name, scoped_id, cell_type, free, span) for push;
    // collect diagnostics separately so we can mutably borrow ctx.templates below.
    let mut cells_to_push: Vec<(String, ValueCellId, reify_core::Type, bool, reify_core::SourceSpan)> = Vec::new();

    for req in &pending {
        // Look up the child template.
        let child_tmpl = match template_registry.get(req.sub_structure_name.as_str()) {
            Some(t) => *t,
            None => {
                // Child structure unknown even after all templates are compiled.
                // The structure-reference error is raised elsewhere (bound-check
                // pass); skip silently here to avoid a confusing second error.
                continue;
            }
        };

        // Look up the member in the child template's value_cells.
        let cell_type = match child_tmpl
            .value_cells
            .iter()
            .find(|vc| vc.id.member == req.override_member)
            .map(|vc| vc.cell_type.clone())
        {
            Some(ty) => ty,
            None => {
                // Genuinely absent member — emit the error (same wording as the
                // inline path in entity.rs; the label span comes from req.span
                // which points to the `auto` / `auto(free)` expression).
                ctx.diagnostics.push(
                    Diagnostic::error(format!(
                        "sub `{}`: override for `{}` — no such param in `{}`",
                        req.sub_name, req.override_member, req.sub_structure_name
                    ))
                    .with_label(DiagnosticLabel::new(
                        req.span,
                        "this member does not exist in the child structure",
                    )),
                );
                continue;
            }
        };

        let scoped_entity = format!("{}.{}", req.parent_entity_name, req.sub_name);
        let scoped_id = ValueCellId::new(&scoped_entity, req.override_member.as_str());
        cells_to_push.push((req.parent_entity_name.clone(), scoped_id, cell_type, req.free, req.span));
    }

    // Apply the collected cell pushes.  We look up the parent template by name
    // and push into its `value_cells`.
    for (parent_name, scoped_id, cell_type, free, span) in cells_to_push {
        if let Some(parent_tmpl) = ctx.templates.iter_mut().find(|t| t.name == parent_name) {
            parent_tmpl.value_cells.push(ValueCellDecl {
                id: scoped_id,
                kind: ValueCellKind::Auto { free },
                visibility: Visibility::Public,
                cell_type,
                default_expr: None,
                solver_hints: vec![],
                span,
                // Auto sub-override cells are never aux declarations.
                is_aux: false,
            });
        }
    }
}

/// Post-compilation pass: walk compiled IR looking for `UserFunctionCall` nodes
/// whose function has trait-object params, and validate each arg against its
/// declared param type via `check_fn_arg_conformance`.
///
/// ## Registry composition
///
/// Rebuilds `template_registry` and `trait_registry` using the same
/// prelude-then-local composition as `phase_pending_bound_checks` so the
/// conformance walker sees the same templates as the structure path does.
///
/// ## Scope coverage
///
/// Covers: entity value-cell `default_expr`s, entity `constraints[*].expr`,
/// and function bodies (param_defaults + let-bindings + result_expr).
/// Out of scope: ports/connections/objective/realizations/match-arm-groups and
/// compiled_purposes (compiled after this pass). This bound matches the existing
/// structure-conformance baseline (sub-component-only, runs before purposes);
/// see task-4081 design decision §4.
///
/// ## Eval-builtin protection
///
/// Eval-builtins (bind/sweep/dim) have no `.ri` user-function signature, so
/// their calls lower to `CompiledExprKind::FunctionCall` (not `UserFunctionCall`).
/// For any remaining `UserFunctionCall`, we re-resolve the overload via
/// `resolve_function_overload`; eval-builtins are absent from
/// `ctx.resolution_functions` and produce `NoUserFunctions`, which is skipped.
/// Calls that already produced `NoMatch` or `Ambiguous` at the original call
/// site are also skipped here (they carry their own diagnostics).
///
/// ## Overload disambiguation (task-4081 design decision §5)
///
/// `resolution_functions` is a flat `Vec` that can contain multiple same-name
/// overloads.  Collecting it into a `name → &CompiledFunction` HashMap would
/// collapse overloads to the last-inserted entry, so conformance could target
/// the wrong signature (e.g. `couple(DrivingJoint)` + `couple(Real)` → only
/// `couple(Real)` checked → non-conformance silently missed).  Instead we
/// re-resolve each call via `resolve_function_overload` using the args'
/// `result_type`s, which reproduces exactly the overload selected at the
/// original call site.
pub(crate) fn phase_fn_arg_conformance(ctx: &mut CompilationCtx, prelude: &[&CompiledModule]) {
    // Build template registry (same composition as phase_pending_bound_checks).
    let template_registry: HashMap<String, &TopologyTemplate> = prelude
        .iter()
        .flat_map(|m| m.templates.iter())
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t: &TopologyTemplate| (t.name.clone(), t))
        .chain(ctx.templates.iter().map(|t| (t.name.clone(), t)))
        .collect();

    // Build trait registry (same composition as phase_pending_bound_checks).
    let trait_registry = build_trait_registry(&ctx.trait_defs, prelude);

    // Take a reference to the full resolution_functions slice.  Re-resolution
    // per call (see doc-comment) requires the full table, not a name-keyed map.
    let resolution_functions: &[CompiledFunction] = &ctx.resolution_functions;

    // Collect diagnostics into a local vec to avoid borrow-checker conflicts
    // (we hold shared borrows on ctx.templates and ctx.resolution_functions via
    // template_registry / resolution_functions while also needing &mut ctx.diagnostics).
    let mut new_diagnostics: Vec<reify_core::Diagnostic> = Vec::new();

    let walk = |expr: &CompiledExpr, span: SourceSpan, diags: &mut Vec<reify_core::Diagnostic>| {
        check_expr_fn_arg_conformance(
            expr,
            resolution_functions,
            &template_registry,
            &trait_registry,
            span,
            diags,
        );
    };

    // Walk entity value-cell default_expr fields and constraint exprs.
    for template in &ctx.templates {
        for vc in &template.value_cells {
            if let Some(expr) = &vc.default_expr {
                walk(expr, vc.span, &mut new_diagnostics);
            }
        }
        for constraint in &template.constraints {
            walk(&constraint.expr, constraint.span, &mut new_diagnostics);
        }
    }

    // Walk function bodies: param defaults, let-bindings, result expr.
    for f in &ctx.functions {
        for default in f.param_defaults.iter().flatten() {
            walk(default, SourceSpan::empty(0), &mut new_diagnostics);
        }
        for (_, expr) in &f.body.let_bindings {
            walk(expr, SourceSpan::empty(0), &mut new_diagnostics);
        }
        walk(&f.body.result_expr, SourceSpan::empty(0), &mut new_diagnostics);
    }

    ctx.diagnostics.extend(new_diagnostics);
}

/// Walk a single `CompiledExpr` tree, calling `check_fn_arg_conformance` for
/// every `UserFunctionCall` node whose resolved overload has trait-object params.
///
/// Re-resolves each call via `resolve_function_overload` using the args'
/// `result_type`s to pick the correct overload when multiple same-name functions
/// exist.  Only `OverloadResolution::Resolved` is acted upon; `NoMatch`,
/// `Ambiguous`, and `NoUserFunctions` are skipped (they carry their own
/// diagnostics from the original call site, or indicate an eval-builtin).
fn check_expr_fn_arg_conformance(
    expr: &CompiledExpr,
    functions: &[CompiledFunction],
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    representative_span: SourceSpan,
    diagnostics: &mut Vec<reify_core::Diagnostic>,
) {
    expr.walk(&mut |node: &CompiledExpr| {
        let CompiledExprKind::UserFunctionCall { function_name, args } = &node.kind else {
            return;
        };
        // Re-resolve the overload using the args' result_types.
        // This correctly disambiguates same-name overloads and skips
        // eval-builtins (NoUserFunctions) and already-diagnosed failures
        // (NoMatch / Ambiguous).
        let arg_result_types: Vec<Type> =
            args.iter().map(|a| a.result_type.clone()).collect();
        let f = match resolve_function_overload(function_name, &arg_result_types, functions) {
            OverloadResolution::Resolved(f) => f,
            _ => return,
        };
        // Check each param whose type carries a trait object.
        for ((param_name, param_ty), arg) in f.params.iter().zip(args.iter()) {
            if !type_carries_trait_object(param_ty) {
                continue;
            }
            check_fn_arg_conformance(
                param_ty,
                param_name,
                arg,
                representative_span,
                template_registry,
                trait_registry,
                diagnostics,
            );
        }
    });
}
