pub(super) mod checker;
use checker::*;

pub(super) mod sub_component_validation;
pub(crate) use sub_component_validation::check_sub_structure_existence;

use super::*;
use crate::ambient_defaults::AmbientDefaults;
use crate::geometry_traits_inference::{
    GeometryTrait, InferredTraits, LetBindingEnv, infer_traits_for_expr_in_env, infer_traits_for_op,
};
use std::cell::RefCell;

/// Severity knob for struct-constructor field-conformance diagnostics
/// (task 5302, struct-ctor-conformance α).
///
/// α generalizes the 4584 struct-ctor conformance chokepoint from its original
/// 4-family allowlist (`List<TraitObject>` / `StructureRef` / `Vector` /
/// `Selector`) to ALL concrete field types, at **Warning** severity behind this
/// single const. The ctor field-conformance surface reads its severity from
/// here (threaded through `WalkCtx.severity`), so the δ follow-up is a literal
/// one-const flip to `Severity::Error` that promotes that surface uniformly.
///
/// **Two emit sites are deliberately outside this knob** and will NOT flip with
/// the const: (1) the fn-call conformance entry (`check_fn_arg_conformance`)
/// hard-codes `Severity::Error` (out of scope; see the `WalkCtx.severity` field
/// doc); (2) the geometry-trait leaf (`Bounded` / `Connected` / `Convex`, reached
/// via [`emit_geometry_unbounded`] / [`emit_geometry_trait_violation`]) hard-codes
/// `Severity::Error` because those codes (`GeometryUnbounded` and the geometry
/// `TypeNotConformingToTrait`) belong to the geometry-primitive-constructors PRD,
/// not to this ctor-field knob (see the `WalkCtx.severity` doc's carve-out note).
const CTOR_FIELD_CONFORMANCE_SEVERITY: Severity = Severity::Warning;

/// Build a `Diagnostic` at an explicit `severity`.
///
/// The `Diagnostic` builder exposes `error`/`warning`/`info` constructors but no
/// severity-parameterized one; the ctor-conformance walker needs the severity to
/// be read from `WalkCtx.severity` at every emit site so the single knob above
/// governs the whole surface. `Diagnostic.severity` is a public field, so this
/// sets it directly after building — keeping the change confined to this crate
/// (no reify-core change, per task 5302).
fn diag_at(severity: Severity, message: impl Into<String>) -> Diagnostic {
    let mut d = Diagnostic::error(message);
    d.severity = severity;
    d
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn check_trait_conformance(
    structure: &EntityDefRef<'_>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    scope: &mut CompilationScope,
    value_cells: &mut Vec<ValueCellDecl>,
    constraints: &mut Vec<CompiledConstraint>,
    constraint_index: &mut u32,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    // task 4497 (ambient-default-material B): the ambient-default table resolved
    // for this structure's scope. Top-level structures are fed the file-level
    // table and resolve at file scope (DD6 → `purpose = None`).
    ambient: &AmbientDefaults,
    // task 4639: enclosing purpose name for innermost-wins resolution. `None`
    // for top-level structures (file scope); `Some(name)` for structures
    // nested inside a purpose body.
    purpose: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
    // task 3939 δ: out-param receiving the resolved assoc-fn table, populated by
    // `check_phase_resolve_assoc_fns` (step-8). entity.rs stores it on the
    // conformer's TopologyTemplate (step-12).
    assoc_fns_out: &mut Vec<CompiledAssocFn>,
    // task 3972: out-param receiving the resolved assoc-type table, populated by
    // `check_phase_resolve_assoc_types`. entity.rs stores it on the conformer's
    // TopologyTemplate.
    assoc_types_out: &mut Vec<CompiledAssocType>,
) {
    // task 3973 ιγ: build the assoc-type scope BEFORE resolving structure
    // members so that `param x : AssocTypeName` annotations resolve correctly
    // in check_phase_resolve_structure_members.  We need:
    //   (a) the structure's own `type X = T` bindings (authoritative diagnostics
    //       — UnresolvedType for a bad RHS, TraitAssocTypeNotBound for unbound
    //       required types — are emitted here once, not duplicated downstream);
    //   (b) trait defaults for names the structure didn't bind;
    //   (c) declared_assoc_names for the anti-cascade poison sentinel.
    // This also moves the collect_structure_assoc_type_bindings call early so it
    // is not called twice (the result is reused below for phase 5).
    let structure_assoc_type_bindings = collect_structure_assoc_type_bindings(
        structure,
        alias_registry,
        structure_names,
        trait_names,
        diagnostics,
    );
    // Build assoc_type_scope = own bindings + trait defaults (own wins).
    let mut assoc_type_scope: HashMap<String, Type> = structure_assoc_type_bindings.clone();
    let mut declared_assoc_names: HashSet<String> = HashSet::new();
    for bound in structure.trait_bounds {
        if let Some(ct) = trait_registry.get(&bound.name) {
            for req in &ct.required_members {
                if matches!(req.kind, RequirementKind::AssocType(_)) {
                    declared_assoc_names.insert(req.name.clone());
                }
            }
            for def in &ct.defaults {
                if let DefaultKind::AssocType(ty) = &def.kind
                    && let Some(n) = &def.name
                {
                    declared_assoc_names.insert(n.clone());
                    assoc_type_scope
                        .entry(n.clone())
                        .or_insert_with(|| ty.clone());
                }
            }
        }
    }

    let (structure_param_members, structure_let_members, structure_constraint_labels) =
        check_phase_resolve_structure_members(
            structure,
            structure_names,
            trait_names,
            enum_defs,
            alias_registry,
            &assoc_type_scope,
            &declared_assoc_names,
            diagnostics,
        );

    // Build a combined name→type view for phases that only need to know whether the
    // structure declares a member by name (conflict-suppression, injection-skip, scope
    // pre-registration). Phases that must distinguish param from let members use
    // `structure_param_members` / `structure_let_members` directly (phase 5).
    //
    // If a name appears in both maps (no upstream diagnostic prevents it today),
    // the param entry wins: let-first chain means param entries overwrite let entries.
    let structure_all_members: HashMap<String, Type> = structure_let_members
        .iter()
        .chain(structure_param_members.iter())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Derive the structure's own assoc-fn signatures (task 3939 δ) so phase 5
    // can exact-match a provided `fn` override against its trait requirement.
    let structure_fn_sigs =
        collect_structure_assoc_fn_sigs(structure, alias_registry, structure_names, trait_names);

    // (structure_assoc_type_bindings was already collected above — before the
    // check_phase_resolve_structure_members call — so it is in scope for phase 5.)

    let mut ctx = check_phase_collect_trait_bounds(
        structure,
        trait_registry,
        &structure_all_members,
        diagnostics,
    );

    // task 4497: synthesize ambient-default Param entries into `ctx.defaults`
    // for unfilled `Param(StructureRef(T))` requirements, BEFORE the
    // pre-register / available-defaults / inject phases read `ctx.defaults` —
    // so an ambient default rides the trait-default rails exactly like a
    // trait-declared param default (DD2). Top-level structures resolve at file
    // scope (DD6 → `purpose = None`); purpose-nested structures pass
    // `Some(purpose_name)` for innermost-wins resolution (task 4639).
    check_phase_inject_ambient_defaults(&mut ctx, ambient, &structure_all_members, purpose);

    let pre = check_phase_pre_register_default_types(
        &ctx,
        &structure_all_members,
        structure.name,
        scope,
        enum_defs,
        functions,
        diagnostics,
    );

    let available_defaults = check_phase_build_available_defaults_map(
        &ctx,
        &pre.inferred_let_exprs,
        &pre.pass1_skipped,
        &pre.pass1_param_skipped,
        &pre.pass2_skipped,
        &pre.pass2_compile_errors,
    );

    check_phase_check_members_against_requirements(
        &ctx,
        structure,
        &structure_param_members,
        &structure_let_members,
        &available_defaults,
        &structure_fn_sigs,
        &structure_assoc_type_bindings,
        diagnostics,
    );

    // task 3941 ζ: conformer member-name set for the assoc-fn body
    // bare-member → `self.member` desugar (PRD §4.4). The structure's own
    // params/lets (`structure_all_members`) plus any param/let the conformer
    // gains purely via trait conformance — a trait requirement or trait default
    // it does not redeclare in its own AST — so a default-only trait param
    // referenced bare in an assoc-fn body still desugars to a `self` field.
    let mut conformer_member_names: HashSet<String> =
        structure_all_members.keys().cloned().collect();
    for req in &ctx.requirements {
        if matches!(
            req.kind,
            RequirementKind::Param(_) | RequirementKind::Let(_)
        ) {
            conformer_member_names.insert(req.name.clone());
        }
    }
    for def in &ctx.defaults {
        if matches!(
            def.kind,
            DefaultKind::Param { .. } | DefaultKind::Let { .. }
        ) && let Some(name) = &def.name
        {
            conformer_member_names.insert(name.clone());
        }
    }

    // task 3939 δ: resolve the override-or-injected-default assoc-fn table.
    // Runs after phase 5 (satisfaction checks) but is independent of default
    // injection — it compiles fn bodies into `assoc_fns_out`, it does not touch
    // value cells / constraints.
    check_phase_resolve_assoc_fns(
        &ctx,
        structure,
        &conformer_member_names,
        enum_defs,
        functions,
        alias_registry,
        structure_names,
        trait_names,
        &structure_fn_sigs,
        diagnostics,
        assoc_fns_out,
    );

    // task 3972: resolve the override-or-injected-default assoc-type table.
    // Runs after phase 5 (same ordering as assoc-fn resolve). Reuses the
    // structure_assoc_type_bindings map built by collect_structure_assoc_type_bindings
    // (which phase 5 already needed for its satisfaction check).
    check_phase_resolve_assoc_types(&ctx, &structure_assoc_type_bindings, assoc_types_out);

    check_phase_inject_defaults(
        &ctx,
        structure,
        &structure_all_members,
        &structure_constraint_labels,
        pre.inferred_let_exprs,
        &pre.pass1_skipped,
        &pre.pass1_param_skipped,
        &pre.pass2_skipped,
        &pre.pass2_compile_errors,
        scope,
        value_cells,
        constraints,
        constraint_index,
        enum_defs,
        functions,
        diagnostics,
    );
}

/// Verify that a compiled arg value's type conforms to the declared param type
/// in the target structure when the declared type is `Type::TraitObject(trait_name)`
/// or a wrapper thereof (`Type::Option(...)`, `Type::List(...)`, `Type::Set(...)`,
/// `Type::Map(...)`).
///
/// The compiled arg carries the full `CompiledExpr` so the recursive walker can
/// descend into `OptionSome` / `ListLiteral` / `ReflectiveCellList` / `SetLiteral` / `MapLiteral` nodes
/// and derive `arg_call_name` from any nested `FunctionCall` for the existing
/// `Real|Int → StructureRef` promotion.
///
/// Conformance strategy:
/// - `(Type::Option(p), OptionSome(a))` → recurse on inner
/// - `(Type::Option(_), OptionNone)` → OK (none is always valid for Option<T>)
/// - `(Type::List(p), ListLiteral(es) | ReflectiveCellList(es))` → recurse on each element
/// - `(Type::Set(p), SetLiteral(es))` → recurse on each element
/// - `(Type::Map(kp, vp), MapLiteral(entries))` → recurse on each (k, v)
/// - `(Type::TraitObject(req), _)` → leaf check (existing logic)
/// - Wrapped param + unmatched arg kind → fall through (no check)
///
/// Skips silently when:
/// - The target template is not found (external/unknown structure).
/// - The arg name is not found in the target's value cells (positional arg or error).
/// - The declared param type is not a trait object or wrapper thereof.
/// - The compiled_arg has `result_type == Type::Error` (anti-cascade).
///
/// Emits at most one diagnostic per leaf conformance failure.
pub(crate) fn check_trait_arg_conformance(
    target_name: &str,
    arg_name: &str,
    compiled_arg: &CompiledExpr,
    span: SourceSpan,
    registries: ConformanceRegistries<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Anti-cascade: if the arg itself had a compilation error, skip.
    if matches!(compiled_arg.result_type, Type::Error) {
        return;
    }

    // Look up the target template — skip if not found (external/forward-ref miss).
    let Some(target) = registries.templates.get(target_name) else {
        return;
    };

    // Find the declared param cell for this arg name.
    let Some(cell) = target
        .value_cells
        .iter()
        .find(|vc| vc.id.member == arg_name)
    else {
        return; // Arg name not found — skip (positional arg or existing error).
    };

    // Recursively walk the param type against the compiled arg.
    // The walker dispatches on (param_type, compiled_arg.kind) pairs for wrappers,
    // and falls through to the leaf helper for Type::TraitObject.
    let mut ctx = WalkCtx {
        arg_name,
        span,
        registries,
        diagnostics,
        // Ctor-conformance entry (value-cell + sub `=` paths): knob-governed
        // (Warning at α). task 5302.
        severity: CTOR_FIELD_CONFORMANCE_SEVERITY,
    };
    walk_param_against_arg(&cell.cell_type, compiled_arg, &mut ctx);
}

/// Verify that a compiled function-call argument conforms to a declared function
/// parameter type when that type is `Type::TraitObject(trait_name)` or a wrapper
/// thereof (`Type::Option(...)`, `Type::List(...)`, `Type::Set(...)`,
/// `Type::Map(...)`).
///
/// This is the function-call analogue of [`check_trait_arg_conformance`]:
/// whereas that entry point looks the callee up in the structure
/// `template_registry` and reads the param type from a `ValueCellDecl`, this
/// entry point receives the param type directly (from `CompiledFunction.params`)
/// and delegates to the same shared recursive walker `walk_param_against_arg` —
/// inheriting wrapper recursion, StructureRef bound-walking, TraitObject
/// refinement, geometry-trait handling, and Type::Error anti-cascade unchanged.
///
/// The `arg_name` is used as the diagnostic's param label, matching reify's
/// keyword-arg == param-name convention used by the structure path.
///
/// # Anti-cascade
///
/// If `compiled_arg.result_type == Type::Error` the call returns immediately
/// without emitting any diagnostic — the root-cause diagnostic was already
/// emitted by the expression that produced the error.
///
/// See task-4081 design decision §3 for rationale.
pub(crate) fn check_fn_arg_conformance(
    param_type: &Type,
    arg_name: &str,
    compiled_arg: &CompiledExpr,
    span: SourceSpan,
    registries: ConformanceRegistries<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Anti-cascade: if the arg itself had a compilation error, skip.
    if matches!(compiled_arg.result_type, Type::Error) {
        return;
    }

    // Recursively walk the param type against the compiled arg.
    let mut ctx = WalkCtx {
        arg_name,
        span,
        registries,
        diagnostics,
        // Fn-call trait-conformance is OUT OF SCOPE for the 5302 ctor knob and
        // must stay a hard error (preserves task-4081 fn-call semantics).
        severity: Severity::Error,
    };
    walk_param_against_arg(param_type, compiled_arg, &mut ctx);
}

/// Check that each `Param`-kind value cell with a default expression in
/// `template` has a default whose type is compatible with the declared
/// `cell_type`, for nominal leaf types (task-4584):
///
/// - **`Type::StructureRef`** params: applies an inline skip-list (see the arm
///   comment below for rationale — concretely, a `StructureRef` default for a
///   `StructureRef` param is intentionally not rejected here because nominal-name
///   mismatch does not imply incompatibility in Reify at eval time) and calls
///   [`emit_structure_ref_mismatch`] directly for clearly incompatible primitive
///   types (String, Int, …).  Does **not** delegate to `walk_param_against_arg`.
///   Skips if `default_expr.result_type` is `Type::Error` (anti-cascade).
///
/// - **`Type::Geometry`** params: uses a geometry-function-aware predicate rather
///   than `type_compatible` — geometry constructors compile to a scalar placeholder
///   (GHR-γ), so plain `type_compatible(Geometry, result_type)` would falsely reject
///   them.  Handled inline by the `Type::Geometry` arm below (no separate helper).
///
/// All other cell types (Real, Int, List, …) are out of scope for this pass.
pub(crate) fn check_param_default_conformance(
    template: &TopologyTemplate,
    registries: ConformanceRegistries<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for vc in &template.value_cells {
        if vc.kind != ValueCellKind::Param {
            continue;
        }
        let Some(default) = &vc.default_expr else {
            continue;
        };
        match &vc.cell_type {
            Type::StructureRef(_) => {
                // Anti-cascade: skip when the default expression itself had a compile error.
                if matches!(default.result_type, Type::Error) {
                    continue;
                }
                // Get the effective type, accounting for FunctionCall→StructureRef promotion
                // (e.g. `Steel_AISI_1045()` may carry a numeric fallback result_type but
                // its callee IS a known structure template → promote to StructureRef).
                let promoted =
                    promote_function_call_to_structure_ref(default, registries.templates);
                let effective_ty = promoted.as_ref().unwrap_or(&default.result_type);
                // Conservatively skip when the effective type is plausibly structure-
                // compatible at the eval level:
                //   • StructureRef(_) — a concrete type used as default for an abstract-like
                //     param (e.g. `Steel_AISI_1045` default for `Material`) is valid in Reify;
                //     nominal-name mismatch here does NOT imply incompatibility.
                //   • TraitObject(_) — may resolve to a conforming struct at evaluation.
                //   • TypeParam(_) — unresolved generic; conformance decided at instantiation.
                //   • Geometry — carries no nominal identity to verify here.
                // Only emit for clearly incompatible primitive types (String, Int, Scalar, …).
                if matches!(
                    effective_ty,
                    Type::StructureRef(_)
                        | Type::TraitObject(_)
                        | Type::TypeParam(_)
                        | Type::Geometry
                ) {
                    continue;
                }
                let mut ctx = WalkCtx {
                    arg_name: vc.id.member.as_str(),
                    span: vc.span,
                    registries,
                    diagnostics,
                    // Param-default conformance is a ctor-conformance entry:
                    // knob-governed (Warning at α). task 5302.
                    severity: CTOR_FIELD_CONFORMANCE_SEVERITY,
                };
                emit_structure_ref_mismatch(&vc.cell_type, effective_ty, &mut ctx);
            }
            Type::Geometry => {
                // Anti-cascade: skip when the default expression itself had an error or
                // is an unresolved type param — both are unverifiable.
                if matches!(default.result_type, Type::Error | Type::TypeParam(_)) {
                    continue;
                }
                // Geometry constructors (box/cylinder/...) compile to a
                // `Type::dimensionless_scalar()` PLACEHOLDER (GHR-γ), so
                // `type_compatible(Geometry, default.result_type)` would FALSELY reject
                // every legitimate geometry default.  Instead we detect geometry by:
                //   1. result_type is already Type::Geometry (a let-bound geometry ref), or
                //   2. the callee is a known geometry function (box/cylinder/sphere/...).
                // This is the same signal `check_leaf_trait_conformance` uses for args.
                let is_geometry_default = matches!(default.result_type, Type::Geometry)
                    || extract_function_call_name(default)
                        .map(is_geometry_function)
                        .unwrap_or(false);
                if !is_geometry_default {
                    // task 5302 α (D8): emit at the ctor-conformance knob severity
                    // (Warning at α), not a hard error — so ALL param-default
                    // diagnostics are governed by the single knob and the δ flip
                    // promotes them uniformly. Previously `Diagnostic::error`.
                    diagnostics.push(
                        diag_at(
                            CTOR_FIELD_CONFORMANCE_SEVERITY,
                            format!(
                                "param '{}' has type 'Geometry' but its default expression has non-geometry type '{}'",
                                vc.id.member, default.result_type
                            ),
                        )
                        .with_code(DiagnosticCode::TypeNotConformingToStructureRef)
                        .with_label(DiagnosticLabel::new(
                            vc.span,
                            format!(
                                "expected a geometry expression, got '{}'",
                                default.result_type
                            ),
                        )),
                    );
                }
            }
            // General concrete-leaf param defaults (task 5302 α, D8): every param
            // type beyond `StructureRef`/`Geometry` — `String`, `Int`, `Real`,
            // `Bool`, `Selector`, `Vector`, `Option<…>`, `List<…>`, `TraitObject`,
            // … — routes its compiled default through the SAME shared walker used
            // by the call-site ctor entries, at the knob severity. This closes the
            // param-default half of the "all concrete field types" generalization:
            // a `param label : String = 42` default now diagnoses identically to a
            // call-site `Widget(label: 42)` (same `ArgTypeMismatch`, same message).
            //
            // The old `_ => continue` silently accepted every non-StructureRef/
            // non-Geometry default (the "future work" hole this task fills). The
            // walker single-sources both the anti-cascade skip list and the
            // param-family gating: its `reject_if_incompatible` skips
            // `Error`/`TypeParam`/`Geometry`/`TraitObject` ARG types, and its
            // general concrete-leaf arm judges a PARAM type only when
            // [`general_leaf_param_family_is_validated`] vets it. So routing here
            // is unconditional but diagnosing is not — a default at an
            // un-vetted family stays silent, and no gating logic is duplicated
            // at this call site. Only the top-level `Error`-default guard is
            // repeated, matching the `StructureRef`/`Geometry` arms above.
            _ => {
                // Anti-cascade: skip when the default expression itself had a
                // compile error (its root-cause diagnostic was already emitted).
                if matches!(default.result_type, Type::Error) {
                    continue;
                }
                let mut ctx = WalkCtx {
                    arg_name: vc.id.member.as_str(),
                    span: vc.span,
                    registries,
                    diagnostics,
                    // Param-default conformance is a ctor-conformance entry:
                    // knob-governed (Warning at α). task 5302.
                    severity: CTOR_FIELD_CONFORMANCE_SEVERITY,
                };
                walk_param_against_arg(&vc.cell_type, default, &mut ctx);
            }
        }
    }
}

/// Context bundle threaded through the four recursive walker helpers.
///
/// Collects the five fields that were previously repeated as trailing arguments
/// in every recursive call to `walk_param_against_arg`, `walk_param_against_arg_type`,
/// `emit_leaf_conformance_for_arg_type`, and `check_leaf_trait_conformance`.  Bundling
/// them reduces each helper from 7 arguments to 2-3, and means adding a new wrapper
/// kind (e.g. `Tuple<T,U>`) only requires a one-line match arm rather than a full
/// 7-arg call duplication.
///
/// Recursive calls simply pass `ctx` — Rust auto-reborrows `&mut` correctly.
/// The three immutable lookup tables every ctor/fn conformance entry point needs,
/// bundled into one borrow-struct.
///
/// `templates`, `traits` and `enum_defs` were threaded as three separate
/// trailing arguments through six functions in this module plus two phase
/// functions in `entities_phase.rs`, which is what forced the
/// `#[allow(clippy::too_many_arguments)]` suppressions on
/// [`check_trait_arg_conformance`], [`check_fn_arg_conformance`] and
/// `check_expr_fn_calls`. [`WalkCtx`] already aggregated exactly this trio, so
/// the tuple was being unbundled at the entry points only to be re-bundled one
/// frame later.
///
/// Adding a fourth table is now a one-line change here rather than a ninth
/// parameter and a fourth suppression — which matters because
/// [`general_leaf_param_family_is_validated`]'s "still unexamined" list names
/// sixteen more `Type` variants, several of which will need their own lookup.
///
/// `Copy` because it is three shared references; callers pass it by value and
/// [`WalkCtx`] stores it by value, so no reborrow ceremony is needed at the
/// recursive call sites.
#[derive(Clone, Copy)]
pub(crate) struct ConformanceRegistries<'a> {
    /// Structure templates visible at the call site, keyed by name.
    pub templates: &'a HashMap<String, &'a TopologyTemplate>,
    /// Compiled traits visible at the call site, keyed by name.
    pub traits: &'a HashMap<String, &'a CompiledTrait>,
    /// Declared enums visible at the call site — `prelude ++ module-local`, i.e.
    /// exactly `CompilationCtx.resolution_enums` (task 5465, family 4).
    ///
    /// Exists SOLELY so the general concrete-leaf arm can tolerate D1/F-Mono
    /// enum erasure: a constructed variant's `result_type` is always the bare
    /// `Type::Enum(name)`, never the applied form, so a declared
    /// `Type::Applied { name: "Result", args: [...] }` param would spuriously
    /// fail raw [`type_compatible`], which has no Applied-vs-Enum arm in EITHER
    /// direction. The general concrete-leaf arm maps BOTH sides through
    /// [`base_enum_name`], which needs this table to recognise an applied ENUM
    /// (vs. an applied generic STRUCTURE like `Coupling<T>`, which is spelled
    /// the same way) while keeping a genuine cross-enum mismatch rejected.
    ///
    /// [`base_enum_name`] is the same oracle [`enum_payload_compatible`] uses at
    /// the payload-field site in `variant_construct.rs`, so the two decisions
    /// cannot drift.
    pub enum_defs: &'a [reify_ir::EnumDef],
}

struct WalkCtx<'a> {
    arg_name: &'a str,
    span: SourceSpan,
    /// The immutable lookup tables (templates / traits / enums) this walk reads.
    registries: ConformanceRegistries<'a>,
    diagnostics: &'a mut Vec<Diagnostic>,
    /// Severity at which conformance diagnostics emitted through this walk are
    /// built (task 5302). The two ctor-conformance entries
    /// (`check_trait_arg_conformance`, `check_param_default_conformance`) set
    /// this to [`CTOR_FIELD_CONFORMANCE_SEVERITY`] (Warning at α); the fn-call
    /// entry (`check_fn_arg_conformance`) sets it to `Severity::Error` so the
    /// out-of-scope fn-call trait-conformance semantics stay hard errors. Every
    /// *field-conformance* emit site (leaf-trait, StructureRef, Vector, selector,
    /// wrapper-shape, and the general concrete-leaf `ArgTypeMismatch`) builds its
    /// `Diagnostic` via [`diag_at`]`(ctx.severity, …)`, so severity is read from
    /// exactly one place per walk (C2(iv) severity-invariance); the δ follow-up
    /// flips only the const.
    ///
    /// **Carve-out — geometry-trait conformance stays always-Error.** The
    /// `Bounded` / `Connected` / `Convex` geometry-trait leaf (reached inside
    /// [`check_leaf_trait_conformance`] via [`emit_geometry_unbounded`] /
    /// [`emit_geometry_trait_violation`]) intentionally does NOT read
    /// `ctx.severity` — those two helpers hard-code `Severity::Error`. Their codes
    /// (`GeometryUnbounded` and the geometry `TypeNotConformingToTrait`) belong to
    /// the geometry-primitive-constructors PRD, not to this knob's ctor-field
    /// surface, and are therefore excluded from α's Warning downgrade set
    /// (Vector / Selector / StructureRef / wrapped-trait / wrapper-shape /
    /// general-leaf). Consequence: the δ one-const flip will NOT promote the
    /// geometry-trait ctor diagnostics; a δ author who wants them under the knob
    /// must route those two emit sites through `ctx.severity` explicitly (and
    /// update the `emit_geometry_*_helper_produces_error_*` unit tests +
    /// `geometry_traits_inference_tests`, which currently pin Error).
    severity: Severity,
}

/// Extract the callee function name from a compiled expression if it is a
/// `FunctionCall`. Returns `None` for any other expression kind.
///
/// Shared by `promote_function_call_to_structure_ref` (promotion check) and
/// `check_leaf_trait_conformance` (suppression guard) so both decisions use
/// the same pattern — any future extension of `FunctionCall` matching applies
/// consistently to both sites.
fn extract_function_call_name(arg: &CompiledExpr) -> Option<&str> {
    match &arg.kind {
        CompiledExprKind::FunctionCall { function, .. } => Some(function.name.as_str()),
        _ => None,
    }
}

/// Promote a `Real`/`Int`-typed `FunctionCall` whose callee is a known structure
/// template into a `Type::StructureRef(callee_name)`.
///
/// Returns `None` when the arg is not a numeric-typed FunctionCall, or when the
/// callee name is not registered in the template map (external/forward-ref miss).
///
/// Used by both `walk_param_against_arg`'s fallback arm and
/// `check_leaf_trait_conformance` so future wrapper kinds (e.g. `Tuple<T,U>`)
/// stay in sync — both call sites must promote identically to avoid drift in
/// diagnostic wording or trait-bound walks.
fn promote_function_call_to_structure_ref(
    arg: &CompiledExpr,
    templates: &HashMap<String, &TopologyTemplate>,
) -> Option<Type> {
    if !matches!(arg.result_type, Type::Scalar { .. } | Type::Int) {
        return None;
    }
    let name = extract_function_call_name(arg)?;
    if !templates.contains_key(name) {
        return None;
    }
    Some(Type::StructureRef(name.to_owned()))
}

/// Recursive dispatcher: walk `param_type` lockstep against `compiled_arg`,
/// recursing into Option/List/Set/Map wrapper pairs and delegating `TraitObject`
/// to the leaf helper.
fn walk_param_against_arg(param_type: &Type, compiled_arg: &CompiledExpr, ctx: &mut WalkCtx<'_>) {
    match (param_type, &compiled_arg.kind) {
        // Option wrapper: recurse into the inner value.
        (Type::Option(inner_p), CompiledExprKind::OptionSome(inner_a)) => {
            walk_param_against_arg(inner_p, inner_a, ctx);
        }
        // none is always valid for any Option<T> param.
        (Type::Option(_), CompiledExprKind::OptionNone) => {}
        // List wrapper: recurse on each element. Covers both user-written
        // ListLiteral (compile-time) and ReflectiveCellList (post-activation,
        // task-2458) — RCL behaves identically to ListLiteral at this layer
        // and is included so future refactors that move activation earlier
        // do not silently fall through to the type-level fallback (task-2543).
        (
            Type::List(inner_p),
            CompiledExprKind::ListLiteral(elements)
            | CompiledExprKind::ReflectiveCellList(elements),
        ) => {
            for elem in elements {
                walk_param_against_arg(inner_p, elem, ctx);
            }
        }
        // Set wrapper: recurse on each element.
        (Type::Set(inner_p), CompiledExprKind::SetLiteral(elements)) => {
            for elem in elements {
                walk_param_against_arg(inner_p, elem, ctx);
            }
        }
        // Map wrapper: recurse on both key and value positions independently.
        (Type::Map(key_p, val_p), CompiledExprKind::MapLiteral(entries)) => {
            for (key_expr, val_expr) in entries {
                walk_param_against_arg(key_p, key_expr, ctx);
                walk_param_against_arg(val_p, val_expr, ctx);
            }
        }
        // Leaf: param type is a trait object — call the existing conformance logic.
        (Type::TraitObject(required_trait), _) => {
            check_leaf_trait_conformance(required_trait, compiled_arg, ctx);
        }
        // Fallback: for non-literal wrapped args (e.g. ValueRef), attempt a type-level
        // wrapper walk comparing param_type against compiled_arg.result_type. This handles
        // cases like passing `param p : Option<Physical>` to a slot of `Option<Material>`
        // where `Physical : Material`. Non-wrapper and non-trait params fall through
        // silently inside walk_param_against_arg_type.
        //
        // Apply StructureRef promotion when the compiled arg is a FunctionCall whose
        // result_type defaulted to Real/Int (the expression compiler's numeric fallback
        // for structure calls) and the callee is a known structure template. Without this,
        // wrapper-shape diagnostics would show the misleading numeric fallback type (e.g.
        // 'Real') instead of the structure name (e.g. 'Steel') for cases like
        // `Host(m: Steel())` where `m : Option<MaterialSpec>`.
        _ => {
            let promoted =
                promote_function_call_to_structure_ref(compiled_arg, ctx.registries.templates);
            let effective_type = promoted.as_ref().unwrap_or(&compiled_arg.result_type);
            walk_param_against_arg_type(param_type, effective_type, ctx);
        }
    }
}

/// Emit `DiagnosticCode::GeometryUnbounded` for a geometry-typed argument
/// at a `param g : Bounded`-shaped call site whose inferred trait set lacks
/// `bounded`.
///
/// Pushes exactly one `Diagnostic::error(...)` with code
/// [`DiagnosticCode::GeometryUnbounded`] and a single label at `span`. The
/// canonical message wording is documented on the variant declaration in
/// `crates/reify-types/src/diagnostics.rs` — keep the two in sync.
///
/// **Always Error — carve-out from the task 5302 ctor-field severity knob.**
/// This helper is reached on the ctor walk path but intentionally hard-codes
/// `Severity::Error` rather than reading `WalkCtx.severity`, so the δ one-const
/// flip does NOT govern it (see the `CTOR_FIELD_CONFORMANCE_SEVERITY` /
/// `WalkCtx.severity` carve-out docs). `emit_geometry_unbounded_helper_produces_error_*`
/// pins this.
///
/// Reserved for the **Bounded** case only. `Connected`/`Convex` violations
/// at the same call-site shape reuse [`DiagnosticCode::TypeNotConformingToTrait`]
/// per the task's design decision §2 (the PRD only allocates
/// `E_GEOMETRY_UNBOUNDED` for missing Bounded).
pub(crate) fn emit_geometry_unbounded(
    arg_name: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        Diagnostic::error(format!(
            "geometry argument '{}' is not Bounded; required by trait parameter",
            arg_name
        ))
        .with_code(DiagnosticCode::GeometryUnbounded)
        .with_label(DiagnosticLabel::new(
            span,
            format!("geometry argument '{}' is not Bounded", arg_name),
        )),
    );
}

/// Emit `DiagnosticCode::GeometryProfileRequired` for a geometry-typed argument
/// at a profile-consuming op (`extrude`/`extrude_symmetric`/`revolve`/`loft`/
/// `loft_guided`/`sweep`/`sweep_guided`/`pipe`) whose statically-known operand
/// violates the op's dimensionality precondition.
///
/// Pushes exactly one `Diagnostic::error(...)` with code
/// [`DiagnosticCode::GeometryProfileRequired`] and a single label at `span`. The
/// `requirement` is parameterized so the one helper serves both the Surface
/// profile consumers (`requirement` e.g. `"a 2D Surface profile (Closed, Planar)"`)
/// and the Curve path consumers (`requirement` e.g. `"a 1D Curve path"`). The
/// canonical message wording is documented on the variant declaration in
/// `crates/reify-core/src/diagnostics.rs` — keep the two in sync.
///
/// `arg_name` is the **consumer's parameter-slot label** (e.g. `"profile"` or
/// `"path"`), shown to the user as the offending argument's name — callers pass
/// the slot role, NOT the operand's own producer function name, so the message
/// points at the role that is wrong (`geometry argument 'profile' must be …`).
/// Callers should pass `span` as the offending operand's own span so the label
/// pinpoints that argument.
///
/// Sibling of [`emit_geometry_unbounded`]: same shape (one Error diagnostic, one
/// label at `span`) and the same non-fatal contract — the caller still lowers
/// the op. See PRD `docs/prds/geometry-primitive-constructors.md` task α.
pub(crate) fn emit_geometry_profile_required(
    arg_name: &str,
    requirement: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        Diagnostic::error(format!(
            "geometry argument '{}' must be {}",
            arg_name, requirement
        ))
        .with_code(DiagnosticCode::GeometryProfileRequired)
        .with_label(DiagnosticLabel::new(
            span,
            format!("'{}' is not {}", arg_name, requirement),
        )),
    );
}

/// Emit a "does not conform to trait" geometry diagnostic for the `Connected`/`Convex`
/// cases — the symmetric sibling of [`emit_geometry_unbounded`] for the `Bounded` case.
///
/// Pushes exactly one `Diagnostic::error(...)` with code
/// [`DiagnosticCode::TypeNotConformingToTrait`] and a single label at `span`. The PRD
/// only allocates `E_GEOMETRY_UNBOUNDED` for missing `Bounded`; `Connected`/`Convex`
/// reuse `TypeNotConformingToTrait`.
///
/// **Always Error — carve-out from the task 5302 ctor-field severity knob**, for the
/// same reason as its sibling [`emit_geometry_unbounded`]: geometry-trait conformance
/// is not part of α's Warning downgrade set, so this hard-codes `Severity::Error` and
/// is not flipped by the δ const. `emit_geometry_trait_violation_helper_produces_error_*`
/// pins this.
///
/// The message intentionally does **not** include a separate param-name slot. Under
/// reify's keyword-arg convention the arg name and param name are identical in practice,
/// so appending `required by param '{}'` a second time is redundant and was dropped.
pub(crate) fn emit_geometry_trait_violation(
    arg_name: &str,
    required_trait: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        Diagnostic::error(format!(
            "geometry argument '{}' does not conform to trait '{}'",
            arg_name, required_trait
        ))
        .with_code(DiagnosticCode::TypeNotConformingToTrait)
        .with_label(DiagnosticLabel::new(
            span,
            format!("geometry argument '{}' is not {}", arg_name, required_trait),
        )),
    );
}

/// Shared leaf helper: emit a "does not conform to trait" diagnostic if `arg_type`
/// does not satisfy `required_trait`.
///
/// Handles `Type::StructureRef` (walks trait bounds transitively via
/// `satisfies_trait_bound`) and `Type::TraitObject` (equality-or-refinement via
/// `trait_satisfies`). Other types are skipped silently — callers add their own
/// fallback arm for non-struct/non-trait types.
///
/// This is the single source of the StructureRef/TraitObject diagnostic wording and
/// `DiagnosticCode::TypeNotConformingToTrait` code shared by both the literal walker
/// (`check_leaf_trait_conformance`) and the type-level fallback walker
/// (`walk_param_against_arg_type`).
fn emit_leaf_conformance_for_arg_type(
    arg_type: &Type,
    required_trait: &str,
    ctx: &mut WalkCtx<'_>,
) {
    match arg_type {
        Type::StructureRef(struct_name) => {
            let Some(arg_template) = ctx.registries.templates.get(struct_name.as_str()) else {
                return;
            };
            if !satisfies_trait_bound(
                &arg_template.trait_bounds,
                required_trait,
                ctx.registries.traits,
            ) {
                ctx.diagnostics.push(
                    diag_at(
                        ctx.severity,
                        format!(
                            "type '{}' does not conform to trait '{}' required by param '{}'",
                            struct_name, required_trait, ctx.arg_name
                        ),
                    )
                    .with_code(DiagnosticCode::TypeNotConformingToTrait)
                    .with_label(DiagnosticLabel::new(
                        ctx.span,
                        format!(
                            "type '{}' does not conform to trait '{}'",
                            struct_name, required_trait
                        ),
                    )),
                );
            }
        }
        Type::TraitObject(arg_trait_name) => {
            let mut visited = HashSet::new();
            if !trait_satisfies(
                arg_trait_name,
                required_trait,
                ctx.registries.traits,
                &mut visited,
            ) {
                ctx.diagnostics.push(
                    diag_at(
                        ctx.severity,
                        format!(
                            "type '{}' does not conform to trait '{}' required by param '{}'",
                            arg_trait_name, required_trait, ctx.arg_name
                        ),
                    )
                    .with_code(DiagnosticCode::TypeNotConformingToTrait)
                    .with_label(DiagnosticLabel::new(
                        ctx.span,
                        format!(
                            "trait '{}' does not refine trait '{}'",
                            arg_trait_name, required_trait
                        ),
                    )),
                );
            }
        }
        // Non-struct, non-trait arg type — callers handle this with their own fallback arm.
        _ => {}
    }
}

/// Emit a single `Diagnostic::error` with
/// [`DiagnosticCode::TypeNotConformingToStructureRef`] when an arg type does not
/// match a `Type::StructureRef` param (task-4584).
///
/// Modelled on [`emit_leaf_conformance_for_arg_type`]: one diagnostic, one label
/// at `ctx.span`, message names the required structure and the offending type.
fn emit_structure_ref_mismatch(param_type: &Type, arg_type: &Type, ctx: &mut WalkCtx<'_>) {
    ctx.diagnostics.push(
        diag_at(
            ctx.severity,
            format!(
                "argument '{}' has type '{}' but param '{}' requires structure type '{}'",
                ctx.arg_name, arg_type, ctx.arg_name, param_type,
            ),
        )
        .with_code(DiagnosticCode::TypeNotConformingToStructureRef)
        .with_label(DiagnosticLabel::new(
            ctx.span,
            format!("expected '{}', got '{}'", param_type, arg_type),
        )),
    );
}

/// Emit a single `Diagnostic::error` with
/// [`DiagnosticCode::TypeNotConformingToVector`] when an arg does not conform to a
/// `Type::Vector`-typed param (task-4622): either the arg is not vector-shaped, or it is
/// `Type::Vector` with a mismatched arity (n).
///
/// Conformance rules:
/// - A `Type::Vector { n, .. }` arg is accepted only when `n` matches the param's `n`.
///   Arity mismatch (e.g. vec2 passed to a vec3 param) is a genuine type error.
/// - A `Type::Tensor { rank: 1, .. }` arg is accepted regardless of element count
///   (Tensor arity is not yet pinned in param signatures, so accepted conservatively).
/// - The quantity slot is intentionally loose (ty.rs Point/Vector quantity-slot convention).
/// - Bare scalars, strings, bools, and other non-vector kinds are rejected.
///
/// Modelled on [`emit_structure_ref_mismatch`]: one diagnostic, one label at `ctx.span`,
/// message names the required vector type and the offending arg type.
fn emit_vector_mismatch(param_type: &Type, arg_type: &Type, ctx: &mut WalkCtx<'_>) {
    ctx.diagnostics.push(
        diag_at(
            ctx.severity,
            format!(
                "argument '{}' has type '{}' but param '{}' requires vector type '{}'",
                ctx.arg_name, arg_type, ctx.arg_name, param_type,
            ),
        )
        .with_code(DiagnosticCode::TypeNotConformingToVector)
        .with_label(DiagnosticLabel::new(
            ctx.span,
            format!("expected '{}', got '{}'", param_type, arg_type),
        )),
    );
}

/// Emit a single diagnostic when an arg type does not match a `Type::Selector` or
/// `Type::AnySelector` param (task-4598), branching the code on the *nature* of the
/// mismatch (task 5302 α, D2):
///
/// - **Selector-kind mismatch** — a concrete `Selector(j)` arg with `j != k`, or a
///   bare `AnySelector` arg, against a single-kind `Selector(k)` param — carries
///   [`DiagnosticCode::SelectorKindMismatch`] and names the expected/found selector
///   kinds (the `SelectorKind` Display strings, e.g. `FaceSelector`/`EdgeSelector`).
///   This mirrors the fn-call param-binding path's `SelectorKindMismatch` tagging
///   (task 4581 / 4371 BT1) so the same wrong-kind error is coded uniformly in both
///   the ctor and fn-call surfaces.
/// - **Pose-vs-set** — a coordinate-pose arg (`Frame`/`Transform`/`Point`) against
///   a selector-typed param — keeps [`DiagnosticCode::ArgTypeMismatch`] but appends
///   the fixed hint substring `a coordinate pose is not a region target; select a
///   face/edge/vertex instead` (PRD §4 D2; task 4833's fixtures assert on it). A
///   message variant, not a new failure class.
/// - **Every other selector-slot mismatch** — a non-selector arg (`String`, `Int`,
///   …), or any arg against a bare `AnySelector` param — keeps
///   [`DiagnosticCode::ArgTypeMismatch`] with the plain message (the disallow-string
///   over-tag guard per 4581).
///
/// Modelled on [`emit_structure_ref_mismatch`] / [`emit_vector_mismatch`]: one
/// diagnostic, one label at `ctx.span`. No new code minted (both already exist in
/// diagnostics.rs). Severity is `ctx.severity` in BOTH branches (severity-invariant:
/// the 5302 knob governs the whole selector surface uniformly — see design decision
/// Q3/Option-A). The same-kind case (`Selector(k)` arg vs `Selector(k)` param) never
/// reaches here — `reject_if_incompatible`'s `type_compatible` gate accepts it — so
/// the `j != k` guard is defensive, not load-bearing.
fn emit_selector_mismatch(param_type: &Type, arg_type: &Type, ctx: &mut WalkCtx<'_>) {
    let is_kind_mismatch = match (param_type, arg_type) {
        (Type::Selector(k), Type::Selector(j)) => j != k,
        (Type::Selector(_), Type::AnySelector) => true,
        _ => false,
    };
    if is_kind_mismatch {
        ctx.diagnostics.push(
            diag_at(
                ctx.severity,
                format!(
                    "argument '{}' has selector kind '{}' but param '{}' requires selector kind '{}'",
                    ctx.arg_name, arg_type, ctx.arg_name, param_type,
                ),
            )
            .with_code(DiagnosticCode::SelectorKindMismatch)
            .with_label(DiagnosticLabel::new(
                ctx.span,
                format!("expected '{}', got '{}'", param_type, arg_type),
            )),
        );
    } else {
        // Pose-vs-set (task 5302 α, PRD §4 D2): a coordinate pose — `Frame(n)`,
        // `Transform(n)`, or `Point { .. }` — passed to a selector-typed field is
        // a common authoring confusion (a pose locates a datum; a selector names a
        // region target). It stays `ArgTypeMismatch` (a message variant, NOT a new
        // failure class / code) but carries a fixed hint substring — the
        // deterministic string task 4833's pose-vs-set fixtures assert on
        // verbatim. Every non-pose non-selector arg (`String`, `Int`, …) keeps the
        // plain message (disallow-string over-tag guard, 4581).
        let is_pose = matches!(
            arg_type,
            Type::Frame(_) | Type::Transform(_) | Type::Point { .. }
        );
        let mut message = format!(
            "argument '{}' has type '{}' but param '{}' requires selector type '{}'",
            ctx.arg_name, arg_type, ctx.arg_name, param_type,
        );
        if is_pose {
            message.push_str(
                "; a coordinate pose is not a region target; select a face/edge/vertex instead",
            );
        }
        ctx.diagnostics.push(
            diag_at(ctx.severity, message)
                .with_code(DiagnosticCode::ArgTypeMismatch)
                .with_label(DiagnosticLabel::new(
                    ctx.span,
                    format!("expected '{}', got '{}'", param_type, arg_type),
                )),
        );
    }
}

/// Skip-guard + `type_compatible` gate shared by the `StructureRef` and `Selector/AnySelector`
/// leaf arms in [`walk_param_against_arg_type`] (task-4584, task-4598).
///
/// Returns without emitting when `arg_ty` is one of the conservatively-skipped kinds:
/// - `Error` — anti-cascade poison sentinel.
/// - `TypeParam` — unresolved generic; conformance decided at instantiation.
/// - `Geometry` — carries no nominal identity verifiable at the type level.
/// - `TraitObject` — may resolve to a conforming type; defer.
///
/// For all other concrete arg types, calls `type_compatible`; if that returns false,
/// delegates to `emit` to push the mismatch diagnostic. Encapsulating the three-line
/// body prevents the anti-cascade skip list from drifting between the two arms.
///
/// Emit a single `DiagnosticCode::ArgTypeMismatch` diagnostic (at `ctx.severity`)
/// for a general concrete-leaf param/arg type mismatch (task 5302 α — the fully
/// general arg-shape pass that replaces the old silent `_` fall-through in
/// [`walk_param_against_arg_type`]).
///
/// Message shape mirrors the 4598 selector wording (`emit_selector_mismatch`):
/// `"argument 'X' has type 'A' but param 'X' requires type 'B'"`. Reused for any
/// non-wrapper / non-trait / non-selector / non-vector / non-structure leaf whose
/// arg type is not `type_compatible` with the declared param type (String←Int,
/// Bool←String, dimensioned-scalar mismatch, …). No new diagnostic code is minted
/// — `ArgTypeMismatch` already exists.
fn emit_arg_type_mismatch(param_type: &Type, arg_ty: &Type, ctx: &mut WalkCtx<'_>) {
    ctx.diagnostics.push(
        diag_at(
            ctx.severity,
            format!(
                "argument '{}' has type '{}' but param '{}' requires type '{}'",
                ctx.arg_name, arg_ty, ctx.arg_name, param_type,
            ),
        )
        .with_code(DiagnosticCode::ArgTypeMismatch)
        .with_label(DiagnosticLabel::new(
            ctx.span,
            format!("expected '{}', got '{}'", param_type, arg_ty),
        )),
    );
}

/// The arg types that every leaf gate in this walker conservatively SKIPS rather
/// than judges: each is "unknown / unverifiable here", NOT "definitely
/// non-conforming", so emitting on one would be a false positive.
///
/// * `Error` — anti-cascade poison sentinel; the root-cause diagnostic already fired.
/// * `TypeParam` — an unresolved generic type variable; real conformance is
///   decided when the var is bound at instantiation.
/// * `Geometry` — carries no nominal identity verifiable at the type level
///   (geometry conformance is decided from the compiled op-array, reachable only
///   through the literal walker).
/// * `TraitObject` — may resolve to a conforming type at evaluation; defer.
///
/// # Why a named helper rather than a repeated `matches!`
///
/// [`reject_if_incompatible`] and the four SHAPE-BASED leaf arms (`Vector`,
/// `Point`, `Matrix`/`Tensor`, `Field`) all need exactly this set. Before task
/// 5465's amendment pass each spelled it out inline and the arm comments claimed
/// the copies were "character-for-character" identical so they "cannot drift" —
/// a claim enforced only by comment. Routing every caller through one function
/// makes it structurally true.
///
/// # `Type::ScalarParam` is deliberately NOT in this set
///
/// `Type::ScalarParam(Q)` is the unresolved *dimension*-parameter placeholder
/// (`type_resolution.rs:3134`), which superficially looks like a sibling of
/// `TypeParam`. It is not, for this purpose: with `TypeParam` the whole type is
/// unknown, whereas with `ScalarParam` the FAMILY is known (it is a scalar) and
/// only its dimension is open. Adding it here would silence genuine family-level
/// mismatches such as `String ← Scalar<Q>` at every arm at once. Instead, the
/// arms that already tolerate a scalar-family arg as the expression compiler's
/// numeric-fallback placeholder (`Point`, `Matrix`/`Tensor`) accept it through
/// [`is_numeric_placeholder_leaf`], which is where its scalar-ness — not its
/// unknown-ness — is the load-bearing property.
fn arg_type_is_unverifiable(arg_ty: &Type) -> bool {
    matches!(
        arg_ty,
        Type::Error | Type::TypeParam(_) | Type::Geometry | Type::TraitObject(_)
    )
}

/// The NARROW numeric-leaf predicate the `Point` and `Matrix`/`Tensor` shape
/// arms use to accept the expression compiler's numeric-fallback placeholder
/// (task 5465).
///
/// `point3(…)` and friends are stdlib eval-builtins with no `.ri` return type,
/// so their calls compile to a `FunctionCall` typed `Scalar[m]` / `Int` rather
/// than `Type::Point`. `Type::ScalarParam(_)` is the same shape with an
/// unresolved dimension (see [`arg_type_is_unverifiable`]'s closing note).
///
/// Deliberately NOT `type_compat.rs::is_scalar_like_leaf`, which also admits
/// `Bool`, `String`, `Enum`, `StructureRef`, `TraitObject` and `Geometry` — that
/// would make `Anchor(origin: "origin")` silent and defeat the value floors.
fn is_numeric_placeholder_leaf(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Scalar { .. } | Type::ScalarParam(_))
}

/// The CONCRETE dimension named by a `Vector`/`Point`/`Matrix`/`Tensor` quantity
/// slot, or `None` when that slot does not name one (task 5766).
///
/// `None` is returned — and the quantity rule therefore stays SILENT — for a
/// dimensionless `Scalar`, `Type::Int`, `Type::ScalarParam(_)`, and anything
/// else. Only a `Type::Scalar` carrying a non-dimensionless [`DimensionVector`]
/// yields `Some`.
///
/// # Why the rule is dimensionless-TOLERANT and not strict equality
///
/// Strict `DimensionVector` equality — the rule the bare `Scalar` leaf uses — is
/// not available here, because the ARG side of these arms is systematically
/// ERASED:
///
///   * `point3(…)` and friends are eval-builtins with no `.ri` return type, so a
///     real `Point` arg arrives as a `Scalar[m]` / `Int` placeholder through
///     [`is_numeric_placeholder_leaf`] — no quantity slot at all.
///   * `MassProperties.inertia : Matrix<3,3,MomentOfInertia>` is spelled
///     `[[…], […], […]]` at every corpus site, which compiles to
///     `List<List<Real>>` — again no quantity slot (see
///     [`list_bottoms_out_numeric`]).
///   * `Value::Vector::infer_type()` may yield a dimensionless or `Int`
///     quantity, so `vec3(0, 1, 0)` at a `Vec3<Length>` param is the IDIOMATIC
///     corpus spelling (`examples/dynamics/pendulum_idyn.ri:32`).
///
/// Comparing a declaration against one of those holes is exactly the defect the
/// `Type::Field` arm's comment records as having produced six false warnings in
/// `examples/fea_shell_channels.ri`. `vector_param_accepts_dimensionless_vector_arg`
/// and `point_param_accepts_dimensionless_point_arg` pin the dimensionless leg as
/// INTENTIONAL rather than accidental.
///
/// The normative statement of the resulting rule — including the residual it
/// knowingly leaves and why `Type::Field` is held loose — is the
/// "Point / Vector quantity-slot convention" section of
/// `crates/reify-core/src/ty.rs`. This function is its implementation, not a
/// second source of truth.
fn quantity_slot_dimension(quantity: &Type) -> Option<DimensionVector> {
    match quantity {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Some(*dimension),
        _ => None,
    }
}

/// Whether a param's and an arg's quantity slots CONFLICT: true only when both
/// name a concrete dimension and those dimensions differ (task 5766).
///
/// The comparison is the derived `PartialEq` on [`DimensionVector`], i.e. the
/// same strict-equality primitive the bare-`Scalar` leaf rule uses — so the two
/// rules provably agree on what "the same dimension" means.
///
/// # The [`is_numeric_placeholder_leaf`] unknown-ness fence is preserved BY
/// CONSTRUCTION, not by care
///
/// Every caller applies this predicate (a) only AFTER its arm's existing
/// family/arity check has passed, and (b) only to args that actually carry a
/// quantity slot. Both halves of the fence therefore survive mechanically:
///
///   * A `ScalarParam(_)` / `Int` / `Scalar` arg has NO quantity slot, so it
///     never reaches this comparison and is still accepted through the untouched
///     [`is_numeric_placeholder_leaf`] branch.
///   * A `String` arg is rejected by the family check BEFORE this comparison, so
///     it is still rejected — and keeps its arm's own diagnostic code.
///
/// [`arg_type_is_unverifiable`] is deliberately NOT widened to carry this rule
/// (PRD 4's D4-5): doing so would silence genuine family-level mismatches such
/// as `String ← Scalar<Q>` at every arm at once. Because
/// [`quantity_slot_dimension`] also returns `None` for a `ScalarParam` or `Int`
/// on the PARAM side, a dimension-generic wrapper such as `Vec3<Q: Dimension>`
/// (`stdlib/trajectory.ri:103`) is tolerated in BOTH directions.
fn quantity_slots_conflict(param_quantity: &Type, arg_quantity: &Type) -> bool {
    matches!(
        (
            quantity_slot_dimension(param_quantity),
            quantity_slot_dimension(arg_quantity),
        ),
        (Some(param_dim), Some(arg_dim)) if param_dim != arg_dim
    )
}

/// Whether a `Type::List` arg at a `Matrix`/`Tensor` param is plausibly the
/// idiomatic nested-list-literal spelling of that matrix — i.e. it bottoms out
/// in a NUMERIC (or already tensor-shaped) element type.
///
/// The `Matrix`/`Tensor` arm accepts `Type::List` because
/// `[[0.0, …], [0.0, …], [0.0, …]]` is how every `examples/dynamics/*_idyn.ri`
/// spells a `MassProperties.inertia`, and that literal compiles to
/// `List<List<Real>>`. But "is a list" is not the same claim as "is a matrix
/// literal": an UNCONSTRAINED `Type::List(_)` accept also swallows
/// `Body(inertia: ["a", "b"])` (arg type `List<String>`), because a
/// `Matrix`-typed param never pairs with the literal walker's
/// `(Type::List(param), ListLiteral)` arm and so arrives here as a bare type.
/// That left the just-promoted family with a hole on exactly the shape the
/// accept was written for (reviewer_comprehensive, correctness-coverage-gap).
///
/// Peeling `List` recursively (rather than checking one level) keeps the rule
/// rank-agnostic: `List<Real>` for a rank-1 tensor, `List<List<Real>>` for a
/// 3×3 matrix, `List<List<List<Real>>>` for a rank-3 tensor. ARITY is still not
/// checked — see the arm comment for why that asymmetry with `Point`/`Vector` is
/// deliberate — only the element FAMILY is.
///
/// `Error` and `TypeParam` bottoms return true: those elements are unknown, not
/// definitely non-numeric, and rejecting them would violate the same
/// anti-cascade / defer-to-instantiation contract [`arg_type_is_unverifiable`]
/// encodes at the top level. `Geometry` / `TraitObject` are NOT included: a
/// `List<Geometry>` at a matrix slot is a genuine family mismatch, and neither
/// is a numeric-literal shape.
fn list_bottoms_out_numeric(ty: &Type) -> bool {
    match ty {
        Type::List(inner) => list_bottoms_out_numeric(inner),
        Type::Vector { .. } | Type::Matrix { .. } | Type::Tensor { .. } => true,
        Type::Error | Type::TypeParam(_) => true,
        other => is_numeric_placeholder_leaf(other),
    }
}

/// Vector's bespoke arity logic (`emit_vector_mismatch`) is intentionally separate.
fn reject_if_incompatible<F>(param_type: &Type, arg_ty: &Type, ctx: &mut WalkCtx<'_>, emit: F)
where
    F: FnOnce(&Type, &Type, &mut WalkCtx<'_>),
{
    if !arg_type_is_unverifiable(arg_ty) && !type_compatible(param_type, arg_ty) {
        emit(param_type, arg_ty, ctx);
    }
}

/// Type-level fallback walker: compare `param_type` against `arg_type` wrapper-by-wrapper.
///
/// Used when the compiled arg is not a literal (e.g. a `ValueRef`), so its inner
/// expressions are not available for inspection. Walks the wrapper structure of
/// `arg_type` in lockstep with `param_type`. At a `Type::TraitObject` leaf,
/// `StructureRef`/`TraitObject` args defer to `emit_leaf_conformance_for_arg_type`;
/// `Geometry`/`Error`/`TypeParam` args are skipped (unverifiable at the type level /
/// anti-cascade / unresolved generic); any other leaf type (scalar, enum, point, …)
/// cannot carry trait bounds and emits a
/// `TypeNotConformingToTrait` directly — mirroring the literal walker's fallback so a
/// wrapper-nested trait param reached with a non-literal scalar arg is not silently
/// accepted (task-4081 soundness, esc-4081-174).
///
/// Wrapper-shape mismatches (e.g. `Option<T>` param vs `List<T>` arg, or bare leaf arg
/// vs wrapper param) emit a `TypeNotConformingToTrait` diagnostic when `param_type` is
/// `Option/List/Set/Map`. A top-level `Type::Error` in either `param_type` or `arg_type`
/// short-circuits the whole walk via an early return, so no diagnostic — wrapper-shape,
/// leaf-conformance, or future-arm — is emitted on top of an already-reported upstream
/// error. The guard runs before the match, making the anti-cascade contract uniform
/// across all arms (current and future). Note: this guard is top-level only — an `Error`
/// nested inside a wrapper (e.g. `Option<Error>`) is not detected and may produce a
/// secondary wrapper-shape diagnostic on top of the root-cause error. Non-wrapper,
/// non-trait param types (e.g. `Real`, `Int`) fall through silently — a fully general
/// arg-shape pass is tracked as future work.
fn walk_param_against_arg_type(param_type: &Type, arg_type: &Type, ctx: &mut WalkCtx<'_>) {
    // Anti-cascade: skip when either type carries the poison sentinel so no
    // wrapper-shape, leaf-conformance, or future-arm diagnostic piles on top
    // of an already-reported upstream error. Hoisted above the match so the
    // contract is explicit and uniform across all current and future arms.
    if matches!(param_type, Type::Error) || matches!(arg_type, Type::Error) {
        return;
    }
    match (param_type, arg_type) {
        // Wrapper pairs: recurse into inner types lockstep.
        (Type::Option(inner_p), Type::Option(inner_a)) => {
            walk_param_against_arg_type(inner_p, inner_a, ctx);
        }
        (Type::List(inner_p), Type::List(inner_a)) => {
            walk_param_against_arg_type(inner_p, inner_a, ctx);
        }
        (Type::Set(inner_p), Type::Set(inner_a)) => {
            walk_param_against_arg_type(inner_p, inner_a, ctx);
        }
        (Type::Map(key_p, val_p), Type::Map(key_a, val_a)) => {
            walk_param_against_arg_type(key_p, key_a, ctx);
            walk_param_against_arg_type(val_p, val_a, ctx);
        }
        // Leaf: param type is a trait object.
        // No FunctionCall promotion needed here — we work with the resolved result_type.
        (Type::TraitObject(required_trait), arg_ty) => match arg_ty {
            // StructureRef / TraitObject args: walk trait bounds via the shared helper.
            Type::StructureRef(_) | Type::TraitObject(_) => {
                emit_leaf_conformance_for_arg_type(arg_ty, required_trait, ctx);
            }
            // Geometry conformance (Bounded/Connected/Convex) is decided from the
            // compiled op-array, which is only reachable through the literal walker
            // (`check_leaf_trait_conformance`). A type-level `Type::Geometry` leaf
            // (e.g. an `Option<Geometry>` ValueRef) carries no op-array here, so we
            // cannot verify it — skip rather than risk a false positive. `Type::Error`
            // is anti-cascade (a nested poison sentinel reaching this leaf).
            // `Type::TypeParam` is an unresolved generic type variable (e.g. `T` in a
            // generic structure/function that forwards a type-param-typed value to a
            // trait param). It is "unknown/unresolved", NOT "definitely
            // non-conforming" — its real conformance is decided once `T` is bound to a
            // concrete type at instantiation — so it is unverifiable at this leaf and
            // skipped. Emitting here would be a false positive (esc-4081 amend,
            // reviewer_comprehensive robustness_false_positive).
            Type::Geometry | Type::Error | Type::TypeParam(_) => {}
            // Any other leaf type (scalar, enum, point, vector, …) can never carry
            // trait bounds, so it cannot conform to a trait param. Emit the same
            // diagnostic the literal walker emits for non-struct/non-trait leaves.
            // Without this arm, a wrapper-nested trait param reached with a
            // non-literal arg — e.g. `param j : Option<DrivingJoint>` given an
            // `Option<Real>` ValueRef — would silently lose the previously-existing
            // "no matching overload" hard error (task-4081 soundness regression,
            // esc-4081-174). The bare (unwrapped) trait-param case is already
            // covered by the literal walker's own fallback arm.
            _ => {
                ctx.diagnostics.push(
                    diag_at(
                        ctx.severity,
                        format!(
                            "type '{}' does not conform to trait '{}' required by param '{}'",
                            arg_ty, required_trait, ctx.arg_name
                        ),
                    )
                    .with_code(DiagnosticCode::TypeNotConformingToTrait)
                    .with_label(DiagnosticLabel::new(
                        ctx.span,
                        format!("expected a type conforming to trait '{}'", required_trait),
                    )),
                );
            }
        },
        // Leaf: param type is a StructureRef (nominal structure type, task-4584).
        // Skip/gate logic is in `reject_if_incompatible` (Error anti-cascade,
        // TypeParam unresolved, Geometry unverifiable, TraitObject may resolve).
        (Type::StructureRef(_), arg_ty) => {
            reject_if_incompatible(param_type, arg_ty, ctx, emit_structure_ref_mismatch);
        }
        // Leaf: param type is a Vector (task-4622). SHAPE-BASED with an arity check
        // and, since task 5766, a QUANTITY-SLOT check: accept any vector-shaped arg,
        // rejecting only when both the param's and the arg's quantity slots name a
        // CONCRETE dimension and those dimensions disagree. A dimensionless / `Int` /
        // `ScalarParam` quantity on either side stays loose — see
        // [`quantity_slot_dimension`] for why the rule is dimensionless-tolerant
        // rather than strict, and `crates/reify-core/src/ty.rs`'s
        // "Point / Vector quantity-slot convention" for the normative statement.
        // For `Type::Vector` args, additionally
        // require matching arity (n): `vec2` is NOT a valid substitute for a `vec3` param.
        // `Type::Tensor { rank: 1, .. }` args are accepted regardless of element count
        // (Tensor arity is not yet pinned in param signatures, so accepted conservatively).
        // Reject bare scalars, strings, bools, and any other non-vector kind.
        //
        // Unverifiable arg kinds are skipped through the shared
        // [`arg_type_is_unverifiable`] gate (Error anti-cascade, TypeParam
        // unresolved-generic, Geometry, TraitObject) — the SAME function the other
        // three shape arms and `reject_if_incompatible` call, so the set cannot
        // drift between them. For all other concrete arg types, reject unless the
        // arg is vector-shaped with matching arity.
        //
        // NOT type_compatible: implicitly_converts_to has no Vector<->Vector
        // quantity-coercion arm, so type_compatible(Vector3<Length>, Vector3<Real>)
        // is FALSE — a naive type_compatible gate would falsely reject `vec3(0,0,1)`
        // (dimensionless) for a Length-quantity param (see task-4622 design decision D1).
        // Task 5766's quantity check does NOT reintroduce that hazard: it is keyed on
        // both sides naming a concrete dimension, so the dimensionless case it would
        // have false-rejected is precisely the case that yields `None` and stays silent.
        (Type::Vector {
            quantity: param_quantity,
            ..
        }, arg_ty)
            if !arg_type_is_unverifiable(arg_ty) =>
        {
            // Accept vector-shaped args; for Type::Vector args, also require matching
            // arity (n). A Tensor{rank:1} is accepted regardless of its element count.
            let is_conforming = match arg_ty {
                Type::Vector { n: arg_n, .. } => match param_type {
                    Type::Vector { n: param_n, .. } => param_n == arg_n,
                    _ => true, // unreachable: outer arm guards param_type as Type::Vector
                },
                Type::Tensor { rank: 1, .. } => true,
                _ => false,
            };
            if !is_conforming {
                emit_vector_mismatch(param_type, arg_ty, ctx);
            } else if let Some(arg_quantity) = match arg_ty {
                // QUANTITY SLOT (task 5766), checked only AFTER the family/arity
                // check above has passed, and only for the two arg families that
                // actually CARRY a comparable slot. Every other accepted arg
                // shape reaches here with no slot to compare and stays silent —
                // which is how the `is_numeric_placeholder_leaf` unknown-ness
                // fence survives by construction (see
                // [`quantity_slots_conflict`]).
                Type::Vector { quantity, .. } | Type::Tensor { rank: 1, quantity, .. } => {
                    Some(quantity)
                }
                _ => None,
            } {
                // Routed through `emit_arg_type_mismatch`, NOT
                // `emit_vector_mismatch`: this arm's bespoke
                // `TypeNotConformingToVector` code keeps owning ARITY/FAMILY
                // failures only, so the diagnostic code carries real information
                // about WHICH rule fired instead of collapsing two distinct
                // failures into one label.
                if quantity_slots_conflict(param_quantity, arg_quantity) {
                    emit_arg_type_mismatch(param_type, arg_ty, ctx);
                }
            }
        }
        // Leaf: param type is a Point (task 5465, family 1). SHAPE-BASED with an
        // arity check, modelled directly on the `Type::Vector` arm above and
        // sharing its anti-cascade/unverifiable skip guard via the same
        // [`arg_type_is_unverifiable`] function.
        //
        // Accepts:
        //   • `Type::Point { n: arg_n, .. }` when `arg_n` matches the param's
        //     `n` AND the two QUANTITY slots do not conflict. Since task 5766 the
        //     slot rule is: loose whenever either side declines to name a
        //     concrete dimension (a dimensionless `Scalar`, `Type::Int`, or
        //     `Type::ScalarParam` — `Value::Point::infer_type` yields the first
        //     two), strict `DimensionVector` equality when BOTH name one. The
        //     normative statement lives in the "Point / Vector quantity-slot
        //     convention" section of `crates/reify-core/src/ty.rs` and is
        //     deliberately not restated here; the predicate is
        //     [`quantity_slots_conflict`].
        //   • Scalar-like numeric args, as the expression compiler's
        //     numeric-fallback placeholder for point-producing builtins.
        //
        // The placeholder predicate is deliberately NARROW — see
        // [`is_numeric_placeholder_leaf`] (`Int | Scalar | ScalarParam`).
        // `type_compat.rs::is_scalar_like_leaf` is NOT reused: it also admits
        // `Bool`, `String`, `Enum`, `StructureRef`, `TraitObject` and
        // `Geometry`, which would make `Anchor(origin: "origin")` silent.
        //
        // WHY THE TOLERANCE LIVES INSIDE THIS ARM rather than as an arg-side
        // `CompiledExpr` skip in the shared fallback. A broad "skip any
        // FunctionCall arg whose result_type is Scalar/Int" would punch a hole
        // through the ALREADY-allowlisted families — `Widget(label:
        // some_scalar_fn())` at a `String` param would go silent. Keyed on
        // `Type::Point`, this arm structurally cannot be reached by a `String` /
        // `Bool` / `Int` / `Real` param, so that hazard is dissolved by
        // construction. It also covers the `let p = point3(…); Anchor(origin: p)`
        // shape, which an arg-side skip would MISS: the placeholder type
        // propagates through the value cell and only the type-level walker sees
        // the resulting `ValueRef`.
        //
        // THE BOUNDED, DELIBERATE COST: a bare numeric literal at a Point slot
        // (`Anchor(origin: 5)`) stays silent. That is identical in kind to the
        // pre-existing `Type::Geometry` placeholder exclusion (geometry
        // constructors compile to a dimensionless-scalar placeholder, GHR-γ).
        // The tolerance can be tightened to a FunctionCall-shaped check once
        // `point3` carries a real return type — tracked by the family-5 /
        // placeholder follow-up filed with this task.
        //
        // That bounded cost is UNCHANGED by task 5766's quantity rule, and for the
        // same structural reason it exists: a bare numeric literal reaches this arm
        // through the `is_numeric_placeholder_leaf` branch, which carries no
        // quantity slot for the rule to compare. 5766 tightens the case where the
        // arg IS a `Type::Point` and its slot names a conflicting dimension; it
        // does not and cannot reach the placeholder branch.
        (
            Type::Point {
                quantity: param_quantity,
                ..
            },
            arg_ty,
        ) if !arg_type_is_unverifiable(arg_ty) => {
            let is_conforming = match arg_ty {
                Type::Point { n: arg_n, .. } => match param_type {
                    Type::Point { n: param_n, .. } => param_n == arg_n,
                    _ => true, // unreachable: outer arm guards param_type as Type::Point
                },
                // Numeric-fallback placeholder for point-producing builtins.
                other => is_numeric_placeholder_leaf(other),
            };
            if !is_conforming {
                emit_arg_type_mismatch(param_type, arg_ty, ctx);
            } else if let Type::Point {
                quantity: arg_quantity,
                ..
            } = arg_ty
            {
                // QUANTITY SLOT (task 5766), checked only AFTER the arity match
                // above has succeeded, and only for a genuinely `Type::Point` arg.
                // The `is_numeric_placeholder_leaf` branch is deliberately NOT
                // routed through here: a placeholder scalar has no quantity slot,
                // and that branch is the one every real corpus `point3(…)` arg
                // takes — it must stay dimension-blind.
                if quantity_slots_conflict(param_quantity, arg_quantity) {
                    emit_arg_type_mismatch(param_type, arg_ty, ctx);
                }
            }
        }
        // Leaf: param type is a Matrix or Tensor (task 5465, family 2).
        // SHAPE-BASED, sharing the other shape arms' anti-cascade/unverifiable
        // skip guard through [`arg_type_is_unverifiable`]. Handled as ONE arm
        // because `type_compat.rs` already treats the two families as
        // interconvertible.
        //
        // Accepts, each for a stated reason:
        //   • `Type::Matrix` / `Type::Tensor` — the nominal families themselves.
        //     Rule 3 (`type_compat.rs:163-179`) already makes
        //     `Tensor<2,N,Q> → Matrix<N,N,Q>` legal. Since task 5766 the FAMILY
        //     accept is followed by a QUANTITY-SLOT check (below), so the slot is
        //     loose only while at least one side declines to name a concrete
        //     dimension.
        //   • `Type::Vector` — Rules 1a/1b (`type_compat.rs:83-108`) make
        //     `Vector<N,Q>` and `Tensor<1,N,Q>` interconvertible.
        //   • `Type::List` — the idiomatic nested-list-literal spelling of a
        //     matrix, which compiles to `List<List<Real>>` — but ONLY when it
        //     bottoms out in a numeric/tensor element type. See
        //     [`list_bottoms_out_numeric`]: an unconstrained `Type::List(_)`
        //     accept also swallowed `Body(inertia: ["a", "b"])`, leaving the
        //     promoted family a hole on the very shape the accept exists for.
        //   • `Type::Int | Type::Scalar | Type::ScalarParam` — rank-0 scalar
        //     equivalence (Rules 2a/2b) plus matrix/tensor-builtin
        //     numeric-fallback placeholders, via the same narrow
        //     [`is_numeric_placeholder_leaf`] the `Point` arm uses.
        //
        // Everything else (`String`, `Bool`, `Selector`/`AnySelector`, `Enum`,
        // `Applied`, `StructureRef`, `Field`, `Point`, `Frame`, `Transform`,
        // `Set`, `Map`, a non-numeric `List`, …) is rejected via the shared
        // `emit_arg_type_mismatch`.
        //
        // NO ARITY CHECK, in deliberate contrast to the `Point` and `Vector`
        // arms. `List<List<Real>>` — the idiomatic spelling — carries no element
        // counts in its type, so an m/n (or rank) check would either false-reject
        // every list-literal matrix or be silently unenforceable for exactly the
        // shape that matters. Constraining the ELEMENT FAMILY (above) is the part
        // that is decidable from the type alone; arity/PSD validation for
        // `MassProperties.inertia` is the ctor-time inertia hook's job (see
        // `examples/dynamics/pendulum_idyn.ri:24-26`), not this walker's.
        //
        // That asymmetry is UNCHANGED by task 5766 and ORTHOGONAL to it: ARITY is
        // undecidable here because the idiomatic `Type::List` spelling drops the
        // element counts, whereas the QUANTITY slot is decidable whenever the arg
        // happens to arrive in one of the three nominal families. The same
        // `Type::List` accept is also why strict `DimensionVector` equality is
        // unavailable for this family and the rule must stay dimensionless-
        // tolerant: `MassProperties.inertia` is spelled `List<List<Real>>` at all
        // 12 corpus sites, so a strict rule would compare a declared
        // `MomentOfInertia` against a hole. See [`quantity_slot_dimension`], and
        // `crates/reify-core/src/ty.rs` for the normative statement.
        (
            Type::Matrix {
                quantity: param_quantity,
                ..
            }
            | Type::Tensor {
                quantity: param_quantity,
                ..
            },
            arg_ty,
        ) if !arg_type_is_unverifiable(arg_ty) => {
            let is_conforming = match arg_ty {
                Type::Matrix { .. } | Type::Tensor { .. } | Type::Vector { .. } => true,
                Type::List(_) => list_bottoms_out_numeric(arg_ty),
                other => is_numeric_placeholder_leaf(other),
            };
            if !is_conforming {
                emit_arg_type_mismatch(param_type, arg_ty, ctx);
            } else if let Some(arg_quantity) = match arg_ty {
                // QUANTITY SLOT (task 5766), checked only AFTER the family check
                // above has passed. ONLY the three nominally-shaped arg families
                // carry a comparable slot; the `Type::List` and
                // `is_numeric_placeholder_leaf` branches deliberately yield
                // `None` here and are left EXACTLY as they were:
                //
                //   * `Type::List` — a nested list literal carries no quantity in
                //     its type at all, which is precisely why strict equality is
                //     unavailable for this family (`MassProperties.inertia` is
                //     spelled `List<List<Real>>` at all 12 corpus sites).
                //   * a placeholder scalar — the `is_numeric_placeholder_leaf`
                //     unknown-ness fence, which this rule must not erode.
                Type::Matrix { quantity, .. }
                | Type::Tensor { quantity, .. }
                | Type::Vector { quantity, .. } => Some(quantity),
                _ => None,
            } && quantity_slots_conflict(param_quantity, arg_quantity)
            {
                emit_arg_type_mismatch(param_type, arg_ty, ctx);
            }
        }
        // Leaf: param type is a Selector or AnySelector (task-4598).
        // Skip/gate logic is in `reject_if_incompatible`; `type_compat.rs` AnySelector
        // arms encode: AnySelector accepts any Selector(k), rejects Real/String/Int/…;
        // Selector(k) accepts only Selector(k), rejects AnySelector/others.
        (Type::Selector(_) | Type::AnySelector, arg_ty) => {
            reject_if_incompatible(param_type, arg_ty, ctx, emit_selector_mismatch);
        }
        // Leaf: param type is a Field (task 5465, family 3). SHAPE-BASED, and
        // deliberately WITHOUT a domain/codomain check.
        //
        // Both `Field` slots erase to the expression compiler's numeric
        // fallback: a legitimate arg's `result_type` is `Field<Real, Real>`
        // however the field was declared (an analytical `field def` is the
        // common case — `examples/fea_shell_channels.ri` alone produced six such
        // false warnings before this arm existed: top / mid / bottom /
        // displacement / stress / frame). Comparing the declared slots against
        // that placeholder would compare a declaration against a hole, so this
        // arm is NOT routed through `type_compatible` — the same reason the
        // `Type::Vector` arm above is shape-based (task-4622 D1).
        //
        // The skip guard is the shared [`arg_type_is_unverifiable`] function, so
        // the anti-cascade / unverifiable set cannot drift between the arms:
        // Error (anti-cascade), TypeParam (unresolved generic, decided at
        // instantiation), Geometry (unverifiable at the type level), TraitObject
        // (may resolve to a field-producing type).
        (Type::Field { .. }, arg_ty) if !arg_type_is_unverifiable(arg_ty) => {
            // Accept any field-shaped arg, plus a lambda: `Type::Function` is a
            // legitimate spelling of a field-shaped arg at a `Field` slot.
            // Everything else (String, Bool, Int, Scalar, Point, List, …) is
            // rejected through the shared `emit_arg_type_mismatch`, which already
            // emits `ArgTypeMismatch` at `ctx.severity` — no new DiagnosticCode.
            if !matches!(arg_ty, Type::Field { .. } | Type::Function { .. }) {
                emit_arg_type_mismatch(param_type, arg_ty, ctx);
            }
        }
        // Option-unwrap (implicit-Some, C1 rule 6 — task 5302 α): an `Option<T>`
        // param supplied a bare non-Option arg of type `T` is a valid `Some(arg)`.
        // Recurse on the inner param type so the arg is checked against `T`. This
        // MUST precede the wrapper-shape catch-all below: without it, a valid
        // implicit-Some arg (e.g. `Option<FaceSelector>` ← `Selector(Face)`) would
        // fall through to the wrapper-shape arm and spuriously warn — the live hole
        // this task closes. The `Option`/`Option` pair is handled by the wrapper-
        // pair arm at the top of this match, so the guard here only ever fires for a
        // non-Option arg.
        (Type::Option(inner_p), arg_ty) if !matches!(arg_ty, Type::Option(_)) => {
            walk_param_against_arg_type(inner_p, arg_ty, ctx);
        }
        // Wrapper-shape mismatch OR general concrete-leaf mismatch.
        _ => {
            if matches!(
                param_type,
                Type::List(_) | Type::Set(_) | Type::Map(_, _)
            ) {
                // Wrapper-shape mismatch: a `List/Set/Map<T>` param supplied an arg
                // whose shape doesn't match (e.g. a bare leaf, or a differently-
                // shaped wrapper). (`Option<T>` params never reach here — the
                // Option-unwrap arm above handles every non-Option arg, and the
                // wrapper-pair arm handles `Option`/`Option`.) Emitted at ctx.severity.
                ctx.diagnostics.push(
                    diag_at(
                        ctx.severity,
                        format!(
                            "type '{}' does not match wrapper shape required by param '{}' (expected '{}')",
                            arg_type, ctx.arg_name, param_type
                        ),
                    )
                    .with_code(DiagnosticCode::TypeNotConformingToTrait)
                    .with_label(DiagnosticLabel::new(
                        ctx.span,
                        format!("expected '{}', got '{}'", param_type, arg_type),
                    )),
                );
            } else if let Some(param_enum) = base_enum_name(param_type, ctx.registries.enum_defs) {
                // ENUM FAMILY (task 5465, family 4). Handled as its own branch,
                // BEFORE the allowlist, so the D1/F-Mono erasure gap is absorbed
                // without a `type_compatible` call that cannot express it.
                //
                // Under erasure (PRD §7.1: "resolved args live only in the
                // per-site substitution map, never on the persisted `Type` or
                // `Value`"), a constructed variant's `result_type` is the bare
                // `Type::Enum(name)`, while a declared annotation may be the
                // applied `Type::Applied { name, args }`. `type_compatible` has
                // no Applied-vs-Enum rule in EITHER direction, so both spellings
                // must be reconciled here.
                //
                // SYMMETRIC by construction. The first cut short-circuited on
                // `enum_payload_compatible`, which only matches a bare
                // `Type::Enum` on the SUPPLIED side (type_compat.rs:334-337).
                // Combined with `Type::Enum(_)` joining the allowlist, the
                // reverse pairing — a param declared bare `Enum("Result")`
                // supplied an arg whose `result_type` is the applied
                // `Applied { name: "Result", .. }` (a `ValueRef` to a cell
                // declared `Result<Length, String>`, or a call to a fn whose
                // return annotation is the applied form, cf.
                // `examples/m6_result_recovery.ri:30`) — fell through to
                // `type_compatible` and produced a SPURIOUS `ArgTypeMismatch`
                // that was silent before the promotion. On the
                // `check_fn_arg_conformance` path that is a hard Error, so it
                // would have failed the compile rather than merely added noise
                // (reviewer_comprehensive, robustness-false-positive).
                //
                // Mapping BOTH sides through [`base_enum_name`] — the same
                // oracle `enum_payload_compatible` itself now uses, so the
                // payload-field guard in `variant_construct.rs` and this arm
                // cannot drift — makes the tolerance direction-agnostic. It also
                // removes the double `enum_defs` scan the first cut performed
                // per `Applied` leaf (once inside `enum_payload_compatible`,
                // again inside `general_leaf_param_family_is_validated`);
                // membership is now resolved exactly once per side, and not at
                // all for the non-enum params that dominate the corpus
                // (reviewer_comprehensive, efficiency).
                //
                // STILL A TARGETED TOLERANCE, NOT A FAMILY BYPASS: only the
                // same-base-name pairing is accepted. A genuine cross-enum
                // mismatch (`Hue` ← `Outline`) or a non-enum arg
                // (`Result<…>` ← `String`) falls straight through to the shared
                // `reject_if_incompatible` gate below and is rejected exactly as
                // an allowlisted family would be. Type-ARG agreement is the
                // inference/pin passes' job, not this walker's — see
                // `enum_payload_compatible`'s doc.
                if base_enum_name(arg_type, ctx.registries.enum_defs) != Some(param_enum) {
                    reject_if_incompatible(param_type, arg_type, ctx, emit_arg_type_mismatch);
                }
            } else if general_leaf_param_family_is_validated(param_type) {
                // GENERAL CONCRETE-LEAF arm (task 5302 α): this replaces the former
                // silent `_` fall-through for the param families that
                // [`general_leaf_param_family_is_validated`] vets, routing them
                // through the shared `reject_if_incompatible` → `type_compatible`
                // gate and emitting `ArgTypeMismatch` at ctx.severity for a genuine
                // mismatch. Every other family keeps falling through silently,
                // exactly as before α — see that predicate's doc comment for the
                // per-family rationale and the sound-by-construction posture.
                reject_if_incompatible(param_type, arg_type, ctx, emit_arg_type_mismatch);
            }
        }
    }
}

/// Whether `param_type` belongs to a param family whose args are safe to judge
/// with raw [`type_compatible`] at the general concrete-leaf arm of
/// [`walk_param_against_arg_type`].
///
/// # Why this is an allowlist and not a skip list
///
/// The α cut of the general-leaf arm used a NEGATIVE guard
/// (`!matches!(param_type, Type::Geometry | Type::TypeParam(_))`), which made it
/// unsound BY DEFAULT: every present and future [`Type`] variant was opted in to
/// `type_compatible` unless somebody remembered to carve it out. Five families
/// were false-rejected across 15+ shipped examples before the corpus gate in
/// `examples_smoke.rs` caught it.
///
/// The inversion here makes the arm sound BY CONSTRUCTION — a newly-added `Type`
/// variant defaults to silence, not to a false positive. Broadening the list is
/// a deliberate act that must come with corpus evidence.
///
/// # Why these families
///
/// `type_compatible` is a *call-site coercion* predicate: it presumes both sides
/// carry genuinely inferred types. `Bool`, `Int`, `String` and DIMENSIONLESS
/// `Scalar` (spelled `Real` in source, and by `Display`) are the families for
/// which that presumption holds unconditionally at a struct-ctor arg position —
/// the expression compiler infers them precisely, and `type_compat.rs` has real
/// arms for each.
///
/// # Handled by dedicated shape-based arms instead (task 5465)
///
/// `Point`, `Field`, `Matrix` and `Tensor` are NOT excluded — they are handled
/// by dedicated SHAPE-BASED arms in [`walk_param_against_arg_type`], exactly as
/// `Type::Vector` is, so none ever reaches this predicate. The
/// placeholder/erasure/missing-coercion rationale below is still why those arms
/// are shape-based rather than `type_compatible`-based:
///
/// * `point3(0m, 0m, 0m)` is a `FunctionCall` whose result_type is the numeric
///   fallback `Scalar[m]`, never `Type::Point`;
/// * an analytical `field def` erases both slots to `Field<Real, Real>` whatever
///   its declaration says;
/// * a nested list literal is the idiomatic `Matrix3x3` spelling but compiles to
///   `List<List<Real>>`, for which no `List`→`Matrix` coercion arm exists.
///
/// But "unverifiable SLOTS" is not the same as "unverifiable FAMILY", and the
/// families themselves are now checked. For the two placeholder families this is
/// the same class the `Type::Geometry` exclusion below and
/// [`promote_function_call_to_structure_ref`] already exist for.
///
/// # Deliberately excluded, with evidence
///
/// Contrary to what this comment claimed before task 5465, promoting those four
/// families needed NO new `type_compat.rs` coercion RULE — the shape-based arms
/// and the [`base_enum_name`]-keyed enum branch sit entirely inside this walker
/// (`base_enum_name` itself is a factoring of a predicate `type_compat.rs`
/// already had, not a new coercion). What remains excluded is excluded for its
/// own stated reason, and each
/// entry names an owner (INV-SF-5 `placeholders-owned-and-loud`,
/// `docs/legibility/design-invariants.md:126-128`, forbids a blanket escape that
/// names none):
///
/// * **Dimensioned `Scalar`** — supplying a bare dimensionless numeric literal
///   at a dimensioned slot is idiomatic throughout the corpus. HELD, not
///   forgotten: whether it should be legal is a language-semantics question
///   about the dimensionless↔dimensioned slot convention, and today's answer is
///   position-dependent across six gates (legal at ctor field slots, literal
///   `param`/`let` defaults and constraint-def args; illegal at user-fn param
///   slots, fn-param defaults, ambient defaults, compound initializers and all
///   arithmetic). Owner: task #5627 (filed from this task as ticket
///   `tkt_0RRQW5X0WYH2ZW0TZY1JZ6E189`, escalation `agent-followup-5465`), which
///   carries the four candidate resolutions and their costs.
/// * **`Geometry`** — geometry constructors compile to a dimensionless-scalar
///   placeholder (GHR-γ) and `type_compatible` has no `Geometry` arm at all;
///   geometry conformance is decided only through the literal walker's op-array
///   inference.
/// * **`TypeParam`** — an unresolved generic param, whose real conformance is
///   decided when the type var is bound at instantiation (mirrors the arg-side
///   `TypeParam` skip inside [`reject_if_incompatible`]).
///
/// The last two were the α negative guard's explicit carve-outs; they are
/// subsumed by simple absence from the allowlist, so the redundant `matches!`
/// is gone rather than left as dead belt-and-braces.
///
/// # Still unexamined
///
/// Every other [`Type`] variant defaults to silence and has simply not been
/// looked at yet. Enumerated against the real `Type` enum
/// (`crates/reify-core/src/ty.rs:120-345`) so the list cannot quietly rot:
/// `Complex`, `Orientation`, `Frame`, `Transform`, `AffineMap`, `Range`,
/// `Plane`, `Axis`, `Direction`, `BoundingBox`, `Feature`, `Function`, `Keyed`,
/// `Union`, `Projection`, `ScalarParam`. Promoting any of them is the same
/// deliberate act the four families above went through: a per-family probe pair
/// (clean fixture + value floor) plus a green corpus gate, with no new
/// `SKIP_SET` entry.
///
/// # Enum families (task 5465, family 4) — handled by the caller, not here
///
/// `Type::Enum(_)` and enum-named `Type::Applied` are promoted, but they never
/// reach this predicate: the general concrete-leaf arm dispatches them into its
/// own [`base_enum_name`]-keyed branch first, which absorbs the D1/F-Mono
/// erasure gap SYMMETRICALLY (either side may be the bare or the applied
/// spelling) and then falls into the same shared `reject_if_incompatible` gate
/// for a residual genuine mismatch (`Hue` ← `Outline`, `Result<…>` ← `String`).
///
/// The first cut of family 4 instead listed both variants here and relied on a
/// preceding `enum_payload_compatible` short-circuit. That was one-directional
/// (it matches only a bare `Type::Enum` on the SUPPLIED side) and it scanned
/// `enum_defs` twice per applied leaf. Keeping the whole enum decision in one
/// branch fixes both — see the arm comment.
///
/// Note that `Type::Applied` was never a candidate for a BLANKET entry here
/// regardless: it also represents generic STRUCTURE refs (e.g. `Coupling<T>`,
/// see `type_arg_applied_resolution_tests.rs`), and `implicitly_converts_to` has
/// no Applied-vs-StructureRef arm, so an unconditional entry would reintroduce
/// exactly the class of false positive this allowlist exists to prevent. An
/// applied generic structure still falls through this predicate silently.
fn general_leaf_param_family_is_validated(param_type: &Type) -> bool {
    match param_type {
        Type::Bool | Type::Int | Type::String => true,
        Type::Scalar { dimension } => dimension.is_dimensionless(),
        _ => false,
    }
}

/// RAII guard that pops the top entry from an in-flight Vec when dropped.
///
/// Ensures the `push`–`pop` invariant around [`infer_traits_for_expr_in_env`]
/// in [`RealizationLetEnv::lookup`] is upheld even if the inner call panics.
/// Used only within `RealizationLetEnv::lookup`.
struct InFlightGuard<'a> {
    stack: &'a RefCell<Vec<ValueCellId>>,
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.stack.borrow_mut().pop();
    }
}

/// A `LetBindingEnv` that resolves geometry-typed `let` bindings by looking up
/// their compiled op-arrays in the parent template's `realizations` list.
///
/// For non-geometry lets (`ValueCellKind::Let` with a `default_expr`), falls
/// back to `infer_traits_for_expr_in_env` on the resolved `CompiledExpr`,
/// threading `self` through so chained `ValueRef` lets are resolved recursively.
/// For any id that doesn't match a known let or realization, returns `None`
/// (safe-default: the caller falls back to `InferredTraits::all()`).
///
/// # Geometry-let lookup
///
/// `compile_entity` Pass 2 explicitly `continue`s past geometry-typed
/// `MemberDecl::Let`, so geometry lets never enter `value_cells`. They are
/// lowered in Pass 3 to `Vec<CompiledGeometryOp>` stored on
/// `RealizationDecl.operations` with `RealizationDecl.name = Some(let_name)`.
/// `lookup` therefore scans `realizations` first (geometry path) and falls
/// back to `value_cells` (non-geometry path).
///
/// # Chained let
///
/// `compile_geometry_call` already handles geometry-let chaining at the AST
/// level: for `let g = h; let h = union(box, box)`, the realization for `g`
/// recursively compiles `h`'s init expression and emits the SAME op array
/// under `realization.name = Some("g")`. No chained-ValueRef walk is needed
/// here.
///
/// # Cycle guard
///
/// The value_cells fallback calls `infer_traits_for_expr_in_env(expr, self)`,
/// which recurses back into `lookup` for any nested `ValueRef`. Malformed or
/// forward-referencing IR can produce cycles (`let g = ValueRef(g)`).  Rather
/// than stack-overflowing, the `in_flight` visited set detects re-entry and
/// returns `None`; the caller's `unwrap_or(InferredTraits::all())` mechanism
/// safe-defaults the result.  The realization arm does NOT recurse through
/// the env (it calls `infer_traits_for_op` instead), so `in_flight` is only
/// checked on the value_cells path.
struct RealizationLetEnv<'a> {
    templates: &'a HashMap<String, &'a TopologyTemplate>,
    /// In-flight visited set used to terminate cycles in the value_cells fallback.
    ///
    /// `RefCell` is required because `LetBindingEnv::lookup` takes `&self`;
    /// changing the trait to `&mut self` would cascade to all other impls.
    /// `Vec` (not `HashSet`) because typical recursion depth is 1–4; a linear
    /// scan is faster than hash overhead at this size, and avoids a new dep.
    in_flight: RefCell<Vec<ValueCellId>>,
}

impl<'a> LetBindingEnv for RealizationLetEnv<'a> {
    fn lookup(&self, id: &ValueCellId) -> Option<InferredTraits> {
        let template = self.templates.get(id.entity.as_str())?;

        // Geometry-let path: realization with matching name.
        if let Some(real) = template
            .realizations
            .iter()
            .find(|r| r.name.as_deref() == Some(id.member.as_str()))
        {
            return Some(infer_traits_for_op(&real.operations));
        }

        // Non-geometry-let fallback: value_cells Let with a resolved default_expr.
        // Match on both entity and member for symmetry with the realization arm
        // above (entity is already narrowed by `templates.get(id.entity.as_str())`,
        // but the explicit check makes the intent self-documenting and adds a
        // cheap guard against any future refactor that reuses cells across templates).

        // Cycle guard: if `id` is already being resolved on the current call stack,
        // return None.  The caller's `unwrap_or(InferredTraits::all())` in
        // `infer_traits_for_expr_in_env` converts None into the safe default, so
        // cycles in malformed or forward-referencing IR yield `all()` rather than
        // a stack overflow.
        if self.in_flight.borrow().iter().any(|p| p == id) {
            return None;
        }

        if let Some(cell) = template.value_cells.iter().find(|vc| {
            vc.id.entity == id.entity
                && vc.id.member == id.member
                && matches!(vc.kind, ValueCellKind::Let)
                && vc.default_expr.is_some()
        }) {
            // Thread the env through the resolved CompiledExpr so that any
            // ValueRef nested inside a non-geometry let (e.g. a chained let
            // whose default_expr references a geometry binding) is resolved via
            // the env rather than silently safe-defaulting to all(). The
            // in-flight guard above terminates any cycle in well-formed or
            // malformed IR before a stack overflow can occur.
            let expr = cell.default_expr.as_ref()?;
            self.in_flight.borrow_mut().push(id.clone());
            let _guard = InFlightGuard {
                stack: &self.in_flight,
            };
            let result = infer_traits_for_expr_in_env(expr, self);
            return Some(result);
        }

        None
    }
}

/// Leaf conformance check: verify that `compiled_arg` conforms to `required_trait`.
///
/// Derives `arg_call_name` from `compiled_arg.kind` so that any nested
/// `FunctionCall` discovered by the walk re-uses the existing `Real|Int →
/// StructureRef` promotion.
///
/// Conformance strategy:
/// - `Type::StructureRef` args: uses `satisfies_trait_bound` to walk the structure's declared
///   trait bounds, following refinement chains transitively (e.g. `Rigid : Physical : Material`
///   satisfies a `Material` param).
/// - `Type::TraitObject` args: uses `trait_satisfies` to check equality-or-refinement between
///   the arg trait and the required trait.
///
/// Emits at most one diagnostic per call.
fn check_leaf_trait_conformance(
    required_trait: &str,
    compiled_arg: &CompiledExpr,
    ctx: &mut WalkCtx<'_>,
) {
    let arg_type = &compiled_arg.result_type;

    // Derive arg_call_name using the shared helper so both the promotion check
    // (promote_function_call_to_structure_ref) and this suppression guard use the
    // same source of truth for "is this arg a function call?".
    let arg_call_name = extract_function_call_name(compiled_arg);

    // When the compiled arg_type defaulted to a numeric fallback (Real or Int)
    // from a FunctionCall expression and the callee is a known structure
    // template, promote to StructureRef so the conformance check can walk the
    // structure's trait bounds. Int appears when the callee's first arg is a
    // whole-number literal (e.g. `Steel(density: 1000.0)` — the literal 1000.0
    // is canonicalized to Int by the expression compiler).
    let promoted = promote_function_call_to_structure_ref(compiled_arg, ctx.registries.templates);
    let effective_arg_type = promoted.as_ref().unwrap_or(arg_type);

    // Geometry args at compile-inferred trait slots (`Bounded`/`Connected`/`Convex`):
    // route to the per-op inference table instead of the StructureRef/TraitObject
    // walker. The set of compile-inferred geometry traits is closed by the trait
    // markers in `crates/reify-compiler/stdlib/geometry_traits.ri` (task 2297) and
    // mirrored by the [`GeometryTrait`] enum in
    // `crates/reify-compiler/src/geometry_traits_inference.rs`.
    //
    // Per design decision §2 (see plan): missing `Bounded` emits the dedicated
    // [`DiagnosticCode::GeometryUnbounded`] code via [`emit_geometry_unbounded`];
    // missing `Connected`/`Convex` reuse the existing
    // [`DiagnosticCode::TypeNotConformingToTrait`] code (the PRD only allocates
    // `E_GEOMETRY_UNBOUNDED` for the Bounded case).
    //
    // The arg is treated as geometry-typed when **either**:
    //   1. `result_type == Type::Geometry` (a value-ref to a `let g = box(...)`
    //      previously registered as `Type::Geometry`), or
    //   2. it is a `FunctionCall` whose callee is in
    //      [`is_geometry_function`] — `box(...)` etc. compile to a
    //      `Type::dimensionless_scalar()` *placeholder* in the expression
    //      compiler (see `expr.rs:782`); without this fallback the placeholder
    //      Scalar would route to the generic-cascade arm.
    let is_geometry_arg = matches!(effective_arg_type, Type::Geometry)
        || extract_function_call_name(compiled_arg)
            .map(is_geometry_function)
            .unwrap_or(false);
    if is_geometry_arg {
        let geom_trait = match required_trait {
            "Bounded" => Some(GeometryTrait::Bounded),
            "Connected" => Some(GeometryTrait::Connected),
            "Convex" => Some(GeometryTrait::Convex),
            _ => None,
        };
        if let Some(trait_kind) = geom_trait {
            let env = RealizationLetEnv {
                templates: ctx.registries.templates,
                in_flight: RefCell::new(Vec::new()),
            };
            let inferred = infer_traits_for_expr_in_env(compiled_arg, &env);
            if !inferred.has(trait_kind) {
                // CARVE-OUT (task 5302 α): these two geometry-trait emit sites
                // deliberately stay `Severity::Error` — they do NOT read
                // `ctx.severity`. `GeometryUnbounded` and the geometry
                // `TypeNotConformingToTrait` belong to the geometry-primitive-
                // constructors PRD, not to this task's ctor-field knob, so they
                // are excluded from α's Warning downgrade set and the δ one-const
                // flip will NOT promote them. A future author who wants
                // geometry-trait ctor diagnostics governed by the knob must thread
                // `ctx.severity` through these helpers (see the
                // `CTOR_FIELD_CONFORMANCE_SEVERITY` / `WalkCtx.severity` docs).
                if matches!(trait_kind, GeometryTrait::Bounded) {
                    emit_geometry_unbounded(ctx.arg_name, ctx.span, ctx.diagnostics);
                } else {
                    emit_geometry_trait_violation(
                        ctx.arg_name,
                        required_trait,
                        ctx.span,
                        ctx.diagnostics,
                    );
                }
            }
            return;
        }
        // Other required traits against a geometry arg fall through to the
        // existing anti-cascade arm below.
    }

    // Check conformance based on effective_arg_type.
    // StructureRef and TraitObject are handled by the shared helper; non-struct/non-trait
    // types fall to the anti-cascade arm below.
    match effective_arg_type {
        Type::StructureRef(_) | Type::TraitObject(_) => {
            emit_leaf_conformance_for_arg_type(effective_arg_type, required_trait, ctx);
        }
        // Unresolved generic type variable — unverifiable at this leaf, mirroring the
        // type-level walker's `TypeParam` skip. A `T`-typed arg forwarded to a trait
        // param (e.g. `structure W<T> { let z = couple(inner) }` with `inner : T`) is
        // "unknown", not "definitely non-conforming"; its conformance is decided when
        // `T` is bound at instantiation. Skipping avoids a false positive (esc-4081
        // amend, reviewer_comprehensive robustness_false_positive).
        Type::TypeParam(_) => {}
        _ => {
            // Anti-cascade: when arg_type is a dimensionless-scalar/Int fallback
            // and arg_call_name refers to an UNKNOWN callee (not in the template
            // registry, so promotion returned None, and not a known geometry
            // function), an "undefined function" diagnostic already fired.
            // Emitting "type does not conform to trait 'X'" here would be
            // misleading — the fallback type is not the author's intended type.
            // Geometry functions (box, sphere, …) return dimensionless_scalar()
            // as a compile-time placeholder and are NOT unknown; they must NOT
            // be suppressed so a non-geometry trait receives the correct error.
            if matches!(arg_type, Type::Scalar { .. } | Type::Int)
                && arg_call_name.is_some_and(|n| !is_geometry_function(n))
            {
                return;
            }
            // Neither StructureRef nor TraitObject — cannot conform to a trait.
            // The original arg_type is used in the message (not the effective type,
            // which equals arg_type here since promotion didn't apply).
            ctx.diagnostics.push(
                diag_at(
                    ctx.severity,
                    format!(
                        "type '{}' does not conform to trait '{}' required by param '{}'",
                        arg_type, required_trait, ctx.arg_name
                    ),
                )
                .with_code(DiagnosticCode::TypeNotConformingToTrait)
                .with_label(DiagnosticLabel::new(
                    ctx.span,
                    format!("expected a type conforming to trait '{}'", required_trait),
                )),
            );
        }
    }
}

// ── Mechanism DrivingJoint bound check (task 4310 — mechanism γ) ─────────────

/// Resolve the nominal joint-structure type of a compiled argument expression.
///
/// **Path A** — `arg.result_type == Type::StructureRef(name)`: returns the name
/// directly. Covers structure-constructor `Coupling()` (once β lands), typed
/// `let` bindings annotated with a joint type, and any future site where the
/// compiler assigns `StructureRef` to a joint expression.
///
/// **Path B** — `arg` is a `CompiledExprKind::FunctionCall` whose callee is a
/// known joint-constructor builtin: maps the lowercase builtin name to the
/// PascalCase nominal type string.
///   couple / gear / screw / rack_and_pinion → "Coupling"
///   fixed                                   → "Fixed"
///   prismatic / revolute / cylindrical /
///   planar / spherical                      → their PascalCase kind names
///
/// Returns `None` for under-typed args (Real, Int, ValueRef without StructureRef
/// result type) and for unknown function names.  Callers skip the check in this
/// case; the α runtime guard backstops these sites (PRD D2).
fn resolve_joint_nominal_type(arg: &CompiledExpr) -> Option<String> {
    // Path A: result_type carries a nominal structure tag.
    //
    // Two variants (task #4605 ε, PRD §4.1):
    //   StructureRef(name)         — e.g. from prismatic(), revolute(), or a
    //                                 literal Coupling joint in a relate block.
    //   Applied { name, .. }       — e.g. from couple(j, ratio) after the
    //                                 couple→Applied change; phantom args are
    //                                 name-ignored (name-only extraction, PRD §4.1).
    //
    // A let-bound coupling (`let c = couple(j, ratio)`) has result_type
    // Applied{"Coupling",[StructureRef("Prismatic")]}; Path B (FunctionCall
    // name-map) does NOT match a ValueRef, so without this Applied arm the L2
    // E_MECHANISM_NONDRIVING_JOINT for `bind(c, v)` would be silently dropped,
    // regressing mechanism_nondriving_joint_diag_e2e.rs.
    match &arg.result_type {
        Type::StructureRef(name) => return Some(name.clone()),
        Type::Applied { name, .. } => return Some(name.clone()),
        _ => {}
    }
    // Path B: arg is a FunctionCall to a known joint-constructor builtin.
    //
    // DRIFT NOTE: this name→PascalCase mapping must stay in sync with the
    // joint-constructor builtins and JOINT_KINDS in reify-stdlib/src/joints.rs
    // (pub(crate), not directly importable here).  The unit test
    // `l2_joint_constructor_name_mapping_exhaustive` (conformance/mod.rs mod tests)
    // pins every arm; adding a new joint kind requires updating BOTH this match
    // AND that test.
    if let CompiledExprKind::FunctionCall { function, .. } = &arg.kind {
        let type_name = match function.name.as_str() {
            "couple" | "gear" | "screw" | "rack_and_pinion" => "Coupling",
            "fixed" => "Fixed",
            "prismatic" => "Prismatic",
            "revolute" => "Revolute",
            "cylindrical" => "Cylindrical",
            "planar" => "Planar",
            "spherical" => "Spherical",
            _ => return None,
        };
        return Some(type_name.to_owned());
    }
    None
}

/// Walk a `CompiledExpr` tree and emit
/// [`DiagnosticCode::MechanismNonDrivingJoint`] for each eval-builtin
/// mechanism call whose joint argument resolves to a known non-driving joint type.
///
/// ## Covered builtins (task γ step-8)
///
/// | Builtin | Arity condition | Joint arg index |
/// |---------|-----------------|-----------------|
/// | `bind`  | any             | 0               |
/// | `dim`   | any             | 0               |
/// | `sweep` | arity == 4      | 1               |
///
/// The arity-4 guard on `sweep` disambiguates the kinematic
/// `sweep(mechanism, joint, range, steps)` from the geometry
/// `sweep(profile, path)` (arity 2).  The geometry form is not a
/// mechanism builtin and must not be checked.
///
/// ## Joint-type resolution
///
/// Uses [`resolve_joint_nominal_type`]: Path A (`result_type == StructureRef`)
/// first, then Path B (FunctionCall to a known joint-constructor builtin).
/// Under-typed args (Real, Int, etc. with unknown callee) are silently skipped
/// — the α runtime guard backstops those sites (PRD D2).
///
/// ## Trait-bound check
///
/// Looks up the resolved structure name in `template_registry`.  If the
/// structure's `trait_bounds` do NOT satisfy `DrivingJoint` (checked via
/// [`satisfies_trait_bound`], which walks the refinement chain transitively),
/// one `MechanismNonDrivingJoint` diagnostic is emitted naming the type.
///
/// Coupling (trait_bounds = ["Joint"], NOT "DrivingJoint") → rejected.
/// Prismatic/Revolute/etc. (trait_bounds = ["DrivingJoint"]) → accepted.
///
/// Crucially, the check is **gated on Joint conformance**: if the resolved
/// structure does NOT satisfy `Joint` (i.e. it is not a joint type at all,
/// e.g. someone writes `bind(someBox, v)`), the function silently skips it
/// rather than emitting a misleading "joint type 'Box' does not satisfy
/// DrivingJoint" message.  A future generic type-mismatch pass will handle
/// that case.
///
/// ## L1/L2 disjoint invariant (PRD D2, "one code two sites")
///
/// [`DiagnosticCode::MechanismNonDrivingJoint`] has two emission sites:
/// - **L1 (eval):** `detect_nondriving_joint_errors` in `engine_eval.rs`
/// - **L2 (compile):** this function (task γ)
///
/// Double-emission is not a problem in the `reify check` CLI: `cmd_check`
/// (reify-cli/src/main.rs) returns early on any compile-time
/// `Severity::Error` diagnostic **before** invoking `engine.check()` (eval).
/// So: L2 fires → CLI exits → L1 never runs for the same expression.
/// The two sites are complementary (L2 for direct-constructor / nominal-typed
/// args; L1 for dynamically-determined joint values), not cumulative.
///
/// ## Diagnostic span
///
/// Uses the `representative_span` threaded by [`phase_fn_arg_conformance`]
/// from the enclosing field / constraint / realization.
pub(crate) fn check_expr_mechanism_joint_bound(
    expr: &CompiledExpr,
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    expr.walk(&mut |node: &CompiledExpr| {
        let CompiledExprKind::FunctionCall { function, args } = &node.kind else {
            return;
        };
        // Determine which argument position carries the joint.
        // bind/dim: arg0.  sweep@arity4 (kinematic): arg1.
        // Arity-2 sweep = geometry (CSG) — skip.
        let joint_arg_idx: usize = match function.name.as_str() {
            "bind" | "dim" => 0,
            "sweep" if args.len() == 4 => 1,
            _ => return,
        };
        let Some(joint_arg) = args.get(joint_arg_idx) else {
            return; // Arity too low — skip (malformed / already-diagnosed).
        };
        // Resolve the nominal joint type.
        let Some(type_name) = resolve_joint_nominal_type(joint_arg) else {
            return; // Under-typed / unknown — skip (D2: α runtime backstops).
        };
        // Look up the structure in the template registry.
        let Some(tmpl) = template_registry.get(type_name.as_str()) else {
            return; // Not a known structure in this scope — skip.
        };
        // Gate on Joint conformance: only emit MechanismNonDrivingJoint when the
        // structure actually conforms to Joint.  A non-joint StructureRef (e.g.
        // `bind(someBox, v)`) resolved via Path A would otherwise produce a
        // misleading "joint type 'Box' does not satisfy DrivingJoint" message when
        // Box is not a joint at all.  Skipping here keeps the diagnostic precise:
        // "you passed a Joint that is not a DrivingJoint."  The real type-mismatch
        // error (non-joint arg to bind/dim/sweep) is a separate concern for a
        // future generic type-check pass.
        if !satisfies_trait_bound(&tmpl.trait_bounds, "Joint", trait_registry) {
            return; // Not a Joint at all — skip; type-mismatch diagnostics handle it.
        }
        // Check DrivingJoint conformance (transitive through refinements).
        if !satisfies_trait_bound(&tmpl.trait_bounds, "DrivingJoint", trait_registry) {
            diagnostics.push(
                Diagnostic::error(format!(
                    "joint type '{}' does not satisfy DrivingJoint: \
                     bind/dim/sweep require an independent motion variable; \
                     Coupling (derived motion) and Fixed (0-DOF) cannot be driven",
                    type_name
                ))
                .with_code(DiagnosticCode::MechanismNonDrivingJoint)
                .with_label(DiagnosticLabel::new(
                    span,
                    format!("'{}' is not a DrivingJoint", type_name),
                )),
            );
        }
    });
}

#[cfg(test)]
/// # Why these tests live here (and cannot move to `tests/*.rs`)
///
/// All four tests in this module call `pub(crate) check_trait_conformance` via
/// `use super::*;`.  Rust integration-test binaries in `tests/*.rs` are separate
/// crates and can only access `pub` (not `pub(crate)`) items, so none of these
/// tests can be moved to an integration-test file without also making
/// `check_trait_conformance` (and `MergeContext`, `collect_all_requirements`,
/// `check_trait_arg_conformance`) part of the public API — a non-trivial
/// architectural change that would require its own RFC-level task.
///
/// **Tests 1–2** (`check_trait_conformance_resolves_enum_typed_param_and_let`,
/// `option_b_fix_blocks_phantom_let_entry_for_pass2_skipped_name`) hand-build
/// `RequirementKind::Let` fixtures.  `RequirementKind::Let` is **not
/// parser-reachable** from reify source today (see `trait_merge_tests.rs:282`
/// and `let_type_disambiguation_tests.rs:234`), so there is no
/// `compile_source(...)` string that produces this variant.  An integration-level
/// rewrite is therefore impossible, not just inconvenient.
///
/// **Tests 3–4** (`enum_with_type_args_emits_error_diagnostic`,
/// `unknown_named_type_with_type_args_produces_unresolved_diagnostic`) assert an
/// **exact count of 1** on diagnostic substrings.  Under full-pipeline
/// compilation the same diagnostics are also emitted from `entity.rs:329` and
/// `traits.rs:36`, so a `compile_source`-based rewrite would see 2+ emissions
/// and break the exact-count assertions.  Relaxing to `any(...)` would lose the
/// path-specificity that makes these tests load-bearing (they pin that the
/// `conformance/checker.rs` emission site in `check_phase_resolve_structure_members`
/// fires in both debug and release builds).
///
/// **Closest integration-level sibling** that covers the *parser-reachable*
/// scenarios:
/// - `reject_unresolved_type_in_trait_conformance`
///   (`tests/boundary1_consumer.rs:280`)
///
/// For full rationale and alternative paths (structural extraction,
/// test-only feature-flag API, `src/conformance_tests.rs` sibling module)
/// see the escalate_info record for task 2033.
mod tests {
    use super::*;

    /// Run `check_trait_conformance` against the given traits and structure, returning all
    /// diagnostics emitted.
    ///
    /// Thin wrapper over [`run_conformance_with_assoc_fns`] that discards the
    /// populated assoc-fn table — the two differ ONLY in whether they surface
    /// that table, so the shared ~20-line scaffolding (scope/value_cells/
    /// constraints init, registry construction, alias_registry, the 13-arg call
    /// itself) lives in exactly one place and a future signature change to
    /// `check_trait_conformance` is edited once. Each test only needs to build
    /// its trait and structure fixtures and then assert on the returned
    /// `Vec<Diagnostic>`.
    fn run_conformance(
        traits: &[CompiledTrait],
        structure_def: &reify_ast::StructureDef,
        enum_defs: &[reify_ir::EnumDef],
    ) -> Vec<Diagnostic> {
        run_conformance_with_assoc_fns(traits, structure_def, enum_defs).0
    }

    /// RED (task 3939 δ, step-3): a required associated function that the
    /// conforming structure does not provide must surface an
    /// `E_TRAIT_FN_NOT_SATISFIED` (`DiagnosticCode::TraitFnNotSatisfied`)
    /// diagnostic naming the declaring trait and the missing fn.
    ///
    /// Lives here (not in `checker.rs`) because the `run_conformance` harness
    /// and the `pub(crate) check_trait_conformance` entry point are in this
    /// module — integration-test crates can only see `pub` items.
    ///
    /// Fails until step-4 wires the phase-5 `RequirementKind::Fn` satisfaction
    /// check (today `collect_all_requirements` drops Fn requirements and the
    /// phase-5 arm is a placeholder `continue`, so zero diagnostics fire).
    #[test]
    fn required_assoc_fn_not_provided_emits_trait_fn_not_satisfied() {
        // Trait `Shape` requires a bodyless `fn req(self) -> Real`.
        let shape = CompiledTrait {
            name: "Shape".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "req".to_string(),
                kind: RequirementKind::Fn(CompiledAssocFnSig {
                    name: "req".to_string(),
                    has_self: true,
                    params: vec![],
                    return_type: Type::dimensionless_scalar(),
                }),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure `S : Shape { }` provides NO `fn req` member.
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![reify_ast::TraitBoundRef {
                name: "Shape".to_string(),
                type_args: vec![],
                span: SourceSpan::empty(0),
            }],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[shape], &structure_def, &[]);

        let fn_not_satisfied: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnNotSatisfied))
            .collect();
        assert_eq!(
            fn_not_satisfied.len(),
            1,
            "expected exactly one TraitFnNotSatisfied diagnostic for the missing \
             required assoc fn 'req'; got: {:?}",
            diagnostics
        );
        let msg = &fn_not_satisfied[0].message;
        assert!(
            msg.contains("Shape"),
            "diagnostic should name the declaring trait 'Shape'; got: {}",
            msg
        );
        assert!(
            msg.contains("req"),
            "diagnostic should name the missing fn 'req'; got: {}",
            msg
        );
    }

    // --- task 3939 δ step-5/6 fixtures: assoc-fn signature exact-match ---

    /// The implicit `self` receiver: `is_self == true` with the sentinel `self`
    /// named type (per decl.rs:818-823). Excluded from the derived signature's
    /// `params` and recorded as `has_self`.
    fn assoc_self_param() -> reify_ast::FnParam {
        reify_ast::FnParam {
            name: "self".to_string(),
            is_self: true,
            type_expr: reify_ast::TypeExpr {
                kind: reify_ast::TypeExprKind::Named {
                    name: "self".to_string(),
                    type_args: vec![],
                },
                span: SourceSpan::empty(0),
            },
            default: None,
            span: SourceSpan::empty(0),
        }
    }

    /// A non-self param `name : type_name` (bare named type).
    fn assoc_named_param(name: &str, type_name: &str) -> reify_ast::FnParam {
        reify_ast::FnParam {
            name: name.to_string(),
            is_self: false,
            type_expr: reify_ast::TypeExpr {
                kind: reify_ast::TypeExprKind::Named {
                    name: type_name.to_string(),
                    type_args: vec![],
                },
                span: SourceSpan::empty(0),
            },
            default: None,
            span: SourceSpan::empty(0),
        }
    }

    /// A structure-body `fn <name>(<params>) -> <return_type_name> { 0.0 }` member.
    /// The trivial body is irrelevant to signature matching — δ compares only the
    /// receiver / param / return signature.
    fn assoc_fn_member(
        name: &str,
        params: Vec<reify_ast::FnParam>,
        return_type_name: &str,
    ) -> reify_ast::MemberDecl {
        reify_ast::MemberDecl::Fn(reify_ast::FnDef {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            params,
            return_type: Some(reify_ast::TypeExpr {
                kind: reify_ast::TypeExprKind::Named {
                    name: return_type_name.to_string(),
                    type_args: vec![],
                },
                span: SourceSpan::empty(0),
            }),
            body: Some(reify_ast::FnBody {
                let_bindings: vec![],
                result_expr: reify_ast::Expr {
                    kind: reify_ast::ExprKind::NumberLiteral {
                        value: 0.0,
                        is_real: true,
                    },
                    span: SourceSpan::empty(0),
                },
            }),
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            annotations: vec![],
        })
    }

    /// Build trait `Shape` requiring a single bodyless `fn <fn_name>(self) -> Real`.
    fn shape_requiring_fn(fn_name: &str) -> CompiledTrait {
        CompiledTrait {
            name: "Shape".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: fn_name.to_string(),
                kind: RequirementKind::Fn(CompiledAssocFnSig {
                    name: fn_name.to_string(),
                    has_self: true,
                    params: vec![],
                    return_type: Type::dimensionless_scalar(),
                }),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        }
    }

    /// Build structure `S : Shape { <members> }`.
    fn structure_s_conforming_shape(
        members: Vec<reify_ast::MemberDecl>,
    ) -> reify_ast::StructureDef {
        reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![reify_ast::TraitBoundRef {
                name: "Shape".to_string(),
                type_args: vec![],
                span: SourceSpan::empty(0),
            }],
            members,
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        }
    }

    /// RED (task 3939 δ, step-5): a provided assoc fn whose RETURN TYPE differs
    /// from the trait requirement must surface `TraitFnSignatureMismatch`
    /// (§8.8 same-name-different-type) and must NOT also surface
    /// `TraitFnNotSatisfied` — the fn IS present, it is just mis-typed.
    ///
    /// Fails until step-6 adds exact-match signature comparison (today the
    /// phase-5 Fn arm checks the fn NAME only, so a wrong-typed `area` is
    /// silently treated as satisfying the requirement).
    #[test]
    fn provided_assoc_fn_with_wrong_return_type_emits_signature_mismatch() {
        let shape = shape_requiring_fn("area");
        // Structure provides `fn area(self) -> Length { 0.0 }` (Length != Real).
        let structure_def = structure_s_conforming_shape(vec![assoc_fn_member(
            "area",
            vec![assoc_self_param()],
            "Length",
        )]);

        let diagnostics = run_conformance(&[shape], &structure_def, &[]);

        let mismatch: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch))
            .collect();
        assert_eq!(
            mismatch.len(),
            1,
            "expected exactly one TraitFnSignatureMismatch for the wrong-return-type \
             provided fn 'area'; got: {:?}",
            diagnostics
        );
        assert!(
            mismatch[0].message.contains("area"),
            "signature-mismatch diagnostic should name the fn 'area'; got: {}",
            mismatch[0].message
        );

        let not_satisfied: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnNotSatisfied))
            .collect();
        assert!(
            not_satisfied.is_empty(),
            "the fn 'area' is present (just mis-typed) — TraitFnNotSatisfied must NOT \
             fire; got: {:?}",
            diagnostics
        );
    }

    /// RED (task 3939 δ, step-5): a provided assoc fn with the wrong ARITY (an
    /// extra non-self param) is also a signature mismatch. Same assertions as
    /// the return-type case. Fails until step-6.
    #[test]
    fn provided_assoc_fn_with_wrong_arity_emits_signature_mismatch() {
        let shape = shape_requiring_fn("area");
        // Structure provides `fn area(self, extra: Real) -> Real { 0.0 }`
        // (params [Real] != the required []).
        let structure_def = structure_s_conforming_shape(vec![assoc_fn_member(
            "area",
            vec![assoc_self_param(), assoc_named_param("extra", "Real")],
            "Real",
        )]);

        let diagnostics = run_conformance(&[shape], &structure_def, &[]);

        let mismatch: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch))
            .collect();
        assert_eq!(
            mismatch.len(),
            1,
            "expected exactly one TraitFnSignatureMismatch for the wrong-arity \
             provided fn 'area'; got: {:?}",
            diagnostics
        );
        assert!(
            mismatch[0].message.contains("area"),
            "signature-mismatch diagnostic should name the fn 'area'; got: {}",
            mismatch[0].message
        );

        let not_satisfied: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnNotSatisfied))
            .collect();
        assert!(
            not_satisfied.is_empty(),
            "the fn 'area' is present (just wrong arity) — TraitFnNotSatisfied must \
             NOT fire; got: {:?}",
            diagnostics
        );
    }

    // --- task 3939 δ step-7/8 fixtures: assoc-fn table population ---

    /// Build a body-carrying `fn <name>(<params>) -> <return_type_name> { <value> }`
    /// `FnDef`. The distinct `value` lets the override/default tests assert that
    /// override and default bodies compile to different content hashes.
    fn assoc_fn_def(
        name: &str,
        params: Vec<reify_ast::FnParam>,
        return_type_name: &str,
        value: f64,
    ) -> reify_ast::FnDef {
        reify_ast::FnDef {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            params,
            return_type: Some(reify_ast::TypeExpr {
                kind: reify_ast::TypeExprKind::Named {
                    name: return_type_name.to_string(),
                    type_args: vec![],
                },
                span: SourceSpan::empty(0),
            }),
            body: Some(reify_ast::FnBody {
                let_bindings: vec![],
                result_expr: reify_ast::Expr {
                    kind: reify_ast::ExprKind::NumberLiteral {
                        value,
                        is_real: true,
                    },
                    span: SourceSpan::empty(0),
                },
            }),
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            annotations: vec![],
        }
    }

    /// Trait `Shape` with a single default-providing assoc fn
    /// `fn <fn_name>(self) -> Real { <value> }` and NO required members.
    fn shape_with_default_fn(fn_name: &str, value: f64) -> CompiledTrait {
        CompiledTrait {
            name: "Shape".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some(fn_name.to_string()),
                kind: DefaultKind::Fn(assoc_fn_def(
                    fn_name,
                    vec![assoc_self_param()],
                    "Real",
                    value,
                )),
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        }
    }

    /// Sibling of [`run_conformance`] that also surfaces the populated assoc-fn
    /// table so the table-population tests can assert on its contents.
    fn run_conformance_with_assoc_fns(
        traits: &[CompiledTrait],
        structure_def: &reify_ast::StructureDef,
        enum_defs: &[reify_ir::EnumDef],
    ) -> (Vec<Diagnostic>, Vec<CompiledAssocFn>) {
        let entity_ref = EntityDefRef::from(structure_def);
        let trait_registry: HashMap<String, &CompiledTrait> =
            traits.iter().map(|t| (t.name.clone(), t)).collect();
        let trait_names: HashSet<String> = trait_registry.keys().cloned().collect();
        let structure_names: HashSet<String> = HashSet::new();
        let mut scope = CompilationScope::new(&structure_def.name);
        let mut value_cells: Vec<ValueCellDecl> = vec![];
        let mut constraints: Vec<CompiledConstraint> = vec![];
        let mut constraint_index = 0u32;
        let functions: &[CompiledFunction] = &[];
        let alias_registry = TypeAliasRegistry::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut assoc_fns: Vec<CompiledAssocFn> = vec![];
        let mut assoc_types: Vec<CompiledAssocType> = vec![];

        check_trait_conformance(
            &entity_ref,
            &trait_registry,
            &structure_names,
            &trait_names,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            enum_defs,
            functions,
            &alias_registry,
            // task 4497: empty placeholder table — these conformance unit tests
            // exercise trait-default behavior, not ambient injection (the real
            // file-level table is threaded from entities_phase in step-10).
            &AmbientDefaults::default(),
            // task 4639: file scope (no enclosing purpose) for unit tests.
            None,
            &mut diagnostics,
            &mut assoc_fns,
            &mut assoc_types,
        );

        (diagnostics, assoc_fns)
    }

    /// RED (task 3939 δ, step-7a): a trait's default-providing assoc fn is
    /// injected into the conformer's assoc-fn table when the structure does NOT
    /// override it — `is_override == false`, keyed by `(trait, fn)`.
    ///
    /// Fails until step-8 wires `check_phase_resolve_assoc_fns` (today
    /// `DefaultKind::Fn` defaults are dropped during the merge and no phase
    /// populates the out-param, so the table is empty).
    #[test]
    fn default_assoc_fn_injected_into_table_when_not_overridden() {
        let shape = shape_with_default_fn("area", 1.0);
        let structure_def = structure_s_conforming_shape(vec![]); // no override

        let (diagnostics, assoc_fns) =
            run_conformance_with_assoc_fns(&[shape], &structure_def, &[]);

        // A default-providing fn imposes no requirement, so conformance is clean.
        assert!(
            !diagnostics.iter().any(|d| {
                d.code == Some(DiagnosticCode::TraitFnNotSatisfied)
                    || d.code == Some(DiagnosticCode::TraitFnSignatureMismatch)
            }),
            "a default-providing assoc fn should conform cleanly; got: {:?}",
            diagnostics
        );
        assert_eq!(
            assoc_fns.len(),
            1,
            "expected exactly one injected assoc-fn table entry; got: {:?}",
            assoc_fns
        );
        let entry = &assoc_fns[0];
        assert_eq!(
            entry.trait_name, "Shape",
            "entry should be keyed by the declaring trait"
        );
        assert_eq!(
            entry.fn_name, "area",
            "entry should be keyed by the fn name"
        );
        assert!(
            !entry.is_override,
            "a non-overridden default must have is_override == false; got: {:?}",
            entry
        );
        assert_eq!(entry.function.name, "area");
        assert_eq!(
            entry.function.return_type,
            Type::dimensionless_scalar(),
            "the injected default's compiled return type should be Real"
        );
    }

    /// RED (task 3939 δ, step-7b): when the structure overrides a trait's
    /// default-providing assoc fn, the table entry is the OVERRIDE
    /// (`is_override == true`) and its compiled `function.content_hash` differs
    /// from the injected-default's (distinct body ⇒ distinct hash). Fails until
    /// step-8.
    #[test]
    fn override_assoc_fn_beats_default_in_table() {
        // Reference run: no override → the injected default body (1.0).
        let s_default = structure_s_conforming_shape(vec![]);
        let (_d0, assoc_default) =
            run_conformance_with_assoc_fns(&[shape_with_default_fn("area", 1.0)], &s_default, &[]);
        assert_eq!(
            assoc_default.len(),
            1,
            "default run should populate one entry"
        );
        assert!(!assoc_default[0].is_override);

        // Override run: structure provides `fn area(self) -> Real { 2.0 }`.
        let s_override = structure_s_conforming_shape(vec![reify_ast::MemberDecl::Fn(
            assoc_fn_def("area", vec![assoc_self_param()], "Real", 2.0),
        )]);
        let (_d1, assoc_override) =
            run_conformance_with_assoc_fns(&[shape_with_default_fn("area", 1.0)], &s_override, &[]);
        assert_eq!(
            assoc_override.len(),
            1,
            "override run should populate one entry"
        );
        let entry = &assoc_override[0];
        assert_eq!(entry.trait_name, "Shape");
        assert_eq!(entry.fn_name, "area");
        assert!(
            entry.is_override,
            "a structure-provided body must set is_override == true; got: {:?}",
            entry
        );

        // override-beats-default: distinct body (2.0 vs 1.0) ⇒ distinct hash.
        assert_ne!(
            entry.function.content_hash, assoc_default[0].function.content_hash,
            "the override body (2.0) must compile to a different content hash than \
             the injected default body (1.0)"
        );
    }

    /// Regression (task 3939 δ, reviewer amendment): overriding a trait's
    /// default-providing assoc fn with a MISMATCHED signature must surface
    /// `TraitFnSignatureMismatch` and keep the wrongly-typed override OUT of the
    /// dispatch table. A default-only fn produces no `RequirementKind::Fn`, so
    /// phase 5 never checks it — `check_phase_resolve_assoc_fns` is the sole
    /// validation site (PRD §5.4 exact-match-for-overrides). Before the
    /// amendment the bad override landed in the table with no diagnostic.
    #[test]
    fn override_of_default_assoc_fn_with_wrong_signature_emits_mismatch_and_skips_table() {
        // Trait default `fn area(self) -> Real { 1.0 }`.
        let shape = shape_with_default_fn("area", 1.0);
        // Structure overrides with `fn area(self) -> Length { 0.0 }` (Length != Real).
        let s_override = structure_s_conforming_shape(vec![assoc_fn_member(
            "area",
            vec![assoc_self_param()],
            "Length",
        )]);

        let (diagnostics, assoc_fns) = run_conformance_with_assoc_fns(&[shape], &s_override, &[]);

        let mismatch: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch))
            .collect();
        assert_eq!(
            mismatch.len(),
            1,
            "a default override with a wrong return type must emit exactly one \
             TraitFnSignatureMismatch; got: {:?}",
            diagnostics
        );
        assert!(
            mismatch[0].message.contains("area"),
            "signature-mismatch diagnostic should name the fn 'area'; got: {}",
            mismatch[0].message
        );
        // The wrongly-typed override must NOT enter the dispatch table — task ζ
        // would otherwise key dispatch on an entry inconsistent with the error.
        assert!(
            assoc_fns.is_empty(),
            "a signature-mismatched default override must be kept out of the \
             assoc-fn table; got: {:?}",
            assoc_fns
        );
    }

    /// Regression (task 3939 δ, reviewer amendment): a required-fn override whose
    /// return annotation is UNRESOLVABLE must yield exactly one diagnostic — the
    /// `UnresolvedType` from `compile_assoc_function` — with NO accompanying
    /// `TraitFnSignatureMismatch`. The structure-derived sig carries `Type::Error`,
    /// which `assoc_fn_sig_has_error` recognises so the otherwise-spurious mismatch
    /// is suppressed (the sig "differs" only because resolution failed). Locks in
    /// the documented anti-cascade contract (one root cause → one diagnostic) and
    /// confirms the errored override is also kept out of the dispatch table.
    #[test]
    fn required_assoc_fn_override_with_unresolved_annotation_yields_single_diagnostic() {
        // Trait requires bodyless `fn area(self) -> Real`.
        let shape = shape_requiring_fn("area");
        // Structure provides `fn area(self) -> Nonexistent { 0.0 }` — the return
        // annotation does not resolve to any built-in / alias / structure / trait.
        let s_override = structure_s_conforming_shape(vec![assoc_fn_member(
            "area",
            vec![assoc_self_param()],
            "Nonexistent",
        )]);

        let (diagnostics, assoc_fns) = run_conformance_with_assoc_fns(&[shape], &s_override, &[]);

        let unresolved: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::UnresolvedType))
            .collect();
        assert_eq!(
            unresolved.len(),
            1,
            "the unresolved return annotation should produce exactly one \
             UnresolvedType diagnostic; got: {:?}",
            diagnostics
        );
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch)),
            "the Type::Error sig must suppress a spurious TraitFnSignatureMismatch \
             (anti-cascade: one root cause, one diagnostic); got: {:?}",
            diagnostics
        );
        // The fn IS present (just mis-annotated), so absence is not the failure.
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::TraitFnNotSatisfied)),
            "the fn 'area' is present — TraitFnNotSatisfied must NOT fire; got: {:?}",
            diagnostics
        );
        // The errored override must not enter the dispatch table.
        assert!(
            assoc_fns.is_empty(),
            "an errored / sig-mismatched required override must be kept out of the \
             assoc-fn table; got: {:?}",
            assoc_fns
        );
    }

    /// Characterization test that enum-typed `param` and `let` members resolve to
    /// `Type::Enum` through `check_trait_conformance`.
    ///
    /// Serves as a tripwire for the step-4 refactor (HashSet + closure extraction):
    /// any drift in enum resolution or diagnostic messages in the filter_map is caught
    /// immediately.
    ///
    /// ## Why negative assertions?
    ///
    /// `structure_members` is a local binding inside `check_trait_conformance` and is not
    /// directly observable from outside the function.  Rather than restructuring the API,
    /// this test uses three negative-assertion sentinels as a proxy for correct
    /// `Type::Enum("Polarity")` resolution:
    ///
    /// - Absence of **"unresolved type"** → both `dir` and `kind` were resolved (not fallen
    ///   back to `Type::dimensionless_scalar()`)
    /// - Absence of **"type mismatch"** → the resolved types matched the trait's
    ///   `Type::Enum("Polarity")` requirements
    /// - Absence of **"missing required member"** → both members appeared in `structure_members`
    ///
    /// Together these three imply `Type::Enum("Polarity")` was produced.  A regression that
    /// accidentally resolves enum params to `Type::dimensionless_scalar()` would trip "type mismatch", and one
    /// that omits a member from `structure_members` would trip "missing required member".
    #[test]
    fn check_trait_conformance_resolves_enum_typed_param_and_let() {
        // Polarity enum defined in the same module
        let enum_defs = vec![reify_ir::EnumDef {
            name: "Polarity".to_string(),
            variants: vec!["In".into(), "Out".into()],
            doc: None,
            type_params: vec![],
        }];

        // TypeExpr for `Polarity` (bare named type, no type_args)
        let direction_type_expr = reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: "Polarity".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::empty(0),
        };

        // TraitDir: requires `param dir : Polarity` and `let kind : Polarity`
        let trait_dir = CompiledTrait {
            name: "TraitDir".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![
                TraitRequirement {
                    name: "dir".to_string(),
                    kind: RequirementKind::Param(Type::Enum("Polarity".to_string())),
                    span: SourceSpan::empty(0),
                },
                TraitRequirement {
                    name: "kind".to_string(),
                    kind: RequirementKind::Let(Type::Enum("Polarity".to_string())),
                    span: SourceSpan::empty(0),
                },
            ],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure S : TraitDir { param dir : Polarity; let kind : Polarity = 0.0; }
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![reify_ast::TraitBoundRef {
                name: "TraitDir".to_string(),
                type_args: vec![],
                span: SourceSpan::empty(0),
            }],
            members: vec![
                reify_ast::MemberDecl::Param(reify_ast::ParamDecl {
                    name: "dir".to_string(),
                    doc: None,
                    is_priv: false,
                    type_expr: Some(direction_type_expr.clone()),
                    default: None,
                    where_clause: None,
                    annotations: vec![],
                    span: SourceSpan::empty(0),
                    content_hash: ContentHash(0),
                }),
                reify_ast::MemberDecl::Let(reify_ast::LetDecl {
                    name: "kind".to_string(),
                    doc: None,
                    is_pub: false,
                    is_priv: false,
                    is_aux: false,
                    type_expr: Some(direction_type_expr),
                    value: reify_ast::Expr {
                        kind: reify_ast::ExprKind::NumberLiteral {
                            value: 0.0,
                            is_real: false,
                        },
                        span: SourceSpan::empty(0),
                    },
                    where_clause: None,
                    annotations: vec![],
                    span: SourceSpan::empty(0),
                    content_hash: ContentHash(0),
                }),
            ],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[trait_dir], &structure_def, &enum_defs);

        // No "unresolved type" → both dir and kind resolved successfully (to Type::Enum)
        let unresolved_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("unresolved type"))
            .collect();
        assert!(
            unresolved_diags.is_empty(),
            "Expected no 'unresolved type' diagnostics; got: {:?}",
            diagnostics
        );

        // No "type mismatch" → both resolved to Type::Enum("Polarity"), satisfying the trait
        let mismatch_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("type mismatch"))
            .collect();
        assert!(
            mismatch_diags.is_empty(),
            "Expected no 'type mismatch' diagnostics; got: {:?}",
            diagnostics
        );

        // No "missing required member" → both dir and kind were found in structure_members
        let missing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("missing required member"))
            .collect();
        assert!(
            missing_diags.is_empty(),
            "Expected no 'missing required member' diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// Unit test for the Option B fix (task 1951).
    ///
    /// This test exercises a code path no `compile_source`-based integration test can
    /// reach today: it hand-builds a `RequirementKind::Let` requirement (not parseable
    /// from reify source — see `let_type_disambiguation_tests.rs:470-497` and
    /// esc-1951-6) and verifies that the Option B guard in `available_defaults`
    /// suppresses the phantom `(name, Let) -> Type::dimensionless_scalar()` entry for names recorded in
    /// `pass2_skipped`.
    ///
    /// ## Scenario
    ///
    /// - **TraitX**: requires `let x : Length` (hand-built `RequirementKind::Let` — not
    ///   parser-reachable today)
    /// - **TraitY**: provides `param x : Length` — Pass 1 claims the scope slot for "x"
    /// - **TraitZ**: provides `let x = 5.5` (unannotated; `cell_type: None`) — Pass 2
    ///   sees the slot already claimed and records "x" in `pass2_skipped`
    /// - **Structure S : TraitX + TraitY + TraitZ { }** — no member override
    ///
    /// ## Expected behavior (post-fix)
    ///
    /// The `pass2_skipped.contains(name)` guard in the `DefaultKind::Let` arm of
    /// `available_defaults` returns `None` before reaching the `Type::dimensionless_scalar()` fallback.
    /// The `RequirementKind::Let` lookup for "x" finds no entry → the `None` arm fires →
    /// correct "missing required member" diagnostic (not the spurious "available default
    /// has Real" phantom type-mismatch).
    ///
    /// ## Pre-fix behavior (should NOT happen after fix)
    ///
    /// Without the guard, `available_defaults` contained `("x", Let) -> Type::dimensionless_scalar()`.
    /// The lookup found it, `implicitly_converts_to(Real, Length)` was false, and a
    /// spurious "requirement expects …, available default has Real" diagnostic was emitted.
    #[test]
    fn option_b_fix_blocks_phantom_let_entry_for_pass2_skipped_name() {
        // --- Build CompiledTrait fixtures ---

        // TraitX: requires `let x : Length` (hand-built — not parser-reachable)
        let trait_x = CompiledTrait {
            name: "TraitX".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "x".to_string(),
                kind: RequirementKind::Let(Type::length()),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // TraitY: `param x : Length` — no default expression needed.
        // Pass 1 registers "x" → Type::length() in the scope.
        let trait_y = CompiledTrait {
            name: "TraitY".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Param {
                    cell_type: Type::length(),
                    default_decl: reify_ast::ParamDecl {
                        name: "x".to_string(),
                        doc: None,
                        is_priv: false,
                        type_expr: None,
                        default: None, // no default expression
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // TraitZ: `let x = 5.5` (unannotated; cell_type: None).
        // Pass 2 compiles NumberLiteral(5.5) → Type::dimensionless_scalar(), finds "x" already in scope,
        // and records "x" in pass2_skipped (no inferred_let_exprs cache entry).
        let trait_z = CompiledTrait {
            name: "TraitZ".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl: reify_ast::LetDecl {
                        name: "x".to_string(),
                        doc: None,
                        is_pub: false,
                        is_priv: false,
                        is_aux: false,
                        type_expr: None,
                        value: reify_ast::Expr {
                            kind: reify_ast::ExprKind::NumberLiteral {
                                value: 5.5,
                                is_real: true,
                            },
                            span: SourceSpan::empty(0),
                        },
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure S : TraitX + TraitY + TraitZ { } — no member overrides
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![
                reify_ast::TraitBoundRef {
                    name: "TraitX".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
                reify_ast::TraitBoundRef {
                    name: "TraitY".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
                reify_ast::TraitBoundRef {
                    name: "TraitZ".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
            ],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[trait_x, trait_y, trait_z], &structure_def, &[]);

        // --- Assertion 1: no phantom type-mismatch diagnostic ---
        // Pre-fix: `available_defaults` had `("x", Let) -> Real`; the
        // RequirementKind::Let lookup found it, `implicitly_converts_to(Real, Length)` was
        // false, and a spurious "requirement expects …, available default has Real"
        // diagnostic was emitted.
        // Post-fix: no phantom entry → this filter collects nothing.
        // Filter uses the quoted member name `"'x'"` — diagnostic templates surround the
        // member name with single quotes (`'{}'`), so `"'x'"` rejects incidental `x`
        // letters in prose like "expr", "exists", "mismatch" (same pattern as
        // `inferred_let_expr_incompatible_with_let_requirement` at mod.rs:993).
        let phantom_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.message.contains("available default")
                    && d.message.contains("Real")
                    && d.message.contains("'x'")
            })
            .collect();
        assert!(
            phantom_diags.is_empty(),
            "Option B fix violated: phantom `(x, Let) -> Type::dimensionless_scalar()` advertisement caused \
             a spurious type-mismatch diagnostic. Expected no phantom diagnostic. Got: {:?}",
            phantom_diags
        );

        // --- Assertion 2: correct "missing required member" diagnostic IS present ---
        // With the phantom entry absent, the None arm of the available_defaults lookup
        // fires and emits the correct "missing required member" diagnostic.
        // Filter uses the quoted member name `"'x'"` to avoid matching incidental `x`
        // letters in diagnostic prose.
        let missing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("missing required member") && d.message.contains("'x'"))
            .collect();
        assert_eq!(
            missing_diags.len(),
            1,
            "Expected exactly one 'missing required member' diagnostic for 'x' (Option B fix). \
             Got: {:?}",
            diagnostics
        );
    }

    /// Phase-level guard test: the narrowed `pass2_skipped` guard in
    /// `check_phase_build_available_defaults_map` preserves an annotated-Let advertisement
    /// when a sibling unannotated Let for the same name populated `pass2_skipped`.
    ///
    /// ## Why this is not reachable via `run_conformance`
    ///
    /// The target scenario — two `Let` defaults (annotated + unannotated) for the same name
    /// coexisting in `ctx.defaults` with `pass2_skipped` containing the name — is not
    /// reachable through `run_conformance` / `check_trait_conformance` end-to-end.
    /// `collect_all_requirements` in `trait_requirements.rs` deduplicates `Let` defaults by
    /// name via `seen_let_hashes`, so only one of the two ever survives into `ctx.defaults`.
    /// Whichever survives, the other pathway that populates `pass2_skipped` is never taken.
    ///
    /// This test is therefore *defensive*: it protects against future regressions where
    /// deduplication relaxes, a new code path writes to `pass2_skipped` while annotated Let
    /// defaults remain in `ctx.defaults`, or a parser extension introduces a new way to
    /// generate the combination. It drives the affected phases directly with a hand-built
    /// `MergeContext` that bypasses `collect_all_requirements`.
    ///
    /// ## Scenario (hand-built `MergeContext`)
    ///
    /// - **Annotated Let**: `let x : Length` (`cell_type: Some(Type::length())`) — Pass 1
    ///   claims the scope slot for "x" with `Type::length()`.
    /// - **Unannotated Let**: `let x = 5.5` (`cell_type: None`) — Pass 2 finds "x" already
    ///   in scope, compiles the expression successfully, but records "x" in `pass2_skipped`
    ///   (slot already claimed by Pass 1).
    /// - **Requirement**: `let x : Length` (`RequirementKind::Let(Type::length())`) —
    ///   hand-built because `RequirementKind::Let` is not parser-reachable today.
    ///
    /// ## Expected behavior (post-guard-narrowing)
    ///
    /// The narrowed guard (`if cell_type.is_none() && pass2_skipped.contains(name)`) fires
    /// only for the unannotated Let (cell_type: None). The annotated Let (cell_type: Some(_))
    /// passes through and is advertised as `("x", Let) -> Type::length()`.
    /// `check_phase_check_members_against_requirements` finds the advertisement and the
    /// requirement is satisfied — no diagnostics emitted.
    ///
    /// ## Failure behavior under the over-broad guard (current code before step 2)
    ///
    /// The over-broad guard (`if pass2_skipped.contains(name)`) fires on **both** Let
    /// entries (annotated and unannotated) because `pass2_skipped` contains "x". Both are
    /// dropped → `available_defaults` is empty → the `None` arm fires and emits a spurious
    /// "missing required member 'x'" diagnostic. Assertion 1 (`Some(&Type::length())`) and
    /// assertion 2 (no missing-member) both fail.
    ///
    /// ## Cross-references
    ///
    /// - Sibling test: `option_b_fix_blocks_phantom_let_entry_for_pass2_skipped_name`
    ///   covers the `pass2_skipped`-triggered-by-Param scenario.
    /// - Guard site: `check_phase_build_available_defaults_map` in `checker.rs`, the
    ///   `DefaultKind::Let` arm, at the `pass2_skipped` exclusion block.
    #[test]
    fn option_b_guard_preserves_annotated_let_advertisement_when_sibling_unannotated_let_is_pass2_skipped()
     {
        // --- Build LetDecl fixtures ---
        //
        // Annotated Let: `let x : Length = 1.0` — cell_type: Some(Type::length())
        // Pass 1 claims "x" -> Type::length() in the scope (uses cell_type directly;
        // the value expression is compiled later, in phase 6's inject_defaults).
        let annotated_let_decl = reify_ast::LetDecl {
            name: "x".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None, // cell_type carries the annotation; type_expr is the raw AST form
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 1.0,
                    is_real: false,
                },
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };
        // Unannotated Let: `let x = 5.5` — cell_type: None
        // Pass 2 compiles NumberLiteral(5.5) → Type::dimensionless_scalar(), finds "x" already in scope
        // (from the annotated Let above), and records "x" in pass2_skipped.
        let unannotated_let_decl = reify_ast::LetDecl {
            name: "x".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None,
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 5.5,
                    is_real: true,
                },
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![
            // Annotated Let — Pass 1 claims "x" -> Type::length()
            TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: Some(Type::length()),
                    let_decl: annotated_let_decl,
                },
                span: SourceSpan::empty(0),
            },
            // Unannotated Let — Pass 2 finds "x" already claimed -> records in pass2_skipped
            TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl: unannotated_let_decl,
                },
                span: SourceSpan::empty(0),
            },
        ];
        // RequirementKind::Let is not parser-reachable today; hand-built here.
        ctx.requirements = vec![TraitRequirement {
            name: "x".to_string(),
            kind: RequirementKind::Let(Type::length()),
            span: SourceSpan::empty(0),
        }];

        // Minimal structure scaffolding (no members — requirement must be satisfied by default).
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let structure_all_members: HashMap<String, Type> = HashMap::new();
        let mut scope = CompilationScope::new("S");
        let mut diagnostics: Vec<Diagnostic> = vec![];

        // --- Phase 3: pre-register default types ---
        let pre = check_phase_pre_register_default_types(
            &ctx,
            &structure_all_members,
            "S",
            &mut scope,
            &[],
            &[],
            &mut diagnostics,
        );

        // Sanity: pass2_skipped must contain "x" for the guard path to be exercised.
        // If this fails, the fixture is misconfigured and would not test the guard at all.
        assert!(
            pre.pass2_skipped.contains("x"),
            "Fixture sanity failed: expected 'x' in pass2_skipped after Pass 2 found its scope \
             slot already claimed by the annotated Let; the guard path would not be exercised. \
             Got pass2_skipped = {:?}",
            pre.pass2_skipped
        );

        // --- Phase 4: build available defaults map ---
        let available_defaults = check_phase_build_available_defaults_map(
            &ctx,
            &pre.inferred_let_exprs,
            &pre.pass1_skipped,
            &pre.pass1_param_skipped,
            &pre.pass2_skipped,
            &pre.pass2_compile_errors,
        );

        // --- Phase 5: check members against requirements (no structure members) ---
        let empty_param_members: HashMap<String, Type> = HashMap::new();
        let empty_let_members: HashMap<String, Type> = HashMap::new();
        check_phase_check_members_against_requirements(
            &ctx,
            &entity_ref,
            &empty_param_members,
            &empty_let_members,
            &available_defaults,
            // No structure assoc fns in this test.
            &HashMap::<String, CompiledAssocFnSig>::new(),
            // No structure assoc type bindings in this test.
            &HashMap::<String, Type>::new(),
            &mut diagnostics,
        );

        // --- Assertion 1 (positive): annotated-Let advertisement preserved ---
        // Under the over-broad guard this returns None (both Lets dropped);
        // under the narrowed guard it returns Some(&Type::length()).
        assert_eq!(
            available_defaults.get(&("x".to_string(), AvailableDefaultKind::Let)),
            Some(&Type::length()),
            "narrowed guard must preserve the annotated-Let advertisement even when a sibling \
             unannotated Let populated pass2_skipped for the same name; the over-broad guard \
             would drop both Lets and return None here"
        );

        // --- Assertion 2 (negative): no spurious 'x' missing-member diagnostic ---
        // Under the over-broad guard the None arm fires and emits "missing required member 'x'".
        // Under the narrowed guard the annotated-Let advertisement satisfies the requirement.
        let missing_x_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("missing required member") && d.message.contains("'x'"))
            .collect();
        assert!(
            missing_x_diags.is_empty(),
            "narrowed guard must not emit 'missing required member' for 'x': \
             the annotated Let provides Type::length() which satisfies \
             RequirementKind::Let(Length). Got: {:?}",
            missing_x_diags
        );

        // --- Assertion 3 (negative): no type-mismatch cascade for 'x' ---
        // Type::length() satisfies RequirementKind::Let(Length) via implicitly_converts_to,
        // so no "available default has" cascade should fire.
        let mismatch_x_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("available default") && d.message.contains("'x'"))
            .collect();
        assert!(
            mismatch_x_diags.is_empty(),
            "no type-mismatch cascade for 'x': Type::length() satisfies \
             RequirementKind::Let(Length). Got: {:?}",
            mismatch_x_diags
        );
    }

    /// Test that a `param` annotation with `EnumName<T>` (non-empty type_args) emits a
    /// user-facing `Diagnostic::error` with the message
    /// "enum `Polarity` does not accept type arguments".
    ///
    /// Unlike a `debug_assert!`, the diagnostic is emitted in both debug and release builds,
    /// so this test validates the error is always surfaced to users regardless of build profile.
    #[test]
    fn enum_with_type_args_emits_error_diagnostic() {
        // Polarity<Something> — non-empty type_args that should trigger the diagnostic
        let bogus_type_arg = reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: "Something".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::empty(0),
        };
        let direction_with_args = reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: "Polarity".to_string(),
                type_args: vec![bogus_type_arg],
            },
            span: SourceSpan::empty(0),
        };

        let enum_defs = vec![reify_ir::EnumDef {
            name: "Polarity".to_string(),
            variants: vec!["In".into(), "Out".into()],
            doc: None,
            type_params: vec![],
        }];

        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![reify_ast::MemberDecl::Param(reify_ast::ParamDecl {
                name: "dir".to_string(),
                doc: None,
                is_priv: false,
                type_expr: Some(direction_with_args),
                default: None,
                where_clause: None,
                annotations: vec![],
                span: SourceSpan::empty(0),
                content_hash: ContentHash(0),
            })],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[], &structure_def, &enum_defs);

        // Expect exactly one diagnostic reporting the type-args error.
        let type_args_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("does not accept type arguments"))
            .collect();
        assert_eq!(
            type_args_errors.len(),
            1,
            "Expected exactly one 'does not accept type arguments' diagnostic; got: {:?}",
            diagnostics
        );
    }

    /// A non-enum type name with non-empty type_args (e.g. `NotAnEnum<Something>`) should
    /// produce exactly one "unresolved type" diagnostic — the same outcome as `NotAnEnum`
    /// without type_args, because enum-resolution is gated on the name matching an enum.
    ///
    /// The positive assertion (`unresolved.len() == 1`) is the load-bearing check here:
    /// it verifies that an unknown parameterized type name falls through to the
    /// "unresolved type" diagnostic rather than silently resolving to `Type::dimensionless_scalar()` or
    /// emitting a spurious "does not accept type arguments" error.
    #[test]
    fn unknown_named_type_with_type_args_produces_unresolved_diagnostic() {
        // NotAnEnum<Something> — non-empty type_args but "NotAnEnum" is not in enum_defs
        let bogus_type_arg = reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: "Something".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::empty(0),
        };
        let non_enum_with_args = reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: "NotAnEnum".to_string(),
                type_args: vec![bogus_type_arg],
            },
            span: SourceSpan::empty(0),
        };

        let enum_defs = vec![reify_ir::EnumDef {
            name: "Polarity".to_string(),
            variants: vec!["In".into(), "Out".into()],
            doc: None,
            type_params: vec![],
        }];

        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![reify_ast::MemberDecl::Param(reify_ast::ParamDecl {
                name: "p".to_string(),
                doc: None,
                is_priv: false,
                type_expr: Some(non_enum_with_args),
                default: None,
                where_clause: None,
                annotations: vec![],
                span: SourceSpan::empty(0),
                content_hash: ContentHash(0),
            })],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        // Should NOT panic — "NotAnEnum" is not in enum_defs, so the enum-match arm
        // (where the debug_assert lives) is never taken.
        let diagnostics = run_conformance(&[], &structure_def, &enum_defs);

        // The unknown type produces an "unresolved type" diagnostic — not a panic.
        let unresolved: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("unresolved type"))
            .collect();
        assert_eq!(
            unresolved.len(),
            1,
            "Expected exactly one 'unresolved type' diagnostic"
        );
    }

    /// Pins the `inferred_let_exprs.get(name)` fallback at conformance.rs:358-363
    /// and the `Some(default_type) if implicitly_converts_to(...)` satisfaction arm
    /// at conformance.rs:406-410.
    ///
    /// `RequirementKind::Let` is not parser-reachable from reify source today
    /// (see `let_with_type_and_no_value_parses_as_empty_trait` and
    /// `let_type_disambiguation_tests.rs:470-497`), so only hand-built fixtures
    /// reach this path.
    ///
    /// ## Scenario
    ///
    /// - **TraitA**: requires `let x : Length` (hand-built `RequirementKind::Let` — not
    ///   parser-reachable)
    /// - **TraitB**: provides unannotated `let x = 80mm` (`DefaultKind::Let { cell_type: None,
    ///   let_decl.value: QuantityLiteral { 80.0, "mm" } }`) — Pass 2 infers `Type::length()`
    ///   and caches it in `inferred_let_exprs`
    /// - **Structure S : TraitA + TraitB { }** — no member overrides
    ///
    /// ## Expected behavior
    ///
    /// The `available_defaults` builder falls back to `inferred_let_exprs.get("x")`
    /// → `Type::length()`. The `Some(default_type) if implicitly_converts_to(...)` arm
    /// finds the types compatible → requirement satisfied → no diagnostics.
    #[test]
    fn inferred_let_expr_satisfies_let_requirement() {
        // TraitA: requires `let x : Length` (hand-built — not parser-reachable)
        let trait_a = CompiledTrait {
            name: "TraitA".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "x".to_string(),
                kind: RequirementKind::Let(Type::length()),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // TraitB: `let x = 80mm` (unannotated; cell_type: None).
        // Pass 2 compiles QuantityLiteral { value: 80.0, unit: "mm" } →
        // Type::Scalar { dimension: LENGTH } = Type::length(), finds "x" vacant in scope,
        // caches in inferred_let_exprs.
        let trait_b = CompiledTrait {
            name: "TraitB".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl: reify_ast::LetDecl {
                        name: "x".to_string(),
                        doc: None,
                        is_pub: false,
                        is_priv: false,
                        is_aux: false,
                        type_expr: None,
                        value: reify_ast::Expr {
                            kind: reify_ast::ExprKind::QuantityLiteral {
                                value: 80.0,
                                unit: reify_ast::UnitExpr::Unit("mm".to_string()),
                            },
                            span: SourceSpan::empty(0),
                        },
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure S : TraitA + TraitB { } — no member overrides
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![
                reify_ast::TraitBoundRef {
                    name: "TraitA".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
                reify_ast::TraitBoundRef {
                    name: "TraitB".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
            ],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[trait_a, trait_b], &structure_def, &[]);

        // A clean satisfaction path produces zero diagnostics.  Using is_empty() rather than
        // filtered substring checks means any unrelated upstream failure (e.g. a silent
        // compile_expr error) also trips this assertion — making it load-bearing beyond just
        // the two previously-checked categories ("type mismatch" / "missing required member").
        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics: inferred Type::length() should satisfy \
             RequirementKind::Let(Length) via the `Some(default_type) if \
             implicitly_converts_to(...)` arm at conformance.rs:406-410; got: {:?}",
            diagnostics
        );
    }

    /// Pins the `Some(default_type) =>` type-mismatch branch at conformance.rs:411-423
    /// for the `RequirementKind::Let` path when the inferred-let type is incompatible.
    ///
    /// `implicitly_converts_to(Type::dimensionless_scalar(), Type::length())` is false — `Real` and
    /// `Scalar { LENGTH }` are distinct types with no implicit conversion
    /// (type_compat.rs:3-96).
    ///
    /// ## Scenario
    ///
    /// Identical to `inferred_let_expr_satisfies_let_requirement` except the let
    /// expression is `ExprKind::NumberLiteral(5.5)` (inferred `Type::dimensionless_scalar()`)
    /// instead of `QuantityLiteral { 80.0, "mm" }`.
    ///
    /// ## Expected behavior
    ///
    /// `available_defaults` advertises `("x", Let) -> Type::dimensionless_scalar()` (via the
    /// `inferred_let_exprs.get("x")` fallback). The `Some(default_type) =>` arm
    /// fires → exactly one "type mismatch" + "available default" + "x" diagnostic.
    /// No "missing required member" for "x" (the default IS present in
    /// `available_defaults`, just with an incompatible type).
    #[test]
    fn inferred_let_expr_incompatible_with_let_requirement() {
        // TraitA: requires `let x : Length` (hand-built — not parser-reachable)
        let trait_a = CompiledTrait {
            name: "TraitA".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "x".to_string(),
                kind: RequirementKind::Let(Type::length()),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // TraitB: `let x = 5.5` (unannotated; cell_type: None).
        // Pass 2 compiles NumberLiteral(5.5) → Type::dimensionless_scalar(), finds "x" vacant in scope,
        // caches in inferred_let_exprs.
        let trait_b = CompiledTrait {
            name: "TraitB".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl: reify_ast::LetDecl {
                        name: "x".to_string(),
                        doc: None,
                        is_pub: false,
                        is_priv: false,
                        is_aux: false,
                        type_expr: None,
                        value: reify_ast::Expr {
                            kind: reify_ast::ExprKind::NumberLiteral {
                                value: 5.5,
                                is_real: true,
                            },
                            span: SourceSpan::empty(0),
                        },
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure S : TraitA + TraitB { } — no member overrides
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![
                reify_ast::TraitBoundRef {
                    name: "TraitA".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
                reify_ast::TraitBoundRef {
                    name: "TraitB".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
            ],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[trait_a, trait_b], &structure_def, &[]);

        // Assertion 1: exactly one "type mismatch" + "available default" + "'x'" diagnostic.
        // Using "'x'" (quoted member name as it appears in the diagnostic template at
        // conformance.rs:415) rather than bare 'x' avoids false matches on words like
        // "expects" that also contain the character.  This pins the `Some(default_type) =>`
        // branch at conformance.rs:411-423.
        let mismatch: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.message.contains("type mismatch")
                    && d.message.contains("available default")
                    && d.message.contains("'x'")
            })
            .collect();
        assert_eq!(
            mismatch.len(),
            1,
            "expected exactly one type-mismatch diagnostic from the `Some(default_type) =>` \
             branch; got: {:?}",
            diagnostics
        );

        // Assertion 2: no "missing required member" for "'x'" (quoted, same rationale).
        // The inferred_let_exprs fallback advertised `("x", Let)` so the None arm was
        // never reached — the default IS present in available_defaults, just with an
        // incompatible type.
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("missing required member") && d.message.contains("'x'")),
            "negative case should hit the Some(default_type) arm, not the None arm; \
             got: {:?}",
            diagnostics
        );
    }

    /// Phase-contract test for `check_phase_resolve_structure_members`.
    ///
    /// Verifies that the helper correctly builds both the `structure_members`
    /// HashMap and the `structure_constraint_labels` HashSet from a minimal
    /// StructureDef fixture. This test fails to compile until the helper exists
    /// (TDD compile-tripwire) and pins the helper's return type signature.
    #[test]
    fn check_phase_resolve_structure_members_builds_member_and_constraint_maps() {
        let real_type_expr = reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: "Real".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::empty(0),
        };
        let length_type_expr = reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: "Length".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::empty(0),
        };

        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![
                reify_ast::MemberDecl::Param(reify_ast::ParamDecl {
                    name: "width".to_string(),
                    doc: None,
                    is_priv: false,
                    type_expr: Some(real_type_expr),
                    default: None,
                    where_clause: None,
                    annotations: vec![],
                    span: SourceSpan::empty(0),
                    content_hash: ContentHash(0),
                }),
                reify_ast::MemberDecl::Let(reify_ast::LetDecl {
                    name: "length".to_string(),
                    doc: None,
                    is_pub: false,
                    is_priv: false,
                    is_aux: false,
                    type_expr: Some(length_type_expr),
                    value: reify_ast::Expr {
                        kind: reify_ast::ExprKind::NumberLiteral {
                            value: 0.0,
                            is_real: false,
                        },
                        span: SourceSpan::empty(0),
                    },
                    where_clause: None,
                    annotations: vec![],
                    span: SourceSpan::empty(0),
                    content_hash: ContentHash(0),
                }),
                reify_ast::MemberDecl::Constraint(reify_ast::ConstraintDecl {
                    label: Some("bound".to_string()),
                    expr: reify_ast::Expr {
                        kind: reify_ast::ExprKind::NumberLiteral {
                            value: 1.0,
                            is_real: false,
                        },
                        span: SourceSpan::empty(0),
                    },
                    where_clause: None,
                    is_priv: false,
                    span: SourceSpan::empty(0),
                    content_hash: ContentHash(0),
                }),
            ],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let entity_ref = EntityDefRef::from(&structure_def);
        let structure_names: HashSet<String> = HashSet::new();
        let trait_names: HashSet<String> = HashSet::new();
        let alias_registry = TypeAliasRegistry::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let empty_assoc_scope: HashMap<String, Type> = HashMap::new();
        let empty_declared_assoc: HashSet<String> = HashSet::new();
        let (structure_param_members, structure_let_members, structure_constraint_labels) =
            check_phase_resolve_structure_members(
                &entity_ref,
                &structure_names,
                &trait_names,
                &[],
                &alias_registry,
                &empty_assoc_scope,
                &empty_declared_assoc,
                &mut diagnostics,
            );

        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics; got: {:?}",
            diagnostics
        );
        assert!(
            structure_param_members.contains_key("width"),
            "Expected 'width' in structure_param_members"
        );
        assert!(
            structure_let_members.contains_key("length"),
            "Expected 'length' in structure_let_members"
        );
        assert!(
            structure_constraint_labels.contains("bound"),
            "Expected 'bound' in structure_constraint_labels"
        );
    }

    /// Phase-contract test for `check_phase_collect_trait_bounds`.
    ///
    /// Verifies that the helper populates a MergeContext with the trait requirements
    /// from the structure's trait bounds. This test fails to compile until the helper
    /// exists (TDD compile-tripwire) and pins the helper's return type signature.
    #[test]
    fn check_phase_collect_trait_bounds_populates_ctx_requirements() {
        let trait_a = CompiledTrait {
            name: "TraitA".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "w".to_string(),
                kind: RequirementKind::Param(Type::dimensionless_scalar()),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![reify_ast::TraitBoundRef {
                name: "TraitA".to_string(),
                type_args: vec![],
                span: SourceSpan::empty(0),
            }],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let entity_ref = EntityDefRef::from(&structure_def);
        let trait_registry: HashMap<String, &CompiledTrait> =
            [("TraitA".to_string(), &trait_a)].into_iter().collect();
        let structure_members: HashMap<String, Type> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let ctx = check_phase_collect_trait_bounds(
            &entity_ref,
            &trait_registry,
            &structure_members,
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics; got: {:?}",
            diagnostics
        );
        assert_eq!(ctx.requirements.len(), 1, "Expected 1 requirement");
        assert_eq!(
            ctx.requirements[0].name, "w",
            "Expected requirement name 'w'"
        );
    }

    /// Phase-contract test for `check_phase_pre_register_default_types`.
    ///
    /// Verifies that the helper registers an annotated Param default into the scope
    /// (Pass 1) and returns empty caches (no unannotated Let defaults to process).
    /// This test fails to compile until the helper exists (TDD compile-tripwire) and
    /// pins the helper's `PreRegisterOutput` return type.
    #[test]
    fn check_phase_pre_register_default_types_registers_annotated_param_into_scope() {
        let param_decl = reify_ast::ParamDecl {
            name: "x".to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![TraitDefault {
            name: Some("x".to_string()),
            kind: DefaultKind::Param {
                cell_type: Type::dimensionless_scalar(),
                default_decl: param_decl,
            },
            span: SourceSpan::empty(0),
        }];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let mut scope = CompilationScope::new("S");
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let out = check_phase_pre_register_default_types(
            &ctx,
            &structure_members,
            "S",
            &mut scope,
            &[],
            &[],
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics; got: {:?}",
            diagnostics
        );
        assert!(
            out.inferred_let_exprs.is_empty(),
            "Expected no inferred_let_exprs for a param-only context"
        );
        // Negative control: a param-only fixture must never populate pass1_skipped
        // (no annotated-Let losers can exist without an annotated Let in ctx.defaults).
        assert!(
            out.pass1_skipped.is_empty(),
            "Expected pass1_skipped to be empty for a param-only context; \
             got: {:?}",
            out.pass1_skipped
        );
        // Negative control: a param-only fixture (single Param, no competing annotated Let)
        // must never populate pass1_param_skipped (no Param can lose without a prior annotated Let).
        assert!(
            out.pass1_param_skipped.is_empty(),
            "Expected pass1_param_skipped to be empty for a param-only context; \
             got: {:?}",
            out.pass1_param_skipped
        );
        assert!(
            out.pass2_skipped.is_empty(),
            "Expected no pass2_skipped for a param-only context"
        );
        assert!(
            out.pass2_compile_errors.is_empty(),
            "Expected no pass2_compile_errors for a param-only context"
        );
        // Verify "x" was registered in scope: a second register_if_absent call for "x"
        // should find it occupied (Some(..)) — no direct lookup API needed.
        let conflict = scope.register_if_absent("x", Type::Int);
        assert!(
            conflict.is_some(),
            "Expected 'x' to be registered in scope (register_if_absent should find it occupied)"
        );
    }

    /// Phase-contract test: Pass 2 of `check_phase_pre_register_default_types` caches an
    /// unannotated Let's compiled expression into `inferred_let_exprs`.
    ///
    /// When a `DefaultKind::Let { cell_type: None }` default is present and its scope slot
    /// is not yet claimed (no preceding annotated default for the same name), Pass 2 should
    /// compile the expression, register the inferred type in scope, and store the compiled
    /// expression in `inferred_let_exprs` for phase 6 to reuse without double-compilation.
    #[test]
    fn check_phase_pre_register_default_types_caches_unannotated_let_in_inferred_map() {
        let let_decl = reify_ast::LetDecl {
            name: "y".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None, // unannotated — must go through Pass 2 inference
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 2.5,
                    is_real: true,
                },
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![TraitDefault {
            name: Some("y".to_string()),
            kind: DefaultKind::Let {
                cell_type: None,
                let_decl,
            },
            span: SourceSpan::empty(0),
        }];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let mut scope = CompilationScope::new("S");
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let out = check_phase_pre_register_default_types(
            &ctx,
            &structure_members,
            "S",
            &mut scope,
            &[],
            &[],
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics for a simple number literal; got: {:?}",
            diagnostics
        );
        // Negative control: an unannotated-Let-only fixture must never populate
        // pass1_skipped (no annotated-Let loser can exist without a competing
        // annotated default earlier in ctx.defaults).
        assert!(
            out.pass1_skipped.is_empty(),
            "Expected pass1_skipped to be empty for a unannotated-let-only context; \
             got: {:?}",
            out.pass1_skipped
        );
        // Negative control: an unannotated-Let-only fixture (no Param, no annotated Let)
        // must never populate pass1_param_skipped.
        assert!(
            out.pass1_param_skipped.is_empty(),
            "Expected pass1_param_skipped to be empty for an unannotated-let-only context; \
             got: {:?}",
            out.pass1_param_skipped
        );
        assert!(
            out.pass2_skipped.is_empty(),
            "Expected no pass2_skipped when no scope collision occurred"
        );
        assert!(
            out.pass2_compile_errors.is_empty(),
            "Expected no pass2_compile_errors for a successful compilation"
        );
        assert!(
            out.inferred_let_exprs
                .contains_key(&("y".to_string(), AvailableDefaultKind::Let)),
            "Expected composite key ('y', Let) in inferred_let_exprs after Pass 2 compiled the unannotated let"
        );
        assert_eq!(
            out.inferred_let_exprs[&("y".to_string(), AvailableDefaultKind::Let)].result_type,
            Type::dimensionless_scalar(),
            "Expected Type::dimensionless_scalar() for a floating-point number literal 2.5"
        );
    }

    /// Phase-contract test: Pass 2 of `check_phase_pre_register_default_types` records
    /// the name in `pass2_skipped` (not `inferred_let_exprs`) when the scope slot was
    /// already claimed by a Pass 1 annotated default.
    ///
    /// When a `DefaultKind::Param` (or annotated `DefaultKind::Let`) for name "x" is
    /// processed first in Pass 1, the scope slot for "x" is claimed. A subsequent
    /// unannotated `DefaultKind::Let` for the same name in Pass 2 sees the slot occupied
    /// and must record "x" in `pass2_skipped` (preventing duplicate cell injection in
    /// phase 6) without inserting into `inferred_let_exprs`.
    #[test]
    fn check_phase_pre_register_default_types_records_collision_in_pass2_skipped() {
        let param_decl = reify_ast::ParamDecl {
            name: "x".to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };
        let let_decl = reify_ast::LetDecl {
            name: "x".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None, // unannotated — will be compiled in Pass 2
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 5.5,
                    is_real: true,
                },
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![
            // Pass 1 claims the scope slot for "x" with Type::length()
            TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Param {
                    cell_type: Type::length(),
                    default_decl: param_decl,
                },
                span: SourceSpan::empty(0),
            },
            // Pass 2 finds "x" already claimed → should record in pass2_skipped
            TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl,
                },
                span: SourceSpan::empty(0),
            },
        ];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let mut scope = CompilationScope::new("S");
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let out = check_phase_pre_register_default_types(
            &ctx,
            &structure_members,
            "S",
            &mut scope,
            &[],
            &[],
            &mut diagnostics,
        );

        // The collision must be recorded in pass2_skipped, not the expression cache.
        assert!(
            out.inferred_let_exprs.is_empty(),
            "Expected inferred_let_exprs to be empty when Pass 2 finds a scope collision; \
             got: {:?}",
            out.inferred_let_exprs.keys().collect::<Vec<_>>()
        );
        assert!(
            out.pass2_skipped.contains("x"),
            "Expected 'x' in pass2_skipped after Pass 2 found its scope slot already claimed"
        );
        // Negative control: an unannotated Let losing to a Param goes into pass2_skipped,
        // NOT pass1_skipped. The two sets are mutually exclusive by cell_type predicate.
        assert!(
            out.pass1_skipped.is_empty(),
            "Expected pass1_skipped to be empty: the unannotated-Let loser is recorded in \
             pass2_skipped, not pass1_skipped; got: {:?}",
            out.pass1_skipped
        );
        // Negative control: unannotated Let losing to a Param must not populate
        // pass1_param_skipped (only Param losers to an annotated Let go there).
        assert!(
            out.pass1_param_skipped.is_empty(),
            "Expected pass1_param_skipped to be empty: the unannotated-Let loser goes into \
             pass2_skipped, not pass1_param_skipped; got: {:?}",
            out.pass1_param_skipped
        );
        assert!(
            out.pass2_compile_errors.is_empty(),
            "Expected no pass2_compile_errors for a successful compilation"
        );
    }

    /// Phase-contract test: Pass 1 of `check_phase_pre_register_default_types` records
    /// an annotated-Let loser in `pass1_skipped` (task 1952 step-2).
    ///
    /// ## Scenario (Pass 1 ↔ Pass 2 symmetry)
    ///
    /// Pass 2 has a symmetric guard: when an unannotated Let loses `register_if_absent` to a
    /// Pass 1 annotated type, the name is recorded in `pass2_skipped` so the injection loop
    /// skips Let-cell emission. This test pins the **annotated-Let mirror**: when a Pass 1
    /// annotated-Let default loses `register_if_absent` to an earlier-in-ctx-defaults Param,
    /// the name must be recorded in the NEW `pass1_skipped` set (not in `pass2_skipped`).
    ///
    /// ## Fixture
    ///
    /// `ctx.defaults = [Param "x":Length, annotated Let "x":Length]`
    ///
    /// Pass 1 processes both: the Param claims the scope slot first, then the annotated Let's
    /// `register_if_absent` returns `Occupied`. The name "x" must be recorded in `pass1_skipped`.
    ///
    /// ## Assertions
    ///
    /// - `pass1_skipped.contains("x")`: the annotated-Let loser is recorded.
    /// - `pass2_skipped.is_empty()`: no unannotated Let reached Pass 2 (nothing to skip there).
    /// - `inferred_let_exprs.is_empty()`: no expression compiled (annotated Lets skip Pass 2).
    /// - `pass2_compile_errors.is_empty()`: no compile errors (no Pass 2 expressions attempted).
    /// - `diagnostics.is_empty()`: `register_if_absent` conflict is a debug-log only event.
    #[test]
    fn check_phase_pre_register_default_types_records_collision_in_pass1_skipped() {
        let param_decl = reify_ast::ParamDecl {
            name: "x".to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };
        // Annotated Let: cell_type = Some(Type::length()) encodes the resolved type directly.
        // Pass 1 processes this because cell_type is Some(_); Pass 2 is bypassed.
        let let_decl = reify_ast::LetDecl {
            name: "x".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None, // type_expr in LetDecl is not consulted by Pass 1 — DefaultKind carries cell_type
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 80.0,
                    is_real: false,
                },
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![
            // Pass 1 claims the scope slot for "x" with Type::length()
            TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Param {
                    cell_type: Type::length(),
                    default_decl: param_decl,
                },
                span: SourceSpan::empty(0),
            },
            // Pass 1 also processes this annotated Let — finds "x" already claimed → pass1_skipped
            TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: Some(Type::length()), // annotated → goes through Pass 1, not Pass 2
                    let_decl,
                },
                span: SourceSpan::empty(0),
            },
        ];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let mut scope = CompilationScope::new("S");
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let out = check_phase_pre_register_default_types(
            &ctx,
            &structure_members,
            "S",
            &mut scope,
            &[],
            &[],
            &mut diagnostics,
        );

        // The annotated-Let loser must be recorded in pass1_skipped.
        assert!(
            out.pass1_skipped.contains("x"),
            "Expected 'x' in pass1_skipped after Pass 1 found the annotated Let's scope slot \
             already claimed by the Param; got pass1_skipped = {:?}",
            out.pass1_skipped
        );
        // No unannotated Let reached Pass 2, so pass2_skipped must be empty.
        assert!(
            out.pass2_skipped.is_empty(),
            "Expected pass2_skipped to be empty — no unannotated Let was processed; \
             got: {:?}",
            out.pass2_skipped
        );
        // No expression was compiled (annotated Lets skip Pass 2).
        assert!(
            out.inferred_let_exprs.is_empty(),
            "Expected inferred_let_exprs to be empty — annotated Lets do not go through \
             Pass 2 inference; got: {:?}",
            out.inferred_let_exprs.keys().collect::<Vec<_>>()
        );
        // No compile-error names (no Pass 2 compilation was attempted).
        assert!(
            out.pass2_compile_errors.is_empty(),
            "Expected pass2_compile_errors to be empty; got: {:?}",
            out.pass2_compile_errors
        );
        // register_if_absent conflicts are debug-log only — no diagnostic emitted.
        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics — register_if_absent conflict is logged at debug \
             level only; got: {:?}",
            diagnostics
        );
    }

    /// Phase-contract test: Pass 1 of `check_phase_pre_register_default_types` records
    /// a Param loser in `pass1_param_skipped` (task 2208 step-1).
    ///
    /// ## Scenario (Pass 1 Param-loser symmetry)
    ///
    /// The reverse of `check_phase_pre_register_default_types_records_collision_in_pass1_skipped`:
    /// when the annotated Let appears *before* the Param in `ctx.defaults`, the annotated Let
    /// wins the scope slot and the Param loses. The Param loser must be recorded in the NEW
    /// `pass1_param_skipped` set (not in `pass1_skipped`, which only tracks annotated-Let losers).
    ///
    /// ## Fixture
    ///
    /// `ctx.defaults = [annotated Let "x":Length = 80, Param "x":Length]`
    ///
    /// Pass 1 processes both: the annotated Let claims the scope slot first, then the Param's
    /// `register_if_absent` returns `Occupied`. The name "x" must be recorded in `pass1_param_skipped`.
    ///
    /// ## Assertions
    ///
    /// - `pass1_param_skipped.contains("x")`: the Param loser is recorded.
    /// - `pass1_skipped.is_empty()`: no annotated-Let loser this direction.
    /// - `pass2_skipped.is_empty()`: no unannotated Let reached Pass 2.
    /// - `pass2_compile_errors.is_empty()`: no compile errors (no Pass 2 expressions attempted).
    /// - `inferred_let_exprs.is_empty()`: no expression cached (annotated Lets skip Pass 2).
    /// - `diagnostics.is_empty()`: `register_if_absent` conflict is a debug-log only event.
    #[test]
    fn check_phase_pre_register_default_types_records_collision_in_pass1_param_skipped() {
        // Annotated Let: cell_type = Some(Type::length()) — wins the scope slot first.
        let let_decl = reify_ast::LetDecl {
            name: "x".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None, // type_expr in LetDecl is not consulted by Pass 1 — DefaultKind carries cell_type
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 80.0,
                    is_real: false,
                },
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };
        // Param: loses the scope slot because annotated Let appeared first.
        let param_decl = reify_ast::ParamDecl {
            name: "x".to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![
            // Pass 1 claims the scope slot for "x" with the annotated Let first
            TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: Some(Type::length()), // annotated → processes in Pass 1, wins slot
                    let_decl,
                },
                span: SourceSpan::empty(0),
            },
            // Pass 1 also processes this Param — finds "x" already claimed → pass1_param_skipped
            TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Param {
                    cell_type: Type::length(),
                    default_decl: param_decl,
                },
                span: SourceSpan::empty(0),
            },
        ];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let mut scope = CompilationScope::new("S");
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let out = check_phase_pre_register_default_types(
            &ctx,
            &structure_members,
            "S",
            &mut scope,
            &[],
            &[],
            &mut diagnostics,
        );

        // The Param loser must be recorded in pass1_param_skipped.
        assert!(
            out.pass1_param_skipped.contains("x"),
            "Expected 'x' in pass1_param_skipped after Pass 1 found the Param's scope slot \
             already claimed by the annotated Let; got pass1_param_skipped = {:?}",
            out.pass1_param_skipped
        );
        // No annotated-Let loser in this direction, so pass1_skipped must be empty.
        assert!(
            out.pass1_skipped.is_empty(),
            "Expected pass1_skipped to be empty — the annotated Let won (not lost); \
             got: {:?}",
            out.pass1_skipped
        );
        // No unannotated Let reached Pass 2, so pass2_skipped must be empty.
        assert!(
            out.pass2_skipped.is_empty(),
            "Expected pass2_skipped to be empty — no unannotated Let was processed; \
             got: {:?}",
            out.pass2_skipped
        );
        // No compile-error names (no Pass 2 compilation was attempted).
        assert!(
            out.pass2_compile_errors.is_empty(),
            "Expected pass2_compile_errors to be empty; got: {:?}",
            out.pass2_compile_errors
        );
        // No expression was compiled (annotated Lets skip Pass 2).
        assert!(
            out.inferred_let_exprs.is_empty(),
            "Expected inferred_let_exprs to be empty — annotated Lets do not go through \
             Pass 2 inference; got: {:?}",
            out.inferred_let_exprs.keys().collect::<Vec<_>>()
        );
        // register_if_absent conflicts are debug-log only — no diagnostic emitted.
        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics — register_if_absent conflict is logged at debug \
             level only; got: {:?}",
            diagnostics
        );
    }

    /// Phase-contract test: Pass 2 treats a warning-only `compile_expr` result as success,
    /// not as a compile error (task 2158 step-1, severity-safe detection).
    ///
    /// ## Scenario
    ///
    /// `let x = []` — an empty list literal — causes `compile_expr` to push a
    /// `Severity::Warning` diagnostic ("cannot infer element type of empty list literal,
    /// defaulting to Real") but emits **no** `Severity::Error`.  The expression compiles
    /// successfully to `Type::List(Box::new(Type::dimensionless_scalar()))`.
    ///
    /// ## What this test locks in
    ///
    /// The len-based snapshot (`diagnostics.len() > diag_before`) used before task 2158
    /// counted the warning as a compile failure, wrongly inserting `x` into
    /// `pass2_compile_errors` and skipping both the `inferred_let_exprs` insert and the
    /// scope registration.  After the severity-filter fix, only `Severity::Error` additions
    /// are counted — the warning is tolerated and the expression is cached normally.
    ///
    /// Assertions:
    ///   (a) At least 1 diagnostic emitted, and ALL diagnostics have non-Error severity.
    ///   (b) `pass2_compile_errors` is empty — the warning must NOT be classified as failure.
    ///   (c) `("x", Let)` is present in `inferred_let_exprs`.
    ///   (d) The cached expression has `result_type == Type::List(Box::new(Type::dimensionless_scalar()))`.
    ///   (e) The scope slot for "x" is occupied after Pass 2 and holds the inferred type
    ///       `Type::List(Real)` — verified via the non-mutating `scope.resolve("x")` probe.
    #[test]
    fn check_phase_pre_register_default_types_treats_warning_only_compile_as_success() {
        let let_decl = reify_ast::LetDecl {
            name: "x".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None, // unannotated — must go through Pass 2 inference
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::ListLiteral(vec![]), // empty list → Warning, not Error
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![TraitDefault {
            name: Some("x".to_string()),
            kind: DefaultKind::Let {
                cell_type: None,
                let_decl,
            },
            span: SourceSpan::empty(0),
        }];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let mut scope = CompilationScope::new("S");
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let out = check_phase_pre_register_default_types(
            &ctx,
            &structure_members,
            "S",
            &mut scope,
            &[],
            &[],
            &mut diagnostics,
        );

        // (a) At least one diagnostic was emitted and none are Error-severity.
        //     The contract under test is "no Error ⇒ expression is cached", not the exact count.
        //     Pinning to exactly 1 Warning would break if compile_expr adds a future Info/note
        //     without any real regression.
        assert!(
            !diagnostics.is_empty(),
            "Expected at least one diagnostic (the empty-list-literal warning); got none"
        );
        assert!(
            diagnostics.iter().all(|d| d.severity != Severity::Error),
            "Expected NO Error-severity diagnostic — warning-only compile_expr must be treated \
             as success; got: {:?}",
            diagnostics
        );

        // (b) The warning must NOT be classified as a compile failure.
        assert!(
            out.pass2_compile_errors.is_empty(),
            "Expected pass2_compile_errors to be empty — a Warning-only compile_expr result \
             must be treated as success; got: {:?}",
            out.pass2_compile_errors
        );

        // pass2_skipped should also be empty (no scope-slot collision).
        assert!(
            out.pass2_skipped.is_empty(),
            "Expected pass2_skipped to be empty; got: {:?}",
            out.pass2_skipped
        );

        // (c) The compiled expression must be cached in inferred_let_exprs.
        assert!(
            out.inferred_let_exprs
                .contains_key(&("x".to_string(), AvailableDefaultKind::Let)),
            "Expected composite key ('x', Let) in inferred_let_exprs — Pass 2 must cache \
             the compiled expression even when compile_expr emitted a Warning; \
             got keys: {:?}",
            out.inferred_let_exprs.keys().collect::<Vec<_>>()
        );

        // (d) The cached expression has the expected inferred type for an empty list literal.
        assert_eq!(
            out.inferred_let_exprs[&("x".to_string(), AvailableDefaultKind::Let)].result_type,
            Type::List(Box::new(Type::dimensionless_scalar())),
            "Expected Type::List(Real) for an empty list literal (defaulting to Real element type)"
        );

        // (e) Scope slot for "x" is occupied after Pass 2 and holds the inferred type.
        //     Use the non-mutating scope.resolve probe so the assertion cannot accidentally
        //     insert an unrelated key if Pass 2 had failed to register silently.
        let resolved = scope.resolve("x");
        assert!(
            resolved.is_some(),
            "Expected 'x' to be registered in scope by Pass 2 (scope.resolve should return \
             Some(..)); got None, meaning Pass 2 failed to register the inferred type"
        );
        assert_eq!(
            resolved.unwrap().1,
            &Type::List(Box::new(Type::dimensionless_scalar())),
            "Expected scope slot for 'x' to hold the inferred Type::List(Real) after Pass 2; \
             got: {:?}",
            resolved.unwrap().1
        );
    }

    /// Phase-level guard test: chained-let `Type::Error` sentinel suppresses the
    /// requirement-check cascade (task 2158 amendment #3).
    ///
    /// Companion to the integration test
    /// `chained_unannotated_lets_with_unresolved_ref_do_not_cascade`
    /// (`tests/let_type_disambiguation_tests.rs`), which pins the
    /// absence-of-cascade at the "unresolved name" site.  This test locks in the
    /// *downstream* consequence: the `Type::Error` sentinel propagated through
    /// sibling `let c = a` lands in `available_defaults` as
    /// `("c", Let) -> Type::Error`, and `implicitly_converts_to(Error, _) -> true`
    /// (wildcard in `type_compat.rs`) absorbs the mismatch silently at the
    /// requirement-check site.
    ///
    /// ## Why this is here and not in `let_type_disambiguation_tests.rs`
    ///
    /// `RequirementKind::Let` is not parser-reachable from reify source today
    /// (see mod.rs:256-260).  The test hand-builds the requirement so we can
    /// exercise `check_phase_check_members_against_requirements`' anti-cascade
    /// path without an RFC-level API promotion.
    ///
    /// ## Scenario
    ///
    /// - Trait T provides `let a = b` (b undefined → compile error → `a` poisoned
    ///   with `Type::Error` in scope) and `let c = a` (a resolves to `Type::Error`
    ///   → c compiles silently to `Type::Error`, no new error diagnostic).
    /// - Trait T also declares `require let c : Length`
    ///   (`RequirementKind::Let(Type::length())` — hand-built).
    /// - Structure S : T { } — no member overrides.
    ///
    /// ## Expected behavior
    ///
    /// Pass 2 records "a" in `pass2_compile_errors` (compile failure).  "c" is NOT
    /// in `pass2_compile_errors` — `c = a` compiles successfully to `Type::Error`
    /// via the scope sentinel.  `available_defaults` advertises
    /// `("c", Let) -> Type::Error`.  The requirement-check calls
    /// `implicitly_converts_to(Type::Error, Type::length())` → `true` → no
    /// "type mismatch for trait member 'c'" cascade.
    ///
    /// The only expected diagnostic is the root-cause "unresolved name: b".
    ///
    /// ## Failure without task 2158 step-4 (scope-poison)
    ///
    /// Without the scope-poison fix, "a" is absent from scope when `c = a` is
    /// compiled, emitting "unresolved name: a" and adding "c" to
    /// `pass2_compile_errors`.  `available_defaults` then has no entry for
    /// `("c", Let)`, and the requirement check emits "missing required member 'c'"
    /// — a spurious secondary cascade on top of the root-cause "unresolved b".
    #[test]
    fn chained_unannotated_lets_error_sentinel_suppresses_requirement_check_cascade() {
        // --- let a = b (b is undefined — will fail compile) ---
        let let_decl_a = reify_ast::LetDecl {
            name: "a".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None, // unannotated
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::Ident("b".to_string()), // b is undefined
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        // --- let c = a (a resolves to Type::Error from the scope sentinel) ---
        let let_decl_c = reify_ast::LetDecl {
            name: "c".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None, // unannotated
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::Ident("a".to_string()), // a → Type::Error sentinel
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        // --- Trait T: requires `let c : Length`; provides `let a = b`, `let c = a` ---
        // `RequirementKind::Let` is hand-built because it is not parser-reachable today.
        let trait_t = CompiledTrait {
            name: "TraitT".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "c".to_string(),
                kind: RequirementKind::Let(Type::length()),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![
                TraitDefault {
                    name: Some("a".to_string()),
                    kind: DefaultKind::Let {
                        cell_type: None,
                        let_decl: let_decl_a,
                    },
                    span: SourceSpan::empty(0),
                },
                TraitDefault {
                    name: Some("c".to_string()),
                    kind: DefaultKind::Let {
                        cell_type: None,
                        let_decl: let_decl_c,
                    },
                    span: SourceSpan::empty(0),
                },
            ],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // --- Structure S : TraitT { } — no member overrides ---
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![reify_ast::TraitBoundRef {
                name: "TraitT".to_string(),
                type_args: vec![],
                span: SourceSpan::empty(0),
            }],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[trait_t], &structure_def, &[]);
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();

        // (a) Root-cause "unresolved b" error must be present.
        let has_unresolved_b = errors.iter().any(|d| {
            d.message.contains("unresolved")
                && (d.message.contains(": b")
                    || d.message.contains("'b'")
                    || d.message.contains("`b`"))
        });
        assert!(
            has_unresolved_b,
            "expected at least one diagnostic naming the unresolved identifier `b` \
             (root cause); got: {:?}",
            errors
        );

        // (b) No "type mismatch for trait member 'c'" cascade.
        // After scope-poison, `c = a` compiles to Type::Error (a is in scope as
        // the sentinel).  `implicitly_converts_to(Type::Error, Type::length())`
        // returns true via the wildcard in `type_compat.rs`.
        let cascade_mismatch: Vec<_> = errors
            .iter()
            .filter(|d| {
                d.message.contains("type mismatch for trait member")
                    && (d.message.contains("'c'") || d.message.contains("\"c\""))
            })
            .collect();
        assert!(
            cascade_mismatch.is_empty(),
            "cascade 'type mismatch for trait member c' found — \
             `implicitly_converts_to(Type::Error, Type::length())` should return true \
             (wildcard in type_compat.rs), absorbing the poisoned type rather than \
             emitting a mismatch.  Cascade diagnostics: {:?}",
            cascade_mismatch
        );

        // (c) No "missing required member 'c'" cascade.
        // Without scope-poison, `c = a` would also fail compile (a unresolved),
        // putting "c" in pass2_compile_errors and leaving no entry in
        // available_defaults — causing a spurious "missing required member 'c'".
        let missing_c: Vec<_> = errors
            .iter()
            .filter(|d| {
                d.message.contains("missing required member")
                    && (d.message.contains("'c'") || d.message.contains("\"c\""))
            })
            .collect();
        assert!(
            missing_c.is_empty(),
            "spurious 'missing required member c' found — without scope-poison, `c = a` \
             would fail compile (a unresolved in scope) and c would be excluded from \
             available_defaults entirely.  Diagnostics: {:?}",
            missing_c
        );
    }

    /// Phase-contract test for `check_phase_build_available_defaults_map`.
    ///
    /// Verifies that the helper builds a composite-keyed HashMap from ctx.defaults,
    /// including Param defaults and excluding Constraint defaults. This test fails to
    /// compile until the helper exists (TDD compile-tripwire) and pins the helper's
    /// return type signature.
    #[test]
    fn check_phase_build_available_defaults_map_uses_composite_key() {
        let param_decl = reify_ast::ParamDecl {
            name: "x".to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };
        let constraint_decl = reify_ast::ConstraintDecl {
            label: Some("bound".to_string()),
            expr: reify_ast::Expr {
                kind: reify_ast::ExprKind::BoolLiteral(true),
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            is_priv: false,
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![
            TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Param {
                    cell_type: Type::dimensionless_scalar(),
                    default_decl: param_decl,
                },
                span: SourceSpan::empty(0),
            },
            TraitDefault {
                name: Some("bound".to_string()),
                kind: DefaultKind::Constraint(constraint_decl),
                span: SourceSpan::empty(0),
            },
        ];

        let inferred_let_exprs: HashMap<(String, AvailableDefaultKind), CompiledExpr> =
            HashMap::new();
        let pass1_skipped: HashSet<String> = HashSet::new();
        let pass1_param_skipped: HashSet<String> = HashSet::new();
        let pass2_skipped: HashSet<String> = HashSet::new();
        let pass2_compile_errors: HashSet<String> = HashSet::new();

        let available_defaults = check_phase_build_available_defaults_map(
            &ctx,
            &inferred_let_exprs,
            &pass1_skipped,
            &pass1_param_skipped,
            &pass2_skipped,
            &pass2_compile_errors,
        );

        assert_eq!(
            available_defaults.len(),
            1,
            "Expected exactly 1 entry (Param); Constraint should be filtered. Got: {:?}",
            available_defaults.keys().collect::<Vec<_>>()
        );
        assert!(
            available_defaults.contains_key(&("x".to_string(), AvailableDefaultKind::Param)),
            "Expected key ('x', Param) in available_defaults"
        );
        assert_eq!(
            available_defaults[&("x".to_string(), AvailableDefaultKind::Param)],
            Type::dimensionless_scalar(),
            "Expected Type::dimensionless_scalar() for key ('x', Param)"
        );
    }

    /// Phase-contract test: `check_phase_build_available_defaults_map` excludes an
    /// annotated-Let advertisement when its name is in `pass1_skipped` (task 1952 step-4).
    ///
    /// ## Invariant: advertisement mirrors injection
    ///
    /// The injection loop skips annotated-Let cell emission when `pass1_skipped.contains(name)`.
    /// For correctness, the advertisement map must also omit the corresponding `(name, Let)`
    /// entry — otherwise a `RequirementKind::Let` lookup could match a phantom advertisement
    /// that has no injected cell backing it, producing a spurious "requirement satisfied"
    /// answer.
    ///
    /// This test pins the suppression guard: `cell_type.is_some() && pass1_skipped.contains(name)`
    /// returns `None` (excluded). The companion positive-control assertion below confirms that
    /// the same fixture WITHOUT `pass1_skipped` DOES produce `Some(&Type::length())`, so the
    /// test isolates exactly the new guard's effect.
    ///
    /// ## Fixture
    ///
    /// `ctx.defaults = [annotated Let "x":Length]` — the Let that Pass 1 would have skipped.
    /// `pass1_skipped = {"x"}` — simulates the Pass 1 race loss.
    ///
    /// ## Assertions
    ///
    /// - With `pass1_skipped = {"x"}`: `available_defaults.get(("x", Let)) == None`.
    /// - Positive control (empty pass1_skipped): `available_defaults.get(("x", Let)) == Some(&Length)`.
    ///
    /// **COMPILE-TRIPWIRE**: fails to compile until step-5 adds `pass1_skipped: &HashSet<String>`
    /// as the new third parameter of `check_phase_build_available_defaults_map`.
    #[test]
    fn check_phase_build_available_defaults_map_excludes_annotated_let_for_pass1_skipped_name() {
        let let_decl = reify_ast::LetDecl {
            name: "x".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None, // type_expr not consulted — DefaultKind carries cell_type directly
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 80.0,
                    is_real: false,
                },
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![TraitDefault {
            name: Some("x".to_string()),
            kind: DefaultKind::Let {
                cell_type: Some(Type::length()), // annotated Let — Pass 1 processes this
                let_decl,
            },
            span: SourceSpan::empty(0),
        }];

        let inferred_let_exprs: HashMap<(String, AvailableDefaultKind), CompiledExpr> =
            HashMap::new();
        // Positive control: without pass1_skipped, the annotated Let IS advertised.
        let empty_pass1_skipped: HashSet<String> = HashSet::new();
        let empty_pass1_param_skipped: HashSet<String> = HashSet::new();
        let pass2_skipped: HashSet<String> = HashSet::new();
        let pass2_compile_errors: HashSet<String> = HashSet::new();

        // COMPILE-TRIPWIRE: `&empty_pass1_skipped` is the new 3rd parameter — fails to compile
        // until step-5 adds `pass1_skipped: &HashSet<String>` to the function signature.
        let positive_control = check_phase_build_available_defaults_map(
            &ctx,
            &inferred_let_exprs,
            &empty_pass1_skipped,
            &empty_pass1_param_skipped,
            &pass2_skipped,
            &pass2_compile_errors,
        );
        assert_eq!(
            positive_control.get(&("x".to_string(), AvailableDefaultKind::Let)),
            Some(&Type::length()),
            "positive control: without pass1_skipped, annotated Let 'x' must be advertised \
             as Some(&Type::length()); the guard must not over-suppress"
        );

        // Negative test: with pass1_skipped = {"x"}, the annotated Let must be suppressed.
        let mut pass1_skipped: HashSet<String> = HashSet::new();
        pass1_skipped.insert("x".to_string());

        let available_defaults = check_phase_build_available_defaults_map(
            &ctx,
            &inferred_let_exprs,
            &pass1_skipped,
            &empty_pass1_param_skipped,
            &pass2_skipped,
            &pass2_compile_errors,
        );
        assert_eq!(
            available_defaults.get(&("x".to_string(), AvailableDefaultKind::Let)),
            None,
            "annotated Let 'x' must be excluded from available_defaults when its name is \
             in pass1_skipped — advertisement mirrors injection (no phantom entry for a \
             cell that will not be injected)"
        );
    }

    /// Phase-contract test: `check_phase_build_available_defaults_map` excludes a Param
    /// advertisement when its name is in `pass1_param_skipped` (task 2208 step-3).
    ///
    /// ## Invariant: advertisement mirrors injection
    ///
    /// The injection loop (step-6) will skip Param cell emission when
    /// `pass1_param_skipped.contains(name)`. For correctness, the advertisement map must
    /// also omit the corresponding `(name, Param)` entry — otherwise a
    /// `RequirementKind::Param` lookup could match a phantom advertisement with no injected
    /// cell backing it, producing a spurious "requirement satisfied" answer.
    ///
    /// ## Fixture
    ///
    /// `ctx.defaults = [Param "x":Length]` — the Param that Pass 1 would have skipped.
    /// `pass1_param_skipped = {"x"}` — simulates the Pass 1 race loss (annotated Let won).
    ///
    /// ## Assertions
    ///
    /// - With `pass1_param_skipped = {"x"}`: `available_defaults.get(("x", Param)) == None`.
    /// - Positive control (empty `pass1_param_skipped`): `available_defaults.get(("x", Param)) == Some(&Length)`.
    ///
    /// **COMPILE-TRIPWIRE**: fails to compile until step-4 adds
    /// `pass1_param_skipped: &HashSet<String>` as a new parameter of
    /// `check_phase_build_available_defaults_map`.
    #[test]
    fn check_phase_build_available_defaults_map_excludes_param_for_pass1_param_skipped_name() {
        let param_decl = reify_ast::ParamDecl {
            name: "x".to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![TraitDefault {
            name: Some("x".to_string()),
            kind: DefaultKind::Param {
                cell_type: Type::length(),
                default_decl: param_decl,
            },
            span: SourceSpan::empty(0),
        }];

        let inferred_let_exprs: HashMap<(String, AvailableDefaultKind), CompiledExpr> =
            HashMap::new();
        let pass1_skipped: HashSet<String> = HashSet::new();
        let pass2_skipped: HashSet<String> = HashSet::new();
        let pass2_compile_errors: HashSet<String> = HashSet::new();

        // Positive control: without pass1_param_skipped suppression, the Param IS advertised.
        let empty_pass1_param_skipped: HashSet<String> = HashSet::new();

        // COMPILE-TRIPWIRE: `&empty_pass1_param_skipped` is the new parameter — fails to compile
        // until step-4 adds `pass1_param_skipped: &HashSet<String>` to the function signature.
        let positive_control = check_phase_build_available_defaults_map(
            &ctx,
            &inferred_let_exprs,
            &pass1_skipped,
            &empty_pass1_param_skipped,
            &pass2_skipped,
            &pass2_compile_errors,
        );
        assert_eq!(
            positive_control.get(&("x".to_string(), AvailableDefaultKind::Param)),
            Some(&Type::length()),
            "positive control: without pass1_param_skipped, Param 'x' must be advertised \
             as Some(&Type::length()); the guard must not over-suppress"
        );

        // Negative test: with pass1_param_skipped = {"x"}, the Param must be suppressed.
        let mut pass1_param_skipped: HashSet<String> = HashSet::new();
        pass1_param_skipped.insert("x".to_string());

        let available_defaults = check_phase_build_available_defaults_map(
            &ctx,
            &inferred_let_exprs,
            &pass1_skipped,
            &pass1_param_skipped,
            &pass2_skipped,
            &pass2_compile_errors,
        );
        assert_eq!(
            available_defaults.get(&("x".to_string(), AvailableDefaultKind::Param)),
            None,
            "Param 'x' must be excluded from available_defaults when its name is in \
             pass1_param_skipped — advertisement mirrors injection (no phantom entry for \
             a cell that will not be injected)"
        );
    }

    /// Phase-contract test for `check_phase_check_members_against_requirements`.
    ///
    /// Verifies that the helper emits a "missing required member" diagnostic when a
    /// structure satisfies neither the member directly nor a same-kind default.
    /// This test fails to compile until the helper exists (TDD compile-tripwire) and
    /// pins the helper's signature.
    #[test]
    fn check_phase_check_members_against_requirements_emits_missing_member_when_unsatisfied() {
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![], // No members — requirement "w" is unsatisfied
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let mut ctx = MergeContext::new();
        ctx.requirements = vec![TraitRequirement {
            name: "w".to_string(),
            kind: RequirementKind::Param(Type::dimensionless_scalar()),
            span: SourceSpan::empty(0),
        }];

        let structure_param_members: HashMap<String, Type> = HashMap::new();
        let structure_let_members: HashMap<String, Type> = HashMap::new();
        let available_defaults: HashMap<(String, AvailableDefaultKind), Type> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_phase_check_members_against_requirements(
            &ctx,
            &entity_ref,
            &structure_param_members,
            &structure_let_members,
            &available_defaults,
            // No structure assoc fns in this test.
            &HashMap::<String, CompiledAssocFnSig>::new(),
            // No structure assoc type bindings in this test.
            &HashMap::<String, Type>::new(),
            &mut diagnostics,
        );

        assert_eq!(
            diagnostics.len(),
            1,
            "Expected 1 diagnostic for missing member 'w'; got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics[0].message.contains("missing required member"),
            "Expected 'missing required member' in diagnostic; got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("'w'"),
            "Expected member name 'w' in diagnostic; got: {}",
            diagnostics[0].message
        );
    }

    /// Phase-contract test: a structure `param` member does NOT satisfy a `RequirementKind::Let`
    /// requirement.
    ///
    /// `RequirementKind::Let` is not parser-reachable from reify source today — the parser maps
    /// `let x = …` in a trait body to `DefaultKind::Let` (a default), not a requirement.  This
    /// unit test hand-builds the fixture to lock down the kind-aware routing in
    /// `check_phase_check_members_against_requirements`: when the requirement is `Let`, the
    /// checker looks in `structure_let_members` (not `structure_param_members`), so a structure
    /// `param x` does not satisfy it.
    ///
    /// This is the symmetric counterpart to the integration test
    /// `structure_let_does_not_satisfy_param_requirement` in `trait_conformance_tests.rs`.
    #[test]
    fn param_member_does_not_satisfy_let_requirement() {
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let mut ctx = MergeContext::new();
        // Hand-built RequirementKind::Let — not reachable from the parser today.
        ctx.requirements = vec![TraitRequirement {
            name: "x".to_string(),
            kind: RequirementKind::Let(Type::length()),
            span: SourceSpan::empty(0),
        }];

        // Structure provides `param x : Length` (in param map), NOT in let map.
        let mut structure_param_members: HashMap<String, Type> = HashMap::new();
        structure_param_members.insert("x".to_string(), Type::length());
        let structure_let_members: HashMap<String, Type> = HashMap::new();
        let available_defaults: HashMap<(String, AvailableDefaultKind), Type> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_phase_check_members_against_requirements(
            &ctx,
            &entity_ref,
            &structure_param_members,
            &structure_let_members,
            &available_defaults,
            // No structure assoc fns in this test.
            &HashMap::<String, CompiledAssocFnSig>::new(),
            // No structure assoc type bindings in this test.
            &HashMap::<String, Type>::new(),
            &mut diagnostics,
        );

        // A param member must NOT satisfy a let requirement — the checker looks in
        // structure_let_members for Let requirements, so it finds nothing and emits missing-member.
        assert_eq!(
            diagnostics.len(),
            1,
            "Expected 1 diagnostic: param 'x' must not satisfy let requirement 'x'; got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics[0].message.contains("missing required member"),
            "Expected 'missing required member' in diagnostic; got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("'x'"),
            "Expected member name 'x' in diagnostic; got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("let"),
            "Expected kind 'let' mentioned in diagnostic; got: {}",
            diagnostics[0].message
        );
    }

    /// Phase-contract test: `check_phase_check_members_against_requirements` emits a
    /// "type mismatch" diagnostic when the structure provides the required member but
    /// with the wrong type.
    ///
    /// Covers the `Some(actual_type)` arm where `implicitly_converts_to(actual, expected)`
    /// is false — the mismatch path inside the kind-routed map lookup.
    #[test]
    fn check_phase_check_members_against_requirements_emits_type_mismatch_for_wrong_member_type() {
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let mut ctx = MergeContext::new();
        ctx.requirements = vec![TraitRequirement {
            name: "w".to_string(),
            kind: RequirementKind::Param(Type::length()),
            span: SourceSpan::empty(0),
        }];

        // Structure param member "w" exists but has wrong type: Real, not Length
        let mut structure_param_members: HashMap<String, Type> = HashMap::new();
        structure_param_members.insert("w".to_string(), Type::dimensionless_scalar());
        let structure_let_members: HashMap<String, Type> = HashMap::new();
        let available_defaults: HashMap<(String, AvailableDefaultKind), Type> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_phase_check_members_against_requirements(
            &ctx,
            &entity_ref,
            &structure_param_members,
            &structure_let_members,
            &available_defaults,
            // No structure assoc fns in this test.
            &HashMap::<String, CompiledAssocFnSig>::new(),
            // No structure assoc type bindings in this test.
            &HashMap::<String, Type>::new(),
            &mut diagnostics,
        );

        assert_eq!(
            diagnostics.len(),
            1,
            "Expected 1 type-mismatch diagnostic; got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics[0].message.contains("type mismatch"),
            "Expected 'type mismatch' in diagnostic; got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("'w'"),
            "Expected member name 'w' in diagnostic; got: {}",
            diagnostics[0].message
        );
    }

    /// Phase-contract test: `check_phase_check_members_against_requirements` emits a
    /// "type mismatch" diagnostic when no structure member exists but there IS a same-kind
    /// available default whose type does not satisfy the requirement.
    ///
    /// Covers the `Some(default_type)` (non-matching) arm of the `available_defaults.get`
    /// match — the default exists but `implicitly_converts_to(default_type, expected)` is
    /// false, so the "requirement expects …, available default has …" diagnostic fires.
    #[test]
    fn check_phase_check_members_against_requirements_emits_mismatch_for_wrong_default_type() {
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let mut ctx = MergeContext::new();
        ctx.requirements = vec![TraitRequirement {
            name: "w".to_string(),
            kind: RequirementKind::Param(Type::length()),
            span: SourceSpan::empty(0),
        }];

        // No structure member "w", but there IS a same-kind (Param) default with wrong type
        let structure_param_members: HashMap<String, Type> = HashMap::new();
        let structure_let_members: HashMap<String, Type> = HashMap::new();
        let mut available_defaults: HashMap<(String, AvailableDefaultKind), Type> = HashMap::new();
        available_defaults.insert(
            ("w".to_string(), AvailableDefaultKind::Param),
            Type::dimensionless_scalar(), // Wrong type — Length is required
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_phase_check_members_against_requirements(
            &ctx,
            &entity_ref,
            &structure_param_members,
            &structure_let_members,
            &available_defaults,
            // No structure assoc fns in this test.
            &HashMap::<String, CompiledAssocFnSig>::new(),
            // No structure assoc type bindings in this test.
            &HashMap::<String, Type>::new(),
            &mut diagnostics,
        );

        assert_eq!(
            diagnostics.len(),
            1,
            "Expected 1 diagnostic for wrong-typed default; got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics[0].message.contains("available default"),
            "Expected 'available default' in diagnostic message; got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("'w'"),
            "Expected member name 'w' in diagnostic; got: {}",
            diagnostics[0].message
        );
    }

    /// Phase-contract test: `check_phase_check_members_against_requirements` emits a
    /// "missing required sub-component" diagnostic for a `RequirementKind::Sub` requirement
    /// when the structure has no matching sub-component declaration.
    ///
    /// Covers the `RequirementKind::Sub` arm: when `structure.members` contains no
    /// `MemberDecl::Sub` entry matching both the required name and structure type, the
    /// "missing required sub-component" diagnostic is emitted.
    #[test]
    fn check_phase_check_members_against_requirements_emits_missing_sub_for_absent_sub_component() {
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![], // No sub-components declared
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let mut ctx = MergeContext::new();
        ctx.requirements = vec![TraitRequirement {
            name: "mount".to_string(),
            kind: RequirementKind::Sub("Hole".to_string()),
            span: SourceSpan::empty(0),
        }];

        let structure_param_members: HashMap<String, Type> = HashMap::new();
        let structure_let_members: HashMap<String, Type> = HashMap::new();
        let available_defaults: HashMap<(String, AvailableDefaultKind), Type> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_phase_check_members_against_requirements(
            &ctx,
            &entity_ref,
            &structure_param_members,
            &structure_let_members,
            &available_defaults,
            // No structure assoc fns in this test.
            &HashMap::<String, CompiledAssocFnSig>::new(),
            // No structure assoc type bindings in this test.
            &HashMap::<String, Type>::new(),
            &mut diagnostics,
        );

        assert_eq!(
            diagnostics.len(),
            1,
            "Expected 1 diagnostic for missing sub-component 'mount'; got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics[0]
                .message
                .contains("missing required sub-component"),
            "Expected 'missing required sub-component' in diagnostic; got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("'mount'"),
            "Expected sub-component name 'mount' in diagnostic; got: {}",
            diagnostics[0].message
        );
    }

    /// Phase-contract test for `check_phase_inject_defaults`.
    ///
    /// Verifies that the helper injects a Param value cell when the structure does not
    /// override the default. The injected cell should have kind=Param, member="x",
    /// no constraints, and no diagnostics. This test fails to compile until the helper
    /// exists (TDD compile-tripwire) and pins the helper's signature.
    #[test]
    fn check_phase_inject_defaults_injects_param_cell_for_non_overridden_default() {
        let param_decl = reify_ast::ParamDecl {
            name: "x".to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: None, // No default expression
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![TraitDefault {
            name: Some("x".to_string()),
            kind: DefaultKind::Param {
                cell_type: Type::dimensionless_scalar(),
                default_decl: param_decl,
            },
            span: SourceSpan::empty(0),
        }];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let structure_constraint_labels: HashSet<String> = HashSet::new();
        let inferred_let_exprs: HashMap<(String, AvailableDefaultKind), CompiledExpr> =
            HashMap::new();
        let pass2_skipped: HashSet<String> = HashSet::new();
        let pass2_compile_errors: HashSet<String> = HashSet::new();
        let mut scope = CompilationScope::new("S");
        let mut value_cells: Vec<ValueCellDecl> = vec![];
        let mut constraints: Vec<CompiledConstraint> = vec![];
        let mut constraint_index: u32 = 0;
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let pass1_skipped: HashSet<String> = HashSet::new();
        let pass1_param_skipped: HashSet<String> = HashSet::new();
        check_phase_inject_defaults(
            &ctx,
            &entity_ref,
            &structure_members,
            &structure_constraint_labels,
            inferred_let_exprs,
            &pass1_skipped,
            &pass1_param_skipped,
            &pass2_skipped,
            &pass2_compile_errors,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            &[],
            &[],
            &mut diagnostics,
        );

        assert_eq!(
            value_cells.len(),
            1,
            "Expected 1 value cell for injected param 'x'; got: {:?}",
            value_cells
        );
        assert_eq!(
            value_cells[0].id.member, "x",
            "Expected cell member='x'; got: {}",
            value_cells[0].id.member
        );
        assert_eq!(
            value_cells[0].kind,
            ValueCellKind::Param,
            "Expected ValueCellKind::Param"
        );
        assert!(
            constraints.is_empty(),
            "Expected no constraints; got: {:?}",
            constraints
        );
        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// Phase-contract test: `check_phase_inject_defaults` skips annotated-Let cell injection
    /// when the name is in `pass1_skipped` (task 1952 step-6).
    ///
    /// ## Invariant: annotated-Let injection is suppressed when name in pass1_skipped
    ///
    /// This is the mirror of the unannotated-Let / `pass2_skipped` suppression at
    /// `checker.rs` (the `None`-cache-miss arm that calls `continue` when
    /// `pass2_skipped.contains(name)`).  For annotated Lets, the new guard is:
    ///
    /// ```text
    /// if cell_type.is_some() && pass1_skipped.contains(name) { continue; }
    /// ```
    ///
    /// placed **before** `compile_expr` so the expression is never compiled for a
    /// skipped name — no cell is pushed, no diagnostic is emitted.
    ///
    /// ## Fixture
    ///
    /// A single annotated Let default for "x" (cell_type = Some(Type::length())),
    /// with pass1_skipped = {"x"}.  The injection loop must skip "x" entirely:
    /// - `value_cells.is_empty()` — no Let cell emitted
    /// - `constraints.is_empty()` — no constraint emitted
    /// - `diagnostics.is_empty()` — no compile-error or any other diagnostic
    ///
    /// ## Cross-references
    ///
    /// - Step-7 adds `pass1_skipped: &HashSet<String>` to `check_phase_inject_defaults`
    ///   (between `inferred_let_exprs` and `pass2_skipped`) and the guard in the
    ///   `DefaultKind::Let { cell_type: Some(_), .. }` arm.
    /// - Sibling test: `check_phase_inject_defaults_injects_param_cell_for_non_overridden_default`
    ///   (the passthrough case, pass1_skipped = empty).
    ///
    /// **COMPILE-TRIPWIRE**: fails to compile until step-7 adds `pass1_skipped: &HashSet<String>`
    /// as a parameter of `check_phase_inject_defaults` (between `inferred_let_exprs` and
    /// `pass2_skipped`).
    #[test]
    fn check_phase_inject_defaults_skips_annotated_let_cell_for_pass1_skipped_name() {
        // Annotated Let "x": Length = 80.0 — cell_type carried in DefaultKind, not type_expr.
        let let_decl = reify_ast::LetDecl {
            name: "x".to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None, // type_expr not consulted — DefaultKind::Let carries cell_type directly
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 80.0,
                    is_real: false,
                },
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![TraitDefault {
            name: Some("x".to_string()),
            kind: DefaultKind::Let {
                cell_type: Some(Type::length()), // annotated — would be compiled if NOT in pass1_skipped
                let_decl,
            },
            span: SourceSpan::empty(0),
        }];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let structure_constraint_labels: HashSet<String> = HashSet::new();
        let inferred_let_exprs: HashMap<(String, AvailableDefaultKind), CompiledExpr> =
            HashMap::new();
        // COMPILE-TRIPWIRE: `&pass1_skipped` is the new parameter added by step-7 between
        // `inferred_let_exprs` and `pass2_skipped`. Fails to compile until step-7 updates
        // `check_phase_inject_defaults` to accept `pass1_skipped: &HashSet<String>`.
        let mut pass1_skipped: HashSet<String> = HashSet::new();
        pass1_skipped.insert("x".to_string());
        let pass1_param_skipped: HashSet<String> = HashSet::new();
        let pass2_skipped: HashSet<String> = HashSet::new();
        let pass2_compile_errors: HashSet<String> = HashSet::new();
        let mut scope = CompilationScope::new("S");
        let mut value_cells: Vec<ValueCellDecl> = vec![];
        let mut constraints: Vec<CompiledConstraint> = vec![];
        let mut constraint_index: u32 = 0;
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_phase_inject_defaults(
            &ctx,
            &entity_ref,
            &structure_members,
            &structure_constraint_labels,
            inferred_let_exprs,
            &pass1_skipped,
            &pass1_param_skipped,
            &pass2_skipped,
            &pass2_compile_errors,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            &[],
            &[],
            &mut diagnostics,
        );

        assert!(
            value_cells.is_empty(),
            "Expected no value cells: annotated Let 'x' is in pass1_skipped and must be \
             suppressed by the injection loop; got: {:?}",
            value_cells
        );
        assert!(
            constraints.is_empty(),
            "Expected no constraints; got: {:?}",
            constraints
        );
        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics (pass1_skipped suppression must be silent); got: {:?}",
            diagnostics
        );
    }

    /// Phase-contract test: `check_phase_inject_defaults` skips Param cell injection
    /// when the name is in `pass1_param_skipped` (task 2208 step-5).
    ///
    /// ## Invariant: Param injection is suppressed when name in pass1_param_skipped
    ///
    /// This is the mirror of the annotated-Let / `pass1_skipped` suppression (step-7 of task 1952).
    /// When a Param default lost the Pass 1 `register_if_absent` race (annotated Let won), the
    /// Param injection arm must not emit a cell — the winning annotated Let will inject it.
    /// The new guard (step-6):
    ///
    /// ```text
    /// if pass1_param_skipped.contains(name) { continue; }
    /// ```
    ///
    /// placed at the TOP of the `DefaultKind::Param` arm — before the `structure_members.contains_key`
    /// check and any `compile_expr` call.
    ///
    /// ## Fixture
    ///
    /// A single Param default for "x" (cell_type = Type::dimensionless_scalar(), no default expression),
    /// with `pass1_param_skipped = {"x"}`.  The injection loop must skip "x" entirely:
    /// - `value_cells.is_empty()` — no Param cell emitted
    /// - `constraints.is_empty()` — no constraint emitted
    /// - `diagnostics.is_empty()` — no compile-error or any other diagnostic
    ///
    /// **COMPILE-TRIPWIRE**: fails to compile until step-6 adds
    /// `pass1_param_skipped: &HashSet<String>` as a parameter of `check_phase_inject_defaults`
    /// (between `pass1_skipped` and `pass2_skipped`).
    #[test]
    fn check_phase_inject_defaults_skips_param_cell_for_pass1_param_skipped_name() {
        let param_decl = reify_ast::ParamDecl {
            name: "x".to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        };

        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };
        let entity_ref = EntityDefRef::from(&structure_def);

        let mut ctx = MergeContext::new();
        ctx.defaults = vec![TraitDefault {
            name: Some("x".to_string()),
            kind: DefaultKind::Param {
                cell_type: Type::dimensionless_scalar(),
                default_decl: param_decl,
            },
            span: SourceSpan::empty(0),
        }];

        let structure_members: HashMap<String, Type> = HashMap::new();
        let structure_constraint_labels: HashSet<String> = HashSet::new();
        let inferred_let_exprs: HashMap<(String, AvailableDefaultKind), CompiledExpr> =
            HashMap::new();
        let pass1_skipped: HashSet<String> = HashSet::new();
        // COMPILE-TRIPWIRE: `&pass1_param_skipped` is the new parameter added by step-6 between
        // `pass1_skipped` and `pass2_skipped`. Fails to compile until step-6 updates
        // `check_phase_inject_defaults` to accept `pass1_param_skipped: &HashSet<String>`.
        let mut pass1_param_skipped: HashSet<String> = HashSet::new();
        pass1_param_skipped.insert("x".to_string());
        let pass2_skipped: HashSet<String> = HashSet::new();
        let pass2_compile_errors: HashSet<String> = HashSet::new();
        let mut scope = CompilationScope::new("S");
        let mut value_cells: Vec<ValueCellDecl> = vec![];
        let mut constraints: Vec<CompiledConstraint> = vec![];
        let mut constraint_index: u32 = 0;
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_phase_inject_defaults(
            &ctx,
            &entity_ref,
            &structure_members,
            &structure_constraint_labels,
            inferred_let_exprs,
            &pass1_skipped,
            &pass1_param_skipped,
            &pass2_skipped,
            &pass2_compile_errors,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            &[],
            &[],
            &mut diagnostics,
        );

        assert!(
            value_cells.is_empty(),
            "Expected no value cells: Param 'x' is in pass1_param_skipped and must be \
             suppressed by the injection loop; got: {:?}",
            value_cells
        );
        assert!(
            constraints.is_empty(),
            "Expected no constraints; got: {:?}",
            constraints
        );
        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics (pass1_param_skipped suppression must be silent); got: {:?}",
            diagnostics
        );
    }

    // ── task 1914 step-5: unit tests for resolve_let_advertised_type helper ──
    // These tests are compile-tripwires: they fail to compile until step-6 defines
    // `pub(super) fn resolve_let_advertised_type` in checker.rs.

    /// Pins the annotation-wins branch of `resolve_let_advertised_type`.
    ///
    /// When a cell_type annotation is present, the helper must return it unchanged
    /// regardless of what the inferred expression type is. This matches the behavior
    /// at `check_phase_build_available_defaults_map` (site 1) where annotated Let
    /// defaults bypass the inferred cache entirely.
    #[test]
    fn resolve_let_advertised_type_prefers_annotation_over_inferred() {
        let inferred = CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar());
        let result = resolve_let_advertised_type(&Some(Type::length()), Some(&inferred));
        assert_eq!(
            result,
            Type::length(),
            "Expected annotation type (Length) to win over inferred type (Real)"
        );
    }

    /// Pins the inferred-fallback branch of `resolve_let_advertised_type`.
    ///
    /// When there is no cell_type annotation but an inferred CompiledExpr is available,
    /// the helper must return the inferred expression's result_type. This is the common
    /// path for unannotated let defaults that passed Pass 2 compilation successfully.
    /// After task 1914 suggestion #1, this is the only non-defensive path for
    /// unannotated lets: names in `pass2_compile_errors` never reach this helper.
    #[test]
    fn resolve_let_advertised_type_uses_inferred_when_no_annotation() {
        let inferred = CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar());
        let result = resolve_let_advertised_type(&None, Some(&inferred));
        assert_eq!(
            result,
            Type::dimensionless_scalar(),
            "Expected inferred type (Real) when no annotation is present"
        );
    }

    /// Pins the anti-cascade-poison fallback branch of `resolve_let_advertised_type`.
    ///
    /// When neither a cell_type annotation nor an inferred expression is available,
    /// the helper returns `Type::Error` (anti-cascade poison). This arm should not be
    /// reached in practice — the `debug_assert!` at the call site in
    /// `check_phase_build_available_defaults_map` (checker.rs:766-772) catches drift;
    /// the `Type::Error` fallback is defense in depth for release builds where the
    /// `debug_assert!` is a no-op (task 3749, tightening of the 3639 G-allow carve-out).
    #[test]
    fn resolve_let_advertised_type_falls_back_to_error_when_neither_annotation_nor_inferred() {
        let result = resolve_let_advertised_type(&None, None);
        assert_eq!(
            result,
            Type::Error,
            "Expected Type::Error anti-cascade poison when no annotation and no inferred expression"
        );
    }

    /// `emit_geometry_unbounded` pushes exactly one `Diagnostic` with severity
    /// `Error`, code `Some(DiagnosticCode::GeometryUnbounded)`, a message
    /// mentioning the arg name and the `Bounded` trait, and a `DiagnosticLabel`
    /// at the supplied span. This pins the diagnostic-shape contract
    /// independent of the conformance-walker integration (which is exercised
    /// end-to-end by the inference test file's positive case).
    #[test]
    fn emit_geometry_unbounded_helper_produces_error_with_code_and_label() {
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let span = SourceSpan::new(7, 19);
        emit_geometry_unbounded("g", span, &mut diagnostics);

        assert_eq!(
            diagnostics.len(),
            1,
            "emit_geometry_unbounded should push exactly one diagnostic"
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::GeometryUnbounded));
        assert!(
            d.message.contains("Bounded"),
            "message should mention the Bounded trait, got: {}",
            d.message
        );
        assert!(
            d.message.contains("'g'"),
            "message should mention the arg name 'g', got: {}",
            d.message
        );
        assert_eq!(
            d.labels.len(),
            1,
            "expected exactly one label attached at the supplied span"
        );
        assert_eq!(d.labels[0].span, span);
    }

    /// `emit_geometry_profile_required` pushes exactly one `Diagnostic` with
    /// severity `Error`, code `Some(DiagnosticCode::GeometryProfileRequired)`, a
    /// message naming both the arg and the `requirement`, and a single
    /// `DiagnosticLabel` at the supplied span. Pins the diagnostic-shape contract
    /// independent of the geometry.rs consumer wiring (which is exercised
    /// end-to-end in `geometry_profile_precondition_tests.rs`). Mirrors
    /// `emit_geometry_unbounded_helper_produces_error_with_code_and_label`; the
    /// `requirement` is parameterized so the one helper serves both the Surface
    /// profile and Curve path consumers.
    #[test]
    fn emit_geometry_profile_required_helper_produces_error_with_code_and_label() {
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let span = SourceSpan::new(7, 19);
        emit_geometry_profile_required(
            "g",
            "a 2D Surface profile (Closed, Planar)",
            span,
            &mut diagnostics,
        );

        assert_eq!(
            diagnostics.len(),
            1,
            "emit_geometry_profile_required should push exactly one diagnostic"
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::GeometryProfileRequired));
        assert!(
            d.message.contains("Surface"),
            "message should mention the requirement text, got: {}",
            d.message
        );
        assert!(
            d.message.contains("'g'"),
            "message should mention the arg name 'g', got: {}",
            d.message
        );
        assert_eq!(
            d.labels.len(),
            1,
            "expected exactly one label attached at the supplied span"
        );
        assert_eq!(d.labels[0].span, span);
    }

    /// `emit_geometry_trait_violation` pushes exactly one `Diagnostic` with severity
    /// `Error`, code `Some(DiagnosticCode::TypeNotConformingToTrait)`, the exact message
    /// `"geometry argument 'g' does not conform to trait 'Connected'"`, and a
    /// `DiagnosticLabel` at the supplied span. This is the symmetric sibling of
    /// `emit_geometry_unbounded` for the `Connected`/`Convex` cases.
    ///
    /// The full `assert_eq!` on `d.message` is the wording contract: it simultaneously
    /// confirms the arg name and trait name are present and that there is no redundant
    /// `"required by param"` suffix (the old inline branch repeated the arg name there).
    #[test]
    fn emit_geometry_trait_violation_helper_produces_error_with_code_and_label() {
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let span = SourceSpan::new(7, 19);
        emit_geometry_trait_violation("g", "Connected", span, &mut diagnostics);

        assert_eq!(
            diagnostics.len(),
            1,
            "emit_geometry_trait_violation should push exactly one diagnostic"
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::TypeNotConformingToTrait));
        assert_eq!(
            d.message, "geometry argument 'g' does not conform to trait 'Connected'",
            "message wording contract: arg name + trait name, no 'required by param' suffix"
        );
        assert_eq!(
            d.labels.len(),
            1,
            "expected exactly one label attached at the supplied span"
        );
        assert_eq!(d.labels[0].span, span);
    }

    /// Build a minimal `TopologyTemplate` for cycle-guard tests.
    ///
    /// The template has no realizations so `RealizationLetEnv::lookup` always
    /// falls through to the value_cells arm — the arm containing the recursive
    /// call to `infer_traits_for_expr_in_env(expr, self)`.  All other fields are
    /// zeroed/empty; they are irrelevant to the cycle-guard path.
    fn minimal_template(name: &str, cells: Vec<ValueCellDecl>) -> TopologyTemplate {
        TopologyTemplate {
            name: name.to_string(),
            doc: None,
            entity_kind: EntityKind::Structure,
            visibility: Visibility::Public,
            type_params: vec![],
            trait_bounds: vec![],
            value_cells: cells,
            constraints: vec![],
            realizations: vec![],
            sub_components: vec![],
            relations: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: HashSet::new(),
            objective: None,
            meta: HashMap::new(),
            content_hash: ContentHash(0),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
            assoc_fns: vec![],
            assoc_types: vec![],
        }
    }

    /// Build a `Let`-kind `ValueCellDecl` whose `default_expr` is a `ValueRef`
    /// pointing at `ref_id`.  Used to construct cycle fixtures in the tests below.
    fn let_cell_with_ref(id: ValueCellId, ref_id: ValueCellId) -> ValueCellDecl {
        ValueCellDecl {
            id,
            kind: ValueCellKind::Let,
            visibility: Visibility::Private,
            is_aux: false,
            cell_type: Type::dimensionless_scalar(),
            default_expr: Some(CompiledExpr::value_ref(
                ref_id,
                Type::dimensionless_scalar(),
            )),
            solver_hints: vec![],
            span: SourceSpan::empty(0),
        }
    }

    /// Cycle guard: a self-referential `let` binding (`let g : Real = ValueRef(g)`)
    /// must not overflow the stack; the in-flight visited set terminates the cycle
    /// and `lookup` returns the safe-default `Some(InferredTraits::all())`.
    ///
    /// # Why the value_cells path is exercised
    ///
    /// Entity "E" has no geometry realization for "g" (no realization in the
    /// template), so the realization arm is skipped and `lookup` falls through to
    /// the value_cells arm — the arm that contains the recursive call to
    /// `infer_traits_for_expr_in_env(expr, self)`.
    ///
    /// # Why `Type::dimensionless_scalar()`
    ///
    /// A non-geometry type confirms the fixture bypasses the realization arm
    /// and only exercises the value_cells fallback where the recursion lives.
    ///
    /// # Assertion rationale
    ///
    /// `Some(InferredTraits::all())` is the safe-default propagation chain:
    /// inner `lookup` hits the in-flight guard → returns `None` →
    /// `infer_traits_for_expr_in_env(...).unwrap_or(all())` returns `all()` →
    /// outer `lookup` wraps in `Some`.
    #[test]
    fn lookup_self_referential_let_returns_safe_default_without_overflow() {
        use std::cell::RefCell;

        // let g : Real = ValueRef(g)  — direct self-reference
        let g_id = ValueCellId::new("E", "g");
        let template = minimal_template("E", vec![let_cell_with_ref(g_id.clone(), g_id.clone())]);

        let mut templates = HashMap::new();
        templates.insert("E".to_string(), &template);

        let env = RealizationLetEnv {
            templates: &templates,
            in_flight: RefCell::new(Vec::new()),
        };

        assert_eq!(
            env.lookup(&g_id),
            Some(InferredTraits::all()),
            "self-referential let must safe-default to all() rather than stack-overflowing"
        );
    }

    /// Cycle guard: a chained-cycle let (`let g = ValueRef(h); let h = ValueRef(g)`)
    /// must not overflow the stack; the in-flight visited set terminates the cycle
    /// at re-entry and `lookup` returns the safe-default `Some(InferredTraits::all())`.
    ///
    /// Pins that the guard handles cycles longer than direct self-reference.
    /// The assertion shape is identical to the self-referential test; both produce
    /// `Some(InferredTraits::all())`.
    #[test]
    fn lookup_chained_cycle_let_returns_safe_default_without_overflow() {
        use std::cell::RefCell;

        // let g : Real = ValueRef(h)
        // let h : Real = ValueRef(g)
        let g_id = ValueCellId::new("E", "g");
        let h_id = ValueCellId::new("E", "h");
        let template = minimal_template(
            "E",
            vec![
                let_cell_with_ref(g_id.clone(), h_id.clone()),
                let_cell_with_ref(h_id.clone(), g_id.clone()),
            ],
        );

        let mut templates = HashMap::new();
        templates.insert("E".to_string(), &template);

        let env = RealizationLetEnv {
            templates: &templates,
            in_flight: RefCell::new(Vec::new()),
        };

        assert_eq!(
            env.lookup(&g_id),
            Some(InferredTraits::all()),
            "chained-cycle let must safe-default to all() rather than stack-overflowing"
        );
    }

    /// Non-cyclic chain: `let g = ValueRef(h); let h = difference(box(), box())`.
    ///
    /// Asserts that the in-flight guard does **not** short-circuit a non-cyclic
    /// chain.  `lookup(h)` must resolve to `bounded_only()` — the inferred traits
    /// of `difference` applied to two bounded primitives — and `lookup(g)` must
    /// propagate that result rather than safe-defaulting to `all()`.
    ///
    /// This test distinguishes a correct guard (fires only when `id` is already
    /// in the in-flight set) from a guard that fires too eagerly — for example,
    /// one that returns `None` whenever `in_flight` is non-empty regardless of
    /// which id is being resolved.  Without this test, both cycle tests pass even
    /// with such a premature-firing guard because they assert `Some(all())`.
    ///
    /// # Fixture rationale
    ///
    /// `h`'s `default_expr` is `FunctionCall("difference", [box(), box()])` with
    /// `result_type: Type::Geometry`.  `infer_traits_for_expr_in_env` dispatches
    /// on `expr.kind` (not `cell_type`), so the `FunctionCall` arm fires and
    /// returns `combine_difference(all(), all()) = bounded_only()`.
    #[test]
    fn lookup_chained_noncyclic_let_propagates_terminal_traits() {
        use std::cell::RefCell;

        // box_expr: FunctionCall("box", [], Geometry) → InferredTraits::all()
        let box_expr = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "box".to_string(),
                    qualified_name: "std::box".to_string(),
                },
                args: vec![],
            },
            result_type: Type::Geometry,
            content_hash: ContentHash(0),
        };

        // diff_expr: FunctionCall("difference", [box, box]) → bounded_only()
        let diff_expr = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "difference".to_string(),
                    qualified_name: "std::difference".to_string(),
                },
                args: vec![box_expr.clone(), box_expr],
            },
            result_type: Type::Geometry,
            content_hash: ContentHash(0),
        };

        let g_id = ValueCellId::new("E", "g");
        let h_id = ValueCellId::new("E", "h");

        // g: Let Real = ValueRef(h)  — bridges g into the value_cells fallback
        let g_cell = let_cell_with_ref(g_id.clone(), h_id.clone());
        // h: Let Real = difference(box(), box())  — terminal, resolves to bounded_only()
        let h_cell = ValueCellDecl {
            id: h_id,
            kind: ValueCellKind::Let,
            visibility: Visibility::Private,
            is_aux: false,
            cell_type: Type::dimensionless_scalar(),
            default_expr: Some(diff_expr),
            solver_hints: vec![],
            span: SourceSpan::empty(0),
        };

        let template = minimal_template("E", vec![g_cell, h_cell]);
        let mut templates = HashMap::new();
        templates.insert("E".to_string(), &template);

        let env = RealizationLetEnv {
            templates: &templates,
            in_flight: RefCell::new(Vec::new()),
        };

        assert_eq!(
            env.lookup(&g_id),
            Some(InferredTraits::bounded_only()),
            "non-cyclic chain g→h→difference(box,box) must propagate bounded_only(), \
             not short-circuit to all() via premature cycle-guard firing"
        );
    }

    /// Regression for task-2543 / task-2458: `check_trait_arg_conformance` must
    /// recurse into `ReflectiveCellList` elements the same way it recurses into
    /// `ListLiteral` elements.
    ///
    /// # Why this test lives here (and cannot move to `tests/*.rs`)
    ///
    /// `ReflectiveCellList` is produced exclusively at activation time by
    /// `expand_purpose_reflective_placeholders` (reify-eval), *after* compile-time
    /// conformance has already run — there is no `compile_source(...)` string that
    /// routes an RCL node into `check_trait_arg_conformance`. The fixture must be
    /// hand-built, which requires `pub(crate)` access to
    /// `check_trait_arg_conformance`. See the module-level comment above for the
    /// full rationale shared by all unit tests in this block.
    ///
    /// # RED before the fix (task-2543)
    ///
    /// The `(Type::List, ListLiteral)` arm in `walk_param_against_arg` does not
    /// match the `ReflectiveCellList` variant. The walker falls through to the
    /// type-level fallback; `walk_param_against_arg_type(List<MS>, List<MS>)` sees
    /// identical wrapper shapes and emits no diagnostic. The element's actual
    /// `StructureRef("NotAMaterial")` is never inspected → 0 diagnostics.
    ///
    /// # GREEN after the fix (task-2543)
    ///
    /// The merged `ListLiteral | ReflectiveCellList` arm iterates the elements.
    /// The leaf check sees `StructureRef("NotAMaterial")`, looks up "NotAMaterial"
    /// (no `MaterialSpec` in `trait_bounds`), and emits
    /// `TypeNotConformingToTrait`.
    ///
    /// # Fixture shape (design decision #3)
    ///
    /// The RCL `result_type` is set to `List<TraitObject("MaterialSpec")>` —
    /// identical to the param type — so the type-level fallback finds no
    /// wrapper-level mismatch. This is what makes the test RED before the fix:
    /// only the dedicated literal arm can see the element's actual type.
    #[test]
    fn walk_param_against_arg_recurses_into_reflective_cell_list_elements() {
        // ── Trait registry ────────────────────────────────────────────────────
        let material_spec = CompiledTrait {
            name: "MaterialSpec".to_string(),
            is_pub: true,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("MaterialSpec".to_string(), &material_spec);

        // ── Template registry ─────────────────────────────────────────────────
        // "Host" template with one param: `ms : List<MaterialSpec>`.
        let ms_cell = ValueCellDecl {
            id: ValueCellId::new("Host", "ms"),
            kind: ValueCellKind::Param,
            visibility: Visibility::Public,
            is_aux: false,
            cell_type: Type::List(Box::new(Type::TraitObject("MaterialSpec".to_string()))),
            default_expr: None,
            solver_hints: vec![],
            span: SourceSpan::empty(0),
        };
        let host_template = minimal_template("Host", vec![ms_cell]);

        // "NotAMaterial" template — no trait_bounds, so does NOT refine MaterialSpec.
        let not_a_material = minimal_template("NotAMaterial", vec![]);

        let mut template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        template_registry.insert("Host".to_string(), &host_template);
        template_registry.insert("NotAMaterial".to_string(), &not_a_material);

        // ── Hand-built RCL compiled arg ───────────────────────────────────────
        // RCL result_type intentionally matches the param type (List<MaterialSpec>)
        // so the type-level fallback finds no wrapper mismatch — the only way to
        // catch the element violation is via the dedicated literal arm (task-2543
        // design decision #3).
        let elem = CompiledExpr::value_ref(
            ValueCellId::new("E", "x"),
            Type::StructureRef("NotAMaterial".to_string()),
        );
        let rcl = CompiledExpr::reflective_cell_list(
            vec![elem],
            Type::List(Box::new(Type::TraitObject("MaterialSpec".to_string()))),
        );

        // ── Invoke the walker ─────────────────────────────────────────────────
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_trait_arg_conformance(
            "Host",
            "ms",
            &rcl,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        // ── Assertions ────────────────────────────────────────────────────────
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 diagnostic (RCL element 'NotAMaterial' does not conform \
             to 'MaterialSpec'), got {}: {diagnostics:?}",
            diagnostics.len(),
        );
        let d = &diagnostics[0];
        // task 5302 α (Option-A uniform downgrade): check_trait_arg_conformance is a
        // ctor-conformance entry, so its diagnostics are emitted at
        // CTOR_FIELD_CONFORMANCE_SEVERITY (Warning) rather than Error. Code/count/message
        // are unchanged; δ later flips the knob back to Error.
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToTrait),
            "expected TypeNotConformingToTrait, got {:?}",
            d.code,
        );
        assert!(
            d.message.contains("NotAMaterial"),
            "diagnostic message should mention 'NotAMaterial', got: {:?}",
            d.message,
        );
        assert!(
            d.message.contains("MaterialSpec"),
            "diagnostic message should mention 'MaterialSpec', got: {:?}",
            d.message,
        );
    }

    // ── task-4081 step-3: check_fn_arg_conformance unit tests ────────────────

    /// Build a marker trait with no requirements.
    fn marker_trait(name: &str) -> CompiledTrait {
        CompiledTrait {
            name: name.to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        }
    }

    /// Build a `TopologyTemplate` with the given `trait_bounds` and no value cells.
    fn template_with_bounds(name: &str, bounds: Vec<&str>) -> TopologyTemplate {
        TopologyTemplate {
            name: name.to_string(),
            doc: None,
            entity_kind: EntityKind::Structure,
            visibility: Visibility::Public,
            type_params: vec![],
            trait_bounds: bounds.into_iter().map(|s| s.to_string()).collect(),
            value_cells: vec![],
            constraints: vec![],
            realizations: vec![],
            sub_components: vec![],
            relations: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: HashSet::new(),
            objective: None,
            meta: HashMap::new(),
            content_hash: ContentHash(0),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
            assoc_fns: vec![],
            assoc_types: vec![],
        }
    }

    /// (a) NonConforming arg against DrivingJoint param → one TypeNotConformingToTrait.
    /// RED until step-4: `check_fn_arg_conformance` does not exist yet.
    #[test]
    fn fn_arg_conformance_non_conforming_emits_diagnostic() {
        let conforming = template_with_bounds("Conforming", vec!["DrivingJoint"]);
        let non_conforming = template_with_bounds("NonConforming", vec![]);
        let template_registry: HashMap<String, &TopologyTemplate> = [
            ("Conforming", &conforming),
            ("NonConforming", &non_conforming),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

        let dj = marker_trait("DrivingJoint");
        let trait_registry: HashMap<String, &CompiledTrait> = [("DrivingJoint", &dj)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "x"),
            Type::StructureRef("NonConforming".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::TraitObject("DrivingJoint".to_string()),
            "joint",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 TypeNotConformingToTrait diagnostic, got: {:?}",
            diagnostics
        );
        let d = &diagnostics[0];
        assert_eq!(d.code, Some(DiagnosticCode::TypeNotConformingToTrait));
        assert!(
            d.message.contains("NonConforming"),
            "diagnostic should mention 'NonConforming'; got: {}",
            d.message
        );
        assert!(
            d.message.contains("DrivingJoint"),
            "diagnostic should mention 'DrivingJoint'; got: {}",
            d.message
        );
    }

    /// (b) Conforming arg against DrivingJoint param → zero diagnostics.
    #[test]
    fn fn_arg_conformance_conforming_emits_no_diagnostic() {
        let conforming = template_with_bounds("Conforming", vec!["DrivingJoint"]);
        let template_registry: HashMap<String, &TopologyTemplate> = [("Conforming", &conforming)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let dj = marker_trait("DrivingJoint");
        let trait_registry: HashMap<String, &CompiledTrait> = [("DrivingJoint", &dj)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "x"),
            Type::StructureRef("Conforming".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::TraitObject("DrivingJoint".to_string()),
            "joint",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics for conforming arg, got: {:?}",
            diagnostics
        );
    }

    /// (c) Type::Error arg → zero diagnostics (anti-cascade).
    #[test]
    fn fn_arg_conformance_error_arg_no_cascade() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();

        let error_arg = CompiledExpr::literal(Value::Real(0.0), Type::Error);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::TraitObject("DrivingJoint".to_string()),
            "joint",
            &error_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Type::Error arg must not produce diagnostics (anti-cascade), got: {:?}",
            diagnostics
        );
    }

    /// (d) Bare trait param + `Type::TypeParam` (non-literal ValueRef) arg → zero
    /// diagnostics. An unresolved generic type variable forwarded to a trait param
    /// routes through the literal walker (`check_leaf_trait_conformance`); it is
    /// unverifiable here, not non-conforming, so it must be skipped rather than emit a
    /// false-positive `TypeNotConformingToTrait` (esc-4081 amend,
    /// reviewer_comprehensive robustness_false_positive).
    #[test]
    fn fn_arg_conformance_type_param_arg_skipped() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let dj = marker_trait("DrivingJoint");
        let trait_registry: HashMap<String, &CompiledTrait> = [("DrivingJoint", &dj)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        // A non-literal arg (ValueRef) whose result_type is an unresolved type param.
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "inner"),
            Type::TypeParam("T".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::TraitObject("DrivingJoint".to_string()),
            "joint",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "an unresolved TypeParam arg must not produce a conformance diagnostic \
             (unverifiable, not non-conforming), got: {:?}",
            diagnostics
        );
    }

    /// (e) Wrapper trait param `Option<DrivingJoint>` + `Option<TypeParam>`
    /// (non-literal ValueRef) arg → zero diagnostics. The wrapper case routes through
    /// the type-level walker (`walk_param_against_arg_type`); its `TypeParam` leaf must
    /// be skipped for the same reason as the bare case above.
    #[test]
    fn fn_arg_conformance_wrapped_type_param_arg_skipped() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let dj = marker_trait("DrivingJoint");
        let trait_registry: HashMap<String, &CompiledTrait> = [("DrivingJoint", &dj)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        // Non-literal `Option<T>` ValueRef against an `Option<DrivingJoint>` param.
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "inner"),
            Type::Option(Box::new(Type::TypeParam("T".to_string()))),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::Option(Box::new(Type::TraitObject("DrivingJoint".to_string()))),
            "joint",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "an unresolved Option<TypeParam> arg must not produce a conformance \
             diagnostic (unverifiable, not non-conforming), got: {:?}",
            diagnostics
        );
    }

    // ── Drift-guard: L2 joint-constructor name → PascalCase type mapping ──────

    /// Pin every arm of [`resolve_joint_nominal_type`]'s Path-B match so that
    /// adding a new joint kind to `reify-stdlib` forces an update here (and in
    /// the match itself).
    ///
    /// **Why this lives here (not in `tests/*.rs`):**
    /// `resolve_joint_nominal_type` is `fn` (private) — only accessible from
    /// `use super::*` inside this `mod tests` block.
    ///
    /// **DRIFT NOTE:** the canonical list of joint constructor builtins and their
    /// runtime `kind` strings lives in `reify-stdlib/src/joints.rs`
    /// (`JOINT_KINDS`, `pub(crate)`) — not directly importable from
    /// `reify-compiler`.  This behavioral test substitutes for a structural
    /// assertion against `JOINT_KINDS`.  A future kind addition requires updates
    /// in BOTH the Path-B match in `resolve_joint_nominal_type` AND this test.
    #[test]
    fn l2_joint_constructor_name_mapping_exhaustive() {
        // Build a minimal FunctionCall CompiledExpr for a given constructor name.
        // result_type = Type::dimensionless_scalar() (arbitrary — Path B ignores it and keeys on
        // the function.name string).
        let make_call = |name: &str| -> CompiledExpr {
            CompiledExpr {
                kind: CompiledExprKind::FunctionCall {
                    function: ResolvedFunction {
                        name: name.to_owned(),
                        qualified_name: format!("std::kinematic::{name}"),
                    },
                    args: vec![],
                },
                result_type: Type::dimensionless_scalar(),
                content_hash: ContentHash(0),
            }
        };

        // ── Non-driving coupling constructors → "Coupling" ────────────────────
        for ctor in ["couple", "gear", "screw", "rack_and_pinion"] {
            assert_eq!(
                resolve_joint_nominal_type(&make_call(ctor)).as_deref(),
                Some("Coupling"),
                "constructor '{ctor}' must resolve to 'Coupling'"
            );
        }

        // ── Non-driving fixed constructor → "Fixed" ───────────────────────────
        assert_eq!(
            resolve_joint_nominal_type(&make_call("fixed")).as_deref(),
            Some("Fixed"),
            "constructor 'fixed' must resolve to 'Fixed'"
        );

        // ── Driving joint constructors → their PascalCase kind names ──────────
        let driving = [
            ("prismatic", "Prismatic"),
            ("revolute", "Revolute"),
            ("cylindrical", "Cylindrical"),
            ("planar", "Planar"),
            ("spherical", "Spherical"),
        ];
        for (ctor, expected) in driving {
            assert_eq!(
                resolve_joint_nominal_type(&make_call(ctor)).as_deref(),
                Some(expected),
                "constructor '{ctor}' must resolve to '{expected}'"
            );
        }

        // ── Unknown constructor → None (skip, no diagnostic) ─────────────────
        assert_eq!(
            resolve_joint_nominal_type(&make_call("unknown_joint")).as_deref(),
            None,
            "unknown constructor must return None (not a recognized joint builtin)"
        );
    }

    // ── task-4584 step-1: walk_param_against_arg_type StructureRef leaf arm ──

    /// RED until step-2 (impl): `walk_param_against_arg_type` currently falls
    /// through the `_` arm silently for `Type::StructureRef` params, so no
    /// diagnostic is emitted even for clear mismatches like String-arg vs Part-param.
    ///
    /// (a) String-typed arg against StructureRef("Part") param → exactly one
    ///     `TypeNotConformingToStructureRef`.
    #[test]
    fn structureref_param_rejects_string_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(ValueCellId::new("Test", "x"), Type::String);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::StructureRef("Part".to_string()),
            "part",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 TypeNotConformingToStructureRef diagnostic for String arg \
             against Part param, got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToStructureRef),
            "expected TypeNotConformingToStructureRef, got {:?}",
            d.code,
        );
    }

    /// (b) Matching `StructureRef("Part")` arg against `StructureRef("Part")`
    ///     param → ZERO diagnostics (identity passes).
    #[test]
    fn structureref_param_accepts_same_structureref_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "x"),
            Type::StructureRef("Part".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::StructureRef("Part".to_string()),
            "part",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "expected ZERO diagnostics for Part arg vs Part param (identity), \
             got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// (c) Different `StructureRef("Other")` arg against `StructureRef("Part")`
    ///     param → exactly one `TypeNotConformingToStructureRef` (nominal mismatch).
    #[test]
    fn structureref_param_rejects_different_structureref_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "x"),
            Type::StructureRef("Other".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::StructureRef("Part".to_string()),
            "part",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 TypeNotConformingToStructureRef for Other vs Part, \
             got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToStructureRef),
            "expected TypeNotConformingToStructureRef, got {:?}",
            d.code,
        );
    }

    /// (d) `Type::TypeParam` and `Type::Error` args against a `StructureRef`
    ///     param → ZERO diagnostics (anti-cascade / unverifiable skip).
    #[test]
    fn structureref_param_skips_typeparam_and_error_args() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();

        // Type::TypeParam — unresolved generic, skip.
        let typeparam_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "x"),
            Type::TypeParam("T".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::StructureRef("Part".to_string()),
            "part",
            &typeparam_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "TypeParam arg must emit ZERO diagnostics (unverifiable), \
             got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );

        // Type::Error — anti-cascade, check_fn_arg_conformance returns early.
        let error_arg = CompiledExpr::value_ref(ValueCellId::new("Test", "y"), Type::Error);
        let mut diagnostics2: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::StructureRef("Part".to_string()),
            "part",
            &error_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics2,
        );
        assert_eq!(
            diagnostics2.len(),
            0,
            "Type::Error arg must emit ZERO diagnostics (anti-cascade early return), \
             got {}: {:?}",
            diagnostics2.len(),
            diagnostics2,
        );
    }

    /// Pin the intentional leniency in `check_param_default_conformance` for the
    /// `Type::StructureRef` arm: a `StructureRef("Other")` effective type in the
    /// default expression is conservatively **skipped** (zero diagnostics), even
    /// though the param's declared cell_type is `StructureRef("Part")`.
    ///
    /// ## Why this is intentional
    ///
    /// In Reify, a concrete structure type used as a default for a param that is
    /// declared with a *different* structure name is not necessarily incompatible —
    /// structural compatibility is assessed at evaluation time, not by a nominal
    /// name comparison alone (e.g. `param material : Material = Steel_AISI_1045()`
    /// is a common valid pattern).  The `check_param_default_conformance` arm
    /// therefore skips StructureRef effective types and only rejects clearly
    /// incompatible *primitive* types (String, Int, …).
    ///
    /// This contrasts with the constructor-arg path (`walk_param_against_arg_type`
    /// StructureRef arm) which uses `type_compatible` and does reject a
    /// `StructureRef("Other")` arg for a `StructureRef("Part")` param.  The two
    /// paths are deliberately asymmetric on this case; this test pins that gap so
    /// any future tightening is made consciously.
    #[test]
    fn structureref_param_default_with_different_structureref_silently_accepted() {
        // Build a Param cell: `param part : Part = <expr of type Other>`.
        let param_cell = ValueCellDecl {
            id: ValueCellId::new("Test", "part"),
            kind: ValueCellKind::Param,
            visibility: Visibility::Private,
            is_aux: false,
            cell_type: Type::StructureRef("Part".to_string()),
            default_expr: Some(CompiledExpr::value_ref(
                ValueCellId::new("OtherStruct", "instance"),
                Type::StructureRef("Other".to_string()),
            )),
            solver_hints: vec![],
            span: SourceSpan::empty(0),
        };
        let template = minimal_template("Test", vec![param_cell]);
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_param_default_conformance(
            &template,
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "StructureRef default for StructureRef param with different name must emit \
             ZERO diagnostics (intentional leniency — not a primitive mismatch), \
             got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    // ── task-4622: walk_param_against_arg_type Vector leaf arm ───────────────

    /// (a) Bare scalar arg against `Vector3<Length>` param →
    /// exactly one `TypeNotConformingToVector` Error.
    ///
    /// RED until S4 adds the `Type::Vector` arm to `walk_param_against_arg_type`.
    #[test]
    fn vector_param_rejects_scalar_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        // Bare Real scalar: `1.0`
        let compiled_arg =
            CompiledExpr::literal(reify_ir::Value::Real(1.0), Type::dimensionless_scalar());
        let param_type = Type::vec3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        });
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "axis",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 TypeNotConformingToVector diagnostic for scalar arg \
             against Vector3<Length> param, got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToVector),
            "expected TypeNotConformingToVector, got {:?}",
            d.code,
        );
    }

    /// (b) A dimensionless `Vector{n:3}` arg against `Vector3<Length>` param →
    /// ZERO diagnostics (locks the loose-quantity positive leg: a dimensionless
    /// vec3 is accepted for a Length-quantity param).
    ///
    /// RED until S4 adds the arm (before S4 the `_` arm silently produces 0;
    /// after S4 the arm explicitly accepts vector-shaped args → still 0, but now
    /// for the right structural reason). The positive-leg stays green before AND
    /// after S4; what changes is the REJECTION leg in test (a).
    #[test]
    fn vector_param_accepts_dimensionless_vector_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        // A dimensionless Vec3 arg: `vec3(0.0, 0.0, 1.0)` compiles to Vector{n:3, Real}.
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "v"),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
        );
        let param_type = Type::vec3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        });
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "axis",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "dimensionless Vector3 arg must be accepted for Vector3<Length> param \
             (loose-quantity convention), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// (c) A `Vector{n:2}` (vec2) arg against `Vector3<Length>` param →
    /// exactly one `TypeNotConformingToVector` Error (arity mismatch).
    ///
    /// Locks the arity-check added in the amendment pass: shape-based conformance now
    /// also requires matching `n` for `Type::Vector` args. `vec2` is a real mismatch
    /// for a `vec3`-typed param and must be rejected at compile time.
    #[test]
    fn vector_param_rejects_wrong_arity_vector_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        // A vec2 arg: Vector{n:2, dimensionless} — correct shape, wrong arity.
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "v"),
            Type::Vector {
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
        );
        let param_type = Type::vec3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        });
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "axis",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 TypeNotConformingToVector for Vector2 arg vs \
             Vector3<Length> param (arity mismatch), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToVector),
            "expected TypeNotConformingToVector, got {:?}",
            d.code,
        );
    }

    /// Loose-quantity positive leg of the `Type::Point` arm (task 5465,
    /// family 1): a DIMENSIONLESS `Point{n:3}` arg is accepted for a
    /// `Point3<Length>` param.
    ///
    /// Pins the ty.rs "Point / Vector quantity-slot convention" — the resolver
    /// always wraps `Q` in `Type::Scalar` while `Value::Point::infer_type` may
    /// yield a dimensionless or `Int` quantity, and the looseness is
    /// intentional. Sibling of `vector_param_accepts_dimensionless_vector_arg`.
    #[test]
    fn point_param_accepts_dimensionless_point_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "p"),
            Type::Point {
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
        );
        let param_type = Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        });
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "origin",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "dimensionless Point3 arg must be accepted for Point3<Length> param \
             (loose-quantity convention), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// CROSS-DIMENSION leg of the `Type::Point` arm (task 5766): a `Point{n:3}`
    /// arg whose quantity slot names a CONCRETE dimension different from the
    /// param's is exactly one `ArgTypeMismatch`.
    ///
    /// **Why this is an in-module unit test and not a `.ri` fixture.**
    /// `resolve_parameterized_builtin_type` recognises `Point3` only, and
    /// `point3(…)` is an eval-builtin with no `.ri` return type — its calls
    /// compile to a `Scalar[m]` / `Int` placeholder. No `.ri` source can
    /// therefore produce a *dimensioned* `Type::Point` arg, so the `Type` is
    /// constructed directly, exactly as the adjacent arity and dimensionless
    /// probes do (see `struct_ctor_field_conformance_tests.rs`'s own note on this).
    ///
    /// The complement of `point_param_accepts_dimensionless_point_arg` directly
    /// above: that one pins the TOLERANT half (either side declines to name a
    /// concrete dimension ⇒ silent), this one pins the STRICT half (both name one
    /// and they differ ⇒ reject).
    #[test]
    fn point_param_rejects_cross_dimension_point_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "p"),
            Type::Point {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::MASS,
                }),
            },
        );
        let param_type = Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        });
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "origin",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "a Point3<Mass> arg at a Point3<Length> param must be rejected — both sides \
             name a concrete dimension and they disagree (task 5766), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        assert_eq!(
            diagnostics[0].code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "expected ArgTypeMismatch, got {:?}",
            diagnostics[0].code,
        );
    }

    /// CROSS-DIMENSION leg of the `Type::Vector` arm at the direct-`Type` seam
    /// (task 5766), sibling of `point_param_rejects_cross_dimension_point_arg`.
    ///
    /// The `.ri` seam for the same rule is
    /// `vec3_cross_dimension_at_dimensioned_vector_param_warns_arg_type_mismatch`
    /// in `struct_ctor_field_conformance_tests.rs`. Pinning BOTH seams matters
    /// because they reach the arm by different routes — a `ValueRef` carrying a
    /// persisted `Type::Vector` here, versus a `FunctionCall`'s inferred
    /// `result_type` there — and only the type-level walker sees the former.
    ///
    /// The code assertion is the load-bearing half: a quantity conflict must
    /// route to `ArgTypeMismatch`, NOT to this arm's bespoke
    /// `TypeNotConformingToVector`, which keeps owning ARITY/FAMILY failures only
    /// (`vector_param_rejects_wrong_arity_vector_arg` pins that side).
    #[test]
    fn vector_param_rejects_cross_dimension_vector_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "v"),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::MASS,
                }),
            },
        );
        let param_type = Type::vec3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        });
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "axis",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "a Vector3<Mass> arg at a Vector3<Length> param must be rejected — both sides \
             name a concrete dimension and they disagree (task 5766), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        assert_eq!(
            diagnostics[0].code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "a QUANTITY conflict must route to ArgTypeMismatch, leaving \
             TypeNotConformingToVector owning ARITY/FAMILY only, got {:?}",
            diagnostics[0].code,
        );
    }

    /// Shared scaffold for the task 5766 boundary fences below: assert that
    /// `arg_ty` at `param_type` produces NO diagnostic.
    ///
    /// Same empty-registry `check_fn_arg_conformance` scaffold as the probes
    /// above; factored out only because the four fences differ solely in their
    /// two `Type`s and their reason.
    fn assert_quantity_slot_clean(param_type: Type, arg_ty: Type, arg_name: &str, why: &str) {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(ValueCellId::new("Test", "a"), arg_ty);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            arg_name,
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "{why}\ngot {} diagnostic(s): {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// FENCE (a-i), task 5766 unknown-ness boundary: a dimension-GENERIC param
    /// slot accepts a concretely-dimensioned arg.
    ///
    /// `Vec3<Q: Dimension>` (`stdlib/trajectory.ri:103`) resolves its quantity to
    /// `Type::ScalarParam("Q")`. A generic slot names no dimension, so there is
    /// nothing to conflict with and the rule must stay silent — otherwise every
    /// instantiation of a dimension-generic vector wrapper would warn.
    #[test]
    fn generic_quantity_param_accepts_dimensioned_vector_arg() {
        assert_quantity_slot_clean(
            Type::vec3(Type::ScalarParam("Q".to_string())),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            },
            "axis",
            "a Vector3<Length> arg at a dimension-GENERIC Vector3<Scalar<Q>> param must stay \
             CLEAN — a ScalarParam slot names no concrete dimension, so quantity_slot_dimension \
             yields None and there is nothing to conflict with (task 5766).",
        );
    }

    /// FENCE (a-ii), the SAME boundary in the opposite direction: a concretely
    /// dimensioned param slot accepts a dimension-GENERIC arg.
    ///
    /// Pinned separately from (a-i) because a rule that compared only one side
    /// against `None` would pass (a-i) and fail here. Together they pin that
    /// `quantity_slots_conflict` requires BOTH sides to name a dimension —
    /// which is also how the `is_numeric_placeholder_leaf` unknown-ness fence
    /// (D4-5) survives at these arms.
    #[test]
    fn dimensioned_quantity_param_accepts_generic_vector_arg() {
        assert_quantity_slot_clean(
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::ScalarParam("Q".to_string())),
            },
            "axis",
            "a dimension-GENERIC Vector3<Scalar<Q>> arg at a Vector3<Length> param must stay \
             CLEAN — the rule requires BOTH sides to name a concrete dimension before it can \
             fire (task 5766); widening it here would re-open the D4-5 unknown-ness hole.",
        );
    }

    /// FENCE (b), task 5766: an `Int` quantity slot stays CLEAN.
    ///
    /// Pins the ty.rs convention's statement that `Value::Vector::infer_type()`
    /// may yield a `Type::Int` quantity for a unit-less component vector — the
    /// shape `vec3(0, 1, 0)` takes. `Type::Int` names no dimension, so the rule
    /// is silent for the same structural reason as the dimensionless leg.
    #[test]
    fn int_quantity_vector_arg_at_dimensioned_param_stays_clean() {
        assert_quantity_slot_clean(
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Int),
            },
            "axis",
            "a Vector3<Int> arg at a Vector3<Length> param must stay CLEAN — the ty.rs \
             quantity-slot convention records that Value::Vector::infer_type() may yield an \
             Int quantity, and Int names no dimension (task 5766).",
        );
    }

    /// FENCE (c) — THE `Field` HOLD. This is a DECISION, not an oversight.
    ///
    /// `Type::Field` is not on the same axis as the four families task 5766
    /// tightens: **it has no quantity slot at all.** It carries `domain` and
    /// `codomain` `Type` slots, and BOTH erase to `Real` at every call site —
    /// `examples/fea_shell_channels.ri` supplies `Real -> Real` analytical field
    /// defs to `solver_elastic.ri`'s `Field<Point3<Length>, Tensor<2,3,Pressure>>`
    /// slots, and the arm's own comment records that comparing the declaration
    /// against that placeholder produced six false warnings before the arm
    /// existed. There is literally nothing for a quantity rule to compare, so a
    /// `Field` tightening would be zero-signal and non-zero-risk.
    ///
    /// This test exists so that a later session noticing the asymmetry with the
    /// other four arms finds the RULING here instead of "fixing" it by false
    /// symmetry. The normative statement is in `crates/reify-core/src/ty.rs`.
    /// The `.ri`-level leg of the same claim is already pinned by
    /// `field_param_given_erased_analytical_field_stays_clean` in
    /// `struct_ctor_field_conformance_tests.rs`.
    #[test]
    fn field_param_holds_loose_against_fully_erased_field_arg() {
        assert_quantity_slot_clean(
            Type::Field {
                domain: Box::new(Type::point3(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                })),
                codomain: Box::new(Type::tensor(
                    2,
                    3,
                    Type::Scalar {
                        dimension: DimensionVector::PRESSURE,
                    },
                )),
            },
            Type::Field {
                domain: Box::new(Type::dimensionless_scalar()),
                codomain: Box::new(Type::dimensionless_scalar()),
            },
            "stress",
            "THE FIELD HOLD IS A DECISION, NOT AN OVERSIGHT (task 5766): a \
             Field<Point3<Length>, Tensor<2,3,Pressure>> param fed a Field<Real, Real> arg must \
             stay CLEAN. Field has NO quantity slot — it has domain/codomain Type slots, both of \
             which erase to Real at every call site (examples/fea_shell_channels.ri supplies \
             Real -> Real analytical fields to solver_elastic.ri's dimensioned codomain slots), \
             so there is nothing for a quantity rule to compare. Do NOT 'fix' this by symmetry \
             with the Vector/Point/Matrix/Tensor arms; see the quantity-slot convention block in \
             crates/reify-core/src/ty.rs.",
        );
    }

    /// Arity leg of the `Type::Point` arm (task 5465, family 1): a `Point{n:2}`
    /// arg against a `Point3<Length>` param is exactly one `ArgTypeMismatch`.
    ///
    /// **Why this is an in-module unit test and not an integration probe in
    /// `struct_ctor_field_conformance_tests.rs` (where the other four Point
    /// probes live).** The surface language has no `Point2` spelling —
    /// `resolve_parameterized_builtin_type` recognises `Point3` only
    /// (`type_resolution.rs:3192`) — so no `.ri` source can produce a
    /// `Type::Point { n: 2, .. }` arg and the arity rule is unreachable from
    /// inline-source fixtures. Constructing the `Type` directly is the only way
    /// to pin it. Sibling of `vector_param_rejects_wrong_arity_vector_arg`,
    /// which exists for the same reason.
    #[test]
    fn point_param_rejects_wrong_arity_point_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "p"),
            Type::Point {
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
        );
        let param_type = Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        });
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "origin",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 ArgTypeMismatch for Point2 arg vs Point3<Length> param \
             (arity mismatch), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        assert_eq!(
            diagnostics[0].code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "expected ArgTypeMismatch, got {:?}",
            diagnostics[0].code,
        );
    }

    /// Self-accept leg of the `Matrix`/`Tensor` arm (task 5465, family 2): a
    /// genuinely `Type::Matrix`-typed value cell at a `Matrix` param.
    ///
    /// The family's integration probes all exercise the LOOSE accepts — the
    /// nested-list-literal spelling and the `Vector` → `Tensor<1,…>` conversion —
    /// so nothing pinned that the nominal family accepts ITSELF. Note the
    /// deliberate m/n difference (`Matrix<2,2>` arg at a `Matrix<3,3>` param):
    /// the arm applies NO arity check, because `List<List<Real>>` — the
    /// idiomatic spelling — carries no element counts, so an arity rule would be
    /// unenforceable for exactly the shape that matters. This test is what makes
    /// that asymmetry with the `Point`/`Vector` arms a pinned fact rather than
    /// an unstated consequence.
    #[test]
    fn matrix_param_accepts_matrix_arg_without_arity_check() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "m"),
            Type::Matrix {
                m: 2,
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
        );
        let param_type = Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        };
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "inertia",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "a Matrix-typed arg must be accepted at a Matrix param (nominal family self-accept, \
             loose quantity, NO arity check), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// Lambda leg of the `Type::Field` arm (task 5465, family 3): a
    /// `Type::Function` arg is a legitimate spelling of a field-shaped arg at a
    /// `Field` slot.
    ///
    /// That accept is an explicit, deliberate branch in the arm and had zero
    /// coverage — nothing would have noticed it being dropped, or the arm being
    /// rewritten to reject lambdas. The integration probes for this family cover
    /// only the erased-`field def` accept and the `String` value floor.
    #[test]
    fn field_param_accepts_function_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "f"),
            Type::Function {
                params: vec![Type::dimensionless_scalar()],
                return_type: Box::new(Type::dimensionless_scalar()),
            },
        );
        let param_type = Type::Field {
            domain: Box::new(Type::point3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
            codomain: Box::new(Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
        };
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "mode_shape",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "a lambda (Type::Function) arg must be accepted at a Field param, got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// A minimal declared-enum table for the two erasure-symmetry probes below.
    ///
    /// Only `EnumDef::name` is load-bearing here — [`base_enum_name`]'s
    /// membership test is what distinguishes an applied ENUM from an applied
    /// generic STRUCTURE (`Coupling<T>`), and it reads nothing else.
    fn enum_table(names: &[&str]) -> Vec<reify_ir::EnumDef> {
        names
            .iter()
            .map(|n| reify_ir::EnumDef {
                name: (*n).to_string(),
                variants: vec![],
                doc: None,
                type_params: vec![],
            })
            .collect()
    }

    /// REVERSE leg of the enum-erasure tolerance (task 5465 amendment): a param
    /// declared as the BARE `Type::Enum("Result")` supplied an arg whose
    /// `result_type` is the APPLIED `Type::Applied { name: "Result", .. }` must
    /// be accepted.
    ///
    /// **Why this is an in-module unit test.** The integration probes in
    /// `struct_ctor_field_conformance_tests.rs` cover the forward leg (applied
    /// param ← bare-enum arg), which is what enum erasure produces from a
    /// constructed variant. This pairing is the mirror image, reachable from a
    /// `ValueRef` to a cell declared with the applied annotation or from a fn
    /// whose return annotation is the applied form (cf.
    /// `examples/m6_result_recovery.ri:30`); constructing the two `Type`s
    /// directly pins it without depending on which surface spellings happen to
    /// survive inference today.
    ///
    /// It is a REGRESSION fence, not a new capability: the pairing was silent
    /// before `Type::Enum(_)` was promoted, and the first cut of family 4 made
    /// it a spurious `ArgTypeMismatch` because `enum_payload_compatible` matches
    /// only a bare `Type::Enum` on the supplied side. Routing both sides through
    /// [`base_enum_name`] restored the acceptance symmetrically. `Severity` here
    /// is `Error` (this entry point is the fn-call path), which is exactly why
    /// the false positive mattered: it failed the compile rather than warning.
    #[test]
    fn enum_param_accepts_applied_enum_arg_of_same_base() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let enum_defs = enum_table(&["Result"]);
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "r"),
            Type::Applied {
                name: "Result".to_string(),
                args: vec![
                    Type::Scalar {
                        dimension: DimensionVector::LENGTH,
                    },
                    Type::String,
                ],
            },
        );
        let param_type = Type::Enum("Result".to_string());
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "r",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &enum_defs,
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "a bare Enum(\"Result\") param given an Applied{{\"Result\"}} arg must be accepted — \
             the erasure tolerance is symmetric, and type_compatible has no Applied-vs-Enum arm \
             in EITHER direction. Got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// Fence on the reverse leg: symmetry must not become a blanket enum bypass.
    ///
    /// A bare `Enum("Hue")` param supplied an applied `Result<…>` arg resolves
    /// to two DIFFERENT base names, so it must still be exactly one
    /// `ArgTypeMismatch` — the same verdict the forward-direction cross-enum
    /// probe (`enum_param_given_wrong_enum_warns_arg_type_mismatch`) pins.
    #[test]
    fn enum_param_rejects_applied_enum_arg_of_different_base() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let enum_defs = enum_table(&["Hue", "Result"]);
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "r"),
            Type::Applied {
                name: "Result".to_string(),
                args: vec![Type::String, Type::String],
            },
        );
        let param_type = Type::Enum("Hue".to_string());
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "c",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &enum_defs,
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 ArgTypeMismatch for Applied{{\"Result\"}} arg vs Enum(\"Hue\") \
             param (different base enums), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        assert_eq!(
            diagnostics[0].code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "expected ArgTypeMismatch, got {:?}",
            diagnostics[0].code,
        );
    }

    /// Fence on [`base_enum_name`]'s `enum_defs` membership test at THIS arm: an
    /// applied generic STRUCTURE (`Coupling<T>`) is spelled `Type::Applied` too,
    /// and must NOT be treated as an enum.
    ///
    /// With "Coupling" absent from the enum table both sides resolve to `None`,
    /// so the leaf falls through to `general_leaf_param_family_is_validated`,
    /// which does not vet `Type::Applied` — the pre-5465 silence for applied
    /// generic structures is preserved. Without the membership test this would
    /// instead take the enum branch.
    #[test]
    fn applied_generic_structure_param_is_not_treated_as_enum() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let enum_defs = enum_table(&["Result"]);
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "c"),
            Type::Enum("Coupling".to_string()),
        );
        let param_type = Type::Applied {
            name: "Coupling".to_string(),
            args: vec![Type::String],
        };
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "c",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &enum_defs,
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "an applied generic STRUCTURE param must stay outside the enum branch and fall \
             through the allowlist silently (pre-5465 behaviour), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }
}
