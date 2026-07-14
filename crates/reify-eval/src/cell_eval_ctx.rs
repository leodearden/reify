//! Required-args `cell_eval_ctx` free-function constructor (INV-EVAL-2).
//!
//! `reify_expr::EvalContext` is an optional-capability builder: omitting
//! `.with_determinacy` / `.with_runtime_diagnostics` / `.with_containment`
//! silently degrades evaluation rather than failing to compile. This
//! module's `cell_eval_ctx` makes those three capabilities REQUIRED (plain
//! `&'a T`, not `Option`), so omitting one at a call site is a compile
//! error instead.
//!
//! See `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.5, §8.

use std::cell::RefCell;
use std::collections::HashMap;

use reify_core::{Diagnostic, ValueCellId};
use reify_expr::{ContainmentQuery, EvalContext};
use reify_ir::{CompiledFunction, DeterminacyState, PersistentMap, Value, ValueMap};

/// Required-args free-function constructor for cell-eval contexts
/// (INV-EVAL-2). `determinacy` / `runtime_sink` / `containment` are
/// REQUIRED (plain `&'a T`, not `Option`); omitting one is a compile error
/// (E0061).
///
/// Transitional note: this coexists with the pre-existing
/// `Engine::cell_eval_ctx` *method* (`engine_eval.rs`) — same name,
/// different namespace (`self.cell_eval_ctx(..)` vs. this free function's
/// `crate::cell_eval_ctx::cell_eval_ctx(..)`). The method remains the
/// incumbent at today's production call sites; this free function is the
/// constructor γ/δ/ε migrate those call sites onto, superseding the method
/// once that adoption lands. It is not yet "the only" constructor in use —
/// pick deliberately at call sites until the method is retired.
///
/// TODO(#5053, #5056, #5065): closing this coexistence needs the
/// method's own call sites gone. Today those are in `engine_eval.rs`
/// (γ) and `engine_edit.rs` (δ), plus this crate's dead-code
/// `concurrent.rs` module, which ο deletes outright rather than
/// migrating. Retire `Engine::cell_eval_ctx` once all three land —
/// re-check for stragglers first. (ε / #5057 targets `unfold.rs`, which
/// doesn't call the method today, so it isn't a precondition here.)
///
/// Until then, this body duplicates the method's capability-wiring chain
/// verbatim (deliberate transitional coexistence, not drift) — no test in
/// either module would catch the two diverging, so keep them in lockstep
/// by hand. Retire by making the method delegate to (or be replaced by)
/// this free function, so the crate ends up with one implementation of the
/// wiring chain instead of two independent copies.
///
/// Lifts `functions` / `meta_map` / `containment` out of `&self` into
/// explicit params. `undef_causes` stays unset — it is wired separately by
/// `record_op_contract_failures`, not a cell-eval-ctx capability.
///
/// Does not migrate existing call sites (γ/δ/ε own adoption in
/// `engine_eval.rs` / `engine_edit.rs` / `unfold.rs`); each must
/// re-validate this signature against its own call site's borrow shape.
//
// TODO(#5053, #5056, #5057): only caller today is the test module below
// (built under `#[cfg(test)]`, so it doesn't count for a normal build) —
// a real caller lands once tasks γ/δ/ε adopt this constructor at their
// respective call sites. Drop this `allow` once any of them lands.
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
    use std::sync::Arc;

    use reify_core::{ContentHash, Diagnostic, DiagnosticCode, Type, ValueCellId};
    use reify_expr::{ContainmentQuery, EvalContext, eval_expr};
    use reify_ir::{
        CompiledExpr, CompiledExprKind, CompiledFunction, DeterminacyPredicateKind,
        DeterminacyState, FieldSourceKind, PersistentMap, ResolvedFunction, Value, ValueMap,
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

    /// A `ContainmentQuery` that reports every point as inside the region —
    /// the complement of `NoContainment` above, used to drive the
    /// `sample(restrict(field, region), point)` dispatch arm down its
    /// "inside" branch so a behavioral test can observe the wired
    /// `containment` capability actually taking effect.
    struct AlwaysInside;

    impl ContainmentQuery for AlwaysInside {
        fn contains(&self, _region: &Value, _point: &Value) -> Option<bool> {
            Some(true)
        }
    }

    /// Return type of [`empty_inputs`] — named so the tuple's shape doesn't
    /// trip `clippy::type_complexity`; unlike the fn-pointer guard below,
    /// nothing here depends on the type staying spelled out literally.
    type EmptyCtxInputs = (
        ValueMap,
        HashMap<String, HashMap<String, String>>,
        PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        RefCell<Vec<Diagnostic>>,
    );

    /// Shared empty fixtures for the tests below: an empty `ValueMap`, meta
    /// map, determinacy map, and diagnostics sink. Only two axes actually
    /// vary between tests — determinacy-map contents and the `containment`
    /// impl — so callers destructure this tuple and then override just the
    /// field they exercise (e.g. `.insert(..)` into the returned
    /// `determinacy`, or construct `AlwaysInside` instead of
    /// `NoContainment`) rather than each re-declaring all four empty
    /// fixtures inline.
    fn empty_inputs() -> EmptyCtxInputs {
        (
            ValueMap::new(),
            HashMap::new(),
            PersistentMap::new(),
            RefCell::new(Vec::new()),
        )
    }

    /// Type-level regression guard for INV-EVAL-2: pins `cell_eval_ctx`'s
    /// all-required signature (`determinacy`/`runtime_sink`/`containment` as
    /// plain `&'a T`, never `Option`), so an edit that Option-ifies any of
    /// them — or drops a param — fails to compile. `pub(crate)` blocks
    /// rustdoc `compile_fail` doctests (`pub`-only) and the workspace has no
    /// trybuild, so this fn-pointer coercion is the in-crate enforcement
    /// mechanism; unlike the call-site test below, it stays enforced even if
    /// that test is later deleted or refactored.
    ///
    /// Note: an intentional signature change to `cell_eval_ctx` must update
    /// this const's type to match, or the crate stops compiling — that's
    /// the guard doing its job, not a bug.
    ///
    /// `clippy::type_complexity` is allowed locally: the fully-spelled-out
    /// `for<'a> fn(..)` type IS the guard — hiding it behind a `type` alias
    /// would defeat the point of pinning the exact signature.
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
        let (values, meta_map, determinacy, sink) = empty_inputs();
        let functions: &[CompiledFunction] = &[];
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
        assert!(
            ctx.determinacy
                .is_some_and(|d| std::ptr::eq(d, &determinacy))
        );
        assert!(ctx.diagnostics.is_some_and(|d| std::ptr::eq(d, &sink)));
        assert!(
            ctx.containment
                .is_some_and(|c| std::ptr::eq(c, containment_ref))
        );
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

        let (values, meta_map, mut determinacy, sink) = empty_inputs();
        let functions: &[CompiledFunction] = &[];
        determinacy.insert(
            cell_id.clone(),
            (Value::Real(2.5), DeterminacyState::Determined),
        );
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

    /// Behavioral complement to the wiring test above: proves the
    /// `containment` capability threaded through `cell_eval_ctx` is
    /// actually *effective* during evaluation, not just present as a
    /// `Some` field.
    ///
    /// Evaluates `sample(restrict(inner, region), point)` through the
    /// returned context. Per the `Restricted` arm in
    /// `reify_expr`'s field-sample dispatch, this resolves to the inner
    /// field's value only when `ctx.containment.and_then(|c| c.contains(..))`
    /// is `Some(true)` (as here, via `AlwaysInside`) and would instead
    /// silently degrade to `Value::Undef` if `ctx.containment` were `None`
    /// — e.g. if a future edit dropped the `.with_containment(..)` link
    /// from `cell_eval_ctx`'s body. Mirrors the mock-resolver construction
    /// in `reify-expr/tests/field_op_dispatch_tests.rs`, but exercised
    /// through `cell_eval_ctx` itself rather than
    /// `EvalContext::simple(..).with_containment(..)`.
    #[test]
    fn cell_eval_ctx_containment_resolves_via_wired_query() {
        let x_id = ValueCellId::new("$lambda_inner.S", "x");
        let inner_field = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Lambda {
                params: vec![("x".to_string(), x_id)],
                body: Box::new(CompiledExpr::literal(
                    Value::Real(42.0),
                    Type::dimensionless_scalar(),
                )),
                captures: ValueMap::new(),
            }),
        };
        // Sentinel — NOT Undef (eval_expr's strict-Undef short-circuit would
        // otherwise fire before the Restricted dispatch arm runs).
        // AlwaysInside ignores the actual region value.
        let region = Value::Bool(false);
        let restricted = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Restricted,
            lambda: Arc::new(Value::List(vec![inner_field, region])),
        };
        let field_type = Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        };

        let (values, meta_map, determinacy, sink) = empty_inputs();
        let functions: &[CompiledFunction] = &[];
        let containment = AlwaysInside;

        let ctx = cell_eval_ctx(
            &values,
            functions,
            &meta_map,
            &determinacy,
            &sink,
            &containment,
        );

        let sample_expr = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "sample".to_string(),
                    qualified_name: "std::sample".to_string(),
                },
                args: vec![
                    CompiledExpr::literal(restricted, field_type),
                    CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar()),
                ],
            },
            result_type: Type::dimensionless_scalar(),
            content_hash: ContentHash::of(b"cell_eval_ctx_containment_test"),
        };

        assert_eq!(
            eval_expr(&sample_expr, &ctx),
            Value::Real(42.0),
            "sample(restrict(inner, region), pt) should resolve to the inner field's value \
             via the containment query threaded through cell_eval_ctx, not silently degrade \
             to Value::Undef"
        );
    }

    /// Behavioral complement to the wiring test above: proves the
    /// `runtime_sink` capability threaded through `cell_eval_ctx` is
    /// actually *effective* during evaluation, not just present as a
    /// `Some` field.
    ///
    /// Evaluates `from_samples(points, values, method)` with a non-`List`
    /// `points` argument through the returned context. `from_samples`'s
    /// `FunctionCall` dispatch gate is arity-only (3 args); the `List`-shape
    /// check — and the diagnostic push — happens inside `eval_from_samples`
    /// itself, so a malformed `points` arg pushes a
    /// `DiagnosticCode::FieldSamplesNotGrid` diagnostic into `ctx.diagnostics`
    /// (when a sink is attached, as here) and would instead silently drop
    /// the warning if `ctx.diagnostics` were `None` — e.g. if a future edit
    /// dropped the `.with_runtime_diagnostics(..)` link from
    /// `cell_eval_ctx`'s body. The pointer-identity assertion above would
    /// not catch that class of regression; this test does.
    #[test]
    fn cell_eval_ctx_runtime_sink_receives_diagnostics_during_eval() {
        let (values, meta_map, determinacy, sink) = empty_inputs();
        let functions: &[CompiledFunction] = &[];
        let containment = NoContainment;

        let ctx = cell_eval_ctx(
            &values,
            functions,
            &meta_map,
            &determinacy,
            &sink,
            &containment,
        );

        // Sentinel non-List, non-Undef args: the malformed-shape check (and
        // the diagnostic push) is on `points` specifically, so `values` /
        // `method` just need to avoid tripping the top-level strict-Undef
        // short-circuit.
        let malformed_points = Value::Bool(true);
        let placeholder = Value::Bool(true);
        let from_samples_expr = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "from_samples".to_string(),
                    qualified_name: "std::from_samples".to_string(),
                },
                args: vec![
                    CompiledExpr::literal(malformed_points, Type::dimensionless_scalar()),
                    CompiledExpr::literal(placeholder.clone(), Type::dimensionless_scalar()),
                    CompiledExpr::literal(placeholder, Type::dimensionless_scalar()),
                ],
            },
            result_type: Type::dimensionless_scalar(),
            content_hash: ContentHash::of(b"cell_eval_ctx_runtime_sink_test"),
        };

        let result = eval_expr(&from_samples_expr, &ctx);
        assert_eq!(result, Value::Undef);

        let recorded = sink.borrow();
        assert_eq!(
            recorded.len(),
            1,
            "from_samples with a non-List points argument should push exactly one diagnostic \
             into the sink threaded through cell_eval_ctx, got {:?}",
            *recorded
        );
        assert_eq!(recorded[0].code, Some(DiagnosticCode::FieldSamplesNotGrid));
    }

    // NOTE: no `cell_eval_ctx_meta_resolves_via_wired_map` behavioral test
    // here (deliberately, per review — test_duplication). `cell_eval_ctx`'s
    // meta wiring is entirely delegated to `eval_ctx_with_meta`, whose own
    // `eval_ctx_with_meta_resolves_meta_access` test (lib.rs) already proves
    // `MetaAccess` resolves via a meta map threaded that way; the
    // pointer-identity assertion on `ctx.meta` in
    // `cell_eval_ctx_wires_all_required_capabilities` above already proves
    // `cell_eval_ctx` forwards the caller's `meta_map` unchanged into that
    // same delegate. Together those two existing tests transitively cover
    // "meta resolves via the map threaded through `cell_eval_ctx`" with no
    // gap, so a third, purely-redundant behavioral re-test was trimmed
    // rather than kept for symmetry with the determinacy/containment/
    // runtime_sink tests (which — unlike meta — have no pre-existing
    // behavioral coverage elsewhere and so are not redundant).
}
