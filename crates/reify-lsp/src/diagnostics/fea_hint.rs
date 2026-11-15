//! The `fea-not-evaluated` Info hint for constraints whose value reaches an
//! `@optimized` compute target the engine has no trampoline for (task 5078,
//! PRD `compute-fea-hardening.md` C2).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl_oracle::test_fixtures::{FEA_BEARING_SRC, test_uri};
    use crate::diagnostics::document_compile::compile_document;
    use reify_compiler::cfg::CfgSet;
    use reify_constraints::SimpleConstraintChecker;
    use reify_core::{ModulePath, VersionId};
    use reify_ir::Satisfaction;

    /// Amendment regression lock (task 5078, PRD `compute-fea-hardening.md`
    /// C2): pins the *other* half of
    /// `constraint_depends_on_unregistered_optimized_compute`'s
    /// discriminator — `engine.compute_dispatch(t).is_none()` — that every
    /// FEA-hint test in `diagnostics.rs` leaves unexercised. Those tests run
    /// the pipeline on `EvalState::new()`, which never registers a compute
    /// trampoline, so `compute_dispatch` returns `None` for every target in
    /// each of them; the "registered target ⇒ not FEA-dependent ⇒ no
    /// hint" branch of the discriminator never runs there.
    ///
    /// Registers a trampoline for `"solver::elastic_static"` that itself
    /// completes with `Value::Undef` — deliberately mirroring the *value*
    /// the unregistered body-inline fallback would have produced, so the
    /// constraint remains genuinely `Satisfaction::Indeterminate` (the fixture
    /// sanity check below confirms this) and the emission loop actually
    /// reaches the discriminator instead of short-circuiting earlier on
    /// `entry.satisfaction != Indeterminate`. Only the *registration status*
    /// differs from the positive control taken on the same engine before
    /// registering — isolating the `compute_dispatch(..).is_none()` check as
    /// the thing this test can catch a regression in.
    ///
    /// Drives [`fea_not_evaluated_hints`] on a hand-built engine: no
    /// registered trampoline is reachable through the public pipeline.
    #[test]
    fn fea_hint_does_not_fire_when_compute_target_is_registered() {
        fn undef_result_fn(
            _value_inputs: &[reify_ir::Value],
            _realization_inputs: &[reify_eval::RealizationReadHandle],
            _options: &reify_ir::Value,
            _prior_warm_state: Option<&reify_ir::OpaqueState>,
            _cancellation: &reify_eval::CancellationHandle,
        ) -> reify_eval::ComputeOutcome {
            reify_eval::ComputeOutcome::Completed {
                result: reify_ir::Value::Undef,
                new_warm_state: None,
                cost_per_byte: None,
                diagnostics: vec![],
                structured_detail: vec![],
            }
        }

        let parsed = reify_compiler::parse_with_stdlib(FEA_BEARING_SRC, ModulePath::single("test"));
        let document =
            compile_document(&parsed, FEA_BEARING_SRC, &test_uri(), &CfgSet::host_default());
        let compiled = document.module();
        let mut engine = reify_eval::Engine::new(
            reify_eval::SolverPosture::SolverFree,
            Box::new(SimpleConstraintChecker),
            None,
        );
        let _ = engine.eval(compiled);

        // Positive control: the same engine and module, target still
        // unregistered, DO produce hints — so the empty result below is the
        // registration's doing, not a fixture that never hints.
        let unregistered_check_result = engine
            .check_snapshot(compiled)
            .expect("the engine should hold a snapshot for FEA_BEARING_SRC immediately after eval");
        assert!(
            !fea_not_evaluated_hints(
                &engine,
                &document,
                &unregistered_check_result,
                FEA_BEARING_SRC
            )
            .is_empty(),
            "positive control: with solver::elastic_static unregistered, the \
             FEA-bearing fixture must produce at least one hint"
        );

        engine.register_compute_fn(
            "solver::elastic_static",
            undef_result_fn as reify_eval::ComputeFn,
        );
        let _ = engine.eval_cached(compiled, VersionId(2));

        let check_result = engine.check_snapshot(compiled).expect(
            "the engine should hold a snapshot for FEA_BEARING_SRC immediately after eval_cached",
        );
        let indeterminate_count = check_result
            .constraint_results
            .iter()
            .filter(|e| e.satisfaction == Satisfaction::Indeterminate)
            .count();
        assert!(
            indeterminate_count >= 1,
            "fixture/test sanity: the registered trampoline returns Undef \
             (mirroring the unregistered body-inline fallback's value) so \
             the constraint(s) must still be Indeterminate here — otherwise \
             this test would trivially pass via the `entry.satisfaction != \
             Indeterminate` early-exit instead of exercising the \
             discriminator's `compute_dispatch(..).is_none()` check; got \
             constraint_results: {:#?}",
            check_result.constraint_results
        );

        let hints = fea_not_evaluated_hints(&engine, &document, &check_result, FEA_BEARING_SRC);
        assert!(
            hints.is_empty(),
            "no FEA \"not evaluated in editor\" hint should fire once \
             solver::elastic_static has a registered compute trampoline \
             (compute_dispatch(..).is_some()) — got: {hints:#?}"
        );
    }
}
