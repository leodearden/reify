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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use reify_core::{Diagnostic, ValueCellId};
    use reify_expr::{ContainmentQuery, EvalContext};
    use reify_ir::{CompiledFunction, DeterminacyState, PersistentMap, Value, ValueMap};

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
    #[allow(dead_code)]
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

        let ctx = cell_eval_ctx(
            &values,
            functions,
            &meta_map,
            &determinacy,
            &sink,
            &NoContainment,
        );

        assert!(ctx.determinacy.is_some());
        assert!(ctx.diagnostics.is_some());
        assert!(ctx.containment.is_some());
        assert!(ctx.meta.is_some());
    }
}
