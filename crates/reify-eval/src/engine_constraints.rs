// Split from lib.rs (task 2032) — constraints methods.

use std::collections::{BTreeMap, HashMap};

use reify_compiler::{CompiledConstraint, CompiledModule, TopologyTemplate};
use reify_types::{
    CompiledExpr, CompiledFunction, ConstraintInput, ConstraintNodeId, ConstraintResult,
    DeterminacyState, Diagnostic, OptimizedImplInput, PersistentMap, Value, ValueCellId, ValueMap,
};

use crate::{CheckResult, ConstraintCheckEntry, Engine, EngineError};

impl Engine {
    /// Dispatch a batch of constraints to either their registered optimized
    /// implementation or the language-level `ConstraintChecker`, preserving
    /// the order of `entries` in the returned results (Task 273).
    ///
    /// Each entry is `(id, expr, optimized_target)`. Constraints whose
    /// `optimized_target` is `Some(t)` AND `t` is in `optimization_registry`
    /// are sent to that impl; everything else falls through to
    /// `self.constraint_checker`.
    ///
    /// Dispatch across registered targets happens in deterministic order
    /// (targets are iterated via a `BTreeMap`) so that any side effects —
    /// logging, metrics, impls that share mutable state — are reproducible
    /// from run to run.
    pub(crate) fn dispatch_constraints<'a>(
        &self,
        entries: Vec<(ConstraintNodeId, &'a CompiledExpr, Option<&'a str>)>,
        values: &'a ValueMap,
        functions: &'a [CompiledFunction],
        determinacy: Option<&'a PersistentMap<ValueCellId, (Value, DeterminacyState)>>,
    ) -> (Vec<ConstraintResult>, Vec<Diagnostic>) {
        if entries.is_empty() {
            return (Vec::new(), Vec::new());
        }

        // Fast path: when no optimized impls are registered every entry goes to
        // the language-level fallback. Skip the BTreeMap/Option-Vec/unzip
        // allocations and go directly to the checker — same code path as before
        // Task 273 introduced the bucketing logic.
        if self.optimization_registry.is_empty() {
            let constraints = entries
                .into_iter()
                .map(|(id, expr, _target)| (id, expr))
                .collect();
            let input = ConstraintInput {
                constraints,
                values,
                functions,
                determinacy,
            };
            return (self.constraint_checker.check(&input), Vec::new());
        }

        // Results in input order. We fill slots as each path completes.
        let mut results: Vec<Option<ConstraintResult>> = (0..entries.len()).map(|_| None).collect();

        // Diagnostics emitted by this function (contract violations only —
        // per-constraint diagnostics remain inside ConstraintResult).
        let mut dispatch_diagnostics: Vec<Diagnostic> = Vec::new();

        // Bucket entries by registered target. Keys borrow from the entry's
        // `Option<&'a str>` — no allocation. A `BTreeMap` gives deterministic
        // dispatch order across targets. `None` targets and targets with no
        // registered impl go to the language-level fallback bucket.
        //
        // We move `(ConstraintNodeId, &CompiledExpr)` directly into the
        // buckets so the dispatch path never clones a `ConstraintNodeId`.
        //
        // Each bucket entry keeps the *original index* alongside the payload
        // so the merge step below can weave results back into the caller-
        // visible order regardless of which group they were dispatched to.
        type BucketEntry<'b> = (usize, (ConstraintNodeId, &'b CompiledExpr));
        let mut optimized_groups: BTreeMap<&'a str, Vec<BucketEntry<'a>>> = BTreeMap::new();
        let mut fallback: Vec<BucketEntry<'a>> = Vec::new();
        for (i, (id, expr, target)) in entries.into_iter().enumerate() {
            match target {
                Some(t) if self.optimization_registry.contains_key(t) => {
                    optimized_groups.entry(t).or_default().push((i, (id, expr)));
                }
                _ => fallback.push((i, (id, expr))),
            }
        }

        // Dispatch each optimized group through its registered impl. The
        // contract is that the impl returns one `ConstraintResult` per input
        // constraint, in the same order. We weave results back into the
        // original result vector via each entry's recorded original index.
        //
        // On a count mismatch (third-party impl bug): emit a Diagnostic::error
        // and fall back to the language-level ConstraintChecker for this batch.
        // The entire batch is discarded — partial results cannot be reliably
        // correlated when we don't know which constraints they correspond to.
        for (target, bucket) in optimized_groups {
            let imp = self
                .optimization_registry
                .get(target)
                .expect("target was just bucketed from optimization_registry");
            let (indices, constraints): (Vec<usize>, Vec<(ConstraintNodeId, &'a CompiledExpr)>) =
                bucket.into_iter().unzip();
            let input = OptimizedImplInput {
                constraints,
                values,
                functions,
                determinacy,
            };
            let output = imp.check(&input);
            if output.results.len() != indices.len() {
                // Contract violation: the impl returned the wrong number of
                // results. Emit an error diagnostic and fall back to the
                // language-level checker for this entire batch.
                dispatch_diagnostics.push(Diagnostic::error(format!(
                    "OptimizedImpl for target {:?} returned {} results for {} constraints \
                     — falling back to language-level checker for this batch",
                    target,
                    output.results.len(),
                    indices.len(),
                )));
                let fallback_input = ConstraintInput {
                    constraints: input.constraints,
                    values,
                    functions,
                    determinacy,
                };
                let fallback_results = self.constraint_checker.check(&fallback_input);
                // INVARIANT ASSERT — intentional panic: `self.constraint_checker` is
                // code we own (the language-level evaluator). A count mismatch from it
                // is a *logic bug in our own code* and must fail loudly so it is caught
                // immediately. This is distinct from the `OptimizedImpl` count mismatch
                // checked above (line 109), which gets graceful fallback (Diagnostic::error
                // + re-run through the language-level checker) because third-party impls
                // are untrusted and must never be able to crash the engine.
                assert_eq!(
                    fallback_results.len(),
                    indices.len(),
                    "ConstraintChecker returned {} results for {} constraints during \
                     OptimizedImpl fallback",
                    fallback_results.len(),
                    indices.len(),
                );
                for (orig_idx, result) in indices.into_iter().zip(fallback_results) {
                    results[orig_idx] = Some(result);
                }
            } else {
                for (orig_idx, result) in indices.into_iter().zip(output.results) {
                    results[orig_idx] = Some(result);
                }
            }
        }

        // Dispatch the remainder through the language-level checker — same
        // input shape the callers used before Task 273.
        if !fallback.is_empty() {
            let (indices, constraints): (Vec<usize>, Vec<(ConstraintNodeId, &'a CompiledExpr)>) =
                fallback.into_iter().unzip();
            let input = ConstraintInput {
                constraints,
                values,
                functions,
                determinacy,
            };
            let fallback_results = self.constraint_checker.check(&input);
            // Same invariant assert as in the OptimizedImpl-fallback branch above —
            // see the comment there for the full rationale. Short form: this is our
            // own code; a wrong count is a logic bug that must panic, not be handled
            // gracefully.
            assert_eq!(
                fallback_results.len(),
                indices.len(),
                "ConstraintChecker returned {} results for {} constraints",
                fallback_results.len(),
                indices.len(),
            );
            for (orig_idx, result) in indices.into_iter().zip(fallback_results) {
                results[orig_idx] = Some(result);
            }
        }

        let constraint_results = results
            .into_iter()
            .map(|r| r.expect("dispatch_constraints: every slot must be filled"))
            .collect();
        (constraint_results, dispatch_diagnostics)
    }

    /// Replace occurrences of the raw ConstraintNodeId string in diagnostic
    /// messages with a human-readable label, when a label is present.
    ///
    /// This enriches engine-level diagnostics for constraint def instantiations
    /// so that messages read "constraint MinWall[0] violated" instead of
    /// "constraint S#constraint[0] violated". When `label` is `None` (inline
    /// constraints without a label), the messages are returned unchanged.
    pub(crate) fn labeled_diagnostics(
        messages: Vec<Diagnostic>,
        id: &reify_types::ConstraintNodeId,
        label: Option<&str>,
    ) -> Vec<Diagnostic> {
        let Some(lbl) = label else {
            return messages;
        };
        let id_str = id.to_string();
        messages
            .into_iter()
            .map(|mut d| {
                d.message = d.message.replace(&id_str, lbl);
                d
            })
            .collect()
    }

    /// Incrementally re-evaluate and check constraints after changing a parameter.
    ///
    /// Combines edit_param() (incremental value evaluation + re-resolution)
    /// with constraint satisfaction checking against the updated values.
    /// Check all constraints against the given values.
    ///
    /// Returns constraint check entries and any diagnostics produced by
    /// violated constraints. Uses the current snapshot's constraint graph.
    ///
    /// This is the shared constraint-checking logic used by both `edit_check`
    /// (sequential path) and `edit_check_concurrent` (concurrent path).
    pub fn check_constraints_with_values(
        &self,
        values: &ValueMap,
    ) -> Result<(Vec<ConstraintCheckEntry>, Vec<Diagnostic>), EngineError> {
        let mut constraint_results = Vec::new();
        let mut diagnostics = Vec::new();

        let state = self
            .eval_state
            .as_ref()
            .ok_or(EngineError::NotInitialized)?;

        let active_ids = state.snapshot.graph.active_constraint_ids(values);
        let constraint_nodes: Vec<_> = state
            .snapshot
            .graph
            .constraints
            .iter()
            .map(|(_, cnode)| cnode)
            .filter(|cnode| active_ids.contains(&cnode.id))
            .collect();

        if !constraint_nodes.is_empty() {
            let entries: Vec<_> = constraint_nodes
                .iter()
                .map(|cnode| {
                    (
                        cnode.id.clone(),
                        &cnode.expr,
                        cnode.optimized_target.as_deref(),
                    )
                })
                .collect();

            let (results, dispatch_diags) = self.dispatch_constraints(
                entries,
                values,
                &self.functions,
                Some(&state.snapshot.values),
            );
            diagnostics.extend(dispatch_diags);
            for (result, cnode) in results.into_iter().zip(constraint_nodes.iter()) {
                diagnostics.extend(Self::labeled_diagnostics(
                    result.diagnostics.messages,
                    &result.id,
                    cnode.label.as_deref(),
                ));
                constraint_results.push(ConstraintCheckEntry {
                    id: result.id,
                    label: cnode.label.clone(),
                    satisfaction: result.satisfaction,
                });
            }
        }

        Ok((constraint_results, diagnostics))
    }

    /// Check constraints using the current snapshot values, without re-calling eval().
    ///
    /// Returns `None` if no snapshot exists (i.e. eval() hasn't been called yet).
    /// Otherwise builds a ValueMap from the snapshot, runs constraint checking,
    /// and returns constraint results. This is the incremental companion to check():
    /// after edit_param() updates values, call check_snapshot() to see constraint
    /// status without destroying the incremental state.
    pub fn check_snapshot(&self, module: &CompiledModule) -> Option<CheckResult> {
        let state = self.eval_state.as_ref()?;

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        let (constraint_results, diagnostics) =
            self.check_constraints_against_templates(module, &values, Some(&state.snapshot.values));

        Some(CheckResult {
            values,
            constraint_results,
            diagnostics,
            resolved_params: HashMap::new(),
        })
    }

    /// Collect active constraints from a template given current values.
    ///
    /// Returns top-level constraints unconditionally, plus guarded constraints
    /// whose guard is currently active (true→group.constraints,
    /// false→group.else_constraints, Undef→neither branch).
    pub(crate) fn collect_active_constraints<'a>(
        template: &'a TopologyTemplate,
        values: &ValueMap,
    ) -> Vec<&'a CompiledConstraint> {
        let mut active: Vec<&'a CompiledConstraint> = Vec::new();

        // Top-level (unguarded) constraints are always active
        for c in &template.constraints {
            active.push(c);
        }

        // Guard-gated constraints
        for group in &template.guarded_groups {
            let guard_val = values.get(&group.guard_value_cell);
            match guard_val {
                Some(Value::Bool(true)) => {
                    for c in &group.constraints {
                        active.push(c);
                    }
                }
                Some(Value::Bool(false)) => {
                    for c in &group.else_constraints {
                        active.push(c);
                    }
                }
                _ => {
                    // Undef or non-Bool: neither branch active
                }
            }
        }

        active
    }

    /// Check all active constraints across all templates against the given values.
    ///
    /// Iterates over `module.templates`, collects active constraints via
    /// [`collect_active_constraints`], dispatches them via [`dispatch_constraints`],
    /// and accumulates [`ConstraintCheckEntry`] records and diagnostics. This is
    /// the per-template constraint loop shared by [`check`], [`check_snapshot`],
    /// `build_snapshot`, and `tessellate_snapshot` — the four sites that need
    /// guard-aware constraint checking against an evaluated value set.
    ///
    /// `determinacy` is forwarded to [`dispatch_constraints`] (typically
    /// `Some(&snapshot.values)` for determinacy-aware checking).
    pub(crate) fn check_constraints_against_templates(
        &self,
        module: &CompiledModule,
        values: &ValueMap,
        determinacy: Option<&PersistentMap<ValueCellId, (Value, DeterminacyState)>>,
    ) -> (Vec<ConstraintCheckEntry>, Vec<Diagnostic>) {
        let mut constraint_results = Vec::new();
        let mut diagnostics = Vec::new();

        for template in &module.templates {
            let active_constraints = Self::collect_active_constraints(template, values);

            if active_constraints.is_empty() {
                continue;
            }

            let entries: Vec<_> = active_constraints
                .iter()
                .map(|c| (c.id.clone(), &c.expr, c.optimized_target.as_deref()))
                .collect();

            let (results, dispatch_diags) =
                self.dispatch_constraints(entries, values, &self.functions, determinacy);
            diagnostics.extend(dispatch_diags);

            for (result, compiled) in results.into_iter().zip(active_constraints.iter()) {
                diagnostics.extend(Self::labeled_diagnostics(
                    result.diagnostics.messages,
                    &result.id,
                    compiled.label.as_deref(),
                ));
                constraint_results.push(ConstraintCheckEntry {
                    id: result.id,
                    label: compiled.label.clone(),
                    satisfaction: result.satisfaction,
                });
            }
        }

        (constraint_results, diagnostics)
    }

    /// Evaluate and check constraints (guard-aware).
    ///
    /// Checks top-level (unguarded) constraints unconditionally, plus
    /// guarded constraints whose guard is active (true→group.constraints,
    /// false→group.else_constraints, Undef→neither).
    pub fn check(&mut self, module: &CompiledModule) -> CheckResult {
        let eval_result = self.eval(module);
        let mut diagnostics = eval_result.diagnostics;

        // After eval(), eval_state is always Some — unwrap is safe here.
        let det_values = &self.eval_state.as_ref().unwrap().snapshot.values;
        let (constraint_results, constraint_diags) =
            self.check_constraints_against_templates(module, &eval_result.values, Some(det_values));
        diagnostics.extend(constraint_diags);

        CheckResult {
            values: eval_result.values,
            constraint_results,
            diagnostics,
            resolved_params: eval_result.resolved_params,
        }
    }
}
