//! Geometry function dispatch for the Reify compiler.
//!
//! # Silent-fallback vs. labelled-per-arg policy
//!
//! When a positional argument to a geometry call is **not** a geometry expression,
//! two policies apply depending on the function family:
//!
//! - **sweep / sweep_guided** → labelled per-arg diagnostic via `resolve_named_geom_arg`.
//!   Arity is fixed and each slot is individually meaningful (profile, path, guide), so
//!   one actionable error per slot is the right UX.
//!
//! - **loft / loft_guided** → silent fallback to `GeomRef::Step(step_offset + i)` via
//!   `resolve_loft_like_args`. The loft family is variadic (≥ 2 profiles), so emitting
//!   one diagnostic per profile slot would flood the user with N near-duplicate
//!   "argument K must be a geometry expression" errors instead of one actionable signal.
//!   The silent fallback also preserves per-profile index uniqueness for downstream
//!   analysis.
//!
//! - **Single-geom-arg ops** (extrude, revolve, translate, …) → silent fallback via the
//!   local `geom_ref` closure. Same rationale as loft: one geometry slot, no actionable
//!   per-slot error needed.
//!
//! See `loft_guided_compiler_non_geom_args_silent_fallback` and
//! `loft_non_geom_args_fallback_uses_step_offset` for the behavioural tests that pin
//! this policy.

use super::*;

/// Returns `true` iff `expr` should be lowered as a geometry realization (a
/// `GeomRef` or equivalent solid-geometry node) rather than as a plain value
/// cell.
///
/// Three expression forms are recognised:
///
/// - **`FunctionCall`** — `true` when the callee name is a built-in geometry
///   function (`is_geometry_function`) *and* no user function with the same name
///   exists in `functions`. The `functions` slice is queried via
///   `.iter().any(…)` and is therefore **order-independent** here. The
///   first-match-wins user-vs-prelude shadow rule that determines which entries
///   appear in `functions` is applied upstream by `merge_prelude_functions`
///   (`lib.rs`).
///
/// - **`Ident`** — `true` when the identifier name is already present in
///   `known_geometry_lets`. No `functions` shadow check is needed: an identifier
///   is syntactically distinct from a function call, so a user-defined function
///   cannot collide with a geometry let via this branch.
///
/// - **`Conditional`** — `true` when EITHER branch (recursively) classifies as
///   a geometry expression. The let is then routed to `compile_geometry_call`
///   where it surfaces a clean compile-time Error explaining that
///   geometry-typed if-then-else is not yet supported (see task 3395).
///   Returning `false` here would leave the let as a plain value cell and
///   silently produce the cryptic "unresolvable GeomRef::Step(0)" crash.
///   "Either branch" is sufficient because mixed-type Conditionals are caught
///   by the type system elsewhere; we just need any geometry-branch path to
///   route through `compile_geometry_call`'s new Error arm.
///
/// - **`Match`** — `true` when ANY arm body (recursively) classifies as a
///   geometry expression. Same rationale as Conditional: "any arm" is
///   sufficient because mixed-type Match arms are caught by the type system
///   elsewhere; we just need any geometry-arm path to route to the Error arm
///   in `compile_geometry_call` (see task 3418).
///
/// # Ordering invariant for `known_geometry_lets`
///
/// `known_geometry_lets` is built **incrementally** by the caller. It grows as
/// each member is visited in `compile_entity`'s pass 1 (entity.rs) and inside
/// `register_guarded_names` / `compile_guarded_members` (guards.rs).
/// Consequently, whether an `Ident` expression is classified as a geometry let
/// depends on *when* it is evaluated relative to the aliased name being
/// inserted:
///
/// - If a let's value is `Ident("a")` and `"a"` is already in
///   `known_geometry_lets`, the let is classified as geometry and its own name
///   is appended — transitive chaining works forward.
/// - If `"a"` has not yet been inserted (the alias appears before its referent
///   in member order), the let is **not** classified as geometry, even if `"a"`
///   is eventually inserted when that later member is processed.
///
/// This intentional conservative behaviour is pinned by
/// `let_scope_tests::cyclic_ident_alias_does_not_crash`, whose inline comment
/// notes "the forward-pass incremental set never adds either to
/// known_geometry_lets".
///
/// Contrast with scope *name resolution*, which is fully order-free: all names
/// are registered in pass 1 before any expression is compiled in pass 2, so
/// an expression in pass 2 may freely reference a name declared later in the
/// member list.
///
/// Called from two pre-pass sites that must stay consistent: the `compile_entity`
/// pre-pass (entity.rs:~531) and `register_guarded_names` (guards.rs:~183).
/// Formerly also referenced via `is_solid_geometry_param` — that wrapper was
/// retired in GHR-γ (task 3605).
///
/// Returns `true` if `expr` is syntactically a selector-producing expression:
/// - A direct call to one of the 7 predicate/all selector constructors
///   (`faces`, `edges`, `faces_by_normal`, `faces_by_area`, `edges_by_length`,
///   `edges_at_height`, `edges_parallel_to`) or the Named-leaf constructors
///   (`face`, `edge`, `solid_body`, added by task 4119 δ).
/// - A nested selector composition (`union`/`intersect`/`difference`) whose
///   operands are themselves selector exprs (recursive).
///
/// IMPORTANT: does NOT include `split`, `adjacent_faces`, or `shared_edges` —
/// those produce `Type::List(Geometry)`, not a composable `Value::Selector`.
/// Called by `is_geometry_let` to detect when `union`/`difference` operands
/// are selector-valued and thus MUST NOT route to the CSG path.
///
/// # Ident operands and the `known_selector_lets` accumulator
///
/// `known_selector_lets` is the incremental set of let names whose values have
/// already been classified as selector expressions in the current pre-pass walk.
/// An `ExprKind::Ident(name)` arm returns `true` iff `name` is present in that
/// set, enabling all-ident compositions like `let u = union(top, big)` (where
/// `top`/`big` are themselves selector-typed lets) to be correctly identified.
///
/// The flip is **gated on membership**: a genuinely-unknown ident (not in
/// `known_selector_lets`) returns `false`, preserving the CSG default for
/// idents that have not been classified as selector lets.
///
/// Promoted to `pub(crate)` so `entity.rs` and `guards.rs` can reuse this
/// function as the selector-let classifier when populating `known_selector_lets`
/// (avoids a duplicate detector that could drift from the composition-check
/// logic). (task 4527)
pub(crate) fn is_selector_expr(
    expr: &reify_ast::Expr,
    functions: &[CompiledFunction],
    known_selector_lets: &HashSet<&str>,
) -> bool {
    match &expr.kind {
        reify_ast::ExprKind::FunctionCall { name, args, .. } => {
            let name = name.as_str();
            let args = args.as_slice();

            // User-defined functions shadow any builtin: if a function with this name
            // is in scope, the call is not guaranteed to produce a Selector.
            if functions.iter().any(|f| f.name == name) {
                return false;
            }

            match name {
                // ── 7 predicate/all selector constructors (task 4118 γ) ─────────────
                "faces" | "edges" | "faces_by_normal" | "faces_by_area" | "edges_by_length"
                | "edges_at_height" | "edges_parallel_to" => true,
                // ── Named-leaf constructors (task 4119 δ) ───────────────────────────
                "face" | "edge" | "solid_body" => true,
                // ── Attribute-role selector (task 4536) ─────────────────────────────
                // mid_surface(body) -> Selector(Face): a single-arg constructor that
                // builds a LeafQuery::ByRole(MidSurfaceFace) leaf. Classified here so
                // `let m = mid_surface(b)` routes through the selector path, not CSG.
                "mid_surface" => true,
                // ── Selector composition (recursive) ────────────────────────────────
                // "union" and "difference" are also CSG names, so we recurse to check
                // that at least one operand is itself a selector expr before committing.
                // "intersect" is not a geometry function (never reached CSG path), but
                // we still validate that its operands look like selectors for clarity.
                "union" | "intersect" | "difference" => {
                    args.iter().any(|arg| is_selector_expr(arg, functions, known_selector_lets))
                }
                _ => false,
            }
        }
        // Ident operands bound to already-classified selector lets (task 4527).
        // Only names present in `known_selector_lets` return true — genuinely-unknown
        // idents default to false (preserving CSG routing for unknown idents).
        reify_ast::ExprKind::Ident(name) => known_selector_lets.contains(name.as_str()),
        _ => false,
    }
}

pub(crate) fn is_geometry_let(
    expr: &reify_ast::Expr,
    functions: &[CompiledFunction],
    known_geometry_lets: &HashSet<&str>,
    known_selector_lets: &HashSet<&str>,
) -> bool {
    match &expr.kind {
        reify_ast::ExprKind::FunctionCall { name, args, .. } => {
            // Disambiguate the CSG `sweep(profile, path) -> Solid` (docs §3,
            // 2-ary geometry) from the kinematic
            // `sweep(mechanism, joint, range, steps) -> List<Snapshot>`
            // (docs §13.4, 4-ary eval-time builtin) by arity. The 4-arg
            // form is not a geometry let — it routes through eval-time
            // dispatch where the kinematic arm resolves it. Other arities
            // still flow into compile_geometry_call's sweep arm and get
            // its strict "expects exactly 2 arguments" diagnostic.
            let is_kinematic_sweep = name == "sweep" && args.len() == 4;
            // Task 4119 δ: disambiguate CSG `union`/`difference` (geometry boolean
            // ops) from selector-composition `union`/`intersect`/`difference`
            // (selector algebra combinators). When ANY operand is syntactically a
            // selector-producing expression the call routes to the value-typing path
            // where E_SELECTOR_KIND_MISMATCH is checked. Mirrors the sweep-arity
            // guard above.
            // NOTE: "intersect" (selector combinator) ≠ "intersection" (CSG); they
            // are distinct names. "intersect" is never a geometry function so it
            // cannot reach this arm — but including it in the guard keeps this list
            // in sync with `units.rs`'s selector_composition_result_type (which
            // handles "union"|"intersect"|"difference"). "intersection" is the CSG
            // name and is intentionally absent: a call like `intersection(faces(b),…)`
            // is a misuse of the CSG op, not a selector composition; routing it to
            // is_geometry_function gives the user a geometry-type error rather than
            // a silent Undef via the selector path.
            let is_selector_composition =
                matches!(name.as_str(), "union" | "intersect" | "difference")
                    && args.iter().any(|a| is_selector_expr(a, functions, known_selector_lets));
            is_geometry_function(name)
                && !functions.iter().any(|f| f.name == *name)
                && !is_kinematic_sweep
                && !is_selector_composition
        }
        // No `!functions.iter().any(...)` guard needed: `known_geometry_lets` is
        // populated only from let-binding names (never function names), and an Ident
        // expression is syntactically distinct from FunctionCall, so a user-defined
        // function cannot collide with a geometry let via this branch.
        reify_ast::ExprKind::Ident(name) => known_geometry_lets.contains(name.as_str()),
        // Conditional — see rustdoc above for rationale (task 3395).
        reify_ast::ExprKind::Conditional {
            then_branch,
            else_branch,
            ..
        } => {
            is_geometry_let(then_branch, functions, known_geometry_lets, known_selector_lets)
                || is_geometry_let(else_branch, functions, known_geometry_lets, known_selector_lets)
        }
        // Match — see rustdoc above for rationale (task 3418).
        reify_ast::ExprKind::Match { arms, .. } => arms
            .iter()
            .any(|arm| is_geometry_let(&arm.body, functions, known_geometry_lets, known_selector_lets)),
        // Future branching/wrapping ExprKinds (e.g. pipe expressions,
        // try/else-style fallbacks) extend here with the same
        // any-sub-yields-geometry pattern.  Note: ExprKind has no Block variant
        // (parenthesised expressions are unwrapped during lowering and never
        // reach this predicate as a distinct kind); Lambda bodies produce a
        // function value, not a geometry value, so they are not candidates.
        _ => false,
    }
}

/// Returns the arg indices that are geometry refs for each non-boolean geometry function.
/// Empty slice means no geometry args (primitives, curves).
/// Boolean ops are excluded — they handle geometry args with their own recursive block.
fn geometry_arg_indices(name: &str) -> &'static [usize] {
    match name {
        "translate" | "rotate" | "scale" | "rotate_around" | "apply_transform"
        | "circular_pattern" | "linear_pattern" | "mirror" | "extrude" | "extrude_symmetric"
        | "revolve" | "revolve_full" | "shell" | "shell_open" | "thicken" | "offset_solid"
        | "offset_curve" | "draft" | "chamfer" | "chamfer_asymmetric" | "fillet" | "fillet_all"
        | "zone_slab" | "zone_cylinder" | "zone_annulus" | "zone_profile" => &[0],
        "sweep" => &[0, 1],
        "sweep_guided" => &[0, 1, 2],
        "pipe" => &[0],
        // NOTE: `loft` is handled specially (variadic geometry args) in the resolution block.
        // IMPORTANT: New geometry functions that take geometry args MUST be registered here
        // (or handled like loft for variadic cases). Missing entries are silently treated as
        // having no geometry args, breaking let-bound geometry references for those functions.
        _ => &[],
    }
}

/// Resolve the geometry ref for a named positional argument of a sweep-family
/// dispatch arm, emitting a per-argument diagnostic when the arg is not a
/// geometry expression and falling back to `GeomRef::Step(step_offset + idx)`.
///
/// Used by `sweep` and `sweep_guided`. Centralising here keeps the diagnostic
/// wording (`"{name}() {label} (argument {n}) must be a geometry expression"`)
/// and the fallback step index in sync across arms.
///
/// For the loft family (silent fallback, no diagnostic), see
/// `resolve_loft_like_args` and the module-level note on
/// silent-fallback vs. labelled-per-arg policy.
fn resolve_named_geom_arg(
    idx: usize,
    fn_name: &str,
    arg_label: &str,
    args: &[reify_ast::Expr],
    geom_refs: &HashMap<usize, GeomRef>,
    diagnostics: &mut Vec<Diagnostic>,
    step_offset: usize,
) -> GeomRef {
    if let Some(r) = geom_refs.get(&idx).cloned() {
        return r;
    }
    diagnostics.push(
        Diagnostic::error(format!(
            "{}() {} (argument {}) must be a geometry expression",
            fn_name,
            arg_label,
            idx + 1,
        ))
        .with_label(DiagnosticLabel::new(
            args[idx].span,
            "not a geometry expression",
        )),
    );
    GeomRef::Step(step_offset + idx)
}

/// Build the `(profiles, named_args)` pair for a loft-family dispatch arm.
///
/// For each arg slot `0..n` (where `n = compiled_args.len()`):
/// - `profiles[i]` is `geom_refs[i]` when present, otherwise silently
///   `GeomRef::Step(step_offset + i)`.
/// - `named_args[i].0` is `"profile_{i}"` for all slots; when
///   `guide_suffix` is `true` the last slot's key is `"guide"` instead.
///
/// `compiled_args` is consumed by value.
///
/// See the module-level "Silent-fallback vs. labelled-per-arg policy" note for rationale.
fn resolve_loft_like_args(
    compiled_args: Vec<CompiledExpr>,
    geom_refs: &HashMap<usize, GeomRef>,
    step_offset: usize,
    guide_suffix: bool,
) -> (Vec<GeomRef>, Vec<(String, CompiledExpr)>) {
    let n = compiled_args.len();
    // Helper-level precondition: guide_suffix=true requires ≥ 2 args (profile + guide).
    // Current callers (loft_guided arm) enforce n >= 3 upstream, so this assert only fires
    // for hypothetical future call sites that bypass those arity guards.  It is intentionally
    // weaker than the caller contract — the helper owns n >= 2, callers own user-visible arity.
    debug_assert!(
        !guide_suffix || n >= 2,
        "loft_guided requires at least 2 args: profiles + guide"
    );
    let profiles: Vec<GeomRef> = (0..n)
        .map(|i| {
            geom_refs
                .get(&i)
                .cloned()
                .unwrap_or(GeomRef::Step(step_offset + i))
        })
        .collect();
    let named_args: Vec<(String, CompiledExpr)> = compiled_args
        .into_iter()
        .enumerate()
        .map(|(i, expr)| {
            let key = if guide_suffix && n > 0 && i == n - 1 {
                "guide".to_string()
            } else {
                format!("profile_{}", i)
            };
            (key, expr)
        })
        .collect();
    (profiles, named_args)
}

/// Recognise the cross-sub geometry pattern `self.<sub>.<member>` where
/// `<sub>` is a non-collection sub of the current entity AND `<member>` is a
/// geometry realisation on the sub's child template (per
/// `scope.sub_realization_names[sub].contains(member)`).
///
/// On a match, returns `Some(GeomRef::Sub(format!("{}.{}", sub, member)))`.
/// The compound key `"<sub>.<member>"` is the eval-side handshake: the engine
/// populates `named_steps["<sub>.<member>"]` with the child template's named
/// realisation handle before processing the parent's ops (see
/// `engine_build.rs` cross-template threading, task 3441).
///
/// Returns `None` for every other shape — the caller falls through to its
/// existing recursive `compile_geometry_call(...)` path.
///
/// Collection-sub accesses (e.g. `bolts[0].body`, `self.bolts.body`) are
/// **not** recognised here; collection-sub geometry composition is deferred
/// past v0.1 because per-instance handles would require per-element realisation.
/// The parallel value-level diagnostic continues to fire in `expr.rs` at the
/// two collection-sub call sites.
/// Match the two-level `self.<sub>.<member>` MemberAccess AST shape.
///
/// Returns `Some((sub_name, member))` when `expr` is exactly
/// `MemberAccess { object: MemberAccess { object: Ident("self"), member: sub }, member }`,
/// i.e. the cross-sub `self.<sub>.<member>` pattern.  Returns `None` for any
/// other expression shape (bare ident, one-level member access, indexed
/// access, etc.).
///
/// This is the **single source of truth** for detecting the `self.<sub>.<member>`
/// AST shape.  Callers apply their own domain-specific filters on top (e.g. the
/// `collection_sub_names` check in `try_resolve_cross_sub_geom_ref`, or the
/// `try_emit_cross_sub_geometry` call in `geometry_boolean.rs`).
pub(crate) fn match_self_sub_member(
    expr: &reify_ast::Expr,
) -> Option<(&str, &str)> {
    if let reify_ast::ExprKind::MemberAccess { object, member } = &expr.kind
        && let reify_ast::ExprKind::MemberAccess {
            object: inner_obj,
            member: sub_name,
        } = &object.kind
        && let reify_ast::ExprKind::Ident(self_name) = &inner_obj.kind
        && self_name == "self"
    {
        Some((sub_name.as_str(), member.as_str()))
    } else {
        None
    }
}

pub(crate) fn try_resolve_cross_sub_geom_ref(
    expr: &reify_ast::Expr,
    scope: &CompilationScope<'_>,
) -> Option<GeomRef> {
    if let Some((sub_name, member)) = match_self_sub_member(expr)
        && !scope.collection_sub_names.contains(sub_name)
    {
        // Single source of truth shared with
        // `expr.rs::try_resolve_cross_sub_geometry_value_ref` (task 3455) — the
        // value-ref / GeomRef::Sub handshake stays in lockstep by construction.
        if scope.sub_member_is_cross_sub_geometry_or_forward_declared(sub_name, member) {
            return Some(GeomRef::Sub(format!("{}.{}", sub_name, member)));
        }
    }
    None
}

/// Returns `true` when `expr` is a bare `self.<sub>.<member>` cross-sub
/// geometry reference that would be lowered to a synthetic identity-translate
/// realization. Thin wrapper over `try_resolve_cross_sub_geom_ref` — single
/// source of truth, no drift from the resolver.
pub(crate) fn is_bare_cross_sub_geometry_alias(
    expr: &reify_ast::Expr,
    scope: &CompilationScope<'_>,
) -> bool {
    try_resolve_cross_sub_geom_ref(expr, scope).is_some()
}

// ─── task-3815: scalar-arg hoisting for geometry-typed if-then-else ──────────

/// Rewrite a geometry-typed `if cond then a else b` into a single geometry call
/// whose differing scalar leaves become scalar `if cond then x else y`
/// sub-expressions, all of which `compile_expr` already lowers to
/// `CompiledExprKind::Conditional` (evaluated branch-selectively by
/// `eval_expr` at run time).
///
/// Returns `Some(merged)` only when the merged root is itself a geometry
/// `FunctionCall` — i.e. both branches had the same geometry constructor name
/// and arity.  Returns `None` for structurally-incompatible branches
/// (box vs cylinder, arity mismatch, Ident-let branch), which then fall through
/// to the existing graceful compile-time Error.
///
/// Example:
/// ```text
/// if c then box(40, 40, 40) else box(80, 20, 20)
///   →  box(if c then 40 else 80, if c then 40 else 20, if c then 40 else 20)
/// ```
pub(crate) fn try_hoist_geometry_conditional(
    expr: &reify_ast::Expr,
    functions: &[CompiledFunction],
) -> Option<reify_ast::Expr> {
    let (cond, then_branch, else_branch) = match &expr.kind {
        reify_ast::ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => (
            condition.as_ref(),
            then_branch.as_ref(),
            else_branch.as_ref(),
        ),
        _ => return None,
    };

    let merged = merge_branches(cond, then_branch, else_branch, functions, expr.span);

    // `merge_branches` only emits a FunctionCall root when both branches were
    // matching geometry constructors that were NOT user-shadowed — those checks
    // are already enforced inside `merge_branches` (the `is_geometry_function +
    // user-shadow` predicate at the FunctionCall-match site).  The guard here
    // just distinguishes "a merged geometry call was produced" (FunctionCall
    // root) from "the scalar-Conditional fallback was returned" (Conditional).
    match &merged.kind {
        reify_ast::ExprKind::FunctionCall { .. } => Some(merged),
        _ => None,
    }
}

/// Returns `true` when `a` and `b` are structurally identical scalar leaf
/// expressions — `NumberLiteral`, `BoolLiteral`, or `Ident` with the same
/// payload.
///
/// Used by [`merge_branches`] as a cheap peephole to avoid wrapping a
/// shared-constant argument in a redundant `Conditional { cond, x, x }` node
/// (common for boilerplate zero-valued axes in 3D ops such as `translate`).
/// Only the three leaf forms that are safe to compare structurally are matched;
/// any other kind returns `false` to stay conservative.
fn are_scalar_equal(a: &reify_ast::Expr, b: &reify_ast::Expr) -> bool {
    match (&a.kind, &b.kind) {
        (
            reify_ast::ExprKind::NumberLiteral {
                value: va,
                is_real: ra,
            },
            reify_ast::ExprKind::NumberLiteral {
                value: vb,
                is_real: rb,
            },
        ) => va == vb && ra == rb,
        (reify_ast::ExprKind::BoolLiteral(va), reify_ast::ExprKind::BoolLiteral(vb)) => {
            va == vb
        }
        (reify_ast::ExprKind::Ident(na), reify_ast::ExprKind::Ident(nb)) => na == nb,
        _ => false,
    }
}

/// Recursively merge two AST expressions `a` and `b` under condition `cond`.
///
/// Merging rule:
/// - If both `a` and `b` are geometry `FunctionCall`s with the **same** name,
///   the **same** arity, and the name is a built-in geometry function not
///   user-shadowed in `functions`, build a new `FunctionCall` with the same
///   name and recursively-merged args.
/// - Otherwise, emit a scalar `Conditional { cond, a, b }`.  Scalar args
///   and non-matching constructors both land here; the resulting `Conditional`
///   is lowered by `compile_expr` into `CompiledExprKind::Conditional`, which
///   `eval_expr` already evaluates by selecting the active branch.
///
/// **Else-if chain reduction:** before comparing, any branch that is itself
/// a `Conditional` is reduced by recursively calling `merge_branches` on its
/// inner `(condition, then, else)`.  This collapses
/// `box(p) else if c2 then box(q) else box(r)` → `box(C2, C2, C2)` so
/// the outer comparison can match it against `box(p)` and produce a single
/// `box` op with nested scalar `Conditional` args.
///
/// The scalar fallback always uses the **original** (un-reduced) `a` and `b`
/// as the `then_branch` / `else_branch` of the emitted scalar `Conditional` —
/// this keeps the scalar args as valid AST that `compile_expr` can evaluate
/// without encountering geometry function calls in a scalar position.
///
/// Spans for synthesized geometry `FunctionCall` nodes are taken from the
/// effective (possibly-reduced) `a`'s span to keep source locations
/// approximately correct.  Scalar `Conditional` nodes reuse `outer_span`
/// (the enclosing conditional's span) so diagnostic labels point at a
/// sensible location if the scalar arg itself later triggers a type error.
///
/// **Condition cloning and runtime cost:** `cond` is cloned into every scalar
/// `Conditional` leaf this function emits (one per differing scalar argument).
/// Consequently `eval_expr` re-evaluates the condition once per geometry argument
/// at run time, and `compile_expr` re-type-checks it once per clone at compile
/// time.  If the condition itself emits a diagnostic (e.g. an unresolved
/// identifier) the diagnostic may appear N times.  **Callers should ensure
/// conditions are pure and inexpensive** (a parameter comparison, not a function
/// call with side-effects).  Lifting the condition to a synthesised scalar `let`
/// binding for single evaluation is a v2 optimisation.
///
/// **Peephole — identical scalar leaves:** when `a` and `b` are structurally
/// identical scalar literals or Ident references the emitted `Conditional` would
/// be redundant (both branches evaluate to the same value).  [`are_scalar_equal`]
/// detects this case and returns `a.clone()` directly, keeping the compiled IR
/// lean (common for shared-constant axes in 3D ops such as `translate`).
///
/// **Geometry-arg mismatch fall-through:** when matching outer constructors
/// (e.g. two `translate(…)` calls) have a geometry sub-arg pair that differs by
/// constructor (e.g. `translate(box(…), …)` vs `translate(cyl(…), …)`), the
/// recursion produces `translate(Conditional{cond, box(…), cyl(…)}, …)`.  The
/// outer `translate` FunctionCall root passes
/// [`try_hoist_geometry_conditional`]'s check, so it IS returned as
/// `Some(merged)`.  When `compile_geometry_call` re-enters on the synthesised
/// `translate`, it encounters the inner `Conditional{cond, box, cyl}` as a
/// geometry arg; that re-enters `compile_geometry_call`,
/// `try_hoist_geometry_conditional` returns `None` (box vs cyl), and the
/// existing "if-then-else returning geometry" graceful error fires with a label
/// at `outer_span`.
fn merge_branches(
    cond: &reify_ast::Expr,
    a: &reify_ast::Expr,
    b: &reify_ast::Expr,
    functions: &[CompiledFunction],
    outer_span: reify_core::SourceSpan,
) -> reify_ast::Expr {
    // Else-if chain reduction: if a branch is itself a Conditional, reduce it
    // to a (potentially geometry-typed) expression so the outer match can
    // compare geometry constructors.
    let a_owned;
    let a_eff: &reify_ast::Expr =
        if let reify_ast::ExprKind::Conditional {
            condition: c2,
            then_branch: t2,
            else_branch: e2,
        } = &a.kind
        {
            a_owned = merge_branches(c2, t2, e2, functions, a.span);
            &a_owned
        } else {
            a
        };

    let b_owned;
    let b_eff: &reify_ast::Expr =
        if let reify_ast::ExprKind::Conditional {
            condition: c2,
            then_branch: t2,
            else_branch: e2,
        } = &b.kind
        {
            b_owned = merge_branches(c2, t2, e2, functions, b.span);
            &b_owned
        } else {
            b
        };

    if let (
        reify_ast::ExprKind::FunctionCall {
            name: name_a,
            args: args_a,
            ..
        },
        reify_ast::ExprKind::FunctionCall {
            name: name_b,
            args: args_b,
            ..
        },
    ) = (&a_eff.kind, &b_eff.kind)
        && name_a == name_b
        && args_a.len() == args_b.len()
        && is_geometry_function(name_a)
        && !functions.iter().any(|f| f.name == *name_a)
    {
        // Use args from the EFFECTIVE (reduced) forms so scalar args
        // from collapsed else-if chains are properly threaded.
        let merged_args: Vec<reify_ast::Expr> = args_a
            .iter()
            .zip(args_b.iter())
            .map(|(x, y)| merge_branches(cond, x, y, functions, outer_span))
            .collect();
        let n = merged_args.len();
        return reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: name_a.clone(),
                args: merged_args,
                arg_names: vec![None; n],
            },
            span: a_eff.span,
        };
    }
    // Scalar leaf, incompatible names/arities, or Ident branch — emit a
    // plain scalar Conditional using the ORIGINAL a and b so that compile_expr
    // receives well-typed scalar AST nodes without geometry-call subexpressions.

    // Peephole: if a and b are structurally identical scalar literals or Ident
    // references the Conditional would be redundant — both branches evaluate to
    // the same value regardless of the condition.  Return a.clone() directly to
    // keep the compiled IR lean (see doc-comment for rationale and examples).
    if are_scalar_equal(a, b) {
        return a.clone();
    }

    reify_ast::Expr {
        kind: reify_ast::ExprKind::Conditional {
            condition: Box::new(cond.clone()),
            then_branch: Box::new(a.clone()),
            else_branch: Box::new(b.clone()),
        },
        span: outer_span,
    }
}

// ─── Profile-precondition check (PRD task α, decision 5) ────────────────────
//
// The eight profile-consuming ops (extrude/extrude_symmetric/revolve/loft/
// loft_guided/sweep/sweep_guided/pipe) require their profile/path operands to
// have a particular dimensionality. We emit `GeometryProfileRequired` ONLY for a
// statically-known mismatch — an operand that compiled to a `FunctionCall` to a
// known geometry producer (so its `InferredTraits` are knowable without
// evaluation). Opaque `param`/`let` operands compile to `ValueRef` and are
// skipped, which is the load-bearing permissive back-compat pin.

/// One profile/path dimensionality precondition, bundling the three correlated
/// values a check needs: the user-facing slot `role` (shown as the offending
/// argument name), the human-readable `requirement` text, and the `violates`
/// predicate over inferred traits. Bundling them keeps a slot's role, message,
/// and predicate impossible to mismatch across the `check_profile_preconditions`
/// call sites.
struct ProfileSlot {
    /// Parameter-slot label shown to the user (e.g. `profile` / `path`) — the
    /// consumer's role for this argument, NOT the operand's producer function
    /// name (a `box(...)` passed as a profile is still reported as the
    /// `'profile'` slot, not as `'box'`).
    role: &'static str,
    /// Requirement wording, kept in sync with the canonical message documented
    /// on `DiagnosticCode::GeometryProfileRequired`.
    requirement: &'static str,
    /// `true` iff the operand's inferred traits violate this slot's precondition.
    violates: fn(&crate::geometry_traits_inference::InferredTraits) -> bool,
}

/// Surface-profile slot: extrude/extrude_symmetric/revolve/loft/loft_guided
/// profiles and the sweep/sweep_guided profile argument.
const PROFILE_SLOT: ProfileSlot = ProfileSlot {
    role: "profile",
    requirement: "a 2D Surface profile (Closed, Planar)",
    violates: violates_profile_requirement,
};

/// Curve-path slot: sweep/sweep_guided/pipe path arguments.
const PATH_SLOT: ProfileSlot = ProfileSlot {
    role: "path",
    requirement: "a 1D Curve path",
    violates: violates_path_requirement,
};

/// `true` iff `t` violates the Surface-profile precondition: a profile must be a
/// 2-D `Surface` that is both `Closed` and `Planar`.
///
/// For task α no producer is a `Surface` (closed/planar are `false` everywhere),
/// so the `dimension != Surface` clause already rejects every statically-known
/// task-α producer; the full `Surface ∧ Closed ∧ Planar` conjunction is wired
/// now so tasks ζ/η's Surface constructors (which set closed=planar=true) slot
/// in with no consumer change.
fn violates_profile_requirement(t: &crate::geometry_traits_inference::InferredTraits) -> bool {
    use crate::geometry_traits_inference::GeomDim;
    t.dimension != GeomDim::Surface || !t.closed || !t.planar
}

/// `true` iff `t` violates the Curve-path precondition: a sweep/pipe path must
/// be a 1-D `Curve`.
fn violates_path_requirement(t: &crate::geometry_traits_inference::InferredTraits) -> bool {
    use crate::geometry_traits_inference::GeomDim;
    t.dimension != GeomDim::Curve
}

/// Check the argument at `idx` against a dimensionality `slot`, emitting a
/// non-fatal `GeometryProfileRequired` diagnostic on a statically-known mismatch.
///
/// Permissive back-compat (PRD decision 5): only operands that are `FunctionCall`
/// CompiledExprs to a known geometry producer (resolved via
/// [`crate::geometry_traits_inference::try_infer_traits_for_function_call`], which
/// recurses through combinators) are inspected. `param`/`let` operands compile to
/// `ValueRef` and are skipped, so existing call sites whose profiles are opaque
/// bindings (e.g. `examples/sweep_degenerate.ri`) keep compiling. An unknown
/// producer name returns `None` and is likewise skipped.
///
/// The diagnostic reports `slot.role` (the consumer's parameter-slot name, e.g.
/// `profile`/`path`) rather than the operand's producer function name, and is
/// labelled at the offending operand's own span (`ast_args[idx].span`) — so for a
/// multi-profile `loft` each bad operand is pinpointed individually instead of
/// every diagnostic pointing at the whole call expression. `compiled_args` is
/// built 1:1 from `ast_args` in [`compile_geometry_call`], so the indices align.
fn check_profile_arg(
    compiled_args: &[CompiledExpr],
    ast_args: &[reify_ast::Expr],
    idx: usize,
    slot: &ProfileSlot,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(arg) = compiled_args.get(idx) else {
        return;
    };
    // `compiled_args[idx]` and `ast_args[idx]` are the same operand (built 1:1);
    // the AST node carries the operand's source span for a pinpointed label.
    let Some(ast_arg) = ast_args.get(idx) else {
        return;
    };
    let CompiledExprKind::FunctionCall { function, args } = &arg.kind else {
        return;
    };
    if let Some(traits) = crate::geometry_traits_inference::try_infer_traits_for_function_call(
        function.name.as_str(),
        args,
    ) && (slot.violates)(&traits)
    {
        crate::conformance::emit_geometry_profile_required(
            slot.role,
            slot.requirement,
            ast_arg.span,
            diagnostics,
        );
    }
}

/// Profile-precondition check for the eight profile-consuming geometry ops
/// (`extrude`, `extrude_symmetric`, `revolve`, `loft`, `loft_guided`, `sweep`,
/// `sweep_guided`, `pipe`) — PRD `docs/prds/geometry-primitive-constructors.md`
/// task α.
///
/// Called once immediately before the lowering `match name`, reading
/// `compiled_args` by reference so the arms' `into_iter` consumption is
/// undisturbed. Dispatch-by-name routes each slot to its requirement:
/// extrude/extrude_symmetric/revolve → arg0 is a Surface profile; loft → every
/// arg is a profile; loft_guided → all but the trailing guide are profiles;
/// sweep/sweep_guided → arg0 profile + arg1 Curve path; pipe → arg0 Curve path.
///
/// The check is **non-fatal**: it pushes diagnostics but the op is still lowered
/// (mirrors [`crate::conformance::emit_geometry_unbounded`]). Dispatch-by-name
/// with a `_ => {}` arm makes it a negligible-overhead no-op for non-consumers.
///
/// `revolve_full` is intentionally NOT wired — it is omitted from the task's
/// explicit consumer list and the PRD contract table; a conscious scope gap that
/// can be added additively later (it shares `SweepKind::Revolve`).
fn check_profile_preconditions(
    name: &str,
    compiled_args: &[CompiledExpr],
    ast_args: &[reify_ast::Expr],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // The positional slot→role mapping below MUST stay aligned with the lowering
    // `match name` arms further down in `compile_geometry_call`, which consume the
    // same positional `compiled_args`. Each arm names its companion lowering arm so
    // an argument-order change there is caught here; if you reorder a lowering arm's
    // positional args, update the matching arm below in lock-step.
    match name {
        // Lowering arms `"extrude"` / `"extrude_symmetric"` / `"revolve"`: arg0 is
        // the Surface profile (followed by distance, or by revolve's axis+angle).
        "extrude" | "extrude_symmetric" | "revolve" => {
            check_profile_arg(compiled_args, ast_args, 0, &PROFILE_SLOT, diagnostics);
        }
        // Lowering arms `"sweep"` / `"sweep_guided"`: arg0 is the Surface profile,
        // arg1 is the Curve path; sweep_guided's trailing guide (arg2) is unchecked.
        "sweep" | "sweep_guided" => {
            check_profile_arg(compiled_args, ast_args, 0, &PROFILE_SLOT, diagnostics);
            check_profile_arg(compiled_args, ast_args, 1, &PATH_SLOT, diagnostics);
        }
        // Lowering arm `"pipe"`: arg0 is the Curve path; arg1 is the radius (scalar).
        // Lowering arm `"zone_cylinder"`: arg0 is the Curve axis path; arg1 is the width (scalar).
        // Lowering arm `"zone_annulus"`: arg0 is the Curve axis path; args 1-3 are scalars.
        "pipe" | "zone_cylinder" | "zone_annulus" => {
            check_profile_arg(compiled_args, ast_args, 0, &PATH_SLOT, diagnostics);
        }
        // Lowering arm `"loft"`: every argument is a Surface profile.
        "loft" => {
            for idx in 0..compiled_args.len() {
                check_profile_arg(compiled_args, ast_args, idx, &PROFILE_SLOT, diagnostics);
            }
        }
        // Lowering arm `"loft_guided"`: args[0..len-1] are Surface profiles; the
        // trailing guide (last arg) is unchecked.
        "loft_guided" => {
            for idx in 0..compiled_args.len().saturating_sub(1) {
                check_profile_arg(compiled_args, ast_args, idx, &PROFILE_SLOT, diagnostics);
            }
        }
        // Not a profile-consumer (incl. revolve_full — out of scope). No-op.
        _ => {}
    }
}

/// Compile a geometry function call expression into CompiledGeometryOps.
///
/// Maps positional arguments to the named parameters expected by each primitive:
/// - `box(width, height, depth)`
/// - `cylinder(radius, height)`
/// - `sphere(radius)`
///
/// Boolean operations (union, intersection, difference) take nested geometry
/// call expressions as arguments. Each arg is recursively compiled into ops,
/// and GeomRef::Step indices are assigned globally using `step_offset` (the
/// index of the first op this call will emit in the flat step_handles array).
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_geometry_call(
    expr: &reify_ast::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    step_offset: usize,
    geometry_lets: &HashMap<&str, &reify_ast::Expr>,
    visiting: &mut HashSet<String>,
) -> Option<Vec<CompiledGeometryOp>> {
    // Resolve let-bound geometry variable references: when the expression is an
    // Ident that names a geometry let, recursively compile the initializer.
    // Guard against cycles (e.g. `let a = difference(b, x); let b = difference(a, y);`)
    // by tracking which names are currently being resolved.
    if let reify_ast::ExprKind::Ident(name) = &expr.kind {
        if let Some(init_expr) = geometry_lets.get(name.as_str()) {
            if !visiting.insert(name.clone()) {
                diagnostics.push(Diagnostic::error(format!(
                    "cyclic geometry let reference: '{}' references itself (directly or indirectly)",
                    name
                )));
                return None;
            }
            let result = compile_geometry_call(
                init_expr,
                scope,
                enum_defs,
                functions,
                diagnostics,
                step_offset,
                geometry_lets,
                visiting,
            );
            visiting.remove(name.as_str());
            return result;
        }
        return None;
    }

    // Task 3815: for a Conditional (if-then-else) whose branches are
    // structurally-identical geometry constructor trees, attempt scalar-arg
    // hoisting before falling through to the error.  `try_hoist_geometry_conditional`
    // returns `Some(merged_geometry_call)` when both branches share the same
    // geometry function name and arity; in that case we re-enter
    // `compile_geometry_call` on the synthesised merged expression — all existing
    // primitive/boolean/transform arms handle it transparently.
    //
    // Placed AFTER the Ident handling above (so transitive let-references remain
    // unaffected) and BEFORE the generic branching-error block below (so Match
    // remains rejected and the graceful-error fallback fires for incompatible
    // Conditional branches).
    if let Some(hoisted) = try_hoist_geometry_conditional(expr, functions) {
        return compile_geometry_call(
            &hoisted,
            scope,
            enum_defs,
            functions,
            diagnostics,
            step_offset,
            geometry_lets,
            visiting,
        );
    }

    // Tasks 3395, 3418: emit a clean compile-time Error for branching expressions
    // (Conditional, Match) that return a geometry value.  Prior to task 3395,
    // Conditional fell through the `_ => return None` arm below with no
    // diagnostic, leaving the caller's silent fallback to emit
    // `GeomRef::Step(0)` and produce the cryptic "unresolvable GeomRef::Step(0)"
    // runtime crash.  Task 3418 extends this to Match with a unified,
    // parameterised diagnostic.
    //
    // For Conditional, this block is only reached when `try_hoist_geometry_conditional`
    // returned `None` (incompatible branches: box vs cylinder, arity mismatch, or
    // Ident-let branch).
    //
    // Placed AFTER the Ident handling above (so transitive let-references
    // remain unaffected) and BEFORE the `(name, args)` extraction (so the
    // check fires regardless of whether the branching expr appears at the let's
    // root or as a sub-arg of another geometry call that recurses back here).
    let branching_kind_label = match &expr.kind {
        reify_ast::ExprKind::Conditional { .. } => Some("if-then-else"),
        reify_ast::ExprKind::Match { .. } => Some("match expression"),
        _ => None,
    };
    if let Some(kind) = branching_kind_label {
        // For Conditional, this block is only reached when
        // `try_hoist_geometry_conditional` returned `None` — i.e. the branches
        // are NOT structurally-identical geometry constructors (different name,
        // different arity, or an Ident-let reference).  Tailor the hint
        // accordingly so users know *why* auto-hoisting did not fire.
        let hint = if matches!(&expr.kind, reify_ast::ExprKind::Conditional { .. }) {
            "; automatic hoisting requires both branches to be the same \
             geometry constructor with the same arity (e.g. both `box(…)`) — \
             use structurally-identical constructors or select scalar arguments manually"
        } else {
            "; select scalar arguments first, then build the geometry \
             unconditionally outside the match expression"
        };
        diagnostics.push(
            Diagnostic::error(format!(
                "{kind} returning a geometry value cannot be used as a geometry \
                 expression{hint}",
            ))
            .with_label(DiagnosticLabel::new(
                expr.span,
                format!("geometry-typed {kind}"),
            )),
        );
        return None;
    }

    // task 3891: a bare `self.<sub>.<member>` cross-sub geometry reference has
    // no geometry-call syntax to lower directly.  Synthesize
    // `translate(<expr>, 0mm, 0mm, 0mm)` and re-enter — mirrors the
    // `try_hoist_geometry_conditional` re-entry pattern (line 425) and reuses
    // the existing translate lowering verbatim.  The translate arg pre-check
    // (line ~998) resolves arg[0] to `GeomRef::Sub("<sub>.<member>")`, producing
    // an identity-transform op byte-identical to the user-written wrapped form.
    if try_resolve_cross_sub_geom_ref(expr, scope).is_some() {
        let zero_mm = reify_ast::Expr {
            kind: reify_ast::ExprKind::QuantityLiteral {
                value: 0.0,
                unit: reify_ast::UnitExpr::Unit("mm".to_string()),
            },
            span: expr.span,
        };
        let synthetic = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "translate".to_string(),
                args: vec![expr.clone(), zero_mm.clone(), zero_mm.clone(), zero_mm],
                arg_names: vec![None; 4],
            },
            span: expr.span,
        };
        return compile_geometry_call(
            &synthetic,
            scope,
            enum_defs,
            functions,
            diagnostics,
            step_offset,
            geometry_lets,
            visiting,
        );
    }

    let (name, args) = match &expr.kind {
        reify_ast::ExprKind::FunctionCall { name, args, .. } => (name.as_str(), args),
        _ => return None,
    };

    // Boolean ops: args are nested geometry calls, NOT scalars.
    // Handle before scalar arg compilation below.
    match name {
        "union" | "intersection" | "difference" | "union_all" | "intersection_all" => {
            return compile_boolean_op(
                name,
                args,
                expr.span,
                scope,
                enum_defs,
                functions,
                diagnostics,
                step_offset,
                geometry_lets,
                visiting,
            );
        }
        _ => {}
    }

    // Generic geometry-arg resolution: for each arg index that is a geometry ref,
    // recursively compile the geometry expression, collect sub-ops, and record the
    // result step in geom_refs. Boolean ops are handled above and excluded here.
    // Short-circuit for primitives and curves (no geometry args) to avoid
    // unnecessary allocations on the hot path for the majority of calls.
    let static_indices = geometry_arg_indices(name);
    let needs_geom_resolution =
        name == "loft" || name == "loft_guided" || !static_indices.is_empty();

    let mut sub_ops: Vec<CompiledGeometryOp> = Vec::new();
    let mut geom_refs: HashMap<usize, GeomRef> = HashMap::new();
    let mut current_offset = step_offset;

    if needs_geom_resolution {
        let effective_indices: Vec<usize> = if name == "loft" || name == "loft_guided" {
            (0..args.len()).collect()
        } else {
            static_indices.to_vec()
        };
        for idx in &effective_indices {
            if *idx < args.len() {
                // Cross-sub geometry pre-check (task 3441): when the arg is
                // `self.<sub>.<member>` referring to a geometry realisation on a
                // non-collection sub's child template, lower it to a
                // GeomRef::Sub with compound key — no sub-op accumulation, no
                // step_offset perturbation for sibling args.  The eval side
                // populates the matching `named_steps["<sub>.<member>"]` entry
                // before processing this template's ops.
                if let Some(sub_ref) = try_resolve_cross_sub_geom_ref(&args[*idx], scope) {
                    geom_refs.insert(*idx, sub_ref);
                    continue;
                }
                // ── Two bare-Ident → GeomRef::Sub pre-checks, merged here
                // (task-3891 ⨉ task #4668).  Both lower a bare ident geometry
                // arg to a cross-realization GeomRef::Sub(<ident>) reference
                // instead of re-inlining its initializer as a fresh sub-op, and
                // both emit IDENTICAL output for any ident they BOTH match (a
                // top-level cross-sub alias is in geometry_lets AND
                // geometry_realization_names) — so their relative order is
                // immaterial.  Each independently covers a case the other does
                // not: 3891 catches nested/aliased cross-sub refs that live in
                // geometry_lets but are NOT top-level realizations; 4668 catches
                // same-structure top-level sibling realizations (e.g. a curated
                // fillet's `let b = box(...)` target) that are not cross-sub
                // aliases. ──
                //
                // task-3891: bare-alias ident arg — if the arg is an Ident
                // naming a bare cross-sub alias (one that is in geometry_lets
                // because collect_geometry_exprs added it), resolve it to
                // GeomRef::Sub(alias_name) so that the translate targets the
                // alias's own named realization step rather than re-evaluating
                // the cross-sub expression as an inline sub-op (which would
                // emit a duplicate identity-translate and produce the wrong
                // target chain).  The alias realization runs in declaration
                // order before any realization that consumes it, so
                // named_steps[alias_name] is populated by the time this op
                // executes.
                if let reify_ast::ExprKind::Ident(alias_name) = &args[*idx].kind
                    && let Some(alias_init) = geometry_lets.get(alias_name.as_str())
                    && is_bare_cross_sub_geometry_alias(alias_init, scope)
                {
                    geom_refs.insert(*idx, GeomRef::Sub(alias_name.clone()));
                    continue;
                }
                // Sibling-let pre-check (task #4668): when the arg is a bare Ident
                // naming a sibling top-level geometry realization in this structure,
                // emit GeomRef::Sub(name) — do NOT inline the initializer as a fresh
                // sub-op.  One let = one realization = one canonical named_steps entry;
                // curated selectors (edges_at_height, etc.) then belong to the same
                // solid instance at eval time.
                if let reify_ast::ExprKind::Ident(arg_name) = &args[*idx].kind
                    && scope.geometry_realization_names.contains(arg_name.as_str())
                {
                    geom_refs.insert(*idx, GeomRef::Sub(arg_name.clone()));
                    continue;
                }
                let diag_len_before = diagnostics.len();
                let inner_ops = compile_geometry_call(
                    &args[*idx],
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_offset,
                    geometry_lets,
                    visiting,
                );
                if let Some(ops) = inner_ops {
                    let result_step = current_offset + ops.len() - 1;
                    current_offset += ops.len();
                    geom_refs.insert(*idx, GeomRef::Step(result_step));
                    sub_ops.extend(ops);
                } else if diagnostics.len() > diag_len_before {
                    // A diagnostic was pushed during geometry-arg compilation
                    // (e.g. an incompatible if-then-else inside a transform arg).
                    // Propagate the failure so callers see None rather than ops
                    // built from a silent GeomRef::Step fallback.
                    return None;
                }
                // else: silent fallback — arg is not a geometry expression and no
                // diagnostic was pushed; existing behaviour for non-geometry args.
            }
        }
    }

    let compiled_args: Vec<CompiledExpr> = args
        .iter()
        .map(|arg| compile_expr(arg, scope, enum_defs, functions, diagnostics))
        .collect();

    // Silent fallback for single-geom-arg ops — see module-level note on silent-fallback vs. labelled-per-arg.
    let geom_ref = |idx: usize| -> GeomRef {
        geom_refs
            .get(&idx)
            .cloned()
            .unwrap_or(GeomRef::Step(step_offset))
    };

    // Profile-precondition check (PRD task α): emit GeometryProfileRequired for a
    // statically-known wrong-dimension operand at a profile/path consumer slot.
    // Non-fatal and read-only over compiled_args, so the lowering arms below
    // still consume compiled_args by value unchanged. `args` (the AST operands)
    // are passed alongside so each diagnostic is labelled at the offending
    // operand's own span rather than the whole call expression.
    check_profile_preconditions(name, &compiled_args, args, diagnostics);

    match name {
        // --- Primitives ---
        // box_centered(width, height, depth) is an op-identical alias for box:
        // make_box already centres the origin at the centroid
        // (occt_wrapper.cpp gp_Pnt corner(-w/2,-h/2,-d/2)).
        // `name` is forwarded to check_arg_count_exact so diagnostics report
        // "box" or "box_centered" as appropriate.
        "box" | "box_centered" => {
            if !check_arg_count_exact(name, compiled_args.len(), 3, expr.span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                    ("depth".to_string(), it.next().unwrap()),
                ],
            }])
        }
        "cylinder" => {
            if !check_arg_count_exact("cylinder", compiled_args.len(), 2, expr.span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Cylinder,
                args: vec![
                    ("radius".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // cylinder_centered(radius, height) — centred on the z-axis at z=0.
        //
        // The OCCT cylinder is base-at-origin (z ∈ [0, h]), so we compose a
        // Translate(dz = -height/2) to shift the centroid to z = 0.
        //
        // Two sub-ops are returned:
        //   [0] Primitive(Cylinder){radius, height}  — at step_offset
        //   [1] Transform(Translate){target=Step(step_offset), dx=0, dy=0, dz=-(height/2)}
        //
        // ops.last() (the Translate) is the realization root, matching the
        // "result_step = current_offset + ops.len() - 1" convention in the caller.
        "cylinder_centered" => {
            if !check_arg_count_exact(
                "cylinder_centered",
                compiled_args.len(),
                2,
                expr.span,
                diagnostics,
            ) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            let radius = it.next().unwrap();
            let height = it.next().unwrap();

            // dz = -(height / 2)  — same Length type so eval.as_f64() yields SI metres.
            let dz = CompiledExpr::binop(
                BinOp::Mul,
                height.clone(),
                CompiledExpr::literal(Value::Real(-0.5), reify_core::Type::dimensionless_scalar()),
                height.result_type.clone(),
            );
            let zero = CompiledExpr::literal(Value::Real(0.0), reify_core::Type::dimensionless_scalar());

            // Cylinder lands at step_offset (sub_ops was empty entering this arm).
            let cylinder_step = step_offset;

            Some(vec![
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Cylinder,
                    args: vec![
                        ("radius".to_string(), radius),
                        ("height".to_string(), height),
                    ],
                },
                CompiledGeometryOp::Transform {
                    kind: TransformKind::Translate,
                    target: GeomRef::Step(cylinder_step),
                    args: vec![
                        ("dx".to_string(), zero.clone()),
                        ("dy".to_string(), zero),
                        ("dz".to_string(), dz),
                    ],
                },
            ])
        }
        "sphere" => {
            if !check_arg_count_exact("sphere", compiled_args.len(), 1, expr.span, diagnostics) {
                return None;
            }
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Sphere,
                args: vec![(
                    "radius".to_string(),
                    compiled_args.into_iter().next().unwrap(),
                )],
            }])
        }
        "tube" => {
            if !check_arg_count_exact("tube", compiled_args.len(), 3, expr.span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Tube,
                args: vec![
                    ("outer_r".to_string(), it.next().unwrap()),
                    ("inner_r".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                ],
            }])
        }
        "cone" => {
            if !check_arg_count_exact("cone", compiled_args.len(), 3, expr.span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Cone,
                args: vec![
                    ("bottom_radius".to_string(), it.next().unwrap()),
                    ("top_radius".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                ],
            }])
        }
        "wedge" => {
            if !check_arg_count_exact("wedge", compiled_args.len(), 4, expr.span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Wedge,
                args: vec![
                    ("width".to_string(), it.next().unwrap()),
                    ("depth".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                    ("top_width".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // --- 2-D profile face constructors ---
        // rectangle(width, height) → CompiledGeometryOp::Profile(Rectangle)
        "rectangle" => {
            if !check_arg_count_exact("rectangle", compiled_args.len(), 2, expr.span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Profile {
                kind: ProfileKind::Rectangle,
                args: vec![
                    ("width".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // circle(radius) → CompiledGeometryOp::Profile(Circle)
        "circle" => {
            if !check_arg_count_exact("circle", compiled_args.len(), 1, expr.span, diagnostics) {
                return None;
            }
            Some(vec![CompiledGeometryOp::Profile {
                kind: ProfileKind::Circle,
                args: vec![
                    ("radius".to_string(), compiled_args.into_iter().next().unwrap()),
                ],
            }])
        }
        // polygon(x1,y1, x2,y2, ...) → CompiledGeometryOp::Profile(Polygon)
        // Variadic coordinate PAIRS: ≥6 args (3 pts min), must be multiple of 2.
        "polygon" => {
            let n = compiled_args.len();
            if n < 6 || !n.is_multiple_of(2) {
                push_labeled_arg_count_error(
                    format!(
                        "polygon() expects coordinate pairs (at least 6 args for 3 points), got {}",
                        n
                    ),
                    expr.span,
                    diagnostics,
                );
                return None;
            }
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, e)| (format!("c{}", i), e))
                .collect();
            Some(vec![CompiledGeometryOp::Profile {
                kind: ProfileKind::Polygon,
                args,
            }])
        }
        // ellipse(semi_major, semi_minor) → CompiledGeometryOp::Profile(Ellipse)
        "ellipse" => {
            if !check_arg_count_exact("ellipse", compiled_args.len(), 2, expr.span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Profile {
                kind: ProfileKind::Ellipse,
                args: vec![
                    ("semi_major".to_string(), it.next().unwrap()),
                    ("semi_minor".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // torus(major_radius, minor_radius) — mirrors cylinder's 2-arg lowering.
        "torus" => {
            if !check_arg_count_exact("torus", compiled_args.len(), 2, expr.span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Torus,
                args: vec![
                    ("major_radius".to_string(), it.next().unwrap()),
                    ("minor_radius".to_string(), it.next().unwrap()),
                ],
            }])
        }

        // --- Patterns ---
        // linear_pattern(target, dx, dy, dz, count, spacing)
        "linear_pattern" => {
            if !check_arg_count_exact(
                "linear_pattern",
                compiled_args.len(),
                6,
                expr.span,
                diagnostics,
            ) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            let target = geom_ref(0);
            let op = CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear,
                target,
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("dx".to_string(), it.next().unwrap()),
                    ("dy".to_string(), it.next().unwrap()),
                    ("dz".to_string(), it.next().unwrap()),
                    ("count".to_string(), it.next().unwrap()),
                    ("spacing".to_string(), it.next().unwrap()),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // circular_pattern(target, ox, oy, oz, ax, ay, az, count, angle)
        // circular_pattern(target, ox, oy, oz, ax, ay, az, count, angle) — scalar form (9 args)
        // circular_pattern(target, axis_value, count, angle)              — value form  (4 args)
        "circular_pattern" => {
            let n = compiled_args.len();
            if n == 9 {
                let mut it = compiled_args.into_iter();
                let target = geom_ref(0);
                let op = CompiledGeometryOp::Pattern {
                    kind: PatternKind::Circular,
                    target,
                    args: vec![
                        ("target".to_string(), it.next().unwrap()),
                        ("ox".to_string(), it.next().unwrap()),
                        ("oy".to_string(), it.next().unwrap()),
                        ("oz".to_string(), it.next().unwrap()),
                        ("ax".to_string(), it.next().unwrap()),
                        ("ay".to_string(), it.next().unwrap()),
                        ("az".to_string(), it.next().unwrap()),
                        ("count".to_string(), it.next().unwrap()),
                        ("angle".to_string(), it.next().unwrap()),
                    ],
                };
                sub_ops.push(op);
                Some(sub_ops)
            } else if n == 4 {
                let mut it = compiled_args.into_iter();
                let target = geom_ref(0);
                let op = CompiledGeometryOp::Pattern {
                    kind: PatternKind::Circular,
                    target,
                    args: vec![
                        ("target".to_string(), it.next().unwrap()),
                        ("axis".to_string(), it.next().unwrap()),
                        ("count".to_string(), it.next().unwrap()),
                        ("angle".to_string(), it.next().unwrap()),
                    ],
                };
                sub_ops.push(op);
                Some(sub_ops)
            } else {
                push_labeled_arg_count_error(
                    format!("circular_pattern() expects 9 arguments (scalar) or 4 (axis-value), got {n}"),
                    expr.span,
                    diagnostics,
                );
                None
            }
        }
        // mirror(target, ox, oy, oz, nx, ny, nz) — scalar form (7 args)
        // mirror(target, plane_value)            — value form  (2 args)
        "mirror" => {
            let n = compiled_args.len();
            if n == 7 {
                let mut it = compiled_args.into_iter();
                let target = geom_ref(0);
                let op = CompiledGeometryOp::Pattern {
                    kind: PatternKind::Mirror,
                    target,
                    args: vec![
                        ("target".to_string(), it.next().unwrap()),
                        ("ox".to_string(), it.next().unwrap()),
                        ("oy".to_string(), it.next().unwrap()),
                        ("oz".to_string(), it.next().unwrap()),
                        ("nx".to_string(), it.next().unwrap()),
                        ("ny".to_string(), it.next().unwrap()),
                        ("nz".to_string(), it.next().unwrap()),
                    ],
                };
                sub_ops.push(op);
                Some(sub_ops)
            } else if n == 2 {
                let mut it = compiled_args.into_iter();
                let target = geom_ref(0);
                let op = CompiledGeometryOp::Pattern {
                    kind: PatternKind::Mirror,
                    target,
                    args: vec![
                        ("target".to_string(), it.next().unwrap()),
                        ("plane".to_string(), it.next().unwrap()),
                    ],
                };
                sub_ops.push(op);
                Some(sub_ops)
            } else {
                push_labeled_arg_count_error(
                    format!("mirror() expects 7 arguments (scalar) or 2 (plane-value), got {n}"),
                    expr.span,
                    diagnostics,
                );
                None
            }
        }
        // linear_pattern_2d(target, dx1, dy1, dz1, count1, spacing1, dx2, dy2, dz2, count2, spacing2)
        "linear_pattern_2d" => {
            if !check_arg_count_exact(
                "linear_pattern_2d",
                compiled_args.len(),
                11,
                expr.span,
                diagnostics,
            ) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear2D,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("dx1".to_string(), it.next().unwrap()),
                    ("dy1".to_string(), it.next().unwrap()),
                    ("dz1".to_string(), it.next().unwrap()),
                    ("count1".to_string(), it.next().unwrap()),
                    ("spacing1".to_string(), it.next().unwrap()),
                    ("dx2".to_string(), it.next().unwrap()),
                    ("dy2".to_string(), it.next().unwrap()),
                    ("dz2".to_string(), it.next().unwrap()),
                    ("count2".to_string(), it.next().unwrap()),
                    ("spacing2".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // arbitrary_pattern(target, dx1, dy1, dz1, dx2, dy2, dz2, ...)
        "arbitrary_pattern" => {
            if compiled_args.len() < 4 || !(compiled_args.len() - 1).is_multiple_of(3) {
                diagnostics.push(Diagnostic::error(format!(
                    "arbitrary_pattern() expects target + N*(dx,dy,dz) triples (>= 4 args, (len-1) % 3 == 0), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            let mut args = vec![("target".to_string(), it.next().unwrap())];
            let coords: Vec<_> = it.collect();
            for (idx, chunk) in coords.chunks_exact(3).enumerate() {
                args.push((format!("t{}_dx", idx), chunk[0].clone()));
                args.push((format!("t{}_dy", idx), chunk[1].clone()));
                args.push((format!("t{}_dz", idx), chunk[2].clone()));
            }
            Some(vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Arbitrary,
                target: GeomRef::Step(0),
                args,
            }])
        }
        // --- Sweeps ---
        // loft(profile1, profile2, ...)
        "loft" => {
            if compiled_args.len() < 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "loft() expects at least 2 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            // Silent fallback per profile slot — see module-level note on silent-fallback vs. labelled-per-arg.
            let (profiles, loft_args) =
                resolve_loft_like_args(compiled_args, &geom_refs, step_offset, false);
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Loft,
                profiles,
                args: loft_args,
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // loft_guided(profile_1, profile_2, ..., guide) — pipe-shell loft
        // with a trailing guide wire. Variadic profiles (>= 2) + 1 guide,
        // so total args.len() must be >= 3.
        "loft_guided" => {
            if compiled_args.len() < 3 {
                diagnostics.push(Diagnostic::error(format!(
                    "loft_guided() expects at least 3 arguments \
                     (profile_1, profile_2, ..., guide), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            // Silent fallback per profile slot — see module-level note on silent-fallback vs. labelled-per-arg.
            // Convention: last arg is the guide wire; preceding args are profiles.
            let (refs, loft_guided_args) =
                resolve_loft_like_args(compiled_args, &geom_refs, step_offset, true);
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::LoftGuided,
                profiles: refs,
                args: loft_guided_args,
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // extrude(profile, distance)
        "extrude" => {
            if compiled_args.len() != 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "extrude() expects exactly 2 arguments (profile, distance), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            let profile_expr = it.next().unwrap();
            let distance_expr = it.next().unwrap();
            let profile = geom_ref(0);
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Extrude,
                profiles: vec![profile],
                args: vec![
                    ("profile".to_string(), profile_expr),
                    ("distance".to_string(), distance_expr),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // extrude_symmetric(profile, distance) — extrudes distance/2 each way
        "extrude_symmetric" => {
            if compiled_args.len() != 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "extrude_symmetric() expects exactly 2 arguments (profile, distance), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            let profile_expr = it.next().unwrap();
            let distance_expr = it.next().unwrap();
            let profile = geom_ref(0);
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::ExtrudeSymmetric,
                profiles: vec![profile],
                args: vec![
                    ("profile".to_string(), profile_expr),
                    ("distance".to_string(), distance_expr),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // revolve(profile, ox, oy, oz, ax, ay, az, angle)
        "revolve" => {
            if compiled_args.len() != 8 {
                diagnostics.push(Diagnostic::error(format!(
                    "revolve() expects exactly 8 arguments (profile, ox, oy, oz, ax, ay, az, angle), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            let profile_expr = it.next().unwrap();
            let ox = it.next().unwrap();
            let oy = it.next().unwrap();
            let oz = it.next().unwrap();
            let ax = it.next().unwrap();
            let ay = it.next().unwrap();
            let az = it.next().unwrap();
            let angle = it.next().unwrap();
            let profile = geom_ref(0);
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Revolve,
                profiles: vec![profile],
                args: vec![
                    ("profile".to_string(), profile_expr),
                    ("ox".to_string(), ox),
                    ("oy".to_string(), oy),
                    ("oz".to_string(), oz),
                    ("ax".to_string(), ax),
                    ("ay".to_string(), ay),
                    ("az".to_string(), az),
                    ("angle".to_string(), angle),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // revolve_full(profile, ox, oy, oz, ax, ay, az) — injects 2π for angle
        "revolve_full" => {
            if compiled_args.len() != 7 {
                diagnostics.push(Diagnostic::error(format!(
                    "revolve_full() expects exactly 7 arguments (profile, ox, oy, oz, ax, ay, az), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            let profile_expr = it.next().unwrap();
            let ox = it.next().unwrap();
            let oy = it.next().unwrap();
            let oz = it.next().unwrap();
            let ax = it.next().unwrap();
            let ay = it.next().unwrap();
            let az = it.next().unwrap();
            // Inject literal 2π for the angle
            let tau_expr =
                CompiledExpr::literal(Value::Real(std::f64::consts::TAU), reify_core::Type::dimensionless_scalar());
            let profile = geom_ref(0);
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Revolve,
                profiles: vec![profile],
                args: vec![
                    ("profile".to_string(), profile_expr),
                    ("ox".to_string(), ox),
                    ("oy".to_string(), oy),
                    ("oz".to_string(), oz),
                    ("ax".to_string(), ax),
                    ("ay".to_string(), ay),
                    ("az".to_string(), az),
                    ("angle".to_string(), tau_expr),
                ],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // sweep(profile, path)
        "sweep" => {
            if compiled_args.len() != 2 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "sweep() expects exactly 2 arguments (profile, path), got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
                return None;
            }
            // Labelled per-arg diagnostic — see module-level note on silent-fallback vs. labelled-per-arg.
            let profile = resolve_named_geom_arg(
                0,
                "sweep",
                "profile",
                args,
                &geom_refs,
                diagnostics,
                step_offset,
            );
            let path = resolve_named_geom_arg(
                1,
                "sweep",
                "path",
                args,
                &geom_refs,
                diagnostics,
                step_offset,
            );
            // SweepKind::Sweep carries all geometry data in `profiles`;
            // `args` is intentionally empty (task-383 S6).
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Sweep,
                profiles: vec![profile, path],
                args: vec![],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // sweep_guided(profile, path, guide) — pipe-shell sweep with an
        // auxiliary guide wire constraining orientation.
        "sweep_guided" => {
            if compiled_args.len() != 3 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "sweep_guided() expects exactly 3 arguments (profile, path, guide), got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
                return None;
            }
            // Labelled per-arg diagnostic — see module-level note on silent-fallback vs. labelled-per-arg.
            let profile = resolve_named_geom_arg(
                0,
                "sweep_guided",
                "profile",
                args,
                &geom_refs,
                diagnostics,
                step_offset,
            );
            let path = resolve_named_geom_arg(
                1,
                "sweep_guided",
                "path",
                args,
                &geom_refs,
                diagnostics,
                step_offset,
            );
            let guide = resolve_named_geom_arg(
                2,
                "sweep_guided",
                "guide",
                args,
                &geom_refs,
                diagnostics,
                step_offset,
            );
            // SweepKind::SweepGuided carries all geometry data in `profiles`;
            // `args` is intentionally empty (task-2122, following task-383 S6).
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::SweepGuided,
                profiles: vec![profile, path, guide],
                args: vec![],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // pipe(path, radius) — circular cross-section sweep along a path
        "pipe" => {
            if compiled_args.len() != 2 {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "pipe() expects exactly 2 arguments (path, radius), got {}",
                        compiled_args.len()
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "wrong number of arguments")),
                );
                return None;
            }
            // Labelled per-arg diagnostic — path is the only geom arg; radius is scalar.
            let path_ref = resolve_named_geom_arg(
                0,
                "pipe",
                "path",
                args,
                &geom_refs,
                diagnostics,
                step_offset,
            );
            // SweepKind::Pipe: the path is resolved through `profiles[0]` (GeomRef);
            // only the scalar "radius" belongs in args (task-383 S6).
            // Use nth(1) to skip the first (path) expression cleanly.
            let radius_expr = compiled_args.into_iter().nth(1).unwrap();
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Pipe,
                profiles: vec![path_ref],
                args: vec![("radius".to_string(), radius_expr)],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // zone_cylinder(axis: Geometry, width: Length) — GD&T Ø-zone cylinder.
        //
        // Lowers to Sweep{kind:Pipe} along the axis wire with radius = width/2.
        // `width` is the DIAMETER of the tolerance zone, so radius = width * 0.5.
        // The axis wire's length defines the cylinder length (no separate length arg).
        //
        // Sub-ops emitted (step_offset N):
        //   [N+0] Curve(LineSegment/…) — the axis wire from geom_ref(0)
        //   [N+1] Sweep{Pipe, profiles:[Step(N)], args:[("radius", width*0.5)]}
        "zone_cylinder" => {
            if !check_arg_count_exact(
                "zone_cylinder",
                compiled_args.len(),
                2,
                expr.span,
                diagnostics,
            ) {
                return None;
            }
            let path_ref = resolve_named_geom_arg(
                0,
                "zone_cylinder",
                "axis",
                args,
                &geom_refs,
                diagnostics,
                step_offset,
            );
            // width is the Ø-zone DIAMETER; radius = width * 0.5
            let width = compiled_args.into_iter().nth(1).unwrap();
            let radius = CompiledExpr::binop(
                BinOp::Mul,
                width.clone(),
                CompiledExpr::literal(
                    Value::Real(0.5),
                    reify_core::Type::dimensionless_scalar(),
                ),
                width.result_type.clone(),
            );
            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Pipe,
                profiles: vec![path_ref],
                args: vec![("radius".to_string(), radius)],
            };
            sub_ops.push(op);
            Some(sub_ops)
        }
        // zone_annulus(axis: Geometry, nominal_radius: Length, width: Length, length: Length)
        //   — GD&T annular tolerance zone (hollow cylinder shell).
        //
        // Lowers to:
        //   [N+0] Curve(…) — axis wire from geom_ref(0)
        //   [N+1] Sweep{Pipe, radius=R+w/2} — outer cylinder
        //   [N+2] Sweep{Pipe, radius=R-w/2} — inner cylinder
        //   [N+3] Boolean{Difference, left:Step(N+1), right:Step(N+2)} — annular shell
        //
        // `length` (arg 3) is accepted/validated per the C6 signature but the swept
        // extent comes from the axis wire (ratified L2 esc-4476-88 Option A).
        // V = 2π * R * w * L.
        "zone_annulus" => {
            if !check_arg_count_exact(
                "zone_annulus",
                compiled_args.len(),
                4,
                expr.span,
                diagnostics,
            ) {
                return None;
            }
            let path_ref = resolve_named_geom_arg(
                0,
                "zone_annulus",
                "axis",
                args,
                &geom_refs,
                diagnostics,
                step_offset,
            );
            let mut iter = compiled_args.into_iter();
            let _axis = iter.next().unwrap(); // arg0: axis (geometry; path_ref handles geometry side)
            let r = iter.next().unwrap();     // arg1: nominal_radius (Length)
            let w = iter.next().unwrap();     // arg2: zone width (Length)
            let _l = iter.next().unwrap();    // arg3: zone length (validated; axis wire supplies extent)

            // half_w = w * 0.5  (same Length dimension)
            let half_w = CompiledExpr::binop(
                BinOp::Mul,
                w.clone(),
                CompiledExpr::literal(Value::Real(0.5), reify_core::Type::dimensionless_scalar()),
                w.result_type.clone(),
            );
            // outer_r = R + w/2, inner_r = R - w/2
            let outer_r = CompiledExpr::binop(
                BinOp::Add,
                r.clone(),
                half_w.clone(),
                r.result_type.clone(),
            );
            let inner_r = CompiledExpr::binop(
                BinOp::Sub,
                r.clone(),
                half_w,
                r.result_type.clone(),
            );

            // Track absolute step indices for the Boolean Difference.
            let outer_step = step_offset + sub_ops.len(); // step after axis wire
            sub_ops.push(CompiledGeometryOp::Sweep {
                kind: SweepKind::Pipe,
                profiles: vec![path_ref.clone()],
                args: vec![("radius".to_string(), outer_r)],
            });
            let inner_step = step_offset + sub_ops.len(); // step after outer pipe
            sub_ops.push(CompiledGeometryOp::Sweep {
                kind: SweepKind::Pipe,
                profiles: vec![path_ref],
                args: vec![("radius".to_string(), inner_r)],
            });
            sub_ops.push(CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                left: GeomRef::Step(outer_step),
                right: GeomRef::Step(inner_step),
            });
            Some(sub_ops)
        }
        // zone_profile(solid: Geometry, width: Length) — GD&T profile tolerance zone.
        //
        // Lowers to:
        //   [N+0] <solid ops> — resolved geometry from geom_ref(0)
        //   [N+k] Modify{Thicken, target:Step(N+k-1), offset=+w/2}  — outer shell boundary
        //   [N+k+1] Modify{Thicken, target:Step(N+k-1), offset=−w/2} — inner shell boundary
        //   [N+k+2] Boolean{Difference, left:Step(N+k), right:Step(N+k+1)}
        //
        // Both Thicken ops target the SAME solid (the last solid op in sub_ops before pushing).
        // No closed-form volume: V ≈ surface_area × width; realized via OCCT Thicken.
        "zone_profile" => {
            if !check_arg_count_exact(
                "zone_profile",
                compiled_args.len(),
                2,
                expr.span,
                diagnostics,
            ) {
                return None;
            }
            let mut iter = compiled_args.into_iter();
            let solid_expr = iter.next().unwrap(); // arg0: solid (geometry; also compiled as scalar)
            let w = iter.next().unwrap();           // arg1: zone width (Length)

            let solid_target = geom_ref(0); // the input solid

            // plus_offset = +w/2 = w * 0.5
            let plus_offset = CompiledExpr::binop(
                BinOp::Mul,
                w.clone(),
                CompiledExpr::literal(Value::Real(0.5), reify_core::Type::dimensionless_scalar()),
                w.result_type.clone(),
            );
            // minus_offset = -w/2 = w * (-0.5)
            let minus_offset = CompiledExpr::binop(
                BinOp::Mul,
                w.clone(),
                CompiledExpr::literal(Value::Real(-0.5), reify_core::Type::dimensionless_scalar()),
                w.result_type.clone(),
            );

            // Track absolute step indices for the Boolean Difference.
            let plus_step = step_offset + sub_ops.len(); // step of plus-thicken op
            sub_ops.push(CompiledGeometryOp::Modify {
                kind: ModifyKind::Thicken,
                target: solid_target.clone(),
                args: vec![
                    ("target".to_string(), solid_expr.clone()),
                    ("offset".to_string(), plus_offset),
                ],
            });
            let minus_step = step_offset + sub_ops.len(); // step of minus-thicken op
            sub_ops.push(CompiledGeometryOp::Modify {
                kind: ModifyKind::Thicken,
                target: solid_target,
                args: vec![
                    ("target".to_string(), solid_expr),
                    ("offset".to_string(), minus_offset),
                ],
            });
            sub_ops.push(CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                left: GeomRef::Step(plus_step),
                right: GeomRef::Step(minus_step),
            });
            Some(sub_ops)
        }
        // --- Transforms ---
        "translate" | "rotate" | "scale" | "rotate_around" | "apply_transform" => compile_transform_op(
            name,
            compiled_args,
            geom_ref(0),
            expr.span,
            diagnostics,
            sub_ops,
        ),
        // --- Modify extensions ---
        // These modifiers take a geometry target as their first argument (correctly
        // resolved from geom_refs via geom_ref(0)) and are registered in geometry_arg_indices().
        "shell" | "shell_open" | "thicken" | "offset_solid" | "offset_curve" | "draft"
        | "chamfer" | "chamfer_asymmetric" | "fillet" | "fillet_all" | "zone_slab" => {
            compile_modify_op(
                name,
                compiled_args,
                geom_ref(0),
                expr.span,
                diagnostics,
                sub_ops,
            )
        }
        // --- Curve constructors ---
        "line_segment" | "arc" | "helix" | "interp" | "bezier" | "nurbs" => {
            compile_curve_op(name, compiled_args, expr.span, diagnostics, sub_ops)
        }
        // nurbs_surface(control_points, weights, u_knots, v_knots, u_degree, v_degree)
        // → CompiledGeometryOp::Surface { kind: SurfaceKind::Nurbs, args }
        "nurbs_surface" => {
            if !check_arg_count_exact("nurbs_surface", compiled_args.len(), 6, expr.span, diagnostics) {
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Surface {
                kind: SurfaceKind::Nurbs,
                args: vec![
                    ("control_points".to_string(), it.next().unwrap()),
                    ("weights".to_string(), it.next().unwrap()),
                    ("u_knots".to_string(), it.next().unwrap()),
                    ("v_knots".to_string(), it.next().unwrap()),
                    ("u_degree".to_string(), it.next().unwrap()),
                    ("v_degree".to_string(), it.next().unwrap()),
                ],
            }])
        }
        _ => {
            diagnostics.push(Diagnostic::error(unsupported_geometry_fn_message(name)));
            None
        }
    }
}

/// Detect if a constraint expression matches the count constraint pattern:
///   `collection_name.count == expr`  or  `expr == collection_name.count`
/// Returns `(collection_name, count_expr)` where count_expr is the non-.count side.
pub(crate) fn extract_count_constraint<'a>(
    expr: &'a reify_ast::Expr,
    collection_sub_names: &HashSet<String>,
) -> Option<(String, &'a reify_ast::Expr)> {
    if let reify_ast::ExprKind::BinOp { op, left, right } = &expr.kind {
        if op != "==" {
            return None;
        }
        // Check LHS: collection_name.count == expr
        if let Some(name) = extract_collection_count(left, collection_sub_names) {
            return Some((name, right));
        }
        // Check RHS: expr == collection_name.count
        if let Some(name) = extract_collection_count(right, collection_sub_names) {
            return Some((name, left));
        }
    }
    None
}

/// Check if an expression is `collection_name.count` for a known collection sub.
pub(crate) fn extract_collection_count(
    expr: &reify_ast::Expr,
    collection_sub_names: &HashSet<String>,
) -> Option<String> {
    if let reify_ast::ExprKind::MemberAccess { object, member } = &expr.kind
        && member == "count"
        && let reify_ast::ExprKind::Ident(name) = &object.kind
        && collection_sub_names.contains(name.as_str())
    {
        return Some(name.clone());
    }
    None
}

/// Prefix of the diagnostic emitted by the wildcard arm in `compile_geometry_call`
/// when a function name is not recognised.  Declared here (not inside `#[cfg(test)]`)
/// so the production wildcard arm and the registry-cross-check test can share the
/// same string — if the wording ever changes, both sites update together.
pub(crate) const UNSUPPORTED_GEOMETRY_FN_MSG: &str = "unsupported geometry function";

/// Full diagnostic message emitted by the wildcard arm in `compile_geometry_call`
/// for an unrecognised geometry function `name`.  Both the production wildcard arm
/// and the registry-cross-check test call this function, so a formatting change
/// only needs to be made in one place and cannot silently diverge between the two
/// sites.
pub(crate) fn unsupported_geometry_fn_message(name: &str) -> String {
    format!("{}: {}", UNSUPPORTED_GEOMETRY_FN_MSG, name)
}

// ─── Registry cross-check (task-1733) ────────────────────────────────────────
//
// The test below cross-checks the set of function names handled in
// `geometry_arg_indices` against the names dispatched in `compile_geometry_call`.
// When a new geometry function is added to the dispatch block, it must also be
// added to one of the lists below, ensuring `geometry_arg_indices` is kept in
// sync and geometry-arg resolution is not silently broken.

// ─── Feature-tag derivation (task 2323) ──────────────────────────────────────

/// Derive a parallel `Vec<FeatureTag>` for the given op stream.
///
/// Each tag carries the enclosing realization's `span`, the coarse `StepKind`
/// classification of the op, and a zero-based `sub_index`.
///
/// The `match` is exhaustive over all `CompiledGeometryOp` variants so that
/// adding a new variant forces a compile error here, keeping the mapping
/// up-to-date (single source of truth, similar to `ModifyKind::ALL`).
pub fn derive_feature_tags(
    ops: &[CompiledGeometryOp],
    span: reify_core::SourceSpan,
) -> Vec<reify_ir::FeatureTag> {
    let tags: Vec<_> = ops
        .iter()
        .enumerate()
        .map(|(i, op)| {
            let step_kind = match op {
                CompiledGeometryOp::Primitive { .. } => reify_ir::StepKind::Primitive,
                CompiledGeometryOp::Boolean { .. } => reify_ir::StepKind::Boolean,
                CompiledGeometryOp::Modify { .. } => reify_ir::StepKind::Modify,
                CompiledGeometryOp::Transform { .. } => reify_ir::StepKind::Transform,
                CompiledGeometryOp::Pattern { .. } => reify_ir::StepKind::Pattern,
                CompiledGeometryOp::Sweep { .. } => reify_ir::StepKind::Sweep,
                CompiledGeometryOp::Curve { .. } => reify_ir::StepKind::Curve,
                CompiledGeometryOp::Profile { .. } => reify_ir::StepKind::Profile,
                CompiledGeometryOp::Surface { .. } => reify_ir::StepKind::Surface,
            };
            reify_ir::FeatureTag {
                source_span: span,
                step_kind,
                sub_index: i as u32,
            }
        })
        .collect();
    // No debug_assert needed: `tags` is constructed by `.map(...).collect()` over
    // `ops.iter()`, so it is structurally impossible for the lengths to diverge.
    // The meaningful invariant — that the caller passes a feature_tags slice of the
    // same length as operations when invoking execute_realization_ops — is enforced
    // at the call site in engine_build.rs.
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every non-boolean, non-loft function dispatched in `compile_geometry_call`
    /// that takes at least one geometry arg (first arg is target/profile/etc.).
    /// These MUST return non-empty from `geometry_arg_indices`.
    const GEOM_ARG_FUNCTIONS: &[&str] = &[
        "translate",
        "rotate",
        "scale",
        "rotate_around",
        "apply_transform",
        "circular_pattern",
        "linear_pattern",
        "mirror",
        "extrude",
        "extrude_symmetric",
        "revolve",
        "revolve_full",
        "shell",
        "shell_open",
        "thicken",
        "offset_solid",
        "offset_curve",
        "draft",
        "chamfer",
        "chamfer_asymmetric",
        "fillet",
        "fillet_all",
        "zone_slab",
        "zone_cylinder",
        "zone_annulus",
        "zone_profile",
        "sweep",
        "sweep_guided",
        "pipe",
    ];

    /// Every non-boolean function dispatched in `compile_geometry_call` that has
    /// NO geometry args (primitives, curves, patterns that don't use geom_ref).
    /// These MUST return empty from `geometry_arg_indices`.
    const NO_GEOM_ARG_FUNCTIONS: &[&str] = &[
        "box",
        "box_centered",
        "cylinder",
        "cylinder_centered",
        "sphere",
        "tube",
        "cone",
        "wedge",
        "torus",
        "linear_pattern_2d",
        "arbitrary_pattern",
        "line_segment",
        "arc",
        "helix",
        "interp",
        "bezier",
        "nurbs",
        "rectangle",
        "circle",
        "polygon",
        "ellipse",
        "nurbs_surface",
    ];

    /// Boolean set-operation functions — handled by the early-return path to
    /// `compile_boolean_op` in `compile_geometry_call` before the main dispatch
    /// match.  `geometry_arg_indices` is never consulted for these.
    const BOOLEAN_OP_FUNCTIONS: &[&str] = &[
        "union",
        "intersection",
        "difference",
        "union_all",
        "intersection_all",
    ];

    /// Variadic solid-construction function handled via a dedicated arm in the
    /// main dispatch match.  `geometry_arg_indices` returns empty for loft
    /// (verified by `geometry_arg_indices_loft_is_empty_handled_specially`).
    const LOFT_FUNCTIONS: &[&str] = &["loft", "loft_guided"];

    /// Canary pin: the total number of distinct function names dispatched by
    /// `compile_geometry_call`, spread across the four category lists.
    ///
    /// Breakdown at time of writing:
    /// ```text
    /// GEOM_ARG_FUNCTIONS    28  (offset_solid, offset_curve, fillet_all, zone_slab,
    ///                            apply_transform, zone_cylinder, zone_annulus,
    ///                            zone_profile, chamfer_asymmetric)
    /// NO_GEOM_ARG_FUNCTIONS 22  (rectangle, circle, polygon, ellipse 2-D faces; torus;
    ///                            nurbs_surface task #4191)
    /// boolean ops            5
    /// loft-variadic          2  (loft, loft_guided)
    /// Total                 58
    /// ```
    ///
    /// **Maintenance rule:** whenever a new arm is added to `compile_geometry_call`,
    ///   1. Add the function name to exactly one of the four lists in
    ///      `all_dispatch_functions_accounted_for`.
    ///   2. Increment this constant.
    ///   3. Confirm the `assert_eq!` in `all_dispatch_functions_accounted_for` still passes.
    ///
    /// The constant is declared separately from the lists so any mutation of the lists
    /// that omits the corresponding increment will trip the assertion, prompting a
    /// conscious audit.
    // 54 (base) + offset_curve + chamfer_asymmetric (main) + shell_open (task/4187)
    // + nurbs_surface (task #4191, η) = 58
    const EXPECTED_DISPATCH_COUNT: usize = 58;

    #[test]
    fn geometry_arg_indices_covers_all_geom_arg_functions() {
        for &name in GEOM_ARG_FUNCTIONS {
            assert!(
                !geometry_arg_indices(name).is_empty(),
                "geometry_arg_indices(\"{}\") returned empty — \
                 this function takes geometry args but is not registered in the index",
                name
            );
        }
    }

    #[test]
    fn geometry_arg_indices_empty_for_no_geom_arg_functions() {
        for &name in NO_GEOM_ARG_FUNCTIONS {
            assert!(
                geometry_arg_indices(name).is_empty(),
                "geometry_arg_indices(\"{}\") returned non-empty — \
                 this function should not have geometry args registered",
                name
            );
        }
    }

    #[test]
    fn geometry_arg_indices_loft_is_empty_handled_specially() {
        // loft and loft_guided are variadic — handled with special logic in
        // compile_geometry_call, not via geometry_arg_indices. Verify they
        // return empty (the wildcard arm) so we know the special path is the
        // only handler.
        for &name in LOFT_FUNCTIONS {
            assert!(
                geometry_arg_indices(name).is_empty(),
                "{} should not be in geometry_arg_indices — it uses variadic handling",
                name
            );
        }
    }

    /// `offset_curve` (ι, task 4193) takes its curve target at position 0 and its
    /// scalar distance (+ optional reference/direction) as plain value args, so
    /// only index 0 is a geometry ref — exactly like the `thicken`/`shell` modify
    /// family. RED until step-10 adds the `"offset_curve" => &[0]` arm to
    /// `geometry_arg_indices`.
    #[test]
    fn geometry_arg_indices_offset_curve_target_only() {
        assert_eq!(
            geometry_arg_indices("offset_curve"),
            &[0],
            "offset_curve's only geometry-ref arg is the curve target at index 0"
        );
    }

    #[test]
    fn all_dispatch_functions_accounted_for() {
        // Ensure the two lists together with loft and the boolean ops cover every
        // arm in compile_geometry_call.  If a new function is added there but not
        // listed here, this test should be updated (the developer will notice
        // because the new function is absent from both lists).
        let mut all: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for &name in GEOM_ARG_FUNCTIONS
            .iter()
            .chain(NO_GEOM_ARG_FUNCTIONS.iter())
            .chain(BOOLEAN_OP_FUNCTIONS.iter())
            .chain(LOFT_FUNCTIONS.iter())
        {
            assert!(
                all.insert(name),
                "duplicate function name \"{}\" in cross-check lists",
                name
            );
        }

        // The per-function tests above (`geometry_arg_indices_covers_all_geom_arg_functions`
        // and `geometry_arg_indices_empty_for_no_geom_arg_functions`) are the primary
        // correctness guardrail — they verify each function is in the right list.
        // `EXPECTED_DISPATCH_COUNT` is the canary pin for the four lists above.  If any of
        // GEOM_ARG_FUNCTIONS, NO_GEOM_ARG_FUNCTIONS, BOOLEAN_OP_FUNCTIONS, or LOFT_FUNCTIONS changes,
        // bump that constant and verify that `compile_geometry_call` contains a matching
        // dispatch arm for the new entry.
        // NOTE: this test does NOT detect the reverse — an arm added to
        // `compile_geometry_call` whose name is not listed in any of the four lists.
        // The companion `all_registry_names_reach_non_wildcard_arm` only covers the
        // forward direction (list → dispatch). True bidirectional coverage would
        // require a source-text scan of the match arms.
        assert_eq!(
            all.len(),
            EXPECTED_DISPATCH_COUNT,
            "total dispatched geometry function count changed — \
             bump EXPECTED_DISPATCH_COUNT and make sure the new function is added to \
             GEOM_ARG_FUNCTIONS, NO_GEOM_ARG_FUNCTIONS, BOOLEAN_OP_FUNCTIONS, or LOFT_FUNCTIONS above"
        );
    }

    #[test]
    fn all_registry_names_reach_non_wildcard_arm() {
        // Verify that every function name in the four registry lists dispatches to a
        // concrete arm in `compile_geometry_call` and does NOT reach the wildcard `_ =>`
        // arm (which emits the "unsupported geometry function" diagnostic).
        //
        // Passing `args: vec![]` is intentional: every dispatch arm returns early via
        // `push_diagnostic + return None` on arg-count/type mismatches, so no arm
        // panics on empty args — each generates its own arm-specific diagnostic, NOT
        // the wildcard marker.
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];

        for &name in GEOM_ARG_FUNCTIONS
            .iter()
            .chain(NO_GEOM_ARG_FUNCTIONS.iter())
            .chain(BOOLEAN_OP_FUNCTIONS.iter())
            .chain(LOFT_FUNCTIONS.iter())
        {
            let expr = reify_ast::Expr {
                kind: reify_ast::ExprKind::FunctionCall {
                    name: name.to_string(),
                    args: vec![],
                    arg_names: vec![],
                },
                span: reify_core::SourceSpan::new(0, 1),
            };
            let scope = CompilationScope::new("test");
            let mut diagnostics: Vec<Diagnostic> = vec![];
            let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

            compile_geometry_call(
                &expr,
                &scope,
                &enum_defs,
                &functions,
                &mut diagnostics,
                0,
                &geometry_lets,
                &mut HashSet::new(),
            );

            let wildcard_msg = unsupported_geometry_fn_message(name);
            assert!(
                !diagnostics.iter().any(|d| d.message == wildcard_msg),
                "registry name {:?} reached the wildcard arm in compile_geometry_call \
                 (\"{}: {}\" diagnostic was emitted) — \
                 add a dispatch arm for this name or remove it from the registry lists",
                name,
                UNSUPPORTED_GEOMETRY_FN_MSG,
                name
            );
        }
    }

    // ─── extrude_symmetric (task-322 step-5) ─────────────────────────────────

    /// extrude_symmetric() with 1 arg (missing distance) should produce diagnostics.
    #[test]
    fn extrude_symmetric_compiler_rejects_one_arg() {
        let source = r#"structure S {
    param profile: Length = 5mm
    let result = extrude_symmetric(profile)
}"#;
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_extsym1"));
        let compiled = crate::compile(&parsed);
        let template = &compiled.templates[0];
        let has_op = template.realizations.iter().any(|r| {
            r.operations.iter().any(|op| {
                matches!(
                    op,
                    crate::CompiledGeometryOp::Sweep {
                        kind: crate::SweepKind::ExtrudeSymmetric,
                        ..
                    }
                )
            })
        });
        assert!(
            !compiled.diagnostics.is_empty(),
            "expected error diagnostic for wrong arg count (1 arg)"
        );
        assert!(
            !has_op,
            "should not produce Sweep(ExtrudeSymmetric) op with wrong arg count (1 arg)"
        );
    }

    /// extrude_symmetric() with 3 args should produce diagnostics.
    #[test]
    fn extrude_symmetric_compiler_rejects_three_args() {
        let source = r#"structure S {
    param profile: Length = 5mm
    param dist: Length = 10mm
    let result = extrude_symmetric(profile, dist, dist)
}"#;
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_extsym3"));
        let compiled = crate::compile(&parsed);
        let template = &compiled.templates[0];
        let has_op = template.realizations.iter().any(|r| {
            r.operations.iter().any(|op| {
                matches!(
                    op,
                    crate::CompiledGeometryOp::Sweep {
                        kind: crate::SweepKind::ExtrudeSymmetric,
                        ..
                    }
                )
            })
        });
        assert!(
            !compiled.diagnostics.is_empty(),
            "expected error diagnostic for wrong arg count (3 args)"
        );
        assert!(
            !has_op,
            "should not produce Sweep(ExtrudeSymmetric) op with wrong arg count (3 args)"
        );
    }

    /// sweep_guided() with 2 args should produce diagnostics (missing guide).
    #[test]
    fn sweep_guided_compiler_rejects_two_args() {
        let source = r#"structure S {
    param a: Length = 1mm
    param b: Length = 1mm
    let result = sweep_guided(a, b)
}"#;
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_swg2"));
        let compiled = crate::compile(&parsed);
        let template = &compiled.templates[0];
        let has_op = template.realizations.iter().any(|r| {
            r.operations.iter().any(|op| {
                matches!(
                    op,
                    crate::CompiledGeometryOp::Sweep {
                        kind: crate::SweepKind::SweepGuided,
                        ..
                    }
                )
            })
        });
        assert!(
            !compiled.diagnostics.is_empty(),
            "expected error diagnostic for wrong arg count (2 args)"
        );
        assert!(
            !has_op,
            "should not produce Sweep(SweepGuided) op with wrong arg count"
        );
    }

    /// sweep_guided() with 4 args should produce diagnostics.
    #[test]
    fn sweep_guided_compiler_rejects_four_args() {
        let source = r#"structure S {
    param a: Length = 1mm
    param b: Length = 1mm
    param c: Length = 1mm
    param d: Length = 1mm
    let result = sweep_guided(a, b, c, d)
}"#;
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_swg4"));
        let compiled = crate::compile(&parsed);
        let template = &compiled.templates[0];
        let has_op = template.realizations.iter().any(|r| {
            r.operations.iter().any(|op| {
                matches!(
                    op,
                    crate::CompiledGeometryOp::Sweep {
                        kind: crate::SweepKind::SweepGuided,
                        ..
                    }
                )
            })
        });
        assert!(
            !compiled.diagnostics.is_empty(),
            "expected error diagnostic for wrong arg count (4 args)"
        );
        assert!(
            !has_op,
            "should not produce Sweep(SweepGuided) op with wrong arg count"
        );
    }

    /// sweep_guided() with 3 non-geometry args emits per-arg diagnostics
    /// (mirroring sweep() behaviour at geometry.rs:552-579).
    #[test]
    fn sweep_guided_compiler_rejects_non_geometry_args() {
        let source = r#"structure S {
    param a: Length = 1mm
    param b: Length = 1mm
    param c: Length = 1mm
    let result = sweep_guided(a, b, c)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_core::ModulePath::single("test_swg_nongeom"));
        let compiled = crate::compile(&parsed);
        // Expect three per-arg diagnostics mentioning the arg labels.
        let profile_diag = compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("profile") && d.message.contains("sweep_guided"));
        let path_diag = compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("path") && d.message.contains("sweep_guided"));
        let guide_diag = compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("guide") && d.message.contains("sweep_guided"));
        assert!(
            profile_diag,
            "expected profile-arg diagnostic, got: {:?}",
            compiled.diagnostics
        );
        assert!(
            path_diag,
            "expected path-arg diagnostic, got: {:?}",
            compiled.diagnostics
        );
        assert!(
            guide_diag,
            "expected guide-arg diagnostic, got: {:?}",
            compiled.diagnostics
        );
    }

    /// sweep_guided() with 3 geometry args should produce a Sweep(SweepGuided)
    /// realization with no diagnostics.
    #[test]
    fn sweep_guided_compiler_accepts_three_geometry_args() {
        let source = r#"structure S {
    let profile = sphere(5mm)
    let path = sphere(3mm)
    let guide = sphere(2mm)
    let result = sweep_guided(profile, path, guide)
}"#;
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_swg_ok"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = crate::compile(&parsed);
        let template = &compiled.templates[0];
        // Each `let sphere(...)` creates its own realization; the final
        // `let result = sweep_guided(...)` adds a realization whose ops contain
        // the inlined sphere sub-ops plus the Sweep(SweepGuided) terminal op.
        let has_op = template.realizations.iter().any(|r| {
            r.operations.iter().any(|op| {
                matches!(
                    op,
                    crate::CompiledGeometryOp::Sweep {
                        kind: crate::SweepKind::SweepGuided,
                        ..
                    }
                )
            })
        });
        assert!(
            has_op,
            "expected Sweep(SweepGuided) op somewhere in realizations"
        );
        assert!(
            compiled.diagnostics.is_empty(),
            "expected no diagnostics for sweep_guided(profile, path, guide), got: {:?}",
            compiled.diagnostics
        );
    }

    /// extrude_symmetric() with 2 args should produce a Sweep(ExtrudeSymmetric) realization.
    #[test]
    fn extrude_symmetric_compiler_accepts_two_args() {
        let source = r#"structure S {
    param profile: Length = 5mm
    param dist: Length = 10mm
    let result = extrude_symmetric(profile, dist)
}"#;
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_extsym2"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = crate::compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for extrude_symmetric call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                crate::CompiledGeometryOp::Sweep {
                    kind: crate::SweepKind::ExtrudeSymmetric,
                    ..
                }
            ),
            "expected Sweep(ExtrudeSymmetric), got {:?}",
            op
        );
        assert!(
            compiled.diagnostics.is_empty(),
            "expected no diagnostics for extrude_symmetric(profile, dist), got: {:?}",
            compiled.diagnostics
        );
    }

    // ─── loft_guided (task-322 step-9) ──────────────────────────────────────

    /// loft_guided() with <3 args should produce an arity diagnostic.
    #[test]
    fn loft_guided_compiler_rejects_two_args() {
        let source = r#"structure S {
    let p1 = sphere(5mm)
    let p2 = sphere(3mm)
    let result = loft_guided(p1, p2)
}"#;
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_lg2"));
        let compiled = crate::compile(&parsed);
        let template = &compiled.templates[0];
        let has_op = template.realizations.iter().any(|r| {
            r.operations.iter().any(|op| {
                matches!(
                    op,
                    crate::CompiledGeometryOp::Sweep {
                        kind: crate::SweepKind::LoftGuided,
                        ..
                    }
                )
            })
        });
        assert!(
            !compiled.diagnostics.is_empty(),
            "expected error diagnostic for wrong arg count (2 args)"
        );
        assert!(
            !has_op,
            "should not produce Sweep(LoftGuided) op with wrong arg count (2 args)"
        );
    }

    /// loft_guided() with exactly 3 args (p1, p2, guide) should produce
    /// a Sweep(LoftGuided) op with 3 profile refs (2 profiles + 1 guide).
    #[test]
    fn loft_guided_compiler_accepts_three_args() {
        let source = r#"structure S {
    let p1 = sphere(5mm)
    let p2 = sphere(3mm)
    let guide = sphere(2mm)
    let result = loft_guided(p1, p2, guide)
}"#;
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_lg3"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = crate::compile(&parsed);
        let template = &compiled.templates[0];
        // Find the Sweep(LoftGuided) op across all realizations.
        let op = template
            .realizations
            .iter()
            .flat_map(|r| r.operations.iter())
            .find(|op| {
                matches!(
                    op,
                    crate::CompiledGeometryOp::Sweep {
                        kind: crate::SweepKind::LoftGuided,
                        ..
                    }
                )
            });
        let op = op.expect("expected Sweep(LoftGuided) op");
        // profiles slice should contain 3 GeomRef entries: 2 profiles + 1 guide.
        match op {
            crate::CompiledGeometryOp::Sweep { profiles, args, .. } => {
                assert_eq!(
                    profiles.len(),
                    3,
                    "expected 3 GeomRefs (2 profiles + 1 guide), got {}",
                    profiles.len()
                );
                // Last arg must be the guide (by convention).
                let last_key = args.last().map(|(k, _)| k.as_str()).unwrap_or("");
                assert_eq!(
                    last_key, "guide",
                    "expected last arg to be keyed 'guide', got {:?}",
                    last_key
                );
            }
            _ => unreachable!(),
        }
        assert!(
            compiled.diagnostics.is_empty(),
            "expected no diagnostics for loft_guided(p1, p2, guide), got: {:?}",
            compiled.diagnostics
        );
    }

    /// loft_guided() with 4 args should compile as 3 profile refs + 1 guide.
    #[test]
    fn loft_guided_compiler_accepts_four_args() {
        let source = r#"structure S {
    let p1 = sphere(5mm)
    let p2 = sphere(4mm)
    let p3 = sphere(3mm)
    let guide = sphere(2mm)
    let result = loft_guided(p1, p2, p3, guide)
}"#;
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_lg4"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = crate::compile(&parsed);
        let template = &compiled.templates[0];
        let op = template
            .realizations
            .iter()
            .flat_map(|r| r.operations.iter())
            .find(|op| {
                matches!(
                    op,
                    crate::CompiledGeometryOp::Sweep {
                        kind: crate::SweepKind::LoftGuided,
                        ..
                    }
                )
            });
        let op = op.expect("expected Sweep(LoftGuided) op");
        match op {
            crate::CompiledGeometryOp::Sweep { profiles, args, .. } => {
                assert_eq!(
                    profiles.len(),
                    4,
                    "expected 4 GeomRefs (3 profiles + 1 guide), got {}",
                    profiles.len()
                );
                let last_key = args.last().map(|(k, _)| k.as_str()).unwrap_or("");
                assert_eq!(last_key, "guide");
            }
            _ => unreachable!(),
        }
        assert!(
            compiled.diagnostics.is_empty(),
            "expected no diagnostics for loft_guided with 4 args, got: {:?}",
            compiled.diagnostics
        );
    }

    /// loft_guided() with non-geometry args should silently fall back like loft.
    /// The op is still produced with GeomRef::Step fallbacks so downstream
    /// analysis sees the structure.
    #[test]
    fn loft_guided_compiler_non_geom_args_silent_fallback() {
        let source = r#"structure S {
    param a: Length = 1mm
    param b: Length = 1mm
    param c: Length = 1mm
    let result = loft_guided(a, b, c)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_core::ModulePath::single("test_lg_nongeom"));
        let compiled = crate::compile(&parsed);
        let template = &compiled.templates[0];
        // An op should still be produced with fallback GeomRef::Step refs
        // (silent fallback mirroring loft's behavior).
        let has_op = template.realizations.iter().any(|r| {
            r.operations.iter().any(|op| {
                matches!(
                    op,
                    crate::CompiledGeometryOp::Sweep {
                        kind: crate::SweepKind::LoftGuided,
                        ..
                    }
                )
            })
        });
        assert!(
            has_op,
            "expected Sweep(LoftGuided) op produced with fallback refs"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "loft_guided requires at least 2 args")]
    fn resolve_loft_like_args_debug_asserts_guide_suffix_requires_two_args() {
        let compiled_args = vec![CompiledExpr::literal(
            Value::Real(0.0),
            reify_core::Type::dimensionless_scalar(),
        )];
        let geom_refs: HashMap<usize, GeomRef> = HashMap::new();
        // guide_suffix=true with only 1 arg must panic via debug_assert!
        resolve_loft_like_args(compiled_args, &geom_refs, 0, true);
    }

    /// Unit test for the `resolve_loft_like_args` helper.
    ///
    /// Covers:
    ///   (a) `guide_suffix=false` (loft shape): 3 CompiledExprs, geom_refs maps
    ///       idx 1 → Step(42) only (0 and 2 missing), step_offset=10.
    ///       Expected profiles: [Step(10), Step(42), Step(12)].
    ///       Expected named-arg keys: ["profile_0", "profile_1", "profile_2"].
    ///
    ///   (b) `guide_suffix=true` (loft_guided shape): 3 CompiledExprs, geom_refs
    ///       maps idx 0 → Step(7) only, step_offset=5.
    ///       Expected profiles: [Step(7), Step(6), Step(7)].
    ///       Expected named-arg keys: ["profile_0", "profile_1", "guide"].
    #[test]
    fn resolve_loft_like_args_builds_profiles_and_named_args() {
        // Each slot carries a distinct Real marker (slot index as f64) so that
        // any accidental reordering in the into_iter().enumerate() pipeline
        // (e.g. .rev(), shuffled zip) would be caught by the ordering assertions
        // below.  Using identical 1.0 markers for every slot would hide such regressions.
        fn make_args(n: usize) -> Vec<CompiledExpr> {
            (0..n)
                .map(|i| CompiledExpr::literal(Value::Real(i as f64), reify_core::Type::dimensionless_scalar()))
                .collect()
        }

        // ── (a) guide_suffix = false (loft) ─────────────────────────────────
        {
            let mut geom_refs: HashMap<usize, GeomRef> = HashMap::new();
            geom_refs.insert(1, GeomRef::Step(42));
            let compiled_args = make_args(3);
            let step_offset = 10;

            let (profiles, named_args) =
                resolve_loft_like_args(compiled_args, &geom_refs, step_offset, false);

            assert_eq!(
                profiles,
                vec![GeomRef::Step(10), GeomRef::Step(42), GeomRef::Step(12)],
                "loft: expected silent fallback for missing indices 0 and 2"
            );
            let keys: Vec<&str> = named_args.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(
                keys,
                vec!["profile_0", "profile_1", "profile_2"],
                "loft: all keys should be profile_N"
            );
            // Ordering pin: named_args[i] must carry the marker for slot i (i as f64).
            for (i, (_, expr)) in named_args.iter().enumerate() {
                match &expr.kind {
                    CompiledExprKind::Literal(Value::Real(f)) => {
                        assert!(
                            *f == i as f64,
                            "loft: named_args[{i}] has marker {f}, expected {i} — ordering broken"
                        );
                    }
                    other => {
                        panic!("loft: named_args[{i}].kind is {other:?}, expected Literal(Real)")
                    }
                }
            }
        }

        // ── (b) guide_suffix = true (loft_guided) ───────────────────────────
        {
            let mut geom_refs: HashMap<usize, GeomRef> = HashMap::new();
            geom_refs.insert(0, GeomRef::Step(7));
            let compiled_args = make_args(3);
            let step_offset = 5;

            let (profiles, named_args) =
                resolve_loft_like_args(compiled_args, &geom_refs, step_offset, true);

            assert_eq!(
                profiles,
                vec![GeomRef::Step(7), GeomRef::Step(6), GeomRef::Step(7)],
                "loft_guided: idx 0 from map, idx 1 fallback=5+1=6, idx 2 fallback=5+2=7"
            );
            let keys: Vec<&str> = named_args.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(
                keys,
                vec!["profile_0", "profile_1", "guide"],
                "loft_guided: last key should be 'guide'"
            );
            // Ordering pin: named_args[i] must carry the marker for slot i (i as f64).
            for (i, (_, expr)) in named_args.iter().enumerate() {
                match &expr.kind {
                    CompiledExprKind::Literal(Value::Real(f)) => {
                        assert!(
                            *f == i as f64,
                            "loft_guided: named_args[{i}] has marker {f}, expected {i} — ordering broken"
                        );
                    }
                    other => panic!(
                        "loft_guided: named_args[{i}].kind is {other:?}, expected Literal(Real)"
                    ),
                }
            }
        }
    }

    // --- Step 11: Directly test the catch-all branch in compile_geometry_call ---

    #[test]
    fn unsupported_geometry_fn_emits_diagnostic() {
        // Fabricate a FunctionCall expr with a name that is NOT in the
        // compile_geometry_call match arms (e.g., "make_cube").  This directly
        // exercises the `_ =>` catch-all branch added in step-4.
        let expr = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "make_cube".to_string(),
                args: vec![reify_ast::Expr {
                    kind: reify_ast::ExprKind::NumberLiteral {
                        value: 1.0,
                        is_real: false,
                    },
                    span: reify_core::SourceSpan::new(0, 1),
                }],
                arg_names: vec![None],
            },
            span: reify_core::SourceSpan::new(0, 10),
        };
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();
        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "unrecognized geometry fn should return None"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("unsupported geometry function")),
            "expected 'unsupported geometry function' diagnostic, got: {:?}",
            diagnostics
        );
    }

    // --- Bug fix tests: GeomRef::Step(0) fallback hardcoding (task-612/task-1732) ---

    #[test]
    fn sweep_non_geom_profile_fallback_uses_step_offset() {
        // sweep() where the profile arg is a literal number (not a geometry expression).
        // When step_offset=3, the profile GeomRef fallback should be Step(3), not Step(0).
        // The path arg is also a literal, so it falls back to Step(step_offset + 1) = Step(4).
        let expr = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "sweep".to_string(),
                args: vec![
                    reify_ast::Expr {
                        kind: reify_ast::ExprKind::NumberLiteral {
                            value: 1.0,
                            is_real: false,
                        },
                        span: reify_core::SourceSpan::new(0, 1),
                    },
                    reify_ast::Expr {
                        kind: reify_ast::ExprKind::NumberLiteral {
                            value: 2.0,
                            is_real: false,
                        },
                        span: reify_core::SourceSpan::new(0, 1),
                    },
                ],
                arg_names: vec![None, None],
            },
            span: reify_core::SourceSpan::new(0, 10),
        };
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            3, // step_offset = 3
            &geometry_lets,
            &mut HashSet::new(),
        );

        let ops = result.expect("sweep() should produce ops even with non-geometry args");
        let sweep_op = ops.last().expect("should have at least one op");
        match sweep_op {
            CompiledGeometryOp::Sweep {
                kind: SweepKind::Sweep,
                profiles,
                ..
            } => {
                assert_eq!(
                    profiles.len(),
                    2,
                    "sweep should have 2 profiles (profile, path)"
                );
                assert_eq!(
                    profiles[0],
                    GeomRef::Step(3),
                    "sweep profile fallback should be Step(step_offset=3), not {:?}",
                    profiles[0]
                );
                assert_eq!(
                    profiles[1],
                    GeomRef::Step(4),
                    "sweep path fallback should be Step(step_offset+1=4)"
                );
            }
            other => panic!("expected Sweep(Sweep), got {:?}", other),
        }
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].message.contains("profile"));
        assert!(diagnostics[1].message.contains("path"));
    }

    #[test]
    fn loft_non_geom_args_fallback_uses_step_offset() {
        // loft() with 3 literal-number args (not geometry expressions).
        // When step_offset=5:
        //   - The fallback GeomRef for profile i is GeomRef::Step(5 + i) — unique per
        //     profile, preserving loft's "distinct cross-sections" semantics in the
        //     fallback (consistent with sweep()'s profile=step_offset, path=step_offset+1
        //     convention applied per profile).
        //   - The fallback is silent: no per-argument diagnostic is emitted, matching
        //     the geom_ref convention used by extrude/revolve/translate/etc.
        //   - Ops are still produced (fallback refs allow compilation to continue).
        let expr = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "loft".to_string(),
                args: vec![
                    reify_ast::Expr {
                        kind: reify_ast::ExprKind::NumberLiteral {
                            value: 1.0,
                            is_real: false,
                        },
                        span: reify_core::SourceSpan::new(0, 1),
                    },
                    reify_ast::Expr {
                        kind: reify_ast::ExprKind::NumberLiteral {
                            value: 2.0,
                            is_real: false,
                        },
                        span: reify_core::SourceSpan::new(0, 1),
                    },
                    reify_ast::Expr {
                        kind: reify_ast::ExprKind::NumberLiteral {
                            value: 3.0,
                            is_real: false,
                        },
                        span: reify_core::SourceSpan::new(0, 1),
                    },
                ],
                arg_names: vec![None, None, None],
            },
            span: reify_core::SourceSpan::new(0, 10),
        };
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            5, // step_offset = 5
            &geometry_lets,
            &mut HashSet::new(),
        );

        // loft() with non-geometry args should still produce an op (with fallback refs)
        let ops = result.expect("loft() should produce ops even with non-geometry args");
        let loft_op = ops.last().expect("should have at least one op");
        match loft_op {
            CompiledGeometryOp::Sweep {
                kind: SweepKind::Loft,
                profiles,
                ..
            } => {
                assert_eq!(profiles.len(), 3, "loft should have 3 profiles");
                for (i, profile) in profiles.iter().enumerate() {
                    assert_eq!(
                        *profile,
                        GeomRef::Step(5 + i),
                        "loft fallback for profile {} should be Step(step_offset + {0} = {}), not {:?}",
                        i,
                        5 + i,
                        profile
                    );
                }
            }
            other => panic!("expected Sweep(Loft), got {:?}", other),
        }

        // No per-argument geometry-expression diagnostics should be emitted by the
        // loft fallback path — silent fallback matches the geom_ref convention.
        let geom_expr_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("must be a geometry expression"))
            .collect();
        assert!(
            geom_expr_diags.is_empty(),
            "expected silent fallback (no per-arg diagnostics), got: {:?}",
            geom_expr_diags
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    // --- Regression pin: CSG vs kinematic `sweep` arity disambiguation (task 2529) ---

    /// Helper: build a `FunctionCall` Expr with `n` numeric-literal args, named `name`.
    fn make_call_with_arity(name: &str, n: usize) -> reify_ast::Expr {
        let args = (0..n)
            .map(|_| reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 1.0,
                    is_real: false,
                },
                span: reify_core::SourceSpan::new(0, 1),
            })
            .collect();
        reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: name.to_string(),
                arg_names: vec![None; n],
                args,
            },
            span: reify_core::SourceSpan::new(0, 1),
        }
    }

    /// `is_geometry_let` must classify the 2-arg CSG `sweep(profile, path)` as
    /// a geometry let (docs §3) and the 4-arg kinematic
    /// `sweep(mechanism, joint, range, steps)` as NOT a geometry let
    /// (docs §13.4) so the latter routes through eval-time dispatch.
    #[test]
    fn is_geometry_let_disambiguates_csg_vs_kinematic_sweep_by_arity() {
        let functions: Vec<CompiledFunction> = vec![];
        let known: HashSet<&str> = HashSet::new();

        let csg_2 = make_call_with_arity("sweep", 2);
        assert!(
            is_geometry_let(&csg_2, &functions, &known, &HashSet::new()),
            "2-arg sweep (CSG profile/path) must classify as a geometry let"
        );

        let kinematic_4 = make_call_with_arity("sweep", 4);
        assert!(
            !is_geometry_let(&kinematic_4, &functions, &known, &HashSet::new()),
            "4-arg sweep (kinematic mechanism/joint/range/steps) must NOT \
             classify as a geometry let — it routes via eval-time dispatch"
        );

        // Other arities (typos) still flow into compile_geometry_call's CSG arm
        // so the user gets the strict "expects exactly 2 arguments" diagnostic.
        for n in [0, 1, 3, 5] {
            let other = make_call_with_arity("sweep", n);
            assert!(
                is_geometry_let(&other, &functions, &known, &HashSet::new()),
                "{n}-arg sweep must still classify as a geometry let so the \
                 CSG arity diagnostic fires; only the 4-arg kinematic form \
                 falls through"
            );
        }
    }

    // --- step-1 (task 4119 δ): is_geometry_let selector-call routing ---

    /// `is_geometry_let` must return FALSE for `union`/`difference` when ANY
    /// operand is a direct selector constructor call (`faces(b)`, `edges(b)`,
    /// etc.), so those compositions route to the value-typing path (and later
    /// emit `E_SELECTOR_KIND_MISMATCH` for mixed kinds) rather than the CSG
    /// `compile_boolean_op` path.
    ///
    /// At the same time it must STILL return TRUE for CSG `union`/`difference`
    /// whose operands are pure geometry (e.g. `box(…)`, `union(box(…), box(…))`).
    ///
    /// `intersect` is NOT a geometry function (`GEOMETRY_FUNCTION_NAMES` does
    /// not include it) so it already routes to the value path regardless.
    #[test]
    fn is_geometry_let_selector_operand_excludes_union_difference() {
        let functions: Vec<CompiledFunction> = vec![];
        let known: HashSet<&str> = HashSet::new();

        // --- selector-operand cases (must NOT be geometry lets) ---

        // union(faces(b), edges(b)) — mixed-kind but the operands are selector calls
        let union_sel = {
            let faces_b = make_call_with_arity("faces", 1);
            let edges_b = make_call_with_arity("edges", 1);
            reify_ast::Expr {
                kind: reify_ast::ExprKind::FunctionCall {
                    name: "union".to_string(),
                    arg_names: vec![None, None],
                    args: vec![faces_b, edges_b],
                },
                span: reify_core::SourceSpan::new(0, 1),
            }
        };
        assert!(
            !is_geometry_let(&union_sel, &functions, &known, &HashSet::new()),
            "union(faces(b), edges(b)) must NOT be a geometry let — \
             selector operands divert to the value-typing path"
        );

        // difference(faces(b), faces_by_normal(b, …)) — same-kind, both selector calls
        let diff_sel = {
            let faces_b = make_call_with_arity("faces", 1);
            let fbn = make_call_with_arity("faces_by_normal", 3);
            reify_ast::Expr {
                kind: reify_ast::ExprKind::FunctionCall {
                    name: "difference".to_string(),
                    arg_names: vec![None, None],
                    args: vec![faces_b, fbn],
                },
                span: reify_core::SourceSpan::new(0, 1),
            }
        };
        assert!(
            !is_geometry_let(&diff_sel, &functions, &known, &HashSet::new()),
            "difference(faces(b), faces_by_normal(b,...)) must NOT be a geometry let — \
             selector operands divert to the value-typing path"
        );

        // --- CSG cases (must STILL be geometry lets) ---

        // union(box(…), box(…)) — both operands are geometry constructors
        let union_csg = {
            let box1 = make_call_with_arity("box", 3);
            let box2 = make_call_with_arity("box", 3);
            reify_ast::Expr {
                kind: reify_ast::ExprKind::FunctionCall {
                    name: "union".to_string(),
                    arg_names: vec![None, None],
                    args: vec![box1, box2],
                },
                span: reify_core::SourceSpan::new(0, 1),
            }
        };
        assert!(
            is_geometry_let(&union_csg, &functions, &known, &HashSet::new()),
            "union(box(…), box(…)) must STILL be a geometry let — CSG path preserved"
        );

        // difference(box(…), cylinder(…)) — geometry operands
        let diff_csg = {
            let box1 = make_call_with_arity("box", 3);
            let cyl = make_call_with_arity("cylinder", 2);
            reify_ast::Expr {
                kind: reify_ast::ExprKind::FunctionCall {
                    name: "difference".to_string(),
                    arg_names: vec![None, None],
                    args: vec![box1, cyl],
                },
                span: reify_core::SourceSpan::new(0, 1),
            }
        };
        assert!(
            is_geometry_let(&diff_csg, &functions, &known, &HashSet::new()),
            "difference(box(…), cylinder(…)) must STILL be a geometry let — CSG path preserved"
        );
    }

    // --- task 4527: Ident operand routing — new behaviour with known_selector_lets ---

    /// Characterises the new `known_selector_lets` accumulator behaviour (task 4527):
    ///
    /// - With `known_selector_lets = {"top", "big"}`:
    ///   `union(top, big)` and `difference(top, big)` → `is_geometry_let` == **false**
    ///   (selector path: is_selector_expr returns true for each Ident in the set).
    ///
    /// - With EMPTY `known_selector_lets`:
    ///   `union(top, big)` → **true** (unknown idents → CSG default, safety property).
    ///
    /// - With `top`/`big` in `known_GEOMETRY_lets` (NOT in known_selector_lets):
    ///   `union(top, big)` → **true** (CSG preserved for geometry idents).
    #[test]
    fn is_geometry_let_all_ident_selector_operands_route_to_selector() {
        let functions: Vec<CompiledFunction> = vec![];

        // Helper to build an Ident expression.
        let make_ident = |name: &str| reify_ast::Expr {
            kind: reify_ast::ExprKind::Ident(name.to_string()),
            span: reify_core::SourceSpan::new(0, name.len() as u32),
        };

        // union(top, big) where both operands are Idents.
        let union_ident = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "union".to_string(),
                arg_names: vec![None, None],
                args: vec![make_ident("top"), make_ident("big")],
            },
            span: reify_core::SourceSpan::new(0, 12),
        };
        // difference(top, big) — same operand structure.
        let diff_ident = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "difference".to_string(),
                arg_names: vec![None, None],
                args: vec![make_ident("top"), make_ident("big")],
            },
            span: reify_core::SourceSpan::new(0, 18),
        };

        // Case 1 — selector idents in known_selector_lets → selector path (false).
        let geom_empty: HashSet<&str> = HashSet::new();
        let mut sel_known: HashSet<&str> = HashSet::new();
        sel_known.insert("top");
        sel_known.insert("big");
        assert!(
            !is_geometry_let(&union_ident, &functions, &geom_empty, &sel_known),
            "union(top, big) with top/big in known_selector_lets must route to \
             selector path (is_geometry_let == false)"
        );
        assert!(
            !is_geometry_let(&diff_ident, &functions, &geom_empty, &sel_known),
            "difference(top, big) with top/big in known_selector_lets must route \
             to selector path (is_geometry_let == false)"
        );

        // Case 2 — empty known_selector_lets → unknown idents default to CSG (true).
        // Safety property: genuinely-unknown idents must not flip to selector path.
        let sel_empty: HashSet<&str> = HashSet::new();
        assert!(
            is_geometry_let(&union_ident, &functions, &geom_empty, &sel_empty),
            "union(top, big) with EMPTY known_selector_lets must still be treated \
             as a geometry let (CSG default) — unknown idents must not flip routing"
        );

        // Case 3 — same name in BOTH known_geometry_lets AND known_selector_lets:
        // is_selector_composition is driven by is_selector_expr (which consults
        // known_selector_lets via the Ident arm), so when top/big are in both sets
        // is_selector_composition = true → is_geometry_let = false (selector path).
        // Documents the actual tie-breaking rule: known_selector_lets membership in
        // the operand controls the union/difference routing, not known_geometry_lets.
        // In practice a name is never in both sets (they are populated in the else-branch
        // of each other), but this pins the exact behavior if that invariant were broken.
        let mut geom_both: HashSet<&str> = HashSet::new();
        let mut sel_both: HashSet<&str> = HashSet::new();
        geom_both.insert("top");
        geom_both.insert("big");
        sel_both.insert("top");
        sel_both.insert("big");
        assert!(
            !is_geometry_let(&union_ident, &functions, &geom_both, &sel_both),
            "union(top, big) with top/big in BOTH sets: known_selector_lets membership \
             drives is_selector_composition → is_geometry_let must be false (selector \
             path), even when the operand names are also in known_geometry_lets"
        );
    }

    // --- is_selector_expr: mid_surface selector classification (task 4536) ---

    /// Task 4536 (step-3 RED). Single-arg `mid_surface(b)` must be classified as
    /// a selector expression by `is_selector_expr` so a `let m = mid_surface(b)`
    /// binding routes through the selector/ResolveSelector path, NOT CSG
    /// geometry-let handling. Mirrors the `face`/`edge`/`solid_body` single-arg
    /// constructor classification. RED until step-4 adds "mid_surface" to the
    /// explicit name match.
    #[test]
    fn is_selector_expr_recognises_mid_surface() {
        let functions: Vec<CompiledFunction> = vec![];
        let known: HashSet<&str> = HashSet::new();
        let expr = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "mid_surface".to_string(),
                arg_names: vec![None],
                args: vec![reify_ast::Expr {
                    kind: reify_ast::ExprKind::Ident("b".to_string()),
                    span: reify_core::SourceSpan::new(0, 1),
                }],
            },
            span: reify_core::SourceSpan::new(0, 16),
        };
        assert!(
            is_selector_expr(&expr, &functions, &known),
            "mid_surface(b) must be classified as a selector expression so \
             `let m = mid_surface(b)` routes through the selector path, not CSG \
             geometry-let handling"
        );
    }

    // --- is_selector_expr / is_geometry_let: vertices/vertex classification (task 4368, steps 11-12) ---

    /// Task 4368 (step-11 RED). `vertices(b)` (All-leaf, arity-1) and
    /// `vertex(b, "tip")` (Named-leaf, arity-2) must be classified as selector
    /// expressions by `is_selector_expr` so a `let vs = vertices(b)` binding
    /// routes through the selector / ResolveSelector value-typing path, NOT CSG
    /// geometry-let handling. Mirrors `is_selector_expr_recognises_mid_surface`
    /// and the `face`/`edge`/`solid_body` Named-ctor classification. RED until
    /// step-12 adds `"vertices" | "vertex"` to the explicit name match.
    #[test]
    fn is_selector_expr_recognises_vertices_and_vertex() {
        let functions: Vec<CompiledFunction> = vec![];
        let known: HashSet<&str> = HashSet::new();

        // vertices(b) — single-arg All-leaf constructor.
        let vertices_call = make_call_with_arity("vertices", 1);
        assert!(
            is_selector_expr(&vertices_call, &functions, &known),
            "vertices(b) must be classified as a selector expression so \
             `let vs = vertices(b)` routes through the selector path, not CSG \
             geometry-let handling"
        );

        // vertex(b, "tip") — two-arg Named-leaf constructor.
        let vertex_call = make_call_with_arity("vertex", 2);
        assert!(
            is_selector_expr(&vertex_call, &functions, &known),
            "vertex(b, \"tip\") must be classified as a selector expression so \
             `let v = vertex(b, \"tip\")` routes through the selector path, not \
             CSG geometry-let handling"
        );
    }

    /// Task 4368 (step-11 RED). `is_geometry_let` must return FALSE for
    /// `union`/`difference` when ANY operand is a vertex-selector constructor
    /// (`vertices(b)` / `vertex(b, "tip")`), so those compositions route to the
    /// value-typing path (and later emit `E_SELECTOR_KIND_MISMATCH` on mixed
    /// kinds) rather than the CSG `compile_boolean_op` path — while STILL
    /// returning TRUE for pure-geometry CSG `union`/`difference`. Mirrors
    /// `is_geometry_let_selector_operand_excludes_union_difference` with the new
    /// vertex constructors. RED until step-12 classifies `"vertices"`/`"vertex"`.
    #[test]
    fn is_geometry_let_vertex_selector_operand_excludes_union_difference() {
        let functions: Vec<CompiledFunction> = vec![];
        let known: HashSet<&str> = HashSet::new();

        // --- selector-operand cases (must NOT be geometry lets) ---

        // union(vertices(a), vertices(b)) — same-kind, both selector calls.
        let union_sel = {
            let vertices_a = make_call_with_arity("vertices", 1);
            let vertices_b = make_call_with_arity("vertices", 1);
            reify_ast::Expr {
                kind: reify_ast::ExprKind::FunctionCall {
                    name: "union".to_string(),
                    arg_names: vec![None, None],
                    args: vec![vertices_a, vertices_b],
                },
                span: reify_core::SourceSpan::new(0, 1),
            }
        };
        assert!(
            !is_geometry_let(&union_sel, &functions, &known, &HashSet::new()),
            "union(vertices(a), vertices(b)) must NOT be a geometry let — \
             vertex-selector operands divert to the value-typing path"
        );

        // difference(vertices(b), vertex(b, "tip")) — both vertex-selector calls.
        let diff_sel = {
            let vertices_b = make_call_with_arity("vertices", 1);
            let vertex_b = make_call_with_arity("vertex", 2);
            reify_ast::Expr {
                kind: reify_ast::ExprKind::FunctionCall {
                    name: "difference".to_string(),
                    arg_names: vec![None, None],
                    args: vec![vertices_b, vertex_b],
                },
                span: reify_core::SourceSpan::new(0, 1),
            }
        };
        assert!(
            !is_geometry_let(&diff_sel, &functions, &known, &HashSet::new()),
            "difference(vertices(b), vertex(b, \"tip\")) must NOT be a geometry let — \
             vertex-selector operands divert to the value-typing path"
        );

        // --- CSG cases (must STILL be geometry lets) ---

        // union(box(…), box(…)) — both operands are geometry constructors.
        let union_csg = {
            let box1 = make_call_with_arity("box", 3);
            let box2 = make_call_with_arity("box", 3);
            reify_ast::Expr {
                kind: reify_ast::ExprKind::FunctionCall {
                    name: "union".to_string(),
                    arg_names: vec![None, None],
                    args: vec![box1, box2],
                },
                span: reify_core::SourceSpan::new(0, 1),
            }
        };
        assert!(
            is_geometry_let(&union_csg, &functions, &known, &HashSet::new()),
            "union(box(…), box(…)) must STILL be a geometry let — CSG path preserved"
        );

        // difference(box(…), cylinder(…)) — geometry operands.
        let diff_csg = {
            let box1 = make_call_with_arity("box", 3);
            let cyl = make_call_with_arity("cylinder", 2);
            reify_ast::Expr {
                kind: reify_ast::ExprKind::FunctionCall {
                    name: "difference".to_string(),
                    arg_names: vec![None, None],
                    args: vec![box1, cyl],
                },
                span: reify_core::SourceSpan::new(0, 1),
            }
        };
        assert!(
            is_geometry_let(&diff_csg, &functions, &known, &HashSet::new()),
            "difference(box(…), cylinder(…)) must STILL be a geometry let — CSG path preserved"
        );
    }

    /// Task 4368 (step-11). Selector-let-chain routing for vertex selectors:
    /// with `known_selector_lets = {"vs", "ws"}` (the set entity.rs populates
    /// once `let vs = vertices(b)` / `let ws = vertices(c)` are classified by
    /// is_selector_expr), `union(vs, ws)` and `difference(vs, ws)` (Ident
    /// operands) must route to the selector path (`is_geometry_let == false`).
    /// This locks the entity.rs known_selector_lets population that depends on
    /// is_selector_expr classifying `let vs = vertices(b)`. Mirrors
    /// `is_geometry_let_all_ident_selector_operands_route_to_selector`.
    #[test]
    fn is_geometry_let_vertex_ident_selector_operands_route_to_selector() {
        let functions: Vec<CompiledFunction> = vec![];

        let make_ident = |name: &str| reify_ast::Expr {
            kind: reify_ast::ExprKind::Ident(name.to_string()),
            span: reify_core::SourceSpan::new(0, name.len() as u32),
        };

        // union(vs, ws) where both operands are Idents.
        let union_ident = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "union".to_string(),
                arg_names: vec![None, None],
                args: vec![make_ident("vs"), make_ident("ws")],
            },
            span: reify_core::SourceSpan::new(0, 12),
        };
        // difference(vs, ws) — same operand structure.
        let diff_ident = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "difference".to_string(),
                arg_names: vec![None, None],
                args: vec![make_ident("vs"), make_ident("ws")],
            },
            span: reify_core::SourceSpan::new(0, 18),
        };

        let geom_empty: HashSet<&str> = HashSet::new();
        let mut sel_known: HashSet<&str> = HashSet::new();
        sel_known.insert("vs");
        sel_known.insert("ws");
        assert!(
            !is_geometry_let(&union_ident, &functions, &geom_empty, &sel_known),
            "union(vs, ws) with vs/ws in known_selector_lets must route to the \
             selector path (is_geometry_let == false)"
        );
        assert!(
            !is_geometry_let(&diff_ident, &functions, &geom_empty, &sel_known),
            "difference(vs, ws) with vs/ws in known_selector_lets must route to \
             the selector path (is_geometry_let == false)"
        );
    }

    // --- compile_geometry_call: Conditional emits Error (task 3395) ---

    /// `compile_geometry_call` must emit a clean Error diagnostic (and return
    /// `None`) when given a `Conditional` with STRUCTURALLY-INCOMPATIBLE branches
    /// (box vs cylinder — different names), rather than silently falling through
    /// to `_ => return None` with no message.
    ///
    /// The tested source uses incompatible branches (box vs cylinder) so the
    /// graceful-error fallback path is covered.  The box-vs-box case (compatible
    /// branches, same arity) is now covered by the hoisting tests in
    /// `let_scope_tests.rs` — this test exercises the non-hoistable path only.
    #[test]
    fn compile_geometry_call_conditional_with_incompatible_branches_emits_error() {
        // Build: if true then box(1, 1, 1) else cylinder(1, 1) — incompatible.
        let bool_cond = reify_ast::Expr {
            kind: reify_ast::ExprKind::BoolLiteral(true),
            span: reify_core::SourceSpan::new(0, 4),
        };
        let box_expr = make_call_with_arity("box", 3);
        let cyl_expr = make_call_with_arity("cylinder", 2);
        let cond_expr = make_conditional(bool_cond, box_expr, cyl_expr);

        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &cond_expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        // (a) Must return None — no ops produced.
        assert!(
            result.is_none(),
            "compile_geometry_call must return None for a Conditional expression"
        );

        // (b) Must emit exactly one Error-severity diagnostic.
        let error_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Error)
            .collect();
        assert_eq!(
            error_diags.len(),
            1,
            "expected exactly one Error diagnostic, got: {:?}",
            error_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        // (c) The Error message must mention "if-then-else" and "geometry".
        let msg = &error_diags[0].message;
        assert!(
            msg.contains("if-then-else") && msg.contains("geometry"),
            "Error message must contain 'if-then-else' and 'geometry', got: {:?}",
            msg
        );

        // (d) The Error must have at least one DiagnosticLabel attached.
        assert!(
            !error_diags[0].labels.is_empty(),
            "Error diagnostic must have at least one label, got none"
        );
    }

    // --- Conditional branch geometry recognition (task 3395) ---

    /// Helper: build a `Conditional` Expr from three child Exprs.
    fn make_conditional(
        cond: reify_ast::Expr,
        then_branch: reify_ast::Expr,
        else_branch: reify_ast::Expr,
    ) -> reify_ast::Expr {
        reify_ast::Expr {
            kind: reify_ast::ExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            span: reify_core::SourceSpan::new(0, 1),
        }
    }

    /// `is_geometry_let` must classify an `if-then-else` expression as a
    /// geometry let when EITHER branch is a geometry call, so the expression
    /// is routed to `compile_geometry_call` where a clean compile-time Error
    /// is emitted (rather than silently falling through to the Step(0) crash).
    ///
    /// Task 3395 — this test MUST FAIL before the Conditional arm is added to
    /// `is_geometry_let` (the wildcard `_ => false` arm catches Conditional
    /// today).
    #[test]
    fn is_geometry_let_recognizes_conditional_with_geometry_branches() {
        let functions: Vec<CompiledFunction> = vec![];
        let known: HashSet<&str> = HashSet::new();

        let bool_cond = reify_ast::Expr {
            kind: reify_ast::ExprKind::BoolLiteral(true),
            span: reify_core::SourceSpan::new(0, 1),
        };
        let num_literal = reify_ast::Expr {
            kind: reify_ast::ExprKind::NumberLiteral {
                value: 1.0,
                is_real: false,
            },
            span: reify_core::SourceSpan::new(0, 1),
        };

        // (a) Both branches geometry → true
        let box_box = make_conditional(
            bool_cond.clone(),
            make_call_with_arity("box", 3),
            make_call_with_arity("box", 3),
        );
        assert!(
            is_geometry_let(&box_box, &functions, &known, &HashSet::new()),
            "Conditional with two geometry branches must classify as a geometry let"
        );

        // (b) Neither branch geometry → false
        let num_num = make_conditional(bool_cond.clone(), num_literal.clone(), num_literal.clone());
        assert!(
            !is_geometry_let(&num_num, &functions, &known, &HashSet::new()),
            "Conditional with no geometry branches must NOT classify as a geometry let"
        );

        // (c) Only then-branch geometry → true (either branch suffices)
        let box_num = make_conditional(
            bool_cond.clone(),
            make_call_with_arity("box", 3),
            num_literal.clone(),
        );
        assert!(
            is_geometry_let(&box_num, &functions, &known, &HashSet::new()),
            "Conditional with one geometry branch must classify as a geometry let"
        );

        // (d) Nested Conditional — geometry only in the inner else_branch;
        // recursion must traverse the outer else_branch to find it.
        let nested = make_conditional(
            bool_cond.clone(),
            num_literal.clone(),
            make_conditional(
                bool_cond.clone(),
                make_call_with_arity("box", 3),
                num_literal.clone(),
            ),
        );
        assert!(
            is_geometry_let(&nested, &functions, &known, &HashSet::new()),
            "Nested Conditional whose inner branch is geometry must classify as a geometry let"
        );

        // (e) Ident branch referencing a known geometry let — transitive recognition.
        let mut known_with_g: HashSet<&str> = HashSet::new();
        known_with_g.insert("g");
        let ident_g = reify_ast::Expr {
            kind: reify_ast::ExprKind::Ident("g".to_string()),
            span: reify_core::SourceSpan::new(0, 1),
        };
        let cond_ident = make_conditional(bool_cond.clone(), ident_g, num_literal.clone());
        assert!(
            is_geometry_let(&cond_ident, &functions, &known_with_g, &HashSet::new()),
            "Conditional with an Ident then-branch referencing a known geometry let must classify as a geometry let"
        );
    }

    // --- compile_geometry_call: Match emits Error (task 3418) ---

    /// `compile_geometry_call` must emit a clean Error diagnostic (and return
    /// `None`) when given a `Match` expression rather than silently falling
    /// through to `_ => return None` with no message.
    ///
    /// This test MUST FAIL before the Match arm is added to
    /// `compile_geometry_call` (today the expression falls through the
    /// `_ => return None` catch-all with no diagnostic emitted).
    #[test]
    fn compile_geometry_call_match_returning_solid_emits_error_and_returns_none() {
        // Build: match axis { X => box(1,1,1), Y => box(1,1,1) }
        let discriminant = reify_ast::Expr {
            kind: reify_ast::ExprKind::Ident("axis".to_string()),
            span: reify_core::SourceSpan::new(0, 4),
        };
        let match_expr = make_match(
            discriminant,
            vec![
                make_call_with_arity("box", 3),
                make_call_with_arity("box", 3),
            ],
        );

        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &match_expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        // (a) Must return None — no ops produced.
        assert!(
            result.is_none(),
            "compile_geometry_call must return None for a Match expression"
        );

        // (b) Must emit exactly one Error-severity diagnostic.
        let error_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Error)
            .collect();
        assert_eq!(
            error_diags.len(),
            1,
            "expected exactly one Error diagnostic, got: {:?}",
            error_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        // (c) The Error message must mention "match" and "geometry".
        let msg = &error_diags[0].message;
        assert!(
            msg.contains("match") && msg.contains("geometry"),
            "Error message must contain 'match' and 'geometry', got: {:?}",
            msg
        );

        // (d) The Error must have at least one DiagnosticLabel attached.
        assert!(
            !error_diags[0].labels.is_empty(),
            "Error diagnostic must have at least one label, got none"
        );
    }

    // --- Match branch geometry recognition (task 3418) ---

    /// Helper: build a `Match` Expr from a discriminant Expr and a slice of arm body Exprs.
    /// Each arm gets a single string pattern ("X", "Y", "Z", ...) assigned in order.
    fn make_match(
        discriminant: reify_ast::Expr,
        bodies: Vec<reify_ast::Expr>,
    ) -> reify_ast::Expr {
        let pattern_names = ["X", "Y", "Z", "W", "V"];
        let arms = bodies
            .into_iter()
            .enumerate()
            .map(|(i, body)| reify_ast::MatchArm {
                patterns: vec![reify_ast::MatchPattern::Variant(
                    pattern_names[i % pattern_names.len()].to_string(),
                )],
                body,
                span: reify_core::SourceSpan::new(0, 1),
            })
            .collect();
        reify_ast::Expr {
            kind: reify_ast::ExprKind::Match {
                discriminant: Box::new(discriminant),
                arms,
            },
            span: reify_core::SourceSpan::new(0, 1),
        }
    }

    /// `is_geometry_let` must classify a `match` expression as a geometry let
    /// when ANY arm is a geometry call, so the expression is routed to
    /// `compile_geometry_call` where a clean compile-time Error is emitted
    /// (rather than silently falling through to the Step(0) crash).
    ///
    /// Task 3418 — this test MUST FAIL before the Match arm is added to
    /// `is_geometry_let` (the wildcard `_ => false` arm catches Match today).
    #[test]
    fn is_geometry_let_recognizes_match_with_geometry_arms() {
        let functions: Vec<CompiledFunction> = vec![];
        let known: HashSet<&str> = HashSet::new();

        let discriminant = reify_ast::Expr {
            kind: reify_ast::ExprKind::Ident("axis".to_string()),
            span: reify_core::SourceSpan::new(0, 4),
        };
        let num_literal = reify_ast::Expr {
            kind: reify_ast::ExprKind::NumberLiteral {
                value: 1.0,
                is_real: false,
            },
            span: reify_core::SourceSpan::new(0, 1),
        };

        // (a) All arms geometry → true
        let all_geom = make_match(
            discriminant.clone(),
            vec![
                make_call_with_arity("box", 3),
                make_call_with_arity("box", 3),
                make_call_with_arity("box", 3),
            ],
        );
        assert!(
            is_geometry_let(&all_geom, &functions, &known, &HashSet::new()),
            "Match with all geometry arms must classify as a geometry let"
        );

        // (b) No arms geometry → false
        let no_geom = make_match(
            discriminant.clone(),
            vec![
                num_literal.clone(),
                num_literal.clone(),
                num_literal.clone(),
            ],
        );
        assert!(
            !is_geometry_let(&no_geom, &functions, &known, &HashSet::new()),
            "Match with no geometry arms must NOT classify as a geometry let"
        );

        // (c) One arm geometry, rest numeric → true (any arm suffices)
        let one_geom = make_match(
            discriminant.clone(),
            vec![
                make_call_with_arity("box", 3),
                num_literal.clone(),
                num_literal.clone(),
            ],
        );
        assert!(
            is_geometry_let(&one_geom, &functions, &known, &HashSet::new()),
            "Match with one geometry arm must classify as a geometry let"
        );

        // (d) Nested Match whose inner arm is geometry → true; recursion traverses.
        let inner_match = make_match(
            discriminant.clone(),
            vec![make_call_with_arity("box", 3), num_literal.clone()],
        );
        let outer_match = make_match(discriminant.clone(), vec![num_literal.clone(), inner_match]);
        assert!(
            is_geometry_let(&outer_match, &functions, &known, &HashSet::new()),
            "Nested Match whose inner arm is geometry must classify as a geometry let"
        );

        // (e) Ident arm referencing a known geometry let → transitive recognition.
        let mut known_with_g: HashSet<&str> = HashSet::new();
        known_with_g.insert("g");
        let ident_g = reify_ast::Expr {
            kind: reify_ast::ExprKind::Ident("g".to_string()),
            span: reify_core::SourceSpan::new(0, 1),
        };
        let match_ident = make_match(discriminant.clone(), vec![ident_g, num_literal.clone()]);
        assert!(
            is_geometry_let(&match_ident, &functions, &known_with_g, &HashSet::new()),
            "Match with an Ident arm referencing a known geometry let must classify as a geometry let"
        );
    }

    // ─── task-3815: merge_branches + try_hoist_geometry_conditional unit tests ──

    /// Helper: build a numeric literal Expr with value 1.
    #[allow(dead_code)]
    fn num_lit() -> reify_ast::Expr {
        reify_ast::Expr {
            kind: reify_ast::ExprKind::NumberLiteral {
                value: 1.0,
                is_real: false,
            },
            span: reify_core::SourceSpan::new(0, 1),
        }
    }

    /// Helper: build a bool-literal condition Expr (true).
    fn bool_cond_expr() -> reify_ast::Expr {
        reify_ast::Expr {
            kind: reify_ast::ExprKind::BoolLiteral(true),
            span: reify_core::SourceSpan::new(0, 4),
        }
    }

    /// Helper: build a FunctionCall Expr named "box" with three specific numeric arg values.
    fn make_box_with_values(w: f64, h: f64, d: f64) -> reify_ast::Expr {
        let num = |v: f64| reify_ast::Expr {
            kind: reify_ast::ExprKind::NumberLiteral {
                value: v,
                is_real: false,
            },
            span: reify_core::SourceSpan::new(0, 1),
        };
        reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "box".to_string(),
                args: vec![num(w), num(h), num(d)],
                arg_names: vec![None, None, None],
            },
            span: reify_core::SourceSpan::new(0, 1),
        }
    }

    /// `merge_branches`: box(10,20,30) vs box(40,50,60) →
    /// FunctionCall{"box", [Conditional{cond,10,40}, Conditional{cond,20,50}, Conditional{cond,30,60}]}.
    ///
    /// Uses distinct per-arg values so a branch-swap bug or condition-threading
    /// bug would fail the assertions — satisfies the requirement that tests
    /// verify condition identity and then/else branch values, not just shape.
    #[test]
    fn merge_branches_box_vs_box_produces_geometry_call_with_conditional_args() {
        let functions: Vec<CompiledFunction> = vec![];
        let cond = bool_cond_expr(); // BoolLiteral(true)
        let a_vals = [10.0_f64, 20.0, 30.0];
        let b_vals = [40.0_f64, 50.0, 60.0];
        let a = make_box_with_values(a_vals[0], a_vals[1], a_vals[2]);
        let b = make_box_with_values(b_vals[0], b_vals[1], b_vals[2]);
        let outer_span = reify_core::SourceSpan::new(0, 10);

        let merged = merge_branches(&cond, &a, &b, &functions, outer_span);

        let args = match &merged.kind {
            reify_ast::ExprKind::FunctionCall { name, args, .. } => {
                assert_eq!(name, "box");
                assert_eq!(args.len(), 3);
                args
            }
            other => panic!("expected FunctionCall, got {:?}", other),
        };
        for (i, arg) in args.iter().enumerate() {
            match &arg.kind {
                reify_ast::ExprKind::Conditional {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    // condition must be the outer BoolLiteral(true)
                    assert!(
                        matches!(&condition.kind, reify_ast::ExprKind::BoolLiteral(true)),
                        "arg {i}: condition should be BoolLiteral(true), got {:?}",
                        condition.kind
                    );
                    // then_branch must carry the a-side value
                    assert!(
                        matches!(
                            &then_branch.kind,
                            reify_ast::ExprKind::NumberLiteral { value, .. }
                            if (*value - a_vals[i]).abs() < 1e-12
                        ),
                        "arg {i}: then_branch should be NumberLiteral({:.1}), got {:?}",
                        a_vals[i],
                        then_branch.kind
                    );
                    // else_branch must carry the b-side value
                    assert!(
                        matches!(
                            &else_branch.kind,
                            reify_ast::ExprKind::NumberLiteral { value, .. }
                            if (*value - b_vals[i]).abs() < 1e-12
                        ),
                        "arg {i}: else_branch should be NumberLiteral({:.1}), got {:?}",
                        b_vals[i],
                        else_branch.kind
                    );
                }
                other => panic!("arg {i}: expected Conditional, got {:?}", other),
            }
        }
    }

    /// `merge_branches`: box vs cylinder (different names) → scalar Conditional (not hoistable).
    #[test]
    fn merge_branches_box_vs_cylinder_produces_scalar_conditional() {
        let functions: Vec<CompiledFunction> = vec![];
        let cond = bool_cond_expr();
        let a = make_call_with_arity("box", 3);
        let b = make_call_with_arity("cylinder", 2);
        let outer_span = reify_core::SourceSpan::new(0, 10);

        let merged = merge_branches(&cond, &a, &b, &functions, outer_span);

        assert!(
            matches!(&merged.kind, reify_ast::ExprKind::Conditional { .. }),
            "box vs cylinder should produce a scalar Conditional, got {:?}",
            merged.kind
        );
    }

    /// `merge_branches`: box(1,1,1) vs box(1,1) (arity mismatch) → scalar Conditional.
    #[test]
    fn merge_branches_box_arity_mismatch_produces_scalar_conditional() {
        let functions: Vec<CompiledFunction> = vec![];
        let cond = bool_cond_expr();
        let a = make_call_with_arity("box", 3);
        let b = make_call_with_arity("box", 2); // unusual but possible
        let outer_span = reify_core::SourceSpan::new(0, 10);

        let merged = merge_branches(&cond, &a, &b, &functions, outer_span);

        assert!(
            matches!(&merged.kind, reify_ast::ExprKind::Conditional { .. }),
            "box arity mismatch should produce a scalar Conditional, got {:?}",
            merged.kind
        );
    }

    /// `try_hoist_geometry_conditional`: box-vs-box returns Some(box_call).
    #[test]
    fn try_hoist_geometry_conditional_box_vs_box_returns_some() {
        let functions: Vec<CompiledFunction> = vec![];
        let cond = bool_cond_expr();
        let a = make_call_with_arity("box", 3);
        let b = make_call_with_arity("box", 3);
        let outer_span = reify_core::SourceSpan::new(0, 20);
        let cond_expr = reify_ast::Expr {
            kind: reify_ast::ExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(a),
                else_branch: Box::new(b),
            },
            span: outer_span,
        };

        let result = try_hoist_geometry_conditional(&cond_expr, &functions);
        assert!(
            result.is_some(),
            "box-vs-box should hoist: expected Some(...), got None"
        );
        let hoisted = result.unwrap();
        assert!(
            matches!(
                &hoisted.kind,
                reify_ast::ExprKind::FunctionCall { name, .. } if name == "box"
            ),
            "hoisted expr should be FunctionCall{{\"box\", ...}}, got {:?}",
            hoisted.kind
        );
    }

    /// `try_hoist_geometry_conditional`: box-vs-cylinder returns None.
    #[test]
    fn try_hoist_geometry_conditional_box_vs_cylinder_returns_none() {
        let functions: Vec<CompiledFunction> = vec![];
        let cond = bool_cond_expr();
        let a = make_call_with_arity("box", 3);
        let b = make_call_with_arity("cylinder", 2);
        let outer_span = reify_core::SourceSpan::new(0, 20);
        let cond_expr = reify_ast::Expr {
            kind: reify_ast::ExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(a),
                else_branch: Box::new(b),
            },
            span: outer_span,
        };

        let result = try_hoist_geometry_conditional(&cond_expr, &functions);
        assert!(
            result.is_none(),
            "box-vs-cylinder should NOT hoist: expected None, got {:?}",
            result.map(|e| format!("{:?}", e.kind))
        );
    }

    /// `try_hoist_geometry_conditional`: non-Conditional input returns None.
    #[test]
    fn try_hoist_geometry_conditional_non_conditional_returns_none() {
        let functions: Vec<CompiledFunction> = vec![];
        let box_expr = make_call_with_arity("box", 3);
        let result = try_hoist_geometry_conditional(&box_expr, &functions);
        assert!(
            result.is_none(),
            "non-Conditional should return None, got {:?}",
            result.map(|e| format!("{:?}", e.kind))
        );
    }

    // ─── task-3815 step-3: recursive union + else-if reduction unit tests ────

    /// `merge_branches` recursion: union(box,box) vs union(box,box) (distinct arg values)
    /// produces `union(box(C,C,C), box(C,C,C))` — a geometry FunctionCall with box sub-calls.
    #[test]
    fn merge_branches_union_tree_produces_geometry_call_with_conditional_box_args() {
        let functions: Vec<CompiledFunction> = vec![];
        let cond = bool_cond_expr();
        // a = union(box(1,1,1), box(1,1,1))  — "then" tree
        let a = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "union".to_string(),
                args: vec![
                    make_box_with_values(1.0, 1.0, 1.0),
                    make_box_with_values(1.0, 1.0, 1.0),
                ],
                arg_names: vec![None, None],
            },
            span: reify_core::SourceSpan::new(0, 1),
        };
        // b = union(box(2,2,2), box(2,2,2))  — "else" tree (distinct values, args differ)
        let b = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "union".to_string(),
                args: vec![
                    make_box_with_values(2.0, 2.0, 2.0),
                    make_box_with_values(2.0, 2.0, 2.0),
                ],
                arg_names: vec![None, None],
            },
            span: reify_core::SourceSpan::new(0, 1),
        };
        let outer_span = reify_core::SourceSpan::new(0, 20);

        let merged = merge_branches(&cond, &a, &b, &functions, outer_span);

        // Merged root should be union(...)
        let (name, args) = match &merged.kind {
            reify_ast::ExprKind::FunctionCall { name, args, .. } => (name, args),
            other => panic!("expected FunctionCall, got {:?}", other),
        };
        assert_eq!(name, "union");
        assert_eq!(args.len(), 2, "union should have 2 args");

        // Each sub-arg should be box(C,C,C)
        for (i, sub) in args.iter().enumerate() {
            let sub_args = match &sub.kind {
                reify_ast::ExprKind::FunctionCall { name, args, .. } => {
                    assert_eq!(name, "box", "sub-arg {} should be box", i);
                    args
                }
                other => panic!("sub-arg {}: expected FunctionCall{{box}}, got {:?}", i, other),
            };
            assert_eq!(sub_args.len(), 3);
            for sub_arg in sub_args {
                assert!(
                    matches!(&sub_arg.kind, reify_ast::ExprKind::Conditional { .. }),
                    "sub-arg {}: box arg should be Conditional, got {:?}",
                    i,
                    sub_arg.kind
                );
            }
        }
    }

    /// `try_hoist_geometry_conditional` (step-4): else-if chain
    /// `if c1 then box(p) else (if c2 then box(q) else box(r))` reduces to
    /// `box(nested_Conditional, ...)` via else-if chain reduction.
    ///
    /// Uses distinct arg values (p=10, q=20, r=30) so the peephole does not
    /// short-circuit the Conditional wrapping and the nested structure is visible.
    #[test]
    fn try_hoist_geometry_conditional_else_if_chain_returns_some() {
        let functions: Vec<CompiledFunction> = vec![];
        let cond1 = bool_cond_expr();
        let cond2 = bool_cond_expr();
        // Distinct dims so are_scalar_equal does not fire and Conditionals are emitted.
        let box_p = make_box_with_values(10.0, 10.0, 10.0);
        let box_q = make_box_with_values(20.0, 20.0, 20.0);
        let box_r = make_box_with_values(30.0, 30.0, 30.0);

        // else_branch is itself: `if cond2 then box(q) else box(r)`
        let inner_cond = reify_ast::Expr {
            kind: reify_ast::ExprKind::Conditional {
                condition: Box::new(cond2.clone()),
                then_branch: Box::new(box_q),
                else_branch: Box::new(box_r),
            },
            span: reify_core::SourceSpan::new(0, 10),
        };

        let outer_span = reify_core::SourceSpan::new(0, 20);

        // try_hoist on `if cond1 then box(p) else (if cond2 then box(q) else box(r))`
        let cond_expr = reify_ast::Expr {
            kind: reify_ast::ExprKind::Conditional {
                condition: Box::new(cond1.clone()),
                then_branch: Box::new(box_p),
                else_branch: Box::new(inner_cond),
            },
            span: outer_span,
        };

        // Step-4: else-if chain reduction makes this hoistable → Some(box with nested Conditional args).
        let result = try_hoist_geometry_conditional(&cond_expr, &functions);
        assert!(
            result.is_some(),
            "else-if chain should hoist after step-4: expected Some(box(...)), got None"
        );
        let hoisted = result.unwrap();
        match &hoisted.kind {
            reify_ast::ExprKind::FunctionCall { name, args, .. } => {
                assert_eq!(name, "box", "hoisted should be box");
                assert_eq!(args.len(), 3);
                for arg in args {
                    // Each arg should be a (potentially nested) Conditional.
                    assert!(
                        matches!(&arg.kind, reify_ast::ExprKind::Conditional { .. }),
                        "else-if chain: box arg should be Conditional, got {:?}",
                        arg.kind
                    );
                }
            }
            other => panic!("expected FunctionCall{{box}}, got {:?}", other),
        }
    }

    // ─── task-3815 amendments: peephole, shadow guard, geometry-arg mismatch ──

    /// `are_scalar_equal` peephole: merge_branches with identical NumberLiteral
    /// args returns the literal directly (no Conditional wrapper).
    ///
    /// Exercises the common case where `translate(box(…), tx, 0, 0)` appears in
    /// both branches and the shared-constant `0` axis args are NOT wrapped.
    #[test]
    fn merge_branches_identical_number_literal_args_not_wrapped_in_conditional() {
        let functions: Vec<CompiledFunction> = vec![];
        let cond = bool_cond_expr();
        let zero_a = reify_ast::Expr {
            kind: reify_ast::ExprKind::NumberLiteral {
                value: 0.0,
                is_real: false,
            },
            span: reify_core::SourceSpan::new(0, 1),
        };
        let zero_b = reify_ast::Expr {
            kind: reify_ast::ExprKind::NumberLiteral {
                value: 0.0,
                is_real: false,
            },
            span: reify_core::SourceSpan::new(0, 1),
        };
        let outer_span = reify_core::SourceSpan::new(0, 10);

        let merged = merge_branches(&cond, &zero_a, &zero_b, &functions, outer_span);

        // Peephole must return the literal directly, not a Conditional.
        assert!(
            matches!(
                &merged.kind,
                reify_ast::ExprKind::NumberLiteral { value, .. }
                if (*value - 0.0_f64).abs() < 1e-12
            ),
            "identical 0.0 args should be returned as-is (no Conditional), got {:?}",
            merged.kind
        );
    }

    /// Shadow-guard test: `try_hoist_geometry_conditional` must return `None`
    /// when the constructor name is shadowed by a user-defined function in the
    /// `functions` slice — even when both branches are structurally identical
    /// geometry calls.
    ///
    /// This pin ensures the auto-hoisting rewrite is never applied to user
    /// functions that happen to share a name with a built-in geometry primitive.
    #[test]
    fn try_hoist_geometry_conditional_returns_none_when_box_is_user_shadowed() {
        // Simulate `fn box(w, h, d) { … }` in user code.
        let params = vec![
            ("w".to_string(), reify_core::Type::dimensionless_scalar()),
            ("h".to_string(), reify_core::Type::dimensionless_scalar()),
            ("d".to_string(), reify_core::Type::dimensionless_scalar()),
        ];
        let box_shadow_fn = CompiledFunction {
            name: "box".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
            return_type: reify_core::Type::dimensionless_scalar(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr {
                    kind: CompiledExprKind::Literal(Value::Real(0.0)),
                    result_type: reify_core::Type::dimensionless_scalar(),
                    content_hash: ContentHash::of_str("box_shadow_hoist_stub"),
                },
            },
            content_hash: ContentHash::of_str("fn_box_shadow_hoist"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };
        let functions = vec![box_shadow_fn];

        let cond = bool_cond_expr();
        // Both branches are box(…) with distinct values — would hoist with empty functions.
        let a = make_box_with_values(1.0, 2.0, 3.0);
        let b = make_box_with_values(4.0, 5.0, 6.0);
        let outer_span = reify_core::SourceSpan::new(0, 20);
        let cond_expr = reify_ast::Expr {
            kind: reify_ast::ExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(a),
                else_branch: Box::new(b),
            },
            span: outer_span,
        };

        let result = try_hoist_geometry_conditional(&cond_expr, &functions);
        assert!(
            result.is_none(),
            "user-shadowed 'box' must NOT be auto-hoisted: expected None, got Some({:?})",
            result.map(|e| format!("{:?}", e.kind))
        );
    }

    /// Shadow-guard test: `merge_branches` with a user-shadowed constructor name
    /// must return a scalar `Conditional` (not a `FunctionCall`) — the shadow
    /// guard inside `merge_branches` prevents hoisting.
    #[test]
    fn merge_branches_returns_scalar_conditional_when_box_is_user_shadowed() {
        let params = vec![
            ("w".to_string(), reify_core::Type::dimensionless_scalar()),
            ("h".to_string(), reify_core::Type::dimensionless_scalar()),
            ("d".to_string(), reify_core::Type::dimensionless_scalar()),
        ];
        let box_shadow_fn = CompiledFunction {
            name: "box".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
            return_type: reify_core::Type::dimensionless_scalar(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr {
                    kind: CompiledExprKind::Literal(Value::Real(0.0)),
                    result_type: reify_core::Type::dimensionless_scalar(),
                    content_hash: ContentHash::of_str("box_shadow_merge_stub"),
                },
            },
            content_hash: ContentHash::of_str("fn_box_shadow_merge"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };
        let functions = vec![box_shadow_fn];

        let cond = bool_cond_expr();
        let a = make_box_with_values(1.0, 2.0, 3.0);
        let b = make_box_with_values(4.0, 5.0, 6.0);
        let outer_span = reify_core::SourceSpan::new(0, 10);

        let merged = merge_branches(&cond, &a, &b, &functions, outer_span);

        // Must be a scalar Conditional, NOT a FunctionCall.
        assert!(
            matches!(&merged.kind, reify_ast::ExprKind::Conditional { .. }),
            "user-shadowed 'box': merge_branches should return scalar Conditional, got {:?}",
            merged.kind
        );
    }

    /// Regression: `compile_geometry_call` on
    /// `if cond then translate(box(…),…) else translate(cyl(…),…)` must emit a
    /// clean "if-then-else" + "geometry" Error and return `None`.
    ///
    /// Path: `merge_branches` produces `translate(Conditional{cond,box,cyl},…)`;
    /// `try_hoist` returns `Some` (outer `translate` root matches); re-entering
    /// `compile_geometry_call` on the synthesised `translate` then encounters the
    /// inner `Conditional{cond,box,cyl}` as a geometry arg, which fires the
    /// existing graceful-error path (try_hoist returns None for box vs cyl).
    #[test]
    fn compile_geometry_call_translate_with_mismatched_inner_geometry_emits_error() {
        let zero = || reify_ast::Expr {
            kind: reify_ast::ExprKind::NumberLiteral {
                value: 0.0,
                is_real: false,
            },
            span: reify_core::SourceSpan::new(0, 1),
        };
        let one = || reify_ast::Expr {
            kind: reify_ast::ExprKind::NumberLiteral {
                value: 1.0,
                is_real: false,
            },
            span: reify_core::SourceSpan::new(0, 1),
        };
        // translate(box(1,1,1), 1, 0, 0)
        let translate_box = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "translate".to_string(),
                args: vec![make_box_with_values(1.0, 1.0, 1.0), one(), zero(), zero()],
                arg_names: vec![None, None, None, None],
            },
            span: reify_core::SourceSpan::new(0, 1),
        };
        // translate(cylinder(1,1), 1, 0, 0)
        let translate_cyl = reify_ast::Expr {
            kind: reify_ast::ExprKind::FunctionCall {
                name: "translate".to_string(),
                args: vec![make_call_with_arity("cylinder", 2), one(), zero(), zero()],
                arg_names: vec![None, None, None, None],
            },
            span: reify_core::SourceSpan::new(0, 1),
        };
        let bool_cond = reify_ast::Expr {
            kind: reify_ast::ExprKind::BoolLiteral(true),
            span: reify_core::SourceSpan::new(0, 4),
        };
        let cond_expr = make_conditional(bool_cond, translate_box, translate_cyl);

        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &cond_expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "expected None for mismatched inner geometry (translate(box) vs translate(cyl)), got ops"
        );
        let error_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Error)
            .collect();
        assert!(
            !error_diags.is_empty(),
            "expected at least one Error diagnostic for inner geometry mismatch"
        );
        let msg = &error_diags[0].message;
        assert!(
            msg.contains("if-then-else") && msg.contains("geometry"),
            "Error message must contain 'if-then-else' and 'geometry', got: {:?}",
            msg
        );
        assert!(
            !error_diags[0].labels.is_empty(),
            "Error diagnostic must have a label at the conditional span"
        );
    }

    // --- cone() compiler dispatch (task-4156, step-5 RED) ---

    /// `cone(10mm, 5mm, 20mm)` must compile to a single
    /// `CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cone }` with three
    /// named args: `bottom_radius`, `top_radius`, `height`.
    ///
    /// RED until step-6 adds the "cone" arm in `compile_geometry_call` and
    /// adds `PrimitiveKind::Cone` to `types.rs`.
    #[test]
    fn compile_geometry_call_cone_3args_returns_primitive_cone() {
        let expr = make_call_with_arity("cone", 3);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        let ops = result.expect("cone(_, _, _) should produce ops");
        assert_eq!(ops.len(), 1, "cone must produce exactly 1 op");
        match &ops[0] {
            CompiledGeometryOp::Primitive { kind, args } => {
                assert_eq!(
                    *kind,
                    PrimitiveKind::Cone,
                    "cone op must have kind PrimitiveKind::Cone"
                );
                let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(
                    names,
                    vec!["bottom_radius", "top_radius", "height"],
                    "cone arg names must be bottom_radius / top_radius / height"
                );
            }
            other => panic!("expected Primitive(Cone), got {:?}", other),
        }
        assert!(diagnostics.is_empty(), "cone(_, _, _) must emit no diagnostics");
    }

    /// `cone(10mm, 5mm)` (2 args) must emit an arity diagnostic and return `None`.
    ///
    /// RED until step-6 adds the "cone" arm in `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_cone_wrong_arity_emits_diagnostic() {
        let expr = make_call_with_arity("cone", 2);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "cone with 2 args must return None (arity error)"
        );
        assert!(
            !diagnostics.is_empty(),
            "cone with 2 args must emit at least one diagnostic"
        );
    }

    // --- wedge() compiler dispatch (task-4158, step-7 RED) ---

    /// `wedge(20mm,10mm,15mm,5mm)` must compile to a single
    /// `CompiledGeometryOp::Primitive { kind: PrimitiveKind::Wedge }` with four
    /// named args: `width`, `depth`, `height`, `top_width`.
    ///
    /// RED until step-8 adds the "wedge" arm in `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_wedge_4args_returns_primitive_wedge() {
        let expr = make_call_with_arity("wedge", 4);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        let ops = result.expect("wedge(_, _, _, _) should produce ops");
        assert_eq!(ops.len(), 1, "wedge must produce exactly 1 op");
        match &ops[0] {
            CompiledGeometryOp::Primitive { kind, args } => {
                assert_eq!(
                    *kind,
                    PrimitiveKind::Wedge,
                    "wedge op must have kind PrimitiveKind::Wedge"
                );
                let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(
                    names,
                    vec!["width", "depth", "height", "top_width"],
                    "wedge arg names must be width / depth / height / top_width"
                );
            }
            other => panic!("expected Primitive(Wedge), got {:?}", other),
        }
        assert!(diagnostics.is_empty(), "wedge(_, _, _, _) must emit no diagnostics");
    }

    /// `wedge(10mm, 5mm, 20mm)` (3 args) must emit an arity diagnostic and return `None`.
    ///
    /// RED until step-8 adds the "wedge" arm in `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_wedge_wrong_arity_emits_diagnostic() {
        let expr = make_call_with_arity("wedge", 3);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "wedge with 3 args must return None (arity error)"
        );
        assert!(
            !diagnostics.is_empty(),
            "wedge with 3 args must emit at least one diagnostic"
        );
    }

    // --- rectangle() compiler dispatch (task-4160, step-5 RED) ---

    /// `rectangle(20mm, 10mm)` (2 args) must compile to a single
    /// `CompiledGeometryOp::Profile { kind: ProfileKind::Rectangle }` with two
    /// named args: `width` and `height`.
    ///
    /// RED until step-6 adds ProfileKind, CompiledGeometryOp::Profile, and the
    /// "rectangle" arm in `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_rectangle_2args_returns_profile_rectangle() {
        let expr = make_call_with_arity("rectangle", 2);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        let ops = result.expect("rectangle(_, _) should produce ops");
        assert_eq!(ops.len(), 1, "rectangle must produce exactly 1 op");
        match &ops[0] {
            CompiledGeometryOp::Profile { kind, args } => {
                assert_eq!(
                    *kind,
                    ProfileKind::Rectangle,
                    "rectangle op must have kind ProfileKind::Rectangle"
                );
                let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(
                    names,
                    vec!["width", "height"],
                    "rectangle arg names must be width / height"
                );
            }
            other => panic!("expected Profile(Rectangle), got {:?}", other),
        }
        assert!(diagnostics.is_empty(), "rectangle(_, _) must emit no diagnostics");
    }

    /// `rectangle(10mm)` (1 arg) must emit an arity diagnostic and return `None`.
    ///
    /// RED until step-6 adds the "rectangle" arm in `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_rectangle_wrong_arity_emits_diagnostic() {
        let expr = make_call_with_arity("rectangle", 1);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "rectangle with 1 arg must return None (arity error)"
        );
        assert!(
            !diagnostics.is_empty(),
            "rectangle with 1 arg must emit at least one diagnostic"
        );
    }

    // --- circle() compiler dispatch (task-4160, step-5 RED) ---

    /// `circle(8mm)` (1 arg) must compile to a single
    /// `CompiledGeometryOp::Profile { kind: ProfileKind::Circle }` with one
    /// named arg: `radius`.
    ///
    /// RED until step-6 adds ProfileKind, CompiledGeometryOp::Profile, and the
    /// "circle" arm in `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_circle_1arg_returns_profile_circle() {
        let expr = make_call_with_arity("circle", 1);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        let ops = result.expect("circle(_) should produce ops");
        assert_eq!(ops.len(), 1, "circle must produce exactly 1 op");
        match &ops[0] {
            CompiledGeometryOp::Profile { kind, args } => {
                assert_eq!(
                    *kind,
                    ProfileKind::Circle,
                    "circle op must have kind ProfileKind::Circle"
                );
                let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["radius"], "circle arg name must be radius");
            }
            other => panic!("expected Profile(Circle), got {:?}", other),
        }
        assert!(diagnostics.is_empty(), "circle(_) must emit no diagnostics");
    }

    /// `circle()` (0 args) must emit an arity diagnostic and return `None`.
    ///
    /// RED until step-6 adds the "circle" arm in `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_circle_wrong_arity_emits_diagnostic() {
        let expr = make_call_with_arity("circle", 0);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "circle with 0 args must return None (arity error)"
        );
        assert!(
            !diagnostics.is_empty(),
            "circle with 0 args must emit at least one diagnostic"
        );
    }

    // --- polygon() compiler dispatch (task-4161, step-5 RED) ---

    /// `polygon(0mm,0mm, 10mm,0mm, 10mm,10mm)` (6 args — 3 pairs) must compile to
    /// a single `CompiledGeometryOp::Profile { kind: ProfileKind::Polygon }` with
    /// positional args named `c0..c5`.
    ///
    /// RED until step-6 adds `ProfileKind::Polygon` and the `"polygon"` arm in
    /// `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_polygon_6args_returns_profile_polygon() {
        let expr = make_call_with_arity("polygon", 6);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        let ops = result.expect("polygon with 6 args should produce ops");
        assert_eq!(ops.len(), 1, "polygon must produce exactly 1 op");
        match &ops[0] {
            CompiledGeometryOp::Profile { kind, args } => {
                assert_eq!(
                    *kind,
                    ProfileKind::Polygon,
                    "polygon op must have kind ProfileKind::Polygon"
                );
                let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(
                    names,
                    vec!["c0", "c1", "c2", "c3", "c4", "c5"],
                    "polygon arg names must be c0..c5 for 6 positional args"
                );
            }
            other => panic!("expected Profile(Polygon), got {:?}", other),
        }
        assert!(diagnostics.is_empty(), "polygon with 6 args must emit no diagnostics");
    }

    /// `polygon(0mm,0mm, 10mm,0mm, 10mm)` (5 args — odd, not multiple of 2) must
    /// emit an arg-count diagnostic and return `None`.
    ///
    /// RED until step-6 adds the `"polygon"` arm in `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_polygon_odd_arity_emits_diagnostic() {
        let expr = make_call_with_arity("polygon", 5);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "polygon with 5 args (odd) must return None (arity error)"
        );
        assert!(
            !diagnostics.is_empty(),
            "polygon with 5 args (odd) must emit at least one diagnostic"
        );
    }

    /// `polygon(0mm,0mm, 10mm,0mm)` (4 args — only 2 pairs, < 3 required) must
    /// emit an arg-count diagnostic and return `None`.
    ///
    /// RED until step-6 adds the `"polygon"` arm in `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_polygon_too_few_args_emits_diagnostic() {
        let expr = make_call_with_arity("polygon", 4);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "polygon with 4 args (< 6 = 3 pairs min) must return None (arity error)"
        );
        assert!(
            !diagnostics.is_empty(),
            "polygon with 4 args must emit at least one diagnostic"
        );
    }

    // --- ellipse() compiler dispatch (task-4161, step-5 RED) ---

    /// `ellipse(10mm, 5mm)` (2 args) must compile to a single
    /// `CompiledGeometryOp::Profile { kind: ProfileKind::Ellipse }` with named
    /// args `semi_major` and `semi_minor`.
    ///
    /// RED until step-6 adds `ProfileKind::Ellipse` and the `"ellipse"` arm in
    /// `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_ellipse_2args_returns_profile_ellipse() {
        let expr = make_call_with_arity("ellipse", 2);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        let ops = result.expect("ellipse(_, _) should produce ops");
        assert_eq!(ops.len(), 1, "ellipse must produce exactly 1 op");
        match &ops[0] {
            CompiledGeometryOp::Profile { kind, args } => {
                assert_eq!(
                    *kind,
                    ProfileKind::Ellipse,
                    "ellipse op must have kind ProfileKind::Ellipse"
                );
                let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(
                    names,
                    vec!["semi_major", "semi_minor"],
                    "ellipse arg names must be semi_major / semi_minor"
                );
            }
            other => panic!("expected Profile(Ellipse), got {:?}", other),
        }
        assert!(diagnostics.is_empty(), "ellipse(_, _) must emit no diagnostics");
    }

    /// `ellipse(10mm)` (1 arg) must emit an arity diagnostic and return `None`.
    ///
    /// RED until step-6 adds the `"ellipse"` arm in `compile_geometry_call`.
    #[test]
    fn compile_geometry_call_ellipse_wrong_arity_emits_diagnostic() {
        let expr = make_call_with_arity("ellipse", 1);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "ellipse with 1 arg must return None (arity error)"
        );
        assert!(
            !diagnostics.is_empty(),
            "ellipse with 1 arg must emit at least one diagnostic"
        );
    }

    // --- nurbs_surface() compiler dispatch (task #4191, step-5 RED) ---

    /// `nurbs_surface(cp, w, uk, vk, ud, vd)` (6 args) must compile to a single
    /// `CompiledGeometryOp::Surface { kind: SurfaceKind::Nurbs, args }` whose
    /// named args are exactly [control_points, weights, u_knots, v_knots,
    /// u_degree, v_degree] in order.
    ///
    /// RED until step-6 adds SurfaceKind, CompiledGeometryOp::Surface, and the
    /// "nurbs_surface" dispatch arm.
    #[test]
    fn compile_geometry_call_nurbs_surface_6args_returns_surface_nurbs() {
        let expr = make_call_with_arity("nurbs_surface", 6);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        let ops = result.expect("nurbs_surface(_, _, _, _, _, _) should produce ops");
        assert_eq!(ops.len(), 1, "nurbs_surface must produce exactly 1 op");
        match &ops[0] {
            CompiledGeometryOp::Surface { kind, args } => {
                assert_eq!(
                    *kind,
                    SurfaceKind::Nurbs,
                    "nurbs_surface op must have kind SurfaceKind::Nurbs"
                );
                let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(
                    names,
                    vec!["control_points", "weights", "u_knots", "v_knots", "u_degree", "v_degree"],
                    "nurbs_surface arg names must be control_points / weights / u_knots / v_knots / u_degree / v_degree"
                );
            }
            other => panic!("expected Surface(Nurbs), got {:?}", other),
        }
        assert!(diagnostics.is_empty(), "nurbs_surface(6 args) must emit no diagnostics");
    }

    /// `nurbs_surface(cp, w, uk, vk, ud)` (5 args, wrong arity) must emit an
    /// error-severity diagnostic and return None.
    ///
    /// RED until step-6 adds the "nurbs_surface" dispatch arm.
    #[test]
    fn compile_geometry_call_nurbs_surface_wrong_arity_emits_diagnostic() {
        let expr = make_call_with_arity("nurbs_surface", 5);
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_ir::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_ast::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "nurbs_surface with 5 args must return None (arity error)"
        );
        assert!(
            !diagnostics.is_empty(),
            "nurbs_surface with 5 args must emit at least one diagnostic"
        );
    }
}
