// Split from lib.rs (task 2032) — constraints methods.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use reify_compiler::{CompiledConstraint, CompiledModule, CompiledTrait, TopologyTemplate, satisfies_trait_bound};
use reify_core::{
    ConstraintNodeId, Diagnostic, DiagnosticCode, DiagnosticLabel, DimensionVector, Severity,
    SourceSpan, ValueCellId,
};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{
    CompiledExpr, CompiledFunction, ConstraintDiagnostics, ConstraintInput, ConstraintResult,
    DeterminacyState, GeometryHandleId, OptimizedImplInput, PersistentMap, Satisfaction,
    StructureInstanceData, StructureTypeId, Value, ValueMap,
};

use crate::{CheckResult, ConstraintCheckEntry, Engine, EngineError};
use crate::topology_selectors;

// ── DFM auto-measurement types (task 4408 γ) ─────────────────────────────────

/// The process-category kind of a recognized DFM rule, together with the
/// relevant capability threshold (in SI radians).
///
/// Determined by duck-typing the `applies_to` conformer's capability param:
/// if `max_overhang_angle` is present → `Overhang`; if `draft_angle` is
/// present (and `max_overhang_angle` is absent) → `Draft`.
/// Overhang takes precedence when both fields are present.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DfmRuleKind {
    /// Additive-manufacturing overhang check.
    /// `max_angle_rad` is the process's `max_overhang_angle` in radians.
    Overhang { max_angle_rad: f64 },
    /// Mould/die draft-angle check.
    /// `min_draft_rad` is the process's `draft_angle` in radians.
    Draft { min_draft_rad: f64 },
}

/// A fully-parsed DFM rule ready for the auto-measurement pass.
///
/// Produced by [`dfm_rule_spec`] from a `Value::StructureInstance` that
/// conforms (by duck-typing) to the `DFMRule` trait shape.
#[derive(Debug, Clone)]
pub(crate) struct DfmRuleSpec {
    /// The process category and associated capability threshold.
    pub kind: DfmRuleKind,
    /// Live kernel handle for the rule's `subject` Solid, or `None` if the
    /// subject value was not a `Value::GeometryHandle` at check time (C1 guard).
    pub subject_handle: Option<GeometryHandleId>,
    /// The original DFM rule `Value::StructureInstance` (cloned), passed as
    /// `args[0]` to `dfm_diagnose` so it can read the `severity` field.
    pub rule_value: Value,
}

/// Attempt to parse a `Value` as a DFM rule and extract a [`DfmRuleSpec`].
///
/// Recognition (duck-typing, no `type_name` check — conformers keep their own
/// concrete `type_name`):
/// - `v` must be a `Value::StructureInstance`.
/// - Must have a `severity` field that is a `Value::Enum { type_name:
///   "DFMSeverity", .. }` (same shape as `parse_dfm_severity` in dfm.rs).
/// - Must have an `applies_to` field that is itself a `StructureInstance`.
/// - Must have a `subject` field (any value; `None` handle if not a
///   `GeometryHandle`).
///
/// Process category (duck-typing the `applies_to` capability param):
/// - `applies_to.fields["max_overhang_angle"]` is an ANGLE scalar →
///   `Overhang { max_angle_rad }`.  (Checked first; takes precedence.)
/// - `applies_to.fields["draft_angle"]` is an ANGLE scalar →
///   `Draft { min_draft_rad }`.
/// - Neither → `None` (not a DFM rule we handle).
///
/// Returns `None` when the shape doesn't match.
pub(crate) fn dfm_rule_spec(v: &Value) -> Option<DfmRuleSpec> {
    let data = match v {
        Value::StructureInstance(d) => d,
        _ => return None,
    };

    // Require a DFMSeverity `severity` field.
    match data.fields.get("severity") {
        Some(Value::Enum { type_name, .. }) if type_name == "DFMSeverity" => {}
        _ => return None,
    }

    // Require an `applies_to` StructureInstance.
    let applies_to = match data.fields.get("applies_to") {
        Some(Value::StructureInstance(d)) => d,
        _ => return None,
    };

    // Require a `subject` field (value irrelevant for recognition;
    // we use `get` which accepts `&str` via the Borrow bound).
    data.fields.get("subject")?;

    // Extract angle scalar helper: returns si_value if the field is an
    // ANGLE-dimension scalar.
    let angle_si = |fields: &PersistentMap<String, Value>, key: &str| -> Option<f64> {
        match fields.get(key) {
            Some(Value::Scalar { si_value, dimension }) if *dimension == DimensionVector::ANGLE => {
                Some(*si_value)
            }
            _ => None,
        }
    };

    // Determine category: overhang takes precedence over draft.
    let kind = if let Some(max_angle_rad) = angle_si(&applies_to.fields, "max_overhang_angle") {
        DfmRuleKind::Overhang { max_angle_rad }
    } else if let Some(min_draft_rad) = angle_si(&applies_to.fields, "draft_angle") {
        DfmRuleKind::Draft { min_draft_rad }
    } else {
        return None;
    };

    // Extract subject handle (None if not a live GeometryHandle).
    let subject_handle = match data.fields.get("subject") {
        Some(Value::GeometryHandle { kernel_handle, .. }) => Some(*kernel_handle),
        _ => None,
    };

    Some(DfmRuleSpec { kind, subject_handle, rule_value: v.clone() })
}

// ── GD&T callout descriptor (C1, task 4475 β) ───────────────────────────────

/// A single GD&T callout instance enumerated by [`Engine::enumerate_gdt_callouts`].
///
/// Descriptor returned by the C1 enumerator (task 4475 β), reused verbatim by
/// the η conformance pass.
#[derive(Debug, Clone)]
pub struct GdtCallout {
    /// Structure type name (e.g. `"Flatness"`, `"Position"`).
    pub type_name: String,
    /// Instantiation source span (`ValueCellDecl.span`) — the ctor-let site.
    /// Anchor for the B7 "at the instantiation span" diagnostic label.
    pub span: SourceSpan,
    /// `material_condition` field variant, if the field was a concrete
    /// [`Value::Enum`] at eval time (e.g. `Some("MMC")`). `None` when absent
    /// or not a concrete enum value — no-false-positive invariant.
    pub material_condition: Option<String>,
    /// `zone_shape` field variant, if present and concrete (e.g. `Some("Width")`).
    pub zone_shape: Option<String>,
}

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

        // ── Fast path for non-assertion modules (C2) ──────────────────────────
        // When `achieved_repr_tol` is empty (no tessellation has run) AND no
        // optimised impls are registered, we know no entry can be a live
        // `RepresentationWithin` assertion — skip the pre-pass entirely and use
        // the original zero-allocation path.  This covers the universal
        // non-assertion case: every `reify check` call on a module without
        // `RepresentationWithin` constraints, where `cmd_check` never calls
        // `set_capture_repr_tol` / `tessellate_realizations` and the map stays
        // empty.
        if self.achieved_repr_tol.is_empty() && self.optimization_registry.is_empty() {
            let constraints: Vec<(ConstraintNodeId, &CompiledExpr)> = entries
                .into_iter()
                .map(|(id, expr, _target)| (id, expr))
                .collect();
            let input = ConstraintInput {
                constraints: Cow::Owned(constraints),
                values,
                functions,
                determinacy,
            };
            return (self.constraint_checker.check(&input), Vec::new());
        }

        // ── RepresentationWithin interception ─────────────────────────────────
        // Reached only when `achieved_repr_tol` is non-empty (a tessellation
        // ran) or an optimised impl is registered.  Peel RepresentationWithin
        // entries off the batch before bucketing so that they never reach the
        // language-level ConstraintChecker (which has no access to
        // self.achieved_repr_tol).  Each matched entry is evaluated engine-side;
        // unmatched entries go to the existing paths.
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

        // No RepresentationWithin entries found in this batch (achieved_repr_tol
        // is non-empty — tessellation ran — but no entry in this specific batch
        // matched the shape) AND no registered impls.  Take the pass-through
        // path.  Note: rw_slots and rest were allocated by the pre-pass above;
        // the early fast path handles the universal non-assertion case without
        // this overhead.
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

    /// Auto-measurement check-time pass: identify DFM rule structure-instances
    /// by duck-typing (from both top-level templates and sub-component values),
    /// and emit `{W,E}_DFM_OVERHANG` / `_DRAFT` / `E_DFM_UNDERCUT` diagnostics
    /// based on the realized solid's geometry (task 4408 γ).
    ///
    /// # C1 invariant — no false violations
    ///
    /// When no geometry kernel is present (`default_kernel_name` is `None`)
    /// **or** the rule's `subject` did not re-hydrate to a live
    /// `Value::GeometryHandle` (e.g. the module was never `build()`-ed),
    /// the pass emits **nothing** — it is a complete no-op.  This mirrors the
    /// `RepresentationWithin` empty-`achieved_repr_tol` → Indeterminate path.
    ///
    /// # DFMRule discovery — two sources
    ///
    /// **(A) Top-level templates**: for each `module.templates` entry, the
    /// evaluator stores individual field cells (e.g. `OverhangRule.severity`)
    /// but no whole-structure `Value::StructureInstance`.  We synthesize one
    /// from the cells so that `dfm_rule_spec` can duck-type the shape.  This
    /// covers `structure def MyRule : DFMRule { ... }` at the module level.
    ///
    /// **(B) Sub-component instances**: task-3540 (SIR-α) emits a synthetic
    /// `Value::StructureInstance` at `ValueCellId(parent, sub.name)` for every
    /// non-collection sub.  Scanning `values.iter()` catches these, handling
    /// the nested case `structure def Part { let rule = MyRule() }`.
    ///
    /// # Borrow order
    ///
    /// We collect OWNED specs (cloned rule `Value` + `Copy` `GeometryHandleId`)
    /// from `&module` + `&values` first, then borrow `self.geometry_kernels`
    /// mutably.  The two borrows never overlap.
    pub(crate) fn measure_dfm_rules(&mut self, module: &CompiledModule, values: &ValueMap) -> Vec<Diagnostic> {
        // C1 guard: no default kernel → nothing to measure.
        let kernel_name = match self.default_kernel_name.as_deref() {
            Some(n) => n.to_string(),
            None => return Vec::new(),
        };

        // Collect specs with live handles (skip None subject_handle entries).
        let mut specs: Vec<DfmRuleSpec> = Vec::new();

        // (A) Top-level templates — synthesize a StructureInstance from their
        // evaluated cell values so that dfm_rule_spec can duck-type the shape.
        //
        // Geometry cells (e.g. `param subject : Solid = box(...)`) are NOT
        // present as live `Value::GeometryHandle` in `eval_result.values` after
        // a fresh `eval()` call: the kernel is not invoked during pure evaluation
        // (only `build()` calls the kernel and stamps `realization_handles`).
        // To expose live handles, we additionally iterate the template's named
        // realizations and inject the handle from `self.realization_handles` so
        // that `dfm_rule_spec` can find a non-None `subject_handle`.
        for template in &module.templates {
            let mut fields = PersistentMap::new();
            for cell_decl in &template.value_cells {
                if let Some(val) = values.get(&cell_decl.id) {
                    fields.insert(cell_decl.id.member.clone(), val.clone());
                }
            }
            // Override / fill geometry fields from live realization_handles.
            // This is safe because realization_handles is a disjoint Engine
            // field from geometry_kernels — both accessed in sequence, not
            // simultaneously.
            for realization in &template.realizations {
                let Some(ref name) = realization.name else { continue };
                if let Some(&kernel_handle) = self.realization_handles.get(&realization.id) {
                    fields.insert(
                        name.clone(),
                        Value::GeometryHandle {
                            realization_ref: realization.id.clone(),
                            upstream_values_hash: [0u8; 32],
                            kernel_handle,
                        },
                    );
                }
            }
            if !fields.is_empty() {
                let si = Value::StructureInstance(Box::new(StructureInstanceData {
                    type_id: StructureTypeId(0),
                    type_name: template.name.clone(),
                    version: template.version(),
                    fields,
                }));
                if let Some(spec) = dfm_rule_spec(&si)
                    && spec.subject_handle.is_some()
                {
                    specs.push(spec);
                }
            }
        }

        // (B) Sub-component StructureInstance values (task-3540 synthetic cells).
        for (_, v) in values.iter() {
            if let Some(spec) = dfm_rule_spec(v)
                && spec.subject_handle.is_some()
            {
                specs.push(spec);
            }
        }

        // Dedup by (kind, subject_handle) so that a DFMRule which is both
        // discovered via its template definition (source A) and an instantiated
        // sub-component value (source B) emits exactly one diagnostic.  Two
        // specs with the same kind and the same kernel handle would produce
        // identical measurement results — keep the first occurrence only.
        {
            let mut seen: HashSet<(u8, GeometryHandleId)> = HashSet::new();
            specs.retain(|spec| {
                let disc = match &spec.kind {
                    DfmRuleKind::Overhang { .. } => 0u8,
                    DfmRuleKind::Draft { .. } => 1u8,
                };
                seen.insert((disc, spec.subject_handle.expect("filtered above")))
            });
        }

        if specs.is_empty() {
            return Vec::new();
        }

        // Now we can borrow self.geometry_kernels mutably.
        let kernel = match self.geometry_kernels.get_mut(&kernel_name) {
            Some(k) => k.as_mut(),
            None => return Vec::new(),
        };

        let mut diags = Vec::new();
        for spec in specs {
            let handle = spec.subject_handle.expect("filtered above");
            match spec.kind {
                DfmRuleKind::Overhang { max_angle_rad } => {
                    match topology_selectors::unsupported_overhang_faces(
                        kernel,
                        handle,
                        // +Z is the default build direction (PRD §4.4 / §5 / §9 Q2).
                        // A future rule-supplied direction would be threaded in here.
                        [0.0, 0.0, 1.0],
                        max_angle_rad,
                    ) {
                        Ok((faces, _worst_dip)) => {
                            let verdict = Value::Bool(!faces.is_empty());
                            diags.extend(reify_stdlib::dfm_diagnose(
                                "unsupported_overhang_faces",
                                &[spec.rule_value],
                                &verdict,
                            ));
                        }
                        Err(err) => {
                            // Indeterminate — never a false violation (C1).
                            tracing::debug!(
                                ?handle, ?err,
                                "DFM overhang selector error; treating as Indeterminate"
                            );
                        }
                    }
                }
                DfmRuleKind::Draft { min_draft_rad } => {
                    match topology_selectors::min_draft_angle(
                        kernel,
                        handle,
                        // +Z is the assumed pull direction (intentional default; PRD §4.4).
                        // A future rule-supplied direction would be threaded in here.
                        [0.0, 0.0, 1.0],
                    ) {
                        Ok((signed_min_draft, has_undercut)) => {
                            let verdict = Value::List(vec![
                                Value::Bool(signed_min_draft < min_draft_rad),
                                Value::Bool(has_undercut),
                            ]);
                            diags.extend(reify_stdlib::dfm_diagnose(
                                "min_draft_angle",
                                &[spec.rule_value],
                                &verdict,
                            ));
                        }
                        Err(err) => {
                            // Indeterminate — never a false violation (C1).
                            tracing::debug!(
                                ?handle, ?err,
                                "DFM draft selector error; treating as Indeterminate"
                            );
                        }
                    }
                }
            }
        }
        diags
    }

    /// GD&T geometric-conformance measurement pass (task 4480 η, PRD v0_6 C3/C5).
    ///
    /// For every active `Conforms` instance that carries an **explicit** `actual`
    /// argument binding (the η detection signal, captured on
    /// [`CompiledConstraint::arg_bindings`]), measure the deviation of the bound
    /// `actual` geometry from the tolerance's nominal `feature` via
    /// [`reify_ir::GeometryQuery::MaxDeviation`], feed the measured value into the
    /// shipped scalar predicate (`effective_tolerance_zone(...) >= measured`), and
    /// OVERRIDE the matching [`ConstraintCheckEntry`] (by [`ConstraintNodeId`], in
    /// caller order) with the geometric verdict — Satisfied or Violated.
    ///
    /// # C1 invariant — never a false Violated
    ///
    /// When no geometry kernel is present, or the `actual`/`feature` handle is
    /// unrealizable, or the kernel query fails, the verdict is **Indeterminate**
    /// plus a diagnostic — never a (false) Violated. This is exactly why the
    /// geometry stays in this check-time pass and out of the constraint body
    /// (which evaluates in the kernel-less P1 phase): the structural shape that
    /// sidesteps the trap that blocked #4275.
    ///
    /// # B4 — scalar path untouched
    ///
    /// A `Conforms` with no explicit `actual` (its `nominal()` default) is never
    /// touched: its scalar `ConstraintCheckEntry` is left exactly as
    /// [`check_constraints_against_templates`](Self::check_constraints_against_templates)
    /// produced it. Modules with no explicit-`actual` Conforms hit the fast
    /// no-op early-return and are byte-identical.
    ///
    /// # Borrow order
    ///
    /// Phase 1 resolves all specs from `&module` + `values` + immutable `self`
    /// (prelude/functions/`realization_handles`) into owned [`GdtConformanceWork`];
    /// phase 2 borrows `self.geometry_kernels` immutably to run the queries. The
    /// two borrow regions never overlap. `achieved_repr_tol` and DFM state are
    /// never touched.
    pub(crate) fn measure_gdt_conformance(
        &mut self,
        module: &CompiledModule,
        values: &ValueMap,
        constraint_results: &mut Vec<ConstraintCheckEntry>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Fast no-op for non-GD&T modules (B4 / C2): keep every module without an
        // explicit-`actual` Conforms byte-identical and allocation-free.
        let has_geometric_conforms = module.templates.iter().any(|t| {
            let top = t.constraints.iter();
            let guarded = t
                .guarded_groups
                .iter()
                .flat_map(|g| g.constraints.iter().chain(g.else_constraints.iter()));
            top.chain(guarded)
                .any(|c| c.arg_bindings.iter().any(|(n, _)| n == "actual"))
        });
        if !has_geometric_conforms {
            return;
        }

        // ── Phase 1: resolve specs (immutable prelude/functions/handles borrows) ──
        let work: Vec<GdtConformanceWork> = {
            // Trait + template registries (prelude + module), mirroring
            // `enumerate_gdt_callouts` so the GeometricTolerance walk is identical.
            let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
            for prelude_mod in self.prelude {
                for t in &prelude_mod.trait_defs {
                    trait_registry.insert(t.name.clone(), t);
                }
            }
            for t in &module.trait_defs {
                trait_registry.insert(t.name.clone(), t);
            }
            let mut template_by_name: HashMap<&str, &TopologyTemplate> = HashMap::new();
            for prelude_mod in self.prelude {
                for t in &prelude_mod.templates {
                    template_by_name.insert(t.name.as_str(), t);
                }
            }
            for t in &module.templates {
                template_by_name.insert(t.name.as_str(), t);
            }

            let ctx = EvalContext::new(values, &self.functions);
            let realization_handles = &self.realization_handles;
            // Resolve a `Value::GeometryHandle` to a live kernel handle: prefer the
            // post-build realization bridge (the authoritative live handle, exactly
            // like `measure_dfm_rules`), else the value's own kernel handle.
            let resolve_handle = |v: &Value| -> Option<GeometryHandleId> {
                let Value::GeometryHandle { kernel_handle, realization_ref, .. } = v else {
                    return None;
                };
                if let Some(h) = realization_handles.get(realization_ref).copied() {
                    return Some(h);
                }
                if *kernel_handle != GeometryHandleId::INVALID {
                    return Some(*kernel_handle);
                }
                None
            };

            let mut work = Vec::new();
            for template in &module.templates {
                for c in Self::collect_active_constraints(template, values) {
                    // η detection signal: an EXPLICIT `actual` binding. The Conforms
                    // predicate never references `actual`, so this binding (not the
                    // body) is the only trace of geometric intent.
                    if !c.arg_bindings.iter().any(|(n, _)| n == "actual") {
                        continue;
                    }
                    let binding = |name: &str| {
                        c.arg_bindings.iter().find(|(n, _)| n == name).map(|(_, e)| e)
                    };
                    let (Some(tol_expr), Some(act_expr)) =
                        (binding("tolerance"), binding("actual"))
                    else {
                        // An `actual` with no `tolerance` is not a Conforms callout.
                        continue;
                    };

                    let resolution = (|| {
                        // Resolve `tolerance` → a GeometricTolerance StructureInstance.
                        let tol_val = eval_expr(tol_expr, &ctx);
                        let data = match &tol_val {
                            Value::StructureInstance(d) => d,
                            _ => {
                                return GdtConformanceResolution::Indeterminate(
                                    "`tolerance` did not resolve to a GeometricTolerance instance"
                                        .to_string(),
                                );
                            }
                        };
                        let conforms = template_by_name
                            .get(data.type_name.as_str())
                            .map(|t| {
                                satisfies_trait_bound(
                                    &t.trait_bounds,
                                    "GeometricTolerance",
                                    &trait_registry,
                                )
                            })
                            .unwrap_or(false);
                        if !conforms {
                            return GdtConformanceResolution::Indeterminate(format!(
                                "`tolerance` type `{}` is not a GeometricTolerance",
                                data.type_name
                            ));
                        }
                        // Nominal feature handle (read off the tolerance instance).
                        let feature = match data.fields.get("feature").and_then(&resolve_handle) {
                            Some(h) => h,
                            None => {
                                return GdtConformanceResolution::Indeterminate(
                                    "could not resolve the nominal `feature` geometry handle"
                                        .to_string(),
                                );
                            }
                        };
                        // Actual (measured) geometry handle.
                        let actual_val = eval_expr(act_expr, &ctx);
                        let actual = match resolve_handle(&actual_val) {
                            Some(h) => h,
                            None => {
                                return GdtConformanceResolution::Indeterminate(
                                    "could not resolve the `actual` geometry handle".to_string(),
                                );
                            }
                        };
                        // Tolerance zone via the SHIPPED scalar predicate's helper —
                        // feed, not replace, `effective_tolerance_zone` (D3).
                        let tol_value =
                            data.fields.get("tolerance_value").cloned().unwrap_or(Value::Undef);
                        let material_condition = data
                            .fields
                            .get("material_condition")
                            .cloned()
                            .unwrap_or(Value::Enum {
                                type_name: "MaterialCondition".to_string(),
                                variant: "RFS".to_string(),
                            });
                        let feature_departure = binding("feature_departure")
                            .map(|e| eval_expr(e, &ctx))
                            .unwrap_or(Value::Scalar {
                                si_value: 0.0,
                                dimension: DimensionVector::LENGTH,
                            });
                        let zone = reify_stdlib::eval_builtin(
                            "effective_tolerance_zone",
                            &[tol_value, material_condition, feature_departure],
                        );
                        let zone_m = match zone {
                            Value::Scalar { si_value, .. }
                                if si_value.is_finite() && si_value >= 0.0 =>
                            {
                                si_value
                            }
                            _ => {
                                return GdtConformanceResolution::Indeterminate(
                                    "could not compute the effective tolerance zone".to_string(),
                                );
                            }
                        };
                        GdtConformanceResolution::Resolved { actual, feature, zone_m }
                    })();

                    work.push(GdtConformanceWork {
                        id: c.id.clone(),
                        span: c.span,
                        resolution,
                    });
                }
            }
            work
        };

        if work.is_empty() {
            return;
        }

        // ── Phase 2: query the kernel + weave verdicts (immutable kernel borrow) ──
        let kernel = self
            .default_kernel_name
            .as_deref()
            .and_then(|n| self.geometry_kernels.get(n));

        for w in work {
            let (satisfaction, diag): (Satisfaction, Option<Diagnostic>) = match w.resolution {
                GdtConformanceResolution::Indeterminate(reason) => (
                    Satisfaction::Indeterminate,
                    Some(gdt_indeterminate_diag(w.span, &reason)),
                ),
                GdtConformanceResolution::Resolved { actual, feature, zone_m } => match &kernel {
                    None => (
                        Satisfaction::Indeterminate,
                        Some(gdt_indeterminate_diag(
                            w.span,
                            "no geometry kernel available to measure the `actual` deviation \
                             against the nominal feature",
                        )),
                    ),
                    Some(k) => {
                        let query = reify_ir::GeometryQuery::MaxDeviation {
                            actual,
                            nominal: feature,
                            tolerance: GDT_CONFORMANCE_TESSELLATION_TOLERANCE_M,
                        };
                        match k.query(&query) {
                            Ok(reply) => match measured_deviation_m(&reply) {
                                Some(measured_m) => gdt_verdict(zone_m, measured_m, w.span),
                                None => (
                                    Satisfaction::Indeterminate,
                                    Some(gdt_indeterminate_diag(
                                        w.span,
                                        &format!(
                                            "geometry kernel returned an unusable MaxDeviation \
                                             reply ({reply:?})"
                                        ),
                                    )),
                                ),
                            },
                            Err(err) => (
                                Satisfaction::Indeterminate,
                                Some(gdt_indeterminate_diag(
                                    w.span,
                                    &format!("geometry kernel MaxDeviation query failed: {err}"),
                                )),
                            ),
                        }
                    }
                },
            };

            // Weave: OVERRIDE the matching entry in caller order; push if absent
            // (defensive — the scalar path normally pre-populates it).
            if let Some(entry) = constraint_results.iter_mut().find(|e| e.id == w.id) {
                entry.satisfaction = satisfaction;
            } else {
                constraint_results.push(ConstraintCheckEntry {
                    id: w.id.clone(),
                    label: Some("Conforms".to_string()),
                    satisfaction,
                });
            }
            if let Some(d) = diag {
                diagnostics.push(d);
            }
        }
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
    ///
    /// ## DFM auto-measurement (task-4408 γ)
    ///
    /// After `check_constraints_against_templates`, `measure_dfm_rules` scans
    /// the evaluated values for `DFMRule` structure-instances and emits
    /// `{W,E}_DFM_OVERHANG` / `_DRAFT` / `E_DFM_UNDERCUT` diagnostics when a
    /// live OCCT kernel is present and the rule's `subject` was realized by a
    /// prior `build()` call.  No kernel or un-realized subject → no-op (C1).
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

        // DFM auto-measurement pass (task 4408 γ).
        // eval_result.values is a separate owned ValueMap — collect DFM specs
        // from module + values before the mutable self.geometry_kernels borrow in
        // measure_dfm_rules (no borrow conflict).
        let dfm_diags = self.measure_dfm_rules(module, &eval_result.values);
        diagnostics.extend(dfm_diags);

        // ── C2 GD&T legality pass (task 4475 β) ──────────────────────────────
        // Delegated to `run_gdt_check_passes` so the CLI `--purpose` branch can
        // call the same shared seam without going through `Engine::check`.
        diagnostics.extend(self.run_gdt_check_passes(module, &eval_result.values));


        CheckResult {
            values: eval_result.values,
            constraint_results,
            diagnostics,
            resolved_params: eval_result.resolved_params,
        }
    }

    // ── C1 enumerator (task 4475 β) ──────────────────────────────────────────

    /// Enumerate all GD&T callout instances in the module.
    ///
    /// A callout is a [`Value::StructureInstance`] in the post-eval `values` map
    /// whose declared structure template transitively conforms to the
    /// `GeometricTolerance` trait (via the canonical `satisfies_trait_bound` walk
    /// over `module.trait_defs`).
    ///
    /// Returned in declaration order (template order, then value-cell order within
    /// each template).  Dead / guard-inactive branches are naturally skipped because
    /// they produce no live `Value::StructureInstance` in `values`.
    ///
    /// # No-false-positive invariant
    /// If a modifier field (`material_condition`, `zone_shape`) is not a concrete
    /// [`Value::Enum`] at eval time, its slot in the returned [`GdtCallout`] is
    /// `None` — the enumerator never emits on an indeterminate value.  This mirrors
    /// the `RepresentationWithin` C1 invariant at `engine_constraints.rs:42-62`.
    ///
    /// # Fast-path
    /// After building the trait/template registries, returns an empty `Vec`
    /// immediately if no evaluated `Value::StructureInstance` in `values` conforms
    /// to `GeometricTolerance`.  This is a real guard: the prelude always contains
    /// GeometricTolerance-conforming *templates* (Flatness, Position, etc.), so a
    /// template-only check would be vacuously true for every module.  Checking
    /// *instance values* instead correctly fast-paths non-GD&T modules that happen
    /// to load the stdlib tolerancing prelude.
    ///
    /// # Reuse contract (C1 — task 4475 β)
    /// This function is the *shared* enumerator: it is consumed unchanged by
    /// `check_gdt_legality` (β) and will be reused verbatim by the η conformance
    /// pass.  Do not add β-specific logic here.
    pub fn enumerate_gdt_callouts(
        &self,
        module: &CompiledModule,
        values: &ValueMap,
    ) -> Vec<GdtCallout> {
        // Build trait registry from prelude trait_defs + user module trait_defs.
        // Stdlib traits like GeometricTolerance live in the prelude, not in
        // module.trait_defs (which only holds user-defined traits).
        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        for prelude_mod in self.prelude {
            for t in &prelude_mod.trait_defs {
                trait_registry.insert(t.name.clone(), t);
            }
        }
        for t in &module.trait_defs {
            trait_registry.insert(t.name.clone(), t);
        }

        // Build combined template lookup from prelude + user templates.
        // Stdlib types (Flatness, Position, etc.) live in the prelude, not in
        // module.templates (which only holds user-defined structure templates).
        let mut template_by_name: HashMap<&str, &TopologyTemplate> = HashMap::new();
        for prelude_mod in self.prelude {
            for t in &prelude_mod.templates {
                template_by_name.insert(t.name.as_str(), t);
            }
        }
        for t in &module.templates {
            template_by_name.insert(t.name.as_str(), t);
        }

        // Fast-path: if no evaluated StructureInstance value conforms to
        // GeometricTolerance, there are no GD&T callouts to collect.
        // Checking values (not templates) is correct: the prelude always carries
        // GeometricTolerance-conforming templates, so a template-only check would
        // be vacuously true for every module that loads the tolerancing stdlib.
        let has_gdt_instance = values.iter().any(|(_, v)| match v {
            Value::StructureInstance(data) => template_by_name
                .get(data.type_name.as_str())
                .map(|t| satisfies_trait_bound(&t.trait_bounds, "GeometricTolerance", &trait_registry))
                .unwrap_or(false),
            _ => false,
        });
        if !has_gdt_instance {
            return Vec::new();
        }

        let mut callouts = Vec::new();

        // Walk templates in declaration order, then value_cells in declaration order.
        for template in &module.templates {
            for cell in &template.value_cells {
                // Look up the evaluated value for this cell.
                let value = match values.get(&cell.id) {
                    Some(v) => v,
                    None => continue,
                };

                // Only care about StructureInstances.
                let instance_data = match value {
                    Value::StructureInstance(data) => data,
                    _ => continue,
                };

                // Look up the instance's template to check trait bounds.
                let instance_tmpl = match template_by_name.get(instance_data.type_name.as_str()) {
                    Some(t) => t,
                    None => continue, // unknown type — skip; no false positive
                };

                // Check if the instance conforms to GeometricTolerance.
                if !satisfies_trait_bound(
                    &instance_tmpl.trait_bounds,
                    "GeometricTolerance",
                    &trait_registry,
                ) {
                    continue;
                }

                // Extract modifier fields; skip (None) if not a concrete enum.
                let material_condition =
                    enum_field_variant(&instance_data.fields, "material_condition");
                let zone_shape = enum_field_variant(&instance_data.fields, "zone_shape");

                callouts.push(GdtCallout {
                    type_name: instance_data.type_name.clone(),
                    span: cell.span,
                    material_condition,
                    zone_shape,
                });
            }
        }

        callouts
    }

    // ── C2 rule table (task 4475 β) ──────────────────────────────────────────

    /// Shared aggregation point for all purpose-independent, kernel-free static
    /// GD&T check passes (task 4589).
    ///
    /// **Both** `Engine::check` and the CLI `--purpose` branch call this method,
    /// so any static GD&T pass added here is automatically enforced on both paths.
    /// Future passes to add here:
    /// - task 4480 η conformance pass
    /// - kappa DRF seam pass
    ///
    /// **Intentionally excluded:** the DFM measurement pass (`measure_dfm_rules`)
    /// takes `&mut self`, requires a live geometry kernel, and belongs to the
    /// build/geometry path — not the lightweight static lint path.
    ///
    /// Delegates to [`check_gdt_legality`] unchanged; no rule-table edits.
    pub fn run_gdt_check_passes(
        &self,
        module: &CompiledModule,
        values: &ValueMap,
    ) -> Vec<Diagnostic> {
        self.check_gdt_legality(module, values)
    }

    /// Apply the GD&T legality rule table to all callouts in `module`.
    ///
    /// Returns zero or more diagnostics:
    /// - [`DiagnosticCode::GdtIllegalModifier`] (Error) for any callout whose
    ///   characteristic family is RFS-only but carries `MMC` or `LMC`.
    /// - [`DiagnosticCode::GdtRemoved2018`] (Warning) for `Concentricity` /
    ///   `Symmetry` (added in step-8).
    ///
    /// Unknown user-defined `GeometricTolerance` subtypes are silently skipped
    /// (no false error).
    ///
    /// Fast-path: delegates to `enumerate_gdt_callouts`, which returns empty for
    /// modules with no `GeometricTolerance`-conforming templates — keeping every
    /// non-GD&T `check()` byte-identical.
    pub(crate) fn check_gdt_legality(
        &self,
        module: &CompiledModule,
        values: &ValueMap,
    ) -> Vec<Diagnostic> {
        let callouts = self.enumerate_gdt_callouts(module, values);
        if callouts.is_empty() {
            return Vec::new();
        }

        let mut diags = Vec::new();

        for callout in &callouts {
            classify_callout(callout, &mut diags);
        }

        diags
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

// ── η/4480 GD&T conformance pass helpers ─────────────────────────────────────

/// Tessellation deflection forwarded to [`reify_ir::GeometryQuery::MaxDeviation`]'s
/// `tolerance` by [`Engine::measure_gdt_conformance`]. Mirrors
/// `geometry_ops::MAX_DEVIATION_TESSELLATION_TOLERANCE_M` (= 0.0001 m) and
/// `Engine::DEFAULT_TESSELLATION_TOLERANCE`; the matching value is what
/// `MockGeometryKernel::with_max_deviation_result` keys on in the η unit tests.
const GDT_CONFORMANCE_TESSELLATION_TOLERANCE_M: f64 = 0.0001;

/// Outcome of resolving a geometric `Conforms` instance during phase 1 of
/// [`Engine::measure_gdt_conformance`], before the kernel is queried.
enum GdtConformanceResolution {
    /// Both handles + the tolerance zone resolved; ready for a MaxDeviation query.
    Resolved {
        actual: GeometryHandleId,
        feature: GeometryHandleId,
        zone_m: f64,
    },
    /// Resolution failed (non-conforming tolerance, unrealizable handle, …) —
    /// the verdict is Indeterminate with this reason (C1: never a false Violated).
    Indeterminate(String),
}

/// A single geometric `Conforms` instance to weave back into the check results.
struct GdtConformanceWork {
    id: ConstraintNodeId,
    span: SourceSpan,
    resolution: GdtConformanceResolution,
}

/// Extract a non-negative, finite metres deviation from a MaxDeviation kernel
/// reply (`Value::Real`, or defensively `Value::Scalar`). `None` for any other
/// or degenerate reply — the caller maps `None` to Indeterminate (C1).
fn measured_deviation_m(reply: &Value) -> Option<f64> {
    match reply {
        Value::Real(v) if v.is_finite() && *v >= 0.0 => Some(*v),
        Value::Scalar { si_value, .. } if si_value.is_finite() && *si_value >= 0.0 => {
            Some(*si_value)
        }
        _ => None,
    }
}

/// Decide a geometric Conforms verdict from the tolerance zone and measured
/// deviation (both SI metres): `zone >= measured` → Satisfied (no diagnostic);
/// else Violated with a diagnostic carrying the measured magnitude + zone width
/// (both in mm). Mirrors the shipped scalar predicate `effective_tolerance_zone(...)
/// >= measured_deviation`.
fn gdt_verdict(zone_m: f64, measured_m: f64, span: SourceSpan) -> (Satisfaction, Option<Diagnostic>) {
    if zone_m >= measured_m {
        return (Satisfaction::Satisfied, None);
    }
    let measured_mm = measured_m * 1e3;
    let zone_mm = zone_m * 1e3;
    (
        Satisfaction::Violated,
        Some(
            Diagnostic::error(format!(
                "Conforms VIOLATED: measured deviation {measured_mm:.4} mm exceeds the \
                 {zone_mm:.4} mm tolerance zone"
            ))
            .with_code(DiagnosticCode::ConstraintViolated)
            .with_label(DiagnosticLabel::new(span, "geometric conformance violated")),
        ),
    )
}

/// Build the Indeterminate diagnostic for a geometric Conforms that could not be
/// measured (missing kernel, unrealizable handle, kernel error). Warning, not
/// error — Indeterminate never fails the check (C1).
fn gdt_indeterminate_diag(span: SourceSpan, reason: &str) -> Diagnostic {
    Diagnostic::warning(format!("Conforms INDETERMINATE: {reason}"))
        .with_code(DiagnosticCode::ConstraintIndeterminate)
        .with_label(DiagnosticLabel::new(
            span,
            "geometric conformance could not be measured",
        ))
}

/// Extract a `Value::Enum` variant string from a `StructureInstanceData.fields` map.
/// Returns `None` if the key is absent or the value is not a concrete `Value::Enum`.
/// No-false-positive invariant: only concrete enum values produce `Some(...)`.
fn enum_field_variant(fields: &PersistentMap<String, Value>, field_name: &str) -> Option<String> {
    match fields.get(&field_name.to_string()) {
        Some(Value::Enum { variant, .. }) => Some(variant.clone()),
        _ => None,
    }
}

/// Returns `true` if `modifier` is `MMC` or `LMC` (i.e. not RFS and not unknown).
#[inline]
fn is_non_rfs(modifier: Option<&str>) -> bool {
    matches!(modifier, Some("MMC") | Some("LMC"))
}

/// Classify a single GDT callout against the C2 rule table and push diagnostics.
///
/// Family classification (β legality table):
/// - Form (Flatness, Straightness, Circularity, Cylindricity): RFS-only.
/// - FormAxis (StraightnessOfAxis): MMC-eligible (FOS axis variant).
/// - Orientation (Parallelism, Perpendicularity, Angularity): MMC-eligible iff
///   zone_shape == Cylindrical; Width zone is RFS-only.
/// - Location Position: MMC-eligible (default Cylindrical zone).
/// - Removed (Concentricity, Symmetry): emits GdtRemoved2018 warning (step-8).
/// - Runout (CircularRunout, TotalRunout): RFS-only.
/// - Profile (ProfileOfSurface, ProfileOfLine, …Related): RFS-only.
///
/// Unknown user-defined GeometricTolerance subtypes → skip (no false error).
fn classify_callout(callout: &GdtCallout, diags: &mut Vec<Diagnostic>) {
    let mc = callout.material_condition.as_deref();

    match callout.type_name.as_str() {
        // ── RFS-only Form family ───────────────────────────────────────────
        "Flatness" | "Straightness" | "Circularity" | "Cylindricity" => {
            if is_non_rfs(mc) {
                diags.push(illegal_modifier_error(callout));
            }
        }

        // ── FOS-axis Form variant: MMC-eligible unconditionally ───────────
        "StraightnessOfAxis" => {
            // MMC/LMC is permitted on the FOS derived median line.
        }

        // ── Orientation: MMC-eligible only with Cylindrical zone ──────────
        "Parallelism" | "Perpendicularity" | "Angularity" => {
            let cylindrical = callout.zone_shape.as_deref() == Some("Cylindrical");
            if is_non_rfs(mc) && !cylindrical {
                diags.push(illegal_modifier_error(callout));
            }
        }

        // ── Location Position: MMC-eligible (default Cylindrical zone) ────
        "Position" => {
            // Cylindrical zone (the default) makes this FOS-eligible — permit MMC/LMC.
        }

        // ── Removed-in-2018 family ────────────────────────────────────────
        // Concentricity and Symmetry were removed from ASME Y14.5-2018.
        // Emit a warning unconditionally (independent of material_condition);
        // suppress GdtIllegalModifier so an MMC callout yields only this warning.
        "Concentricity" | "Symmetry" => {
            diags.push(
                Diagnostic::warning(format!(
                    "`{}` was removed in ASME Y14.5-2018; \
                     use Position, ProfileOfSurface, or Runout instead",
                    callout.type_name
                ))
                .with_code(DiagnosticCode::GdtRemoved2018)
                .with_label(DiagnosticLabel::new(
                    callout.span,
                    "removed in ASME Y14.5-2018",
                )),
            );
        }

        // ── RFS-only Runout family ────────────────────────────────────────
        "CircularRunout" | "TotalRunout" => {
            if is_non_rfs(mc) {
                diags.push(illegal_modifier_error(callout));
            }
        }

        // ── RFS-only Profile family ───────────────────────────────────────
        "ProfileOfSurface"
        | "ProfileOfLine"
        | "ProfileOfSurfaceRelated"
        | "ProfileOfLineRelated"
            if is_non_rfs(mc) => {
            diags.push(illegal_modifier_error(callout));
        }

        // Unknown user-defined GeometricTolerance subtypes → no opinion.
        _ => {}
    }
}

/// Build a `GdtIllegalModifier` error diagnostic anchored at `callout.span`.
fn illegal_modifier_error(callout: &GdtCallout) -> Diagnostic {
    Diagnostic::error(format!(
        "`{}` is an RFS-only tolerance characteristic; \
         material condition modifiers (MMC/LMC) are not permitted",
        callout.type_name
    ))
    .with_code(DiagnosticCode::GdtIllegalModifier)
    .with_label(DiagnosticLabel::new(
        callout.span,
        "illegal material condition modifier applied here",
    ))
}

// ── Unit tests for DFM auto-measurement helpers ───────────────────────────────

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;
    use reify_core::identity::RealizationNodeId;
    use reify_ir::geometry::GeometryHandleId;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

    use super::{DfmRuleKind, dfm_rule_spec};

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Build a minimal DFMSeverity enum variant.
    fn severity_warning() -> Value {
        Value::Enum {
            type_name: "DFMSeverity".to_string(),
            variant: "Warning".to_string(),
        }
    }

    /// Build a `Value::StructureInstance` with the given field pairs.
    fn structure(type_name: &str, pairs: &[(&str, Value)]) -> Value {
        let fields: PersistentMap<String, Value> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: type_name.to_string(),
            version: 1,
            fields,
        }))
    }

    /// Build an ANGLE scalar from a value in radians.
    fn angle(radians: f64) -> Value {
        Value::Scalar { si_value: radians, dimension: DimensionVector::ANGLE }
    }

    /// Build a dummy `Value::GeometryHandle` with the given kernel handle id.
    fn geometry_handle(kernel_id: u64) -> Value {
        Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("TestPart", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: GeometryHandleId(kernel_id),
        }
    }

    // ── step-1: dfm_rule_spec recognises Overhang branch ─────────────────────

    /// A well-formed DFMRule-shaped StructureInstance with `applies_to` carrying
    /// `max_overhang_angle` should be recognized as `Overhang`.
    #[test]
    fn step1_dfm_rule_spec_overhang_recognised() {
        let max_angle_rad = std::f64::consts::FRAC_PI_4; // 45 deg

        // applies_to: an Adding-like process with max_overhang_angle
        let applies_to = structure("AddingProc", &[
            ("max_overhang_angle", angle(max_angle_rad)),
        ]);

        // subject: a live GeometryHandle
        let kernel_id = 42u64;
        let subj = geometry_handle(kernel_id);

        let rule = structure("MyAddingRule", &[
            ("rule_name", Value::String("overhang-check".to_string())),
            ("severity", severity_warning()),
            ("applies_to", applies_to),
            ("subject", subj),
        ]);

        let spec = dfm_rule_spec(&rule).expect("expected Some(DfmRuleSpec)");

        assert_eq!(spec.kind, DfmRuleKind::Overhang { max_angle_rad });
        assert_eq!(spec.subject_handle, Some(GeometryHandleId(kernel_id)));
    }

    /// A StructureInstance missing the `subject` field returns None.
    #[test]
    fn step1_dfm_rule_spec_missing_subject_none() {
        let applies_to = structure("AddingProc", &[
            ("max_overhang_angle", angle(0.5)),
        ]);
        let rule = structure("MyRule", &[
            ("severity", severity_warning()),
            ("applies_to", applies_to),
            // no "subject"
        ]);
        assert!(dfm_rule_spec(&rule).is_none(), "missing subject should return None");
    }

    /// A StructureInstance missing a DFMSeverity `severity` field returns None.
    #[test]
    fn step1_dfm_rule_spec_missing_severity_none() {
        let applies_to = structure("AddingProc", &[
            ("max_overhang_angle", angle(0.5)),
        ]);
        let rule = structure("MyRule", &[
            ("applies_to", applies_to),
            ("subject", geometry_handle(1)),
            // no "severity"
        ]);
        assert!(dfm_rule_spec(&rule).is_none(), "missing severity should return None");
    }

    // ── step-3: dfm_rule_spec Draft branch + no-handle path ──────────────────

    /// Draft branch: applies_to has draft_angle but NO max_overhang_angle.
    /// subject = Value::Undef → subject_handle == None.
    #[test]
    fn step3_dfm_rule_spec_draft_recognised_no_handle() {
        let min_draft_rad = 0.05235987756; // ~3 deg

        let applies_to = structure("FormingProc", &[
            ("draft_angle", angle(min_draft_rad)),
        ]);
        let rule = structure("MyFormingRule", &[
            ("rule_name", Value::String("draft-check".to_string())),
            ("severity", severity_warning()),
            ("applies_to", applies_to),
            ("subject", Value::Undef), // no live handle
        ]);

        let spec = dfm_rule_spec(&rule).expect("expected Some(DfmRuleSpec) for Draft rule");
        assert_eq!(
            spec.kind,
            DfmRuleKind::Draft { min_draft_rad },
            "should be Draft with the correct angle"
        );
        assert_eq!(spec.subject_handle, None, "Undef subject → None handle");
    }

    /// When applies_to has BOTH max_overhang_angle and draft_angle,
    /// Overhang takes precedence.
    #[test]
    fn step3_dfm_rule_spec_overhang_takes_precedence_over_draft() {
        let max_angle_rad = std::f64::consts::FRAC_PI_4; // 45 deg
        let draft_angle_rad = 0.05235987756; // 3 deg

        let applies_to = structure("BothCapabilityProc", &[
            ("max_overhang_angle", angle(max_angle_rad)),
            ("draft_angle", angle(draft_angle_rad)),
        ]);
        let rule = structure("BothRule", &[
            ("severity", severity_warning()),
            ("applies_to", applies_to),
            ("subject", geometry_handle(7)),
        ]);

        let spec = dfm_rule_spec(&rule).expect("expected Some(DfmRuleSpec)");
        assert_eq!(
            spec.kind,
            DfmRuleKind::Overhang { max_angle_rad },
            "Overhang should take precedence when both fields are present"
        );
    }

    /// applies_to with NEITHER capability param → None.
    #[test]
    fn step3_dfm_rule_spec_no_capability_none() {
        let applies_to = structure("GenericProc", &[
            ("duration", Value::Scalar {
                si_value: 3600.0,
                dimension: DimensionVector::TIME,
            }),
        ]);
        let rule = structure("NoCapRule", &[
            ("severity", severity_warning()),
            ("applies_to", applies_to),
            ("subject", geometry_handle(1)),
        ]);
        assert!(dfm_rule_spec(&rule).is_none(), "no capability param → None");
    }
}

// ── η/4480 step-9: measure_gdt_conformance core logic (MockGeometryKernel) ─────
//
// Unit tests for `Engine::measure_gdt_conformance` — the check-time GD&T
// conformance measurement pass (PRD v0_6 task η, C3/C5).  Driven by a
// `MockGeometryKernel` (`with_max_deviation_result`) so the Satisfied / Violated
// / Indeterminate / weave logic is exercised deterministically without OCCT.
//
// Cases:
//   (a) explicit `actual` + kernel deviation 0.5mm vs 0.1mm zone → Violated,
//       diagnostic carries the measured magnitude + the zone width.
//   (b) deviation 0mm vs 0.1mm zone → Satisfied.
//   (c) explicit `actual` + NO kernel → Indeterminate + missing-kernel
//       diagnostic, never a (false) Violated (C1 invariant).
//   (d) Conforms with NO explicit `actual` → the scalar ConstraintCheckEntry is
//       left untouched (no override, no geometric diagnostic) (B4).
//
// Results are woven back by matching `ConstraintNodeId` in caller order: the
// pass OVERRIDES the existing entry for the geometric Conforms only.
#[cfg(test)]
mod gdt_conformance_tests {
    use reify_compiler::{CompiledConstraint, CompiledModule};
    use reify_constraints::SimpleConstraintChecker;
    use reify_core::DimensionVector;
    use reify_core::identity::{RealizationNodeId, ValueCellId};
    use reify_ir::{
        CompiledExprKind, GeometryHandleId, PersistentMap, Satisfaction, StructureInstanceData,
        StructureTypeId, Value, ValueMap,
    };
    use reify_test_support::{MockGeometryKernel, parse_and_compile_with_stdlib};

    use crate::{ConstraintCheckEntry, Engine};

    /// A geometric Conforms: `actual` is explicitly bound (the η detection
    /// signal). Both `tolerance` and `actual` are bare param refs, so they
    /// compile to `ValueRef` arg-bindings the test can resolve by cell id.
    const GEOMETRIC_SOURCE: &str = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    param act : Geometry = box(1mm, 1mm, 1mm)
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)
}
"#;

    /// A scalar Conforms: `actual` is omitted (falls to its `nominal()` default),
    /// so the pass must leave its scalar verdict untouched (B4).
    const SCALAR_SOURCE: &str = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm)
}
"#;

    /// Find the single Conforms instance in `Probe` (recognised by binding
    /// `tolerance`). There is exactly one per fixture.
    fn find_conforms(module: &CompiledModule) -> &CompiledConstraint {
        module
            .templates
            .iter()
            .find(|t| t.name == "Probe")
            .expect("Probe template")
            .constraints
            .iter()
            .find(|c| c.arg_bindings.iter().any(|(n, _)| n == "tolerance"))
            .expect("Conforms instance binding `tolerance`")
    }

    /// Extract the `ValueCellId` that the named arg-binding references
    /// (the call-site arg compiles to a `ValueRef` for a bare param ref).
    fn ref_cell(cc: &CompiledConstraint, name: &str) -> ValueCellId {
        let (_, expr) = cc
            .arg_bindings
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("arg-binding `{name}` not captured"));
        match &expr.kind {
            CompiledExprKind::ValueRef(id) => id.clone(),
            other => panic!("expected ValueRef for arg `{name}`, got {other:?}"),
        }
    }

    /// A `Value::GeometryHandle` carrying a *valid* kernel handle (so the pass
    /// resolves it directly — the realization-bridge path is covered by the
    /// OCCT CLI test).
    fn handle_value(kernel: GeometryHandleId) -> Value {
        Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("Probe", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: kernel,
        }
    }

    /// A Flatness (GeometricTolerance-conforming) StructureInstance with the
    /// given tolerance zone (metres), RFS material condition, and feature handle.
    fn flatness_value(tolerance_value_m: f64, feature: GeometryHandleId) -> Value {
        let mut fields: PersistentMap<String, Value> = PersistentMap::new();
        fields.insert(
            "tolerance_value".to_string(),
            Value::Scalar { si_value: tolerance_value_m, dimension: DimensionVector::LENGTH },
        );
        fields.insert(
            "material_condition".to_string(),
            Value::Enum {
                type_name: "MaterialCondition".to_string(),
                variant: "RFS".to_string(),
            },
        );
        fields.insert("feature".to_string(), handle_value(feature));
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "Flatness".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Build the value map binding the geometric Conforms's `tolerance` cell to a
    /// Flatness (0.1mm zone, RFS, `feature`) and its `actual` cell to `actual`.
    fn geometric_values(
        conforms: &CompiledConstraint,
        actual: GeometryHandleId,
        feature: GeometryHandleId,
    ) -> ValueMap {
        let mut values = ValueMap::new();
        values.insert(ref_cell(conforms, "tolerance"), flatness_value(1e-4, feature));
        values.insert(ref_cell(conforms, "actual"), handle_value(actual));
        values
    }

    /// (a) Explicit actual, measured 0.5mm > 0.1mm zone → Violated + diagnostic
    /// carrying the measured magnitude (0.5mm) and the zone width (0.1mm).
    #[test]
    fn explicit_actual_deviation_exceeds_zone_is_violated() {
        let module = parse_and_compile_with_stdlib(GEOMETRIC_SOURCE);
        let conforms = find_conforms(&module);
        let node_id = conforms.id.clone();
        let label = conforms.label.clone();

        let actual = GeometryHandleId(202);
        let feature = GeometryHandleId(101);
        let values = geometric_values(conforms, actual, feature);

        // Kernel measures a 0.5mm (5e-4 m) deviation between actual and feature.
        let mock = MockGeometryKernel::new().with_max_deviation_result(
            actual,
            feature,
            0.0001,
            Value::Real(5e-4),
        );
        let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(Box::new(mock)));

        // Scalar path's pre-existing (default-Satisfied) entry, to be OVERRIDDEN.
        let mut results = vec![ConstraintCheckEntry {
            id: node_id.clone(),
            label,
            satisfaction: Satisfaction::Satisfied,
        }];
        let mut diags = Vec::new();
        engine.measure_gdt_conformance(&module, &values, &mut results, &mut diags);

        assert_eq!(results.len(), 1, "weave must override in place, not append");
        assert_eq!(
            results[0].satisfaction,
            Satisfaction::Violated,
            "measured 0.5mm exceeds the 0.1mm zone → Violated"
        );
        let msg = diags
            .iter()
            .map(|d| d.message.as_str())
            .find(|m| m.contains("VIOLATED"))
            .unwrap_or_else(|| panic!("expected a VIOLATED diagnostic, got: {diags:#?}"));
        assert!(
            msg.contains("0.5000"),
            "diagnostic must carry the measured magnitude (0.5mm): {msg}"
        );
        assert!(
            msg.contains("0.1000"),
            "diagnostic must carry the zone width (0.1mm): {msg}"
        );
    }

    /// (b) Explicit actual, measured 0mm ≤ 0.1mm zone → Satisfied.
    #[test]
    fn explicit_actual_within_zone_is_satisfied() {
        let module = parse_and_compile_with_stdlib(GEOMETRIC_SOURCE);
        let conforms = find_conforms(&module);
        let node_id = conforms.id.clone();

        let actual = GeometryHandleId(202);
        let feature = GeometryHandleId(101);
        let values = geometric_values(conforms, actual, feature);

        let mock = MockGeometryKernel::new().with_max_deviation_result(
            actual,
            feature,
            0.0001,
            Value::Real(0.0),
        );
        let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(Box::new(mock)));

        let mut results = vec![ConstraintCheckEntry {
            id: node_id,
            label: None,
            satisfaction: Satisfaction::Indeterminate,
        }];
        let mut diags = Vec::new();
        engine.measure_gdt_conformance(&module, &values, &mut results, &mut diags);

        assert_eq!(
            results[0].satisfaction,
            Satisfaction::Satisfied,
            "measured 0mm within the 0.1mm zone → Satisfied"
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("VIOLATED")),
            "a Satisfied geometric Conforms must not emit a VIOLATED diagnostic"
        );
    }

    /// (c) Explicit actual but NO kernel → Indeterminate + missing-kernel
    /// diagnostic, never a (false) Violated (C1 invariant).
    #[test]
    fn explicit_actual_no_kernel_is_indeterminate_not_violated() {
        let module = parse_and_compile_with_stdlib(GEOMETRIC_SOURCE);
        let conforms = find_conforms(&module);
        let node_id = conforms.id.clone();

        let values = geometric_values(conforms, GeometryHandleId(202), GeometryHandleId(101));

        // No geometry kernel.
        let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);

        let mut results = vec![ConstraintCheckEntry {
            id: node_id,
            label: None,
            satisfaction: Satisfaction::Satisfied,
        }];
        let mut diags = Vec::new();
        engine.measure_gdt_conformance(&module, &values, &mut results, &mut diags);

        assert_eq!(
            results[0].satisfaction,
            Satisfaction::Indeterminate,
            "no kernel → Indeterminate (never a false Violated)"
        );
        assert_ne!(results[0].satisfaction, Satisfaction::Violated);
        let msg = diags
            .iter()
            .map(|d| d.message.as_str())
            .find(|m| m.contains("INDETERMINATE"))
            .unwrap_or_else(|| panic!("expected an INDETERMINATE diagnostic, got: {diags:#?}"));
        assert!(
            msg.to_lowercase().contains("kernel"),
            "Indeterminate diagnostic must name the missing kernel: {msg}"
        );
    }

    /// (d) A Conforms with NO explicit actual: the pass must leave its scalar
    /// ConstraintCheckEntry untouched (no override, no geometric diagnostic).
    #[test]
    fn scalar_conforms_without_actual_is_untouched() {
        let module = parse_and_compile_with_stdlib(SCALAR_SOURCE);
        let conforms = find_conforms(&module);
        assert!(
            !conforms.arg_bindings.iter().any(|(n, _)| n == "actual"),
            "fixture precondition: the scalar Conforms binds no `actual`"
        );
        let node_id = conforms.id.clone();

        // A kernel IS present — but with no explicit actual the pass must not run.
        let mock = MockGeometryKernel::new();
        let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(Box::new(mock)));

        let mut results = vec![ConstraintCheckEntry {
            id: node_id,
            label: Some("Conforms".to_string()),
            satisfaction: Satisfaction::Satisfied,
        }];
        let mut diags = Vec::new();
        engine.measure_gdt_conformance(&module, &ValueMap::new(), &mut results, &mut diags);

        assert_eq!(results.len(), 1, "no override, no append");
        assert_eq!(
            results[0].satisfaction,
            Satisfaction::Satisfied,
            "scalar Conforms (no actual) must keep its scalar verdict (B4)"
        );
        assert!(diags.is_empty(), "no geometric diagnostic for a scalar Conforms");
    }
}

// ── η/4480 step-11: Engine::check weaves measure_gdt_conformance results ────────
//
// Proves the check()-level wiring (step-12): `Engine::check` invokes
// `measure_gdt_conformance` and OVERRIDES only the geometric Conforms entry,
// leaving a scalar Conforms (no explicit `actual`) and an ordinary constraint
// untouched, while preserving caller (declaration) order (B9 weave).
//
// Driven WITHOUT a geometry kernel: the geometric Conforms cannot resolve a live
// handle, so the pass overrides its scalar-Satisfied verdict with Indeterminate
// (C1 — never a false Violated) and emits a "Conforms INDETERMINATE" diagnostic.
// That is a deterministic, kernel-free signal that the weave ran; the
// Violated/Satisfied measured path is covered by the OCCT-gated CLI test (η B1/B2).
#[cfg(test)]
mod gdt_conformance_check_weave_tests {
    use reify_constraints::SimpleConstraintChecker;
    use reify_ir::Satisfaction;
    use reify_test_support::parse_and_compile_with_stdlib;

    use crate::Engine;

    /// One structure carrying three unguarded constraints in declaration order:
    ///   1. a GEOMETRIC Conforms (explicit `actual`)  — overridden by the η pass
    ///   2. a SCALAR Conforms (no `actual`)            — left untouched (B4)
    ///   3. an ordinary scalar constraint              — left untouched
    const MIXED_SOURCE: &str = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    param act : Geometry = box(1mm, 1mm, 1mm)
    param len : Length = 5mm
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm)
    constraint len >= 0mm
}
"#;

    #[test]
    fn check_weaves_geometric_conforms_override_only() {
        let module = parse_and_compile_with_stdlib(MIXED_SOURCE);

        // Identify the three constraints by their captured arg-binding shape.
        let probe = module
            .templates
            .iter()
            .find(|t| t.name == "Probe")
            .expect("Probe template");
        let has = |c: &reify_compiler::CompiledConstraint, name: &str| {
            c.arg_bindings.iter().any(|(n, _)| n == name)
        };
        let geometric_id = probe
            .constraints
            .iter()
            .find(|c| has(c, "actual"))
            .expect("geometric Conforms (explicit actual)")
            .id
            .clone();
        let scalar_id = probe
            .constraints
            .iter()
            .find(|c| has(c, "tolerance") && !has(c, "actual"))
            .expect("scalar Conforms (no actual)")
            .id
            .clone();
        let ordinary_id = probe
            .constraints
            .iter()
            .find(|c| c.arg_bindings.is_empty())
            .expect("ordinary constraint (no arg bindings)")
            .id
            .clone();

        // No geometry kernel → the geometric Conforms cannot measure → Indeterminate.
        let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
        let result = engine.check(&module);

        let entry = |id: &reify_core::ConstraintNodeId| {
            result
                .constraint_results
                .iter()
                .find(|e| &e.id == id)
                .unwrap_or_else(|| panic!("constraint {id:?} missing from check results"))
        };

        // (1) geometric Conforms: OVERRIDDEN to Indeterminate (was scalar-Satisfied);
        //     never a (false) Violated (C1).
        assert_eq!(
            entry(&geometric_id).satisfaction,
            Satisfaction::Indeterminate,
            "geometric Conforms must be overridden to Indeterminate (no kernel; C1)"
        );
        assert_ne!(entry(&geometric_id).satisfaction, Satisfaction::Violated);

        // (2) scalar Conforms (no actual): untouched — keeps its scalar verdict (B4).
        assert_eq!(
            entry(&scalar_id).satisfaction,
            Satisfaction::Satisfied,
            "scalar Conforms (no actual) must keep its scalar verdict (B4)"
        );

        // (3) ordinary constraint: untouched by the η pass.
        assert_eq!(
            entry(&ordinary_id).satisfaction,
            Satisfaction::Satisfied,
            "ordinary constraint must be untouched by the η pass"
        );

        // The woven pass ran and emitted a geometric-conformance diagnostic.
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Conforms INDETERMINATE")),
            "expected a 'Conforms INDETERMINATE' diagnostic from the woven pass, got: {:#?}",
            result.diagnostics
        );

        // Caller (declaration) order preserved: geometric, then scalar, then ordinary.
        let pos = |id: &reify_core::ConstraintNodeId| {
            result
                .constraint_results
                .iter()
                .position(|e| &e.id == id)
                .expect("id present")
        };
        assert!(
            pos(&geometric_id) < pos(&scalar_id) && pos(&scalar_id) < pos(&ordinary_id),
            "weave must preserve caller (declaration) order"
        );
    }
}
