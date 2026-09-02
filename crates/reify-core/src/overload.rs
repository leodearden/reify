//! The three-tier overload-resolution ladder — one definition, two consumers.
//!
//! This module is the NORMATIVE home of the per-slot predicates that decide
//! whether a candidate function's declared parameter type accepts a given
//! argument type. It exists so that compile-time and eval-time overload
//! selection cannot disagree about the same call: a divergence between them is
//! the esc-4231-120/126 / esc-4093-152 class, where a call typechecks against
//! one overload and then evaluates against another (or, worse, against none —
//! silently yielding `Value::Undef`). Before #5689 the predicates below lived
//! as a hand-synced mirror pair, and had in fact drifted.
//!
//! # The ladder
//!
//! Three tiers, applied in order from narrowest to broadest:
//!
//! | Tier | Name     | Predicate                                       |
//! |------|----------|-------------------------------------------------|
//! | 1    | EXACT    | `param_ty == arg_ty`                            |
//! | 2    | HEAD     | [`slot_matches_head_tier`]                      |
//! | 3    | WILDCARD | [`slot_matches_wildcard_tier`]                  |
//!
//! Resolution order is **exact → head → wildcard**, and an EMPTY tier falls
//! through to the next-broader one. That fall-through is load-bearing: it
//! preserves the deliberate select-then-conflict behaviour where a
//! constructor-headed generic param over-selects a mismatched-head arg so the
//! call site can emit `E_FN_TYPE_ARG_CONFLICT` rather than an opaque no-match.
//!
//! Tier 1 deliberately has NO helper here. It is one operator, it cannot
//! drift, and a wrapper would be pure indirection.
//!
//! # Caller contract: tier 2 is a FILTER, not a pass
//!
//! Tier 2 MUST be applied as a filter over the set surviving tier 3, never
//! standalone. It is narrower than tier 3 on the type-param-param disjunct,
//! but it is **not a subset** of it: `heads_unifiable`'s erased-subject arm
//! (`Applied{name, ..}` vs `Enum(name)`) accepts a pair tier 3 rejects
//! outright, because tier 3 only ever compares an `Applied` param to an `Enum`
//! arg by plain equality. Concretely, for param `Applied{"Result",[Int,
//! String]}` and arg `Enum("Result")` with `is_generic == true`,
//! [`slot_matches_head_tier`] is `true` while [`slot_matches_wildcard_tier`]
//! is `false`. The relation is confined to GENERIC candidates: with
//! `is_generic == false` the head tier's `heads_unifiable` arm is gated off
//! and head genuinely IS a subset — so screening is load-bearing exactly for
//! the generic overloads tier 2 exists to disambiguate.
//!
//! A standalone head pass would therefore WIDEN resolution rather than narrow
//! it, admitting candidates the wildcard tier had already excluded. Screened
//! through tier 3 it can only ever NARROW a would-be ambiguity. The
//! counterexample is pinned by
//! `slot_matches_head_tier_is_not_a_subset_of_the_wildcard_tier` in this
//! module's tests.
//!
//! # What stays caller-side policy
//!
//! This module answers exactly one question — "does THIS param slot accept
//! THIS arg?". Everything above the slot stays with the caller:
//!
//! - the arity check (`f.params.len() == arg_types.len()`);
//! - computing `is_generic = !f.type_params.is_empty()` for the candidate;
//! - the per-candidate `.all()` over slots;
//! - the final classification over the surviving set — `reify-compiler`
//!   reports `Resolved` / `Ambiguous` / `NoMatch` by set size, while
//!   `reify-expr` takes first-match-wins.
//!
//! Those differ legitimately between the two consumers. The DISJUNCT LISTS —
//! the thing that actually drifted — do not, and live here.
//!
//! # Why the API is per-SLOT and not per-candidate
//!
//! A per-candidate API would have to name `CompiledFunction`, which lives in
//! `reify-ir` — and `reify-ir` depends on `reify-core`, so naming it here
//! would break the B1 zero-`reify-*`-deps invariant locked by
//! `crates/reify-core/tests/dag_invariant.rs`. The two callers also hold
//! different argument shapes (`&[Type]` compile-side vs `&[CompiledExpr]`
//! eval-side, needing `arg.result_type`). Per-slot over `&Type` is the widest
//! contract both can share.
//!
//! # Consumers
//!
//! `reify_compiler::type_compat::resolve_function_overload` (compile time) and
//! `reify_expr::find_matching_compiled_function` (eval time). Any new consumer
//! must honour the tier-2-is-a-filter contract above.

use crate::ty::Type;

/// Returns `true` when `t` is, or recursively wraps, a `Type::TraitObject`.
///
/// Covers bare `TraitObject(name)` and the four generic wrappers
/// `Option<T>`, `List<T>`, `Set<T>`, and `Map<K,V>`.  A `Map<TraitObject, V>`
/// or `Map<K, TraitObject>` is also trait-carrying because both positions
/// participate in conformance checking.  `Applied` type args and a
/// `Projection` base are also walked (task 4602 β).
///
/// Used by the overload wildcard tier to make trait-carrying params act as
/// resolution wildcards (match any arg type), while concrete params keep
/// exact-equality semantics.  This disjunct is NOT gated on candidate
/// genericity, so it applies to every function in the program.
///
/// NOTE the DELIBERATE asymmetry with the two sibling predicates
/// [`type_carries_type_param`] and [`type_carries_dim_param`]: this one walks a
/// strictly NARROWER constructor set — no `Field`, `Function`, `Union`,
/// `Keyed`, `Complex`, `Range`, or quantity-slot recursion. That is existing
/// behaviour and must be preserved, not "fixed"; widening it widens overload
/// resolution for every call site. The boundary is pinned by
/// `type_carries_trait_object_covers_its_narrower_walk` in this module's tests.
pub fn type_carries_trait_object(t: &Type) -> bool {
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
/// `unify` and `substitute_type_params` —
/// `List`/`Set`/`Keyed`/`Option`/`Complex`/`Range`,
/// `Point`/`Vector`/`Tensor`/`Matrix` (quantity slot), `Map`, `Field`,
/// `Function` (params + return), and `Union` — so a generic param that embeds a
/// type-param inside ANY of those (e.g. `Field<T, Real>`, `List<Field<T>>`) is
/// recognized. Keeping this predicate aligned with the unify/substitute walks
/// avoids the asymmetry where overload resolution would reject a param shape
/// the downstream inference machinery can actually handle.
///
/// Used by the overload wildcard tier to make a *generic* candidate's
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
pub fn type_carries_type_param(t: &Type) -> bool {
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
/// [`type_carries_type_param`], `unify`, and `substitute_type_params`.
///
/// Wired into the generic-candidate wildcard tier (OR'd with
/// [`type_carries_type_param`]) so that a `Scalar<Q>` parameter is recognised
/// as a generic wildcard slot (task 4235 ζ / D8).
pub fn type_carries_dim_param(t: &Type) -> bool {
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

/// Strict constructor-head compatibility check — the middle tie-break tier
/// of the overload ladder (D-head-exact, result-fallback Layer-B task, B2).
///
/// Mirrors `unify`'s arm structure (the same constructor pairs recurse on
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
///
/// DELIBERATELY `pub(crate)`, not `pub`. This is the *implementation* of the
/// head tier, not a rung of the ladder: both consumers reach it only through
/// [`slot_matches_head_tier`], and this module's own tests are in-crate. Its
/// eval-side predecessor was `#[doc(hidden)] pub` purely for cross-crate test
/// reachability — a concession #5689 was meant to retire, so do not re-widen
/// it to `pub` (that would advertise it as `reify-core` API and pull it into
/// `tests/api_surface.rs`). Public intra-doc links to it are written as plain
/// code spans for the same reason.
pub(crate) fn heads_unifiable(param: &Type, arg: &Type) -> bool {
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

/// Tier 3 of the ladder — the broadest per-slot gate (WILDCARD).
///
/// `is_generic` is the CANDIDATE's genericity (`!f.type_params.is_empty()`),
/// computed caller-side.
///
/// For a GENERIC candidate, a type-param-carrying param is a resolution
/// wildcard (matches any arg) — mirroring the trait-object wildcard. Gated on
/// `is_generic` so non-generic fns (empty `type_params`) are bit-for-bit
/// unchanged (INV-6). A full wildcard (not structural unify) is deliberate: a
/// conflicting generic call (e.g. `pair(1, 1.5)`) still SELECTS the candidate
/// so the call site can emit `E_FN_TYPE_ARG_CONFLICT` rather than a generic
/// no-match.
///
/// D4 (task-4232 γ): A type-param-carrying ARG also acts as a resolution
/// wildcard (matches any param). This lets a generic fn body pass a
/// `TypeParam`-typed value to a concrete-param function without a spurious
/// NoMatch. It is self-scoping: `TypeParam` args only arise inside generic fn
/// bodies, so concrete-arg calls (non-generic callers) are bit-for-bit
/// unchanged — `type_carries_type_param(concrete) == false`. Note it is NOT
/// gated on `is_generic`: the genericity in question belongs to the CALLER
/// whose body produced the `T`-typed value, not to the candidate being
/// matched.
pub fn slot_matches_wildcard_tier(param_ty: &Type, arg_ty: &Type, is_generic: bool) -> bool {
    type_carries_trait_object(param_ty)
        || (is_generic && (type_carries_type_param(param_ty) || type_carries_dim_param(param_ty)))
        || type_carries_type_param(arg_ty)
        || param_ty == arg_ty
}

/// Tier 2 of the ladder — the middle tie-break gate (HEAD).
///
/// Narrower than [`slot_matches_wildcard_tier`] on the type-param-param
/// disjunct: structural `heads_unifiable` instead of a full wildcard. This
/// disambiguates two GENERIC overloads with different container heads — e.g. a
/// user `unwrap_or<T,E>(r: Result<T,E>, ..)` vs the stdlib
/// `unwrap_or<T>(o: Option<T>, ..)` — which would otherwise both
/// wildcard-match any subject via `type_carries_type_param` and force a
/// spurious `Ambiguous`.
///
/// Only the `type_carries_type_param(param_ty)` disjunct is replaced by
/// `heads_unifiable`; [`type_carries_dim_param`] stays a FULL wildcard here —
/// dimension-param overload resolution is orthogonal to enum-head
/// disambiguation.
///
/// D4 (task-4232 γ) in this tier: a type-param ARG is a wildcard ONLY when it
/// is a BARE `Type::TypeParam` (a generic fn body passing a `T`-typed value) —
/// that slot carries no constructor head to disagree on, so `heads_unifiable`
/// cannot discriminate it. A HEADED arg carrying a NESTED type-param (e.g. an
/// `Applied{"Result", [T, E]}` produced by composing two generic stdlib fns
/// over a headless-`Enum` builtin — task #4038 δ) must NOT wildcard-match every
/// candidate: it has a real head, so `heads_unifiable` discriminates it
/// (`Result` matches the `Result<T,E>` overload, not the `Option<T>` one),
/// turning a spurious `Ambiguous` into a clean `Resolved`.
///
/// NOTE (reviewer_comprehensive #2 on the compile-side original): this
/// narrowing also means a NON-generic candidate (`is_generic == false`) is
/// never eligible at this tier against a headed nested-type-param arg — it
/// fails `is_generic`, the bare-`TypeParam` wildcard, and plain equality. The
/// head tier therefore deliberately assumes headed nested-type-param args only
/// ever need to disambiguate GENERIC container overloads (e.g. `Option<T>` vs
/// `Result<T,E>`); a same-name non-generic candidate in the same overload set
/// is excluded from the head tier rather than causing a spurious `Ambiguous`.
/// See `overload_leaky_headed_arg_excludes_non_generic_candidate` in
/// `reify-compiler` for the precedent lock.
///
/// # Caller contract
///
/// This tier MUST be applied as a FILTER over the surviving set of
/// [`slot_matches_wildcard_tier`], never standalone — it is NOT a subset of
/// tier 3. See this module's `//!` doc and the executable counterexample
/// `slot_matches_head_tier_is_not_a_subset_of_the_wildcard_tier`.
pub fn slot_matches_head_tier(param_ty: &Type, arg_ty: &Type, is_generic: bool) -> bool {
    type_carries_trait_object(param_ty)
        || (is_generic && (heads_unifiable(param_ty, arg_ty) || type_carries_dim_param(param_ty)))
        || matches!(arg_ty, Type::TypeParam(_))
        || param_ty == arg_ty
}

#[cfg(test)]
mod tests {
    use crate::ty::Type;

    /// The corpus behind [`heads_unifiable_matches_pinned_corpus_verdicts`]
    /// below: `(param, arg, expected, label)`, with at least one case per match
    /// arm of [`heads_unifiable`].
    ///
    /// Each arm's MATCH case is deliberately paired with a NEAR-MISS that must
    /// fall to the `_ => param == arg` catch-all, so an implementation that
    /// accidentally widened an arm (dropped an `n == n` / arity / name guard) is
    /// caught as readily as one that dropped the arm entirely.
    ///
    /// Keep both verdicts represented: a corpus edited down to all-`true` (or
    /// all-`false`) rows would agree under implementations that had drifted on
    /// the arms it no longer covers.
    fn heads_unifiable_corpus() -> Vec<(Type, Type, bool, &'static str)> {
        let t = || Type::TypeParam("T".to_string());
        let q = || Type::ScalarParam("Q".to_string());
        let result_of = |args: Vec<Type>| Type::Applied {
            name: "Result".to_string(),
            args,
        };
        let proj = |base: Type, member: &str| Type::Projection {
            base: Box::new(base),
            member: member.to_string(),
        };
        let func = |params: Vec<Type>, ret: Type| Type::Function {
            params,
            return_type: Box::new(ret),
        };
        let field = |domain: Type, codomain: Type| Type::Field {
            domain: Box::new(domain),
            codomain: Box::new(codomain),
        };

        // (param, arg, expected, label) — label names the arm the pair exercises.
        vec![
            // Type-param / dim-param wildcard leaves.
            (t(), Type::Int, true, "TypeParam vs anything"),
            (
                t(),
                result_of(vec![Type::Int]),
                true,
                "TypeParam vs constructor",
            ),
            (q(), Type::length(), true, "ScalarParam vs Scalar"),
            (
                q(),
                Type::Int,
                false,
                "ScalarParam vs non-Scalar (catch-all)",
            ),
            // Single-inner-Type constructors, match + near-miss.
            (
                Type::List(Box::new(t())),
                Type::List(Box::new(Type::Int)),
                true,
                "List recurse",
            ),
            (
                Type::List(Box::new(Type::Int)),
                Type::List(Box::new(Type::String)),
                false,
                "List inner mismatch",
            ),
            (
                Type::List(Box::new(Type::Int)),
                Type::Set(Box::new(Type::Int)),
                false,
                "List vs Set head",
            ),
            (
                Type::Set(Box::new(t())),
                Type::Set(Box::new(Type::Int)),
                true,
                "Set recurse",
            ),
            (
                Type::Keyed(Box::new(t())),
                Type::Keyed(Box::new(Type::Int)),
                true,
                "Keyed recurse",
            ),
            (
                Type::Option(Box::new(t())),
                Type::Option(Box::new(Type::length())),
                true,
                "Option recurse",
            ),
            (
                Type::Option(Box::new(t())),
                Type::Enum("Option".to_string()),
                false,
                "Option vs erased Enum (NOT the Applied rule)",
            ),
            (
                Type::Complex(Box::new(t())),
                Type::Complex(Box::new(Type::dimensionless_scalar())),
                true,
                "Complex recurse",
            ),
            (
                Type::Range(Box::new(t())),
                Type::Range(Box::new(Type::Int)),
                true,
                "Range recurse",
            ),
            // Two-inner-Type constructors.
            (
                Type::Map(Box::new(Type::String), Box::new(t())),
                Type::Map(Box::new(Type::String), Box::new(Type::Int)),
                true,
                "Map recurse",
            ),
            (
                Type::Map(Box::new(Type::String), Box::new(Type::Int)),
                Type::Map(Box::new(Type::Int), Box::new(Type::Int)),
                false,
                "Map key mismatch",
            ),
            (
                field(t(), t()),
                field(Type::length(), Type::Int),
                true,
                "Field recurse",
            ),
            (
                field(Type::Int, Type::Int),
                field(Type::String, Type::Int),
                false,
                "Field domain mismatch",
            ),
            // Function: arity guard + recursion on params and return type.
            (
                func(vec![t()], t()),
                func(vec![Type::Int], Type::String),
                true,
                "Function recurse",
            ),
            (
                func(vec![t()], t()),
                func(vec![Type::Int, Type::Int], Type::Int),
                false,
                "Function arity",
            ),
            (
                func(vec![Type::Int], Type::Int),
                func(vec![Type::String], Type::Int),
                false,
                "Function param mismatch",
            ),
            // Quantity-bearing aggregates: shape guards + quantity recursion.
            (
                Type::Point {
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Point {
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                true,
                "Point recurse",
            ),
            (
                Type::Point {
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Point {
                    n: 2,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Point n guard",
            ),
            (
                Type::Vector {
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Vector {
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                true,
                "Vector recurse",
            ),
            (
                Type::Vector {
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Vector {
                    n: 2,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Vector n guard",
            ),
            (
                Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                true,
                "Tensor recurse",
            ),
            (
                Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Tensor {
                    rank: 3,
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Tensor rank guard",
            ),
            (
                Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Tensor {
                    rank: 2,
                    n: 2,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Tensor n guard",
            ),
            (
                Type::Matrix {
                    m: 3,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Matrix {
                    m: 3,
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                true,
                "Matrix recurse",
            ),
            (
                Type::Matrix {
                    m: 3,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Matrix {
                    m: 2,
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Matrix m guard",
            ),
            (
                Type::Matrix {
                    m: 3,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Matrix {
                    m: 3,
                    n: 2,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Matrix n guard",
            ),
            // Union: length guard + arm-by-arm recursion.
            (
                Type::Union(vec![t(), Type::Int]),
                Type::Union(vec![Type::String, Type::Int]),
                true,
                "Union recurse",
            ),
            (
                Type::Union(vec![t()]),
                Type::Union(vec![Type::Int, Type::Int]),
                false,
                "Union length guard",
            ),
            (
                Type::Union(vec![Type::Int, Type::Int]),
                Type::Union(vec![Type::Int, Type::String]),
                false,
                "Union arm mismatch",
            ),
            // Applied: name + arity guards, element-wise recursion.
            (
                result_of(vec![t(), t()]),
                result_of(vec![Type::length(), Type::String]),
                true,
                "Applied recurse",
            ),
            (
                result_of(vec![t(), t()]),
                Type::Applied {
                    name: "Either".to_string(),
                    args: vec![Type::Int, Type::Int],
                },
                false,
                "Applied name guard",
            ),
            (
                result_of(vec![t(), t()]),
                result_of(vec![Type::Int]),
                false,
                "Applied arity guard",
            ),
            // Erased-subject rule — the arm the whole #5685 fix turns on.
            (
                result_of(vec![t(), t()]),
                Type::Enum("Result".to_string()),
                true,
                "Applied vs erased Enum, same name",
            ),
            (
                result_of(vec![t(), t()]),
                Type::Enum("Option".to_string()),
                false,
                "Applied vs erased Enum, different name",
            ),
            (
                Type::Option(Box::new(t())),
                result_of(vec![Type::length(), Type::String]),
                false,
                "Option param vs Applied Result arg (the mis-resolution)",
            ),
            // Projection: member guard + base recursion.
            (
                proj(t(), "Out"),
                proj(Type::Int, "Out"),
                true,
                "Projection recurse",
            ),
            (
                proj(t(), "Out"),
                proj(Type::Int, "In"),
                false,
                "Projection member guard",
            ),
            // Catch-all leaves.
            (Type::Int, Type::Int, true, "catch-all equal"),
            (Type::Int, Type::String, false, "catch-all unequal"),
            (
                Type::length(),
                Type::length(),
                true,
                "catch-all equal Scalar",
            ),
            (
                Type::TraitObject("Load".to_string()),
                Type::StructureRef("PointLoad".to_string()),
                false,
                "catch-all TraitObject vs StructureRef",
            ),
        ]
    }

    #[test]
    fn heads_unifiable_matches_pinned_corpus_verdicts() {
        for (param, arg, expected, label) in &heads_unifiable_corpus() {
            assert_eq!(
                super::heads_unifiable(param, arg),
                *expected,
                "heads_unifiable SEMANTICS changed on `{label}`: it no longer \
                 returns {expected} for param={param:?}, arg={arg:?}. If the \
                 rule genuinely changed, update this corpus row — but note that \
                 relaxing a guard here (e.g. the `dn == en` name guard on the \
                 erased-subject arm) makes unrelated overloads head-match, which \
                 is the #5685 Option/Result mis-selection."
            );
        }
    }

    // ── `type_carries_*` predicate coverage ──────────────────────────────────
    //
    // Cases derived from the compile-side tests these predicates arrived with
    // (`type_carries_type_param_recurses_through_all_constructors`,
    // `type_carries_dim_param_*`), extended to the arms those do not reach.
    // The eval side never had direct coverage at all, which is part of why the
    // two copies could drift.

    /// ScalarParam shorthand, matching the compile-side `sp` helper.
    fn sp(name: &str) -> Type {
        Type::ScalarParam(name.to_string())
    }

    /// TypeParam shorthand, matching the compile-side `tp` helper.
    fn tp(name: &str) -> Type {
        Type::TypeParam(name.to_string())
    }

    /// `type_carries_trait_object` deliberately walks FEWER constructors than
    /// its two siblings: only `Option`/`List`/`Set`/`Map`/`Applied` args /
    /// `Projection` base — no `Field`, `Function`, `Union`, `Keyed`, `Complex`,
    /// `Range` or quantity-slot recursion. That asymmetry is EXISTING
    /// BEHAVIOUR and must be preserved, not "fixed": widening it here would
    /// silently turn more params into resolution wildcards. The two negative
    /// assertions at the end pin the boundary.
    #[test]
    fn type_carries_trait_object_covers_its_narrower_walk() {
        let to = || Type::TraitObject("Load".to_string());

        // The leaf itself.
        assert!(super::type_carries_trait_object(&to()));

        // The constructors this predicate DOES recurse through.
        assert!(super::type_carries_trait_object(&Type::Option(Box::new(
            to()
        ))));
        assert!(super::type_carries_trait_object(&Type::List(Box::new(to()))));
        assert!(super::type_carries_trait_object(&Type::Set(Box::new(to()))));
        assert!(
            super::type_carries_trait_object(&Type::Map(
                Box::new(to()),
                Box::new(Type::Int)
            )),
            "Map KEY position participates in conformance checking"
        );
        assert!(
            super::type_carries_trait_object(&Type::Map(
                Box::new(Type::String),
                Box::new(to())
            )),
            "Map VALUE position participates in conformance checking"
        );
        assert!(super::type_carries_trait_object(&Type::Applied {
            name: "Result".to_string(),
            args: vec![Type::Int, to()],
        }));
        assert!(super::type_carries_trait_object(&Type::Projection {
            base: Box::new(to()),
            member: "Out".to_string(),
        }));

        // Nesting composes through the covered constructors.
        assert!(super::type_carries_trait_object(&Type::List(Box::new(
            Type::Option(Box::new(to()))
        ))));

        // Leaves that carry no trait object.
        assert!(!super::type_carries_trait_object(&tp("T")));
        assert!(!super::type_carries_trait_object(&Type::Int));
        assert!(!super::type_carries_trait_object(&Type::List(Box::new(
            Type::Int
        ))));

        // The DELIBERATE non-recursion. These are `false` today and must stay
        // `false`: `type_carries_trait_object` is the ungated tier-3 wildcard
        // disjunct (it applies even to non-generic candidates), so widening
        // its walk widens overload resolution for every function in the
        // program.
        assert!(
            !super::type_carries_trait_object(&Type::Field {
                domain: Box::new(to()),
                codomain: Box::new(Type::Int),
            }),
            "Field is deliberately NOT walked by type_carries_trait_object"
        );
        assert!(
            !super::type_carries_trait_object(&Type::Function {
                params: vec![to()],
                return_type: Box::new(Type::Int),
            }),
            "Function is deliberately NOT walked by type_carries_trait_object"
        );
        assert!(
            !super::type_carries_trait_object(&Type::Union(vec![Type::Int, to()])),
            "Union is deliberately NOT walked by type_carries_trait_object"
        );
        assert!(
            !super::type_carries_trait_object(&Type::Vector {
                n: 3,
                quantity: Box::new(to()),
            }),
            "the quantity slot is deliberately NOT walked by \
             type_carries_trait_object"
        );
    }

    /// `type_carries_type_param` recurses through the same inner-`Type`-bearing
    /// constructor set as the `unify` / `substitute_type_params` walks, so a
    /// generic param embedding a type-param anywhere is recognised. Returns
    /// `true` at the `TypeParam(_)` leaf and `false` at the `ScalarParam(_)`
    /// leaf — dimension params are a distinct kind (D7) covered by the sibling
    /// [`type_carries_dim_param`].
    #[test]
    fn type_carries_type_param_recurses_through_all_constructors() {
        // The leaf itself, and the sibling leaf it must NOT claim.
        assert!(super::type_carries_type_param(&tp("T")));
        assert!(
            !super::type_carries_type_param(&sp("Q")),
            "a dimension param is not a type param (D7)"
        );

        // Single-inner-Type wrappers.
        assert!(super::type_carries_type_param(&Type::List(Box::new(tp("T")))));
        assert!(super::type_carries_type_param(&Type::Set(Box::new(tp("T")))));
        assert!(super::type_carries_type_param(&Type::Keyed(Box::new(tp(
            "T"
        )))));
        assert!(super::type_carries_type_param(&Type::Option(Box::new(tp(
            "T"
        )))));
        assert!(super::type_carries_type_param(&Type::Complex(Box::new(tp(
            "T"
        )))));
        assert!(super::type_carries_type_param(&Type::Range(Box::new(tp(
            "T"
        )))));

        // Quantity-bearing aggregates: the `quantity` slot.
        assert!(super::type_carries_type_param(&Type::Point {
            n: 3,
            quantity: Box::new(tp("T")),
        }));
        assert!(super::type_carries_type_param(&Type::Vector {
            n: 3,
            quantity: Box::new(tp("T")),
        }));
        assert!(super::type_carries_type_param(&Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(tp("T")),
        }));
        assert!(super::type_carries_type_param(&Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(tp("T")),
        }));

        // Two-inner-Type wrappers: BOTH positions.
        assert!(super::type_carries_type_param(&Type::Map(
            Box::new(tp("K")),
            Box::new(Type::Int)
        )));
        assert!(super::type_carries_type_param(&Type::Map(
            Box::new(Type::String),
            Box::new(tp("V"))
        )));
        assert!(super::type_carries_type_param(&Type::Field {
            domain: Box::new(tp("D")),
            codomain: Box::new(Type::dimensionless_scalar()),
        }));
        assert!(super::type_carries_type_param(&Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(tp("C")),
        }));

        // Function: any param, or the return type.
        assert!(super::type_carries_type_param(&Type::Function {
            params: vec![Type::dimensionless_scalar(), tp("T")],
            return_type: Box::new(Type::dimensionless_scalar()),
        }));
        assert!(super::type_carries_type_param(&Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(tp("R")),
        }));

        // Union arms, Applied args, Projection base.
        assert!(super::type_carries_type_param(&Type::Union(vec![
            Type::Int,
            tp("T")
        ])));
        assert!(super::type_carries_type_param(&Type::Applied {
            name: "Result".to_string(),
            args: vec![Type::Int, tp("E")],
        }));
        assert!(super::type_carries_type_param(&Type::Projection {
            base: Box::new(tp("T")),
            member: "Out".to_string(),
        }));

        // Recursion composes through nesting.
        assert!(
            super::type_carries_type_param(&Type::List(Box::new(Type::Field {
                domain: Box::new(tp("D")),
                codomain: Box::new(Type::dimensionless_scalar()),
            }))),
            "recursion must pass through List into Field"
        );

        // Negative: no type-param anywhere.
        assert!(!super::type_carries_type_param(&Type::dimensionless_scalar()));
        assert!(!super::type_carries_type_param(&Type::Int));
        assert!(!super::type_carries_type_param(&Type::TraitObject(
            "Load".to_string()
        )));
        assert!(!super::type_carries_type_param(&Type::List(Box::new(
            Type::Int
        ))));
        assert!(!super::type_carries_type_param(&Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::length()),
        }));
    }

    /// `type_carries_dim_param` is the mirror image of
    /// [`type_carries_type_param`]: same constructor recursion, `true` at the
    /// `ScalarParam(_)` leaf, `false` at the `TypeParam(_)` leaf.
    #[test]
    fn type_carries_dim_param_recurses_through_all_constructors() {
        // The leaf itself, and the sibling leaf it must NOT claim.
        assert!(
            super::type_carries_dim_param(&sp("Q")),
            "ScalarParam should carry a dim-param"
        );
        assert!(
            !super::type_carries_dim_param(&tp("T")),
            "TypeParam should NOT carry a dim-param"
        );

        // Single-inner-Type wrappers.
        assert!(super::type_carries_dim_param(&Type::List(Box::new(sp("Q")))));
        assert!(super::type_carries_dim_param(&Type::Set(Box::new(sp("Q")))));
        assert!(super::type_carries_dim_param(&Type::Keyed(Box::new(sp("Q")))));
        assert!(super::type_carries_dim_param(&Type::Option(Box::new(sp(
            "Q"
        )))));
        assert!(super::type_carries_dim_param(&Type::Complex(Box::new(sp(
            "Q"
        )))));
        assert!(super::type_carries_dim_param(&Type::Range(Box::new(sp("Q")))));

        // Quantity-bearing aggregates: the `quantity` slot.
        assert!(super::type_carries_dim_param(&Type::Point {
            n: 3,
            quantity: Box::new(sp("Q")),
        }));
        assert!(
            super::type_carries_dim_param(&Type::Vector {
                n: 3,
                quantity: Box::new(sp("Q")),
            }),
            "Vector3<ScalarParam(\"Q\")> should carry a dim-param"
        );
        assert!(super::type_carries_dim_param(&Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(sp("Q")),
        }));
        assert!(super::type_carries_dim_param(&Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(sp("Q")),
        }));

        // Two-inner-Type wrappers: BOTH positions.
        assert!(super::type_carries_dim_param(&Type::Map(
            Box::new(sp("Q")),
            Box::new(Type::Int)
        )));
        assert!(super::type_carries_dim_param(&Type::Map(
            Box::new(Type::String),
            Box::new(sp("Q"))
        )));
        assert!(super::type_carries_dim_param(&Type::Field {
            domain: Box::new(sp("Q")),
            codomain: Box::new(Type::dimensionless_scalar()),
        }));
        assert!(super::type_carries_dim_param(&Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(sp("Q")),
        }));

        // Function: any param, or the return type.
        assert!(super::type_carries_dim_param(&Type::Function {
            params: vec![Type::Int, sp("Q")],
            return_type: Box::new(Type::dimensionless_scalar()),
        }));
        assert!(super::type_carries_dim_param(&Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(sp("Q")),
        }));

        // Union arms, Applied args, Projection base.
        assert!(super::type_carries_dim_param(&Type::Union(vec![
            Type::Int,
            sp("Q")
        ])));
        assert!(super::type_carries_dim_param(&Type::Applied {
            name: "Result".to_string(),
            args: vec![Type::Int, sp("Q")],
        }));
        assert!(super::type_carries_dim_param(&Type::Projection {
            base: Box::new(sp("Q")),
            member: "Out".to_string(),
        }));

        // Recursion composes through nesting.
        assert!(super::type_carries_dim_param(&Type::List(Box::new(
            Type::Field {
                domain: Box::new(sp("Q")),
                codomain: Box::new(Type::Int),
            }
        ))));

        // Negative: no dim-param anywhere.
        assert!(
            !super::type_carries_dim_param(&Type::length()),
            "concrete Scalar{{LENGTH}} should NOT carry a dim-param"
        );
        assert!(!super::type_carries_dim_param(&Type::Int));
        assert!(!super::type_carries_dim_param(&Type::List(Box::new(
            Type::Int
        ))));
        assert!(!super::type_carries_dim_param(&Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::length()),
        }));
    }

    // ── the two shared tier predicates ───────────────────────────────────────

    /// Tier 3 (WILDCARD) — the broadest tier, and the one whose two copies had
    /// actually diverged.
    ///
    /// `type_carries_trait_object(param)` is UNGATED on candidate genericity;
    /// the type-param / dim-param param disjuncts are gated on `is_generic`
    /// (INV-6, task 4231 β) so non-generic fns are bit-for-bit unaffected.
    #[test]
    fn slot_matches_wildcard_tier_covers_each_disjunct() {
        // Trait-object-carrying param is a wildcard even for a NON-generic
        // candidate — the esc-4093-152 `List<Load>` shape.
        assert!(
            super::slot_matches_wildcard_tier(
                &Type::List(Box::new(Type::TraitObject("Load".to_string()))),
                &Type::List(Box::new(Type::StructureRef("PointLoad".to_string()))),
                false,
            ),
            "a trait-carrying param is a wildcard regardless of genericity"
        );

        // Type-param- and dim-param-carrying params are wildcards ONLY for a
        // GENERIC candidate (INV-6).
        assert!(super::slot_matches_wildcard_tier(
            &Type::List(Box::new(tp("T"))),
            &Type::List(Box::new(Type::Int)),
            true,
        ));
        assert!(
            !super::slot_matches_wildcard_tier(
                &Type::List(Box::new(tp("T"))),
                &Type::List(Box::new(Type::Int)),
                false,
            ),
            "INV-6: the type-param param disjunct is gated on is_generic"
        );
        assert!(super::slot_matches_wildcard_tier(
            &sp("Q"),
            &Type::length(),
            true,
        ));
        assert!(
            !super::slot_matches_wildcard_tier(&sp("Q"), &Type::length(), false),
            "INV-6: the dim-param param disjunct is gated on is_generic"
        );

        // Plain equality, and its negation.
        assert!(super::slot_matches_wildcard_tier(
            &Type::Int,
            &Type::Int,
            false
        ));
        assert!(!super::slot_matches_wildcard_tier(
            &Type::Int,
            &Type::String,
            false
        ));
    }

    /// THE UNIFICATION PIN (#5689).
    ///
    /// A type-param-carrying ARG is itself a resolution wildcard: D4 /
    /// task-4232 γ. A generic fn body passing a `T`-typed value to a
    /// CONCRETE-param overload must still select that overload rather than
    /// falling to a spurious no-match.
    ///
    /// The compile-side `matches` closure
    /// (`crates/reify-compiler/src/type_compat.rs`, inside
    /// `resolve_function_overload`) has always carried this
    /// `type_carries_type_param(arg_ty)` disjunct; the eval-side `wildcard`
    /// closure in `reify_expr::find_matching_compiled_function` never did.
    /// That single missing disjunct is the whole compile/eval divergence, and
    /// **the compile side is the reference answer** — the compiler already
    /// typechecked the call, so eval agreeing with it is what makes the pair
    /// consistent. The same witness is pinned compile-side by
    /// `overload_bare_type_param_arg_still_resolves`.
    ///
    /// The disjunct is self-scoping: a `TypeParam`-typed arg only arises inside
    /// a generic fn body, so concrete-arg calls are bit-for-bit unchanged
    /// (`type_carries_type_param(concrete) == false`). Note it is NOT gated on
    /// `is_generic` — the genericity in question belongs to the CALLER whose
    /// body produced the `T`-typed value, not to the CANDIDATE being matched.
    #[test]
    fn slot_matches_wildcard_tier_accepts_a_type_param_carrying_arg() {
        assert!(
            super::slot_matches_wildcard_tier(
                &Type::dimensionless_scalar(),
                &tp("U"),
                false,
            ),
            "D4 / task-4232 γ: a bare TypeParam ARG is a wildcard against a \
             concrete param, even for a non-generic candidate. Dropping this \
             disjunct is exactly the divergence #5689 unified away: eval would \
             return None where the compiler Resolved, and the call would \
             silently evaluate to Value::Undef."
        );
        // Self-scoping: a concrete arg is unaffected by this disjunct.
        assert!(!super::slot_matches_wildcard_tier(
            &Type::dimensionless_scalar(),
            &Type::Int,
            false,
        ));
        // A NESTED type-param arg also carries the wildcard at this tier —
        // tier 2 is where headed args get discriminated, not here.
        assert!(super::slot_matches_wildcard_tier(
            &Type::dimensionless_scalar(),
            &Type::Applied {
                name: "Result".to_string(),
                args: vec![tp("T"), tp("E")],
            },
            false,
        ));
    }

    /// Tier 2 (HEAD) — the middle tie-break tier. Narrower than tier 3 on the
    /// type-param-param disjunct (structural `heads_unifiable` instead of a
    /// full wildcard), but NOT a subset of it; see
    /// [`slot_matches_head_tier_is_not_a_subset_of_the_wildcard_tier`].
    #[test]
    fn slot_matches_head_tier_covers_each_disjunct() {
        // Trait-object param: ungated on genericity, same as tier 3.
        assert!(super::slot_matches_head_tier(
            &Type::TraitObject("Load".to_string()),
            &Type::StructureRef("PointLoad".to_string()),
            false,
        ));

        // `heads_unifiable`, gated on is_generic: this is the tier's whole
        // point — discriminating two GENERIC container overloads.
        let option_t = Type::Option(Box::new(tp("T")));
        let result_len_str = Type::Applied {
            name: "Result".to_string(),
            args: vec![Type::length(), Type::String],
        };
        assert!(
            !super::slot_matches_head_tier(&option_t, &result_len_str, true),
            "an Option<T> param must NOT head-match a Result<..> arg — this is \
             the #5685 mis-selection the head tier exists to prevent"
        );
        assert!(super::slot_matches_head_tier(
            &option_t,
            &Type::Option(Box::new(Type::Int)),
            true,
        ));
        assert!(
            !super::slot_matches_head_tier(
                &option_t,
                &Type::Option(Box::new(Type::Int)),
                false,
            ),
            "the heads_unifiable disjunct is gated on is_generic"
        );

        // The `type_carries_dim_param` FULL-wildcard carve-out survives INSIDE
        // the head tier: dimension-param overload resolution is orthogonal to
        // enum-head disambiguation, so it is deliberately NOT narrowed to
        // `heads_unifiable`.
        assert!(
            !super::heads_unifiable(&sp("Q"), &Type::Int),
            "precondition: heads_unifiable rejects ScalarParam vs a non-Scalar"
        );
        assert!(
            super::slot_matches_head_tier(&sp("Q"), &Type::Int, true),
            "the dim-param param disjunct stays a FULL wildcard in the head \
             tier, even where heads_unifiable says false"
        );

        // The BARE-`TypeParam` ARG disjunct: ungated, matches any param.
        assert!(super::slot_matches_head_tier(&Type::Int, &tp("T"), false));
        assert!(super::slot_matches_head_tier(&option_t, &tp("T"), false));
        // ...but a HEADED arg carrying a NESTED type-param is NOT a wildcard
        // here (task #4038 δ) — it has a real head, so heads_unifiable
        // discriminates it.
        assert!(!super::slot_matches_head_tier(
            &Type::Int,
            &Type::Applied {
                name: "Result".to_string(),
                args: vec![tp("T"), tp("E")],
            },
            false,
        ));

        // Plain equality, and its negation.
        assert!(super::slot_matches_head_tier(&Type::Int, &Type::Int, false));
        assert!(!super::slot_matches_head_tier(
            &Type::Int,
            &Type::String,
            false
        ));
    }

    /// THE SCREENING COUNTEREXAMPLE — why every caller MUST apply tier 2 as a
    /// FILTER over tier 3's surviving set, never standalone.
    ///
    /// It is tempting to read "tier 2 is narrower than tier 3" as "tier 2 ⊆
    /// tier 3", and therefore to run the head pass independently. It is not a
    /// subset: `heads_unifiable`'s erased-subject arm (`Applied{name, ..}` vs
    /// `Enum(name)`) accepts a pair that tier 3 rejects outright, because tier
    /// 3 only ever compares an `Applied` param to an `Enum` arg by plain
    /// equality.
    ///
    /// A standalone head pass would therefore WIDEN resolution rather than
    /// narrow it, admitting candidates the wildcard tier had already excluded.
    /// Until #5689 this contract lived only as prose in
    /// `reify_expr::find_matching_compiled_function`; this test is the
    /// executable form.
    #[test]
    fn slot_matches_head_tier_is_not_a_subset_of_the_wildcard_tier() {
        let param = Type::Applied {
            name: "Result".to_string(),
            args: vec![Type::Int, Type::String],
        };
        let arg = Type::Enum("Result".to_string());

        assert!(
            super::slot_matches_head_tier(&param, &arg, true),
            "heads_unifiable's erased-subject arm accepts Applied{{Result}} vs \
             Enum(Result)"
        );
        assert!(
            !super::slot_matches_wildcard_tier(&param, &arg, true),
            "...but the wildcard tier does NOT: no disjunct fires (the param's \
             args are concrete, so neither type_carries_* is true), and \
             Applied != Enum. Head is NOT a subset of wildcard, so a \
             standalone head pass would WIDEN resolution — every caller must \
             screen tier 2 through tier 3."
        );

        // The non-subset relation is CONFINED to generic candidates, and that
        // is worth pinning too: with `is_generic == false` the head tier
        // collapses to `tcto(param) || matches!(arg, TypeParam(_)) || param ==
        // arg`, and since a bare `TypeParam` arg also satisfies the wildcard
        // tier's `type_carries_type_param(arg)` disjunct, head genuinely IS a
        // subset there. So the screening requirement is not merely defensive
        // hygiene — it is load-bearing exactly for the generic overloads the
        // head tier was introduced to disambiguate.
        assert!(!super::slot_matches_head_tier(&param, &arg, false));
        assert!(!super::slot_matches_wildcard_tier(&param, &arg, false));
    }
}
