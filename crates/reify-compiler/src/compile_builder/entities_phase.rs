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
use crate::conformance::{check_expr_mechanism_joint_bound, check_fn_arg_conformance, check_trait_arg_conformance};
use crate::type_compat::{
    type_carries_trait_object, type_carries_type_param, unify,
    resolve_function_overload, OverloadResolution,
};
use crate::entity::{
    AutoResolutionRequest, EntityDefRef, PendingBoundCheck, PendingSubOverrideAuto,
    check_type_param_bounds, compile_entity,
};
use crate::type_resolution::TypeAliasRegistry;
use reify_core::ValueCellId;
use crate::types::{
    CompiledConstraintDef, CompiledField, CompiledForallBody, CompiledGeometryOp, CompiledImport,
    CompiledTrait, EntityKind, TopologyTemplate, ValueCellDecl, ValueCellKind, Visibility,
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

    // Track (sub_name, override_member) pairs already diagnosed for absent-member
    // errors, so that a duplicate body like `{ nope = auto\n nope = auto }` (which
    // produces two PendingSubOverrideAuto entries for the same member) emits exactly
    // one "no such param" error (task 4123 amendment, suggestion 2).
    let mut reported_absent: HashSet<(String, String)> = HashSet::new();
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
                // First occurrence per (sub_name, member) only; duplicates from
                // a body like `{ nope = auto\n nope = auto }` are suppressed
                // (task 4123 amendment, suggestion 2).
                if reported_absent.insert((req.sub_name.clone(), req.override_member.clone())) {
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
                }
                continue;
            }
        };

        let scoped_entity = format!("{}.{}", req.parent_entity_name, req.sub_name);
        let scoped_id = ValueCellId::new(&scoped_entity, req.override_member.as_str());
        cells_to_push.push((req.parent_entity_name.clone(), scoped_id, cell_type, req.free, req.span));
    }

    // Apply the collected cell pushes.  We look up the parent template by name
    // and push into its `value_cells`.  Accumulate duplicate-override warnings
    // separately to avoid a borrow conflict between ctx.templates and ctx.diagnostics.
    let mut dup_warnings: Vec<Diagnostic> = Vec::new();
    for (parent_name, scoped_id, cell_type, free, span) in cells_to_push {
        if let Some(parent_tmpl) = ctx.templates.iter_mut().find(|t| t.name == parent_name) {
            // Dedup guard (task 4123 S6 + amendment suggestion 1): the deferred
            // post-pass (Case 1) and the inline push (Case 3 in entity.rs) are
            // mutually exclusive by declaration order, so a duplicate can only arise
            // when the specialization body has two param_assignment nodes for the same
            // member (e.g. `{ bore = auto\n    bore = auto }`), producing two
            // PendingSubOverrideAuto entries.  First-assignment-wins; warn and skip if
            // the scoped id is already present in the parent's value_cells.
            if parent_tmpl.value_cells.iter().any(|c| c.id == scoped_id) {
                let member = &scoped_id.member;
                let sub_name = scoped_id.entity.rsplit('.').next().unwrap_or(&scoped_id.entity);
                dup_warnings.push(
                    Diagnostic::warning(format!(
                        "sub `{sub_name}`: duplicate override for member `{member}`; first assignment wins",
                    ))
                    .with_label(DiagnosticLabel::new(
                        span,
                        "this override is a duplicate; it will be ignored",
                    )),
                );
            } else {
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
    ctx.diagnostics.extend(dup_warnings);
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
/// Covers ALL `CompiledExpr`-bearing fields of every entity template — value-cell
/// `default_expr`s, `constraints[*].expr`, `objective`,
/// `realizations[*].operations[*]` geometry-op args, `ports[*]`
/// members/constraints/frame_expr, `guarded_groups[*]`
/// guard/members/constraints/else_*, `match_arm_groups[*]` arm guards,
/// `sub_components[*]` args/pose, and `forall_templates[*]` bodies (enumerated by
/// [`for_each_template_root_expr`]) — plus free-function bodies (`ctx.functions`:
/// param_defaults + let-bindings + result_expr) and associated-function bodies
/// (`ctx.templates[*].assoc_fns[*].function`, trait-default-injected or
/// structure-override: param_defaults + let-bindings + result_expr).
///
/// This breadth deliberately matches the GLOBAL step-2 resolution relaxation:
/// because a trait-carrying param acts as a resolution wildcard at EVERY call
/// site, a non-conforming call must be diagnosed wherever it can appear, else the
/// previously-existing "no matching overload" hard error is silently lost (a
/// soundness regression). See task-4081 design decision §6.
///
/// Residual (documented, no silent cap): `connections` carry only
/// `ConstraintNodeId` references + string `port_mappings` (no inline
/// `CompiledExpr` to walk), and `compiled_purposes` are compiled AFTER this pass
/// (lib.rs:382) — matching the existing structure-conformance baseline, which
/// also runs before purposes.
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
///
/// ## Diagnostic spans
///
/// Template-rooted diagnostics carry a representative span from the enclosing
/// field / realization / sub-component (threaded through
/// [`for_each_template_root_expr`]). Function-body diagnostics (param defaults,
/// `let`-bindings, `result_expr`), however, use `SourceSpan::empty(0)`: neither
/// `CompiledFunction` nor `CompiledFnBody` records per-binding spans, and
/// `CompiledExpr` nodes carry no span of their own. A non-conforming call inside a
/// function body therefore reports without a precise source location. This is a
/// documented (not silent) gap; threading body spans is deferred until the IR
/// records them (esc-4081 amend, reviewer_comprehensive diagnostics_quality).
///
/// ## Performance
///
/// Each `UserFunctionCall` node clones its args' `result_type`s into a fresh
/// `Vec<Type>` and calls `resolve_function_overload`, which linearly scans the
/// whole `resolution_functions` table — i.e. O(calls × functions) with a per-call
/// allocation. This is fine for typical modules. A `name → Vec<&CompiledFunction>`
/// index (built once, preserving all overloads — unlike the rejected collapsing
/// HashMap of design decision §5) would avoid the full-table scan, but
/// `resolve_function_overload` filters by name on a `&[CompiledFunction]` slice, so
/// an index would require changing that shared signature; deferred pending
/// profiling that warrants it (esc-4081 amend, reviewer_comprehensive performance).
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

    // Fast-path gate for type-param bound checking (task-4232 γ D5):
    // compute ONCE whether any function in the resolution table has bounded
    // type-params.  If not (e.g. non-generic sources like bracket_source, or any
    // source compiled without user-defined generic functions), skip the per-root
    // expression walk entirely.  This is O(|fns|) once vs O(|exprs| × |fns|)
    // per invocation and returns the check overhead to zero for non-generic sources.
    let has_bounded_generic_fns = resolution_functions.iter().any(|f| {
        !f.type_params.is_empty() && f.type_params.iter().any(|tp| !tp.bounds.is_empty())
    });

    // Collect diagnostics into a local vec to avoid borrow-checker conflicts
    // (we hold shared borrows on ctx.templates and ctx.resolution_functions via
    // template_registry / resolution_functions while also needing &mut ctx.diagnostics).
    let mut new_diagnostics: Vec<reify_core::Diagnostic> = Vec::new();

    let walk = |expr: &CompiledExpr, span: SourceSpan, diags: &mut Vec<reify_core::Diagnostic>| {
        // task 4232 γ: single walk resolves each UserFunctionCall overload once,
        // performing both trait-object arg conformance (D4) and fn type-param
        // bound validation (D5) from the shared Resolved(f) result — avoiding
        // the double walk + double resolve_function_overload call that separate
        // functions would incur (reviewer_comprehensive performance, code_duplication).
        check_expr_fn_calls(
            expr,
            resolution_functions,
            &template_registry,
            &trait_registry,
            span,
            has_bounded_generic_fns,
            diags,
        );
        // task 4310 (mechanism γ): L2 compile-time DrivingJoint-bound check.
        // Rejects bind(couple(...), v) etc. with MechanismNonDrivingJoint.
        check_expr_mechanism_joint_bound(
            expr,
            &template_registry,
            &trait_registry,
            span,
            diags,
        );
        // task 4444 ζ: StructureInstanceCtor trait-arg conformance for value-cell
        // let bindings.  phase_pending_bound_checks covers sub-component ctor args
        // (queued in entity.rs at sub-lowering time); value-cell `let c = Foo(...)`
        // bindings lower to StructureInstanceCtor expressions and were NOT checked.
        // Walking every StructureInstanceCtor here closes that gap.
        check_expr_struct_ctor_args(
            expr,
            &template_registry,
            &trait_registry,
            span,
            diags,
        );
    };

    // Walk EVERY CompiledExpr-bearing root field of each entity template via the
    // centralized enumerator (value cells, constraints, objective, realizations,
    // ports, guarded groups, match-arm guards, sub-components, forall bodies).
    // Coverage deliberately matches the GLOBAL step-2 resolution relaxation so a
    // non-conforming call is diagnosed wherever it can appear (see doc-comment).
    for template in &ctx.templates {
        for_each_template_root_expr(template, &mut |expr, span| {
            walk(expr, span, &mut new_diagnostics);
        });
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

    // Walk associated-function bodies on every LOCAL template (step-14).
    //
    // The step-2 wildcard relaxation is GLOBAL and also reaches assoc-fn bodies
    // (compiled via compile_assoc_function -> compile_expr ->
    // resolve_function_overload), so a non-conforming `couple(self)` inside an
    // assoc fn now resolves to a `UserFunctionCall` instead of the previous
    // `no matching overload` hard error. `assoc_fns` live on `TopologyTemplate`
    // yet are NOT enumerated by `for_each_template_root_expr` and are NOT in
    // `ctx.functions`, so without this loop that hard error would be silently
    // lost (a soundness regression shipping in this same diff).
    //
    // Mirrors the free-function loop above exactly — assoc fns ARE
    // `CompiledFunction`s. Only `ctx.templates` (local) assoc fns are walked, not
    // prelude templates' assoc fns: those were checked when the prelude compiled,
    // consistent with walking `ctx.functions` (local) rather than prelude fns.
    //
    // No double-count: assoc fns are disjoint from `value_cells`, the template
    // root fields enumerated by `for_each_template_root_expr`, and `ctx.functions`
    // (module/free fns); each expr tree is visited exactly once. `SourceSpan::empty(0)`
    // because assoc-fn bodies carry no per-expr spans (same gap as free-fn bodies).
    for template in &ctx.templates {
        for af in &template.assoc_fns {
            let f = &af.function;
            for default in f.param_defaults.iter().flatten() {
                walk(default, SourceSpan::empty(0), &mut new_diagnostics);
            }
            for (_, expr) in &f.body.let_bindings {
                walk(expr, SourceSpan::empty(0), &mut new_diagnostics);
            }
            walk(&f.body.result_expr, SourceSpan::empty(0), &mut new_diagnostics);
        }
    }

    ctx.diagnostics.extend(new_diagnostics);
}

/// Walk a single `CompiledExpr` tree and, for each `UserFunctionCall` node,
/// resolve the overload **once** then perform two checks from the shared result:
///
/// 1. **Trait-object arg conformance** (D4, task-4232 γ): for every param whose
///    type carries a trait object, verify the arg's concrete type conforms,
///    skipping args whose `result_type` carries a type-param (deferred wildcard).
///
/// 2. **Fn type-param bound validation** (D5, task-4232 γ): if `check_bounds` is
///    true and the resolved fn has ≥1 bounded type-param, re-derive the type-arg
///    substitution via [`unify`] and call [`check_type_param_bounds`].
///
/// Using a single walk with one `resolve_function_overload` call per node avoids
/// the O(2×|fns|) overhead of two separate walks (reviewer_comprehensive
/// performance/code_duplication notes, task-4232 amendment pass).
///
/// Only `OverloadResolution::Resolved` is acted upon; `NoMatch`, `Ambiguous`,
/// and `NoUserFunctions` are skipped (each has its own diagnostic or is an
/// eval-builtin that carries no user-fn semantics).
///
/// Reuses the structure-side bound diagnostic message (no new `DiagnosticCode`
/// — PRD §7.3).  The caller (`phase_fn_arg_conformance`) already owns the
/// registries; no additional registry build is required.
fn check_expr_fn_calls(
    expr: &CompiledExpr,
    functions: &[CompiledFunction],
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    representative_span: SourceSpan,
    check_bounds: bool,
    diagnostics: &mut Vec<reify_core::Diagnostic>,
) {
    expr.walk(&mut |node: &CompiledExpr| {
        let CompiledExprKind::UserFunctionCall { function_name, args } = &node.kind else {
            return;
        };

        // Re-resolve the overload once using the args' result_types.
        // This disambiguates same-name overloads and skips eval-builtins
        // (NoUserFunctions) and already-diagnosed failures (NoMatch / Ambiguous).
        let arg_result_types: Vec<Type> =
            args.iter().map(|a| a.result_type.clone()).collect();
        let f = match resolve_function_overload(function_name, &arg_result_types, functions) {
            OverloadResolution::Resolved(f) => f,
            _ => return,
        };

        // ── Check 1: trait-object arg conformance ──────────────────────────
        for ((param_name, param_ty), arg) in f.params.iter().zip(args.iter()) {
            if !type_carries_trait_object(param_ty) {
                continue;
            }
            // D4 (task-4232 γ): skip conformance for TypeParam-typed args.
            // A type-param-carrying arg forwarded to a trait-object param is
            // "unknown, not definitely non-conforming" — its real conformance
            // is decided when T is bound to a concrete type at the call site.
            if type_carries_type_param(&arg.result_type) {
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

        // ── Check 2: fn type-param bound validation ─────────────────────────
        // Gated on `check_bounds` (= `has_bounded_generic_fns`, computed once
        // before the walk) so non-generic sources pay zero per-node overhead.
        if check_bounds
            && !f.type_params.is_empty()
            && f.type_params.iter().any(|tp| !tp.bounds.is_empty())
        {
            // Re-derive the type-arg substitution by unifying each declared
            // param type with the corresponding arg's result_type — same logic
            // as expr.rs:1530.  An Err(TypeArgConflict) means the call was
            // already poisoned at compile time (expr.rs emits FnTypeArgConflict
            // and the call becomes a poison Literal), so it never appears as a
            // UserFunctionCall in compiled trees; the early return is defensive.
            let mut subst: HashMap<String, Type> = HashMap::new();
            for ((_, param_ty), arg) in f.params.iter().zip(args.iter()) {
                if unify(param_ty, &arg.result_type, &mut subst).is_err() {
                    return;
                }
            }

            // Build declaration-ordered type_args: unbound params self-fill as
            // Type::TypeParam(name) so check_type_param_bounds' TypeParam guard
            // (entity.rs:3692) skips them — delivering "unbound → not checked".
            let type_args: Vec<Type> = f
                .type_params
                .iter()
                .map(|tp| {
                    subst
                        .get(&tp.name)
                        .cloned()
                        .unwrap_or_else(|| Type::TypeParam(tp.name.clone()))
                })
                .collect();

            // Delegate to the structure-side bound checker — reuses the same
            // diagnostic message format, transitive satisfies_trait_bound walk,
            // and arity/default/TypeParam-skip logic.
            check_type_param_bounds(
                &f.type_params,
                &type_args,
                function_name,
                template_registry,
                trait_registry,
                diagnostics,
                representative_span,
            );
        }
    });
}

/// Walk `expr` and its descendants; for every `StructureInstanceCtor` node call
/// `check_trait_arg_conformance` on each named arg whose declared param type is
/// `List<TraitObject(...)>`.
///
/// This closes the gap left by `phase_pending_bound_checks`: that phase only
/// queues `TraitArgConformance` checks for sub-component declarations (entity.rs
/// sub-lowering path).  Value-cell `let c = Foo(...)` bindings lower to
/// `StructureInstanceCtor` expressions and were not checked.  By walking the
/// compiled expression tree here we cover them with the same
/// `check_trait_arg_conformance` logic that sub-components use.
///
/// **Scope: `List<TraitObject>` params only.**  Bare `TraitObject` params (e.g.
/// `ConstitutiveLawInput.law : ConstitutiveLaw`) are intentionally excluded.
/// Those params are either already covered by the fn-call/sub-component paths,
/// or are deliberate type-coercion escape hatches pending trait-coerce support
/// (e.g. `ConstitutiveLawInput`, task δ/3780 `TODO(trait-coerce)`).  Extending
/// to bare `TraitObject` would regress those escape-hatch call sites and is
/// deferred to a follow-up once the coercion story is settled.
fn check_expr_struct_ctor_args(
    expr: &CompiledExpr,
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    representative_span: SourceSpan,
    diagnostics: &mut Vec<reify_core::Diagnostic>,
) {
    expr.walk(&mut |node: &CompiledExpr| {
        let CompiledExprKind::StructureInstanceCtor { type_name, ordered_args, .. } = &node.kind
        else {
            return;
        };
        // Resolve the target template once; skip if not found.
        let Some(template) = template_registry.get(type_name.as_str()) else {
            return;
        };
        for (arg_name, compiled_arg) in ordered_args {
            // Scope to List<TraitObject> params only.  Bare TraitObject params
            // (e.g. `ConstitutiveLawInput.law : ConstitutiveLaw`) are skipped —
            // see fn doc-comment rationale.
            let is_list_trait_param = template
                .value_cells
                .iter()
                .find(|vc| vc.id.member == arg_name.as_str())
                .is_some_and(|vc| {
                    matches!(&vc.cell_type,
                        Type::List(inner) if matches!(inner.as_ref(), Type::TraitObject(_)))
                });
            if !is_list_trait_param {
                continue;
            }
            check_trait_arg_conformance(
                type_name,
                arg_name,
                compiled_arg,
                representative_span,
                template_registry,
                trait_registry,
                diagnostics,
            );
        }
    });
}

/// Enumerate every ROOT `CompiledExpr` of `template` exactly once, invoking `f`
/// with the expr and a representative `SourceSpan`.
///
/// "Root" means the top of a distinct expression tree; the recursive
/// `CompiledExpr::walk` inside [`check_expr_fn_arg_conformance`] descends into
/// nested calls, so callers must NOT pre-walk.
///
/// ## No double-count
///
/// Each field is visited once and the fields are mutually disjoint: geometry
/// `let`s are excluded from `value_cells` (`is_geometry_let` `continue` at
/// entity.rs:1175-1177) so realization args never overlap a value cell, and
/// guard / port / forall / sub members live in their own vecs, never in
/// `template.value_cells`.
///
/// ## Representative spans
///
/// Fields that carry their own span use it (value cells, constraints,
/// realizations via `realization.span`, sub-components via `sub.span`); fields
/// with no span of their own (objective, guard exprs, match-arm guards, port
/// `frame_expr`, forall bodies) use `SourceSpan::empty(0)`. The span is purely
/// diagnostic provenance — conformance correctness depends only on the
/// param/arg types, not the span.
///
/// ## Walked separately (not by this enumerator)
///
/// Associated-function bodies (`template.assoc_fns[*].function`) are walked
/// directly by [`phase_fn_arg_conformance`], mirroring its `ctx.functions`
/// free-function loop — NOT by this template-root enumerator. They are excluded
/// here so each expr tree is visited exactly once.
///
/// ## Residual (not walked at all)
///
/// `connections` carry only `ConstraintNodeId` references + string
/// `port_mappings` (no inline `CompiledExpr`), and `compiled_purposes` are
/// compiled after this pass (lib.rs:382). See `phase_fn_arg_conformance`
/// doc-comment and task-4081 design decision §6/§7.
fn for_each_template_root_expr(
    template: &TopologyTemplate,
    f: &mut impl FnMut(&CompiledExpr, SourceSpan),
) {
    // Value cells: param / let default expressions.
    for vc in &template.value_cells {
        if let Some(expr) = &vc.default_expr {
            f(expr, vc.span);
        }
    }

    // Constraints.
    for constraint in &template.constraints {
        f(&constraint.expr, constraint.span);
    }

    // Objective (the objective set carries no span of its own).
    if let Some(objective) = &template.objective {
        for term in &objective.terms {
            f(&term.expr, SourceSpan::empty(0));
        }
    }

    // Realizations: geometry-op argument expressions. The Boolean arm has no
    // inline `CompiledExpr` args (its operands are `GeomRef`s).
    for realization in &template.realizations {
        for op in &realization.operations {
            match op {
                CompiledGeometryOp::Primitive { args, .. }
                | CompiledGeometryOp::Modify { args, .. }
                | CompiledGeometryOp::Transform { args, .. }
                | CompiledGeometryOp::Pattern { args, .. }
                | CompiledGeometryOp::Sweep { args, .. }
                | CompiledGeometryOp::Curve { args, .. }
                | CompiledGeometryOp::Profile { args, .. } => {
                    for (_, arg) in args {
                        f(arg, realization.span);
                    }
                }
                CompiledGeometryOp::Boolean { .. } => {}
            }
        }
    }

    // Ports: member defaults, constraints, and optional frame expression.
    for port in &template.ports {
        for member in &port.members {
            if let Some(expr) = &member.default_expr {
                f(expr, member.span);
            }
        }
        for constraint in &port.constraints {
            f(&constraint.expr, constraint.span);
        }
        if let Some(frame_expr) = &port.frame_expr {
            f(frame_expr, SourceSpan::empty(0));
        }
    }

    // Guarded groups: guard expr + then/else members and constraints.
    for group in &template.guarded_groups {
        f(&group.guard_expr, SourceSpan::empty(0));
        for member in &group.members {
            if let Some(expr) = &member.default_expr {
                f(expr, member.span);
            }
        }
        for constraint in &group.constraints {
            f(&constraint.expr, constraint.span);
        }
        for member in &group.else_members {
            if let Some(expr) = &member.default_expr {
                f(expr, member.span);
            }
        }
        for constraint in &group.else_constraints {
            f(&constraint.expr, constraint.span);
        }
    }

    // Match-arm decl groups: per-arm guard expressions.
    for group in &template.match_arm_groups {
        for arm in &group.arms {
            f(&arm.guard_expr, SourceSpan::empty(0));
        }
    }

    // Sub-components: argument expressions and optional `at <pose>`.
    for sub in &template.sub_components {
        for (_, arg) in &sub.args {
            f(arg, sub.span);
        }
        if let Some(pose) = &sub.pose {
            f(pose, sub.span);
        }
    }

    // Captured forall templates: per-element constraint / connect body exprs.
    for forall in &template.forall_templates {
        match &forall.body {
            CompiledForallBody::Constraint { body_expr } => {
                f(body_expr, SourceSpan::empty(0));
            }
            CompiledForallBody::Connect { params, .. } => {
                for (_, expr) in params {
                    f(expr, SourceSpan::empty(0));
                }
            }
        }
    }
}
