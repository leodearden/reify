//! Selector → `List<Geometry>` argument coercion (task 4118 γ).
//!
//! Centralizes the `CompiledExprKind::ResolveSelector` insertion shared by the
//! three compiler coercion sites:
//!   1. **param-binding** — `try_selector_coerced_overload` + `coerce_selector_arg`,
//!      called from the `NoMatch` arm of `expr.rs`'s `FunctionCall` path;
//!   2. **`single()`/list-helper** — `list_helpers.rs` reuses `coerce_selector_arg`;
//!   3. **`IndexAccess`-object** — `expr.rs`'s index arm reuses `coerce_selector_arg`.
//!
//! All three sites are gated on the SAME pre-existing β rule
//! [`type_compatible`]`(List<Geometry>, Selector(k))` shipped by task 4117. This
//! module *reads* that rule; it does not extend `type_compat.rs`.
//!
//! **Why a `NoMatch`-arm retry for site #1?** `resolve_function_overload` matches
//! parameters by *exact* type equality, so a `Selector(k)` argument fed to a
//! `List<Geometry>` parameter is an `OverloadResolution::NoMatch` — the primary
//! path never sees the coercion. Rather than relax overload resolution (which
//! lives in the out-of-scope `type_compat.rs`), the coercion is wired as a
//! secondary resolution attempt in the `NoMatch` arm, exactly mirroring the
//! existing `try_default_padding` precedent that already lives there
//! (see esc-4118-61).

use super::*;

/// `true` when `ty` is exactly `List<Geometry>` — the sole parameter shape the
/// selector→list β coercion targets.
pub(crate) fn is_list_geometry(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(inner.as_ref(), Type::Geometry))
}

/// Wrap `arg` in a [`CompiledExpr::resolve_selector`] coercion node when
/// `param_ty` is `List<Geometry>` and `arg`'s result type is a `Selector(k)`.
/// Otherwise return `arg` unchanged.
///
/// The wrap is gated on the β `type_compatible` rule (one-directional
/// `List<Geometry>` ← `Selector(k)`), so it only fires for the ratified
/// coercion. An argument that is already a `List` — or any non-`Selector` — is
/// returned untouched ("do not coerce when arg is already a List").
pub(crate) fn coerce_selector_arg(arg: CompiledExpr, param_ty: &Type) -> CompiledExpr {
    if selector_coerces_to_param(param_ty, &arg.result_type) {
        CompiledExpr::resolve_selector(arg)
    } else {
        arg
    }
}

/// List-helper selector coercion (insertion site #2). When the `single`
/// list-helper receives a `Selector(k)` first argument, wrap it in
/// `ResolveSelector` so the helper sees `List<Geometry>` and
/// [`infer_list_helper_return_type`](crate::list_helpers::infer_list_helper_return_type)
/// collapses `single(List<Geometry>)` → `Geometry` (instead of the first-arg
/// fallback leaving the cell typed `Selector(k)`).
///
/// Scoped to `single` — the ratified γ coercion target (PRD §4.4). `flat_map`'s
/// first argument is also a `List<_>`, but its element type flows into a
/// separately-compiled user lambda, so it is intentionally left out of scope
/// here. Every other name — and any non-`Selector` first argument — passes
/// through untouched, gated on the same β `type_compatible` rule via
/// [`coerce_selector_arg`] ("do not coerce when arg is already a List").
pub(crate) fn coerce_list_helper_args(name: &str, args: Vec<CompiledExpr>) -> Vec<CompiledExpr> {
    if name != "single" {
        return args;
    }
    let list_geometry = Type::List(Box::new(Type::Geometry));
    args.into_iter()
        .enumerate()
        .map(|(i, arg)| {
            if i == 0 {
                coerce_selector_arg(arg, &list_geometry)
            } else {
                arg
            }
        })
        .collect()
}

/// `true` when `arg_ty` is a `Selector(k)` that coerces to the `List<Geometry>`
/// `param_ty` under the β rule.
///
/// Narrow by construction: ONLY the `List<Geometry>` ← `Selector` coercion is
/// admitted — the Int→Real widening and tensor/vector conversions that
/// `type_compatible` also accepts are intentionally NOT relaxed here, preserving
/// `resolve_function_overload`'s exact-match discipline for every non-selector
/// argument.
fn selector_coerces_to_param(param_ty: &Type, arg_ty: &Type) -> bool {
    is_list_geometry(param_ty)
        && matches!(arg_ty, Type::Selector(_))
        && type_compatible(param_ty, arg_ty)
}

/// Secondary overload-resolution attempt for the `NoMatch` case (param-binding
/// coercion, site #1).
///
/// Returns the UNIQUE same-name, same-arity, NON-generic candidate whose every
/// parameter either matches the corresponding argument exactly OR accepts it via
/// the `List<Geometry>` ← `Selector` coercion. Returns `None` — leaving the
/// caller's existing no-match error in place — when zero or multiple candidates
/// qualify, or when no argument is a `Selector` (so ordinary calls are
/// bit-for-bit unchanged).
///
/// Mirrors [`try_default_padding`]: a NoMatch-arm secondary resolver that lives
/// alongside the primary exact-match path. Restricted to non-generic candidates
/// because a generic `List<T>` parameter is already a resolution wildcard in
/// `resolve_function_overload` and would have produced `Resolved` (never
/// reaching this retry); keeping the retry concrete-only avoids perturbing
/// generic type-argument inference.
pub(crate) fn try_selector_coerced_overload<'a>(
    named: &[&'a CompiledFunction],
    arg_types: &[Type],
) -> Option<&'a CompiledFunction> {
    // Guard: only attempt when at least one argument is a Selector. Without a
    // Selector arg this retry can never change the outcome, and skipping it
    // keeps the non-selector no-match path bit-for-bit unchanged.
    if !arg_types.iter().any(|t| matches!(t, Type::Selector(_))) {
        return None;
    }

    let matches: Vec<&CompiledFunction> = named
        .iter()
        .copied()
        .filter(|f| {
            f.type_params.is_empty()
                && f.params.len() == arg_types.len()
                && f.params
                    .iter()
                    .zip(arg_types.iter())
                    .all(|((_, param_ty), arg_ty)| {
                        param_ty == arg_ty || selector_coerces_to_param(param_ty, arg_ty)
                    })
        })
        .collect();

    match matches.len() {
        1 => Some(matches[0]),
        _ => None,
    }
}
