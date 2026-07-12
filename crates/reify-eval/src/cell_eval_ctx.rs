//! Required-args `cell_eval_ctx` free-function constructor (INV-EVAL-2).
//!
//! PRD: `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.5, §8 — task β of
//! the eval cell-commit substrate.
//!
//! `reify_expr::EvalContext` is an optional-capability builder: omitting
//! `.with_determinacy` / `.with_runtime_diagnostics` / `.with_containment`
//! does not fail to compile — it silently degrades evaluation instead
//! (`DeterminacyPredicate` cells resolve to `Value::Undef`, runtime warnings
//! are dropped, and `sample(restrict(field, region), point)` is forced to
//! `Value::Undef`). This module's `cell_eval_ctx` free function makes those
//! three capabilities REQUIRED parameters (plain `&'a T`, not `Option`), so
//! omitting a load-bearing capability at a call site is a compile error
//! rather than a silent behavior change.

use std::cell::RefCell;
use std::collections::HashMap;

use reify_core::{Diagnostic, ValueCellId};
use reify_expr::{ContainmentQuery, EvalContext};
use reify_ir::{CompiledFunction, DeterminacyState, PersistentMap, Value, ValueMap};

/// The only sanctioned in-engine `cell_eval_ctx` constructor (INV-EVAL-2).
///
/// `determinacy`, `runtime_sink`, and `containment` are REQUIRED — plain
/// `&'a T`, not `Option` — so no builder path can omit a load-bearing
/// capability; leaving one out is a compile error (E0061), not a silent
/// behavior change.
///
/// Lifts `functions`, `meta_map`, and `containment` out of `&self` into
/// explicit params, which dissolves the borrow-scope excuse recorded on
/// `Engine::cell_eval_ctx`'s doc comment for building `EvalContext` inline
/// at some call sites.
///
/// `undef_causes` is intentionally left unset (`None`): it is not a
/// cell-eval-ctx capability — the op/builtin contract-failure sink is
/// attached separately by `record_op_contract_failures` during the
/// post-eval re-evaluation pass.
///
/// This constructor does not migrate any existing call site (γ/δ/ε own
/// adoption in `engine_eval.rs` / `engine_edit.rs` / `unfold.rs`).
//
// `allow(dead_code)`: in this task (P1 β, #5039) the only caller is the
// golden unit test below; the production callers land when γ/δ/ε migrate
// `engine_eval.rs` / `engine_edit.rs` / `unfold.rs` onto this constructor.
// See docs/prds/v0_6/eval-cell-commit-substrate.md §2.5, §8.
#[allow(dead_code)]
pub(crate) fn cell_eval_ctx<'a>(
    values: &'a ValueMap,
    functions: &'a [CompiledFunction],
    meta_map: &'a HashMap<String, HashMap<String, String>>,
    determinacy: &'a PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    runtime_sink: &'a RefCell<Vec<Diagnostic>>,
    containment: &'a dyn ContainmentQuery,
) -> EvalContext<'a> {
    crate::eval_ctx_with_meta(values, functions, meta_map)
        .with_determinacy(determinacy)
        .with_runtime_diagnostics(runtime_sink)
        .with_containment(containment)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use reify_core::{Diagnostic, ValueCellId};
    use reify_expr::{ContainmentQuery, EvalContext, eval_expr};
    use reify_ir::{
        CompiledExpr, CompiledFunction, DeterminacyPredicateKind, DeterminacyState, PersistentMap,
        Value, ValueMap,
    };

    use super::cell_eval_ctx;

    /// Trivial `ContainmentQuery` impl so the test doesn't need a full
    /// `Engine`/geometry-kernel to exercise the constructor.
    struct NoContainment;

    impl ContainmentQuery for NoContainment {
        fn contains(&self, _region: &Value, _point: &Value) -> Option<bool> {
            None
        }
    }

    /// Type-level regression guard for INV-EVAL-2: `cell_eval_ctx` must keep
    /// this exact all-required signature — `determinacy`, `runtime_sink`,
    /// and `containment` are plain `&'a T`, never `Option`. If a future edit
    /// makes any of them optional (or drops a parameter), this `const`
    /// binding fails to compile. `pub(crate)` blocks rustdoc `compile_fail`
    /// doctests (they only run on `pub` items) and the workspace has no
    /// trybuild, so this fn-pointer coercion is the in-crate enforcement
    /// mechanism.
    ///
    /// `clippy::type_complexity`: the fully-spelled-out `for<'a> fn(..)` type
    /// IS the regression guard — hiding it behind a `type` alias (clippy's
    /// suggested fix) would defeat the point of pinning the exact signature
    /// here, so the lint is allowed locally rather than refactored away.
    ///
    /// Relation to the test below: `cell_eval_ctx_wires_all_required_capabilities`'s
    /// direct call to `cell_eval_ctx(..)` already pins call-site arity and
    /// argument types too (a dropped/reordered/mistyped param fails that
    /// call to compile) — the two are not unrelated coverage. This const's
    /// own marginal catch is a required `&'a T` silently becoming
    /// `Option<&'a T>`, which stays enforced even if the test is later
    /// deleted or refactored.
    #[allow(dead_code, clippy::type_complexity)]
    const _CELL_EVAL_CTX_REQUIRES_ALL_CAPS: for<'a> fn(
        &'a ValueMap,
        &'a [CompiledFunction],
        &'a HashMap<String, HashMap<String, String>>,
        &'a PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        &'a RefCell<Vec<Diagnostic>>,
        &'a dyn ContainmentQuery,
    ) -> EvalContext<'a> = cell_eval_ctx;

    #[test]
    fn cell_eval_ctx_wires_all_required_capabilities() {
        let values = ValueMap::new();
        let functions: &[CompiledFunction] = &[];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let determinacy: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let containment = NoContainment;
        let containment_ref: &dyn ContainmentQuery = &containment;

        let ctx = cell_eval_ctx(
            &values,
            functions,
            &meta_map,
            &determinacy,
            &sink,
            containment_ref,
        );

        // Pointer-identity, not just `.is_some()`: proves `cell_eval_ctx`
        // threads the caller's own references through unchanged, rather than
        // silently substituting a different (e.g. default/empty) instance
        // for one capability while leaving its `Option` `Some`.
        assert!(ctx.determinacy.is_some_and(|d| std::ptr::eq(d, &determinacy)));
        assert!(ctx.diagnostics.is_some_and(|d| std::ptr::eq(d, &sink)));
        assert!(ctx.containment.is_some_and(|c| std::ptr::eq(c, containment_ref)));
        assert!(ctx.meta.is_some_and(|m| std::ptr::eq(m, &meta_map)));
        // Locks the doc-commented "intentionally unset" contract: undef_causes
        // is not a cell-eval-ctx capability (it's wired separately by
        // `record_op_contract_failures`), so a future edit that accidentally
        // threads an undef-cause sink through this constructor should fail
        // this assertion rather than pass unnoticed.
        assert!(ctx.undef_causes.is_none());
    }

    /// Behavioral complement to the wiring test above: proves the
    /// `determinacy` capability threaded through `cell_eval_ctx` is actually
    /// *effective* during evaluation, not just present as a `Some` field.
    ///
    /// Evaluates a real `DeterminacyPredicate` expression through the
    /// returned context. Per the `DeterminacyPredicate` arm in
    /// `reify_expr::eval_expr`, this resolves via the wired snapshot map
    /// when `ctx.determinacy` is `Some` (as here) and would instead
    /// silently degrade to `Value::Undef` if `ctx.determinacy` were `None`
    /// — e.g. if a future edit dropped the `.with_determinacy(..)` link
    /// from `cell_eval_ctx`'s body. The pointer-identity assertions above
    /// would not catch that class of regression; this test does.
    #[test]
    fn cell_eval_ctx_determinacy_resolves_via_wired_map() {
        let cell_id = ValueCellId::new("S", "a");

        let values = ValueMap::new();
        let functions: &[CompiledFunction] = &[];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut determinacy: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        determinacy.insert(
            cell_id.clone(),
            (Value::Real(2.5), DeterminacyState::Determined),
        );
        let sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());
        let containment = NoContainment;

        let ctx = cell_eval_ctx(
            &values,
            functions,
            &meta_map,
            &determinacy,
            &sink,
            &containment,
        );

        let det_expr =
            CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, cell_id);

        assert_eq!(
            eval_expr(&det_expr, &ctx),
            Value::Bool(true),
            "determined(a) should resolve true via the determinacy map threaded through cell_eval_ctx, \
             not silently degrade to Value::Undef"
        );
    }
}
