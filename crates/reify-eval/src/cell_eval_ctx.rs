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
/// `Engine::cell_eval_ctx`'s doc comment (`engine_eval.rs:4875-4894`) for
/// building `EvalContext` inline at some call sites.
///
/// `undef_causes` is intentionally left unset (`None`): it is not a
/// cell-eval-ctx capability — the op/builtin contract-failure sink is
/// attached separately by `record_op_contract_failures` during the
/// post-eval re-evaluation pass.
///
/// This constructor does not migrate any existing call site (γ/δ/ε own
/// adoption in `engine_eval.rs` / `engine_edit.rs` / `unfold.rs`).
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
