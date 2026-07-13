use super::*;
use reify_ir::EnumDef;

/// Returns `true` if `ty` is a scalar-like leaf type eligible as the `Q`
/// (quantity) side of Rules 2a/2b/2c.
///
/// The spec's "Q is a single value" framing covers the following leaf kinds:
/// plain primitives (`Bool`, `Int`, `Real`, `String`), dimensioned scalars
/// (`Scalar`), enumerations (`Enum`), type parameters (`TypeParam`),
/// user-defined structure references (`StructureRef`), trait objects
/// (`TraitObject`), and the geometry sentinel (`Geometry`).
///
/// Compound/aggregate types (`Vector`, `Tensor`, `Matrix`, `Point`, `List`,
/// `Set`, `Map`, `Option`, `Complex`, `Field`, `Range`, `Function`, `Frame`,
/// `Transform`, `Plane`, `Orientation`, `Axis`, `BoundingBox`) are NOT leaf
/// types and return `false`.
///
/// **Spec reference:** `docs/reify-language-spec.md` §3.3.1 (lines 295–329).
/// Specifically:
/// - Lines 298–301: `Scalar<Q: Dimension>` is defined as an independent rank-0
///   type without spatial dimensionality. Q must be a "Dimension" — a single
///   dimensioned value, not a compound/aggregate carrier of shape.
/// - Line 305: "**Tensor conversion:** `Scalar<Q>` converts implicitly to
///   `Tensor<0, N, Q>` for any `N`, and vice versa." This is the basis of
///   Rules 2a/2b; it only holds when Q is a leaf Dimension type.
/// - Lines 317–320: restates the alias relationship and notes
///   `Vector<N,Q> = Tensor<1,N,Q>`.
///
/// This allowlist (not a denylist) is intentional: future `Type` variants
/// default to *rejected* rather than default-admitted, forcing each new
/// variant to be explicitly evaluated against the spec's "Q is a Dimension /
/// single value" criterion before being added here.
///
/// `Type::Error` is excluded: the anti-cascade guard at the top of
/// `implicitly_converts_to` short-circuits before any leaf check is reached,
/// so Error inputs never arrive here.
fn is_scalar_like_leaf(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Bool
            | Type::Int
            | Type::String
            | Type::Scalar { .. }
            | Type::Enum(_)
            | Type::TypeParam(_)
            | Type::StructureRef(_)
            | Type::TraitObject(_)
            | Type::Geometry
    )
}

pub fn implicitly_converts_to(from: &Type, to: &Type) -> bool {
    // Anti-cascade guard — asymmetric error-wildcard contract (task-448 / task-1918).
    //
    // PRODUCER side (`from.is_error()`): legitimate anti-cascade path. When the
    // producing expression already emitted a diagnostic, its Type::Error sentinel
    // must be accepted everywhere to suppress follow-on "type mismatch" reports at
    // downstream call sites (trait conformance, function-argument checks).
    //
    // CONSUMER side (`to.is_error()`): declared annotations are resolved via
    // `resolve_type_with_aliases`, which always falls back to a concrete type
    // (e.g. Type::dimensionless_scalar(), Type::StructureRef) — Type::Error never legitimately
    // appears as the expected/declared type. The debug_assert below catches any
    // call site that accidentally passes Error as `to` (a bug, not a cascade).
    // In release builds the short-circuit preserves cascade safety as a
    // belt-and-braces fallback (task-448 rationale).
    debug_assert!(
        !to.is_error(),
        "Type::Error must not appear on the consumer/target side of implicitly_converts_to \
         — declared annotations never resolve to the poison sentinel; \
         this indicates a bug at the call site (task-1918)"
    );
    if from.is_error() || to.is_error() {
        return true;
    }

    // Identity: same type always converts to itself.
    if from == to {
        return true;
    }

    match (from, to) {
        // Rule 1a: Vector<N,Q> -> Tensor<1,N,Q>
        (
            Type::Vector {
                n: vn,
                quantity: vq,
            },
            Type::Tensor {
                rank: 1,
                n: tn,
                quantity: tq,
            },
        ) => vn == tn && vq == tq,

        // Rule 1b: Tensor<1,N,Q> -> Vector<N,Q>
        (
            Type::Tensor {
                rank: 1,
                n: tn,
                quantity: tq,
            },
            Type::Vector {
                n: vn,
                quantity: vq,
            },
        ) => tn == vn && tq == vq,

        // Rule 2c: Tensor<0,M,Q> -> Tensor<0,N,Q>  (same Q, any N — N irrelevant for rank-0)
        //
        // Spec rationale: rank-0 tensors are semantically scalar-like; their N dimension
        // carries no indexable information. By transitivity of Rules 2a/2b, if both
        // `Q → Tensor<0,M,Q>` and `Q → Tensor<0,N,Q>` hold, direct
        // `Tensor<0,M,Q> → Tensor<0,N,Q>` must also hold. Without this rule a trait
        // requiring `Tensor<0,5,Q>` would reject a structure providing `Tensor<0,3,Q>`
        // despite them being semantically identical.
        //
        // Guard: `is_scalar_like_leaf(q1)` mirrors the leaf-Q guard on Rules 2a/2b.
        // The transitivity argument only holds when Rules 2a/2b themselves fire (i.e.
        // when Q is a scalar-like leaf). Compound-Q pairs (e.g. Vector, Point) are
        // rejected consistently with Rules 2a/2b. Checking q1 alone is sufficient;
        // `q1 == q2` implies q2 is the same leaf.
        (
            Type::Tensor {
                rank: 0,
                quantity: q1,
                ..
            },
            Type::Tensor {
                rank: 0,
                quantity: q2,
                ..
            },
        ) if is_scalar_like_leaf(q1) => q1 == q2,

        // Rule 2a: Q -> Tensor<0,_,Q>  (N is irrelevant for rank-0)
        //
        // Guard: `from_ty` must be a scalar-like leaf type — see `is_scalar_like_leaf`.
        // Compound/aggregate types are excluded: the spec's "Q is a single value" framing
        // covers only leaf kinds (Bool, Int, Real, String, Scalar, Enum, TypeParam,
        // StructureRef, TraitObject, Geometry). Rule 2c (above) handles Tensor<0>↔Tensor<0>.
        (
            from_ty,
            Type::Tensor {
                rank: 0,
                quantity: tq,
                ..
            },
        ) if is_scalar_like_leaf(from_ty) => from_ty == tq.as_ref(),

        // Rule 2b: Tensor<0,_,Q> -> Q  (N is irrelevant for rank-0)
        //
        // Guard: `to_ty` must be a scalar-like leaf type — see `is_scalar_like_leaf`.
        (
            Type::Tensor {
                rank: 0,
                quantity: tq,
                ..
            },
            to_ty,
        ) if is_scalar_like_leaf(to_ty) => tq.as_ref() == to_ty,

        // Rule 3: Tensor<2,N,Q> -> Matrix<N,N,Q>  (one-way, square matrices only)
        // Note: Matrix->Tensor is NOT allowed; the default `false` arm handles that.
        (
            Type::Tensor {
                rank: 2,
                n: tn,
                quantity: tq,
            },
            Type::Matrix {
                m,
                n: mn,
                quantity: mq,
            },
        ) => tn == m && tn == mn && tq == mq,

        _ => false,
    }
}

/// Check if an argument type is compatible with a declared parameter/annotation type.
///
/// Returns `true` when `arg_ty` can be used where `param_ty` is declared, under
/// any of the following rules:
/// - **Identity**: `param_ty == arg_ty` (delegated to `implicitly_converts_to`).
/// - **Int→Real widening**: whole-number literals parse as `Int` and must be
///   accepted where `Real` is annotated (e.g. `let x : Real = 42` at
///   `conformance.rs:591`).
/// - **Bidirectional implicit conversions**: calls `implicitly_converts_to` in
///   **both** directions (`param→arg` and `arg→param`), so the explicitly
///   one-way Rule 3 (`Tensor<2,N,Q>→Matrix<N,N,Q>`) appears symmetric here.
///   This is intentional for trait-let-binding annotation checks
///   (`conformance.rs:591`), where either annotation direction must be accepted.
///
/// # When to use `implicitly_converts_to` directly
///
/// **Use `implicitly_converts_to` directly when direction matters:**
/// - Trait member conformance (`conformance.rs:384`): producer type must convert
///   *to* the trait's declared type — direction is fixed.
/// - Field composition (`functions.rs:289`): inner codomain must convert *to*
///   outer domain — direction is fixed.
///
/// Using `type_compatible` at those sites would silently accept
/// `Matrix<3,3,Q>→Tensor<2,3,Q>` even though Rule 3 is one-way.
///
/// # Error-wildcard contract (task-448 / task-1918)
///
/// `arg_ty.is_error()` is the **producer-side** anti-cascade path: when the
/// argument expression already emitted a diagnostic, its Type::Error sentinel
/// must be accepted to suppress follow-on "type mismatch" reports.
///
/// `param_ty.is_error()` **must never legitimately occur**: production call sites
/// pass types that originate from `resolve_type_with_aliases`, which always falls
/// back to a concrete type (e.g. `Type::dimensionless_scalar()`, `Type::StructureRef`) and never
/// returns `Type::Error`. The debug_assert below catches any future regression,
/// including the two recursive calls in the body below (both safe by the same
/// invariants). In release builds the short-circuit preserves cascade safety as
/// a belt-and-braces fallback (task-448 rationale).
pub fn type_compatible(param_ty: &Type, arg_ty: &Type) -> bool {
    // Producer-side anti-cascade guard (task-448 / task-1918): asymmetric contract.
    // See doc comment above for full rationale.
    debug_assert!(
        !param_ty.is_error(),
        "Type::Error must not appear on the param/expected side of type_compatible \
         — declared annotations never resolve to the poison sentinel; \
         this indicates a bug at the call site (task-1918)"
    );
    if param_ty.is_error() || arg_ty.is_error() {
        return true;
    }
    // Allow Int→dimensionless-scalar widening coercion
    if let (Type::Scalar { dimension }, Type::Int) = (param_ty, arg_ty)
        && dimension.is_dimensionless()
    {
        return true;
    }
    // PRD §4.4 (task 4117 β): Selector(_) arg coerces ONE-DIRECTIONALLY to a
    // List<Geometry> param. The rule is directional: a selector may be passed
    // where a List<Geometry> is declared, but a List<Geometry> must NOT satisfy
    // a Selector-typed param.
    //
    // This guard lives here (not in `implicitly_converts_to`) because
    // `type_compatible` calls `implicitly_converts_to` BIDIRECTIONALLY below;
    // adding it there would wrongly accept `List<Geometry>` at a Selector param
    // (mirrors the same design decision made for Tensor→Matrix, Rule 3 — also
    // one-directional and placed here rather than in `implicitly_converts_to`).
    //
    // NOTE: `Type::AnySelector` is intentionally excluded from this match.
    // Kind-agnostic selector params resolve to node-sets via task 4092 (a
    // kind-uniform path), NOT via List<Geometry> widening — so there is no
    // valid `(Type::List<Geometry>, Type::AnySelector)` coercion at present.
    // If a List<Geometry> path for agnostic selectors is ever needed, extend
    // the match deliberately to `Type::Selector(_) | Type::AnySelector`.
    if let (Type::List(inner), Type::Selector(_)) = (param_ty, arg_ty)
        && matches!(inner.as_ref(), Type::Geometry)
    {
        return true;
    }
    // PRD §4.2/§11.1 (task 4369/A2): a kind-agnostic `AnySelector` param accepts
    // any concrete selector argument (Face, Edge, Body — and Vertex once A1 lands),
    // ONE-DIRECTIONALLY. A single-kind `Selector(k)` param must NOT accept an
    // agnostic arg (see test step-3(e)).
    //
    // This guard lives here (not in `implicitly_converts_to`) for the same reason
    // as the List<Geometry> rule above: `type_compatible` calls
    // `implicitly_converts_to` in BOTH directions, so placing it there would also
    // accept the reverse (a concrete-kind param accepting an agnostic arg), which
    // would violate the one-directional PRD D3 requirement.
    //
    // Identity (AnySelector vs AnySelector) is already covered by
    // `implicitly_converts_to`'s `from == to` short-circuit below.
    if matches!((param_ty, arg_ty), (Type::AnySelector, Type::Selector(_))) {
        return true;
    }
    // Bidirectional implicit tensor/vector/matrix conversions
    if implicitly_converts_to(param_ty, arg_ty) || implicitly_converts_to(arg_ty, param_ty) {
        return true;
    }
    false
}

/// Enum-base compatibility for a (already type-arg-substituted) generic-enum
/// payload field type against a supplied, D1/F-Mono-erased enum value type
/// (task γ #4031, PRD §5 D3 conservative-skip / recursive-field tolerance).
///
/// Under erasure (§7.1: "resolved args live only in the per-site substitution
/// map, never on the persisted `Type` or `Value`"), a constructed
/// enum-variant value's `result_type` is ALWAYS the bare `Type::Enum(name)` —
/// it never carries type args, even when the enum is generic. So a
/// recursive/applied payload field — e.g. `left: Tree<T>`, declared
/// `Type::Applied { name: "Tree", args: [TypeParam("T")] }` and substituted to
/// a CONCRETE `Type::Applied { "Tree", [Length] }` by a pinned annotation —
/// supplied a constructed `Leaf { .. }` child (`result_type = Type::Enum("Tree")`)
/// would spuriously fail raw [`type_compatible`], which has no Applied-vs-Enum
/// rule.
///
/// Returns `true` when `declared` is enum-shaped (`Type::Enum(n)` or
/// `Type::Applied { name: n, .. }` where `n` names a declared enum in
/// `enum_defs`) and `supplied` is `Type::Enum(n)` of the SAME base name `n`.
/// A differing base name (a genuine cross-enum mismatch) returns `false`,
/// unchanged from today. Type-ARG agreement for enum-typed payload fields is
/// the job of the inference/pin passes in `compile_variant_construct`, not
/// this predicate — it only tolerates the erasure gap, it does not re-check
/// args.
///
/// Bare `Type::Enum(n)` vs `Type::Enum(n)` (a non-generic recursive enum) is
/// already handled by `type_compatible`'s identity short-circuit; this helper
/// only changes behavior for the `Type::Applied` case, which `type_compatible`
/// cannot otherwise satisfy.
///
/// **Why the `enum_defs` membership check.** `Type::Applied { name, .. }` is
/// not enum-exclusive — a generic user-defined structure reference (e.g.
/// `Coupling<T>`) is represented the same way (see
/// `type_arg_applied_resolution_tests.rs`). Without confirming `name` is
/// actually a declared enum, a substituted generic-STRUCT-typed payload field
/// sharing a base name with an unrelated enum (a narrow name-collision edge
/// case) would be spuriously accepted against a supplied `Type::Enum` value
/// of that name. Gating on `enum_defs` membership closes that gap; a struct
/// value's own `result_type` is a `StructureRef`/`Applied`, never
/// `Type::Enum`, so this only ever matters for the declared (LHS) side.
pub(crate) fn enum_payload_compatible(
    declared: &Type,
    supplied: &Type,
    enum_defs: &[EnumDef],
) -> bool {
    let declared_enum_name = match declared {
        Type::Enum(name) => Some(name.as_str()),
        Type::Applied { name, .. } if enum_defs.iter().any(|e| e.name == *name) => {
            Some(name.as_str())
        }
        _ => None,
    };
    match (declared_enum_name, supplied) {
        (Some(dn), Type::Enum(sn)) => dn == sn,
        _ => false,
    }
}

/// Check that a function-param default expression's type is compatible with the
/// declared parameter type.
///
/// **Policy: strict equality, not bidirectional `type_compatible`.**
///
/// The definition-site default-expression check must be at least as strict as
/// the call-site check so that a default cannot synthesize an argument that an
/// explicit call would refuse, creating a type-system inconsistency.  Strict
/// equality is correct here because a struct-ctor default (e.g. `ElasticOptions()`)
/// already produces exactly the param's `StructureRef` type — so the check
/// passes without any relaxation.
///
/// Note: `try_default_padding`'s PREFIX check (whether the provided args match
/// the leading params) uses the same trait/type-param wildcard predicate as
/// `resolve_function_overload` — it is NOT strict equality.  Only this
/// definition-site default-expression-vs-param-type check is strict.
///
/// **Anti-cascade guard.** If either type is `Type::Error` (poison sentinel from
/// a failed `compile_expr`), silently accept — the root-cause diagnostic was
/// already emitted. Mirrors the same short-circuit in `implicitly_converts_to`
/// and `type_compatible` (task-448 / task-1918 cascade-safety contract).
///
/// Note: `param_ty` is always a concrete resolved type (never `Type::Error`) in
/// production — `resolve_type_expr_with_aliases` always falls back to `Type::dimensionless_scalar()`
/// on failure. The `param_ty.is_error()` branch is therefore dead code in practice
/// but is included for symmetry and belt-and-braces safety.
pub(crate) fn fn_param_default_compatible(param_ty: &Type, default_ty: &Type) -> bool {
    if param_ty.is_error() || default_ty.is_error() {
        return true;
    }
    param_ty == default_ty
}

/// Result of attempting to resolve a function call against user-defined functions.
pub(crate) enum OverloadResolution<'a> {
    /// Exactly one user-defined function matches by name, arity, and exact param types.
    Resolved(&'a CompiledFunction),
    /// No user-defined function has this name at all — fall through to stdlib.
    NoUserFunctions,
    /// User-defined functions with this name exist, but none match the given arg types.
    /// Carries all same-name candidates for error reporting.
    NoMatch(Vec<&'a CompiledFunction>),
    /// Multiple user-defined functions match — ambiguous call.
    /// Carries all matching candidates for error reporting.
    Ambiguous(Vec<&'a CompiledFunction>),
}

/// Returns `true` when `t` is, or recursively wraps, a `Type::TraitObject`.
///
/// Covers bare `TraitObject(name)` and the four generic wrappers
/// `Option<T>`, `List<T>`, `Set<T>`, and `Map<K,V>`.  A `Map<TraitObject, V>`
/// or `Map<K, TraitObject>` is also trait-carrying because both positions
/// participate in conformance checking.
///
/// Used by `resolve_function_overload` to make trait-carrying params act as
/// resolution wildcards (match any arg type), while concrete params keep
/// exact-equality semantics.  Eval-builtins (bind/sweep/dim) have no `.ri`
/// signature → their `named` vec is empty → `NoUserFunctions` arm → unaffected.
pub(crate) fn type_carries_trait_object(t: &Type) -> bool {
    match t {
        Type::TraitObject(_) => true,
        Type::Option(inner) => type_carries_trait_object(inner),
        Type::List(inner) => type_carries_trait_object(inner),
        Type::Set(inner) => type_carries_trait_object(inner),
        Type::Map(key, val) => type_carries_trait_object(key) || type_carries_trait_object(val),
        // task 4602 β: Applied — recurse into type args; Projection — recurse into base.
        // Added explicitly (not compiler-forced) to stay verbatim-synced with
        // the reify-expr copy (esc-4231-120/126) and for §5 substrate correctness.
        Type::Applied { args, .. } => args.iter().any(type_carries_trait_object),
        Type::Projection { base, .. } => type_carries_trait_object(base),
        _ => false,
    }
}

/// Returns `true` when `t` is, or recursively wraps, a `Type::TypeParam`.
///
/// Recurses through the **same** inner-`Type`-bearing constructor set as
/// [`unify`] and [`crate::type_resolution::substitute_type_params`] —
/// `List`/`Set`/`Keyed`/`Option`/`Complex`/`Range`,
/// `Point`/`Vector`/`Tensor`/`Matrix` (quantity slot), `Map`, `Field`,
/// `Function` (params + return), and `Union` — so a generic param that embeds a
/// type-param inside ANY of those (e.g. `Field<T, Real>`, `List<Field<T>>`) is
/// recognized. Keeping this predicate aligned with the unify/substitute walks
/// avoids the asymmetry where overload resolution would reject a param shape
/// the downstream inference machinery can actually handle.
///
/// Used by `resolve_function_overload` to make a *generic* candidate's
/// type-param-carrying params act as resolution wildcards (match any arg type),
/// gated on `!f.type_params.is_empty()` so non-generic fns are completely
/// unaffected (INV-6, task 4231 β).
///
/// The `match` is intentionally exhaustive (no `_` wildcard) so a future `Type`
/// variant forces a compile-time decision here, in lock-step with the sibling
/// `unify` / `substitute_type_params` walks.
///
/// See also [`type_carries_dim_param`] for the sibling predicate that covers
/// dimension-kinded parameters (`Type::ScalarParam`). The two predicates are
/// kept separate because dimension params are a distinct kind (D7) — they are
/// NOT substituted by type-param logic. The overload-resolution wildcard ORs
/// them together at two sites.
pub(crate) fn type_carries_type_param(t: &Type) -> bool {
    match t {
        // The type-parameter leaf itself.
        Type::TypeParam(_) => true,

        // Single-inner-Type wrappers: recurse on the child.
        Type::List(inner)
        | Type::Set(inner)
        | Type::Keyed(inner)
        | Type::Option(inner)
        | Type::Complex(inner)
        | Type::Range(inner) => type_carries_type_param(inner),

        // Quantity-bearing aggregates: recurse into the quantity slot.
        Type::Point { quantity, .. }
        | Type::Vector { quantity, .. }
        | Type::Tensor { quantity, .. }
        | Type::Matrix { quantity, .. } => type_carries_type_param(quantity),

        // Two-inner-Type wrappers.
        Type::Map(key, val) => type_carries_type_param(key) || type_carries_type_param(val),
        Type::Field { domain, codomain } => {
            type_carries_type_param(domain) || type_carries_type_param(codomain)
        }

        // Function: any param, or the return type.
        Type::Function {
            params,
            return_type,
        } => params.iter().any(type_carries_type_param) || type_carries_type_param(return_type),

        // Union: any arm.
        Type::Union(arms) => arms.iter().any(type_carries_type_param),

        // task 4602 β: Applied — recurse into type args; Projection — recurse into base.
        Type::Applied { args, .. } => args.iter().any(type_carries_type_param),
        Type::Projection { base, .. } => type_carries_type_param(base),

        // All remaining leaves carry no inner `Type`.
        Type::Bool
        | Type::Int
        | Type::String
        | Type::Scalar { .. }
        | Type::Enum(_)
        | Type::StructureRef(_)
        | Type::TraitObject(_)
        | Type::Geometry
        // Feature identity token (task 4808 / P1 γ): inner-Type-free leaf.
        | Type::Feature
        | Type::Orientation(_)
        | Type::Frame(_)
        | Type::Transform(_)
        | Type::AffineMap(_)
        | Type::Plane
        | Type::Axis
        | Type::Direction
        // Relation directive (γ): an inner-Type-free leaf, carries no type param.
        | Type::Relation
        | Type::BoundingBox
        | Type::Selector(_)
        | Type::AnySelector
        // Dimension-param scalar: carries no *type* param; dimension binding is
        // handled by the dedicated `unify` ScalarParam arm (ζ / D8) and by
        // `type_carries_dim_param` — not by type-param substitution.
        | Type::ScalarParam(_)
        | Type::Error => false,
    }
}

/// Returns `true` when `t` (or any type nested within it) is a
/// `Type::TypeParam` whose name is a member of `conflicted`.
///
/// Sibling of [`type_carries_type_param`] — walks the identical
/// inner-Type-bearing constructor set (List/Set/Keyed/Option/Complex/Range;
/// Point/Vector/Tensor/Matrix quantity slot; Map; Field; Function
/// params+return; Union; Applied args; Projection base) but tests membership
/// in a caller-supplied name set rather than "carries any type param at all".
///
/// **Why this exists (task γ #4031 amendment).** `compile_variant_construct`'s
/// anti-cascade skip must not re-check a declared field type against an
/// already-conflicted type parameter: once a param has conflicted,
/// `subst`'s binding for it is an arbitrary first-writer-wins artifact (see
/// `unify`), so substituting it into a declared type and comparing would
/// emit a second, misleading diagnostic for the same root cause already
/// reported as `EnumTypeArgConflict`. A BARE `Type::TypeParam(p)` declared
/// field is easy to detect with a direct pattern match, but a COMPOUND
/// declared type that merely *mentions* a conflicted param somewhere inside
/// it (e.g. `c: List<T>` when sibling fields `a: T` / `b: T` already
/// conflicted on `T`) needs the identical skip — this predicate answers that
/// in one call regardless of nesting depth or constructor.
///
/// The match is intentionally exhaustive (no `_` wildcard), in lock-step with
/// `type_carries_type_param`, `unify`, and `substitute_type_params`, so a
/// future `Type` variant forces a compile-time decision here too.
pub(crate) fn type_mentions_conflicted_param(t: &Type, conflicted: &HashSet<String>) -> bool {
    match t {
        // The type-parameter leaf itself.
        Type::TypeParam(p) => conflicted.contains(p),

        // Single-inner-Type wrappers: recurse on the child.
        Type::List(inner)
        | Type::Set(inner)
        | Type::Keyed(inner)
        | Type::Option(inner)
        | Type::Complex(inner)
        | Type::Range(inner) => type_mentions_conflicted_param(inner, conflicted),

        // Quantity-bearing aggregates: recurse into the quantity slot.
        Type::Point { quantity, .. }
        | Type::Vector { quantity, .. }
        | Type::Tensor { quantity, .. }
        | Type::Matrix { quantity, .. } => type_mentions_conflicted_param(quantity, conflicted),

        // Two-inner-Type wrappers.
        Type::Map(key, val) => {
            type_mentions_conflicted_param(key, conflicted)
                || type_mentions_conflicted_param(val, conflicted)
        }
        Type::Field { domain, codomain } => {
            type_mentions_conflicted_param(domain, conflicted)
                || type_mentions_conflicted_param(codomain, conflicted)
        }

        // Function: any param, or the return type.
        Type::Function {
            params,
            return_type,
        } => {
            params
                .iter()
                .any(|p| type_mentions_conflicted_param(p, conflicted))
                || type_mentions_conflicted_param(return_type, conflicted)
        }

        // Union: any arm.
        Type::Union(arms) => arms
            .iter()
            .any(|arm| type_mentions_conflicted_param(arm, conflicted)),

        // task 4602 β: Applied — recurse into type args; Projection — recurse into base.
        Type::Applied { args, .. } => args
            .iter()
            .any(|arg| type_mentions_conflicted_param(arg, conflicted)),
        Type::Projection { base, .. } => type_mentions_conflicted_param(base, conflicted),

        // All remaining leaves carry no inner `Type`.
        Type::Bool
        | Type::Int
        | Type::String
        | Type::Scalar { .. }
        | Type::Enum(_)
        | Type::StructureRef(_)
        | Type::TraitObject(_)
        | Type::Geometry
        | Type::Feature
        | Type::Orientation(_)
        | Type::Frame(_)
        | Type::Transform(_)
        | Type::AffineMap(_)
        | Type::Plane
        | Type::Axis
        | Type::Direction
        | Type::Relation
        | Type::BoundingBox
        | Type::Selector(_)
        | Type::AnySelector
        | Type::ScalarParam(_)
        | Type::Error => false,
    }
}

/// Whether `t` (or any type nested within it) carries a dimension-kinded
/// parameter (`Type::ScalarParam`).
///
/// This is the sibling of [`type_carries_type_param`] for dimension params.
/// It uses the SAME constructor recursion (List/Set/Keyed/Option/Complex/Range;
/// Map; Field; Function params+return; Point/Vector/Tensor/Matrix quantity;
/// Union) and returns `true` at the `ScalarParam(_)` leaf, `false` at all
/// other leaves.
///
/// The match is intentionally exhaustive (no `_` wildcard) so that a new
/// `Type` variant forces a compile-time decision here, in lock-step with
/// `type_carries_type_param`, `unify`, and `substitute_type_params`.
///
/// Wired into the generic-candidate wildcard in `resolve_function_overload`
/// and `try_default_padding` (OR'd with `type_carries_type_param`) so that
/// a `Scalar<Q>` parameter is recognised as a generic wildcard slot (task 4235
/// ζ / D8).
pub(crate) fn type_carries_dim_param(t: &Type) -> bool {
    match t {
        // The dimension-parameter leaf itself.
        Type::ScalarParam(_) => true,

        // Single-inner-Type wrappers: recurse on the child.
        Type::List(inner)
        | Type::Set(inner)
        | Type::Keyed(inner)
        | Type::Option(inner)
        | Type::Complex(inner)
        | Type::Range(inner) => type_carries_dim_param(inner),

        // Quantity-bearing aggregates: recurse into the quantity slot.
        Type::Point { quantity, .. }
        | Type::Vector { quantity, .. }
        | Type::Tensor { quantity, .. }
        | Type::Matrix { quantity, .. } => type_carries_dim_param(quantity),

        // Two-inner-Type wrappers.
        Type::Map(key, val) => type_carries_dim_param(key) || type_carries_dim_param(val),
        Type::Field { domain, codomain } => {
            type_carries_dim_param(domain) || type_carries_dim_param(codomain)
        }

        // Function: any param, or the return type.
        Type::Function {
            params,
            return_type,
        } => params.iter().any(type_carries_dim_param) || type_carries_dim_param(return_type),

        // Union: any arm.
        Type::Union(arms) => arms.iter().any(type_carries_dim_param),

        // task 4602 β: Applied — recurse into type args; Projection — recurse into base.
        Type::Applied { args, .. } => args.iter().any(type_carries_dim_param),
        Type::Projection { base, .. } => type_carries_dim_param(base),

        // All remaining leaves carry no `ScalarParam`.
        Type::Bool
        | Type::Int
        | Type::String
        | Type::Scalar { .. }
        | Type::Enum(_)
        | Type::StructureRef(_)
        | Type::TraitObject(_)
        | Type::Geometry
        // Feature identity token (task 4808 / P1 γ): inner-Type-free leaf.
        | Type::Feature
        | Type::Orientation(_)
        | Type::Frame(_)
        | Type::Transform(_)
        | Type::AffineMap(_)
        | Type::Plane
        | Type::Axis
        | Type::Direction
        // Relation directive (γ): an inner-Type-free leaf, carries no dim param.
        | Type::Relation
        | Type::BoundingBox
        | Type::Selector(_)
        | Type::AnySelector
        // Type-param leaf: carries no *dimension* param.
        | Type::TypeParam(_)
        | Type::Error => false,
    }
}

/// A call-site type-argument inference conflict: the same type parameter was
/// bound to two different concrete types across a generic call's arguments.
///
/// Raised by [`unify`] when an earlier argument bound type parameter `param`
/// to `existing` and a later argument requires the incompatible `incoming`.
/// The call site (expr.rs) consumes this to emit
/// `DiagnosticCode::FnTypeArgConflict` (task 4231 β, PRD D2 / §4.2).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TypeArgConflict {
    pub(crate) param: String,
    pub(crate) existing: Type,
    pub(crate) incoming: Type,
}

/// Conservative, single-pass structural unification of a generic function's
/// declared parameter type against a concrete argument type.
///
/// Binds `Type::TypeParam` leaves in `declared` to the corresponding sub-type
/// of `arg`, accumulating into `subst`. Recurses through matching constructors
/// (List/Set/Keyed/Option/Complex/Range, Map, Field, Function of equal arity,
/// Point/Vector/Tensor/Matrix of equal shape, Union of equal length).
///
/// Conservative by design (PRD D2): the ONLY error is a type-parameter
/// double-binding (`Err(TypeArgConflict)`). A structural mismatch where
/// `declared` is not a `TypeParam` and its constructor does not match `arg`'s
/// returns `Ok(())` with no binding — eval is type-erased (INV-2), so a
/// declared/arg shape divergence is not itself a type error at this seam;
/// overload resolution is the separate match gate.
///
/// Pure and side-effect-free apart from mutating `subst`: it takes no
/// diagnostics sink, leaving emission to the call site.
///
/// **β note — `Applied` vs `StructureRef` (task 4602):** `Applied{"C", [T]}`
/// unified against a bare `StructureRef("C")` hits the
/// `(Type::Applied { .. }, _) => Ok(())` fallthrough arm and binds **nothing**
/// for `T`.  This is the deliberate β posture: resolving arg bindings across an
/// Applied↔StructureRef pair requires the per-structure assoc-type table, which
/// belongs to δ (`normalize_type`).  The δ implementer must NOT assume that this
/// inference already happens here.
pub(crate) fn unify(
    declared: &Type,
    arg: &Type,
    subst: &mut HashMap<String, Type>,
) -> Result<(), TypeArgConflict> {
    match (declared, arg) {
        // Type-parameter leaf: bind if absent; re-bind to the same type is Ok;
        // re-bind to a different type conflicts — EXCEPT when exactly one side
        // is itself a bare `TypeParam` (an ERASED/unknown type). An erased
        // binding is provisional: a bare `TypeParam` arg carries no concrete
        // information (it arises inside generic fn bodies, or leaks out of
        // composing generics over a headless-`Enum` builtin — the
        // `or_else(parse_length_r(x), parse_length_r(y))` chain in task #4038 δ,
        // whose erased-arg result is `Applied{"Result", [T, E]}`). Binding a
        // param to such an erased arg must not later HARD-conflict with the
        // real concrete arg for the same param: concrete information wins, the
        // erased side yields. Two differing CONCRETE bindings (a genuine
        // type-arg conflict) and two differing ERASED bindings (e.g. `pair(a:A,
        // b:B)` with distinct generic params) still conflict as before.
        //
        // Design precedent: this mirrors the existing generic-body-permissive
        // posture (task 4232 γ D4, `fn_generic_body_permissive_tests.rs`) of
        // treating a bare `TypeParam` as a resolution wildcard rather than a
        // concrete value. The relaxation is deliberately scoped to that ONE
        // leaf shape — a bare `TypeParam` on the rebind side — and does NOT
        // extend to a HEADED type that merely carries a nested type-param
        // (e.g. the leaky `Applied{"Result", [T, E]}` itself, which is not a
        // `Type::TypeParam` at its own head): two differing CONCRETE bindings
        // still hard-conflict even when one of them is such a headed type.
        // `unify_concrete_vs_headed_concrete_still_conflicts` below pins that
        // boundary with a negative-path proof.
        (Type::TypeParam(p), _) => match subst.get(p) {
            None => {
                subst.insert(p.clone(), arg.clone());
                Ok(())
            }
            Some(existing) if existing == arg => Ok(()),
            // Erased existing yields to concrete incoming: upgrade the binding.
            Some(existing)
                if matches!(existing, Type::TypeParam(_))
                    && !matches!(arg, Type::TypeParam(_)) =>
            {
                subst.insert(p.clone(), arg.clone());
                Ok(())
            }
            // Concrete existing absorbs an erased incoming: keep the concrete.
            Some(existing)
                if !matches!(existing, Type::TypeParam(_))
                    && matches!(arg, Type::TypeParam(_)) =>
            {
                Ok(())
            }
            Some(existing) => Err(TypeArgConflict {
                param: p.clone(),
                existing: existing.clone(),
                incoming: arg.clone(),
            }),
        },

        // Single-inner-Type constructors: recurse on the child.
        (Type::List(d), Type::List(a))
        | (Type::Set(d), Type::Set(a))
        | (Type::Keyed(d), Type::Keyed(a))
        | (Type::Option(d), Type::Option(a))
        | (Type::Complex(d), Type::Complex(a))
        | (Type::Range(d), Type::Range(a)) => unify(d, a, subst),

        // Two-inner-Type constructors.
        (Type::Map(dk, dv), Type::Map(ak, av)) => {
            unify(dk, ak, subst)?;
            unify(dv, av, subst)
        }
        (
            Type::Field {
                domain: dd,
                codomain: dc,
            },
            Type::Field {
                domain: ad,
                codomain: ac,
            },
        ) => {
            unify(dd, ad, subst)?;
            unify(dc, ac, subst)
        }

        // Function: equal arity → unify each param then the return type.
        (
            Type::Function {
                params: dp,
                return_type: dr,
            },
            Type::Function {
                params: ap,
                return_type: ar,
            },
        ) if dp.len() == ap.len() => {
            for (d, a) in dp.iter().zip(ap.iter()) {
                unify(d, a, subst)?;
            }
            unify(dr, ar, subst)
        }

        // Quantity-bearing aggregates: equal shape → unify the quantity slot.
        (
            Type::Point { n: dn, quantity: dq },
            Type::Point { n: an, quantity: aq },
        ) if dn == an => unify(dq, aq, subst),
        (
            Type::Vector { n: dn, quantity: dq },
            Type::Vector { n: an, quantity: aq },
        ) if dn == an => unify(dq, aq, subst),
        (
            Type::Tensor {
                rank: drk,
                n: dn,
                quantity: dq,
            },
            Type::Tensor {
                rank: ark,
                n: an,
                quantity: aq,
            },
        ) if drk == ark && dn == an => unify(dq, aq, subst),
        (
            Type::Matrix {
                m: dm,
                n: dn,
                quantity: dq,
            },
            Type::Matrix {
                m: am,
                n: an,
                quantity: aq,
            },
        ) if dm == am && dn == an => unify(dq, aq, subst),

        // Union: equal length → unify arm-by-arm.
        (Type::Union(da), Type::Union(aa)) if da.len() == aa.len() => {
            for (d, a) in da.iter().zip(aa.iter()) {
                unify(d, a, subst)?;
            }
            Ok(())
        }

        // task 4602 β: Applied — same name + same arity → element-wise unify args.
        (
            Type::Applied { name: dn, args: da },
            Type::Applied { name: an, args: aa },
        ) if dn == an && da.len() == aa.len() => {
            for (d, a) in da.iter().zip(aa.iter()) {
                unify(d, a, subst)?;
            }
            Ok(())
        }

        // task 4602 β: Projection — same member → unify bases.
        (
            Type::Projection { base: db, member: dm },
            Type::Projection { base: ab, member: am },
        ) if dm == am => unify(db, ab, subst),

        // Conservative fallthrough — listed explicitly with NO `_` wildcard so
        // a future `Type` variant forces a compile-time decision here, in
        // lock-step with `type_carries_type_param` and the exhaustive
        // `substitute_type_params`. Every arm below binds nothing and never
        // errors: reaching it means either (a) `declared` is an
        // inner-Type-bearing constructor whose matching-pair arm above did not
        // fire — a structural mismatch (e.g. declared `List<T>` vs arg `Int`,
        // or `Point` of a different `n`), which a type-erased seam (INV-2)
        // treats as a non-error; or (b) `declared` is a leaf with no inner
        // `Type`. (`TypeParam` is consumed by the first arm above.)
        //
        // Inner-Type-bearing constructors (structural mismatch → no binding):
        (Type::List(_), _)
        | (Type::Set(_), _)
        | (Type::Keyed(_), _)
        | (Type::Option(_), _)
        | (Type::Complex(_), _)
        | (Type::Range(_), _)
        | (Type::Map(_, _), _)
        | (Type::Field { .. }, _)
        | (Type::Function { .. }, _)
        | (Type::Point { .. }, _)
        | (Type::Vector { .. }, _)
        | (Type::Tensor { .. }, _)
        | (Type::Matrix { .. }, _)
        | (Type::Union(_), _)
        // task 4602 β: Applied/Projection structural mismatches → no binding.
        | (Type::Applied { .. }, _)
        | (Type::Projection { .. }, _) => Ok(()),

        // Dimension-param scalar: bind when the arg is a concrete Scalar, mirror
        // of the TypeParam arm above (bind / idempotent re-bind = Ok / differing
        // re-bind = Err(TypeArgConflict)). For non-Scalar args the arm falls
        // through to the leaves block and binds nothing (conservative per D8).
        (Type::ScalarParam(p), Type::Scalar { .. }) => match subst.get(p) {
            None => {
                subst.insert(p.clone(), arg.clone());
                Ok(())
            }
            Some(existing) if existing == arg => Ok(()),
            Some(existing) => Err(TypeArgConflict {
                param: p.clone(),
                existing: existing.clone(),
                incoming: arg.clone(),
            }),
        },

        // True leaves (no inner `Type` to bind):
        (Type::Bool, _)
        | (Type::Int, _)
        | (Type::String, _)
        | (Type::Scalar { .. }, _)
        | (Type::Enum(_), _)
        | (Type::StructureRef(_), _)
        | (Type::TraitObject(_), _)
        | (Type::Geometry, _)
        // Feature identity token (task 4808 / P1 γ): inner-Type-free leaf.
        | (Type::Feature, _)
        | (Type::Orientation(_), _)
        | (Type::Frame(_), _)
        | (Type::Transform(_), _)
        | (Type::AffineMap(_), _)
        | (Type::Plane, _)
        | (Type::Axis, _)
        | (Type::Direction, _)
        // Relation directive (γ): a leaf with no inner `Type` to bind.
        | (Type::Relation, _)
        | (Type::BoundingBox, _)
        | (Type::Selector(_), _)
        | (Type::AnySelector, _)
        // Dimension-param scalar against a non-Scalar arg: binds nothing (the
        // ScalarParam vs Scalar{..} case is handled by the arm above; reaching
        // this leaf means arg is not a concrete Scalar).
        | (Type::ScalarParam(_), _)
        | (Type::Error, _) => Ok(()),
    }
}

/// Strict constructor-head compatibility check — the middle tie-break tier
/// in [`resolve_function_overload`] (D-head-exact, result-fallback Layer-B
/// task, B2).
///
/// Mirrors [`unify`]'s arm structure (the same constructor pairs recurse on
/// the same shape), but is a STRICT match gate rather than a permissive one:
/// `unify` treats a constructor-head mismatch as its conservative
/// `Ok(())` fallthrough (binds nothing, never errors), which is exactly why
/// it cannot discriminate `Option<T>` from `Applied{"Result", [T, E]}` — both
/// "unify" against any subject without erroring. `heads_unifiable` instead
/// returns `false` on a head mismatch, so it can serve as a genuine
/// disambiguator between two generic overloads whose type-param-carrying
/// params would otherwise both wildcard-match the same subject.
///
/// Differences from `unify`, both deliberate:
/// - A bare `Type::TypeParam` / `Type::ScalarParam` (matched against a
///   concrete `Scalar`) leaf is a wildcard slot (`true`) — the slot itself
///   carries no constructor head to disagree on.
/// - `Applied{name, ..}` vs `Enum(name)` (same name) is a head match:
///   variant construction (`Ok { .. }` / `Err { .. }`) type-erases its
///   result to `Type::Enum(name)` (`variant_construct.rs`), so a declared
///   `Applied{"Result", ..}` param must still recognise an erased `Result`
///   subject.
/// - The catch-all is `param == arg` (plain equality) rather than `unify`'s
///   permissive `Ok(())` — a head mismatch (or two leaves) must agree
///   exactly to count as "unifiable" here.
fn heads_unifiable(param: &Type, arg: &Type) -> bool {
    match (param, arg) {
        // Type-param / dim-param leaves: wildcard slots, always compatible.
        (Type::TypeParam(_), _) => true,
        (Type::ScalarParam(_), Type::Scalar { .. }) => true,

        // Single-inner-Type constructors: same head → recurse on the child.
        (Type::List(d), Type::List(a))
        | (Type::Set(d), Type::Set(a))
        | (Type::Keyed(d), Type::Keyed(a))
        | (Type::Option(d), Type::Option(a))
        | (Type::Complex(d), Type::Complex(a))
        | (Type::Range(d), Type::Range(a)) => heads_unifiable(d, a),

        // Two-inner-Type constructors.
        (Type::Map(dk, dv), Type::Map(ak, av)) => {
            heads_unifiable(dk, ak) && heads_unifiable(dv, av)
        }
        (
            Type::Field {
                domain: dd,
                codomain: dc,
            },
            Type::Field {
                domain: ad,
                codomain: ac,
            },
        ) => heads_unifiable(dd, ad) && heads_unifiable(dc, ac),

        // Function: equal arity → recurse on each param + the return type.
        (
            Type::Function {
                params: dp,
                return_type: dr,
            },
            Type::Function {
                params: ap,
                return_type: ar,
            },
        ) if dp.len() == ap.len() => {
            dp.iter().zip(ap.iter()).all(|(d, a)| heads_unifiable(d, a)) && heads_unifiable(dr, ar)
        }

        // Quantity-bearing aggregates: same shape → recurse on the quantity slot.
        (
            Type::Point {
                n: dn,
                quantity: dq,
            },
            Type::Point {
                n: an,
                quantity: aq,
            },
        ) if dn == an => heads_unifiable(dq, aq),
        (
            Type::Vector {
                n: dn,
                quantity: dq,
            },
            Type::Vector {
                n: an,
                quantity: aq,
            },
        ) if dn == an => heads_unifiable(dq, aq),
        (
            Type::Tensor {
                rank: drk,
                n: dn,
                quantity: dq,
            },
            Type::Tensor {
                rank: ark,
                n: an,
                quantity: aq,
            },
        ) if drk == ark && dn == an => heads_unifiable(dq, aq),
        (
            Type::Matrix {
                m: dm,
                n: dn,
                quantity: dq,
            },
            Type::Matrix {
                m: am,
                n: an,
                quantity: aq,
            },
        ) if dm == am && dn == an => heads_unifiable(dq, aq),

        // Union: equal length → recurse arm-by-arm.
        (Type::Union(da), Type::Union(aa)) if da.len() == aa.len() => {
            da.iter().zip(aa.iter()).all(|(d, a)| heads_unifiable(d, a))
        }

        // Applied: same name + same arity → recurse element-wise on args.
        (Type::Applied { name: dn, args: da }, Type::Applied { name: an, args: aa })
            if dn == an && da.len() == aa.len() =>
        {
            da.iter().zip(aa.iter()).all(|(d, a)| heads_unifiable(d, a))
        }

        // Erased-subject rule: a declared `Applied{name}` param head-matches
        // an erased `Enum(name)` arg (same name) — see the doc comment above.
        (Type::Applied { name: dn, .. }, Type::Enum(en)) if dn == en => true,

        // Projection: same member → recurse on the bases.
        (
            Type::Projection {
                base: db,
                member: dm,
            },
            Type::Projection {
                base: ab,
                member: am,
            },
        ) if dm == am => heads_unifiable(db, ab),

        // Catch-all: leaves and mismatched/differently-shaped constructors
        // must agree by plain equality — unlike `unify`'s permissive
        // `Ok(())` fallthrough, a head mismatch here is `false`.
        _ => param == arg,
    }
}

/// Resolve a function call against the list of compiled user functions.
///
/// Uses **exact** type matching for concrete params; trait-object-carrying params
/// (`type_carries_trait_object`) act as resolution wildcards and match any arg
/// type.  Int→Real widening is NOT applied during overload resolution so that
/// `f(Int)` and `f(Real)` are treated as distinct overloads.
///
/// When both a concrete and a trait-object overload would match (the wildcard
/// relaxation makes the trait param accept a concrete arg), exact full-equality
/// matches win: the wildcard matches are discarded before Resolved/Ambiguous
/// classification so a concrete arg resolves to its concrete overload rather
/// than being reported as ambiguous.
pub(crate) fn resolve_function_overload<'a>(
    name: &str,
    arg_types: &[Type],
    functions: &'a [CompiledFunction],
) -> OverloadResolution<'a> {
    // All user functions with the given name (for error reporting).
    let named: Vec<&CompiledFunction> = functions.iter().filter(|f| f.name == name).collect();

    if named.is_empty() {
        return OverloadResolution::NoUserFunctions;
    }

    // Among named functions, filter by arity and param-type compatibility.
    // Trait-carrying params are resolution wildcards; concrete params keep
    // exact equality.  This mirrors the structure-instantiation path where
    // named-arg binding is not type-gated and conformance is validated
    // separately (see task-4081 design decision §1).
    let matches: Vec<&CompiledFunction> = named
        .iter()
        .copied()
        .filter(|f| {
            // For a GENERIC candidate, a type-param-carrying param is a
            // resolution wildcard (matches any arg) — mirroring the trait-object
            // wildcard. Gated on `is_generic` so non-generic fns (empty
            // type_params) are bit-for-bit unchanged (INV-6). A full wildcard
            // (not structural unify) is deliberate: a conflicting generic call
            // (e.g. `pair(1, 1.5)`) still SELECTS the candidate so the call site
            // can emit `E_FN_TYPE_ARG_CONFLICT` rather than a generic no-match.
            //
            // D4 (task-4232 γ): A type-param-carrying ARG also acts as a
            // resolution wildcard (matches any param). This lets a generic fn
            // body pass a TypeParam-typed value to a concrete-param function
            // without a spurious NoMatch. It is self-scoping: TypeParam args only
            // arise inside generic fn bodies, so concrete-arg calls (non-generic
            // callers) are bit-for-bit unchanged — type_carries_type_param(concrete) = false.
            let is_generic = !f.type_params.is_empty();
            f.params.len() == arg_types.len()
                && f.params
                    .iter()
                    .zip(arg_types.iter())
                    .all(|((_, param_ty), arg_ty)| {
                        type_carries_trait_object(param_ty)
                            || (is_generic
                                && (type_carries_type_param(param_ty)
                                    || type_carries_dim_param(param_ty)))
                            || type_carries_type_param(arg_ty)
                            || param_ty == arg_ty
                    })
        })
        .collect();

    // Tie-break: prefer candidates that match ALL params by *exact* equality
    // (no wildcard relaxation) over trait-carrying wildcard matches. Without
    // this, a function with both a trait-object overload and a concrete
    // overload — e.g. `couple(DrivingJoint)` + `couple(Real)` — would treat a
    // concrete arg like `couple(2.0)` as matching BOTH (the trait param acts as
    // a wildcard), yielding a spurious `Ambiguous` on previously-valid code.
    // When at least one exact match exists, the wildcard matches are discarded
    // before classification. (task-4081 overload-resolution regression fix.)
    let exact_matches: Vec<&CompiledFunction> = matches
        .iter()
        .copied()
        .filter(|f| {
            f.params
                .iter()
                .zip(arg_types.iter())
                .all(|((_, param_ty), arg_ty)| param_ty == arg_ty)
        })
        .collect();

    // Second tie-break tier (D-head-exact, result-fallback Layer-B task B2):
    // when no exact match exists, prefer candidates whose type-param-carrying
    // params are STRUCTURALLY compatible with their arg (`heads_unifiable`)
    // over the full wildcard relaxation used by `matches`. This disambiguates
    // two GENERIC overloads with different container heads — e.g. a
    // user `unwrap_or<T,E>(r: Result<T,E>, ..)` vs the stdlib
    // `unwrap_or<T>(o: Option<T>, ..)` — which would otherwise both
    // wildcard-match any subject via `type_carries_type_param` and force a
    // spurious `Ambiguous`.
    //
    // `head_matches` FILTERS `matches` (⊆ matches by construction), so it can
    // only NARROW a would-be ambiguity, never introduce a spurious match:
    // single-candidate resolution and the deliberate select-then-conflict
    // behavior (a constructor-headed generic param over-selecting a
    // mismatched-head arg so the call site can emit `E_FN_TYPE_ARG_CONFLICT`)
    // are preserved because an empty `head_matches` falls through to
    // `matches` below. Only the `type_carries_type_param(param_ty)` disjunct
    // is replaced by `heads_unifiable`; `type_carries_dim_param(param_ty)`
    // stays a full wildcard — dimension-param overload resolution is
    // orthogonal to enum-head disambiguation.
    let head_matches: Vec<&CompiledFunction> = matches
        .iter()
        .copied()
        .filter(|f| {
            let is_generic = !f.type_params.is_empty();
            f.params
                .iter()
                .zip(arg_types.iter())
                .all(|((_, param_ty), arg_ty)| {
                    type_carries_trait_object(param_ty)
                        || (is_generic
                            && (heads_unifiable(param_ty, arg_ty)
                                || type_carries_dim_param(param_ty)))
                        // D4 (task-4232 γ) in the head-exact tier: a type-param
                        // arg is a wildcard ONLY when it is a BARE `TypeParam`
                        // (a generic fn body passing a `T`-typed value) — that
                        // slot carries no constructor head to disagree on, so
                        // `heads_unifiable` (above) can't discriminate it. A
                        // HEADED arg carrying a NESTED type-param (e.g. an
                        // `Applied{"Result", [T, E]}` produced by composing two
                        // generic stdlib fns over a headless-`Enum` builtin —
                        // task #4038 δ) must NOT wildcard-match every candidate:
                        // it has a real head, so `heads_unifiable` discriminates
                        // it (`Result` matches the `Result<T,E>` overload, not
                        // the `Option<T>` one), turning a spurious `Ambiguous`
                        // into a clean `Resolved`. This narrows head_matches
                        // (⊆ matches) only; a resulting empty set still falls
                        // through to `matches`, so bare-`TypeParam`-arg
                        // resolution is bit-for-bit unchanged.
                        //
                        // NOTE (reviewer_comprehensive #2): this narrowing also
                        // means a NON-generic candidate (`is_generic == false`)
                        // is never eligible for head_matches against a headed
                        // nested-type-param arg — it fails `is_generic`, the
                        // bare-`TypeParam` wildcard, and plain equality. The
                        // head-exact tier therefore deliberately assumes headed
                        // nested-type-param args only ever need to disambiguate
                        // GENERIC container overloads (e.g. Option<T> vs
                        // Result<T,E>); a same-name non-generic candidate in the
                        // same overload set is excluded from head_matches
                        // rather than causing a spurious `Ambiguous`. See
                        // `overload_leaky_headed_arg_excludes_non_generic_candidate`
                        // for the precedent lock.
                        || matches!(arg_ty, Type::TypeParam(_))
                        || param_ty == arg_ty
                })
        })
        .collect();

    let resolved = if !exact_matches.is_empty() {
        exact_matches
    } else if !head_matches.is_empty() {
        head_matches
    } else {
        matches
    };

    match resolved.len() {
        1 => OverloadResolution::Resolved(resolved[0]),
        0 => OverloadResolution::NoMatch(named),
        _ => OverloadResolution::Ambiguous(resolved),
    }
}

/// Format a function signature for error messages: `name(T1, T2) -> Ret`.
pub(crate) fn format_fn_signature(f: &CompiledFunction) -> String {
    format!(
        "{}({}) -> {}",
        f.name,
        f.params
            .iter()
            .map(|(_, t)| format!("{}", t))
            .collect::<Vec<_>>()
            .join(", "),
        f.return_type
    )
}

// --- Dimension-mismatch diagnostic helpers ---

/// Build the canonical dimension-mismatch error diagnostic.
///
/// Produces `"dimension mismatch in {op_name}: {left_ty} vs {right_ty}"` with
/// `DiagnosticCode::DimensionMismatch` and the primary `"incompatible dimensions"` label.
///
/// When BOTH operands are `Type::Scalar` with a canonical name (see
/// `DimensionVector::canonical_name`) and the two names differ, attaches a
/// secondary label of the form `"<LName> and <RName> are different dimensions
/// and cannot be combined directly"` so the user sees the human-readable
/// dimension name rather than just the unit-symbol form.
pub(crate) fn format_dimension_mismatch_diagnostic(
    op_name: &str,
    left_ty: &Type,
    right_ty: &Type,
    span: SourceSpan,
) -> Diagnostic {
    // Compute the optional secondary label before building the diagnostic so
    // there is a single exit point and no early return.
    let secondary: Option<DiagnosticLabel> =
        if let (Type::Scalar { dimension: ldim }, Type::Scalar { dimension: rdim }) =
            (left_ty, right_ty)
            && let (Some(lname), Some(rname)) = (ldim.canonical_name(), rdim.canonical_name())
            && lname != rname
        {
            Some(DiagnosticLabel::new(
                span,
                format!(
                    "{lname} and {rname} are different dimensions and cannot be combined directly"
                ),
            ))
        } else {
            None
        };

    let mut d = Diagnostic::error(format!(
        "dimension mismatch in {op_name}: {left_ty} vs {right_ty}"
    ))
    .with_code(DiagnosticCode::DimensionMismatch)
    .with_label(DiagnosticLabel::new(span, "incompatible dimensions"));

    if let Some(label) = secondary {
        d = d.with_label(label);
    }

    d
}

// --- Chained comparison helpers ---

/// Returns true if `op` is a comparison operator that participates in chaining.
pub(crate) fn is_comparison_op(op: &str) -> bool {
    matches!(op, "<" | "<=" | ">" | ">=" | "==" | "!=")
}

/// Flatten a left-nested comparison chain into (operands, operators).
///
/// Given `BinOp(op2, BinOp(op1, a, b), c)` where both op1 and op2 are comparison
/// operators, returns `([a, b, c], [op1, op2])`.
///
/// `outer_op`, `left`, and `right` are the components of the outermost BinOp.
/// Precondition: `outer_op` is a comparison op and `left` is a comparison BinOp.
pub(crate) fn flatten_comparison_chain<'a>(
    outer_op: &'a str,
    left: &'a reify_ast::Expr,
    right: &'a reify_ast::Expr,
) -> (Vec<&'a reify_ast::Expr>, Vec<&'a str>) {
    match &left.kind {
        reify_ast::ExprKind::BinOp {
            op: inner_op,
            left: ll,
            right: lr,
        } if is_comparison_op(inner_op) => {
            // Recurse: flatten the left subtree first, then append current right and op
            let (mut operands, mut ops) = flatten_comparison_chain(inner_op, ll, lr);
            operands.push(right);
            ops.push(outer_op);
            (operands, ops)
        }
        _ => {
            // Base case: left is not a comparison chain; operands = [left, right], ops = [outer_op]
            (vec![left, right], vec![outer_op])
        }
    }
}

// --- Constraint-instantiation arg type conformance ---

/// Predicate used by `expand_constraint_inst` (entity.rs) to validate that a
/// constraint instantiation argument's type conforms to the declared parameter
/// type.
///
/// This is a **narrow cross-category conformance check** — it rejects only
/// cross-category mismatches (Bool/String/Enum/aggregate vs numeric/Length
/// etc.) while deliberately tolerating numeric-for-dimensioned at the binding
/// site (e.g. `Int` passed where `Length` is declared). Dimensional strictness
/// within comparison predicates is already enforced by task 4490's
/// `emit_comparison_operand_diagnostics`; duplicating it here at the binding
/// site would cause false-positive rejections for currently-valid
/// instantiations such as `forall v in [1,2,3]: constraint MinThreshold(value: v)`
/// where `param value: Length` and `v` is `Int`.
///
/// # Safety of non-numeric param types
///
/// Non-numeric param types (Geometry, aggregate structs, etc.) are handled
/// safely by the earlier rules, so Rule 5 never incorrectly rejects them:
///
/// - **Trait-typed params** (e.g. `param tolerance : GeometricTolerance`
///   resolving to `Type::TraitObject`) exit early at Rule 2 via
///   `type_carries_trait_object` — conformance for trait params is handled by
///   separate trait-checking machinery, not here.
/// - **Same-type non-numeric params** (e.g. `param actual : Geometry` with a
///   `Geometry`-typed arg) exit at Rule 3 via `type_compatible`'s identity
///   short-circuit (`from == to`).
/// - Rule 5 therefore fires only for genuinely cross-category pairs such as
///   `Bool` or `String` passed where a numeric/`Geometry`/struct param is
///   declared — those are real errors and correctly rejected.
///
/// This invariant is validated by the reify-compiler test suite (including
/// GD&T `Conforms` tolerancing fixtures that use trait-typed and
/// `Geometry`-typed params) — 3735 tests pass with zero false positives.
///
/// # Rules (applied in priority order)
///
/// 1. `param_ty.is_error() || arg_ty.is_error()` → **accept** (anti-cascade
///    guard; also prevents `type_compatible`'s `debug_assert!(!param_ty.is_error())`
///    from firing when a param type failed to resolve at def-compile time).
/// 2. `type_carries_type_param(param_ty) || type_carries_trait_object(param_ty)`
///    → **accept** (generic/trait params are resolved by separate
///    machinery; a bare structural comparison would false-positive on generic
///    constraint defs, e.g. `constraint def Foo<T>(x: T)`).
/// 3. `type_compatible(param_ty, arg_ty)` → **accept** (covers identity,
///    tensor/vector/matrix rules, `Int`→dimensionless-scalar widening, and
///    selector coercions — the common well-typed case).
/// 4. Both sides are numeric (`Type::Int | Type::Scalar{..} | Type::ScalarParam(_)`)
///    → **accept** (numeric leniency: tolerates `Int`-for-`Length` and
///    cross-dimension scalars at the binding site; task 4490 guards
///    dimensional correctness inside comparison predicates).
/// 5. Otherwise → **reject** (cross-category mismatch, e.g. `Bool` vs `Length`,
///    `String` vs `Length`, `Enum(X)` vs `Length`).
pub(crate) fn constraint_arg_type_conforms(param_ty: &Type, arg_ty: &Type) -> bool {
    // Rule 1: Anti-cascade guard — either side poisoned → accept.
    // Also prevents `type_compatible`'s debug_assert!(!param_ty.is_error()) from
    // firing when a param's declared type failed to resolve at def-compile time.
    if param_ty.is_error() || arg_ty.is_error() {
        return true;
    }
    // Rule 2: Generic/trait-typed params — resolved by separate machinery; skip check.
    // `type_carries_type_param` catches TypeParam leaves (incl. inside List<T> etc.).
    // `type_carries_trait_object` catches TraitObject-carrying param types.
    if type_carries_type_param(param_ty) || type_carries_trait_object(param_ty) {
        return true;
    }
    // Rule 3: Standard structural compatibility (identity, tensor rules,
    // Int→dimensionless widening, Selector coercions). Handles the common case.
    if type_compatible(param_ty, arg_ty) {
        return true;
    }
    // Rule 4: Numeric leniency — both sides are some form of numeric scalar.
    // Tolerates Int-for-Length and cross-dimension scalar-for-scalar at the
    // binding site; dimensional strictness within predicates is task 4490's job.
    let is_numeric = |t: &Type| matches!(t, Type::Int | Type::Scalar { .. } | Type::ScalarParam(_));
    if is_numeric(param_ty) && is_numeric(arg_ty) {
        return true;
    }
    // Rule 5: Cross-category mismatch (e.g. Bool vs Length, String vs Length).
    false
}

// --- BinOp resolution ---

/// Parse a string operator into a `BinOp`.
pub(crate) fn resolve_binop(op: &str) -> Option<BinOp> {
    match op {
        "+" => Some(BinOp::Add),
        "-" => Some(BinOp::Sub),
        "*" => Some(BinOp::Mul),
        "/" => Some(BinOp::Div),
        "%" => Some(BinOp::Mod),
        "**" | "^" => Some(BinOp::Pow),
        "==" => Some(BinOp::Eq),
        "!=" => Some(BinOp::Ne),
        "<" => Some(BinOp::Lt),
        "<=" => Some(BinOp::Le),
        ">" => Some(BinOp::Gt),
        ">=" => Some(BinOp::Ge),
        "&&" | "and" => Some(BinOp::And),
        "||" | "or" => Some(BinOp::Or),
        "implies" => Some(BinOp::Implies),
        _ => None,
    }
}

/// Enforce spec §5.1: "modulo is `Int % Int -> Int` ONLY".
///
/// Returns `true` only when both operands are `Type::Int`.  All other shapes
/// (`Real`, `Scalar{Q}`, `Bool`, …) are rejected.
///
/// This is a pure predicate co-located with `resolve_binop` / `infer_binop_type`
/// so it can be unit-tested independently of the compiler pipeline.  Diagnostic
/// *emission* lives in `crates/reify-compiler/src/expr.rs` (the only site with a
/// `&mut Vec<Diagnostic>` sink), following the same split used for the Pow guard
/// (task-3805 / `E_NONINT_EXP_ON_DIMENSIONED`).
///
/// The PRD-prose mnemonic is `E_MODULO_REQUIRES_INT` (severity `E_` → Error).
pub(crate) fn modulo_operands_are_int(left: &Type, right: &Type) -> bool {
    matches!(left, Type::Int) && matches!(right, Type::Int)
}

/// Enforce PRD §7.1: ORDER ops (`<`, `<=`, `>`, `>=`) require both operands
/// to be orderable scalar kinds: `Type::Int`, `Type::Scalar { .. }`, or
/// `Type::ScalarParam(_)`.
///
/// `Type::ScalarParam(_)` is the dimension-parametric scalar `Scalar<Q>` produced
/// inside dimension-kinded generic fn signatures (e.g. `std.fields::threshold`'s
/// `sample(f, p) > value` over `Scalar<Q>`).  It is a genuine, well-formed scalar
/// — comparing `Scalar<Q>` against `Scalar<Q>` is a valid order comparison — so it
/// is accepted here rather than skipped in the caller's gradualism early-return:
/// accepting in the predicate still lets a bad *sibling* operand (e.g.
/// `Tensor > Scalar<Q>`) be flagged.
///
/// All other types — Bool, String, Enum, Vector, Point, Tensor, Matrix, List,
/// and compound types — produce `Value::Undef` at runtime for order comparisons
/// and are therefore rejected at compile time.
///
/// This is a pure predicate co-located with `modulo_operands_are_int` /
/// `is_comparison_op`.  Diagnostic emission lives in
/// `crates/reify-compiler/src/expr.rs` (`emit_comparison_operand_diagnostics`).
///
/// The PRD-prose mnemonic is `E_CmpOperandKind` (severity `E_` → Error).
pub(crate) fn is_orderable_scalar(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Scalar { .. } | Type::ScalarParam(_))
}

/// Enforce PRD §7.1: EQUALITY ops (`==`, `!=`) require both operands to be
/// equatable kinds: `Type::Bool`, `Type::Int`, `Type::String`,
/// `Type::Scalar { .. }`, or `Type::Enum(_)`.
///
/// Aggregate/structural kinds — Vector, Point, Tensor, Matrix, List, etc. —
/// produce `Value::Undef` at runtime for equality comparisons and are rejected.
///
/// NOTE: Enum equality is intentionally PRESERVED here.  `Enum == Enum` is the
/// guarded-declaration idiom `where shape == Shape.Round { ... }` used in
/// committed examples (m5_guarded_enum.ri etc.) and `eval_eq` returns a defined
/// `Bool` for Enum operands.  Rejecting it would break the build with no
/// in-scope fix — §3.3's rationale is tensor-specific.
///
/// This is a pure predicate co-located with `modulo_operands_are_int` /
/// `is_comparison_op`.  Diagnostic emission lives in
/// `crates/reify-compiler/src/expr.rs` (`emit_comparison_operand_diagnostics`).
///
/// The PRD-prose mnemonic is `E_CmpOperandKind` (severity `E_` → Error).
///
/// NOTE: `Type::Frame(_)` is also accepted.  `Frame3 == Frame3` (and `!=`) is
/// the structural port-selector identity idiom used in forall predicates, e.g.
/// `p.p @ face("mount") != p.p @ face("side")`.  `Value::Frame` has a
/// well-defined `PartialEq` impl (compares origin + basis), so this is a
/// semantically valid equality comparison.  Rejecting it would break existing
/// ad-hoc-selector patterns that compile and run correctly today.
pub(crate) fn is_equatable_kind(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Bool
            | Type::Int
            | Type::String
            | Type::Scalar { .. }
            // Dimension-parametric scalar `Scalar<Q>` (see is_orderable_scalar): a
            // well-formed scalar from dimension-kinded generic fns, equatable like
            // any other scalar.
            | Type::ScalarParam(_)
            | Type::Enum(_)
            | Type::Frame(_)
    )
}

/// Returns `true` if `expr` is a syntactic literal zero as defined in §7.2:
///
/// - `NumberLiteral { value == 0.0 }` — covers both `0` (`is_real:false`) and
///   `0.0` (`is_real:true`).
/// - `UnOp { op: "-", operand }` where `operand` is itself a syntactic zero
///   (recursive) — covers `-0`, `-0.0`, and double-negated forms like `--0`.
///
/// HARD BOUND: a constant-folded zero written as `1 - 1` is an
/// `ExprKind::BinOp`, NOT a `NumberLiteral`, so it returns `false` here.
/// This preserves the §7.2 contract that coercion applies only to syntactic
/// literal zeros, not to computed expressions that happen to evaluate to zero.
///
/// Colocated with `is_comparison_op` / `modulo_operands_are_int` as a pure,
/// unit-testable predicate; diagnostic emission / rewrite lives in
/// `expr.rs::compile_binop` (task-4485/β).
pub(crate) fn is_syntactic_zero_literal(expr: &reify_ast::Expr) -> bool {
    match &expr.kind {
        reify_ast::ExprKind::NumberLiteral { value, .. } => *value == 0.0,
        reify_ast::ExprKind::UnOp { op, operand } if op == "-" => {
            is_syntactic_zero_literal(operand)
        }
        _ => false,
    }
}

/// Parse a string unary operator into a `UnOp`.
pub(crate) fn resolve_unop(op: &str) -> Option<UnOp> {
    match op {
        "-" => Some(UnOp::Neg),
        "!" | "not" => Some(UnOp::Not),
        _ => None,
    }
}

// --- Type inference for binary operations ---

/// Scalar-like operand for `*`/`/` static typing: `Int` or `Scalar{..}`.
///
/// `Real` is NOT a distinct `Type` variant — a `Real` literal types as
/// `Scalar{dimension: DIMENSIONLESS}` — so this predicate covers it for free.
fn is_mul_div_scalar_like(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Scalar { .. })
}

/// Bare dimensionless number for `+`/`-` Complex widening: `Int` or
/// `Scalar{DIMENSIONLESS}` (a `Real` literal — see `is_mul_div_scalar_like`).
/// Narrower than `is_mul_div_scalar_like` on purpose: a DIMENSIONED `Scalar`
/// (e.g. `Scalar<Length>`) must NOT widen against a dimensionless `Complex`
/// (the runtime has no arm for it — `eval_add`/`eval_sub` in reify-expr only
/// promote `Value::Real`/`Value::Int`, never `Value::Scalar`, against
/// `Value::Complex`; see `is_dimensionless_complex`'s doc).
fn is_dimensionless_numeric(ty: &Type) -> bool {
    matches!(ty, Type::Int)
        || matches!(ty, Type::Scalar { dimension } if dimension.is_dimensionless())
}

/// Dimensionless `Complex` for `+`/`-` widening (see `is_dimensionless_numeric`).
///
/// A DIMENSIONED `Complex` (e.g. `Complex<Resistance>`) deliberately does NOT
/// match — the runtime's `guard_dimensionless_complex` (reify-expr) returns
/// `Value::Undef` for `Complex<Q> ± Real/Int` when `Q` is not dimensionless
/// (D3 policy), so the static side must not claim a result type there either.
fn is_dimensionless_complex(ty: &Type) -> bool {
    matches!(ty, Type::Complex(q) if matches!(q.as_ref(), Type::Scalar { dimension } if dimension.is_dimensionless()))
}

/// Exact complement of `is_dimensionless_complex`: a `Complex` wrapping a
/// non-dimensionless `Scalar` (e.g. `Complex<Length>`, `Complex<Resistance>`).
fn is_dimensioned_complex(ty: &Type) -> bool {
    matches!(ty, Type::Complex(q) if matches!(q.as_ref(), Type::Scalar { dimension } if !dimension.is_dimensionless()))
}

/// Symmetric reject predicate for `+`/`-` (task 5163): `true` when one
/// operand is a DIMENSIONED `Complex` and the other is a bare dimensionless
/// numeric (`Int`/`Scalar{DIMENSIONLESS}`) — the exact pairing the runtime
/// `guard_dimensionless_complex` (reify-expr) evaluates to `Value::Undef`
/// (D3 policy; see `is_dimensionless_complex`'s doc above). Covers BOTH
/// operand orders in one check, closing the order-dependent asymmetry
/// documented on `infer_binop_type`'s `Add`/`Sub` arm below
/// (`Complex<Length> + 1` vs `1 + Complex<Length>`).
///
/// `Type::Error`/`Type::TypeParam` (and every other non-matching kind)
/// operands satisfy neither `is_dimensioned_complex` nor
/// `is_dimensionless_numeric`, so gradualism is preserved structurally —
/// no explicit skip-set is needed here (unlike the broader Mul/Div
/// `is_mul_div_gradualism_skip`, whose `None`-partition is wider).
///
/// Reuses `is_dimensionless_numeric` unchanged for the bare-numeric side
/// (covers `Int` and `Real`, i.e. `Scalar{DIMENSIONLESS}`).
///
/// Pure and unit-tested independently of the pipeline (see the
/// `add_sub_dimensioned_complex_reject_*`/`is_dimensioned_complex_*` tests
/// below). Consumed by `expr.rs`'s `compile_binop` operand-kind guard
/// (mirrors the β2 Mul/Div `ArithOperandKind` guard) to poison
/// `result_type` to `Type::Error` and emit a diagnostic — this predicate
/// itself does not diagnose or poison.
pub(crate) fn add_sub_dimensioned_complex_reject(left: &Type, right: &Type) -> bool {
    (is_dimensioned_complex(left) && is_dimensionless_numeric(right))
        || (is_dimensionless_numeric(left) && is_dimensioned_complex(right))
}

/// Single source of truth for BOTH the correct static result type of `*`/`/`
/// AND the runtime-supported/unsupported partition (task compiler-type-hygiene
/// β2, INV-COMP-3).
///
/// `Some(ty)` — the operand-kind pair is one the runtime evaluator
/// (`eval_mul`/`eval_div` in `reify-expr`) has an INTENTIONAL arm for; `ty` is
/// the exact static result type for that arm. `None` — no intentional arm
/// exists (a **structural**, kind-level `Value::Undef`, not a data-dependent
/// one like divide-by-zero); the caller (the `expr.rs` operand-kind guard)
/// poisons to `Type::Error` and emits `DiagnosticCode::ArithOperandKind`.
///
/// Pinned row-for-row against the frozen runtime characterization suite:
/// `crates/reify-expr/tests/mul_div_runtime_truth_table.rs` (β1, task 5052).
/// Only that file's `INTENTIONAL` rows become `Some`; `STRUCTURAL-Undef`,
/// `degenerate-NOT-intentional`, and `Matrix-diagnostic` rows all become
/// `None`. `DATA-DRIVEN-Undef` rows (divide-by-zero) are excluded — they are
/// a runtime VALUE question, not a static TYPE question.
///
/// Callers: `infer_binop_type`'s `Mul`/`Div` arm (below) delegates here
/// directly via `mul_div_result_or_placeholder`. `expr.rs`'s `compile_binop`
/// calls this function directly, exactly ONCE per `*`/`/` expression, and
/// threads the resulting `Option` through both `mul_div_result_or_placeholder`
/// (for the static result type) and the operand-kind guard's poison decision
/// (task compiler-type-hygiene β2 amendment round 3 — previously the guard
/// called this function a second, independent time). Keeping both concerns
/// sourced from ONE evaluation makes the static table structurally unable to
/// disagree with itself (mirrors the `modulo_operands_are_int` /
/// `is_orderable_scalar` predicate-here / emission-in-`expr.rs` split).
///
/// Aggregate "scale" arms (`Vector`/`Point`/`Tensor` ⊗ scalar-like) recurse
/// this same function over the aggregate's quantity slot, mirroring the
/// runtime's `scale_components(.., eval_mul/eval_div, ..)`, which itself maps
/// `eval_mul`/`eval_div` over each component against the same "scalar"
/// operand — so a dimensioned "scalar" (e.g. `Scalar<Time>`) combines
/// dimensions with the quantity exactly as `Scalar ⊗ Scalar` would.
pub(crate) fn infer_mul_div_result(op: BinOp, left: &Type, right: &Type) -> Option<Type> {
    debug_assert!(
        matches!(op, BinOp::Mul | BinOp::Div),
        "infer_mul_div_result only handles BinOp::Mul/BinOp::Div, got {op:?}"
    );
    match (left, right) {
        // ── Numeric + Scalar core ────────────────────────────────────────────
        (Type::Int, Type::Int) => Some(Type::Int),

        (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd }) => Some(Type::Scalar {
            dimension: match op {
                BinOp::Mul => ld.mul(rd),
                BinOp::Div => ld.div(rd),
                _ => unreachable!(),
            },
        }),

        // Scalar ⊗ Int: Int carries no dimension, so both Mul and Div preserve
        // the Scalar's dimension unchanged.
        (Type::Scalar { dimension }, Type::Int) => Some(Type::Scalar {
            dimension: *dimension,
        }),
        // Int ⊗ Scalar: Mul is commutative with the above (preserve); Div is
        // the non-commutative reciprocal-dimension arm (`Int / Scalar<Time>`).
        (Type::Int, Type::Scalar { dimension }) => Some(Type::Scalar {
            dimension: match op {
                BinOp::Mul => *dimension,
                BinOp::Div => DimensionVector::DIMENSIONLESS.div(dimension),
                _ => unreachable!(),
            },
        }),

        // ── ScalarParam(Q) — dimension-kinded generic fn params ──────────────
        // `Type::ScalarParam(name)` (`Scalar<Q>` inside a `fn f<Q: Dimension>`
        // body, before call-site substitution binds Q) is a genuine,
        // well-formed scalar whose dimension is merely unresolved — the same
        // treatment `emit_comparison_operand_diagnostics` (expr.rs) already
        // gives it for Cmp ops: accepted directly by the predicate, NOT
        // skipped via the `Type::Error`/`Type::TypeParam` gradualism early-
        // return. Mirrors the `Scalar ⊗ Int` / `Scalar ⊗ Scalar` arms above
        // for the two cases whose result IS representable without inventing
        // compound dimension-expression algebra:
        //
        // - `ScalarParam(Q) ⊗ Int`: Int carries no dimension → preserve Q
        //   (both ops; Int⊗Scalar precedent above).
        // - `ScalarParam(Q) ⊗ Scalar{DIMENSIONLESS}` (i.e. `Scalar<Q> * Real`,
        //   the `scale_q<Q: Dimension>(x: Scalar<Q>, k: Real) -> Scalar<Q> {
        //   x * k }` pattern pinned by
        //   `fn_generic_call_inference_tests::dim_param_scale_q_resolves_at_two_dimensions`
        //   and `examples/generics/dim_param.ri`): DIMENSIONLESS is the
        //   multiplicative identity for dimension algebra → preserve Q.
        //
        // `ScalarParam ⊗ ScalarParam` and `ScalarParam ⊗` a NON-dimensionless
        // concrete `Scalar` are deliberately left unhandled (fall through to
        // `None` below): the combined dimension (e.g. "Q²" or "Q*Length")
        // is not representable by `ScalarParam`'s bare-name form. Extending
        // `ScalarParam` to carry a compound dimension expression is out of
        // scope for this fix.
        //
        // Unlike this function's OTHER `None` pairings (genuine runtime-
        // unsupported operand kinds), this is a static REPRESENTATIONAL gap
        // only: the combination is always runtime-legal once `Q` is
        // substituted with a concrete dimension (e.g.
        // `fn area<Q: Dimension>(x: Scalar<Q>) { x * x }` computes a valid
        // `Q²`-dimensioned value at every call site). So the `expr.rs`
        // operand-kind guard's gradualism skip DOES bypass a bare
        // `Type::ScalarParam` operand for exactly this reason (task
        // compiler-type-hygiene β2 amendment round 3) — mirrors its
        // `TypeParam`/`Projection` skips; see the guard's doc comment for the
        // full rationale. `infer_binop_type`'s `Mul`/`Div` arm correspondingly
        // propagates the `ScalarParam` itself (not `Type::Int`) for this
        // `None` case via `mul_div_result_or_placeholder` below, mirroring its
        // `Type::Projection` propagation, so the unresolved dimension
        // survives follow-on arithmetic instead of leaking as a spuriously-
        // concrete `Int`.
        (Type::ScalarParam(name), Type::Int) => Some(Type::ScalarParam(name.clone())),
        (Type::Int, Type::ScalarParam(name)) if op == BinOp::Mul => {
            Some(Type::ScalarParam(name.clone()))
        }
        (Type::ScalarParam(name), Type::Scalar { dimension }) if dimension.is_dimensionless() => {
            Some(Type::ScalarParam(name.clone()))
        }
        (Type::Scalar { dimension }, Type::ScalarParam(name))
            if op == BinOp::Mul && dimension.is_dimensionless() =>
        {
            Some(Type::ScalarParam(name.clone()))
        }

        // ── Aggregate scale: Vector/Point/Tensor ⊗ scalar-like ───────────────
        // `Aggregate / scalar-like` and `Aggregate * scalar-like` share one arm
        // (valid for both ops with the aggregate on the LEFT). The reverse
        // order (`scalar-like * Aggregate`) is Mul-only — Div has no
        // reverse-scale arm (non-commutative).
        (Type::Vector { n, quantity }, other) if is_mul_div_scalar_like(other) => {
            infer_mul_div_result(op, quantity, other).map(|q| Type::Vector {
                n: *n,
                quantity: Box::new(q),
            })
        }
        (other, Type::Vector { n, quantity })
            if op == BinOp::Mul && is_mul_div_scalar_like(other) =>
        {
            infer_mul_div_result(op, quantity, other).map(|q| Type::Vector {
                n: *n,
                quantity: Box::new(q),
            })
        }
        (Type::Point { n, quantity }, other) if is_mul_div_scalar_like(other) => {
            infer_mul_div_result(op, quantity, other).map(|q| Type::Point {
                n: *n,
                quantity: Box::new(q),
            })
        }
        (other, Type::Point { n, quantity })
            if op == BinOp::Mul && is_mul_div_scalar_like(other) =>
        {
            infer_mul_div_result(op, quantity, other).map(|q| Type::Point {
                n: *n,
                quantity: Box::new(q),
            })
        }
        (Type::Tensor { rank, n, quantity }, other) if is_mul_div_scalar_like(other) => {
            infer_mul_div_result(op, quantity, other).map(|q| Type::Tensor {
                rank: *rank,
                n: *n,
                quantity: Box::new(q),
            })
        }
        (other, Type::Tensor { rank, n, quantity })
            if op == BinOp::Mul && is_mul_div_scalar_like(other) =>
        {
            infer_mul_div_result(op, quantity, other).map(|q| Type::Tensor {
                rank: *rank,
                n: *n,
                quantity: Box::new(q),
            })
        }

        // ── Complex(q) ────────────────────────────────────────────────────────
        // Complex×Complex and Complex×Scalar COMBINE dimensions (mul/div,
        // matching the Scalar⊗Scalar core); Complex×Int PRESERVES the
        // Complex's dimension (Int carries none to combine). Div requires
        // Complex on the LEFT (numerator) — no reverse arm, same
        // non-commutativity as the aggregate-scale Div arms above.
        (Type::Complex(lq), Type::Complex(rq)) => match (lq.as_ref(), rq.as_ref()) {
            (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd }) => {
                Some(Type::complex(Type::Scalar {
                    dimension: match op {
                        BinOp::Mul => ld.mul(rd),
                        BinOp::Div => ld.div(rd),
                        _ => unreachable!(),
                    },
                }))
            }
            _ => None,
        },
        (Type::Complex(cq), Type::Scalar { dimension: sd }) => match cq.as_ref() {
            Type::Scalar { dimension: cd } => Some(Type::complex(Type::Scalar {
                dimension: match op {
                    BinOp::Mul => cd.mul(sd),
                    BinOp::Div => cd.div(sd),
                    _ => unreachable!(),
                },
            })),
            _ => None,
        },
        (Type::Scalar { dimension: sd }, Type::Complex(cq)) if op == BinOp::Mul => {
            match cq.as_ref() {
                Type::Scalar { dimension: cd } => Some(Type::complex(Type::Scalar {
                    dimension: cd.mul(sd),
                })),
                _ => None,
            }
        }
        (Type::Complex(cq), Type::Int) => Some(Type::complex(cq.as_ref().clone())),
        (Type::Int, Type::Complex(cq)) if op == BinOp::Mul => {
            Some(Type::complex(cq.as_ref().clone()))
        }

        // ── Transform(n) — Mul only, matching n required ────────────────────
        // `Transform × Vector -> Vector`, `Transform × Point -> Point`,
        // `Transform × Transform -> Transform` (row-9 pin). Order-sensitive:
        // there is no reverse (`Vector/Point/Transform × Transform`) arm —
        // Div is entirely unsupported for Transform (no runtime arm at all).
        (Type::Transform(n1), Type::Vector { n: n2, quantity }) if op == BinOp::Mul && n1 == n2 => {
            Some(Type::Vector {
                n: *n2,
                quantity: quantity.clone(),
            })
        }
        (Type::Transform(n1), Type::Point { n: n2, quantity }) if op == BinOp::Mul && n1 == n2 => {
            Some(Type::Point {
                n: *n2,
                quantity: quantity.clone(),
            })
        }
        (Type::Transform(n1), Type::Transform(n2)) if op == BinOp::Mul && n1 == n2 => {
            Some(Type::Transform(*n1))
        }

        // Every other operand-kind pairing (aggregate×aggregate; degenerate
        // Tensor×Vector; order-reversed Vector/Point×Transform; Matrix in
        // either position; List/String/Bool; non-commutative Div reversals;
        // and `Type::Applied`/`Type::StructureRef`/`Type::Union` nominal
        // struct/union types) has no runtime-intentional arm and is
        // INTENTIONALLY `None`: none of these are in the
        // `is_mul_div_gradualism_skip` deferred set below, so they correctly
        // poison + emit `E_ArithOperandKind` rather than silently mistyping to
        // `Int`.
        _ => None,
    }
}

/// Infer the result type of a binary operation given operand types.
pub(crate) fn infer_binop_type(op: BinOp, left: &Type, right: &Type) -> Type {
    // Anti-cascade guard (task-448): if either operand is already poisoned,
    // propagate Type::Error so downstream sites don't emit follow-on diagnostics.
    if left.is_error() || right.is_error() {
        return Type::Error;
    }
    // Gradualism propagation (task #4629, W5): an unresolved `TypeParam` operand
    // (e.g. a generic-`Structure` purpose-subject member access typed as
    // `TypeParam("StructureMember")`) must survive ARITHMETIC so the downstream
    // comparison/dimension guards keep early-returning on `TypeParam` instead of
    // adjudicating a spuriously-collapsed concrete type.  Without this, e.g.
    // `let m = subject.a - subject.b  let n = m * 2  constraint n > 0mm` typed
    // `n` as `Int` (the `BinOp::Mul` `_ => Type::Int` fallthrough below), so
    // `n > 0mm` produced a false `Int vs Scalar[m]` mismatch in a generic body.
    // Comparison/logical ops are unaffected: they return `Type::Bool` regardless,
    // and their operand guards handle the `TypeParam` gradualism case directly.
    if matches!(
        op,
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow
    ) {
        if let Type::TypeParam(_) = left {
            return left.clone();
        }
        if let Type::TypeParam(_) = right {
            return right.clone();
        }
    }
    match op {
        BinOp::Eq
        | BinOp::Ne
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Gt
        | BinOp::Ge
        | BinOp::And
        | BinOp::Or
        | BinOp::Implies => Type::Bool,
        // Same dimension required, EXCEPT: a bare dimensionless number
        // (`Int`/`Real`) widens against a dimensionless `Complex` (mirrors the
        // runtime's `guard_dimensionless_complex` in reify-expr's
        // eval_add/eval_sub). Needed for imaginary-literal sugar `n + mj`,
        // which desugars to `n + complex(0, m)` (reify-syntax
        // `lower_imaginary_literal`) — without this arm `w = 3 + 4j` statically
        // typed `Int` (bare `left.clone()`), silently discarding the whole
        // expression's Complex-ness (only surfaced once the β2 Mul/Div guard
        // started rejecting the resulting `Int / Complex` as
        // `E_ArithOperandKind` on e.g. `w / complex(1.0, 2.0)`). A DIMENSIONED
        // Complex operand does not widen — falls through to the unchanged
        // `left.clone()` fallback, same as before this arm existed.
        //
        // CLOSED (task compiler-type-hygiene follow-up 5163): a DIMENSIONED
        // `Complex` + Int/Real (e.g. `Complex<Length> + 1`, either operand
        // order) still statically claims a placeholder type here via the
        // `left.clone()` fallback below — this arm is intentionally
        // UNCHANGED, mirroring the β2 Mul/Div arm's own `Int` placeholder —
        // but the `expr.rs` `compile_binop` operand-kind guard
        // (`type_compat::add_sub_dimensioned_complex_reject`) now overrides
        // `result_type` to `Type::Error` and emits `E_ArithOperandKind`
        // whenever this placeholder would otherwise leak downstream, for
        // BOTH operand orders (closing the order-dependent asymmetry: previously
        // `Complex<Length> + Int` preserved `Complex<Length>` while the
        // reversed `Int + Complex<Length>` silently collapsed to bare `Int`
        // with no diagnostic at all). The placeholder value itself is still
        // pinned by `binop_add_dimensioned_complex_plus_int_does_not_widen`
        // and `binop_add_int_plus_dimensioned_complex_does_not_widen` below
        // (the pure-fn placeholder the guard overrides, same pattern as the
        // Mul/Div placeholder pins); the observable end-to-end behavior is
        // covered by `crates/reify-compiler/tests/add_sub_operand_guard_tests.rs`.
        //
        // SCOPE (code-review confirmation, task 5061 amendment pass): the
        // dimensionless-Complex widening arm above is intentionally folded
        // into β2 rather than split into a separate change — it is a direct
        // prerequisite for β2's own Mul/Div guard, needed to avoid a spurious
        // `E_ArithOperandKind` on the imaginary-literal-sugar path described
        // above (`w = 3 + 4j` followed by e.g. `w / complex(1.0, 2.0)`), so
        // it cannot be bisected away from the Mul/Div guard without
        // reintroducing that false positive.
        BinOp::Add | BinOp::Sub => {
            if is_dimensionless_complex(left) && is_dimensionless_numeric(right) {
                left.clone()
            } else if is_dimensionless_numeric(left) && is_dimensionless_complex(right) {
                right.clone()
            } else {
                left.clone()
            }
        }
        // Delegates to the single source of truth for both the correct static
        // result type and the runtime-supported/unsupported partition (β2,
        // INV-COMP-3) — see `infer_mul_div_result`'s doc. `None` collapses via
        // `mul_div_result_or_placeholder`, which propagates an
        // `is_mul_div_gradualism_skip` operand (`Error`/`TypeParam`/
        // `Projection`/`ScalarParam`) unchanged — so an unresolved or poisoned
        // operand survives arithmetic instead of leaking downstream as a
        // spuriously-concrete `Int` a later guard could misjudge — and
        // otherwise falls back to `Type::Int`, a placeholder the `expr.rs`
        // guard poisons + diagnoses (mirrors the Mod/Pow precedent).
        BinOp::Mul | BinOp::Div => {
            mul_div_result_or_placeholder(infer_mul_div_result(op, left, right), left, right)
        }
        BinOp::Mod => left.clone(),
        BinOp::Pow => left.clone(), // simplified for M1
    }
}

/// The Mul/Div "gradualism" skip-set: operand kinds that must be deferred
/// rather than adjudicated as runtime-unsupported. Single source of truth for
/// two decisions that previously lived in two independently-maintained copies
/// — the `expr.rs` operand-kind guard's skip check, and this file's
/// `mul_div_result_or_placeholder` `None`-collapse cascade below — kept in
/// lockstep only by convention until amendment round 4 factored both out to
/// this one predicate.
///
/// Matches four variants, each deferred for a different reason:
/// - `Type::Error` — already-poisoned (anti-cascade); takes priority over the
///   other three when an operand pair mixes kinds (see
///   `mul_div_result_or_placeholder`'s explicit `is_error` priority check,
///   which mirrors `infer_binop_type`'s own pre-match early-return).
/// - `Type::TypeParam(_)` — unresolved auto/generic type (task #4629 W5).
/// - `Type::Projection { .. }` — an unresolved trait-associated-type
///   reference (e.g. `P::MotionValue` inside a generic `structure def` that
///   declares `P`, before a concrete arg substitutes it). Matched broadly
///   (any `base`) because per `resolve_qualified_assoc_type`'s doc
///   (type_resolution.rs) that is the only shape reachable here — every
///   other base either normalizes to a concrete type or poisons to
///   `Type::Error` first.
/// - `Type::ScalarParam(_)` — a dimension-kinded generic param (`Scalar<Q>`
///   before `Q` substitutes). Unlike the other three this is a STATIC
///   REPRESENTATIONAL gap, not a runtime-unsupported one: `ScalarParam ⊗
///   ScalarParam` is always runtime-legal once substituted, `ScalarParam`
///   just can't represent the combined dimension yet (see
///   `infer_mul_div_result`'s `ScalarParam` arms for the full rationale).
///
/// `Type::Applied`/`Type::StructureRef`/`Type::Union` are deliberately NOT in
/// this set: they are concrete nominal/structural types the runtime has no
/// `eval_mul`/`eval_div` arm for regardless of substitution, so hard-erroring
/// on them (rather than silently mistyping to `Int`, the pre-β2 behavior) is
/// this task's purpose.
pub(crate) fn is_mul_div_gradualism_skip(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Error | Type::TypeParam(_) | Type::Projection { .. } | Type::ScalarParam(_)
    )
}

/// Resolves `infer_mul_div_result`'s `Option<Type>` to the concrete static
/// `Type` used both as `infer_binop_type`'s `Mul`/`Div` result (above) and,
/// via `expr.rs::compile_binop` calling this function directly, as the input
/// to the operand-kind guard's poison decision.
///
/// `Some(ty)` passes through unchanged. `None` collapses to the pre-β2
/// `Type::Int` placeholder (which the `expr.rs` guard then poisons to
/// `Type::Error`), EXCEPT when an operand matches `is_mul_div_gradualism_skip`
/// above — then that operand's own type propagates unchanged instead, so an
/// unresolved/poisoned operand survives arithmetic rather than leaking
/// downstream as a spuriously-concrete `Int` a later guard could misjudge.
/// `Type::Error` takes priority when operands mix skip kinds, matching
/// `infer_binop_type`'s own pre-match early-return.
///
/// Regression pins: `mul_div_result_or_placeholder_error_propagates_not_int`,
/// `_int_times_error_propagates_not_int`, `_type_param_propagates_not_int`,
/// `_int_times_type_param_propagates_not_int` below, plus the `Projection`/
/// `ScalarParam` pins nearby. Integration-level symptom of the gap this
/// closes: `undef_literal_compile_tests::binary_with_undef_emits_no_unresolved_name_diagnostic`.
pub(crate) fn mul_div_result_or_placeholder(
    inferred: Option<Type>,
    left: &Type,
    right: &Type,
) -> Type {
    inferred.unwrap_or_else(|| {
        if left.is_error() || right.is_error() {
            Type::Error
        } else if is_mul_div_gradualism_skip(left) {
            left.clone()
        } else if is_mul_div_gradualism_skip(right) {
            right.clone()
        } else {
            Type::Int
        }
    })
}

/// Attempt to satisfy a `NoMatch` call via default-padding.
///
/// Searches `named` for the UNIQUE same-name candidate where:
/// - the candidate has more params than `provided` args,
/// - the provided prefix `arg_types[..provided]` matches `cand.params[..provided]`
///   using the same trait/type-param wildcard predicate as
///   `resolve_function_overload` (see below), and
/// - every trailing `cand.param_defaults[provided..]` is `Some`.
///
/// **Prefix predicate (mirrors `resolve_function_overload`):**
/// For each `(param_ty, arg_ty)` pair in the provided prefix, the pair
/// *matches* when any of:
/// - `type_carries_trait_object(param_ty)` — trait-object param is a wildcard;
///   the concrete arg type's trait conformance is validated downstream by
///   `phase_fn_arg_conformance`, not here.
/// - `is_generic && type_carries_type_param(param_ty)` — type-param-carrying
///   param in a generic candidate is a wildcard (gated on non-empty `type_params`
///   so concrete candidates are unaffected — INV-6).
/// - `type_carries_type_param(arg_ty)` — a TypeParam-typed arg (inside a
///   generic fn body) matches any param type (D4, task-4232 γ).
/// - `param_ty == arg_ty` — exact equality for concrete params.
///
/// This alignment is intentional: a call padded with defaults is compiled as a
/// normal `UserFunctionCall` whose trait-arg conformance is checked by
/// `phase_fn_arg_conformance`. Using stricter prefix semantics here than in the
/// overload resolver created a gap where `options` defaults on solver functions
/// were unreachable for any call involving trait-typed leading params (e.g. a
/// `ConstitutiveLaw`/`ElasticMaterial` material arg or a `List<Load>` loads
/// arg). (task-4544.)
///
/// `provided` is `arg_types.len()` — callers no longer pass `compiled_args`
/// because only its length was used and `arg_types` is always length-aligned
/// to `compiled_args` by construction (task-3702).
///
/// If exactly one such candidate exists, returns it together with the cloned default
/// `CompiledExpr`s for the trailing params. When multiple candidates are satisfiable,
/// uses specificity scoring to break the tie: counts the provided-prefix params where
/// `param_ty == arg_ty` (strict equality, same predicate as
/// `resolve_function_overload`'s exact-match tie-break) and selects the candidate
/// with the strictly-greatest count; on an equal-count tie returns `None` (genuine
/// ambiguity → caller's NoMatch, preserving today's ambiguity-as-NoMatch UX).
/// Returns `None` when zero candidates are satisfiable (caller falls through to the
/// existing NoMatch error). (task-4788 / esc-4757-65.)
///
/// **Invariant:** every candidate in `named` must satisfy
/// `param_defaults.len() == params.len()` (task-3702 strict alignment now
/// enforced by `debug_assert!`). Violations are programming errors, not
/// recoverable call-site conditions.
pub(crate) fn try_default_padding<'a>(
    named: &[&'a CompiledFunction],
    arg_types: &[Type],
) -> Option<(&'a CompiledFunction, Vec<CompiledExpr>)> {
    let provided = arg_types.len();
    let mut satisfiable: Vec<(&CompiledFunction, Vec<CompiledExpr>)> = Vec::new();

    for &cand in named {
        // Candidate must have strictly more params than provided args.
        if cand.params.len() <= provided {
            continue;
        }
        // Strict invariant: param_defaults must be length-aligned to params.
        // Violations are bugs — surface them in debug builds. In release builds
        // the assert is compiled out, so we also `continue` on mismatch to
        // degrade gracefully instead of panicking on a future invariant-breaking
        // producer (task-3702 amendment-2).
        debug_assert!(
            cand.param_defaults.len() == cand.params.len(),
            "param_defaults.len() == params.len() invariant violated for candidate `{}` (task-3702): expected {}, got {}",
            cand.name,
            cand.params.len(),
            cand.param_defaults.len()
        );
        if cand.param_defaults.len() != cand.params.len() {
            continue;
        }
        // Provided prefix types must match candidate params using the same
        // trait/type-param wildcard predicate as `resolve_function_overload`.
        // See the function-level doc for the full rationale.
        let is_generic = !cand.type_params.is_empty();
        let prefix_matches = cand.params[..provided]
            .iter()
            .zip(arg_types[..provided].iter())
            .all(|((_, param_ty), arg_ty)| {
                type_carries_trait_object(param_ty)
                    || (is_generic
                        && (type_carries_type_param(param_ty) || type_carries_dim_param(param_ty)))
                    || type_carries_type_param(arg_ty)
                    || param_ty == arg_ty
            });
        if !prefix_matches {
            continue;
        }
        // All trailing params must carry Some compiled default.
        let defaults: Option<Vec<CompiledExpr>> =
            cand.param_defaults[provided..].iter().cloned().collect();
        if let Some(defaults) = defaults {
            satisfiable.push((cand, defaults));
        }
    }

    match satisfiable.len() {
        1 => Some(satisfiable.into_iter().next().unwrap()),
        0 => None,
        _ => {
            // Multiple candidates pass the wildcard prefix — use specificity
            // scoring to break the tie.  Score each candidate by counting the
            // provided-prefix params where `param_ty == arg_ty` (strict
            // equality, the same predicate as `resolve_function_overload`'s
            // exact-match tie-break, NOT the wildcard-relaxed satisfiability
            // predicate).  Select the candidate with the strictly-greatest
            // count; on an equal-count tie (including the degenerate
            // all-wildcard score-0 case) return None and let the caller fall
            // through to its generic NoMatch error.
            //
            // This is a strict generalization of the old binary "filter to the
            // all-exact subset": a fully-exact candidate attains the maximum
            // possible count (== provided) and still wins, so no
            // currently-resolving call changes.  The change only newly-resolves
            // calls where one overload is strictly more specific than all
            // others (e.g. overload A scores 3 / overload B scores 4 — B wins
            // even though neither is all-exact).  On a genuine equal-count tie
            // we still surface "no matching overload" rather than a dedicated
            // Ambiguous diagnostic — the intentional UX trade-off preserved
            // from the original arm.  (task-4788 / esc-4757-65.)
            // Scores are precomputed in a single O(N·provided) pass so the
            // subsequent max/filter walk reads from the cache — no double
            // computation of the inner loop.
            let scored: Vec<(usize, (&CompiledFunction, Vec<CompiledExpr>))> = satisfiable
                .into_iter()
                .map(|(cand, defaults)| {
                    let score = cand.params[..provided]
                        .iter()
                        .zip(arg_types[..provided].iter())
                        .map(|((_, param_ty), arg_ty)| usize::from(param_ty == arg_ty))
                        .sum::<usize>();
                    (score, (cand, defaults))
                })
                .collect();
            // scored is non-empty: this arm is only entered from the `_ =>` branch,
            // i.e. satisfiable.len() >= 2, so the unwrap_or(0) fallback is unreachable.
            let max_score = scored.iter().map(|(s, _)| *s).max().unwrap_or(0);
            let winners: Vec<_> = scored
                .into_iter()
                .filter(|(s, _)| *s == max_score)
                .map(|(_, entry)| entry)
                .collect();
            match winners.len() {
                1 => Some(winners.into_iter().next().unwrap()),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Anti-cascade guard tests (task-448): `Type::Error` operands must
    //! propagate `Type::Error`, not fall back to any op-specific result type.
    //!
    //! Renamed from `infer_binop_type_error_tests` per amendment-round-2 S5
    //! to match the codebase-standard `mod tests` convention.
    use super::*;

    // ── task-4081: resolve_function_overload trait-param wildcard ────────────

    /// Helper: build a minimal stub body returning Real(0.0).
    fn stub_body() -> CompiledFnBody {
        CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar()),
        }
    }

    /// Build a minimal `CompiledFunction` with the given name and params.
    fn make_fn(name: &str, params: Vec<(&str, Type)>) -> CompiledFunction {
        let params: Vec<(String, Type)> = params
            .into_iter()
            .map(|(n, t)| (n.to_string(), t))
            .collect();
        let param_defaults = CompiledFunction::no_defaults_for(&params);
        CompiledFunction {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            params,
            param_defaults,
            return_type: Type::dimensionless_scalar(),
            body: stub_body(),
            content_hash: ContentHash::of_str(name),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        }
    }

    /// (a) A single TraitObject param matches any arg type (StructureRef).
    /// RED until step-2: current exact-match makes this NoMatch.
    #[test]
    fn overload_trait_param_matches_any_structure_ref_arg() {
        let fns = vec![make_fn(
            "f",
            vec![("j", Type::TraitObject("DrivingJoint".to_string()))],
        )];
        let result = resolve_function_overload("f", &[Type::StructureRef("X".to_string())], &fns);
        assert!(
            matches!(result, OverloadResolution::Resolved(_)),
            "trait-object param should resolve against any StructureRef arg"
        );
    }

    /// (a2) A single TraitObject param also matches a different TraitObject arg.
    /// RED until step-2.
    #[test]
    fn overload_trait_param_matches_any_trait_object_arg() {
        let fns = vec![make_fn(
            "f",
            vec![("j", Type::TraitObject("DrivingJoint".to_string()))],
        )];
        let result =
            resolve_function_overload("f", &[Type::TraitObject("Other".to_string())], &fns);
        assert!(
            matches!(result, OverloadResolution::Resolved(_)),
            "trait-object param should resolve against any TraitObject arg"
        );
    }

    /// (b) Mixed fn: trait param is a wildcard, concrete param keeps exact equality.
    /// `fn g(j: TraitObject("DrivingJoint"), k: Real)` — calling with (X, Int)
    /// must NOT resolve because the concrete Real param doesn't accept Int.
    /// RED until step-2 (currently both params fail exact-equality, same result).
    #[test]
    fn overload_mixed_fn_concrete_param_still_requires_exact_type() {
        let fns = vec![make_fn(
            "g",
            vec![
                ("j", Type::TraitObject("DrivingJoint".to_string())),
                ("k", Type::dimensionless_scalar()),
            ],
        )];
        // arg k is Int, not Real → no match
        let result =
            resolve_function_overload("g", &[Type::StructureRef("X".to_string()), Type::Int], &fns);
        assert!(
            matches!(result, OverloadResolution::NoMatch(_)),
            "concrete Real param must not accept Int; expected NoMatch"
        );
    }

    /// (c) Baseline all-concrete fn is unchanged: Real matches, Int does not.
    /// Must hold both before and after step-2 (no regression).
    #[test]
    fn overload_all_concrete_fn_unchanged() {
        let fns = vec![make_fn("h", vec![("x", Type::dimensionless_scalar())])];
        let resolved = resolve_function_overload("h", &[Type::dimensionless_scalar()], &fns);
        assert!(
            matches!(resolved, OverloadResolution::Resolved(_)),
            "h(Real) should resolve on Real arg"
        );
        let no_match = resolve_function_overload("h", &[Type::Int], &fns);
        assert!(
            matches!(no_match, OverloadResolution::NoMatch(_)),
            "h(Real) should not resolve on Int arg"
        );
    }

    // --- format_dimension_mismatch_diagnostic tests (step-5) ---

    fn test_span() -> SourceSpan {
        SourceSpan::new(0, 10)
    }

    fn money_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::MONEY,
        }
    }

    fn force_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::FORCE,
        }
    }

    fn length_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }
    }

    fn mass_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::MASS,
        }
    }

    /// (a) Money-vs-Force produces a secondary label naming both dimensions.
    #[test]
    fn fmt_dim_mismatch_money_vs_force_has_secondary_label() {
        let d =
            format_dimension_mismatch_diagnostic("addition", &money_ty(), &force_ty(), test_span());
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));
        assert!(
            d.message.contains("dimension mismatch in addition:"),
            "message was: {}",
            d.message
        );
        assert!(
            d.labels.len() >= 2,
            "expected at least 2 labels, got {}",
            d.labels.len()
        );
        let has_canonical_hint = d
            .labels
            .iter()
            .any(|l| l.message.contains("Money") && l.message.contains("Force"));
        assert!(
            has_canonical_hint,
            "no label mentions both 'Money' and 'Force'; labels: {:?}",
            d.labels.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }

    /// (b) Reverse polarity (Force on left, Money on right) produces the same secondary label.
    #[test]
    fn fmt_dim_mismatch_force_vs_money_has_secondary_label() {
        let d =
            format_dimension_mismatch_diagnostic("addition", &force_ty(), &money_ty(), test_span());
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));
        let has_canonical_hint = d
            .labels
            .iter()
            .any(|l| l.message.contains("Money") && l.message.contains("Force"));
        assert!(
            has_canonical_hint,
            "no label mentions both 'Money' and 'Force'; labels: {:?}",
            d.labels.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }

    /// (c) Length-vs-Mass produces secondary label naming both.
    #[test]
    fn fmt_dim_mismatch_length_vs_mass_has_secondary_label() {
        let d =
            format_dimension_mismatch_diagnostic("addition", &length_ty(), &mass_ty(), test_span());
        let has_canonical_hint = d
            .labels
            .iter()
            .any(|l| l.message.contains("Length") && l.message.contains("Mass"));
        assert!(
            has_canonical_hint,
            "no label mentions both 'Length' and 'Mass'; labels: {:?}",
            d.labels.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }

    /// (d) Composite-vs-named produces ONLY the primary "incompatible dimensions" label (no canonical-names hint),
    /// but still attaches the code.
    #[test]
    fn fmt_dim_mismatch_composite_vs_named_no_secondary_label() {
        let composite = Type::Scalar {
            dimension: DimensionVector::MONEY.div(&DimensionVector::MASS),
        };
        let d =
            format_dimension_mismatch_diagnostic("addition", &composite, &force_ty(), test_span());
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));
        // There should be exactly one label (the primary "incompatible dimensions" label).
        assert_eq!(
            d.labels.len(),
            1,
            "expected exactly 1 label for composite-vs-named, got {}",
            d.labels.len()
        );
        assert_eq!(d.labels[0].message, "incompatible dimensions");
    }

    /// (e) Non-Scalar operands do not panic and still produce a diagnostic with code.
    /// Covers the three asymmetric/symmetric non-Scalar cases the helper may receive:
    /// (Real, Scalar), (Scalar, Real), and (Real, Real).
    #[test]
    fn fmt_dim_mismatch_non_scalar_does_not_panic() {
        // Left non-Scalar, right Scalar
        let d = format_dimension_mismatch_diagnostic(
            "addition",
            &Type::dimensionless_scalar(),
            &force_ty(),
            test_span(),
        );
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));

        // Left Scalar, right non-Scalar
        let d = format_dimension_mismatch_diagnostic(
            "addition",
            &money_ty(),
            &Type::dimensionless_scalar(),
            test_span(),
        );
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));

        // Both non-Scalar
        let d = format_dimension_mismatch_diagnostic(
            "addition",
            &Type::dimensionless_scalar(),
            &Type::dimensionless_scalar(),
            test_span(),
        );
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));
    }

    #[test]
    fn binop_add_left_error_yields_error() {
        assert_eq!(
            infer_binop_type(BinOp::Add, &Type::Error, &Type::Int),
            Type::Error,
        );
    }

    /// Imaginary-literal sugar `n + mj` desugars to `n + complex(0, m)`
    /// (reify-syntax `lower_imaginary_literal`) — `Int + Complex{DIMENSIONLESS}`
    /// must statically widen to `Complex`, not fall to bare `left.clone()`
    /// (`Int`). Mirrors the runtime's `guard_dimensionless_complex` arm in
    /// reify-expr's `eval_add`. Regression pin for the `w = 3 + 4j` /
    /// `w / complex(1.0, 2.0)` chain (compiler-type-hygiene β2 follow-on —
    /// this combination silently mistyped `w` as `Int`, which the new
    /// Mul/Div `E_ArithOperandKind` guard then correctly-but-spuriously
    /// rejected on `w_div`).
    #[test]
    fn binop_add_int_plus_dimensionless_complex_widens_to_complex() {
        let c = Type::complex(Type::dimensionless_scalar());
        assert_eq!(infer_binop_type(BinOp::Add, &Type::Int, &c), c);
    }

    #[test]
    fn binop_add_dimensionless_complex_plus_int_stays_complex() {
        let c = Type::complex(Type::dimensionless_scalar());
        assert_eq!(infer_binop_type(BinOp::Add, &c, &Type::Int), c);
    }

    #[test]
    fn binop_add_real_plus_dimensionless_complex_widens_to_complex() {
        // `Real` is not a distinct `Type` variant — `Scalar{DIMENSIONLESS}`
        // covers the `3.2 + 4.1j` case (complex_literals.ri) for free.
        let c = Type::complex(Type::dimensionless_scalar());
        assert_eq!(
            infer_binop_type(BinOp::Add, &Type::dimensionless_scalar(), &c),
            c
        );
    }

    #[test]
    fn binop_sub_int_minus_dimensionless_complex_widens_to_complex() {
        let c = Type::complex(Type::dimensionless_scalar());
        assert_eq!(infer_binop_type(BinOp::Sub, &Type::Int, &c), c);
    }

    /// Sub-direction counterpart of `binop_add_dimensionless_complex_plus_int_stays_complex`
    /// with the dimensionless `Complex` on the LEFT — exercises the
    /// `is_dimensionless_complex(left) && is_dimensionless_numeric(right)` branch
    /// under `BinOp::Sub` specifically (previously only exercised for `Add`).
    #[test]
    fn binop_sub_dimensionless_complex_minus_int_stays_complex() {
        let c = Type::complex(Type::dimensionless_scalar());
        assert_eq!(infer_binop_type(BinOp::Sub, &c, &Type::Int), c);
    }

    #[test]
    fn binop_add_dimensioned_complex_plus_int_does_not_widen() {
        // D3 policy (reify-expr `guard_dimensionless_complex`): a DIMENSIONED
        // Complex does not promote against a bare Int/Real at runtime (evals
        // Undef) — the static side must not claim a result type either, so
        // this combination is deliberately left on the pre-existing
        // `left.clone()` fallback (unchanged by this fix). This pins the
        // CURRENT (mistyped) behavior, not the desired one — closing the gap
        // is tracked separately, see TODO(#5163) above this match arm.
        let dimensioned = Type::complex(Type::length());
        assert_eq!(
            infer_binop_type(BinOp::Add, &dimensioned, &Type::Int),
            dimensioned
        );
    }

    /// Order-reversed counterpart of `binop_add_dimensioned_complex_plus_int_does_not_widen`
    /// above: with the DIMENSIONED Complex on the RIGHT, neither widening
    /// branch fires (`is_dimensionless_complex(right)` is false — the Complex
    /// is dimensioned, not dimensionless), so the `else` fallthrough returns
    /// `left.clone()` = the bare `Int`, NOT the Complex. This is the
    /// order-dependent asymmetry documented in the TODO(#5163) comment above
    /// this match arm: `Complex<Length> + Int` preserves `Complex<Length>`
    /// (previous test) but `Int + Complex<Length>` collapses to bare `Int`.
    /// Pins the CURRENT (accepted-gap) behavior, not the desired one —
    /// closing the gap in both directions is tracked by #5163.
    #[test]
    fn binop_add_int_plus_dimensioned_complex_does_not_widen() {
        let dimensioned = Type::complex(Type::length());
        assert_eq!(
            infer_binop_type(BinOp::Add, &Type::Int, &dimensioned),
            Type::Int
        );
    }

    // ── task-5163: is_dimensioned_complex / add_sub_dimensioned_complex_reject ──

    #[test]
    fn is_dimensioned_complex_true_for_dimensioned_quantity() {
        assert!(is_dimensioned_complex(&Type::complex(Type::length())));
    }

    #[test]
    fn is_dimensioned_complex_false_for_dimensionless_quantity() {
        assert!(!is_dimensioned_complex(&Type::complex(
            Type::dimensionless_scalar()
        )));
    }

    #[test]
    fn is_dimensioned_complex_false_for_non_complex() {
        assert!(!is_dimensioned_complex(&Type::length()));
        assert!(!is_dimensioned_complex(&Type::Int));
    }

    #[test]
    fn add_sub_dimensioned_complex_reject_true_for_dimensioned_complex_plus_int() {
        let dimensioned = Type::complex(Type::length());
        assert!(add_sub_dimensioned_complex_reject(&dimensioned, &Type::Int));
    }

    /// Order-reversed counterpart: closes the documented asymmetry — both
    /// operand orders must reject.
    #[test]
    fn add_sub_dimensioned_complex_reject_true_for_int_plus_dimensioned_complex_order_reversed() {
        let dimensioned = Type::complex(Type::length());
        assert!(add_sub_dimensioned_complex_reject(&Type::Int, &dimensioned));
    }

    #[test]
    fn add_sub_dimensioned_complex_reject_true_for_dimensioned_complex_plus_dimensionless_scalar()
    {
        let dimensioned = Type::complex(Type::length());
        assert!(add_sub_dimensioned_complex_reject(
            &dimensioned,
            &Type::dimensionless_scalar()
        ));
    }

    #[test]
    fn add_sub_dimensioned_complex_reject_true_for_dimensionless_scalar_plus_dimensioned_complex()
    {
        let dimensioned = Type::complex(Type::length());
        assert!(add_sub_dimensioned_complex_reject(
            &Type::dimensionless_scalar(),
            &dimensioned
        ));
    }

    /// Must NOT reject — this is the pre-existing D3 widening case (`3 + 4j`).
    #[test]
    fn add_sub_dimensioned_complex_reject_false_for_dimensionless_complex_plus_int_widening_case()
    {
        let dimensionless = Type::complex(Type::dimensionless_scalar());
        assert!(!add_sub_dimensioned_complex_reject(
            &dimensionless,
            &Type::Int
        ));
    }

    /// `Complex<Q1> ± Complex<Q2>` dimension-mismatch is a separate, unguarded
    /// gap (out of scope for task 5163) — this predicate must not touch it.
    #[test]
    fn add_sub_dimensioned_complex_reject_false_for_complex_plus_complex_out_of_scope() {
        let dimensioned = Type::complex(Type::length());
        assert!(!add_sub_dimensioned_complex_reject(
            &dimensioned,
            &dimensioned
        ));
    }

    #[test]
    fn add_sub_dimensioned_complex_reject_false_for_error_left_gradualism() {
        let dimensioned = Type::complex(Type::length());
        assert!(!add_sub_dimensioned_complex_reject(
            &Type::Error,
            &dimensioned
        ));
    }

    #[test]
    fn add_sub_dimensioned_complex_reject_false_for_error_right_gradualism() {
        let dimensioned = Type::complex(Type::length());
        assert!(!add_sub_dimensioned_complex_reject(
            &dimensioned,
            &Type::Error
        ));
    }

    #[test]
    fn add_sub_dimensioned_complex_reject_false_for_type_param_gradualism() {
        let dimensioned = Type::complex(Type::length());
        assert!(!add_sub_dimensioned_complex_reject(
            &Type::TypeParam("T".into()),
            &dimensioned
        ));
    }

    /// Plain dimensioned `Scalar` (not `Complex`) + `Int` is handled by the
    /// pre-existing dimension-compat block in `expr.rs`, not this predicate.
    #[test]
    fn add_sub_dimensioned_complex_reject_false_for_dimensioned_scalar_plus_int() {
        assert!(!add_sub_dimensioned_complex_reject(
            &Type::length(),
            &Type::Int
        ));
    }

    #[test]
    fn add_sub_dimensioned_complex_reject_false_for_int_plus_int() {
        assert!(!add_sub_dimensioned_complex_reject(&Type::Int, &Type::Int));
    }

    #[test]
    fn binop_mul_right_error_yields_error() {
        assert_eq!(
            infer_binop_type(BinOp::Mul, &Type::dimensionless_scalar(), &Type::Error),
            Type::Error,
        );
    }

    #[test]
    fn binop_lt_error_operand_yields_error_not_bool() {
        // Comparison ops would normally produce Type::Bool — the error must win.
        assert_eq!(
            infer_binop_type(BinOp::Lt, &Type::Error, &Type::Int),
            Type::Error,
        );
    }

    /// Exhaustive BinOp coverage (amendment-round-2 S3): every variant of
    /// `BinOp` must propagate `Type::Error` when either operand is poisoned.
    /// This pins down the anti-cascade contract for the full enum, not just
    /// the three representatives spot-checked above. Update this list (and
    /// the inner match in `infer_binop_type`) together if a new BinOp arm
    /// is added.
    #[test]
    fn every_binop_variant_propagates_error_from_either_operand() {
        // Compile-time exhaustiveness guard: adding a new BinOp variant to
        // the enum is a build error here until the `ops` list below is also
        // updated. Keeps the test's enumeration honest as the enum grows.
        #[allow(dead_code)]
        fn _exhaustive_binop_check(op: BinOp) {
            match op {
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::Pow
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
                | BinOp::And
                | BinOp::Or
                | BinOp::Implies => {}
            }
        }
        // (op, expected_non_error_result_for_(Real, Real))_label — the second
        // tuple element is just a documentation aid for the reviewer; we
        // never assert on it. We only assert that, with at least one operand
        // poisoned, the result is Type::Error.
        let ops: &[(BinOp, &str)] = &[
            (BinOp::Add, "arithmetic: left.clone()"),
            (BinOp::Sub, "arithmetic: left.clone()"),
            (BinOp::Mul, "arithmetic: scalar/widening rules"),
            (BinOp::Div, "arithmetic: scalar/widening rules"),
            (BinOp::Mod, "arithmetic: left.clone()"),
            (BinOp::Pow, "arithmetic: left.clone()"),
            (BinOp::Eq, "comparison: Bool"),
            (BinOp::Ne, "comparison: Bool"),
            (BinOp::Lt, "comparison: Bool"),
            (BinOp::Le, "comparison: Bool"),
            (BinOp::Gt, "comparison: Bool"),
            (BinOp::Ge, "comparison: Bool"),
            (BinOp::And, "logical: Bool"),
            (BinOp::Or, "logical: Bool"),
            (BinOp::Implies, "logical: Bool"),
        ];
        for (op, label) in ops {
            assert_eq!(
                infer_binop_type(*op, &Type::Error, &Type::dimensionless_scalar()),
                Type::Error,
                "BinOp::{:?} ({}) failed to propagate Type::Error from LEFT operand",
                op,
                label,
            );
            assert_eq!(
                infer_binop_type(*op, &Type::dimensionless_scalar(), &Type::Error),
                Type::Error,
                "BinOp::{:?} ({}) failed to propagate Type::Error from RIGHT operand",
                op,
                label,
            );
            assert_eq!(
                infer_binop_type(*op, &Type::Error, &Type::Error),
                Type::Error,
                "BinOp::{:?} ({}) failed to propagate Type::Error when BOTH operands poisoned",
                op,
                label,
            );
        }
    }

    // ── BinOp::Implies wiring (task-3921) ────────────────────────────────────

    #[test]
    fn resolve_binop_implies_keyword() {
        assert_eq!(resolve_binop("implies"), Some(BinOp::Implies));
    }

    #[test]
    fn infer_binop_implies_bool_bool_yields_bool() {
        assert_eq!(
            infer_binop_type(BinOp::Implies, &Type::Bool, &Type::Bool),
            Type::Bool,
        );
    }

    #[test]
    fn infer_binop_implies_left_error_propagates() {
        assert_eq!(
            infer_binop_type(BinOp::Implies, &Type::Error, &Type::Bool),
            Type::Error,
        );
    }

    #[test]
    fn infer_binop_implies_right_error_propagates() {
        assert_eq!(
            infer_binop_type(BinOp::Implies, &Type::Bool, &Type::Error),
            Type::Error,
        );
    }

    // ── task-3702 tests ───────────────────────────────────────────────────────

    /// Helper: build a minimal `CompiledFnBody` returning a Real(2.0) literal.
    fn stub_body_real() -> CompiledFnBody {
        CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar()),
        }
    }

    /// `try_default_padding` with the tightened signature (no `compiled_args`
    /// argument) returns the expected candidate and default expressions when
    /// exactly one candidate satisfies the padding contract.
    ///
    /// Candidate: `f(x: Real, y: Real)` where param 1 (`y`) has default
    /// `Real(2.0)`. Caller provides 1 arg of type `Real` — the trailing
    /// default must be filled in.
    ///
    /// Expected: `Some((&cand, vec![Real(2.0) literal]))`.
    ///
    /// RED before step-5: the current `try_default_padding` signature still
    /// requires a `compiled_args: &[CompiledExpr]` second argument, so this
    /// call (with only 2 positional args) fails to compile.
    ///
    /// task-3702 (tighten try_default_padding signature)
    #[test]
    fn try_default_padding_new_signature_returns_padded_fn() {
        let default_expr = CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
        let cand = CompiledFunction {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("x".to_string(), Type::dimensionless_scalar()),
                ("y".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, Some(default_expr.clone())],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("f_stub_3702"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };

        // New signature: no compiled_args — only arg_types.
        let result = try_default_padding(&[&cand], &[Type::dimensionless_scalar()]);

        let (matched_fn, defaults) = result.expect("should find a matching candidate");
        assert!(
            std::ptr::eq(matched_fn, &cand),
            "returned candidate must be the same object"
        );
        assert_eq!(defaults.len(), 1, "one trailing default expected");
        assert_eq!(
            defaults[0].content_hash, default_expr.content_hash,
            "default expr content hash must match the Real(2.0) literal"
        );
    }

    // `try_default_padding` fires a `debug_assert!` (panics in debug builds)
    // when a candidate violates the length invariant
    // (`param_defaults.len() != params.len()`).
    //
    // This is the "bad shape" that was previously silently skipped by the
    // defensive filter; after task-3702 it is a programming error surfaced in
    // debug builds.
    //
    // Candidate: deliberately constructed via struct-literal with
    // `params = vec![("x", Real)]` but `param_defaults = Vec::new()` —
    // the legacy empty form that violates the invariant.
    //
    // task-3702 (tighten try_default_padding signature)

    // ── modulo_operands_are_int predicate (task-3916) ────────────────────────

    /// `(Int, Int)` is the one valid modulo shape → `true`.
    #[test]
    fn modulo_operands_int_int_is_true() {
        assert!(modulo_operands_are_int(&Type::Int, &Type::Int));
    }

    /// `(Real, Int)` is rejected (left is Real) → `false`.
    #[test]
    fn modulo_operands_real_int_is_false() {
        assert!(!modulo_operands_are_int(
            &Type::dimensionless_scalar(),
            &Type::Int
        ));
    }

    /// `(Int, Real)` is rejected (right is Real) → `false`.
    #[test]
    fn modulo_operands_int_real_is_false() {
        assert!(!modulo_operands_are_int(
            &Type::Int,
            &Type::dimensionless_scalar()
        ));
    }

    /// `(Real, Real)` — both wrong → `false`.
    #[test]
    fn modulo_operands_real_real_is_false() {
        assert!(!modulo_operands_are_int(
            &Type::dimensionless_scalar(),
            &Type::dimensionless_scalar()
        ));
    }

    /// `(Scalar{LENGTH}, Scalar{LENGTH})` — dimensioned types are not Int → `false`.
    #[test]
    fn modulo_operands_scalar_scalar_is_false() {
        assert!(!modulo_operands_are_int(&length_ty(), &length_ty()));
    }

    /// `(Scalar{LENGTH}, Int)` — left is dimensioned → `false`.
    #[test]
    fn modulo_operands_scalar_int_is_false() {
        assert!(!modulo_operands_are_int(&length_ty(), &Type::Int));
    }

    /// `(Bool, Int)` — Bool is not Int → `false`.
    #[test]
    fn modulo_operands_bool_int_is_false() {
        assert!(!modulo_operands_are_int(&Type::Bool, &Type::Int));
    }

    // ── Selector conformance + Selector→List<Geometry> coercion (task 4117 / β) ─

    /// `type_compatible(Selector(Face), Selector(Face))` must be `true`.
    ///
    /// Relies on the existing identity short-circuit (line 78). Already passes;
    /// locked here as a regression guard.
    #[test]
    fn type_compatible_selector_same_kind_is_true() {
        use reify_core::ty::SelectorKind;
        assert!(
            type_compatible(
                &Type::Selector(SelectorKind::Face),
                &Type::Selector(SelectorKind::Face)
            ),
            "Selector(Face) param with Selector(Face) arg must be compatible"
        );
    }

    /// `type_compatible(Selector(Face), Selector(Edge))` must be `false`.
    ///
    /// Different kinds must be rejected. Already passes via default `_ => false`
    /// in `implicitly_converts_to`; locked here as a regression guard.
    #[test]
    fn type_compatible_selector_cross_kind_is_false() {
        use reify_core::ty::SelectorKind;
        assert!(
            !type_compatible(
                &Type::Selector(SelectorKind::Face),
                &Type::Selector(SelectorKind::Edge)
            ),
            "Selector(Face) param with Selector(Edge) arg must be incompatible"
        );
    }

    /// `type_compatible(List<Geometry>, Selector(Face))` must be `true`.
    ///
    /// PRD §4.4: a selector arg coerces to a `List<Geometry>` param (one-directional).
    /// RED until step-4 adds the explicit guard in `type_compatible`.
    #[test]
    fn type_compatible_list_geometry_param_with_selector_face_arg_is_true() {
        use reify_core::ty::SelectorKind;
        assert!(
            type_compatible(
                &Type::List(Box::new(Type::Geometry)),
                &Type::Selector(SelectorKind::Face)
            ),
            "List<Geometry> param with Selector(Face) arg must be compatible (PRD §4.4)"
        );
    }

    /// `type_compatible(List<Geometry>, Selector(Body))` must be `true`.
    ///
    /// Same rule for Body-kind selectors.
    /// RED until step-4 adds the explicit guard in `type_compatible`.
    #[test]
    fn type_compatible_list_geometry_param_with_selector_body_arg_is_true() {
        use reify_core::ty::SelectorKind;
        assert!(
            type_compatible(
                &Type::List(Box::new(Type::Geometry)),
                &Type::Selector(SelectorKind::Body)
            ),
            "List<Geometry> param with Selector(Body) arg must be compatible (PRD §4.4)"
        );
    }

    /// `type_compatible(List<Geometry>, Selector(Edge))` must be `true`.
    ///
    /// Symmetry check for the Edge selector kind: the `Selector(_)` wildcard in
    /// the coercion guard covers all three kinds; this test locks the Edge case
    /// explicitly alongside Face and Body to guard against future kind-specific
    /// narrowing of the guard.
    #[test]
    fn type_compatible_list_geometry_param_with_selector_edge_arg_is_true() {
        use reify_core::ty::SelectorKind;
        assert!(
            type_compatible(
                &Type::List(Box::new(Type::Geometry)),
                &Type::Selector(SelectorKind::Edge)
            ),
            "List<Geometry> param with Selector(Edge) arg must be compatible (PRD §4.4)"
        );
    }

    /// `type_compatible(Selector(Face), List<Geometry>)` must be `false`.
    ///
    /// One-directional: a `List<Geometry>` arg must NOT satisfy a `Selector`-typed
    /// param. Already passes (no rule admits this); locked here to prevent
    /// inadvertently adding the reverse direction.
    #[test]
    fn type_compatible_selector_param_with_list_geometry_arg_is_false() {
        use reify_core::ty::SelectorKind;
        assert!(
            !type_compatible(
                &Type::Selector(SelectorKind::Face),
                &Type::List(Box::new(Type::Geometry))
            ),
            "Selector(Face) param with List<Geometry> arg must be incompatible (one-directional)"
        );
    }

    /// `type_compatible(List<Real>, Selector(Face))` must be `false`.
    ///
    /// Only `List<Geometry>` coerces from a selector — other list element types
    /// must not be widened.
    #[test]
    fn type_compatible_list_real_param_with_selector_arg_is_false() {
        use reify_core::ty::SelectorKind;
        assert!(
            !type_compatible(
                &Type::List(Box::new(Type::dimensionless_scalar())),
                &Type::Selector(SelectorKind::Face)
            ),
            "List<Real> param with Selector(Face) arg must be incompatible (only List<Geometry> coerces)"
        );
    }

    // ── AnySelector compat (task 4369 / A2) ────────────────────────────────────
    //
    // Contract (PRD §4.2/D3): `type_compatible(AnySelector, Selector(k))` is
    // true for every concrete k (the agnostic param accepts all kinds).  The
    // rule is ONE-DIRECTIONAL: a single-kind param does NOT accept an agnostic
    // arg.  Non-selector arguments are also rejected.
    //
    // Tests (a)/(b)/(c) are RED until step-4 adds the rule in `type_compatible`.
    // Tests (d)/(e)/(f)/(g) are GREEN from pre-1 and serve as regression guards.

    /// (a) AnySelector param accepts a Face-kind selector arg.
    /// RED until step-4.
    #[test]
    fn type_compatible_any_selector_param_face_arg_is_true() {
        use reify_core::ty::SelectorKind;
        assert!(
            type_compatible(&Type::AnySelector, &Type::Selector(SelectorKind::Face)),
            "AnySelector param with Selector(Face) arg must be compatible (PRD §4.2/D3)"
        );
    }

    /// (b) AnySelector param accepts an Edge-kind selector arg.
    /// RED until step-4.
    #[test]
    fn type_compatible_any_selector_param_edge_arg_is_true() {
        use reify_core::ty::SelectorKind;
        assert!(
            type_compatible(&Type::AnySelector, &Type::Selector(SelectorKind::Edge)),
            "AnySelector param with Selector(Edge) arg must be compatible (PRD §4.2/D3)"
        );
    }

    /// (c) AnySelector param accepts a Body-kind selector arg.
    /// RED until step-4.
    #[test]
    fn type_compatible_any_selector_param_body_arg_is_true() {
        use reify_core::ty::SelectorKind;
        assert!(
            type_compatible(&Type::AnySelector, &Type::Selector(SelectorKind::Body)),
            "AnySelector param with Selector(Body) arg must be compatible (PRD §4.2/D3)"
        );
    }

    /// (d) AnySelector param rejects a non-selector arg.
    /// GREEN from pre-1 (no rule fires → falls through to false).
    #[test]
    fn type_compatible_any_selector_param_real_arg_is_false() {
        assert!(
            !type_compatible(&Type::AnySelector, &Type::dimensionless_scalar()),
            "AnySelector param with Real arg must be incompatible"
        );
    }

    /// (e) ONE-DIRECTIONAL: a single-kind param does NOT accept an agnostic arg.
    /// GREEN from pre-1 (no rule fires for this direction).
    #[test]
    fn type_compatible_face_selector_param_any_selector_arg_is_false() {
        use reify_core::ty::SelectorKind;
        assert!(
            !type_compatible(&Type::Selector(SelectorKind::Face), &Type::AnySelector),
            "Selector(Face) param with AnySelector arg must be incompatible (one-directional)"
        );
    }

    /// (f) Regression: single-kind cross-kind rejection unchanged.
    /// GREEN from pre-1 (exact-equality check is untouched).
    #[test]
    fn type_compatible_any_selector_regression_face_body_cross_kind_is_false() {
        use reify_core::ty::SelectorKind;
        assert!(
            !type_compatible(
                &Type::Selector(SelectorKind::Face),
                &Type::Selector(SelectorKind::Body)
            ),
            "Selector(Face) param with Selector(Body) arg must be incompatible (kind mismatch)"
        );
    }

    /// (g) Identity: AnySelector param with AnySelector arg is compatible.
    /// GREEN from pre-1 (from==to short-circuit in implicitly_converts_to).
    #[test]
    fn type_compatible_any_selector_identity_is_true() {
        assert!(
            type_compatible(&Type::AnySelector, &Type::AnySelector),
            "AnySelector param with AnySelector arg must be compatible (identity)"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "param_defaults.len() == params.len()")]
    fn try_default_padding_debug_assert_fires_on_misaligned_param_defaults() {
        // Deliberately bad shape: params has 1 entry, param_defaults is empty.
        let bad_cand = CompiledFunction {
            name: "bad".to_string(),
            doc: None,
            is_pub: false,
            params: vec![("x".to_string(), Type::dimensionless_scalar())],
            param_defaults: Vec::new(), // invariant violation — intentional for this test
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("bad_stub_3702"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };

        // New signature: (named, arg_types). Providing 0 arg types so the
        // candidate has more params than provided (1 > 0) — the invariant
        // check fires before any other filtering.
        let _ = try_default_padding(&[&bad_cand], &[]);
    }

    // ── task 4231 β: unify (call-site type-arg inference) ────────────────────
    //
    // Structural single-pass unification: bind TypeParam leaves from argument
    // types; the ONLY error is a type-param double-binding (TypeArgConflict).
    // Conservative on structural mismatch (Ok, no binding). PRD D2.

    fn tp(name: &str) -> Type {
        Type::TypeParam(name.to_string())
    }

    #[test]
    fn unify_binds_bare_type_param() {
        // (a) unify(TypeParam("T"), Real) → Ok, subst == {T: Real}.
        let mut subst = HashMap::new();
        assert!(unify(&tp("T"), &Type::dimensionless_scalar(), &mut subst).is_ok());
        assert_eq!(subst.get("T"), Some(&Type::dimensionless_scalar()));
        assert_eq!(subst.len(), 1);
    }

    #[test]
    fn unify_recurses_into_list() {
        // (b) unify(List(TypeParam("T")), List(Int)) → Ok, {T: Int}.
        let mut subst = HashMap::new();
        assert!(
            unify(
                &Type::List(Box::new(tp("T"))),
                &Type::List(Box::new(Type::Int)),
                &mut subst,
            )
            .is_ok()
        );
        assert_eq!(subst.get("T"), Some(&Type::Int));
        assert_eq!(subst.len(), 1);
    }

    #[test]
    fn unify_recurses_into_field_both_positions() {
        // (c) unify(Field{B, C}, Field{Real, Length}) → Ok, {B: Real, C: Length}.
        let mut subst = HashMap::new();
        assert!(
            unify(
                &Type::Field {
                    domain: Box::new(tp("B")),
                    codomain: Box::new(tp("C")),
                },
                &Type::Field {
                    domain: Box::new(Type::dimensionless_scalar()),
                    codomain: Box::new(Type::length()),
                },
                &mut subst,
            )
            .is_ok()
        );
        assert_eq!(subst.get("B"), Some(&Type::dimensionless_scalar()));
        assert_eq!(subst.get("C"), Some(&Type::length()));
        assert_eq!(subst.len(), 2);
    }

    #[test]
    fn unify_double_bind_conflict_errors() {
        // (d) bind T:Int then T:Real with the SAME subst → second call Errs,
        //     conflict.param == "T".
        let mut subst = HashMap::new();
        assert!(unify(&tp("T"), &Type::Int, &mut subst).is_ok());
        let err = unify(&tp("T"), &Type::dimensionless_scalar(), &mut subst)
            .expect_err("re-binding T to a different type must conflict");
        assert_eq!(err.param, "T");
        assert_eq!(err.existing, Type::Int);
        assert_eq!(err.incoming, Type::dimensionless_scalar());
    }

    #[test]
    fn unify_erased_type_param_binding_yields_to_concrete() {
        // Task #4038 δ (esc-4038-2): a param first bound to a bare `TypeParam`
        // arg (erased/unknown — e.g. the leaked `Applied{"Result",[T,E]}` from a
        // chained `or_else`) must UPGRADE to the concrete arg for the same param
        // rather than hard-conflicting (the E_FALLBACK_TYPE `T vs Scalar[m]`
        // regression).
        let mut subst = HashMap::new();
        // Erased first, then concrete → upgrade to concrete, no conflict.
        assert!(unify(&tp("T"), &tp("U"), &mut subst).is_ok());
        assert!(unify(&tp("T"), &Type::length(), &mut subst).is_ok());
        assert_eq!(subst.get("T"), Some(&Type::length()));

        // Concrete first, then erased → keep concrete, no conflict.
        let mut subst2 = HashMap::new();
        assert!(unify(&tp("T"), &Type::length(), &mut subst2).is_ok());
        assert!(unify(&tp("T"), &tp("U"), &mut subst2).is_ok());
        assert_eq!(subst2.get("T"), Some(&Type::length()));
    }

    #[test]
    fn unify_two_distinct_erased_params_still_conflict() {
        // Guard: two DIFFERING erased bindings (distinct generic params, e.g.
        // `pair(a: A, b: B)`) must still conflict — the weak-binding relaxation
        // only covers the erased-vs-concrete case, never erased-vs-erased.
        let mut subst = HashMap::new();
        assert!(unify(&tp("T"), &tp("A"), &mut subst).is_ok());
        let err = unify(&tp("T"), &tp("B"), &mut subst)
            .expect_err("two distinct erased params must still conflict");
        assert_eq!(err.param, "T");
    }

    #[test]
    fn unify_concrete_vs_headed_concrete_still_conflicts() {
        // Boundary lock (reviewer_comprehensive #1, task #4038 δ amendment):
        // the erased-vs-concrete relaxation above is scoped to a bare
        // `TypeParam` leaf ONLY. A HEADED type that merely carries a nested
        // type-param — e.g. the leaky `Applied{"Result",[A,B]}` produced by a
        // chained `or_else(parse_length_r(x), parse_length_r(y))` — is NOT a
        // bare `TypeParam` at its own head, so binding it against an existing
        // CONCRETE binding for a different type must still hard-conflict.
        // Without this guard a genuinely-wrong program (e.g. passing a
        // `Result<..>`-shaped value where a `Length` was already bound to the
        // same type parameter) would be silently accepted instead of raising
        // `E_FN_TYPE_ARG_CONFLICT`.
        let mut subst = HashMap::new();
        assert!(unify(&tp("T"), &Type::length(), &mut subst).is_ok());
        let leaky_result = Type::Applied {
            name: "Result".to_string(),
            args: vec![tp("A"), tp("B")],
        };
        let err = unify(&tp("T"), &leaky_result, &mut subst)
            .expect_err("concrete Length binding vs a differently-headed concrete arg must still conflict");
        assert_eq!(err.param, "T");
        assert_eq!(err.existing, Type::length());
        assert_eq!(err.incoming, leaky_result);

        // Symmetric direction: headed-concrete bound first, then a different
        // concrete type — also still conflicts.
        let mut subst2 = HashMap::new();
        let leaky_result2 = Type::Applied {
            name: "Result".to_string(),
            args: vec![tp("A"), tp("B")],
        };
        assert!(unify(&tp("T"), &leaky_result2, &mut subst2).is_ok());
        let err2 = unify(&tp("T"), &Type::length(), &mut subst2)
            .expect_err("headed concrete binding vs a differently-headed concrete arg must still conflict");
        assert_eq!(err2.param, "T");
    }

    #[test]
    fn unify_consistent_rebind_ok() {
        // (e) unify T against Int twice → both Ok, no error, single binding.
        let mut subst = HashMap::new();
        assert!(unify(&tp("T"), &Type::Int, &mut subst).is_ok());
        assert!(unify(&tp("T"), &Type::Int, &mut subst).is_ok());
        assert_eq!(subst.get("T"), Some(&Type::Int));
        assert_eq!(subst.len(), 1);
    }

    #[test]
    fn unify_conservative_on_structural_mismatch() {
        // (f) unify(List(TypeParam("T")), Int) → Ok with EMPTY subst
        //     (declared constructor != arg constructor: no binding, no error).
        let mut subst = HashMap::new();
        assert!(unify(&Type::List(Box::new(tp("T"))), &Type::Int, &mut subst).is_ok());
        assert!(subst.is_empty());
    }

    #[test]
    fn unify_accumulates_across_calls() {
        // (g) two unify calls sharing one subst accumulate distinct params.
        let mut subst = HashMap::new();
        assert!(unify(&tp("A"), &Type::Int, &mut subst).is_ok());
        assert!(unify(&tp("B"), &Type::length(), &mut subst).is_ok());
        assert_eq!(subst.get("A"), Some(&Type::Int));
        assert_eq!(subst.get("B"), Some(&Type::length()));
        assert_eq!(subst.len(), 2);
    }

    // ── task 4231 β: resolve_function_overload selects generic candidates ─────

    /// `make_fn` + non-empty `type_params` + a chosen return type.
    fn make_generic_fn(
        name: &str,
        params: Vec<(&str, Type)>,
        type_param_names: &[&str],
        return_type: Type,
    ) -> CompiledFunction {
        let mut f = make_fn(name, params);
        f.type_params = type_param_names
            .iter()
            .map(|n| reify_ir::TypeParam {
                name: n.to_string(),
                bounds: vec![],
                default: None,
            })
            .collect();
        f.return_type = return_type;
        f
    }

    #[test]
    fn overload_selects_generic_candidate() {
        // A generic fn `id<T>(x: T) -> T` must resolve against a concrete arg.
        // RED until step-6: a TypeParam param fails exact-equality → NoMatch.
        let fns = vec![make_generic_fn("id", vec![("x", tp("T"))], &["T"], tp("T"))];
        assert!(
            matches!(
                resolve_function_overload("id", &[Type::length()], &fns),
                OverloadResolution::Resolved(_)
            ),
            "generic candidate should resolve against a concrete arg"
        );
    }

    #[test]
    fn overload_concrete_beats_generic_on_exact_match() {
        // Tie-break (INV-6 guard): concrete f(Real)->Real + generic f<T>(x:T)->T
        // called with Real resolves to the CONCRETE overload (exact match wins).
        let concrete = make_fn("f", vec![("x", Type::dimensionless_scalar())]); // non-generic, returns Real
        let generic = make_generic_fn("f", vec![("x", tp("T"))], &["T"], tp("T"));
        let fns = vec![concrete, generic];
        match resolve_function_overload("f", &[Type::dimensionless_scalar()], &fns) {
            OverloadResolution::Resolved(matched) => {
                assert!(
                    matched.type_params.is_empty(),
                    "exact concrete overload should win over the generic one"
                );
                assert_eq!(matched.return_type, Type::dimensionless_scalar());
            }
            OverloadResolution::NoMatch(_) => panic!("expected Resolved(concrete), got NoMatch"),
            OverloadResolution::Ambiguous(_) => {
                panic!("expected Resolved(concrete), got Ambiguous")
            }
            OverloadResolution::NoUserFunctions => {
                panic!("expected Resolved(concrete), got NoUserFunctions")
            }
        }
    }

    // ── task 4231 β amendment: type_carries_type_param coverage parity ───────

    #[test]
    fn type_carries_type_param_recurses_through_all_constructors() {
        // The predicate must recognize a type-param embedded in ANY
        // inner-Type-bearing constructor, in parity with unify /
        // substitute_type_params — not just the bare leaf + Option/List/Set/Map.
        // Positive cases across the widened constructor set:
        assert!(type_carries_type_param(&tp("T")));
        assert!(type_carries_type_param(&Type::Field {
            domain: Box::new(tp("D")),
            codomain: Box::new(Type::dimensionless_scalar()),
        }));
        assert!(
            type_carries_type_param(&Type::List(Box::new(Type::Field {
                domain: Box::new(tp("D")),
                codomain: Box::new(Type::dimensionless_scalar()),
            }))),
            "recursion must pass through List into Field"
        );
        assert!(type_carries_type_param(&Type::Function {
            params: vec![Type::dimensionless_scalar(), tp("T")],
            return_type: Box::new(Type::dimensionless_scalar()),
        }));
        assert!(type_carries_type_param(&Type::Union(vec![
            Type::Int,
            tp("T")
        ])));
        assert!(type_carries_type_param(&Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(tp("Q")),
        }));
        assert!(type_carries_type_param(&Type::Keyed(Box::new(tp("T")))));
        assert!(type_carries_type_param(&Type::Complex(Box::new(tp("T")))));
        assert!(type_carries_type_param(&Type::Range(Box::new(tp("T")))));

        // Negative: no type-param anywhere → false (leaves + concrete nesting).
        assert!(!type_carries_type_param(&Type::dimensionless_scalar()));
        assert!(!type_carries_type_param(&Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::length()),
        }));
        assert!(!type_carries_type_param(&Type::List(Box::new(Type::Int))));
    }

    // ── task γ #4031 amendment: type_mentions_conflicted_param ───────────────

    #[test]
    fn type_mentions_conflicted_param_recurses_and_checks_membership() {
        let conflicted: HashSet<String> = ["T".to_string()].into_iter().collect();

        // Bare match: the leaf itself, name in the conflicted set.
        assert!(type_mentions_conflicted_param(&tp("T"), &conflicted));
        // Bare non-match: a DIFFERENT type-param name is not in the set.
        assert!(!type_mentions_conflicted_param(&tp("U"), &conflicted));
        // Non-type-param leaf: never mentions anything.
        assert!(!type_mentions_conflicted_param(
            &Type::dimensionless_scalar(),
            &conflicted
        ));

        // Compound nesting: List<T> mentions conflicted "T" (the case the
        // amendment's anti-cascade skip must catch that a bare-TypeParam-only
        // check would miss).
        assert!(type_mentions_conflicted_param(
            &Type::List(Box::new(tp("T"))),
            &conflicted
        ));
        // Compound nesting with an UNCONFLICTED param: List<U> does not
        // mention "T".
        assert!(!type_mentions_conflicted_param(
            &Type::List(Box::new(tp("U"))),
            &conflicted
        ));
        // Deeper nesting: Applied (user-defined generic, e.g. Tree<T>) args.
        assert!(type_mentions_conflicted_param(
            &Type::Applied {
                name: "Tree".to_string(),
                args: vec![tp("T")],
            },
            &conflicted
        ));
        // Field domain/codomain, in parity with unify/type_carries_type_param.
        assert!(type_mentions_conflicted_param(
            &Type::Field {
                domain: Box::new(tp("T")),
                codomain: Box::new(Type::dimensionless_scalar()),
            },
            &conflicted
        ));
    }

    #[test]
    fn overload_selects_generic_with_field_param() {
        // A generic candidate whose param embeds a type-param inside a
        // NON-collection constructor (Field) must be selectable as a wildcard.
        // Before widening type_carries_type_param this resolved to NoMatch
        // because recursion stopped at Option/List/Set/Map.
        let field_param = Type::Field {
            domain: Box::new(tp("D")),
            codomain: Box::new(Type::dimensionless_scalar()),
        };
        let fns = vec![make_generic_fn(
            "sample",
            vec![("f", field_param)],
            &["D"],
            Type::dimensionless_scalar(),
        )];
        let arg = Type::Field {
            domain: Box::new(Type::length()),
            codomain: Box::new(Type::dimensionless_scalar()),
        };
        assert!(
            matches!(
                resolve_function_overload("sample", &[arg], &fns),
                OverloadResolution::Resolved(_)
            ),
            "generic candidate with a Field<T, Real> param should resolve"
        );
    }

    // ── Step-3 RED: α behavioural contract for infer_binop_type ──────────────
    //
    // infer_binop_type(Div, length(), length()) must return dimensionless_scalar(),
    // not Type::dimensionless_scalar() (the old special-case). RED today: returns Type::dimensionless_scalar().
    #[test]
    fn infer_div_length_by_length_returns_dimensionless_scalar() {
        assert_eq!(
            infer_binop_type(BinOp::Div, &Type::length(), &Type::length()),
            Type::dimensionless_scalar(),
            "Length / Length should produce dimensionless_scalar(), not Type::dimensionless_scalar()"
        );
    }

    // type_compatible(dimensionless_scalar, Int) must return true (Int-widening
    // for the canonical dimensionless type). RED today: returns false (only
    // the (Type::dimensionless_scalar(), Type::Int) guard matches).
    #[test]
    fn type_compatible_dimensionless_scalar_accepts_int() {
        assert!(
            type_compatible(&Type::dimensionless_scalar(), &Type::Int),
            "dimensionless_scalar() should be compatible with Type::Int (Int-widening)"
        );
    }

    // ── task-4544: try_default_padding trait-carrying prefix wildcard ─────────

    /// Positive: a TraitObject-typed leading param acts as a wildcard so that a
    /// StructureRef arg (a concrete type satisfying the trait at runtime) passes
    /// the prefix check and the trailing default is returned.
    ///
    /// Candidate: `f(j: TraitObject("DrivingJoint"), y: Real)` where `y` has
    /// default `Real(1.0)`.  Call: `f(StructureRef("X"))` — 1 arg.
    ///
    /// Expected: `Some((&cand, [Real(1.0)]))`.
    ///
    /// RED before step-2: the strict `param_ty == arg_ty` prefix check rejects
    /// `TraitObject("DrivingJoint") != StructureRef("X")` → returns None.
    #[test]
    fn try_default_padding_resolves_when_leading_param_is_trait_carrying() {
        let default_expr = CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
        let cand = CompiledFunction {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                (
                    "j".to_string(),
                    Type::TraitObject("DrivingJoint".to_string()),
                ),
                ("y".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, Some(default_expr.clone())],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("f_4544_trait_prefix"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };

        // Provide ONE arg of type StructureRef("X") — the TraitObject param is
        // a wildcard (concrete type conforms at runtime), so the trailing
        // Real default must be returned.
        let result = try_default_padding(&[&cand], &[Type::StructureRef("X".to_string())]);
        let (matched_fn, defaults) = result
            .expect("trait-carrying leading param must act as a wildcard: expected Some, got None");
        assert!(
            std::ptr::eq(matched_fn, &cand),
            "returned candidate must be the same object"
        );
        assert_eq!(defaults.len(), 1, "one trailing default expected");
        assert_eq!(
            defaults[0].content_hash, default_expr.content_hash,
            "returned default must be the Real(1.0) literal"
        );
    }

    /// Disambiguation: when two same-name candidates both pass the wildcard prefix
    /// check (one has a trait-typed leading param, the other has a matching exact
    /// concrete param), the exact-match one wins.
    ///
    /// Candidate A: `f(j: TraitObject("T"), y: Real=1.0)` — passes via wildcard for
    ///   any StructureRef arg.
    /// Candidate B: `f(x: StructureRef("X"), y: Real=2.0)` — passes via exact match
    ///   for a StructureRef("X") arg.
    ///
    /// Call: `f(StructureRef("X"))`.  Both pass the wildcard-inclusive prefix check,
    /// so `satisfiable.len() == 2`. The tie-break prefers the exact-match subset
    /// (only B), returning candidate B with default `Real(2.0)`.
    #[test]
    fn try_default_padding_exact_match_wins_over_wildcard() {
        let default_a = CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
        let default_b = CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
        let cand_a = CompiledFunction {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("j".to_string(), Type::TraitObject("T".to_string())),
                ("y".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, Some(default_a)],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("f_4544_tiebreak_a"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };
        let cand_b = CompiledFunction {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("x".to_string(), Type::StructureRef("X".to_string())),
                ("y".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, Some(default_b.clone())],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("f_4544_tiebreak_b"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };

        let result =
            try_default_padding(&[&cand_a, &cand_b], &[Type::StructureRef("X".to_string())]);
        let (matched_fn, defaults) = result
            .expect("exact-match tie-break must resolve to candidate B; expected Some, got None");
        assert!(
            std::ptr::eq(matched_fn, &cand_b),
            "tie-break must prefer the exact-match candidate (cand_b)"
        );
        assert_eq!(defaults.len(), 1, "one trailing default expected");
        assert_eq!(
            defaults[0].content_hash, default_b.content_hash,
            "returned default must be cand_b's Real(2.0)"
        );
    }

    /// Negative control: a concrete (non-trait) leading param that mismatches the
    /// provided arg type must still return `None` — the loosening is scoped to
    /// trait/type-param wildcards only.
    ///
    /// Candidate: `g(x: Int, y: Real)` where `y` has default `Real(1.0)`.
    /// Call: `g(Real)` — Int ≠ Real, concrete param, no wildcard.
    ///
    /// Expected: `None` (both before and after step-2).
    #[test]
    fn try_default_padding_concrete_mismatch_still_returns_none() {
        let default_expr = CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
        let cand = CompiledFunction {
            name: "g".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("x".to_string(), Type::Int),
                ("y".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, Some(default_expr)],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("g_4544_concrete_mismatch"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };

        // Provide Real where Int is expected — concrete mismatch, must stay None.
        let result = try_default_padding(&[&cand], &[Type::dimensionless_scalar()]);
        assert!(
            result.is_none(),
            "concrete leading-param mismatch (Int vs Real) must return None even after loosening"
        );
    }

    /// Ambiguity: two same-name candidates both pass the wildcard prefix check
    /// but neither has an exact-match prefix — `try_default_padding` returns
    /// `None`, falling through to the caller's generic NoMatch error.
    ///
    /// Candidate A: `f(j: TraitObject("Joint1"), y: Real=1.0)`
    /// Candidate B: `f(k: TraitObject("Joint2"), y: Real=2.0)`
    ///
    /// Call: `f(StructureRef("X"))`.  Both pass via wildcard (`TraitObject`
    /// matches any arg); neither matches by strict equality.
    /// `satisfiable.len() == 2`, exact subset is empty → returns `None`.
    ///
    /// This documents the intentional UX contract: genuinely ambiguous
    /// defaultable padding degrades to NoMatch (not Ambiguous).  See the
    /// multi-candidate arm comment in `try_default_padding` for rationale.
    #[test]
    fn try_default_padding_all_wildcard_ambiguity_returns_none() {
        let default_a = CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
        let default_b = CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
        let cand_a = CompiledFunction {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("j".to_string(), Type::TraitObject("Joint1".to_string())),
                ("y".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, Some(default_a)],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("f_4544_allwild_a"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };
        let cand_b = CompiledFunction {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("k".to_string(), Type::TraitObject("Joint2".to_string())),
                ("y".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, Some(default_b)],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("f_4544_allwild_b"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };

        // Both candidates match via wildcard; neither matches by exact equality
        // → exact subset is empty → None (ambiguous padding falls through to
        // NoMatch, not Ambiguous).
        let result =
            try_default_padding(&[&cand_a, &cand_b], &[Type::StructureRef("X".to_string())]);
        assert!(
            result.is_none(),
            "two wildcard-only candidates must return None (ambiguous padding \
             degrades to NoMatch — see multi-candidate arm of try_default_padding)"
        );
    }

    // ── task-4788: try_default_padding specificity scoring ───────────────────

    /// RED driver: two candidates both pass the wildcard-prefix check but the
    /// binary "all-exact subset" tie-break incorrectly returns None because
    /// neither candidate has ALL 6 provided params matching by strict equality
    /// (cand_b's params 4–5 are `List<TraitObject>` while the args carry
    /// concrete `List<StructureRef>`).  Specificity scoring fixes this:
    /// cand_b scores 4 exact matches (Field + 3 × Length) while cand_a scores
    /// only 3 (3 × Length), so cand_b is the unique strict-maximum winner.
    ///
    /// Mirrors the `solve_elastic_static` A/B failure described in esc-4757-65.
    ///
    /// Candidate A: solve_elastic_static(
    ///   law: TraitObject("ConstitutiveLaw"),
    ///   nx: Length, ny: Length, nz: Length,
    ///   loads: List<TraitObject("Load")>,
    ///   supports: List<TraitObject("Support")>,
    ///   options: Real = 1.0)
    ///
    /// Candidate B: identical except params[0] is the concrete Field type.
    ///
    /// Call: 6 args — Field, Length×3, List<StructureRef("PointLoad")>,
    ///   List<StructureRef("FixedSupport")>.
    ///
    /// Expected: `Some(&cand_b, [options_b])`.
    ///
    /// RED before step-2: current all-exact filter yields empty exact subset
    ///   (cand_a fails at params[0]; cand_b fails at params[4]) → returns None.
    ///
    /// task-4788 / esc-4757-65
    #[test]
    fn try_default_padding_specificity_prefers_more_exact_matching_prefix() {
        let default_options_a =
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
        let default_options_b =
            CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());

        let field_ty = Type::Field {
            domain: Box::new(Type::length()),
            codomain: Box::new(Type::dimensionless_scalar()),
        };

        // Candidate A: first param is a trait object (ConstitutiveLaw).
        let cand_a = CompiledFunction {
            name: "solve_elastic_static".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                (
                    "law".to_string(),
                    Type::TraitObject("ConstitutiveLaw".to_string()),
                ),
                ("nx".to_string(), Type::length()),
                ("ny".to_string(), Type::length()),
                ("nz".to_string(), Type::length()),
                (
                    "loads".to_string(),
                    Type::List(Box::new(Type::TraitObject("Load".to_string()))),
                ),
                (
                    "supports".to_string(),
                    Type::List(Box::new(Type::TraitObject("Support".to_string()))),
                ),
                ("options".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, None, None, None, None, None, Some(default_options_a)],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("solve_elastic_a_4788"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };

        // Candidate B: first param is the concrete Field type.
        let cand_b = CompiledFunction {
            name: "solve_elastic_static".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("field".to_string(), field_ty.clone()),
                ("nx".to_string(), Type::length()),
                ("ny".to_string(), Type::length()),
                ("nz".to_string(), Type::length()),
                (
                    "loads".to_string(),
                    Type::List(Box::new(Type::TraitObject("Load".to_string()))),
                ),
                (
                    "supports".to_string(),
                    Type::List(Box::new(Type::TraitObject("Support".to_string()))),
                ),
                ("options".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![
                None,
                None,
                None,
                None,
                None,
                None,
                Some(default_options_b.clone()),
            ],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("solve_elastic_b_4788"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };

        // 6 concrete args: Field (matches B's param[0] exactly), 3×Length
        // (exact for both), List<StructureRef("PointLoad")> (List<TraitObject>
        // != List<StructureRef> for both), List<StructureRef("FixedSupport")>
        // (same). Score: A=3, B=4. Unique max → B wins.
        let args = vec![
            field_ty.clone(),
            Type::length(),
            Type::length(),
            Type::length(),
            Type::List(Box::new(Type::StructureRef("PointLoad".to_string()))),
            Type::List(Box::new(Type::StructureRef("FixedSupport".to_string()))),
        ];

        let result = try_default_padding(&[&cand_a, &cand_b], &args);
        let (matched_fn, defaults) = result.expect(
            "specificity scoring must resolve to cand_b (score 4 > 3); \
             expected Some, got None — RED: current all-exact filter returns None",
        );
        assert!(
            std::ptr::eq(matched_fn, &cand_b),
            "specificity tie-break must prefer cand_b (4 exact matches vs cand_a's 3)"
        );
        assert_eq!(defaults.len(), 1, "one trailing default (options) expected");
        assert_eq!(
            defaults[0].content_hash, default_options_b.content_hash,
            "returned default must be cand_b's options literal (Real(2.0))"
        );

        // Positive control: confirm cand_a is individually satisfiable so the
        // primary assertion above exercises the multi-candidate scoring arm, not
        // the 1-candidate arm.  If cand_a were dropped by the wildcard-prefix
        // filter, the `try_default_padding(&[&cand_a, &cand_b], …)` call above
        // would resolve via the `satisfiable.len() == 1` arm (returning cand_b
        // directly, no scoring) and the score-3-vs-4 tie-break path would be
        // silently untested.
        let result_a_only = try_default_padding(&[&cand_a], &args);
        assert!(
            result_a_only.is_some(),
            "cand_a must be satisfiable on its own (law: TraitObject(\"ConstitutiveLaw\") \
             is a wildcard that admits the Field arg) — proves the primary assertion \
             exercises the multi-candidate scoring arm (task-4788)"
        );
    }

    /// Tie guard (green before AND after step-2): two satisfiable candidates
    /// with EQUAL positive exact-counts → `try_default_padding` returns `None`.
    ///
    /// This documents that the specificity-scoring tie-break does NOT resolve
    /// equal-scoring candidates — it falls through to the caller's NoMatch
    /// error, preserving the existing ambiguity-as-NoMatch UX contract.
    ///
    /// Candidate P: `h(a: Real, b: TraitObject("Load"), opt: Real=1.0)`.
    /// Candidate Q: `h(a: Real, c: TraitObject("Support"), opt: Real=2.0)`.
    /// Call: `h(Real, StructureRef("Concrete"))` — 2 args.
    /// Score: P = 1 (param[0] exact, param[1] trait non-exact),
    ///         Q = 1 (same). Tied → None.
    ///
    /// Cannot be made RED (old and new logic both return None for ties), so it
    /// lives here as a co-located green guard that locks in the preserved
    /// contract.
    ///
    /// A positive-control sub-case (with a third candidate `cand_r` that scores
    /// 2 and uniquely wins) is included alongside the tie assertion.  This proves
    /// that `cand_p` and `cand_q` are both satisfiable and reach the multi-candidate
    /// arm — if neither were satisfiable the test's `result.is_none()` assertion
    /// would be vacuously true via the 0-candidate arm rather than the tie arm.
    ///
    /// task-4788
    #[test]
    fn try_default_padding_equal_exact_count_returns_none() {
        let default_p = CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
        let default_q = CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());

        let cand_p = CompiledFunction {
            name: "h".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("a".to_string(), Type::dimensionless_scalar()),
                ("b".to_string(), Type::TraitObject("Load".to_string())),
                ("opt".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, None, Some(default_p)],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("h_4788_tie_p"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };
        let cand_q = CompiledFunction {
            name: "h".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("a".to_string(), Type::dimensionless_scalar()),
                ("c".to_string(), Type::TraitObject("Support".to_string())),
                ("opt".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, None, Some(default_q)],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("h_4788_tie_q"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };

        let args = vec![
            Type::dimensionless_scalar(),
            Type::StructureRef("Concrete".to_string()),
        ];

        // Primary assertion: equal scores → None (ambiguous-as-NoMatch contract).
        // Both score 1 (param[0] exact, param[1] wildcard-non-exact) → tied.
        let result = try_default_padding(&[&cand_p, &cand_q], &args);
        assert!(
            result.is_none(),
            "equal specificity scores (both 1) must return None — tied candidates \
             must not be resolved by try_default_padding (task-4788)"
        );

        // Positive control: a third candidate with a strictly-higher score (both
        // params[0] and params[1] are exact → score 2) uniquely wins, proving
        // that cand_p and cand_q ARE satisfiable and reach the multi-candidate
        // scoring arm.  If they were not satisfiable, the None above would come
        // from the 0-candidate arm rather than the equal-count tie arm, and this
        // sub-case would surface the regression by returning None instead of
        // Some(cand_r).
        let default_r = CompiledExpr::literal(Value::Real(3.0), Type::dimensionless_scalar());
        let cand_r = CompiledFunction {
            name: "h".to_string(),
            doc: None,
            is_pub: false,
            params: vec![
                ("a".to_string(), Type::dimensionless_scalar()),
                ("b".to_string(), Type::StructureRef("Concrete".to_string())),
                ("opt".to_string(), Type::dimensionless_scalar()),
            ],
            param_defaults: vec![None, None, Some(default_r.clone())],
            return_type: Type::dimensionless_scalar(),
            body: stub_body_real(),
            content_hash: ContentHash::of_str("h_4788_tie_r"),
            annotations: vec![],
            optimized_target: None,
            type_params: vec![],
        };
        // cand_r scores 2 (param[0] Real exact, param[1] StructureRef("Concrete")
        // exact); cand_p scores 1, cand_q scores 1 → unique max cand_r → Some.
        let result_three = try_default_padding(&[&cand_p, &cand_q, &cand_r], &args);
        let (matched_fn, matched_defaults) = result_three.expect(
            "cand_r (score 2) must win over tied pair (score 1 each) — \
             proves cand_p/cand_q are satisfiable and reach the scoring arm (task-4788)",
        );
        assert!(
            std::ptr::eq(matched_fn, &cand_r),
            "unique-max candidate (cand_r, score 2) must win over cand_p/cand_q (score 1 each)"
        );
        assert_eq!(
            matched_defaults.len(),
            1,
            "one trailing default (opt) expected"
        );
        assert_eq!(
            matched_defaults[0].content_hash, default_r.content_hash,
            "returned default must be cand_r's opt literal (Real(3.0))"
        );
    }

    // ── is_syntactic_zero_literal predicate (task-4485/β) ────────────────────

    /// Helper: build a bare AST `Expr` with a dummy span for unit-testing predicates.
    fn make_ast_expr(kind: reify_ast::ExprKind) -> reify_ast::Expr {
        reify_ast::Expr {
            kind,
            span: SourceSpan::new(0, 1),
        }
    }

    /// `NumberLiteral{value:0.0, is_real:false}` — the bare `0` integer form — must
    /// return `true`.
    #[test]
    fn syntactic_zero_int_literal_zero_is_true() {
        let expr = make_ast_expr(reify_ast::ExprKind::NumberLiteral {
            value: 0.0,
            is_real: false,
        });
        assert!(is_syntactic_zero_literal(&expr));
    }

    /// `NumberLiteral{value:0.0, is_real:true}` — the `0.0` real form — must
    /// return `true`.
    #[test]
    fn syntactic_zero_real_literal_zero_is_true() {
        let expr = make_ast_expr(reify_ast::ExprKind::NumberLiteral {
            value: 0.0,
            is_real: true,
        });
        assert!(is_syntactic_zero_literal(&expr));
    }

    /// `UnOp{op:"-", operand: NumberLiteral{0.0}}` — the `-0` form — must
    /// return `true` (unary-neg recursion).
    #[test]
    fn syntactic_zero_neg_zero_is_true() {
        let inner = make_ast_expr(reify_ast::ExprKind::NumberLiteral {
            value: 0.0,
            is_real: false,
        });
        let expr = make_ast_expr(reify_ast::ExprKind::UnOp {
            op: "-".to_string(),
            operand: Box::new(inner),
        });
        assert!(is_syntactic_zero_literal(&expr));
    }

    /// `UnOp{"-", UnOp{"-", 0.0}}` — double-neg zero `--0.0` — must return `true`
    /// (recursive unary-neg chain).
    #[test]
    fn syntactic_zero_double_neg_zero_is_true() {
        let inner = make_ast_expr(reify_ast::ExprKind::NumberLiteral {
            value: 0.0,
            is_real: true,
        });
        let neg_inner = make_ast_expr(reify_ast::ExprKind::UnOp {
            op: "-".to_string(),
            operand: Box::new(inner),
        });
        let expr = make_ast_expr(reify_ast::ExprKind::UnOp {
            op: "-".to_string(),
            operand: Box::new(neg_inner),
        });
        assert!(is_syntactic_zero_literal(&expr));
    }

    /// `NumberLiteral{value:1.0, is_real:false}` — non-zero literal — must return `false`.
    #[test]
    fn syntactic_zero_nonzero_literal_is_false() {
        let expr = make_ast_expr(reify_ast::ExprKind::NumberLiteral {
            value: 1.0,
            is_real: false,
        });
        assert!(!is_syntactic_zero_literal(&expr));
    }

    /// `ExprKind::Ident("x")` — identifier reference — must return `false`.
    #[test]
    fn syntactic_zero_ident_is_false() {
        let expr = make_ast_expr(reify_ast::ExprKind::Ident("x".to_string()));
        assert!(!is_syntactic_zero_literal(&expr));
    }

    /// `UnOp{"-", Ident("x")}` — negated identifier — must return `false`.
    #[test]
    fn syntactic_zero_neg_ident_is_false() {
        let inner = make_ast_expr(reify_ast::ExprKind::Ident("x".to_string()));
        let expr = make_ast_expr(reify_ast::ExprKind::UnOp {
            op: "-".to_string(),
            operand: Box::new(inner),
        });
        assert!(!is_syntactic_zero_literal(&expr));
    }

    /// `BinOp{"-", NumberLiteral{1.0}, NumberLiteral{1.0}}` — constant-folded shape
    /// `1 - 1` — must return `false` (syntactic-only contract, §7.2 HARD BOUND).
    #[test]
    fn syntactic_zero_binop_one_minus_one_is_false() {
        let one_a = make_ast_expr(reify_ast::ExprKind::NumberLiteral {
            value: 1.0,
            is_real: false,
        });
        let one_b = make_ast_expr(reify_ast::ExprKind::NumberLiteral {
            value: 1.0,
            is_real: false,
        });
        let expr = make_ast_expr(reify_ast::ExprKind::BinOp {
            op: "-".to_string(),
            left: Box::new(one_a),
            right: Box::new(one_b),
        });
        assert!(!is_syntactic_zero_literal(&expr));
    }

    // ── task 4235 ζ: unify dimension-slot binding (D8) ───────────────────────

    /// (a) `unify(ScalarParam("Q"), Scalar{LENGTH})` binds Q → Scalar{LENGTH}.
    ///
    /// RED until step-2: the leaf arm `(Type::ScalarParam(_), _) => Ok(())` binds
    /// nothing, so subst stays empty.
    #[test]
    fn unify_scalar_param_binds_to_concrete_scalar() {
        let mut subst = HashMap::new();
        let result = unify(
            &Type::ScalarParam("Q".to_string()),
            &Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
            &mut subst,
        );
        assert!(
            result.is_ok(),
            "expected Ok for ScalarParam(Q) vs Scalar{{LENGTH}}, got {result:?}"
        );
        assert_eq!(
            subst.get("Q"),
            Some(&Type::Scalar {
                dimension: DimensionVector::LENGTH
            }),
            "subst[\"Q\"] should be Scalar{{LENGTH}} after binding, got {:?}",
            subst.get("Q")
        );
    }

    /// (b) Re-unifying the SAME `(ScalarParam("Q"), Scalar{LENGTH})` after Q is
    /// already bound to Scalar{LENGTH} is idempotent — no error.
    ///
    /// RED until step-2: the leaf arm binds nothing, so (a) never binds; the
    /// idempotent check is vacuous before (a) passes — but (a) failing is itself
    /// the RED signal, so (b) is an additional gate once (a) is green.
    #[test]
    fn unify_scalar_param_idempotent_rebind() {
        let mut subst = HashMap::new();
        subst.insert(
            "Q".to_string(),
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let result = unify(
            &Type::ScalarParam("Q".to_string()),
            &Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
            &mut subst,
        );
        assert!(
            result.is_ok(),
            "idempotent rebind of Q→Scalar{{LENGTH}} should be Ok, got {result:?}"
        );
    }

    /// (c) After Q→Scalar{LENGTH}, unifying with Scalar{MASS} → TypeArgConflict.
    ///
    /// RED until step-2: the leaf arm never sets subst, so the conflict arm can
    /// never fire.
    #[test]
    fn unify_scalar_param_conflict_emits_type_arg_conflict() {
        let mut subst = HashMap::new();
        subst.insert(
            "Q".to_string(),
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let result = unify(
            &Type::ScalarParam("Q".to_string()),
            &Type::Scalar {
                dimension: DimensionVector::MASS,
            },
            &mut subst,
        );
        match result {
            Err(TypeArgConflict {
                param,
                existing,
                incoming,
            }) => {
                assert_eq!(param, "Q", "conflict param should be Q");
                assert_eq!(
                    existing,
                    Type::Scalar {
                        dimension: DimensionVector::LENGTH
                    },
                    "existing should be Scalar{{LENGTH}}"
                );
                assert_eq!(
                    incoming,
                    Type::Scalar {
                        dimension: DimensionVector::MASS
                    },
                    "incoming should be Scalar{{MASS}}"
                );
            }
            Ok(()) => panic!("expected TypeArgConflict for Q (LENGTH) vs Scalar{{MASS}}, got Ok"),
        }
    }

    /// (d) Non-scalar arg (`Bool`) against `ScalarParam("Q")` binds nothing and
    /// returns Ok (conservative per D8).
    ///
    /// GREEN even before step-2 (the leaf arm already returns Ok for any arg).
    #[test]
    fn unify_scalar_param_non_scalar_arg_binds_nothing() {
        let mut subst = HashMap::new();
        let result = unify(&Type::ScalarParam("Q".to_string()), &Type::Bool, &mut subst);
        assert!(
            result.is_ok(),
            "non-scalar arg against ScalarParam should be Ok, got {result:?}"
        );
        assert!(
            subst.is_empty(),
            "subst should remain empty for non-scalar arg against ScalarParam, got {subst:?}"
        );
    }

    // ── task 4235 ζ: type_carries_dim_param + overload dim-param wildcard ─────

    /// Helper: ScalarParam shorthand.
    fn sp(name: &str) -> Type {
        Type::ScalarParam(name.to_string())
    }

    /// `type_carries_dim_param(ScalarParam("Q"))` must return true.
    ///
    /// RED until step-6: the function does not exist (compile error).
    #[test]
    fn type_carries_dim_param_bare_scalar_param_is_true() {
        assert!(
            type_carries_dim_param(&sp("Q")),
            "ScalarParam should carry a dim-param"
        );
    }

    /// `type_carries_dim_param(Vector3<ScalarParam("Q")>)` must return true
    /// (dim-param in the quantity slot).
    ///
    /// RED until step-6.
    #[test]
    fn type_carries_dim_param_vector3_quantity_is_true() {
        let vec3_q = Type::Vector {
            n: 3,
            quantity: Box::new(sp("Q")),
        };
        assert!(
            type_carries_dim_param(&vec3_q),
            "Vector3<ScalarParam(\"Q\")> should carry a dim-param"
        );
    }

    /// `type_carries_dim_param(Scalar{LENGTH})` must return false.
    ///
    /// RED until step-6.
    #[test]
    fn type_carries_dim_param_concrete_scalar_is_false() {
        assert!(
            !type_carries_dim_param(&Type::Scalar {
                dimension: DimensionVector::LENGTH
            }),
            "concrete Scalar{{LENGTH}} should NOT carry a dim-param"
        );
    }

    /// `type_carries_dim_param(TypeParam("T"))` must return false — a type-param
    /// is not a dimension-param.
    ///
    /// RED until step-6.
    #[test]
    fn type_carries_dim_param_type_param_is_false() {
        assert!(
            !type_carries_dim_param(&tp("T")),
            "TypeParam should NOT carry a dim-param"
        );
    }

    /// Overload wildcard for dim-param: `scale_q<Q: Dimension>(x: Scalar<Q>, k: Real)`
    /// called with `(Scalar{LENGTH}, Real)` must resolve to Resolved.
    ///
    /// RED until step-6: the wildcard predicate only checks type_carries_type_param,
    /// which returns false for ScalarParam → NoMatch.
    #[test]
    fn overload_selects_generic_candidate_with_scalar_param() {
        let scale_q = make_generic_fn(
            "scale_q",
            vec![("x", sp("Q")), ("k", Type::dimensionless_scalar())],
            &["Q"],
            sp("Q"),
        );
        let fns = vec![scale_q];
        assert!(
            matches!(
                resolve_function_overload(
                    "scale_q",
                    &[
                        Type::Scalar {
                            dimension: DimensionVector::LENGTH
                        },
                        Type::dimensionless_scalar()
                    ],
                    &fns,
                ),
                OverloadResolution::Resolved(_)
            ),
            "generic scale_q<Q> should resolve against (Scalar{{LENGTH}}, Real)"
        );
    }

    /// INV-6 regression: a non-generic all-concrete fn still resolves only on
    /// exact match — adding the dim-param wildcard must not break this.
    ///
    /// GREEN before step-6 too (the non-generic branch is unaffected).
    #[test]
    fn overload_non_generic_concrete_fn_still_requires_exact_match() {
        let concrete = make_fn(
            "scale_concrete",
            vec![
                (
                    "x",
                    Type::Scalar {
                        dimension: DimensionVector::LENGTH,
                    },
                ),
                ("k", Type::dimensionless_scalar()),
            ],
        );
        let fns = vec![concrete];
        // Calling with (MASS, Real) must NOT resolve — only (LENGTH, Real) is exact.
        assert!(
            matches!(
                resolve_function_overload(
                    "scale_concrete",
                    &[
                        Type::Scalar {
                            dimension: DimensionVector::MASS
                        },
                        Type::dimensionless_scalar()
                    ],
                    &fns,
                ),
                OverloadResolution::NoMatch(_)
            ),
            "concrete fn must NOT resolve for wrong dimension arg — exact match only"
        );
    }

    /// Regression lock (task #4038 δ, esc-4038-2): the CHAINED
    /// `fallback(or_else(parse_length_r(x), parse_length_r(y)), dflt)` case.
    ///
    /// `or_else<T,E>(Result<T,E>, Result<T,E>)` over two headless-`Enum("Result")`
    /// subjects binds neither `T` nor `E`, so its result type leaks out as
    /// `Applied{"Result", [TypeParam("T"), TypeParam("E")]}` (a HEADED type
    /// carrying nested type-params). That leaky arg then flows into the outer
    /// `fallback(...)` overload set — `fallback<T>(Option<T>, T)` and
    /// `fallback<T,E>(Result<T,E>, T)`. Before the head-exact-tier narrowing,
    /// the bare `type_carries_type_param(arg_ty)` disjunct wildcard-matched the
    /// leaky arg against BOTH overloads → spurious `Ambiguous`. With the
    /// narrowing (headed args are discriminated by `heads_unifiable`), only the
    /// `Result<T,E>` overload survives → `Resolved`.
    #[test]
    fn overload_chained_result_leaky_arg_resolves_to_result_overload() {
        let leaky_result = Type::Applied {
            name: "Result".to_string(),
            args: vec![tp("T"), tp("E")],
        };
        let option_overload = make_generic_fn(
            "fallback",
            vec![("o", Type::Option(Box::new(tp("T")))), ("dflt", tp("T"))],
            &["T"],
            tp("T"),
        );
        let result_overload = make_generic_fn(
            "fallback",
            vec![
                (
                    "r",
                    Type::Applied {
                        name: "Result".to_string(),
                        args: vec![tp("T"), tp("E")],
                    },
                ),
                ("dflt", tp("T")),
            ],
            &["T", "E"],
            tp("T"),
        );
        let fns = vec![option_overload, result_overload];
        match resolve_function_overload("fallback", &[leaky_result, Type::length()], &fns) {
            OverloadResolution::Resolved(matched) => assert_eq!(
                matched.type_params.len(),
                2,
                "leaky Result<T,E> arg must select the Result<T,E> overload, not Option<T>"
            ),
            OverloadResolution::Ambiguous(_) => {
                panic!("expected Resolved(fallback<T,E> over Result), got Ambiguous")
            }
            OverloadResolution::NoMatch(_) => {
                panic!("expected Resolved(fallback<T,E> over Result), got NoMatch")
            }
            OverloadResolution::NoUserFunctions => {
                panic!("expected Resolved(fallback<T,E> over Result), got NoUserFunctions")
            }
        }
    }

    /// Precedent lock (reviewer_comprehensive #2, task #4038 δ amendment):
    /// the head-exact tier's non-generic exclusion does not regress the
    /// chained-leaky-arg resolution above when a same-name NON-generic
    /// candidate is also present in the overload set.
    ///
    /// `non_generic_overload` (`fallback(r: Length, dflt: Length)`, no type
    /// params) is structurally unrelated to the leaky `Applied{"Result",[T,E]}`
    /// arg, but it IS admitted into the first (`matches`) tier — the
    /// `type_carries_type_param(arg_ty)` disjunct there looks only at the arg,
    /// not `param_ty`, so any candidate is provisionally eligible. Before the
    /// head-exact-tier narrowing (this task), it would ALSO have been eligible
    /// for `head_matches` via the same permissive arg-only check, joining
    /// `result_overload` there and forcing a spurious `Ambiguous`. After the
    /// narrowing, `non_generic_overload` fails `is_generic`, the bare-`TypeParam`
    /// wildcard, and plain equality, so it is excluded from `head_matches` —
    /// only the structurally-matching `result_overload` remains, and
    /// resolution stays a clean `Resolved`, identical to
    /// `overload_chained_result_leaky_arg_resolves_to_result_overload` above.
    #[test]
    fn overload_leaky_headed_arg_excludes_non_generic_candidate() {
        let leaky_result = Type::Applied {
            name: "Result".to_string(),
            args: vec![tp("T"), tp("E")],
        };
        let option_overload = make_generic_fn(
            "fallback",
            vec![("o", Type::Option(Box::new(tp("T")))), ("dflt", tp("T"))],
            &["T"],
            tp("T"),
        );
        let result_overload = make_generic_fn(
            "fallback",
            vec![
                (
                    "r",
                    Type::Applied {
                        name: "Result".to_string(),
                        args: vec![tp("T"), tp("E")],
                    },
                ),
                ("dflt", tp("T")),
            ],
            &["T", "E"],
            tp("T"),
        );
        let non_generic_overload =
            make_fn("fallback", vec![("r", Type::length()), ("dflt", Type::length())]);
        let fns = vec![option_overload, result_overload, non_generic_overload];
        match resolve_function_overload("fallback", &[leaky_result, Type::length()], &fns) {
            OverloadResolution::Resolved(matched) => assert_eq!(
                matched.type_params.len(),
                2,
                "leaky Result<T,E> arg must still select the Result<T,E> overload with a \
                 same-name non-generic candidate present, not go Ambiguous"
            ),
            OverloadResolution::Ambiguous(candidates) => panic!(
                "expected Resolved(fallback<T,E> over Result), got Ambiguous({} candidates) — \
                 the non-generic candidate leaked into head_matches",
                candidates.len()
            ),
            OverloadResolution::NoMatch(_) => {
                panic!("expected Resolved(fallback<T,E> over Result), got NoMatch")
            }
            OverloadResolution::NoUserFunctions => {
                panic!("expected Resolved(fallback<T,E> over Result), got NoUserFunctions")
            }
        }
    }

    /// D4 preservation (task-4232 γ): a BARE `TypeParam` arg (a generic fn body
    /// passing a `T`-typed value to a concrete-param overload) must STILL
    /// resolve after the head-exact-tier narrowing — the narrowing only strips
    /// the wildcard from HEADED nested-type-param args, never bare ones.
    #[test]
    fn overload_bare_type_param_arg_still_resolves() {
        let concrete = make_fn("g", vec![("x", Type::dimensionless_scalar())]);
        let fns = vec![concrete];
        assert!(
            matches!(
                resolve_function_overload("g", &[tp("U")], &fns),
                OverloadResolution::Resolved(_)
            ),
            "a bare TypeParam arg must still wildcard-resolve a single concrete overload"
        );
    }

    // ── task 4602 β: Applied / Projection coverage ──────────────────────────
    // Tests for the new behavioral branches: unify (element-wise Applied,
    // Projection base, and structural-mismatch fallthrough), substitute_type_params
    // (Applied arg rebuild and Projection base rebuild), and type_carries_type_param
    // / type_carries_dim_param recursion into Applied args and Projection base.

    /// unify(Applied{C,[TypeParam(T)]}, Applied{C,[StructureRef(X)]}) must bind T=X.
    #[test]
    fn unify_applied_same_name_arity_binds_type_param() {
        let mut subst = HashMap::new();
        let declared = Type::applied("C", vec![tp("T")]);
        let arg = Type::applied("C", vec![Type::StructureRef("X".to_string())]);
        assert!(unify(&declared, &arg, &mut subst).is_ok());
        assert_eq!(subst.get("T"), Some(&Type::StructureRef("X".to_string())));
        assert_eq!(subst.len(), 1);
    }

    /// unify(Applied{C,…}, Applied{D,…}) with differing name → Ok, no binding.
    #[test]
    fn unify_applied_differing_name_binds_nothing() {
        let mut subst = HashMap::new();
        let declared = Type::applied("C", vec![tp("T")]);
        let arg = Type::applied("D", vec![Type::StructureRef("X".to_string())]);
        assert!(unify(&declared, &arg, &mut subst).is_ok());
        assert!(subst.is_empty(), "differing name: expected no binding");
    }

    /// unify(Applied{C,[T]}, Applied{C,[X,Y]}) with differing arity → Ok, no binding.
    #[test]
    fn unify_applied_differing_arity_binds_nothing() {
        let mut subst = HashMap::new();
        let declared = Type::applied("C", vec![tp("T")]);
        let arg = Type::applied(
            "C",
            vec![
                Type::StructureRef("X".to_string()),
                Type::StructureRef("Y".to_string()),
            ],
        );
        assert!(unify(&declared, &arg, &mut subst).is_ok());
        assert!(subst.is_empty(), "differing arity: expected no binding");
    }

    /// unify(Projection{TypeParam(T),"M"}, Projection{StructureRef(X),"M"})
    /// must unify the bases and bind T=X.
    #[test]
    fn unify_projection_same_member_unifies_base() {
        let mut subst = HashMap::new();
        let declared = Type::projection(tp("T"), "M");
        let arg = Type::projection(Type::StructureRef("X".to_string()), "M");
        assert!(unify(&declared, &arg, &mut subst).is_ok());
        assert_eq!(subst.get("T"), Some(&Type::StructureRef("X".to_string())));
        assert_eq!(subst.len(), 1);
    }

    /// Projection with differing members → Ok, no binding (structural mismatch
    /// via the Applied/Projection fallthrough).
    #[test]
    fn unify_projection_differing_member_binds_nothing() {
        let mut subst = HashMap::new();
        let declared = Type::projection(tp("T"), "M1");
        let arg = Type::projection(Type::StructureRef("X".to_string()), "M2");
        assert!(unify(&declared, &arg, &mut subst).is_ok());
        assert!(subst.is_empty(), "differing member: expected no binding");
    }

    /// Applied-vs-StructureRef hits the fallthrough and conservatively binds
    /// nothing (β posture; δ/normalize_type is responsible for this pair).
    #[test]
    fn unify_applied_vs_structure_ref_conservative_beta_binds_nothing() {
        let mut subst = HashMap::new();
        let declared = Type::applied("C", vec![tp("T")]);
        let arg = Type::StructureRef("C".to_string());
        assert!(unify(&declared, &arg, &mut subst).is_ok());
        assert!(
            subst.is_empty(),
            "Applied-vs-StructureRef must bind nothing in β (see unify doc β note)"
        );
    }

    /// type_carries_type_param returns true for Applied whose args contain a TypeParam.
    #[test]
    fn type_carries_type_param_applied_with_type_param_arg() {
        let t = Type::applied("C", vec![tp("T")]);
        assert!(
            type_carries_type_param(&t),
            "Applied with TypeParam arg must carry a type param"
        );
        // Applied with no TypeParam in args → false.
        let t2 = Type::applied("C", vec![Type::StructureRef("X".to_string())]);
        assert!(
            !type_carries_type_param(&t2),
            "Applied with only concrete args must not carry a type param"
        );
    }

    /// type_carries_type_param returns true for Projection whose base is a TypeParam.
    #[test]
    fn type_carries_type_param_projection_with_type_param_base() {
        let t = Type::projection(tp("T"), "M");
        assert!(
            type_carries_type_param(&t),
            "Projection with TypeParam base must carry a type param"
        );
        let t2 = Type::projection(Type::StructureRef("X".to_string()), "M");
        assert!(
            !type_carries_type_param(&t2),
            "Projection with concrete base must not carry a type param"
        );
    }

    // ── task-4490: is_orderable_scalar / is_equatable_kind predicates ─────────
    //
    // These unit tests document and pin the allowlist contracts for the two
    // comparison-operand predicates used in `emit_comparison_operand_diagnostics`.
    //
    // GRADUALISM NOTE: `Type::Error` and `Type::TypeParam(_)` both return `false`
    // from these predicates — they are NOT in the allowlist.  The gradualism
    // early-return in `emit_comparison_operand_diagnostics` short-circuits before
    // reaching the predicate calls, so Error/TypeParam operands pass through
    // silently.  The predicate returning `false` for them is intentional and
    // correct; it is the early-return that grants the pass-through, not the
    // predicate returning `true`.

    /// `Type::Int` is orderable (integer comparison is defined at runtime).
    #[test]
    fn is_orderable_scalar_int_is_true() {
        assert!(is_orderable_scalar(&Type::Int));
    }

    /// A dimensionless `Scalar` is orderable.
    #[test]
    fn is_orderable_scalar_dimensionless_scalar_is_true() {
        assert!(is_orderable_scalar(&Type::dimensionless_scalar()));
    }

    /// A dimensioned `Scalar` (e.g. Length) is orderable.
    #[test]
    fn is_orderable_scalar_dimensioned_scalar_is_true() {
        assert!(is_orderable_scalar(&Type::length()));
    }

    /// `Type::Bool` is NOT orderable — `eval_cmp` yields `Undef` for Bool operands.
    #[test]
    fn is_orderable_scalar_bool_is_false() {
        assert!(!is_orderable_scalar(&Type::Bool));
    }

    /// `Type::String` is NOT orderable — `eval_cmp` yields `Undef` for String operands.
    #[test]
    fn is_orderable_scalar_string_is_false() {
        assert!(!is_orderable_scalar(&Type::String));
    }

    /// `Type::Enum(_)` is NOT orderable — `eval_cmp` yields `Undef` for Enum operands.
    /// (Enum EQUALITY is preserved via `is_equatable_kind`; only ORDER is rejected.)
    #[test]
    fn is_orderable_scalar_enum_is_false() {
        assert!(!is_orderable_scalar(&Type::Enum("Direction".to_string())));
    }

    /// `Type::Tensor{..}` is NOT orderable — aggregate type, yields `Undef` for order ops.
    #[test]
    fn is_orderable_scalar_tensor_is_false() {
        assert!(!is_orderable_scalar(&Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        }));
    }

    /// `Type::Matrix{..}` is NOT orderable — aggregate type.
    #[test]
    fn is_orderable_scalar_matrix_is_false() {
        assert!(!is_orderable_scalar(&Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        }));
    }

    /// `Type::Vector{..}` is NOT orderable — aggregate type.
    #[test]
    fn is_orderable_scalar_vector_is_false() {
        assert!(!is_orderable_scalar(&Type::Vector {
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        }));
    }

    /// `Type::TypeParam(_)` is NOT in the `is_orderable_scalar` allowlist.
    ///
    /// The gradualism early-return in `emit_comparison_operand_diagnostics` handles
    /// TypeParam by short-circuiting before this predicate is reached, so no
    /// spurious `CmpOperandKind` diagnostic is emitted for unresolved type params.
    #[test]
    fn is_orderable_scalar_type_param_is_false() {
        assert!(!is_orderable_scalar(&Type::TypeParam("T".to_string())));
    }

    /// `Type::Error` (poison) is NOT in the `is_orderable_scalar` allowlist.
    ///
    /// The gradualism early-return handles Error before this predicate; it is
    /// the early-return, not a predicate true-value, that prevents cascade noise.
    #[test]
    fn is_orderable_scalar_error_is_false() {
        assert!(!is_orderable_scalar(&Type::Error));
    }

    // ── is_equatable_kind ─────────────────────────────────────────────────────

    /// `Type::Bool` is equatable — `eval_eq` returns a defined Bool for Bool operands.
    #[test]
    fn is_equatable_kind_bool_is_true() {
        assert!(is_equatable_kind(&Type::Bool));
    }

    /// `Type::Int` is equatable.
    #[test]
    fn is_equatable_kind_int_is_true() {
        assert!(is_equatable_kind(&Type::Int));
    }

    /// `Type::String` is equatable — `eval_eq` returns a defined Bool for String operands.
    #[test]
    fn is_equatable_kind_string_is_true() {
        assert!(is_equatable_kind(&Type::String));
    }

    /// A dimensionless `Scalar` is equatable.
    #[test]
    fn is_equatable_kind_dimensionless_scalar_is_true() {
        assert!(is_equatable_kind(&Type::dimensionless_scalar()));
    }

    /// A dimensioned `Scalar` is equatable.
    #[test]
    fn is_equatable_kind_dimensioned_scalar_is_true() {
        assert!(is_equatable_kind(&Type::length()));
    }

    /// `Type::Enum(_)` IS equatable — CRUX: `eval_eq` returns a defined Bool for Enum.
    ///
    /// The `where shape == Shape.Round { ... }` guarded-enum idiom routes through
    /// the `Eq` arm and must compile cleanly.  This is a pinning test for the
    /// task-4490 scoping decision (design_decision[0]).
    #[test]
    fn is_equatable_kind_enum_is_true() {
        assert!(is_equatable_kind(&Type::Enum("Shape".to_string())));
    }

    /// `Type::Tensor{..}` is NOT equatable — aggregate type, `eval_eq` yields `Undef`.
    #[test]
    fn is_equatable_kind_tensor_is_false() {
        assert!(!is_equatable_kind(&Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        }));
    }

    /// `Type::Matrix{..}` is NOT equatable — aggregate type.
    #[test]
    fn is_equatable_kind_matrix_is_false() {
        assert!(!is_equatable_kind(&Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        }));
    }

    /// `Type::Vector{..}` is NOT equatable — aggregate type.
    #[test]
    fn is_equatable_kind_vector_is_false() {
        assert!(!is_equatable_kind(&Type::Vector {
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        }));
    }

    /// `Type::TypeParam(_)` is NOT in the `is_equatable_kind` allowlist.
    ///
    /// Gradualism in `emit_comparison_operand_diagnostics` short-circuits on
    /// TypeParam before this predicate is reached.
    #[test]
    fn is_equatable_kind_type_param_is_false() {
        assert!(!is_equatable_kind(&Type::TypeParam("T".to_string())));
    }

    /// `Type::Error` (poison) is NOT in the `is_equatable_kind` allowlist.
    ///
    /// Gradualism short-circuits on Error before this predicate is reached.
    #[test]
    fn is_equatable_kind_error_is_false() {
        assert!(!is_equatable_kind(&Type::Error));
    }

    /// `Type::Frame(_)` IS equatable — port-selector identity comparison idiom.
    ///
    /// `@face("mount") != @face("side")` in forall predicates types both
    /// operands as `Frame3`.  `Value::Frame` has a well-defined `PartialEq`
    /// (compares origin + basis), so this is a semantically valid equality.
    /// Rejecting Frame would break the ad-hoc selector pattern.
    #[test]
    fn is_equatable_kind_frame_is_true() {
        assert!(is_equatable_kind(&Type::Frame(3)));
        assert!(is_equatable_kind(&Type::Frame(2)));
    }

    /// `Type::Frame(_)` is NOT orderable — Frame3 has no natural ordering.
    #[test]
    fn is_orderable_scalar_frame_is_false() {
        assert!(!is_orderable_scalar(&Type::Frame(3)));
    }

    // ── constraint_arg_type_conforms (task 4546) ──────────────────────────────

    /// (gap) Bool passed where Length expected → false.
    /// This is the primary gap this task closes.
    #[test]
    fn constraint_arg_type_conforms_bool_for_length_is_false() {
        assert!(
            !constraint_arg_type_conforms(&length_ty(), &Type::Bool),
            "Bool passed as Length param must be rejected"
        );
    }

    /// (numeric leniency) Int passed where Length expected → true.
    /// Dimensional strictness within predicates is task 4490's job.
    #[test]
    fn constraint_arg_type_conforms_int_for_length_is_true() {
        assert!(
            constraint_arg_type_conforms(&length_ty(), &Type::Int),
            "Int passed as Length param must be tolerated (numeric leniency)"
        );
    }

    /// (cross-dimension numeric tolerated) Mass passed where Length expected → true.
    /// Both sides are Scalar — the numeric-leniency rule applies.
    #[test]
    fn constraint_arg_type_conforms_mass_for_length_is_true() {
        assert!(
            constraint_arg_type_conforms(&length_ty(), &mass_ty()),
            "Mass scalar passed as Length param must be tolerated (numeric leniency)"
        );
    }

    /// (dimensionless numeric tolerated) dimensionless Real passed where Length → true.
    #[test]
    fn constraint_arg_type_conforms_dimensionless_for_length_is_true() {
        assert!(
            constraint_arg_type_conforms(&length_ty(), &Type::dimensionless_scalar()),
            "dimensionless Real scalar passed as Length param must be tolerated (numeric leniency)"
        );
    }

    /// (identity) Length vs Length → true.
    #[test]
    fn constraint_arg_type_conforms_length_for_length_is_true() {
        assert!(
            constraint_arg_type_conforms(&length_ty(), &length_ty()),
            "Length vs Length must be accepted"
        );
    }

    /// (same-enum identity) Enum("Q") vs Enum("Q") → true.
    #[test]
    fn constraint_arg_type_conforms_same_enum_is_true() {
        let enum_q = Type::Enum("Q".to_string());
        assert!(
            constraint_arg_type_conforms(&enum_q, &enum_q),
            "Enum(Q) vs Enum(Q) must be accepted"
        );
    }

    /// (generic-param skip) TypeParam("T") in param position → true regardless of arg.
    #[test]
    fn constraint_arg_type_conforms_type_param_param_is_true() {
        assert!(
            constraint_arg_type_conforms(&Type::TypeParam("T".to_string()), &Type::Bool),
            "TypeParam-carrying param must be skipped (generic machinery handles it)"
        );
    }

    /// (Bool vs Bool) concrete identical non-numeric → true.
    #[test]
    fn constraint_arg_type_conforms_bool_for_bool_is_true() {
        assert!(
            constraint_arg_type_conforms(&Type::Bool, &Type::Bool),
            "Bool vs Bool must be accepted"
        );
    }

    /// (anti-cascade) Error on param side → true.
    #[test]
    fn constraint_arg_type_conforms_error_param_is_true() {
        assert!(
            constraint_arg_type_conforms(&Type::Error, &length_ty()),
            "Error param must be accepted (anti-cascade)"
        );
    }

    /// (anti-cascade) Error on arg side → true.
    #[test]
    fn constraint_arg_type_conforms_error_arg_is_true() {
        assert!(
            constraint_arg_type_conforms(&length_ty(), &Type::Error),
            "Error arg must be accepted (anti-cascade)"
        );
    }

    /// (cross-category rejected) String passed where Length expected → false.
    #[test]
    fn constraint_arg_type_conforms_string_for_length_is_false() {
        assert!(
            !constraint_arg_type_conforms(&length_ty(), &Type::String),
            "String passed as Length param must be rejected"
        );
    }

    // ── β2 (task compiler-type-hygiene): infer_mul_div_result — step-1 RED ───
    //
    // Unit tests for the new `infer_mul_div_result(op, left, right) ->
    // Option<Type>`, pinned row-for-row against the β1 runtime truth table
    // (`crates/reify-expr/tests/mul_div_runtime_truth_table.rs`). This batch
    // covers the numeric/Scalar-core and aggregate-scale (Vector/Point/Tensor)
    // arms only — Complex/Transform arms are step-3/4.
    //
    // RED: `infer_mul_div_result` does not exist yet.

    fn time_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::TIME,
        }
    }

    #[test]
    fn infer_mul_div_result_scalar_times_scalar_multiplies_dimensions() {
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::length(), &Type::length()),
            Some(Type::Scalar {
                dimension: DimensionVector::LENGTH.mul(&DimensionVector::LENGTH),
            }),
        );
    }

    #[test]
    fn infer_mul_div_result_scalar_div_scalar_divides_dimensions() {
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &Type::length(), &Type::length()),
            Some(Type::Scalar {
                dimension: DimensionVector::LENGTH.div(&DimensionVector::LENGTH),
            }),
        );
    }

    #[test]
    fn infer_mul_div_result_int_times_int_yields_int() {
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::Int, &Type::Int),
            Some(Type::Int)
        );
    }

    #[test]
    fn infer_mul_div_result_int_div_int_yields_int_exemption() {
        // β3 exemption ledger (PRD decision 4): stays Some(Int) statically even
        // though the runtime widens to Real on non-divisible operands.
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &Type::Int, &Type::Int),
            Some(Type::Int)
        );
    }

    #[test]
    fn infer_mul_div_result_scalar_times_int_preserves_dimension_both_orders() {
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::length(), &Type::Int),
            Some(Type::length()),
        );
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::Int, &Type::length()),
            Some(Type::length()),
        );
    }

    #[test]
    fn infer_mul_div_result_scalar_div_int_preserves_dimension() {
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &Type::length(), &Type::Int),
            Some(Type::length()),
        );
    }

    #[test]
    fn infer_mul_div_result_int_div_scalar_yields_reciprocal_dimension() {
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &Type::Int, &time_ty()),
            Some(Type::Scalar {
                dimension: DimensionVector::DIMENSIONLESS.div(&DimensionVector::TIME),
            }),
        );
    }

    #[test]
    fn infer_mul_div_result_dimensionless_scalar_div_scalar_yields_reciprocal_dimension() {
        // A `Real` literal types as `Scalar{DIMENSIONLESS}` — there is no `Type::Real`.
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &Type::dimensionless_scalar(), &time_ty()),
            Some(Type::Scalar {
                dimension: DimensionVector::DIMENSIONLESS.div(&DimensionVector::TIME),
            }),
        );
    }

    #[test]
    fn infer_mul_div_result_vector_times_dimensionless_preserves_vector() {
        let v = Type::vec3(Type::length());
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &v, &Type::dimensionless_scalar()),
            Some(v),
        );
    }

    #[test]
    fn infer_mul_div_result_int_times_vector_is_commutative() {
        let v = Type::vec3(Type::length());
        assert_eq!(infer_mul_div_result(BinOp::Mul, &Type::Int, &v), Some(v));
    }

    #[test]
    fn infer_mul_div_result_point_times_int_scales_both_orders() {
        let p = Type::point3(Type::length());
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &p, &Type::Int),
            Some(p.clone())
        );
        assert_eq!(infer_mul_div_result(BinOp::Mul, &Type::Int, &p), Some(p));
    }

    #[test]
    fn infer_mul_div_result_tensor_times_int_scales_both_orders() {
        let t = Type::tensor(1, 3, Type::length());
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &t, &Type::Int),
            Some(t.clone())
        );
        assert_eq!(infer_mul_div_result(BinOp::Mul, &Type::Int, &t), Some(t));
    }

    #[test]
    fn infer_mul_div_result_vector_div_dimensionless_preserves_vector_row6() {
        // β1 row-6 pin: `Vector3<Force> / dimensionless -> Vector3<Force>`.
        let v = Type::vec3(force_ty());
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &v, &Type::dimensionless_scalar()),
            Some(v),
        );
    }

    #[test]
    fn infer_mul_div_result_vector_div_scalar_time_yields_reciprocal_component() {
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &Type::vec3(Type::length()), &time_ty()),
            Some(Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH.div(&DimensionVector::TIME),
            })),
        );
    }

    #[test]
    fn infer_mul_div_result_div_vector_scale_is_not_commutative() {
        // Div has no reverse-scale arm: `dimensionless / Vector3<Length>` is unsupported.
        assert_eq!(
            infer_mul_div_result(
                BinOp::Div,
                &Type::dimensionless_scalar(),
                &Type::vec3(Type::length())
            ),
            None,
        );
    }

    #[test]
    fn infer_mul_div_result_vector_times_vector_is_none() {
        let v = Type::vec3(Type::length());
        assert_eq!(infer_mul_div_result(BinOp::Mul, &v, &v), None);
    }

    #[test]
    fn infer_mul_div_result_tensor_times_tensor_is_none() {
        let t = Type::tensor(1, 3, Type::length());
        assert_eq!(infer_mul_div_result(BinOp::Mul, &t, &t), None);
    }

    #[test]
    fn infer_mul_div_result_point_times_point_is_none() {
        let p = Type::point3(Type::length());
        assert_eq!(infer_mul_div_result(BinOp::Mul, &p, &p), None);
    }

    #[test]
    fn infer_mul_div_result_scalar_div_vector_is_none() {
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &Type::length(), &Type::vec3(Type::length())),
            None,
        );
    }

    #[test]
    fn infer_mul_div_result_matrix_operand_is_none_both_orders() {
        let m = Type::matrix(2, 2, Type::length());
        assert_eq!(infer_mul_div_result(BinOp::Mul, &m, &Type::Int), None);
        assert_eq!(infer_mul_div_result(BinOp::Mul, &Type::Int, &m), None);
    }

    #[test]
    fn infer_mul_div_result_list_operand_is_none() {
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::List(Box::new(Type::Int)), &Type::Int),
            None,
        );
    }

    #[test]
    fn infer_mul_div_result_bool_operand_is_none() {
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::Bool, &Type::Int),
            None
        );
    }

    /// `ScalarParam(Q) * ScalarParam(Q)` is deliberately left unhandled: the
    /// combined dimension ("Q²") is not representable by `ScalarParam`'s
    /// bare-name form (see the `ScalarParam` arms' doc comment above) — a
    /// static representational gap, not a runtime error, so the `expr.rs`
    /// guard defers (gradualism skip) rather than poisoning; regression pin
    /// for that documented decision at the `infer_mul_div_result` level. See
    /// `infer_binop_type_scalar_param_times_scalar_param_propagates_q` below
    /// for the corresponding `infer_binop_type`-level pin of what the
    /// deferral actually resolves to.
    #[test]
    fn infer_mul_div_result_scalar_param_times_scalar_param_is_none() {
        let q = Type::ScalarParam("Q".to_string());
        assert_eq!(infer_mul_div_result(BinOp::Mul, &q, &q), None);
    }

    /// `Int / ScalarParam(Q)` is deliberately left unhandled: the `Int ⊗
    /// ScalarParam` arms above cover only `Mul` (dimension-preserving); `Div`
    /// (reciprocal dimension) has no `ScalarParam` arm — regression pin for
    /// that documented decision.
    #[test]
    fn infer_mul_div_result_int_div_scalar_param_is_none() {
        let q = Type::ScalarParam("Q".to_string());
        assert_eq!(infer_mul_div_result(BinOp::Div, &Type::Int, &q), None);
    }

    /// `ScalarParam(Q) * Int` preserves `Q` (Int carries no dimension) — pins
    /// the `(Type::ScalarParam(name), Type::Int)` arm's Some-returning result
    /// type, not just the adjacent None-returning edges above.
    #[test]
    fn infer_mul_div_result_scalar_param_times_int_preserves_q() {
        let q = Type::ScalarParam("Q".to_string());
        assert_eq!(infer_mul_div_result(BinOp::Mul, &q, &Type::Int), Some(q));
    }

    /// `Int * ScalarParam(Q)` preserves `Q` — the commutative (Mul-only)
    /// counterpart of the arm above.
    #[test]
    fn infer_mul_div_result_int_times_scalar_param_preserves_q() {
        let q = Type::ScalarParam("Q".to_string());
        assert_eq!(infer_mul_div_result(BinOp::Mul, &Type::Int, &q), Some(q));
    }

    /// `ScalarParam(Q) * Scalar{DIMENSIONLESS}` preserves `Q` — the
    /// `scale_q<Q: Dimension>(x: Scalar<Q>, k: Real) -> Scalar<Q> { x * k }`
    /// pattern (`dim_param_scale_q_resolves_at_two_dimensions` /
    /// `examples/generics/dim_param.ri`) pinned at the `infer_mul_div_result`
    /// level, not just via the `infer_binop_type` delegation tests.
    #[test]
    fn infer_mul_div_result_scalar_param_times_dimensionless_preserves_q() {
        let q = Type::ScalarParam("Q".to_string());
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &q, &Type::dimensionless_scalar()),
            Some(q)
        );
    }

    /// `Scalar{DIMENSIONLESS} * ScalarParam(Q)` preserves `Q` — the reverse-order
    /// (Mul-only) counterpart of the arm above.
    #[test]
    fn infer_mul_div_result_dimensionless_times_scalar_param_preserves_q() {
        let q = Type::ScalarParam("Q".to_string());
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::dimensionless_scalar(), &q),
            Some(q)
        );
    }

    #[test]
    fn infer_binop_type_delegates_to_infer_mul_div_result_for_vector_scale() {
        let v = Type::vec3(Type::length());
        assert_eq!(infer_binop_type(BinOp::Mul, &v, &Type::Int), v);
    }

    /// `Scalar<Length> / P::MotionValue` (an unresolved trait-associated-type
    /// projection, before a concrete arg substitutes `P`) must propagate the
    /// `Projection` itself, NOT collapse to the `Type::Int` placeholder. The
    /// `expr.rs` operand-kind guard skips `Type::Projection` operands
    /// (mirrors its `TypeParam` skip, PRD decision 3), so an unqualified
    /// `Int` result would leak downstream unpoisoned, risking a spurious
    /// cascade on a later dimensioned op (e.g. `result + x` where
    /// `x: Scalar<Length>` misreading as `Int + Scalar<Length>` in the
    /// Add/Sub dimension guard). Regression pin for the gradualism gap closed
    /// alongside the `Type::Projection` arm in `infer_binop_type`'s Mul/Div
    /// case above.
    #[test]
    fn infer_binop_type_scalar_div_projection_propagates_projection_not_int() {
        let projection = Type::Projection {
            base: Box::new(Type::TypeParam("P".to_string())),
            member: "MotionValue".to_string(),
        };
        assert_eq!(
            infer_binop_type(BinOp::Div, &Type::length(), &projection),
            projection
        );
    }

    /// Reverse-order (Mul) counterpart: `P::MotionValue * Scalar<Length>` also
    /// propagates the `Projection`, not `Int`.
    #[test]
    fn infer_binop_type_projection_mul_scalar_propagates_projection_not_int() {
        let projection = Type::Projection {
            base: Box::new(Type::TypeParam("P".to_string())),
            member: "MotionValue".to_string(),
        };
        assert_eq!(
            infer_binop_type(BinOp::Mul, &projection, &Type::length()),
            projection
        );
    }

    /// `ScalarParam(Q) * ScalarParam(Q)` (e.g. the body of
    /// `fn area<Q: Dimension>(x: Scalar<Q>) { x * x }`, before a concrete arg
    /// substitutes `Q`) must propagate the `ScalarParam` itself, NOT collapse
    /// to the `Type::Int` placeholder — same gradualism-leak class as the
    /// `Type::Projection` pins above, closed for `Type::ScalarParam` by
    /// amendment round 3. The `expr.rs` operand-kind guard now skips
    /// `Type::ScalarParam` operands (mirrors its `Type::Projection` skip), so
    /// an unqualified `Int` result would otherwise leak downstream unpoisoned,
    /// risking a spurious cascade on a later dimensioned op. Regression pin
    /// for the `Type::ScalarParam` arm in `mul_div_result_or_placeholder`.
    #[test]
    fn infer_binop_type_scalar_param_times_scalar_param_propagates_q_not_int() {
        let q = Type::ScalarParam("Q".to_string());
        assert_eq!(infer_binop_type(BinOp::Mul, &q, &q), q);
    }

    /// `Int / ScalarParam(Q)` counterpart: also propagates the `ScalarParam`,
    /// not `Int` — pins the `Div` non-commutative-reciprocal case (no
    /// `ScalarParam` Div arm exists, see `infer_mul_div_result`) through the
    /// same placeholder path.
    #[test]
    fn infer_binop_type_int_div_scalar_param_propagates_q_not_int() {
        let q = Type::ScalarParam("Q".to_string());
        assert_eq!(infer_binop_type(BinOp::Div, &Type::Int, &q), q);
    }

    /// `TypeParam(_) * Int` (e.g. `Type::TypeParam("StructureMember")`, a
    /// purpose-subject member access's static type — `subject.a` in
    /// `purpose marg(subject: Widget) { let m = subject.a - subject.b; let n =
    /// m * 2; constraint n > 0mm }`) must propagate the `TypeParam` itself,
    /// NOT collapse to the `Type::Int` placeholder.
    ///
    /// Deliberately calls `mul_div_result_or_placeholder` directly rather than
    /// `infer_binop_type`: `infer_binop_type`'s own pre-match early-return
    /// (this file, `infer_binop_type`'s `BinOp::Add | BinOp::Sub | BinOp::Mul
    /// | BinOp::Div | BinOp::Mod | BinOp::Pow` guard) already short-circuits a
    /// `TypeParam` operand BEFORE reaching this function, so a test that goes
    /// through `infer_binop_type` cannot observe a regression here.
    /// `expr.rs::compile_binop` calls `infer_mul_div_result` and
    /// `mul_div_result_or_placeholder` DIRECTLY for `*`/`/` (see this
    /// function's own doc), bypassing `infer_binop_type`'s early-return
    /// entirely — so this function needed its OWN `TypeParam` propagation arm
    /// to close the same gradualism gap on that path. Without it, `n` above
    /// statically typed `Int`, and `constraint n > 0mm` produced a false `Int
    /// vs Scalar[m]` mismatch (`purpose_let_multi_let_earlier_let_visibility`
    /// integration regression, `crates/reify-compiler/tests/purpose_compile_tests.rs`).
    /// `Type::Error * Int` (e.g. `undef * 5`, since `undef` compiles to
    /// `Literal(Value::Undef, Type::Error)`) must propagate `Type::Error`
    /// itself, NOT collapse to the `Type::Int` placeholder — same
    /// gradualism-leak class as the `TypeParam`/`Projection`/`ScalarParam`
    /// pins below. `infer_binop_type`'s own pre-match `is_error()`
    /// early-return (above) already handles this case when callers go
    /// through `infer_binop_type` — but `expr.rs::compile_binop` calls
    /// `infer_mul_div_result` and this function DIRECTLY for `*`/`/`,
    /// bypassing that early-return entirely, so without this arm `5 * undef`
    /// statically typed `Int` instead of the anti-cascade `Error`. The
    /// `expr.rs` guard deliberately skips re-poisoning an already-`Error`
    /// operand (gradualism), so nothing downstream corrects the leak.
    /// Regression pin for the `Type::Error` arm in
    /// `mul_div_result_or_placeholder`; integration-level symptom:
    /// `undef_literal_compile_tests::binary_with_undef_emits_no_unresolved_name_diagnostic`.
    #[test]
    fn mul_div_result_or_placeholder_error_propagates_not_int() {
        assert_eq!(
            mul_div_result_or_placeholder(
                infer_mul_div_result(BinOp::Mul, &Type::Error, &Type::Int),
                &Type::Error,
                &Type::Int
            ),
            Type::Error
        );
    }

    /// Reverse-order (`Int * Type::Error`) counterpart of the pin above.
    #[test]
    fn mul_div_result_or_placeholder_int_times_error_propagates_not_int() {
        assert_eq!(
            mul_div_result_or_placeholder(
                infer_mul_div_result(BinOp::Mul, &Type::Int, &Type::Error),
                &Type::Int,
                &Type::Error
            ),
            Type::Error
        );
    }

    #[test]
    fn mul_div_result_or_placeholder_type_param_propagates_not_int() {
        let t = Type::TypeParam("StructureMember".to_string());
        assert_eq!(
            mul_div_result_or_placeholder(
                infer_mul_div_result(BinOp::Mul, &t, &Type::Int),
                &t,
                &Type::Int
            ),
            t
        );
    }

    /// Reverse-order (`Int * TypeParam(_)`) counterpart of the pin above.
    #[test]
    fn mul_div_result_or_placeholder_int_times_type_param_propagates_not_int() {
        let t = Type::TypeParam("StructureMember".to_string());
        assert_eq!(
            mul_div_result_or_placeholder(
                infer_mul_div_result(BinOp::Mul, &Type::Int, &t),
                &Type::Int,
                &t
            ),
            t
        );
    }

    // ── β2 step-3 RED — infer_mul_div_result: Complex + Transform arms ──────
    //
    // Extends infer_mul_div_result with the Complex(q) and Transform(n) arms,
    // pinned row-for-row against the β1 runtime truth table (mul: lib.rs
    // 4361-4458, 4485-4626; div: lib.rs 4699-4754).
    //
    // RED: step-2 returns None for every Complex/Transform combo below (no
    // arm exists yet), so infer_binop_type falls back to the Type::Int
    // placeholder instead of the Complex/aggregate result.

    fn area_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::AREA,
        }
    }

    #[test]
    fn infer_mul_div_result_complex_times_complex_multiplies_dimensions() {
        let length_complex = Type::complex(Type::length());
        let time_complex = Type::complex(time_ty());
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &length_complex, &time_complex),
            Some(Type::complex(Type::Scalar {
                dimension: DimensionVector::LENGTH.mul(&DimensionVector::TIME),
            })),
        );
    }

    #[test]
    fn infer_mul_div_result_complex_times_scalar_multiplies_dimensions_both_orders() {
        let length_complex = Type::complex(Type::length());
        let expected = Some(Type::complex(Type::Scalar {
            dimension: DimensionVector::LENGTH.mul(&DimensionVector::TIME),
        }));
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &length_complex, &time_ty()),
            expected,
        );
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &time_ty(), &length_complex),
            expected,
        );
    }

    #[test]
    fn infer_mul_div_result_complex_times_int_preserves_dimension_both_orders() {
        let length_complex = Type::complex(Type::length());
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &length_complex, &Type::Int),
            Some(length_complex.clone()),
        );
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::Int, &length_complex),
            Some(length_complex),
        );
    }

    #[test]
    fn infer_mul_div_result_complex_div_complex_divides_dimensions() {
        let area_complex = Type::complex(area_ty());
        let length_complex = Type::complex(Type::length());
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &area_complex, &length_complex),
            Some(Type::complex(Type::Scalar {
                dimension: DimensionVector::AREA.div(&DimensionVector::LENGTH),
            })),
        );
    }

    #[test]
    fn infer_mul_div_result_complex_div_scalar_divides_dimensions() {
        let area_complex = Type::complex(area_ty());
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &area_complex, &Type::length()),
            Some(Type::complex(Type::Scalar {
                dimension: DimensionVector::AREA.div(&DimensionVector::LENGTH),
            })),
        );
    }

    #[test]
    fn infer_mul_div_result_complex_div_int_preserves_dimension() {
        let length_complex = Type::complex(Type::length());
        assert_eq!(
            infer_mul_div_result(BinOp::Div, &length_complex, &Type::Int),
            Some(length_complex),
        );
    }

    #[test]
    fn infer_mul_div_result_transform_times_vector_yields_vector() {
        let v = Type::vec3(Type::length());
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::transform(3), &v),
            Some(v),
        );
    }

    #[test]
    fn infer_mul_div_result_transform_times_point_yields_point() {
        let p = Type::point3(Type::length());
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::transform(3), &p),
            Some(p),
        );
    }

    #[test]
    fn infer_mul_div_result_transform_times_transform_yields_transform_row9() {
        // β1 row-9 pin: `Transform(3) * Transform(3) -> Transform(3)`.
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &Type::transform(3), &Type::transform(3)),
            Some(Type::transform(3)),
        );
    }

    #[test]
    fn infer_mul_div_result_vector_times_transform_is_none_order_sensitive() {
        // Order-sensitive: Transform × Vector IS supported (above), but the
        // reverse Vector × Transform has no runtime-intentional arm.
        let v = Type::vec3(Type::length());
        assert_eq!(
            infer_mul_div_result(BinOp::Mul, &v, &Type::transform(3)),
            None,
        );
    }
}
