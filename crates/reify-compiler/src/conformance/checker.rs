use crate::*;

/// Tag used when cross-checking requirements against available defaults.
/// A `param` requirement can only be satisfied by a `param` default, and a `let`
/// requirement only by a `let` default. A kind mismatch is treated the same as "no
/// default" so the user sees "missing required member" rather than a confusing
/// kind-mismatch error (the fix is the same either way: provide the member).
///
/// See also `DefaultKindTag` (module-level) — this enum intentionally omits
/// `Constraint` because constraints are never candidates for satisfying requirements.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum AvailableDefaultKind {
    Param,
    Let,
}

/// Centralised "annotation wins, else inferred, else Type::Error" rule for let defaults.
///
/// Both call sites in the trait-conformance pipeline share this three-way precedence:
/// 1. **`check_phase_build_available_defaults_map`** (site 1): passes
///    `inferred_let_exprs.get(&(name.to_string(), AvailableDefaultKind::Let))` as
///    `inferred`; names in `pass2_compile_errors` or `pass2_skipped` are excluded
///    before reaching this helper.
/// 2. **`check_phase_inject_defaults`** (site 2): passes `Some(&compiled_expr)` because
///    the injection loop already has the compiled expression in hand.
///
/// After task 1914 suggestion #1 (debug_assert at site 1) and task 3749 (this tightening),
/// the `Type::Error` fallback arm is anti-cascade defense in depth: actual callers either
/// hold an annotation (`cell_type` is `Some`) or have a valid inferred expression (the
/// `pass2_compile_errors` filter excludes compile-error names before this helper is reached);
/// the `debug_assert!` at site 1 fires in dev to catch drift, and the `Type::Error` fallback
/// ensures release-mode safety if the assert is bypassed.
pub(super) fn resolve_let_advertised_type(
    cell_type: &Option<Type>,
    inferred: Option<&CompiledExpr>,
) -> Type {
    cell_type.clone().unwrap_or_else(|| {
        inferred
            .map(|e| e.result_type.clone())
            .unwrap_or(Type::Error)
    })
}

/// Phase 1 of trait conformance checking: resolve structure member types and collect
/// constraint labels.
///
/// Builds three outputs from the structure's member list:
/// - `structure_param_members`: a `HashMap<String, Type>` mapping each **param** member name
///   to its resolved type. Only `MemberDecl::Param` entries are included here.
/// - `structure_let_members`: a `HashMap<String, Type>` mapping each **let** member name to
///   its resolved type. Let bindings are only included when they carry an explicit type
///   annotation; unannotated lets are omitted here and handled by the pre-register pass
///   (phase 3).
/// - `structure_constraint_labels`: a `HashSet<String>` of constraint label names, used
///   by phase 6 to detect member overrides before injecting trait defaults.
///
/// ## Kind separation rationale
///
/// Keeping param and let members in separate maps enables **kind-aware requirement lookup**
/// in phase 5 (`check_phase_check_members_against_requirements`): a `param` requirement can
/// only be satisfied by a structure `param` member, and a `let` requirement only by a
/// structure `let` member. When both maps are needed (e.g. for "does the structure override
/// this name?" checks in phases 2, 3, and 6), the caller merges them by `.chain()`-ing the
/// two iterators into a combined map.
///
/// # Type resolution order
///
/// For each Named type annotation the closure calls `resolve_type_with_aliases` (builtin →
/// alias registry → trait-name fallback) and then checks `enum_defs` for a matching enum.
/// Unresolved names and dimensional-op annotations emit a root-cause diagnostic and return
/// `Type::Error` (poison sentinel) to suppress cascade "type mismatch" errors downstream
/// via the asymmetric producer-side wildcard in `type_compat.rs:3–26`.
pub(super) fn check_phase_resolve_structure_members(
    structure: &EntityDefRef<'_>,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    enum_defs: &[reify_ir::EnumDef],
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> (
    HashMap<String, Type>,
    HashMap<String, Type>,
    HashSet<String>,
) {
    // Collect all structure member names for conformance checking.
    let empty_params: HashSet<String> = HashSet::new();
    // Build a HashSet of enum names once (O(E)) so the filter_map below performs
    // O(1) membership checks per member instead of a fresh O(E) scan each time.
    let enum_names: HashSet<&str> = enum_defs.iter().map(|e| e.name.as_str()).collect();

    // Shared resolution logic for Param and Let type annotations in the filter_map.
    // Receives `diagnostics` as an explicit parameter (rather than capturing it) so
    // the filter_map closure can also push to `diagnostics` for the "missing annotation"
    // case without a mutable-borrow conflict.
    //
    // Control flow:
    //   1. Early-reject `DimensionalOp` with the historical "unexpected dimensional
    //      expression" wording (the resolver silently returns None for it).
    //   2. Early-reject `IntegerLiteral` — the resolver pushes its own
    //      "integer literal `N` is only allowed as a type argument of Tensor or Matrix"
    //      diagnostic and returns None; without an early skip we would emit a second,
    //      less-useful "unknown type name" cascade.
    //   3. Otherwise call `resolve_type_expr_with_aliases` (which handles parameterized
    //      builtins like `Option<T>`, `List<T>`, parametric aliases, structure/trait
    //      names). On `None`, fall back to enum-name lookup, then emit a root-cause
    //      "unresolved type in conformance check" diagnostic.
    //
    // All error paths return `Type::Error` (poison sentinel), NOT `Type::Real`.
    // Rationale: `structure_members` (populated by this closure's output) is consumed
    // by the `RequirementKind::{Param,Let}` arm of the requirement-checking loop below,
    // where `actual_type` is passed as the `from`/producer side of
    // `implicitly_converts_to(actual_type, expected_type)`. The asymmetric producer-side
    // wildcard in `type_compat.rs:3–26` short-circuits `implicitly_converts_to(Error, _)`
    // to `true`, suppressing the cascade "type mismatch for trait member" diagnostic
    // that would otherwise appear on top of the root-cause error already emitted here.
    // Returning `Type::Real` instead would poison the downstream requirement check
    // whenever the trait requires a non-Real type (e.g. Length), generating a misleading
    // second diagnostic and obscuring the actual problem for the user.
    //
    // Switching from the simple-name `resolve_type_with_aliases` to the full
    // `resolve_type_expr_with_aliases` is the task-2908 fix: the old path never consulted
    // `type_args`, so `param x : Option<Pressure>` on a conforming structure was rejected
    // as "unresolved type" even though the same shape worked elsewhere. Mirrors the
    // parallel fix in `traits.rs` (commit 10481423b2).
    let resolve_member_annotation_type = |te: &reify_ast::TypeExpr,
                                          diagnostics: &mut Vec<Diagnostic>|
     -> Type {
        match &te.kind {
            reify_ast::TypeExprKind::DimensionalOp { .. } => {
                diagnostics.push(
                    Diagnostic::error(format!("unresolved type in conformance check: {}", te))
                        .with_code(DiagnosticCode::UnresolvedType)
                        .with_label(DiagnosticLabel::new(
                            te.span,
                            "unexpected dimensional expression",
                        )),
                );
                return Type::Error;
            }
            reify_ast::TypeExprKind::IntegerLiteral(_) => {
                // Let the resolver emit its specific "integer literal N is only
                // allowed as a type argument of Tensor or Matrix" diagnostic by
                // calling it once for its side effect, then return Error without
                // adding a second cascade diagnostic.
                let _ = resolve_type_expr_with_aliases(
                    te,
                    &empty_params,
                    alias_registry,
                    diagnostics,
                    structure_names,
                    trait_names,
                );
                return Type::Error;
            }
            _ => {}
        }
        match resolve_type_expr_with_aliases(
            te,
            &empty_params,
            alias_registry,
            diagnostics,
            structure_names,
            trait_names,
        ) {
            Some(t) => t,
            None => {
                if let reify_ast::TypeExprKind::Named { name, type_args } = &te.kind
                    && enum_names.contains(name.as_str())
                {
                    if !type_args.is_empty() {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "enum `{}` does not accept type arguments",
                                name
                            ))
                            .with_label(DiagnosticLabel::new(
                                te.span,
                                "enum types are not generic",
                            )),
                        );
                    }
                    Type::Enum(name.to_string())
                } else {
                    diagnostics.push(
                        Diagnostic::error(format!("unresolved type in conformance check: {}", te))
                            .with_code(DiagnosticCode::UnresolvedType)
                            .with_label(DiagnosticLabel::new(te.span, "unknown type name")),
                    );
                    Type::Error
                }
            }
        }
    };

    // Build separate maps for param and let members so phase 5 can perform
    // kind-aware lookups: a `param` requirement must be satisfied by a structure
    // `param`, not a `let` (which is a computed/derived slot, not externally settable).
    let mut structure_param_members: HashMap<String, Type> = HashMap::new();
    let mut structure_let_members: HashMap<String, Type> = HashMap::new();

    for m in structure.members.iter() {
        match m {
            reify_ast::MemberDecl::Param(p) => {
                let ty = match p.type_expr.as_ref() {
                    Some(te) => resolve_member_annotation_type(te, diagnostics),
                    None => {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "trait member '{}' has no type annotation; cannot infer type",
                                p.name
                            ))
                            .with_label(DiagnosticLabel::new(p.span, "missing type annotation")),
                        );
                        Type::Real
                    }
                };
                structure_param_members.insert(p.name.clone(), ty);
            }
            reify_ast::MemberDecl::Let(l) => {
                // let bindings get their type from expression inference, not annotations.
                // Only include in structure_let_members when there is an explicit type
                // annotation; omitting is safe because if a trait requires this member,
                // the conformance check will report "missing required member" rather than
                // a spurious "no type annotation" error.
                if let Some(te) = l.type_expr.as_ref() {
                    let ty = resolve_member_annotation_type(te, diagnostics);
                    structure_let_members.insert(l.name.clone(), ty);
                }
            }
            _ => {}
        }
    }

    // Collect structure constraint labels.
    let structure_constraint_labels: HashSet<String> = structure
        .members
        .iter()
        .filter_map(|m| {
            if let reify_ast::MemberDecl::Constraint(c) = m {
                c.label.clone()
            } else {
                None
            }
        })
        .collect();

    (
        structure_param_members,
        structure_let_members,
        structure_constraint_labels,
    )
}

/// Phase 2 of trait conformance checking: collect all requirements and defaults from
/// all trait bounds in the structure's trait bound list.
///
/// Creates a fresh `MergeContext` and calls `collect_all_requirements` for each trait bound,
/// which recursively walks refinement chains and deduplicates requirements/defaults across
/// the full bound set. The returned `MergeContext` carries both `requirements` and `defaults`
/// for use by later phases.
///
/// `MergeContext` bundles the output accumulators (`requirements`, `defaults`) and the 5 mutable
/// tracking maps (`visited`, `seen_names`, `seen_defaults`, `seen_let_hashes`,
/// `seen_let_conflict_names`) so the recursive `collect_all_requirements` signature stays
/// within Clippy's argument-count limit.
pub(super) fn check_phase_collect_trait_bounds(
    structure: &EntityDefRef<'_>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    structure_members: &HashMap<String, Type>,
    diagnostics: &mut Vec<Diagnostic>,
) -> MergeContext {
    let mut ctx = MergeContext::new();

    for trait_bound in structure.trait_bounds {
        collect_all_requirements(
            &trait_bound.name,
            trait_registry,
            &mut ctx,
            structure_members,
            structure.span,
            0,
            diagnostics,
        );
    }

    ctx
}

/// Phase 3 of trait conformance checking: pre-register default types into the compilation scope.
///
/// Implements a two-pass pre-registration strategy:
/// - **Pass 1** registers every *annotated* default (Param + Let with `Some(cell_type)`) into
///   `scope` using `register_if_absent`. No expression compilation happens here, so ordering
///   within `ctx.defaults` does not matter for the annotated types made visible to Pass 2.
/// - **Pass 2** compiles each *unannotated* Let's expression against the fully-populated
///   annotated scope from Pass 1, caches the compiled expression in `inferred_let_exprs`, and
///   registers the inferred `result_type`. When `register_if_absent` finds the scope slot
///   already claimed (Pass 1 registered an annotated Param or Let), the name is added to
///   `pass2_skipped` and the Let-cell injection is suppressed in phase 6.
///
/// # INVARIANT for `inferred_let_exprs` composite key
///
/// `collect_all_requirements` deduplicates defaults by (name, kind) across the trait-bound
/// set, so at most one unannotated-let default with a given name reaches this loop. The
/// composite `(String, AvailableDefaultKind)` key makes cross-kind collisions structurally
/// impossible: a `Param`-named `x` and a `Let`-named `x` occupy distinct slots by type.
///
/// **Currently only `AvailableDefaultKind::Let` is inserted or read** — the `Param` arm is
/// never used here because only unannotated `Let` defaults are compiled in Pass 2. The composite
/// key is kept for structural symmetry with `available_defaults` (which uses the same
/// `(String, AvailableDefaultKind)` shape) and to reserve per-kind slots without a cache
/// redesign if a future pass adds `Param`-inference.
/// TODO(future-kinds): revert to `HashMap<String, CompiledExpr>` if no second kind is added.
///
/// # PASS 2 COMPILE-ERROR SUPPRESSION (`pass2_compile_errors`, task 1914 / task 2158)
///
/// Pass 2 snapshots the diagnostic-vector length before each `compile_expr` call, then scans
/// only the newly-appended tail for `Severity::Error`.  When at least one Error-severity
/// diagnostic is found in the tail, the expression itself failed to compile (e.g. unresolved
/// forward reference, unknown unit).  In that case the name is recorded in
/// `pass2_compile_errors` and **excluded from `inferred_let_exprs`**.  The scope slot is
/// additionally **poisoned with `Type::Error`** via `register_if_absent(name, Type::Error)` so
/// that sibling unannotated-let expressions referencing this name resolve to the anti-cascade
/// sentinel rather than emitting a fresh "unresolved name" cascade.
/// `implicitly_converts_to(Error, _) -> true` and `infer_binop_type`'s leading Error guard
/// (see `type_compat.rs`) propagate the poison silently — the same pattern used at
/// `check_phase_resolve_structure_members`.  `register_if_absent` preserves any prior
/// Pass-1 registration so `Type::Error` can never overwrite a real type.
///
/// The `pass2_compile_errors` set is threaded through `check_phase_build_available_defaults_map`
/// (where names in the set are excluded from the advertisement — no phantom `(name, Let) →
/// poison_type` entry) and `check_phase_inject_defaults` (where names in the set silently
/// `continue` in the Let-injection cache-miss branch, parallel to `pass2_skipped`).
///
/// Filtering by `Severity::Error` (rather than total diagnostic count) is required because
/// `compile_expr` legitimately emits `Severity::Warning` and `Severity::Info` on valid paths
/// (e.g. `Diagnostic::warning` for `ExprKind::ListLiteral`/`SetLiteral`/`MapLiteral` (empty
/// literal) and zero-arg return-type inference; `Diagnostic::info` for dynamic collection index
/// and qualified-access not-in-scope). A len-based snapshot would incorrectly classify those
/// non-error emissions as compile failures, silently dropping successfully-typed expressions such
/// as `let x = []` from the inferred cache. Scanning only the tail is safe under future drift:
/// any new warning/info path in `compile_expr` or its callees is silently tolerated.
///
/// Using a diagnostic-based check rather than `matches!(result_type, Type::Error)` is more
/// robust: some error-recovery paths in `expr.rs` (e.g. unknown-unit coercion) return
/// `Type::Scalar{DIMENSIONLESS}` instead of `Type::Error`, and would escape a type-based check.
///
/// **Order-dependence caveat:** cascade suppression via the `Type::Error` scope-poison sentinel
/// is effective only when the failing dependency is defined *before* its dependents in
/// `ctx.defaults` traversal order.  In a reverse-ordered chain (e.g. `let c = a  let a = b`
/// with `b` undefined), Pass 2 compiles `c` first, finds `a` absent, emits "unresolved a", and
/// records `c` in `pass2_compile_errors` — only *then* does `a = b` fail and poison `a` with
/// `Type::Error`.  The cascade for `c` already fired.  A topological pre-pass would eliminate
/// this but is not warranted unless reverse-ordered chains become a real concern.
///
/// # TWO-PASS DESIGN RATIONALE (task 1834 amendment)
///
/// The split restores the pre-1834 tolerance for forward references to any *annotated* member:
/// before this amendment, Pass 1+2 were a single pass that walked `ctx.defaults` in source
/// order, so an unannotated `let a = b + 1mm` appearing before `let b : Length = 2mm` would
/// compile against a scope that did not yet contain `b`. Both passes run BEFORE
/// `available_defaults` is built so Pass 2's inference results feed requirement-matching.
///
/// ## Skip-set symmetry (task 1952 + task 2208 amendments)
///
/// All three passes produce a `HashSet<String>` recording same-name losers so the injection
/// loop (phase 6) and advertisement builder (phase 4) can suppress duplicate cells and phantom
/// entries:
///
/// | Set                   | Populated by | Loser kind              | Cell_type gate         |
/// |-----------------------|--------------|-------------------------|------------------------|
/// | `pass1_skipped`       | Pass 1 loop  | annotated Let (`Some`)  | `cell_type.is_some()`  |
/// | `pass1_param_skipped` | Pass 1 loop  | Param (no Option gate)  | n/a — kind-exclusive   |
/// | `pass2_skipped`       | Pass 2 loop  | unannotated Let (`None`)| `cell_type.is_none()`  |
///
/// The `cell_type`-gated guards in the Let consumers are mutually exclusive:
/// `cell_type.is_some() && pass1_skipped` vs `cell_type.is_none() && pass2_skipped`. The
/// `pass1_param_skipped` guard in the `DefaultKind::Param` arm needs no `cell_type` predicate
/// because the arm is already kind-exclusive. Together the three sets cover all same-name loser
/// directions and restore full Pass-1 symmetry.
///
/// # DESIGN LIMITATION
///
/// Pass 2 still walks `ctx.defaults` in source order. Two *unannotated* lets that
/// forward-reference each other will fail inference for the forward-referencing binding
/// (`b` is not in scope when `a`'s expression is compiled), yielding an `unresolved name`
/// diagnostic. Annotating *either* binding unblocks the case. A topological ordering pass
/// would remove the limitation but is out of scope ("documenting as intentional simplification").
/// Named return type for `check_phase_pre_register_default_types`, replacing the former
/// anonymous 5-tuple to eliminate position-swap bugs between the two `HashSet<String>` skip-sets.
pub(super) struct PreRegisterOutput {
    pub inferred_let_exprs: HashMap<(String, AvailableDefaultKind), CompiledExpr>,
    pub pass1_skipped: HashSet<String>,
    pub pass1_param_skipped: HashSet<String>,
    pub pass2_skipped: HashSet<String>,
    pub pass2_compile_errors: HashSet<String>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn check_phase_pre_register_default_types(
    ctx: &MergeContext,
    structure_members: &HashMap<String, Type>,
    structure_name: &str,
    scope: &mut CompilationScope,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> PreRegisterOutput {
    let mut inferred_let_exprs: HashMap<(String, AvailableDefaultKind), CompiledExpr> =
        HashMap::new();
    // Annotated-Let defaults whose scope slot was already claimed by an earlier Pass 1
    // default (typically a Param registered before this annotated Let in ctx.defaults
    // order).  Pass 1 records names here so the injection loop does not emit a
    // duplicate annotated-Let cell alongside the first-seen default's cell.
    //
    // Symmetric with `pass2_skipped` (task 1952) and `pass1_param_skipped` (task 2208):
    //   `pass1_skipped`       — annotated-Let loser in Pass 1; a valid Param/first-annotated-Let
    //                           cell WILL be injected for this name.
    //   `pass1_param_skipped` — Param loser in Pass 1 (annotated Let appeared earlier in
    //                           ctx.defaults); the winning annotated-Let cell WILL be injected.
    //   `pass2_skipped`       — unannotated-Let loser in Pass 2; a valid Param/annotated-Let
    //                           cell WILL be injected for this name.
    let mut pass1_skipped: HashSet<String> = HashSet::new();
    // Param defaults whose scope slot was already claimed by an earlier annotated-Let default
    // in Pass 1 (i.e., annotated Let appeared before Param in ctx.defaults order).  Pass 1
    // records names here so the injection loop (`check_phase_inject_defaults`) does not emit
    // a duplicate Param cell alongside the winning annotated-Let cell, and so the advertisement
    // builder (`check_phase_build_available_defaults_map`) does not emit a phantom Param entry.
    //
    // Symmetric with `pass1_skipped` (annotated-Let loser) and `pass2_skipped` (unannotated-Let
    // loser).  Unlike `pass1_skipped` and `pass2_skipped`, no `cell_type: Option` discriminant
    // is needed in the consumers — the `DefaultKind::Param` arm is already kind-exclusive.
    let mut pass1_param_skipped: HashSet<String> = HashSet::new();
    // Unannotated-let defaults whose scope slot was already claimed by an annotated
    // type in Pass 1.  Pass 2 records names here and skips the `inferred_let_exprs`
    // insert so the injection loop does not emit a duplicate Let cell alongside the
    // Param/annotated-Let cell that will already be injected for the same name.
    // The injection loop uses this set to distinguish a deliberate skip from drift.
    let mut pass2_skipped: HashSet<String> = HashSet::new();
    // Unannotated-let defaults whose Pass 2 `compile_expr` call pushed at least one
    // diagnostic (i.e. the expression itself failed to compile — e.g. unresolved
    // forward reference or unknown unit).  These names are excluded from
    // `inferred_let_exprs` (no poisoned entry in the cache) and excluded from
    // `available_defaults` (no phantom advertisement) and silently skipped by the
    // injection loop (the root-cause diagnostic was already emitted; injecting a
    // poisoned cell would add noise, not value).
    //
    // Distinct from `pass2_skipped`:
    //   `pass2_skipped`        — Pass 1 occupied the slot; a valid Param/annotated-Let
    //                            cell WILL be injected for this name.
    //   `pass2_compile_errors` — The let expression itself is broken; NO cell is
    //                            injected for this name (root cause already reported).
    let mut pass2_compile_errors: HashSet<String> = HashSet::new();

    // Shared conflict logger for `register_if_absent` Occupied returns.  Captures
    // `structure_name` from the enclosing scope so both Pass 1 and Pass 2 call
    // sites stay structurally identical — no drift risk if the message or fields
    // ever change.
    let log_conflict = |name: &str, ignored_ty: Type| {
        tracing::debug!(
            target: "reify_compiler::conformance",
            name = %name,
            entity = %structure_name,
            ignored_ty = ?ignored_ty,
            "trait-merge conflict: second default with same name ignored; first-seen type wins"
        );
    };

    // Pass 1: register all *annotated* defaults (Param, Let-with-annotation).
    // Unannotated lets and constraints are deferred to Pass 2 / injection.
    // register_if_absent provides the no-overwrite guarantee: first-seen type
    // wins, and the method itself is safe against cross-kind overwrites
    // without a call-site guard.
    for default in &ctx.defaults {
        if let Some(name) = &default.name
            && !structure_members.contains_key(name)
        {
            // Compute both the type to register and kind flags in a single match,
            // so the Occupied path (below) can act on `is_annotated_let` and
            // `is_param` without re-inspecting `default.kind`.
            let (ty, is_annotated_let, is_param) = match &default.kind {
                DefaultKind::Param { cell_type, .. } => (cell_type.clone(), false, true),
                DefaultKind::Let {
                    cell_type: Some(annotation_ty),
                    ..
                } => (annotation_ty.clone(), true, false),
                // Deferred to Pass 2 — needs Pass 1's scope to compile against.
                DefaultKind::Let {
                    cell_type: None, ..
                } => continue,
                DefaultKind::Constraint(_) => continue,
                // Assoc-fn defaults are resolved in the dedicated assoc-fn
                // phase (task 3939 δ), not registered as value-cell defaults.
                DefaultKind::Fn(_) => continue,
                // Assoc-type defaults are resolved in the dedicated assoc-type
                // phase (step-10), not registered as value-cell defaults.
                DefaultKind::AssocType(_) => continue,
            };
            // First-seen type wins. `ty` is moved into `register_if_absent`; on
            // the cold Occupied (conflict) path the method hands it back via
            // `Some(ignored_ty)` for the debug emission, so no clone is needed on
            // the hot Vacant insertion path.
            if let Some(ignored_ty) = scope.register_if_absent(name, ty) {
                log_conflict(name, ignored_ty);
                // SYMMETRIC FIX (task 1952): when the loser in Pass 1 is an annotated
                // Let, record the name in pass1_skipped so the injection loop skips
                // annotated-Let cell emission for this name, preventing a duplicate
                // (entity, member) cell alongside the winning default's cell.
                if is_annotated_let {
                    pass1_skipped.insert(name.to_string());
                }
                // SYMMETRIC FIX (task 2208): when the loser in Pass 1 is a Param
                // (annotated Let appeared earlier in ctx.defaults and won the slot),
                // record the name in pass1_param_skipped so the injection loop skips
                // Param cell emission and the advertisement builder skips the phantom
                // Param entry — restoring full Pass-1 symmetry.
                if is_param {
                    pass1_param_skipped.insert(name.to_string());
                }
            }
        }
    }

    // Pass 2: compile each *unannotated* Let's expression against the
    // fully-populated annotated scope from Pass 1 and register its inferred
    // type.  When `register_if_absent` finds the scope slot already claimed
    // (Pass 1 registered an annotated Param or Let), the compiled expression
    // is discarded and the name is recorded in `pass2_skipped` so the
    // injection loop skips Let-cell injection — preventing a duplicate
    // (entity, member) cell alongside the annotated-type injection.  When the
    // slot is vacant, the expression is cached in `inferred_let_exprs` for
    // reuse by the injection loop (avoids double compilation) and by
    // `available_defaults` (so requirement-matching uses the inferred type
    // instead of the old `Type::Real` fallback).
    //
    // Error suppression (task 1914 suggestion #1, hardened by task 2158):
    // snapshot the diagnostic-vector length before each compile_expr call, then
    // scan only the newly-appended tail for Severity::Error.  If the tail
    // contains at least one Error, the expression failed to compile (root-cause
    // diagnostic already emitted).  In that case the name is recorded in
    // `pass2_compile_errors` and the scope slot is poisoned with Type::Error —
    // no inferred cache entry is produced, preventing phantom "available default
    // has Real/Error" cascade diagnostics downstream.
    //
    // Scanning only the tail (rather than counting total Errors) is required
    // because compile_expr legitimately emits Severity::Warning and
    // Severity::Info on valid paths (e.g. Diagnostic::warning for
    // ExprKind::ListLiteral/SetLiteral/MapLiteral (empty literal), zero-arg
    // return-type inference, and Diagnostic::info for dynamic collection index /
    // qualified-access not-in-scope).  Counting those as compile failures would
    // incorrectly suppress successfully-typed expressions such as `let x = []`.
    // The tail-scan is safe under future drift: any new warning/info path in
    // compile_expr or its callees is silently tolerated.
    //
    // Order-dependence caveat: cascade suppression via the Type::Error scope-poison
    // sentinel is effective only when the failing dependency is defined *before* its
    // dependents in ctx.defaults traversal order.  In a reverse-ordered chain (e.g.
    // `let c = a  let a = b` with b undefined), Pass 2 compiles c first, finds a
    // absent, and emits the cascade error before a = b fails and poisons a.  A
    // topological pre-pass would eliminate this but is not warranted unless such
    // cases become a real concern.
    for default in &ctx.defaults {
        if let Some(name) = &default.name
            && !structure_members.contains_key(name)
            && let DefaultKind::Let {
                cell_type: None,
                let_decl,
            } = &default.kind
        {
            // Snapshot the diagnostic-vector length before compilation, then scan
            // only the newly-appended tail for Severity::Error.  This avoids
            // rescanning the entire (growing) diagnostics vector on every default
            // in the loop — O(emitted diagnostics) rather than O(N × |diagnostics|).
            // Warning/Info entries in the tail are tolerated; only Error entries
            // indicate a compile failure.  Partial error-recovery paths may return
            // non-Error types (e.g. Type::Scalar{DIMENSIONLESS}), so the diagnostic
            // tail-scan is more reliable than inspecting compiled_expr.result_type.
            let diag_before = diagnostics.len();
            let compiled_expr =
                compile_expr(&let_decl.value, scope, enum_defs, functions, diagnostics);
            let had_compile_error = diagnostics[diag_before..]
                .iter()
                .any(|d| d.severity == Severity::Error);

            if had_compile_error {
                // compile_expr itself emitted at least one Severity::Error diagnostic —
                // the expression is broken.  Do NOT insert into inferred_let_exprs
                // (prevents a poisoned entry advertising a phantom type in
                // available_defaults).
                pass2_compile_errors.insert(name.to_string());
                // Poison the scope slot with Type::Error so sibling unannotated-let
                // expressions (or trait constraints) that reference this name resolve to
                // the anti-cascade sentinel rather than emitting a fresh "unresolved name"
                // cascade.  `implicitly_converts_to(Error, _) -> true` and
                // `infer_binop_type`'s leading Type::Error guard (see `type_compat.rs`)
                // then propagate the poison silently through downstream compilation —
                // the same pattern used at `check_phase_resolve_structure_members` for
                // unresolved structure-member annotations.
                //
                // `register_if_absent` preserves any prior registration: a Pass 1 annotated
                // Param/Let that already claimed the slot keeps its real type (Type::Error
                // can never displace a real type, only fill a vacant slot).  A collision
                // here means Pass 1 already registered this name with a real type; log it
                // at trace level for observability.
                if let Some(prior_ty) = scope.register_if_absent(name, Type::Error) {
                    tracing::trace!(
                        target: "reify_compiler::conformance",
                        name = %name,
                        prior_ty = ?prior_ty,
                        "pass2 compile-error: scope slot already claimed by Pass-1 \
                         registration; Type::Error sentinel not inserted (prior type wins)"
                    );
                }
            } else {
                let inferred_ty = compiled_expr.result_type.clone();
                if let Some(ignored_ty) = scope.register_if_absent(name, inferred_ty) {
                    log_conflict(name, ignored_ty);
                    // Scope slot already claimed by an annotated type (Pass 1).
                    // Record in pass2_skipped so the injection loop skips Let-cell
                    // injection for this name and avoids duplicate (entity, member) cells.
                    pass2_skipped.insert(name.to_string());
                } else {
                    inferred_let_exprs
                        .insert((name.clone(), AvailableDefaultKind::Let), compiled_expr);
                }
            }
        }
    }

    PreRegisterOutput {
        inferred_let_exprs,
        pass1_skipped,
        pass1_param_skipped,
        pass2_skipped,
        pass2_compile_errors,
    }
}

/// Phase 4 of trait conformance checking: build the `available_defaults` advertisement map.
///
/// Produces a `HashMap<(String, AvailableDefaultKind), Type>` keyed by `(name, kind)` so that
/// `Param` and `Let` defaults for the same member name occupy separate slots and can be looked
/// up independently. A `Param` default can satisfy a `Param` requirement; a `Let` default can
/// satisfy a `Let` requirement — they do not interfere with each other.
///
/// ## Key structure rationale
///
/// The composite `(name, AvailableDefaultKind)` key was chosen over a two-level map or a
/// single-key map with a kind guard on the value for two reasons:
/// 1. **Pre-filtering**: the requirement-checking loop (phase 5) looks up
///    `(req.name.clone(), required_kind)` directly, performing kind-filtering inside the
///    HashMap lookup rather than in the match branch — the key *is* the filter.
/// 2. **Allocation cost**: `.get(&(req.name.clone(), kind))` allocates a String per lookup
///    because `HashMap<(String, K), V>` has no `Borrow` impl for `(&str, K)`. Requirements
///    are small in practice so this is acceptable; a two-level map is the escape hatch if
///    it ever becomes a hot path.
///
/// ## Pass 1/Pass 2 skipped exclusions — three symmetric guards
///
/// Three symmetric guards suppress phantom advertisements to maintain the
/// "advertisement mirrors injection" invariant (task 1951 Option B + task 1952 + task 2208):
///
/// | Guard                                            | Skipped by | Cell_type predicate         |
/// |--------------------------------------------------|------------|-----------------------------|
/// | `cell_type.is_none() && pass2_skipped.contains` | Pass 2     | unannotated Let (`None`)    |
/// | `cell_type.is_some() && pass1_skipped.contains` | Pass 1     | annotated Let (`Some`)      |
/// | `pass1_param_skipped.contains`                   | Pass 1     | n/a — Param arm, no Option  |
///
/// The Let `cell_type` predicates are mutually exclusive: a given Let entry can only satisfy
/// one of the two Let guards — it is either annotated or unannotated, never both.
///
/// **pass2_skipped**: When `cell_type` is `None` (unannotated Let) *and* the name is in
/// `pass2_skipped`, this entry is excluded. Pass 2 populates `pass2_skipped` exclusively
/// from the `DefaultKind::Let { cell_type: None, .. }` arm; annotated Lets (`Some(_)`)
/// for the same name still advertise normally (the injection loop injects them). The
/// `cell_type.is_none() &&` conjunction is the minimal narrowing from the original guard
/// (task 1951 Option B).
///
/// **pass1_skipped**: When `cell_type` is `Some(_)` (annotated Let) *and* the name is in
/// `pass1_skipped`, this entry is excluded. Pass 1 populates `pass1_skipped` when an
/// annotated Let's `register_if_absent` call finds the scope slot already claimed by an
/// earlier Pass 1 default (task 1952). The injection loop skips annotated-Let cell emission
/// for names in `pass1_skipped`, so advertising such a name would produce a phantom
/// `(name, Let) → Type` entry with no injected cell backing it — a "requirement satisfied"
/// lie. The `cell_type.is_some() &&` conjunction parallels the `is_none()` guard.
///
/// **pass1_param_skipped**: When a Param name is in `pass1_param_skipped`, the `Param` arm
/// returns `None` unconditionally, excluding the phantom `(name, Param) → Type` advertisement.
/// Pass 1 populates `pass1_param_skipped` when a Param's `register_if_absent` call finds the
/// scope slot already claimed by an annotated Let (task 2208). Unlike the two Let guards, no
/// `cell_type` predicate is needed — `DefaultKind::Param` has no `cell_type: Option`
/// discriminant; the arm is already kind-exclusive. The injection loop mirrors this guard via
/// a matching `if pass1_param_skipped.contains(name) { continue; }` at the top of the Param
/// arm in `check_phase_inject_defaults`.
///
/// ## `pass2_compile_errors` exclusion (task 1914 suggestion #1)
///
/// Names in `pass2_compile_errors` are also excluded from the `Let` arm: Pass 2 failed to
/// compile the let expression (root-cause diagnostic already emitted), so there is no valid
/// inferred type to advertise. Including a phantom entry would produce a spurious
/// "available default has Real/Error" cascade diagnostic on top of the root-cause error.
///
/// ## Unannotated let defaults
///
/// For `DefaultKind::Let { cell_type: None, .. }` entries that passed both guards, the
/// advertised type is the inferred result type from `inferred_let_exprs` (populated by phase
/// 3's Pass 2). Falls back to `Type::Real` if the name is absent from the cache — after task
/// 1914 this is a defensive-default that should not be reached in practice: compile-error
/// names are excluded by `pass2_compile_errors` and skipped names by `pass2_skipped`.
/// A `debug_assert!` guards the fallback to catch any drift.
pub(super) fn check_phase_build_available_defaults_map(
    ctx: &MergeContext,
    inferred_let_exprs: &HashMap<(String, AvailableDefaultKind), CompiledExpr>,
    pass1_skipped: &HashSet<String>,
    pass1_param_skipped: &HashSet<String>,
    pass2_skipped: &HashSet<String>,
    pass2_compile_errors: &HashSet<String>,
) -> HashMap<(String, AvailableDefaultKind), Type> {
    ctx.defaults
        .iter()
        .filter_map(|d| {
            let name = d.name.as_deref()?;
            let (kind, ty) = match &d.kind {
                DefaultKind::Param { cell_type, .. } => {
                    // SYMMETRIC FIX (task 2208): suppress the phantom Param advertisement for
                    // names in pass1_param_skipped. Pass 1 populates pass1_param_skipped when a
                    // Param's register_if_absent found the scope slot already claimed by an
                    // earlier annotated Let; the injection loop will NOT emit a Param cell for
                    // this name, so advertising it would produce a phantom (name, Param) entry
                    // with no injected cell — a "requirement satisfied" lie.
                    // Unlike the Let guards there is no cell_type.is_some()/is_none() conjunction
                    // needed here — the DefaultKind::Param arm is already kind-exclusive.
                    if pass1_param_skipped.contains(name) {
                        return None;
                    }
                    (AvailableDefaultKind::Param, cell_type.clone())
                }
                DefaultKind::Let { cell_type, .. } => {
                    // Suppress the *unannotated* Let phantom entry for names in pass2_skipped
                    // (task 1951 Option B, narrowed to unannotated Lets). Pass 2 populates
                    // pass2_skipped exclusively from `cell_type: None` entries; the
                    // `cell_type.is_none()` conjunction restricts suppression to that case.
                    // Regression cover: `super::tests::option_b_fix_blocks_phantom_let_entry_for_pass2_skipped_name`
                    // (hand-builds a `RequirementKind::Let` requirement that no current parser path produces).
                    if cell_type.is_none() && pass2_skipped.contains(name) {
                        return None;
                    }
                    // SYMMETRIC FIX (task 1952): suppress the *annotated* Let phantom entry
                    // for names in pass1_skipped. Pass 1 populates pass1_skipped when an
                    // annotated Let's register_if_absent found the scope slot already claimed;
                    // the injection loop will NOT emit a cell for this name, so advertising
                    // it would produce a phantom (name, Let) entry with no injected cell.
                    // The `cell_type.is_some()` conjunction is the mirror of `is_none()`
                    // above — the two guards are mutually exclusive by construction.
                    if cell_type.is_some() && pass1_skipped.contains(name) {
                        return None;
                    }
                    // Do not advertise a phantom Let entry for names whose Pass 2
                    // compile_expr call emitted a diagnostic (task 1914 suggestion #1).
                    // The root-cause diagnostic was already pushed; advertising a
                    // phantom `(name, Let) → poison_type` entry would produce a
                    // spurious "available default has Real/Error" cascade on top of it.
                    if pass2_compile_errors.contains(name) {
                        return None;
                    }
                    // Guard: unannotated names without an annotation must have a
                    // cache entry (pass2_compile_errors and pass2_skipped names were
                    // excluded above; anything else means the Pass 2 contract is broken).
                    let key = (name.to_string(), AvailableDefaultKind::Let);
                    debug_assert!(
                        cell_type.is_some() || inferred_let_exprs.contains_key(&key),
                        "unannotated Let '{name}' absent from inferred_let_exprs (composite key \
                         (name, Let)) and not in pass2_skipped or pass2_compile_errors — Pass 2 \
                         contract broken; Type::Real fallback would re-introduce the \
                         phantom-type-mismatch bug fixed by task 1951 Option B"
                    );
                    let resolved =
                        resolve_let_advertised_type(cell_type, inferred_let_exprs.get(&key));
                    (AvailableDefaultKind::Let, resolved)
                }
                DefaultKind::Constraint(_) => return None,
                // Assoc-fn defaults are not value-cell defaults; resolved in
                // the dedicated assoc-fn phase (task 3939 δ).
                DefaultKind::Fn(_) => return None,
                // Assoc-type defaults are not value-cell defaults; resolved in
                // the dedicated assoc-type phase (step-10).
                DefaultKind::AssocType(_) => return None,
            };
            Some(((name.to_string(), kind), ty))
        })
        .collect()
}

/// Derive the exact-match [`CompiledAssocFnSig`] for every structure-body
/// associated function (`MemberDecl::Fn`), keyed by fn name.
///
/// Conformer-side sibling of `traits::assoc_fn_sig`: the leading `is_self`
/// receiver is recorded as `has_self` and excluded from `params`; every other
/// param's `type_expr` and the `return_type` resolve through
/// `resolve_type_expr_with_aliases` — the SAME resolver the trait side funnels
/// through — so the structure-derived sig is directly `PartialEq`-comparable
/// with the trait-derived requirement sig. A missing return type defaults to
/// `Type::Real`, matching `compile_function` / `assoc_fn_sig`.
///
/// Resolution here is deliberately side-effect-free: a failure resolves to
/// `Type::Error` and emits NO diagnostic (a throwaway sink absorbs any the
/// resolver pushes). δ is producer-only and this map exists only to compare
/// signatures; a genuinely unresolvable structure-fn annotation is reported
/// when the structure body is compiled (entity.rs) or the override is compiled
/// into the assoc-fn table (task ζ), not duplicated here. (task 3939 δ)
pub(super) fn collect_structure_assoc_fn_sigs(
    structure: &EntityDefRef<'_>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
) -> HashMap<String, CompiledAssocFnSig> {
    let mut sigs: HashMap<String, CompiledAssocFnSig> = HashMap::new();
    for m in structure.members.iter() {
        if let reify_ast::MemberDecl::Fn(fn_def) = m {
            sigs.insert(
                fn_def.name.clone(),
                derive_assoc_fn_sig_silent(fn_def, alias_registry, structure_names, trait_names),
            );
        }
    }
    sigs
}

/// Collect the structure's explicit `type X = T` bindings, resolving each
/// `default_type` to a `Type` via `resolve_type_expr_with_aliases`.
///
/// Parallel to `collect_structure_assoc_fn_sigs`: walks `structure.members` for
/// `MemberDecl::AssociatedType` entries. For a structure binding the
/// `default_type` is always `Some` (a trait-body `type X` without an `=` is a
/// *requirement*, not a binding). Resolution uses a throwaway diagnostic sink so
/// a genuinely unresolvable annotation does not duplicate diagnostics (the real
/// error was already reported when the structure body was compiled). An
/// unresolvable annotation maps to `Type::Error`, which the satisfaction check
/// treats as "bound" (suppresses `TraitAssocTypeNotBound`) so a single root-cause
/// error yields a single diagnostic. (task 3972)
pub(super) fn collect_structure_assoc_type_bindings(
    structure: &EntityDefRef<'_>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
) -> HashMap<String, Type> {
    let mut bindings: HashMap<String, Type> = HashMap::new();
    let empty_params: HashSet<String> = HashSet::new();
    let mut sink: Vec<Diagnostic> = Vec::new();
    for m in structure.members.iter() {
        if let reify_ast::MemberDecl::AssociatedType(at) = m {
            if let Some(type_expr) = &at.default_type {
                let ty = resolve_type_expr_with_aliases(
                    type_expr,
                    &empty_params,
                    alias_registry,
                    &mut sink,
                    structure_names,
                    trait_names,
                )
                .unwrap_or(Type::Error);
                bindings.insert(at.name.clone(), ty);
            }
        }
    }
    bindings
}

/// Derive a side-effect-free [`CompiledAssocFnSig`] from a single `FnDef`.
///
/// The leading `is_self` receiver is recorded as `has_self` and excluded from
/// `params`; every other param's `type_expr` and the `return_type` resolve
/// through `resolve_type_expr_with_aliases` — the same resolver `assoc_fn_sig`
/// (traits.rs) uses on the trait side — so equal annotations on a trait default
/// and a structure override compare equal under the derived `PartialEq`. A
/// missing return type defaults to `Type::Real`, matching `compile_function` /
/// `assoc_fn_sig`.
///
/// Resolution is deliberately side-effect-free: a failure resolves to
/// `Type::Error` and emits NO diagnostic (a throwaway sink absorbs anything the
/// resolver pushes). The genuine root cause is reported once when the body is
/// compiled by `compile_assoc_function`; `assoc_fn_sig_has_error` then lets the
/// signature-comparison sites (phase 5, and the default-override gate in
/// `check_phase_resolve_assoc_fns`) skip a spurious mismatch. (task 3939 δ)
fn derive_assoc_fn_sig_silent(
    fn_def: &reify_ast::FnDef,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
) -> CompiledAssocFnSig {
    let empty_params: HashSet<String> = HashSet::new();
    // Throwaway sink: signature derivation must not emit or duplicate diagnostics.
    let mut sink: Vec<Diagnostic> = Vec::new();
    let mut has_self = false;
    let mut params = Vec::new();
    for p in &fn_def.params {
        if p.is_self {
            has_self = true;
            continue;
        }
        let ty = resolve_type_expr_with_aliases(
            &p.type_expr,
            &empty_params,
            alias_registry,
            &mut sink,
            structure_names,
            trait_names,
        )
        .unwrap_or(Type::Error);
        params.push(ty);
    }
    let return_type = match &fn_def.return_type {
        Some(te) => resolve_type_expr_with_aliases(
            te,
            &empty_params,
            alias_registry,
            &mut sink,
            structure_names,
            trait_names,
        )
        .unwrap_or(Type::Error),
        None => Type::Real,
    };
    CompiledAssocFnSig {
        name: fn_def.name.clone(),
        has_self,
        params,
        return_type,
    }
}

/// Render a [`CompiledAssocFnSig`] in source-like form for diagnostics, e.g.
/// `fn area(self) -> Real` or `fn scale(self, Real) -> Length`. Used by the
/// phase-5 signature-mismatch diagnostic. (task 3939 δ)
fn render_assoc_fn_sig(sig: &CompiledAssocFnSig) -> String {
    let mut parts: Vec<String> = Vec::new();
    if sig.has_self {
        parts.push("self".to_string());
    }
    for p in &sig.params {
        parts.push(p.to_string());
    }
    format!("fn {}({}) -> {}", sig.name, parts.join(", "), sig.return_type)
}

/// True when a derived [`CompiledAssocFnSig`] carries a `Type::Error` in its
/// (receiver-excluded) params or its return type — i.e. an annotation that
/// failed to resolve. `collect_structure_assoc_fn_sigs` records `Type::Error`
/// (via a throwaway diagnostic sink) for unresolvable annotations, while
/// `compile_assoc_function` reports the real `UnresolvedType`; this predicate
/// lets the phase-5 Fn arm skip the spurious `TraitFnSignatureMismatch` so one
/// root cause yields one diagnostic. (task 3939 δ, reviewer amendment)
fn assoc_fn_sig_has_error(sig: &CompiledAssocFnSig) -> bool {
    sig.return_type == Type::Error || sig.params.contains(&Type::Error)
}

/// Find the structure's own `fn <name>` override member, if it declares one.
/// Used by the assoc-fn-resolution phase to pick the override body over the
/// trait default. (task 3939 δ)
fn find_structure_assoc_fn<'a>(
    structure: &EntityDefRef<'a>,
    name: &str,
) -> Option<&'a reify_ast::FnDef> {
    structure.members.iter().find_map(|m| match m {
        reify_ast::MemberDecl::Fn(fd) if fd.name == name => Some(fd),
        _ => None,
    })
}

/// Phase (task 3939 δ): resolve the conformer's associated-function table.
///
/// For every assoc fn in the merged trait-bound set — each `DefaultKind::Fn`
/// default and each `RequirementKind::Fn` requirement — pick the structure's
/// override `fn` body when the structure declares a same-name `fn`, else the
/// trait's default body, compile it via [`compile_assoc_function`] against the
/// conformer receiver type, and push a [`CompiledAssocFn`] keyed by
/// `(trait_name, fn_name)` with `is_override` set accordingly. This is the
/// lookup target for task ζ's `TraitMethodCall` lowering (PRD §4.3).
///
/// Defaults are processed first (they carry a body to inject); a `handled` set
/// then suppresses a duplicate entry for a bodyless `RequirementKind::Fn` of the
/// same name that the same default satisfies. A bodyless requirement with
/// neither a structure override nor a same-name default contributes no entry
/// (phase 5 already emitted `TraitFnNotSatisfied`).
#[allow(clippy::too_many_arguments)]
pub(super) fn check_phase_resolve_assoc_fns(
    ctx: &MergeContext,
    structure: &EntityDefRef<'_>,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    // Exact-match signatures of the structure's own `fn` members (built by
    // `collect_structure_assoc_fn_sigs`, shared with phase 5). Consulted before
    // pushing a required-fn override so a signature-mismatched override never
    // lands in the dispatch table. (reviewer amendment)
    structure_fn_sigs: &HashMap<String, CompiledAssocFnSig>,
    diagnostics: &mut Vec<Diagnostic>,
    assoc_fns_out: &mut Vec<CompiledAssocFn>,
) {
    let conformer = structure.name;
    // Names already given a table entry — prevents a bodyless requirement and a
    // same-name default from both producing one.
    let mut handled: HashSet<String> = HashSet::new();

    // Default-providing assoc fns: the structure override beats the default body.
    for default in &ctx.defaults {
        let DefaultKind::Fn(default_fn_def) = &default.kind else {
            continue;
        };
        let Some(fn_name) = default.name.as_deref() else {
            continue;
        };
        if !handled.insert(fn_name.to_string()) {
            continue;
        }
        let trait_name = ctx
            .seen_fn_default_traits
            .get(fn_name)
            .cloned()
            .unwrap_or_else(|| "<trait>".to_string());
        let (fn_def_to_compile, is_override) = match find_structure_assoc_fn(structure, fn_name) {
            Some(override_def) => (override_def, true),
            None => (default_fn_def, false),
        };
        // Compile unconditionally so a genuine body/type error in the chosen
        // body (default or override) is surfaced even when a signature mismatch
        // keeps the override out of the table below.
        let compiled = compile_assoc_function(
            fn_def_to_compile,
            conformer,
            enum_defs,
            functions,
            alias_registry,
            structure_names,
            trait_names,
            diagnostics,
        );
        // Override signature-lock (reviewer amendment): a default-providing assoc
        // fn imposes the default's signature on any structure override (PRD §5.4
        // exact-match-for-overrides). A default-only fn produces no
        // `RequirementKind::Fn`, so phase 5 never validates it — this is the sole
        // site the override's signature is checked. On mismatch, emit
        // `TraitFnSignatureMismatch` and keep the wrongly-typed override OUT of
        // the dispatch table (symmetric with the required-fn gate below) so task
        // ζ's dispatch never keys on an entry inconsistent with the diagnostic.
        // The `assoc_fn_sig_has_error` guard suppresses a spurious mismatch when
        // the override's annotation failed to resolve — `compile_assoc_function`
        // already reported that as `UnresolvedType` (Type::Error anti-cascade).
        if is_override && let Some(actual_sig) = structure_fn_sigs.get(fn_name) {
            let expected_sig = derive_assoc_fn_sig_silent(
                default_fn_def,
                alias_registry,
                structure_names,
                trait_names,
            );
            if *actual_sig != expected_sig && !assoc_fn_sig_has_error(actual_sig) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "associated function '{}' provided by structure '{}' has signature \
                         `{}` but trait '{}' default declares `{}`",
                        fn_name,
                        structure.name,
                        render_assoc_fn_sig(actual_sig),
                        trait_name,
                        render_assoc_fn_sig(&expected_sig),
                    ))
                    .with_code(DiagnosticCode::TraitFnSignatureMismatch)
                    .with_label(DiagnosticLabel::new(
                        structure.span,
                        "associated function signature does not match the trait default",
                    )),
                );
                continue; // keep the wrongly-typed override out of the dispatch table
            }
        }
        if let Some(function) = compiled {
            assoc_fns_out.push(CompiledAssocFn {
                trait_name,
                fn_name: fn_name.to_string(),
                function,
                is_override,
            });
        }
    }

    // Bodyless required assoc fns satisfied by a structure override. There is no
    // default body for these, so an unsatisfied one (no override, no default)
    // already errored in phase 5 and contributes no entry.
    for req in &ctx.requirements {
        let RequirementKind::Fn(expected_sig) = &req.kind else {
            continue;
        };
        if !handled.insert(req.name.clone()) {
            continue; // a same-name default already produced the entry
        }
        let Some(override_def) = find_structure_assoc_fn(structure, &req.name) else {
            continue; // unsatisfied — phase 5 emitted TraitFnNotSatisfied
        };
        let trait_name = ctx
            .seen_fn_sigs
            .get(&req.name)
            .map(|(_, t)| t.clone())
            .unwrap_or_else(|| "<trait>".to_string());
        // Compile the override unconditionally so a genuine body/type error in
        // it (e.g. an unresolved param/return annotation) is still surfaced by
        // `compile_assoc_function`, even when it will not enter the table below.
        if let Some(function) = compile_assoc_function(
            override_def,
            conformer,
            enum_defs,
            functions,
            alias_registry,
            structure_names,
            trait_names,
            diagnostics,
        ) {
            // Robustness (reviewer amendment): only populate the dispatch table
            // when the override's derived signature exactly matches the
            // requirement. A signature-mismatched override already triggered
            // `TraitFnSignatureMismatch` in phase 5 and the errored module will
            // not ship, but the table must not carry a wrongly-typed
            // `CompiledAssocFn` that task ζ's dispatch would key on — that would
            // leave the table internally inconsistent with the emitted
            // diagnostic. (A sig carrying `Type::Error` from an unresolved
            // annotation also fails this equality and is likewise kept out.)
            if structure_fn_sigs.get(&req.name) == Some(expected_sig) {
                assoc_fns_out.push(CompiledAssocFn {
                    trait_name,
                    fn_name: req.name.clone(),
                    function,
                    is_override: true,
                });
            }
        }
    }
}

/// Phase 5 of trait conformance checking: verify structure members against trait requirements.
///
/// For each requirement in `ctx.requirements`, checks that either:
/// 1. The structure provides a member of the correct type (type-mismatch branch), OR
/// 2. Another trait in the merged bound set provides a same-kind default with a compatible
///    type (`available_defaults` lookup), OR
/// 3. Neither — emits "missing required member" diagnostic.
///
/// ## Same-kind default satisfaction
///
/// Only a same-kind default satisfies a requirement: a `let` default does NOT satisfy a
/// `param` requirement (param slots must be externally settable at call-site). The composite
/// `(name, AvailableDefaultKind)` key in `available_defaults` encodes this: lookups are
/// inherently kind-filtered and require no additional guard on the match arm.
///
/// ## Anti-cascade via `Type::Error`
///
/// Structure members with an unresolved annotation are stored as `Type::Error` by phase 1.
/// `implicitly_converts_to(Error, _)` short-circuits to `true` (producer-side wildcard in
/// `type_compat.rs:3–26`), so a single root-cause diagnostic suppresses the cascade
/// "type mismatch" that would otherwise appear here.
///
/// ## Allocation note
///
/// `.get(&(req.name.clone(), kind))` allocates a `String` per lookup because
/// `HashMap<(String, K), V>` has no `Borrow` impl for `(&str, K)`. Requirement counts are
/// small in practice; the escape hatch is a two-level `HashMap<String, HashMap<K, V>>`.
pub(super) fn check_phase_check_members_against_requirements(
    ctx: &MergeContext,
    structure: &EntityDefRef<'_>,
    structure_param_members: &HashMap<String, Type>,
    structure_let_members: &HashMap<String, Type>,
    available_defaults: &HashMap<(String, AvailableDefaultKind), Type>,
    // Exact-match signatures of the structure's own `fn` members, keyed by fn
    // name (built by `collect_structure_assoc_fn_sigs`). Consulted by the
    // `RequirementKind::Fn` arm to verify a provided assoc fn's signature.
    structure_fn_sigs: &HashMap<String, CompiledAssocFnSig>,
    // Explicit `type X = T` bindings from the structure body, keyed by type
    // name (built by `collect_structure_assoc_type_bindings`). Consulted by
    // the `RequirementKind::AssocType` arm to check satisfaction. (task 3972)
    structure_assoc_type_bindings: &HashMap<String, Type>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for req in &ctx.requirements {
        // A `param` requirement can only be satisfied by a `param` default and a `let`
        // requirement by a `let` default; `sub` is handled separately below. Binding the
        // kind tag and expected type in a single match arm keeps the pairing exhaustive:
        // adding a new `RequirementKind` variant forces a decision here rather than
        // falling through a stale `unreachable!()`.
        let (required_default_kind, expected_type) = match &req.kind {
            RequirementKind::Param(expected) => (AvailableDefaultKind::Param, expected),
            RequirementKind::Let(expected) => (AvailableDefaultKind::Let, expected),
            RequirementKind::Sub(structure_name) => {
                let has_sub = structure.members.iter().any(|m| {
                    if let reify_ast::MemberDecl::Sub(s) = m {
                        s.name == req.name && s.structure_name == *structure_name
                    } else {
                        false
                    }
                });
                if !has_sub {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "missing required sub-component '{}' of type '{}'",
                            req.name, structure_name
                        ))
                        .with_code(DiagnosticCode::MissingRequiredSubComponent)
                        .with_label(DiagnosticLabel::new(structure.span, "required by trait")),
                    );
                }
                continue;
            }
            // Assoc-type requirement (task 3972). Two outcomes:
            //   * structure explicitly binds the name (`type X = T` in the
            //     structure body) OR a `DefaultKind::AssocType` default of the
            //     same name was merged in → satisfied; no diagnostic.
            //   * neither → emit `TraitAssocTypeNotBound` naming the declaring
            //     trait (via ctx.seen_assoc_type_reqs) and the type name, with
            //     a label on structure.span (parallel to the Fn None-branch).
            // The arm must `continue` like Sub/Fn so it never falls through to
            // the param/let routing below.
            RequirementKind::AssocType(_) => {
                let bound_by_structure =
                    structure_assoc_type_bindings.contains_key(req.name.as_str());
                let provided_by_default = ctx.defaults.iter().any(|d| {
                    d.name.as_deref() == Some(req.name.as_str())
                        && matches!(d.kind, DefaultKind::AssocType(_))
                });
                if !bound_by_structure && !provided_by_default {
                    let declaring_trait = ctx
                        .seen_assoc_type_reqs
                        .get(&req.name)
                        .map(|t| t.clone())
                        .unwrap_or_else(|| "<trait>".to_string());
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "associated type '{}' required by trait '{}' is not \
                             bound by structure '{}'",
                            req.name, declaring_trait, structure.name
                        ))
                        .with_code(DiagnosticCode::TraitAssocTypeNotBound)
                        .with_label(DiagnosticLabel::new(
                            structure.span,
                            "required associated type not bound",
                        )),
                    );
                }
                continue;
            }
            // Assoc-fn requirement (task 3939 δ). Three outcomes:
            //   * structure provides a `fn` of this name → exact-match its
            //     signature (§5.4: self-ness, param types, return type — no
            //     subtyping) against the requirement; on mismatch emit
            //     `TraitFnSignatureMismatch`. A present-but-mismatched fn is a
            //     DISTINCT failure mode from a missing one, so it must NOT also
            //     fire `TraitFnNotSatisfied`.
            //   * structure does not provide it, but a trait `DefaultKind::Fn`
            //     of the same name was merged in → satisfied by the default.
            //   * neither → emit `TraitFnNotSatisfied` naming the trait + fn.
            RequirementKind::Fn(expected_sig) => {
                // Resolve the declaring trait lazily — only needed on a failure path.
                let declaring_trait = || {
                    ctx.seen_fn_sigs
                        .get(&req.name)
                        .map(|(_, t)| t.clone())
                        .unwrap_or_else(|| "<trait>".to_string())
                };
                match structure_fn_sigs.get(&req.name) {
                    Some(actual_sig) => {
                        // Present → exact `PartialEq` match. The structure-derived
                        // sig and the requirement sig both resolve through
                        // `resolve_type_expr_with_aliases`, so equal annotations
                        // compare equal.
                        //
                        // Consistency (reviewer amendment): suppress the mismatch
                        // when the structure-derived sig carries a `Type::Error`
                        // (an unresolvable param/return annotation). The genuine
                        // root cause is reported as `UnresolvedType` by
                        // `compile_assoc_function` during table resolution; firing
                        // a signature mismatch here too would double-report one
                        // error and mislead (the sig "differs" only because
                        // resolution failed). Mirrors the `Type::Error`
                        // anti-cascade convention documented on this phase.
                        if actual_sig != expected_sig && !assoc_fn_sig_has_error(actual_sig) {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "associated function '{}' provided by structure '{}' has \
                                     signature `{}` but trait '{}' requires `{}`",
                                    req.name,
                                    structure.name,
                                    render_assoc_fn_sig(actual_sig),
                                    declaring_trait(),
                                    render_assoc_fn_sig(expected_sig),
                                ))
                                .with_code(DiagnosticCode::TraitFnSignatureMismatch)
                                .with_label(DiagnosticLabel::new(
                                    structure.span,
                                    "associated function signature does not match the trait",
                                )),
                            );
                        }
                    }
                    None => {
                        // Absent from the structure: a same-name `DefaultKind::Fn`
                        // (default-providing assoc fn) satisfies it; otherwise it
                        // is unsatisfied.
                        let provided_by_default = ctx.defaults.iter().any(|d| {
                            d.name.as_deref() == Some(req.name.as_str())
                                && matches!(d.kind, DefaultKind::Fn(_))
                        });
                        if !provided_by_default {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "associated function '{}' required by trait '{}' is not \
                                     satisfied by structure '{}'",
                                    req.name,
                                    declaring_trait(),
                                    structure.name
                                ))
                                .with_code(DiagnosticCode::TraitFnNotSatisfied)
                                .with_label(DiagnosticLabel::new(
                                    structure.span,
                                    "required associated function not provided",
                                )),
                            );
                        }
                    }
                }
                continue;
            }
        };
        // Route the structure-member lookup to the kind-appropriate map.
        // A `param` requirement is satisfied only by a structure `param` member;
        // a `let` requirement only by a structure `let` member.  This prevents a
        // structure's computed `let` slot from silently satisfying a trait's
        // settable `param` requirement (the fix for suggestion #4).
        let kind_members = match required_default_kind {
            AvailableDefaultKind::Param => structure_param_members,
            AvailableDefaultKind::Let => structure_let_members,
        };
        match kind_members.get(&req.name) {
            Some(actual_type) => {
                // Intentionally NOT suppressed by `available_defaults`: when a structure
                // explicitly provides a member, that member is the authoritative value the
                // compiler will use — any chain default for the same name is discarded.
                // A type mismatch on an explicit member is therefore always an error,
                // regardless of whether a well-typed default exists elsewhere in the chain.
                // Widening this arm to also check `available_defaults` would silently accept
                // structures whose explicit members carry the wrong type, which is incorrect.
                if !implicitly_converts_to(actual_type, expected_type) {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "type mismatch for trait member '{}': expected {}, got {}",
                            req.name, expected_type, actual_type
                        ))
                        .with_code(DiagnosticCode::TypeMismatchForTraitMember)
                        .with_label(DiagnosticLabel::new(structure.span, "type mismatch")),
                    );
                }
            }
            None => {
                // Check if a matching default from another trait satisfies this requirement.
                // Only a same-kind default can satisfy: a `let` default does NOT satisfy
                // a `param` requirement (param slots must be externally settable).
                // The (name, kind) composite key means the lookup is already kind-filtered —
                // no additional kind-guard is needed on the match arms.
                //
                // Note: `.get(&(req.name.clone(), ...))` allocates a String on every lookup
                // because `HashMap<(String, K), V>` has no `Borrow` impl for `(&str, K)`.
                // Requirement counts are small in practice so this is not a hot path; if it
                // ever becomes one, switch to a two-level map `HashMap<String, HashMap<K, V>>`.
                match available_defaults.get(&(req.name.clone(), required_default_kind)) {
                    Some(default_type) if implicitly_converts_to(default_type, expected_type) => {
                        // Same-kind default with matching type satisfies the requirement.
                    }
                    Some(default_type) => {
                        // Same-kind default but wrong type → type mismatch.
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "type mismatch for trait member '{}': \
                                 requirement expects {}, available default has {}",
                                req.name, expected_type, default_type
                            ))
                            .with_code(DiagnosticCode::TypeMismatchForTraitMember)
                            .with_label(DiagnosticLabel::new(structure.span, "type mismatch")),
                        );
                    }
                    None => {
                        // No default of the required kind — treat as missing.
                        // A param requirement with only a let default in scope means the
                        // structure must provide a settable param slot itself.
                        // Only mention the required kind when the structure declares the
                        // name in the opposite-kind map (the actionable "wrong kind" case);
                        // otherwise the suffix is noise — the user simply forgot the member.
                        let opposite_kind_members = match required_default_kind {
                            AvailableDefaultKind::Param => structure_let_members,
                            AvailableDefaultKind::Let => structure_param_members,
                        };
                        let message = if opposite_kind_members.contains_key(&req.name) {
                            let kind_str = match required_default_kind {
                                AvailableDefaultKind::Param => "param",
                                AvailableDefaultKind::Let => "let",
                            };
                            format!(
                                "missing required member '{}' (expected type: {}; requires a `{}` slot)",
                                req.name, expected_type, kind_str
                            )
                        } else {
                            format!(
                                "missing required member '{}' (expected type: {})",
                                req.name, expected_type
                            )
                        };
                        diagnostics.push(
                            Diagnostic::error(message)
                                .with_code(DiagnosticCode::MissingRequiredMember)
                                .with_label(DiagnosticLabel::new(
                                    structure.span,
                                    "required by trait",
                                )),
                        );
                    }
                }
            }
        }
    }
}

/// Phase 6 of trait conformance checking: inject trait defaults for non-overridden members.
///
/// For each entry in `ctx.defaults`, if the structure does not already declare a member
/// with that name (checked via `structure_members`), injects the default as a `ValueCellDecl`
/// or `CompiledConstraint`. Handles three default kinds:
///
/// ## `DefaultKind::Param`
///
/// Creates a `ValueCellKind::Param` cell with an optional compiled default expression.
/// The `default_decl.default` expression (if present) is compiled on the fly via `compile_expr`.
///
/// ## `DefaultKind::Let`
///
/// Creates a `ValueCellKind::Let` cell. For **annotated** lets (`cell_type: Some(_)`) the
/// expression is compiled freshly. For **unannotated** lets the compiled expression is taken
/// from `inferred_let_exprs` (populated by phase 3's Pass 2), consuming the entry via `.remove()`.
///
/// ### Annotated-Let pass1_skipped short-circuit
///
/// When `cell_type.is_some() && pass1_skipped.contains(name)`, the annotated-Let default lost
/// the Pass 1 `register_if_absent` race to an earlier same-name default (typically a `Param`).
/// The winner's injection arm will emit the cell; this arm must not emit a duplicate. Silent
/// `continue` before `compile_expr` is called — no cell is pushed, no diagnostic is emitted.
/// This is the symmetric mirror of the `pass2_skipped` guard for unannotated Lets (task 1952).
///
/// ### Cache miss handling — three cases
///
/// - **(a) Deliberate skip** (`pass2_skipped.contains(name)`): Pass 2 found an annotated `Param`
///   or `Let` already occupying the scope slot and did not cache this expression. Silent `continue`
///   — the `Param`/annotated-`Let` injection arm will inject its own cell for this name. This
///   prevents duplicate `(entity, member)` cells.
/// - **(b) Pass 2 compile error** (`pass2_compile_errors.contains(name)`): Pass 2's `compile_expr`
///   call pushed at least one diagnostic; the expression is broken. Silent `continue` — the
///   root-cause diagnostic has already been emitted. Injecting a cell with a poisoned type would
///   add noise without corrective value. (task 1914 suggestion #1)
/// - **(c) Unexpected drift**: a refactor decoupled the pre-register guard from this injection guard
///   (e.g. changed the cache key). `debug_assert!(false, …)` fires in dev/test; an error
///   diagnostic fires in release rather than silently recompiling (which would risk duplicating
///   diagnostics already pushed by Pass 2 for the same AST node).
///
/// ### Annotation-vs-expression type check
///
/// When the `Let` default carries an annotation, `type_compatible` (not `implicitly_converts_to`)
/// is used to honor `Int→Real` widening: `let x : Real = 42` lowers `42` as `Type::Int`
/// (no decimal in the source token; see `expr.rs:388-395`) but the annotation captures the
/// user's `Real` intent. This matches the widening relation applied throughout the rest of
/// the compiler (`type_compat.rs:81`). See task 1834 `esc-1834-58` for the trade-off.
///
/// The annotation is authoritative on the injected cell type when present; the inferred
/// expression type is the fallback.
///
/// ## `DefaultKind::Constraint`
///
/// Compiles the constraint expression and pushes a `CompiledConstraint` onto `constraints`,
/// unless the structure already declares a constraint with the same label
/// (`structure_constraint_labels` lookup). `constraint_index` is incremented for each injected
/// constraint (consistent with the entity-level index allocation in `entity.rs`).
///
/// ## Ownership note
///
/// `inferred_let_exprs` is consumed by value because this loop calls `.remove()` on it —
/// each unannotated-let default is reused exactly once. Callers should pass ownership
/// after phase 4 finishes reading it for the advertisement map.
#[allow(clippy::too_many_arguments)]
pub(super) fn check_phase_inject_defaults(
    ctx: &MergeContext,
    structure: &EntityDefRef<'_>,
    structure_members: &HashMap<String, Type>,
    structure_constraint_labels: &HashSet<String>,
    mut inferred_let_exprs: HashMap<(String, AvailableDefaultKind), CompiledExpr>,
    pass1_skipped: &HashSet<String>,
    pass1_param_skipped: &HashSet<String>,
    pass2_skipped: &HashSet<String>,
    pass2_compile_errors: &HashSet<String>,
    scope: &mut CompilationScope,
    value_cells: &mut Vec<ValueCellDecl>,
    constraints: &mut Vec<CompiledConstraint>,
    constraint_index: &mut u32,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Inject defaults for members not overridden by the structure.
    for default in &ctx.defaults {
        match &default.kind {
            DefaultKind::Param {
                cell_type,
                default_decl,
            } => {
                let name = default
                    .name
                    .as_deref()
                    .expect("DefaultKind::Param always has Some(name)");
                // SYMMETRIC FIX (task 2208): Param losers in Pass 1 are recorded in
                // `pass1_param_skipped` (the mirror of `pass1_skipped` for annotated-Let
                // losers). When the Param's `register_if_absent` found the scope slot already
                // claimed by an earlier annotated Let, the annotated Let will inject the
                // definitive cell; skip emission here to prevent duplicate `(entity, member)`
                // cells. Silent `continue` — before `compile_expr`, no cell, no diagnostic.
                if pass1_param_skipped.contains(name) {
                    continue;
                }
                if !structure_members.contains_key(name) {
                    // Inject default param into value_cells
                    let cell_id = ValueCellId {
                        entity: structure.name.to_string(),
                        member: name.to_string(),
                    };

                    let default_expr = default_decl
                        .default
                        .as_ref()
                        .map(|expr| compile_expr(expr, scope, enum_defs, functions, diagnostics));

                    value_cells.push(ValueCellDecl {
                        id: cell_id,
                        kind: ValueCellKind::Param,
                        visibility: Visibility::Private,
                        is_aux: false,
                        cell_type: cell_type.clone(),
                        default_expr,
                        solver_hints: Vec::new(),
                        span: default.span,
                    });
                }
            }
            DefaultKind::Let {
                cell_type,
                let_decl,
            } => {
                let name = default
                    .name
                    .as_deref()
                    .expect("DefaultKind::Let always has Some(name)");
                if !structure_members.contains_key(name) {
                    let cell_id = ValueCellId {
                        entity: structure.name.to_string(),
                        member: name.to_string(),
                    };

                    // SYMMETRIC FIX (task 1952): annotated-Let losers in Pass 1 are recorded
                    // in `pass1_skipped` (the mirror of `pass2_skipped` for unannotated-Let
                    // losers in Pass 2).  When `cell_type.is_some() && pass1_skipped.contains(name)`
                    // the annotated Let lost the `register_if_absent` race to an earlier
                    // same-name default (typically a Param registered first in ctx.defaults
                    // order).  The winner's injection arm will emit the definitive cell;
                    // skip emission here to prevent duplicate `(entity, member)` cells.
                    // This mirrors the `pass2_skipped` / None-cache-miss guard in the
                    // `None` arm of the `DefaultKind::Let` dispatch below in this same
                    // function.
                    if cell_type.is_some() && pass1_skipped.contains(name) {
                        continue;
                    }

                    // Reuse the compiled_expr cached by the pre-register/inference
                    // pass (task 1834 step-9) to avoid a second compilation of the
                    // same expression.  The dispatch mirrors the pre-register
                    // branches: unannotated lets populate the cache unless Pass 2
                    // found the scope slot already claimed (recorded in `pass2_skipped`)
                    // or Pass 2 compilation emitted a diagnostic (`pass2_compile_errors`);
                    // annotated lets never use the cache.
                    //
                    // Cache miss handling: three reasons a `None` arm miss can occur:
                    //   (a) Deliberate skip (`pass2_skipped.contains(name)`): Pass 2
                    //       found an annotated type claiming the scope slot and did not
                    //       cache the expression.  Silent `continue` — the Param/
                    //       annotated-Let default will inject its own cell for this name.
                    //   (b) Pass 2 compile error (`pass2_compile_errors.contains(name)`):
                    //       compile_expr pushed at least one diagnostic; expression broken.
                    //       Silent `continue` — the root-cause diagnostic was already
                    //       emitted; injecting a poisoned cell adds noise, not value.
                    //   (c) Unexpected drift: a refactor decoupled the pre-register
                    //       guard from the injection guard or changed the cache key.
                    //       `debug_assert!(false, …)` fires in dev/test; the error
                    //       diagnostic fires in release rather than silently recompiling
                    //       (which would risk duplicating diagnostics already pushed by
                    //       Pass 2 for the same AST node).
                    let compiled_expr = match cell_type {
                        Some(_) => {
                            compile_expr(&let_decl.value, scope, enum_defs, functions, diagnostics)
                        }
                        None => {
                            match inferred_let_exprs
                                .remove(&(name.to_string(), AvailableDefaultKind::Let))
                            {
                                Some(ce) => ce,
                                None => {
                                    if pass2_skipped.contains(name) {
                                        // (a) Deliberate skip: Pass 2 found an annotated
                                        // type already occupying the scope slot and
                                        // did not cache this expression (see the `pass2_skipped`
                                        // parameter populated by check_phase_pre_register_default_types).
                                        // The Param/annotated-Let default will
                                        // inject its own cell; skip Let injection here
                                        // to prevent duplicate (entity, member) cells.
                                        continue;
                                    }
                                    if pass2_compile_errors.contains(name) {
                                        // (b) Pass 2 compile error: compile_expr pushed at
                                        // least one diagnostic for this expression — the
                                        // root-cause error is already in diagnostics.
                                        // Silently skip injection to avoid a poisoned cell
                                        // with no additional user value.
                                        continue;
                                    }
                                    // (c) Unexpected: pre-register guard and injection guard
                                    // have diverged, or the cache key changed.
                                    debug_assert!(
                                        false,
                                        "unannotated let '{}' has no cached compiled expression \
                                         and is not in pass2_skipped or pass2_compile_errors — \
                                         drift between the pre-register guard and the injection \
                                         guard in check_phase_pre_register_default_types / \
                                         check_phase_inject_defaults",
                                        name
                                    );
                                    diagnostics.push(
                                        Diagnostic::error(format!(
                                            "internal error: compiled expression for unannotated \
                                             trait let '{}' was not cached by the pre-register \
                                             pass; this indicates a drift between the pre-register \
                                             and injection guards in \
                                             check_phase_pre_register_default_types / check_phase_inject_defaults",
                                            name
                                        ))
                                        .with_label(
                                            DiagnosticLabel::new(
                                                default.span,
                                                "internal consistency",
                                            ),
                                        ),
                                    );
                                    continue;
                                }
                            }
                        }
                    };

                    // Cross-check the expression type against the let's annotation.
                    // The annotation captures user intent; any drift here is an error.
                    //
                    // Use `type_compatible` (not `implicitly_converts_to`) so the check
                    // honors Int→Real widening — `let x : Real = 42` lowers `42` as
                    // `Type::Int` (no decimal token; see expr.rs:388-395) while the
                    // annotation captures the user's `Real` intent. `type_compatible` is
                    // the same widening relation applied throughout type checking
                    // (type_compat.rs:81), so accepting it here matches the rest of the
                    // compiler instead of being stricter at this one site. See task 1834
                    // esc-1834-58 for the trade-off; the requirement-vs-member sites inside
                    // `check_phase_check_members_against_requirements` keep the stricter
                    // `implicitly_converts_to` because they compare two annotated types
                    // (no Int-literal source).
                    if let Some(annotation_ty) = cell_type
                        && !type_compatible(annotation_ty, &compiled_expr.result_type)
                    {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "type mismatch for trait let '{}': annotation expects {}, expression evaluates to {}",
                                name, annotation_ty, compiled_expr.result_type
                            ))
                            .with_code(DiagnosticCode::TypeMismatchForTraitMember)
                            .with_label(DiagnosticLabel::new(default.span, "type mismatch")),
                        );
                    }

                    // Annotation is authoritative on the injected cell type when present
                    // (matches the scope pre-registration in check_phase_pre_register_default_types
                    // (Pass 1) which also prefers the annotation over the inferred expression type).
                    // Falls back to the inferred expression type, then to Type::Real (defensive).
                    // Uses the shared resolve_let_advertised_type helper for site 2 of 2.
                    let injected_cell_type =
                        resolve_let_advertised_type(cell_type, Some(&compiled_expr));

                    value_cells.push(ValueCellDecl {
                        id: cell_id,
                        kind: ValueCellKind::Let,
                        visibility: Visibility::Private,
                        is_aux: false,
                        cell_type: injected_cell_type,
                        default_expr: Some(compiled_expr),
                        solver_hints: Vec::new(),
                        span: default.span,
                    });
                }
            }
            DefaultKind::Constraint(constraint_decl) => {
                let label = constraint_decl.label.as_deref();
                let already_has = label.is_some_and(|l| structure_constraint_labels.contains(l));
                if !already_has {
                    let compiled_expr = compile_expr(
                        &constraint_decl.expr,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                    );

                    let constraint_id = ConstraintNodeId {
                        entity: structure.name.to_string(),
                        index: *constraint_index,
                    };
                    *constraint_index += 1;

                    constraints.push(CompiledConstraint {
                        id: constraint_id,
                        label: constraint_decl.label.clone(),
                        expr: compiled_expr,
                        span: default.span,
                        domain: None,
                        optimized_target: None,
                    });
                }
            }
            // Assoc-fn defaults are not injected as value-cells or constraints;
            // they are resolved into the assoc-fn table by the dedicated
            // assoc-fn phase (task 3939 δ, step-8).
            DefaultKind::Fn(_) => {}
            // Assoc-type defaults are not injected as value-cells or constraints;
            // they are resolved into the assoc-type table by the dedicated
            // assoc-type phase (step-10).
            DefaultKind::AssocType(_) => {}
        }
    }
}
