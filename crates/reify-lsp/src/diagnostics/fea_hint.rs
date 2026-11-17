//! The `fea-not-evaluated` Info hint for constraints whose value reaches an
//! `@optimized` compute target the engine has no trampoline for (task 5078,
//! PRD `compute-fea-hardening.md` C2).

use std::collections::HashMap;

use reify_core::{ConstraintNodeId, ValueCellId};
use reify_ir::Satisfaction;
use tower_lsp::lsp_types;

use super::document_compile::CompiledDocument;

/// Task 5078 (PRD `compute-fea-hardening.md` C2): hint diagnostics for
/// FEA-dependent constraints that go unevaluated under
/// `diagnostics::compute_diagnostics_with_state`'s trampoline-free posture.
/// A constraint reading an `@optimized` FEA result (e.g.
/// `solve_elastic_static`'s `max_von_mises`) body-inlines to `Value::Undef`
/// and therefore checks as `Satisfaction::Indeterminate` — neither the
/// `violated_messages` skip-set nor the span-aware `Satisfaction::Violated`
/// loop in that function emits anything for it, so without this hint it
/// surfaces NO diagnostic at all, silently indistinguishable from "no
/// constraint". Surface a non-noisy `Severity::Info` hint instead, anchored
/// to the constraint's span, so the user knows why the constraint shows
/// nothing in the editor.
///
/// Gated by `constraint_depends_on_unregistered_optimized_compute` so this
/// does NOT fire on every `Indeterminate` constraint — e.g. a
/// genuinely-unresolved `auto` param is also `Indeterminate` here (no solver
/// attached), but is not FEA-dependent and must not get this hint.
///
/// Deliberately additive and easily removable: one emitter reading
/// already-computed `check_result` / `compiled` data, no new `EvalState`
/// fields, no keystroke-time solve.
pub(super) fn fea_not_evaluated_hints(
    engine: &reify_eval::Engine,
    document: &CompiledDocument,
    check_result: &reify_eval::CheckResult,
    source: &str,
) -> Vec<lsp_types::Diagnostic> {
    // TODO(#5023): remove this stopgap when async-recalc Phase A lands
    // per-constraint computing/not-evaluated states that supersede this hint.
    //
    // Perf guard: this whole function — including the lookup maps below — runs
    // on the LSP keystroke hot path, so skip it entirely when there is no
    // `Indeterminate` constraint in this document at all. Nothing in the
    // loop below can fire without at least one such entry, so this still
    // frees the common fully-resolved (no `auto` params, no FEA) document
    // from all of the cost below.
    //
    // This guard is intentionally coarse — it is NOT "only FEA documents pay
    // this cost". A genuinely-unresolved `auto` param is *also*
    // `Indeterminate` with no solver attached, and `auto` params are routine
    // in this parametric DSL (`diagnostics.rs`'s
    // `fea_hint_excludes_auto_param_indeterminate_and_dedups_per_constraint`
    // test uses `param gap : Length = auto` as its non-FEA control case), so
    // plenty of non-FEA documents still build the maps below on every
    // keystroke. A cheaper *and correct* pre-filter isn't available for
    // free: `engine.prelude()` is the full stdlib, which always
    // declares `@optimized` functions (`solve_elastic_static` and its
    // siblings), so "does `compiled.functions` ∪ prelude declare any
    // function with `optimized_target.is_some()`" is trivially true on every
    // call and would filter nothing; narrowing that check to
    // `compiled.functions` alone would filter too much, since a real FEA
    // constraint reaches its solver call through a *prelude* function, not a
    // module-local one, and would wrongly lose its hint. A correct tighter
    // filter would have to either walk the same constraint/value-cell
    // closure the code below already performs (no savings) or cache
    // results across calls, which would need new persistent state — ruled
    // out by this hint's "no new `EvalState` fields" design (see this
    // function's doc).
    if !check_result
        .constraint_results
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Indeterminate)
    {
        return Vec::new();
    }

    let compiled = document.module();
    let value_cell_exprs: HashMap<ValueCellId, _> = compiled
        .templates
        .iter()
        .flat_map(|t| t.value_cells.iter())
        .filter_map(|vc| vc.default_expr.as_ref().map(|e| (vc.id.clone(), e)))
        .collect();
    let compiled_constraints: HashMap<&ConstraintNodeId, _> = compiled
        .templates
        .iter()
        .enumerate()
        .flat_map(|(index, t)| t.constraints.iter().map(move |c| (&c.id, (index, c))))
        .collect();
    // Precompute name -> "names an unregistered @optimized compute
    // target" once per document, instead of re-scanning all visible
    // functions for every `UserFunctionCall` node encountered across
    // every constraint's transitive value-cell closure — O(nodes *
    // functions) before, O(functions) once to build plus O(1) lookups
    // after.
    //
    // Iterate the union of the compiled module's own functions and the
    // engine's prelude functions directly, with no intermediate `Vec` —
    // stdlib solvers like `solve_elastic_static` are prelude functions,
    // not user-module functions, so the prelude must be included or
    // every FEA constraint would fail to match. Module-local functions
    // are chained FIRST so that `.or_insert_with` (which keeps only the
    // first-seen function per name) resolves a name shared between a
    // module-local function and a prelude solver to the module's own
    // definition — see the caller precedence note on
    // `constraint_depends_on_unregistered_optimized_compute`.
    let mut unregistered_optimized_fn_names: HashMap<&str, bool> = HashMap::new();
    for f in compiled
        .functions
        .iter()
        .chain(engine.prelude().iter().flat_map(|m| m.functions.iter()))
    {
        unregistered_optimized_fn_names
            .entry(f.name.as_str())
            .or_insert_with(|| {
                f.optimized_target
                    .as_deref()
                    .is_some_and(|t| engine.compute_dispatch(t).is_none())
            });
    }

    // No `(id, label)` dedup guard is needed here. `check_result.
    // constraint_results` is built upstream (reify-eval) with at most
    // one entry per constraint id: `forall` per-element constraints
    // each get a fresh, unique `ConstraintNodeId` (`forall_elaborate.rs`
    // increments `constraint_index` per element, so distinct elements
    // never share an id), and the GD&T/Conforms path explicitly
    // overrides any existing same-id entry in place rather than pushing
    // a second one (`engine_constraints.rs`'s "OVERRIDE the matching
    // entry ... push if absent" comment). So this single iteration
    // already yields at most one hint per constraint without any extra
    // bookkeeping. Pinned by
    // `fea_hint_two_fea_constraints_each_get_distinct_span_hint` (in
    // `diagnostics.rs`'s `mod tests`), which is the case most sensitive
    // to a regression in this upstream invariant.
    let mut hints = Vec::new();
    for entry in &check_result.constraint_results {
        if entry.satisfaction != Satisfaction::Indeterminate {
            continue;
        }
        let Some(&(template_index, constraint)) = compiled_constraints.get(&entry.id) else {
            continue;
        };
        if !constraint_depends_on_unregistered_optimized_compute(
            &constraint.expr,
            &value_cell_exprs,
            &unregistered_optimized_fn_names,
        ) {
            continue;
        }
        hints.push(lsp_types::Diagnostic {
            range: document.template_span_range(source, template_index, constraint.span),
            severity: Some(lsp_types::DiagnosticSeverity::INFORMATION),
            code: Some(lsp_types::NumberOrString::String(
                FEA_NOT_EVALUATED_CODE.to_string(),
            )),
            source: Some("reify".to_string()),
            message: FEA_NOT_EVALUATED_HINT.to_string(),
            ..Default::default()
        });
    }
    hints
}

/// Exact wording of task 5078's (PRD `compute-fea-hardening.md` C2) FEA
/// "not evaluated in editor" `Severity::Info` hint, emitted by
/// [`fea_not_evaluated_hints`].
///
/// Defined once here rather than re-declared as a local constant in each
/// regression test, so the production message and every test that asserts
/// on it share one source of truth: a future wording change updates every
/// consumer at once instead of requiring each test to be hand-edited to
/// keep matching. Tests should NOT use this to *identify* the diagnostic
/// (i.e. as a `filter` predicate) — use [`FEA_NOT_EVALUATED_CODE`] for
/// that, and reserve this for asserting the human-readable wording once a
/// diagnostic is already known to be the FEA hint.
pub(super) const FEA_NOT_EVALUATED_HINT: &str =
    "FEA constraint not evaluated in editor — run `reify test`";

/// Stable `code` for task 5078's FEA "not evaluated in editor" hint (PRD
/// `compute-fea-hardening.md` C2), set on the `lsp_types::Diagnostic`
/// emitted by [`fea_not_evaluated_hints`] alongside
/// [`FEA_NOT_EVALUATED_HINT`].
///
/// Unlike the message, which is free-text prose, this is the
/// wording-independent handle consumers — including the pipeline's
/// regression tests in `diagnostics.rs` — should match on to identify the
/// diagnostic, mirroring the `"computation-pending"` / `"computation-failed"`
/// codes the freshness diagnostics carry (emitted by
/// `diagnostics::freshness_diagnostics`). A future copy-edit to
/// [`FEA_NOT_EVALUATED_HINT`] must never silently break identification for a
/// consumer matching on this code.
pub(super) const FEA_NOT_EVALUATED_CODE: &str = "fea-not-evaluated";

/// Static discriminator for task 5078's FEA "not evaluated in editor" hint
/// (PRD `compute-fea-hardening.md` C2).
///
/// Returns `true` when `constraint_expr` — or the `default_expr` of any
/// value cell it transitively depends on — calls a function whose
/// `@optimized("target")` annotation names a compute target with no
/// registered trampoline on `engine`. Under this crate's trampoline-free LSP
/// posture (see `diagnostics::compute_diagnostics_with_state`'s doc comment)
/// such a target always body-inlines to `Value::Undef`, so this predicate is
/// exactly the "is this constraint's `Indeterminate` result caused by an
/// unevaluated FEA/compute solve, as opposed to some other cause (e.g. a
/// genuinely-unresolved `auto` param)" test.
///
/// `unregistered_optimized_fn_names` should map every visible function name
/// (the union of the compiled module's own functions and the engine's
/// prelude functions — stdlib solvers such as `solve_elastic_static` are
/// prelude functions, not user-module functions) to whether that name's
/// `@optimized("target")` annotation (if any) names a compute target with
/// no registered trampoline on the engine. Building this map is the
/// caller's responsibility (see [`fea_not_evaluated_hints`]) so it can
/// be computed once per document rather than once per constraint.
///
/// Matching is by function NAME rather than full overload resolution: in
/// this engine posture *every* `@optimized` target is unregistered, and the
/// FEA solver overloads uniformly carry the same target string, so a name
/// match cannot mis-resolve which target applies among same-named prelude
/// overloads.
///
/// Caller precedence note: the caller builds this map from module-local
/// functions chained *before* prelude functions and inserts with "first
/// occurrence wins" — mirroring `find_matching_compiled_function`'s
/// (`reify-expr/src/lib.rs`) first-match-wins precedent for "which function
/// does this name resolve to". This means a module-local function that
/// happens to share a prelude solver's name (e.g. a user helper also named
/// `solve_elastic_static`) correctly shadows the prelude entry here,
/// avoiding a false-positive hint attributed to the prelude solver when the
/// resolved call is actually the user's own (non-FEA) function. Residual
/// limitation: if a *module* itself declares more than one same-named
/// overload with differing `@optimized` status, the map keeps whichever
/// happens to iterate first — this mirrors the same overload-blindness
/// already accepted for prelude solver overloads above, not a new gap.
///
/// **Accepted limitation — indirect FEA dependence through a wrapper
/// function is not detected.** The traversal below follows the constraint
/// expression and the transitive closure of value-cell `default_expr`s, but
/// does not descend into the *body* of a called user function. A constraint
/// that reaches an FEA solver only indirectly (e.g. `let s =
/// my_wrapper(...)` where `my_wrapper`'s own body calls
/// `solve_elastic_static`, but `my_wrapper` itself carries no `@optimized`
/// annotation) still evaluates to `Value::Undef`/`Indeterminate` under the
/// trampoline-free posture, but this predicate returns `false` for it — such
/// a constraint falls back to the exact "silently indistinguishable from no
/// constraint" state this hint exists to fix. Closing this gap would require
/// also enqueueing called user-function bodies (guarded by a function-name
/// `visited` set, alongside the existing value-cell one, for cycle-safety);
/// deferred as a follow-up rather than widening this task's traversal. Not
/// exercised by any current fixture — all of them call the solver directly.
///
/// Traversal is a work-queue over value-cell ids (seeded from
/// `constraint_expr.collect_value_refs()`, expanded via each visited cell's
/// own `default_expr.collect_value_refs()`) guarded by a `visited` set for
/// cycle-safety, mirroring the leaf-walk shape of `classify_undef_origins`
/// (`reify-eval/src/engine_eval.rs`) but over static `default_expr`s rather
/// than runtime snapshot values — under this posture the runtime
/// `ComputeNode` graph is empty (insertion is gated on
/// `compute_dispatch(target).is_some()`), so the `@optimized` marker
/// survives only statically on `CompiledFunction.optimized_target`.
fn constraint_depends_on_unregistered_optimized_compute(
    constraint_expr: &reify_ir::CompiledExpr,
    value_cell_exprs: &HashMap<ValueCellId, &reify_ir::CompiledExpr>,
    unregistered_optimized_fn_names: &HashMap<&str, bool>,
) -> bool {
    // `CompiledExpr::walk` (reify-ir/src/expr.rs) takes a plain
    // `FnMut(&CompiledExpr)` with no `ControlFlow`/abort signal, so it
    // cannot be short-circuited: the `if found { return; }` below only
    // skips this closure's own per-node work once a match is found, not
    // the remaining traversal, which `walk` still performs in full. This
    // runs on the LSP keystroke hot path (per constraint, per visited
    // value-cell expr), but is bounded by a single expression's node
    // count; adding a real abort would require a new short-circuiting
    // traversal primitive on `CompiledExpr`, out of this file's scope.
    fn expr_calls_unregistered_optimized_fn(
        expr: &reify_ir::CompiledExpr,
        unregistered_optimized_fn_names: &HashMap<&str, bool>,
    ) -> bool {
        let mut found = false;
        expr.walk(&mut |node| {
            if found {
                return;
            }
            if let reify_ir::CompiledExprKind::UserFunctionCall { function_name, .. } = &node.kind {
                found = unregistered_optimized_fn_names
                    .get(function_name.as_str())
                    .copied()
                    .unwrap_or(false);
            }
        });
        found
    }

    if expr_calls_unregistered_optimized_fn(constraint_expr, unregistered_optimized_fn_names) {
        return true;
    }

    let mut visited: std::collections::HashSet<ValueCellId> = std::collections::HashSet::new();
    let mut queue: Vec<ValueCellId> = constraint_expr.collect_value_refs();
    while let Some(cell_id) = queue.pop() {
        if !visited.insert(cell_id.clone()) {
            continue;
        }
        let Some(default_expr) = value_cell_exprs.get(&cell_id) else {
            continue;
        };
        if expr_calls_unregistered_optimized_fn(default_expr, unregistered_optimized_fn_names) {
            return true;
        }
        for referenced in default_expr.collect_value_refs() {
            if !visited.contains(&referenced) {
                queue.push(referenced);
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::span_to_range;
    use crate::decl_oracle::test_fixtures::{FEA_BEARING_SRC, FEA_WITH_AUTO_PARAM_SRC, test_uri};
    use crate::diagnostics::document_compile::compile_document;
    use reify_compiler::cfg::CfgSet;
    use reify_constraints::SimpleConstraintChecker;
    use reify_core::{ModulePath, Severity, VersionId};

    /// `source` compiled by the production route and evaluated once on the
    /// engine `EvalState::new()` builds: solver-free, with no compute
    /// trampoline registered.
    fn compile_and_eval(source: &str) -> (CompiledDocument, reify_eval::Engine) {
        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
        let document = compile_document(&parsed, source, &test_uri(), &CfgSet::host_default());
        let compile_errors: Vec<_> = document
            .module()
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            compile_errors.is_empty(),
            "fixture sanity: the source must compile without errors; got: {compile_errors:#?}"
        );
        let mut engine = reify_eval::Engine::new(
            reify_eval::SolverPosture::SolverFree,
            Box::new(SimpleConstraintChecker),
            None,
        );
        let _ = engine.eval(document.module());
        (document, engine)
    }

    fn indeterminate_count(check_result: &reify_eval::CheckResult) -> usize {
        check_result
            .constraint_results
            .iter()
            .filter(|e| e.satisfaction == Satisfaction::Indeterminate)
            .count()
    }

    /// The range of the one constraint of `document` whose source text
    /// contains `needle`.
    fn constraint_range(
        document: &CompiledDocument,
        source: &str,
        needle: &str,
    ) -> lsp_types::Range {
        let mut matching = document
            .module()
            .templates
            .iter()
            .flat_map(|t| t.constraints.iter())
            .filter(|c| source[c.span.start as usize..c.span.end as usize].contains(needle));
        let constraint = matching
            .next()
            .unwrap_or_else(|| panic!("fixture sanity: no constraint contains {needle:?}"));
        assert!(
            matching.next().is_none(),
            "fixture sanity: more than one constraint contains {needle:?}"
        );
        span_to_range(source, constraint.span)
    }

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

        let (document, mut engine) = compile_and_eval(FEA_BEARING_SRC);
        let compiled = document.module();

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
        assert!(
            indeterminate_count(&check_result) >= 1,
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

    /// Pins the discriminator's other core behaviour at this module's own
    /// seam: of two `Indeterminate` constraints, only the FEA-dependent one
    /// gets a hint, and exactly one, though it reads the FEA result twice.
    /// The `gap` constraint is `Indeterminate` only because no solver
    /// resolves its `auto` param, which is not an unevaluated compute target.
    #[test]
    fn fea_hint_skips_non_fea_indeterminate_and_dedups_per_constraint() {
        let (document, engine) = compile_and_eval(FEA_WITH_AUTO_PARAM_SRC);
        let check_result = engine
            .check_snapshot(document.module())
            .expect("the engine should hold a snapshot immediately after eval");
        assert_eq!(
            indeterminate_count(&check_result),
            2,
            "fixture sanity: the FEA constraint and the `gap` auto-param \
             constraint must both be Indeterminate; got constraint_results: {:#?}",
            check_result.constraint_results
        );

        let hints =
            fea_not_evaluated_hints(&engine, &document, &check_result, FEA_WITH_AUTO_PARAM_SRC);

        let fea_constraint = constraint_range(&document, FEA_WITH_AUTO_PARAM_SRC, "peak_stress");
        let hint_ranges: Vec<_> = hints.iter().map(|h| h.range).collect();
        assert_eq!(
            hint_ranges,
            vec![fea_constraint],
            "expected exactly one hint, on the FEA constraint; got: {hints:#?}"
        );
    }

    /// Pins the lookup's precedence: a module-local function sharing a
    /// prelude FEA solver's name shadows it, so an `Indeterminate` constraint
    /// calling the module's own `solve_elastic_static` gets no hint even
    /// though the prelude's same-named solver names an unregistered
    /// `@optimized` target.
    #[test]
    fn fea_hint_module_local_function_shadows_same_named_prelude_solver() {
        const SRC: &str = r#"fn solve_elastic_static(x: Length) -> Length { x }

structure ShadowedSolver {
    param gap : Length = auto
    let stress = solve_elastic_static(gap)
    constraint stress > 1mm
}"#;
        let (document, engine) = compile_and_eval(SRC);

        let module_solvers: Vec<_> = document
            .module()
            .functions
            .iter()
            .filter(|f| f.name == "solve_elastic_static")
            .collect();
        assert!(
            !module_solvers.is_empty()
                && module_solvers.iter().all(|f| f.optimized_target.is_none()),
            "fixture sanity: the module must declare its own, non-`@optimized` \
             `solve_elastic_static`"
        );
        assert!(
            engine
                .prelude()
                .iter()
                .flat_map(|m| m.functions.iter())
                .filter(|f| f.name == "solve_elastic_static")
                .any(|f| f
                    .optimized_target
                    .as_deref()
                    .is_some_and(|t| engine.compute_dispatch(t).is_none())),
            "fixture sanity: the prelude must declare a same-named \
             `solve_elastic_static` naming an unregistered `@optimized` target, \
             or there is nothing to shadow"
        );

        let check_result = engine
            .check_snapshot(document.module())
            .expect("the engine should hold a snapshot immediately after eval");
        assert_eq!(
            indeterminate_count(&check_result),
            1,
            "fixture sanity: the constraint over the unresolved `auto` param \
             must be Indeterminate, or the hint loop never reaches the \
             discriminator; got constraint_results: {:#?}",
            check_result.constraint_results
        );

        let hints = fea_not_evaluated_hints(&engine, &document, &check_result, SRC);
        assert!(
            hints.is_empty(),
            "the call resolves to the module's own `solve_elastic_static`, so \
             no FEA hint should fire; got: {hints:#?}"
        );
    }
}
