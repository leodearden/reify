use super::*;

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

        // All remaining leaves carry no inner `Type`.
        Type::Bool
        | Type::Int
        | Type::String
        | Type::Scalar { .. }
        | Type::Enum(_)
        | Type::StructureRef(_)
        | Type::TraitObject(_)
        | Type::Geometry
        | Type::Orientation(_)
        | Type::Frame(_)
        | Type::Transform(_)
        | Type::AffineMap(_)
        | Type::Plane
        | Type::Axis
        | Type::BoundingBox
        | Type::Selector(_)
        | Type::AnySelector
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
pub(crate) fn unify(
    declared: &Type,
    arg: &Type,
    subst: &mut HashMap<String, Type>,
) -> Result<(), TypeArgConflict> {
    match (declared, arg) {
        // Type-parameter leaf: bind if absent; re-bind to the same type is Ok;
        // re-bind to a different type is the sole error case.
        (Type::TypeParam(p), _) => match subst.get(p) {
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
        | (Type::Union(_), _) => Ok(()),

        // True leaves (no inner `Type` to bind):
        (Type::Bool, _)
        | (Type::Int, _)
        | (Type::String, _)
        | (Type::Scalar { .. }, _)
        | (Type::Enum(_), _)
        | (Type::StructureRef(_), _)
        | (Type::TraitObject(_), _)
        | (Type::Geometry, _)
        | (Type::Orientation(_), _)
        | (Type::Frame(_), _)
        | (Type::Transform(_), _)
        | (Type::AffineMap(_), _)
        | (Type::Plane, _)
        | (Type::Axis, _)
        | (Type::BoundingBox, _)
        | (Type::Selector(_), _)
        | (Type::AnySelector, _)
        | (Type::Error, _) => Ok(()),
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
                            || (is_generic && type_carries_type_param(param_ty))
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

    let resolved = if exact_matches.is_empty() {
        matches
    } else {
        exact_matches
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

/// Parse a string unary operator into a `UnOp`.
pub(crate) fn resolve_unop(op: &str) -> Option<UnOp> {
    match op {
        "-" => Some(UnOp::Neg),
        "!" | "not" => Some(UnOp::Not),
        _ => None,
    }
}

// --- Type inference for binary operations ---

/// Infer the result type of a binary operation given operand types.
pub(crate) fn infer_binop_type(op: BinOp, left: &Type, right: &Type) -> Type {
    // Anti-cascade guard (task-448): if either operand is already poisoned,
    // propagate Type::Error so downstream sites don't emit follow-on diagnostics.
    if left.is_error() || right.is_error() {
        return Type::Error;
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
        BinOp::Add | BinOp::Sub => left.clone(), // same dimension required
        BinOp::Mul => match (left, right) {
            (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd }) => Type::Scalar {
                dimension: ld.mul(rd),
            },
            (Type::Scalar { .. }, _) | (_, Type::Scalar { .. }) => {
                // Scalar * non-scalar preserves the scalar type
                if let Type::Scalar { .. } = left {
                    left.clone()
                } else {
                    right.clone()
                }
            }
            _ => Type::Int,
        },
        BinOp::Div => match (left, right) {
            (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd }) => {
                Type::Scalar { dimension: ld.div(rd) }
            }
            (Type::Scalar { .. }, _) => left.clone(),
            _ => Type::Int,
        },
        BinOp::Mod => left.clone(),
        BinOp::Pow => left.clone(), // simplified for M1
    }
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
/// prefers the subset whose prefix matches by strict `param_ty == arg_ty` (mirrors
/// `resolve_function_overload`'s exact-match tie-break); if that subset has exactly
/// one entry it is returned, otherwise `None`. Returns `None` when zero candidates
/// are satisfiable (caller falls through to the existing NoMatch error).
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
                    || (is_generic && type_carries_type_param(param_ty))
                    || type_carries_type_param(arg_ty)
                    || param_ty == arg_ty
            });
        if !prefix_matches {
            continue;
        }
        // All trailing params must carry Some compiled default.
        let defaults: Option<Vec<CompiledExpr>> = cand.param_defaults[provided..]
            .iter()
            .cloned()
            .collect();
        if let Some(defaults) = defaults {
            satisfiable.push((cand, defaults));
        }
    }

    match satisfiable.len() {
        1 => Some(satisfiable.into_iter().next().unwrap()),
        0 => None,
        _ => {
            // Multiple candidates pass the wildcard prefix — prefer the subset
            // whose prefix matches by strict equality (mirrors
            // `resolve_function_overload`'s exact-match tie-break).
            //
            // When the exact subset is empty (all wildcard) or has more than
            // one entry (two exact matches), we return None and let the caller
            // fall through to its generic NoMatch error.  This is an
            // intentional UX trade-off: a genuinely ambiguous defaultable call
            // surfaces "no matching overload" rather than a dedicated Ambiguous
            // diagnostic.  Defaultable-overload ambiguity is rare in practice;
            // if a clearer user-facing diagnostic is ever warranted, this arm
            // could surface an Ambiguous result before returning None.
            let exact: Vec<_> = satisfiable
                .into_iter()
                .filter(|(cand, _)| {
                    cand.params[..provided]
                        .iter()
                        .zip(arg_types[..provided].iter())
                        .all(|((_, param_ty), arg_ty)| param_ty == arg_ty)
                })
                .collect();
            match exact.len() {
                1 => Some(exact.into_iter().next().unwrap()),
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
        let fns = vec![make_fn("f", vec![("j", Type::TraitObject("DrivingJoint".to_string()))])];
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
        let fns = vec![make_fn("f", vec![("j", Type::TraitObject("DrivingJoint".to_string()))])];
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
        let result = resolve_function_overload(
            "g",
            &[Type::StructureRef("X".to_string()), Type::Int],
            &fns,
        );
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
        let d =
            format_dimension_mismatch_diagnostic("addition", &Type::dimensionless_scalar(), &force_ty(), test_span());
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));

        // Left Scalar, right non-Scalar
        let d =
            format_dimension_mismatch_diagnostic("addition", &money_ty(), &Type::dimensionless_scalar(), test_span());
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::DimensionMismatch));

        // Both non-Scalar
        let d =
            format_dimension_mismatch_diagnostic("addition", &Type::dimensionless_scalar(), &Type::dimensionless_scalar(), test_span());
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
        assert!(!modulo_operands_are_int(&Type::dimensionless_scalar(), &Type::Int));
    }

    /// `(Int, Real)` is rejected (right is Real) → `false`.
    #[test]
    fn modulo_operands_int_real_is_false() {
        assert!(!modulo_operands_are_int(&Type::Int, &Type::dimensionless_scalar()));
    }

    /// `(Real, Real)` — both wrong → `false`.
    #[test]
    fn modulo_operands_real_real_is_false() {
        assert!(!modulo_operands_are_int(&Type::dimensionless_scalar(), &Type::dimensionless_scalar()));
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
            !type_compatible(
                &Type::Selector(SelectorKind::Face),
                &Type::AnySelector
            ),
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
        assert!(type_carries_type_param(&Type::Union(vec![Type::Int, tp("T")])));
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
        let default_expr =
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
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
        let result = try_default_padding(
            &[&cand],
            &[Type::StructureRef("X".to_string())],
        );
        let (matched_fn, defaults) = result.expect(
            "trait-carrying leading param must act as a wildcard: expected Some, got None",
        );
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
        let default_a =
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
        let default_b =
            CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
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

        let result = try_default_padding(
            &[&cand_a, &cand_b],
            &[Type::StructureRef("X".to_string())],
        );
        let (matched_fn, defaults) = result.expect(
            "exact-match tie-break must resolve to candidate B; expected Some, got None",
        );
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
        let default_expr =
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
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
        let result = try_default_padding(
            &[&cand],
            &[Type::dimensionless_scalar()],
        );
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
        let default_a =
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
        let default_b =
            CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
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
        let result = try_default_padding(
            &[&cand_a, &cand_b],
            &[Type::StructureRef("X".to_string())],
        );
        assert!(
            result.is_none(),
            "two wildcard-only candidates must return None (ambiguous padding \
             degrades to NoMatch — see multi-candidate arm of try_default_padding)"
        );
    }

    // ── is_syntactic_zero_literal predicate (task-4485/β) ────────────────────

    /// Helper: build a bare AST `Expr` with a dummy span for unit-testing predicates.
    fn make_ast_expr(kind: reify_ast::ExprKind) -> reify_ast::Expr {
        reify_ast::Expr { kind, span: SourceSpan::new(0, 1) }
    }

    /// `NumberLiteral{value:0.0, is_real:false}` — the bare `0` integer form — must
    /// return `true`.
    #[test]
    fn syntactic_zero_int_literal_zero_is_true() {
        let expr = make_ast_expr(reify_ast::ExprKind::NumberLiteral { value: 0.0, is_real: false });
        assert!(is_syntactic_zero_literal(&expr));
    }

    /// `NumberLiteral{value:0.0, is_real:true}` — the `0.0` real form — must
    /// return `true`.
    #[test]
    fn syntactic_zero_real_literal_zero_is_true() {
        let expr = make_ast_expr(reify_ast::ExprKind::NumberLiteral { value: 0.0, is_real: true });
        assert!(is_syntactic_zero_literal(&expr));
    }

    /// `UnOp{op:"-", operand: NumberLiteral{0.0}}` — the `-0` form — must
    /// return `true` (unary-neg recursion).
    #[test]
    fn syntactic_zero_neg_zero_is_true() {
        let inner = make_ast_expr(reify_ast::ExprKind::NumberLiteral { value: 0.0, is_real: false });
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
        let inner = make_ast_expr(reify_ast::ExprKind::NumberLiteral { value: 0.0, is_real: true });
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
        let expr =
            make_ast_expr(reify_ast::ExprKind::NumberLiteral { value: 1.0, is_real: false });
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
        let one_a =
            make_ast_expr(reify_ast::ExprKind::NumberLiteral { value: 1.0, is_real: false });
        let one_b =
            make_ast_expr(reify_ast::ExprKind::NumberLiteral { value: 1.0, is_real: false });
        let expr = make_ast_expr(reify_ast::ExprKind::BinOp {
            op: "-".to_string(),
            left: Box::new(one_a),
            right: Box::new(one_b),
        });
        assert!(!is_syntactic_zero_literal(&expr));
    }
}
