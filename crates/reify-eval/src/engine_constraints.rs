// Split from lib.rs (task 2032) — constraints methods.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use reify_compiler::{CompiledConstraint, CompiledModule, TopologyTemplate};
use reify_core::{ConstraintNodeId, Diagnostic, Severity, ValueCellId};
use reify_ir::{
    CompiledExpr, CompiledFunction, ConstraintDiagnostics, ConstraintInput, ConstraintResult,
    DeterminacyState, OptimizedImplInput, PersistentMap, Value, ValueMap,
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
    ///
    /// ## RepresentationWithin interception (task-4199 γ)
    ///
    /// `RepresentationWithin(subject, bound)` entries are peeled off the batch
    /// **before** bucketing and evaluated engine-side from
    /// `self.achieved_repr_tol` + `values` via
    /// [`crate::tolerance_combine::eval_representation_within`].  The
    /// remaining entries are dispatched through the existing optimised /
    /// language-level paths.  All results are woven back in caller (input)
    /// order via a slot vector so that neither path needs to know about the
    /// other.  The fast-path early-return (no registered impls, no
    /// RepresentationWithin) is preserved so non-assertion modules incur zero
    /// overhead (C2).
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

        // ── RepresentationWithin interception ─────────────────────────────────
        // Peel RepresentationWithin entries off the batch before bucketing so
        // that they never reach the language-level ConstraintChecker (which has
        // no access to self.achieved_repr_tol).  Each matched entry is
        // evaluated engine-side; unmatched entries go to the existing paths.
        //
        // Two-vector approach avoids a second allocation pass: we collect
        // `rest` in-order so the original (id, expr, target) tuples remain
        // borrow-valid for the bucketing step below.
        let n = entries.len();
        let mut rw_slots: Vec<Option<ConstraintResult>> = (0..n).map(|_| None).collect();
        let mut rest: Vec<(usize, ConstraintNodeId, &'a CompiledExpr, Option<&'a str>)> =
            Vec::with_capacity(n);
        let mut any_rw = false;

        for (i, (id, expr, target)) in entries.into_iter().enumerate() {
            match crate::tolerance_combine::eval_representation_within(
                &id,
                expr,
                values,
                &self.achieved_repr_tol,
            ) {
                Some((satisfaction, diag_opt)) => {
                    // Engine-side result from the achieved-repr-tol map.
                    rw_slots[i] = Some(ConstraintResult {
                        id,
                        satisfaction,
                        diagnostics: ConstraintDiagnostics {
                            messages: diag_opt.into_iter().collect(),
                        },
                    });
                    any_rw = true;
                }
                None => {
                    // Not a RepresentationWithin shape — pass through.
                    rest.push((i, id, expr, target));
                }
            }
        }

        // All entries were RepresentationWithin — skip bucketing entirely.
        if rest.is_empty() {
            let constraint_results = rw_slots
                .into_iter()
                .map(|r| r.expect("every RepresentationWithin slot must be filled"))
                .collect();
            return (constraint_results, Vec::new());
        }

        // No RepresentationWithin entries AND no registered impls → original
        // fast path: zero extra allocations for non-assertion modules (C2).
        if !any_rw && self.optimization_registry.is_empty() {
            let constraints: Vec<(ConstraintNodeId, &CompiledExpr)> = rest
                .into_iter()
                .map(|(_i, id, expr, _target)| (id, expr))
                .collect();
            let input = ConstraintInput {
                constraints: Cow::Owned(constraints),
                values,
                functions,
                determinacy,
            };
            return (self.constraint_checker.check(&input), Vec::new());
        }

        // Mixed batch (some RepresentationWithin + some pass-through) or
        // optimised impls are registered: use the slot vector for order
        // preservation.  Reuse rw_slots as the unified results vector.
        let mut results = rw_slots;

        // Diagnostics emitted by this function (contract violations only —
        // per-constraint diagnostics remain inside ConstraintResult).
        let mut dispatch_diagnostics: Vec<Diagnostic> = Vec::new();

        if self.optimization_registry.is_empty() {
            // Fast path for the pass-through subset: no optimised groups.
            let (indices, constraints): (Vec<usize>, Vec<(ConstraintNodeId, &'a CompiledExpr)>) =
                rest.into_iter()
                    .map(|(i, id, expr, _target)| (i, (id, expr)))
                    .unzip();
            let input = ConstraintInput {
                constraints: Cow::Owned(constraints),
                values,
                functions,
                determinacy,
            };
            let fallback_results = self.constraint_checker.check(&input);
            assert_eq!(
                fallback_results.len(),
                indices.len(),
                "ConstraintChecker returned {} results for {} non-RepresentationWithin \
                 constraints",
                fallback_results.len(),
                indices.len(),
            );
            for (orig_idx, result) in indices.into_iter().zip(fallback_results) {
                results[orig_idx] = Some(result);
            }
            let constraint_results = results
                .into_iter()
                .map(|r| r.expect("dispatch_constraints: every slot must be filled"))
                .collect();
            return (constraint_results, dispatch_diagnostics);
        }

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
        for (i, id, expr, target) in rest {
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
                    constraints: Cow::Owned(input.constraints),
                    values,
                    functions,
                    determinacy,
                };
                let fallback_results = self.constraint_checker.check(&fallback_input);
                // INVARIANT ASSERT — intentional panic: `self.constraint_checker` is
                // code we own (the language-level evaluator). A count mismatch from it
                // is a *logic bug in our own code* and must fail loudly so it is caught
                // immediately. This is distinct from the `OptimizedImpl` count mismatch
                // from the `output.results.len() != indices.len()` guard above, which gets
                // graceful fallback (Diagnostic::error + re-run through the language-level
                // checker) because third-party impls are untrusted and must never be able
                // to crash the engine.
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
                constraints: Cow::Owned(constraints),
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
    /// so that messages read "constraint MinWall#0[0] violated" instead of
    /// "constraint S#constraint[0] violated". Replacement covers BOTH the
    /// top-level `Diagnostic::message` AND every `DiagnosticLabel::message`
    /// inside `Diagnostic::labels` — downstream presenters may render either
    /// field, so either carrying the raw id would leak the opaque form.
    ///
    /// In-place mutation (`&mut [Diagnostic]`) avoids the `.collect()`
    /// round-trip used before task 847.2 and enables a `contains`-guarded
    /// `tracing::debug!` drift signal: when an Error-severity message is
    /// present but the raw id is absent, we emit a non-fatal `tracing::debug!`
    /// so first-party format drift is observable without flooding WARN logs
    /// for third-party `ConstraintChecker` implementations that intentionally
    /// emit domain-specific error text. The signal is scoped to Error-severity
    /// because Info/Warning diagnostics attached to a labeled constraint (e.g.
    /// "inputs still undetermined") may be natural-language only and need not
    /// embed the raw id. When `label` is `None` (inline constraints without a
    /// label), the messages are returned unchanged. A slice (not `&mut Vec`) is
    /// taken because the rewrite never adds or removes entries — only
    /// mutates existing ones.
    pub(crate) fn labeled_diagnostics(
        messages: &mut [Diagnostic],
        id: &reify_core::ConstraintNodeId,
        label: Option<&str>,
    ) {
        let Some(lbl) = label else {
            return;
        };
        let id_str = id.to_string();
        let mut replaced_any = false;
        let mut has_error = false;
        for d in messages.iter_mut() {
            if d.severity == Severity::Error {
                has_error = true;
            }
            if d.message.contains(&id_str) {
                d.message = d.message.replace(&id_str, lbl);
                replaced_any = true;
            }
            for lbl_obj in d.labels.iter_mut() {
                if lbl_obj.message.contains(&id_str) {
                    lbl_obj.message = lbl_obj.message.replace(&id_str, lbl);
                    replaced_any = true;
                }
            }
        }
        // Emit a non-fatal drift signal at DEBUG level when at least one
        // Error-severity diagnostic is present but the raw id never appeared in
        // any message. This covers two cases: (1) first-party Display drift —
        // our own ConstraintNodeId::Display impl changed without updating this
        // helper; (2) a third-party ConstraintChecker that emits domain-specific
        // error text without embedding the raw id. Runtime output is still
        // correct in both cases (domain text reaches users unchanged). Using
        // debug! rather than warn! means third-party checkers that legitimately
        // omit the raw id will not flood WARN logs; first-party developers can
        // observe the signal by enabling DEBUG logging for the "reify_eval"
        // target (e.g. RUST_LOG=reify_eval=debug).
        if !replaced_any && has_error {
            tracing::debug!(
                label = ?label,
                id = %id_str,
                "labeled_diagnostics: id format drift or non-embedding ConstraintChecker \
                 — label substitution had no target; Error-severity message present but \
                 ConstraintNodeId did not appear in any message",
            );
        }
    }

    /// Consume a `ConstraintResult`, run the label-rewrite over its diagnostic
    /// messages, extend them into `diagnostics`, and push a matching
    /// `ConstraintCheckEntry` onto `constraint_results`.
    ///
    /// Extracted from the two zip-loops in `check_constraints_with_values` and
    /// `check_constraints_against_templates` (task 847.1). Both sites ran the
    /// same three-step "take-messages / rewrite / extend / push entry" pattern;
    /// centralising it keeps the rewrite invariants (see `labeled_diagnostics`)
    /// in one place.
    ///
    /// **Arch §9.3 separation (Failed vs. Violated).** This helper deliberately
    /// keeps constraint satisfaction (`Satisfaction::Violated` plus
    /// `DiagnosticCode::ConstraintViolated`) on the `ConstraintCheckEntry` /
    /// diagnostics channels and never touches `Freshness::Failed` or emits
    /// `EventKind::Failed`. `Freshness::Failed` is reserved for evaluation-
    /// pipeline failures (panic boundary, kernel error — arch §9.1–§9.2);
    /// folding constraint violations into it would silently merge two
    /// orthogonal channels. The §9.3 separation is regression-tested at
    /// `crates/reify-eval/tests/failed_propagation.rs::
    /// constraint_violation_does_not_produce_failed_freshness_or_error_event`.
    fn push_constraint_result(
        diagnostics: &mut Vec<Diagnostic>,
        constraint_results: &mut Vec<ConstraintCheckEntry>,
        result: ConstraintResult,
        label: Option<&str>,
    ) {
        let mut msgs = result.diagnostics.messages;
        Self::labeled_diagnostics(&mut msgs, &result.id, label);
        diagnostics.extend(msgs);
        constraint_results.push(ConstraintCheckEntry {
            id: result.id,
            label: label.map(|s| s.to_string()),
            satisfaction: result.satisfaction,
        });
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

        // Overlay injected let-cell values onto the incoming values so that
        // constraints referencing purpose let-cells can resolve them (task 4009 δ).
        // active_purpose_let_cells only contains entries for let-bearing purposes
        // (let-less purposes are never inserted), so the fast-path is taken
        // whenever no let-bearing purpose is active — O(1) map-empty check.
        let effective_values: ValueMap = if self.active_purpose_let_cells.is_empty() {
            values.clone()
        } else {
            let mut v = values.clone();
            for let_ids in self.active_purpose_let_cells.values() {
                for id in let_ids {
                    if let Some((val, _det)) = state.snapshot.values.get(id) {
                        v.insert(id.clone(), val.clone());
                    }
                }
            }
            v
        };

        let active_ids = state
            .snapshot
            .graph
            .active_constraint_ids(&effective_values);
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
                &effective_values,
                &self.functions,
                Some(&state.snapshot.values),
            );
            diagnostics.extend(dispatch_diags);
            // Task 846.3: `zip` silently truncates to the shorter iterator, so
            // a length mismatch must be caught BEFORE the loop runs. These are
            // debug-only checks — the invariants already hold today, but future
            // refactors of `dispatch_constraints` could desync the two sequences.
            debug_assert_eq!(
                results.len(),
                constraint_nodes.len(),
                "check_constraints_with_values: results/constraint_nodes length mismatch",
            );
            for (result, cnode) in results.into_iter().zip(constraint_nodes.iter()) {
                debug_assert_eq!(
                    result.id, cnode.id,
                    "check_constraints_with_values: result.id must match cnode.id \
                     — dispatch_constraints reordered results or constraint_nodes changed",
                );
                Self::push_constraint_result(
                    &mut diagnostics,
                    &mut constraint_results,
                    result,
                    cnode.label.as_deref(),
                );
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
            // Task 846.3: see rationale in `check_constraints_with_values` —
            // `zip` truncates silently, so guard the length invariant BEFORE
            // the loop, and the per-result id-match invariant INSIDE it.
            debug_assert_eq!(
                results.len(),
                active_constraints.len(),
                "check_constraints_against_templates: results/active_constraints length mismatch",
            );

            for (result, compiled) in results.into_iter().zip(active_constraints.iter()) {
                debug_assert_eq!(
                    result.id, compiled.id,
                    "check_constraints_against_templates: result.id must match compiled.id \
                     — dispatch_constraints reordered results or active_constraints changed",
                );
                Self::push_constraint_result(
                    &mut diagnostics,
                    &mut constraint_results,
                    result,
                    compiled.label.as_deref(),
                );
            }
        }

        (constraint_results, diagnostics)
    }

    /// Evaluate and check constraints (guard-aware).
    ///
    /// Checks top-level (unguarded) constraints unconditionally, plus
    /// guarded constraints whose guard is active (true→group.constraints,
    /// false→group.else_constraints, Undef→neither).
    ///
    /// ## RepresentationWithin ordering invariant (task-4199 γ / C1)
    ///
    /// `self.achieved_repr_tol` is populated by
    /// [`tessellate_realizations`](crate::Engine::tessellate_realizations) and
    /// is **not cleared** by `eval()` or by this function.  Callers that want
    /// `RepresentationWithin` assertions to produce a `Satisfied`/`Violated`
    /// verdict must call `set_capture_repr_tol(true)` followed by
    /// `tessellate_realizations(&compiled)` **before** calling `check()`.
    /// When the map is empty (no prior tessellation, or no OCCT kernel),
    /// `dispatch_constraints` falls through to `Indeterminate` for every
    /// `RepresentationWithin` entry — never a false `Violated` (C1).
    pub fn check(&mut self, module: &CompiledModule) -> CheckResult {
        let eval_result = self.eval(module);
        let mut diagnostics = eval_result.diagnostics;

        // After eval(), eval_state is always Some — unwrap is safe here.
        // NOTE: eval() does NOT clear self.achieved_repr_tol — the map
        // populated by tessellate_realizations() (before this check() call)
        // remains available when dispatch_constraints() reads it for
        // RepresentationWithin interception (type-name-scan fallback path).
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
