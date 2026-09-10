use std::collections::HashMap;

use reify_constraints::SimpleConstraintChecker;
use reify_core::{
    ConstraintNodeId, ContentHash, Diagnostic, DiagnosticCode, ModulePath, SourceSpan, ValueCellId,
    VersionId,
};
use reify_ir::{Freshness, Satisfaction};
use tower_lsp::lsp_types::{self, Url};

use crate::analysis::module_name_from_uri;
use crate::convert;

/// The eval/check-pass containment guard shared by all three LSP production
/// entry points — see [`eval_guard`] for the hazard it contains and why it is
/// its own module.
pub(crate) mod eval_guard;

/// Persistent evaluation state maintained across edits.
///
/// Holds the Engine and last compiled module so the server can incrementally
/// update diagnostics when the source changes.
pub struct EvalState {
    engine: reify_eval::Engine,
    version_counter: u64,
    last_content_hash: Option<ContentHash>,
}

impl EvalState {
    /// Create a new evaluation state with SimpleConstraintChecker and no
    /// geometry kernel.
    ///
    /// This is the **EVAL-time** checker fed to `reify_eval::Engine::new` —
    /// a separate instance from the **COMPILE-time** checker that
    /// [`compute_diagnostics_with_state`] now injects into
    /// `compile_with_stdlib_checked` (task #6798, PRD leaf pi). The two are
    /// deliberately independent values of the same zero-sized
    /// `SimpleConstraintChecker` type: one feeds constraint-satisfaction
    /// evaluation after eval, the other feeds `auto:` candidate feasibility
    /// during compile. Collapsing them into a single shared instance would
    /// obscure that the compile-time injection is the thing task #6798
    /// changes.
    pub fn new() -> Self {
        let checker = SimpleConstraintChecker;
        Self {
            engine: reify_eval::Engine::new(Box::new(checker), None),
            version_counter: 0,
            last_content_hash: None,
        }
    }

    /// Returns the content hash of the last successfully compiled module, if any.
    pub fn last_content_hash(&self) -> Option<ContentHash> {
        self.last_content_hash
    }

    /// Returns true if the engine has been initialized by a prior `eval()` or `eval_cached()` call.
    pub fn is_engine_initialized(&self) -> bool {
        self.engine.is_initialized()
    }
}

impl Default for EvalState {
    fn default() -> Self {
        Self::new()
    }
}

/// Result from the stateful diagnostics pipeline.
pub struct DiagnosticsResult {
    /// LSP diagnostics to publish.
    pub diagnostics: Vec<lsp_types::Diagnostic>,
    /// Exported geometry data (if geometry kernel is configured).
    pub geometry_output: Option<Vec<u8>>,
}

/// Run the stateful parse → compile → eval → check pipeline.
///
/// Maintains a persistent Engine in EvalState across calls. On each call:
/// re-parse, re-compile, then either incremental `eval_cached` (when the
/// content hash is unchanged) or a fresh cold-start `eval` (when the content
/// changed), then `check_snapshot` for constraint results, and convert to
/// LSP diagnostics.
///
/// ## Compile-time checker: real `SimpleConstraintChecker`, not a stub (task #6798)
///
/// The compile stage below calls
/// `reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker)`,
/// matching `reify check`'s compile path (`parse_and_compile` /
/// `module_dag::compile_entry_with_stdlib_cfg_checked` in
/// `crates/reify-cli/src/main.rs`) and the GUI
/// (`compile_single_file_with_stdlib` in `gui/src-tauri/src/engine.rs`)
/// instead of the compile-time `CompileTimeIndeterminateChecker` stub. So
/// `auto:` candidate feasibility now sees the same constraint verdicts the
/// CLI does: the LSP no longer reports `E_AUTO_TYPE_PARAM_AMBIGUOUS` on a
/// constant constraint that the CLI resolves to
/// `E_AUTO_TYPE_PARAM_NO_CANDIDATE` (PRD
/// `docs/prds/v0_6/driver-contract-implementation.md` leaf pi / §5 BT8 / §12
/// premise correction 3). Cited by symbol rather than file:line, so the
/// pointers survive refactors.
///
/// This is a **compile-time change only**. The eval-time Engine built by
/// [`EvalState::new`] is untouched: still a bare
/// `Engine::new(SimpleConstraintChecker, None)`, with no
/// `register_compute_fns` / `register_compute_trampolines` call, so no
/// keystroke-time FEA/buckling/form-find solve ever runs — the Leo-ratified
/// subtraction described in the next section stands unchanged. Locked by one
/// executable contract, below in `mod tests`:
/// `fea_bearing_constraint_produces_no_false_violation_or_false_pass`, which
/// also serves as the BT8-reverse lock for this compile-time change.
///
/// The checker swap is PAIRED with an eval-skip guard. Failing `auto:`
/// candidate feasibility at compile time is exactly what leaves an
/// unsubstituted `Type::TypeParam` value cell in the graph, which panics eval
/// in debug builds (root cause owned by task **#6851**; mechanism in
/// [`eval_guard::first_unrepresentable_cell`]'s doc). All three
/// LSP production entry points therefore ask [`eval_guard::skip_reason`]
/// after collecting the compile-stage diagnostics, skip the eval/check pass
/// when it answers `Some`, and report which cell forced the skip — to the
/// server log at every site, and additionally as an editor-visible Warning at
/// the two that own a diagnostics list. The user still gets the
/// `E_AUTO_TYPE_PARAM_*` error this section is about where there is one, and
/// the server stays alive. The guard tests the graph for an unrepresentable cell,
/// NOT the diagnostics for an error: a compile error that leaves the graph
/// representable does not suppress eval, because the LSP evaluates through
/// non-fatal ones on purpose.
///
/// ## Engine posture: deliberately NO compute trampolines
///
/// `EvalState::new` builds a bare `Engine::new(SimpleConstraintChecker,
/// None)`, and this function never calls `register_compute_fns` /
/// `register_compute_trampolines` on it — no FEA/buckling/modal compute
/// trampoline is ever registered on the LSP's engine, so no keystroke-time
/// FEA/buckling/modal solve ever runs. This is the Leo-ratified posture for
/// the LSP (PRD `compute-fea-hardening.md`, INV-FEA-1 §2): keystroke-time
/// compute solves are rejected outright here, under any guard scheme — not
/// merely deferred.
///
/// Consequence: `@optimized("solver::elastic_static")` FEA-result
/// constraints (e.g. `constraint peak_stress < limit` over
/// `result.max_von_mises`) body-inline to `undef` and evaluate as
/// `Satisfaction::Indeterminate`. Because the two constraint-diagnostic
/// emitters below (both gated on `entry.satisfaction ==
/// Satisfaction::Violated` — the `violated_messages` skip-set construction
/// and the span-aware ERROR emission) skip `Indeterminate`, such a
/// constraint surfaces **no violation diagnostic** today — and, pending
/// task C2's hint diagnostic, no diagnostic at all, silently
/// indistinguishable from "no constraint".
///
/// **Severity of the missing-trampoline diagnostic (task 5311):** the engine
/// surfaces its own "no registered compute trampoline (falling back to
/// body-inlining)" diagnostic for `@optimized` FEA solves at
/// `Severity::Warning`, carrying
/// `DiagnosticCode::NoRegisteredComputeTrampoline`.  The engine conditions that
/// severity on its compute registry being entirely EMPTY, and the LSP builds
/// `Engine::new(checker, None)` and never registers a trampoline, so it is
/// always on the Warning arm.  (`reify eval` / `reify build` register the
/// production bundle and so keep receiving `Severity::Error`.)
///
/// That is ONLY about this fallback diagnostic.  It leaves the LSP's SEPARATE
/// `fea-not-evaluated` INFORMATION constraint hint (below) entirely untouched,
/// and INV-FEA-1 ratified — the two must not be conflated: one reports that no
/// trampoline was registered, the other that a constraint was consequently not
/// evaluated.
///
/// This trampoline-free posture is an executable contract locked by one
/// test (below, in `mod tests`):
/// `fea_bearing_constraint_produces_no_false_violation_or_false_pass` — the
/// LSP-side analog of `cmd_check`'s `check_fea_violated_constraint_is_not_gated`
/// lock (`crates/reify-cli/tests/harness_cli/cli_build_fea.rs`), and also the
/// lock that task #6798's compile-time checker injection (see the section
/// above) does not disturb this posture, via an exhaustive probe over every
/// target `reify_eval::compute_targets::register_compute_fns` registers —
/// changing this posture requires updating that test intentionally.
pub fn compute_diagnostics_with_state(
    state: &mut EvalState,
    source: &str,
    uri: &Url,
) -> DiagnosticsResult {
    let mut diagnostics = Vec::new();

    // Derive module name from URI
    let module_name = uri
        .path_segments()
        .and_then(|mut segs| segs.next_back())
        .and_then(|name| name.strip_suffix(".ri"))
        .unwrap_or("unnamed");

    // Parse (prelude-aware so stdlib enum references like `CorrosionClass.C5`
    // disambiguate to `EnumAccess` rather than `MemberAccess`; pairs with
    // `compile_with_stdlib_checked` below). See task 2525.
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));
    for err in &parsed.errors {
        diagnostics.push(convert::convert_parse_error(err, source, uri));
    }
    // Early-return on parse errors: the partial AST fed to compile/eval produces
    // misleading secondary diagnostics. Match the behaviour of
    // Engine::load_from_source's early-return on parse errors.
    if !parsed.errors.is_empty() {
        return DiagnosticsResult {
            diagnostics,
            geometry_output: None,
        };
    }

    // Compile. Real checker (task #6798, PRD leaf pi) — see this function's
    // "## Compile-time checker" doc section above for the full rationale.
    let compiled = reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);
    for diag in &compiled.diagnostics {
        diagnostics.push(convert::convert_diagnostic(diag, source, uri));
    }

    // Containment (task #6851): a FAILED `auto:` resolution leaves an
    // unsubstituted `Type::TypeParam` value cell that panics eval in debug
    // builds — see `eval_guard::first_unrepresentable_cell`'s doc comment for the
    // mechanism. The compile-stage diagnostics, which are the user-visible
    // signal task #6798 is about, are already collected in the loop above, so
    // the editor still reports NoCandidate/Ambiguous; we simply never feed such
    // a graph to the engine. Same containment idiom and same reasoning as the
    // parse-error early return ~10 lines above: a graph we know is malformed
    // produces misleading (here: fatal) secondary results.
    //
    // Placement before the state mutations is LOAD-BEARING. `state.version_counter`
    // and `state.last_content_hash` both stay unadvanced, leaving `EvalState`
    // exactly as it was, so the next keystroke evaluates `content_unchanged` to
    // false and takes the cold-start branch. Advancing `last_content_hash` here
    // would create precisely the stale-cache bug the comment below warns about:
    // `eval_cached` returns empty diagnostics by construction, so a module that
    // was never evaluated would silently report none.
    if let Some(skipped) = eval_guard::skip_reason(&compiled) {
        // Observability: skipping eval silently costs the whole document its
        // eval-time diagnostics and constraint results, so say so in the
        // editor as well as the server log. `skip_reason` has already written
        // the log line (throttled); the Warning below is what a user sees on
        // the shapes that carry NO compile diagnostic of their own — see
        // `SkippedEval::diagnostic`.
        diagnostics.push(skipped.diagnostic());
        return DiagnosticsResult {
            diagnostics,
            geometry_output: None,
        };
    }

    // Eval: use incremental eval_cached when structure unchanged, else cold-start.
    state.version_counter += 1;

    // Use the incremental eval_cached path only when content is unchanged AND
    // the engine has already been initialized by a prior eval(). An uninitialized
    // engine must always take the cold-start branch regardless of last_content_hash:
    // eval_cached returns empty diagnostics by construction, so routing an
    // uninitialized engine through it would silently drop eval-time errors.
    let content_unchanged = state
        .last_content_hash
        .map(|h| h == compiled.content_hash)
        .unwrap_or(false)
        && state.is_engine_initialized();

    // Capture eval-time diagnostics from eval() / eval_cached().
    //
    // Both eval() and eval_cached() now emit the same diagnostic kinds:
    // circular let-binding dependencies, sub-component lookup failures,
    // param_override type/dimension mismatches, solver Infeasible / NoProgress.
    // These are NOT reflected in check_snapshot()'s CheckResult and would be
    // silently dropped without this capture.
    //
    // On the rare check_snapshot → None fallback we drop the captured copy
    // because check() internally re-runs eval() and prepends those diagnostics
    // to CheckResult.diagnostics — keeping them would double-emit.
    let mut eval_diagnostics: Vec<Diagnostic> = if content_unchanged {
        state
            .engine
            .eval_cached(&compiled, VersionId(state.version_counter))
            .eval_result
            .diagnostics
    } else {
        // Observability: if the hash *did* match but the engine was uninitialized,
        // that is the specific invariant violation the engine-init guard (above) was
        // added to catch — last_content_hash was set without a preceding eval().
        // Log a warning in debug builds so the decoupling is not silent. We cannot
        // use debug_assert! here because the graceful-handling test intentionally
        // constructs this state to verify the cold-start branch is taken; the right
        // response is to handle it correctly (which we do below) and warn.
        #[cfg(debug_assertions)]
        if state.last_content_hash == Some(compiled.content_hash) && !state.is_engine_initialized()
        {
            eprintln!(
                "[reify-lsp] WARNING: content_hash matched but engine was uninitialized \
                 — last_content_hash was set without a preceding eval(); \
                 cold-start forced to prevent silent diagnostic loss (engine-init guard)"
            );
        }
        let checker = SimpleConstraintChecker;
        state.engine = reify_eval::Engine::new(Box::new(checker), None);
        state.engine.eval(&compiled).diagnostics
    };

    // Check constraints from snapshot, falling back to full check() if snapshot is absent
    let check_result = match state.engine.check_snapshot(&compiled) {
        Some(result) => result,
        None => {
            eprintln!(
                "[reify-lsp] check_snapshot returned None after eval, falling back to full check"
            );
            // check() re-runs eval() internally and includes its diagnostics in
            // CheckResult.diagnostics; drop our independently captured copy to
            // avoid double-emission.
            eval_diagnostics = Vec::new();
            state.engine.check(&compiled)
        }
    };

    // Build constraint span lookup map from compiled module
    let constraint_spans: HashMap<ConstraintNodeId, SourceSpan> = compiled
        .templates
        .iter()
        .flat_map(|t| t.constraints.iter())
        .map(|c| (c.id.clone(), c.span))
        .collect();

    // Convert non-constraint eval diagnostics from check result.
    // Skip constraint checker messages (formats: "constraint {id} violated" and, when
    // a label is present, "constraint {label} violated") since we generate span-aware
    // versions below.
    //
    // When a constraint has a label (e.g. `forall@v[0]` from forall_elaborate.rs),
    // `engine_constraints::labeled_diagnostics` rewrites the message to replace the raw
    // ConstraintNodeId with the label — producing `"constraint forall@v[0] violated"`.
    // We must therefore include both the id-format AND the label-format in the filter set
    // to correctly skip those messages and avoid double-emission.
    let violated_messages: std::collections::HashSet<String> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .flat_map(|e| {
            let id_msg = format!("constraint {} violated", e.id);
            let label_msg = e
                .label
                .as_deref()
                .map(|l| format!("constraint {} violated", l));
            std::iter::once(id_msg).chain(label_msg)
        })
        .collect();
    for diag in &check_result.diagnostics {
        if !violated_messages.contains(&diag.message) {
            diagnostics.push(convert::convert_diagnostic(diag, source, uri));
        }
    }

    // Merge eval-time diagnostics. The `eval_diagnostics_never_use_constraint_violation_format`
    // and `eval_diag_format_*` tests enforce the invariant that eval() never emits the
    // `constraint <entity>#constraint[<index>] violated` format, covering every known
    // eval-time emitter:
    //   - circular let-binding (unfold.rs / engine_eval.rs)
    //   - param_override type-kind / dimension mismatch (engine_eval.rs)
    //   - sub-component lookup failure (engine_eval.rs)
    //   - solver Infeasible / NoProgress (engine_eval.rs)
    // No *format*-based filter is applied here: if the invariant ever breaks, the
    // cluster fails loudly in CI and a maintainer must add a filter or update the
    // merge loop. Keeping a silent defensive filter would hide the very regression
    // the cluster is designed to detect.
    //
    // Exactly one code-based exclusion applies (task 5240):
    // `DiagnosticCode::EvalCachedGuardedGroupsFallback`. Every diagnostic in the
    // list above reports a genuine USER fault. That one does not — it is an
    // internal engine note saying `eval_cached` delegated to the cold `eval()`
    // because the module carries `where`-guarded groups. The values returned are
    // the correct cold-eval ones, so nothing user-facing is wrong and nothing
    // belongs in the editor. Without this drop, every valid `.ri` file using
    // `where` — including the shipped stdlib
    // `crates/reify-compiler/stdlib/determinacy_purposes.ri` and
    // `examples/m5_guarded_enum.ri` — would show a spurious warning that
    // flickers in and out as the user types, appearing only on the keystrokes
    // that take the `content_unchanged` warm path above.
    //
    // Matched by CODE, not by message substring, per this crate's
    // `FEA_NOT_EVALUATED_CODE` precedent (below): a copy-edit to the engine's
    // wording must never silently re-open the leak. Its presence on the engine
    // side is locked by
    // `crates/reify-eval/tests/harness_engine/eval_cached_guarded_groups.rs`.
    for diag in &eval_diagnostics {
        if diag.code == Some(DiagnosticCode::EvalCachedGuardedGroupsFallback) {
            continue;
        }
        diagnostics.push(convert::convert_diagnostic(diag, source, uri));
    }

    // Generate explicit diagnostics for constraint violations with source spans.
    //
    // Per-element forall provenance (PRD criterion 10,
    // docs/prds/forall-statement-form.md): when a constraint originates from a
    // `forall` statement, `forall_elaborate.rs` emits one `CompiledConstraint`
    // per element with `label = Some("forall@<var>[<idx>]")` (e.g. `forall@v[0]`).
    // That label propagates unchanged through
    // `ConstraintCheckEntry.label` and surfaces verbatim in the LSP diagnostic
    // message via the `Some(label)` branch below — the resulting message is
    // `"constraint violated: forall@v[<idx>]"`.  No parsing or reformatting of the
    // label is required; the index is the per-element provenance carrier.
    // Regression-lock: `forall_per_element_constraint_violation_surfaces_element_index`
    // (this file) pins this contract end-to-end.
    for entry in &check_result.constraint_results {
        if entry.satisfaction == Satisfaction::Violated {
            let msg = constraint_violated_message(entry);
            let span_lookup = constraint_spans.get(&entry.id);
            let range = span_lookup
                .map(|span| convert::span_to_range(source, *span))
                .unwrap_or_default();
            diagnostics.push(lsp_types::Diagnostic {
                range,
                severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                source: Some("reify".to_string()),
                message: msg,
                ..Default::default()
            });
        }
    }

    // Task 5078 (PRD `compute-fea-hardening.md` C2): hint diagnostic for
    // FEA-dependent constraints that go unevaluated under this function's
    // trampoline-free posture (see the doc comment above). A constraint
    // reading an `@optimized` FEA result (e.g. `solve_elastic_static`'s
    // `max_von_mises`) body-inlines to `Value::Undef` and therefore checks
    // as `Satisfaction::Indeterminate` — neither the `violated_messages`
    // skip-set above nor the span-aware `Satisfaction::Violated` loop emits
    // anything for it, so today it surfaces NO diagnostic at all, silently
    // indistinguishable from "no constraint". Surface a non-noisy
    // `Severity::Info` hint instead, anchored to the constraint's span, so
    // the user knows why the constraint shows nothing in the editor.
    //
    // Gated by `constraint_depends_on_unregistered_optimized_compute` so this
    // does NOT fire on every `Indeterminate` constraint — e.g. a
    // genuinely-unresolved `auto` param is also `Indeterminate` here (no
    // solver attached), but is not FEA-dependent and must not get this hint.
    //
    // Deliberately additive and easily removable: a single emission branch
    // plus one self-contained helper reading already-computed `check_result`
    // / `compiled` data, no new `EvalState` fields, no keystroke-time solve.
    //
    // TODO(#5023): remove this stopgap when async-recalc Phase A lands
    // per-constraint computing/not-evaluated states that supersede this hint.
    //
    // Perf guard: this whole block — including the lookup maps below — runs
    // on the LSP keystroke hot path, so skip it entirely when there is no
    // `Indeterminate` constraint in this document at all. Nothing in the
    // loop below can fire without at least one such entry, so this still
    // frees the common fully-resolved (no `auto` params, no FEA) document
    // from all of the cost below.
    //
    // This guard is intentionally coarse — it is NOT "only FEA documents pay
    // this cost". A genuinely-unresolved `auto` param is *also*
    // `Indeterminate` with no solver attached, and `auto` params are routine
    // in this parametric DSL (this file's own
    // `fea_hint_excludes_auto_param_indeterminate_and_dedups_per_constraint`
    // test uses `param gap : Length = auto` as its non-FEA control case), so
    // plenty of non-FEA documents still build the maps below on every
    // keystroke. A cheaper *and correct* pre-filter isn't available for
    // free: `state.engine.prelude()` is the full stdlib, which always
    // declares `@optimized` functions (`solve_elastic_static` and its
    // siblings), so "does `compiled.functions` ∪ prelude declare any
    // function with `optimized_target.is_some()`" is trivially true on every
    // call and would filter nothing; narrowing that check to
    // `compiled.functions` alone would filter too much, since a real FEA
    // constraint reaches its solver call through a *prelude* function, not a
    // module-local one, and would wrongly lose its hint. A correct tighter
    // filter would have to either walk the same constraint/value-cell
    // closure the block below already performs (no savings) or cache
    // results across calls, which would need new persistent state — ruled
    // out by this hint's "no new `EvalState` fields" design (see the block
    // comment above this `if`).
    if check_result
        .constraint_results
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Indeterminate)
    {
        let value_cell_exprs: HashMap<ValueCellId, _> = compiled
            .templates
            .iter()
            .flat_map(|t| t.value_cells.iter())
            .filter_map(|vc| vc.default_expr.as_ref().map(|e| (vc.id.clone(), e)))
            .collect();
        let compiled_constraints: HashMap<&ConstraintNodeId, _> = compiled
            .templates
            .iter()
            .flat_map(|t| t.constraints.iter())
            .map(|c| (&c.id, c))
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
        for f in compiled.functions.iter().chain(
            state
                .engine
                .prelude()
                .iter()
                .flat_map(|m| m.functions.iter()),
        ) {
            unregistered_optimized_fn_names
                .entry(f.name.as_str())
                .or_insert_with(|| {
                    f.optimized_target
                        .as_deref()
                        .is_some_and(|t| state.engine.compute_dispatch(t).is_none())
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
        // `fea_hint_two_fea_constraints_each_get_distinct_span_hint` (below,
        // in `mod tests`), which is the case most sensitive to a regression
        // in this upstream invariant.
        for entry in &check_result.constraint_results {
            if entry.satisfaction != Satisfaction::Indeterminate {
                continue;
            }
            let Some(constraint) = compiled_constraints.get(&entry.id) else {
                continue;
            };
            if !constraint_depends_on_unregistered_optimized_compute(
                &constraint.expr,
                &value_cell_exprs,
                &unregistered_optimized_fn_names,
            ) {
                continue;
            }
            let range = constraint_spans
                .get(&entry.id)
                .map(|span| convert::span_to_range(source, *span))
                .unwrap_or_default();
            diagnostics.push(lsp_types::Diagnostic {
                range,
                severity: Some(lsp_types::DiagnosticSeverity::INFORMATION),
                code: Some(lsp_types::NumberOrString::String(
                    FEA_NOT_EVALUATED_CODE.to_string(),
                )),
                source: Some("reify".to_string()),
                message: FEA_NOT_EVALUATED_HINT.to_string(),
                ..Default::default()
            });
        }
    }

    // Emit freshness diagnostics for Pending and Failed cells (arch §7.1, §9.2).
    //
    // Iterate the compiled templates Vec directly (not a HashMap) so diagnostic
    // ordering is deterministic — same compile-defined cell order on every call.
    // Using a HashMap would yield entries in unstable order, causing flapping in
    // snapshot tests and non-deterministic output for the user.
    //
    // Final → no emission (success state, no diagnostic by definition).
    // Intermediate → no emission (transient progress; arch §7.2 "naturally quiets";
    //   not actionable in an editor — LSP diagnostics are for states users can act on).
    // Pending → WARNING with code "computation-pending" (upstream dependency failed).
    //   The message includes the chain-root cell name when known, e.g.
    //   "computation pending: upstream dependency failed (because Bracket.volume failed)".
    //   Falls back to the static string when pending_cause is None (bulk mark_pending
    //   paths that intentionally omit a cause; see cache.rs:482-513).
    // Failed → ERROR with code "computation-failed" (this cell's computation broke).
    //
    // Constraint violations are intentionally NOT emitted here — they route through
    // `Satisfaction::Violated` (arch §9.3), not `Freshness::Failed`. The separation
    // regression test `constraint_violation_does_not_produce_computation_failed` pins this.
    //
    // This block is only in `compute_diagnostics_with_state` (persistent engine).
    // The stateless `compute_diagnostics` has no persistent engine, so it has no
    // freshness state to report and this block is intentionally absent there.
    for template in &compiled.templates {
        for vc in &template.value_cells {
            match state
                .engine
                .freshness(&reify_eval::cache::NodeId::Value(vc.id.clone()))
            {
                Freshness::Failed { error } => {
                    let range = convert::span_to_range(source, vc.span);
                    diagnostics.push(lsp_types::Diagnostic {
                        range,
                        severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                        code: Some(lsp_types::NumberOrString::String(
                            "computation-failed".to_string(),
                        )),
                        source: Some("reify".to_string()),
                        message: format!("computation failed: {}", error.message()),
                        ..Default::default()
                    });
                }
                Freshness::Pending { .. } => {
                    let range = convert::span_to_range(source, vc.span);
                    // O(1) HashMap lookup on NodeCache::pending_cause. When the
                    // cause is present we embed it in the message so the user sees
                    // which upstream cell failed. Falls back to the historic static
                    // string when None (bulk mark_pending path, cache.rs:482-513).
                    let cause = state
                        .engine
                        .pending_cause(&reify_eval::cache::NodeId::Value(vc.id.clone()));
                    let message = match cause {
                        Some(node) => format!(
                            "computation pending: upstream dependency failed (because {} failed)",
                            node
                        ),
                        None => "computation pending: upstream dependency failed".to_string(),
                    };
                    diagnostics.push(lsp_types::Diagnostic {
                        range,
                        severity: Some(lsp_types::DiagnosticSeverity::WARNING),
                        code: Some(lsp_types::NumberOrString::String(
                            "computation-pending".to_string(),
                        )),
                        source: Some("reify".to_string()),
                        message,
                        ..Default::default()
                    });
                }
                Freshness::Final | Freshness::Intermediate { .. } => {
                    // No diagnostic — Final is success, Intermediate is transient (arch §7.2).
                }
            }
        }
    }

    // Record the content hash so the next call can choose incremental vs cold-start.
    state.last_content_hash = Some(compiled.content_hash);

    DiagnosticsResult {
        diagnostics,
        geometry_output: None,
    }
}

/// Format the span-aware "constraint violated" diagnostic message emitted
/// above for a `Satisfaction::Violated` entry.
///
/// Extracted to a shared helper (rather than inlined at the call site) so
/// that the posture-lock test
/// `fea_bearing_constraint_produces_no_false_violation_or_false_pass` (in
/// `mod tests`, below) can assert the *exact* text a false violation would
/// produce — driven from this same function — instead of a substring that
/// could silently drift out of sync with the wording here.
fn constraint_violated_message(entry: &reify_eval::ConstraintCheckEntry) -> String {
    match &entry.label {
        Some(label) => format!("constraint violated: {}", label),
        None => format!("constraint {} violated", entry.id),
    }
}

/// Exact wording of task 5078's (PRD `compute-fea-hardening.md` C2) FEA
/// "not evaluated in editor" `Severity::Info` hint, emitted in
/// [`compute_diagnostics_with_state`] above.
///
/// Defined once here rather than re-declared as a local constant in each
/// regression test, so the production message and every test that asserts
/// on it share one source of truth: a future wording change updates every
/// consumer at once instead of requiring each test to be hand-edited to
/// keep matching. Tests should NOT use this to *identify* the diagnostic
/// (i.e. as a `filter` predicate) — use [`FEA_NOT_EVALUATED_CODE`] for
/// that, and reserve this for asserting the human-readable wording once a
/// diagnostic is already known to be the FEA hint.
const FEA_NOT_EVALUATED_HINT: &str = "FEA constraint not evaluated in editor — run `reify test`";

/// Stable `code` for task 5078's FEA "not evaluated in editor" hint (PRD
/// `compute-fea-hardening.md` C2), set on the `lsp_types::Diagnostic`
/// emitted in [`compute_diagnostics_with_state`] alongside
/// [`FEA_NOT_EVALUATED_HINT`].
///
/// Unlike the message, which is free-text prose, this is the
/// wording-independent handle consumers — including this file's own
/// regression tests — should match on to identify the diagnostic,
/// mirroring the `"computation-pending"` / `"computation-failed"` codes
/// the freshness diagnostics carry (emitted just below in the same
/// function). A future copy-edit to [`FEA_NOT_EVALUATED_HINT`] must never
/// silently break identification for a consumer matching on this code.
const FEA_NOT_EVALUATED_CODE: &str = "fea-not-evaluated";

/// Static discriminator for task 5078's FEA "not evaluated in editor" hint
/// (PRD `compute-fea-hardening.md` C2).
///
/// Returns `true` when `constraint_expr` — or the `default_expr` of any
/// value cell it transitively depends on — calls a function whose
/// `@optimized("target")` annotation names a compute target with no
/// registered trampoline on `engine`. Under this crate's trampoline-free LSP
/// posture (see [`compute_diagnostics_with_state`]'s doc comment) such a
/// target always body-inlines to `Value::Undef`, so this predicate is
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
/// caller's responsibility (see `compute_diagnostics_with_state`) so it can
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

/// Run the full parse → compile → check pipeline and return LSP diagnostics.
///
/// Shares `compute_diagnostics_with_state`'s trampoline-free engine posture —
/// see that function's doc comment for the authoritative posture writeup.
pub fn compute_diagnostics(source: &str, uri: &Url) -> Vec<lsp_types::Diagnostic> {
    let mut result = Vec::new();

    // Derive a module name from the URI
    let module_name = module_name_from_uri(uri);

    // Parse (prelude-aware so stdlib enum references disambiguate correctly;
    // pairs with `compile_with_stdlib_checked` below). See task 2525.
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));

    // Convert parse errors
    for err in &parsed.errors {
        result.push(convert::convert_parse_error(err, source, uri));
    }

    // Compile. Real checker (task #6798, PRD leaf pi) — see
    // `compute_diagnostics_with_state`'s "## Compile-time checker" doc
    // section for the full rationale.
    let compiled = reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);

    // Convert compiler diagnostics
    for diag in &compiled.diagnostics {
        result.push(convert::convert_diagnostic(diag, source, uri));
    }

    // Containment for the unsubstituted-TypeParam panic — see
    // `compute_diagnostics_with_state`'s equivalent guard for the full
    // rationale (task #6851). This stateless surface panics identically and is
    // guarded identically; it has no `EvalState` to leave untouched.
    if let Some(skipped) = eval_guard::skip_reason(&compiled) {
        result.push(skipped.diagnostic());
        return result;
    }

    // Check (eval with constraint checker, no geometry kernel)
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let check_result = engine.check(&compiled);

    // Convert eval diagnostics
    for diag in &check_result.diagnostics {
        result.push(convert::convert_diagnostic(diag, source, uri));
    }

    result
}

/// Shared `auto:` test fixtures and anti-vacuity guards — see
/// [`auto_type_param_fixtures`] for why they live outside this module.
#[cfg(test)]
pub(crate) mod auto_type_param_fixtures;

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{DiagnosticSeverity, Url};

    // Shared `auto:` fixtures + anti-vacuity guards, also imported by
    // `crate::analysis::tests` so the two entry-point test families cannot
    // drift apart.
    use super::auto_type_param_fixtures::{
        AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC, BT8_CONSTANT_CONSTRAINT_SRC,
        DECLARED_ONLY_GENERIC_TYPEPARAM_SRC, EXPLICIT_GENERIC_INSTANTIATION_TYPEPARAM_SRC,
        GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC, NON_AUTO_COMPILE_ERROR_WITH_EVAL_DIAG_SRC,
        UNUSED_TYPEPARAM_AUTO_FAIL_WITH_EVAL_DIAGS_SRC,
        assert_auto_fail_fixture_is_newly_reachable, assert_bt8_fixture_still_diverges,
        assert_compile_clean_typeparam_fixtures_carry_no_error,
        assert_guarded_group_fixture_is_newly_reachable,
    };

    // Additional imports for the eval-diagnostics regression-lock cluster.
    use reify_test_support::MockConstraintSolver;
    use reify_core::{DiagnosticCode, DimensionVector, Severity, ValueCellId};
    use reify_ir::Value;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    /// Compile `parsed` exactly the way the LSP's three production compile
    /// sites do — `reify_compiler::compile_with_stdlib_checked(parsed,
    /// &SimpleConstraintChecker)` (task #6798, PRD leaf pi) — rather than
    /// the compile-time `CompileTimeIndeterminateChecker` stub.
    ///
    /// Any test below that independently reproduces what
    /// `compute_diagnostics` / `compute_diagnostics_with_state` compute
    /// internally (e.g. to precompute a `content_hash` or a `CompiledModule`
    /// for a follow-up `check`/`check_snapshot` call) MUST route through
    /// this helper rather than calling `compile_with_stdlib` directly — a
    /// stub-compiled mirror agrees with production only by coincidence
    /// today (no fixture below carries an `auto:` clause), and would
    /// silently drift the moment one does.
    ///
    /// Do NOT use this helper in the BT8 anti-vacuity guards
    /// ([`assert_bt8_fixture_still_diverges`]), which intentionally compile
    /// both ways to prove the stub and the real checker still diverge.
    fn compile_like_production(parsed: &reify_ast::ParsedModule) -> reify_compiler::CompiledModule {
        reify_compiler::compile_with_stdlib_checked(parsed, &SimpleConstraintChecker)
    }

    /// Minimal source that references two stdlib symbols (Rigid trait, Material struct).
    /// Shared across all task-2176 stdlib-resolution tests to avoid tripling the literal.
    // Post-GHR-α (task 3603): Physical is spec-shape (geometry : Solid +
    // material : Material struct slot); the legacy flat-scalar
    // density/volume/centroid_x/y/z params were retired. Rigid refines Physical;
    // moment_of_inertia is now auto-derived (task 4229 Option A — no longer a
    // required param). Dimensioned density (7850kg/m^3) required so body_density
    // let resolves to a clean Density (avoids resolve_density_arg Warning).
    const STDLIB_PROBE_SRC: &str = r#"structure S : Rigid {
    param geometry: Solid = box(10mm, 20mm, 30mm)
    param material: Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
}"#;

    /// Inline FEA-bearing fixture (PRD `compute-fea-hardening.md` task C1,
    /// INV-FEA-1). Mirrors the shape of
    /// `crates/reify-cli/tests/fixtures/fea_cantilever_violated.ri` (same
    /// solve setup: material / point load / fixed support / `solve_elastic_
    /// static` / `let peak_stress = result.max_von_mises` / FEA-result
    /// constraints) but omits that fixture's `let body = box(...)` geometry
    /// realization — the LSP path attaches no geometry kernel and has no
    /// STEP-file exit-code gate, and `solve_elastic_static` takes scalar
    /// dims (not a `Solid`), so `box` is unnecessary here.
    ///
    /// Deliberately FEA-only: no geometry/dimensional constraint exists that
    /// could be genuinely `Violated`, so a "zero Violated" assertion over
    /// this fixture holds by construction, not by coincidence.
    ///
    /// Reused by `fea_bearing_constraint_produces_no_false_violation_or_false_pass`
    /// below, and intended for reuse by the follow-up hint-diagnostic task
    /// (C2) — keep this fixture FEA-only if extending it.
    const FEA_BEARING_SRC: &str = r#"structure FeaBearing {
    param length : Length = 1000mm
    param width  : Length = 100mm
    param height : Length = 100mm

    let material = Steel_AISI_1045()
    let tip_load = PointLoad(point: "tip", force: 1000.0)
    let mount = FixedSupport(target: "root")

    let result = solve_elastic_static(
        material, length, width, height, [tip_load], [mount], ElasticOptions()
    )

    let peak_stress = result.max_von_mises

    constraint peak_stress < 1MPa
    constraint peak_stress < 100MPa
}"#;

    #[test]
    fn valid_bracket_source_no_errors() {
        let source = reify_test_support::bracket_source();
        let diags = compute_diagnostics(source, &test_uri());
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors.is_empty(),
            "valid source should produce no errors, got: {errors:?}"
        );
    }

    /// BT8 forward (task #6798, PRD `driver-contract-implementation.md` leaf
    /// pi): the two production compile sites in this file must agree with
    /// `reify check`'s real-checker verdict on a CONSTANT `auto:`
    /// constraint, not the compile-time stub's.
    ///
    /// Anti-vacuity guard first, via [`assert_bt8_fixture_still_diverges`]
    /// (shared with `analysis::tests::analysis_context_uses_real_constraint_checker`
    /// so the two forward tests' guards cannot drift apart): assert the stub
    /// and the real checker still genuinely diverge on
    /// [`BT8_CONSTANT_CONSTRAINT_SRC`] before asserting the LSP matches the
    /// real one — if a future compiler change collapses
    /// AMBIGUOUS/NO_CANDIDATE into the same verdict, this guard fails loudly
    /// instead of the LSP assertions below passing vacuously.
    #[test]
    fn lsp_constant_constraint_agrees_with_reify_check_real_checker() {
        // --- Anti-vacuity guard: the fixture must still genuinely diverge ---
        assert_bt8_fixture_still_diverges();

        // --- Stateless surface: compute_diagnostics ---
        let diags = compute_diagnostics(BT8_CONSTANT_CONSTRAINT_SRC, &test_uri());
        let no_candidate_code = Some(lsp_types::NumberOrString::String(
            "AutoTypeParamNoCandidate".to_string(),
        ));
        let ambiguous_code = Some(lsp_types::NumberOrString::String(
            "AutoTypeParamAmbiguous".to_string(),
        ));
        assert!(
            diags.iter().any(|d| d.code == no_candidate_code)
                && !diags.iter().any(|d| d.code == ambiguous_code),
            "BT8 forward (compute_diagnostics): must agree with `reify \
             check`'s real-checker AutoTypeParamNoCandidate verdict on a \
             constant constraint, not the compile-time stub's \
             AutoTypeParamAmbiguous; got diagnostics: {:#?}",
            diags
        );

        // --- Stateful surface: compute_diagnostics_with_state (the live server's path) ---
        let mut state = EvalState::new();
        let result =
            compute_diagnostics_with_state(&mut state, BT8_CONSTANT_CONSTRAINT_SRC, &test_uri());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == no_candidate_code)
                && !result.diagnostics.iter().any(|d| d.code == ambiguous_code),
            "BT8 forward (compute_diagnostics_with_state): must agree with \
             `reify check`'s real-checker AutoTypeParamNoCandidate verdict \
             on a constant constraint, not the compile-time stub's \
             AutoTypeParamAmbiguous; got diagnostics: {:#?}",
            result.diagnostics
        );
    }

    /// The checker swap's OTHER user-visible consequence (task #6798, Gap-C
    /// honesty warning), uncovered by every other BT8 test in this file:
    /// injecting the real
    /// `SimpleConstraintChecker` un-gates
    /// `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED`
    /// (`crates/reify-compiler/src/auto_type_param.rs`,
    /// `emit_unevaluated_constraint_warnings`, gated on
    /// `!constraint_checker.is_compile_time_stub()`), which fires whenever
    /// an `auto:` candidate's constraint reads a cell whose default is a
    /// computed (non-literal) expression — the literal-only compile-time
    /// seeder skips such cells, so the constraint is honestly
    /// `Indeterminate` rather than silently treated as satisfied. This is
    /// already the CLI's behaviour (`SimpleConstraintChecker` is not the
    /// stub there either); the LSP simply never surfaced it before this
    /// leaf.
    ///
    /// Single candidate, deliberately NOT the BT8 AMBIGUOUS/NO_CANDIDATE
    /// shape: `derived_bound`'s default (`bore * 2.0`) is non-literal, so
    /// `derived_bound > 0.0` evaluates to `Undef` → `Indeterminate` under
    /// BOTH checkers alike (the literal-only seeder skips the cell
    /// regardless of which checker runs) — the sole candidate resolves the
    /// same way either way, so the new diagnostic under test is additive,
    /// not a stand-in for AMBIGUOUS/NO_CANDIDATE. `T` stays unused in
    /// `Bearing`'s body, matching [`BT8_CONSTANT_CONSTRAINT_SRC`]'s
    /// documented reason for avoiding the `assert_value_cell_types_representable`
    /// panic hazard.
    ///
    /// Anti-vacuity guard: assert the stub does NOT emit
    /// `AutoTypeParamConstraintUnevaluated` on this fixture (gated off by
    /// `CompileTimeIndeterminateChecker::is_compile_time_stub() == true`)
    /// before asserting that the real checker — and both LSP entry points —
    /// do.
    #[test]
    fn lsp_surfaces_auto_type_param_constraint_unevaluated_warning() {
        const GAP_C_SRC: &str = r#"trait Seal {}
structure def GasketSeal : Seal { param d : Real = 2.0 }
structure def Bearing<T: Seal> {
    param bore : Real = 1.0
    let derived_bound = bore * 2.0
    constraint derived_bound > 0.0
}
structure def Assembly { sub b = Bearing<auto: Seal>() }
"#;
        let unevaluated_code = Some(lsp_types::NumberOrString::String(
            "AutoTypeParamConstraintUnevaluated".to_string(),
        ));

        // --- Anti-vacuity guard: stub must NOT emit the honesty warning ---
        let parsed = reify_compiler::parse_with_stdlib(GAP_C_SRC, ModulePath::single("test"));
        let stub = reify_compiler::compile_with_stdlib(&parsed);
        assert!(
            !stub
                .diagnostics
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::AutoTypeParamConstraintUnevaluated)),
            "anti-vacuity guard: the compile-time stub must not emit \
             AutoTypeParamConstraintUnevaluated (gated on \
             !is_compile_time_stub(), which the stub fails) — if it now \
             does, this fixture no longer isolates the real-checker-only \
             behaviour and this test would pass vacuously; stub \
             diagnostics: {:#?}",
            stub.diagnostics
        );
        let real = reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);
        assert!(
            real.diagnostics
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::AutoTypeParamConstraintUnevaluated)),
            "anti-vacuity guard: the real checker must emit \
             AutoTypeParamConstraintUnevaluated on GAP_C_SRC — if it no \
             longer does, this fixture has stopped exercising Gap-C and \
             this test would pass vacuously; real-checker diagnostics: \
             {:#?}",
            real.diagnostics
        );

        // --- Stateless surface: compute_diagnostics ---
        let diags = compute_diagnostics(GAP_C_SRC, &test_uri());
        assert!(
            diags.iter().any(|d| d.code == unevaluated_code),
            "BT8 amendment (compute_diagnostics): must surface \
             AutoTypeParamConstraintUnevaluated now that the real checker \
             is injected, matching `reify check`; got diagnostics: {:#?}",
            diags
        );

        // --- Stateful surface: compute_diagnostics_with_state (the live server's path) ---
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, GAP_C_SRC, &test_uri());
        assert!(
            result.diagnostics.iter().any(|d| d.code == unevaluated_code),
            "BT8 amendment (compute_diagnostics_with_state): must surface \
             AutoTypeParamConstraintUnevaluated now that the real checker \
             is injected, matching `reify check`; got diagnostics: {:#?}",
            result.diagnostics
        );
    }

    /// The containment guard's NARROWNESS, pinned at the PRODUCTION ENTRY
    /// POINTS rather than only on the predicate.
    ///
    /// The predicate-level test above proves
    /// `eval_guard::compiled_graph_has_unrepresentable_cell` answers `false` for a
    /// non-`auto:` compile error. It cannot prove the CALL SITES ask it: a
    /// future edit widening either site to `reify check`'s blanket
    /// `compiled.diagnostics.iter().any(|d| d.severity == Severity::Error)`
    /// gate would leave that test green and silently stop every eval-time
    /// diagnostic on any document carrying a compile error. This test is the
    /// wiring-level half — it goes RED on exactly that edit.
    ///
    /// Asserts BOTH halves reach the LSP output: the compile-stage
    /// `UnresolvedName` error (the LSP does not swallow it) and the eval-stage
    /// circular let-binding (the LSP evaluated THROUGH it, on purpose).
    #[test]
    fn non_auto_compile_error_still_yields_eval_diagnostics() {
        let unresolved_code = Some(lsp_types::NumberOrString::String(
            "UnresolvedName".to_string(),
        ));
        let has_compile_error =
            |diags: &[lsp_types::Diagnostic]| diags.iter().any(|d| d.code == unresolved_code);
        // The engine's exact message: "circular let-binding dependency in
        // template S: [a, b]" — matched the same way
        // `eval_diagnostics_surfaced_in_stateful_pipeline` matches it.
        let has_eval_diag = |diags: &[lsp_types::Diagnostic]| {
            diags.iter().any(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.contains("circular let-binding dependency")
                    && d.message.contains("in template S")
            })
        };

        // --- Stateless surface: compute_diagnostics ---
        let diags = compute_diagnostics(NON_AUTO_COMPILE_ERROR_WITH_EVAL_DIAG_SRC, &test_uri());
        assert!(
            has_compile_error(&diags) && has_eval_diag(&diags),
            "narrowness (compute_diagnostics): an Error-severity compile \
             diagnostic that is NOT an `auto:` failure must not suppress the \
             eval pass — the LSP evaluates through non-fatal compile errors on \
             purpose. Missing the eval diagnostic means a call site has drifted \
             into `reify check`'s blanket error gate. got: {:#?}",
            diags
        );

        // --- Stateful surface: compute_diagnostics_with_state (live server) ---
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(
            &mut state,
            NON_AUTO_COMPILE_ERROR_WITH_EVAL_DIAG_SRC,
            &test_uri(),
        );
        assert!(
            has_compile_error(&result.diagnostics) && has_eval_diag(&result.diagnostics),
            "narrowness (compute_diagnostics_with_state): the live server's \
             keystroke path must keep surfacing eval-time diagnostics on a \
             document that also carries a non-`auto:` compile error. got: {:#?}",
            result.diagnostics
        );
    }

    /// The guard must not fire on a failed `auto:` resolution that is provably
    /// SAFE to evaluate — the measured OVER-FIRE of the `AutoTypeParam*`
    /// diagnostic-code proxy this guard replaced, pinned end-to-end.
    ///
    /// [`UNUSED_TYPEPARAM_AUTO_FAIL_WITH_EVAL_DIAGS_SRC`] leaves `T` UNUSED in
    /// `Bearing`'s body, so the failed resolution creates no `TypeParam`-typed
    /// value cell and there is nothing for
    /// `assert_value_cell_types_representable` to panic on. Under the code
    /// proxy the single `auto:` clause nonetheless blanked the whole
    /// document's eval pass; all three findings below were lost.
    ///
    /// Anti-vacuity: the fixture must still actually FAIL `auto:` resolution
    /// (asserted via the compile-stage `AutoTypeParamNoCandidate`), otherwise
    /// "the eval diagnostics survived" would be trivially true and this test
    /// would stop being about the guard at all.
    #[test]
    fn failed_auto_resolution_with_unused_type_param_still_yields_eval_diagnostics() {
        let no_candidate_code = Some(lsp_types::NumberOrString::String(
            "AutoTypeParamNoCandidate".to_string(),
        ));
        let check = |label: &str, diags: &[lsp_types::Diagnostic]| {
            assert!(
                diags.iter().any(|d| d.code == no_candidate_code),
                "anti-vacuity ({label}): the fixture must still FAIL `auto:` \
                 resolution, or this test no longer exercises the guard at \
                 all. got: {diags:#?}"
            );
            for needle in [
                "circular let-binding dependency in template Other",
                "constraint Other#constraint[0] violated",
                "constraint Bearing#constraint[0] violated",
            ] {
                assert!(
                    diags.iter().any(|d| d.message.contains(needle)),
                    "over-fire ({label}): a failed `auto:` resolution with an \
                     UNUSED type parameter is safe to evaluate, so `{needle}` \
                     must still reach the editor. Its absence means the \
                     containment guard has regressed to a diagnostic-code \
                     proxy and one `auto:` clause is blanking eval for the \
                     whole document. got: {diags:#?}"
                );
            }
        };

        // --- Stateless surface: compute_diagnostics ---
        check(
            "compute_diagnostics",
            &compute_diagnostics(UNUSED_TYPEPARAM_AUTO_FAIL_WITH_EVAL_DIAGS_SRC, &test_uri()),
        );

        // --- Stateful surface: compute_diagnostics_with_state (live server) ---
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(
            &mut state,
            UNUSED_TYPEPARAM_AUTO_FAIL_WITH_EVAL_DIAGS_SRC,
            &test_uri(),
        );
        check("compute_diagnostics_with_state", &result.diagnostics);
    }

    /// Containment regression lock for the two entry points in this file
    /// (root cause owned by task **#6851**).
    ///
    /// A failed `auto:` type-parameter resolution leaves a `param seal : T`
    /// member carrying `cell_type = Type::TypeParam("T")` into the evaluation
    /// graph, where `reify-eval`'s `#[cfg(debug_assertions)]`
    /// `assert_value_cell_types_representable` PANICS
    /// ("unrepresentable cell_type: value cell `Assembly.b.seal` has
    /// cell_type TypeParam(\"T\")", `crates/reify-eval/src/engine_eval.rs`).
    /// The root-cause fix is owned by task **#6851**; this test locks the
    /// LSP-side containment
    /// ([`super::eval_guard::compiled_graph_has_unrepresentable_cell`]),
    /// which skips the eval/check pass so the language server survives and
    /// still reports the compile-stage error.
    ///
    /// **Reaching the assertions AT ALL is half the contract.** Without the
    /// guard these two calls PANIC before they can return, so "the test ran to
    /// completion" IS the no-panic assertion. Wrapping either call in
    /// `#[should_panic]` or `catch_unwind` would invert the contract this test
    /// exists to state — do not.
    ///
    /// **Two fixtures, one body.** The `param seal : T` member is exercised
    /// both plain and inside a GUARDED group, because the containment
    /// predicate reaches them through different collections: a guarded member
    /// lives in `CompiledGuardedGroup::members`, never in
    /// `TopologyTemplate::value_cells`, and was measured to slip past the
    /// predicate's stage-1 absence proof and panic here at
    /// `crates/reify-eval/src/engine_eval.rs:210` —
    /// "unrepresentable cell_type: value cell `Bearing.seal` has cell_type
    /// TypeParam(\"T\")". Parameterised over a table rather than copied, so the
    /// two shapes' contracts cannot drift apart; every assertion below is
    /// load-bearing for both.
    #[test]
    fn auto_resolution_failure_does_not_panic_diagnostics_entry_points() {
        // Each fixture carries its OWN anti-vacuity guard: both claim "stub
        // clean, real checker fails resolution", but each names its own
        // fixture when that stops being true.
        let fixtures: [(&str, &str, fn()); 2] = [
            (
                "plain member",
                AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC,
                assert_auto_fail_fixture_is_newly_reachable,
            ),
            (
                "guarded-group member",
                GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC,
                assert_guarded_group_fixture_is_newly_reachable,
            ),
        ];

        for (shape, src, assert_fixture_is_newly_reachable) in fixtures {
            // --- Anti-vacuity guard: stub clean, real checker fails resolution ---
            assert_fixture_is_newly_reachable();

            let no_candidate_code = Some(lsp_types::NumberOrString::String(
                "AutoTypeParamNoCandidate".to_string(),
            ));

            // --- Stateless surface: compute_diagnostics ---
            // Panics at engine_eval.rs without the containment guard (measured).
            let diags = compute_diagnostics(src, &test_uri());
            assert!(
                diags.iter().any(|d| d.code == no_candidate_code),
                "containment (compute_diagnostics, {shape}): a failed `auto:` resolution \
                 must still surface the compile-stage AutoTypeParamNoCandidate \
                 error — reaching this assertion at all means the eval pass was \
                 correctly skipped rather than panicking on the unsubstituted \
                 TypeParam cell (task #6851). A panic here means the containment \
                 guard has regressed; an empty/miscoded result means the guard \
                 fires too early and swallows the compile diagnostics. got: {:#?}",
                diags
            );

            // --- Stateful surface: compute_diagnostics_with_state (live server) ---
            //
            // Snapshot `EvalState` first: the guard's placement BEFORE the state
            // mutations is load-bearing, and nothing else pins it. If a refactor
            // moved `state.version_counter += 1` or the `last_content_hash`
            // assignment above the guard, this test would still be green on
            // `result.diagnostics` alone while the SECOND request on the same
            // unchanged document took the `content_unchanged` branch into
            // `eval_cached` — which returns empty diagnostics by construction —
            // and silently reported no eval diagnostics for a module that was
            // never evaluated at all.
            let mut state = EvalState::new();
            let hash_before = state.last_content_hash;
            let version_before = state.version_counter;
            let initialized_before = state.is_engine_initialized();

            let result = compute_diagnostics_with_state(&mut state, src, &test_uri());

            assert_eq!(
                state.last_content_hash, hash_before,
                "containment ({shape}): a guard-suppressed request must leave \
                 `last_content_hash` UNADVANCED, so the next request on the same \
                 document takes the cold-start branch rather than `eval_cached` \
                 (which returns empty diagnostics by construction). Advancing it \
                 here is the stale-cache bug the guard's placement prevents."
            );
            assert_eq!(
                state.version_counter, version_before,
                "containment ({shape}): a guard-suppressed request evaluated nothing, so \
                 it must not burn a version. A bumped counter means the guard drifted \
                 BELOW `state.version_counter += 1`."
            );
            assert_eq!(
                state.is_engine_initialized(),
                initialized_before,
                "containment ({shape}): a guard-suppressed request must leave the engine \
                 UNINITIALIZED — it never fed it a snapshot. An initialized engine \
                 here means the guard drifted below the eval call it exists to \
                 skip, i.e. the panic it contains is reachable again."
            );

            assert!(
                result
                    .diagnostics
                    .iter()
                    .any(|d| d.code == no_candidate_code),
                "containment (compute_diagnostics_with_state, {shape}): the live \
                 server's keystroke path must still surface the compile-stage \
                 AutoTypeParamNoCandidate error without feeding the unsubstituted \
                 TypeParam graph to the engine (task #6851). A panic here means \
                 the containment guard has regressed. got: {:#?}",
                result.diagnostics
            );
        }
    }

    /// Containment on the two COMPILE-CLEAN shapes — the ones on which the
    /// guard has no compile diagnostic to ride on, and must therefore say
    /// something in the editor itself.
    ///
    /// Every other containment test in this file drives a fixture that also
    /// carries an `AutoTypeParamNoCandidate` Error, so each of their
    /// assertions reads "the compile-stage error still surfaces". That leaves
    /// the guard's own "## Blast radius" claim — that it contains two shapes
    /// with NO compile diagnostic at all — asserted nowhere, and it is the
    /// claim under which the guard is at its most consequential: on these
    /// inputs the file is valid `.ri` that the compiler accepted, and the user
    /// silently loses every eval-time diagnostic, every constraint result and
    /// (via the same guard in `AnalysisContext::from_parsed`) every
    /// hover/completion computed value for the whole document.
    ///
    /// So this test pins three things per entry point: no panic (reaching the
    /// assertions IS that half of the contract — do not add `#[should_panic]`
    /// or `catch_unwind`), no Error-severity diagnostic to explain the loss,
    /// and consequently the file-level `EvalSkippedUnrepresentableCell`
    /// Warning naming the offending cell. Drop the Warning and the third
    /// assertion goes red.
    #[test]
    fn compile_clean_unrepresentable_graph_is_contained_and_reported() {
        // Anti-vacuity: both fixtures must still compile CLEAN under both
        // checkers, or "the editor is told nothing else" is not the situation
        // under test.
        assert_compile_clean_typeparam_fixtures_carry_no_error();

        let skip_code = Some(lsp_types::NumberOrString::String(
            "EvalSkippedUnrepresentableCell".to_string(),
        ));

        for (shape, src) in [
            ("declared-only generic", DECLARED_ONLY_GENERIC_TYPEPARAM_SRC),
            (
                "explicit generic instantiation",
                EXPLICIT_GENERIC_INSTANTIATION_TYPEPARAM_SRC,
            ),
        ] {
            for (entry_point, diags) in [
                ("compute_diagnostics", compute_diagnostics(src, &test_uri())),
                (
                    "compute_diagnostics_with_state",
                    compute_diagnostics_with_state(&mut EvalState::new(), src, &test_uri())
                        .diagnostics,
                ),
            ] {
                let errors: Vec<_> = diags
                    .iter()
                    .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
                    .collect();
                assert!(
                    errors.is_empty(),
                    "({entry_point}, {shape}): this fixture is valid `.ri` the \
                     compiler accepts, which is the whole point — an Error here \
                     means the fixture drifted and the report assertion below \
                     could pass by riding on someone else's diagnostic. \
                     errors: {errors:#?}"
                );

                let reported: Vec<_> = diags.iter().filter(|d| d.code == skip_code).collect();
                assert_eq!(
                    reported.len(),
                    1,
                    "({entry_point}, {shape}): a guard-suppressed compile-clean \
                     document must carry exactly ONE editor-visible \
                     EvalSkippedUnrepresentableCell Warning. Zero means the \
                     document silently lost its whole eval pass — every \
                     constraint result and every computed value — with nothing \
                     but a line on the server's stderr to show for it. \
                     Reaching this assertion at all also means the eval pass \
                     was skipped rather than panicking on the unrepresentable \
                     cell (task #6851). got: {diags:#?}"
                );
                let reported = reported[0];
                assert_eq!(
                    reported.severity,
                    Some(DiagnosticSeverity::WARNING),
                    "({entry_point}, {shape}): the skip report is the LSP \
                     describing its own reduced service over a file the \
                     compiler accepted, not a defect in that file — Warning, \
                     not Error."
                );
                assert!(
                    reported.message.contains("seal"),
                    "({entry_point}, {shape}): the skip report must NAME the \
                     offending cell, or it tells the user only that something \
                     was lost. got: {}",
                    reported.message
                );
            }
        }
    }

    /// Regression guard for task 2525: `compute_diagnostics` must accept sources
    /// that reference stdlib enums (e.g. `CorrosionClass.C5`) WITHOUT inline
    /// redeclarations.
    ///
    /// Pre-task, the parser disambiguated `Type.Variant` against the current
    /// source's enum decls only, so the lowered AST carried `MemberAccess`
    /// instead of `EnumAccess` and the downstream `compile_with_stdlib` emitted
    /// an unresolved-name error diagnostic for `CorrosionClass`.
    #[test]
    fn compute_diagnostics_resolves_stdlib_enum_access_without_inline_redecl() {
        let source = "structure Sample {\n  let chosen_class = CorrosionClass.C5\n}\n";
        let diags = compute_diagnostics(source, &test_uri());
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors.is_empty(),
            "stdlib enum reference without inline redecl should produce no error diagnostics, got: {errors:?}"
        );
    }

    #[test]
    fn syntax_error_produces_diagnostic() {
        let source = "structure {";
        let diags = compute_diagnostics(source, &test_uri());
        assert!(!diags.is_empty(), "syntax error should produce diagnostics");
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
            "should have at least one error diagnostic"
        );
    }

    #[test]
    fn unknown_identifier_produces_diagnostic() {
        // Reference a non-existent type/name
        let source = "structure Foo {\n  param x: Length = unknown_name;\n}";
        let diags = compute_diagnostics(source, &test_uri());
        assert!(
            !diags.is_empty(),
            "unknown identifier should produce diagnostics"
        );
    }

    #[test]
    fn empty_source_no_crash() {
        let diags = compute_diagnostics("", &test_uri());
        // Should not panic; may or may not produce diagnostics
        let _ = diags;
    }

    /// End-to-end regression lock: a deep dot-chain in a `let` value flows
    /// through `compute_diagnostics` as an LSP Warning whose source is `reify`,
    /// whose message contains the rendered chain text, and whose range is
    /// non-zero (anchored to the chain's source span via the diagnostic label).
    ///
    /// This pins the LSP-side surface for spec §5.7's `DeepDotChain` lint.
    /// Conversion of the typed `DiagnosticCode::DeepDotChain` to the LSP
    /// `code` field is intentionally out-of-scope here — see plan.json
    /// design decision "Do NOT modify convert_diagnostic to populate
    /// lsp_types::Diagnostic.code" — so we assert on severity, source,
    /// message text, and a non-zero range only.
    #[test]
    fn lsp_compute_diagnostics_surfaces_deep_dot_chain_warning() {
        // 6-segment chain `a.b.c.d.e.f` (length 6 > 4) inside a `let` value
        // forces exactly one DeepDotChain warning from the compiler pre-pass.
        let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e.f
}
"#;
        let diags = compute_diagnostics(source, &test_uri());

        let deep_dot_chain_diags: Vec<_> = diags
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::WARNING) && d.message.contains("a.b.c.d.e.f")
            })
            .collect();

        assert_eq!(
            deep_dot_chain_diags.len(),
            1,
            "expected exactly 1 LSP Warning with chain text `a.b.c.d.e.f`, got {}: {:#?}",
            deep_dot_chain_diags.len(),
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let diag = deep_dot_chain_diags[0];
        assert_eq!(
            diag.severity,
            Some(DiagnosticSeverity::WARNING),
            "expected WARNING severity, got {:?}",
            diag.severity
        );
        assert_eq!(
            diag.source.as_deref(),
            Some("reify"),
            "expected source `reify`, got {:?}",
            diag.source
        );
        // Range must be non-zero — the diagnostic carries a label whose span
        // covers the entire `a.b.c.d.e.f` chain. A zero range would mean the
        // label was dropped and convert_diagnostic fell back to (0,0)-(0,0).
        let range_is_zero = diag.range.start == diag.range.end;
        assert!(
            !range_is_zero,
            "expected non-zero diagnostic range (label span should anchor to \
             chain), got start={:?} end={:?}",
            diag.range.start, diag.range.end
        );
    }

    // --- compute_diagnostics_with_state unit tests (step-25) ---

    #[test]
    fn eval_state_new_starts_with_version_counter_zero() {
        let state = EvalState::new();
        assert_eq!(state.version_counter, 0);
    }

    #[test]
    fn stateful_diagnostics_three_phase_lifecycle() {
        let mut state = EvalState::new();
        let uri = test_uri();

        // Phase 1: valid source — no ERROR diagnostics
        let source_valid = reify_test_support::bracket_source();
        let result1 = compute_diagnostics_with_state(&mut state, source_valid, &uri);
        let errors1: Vec<_> = result1
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors1.is_empty(),
            "Phase 1: valid source should produce no errors, got: {errors1:?}"
        );

        // Phase 2: violating source — at least one constraint violation ERROR
        let source_violating = reify_test_support::bracket_source_violating();
        let result2 = compute_diagnostics_with_state(&mut state, &source_violating, &uri);
        let errors2: Vec<_> = result2
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            !errors2.is_empty(),
            "Phase 2: violating source should produce at least one ERROR"
        );
        let has_constraint_violation = errors2.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("constraint") && msg.contains("violated")
        });
        assert!(
            has_constraint_violation,
            "Phase 2: should have a 'constraint violated' diagnostic, got: {errors2:?}"
        );

        // Phase 3: back to valid source — violations should clear
        let result3 = compute_diagnostics_with_state(&mut state, source_valid, &uri);
        let errors3: Vec<_> = result3
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors3.is_empty(),
            "Phase 3: valid source should clear violations, got: {errors3:?}"
        );

        // Verify version_counter persistence: 3 calls = version_counter 3
        assert_eq!(
            state.version_counter, 3,
            "version_counter should be 3 after three calls"
        );
    }

    // --- check_snapshot fallback robustness tests (step-27) ---

    #[test]
    fn fresh_engine_check_snapshot_returns_none() {
        // A fresh Engine (without prior eval) should have no snapshot
        let checker = SimpleConstraintChecker;
        let engine = reify_eval::Engine::new(Box::new(checker), None);
        let source = reify_test_support::bracket_source();
        let parsed = reify_syntax::parse(source, ModulePath::single("bracket"));
        let compiled = reify_compiler::compile(&parsed);
        let result = engine.check_snapshot(&compiled);
        assert!(
            result.is_none(),
            "fresh Engine without eval should return None from check_snapshot"
        );
    }

    #[test]
    fn stateful_violating_source_always_produces_constraint_violation() {
        // Regression guard: constraint violations must never be silently skipped
        let mut state = EvalState::new();
        let uri = test_uri();
        let source_violating = reify_test_support::bracket_source_violating();
        let result = compute_diagnostics_with_state(&mut state, &source_violating, &uri);
        let constraint_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.to_lowercase().contains("constraint")
                    && d.message.to_lowercase().contains("violated")
            })
            .collect();
        assert!(
            !constraint_errors.is_empty(),
            "violating source must always produce at least one constraint violation ERROR, got diagnostics: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn stateful_empty_source_does_not_panic() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let result = compute_diagnostics_with_state(&mut state, "", &uri);
        // Should not panic; result may contain parse errors but should be valid
        let _ = result;
    }

    // --- parse error early return tests (step-6 / Task 497) ---

    /// When there are parse errors, compile/eval may produce misleading secondary
    /// diagnostics on a broken AST. After the early return added in step-7, the
    /// result should contain exactly the parse error diagnostics — no more.
    #[test]
    fn parse_error_skips_compile_and_eval() {
        let source = "structure {";
        let uri = test_uri();

        // Count parse errors directly using reify_syntax
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let parse_error_count = parsed.errors.len();
        assert!(
            parse_error_count > 0,
            "test source must produce at least one parse error"
        );

        // compute_diagnostics_with_state should return only parse-error diagnostics
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, source, &uri);
        assert_eq!(
            result.diagnostics.len(),
            parse_error_count,
            "on parse error, diagnostics count ({}) should equal parse error count ({}); \
             secondary compile/eval diagnostics must not be included",
            result.diagnostics.len(),
            parse_error_count
        );
        assert!(
            result.geometry_output.is_none(),
            "geometry_output should be None when parse errors exist (eval must be skipped)"
        );
        for diag in &result.diagnostics {
            assert_eq!(
                diag.severity,
                Some(DiagnosticSeverity::ERROR),
                "all parse-error diagnostics must have severity ERROR, got: {:?}",
                diag.severity
            );
        }
    }

    // --- task-2176 step-5: stateful diagnostics resolve stdlib types ---

    #[test]
    fn stateful_diagnostics_resolve_stdlib_material_and_rigid() {
        // Drives the stateful compute_diagnostics_with_state() path.
        // A known-good stdlib source must produce zero UNEXPECTED error-severity diagnostics.
        //
        // The `Rigid` trait (via `Physical`) injects `let centroid = centroid(geometry)` and
        // similar geometry-consumer builtins into conforming structures.  Since task #4651
        // (R1a), those cells correctly emit EvalUnresolved at Error severity on the kernel-less
        // eval() / eval_cached() surface — they require a realized geometry kernel that is not
        // present here.  Filter those out; any remaining Error-severity diagnostic is unexpected.
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, STDLIB_PROBE_SRC, &test_uri());
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.code
                        != Some(lsp_types::NumberOrString::String(
                            "EvalUnresolved".to_string(),
                        ))
            })
            .collect();
        assert!(
            errors.is_empty(),
            "stateful pipeline: stdlib source should compile without unexpected errors; got: {errors:?}"
        );
    }

    // --- task-2176 step-3: stateless diagnostics resolve stdlib types ---

    #[test]
    fn compute_diagnostics_resolves_stdlib_material_and_rigid() {
        // Drives the stateless compute_diagnostics() path.
        // A known-good stdlib source must produce zero UNEXPECTED error-severity diagnostics.
        //
        // The `Rigid` trait (via `Physical`) injects `let centroid = centroid(geometry)` and
        // similar geometry-consumer builtins into conforming structures.  Since task #4651
        // (R1a), those cells correctly emit EvalUnresolved at Error severity on the kernel-less
        // eval() surface — they require a realized geometry kernel that is not present here.
        // Filter those out; any remaining Error-severity diagnostic is unexpected.
        let diags = compute_diagnostics(STDLIB_PROBE_SRC, &test_uri());
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.code
                        != Some(lsp_types::NumberOrString::String(
                            "EvalUnresolved".to_string(),
                        ))
            })
            .collect();
        assert!(
            errors.is_empty(),
            "stdlib source should compile without unexpected errors; got: {errors:?}"
        );
    }

    // --- step-5: cold-start fallback regression lock ---

    #[test]
    fn structural_change_detects_violations_and_updates_content_hash() {
        let uri = test_uri();

        // (1) First call with valid source — no ERROR diagnostics
        let mut state = EvalState::new();
        let source_valid = reify_test_support::bracket_source();
        let result1 = compute_diagnostics_with_state(&mut state, source_valid, &uri);
        let errors1: Vec<_> = result1
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors1.is_empty(),
            "Phase 1: valid source should produce no errors, got: {errors1:?}"
        );
        let hash_after_valid = state.last_content_hash();

        // (2) Second call with violating source (different content_hash) — at least one ERROR
        let source_violating = reify_test_support::bracket_source_violating();
        let result2 = compute_diagnostics_with_state(&mut state, &source_violating, &uri);
        let errors2: Vec<_> = result2
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.to_lowercase().contains("constraint")
                    && d.message.to_lowercase().contains("violated")
            })
            .collect();
        assert!(
            !errors2.is_empty(),
            "Phase 2: violating source should produce at least one constraint violation ERROR"
        );

        // (3) The content hash in state must have changed — an LSP-layer invariant.
        //     Whether cold-start or eval_cached was used internally is an engine-level
        //     detail; diagnostic correctness (assertions 1 and 2) is the behavioral
        //     contract. This assertion locks the state-management invariant that
        //     last_content_hash() always reflects the most recently evaluated source.
        assert_ne!(
            hash_after_valid,
            state.last_content_hash(),
            "last_content_hash must update when source changes"
        );
    }

    // --- step-1 (task-2236): eval_diagnostics_surfaced_in_stateful_pipeline ---

    /// Eval-time diagnostics (e.g. circular let-binding) must appear in the LSP result.
    ///
    /// `structure S { let a = b + 1; let b = a + 1 }` has a cyclic let-binding
    /// dependency that is NOT detected at compile time (only geometry-let cycles
    /// are caught in the compiler). The engine catches it inside
    /// `evaluate_let_bindings` (engine_eval.rs:1529) and records it in
    /// `EvalResult::diagnostics` as "circular let-binding dependency in template S: [a, b]".
    #[test]
    fn eval_diagnostics_surfaced_in_stateful_pipeline() {
        let mut state = EvalState::new();
        let uri = test_uri();
        // Cyclic let-bindings: `a` depends on `b` and `b` depends on `a`.
        let source = "structure S {\n    let a = b + 1\n    let b = a + 1\n}";
        let result = compute_diagnostics_with_state(&mut state, source, &uri);
        let circular_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    // Matches the exact engine message: "circular let-binding dependency in template S: [a, b]"
                    && d.message.contains("circular let-binding dependency")
                    && d.message.contains("in template S")
            })
            .collect();
        assert!(
            !circular_errors.is_empty(),
            "eval-time circular let-binding diagnostic must be surfaced as an LSP ERROR; \
             got diagnostics: {:?}",
            result.diagnostics
        );
    }

    /// Invariant: an uninitialized engine must take the cold-start eval() branch,
    /// even when `last_content_hash` already matches the compiled module's hash.
    ///
    /// This guards against a future decoupling of EvalState's `last_content_hash`
    /// and engine-initialization state: if `last_content_hash` is set without
    /// initializing the engine (e.g. by a new code path), `eval_cached()` would
    /// silently return empty diagnostics — dropping eval-time errors. The
    /// `content_unchanged` predicate must AND in `is_engine_initialized()` to
    /// guarantee the cold-start branch runs whenever the engine is not ready.
    #[test]
    fn cold_start_branch_taken_when_engine_uninitialized_with_matching_hash() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let source = "structure S {\n    let a = b + 1\n    let b = a + 1\n}";

        // Pre-compile to obtain the content_hash for this exact source.
        // Must use compile_like_production + ModulePath::single("test") to
        // match what compute_diagnostics_with_state derives from
        // "file:///test.ri" — compile_like_production is what production
        // actually calls.
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);

        // Inject a matching hash while leaving the engine uninitialized.
        // (private-field write from child mod — same pattern as the
        //  state.version_counter assertion at line 330)
        state.last_content_hash = Some(compiled.content_hash);

        // Sanity: engine must not be initialized — this is the precondition
        // for the bug we are guarding against.
        assert!(
            !state.is_engine_initialized(),
            "engine must be uninitialized after EvalState::new() + hash injection"
        );

        let result = compute_diagnostics_with_state(&mut state, source, &uri);

        // The cold-start eval() branch must run and surface the circular error.
        // On buggy code (before the fix): content_unchanged=true → eval_cached()
        // → empty diagnostics → this assertion fails.
        let circular_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.contains("circular let-binding dependency")
                    && d.message.contains("in template S")
            })
            .collect();
        assert!(
            !circular_errors.is_empty(),
            "cold-start branch must be taken when engine is uninitialized, \
             surfacing the circular let-binding diagnostic; \
             got diagnostics: {:?}",
            result.diagnostics
        );
    }

    /// Canary: `eval_cached()` currently returns empty diagnostics by construction
    /// (engine_eval.rs:1183 — `let diagnostics = Vec::new()` is never appended to).
    ///
    /// This test asserts *current* behavior so it fails loudly the moment
    /// `eval_cached` starts emitting diagnostics — that failure is the expected
    /// signal to update the assertion (flip `is_empty()` → `!is_empty()`).
    /// An `#[ignore]`'d future-state test would bitrot silently; a canary that
    /// asserts today's behavior forces maintainer attention at the right time.
    #[test]
    fn eval_cached_path_surfaces_circular_let_binding_when_fixed() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let source = "structure S {\n    let a = b + 1\n    let b = a + 1\n}";

        // First call: cold-start eval() — must surface the circular let-binding diagnostic.
        let result1 = compute_diagnostics_with_state(&mut state, source, &uri);
        let has_circular_on_cold_start = result1.diagnostics.iter().any(|d| {
            d.severity == Some(DiagnosticSeverity::ERROR)
                && d.message.contains("circular let-binding dependency")
                && d.message.contains("in template S")
        });
        assert!(
            has_circular_on_cold_start,
            "cold-start call must surface circular let-binding diagnostic; got: {:?}",
            result1.diagnostics
        );

        // Second call: same source → content_unchanged=true → eval_cached path.
        // eval_cached() now emits cycle diagnostics (task 2259 fixed the immutable
        // `let diagnostics = Vec::new()` and inserted per-template cycle detection).
        let result2 = compute_diagnostics_with_state(&mut state, source, &uri);
        let circular_on_cached_path: Vec<_> = result2
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.contains("circular let-binding dependency")
                    && d.message.contains("in template S")
            })
            .collect();
        assert!(
            !circular_on_cached_path.is_empty(),
            "eval_cached() must surface the circular let-binding diagnostic on cached path; \
             got: {:?}",
            result2.diagnostics,
        );
    }

    // --- step-3: eval_cached path via basis_version ---

    #[test]
    fn incremental_path_uses_eval_cached_when_content_unchanged() {
        use reify_eval::cache::NodeId;
        use reify_core::ValueCellId;

        let uri = test_uri();
        let source = reify_test_support::bracket_source();

        // (1) First call: cold-start
        let mut state = EvalState::new();
        compute_diagnostics_with_state(&mut state, source, &uri);
        assert_eq!(
            state.version_counter, 1,
            "version_counter should be 1 after first call"
        );

        // (2) Second call with identical source: should use eval_cached path
        compute_diagnostics_with_state(&mut state, source, &uri);
        assert_eq!(
            state.version_counter, 2,
            "version_counter should be 2 after second call"
        );

        // (3) Inspect cache: basis_version of Bracket.width should be > 0
        //     eval_cached passes VersionId(state.version_counter) which is VersionId(2) at call time
        //     (counter is incremented to 2 before eval_cached is called).
        //     A cold-start would reset the engine to a fresh state with basis_version=0.
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        let entry = state
            .engine
            .cache_store()
            .get(&node)
            .expect("Bracket.width cache entry must exist after eval");
        assert!(
            entry.basis_version.0 > 0,
            "eval_cached path should bump basis_version > 0; cold-start path would reset to 0, got {}",
            entry.basis_version.0
        );
    }

    // --- constraint violation diagnostic range tests (step-31) ---

    #[test]
    fn constraint_violation_diagnostic_has_correct_range() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let source_violating = reify_test_support::bracket_source_violating();
        let result = compute_diagnostics_with_state(&mut state, &source_violating, &uri);

        // Find the constraint violation ERROR diagnostic
        let violation_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.to_lowercase().contains("constraint")
                    && d.message.to_lowercase().contains("violated")
            })
            .collect();

        assert!(
            !violation_diags.is_empty(),
            "should have at least one constraint violation diagnostic"
        );

        for diag in &violation_diags {
            // Constraints are on lines 7-9 of bracket source (0-indexed), not line 0
            assert!(
                diag.range.start.line > 0,
                "constraint violation range should not be on line 0, got range: {:?}",
                diag.range
            );
            assert_ne!(
                diag.range,
                lsp_types::Range::default(),
                "constraint violation range should not be Range::default() (0,0)→(0,0)"
            );
        }
    }

    // --- forall per-element constraint violation regression lock ---

    /// Regression-lock test: a `forall` constraint that fails for every element of its
    /// iteration set must surface exactly one ERROR diagnostic per element, with the
    /// element index encoded in the diagnostic message as `forall@<var>[<idx>]`.
    ///
    /// This pins the end-to-end contract for PRD criterion 10
    /// (docs/prds/forall-statement-form.md): the compiler emits one
    /// `CompiledConstraint` per element with label `forall@v[<idx>]`
    /// (see `forall_elaborate.rs`, the `forall@<var>[<idx>]` label format), and the LSP
    /// surfaces that label verbatim via `format!("constraint violated: {}", label)`
    /// in `compute_diagnostics_with_state`.
    ///
    /// The compiler-layer counterpart is `forall_constraint_label_encodes_element_index`
    /// (crates/reify-compiler/tests/forall_statement_lower_tests.rs).
    /// Together the two tests pin the contract end-to-end.
    #[test]
    fn forall_per_element_constraint_violation_surfaces_element_index() {
        let source = "structure S {\n    forall v in [1, 2, 3]: constraint v > 100\n}";
        let mut state = EvalState::new();
        let uri = test_uri();

        let result = compute_diagnostics_with_state(&mut state, source, &uri);

        // Collect ERROR diagnostics whose message encodes the forall element index.
        let forall_violation_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.starts_with("constraint violated: forall@v[")
            })
            .collect();

        // Guard: if any ERROR diagnostic is NOT a constraint-violation, the source
        // likely failed to parse or compile.  Fail fast with the full dump so the real
        // root cause is visible rather than a misleading count-mismatch on an empty list.
        let parse_or_compile_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && !d.message.starts_with("constraint violated:")
            })
            .collect();
        assert!(
            parse_or_compile_errors.is_empty(),
            "unexpected non-constraint ERROR diagnostics (parse/compile failure?): {:#?}",
            parse_or_compile_errors
        );

        // All three elements (indices 0, 1, 2) must appear — one diagnostic each.
        assert_eq!(
            forall_violation_diags.len(),
            3,
            "expected exactly 3 forall per-element violation diagnostics (one per index 0..=2); \
             got {}: {:#?}",
            forall_violation_diags.len(),
            forall_violation_diags
        );

        // Each index must be present exactly once.
        for idx in 0..3usize {
            let substr = format!("forall@v[{}]", idx);
            let count = forall_violation_diags
                .iter()
                .filter(|d| d.message.contains(&substr))
                .count();
            assert_eq!(
                count, 1,
                "expected exactly one diagnostic containing \"{}\"; got {}; diagnostics: {:#?}",
                substr, count, forall_violation_diags
            );
        }

        // Each per-element diagnostic must carry source == "reify".
        for diag in &forall_violation_diags {
            assert_eq!(
                diag.source,
                Some("reify".to_string()),
                "per-element forall violation diagnostic must have source == \"reify\"; got: {:#?}",
                diag
            );
        }

        // Sanity: constraint violations must NOT route through the freshness channel
        // (arch §9.3); no `computation-failed` diagnostic may be present.
        let computation_failed_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code
                    == Some(lsp_types::NumberOrString::String(
                        "computation-failed".to_string(),
                    ))
            })
            .collect();
        assert!(
            computation_failed_diags.is_empty(),
            "forall constraint violations must NEVER produce 'computation-failed' diagnostics \
             (arch §9.3 separation); got: {:#?}",
            computation_failed_diags
        );
    }

    // --- step-5 regression lock: eval diagnostics never use constraint-violation format ---

    /// Regression lock — circular let-binding emitter: `eval()` must never emit diagnostics
    /// in the `"constraint ... violated"` format checked by the inline `strip_prefix /
    /// strip_suffix / !contains(' ')` filter used throughout this cluster.
    ///
    /// This is the first of a six-test cluster (one per known eval-time emitter) that
    /// locks the invariant enabling the unfiltered merge of `eval_diagnostics` in
    /// `compute_diagnostics_with_state`.
    ///
    /// If this test fails, `eval()` has started emitting the constraint-violation format
    /// from the circular-let-binding path (`unfold.rs` / `engine_eval.rs`) — add a
    /// filter on the eval merge in `compute_diagnostics_with_state` or update the merge loop.
    #[test]
    fn eval_diagnostics_never_use_constraint_violation_format() {
        // Use circular-let-binding source: a known eval-time diagnostic emitter
        // (the unfold.rs / engine_eval.rs circular let-binding paths).
        let source = "structure S {\n    let a = b + 1\n    let b = a + 1\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);

        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        let result = engine.eval(&compiled);

        // Sanity: eval must emit at least one diagnostic for the circular let-binding
        // so the negative assertion below cannot pass vacuously.
        assert!(
            !result.diagnostics.is_empty(),
            "eval() must emit at least one diagnostic for circular-let-binding source; \
             got none — check that the source is still erroneous"
        );

        for diag in &result.diagnostics {
            let is_violation_format = diag
                .message
                .strip_prefix("constraint ")
                .and_then(|s| s.strip_suffix(" violated"))
                .is_some_and(|id| !id.is_empty() && !id.contains(' '));
            assert!(
                !is_violation_format,
                "eval() emitted a 'constraint ... violated' format message: {:?}. \
                 The compute_diagnostics_with_state merge loop relies on eval diagnostics \
                 never using this format — add a filter on the eval merge or update the loop.",
                diag.message
            );
        }
    }

    /// Negative-assertion helper: asserts that none of `diags` match the inline
    /// `strip_prefix("constraint ") / strip_suffix(" violated") / !contains(' ')` format
    /// that `compute_diagnostics_with_state` relies on never appearing in eval output.
    /// `label` is embedded in the panic message to identify which emitter path failed.
    fn assert_no_violation_format(diags: &[Diagnostic], label: &str) {
        for diag in diags {
            let is_violation_format = diag
                .message
                .strip_prefix("constraint ")
                .and_then(|s| s.strip_suffix(" violated"))
                .is_some_and(|id| !id.is_empty() && !id.contains(' '));
            assert!(
                !is_violation_format,
                "[{label}] eval() emitted a 'constraint ... violated' format message: {:?}. \
                 The compute_diagnostics_with_state merge loop relies on eval diagnostics \
                 never using this format — add a filter on the eval merge or update the loop.",
                diag.message
            );
        }
    }

    /// Shared setup for the two param-override emitter tests.
    ///
    /// Parses and compiles `"structure S { param width: Length = 100mm }"`, does an initial
    /// eval to warm the engine state, then overrides `width` with `override_value` and returns
    /// the diagnostics from the second eval.
    fn build_param_override_diags(override_value: Value) -> Vec<Diagnostic> {
        let source = "structure S { param width: Length = 100mm }";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);
        let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
        let _ = engine.eval(&compiled);
        engine.set_param_and_invalidate(&ValueCellId::new("S", "width"), override_value);
        engine.eval(&compiled).diagnostics
    }

    /// Shared setup for the two solver pass-through emitter tests.
    ///
    /// Parses and compiles the `"auto" + constraint-on-x` source and installs `solver`.
    /// Returns `(counter, diagnostics)` where `counter` is the live `Arc<AtomicUsize>` from
    /// `solver.counter_handle()`, allowing callers to assert the solver was dispatched.
    fn run_solver_on_constrained_auto_param(
        solver: MockConstraintSolver,
    ) -> (Arc<AtomicUsize>, Vec<Diagnostic>) {
        let source = "structure S {\n    param x: Length = auto\n    constraint x > 1mm\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);
        let counter = solver.counter_handle();
        let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None)
            .with_solver(Box::new(solver));
        let diagnostics = engine.eval(&compiled).diagnostics;
        (counter, diagnostics)
    }

    /// Locks the `ConstraintNodeId` Display invariant independently of any emitter.
    ///
    /// The production format `format!("constraint {} violated", ConstraintNodeId::new("S", 0))`
    /// must satisfy the inline `strip_prefix / strip_suffix / !contains(' ')` check so that
    /// drift in `ConstraintNodeId::Display` trips this test before the negative checks in the
    /// per-emitter cluster.
    #[test]
    fn eval_diag_format_anchor() {
        let real_id = ConstraintNodeId::new("S", 0u32);
        let anchor = format!("constraint {} violated", real_id);
        assert!(
            anchor
                .strip_prefix("constraint ")
                .and_then(|s| s.strip_suffix(" violated"))
                .is_some_and(|id| !id.is_empty() && !id.contains(' ')),
            "anchor: ConstraintNodeId::new(\"S\", 0) formats as {real_id:?} which does not \
             match the inline constraint-violation check; if ConstraintNodeId Display changed, \
             update the inline check in this cluster and in \
             eval_diagnostics_never_use_constraint_violation_format."
        );
    }

    /// Per-emitter regression lock — param_override type-kind mismatch path
    /// (engine_eval.rs param_override type-kind path).
    ///
    /// Locks the invariant that `eval()` never emits the `"constraint ... violated"` format
    /// from the param_override type-kind mismatch emitter.
    /// Counter contract for this emitter lives in `crates/reify-eval/tests/eval_instrumentation_counters.rs`.
    #[test]
    fn eval_diag_format_param_override_type_kind() {
        let diags = build_param_override_diags(Value::Bool(true));

        assert!(
            !diags.is_empty(),
            "param_override_type_kind: engine emitted no diagnostics"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "param_override_type_kind: expected at least one Warning-severity diagnostic; \
             got: {:#?}",
            diags
        );
        // Substring discriminator: ensures the right emitter fired (the counter contract is
        // anchored at `crates/reify-eval/tests/eval_instrumentation_counters.rs`).
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("type-kind mismatch")),
            "param_override_type_kind: expected a diagnostic containing 'type-kind mismatch'; \
             got: {:#?}",
            diags
        );
        assert_no_violation_format(&diags, "param_override_type_kind");
    }

    /// Per-emitter regression lock — param_override dimension mismatch path
    /// (engine_eval.rs param_override dimension path).
    ///
    /// Locks the invariant that `eval()` never emits the `"constraint ... violated"` format
    /// from the param_override dimension mismatch emitter.
    /// Counter contract for this emitter lives in `crates/reify-eval/tests/eval_instrumentation_counters.rs`.
    #[test]
    fn eval_diag_format_param_override_dimension() {
        let diags = build_param_override_diags(Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        });

        assert!(
            !diags.is_empty(),
            "param_override_dimension: engine emitted no diagnostics"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "param_override_dimension: expected at least one Warning-severity diagnostic; \
             got: {:#?}",
            diags
        );
        // Substring discriminator: ensures the right emitter fired (the counter contract is
        // anchored at `crates/reify-eval/tests/eval_instrumentation_counters.rs`).
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("dimension mismatch")),
            "param_override_dimension: expected a diagnostic containing 'dimension mismatch'; \
             got: {:#?}",
            diags
        );
        assert_no_violation_format(&diags, "param_override_dimension");
    }

    /// Per-emitter regression lock — sub-component lookup failure path
    /// (engine_eval.rs sub-component lookup).
    ///
    /// Locks the invariant that `eval()` never emits the `"constraint ... violated"` format
    /// from the sub-component unknown-structure emitter.
    /// Counter contract for this emitter lives in `crates/reify-eval/tests/eval_instrumentation_counters.rs`.
    #[test]
    fn eval_diag_format_sub_component_unknown() {
        let source = "structure S { sub x = Unknown() }";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);
        let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
        let diags = engine.eval(&compiled).diagnostics;

        assert!(
            !diags.is_empty(),
            "sub_component_unknown: engine emitted no diagnostics"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Error),
            "sub_component_unknown: expected at least one Error-severity diagnostic; \
             got: {:#?}",
            diags
        );
        // Substring discriminator: ensures the right emitter fired (the counter contract is
        // anchored at `crates/reify-eval/tests/eval_instrumentation_counters.rs`).
        assert!(
            diags.iter().any(|d| d.message.contains("sub-component")
                && d.message.contains("references unknown structure")),
            "sub_component_unknown: expected a diagnostic containing both 'sub-component' and \
             'references unknown structure'; got: {:#?}",
            diags
        );
        assert_no_violation_format(&diags, "sub_component_unknown");
    }

    /// Per-emitter regression lock — solver Infeasible pass-through path
    /// (engine_eval.rs solver Infeasible pass-through).
    ///
    /// Locks the invariant that `eval()` never emits the `"constraint ... violated"` format
    /// from the solver Infeasible emitter. Also verifies via `MockConstraintSolver::counter_handle()`
    /// that the injected solver was actually dispatched.
    #[test]
    fn eval_diag_format_solver_infeasible() {
        let solver = MockConstraintSolver::new_infeasible(vec![Diagnostic::error(
            "infeasible: x has no satisfying assignment",
        )]);
        let (counter, diags) = run_solver_on_constrained_auto_param(solver);

        assert!(
            counter.load(Ordering::Relaxed) > 0,
            "solver_infeasible: MockConstraintSolver.solve() was never called; \
             the 'auto' param + constraint source may not trigger solver dispatch"
        );
        assert!(
            !diags.is_empty(),
            "solver_infeasible: engine emitted no diagnostics"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Error),
            "solver_infeasible: expected at least one Error-severity diagnostic; got: {:#?}",
            diags
        );
        assert_no_violation_format(&diags, "solver_infeasible");
    }

    /// Per-emitter regression lock — solver NoProgress pass-through path
    /// (engine_eval.rs solver NoProgress pass-through).
    ///
    /// Locks the invariant that `eval()` never emits the `"constraint ... violated"` format
    /// from the solver NoProgress emitter. Also verifies via `MockConstraintSolver::counter_handle()`
    /// that the injected solver was actually dispatched.
    #[test]
    fn eval_diag_format_solver_no_progress() {
        let solver = MockConstraintSolver::new_no_progress("iteration limit reached");
        let (counter, diags) = run_solver_on_constrained_auto_param(solver);

        assert!(
            counter.load(Ordering::Relaxed) > 0,
            "solver_no_progress: MockConstraintSolver.solve() was never called; \
             the 'auto' param + constraint source may not trigger solver dispatch"
        );
        assert!(
            !diags.is_empty(),
            "solver_no_progress: engine emitted no diagnostics"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "solver_no_progress: expected at least one Warning-severity diagnostic; got: {:#?}",
            diags
        );
        assert_no_violation_format(&diags, "solver_no_progress");
    }

    // --- task #2337 step-17: freshness diagnostic tests ---

    /// Helper: build an EvalState whose engine has been pre-evaluated with
    /// bracket_source and a forced panic on `cell_id`.  The engine has gone
    /// through two full `eval()` passes:
    ///   1. Cold eval (all cells → Final).
    ///   2. Hot eval with forced panic on `cell_id` (cell → Failed; cells
    ///      that depend on it → Pending via §9.2 propagation).
    ///
    /// The returned EvalState has `last_content_hash` and `version_counter`
    /// pre-set so that the NEXT call to `compute_diagnostics_with_state` with
    /// the same bracket_source takes the **eval_cached** path (not cold-start).
    /// This avoids the cold-start branch recreating the engine (which would
    /// discard the freshness state we just set up).
    ///
    /// `test-instrumentation` feature is enabled in dev-deps (Cargo.toml line 29).
    #[cfg(any(test, feature = "test-support"))]
    fn build_eval_state_with_failed_cell(cell_id: ValueCellId) -> EvalState {
        let source = reify_test_support::bracket_source();
        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);

        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);

        // Pass 1: cold eval — initialises the cache (all cells → Final).
        let _ = engine.eval(&compiled);

        // Inject forced panic and run a second full eval.
        engine.set_panic_on_eval(cell_id);
        let _ = engine.eval(&compiled);
        // After pass 2: `cell_id` is Failed; its dependents are Pending.

        // Package into EvalState with matching hash so next call uses eval_cached.
        let mut state = EvalState::new();
        state.engine = engine;
        state.last_content_hash = Some(compiled.content_hash);
        state.version_counter = 2;
        state
    }

    /// (a) `compute_diagnostics_with_state` must emit exactly one ERROR diagnostic
    /// with `code == "computation-failed"` for a cell whose freshness is Failed
    /// (forced-panic via test-instrumentation).
    ///
    /// This test is intentionally RED before step-18 adds the freshness-diagnostic
    /// emission block to `compute_diagnostics_with_state`.
    #[cfg(any(test, feature = "test-support"))]
    #[test]
    fn compute_diagnostics_with_state_emits_failed_diagnostic_for_failed_cell() {
        // Force `Bracket.volume` (the only `let` in bracket_source) to fail.
        let volume_id = ValueCellId::new("Bracket", "volume");
        let mut state = build_eval_state_with_failed_cell(volume_id.clone());

        let uri = test_uri();
        let source = reify_test_support::bracket_source();

        // eval_cached path: content unchanged, engine initialized.
        let result = compute_diagnostics_with_state(&mut state, source, &uri);

        let failed_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code
                    == Some(lsp_types::NumberOrString::String(
                        "computation-failed".to_string(),
                    ))
            })
            .collect();

        assert!(
            !failed_diags.is_empty(),
            "expected at least one 'computation-failed' ERROR diagnostic (got 0); \
             all diagnostics: {:#?}",
            result.diagnostics
        );

        // Pin that Bracket.volume's source span is among the failed diagnostics.
        // Re-compile with the same pipeline to get the canonical cell span.  This
        // avoids a hardcoded line number (which would drift if bracket_source changes)
        // and is resilient to stdlib templates adding extra let cells in the future.
        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);
        let volume_span = compiled
            .templates
            .iter()
            .flat_map(|t| t.value_cells.iter())
            .find(|vc| vc.id == volume_id)
            .map(|vc| vc.span)
            .expect("Bracket.volume cell must be present in compiled bracket_source");
        let volume_range = convert::span_to_range(source, volume_span);

        let covers_volume = failed_diags.iter().any(|d| d.range == volume_range);
        assert!(
            covers_volume,
            "expected a 'computation-failed' diagnostic anchored at Bracket.volume's range \
             ({:?}); failed diagnostics: {:#?}",
            volume_range, failed_diags
        );
        assert_eq!(
            failed_diags
                .iter()
                .find(|d| d.range == volume_range)
                .unwrap()
                .severity,
            Some(DiagnosticSeverity::ERROR),
            "computation-failed diagnostic for Bracket.volume must have ERROR severity"
        );
    }

    /// (b) `compute_diagnostics_with_state` must emit at least one WARNING diagnostic
    /// with `code == "computation-pending"` for a cell that is Pending because its
    /// upstream dependency failed (Failed leaf → Pending consumer, arch §9.2).
    ///
    /// Setup: use a custom let-chain source (`S.base` → `S.derived`) so that
    /// `set_panic_on_eval(S.base)` (a let cell) causes `S.base` to fail and
    /// `S.derived` to become Pending.
    ///
    /// Note: `set_panic_on_eval` only affects `let` cells (evaluated in the
    /// let-binding evaluation loop) — not `param` cells.  Hence we use a
    /// dedicated let-chain source rather than bracket_source (where `width` is
    /// a param and would not be affected by `set_panic_on_eval`).
    #[cfg(any(test, feature = "test-support"))]
    #[test]
    fn compute_diagnostics_with_state_emits_pending_diagnostic_for_pending_cell() {
        // Source with a let-chain: S.derived depends on S.base.
        // Forcing S.base (a let) to fail makes S.derived Pending (arch §9.2).
        // Module name "test" matches what compute_diagnostics_with_state derives
        // from "file:///test.ri" (strip ".ri" suffix).
        let source = "structure S {\n    let base = 1.0\n    let derived = base + 1.0\n}";
        let base_id = ValueCellId::new("S", "base");

        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);

        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);

        // Pass 1: cold eval — initialises cache (all cells → Final).
        let _ = engine.eval(&compiled);

        // Pass 2: force S.base to fail; S.derived becomes Pending (arch §9.2).
        engine.set_panic_on_eval(base_id);
        let _ = engine.eval(&compiled);

        // Package into EvalState with matching hash so next call uses eval_cached.
        let mut state = EvalState::new();
        state.engine = engine;
        state.last_content_hash = Some(compiled.content_hash);
        state.version_counter = 2;

        let uri = test_uri();
        let result = compute_diagnostics_with_state(&mut state, source, &uri);

        let pending_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code
                    == Some(lsp_types::NumberOrString::String(
                        "computation-pending".to_string(),
                    ))
            })
            .collect();

        assert!(
            !pending_diags.is_empty(),
            "expected at least one 'computation-pending' WARNING diagnostic \
             (S.derived is Pending because S.base failed), \
             got zero; all diagnostics: {:#?}",
            result.diagnostics
        );
        for d in &pending_diags {
            assert_eq!(
                d.severity,
                Some(DiagnosticSeverity::WARNING),
                "computation-pending diagnostic must have WARNING severity, got {:?}",
                d.severity
            );
        }
    }

    /// (b2) The `computation-pending` diagnostic message must embed the upstream
    /// cell name (e.g. `"S.base"`) so the user sees which dependency failed.
    ///
    /// Setup is identical to the sibling test above: `S.base` → `S.derived`
    /// let-chain; force `S.base` Failed; `S.derived` becomes Pending.
    ///
    /// Asserts:
    ///   (i)   at least one `computation-pending` diagnostic exists.
    ///   (ii)  its `message` starts with the static prefix
    ///         `"computation pending: upstream dependency failed"` (backward-
    ///         compatible prefix that editor integrations may match on).
    ///   (iii) its `message` contains the parenthesized suffix
    ///         `"(because S.base failed)"` — pinning the exact user-visible
    ///         wording the enrichment introduces.
    ///
    /// Step-3 test: will fail with today's static message until step-4 enriches
    /// the Pending arm to call `Engine::pending_cause`.
    #[cfg(any(test, feature = "test-support"))]
    #[test]
    fn pending_diagnostic_message_includes_upstream_cell_name() {
        // Same let-chain source and setup as the sibling pending-diagnostic test.
        let source = "structure S {\n    let base = 1.0\n    let derived = base + 1.0\n}";
        let base_id = ValueCellId::new("S", "base");

        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);

        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);

        // Pass 1: cold eval — initialises cache (all cells → Final).
        let _ = engine.eval(&compiled);

        // Pass 2: force S.base to fail; S.derived becomes Pending (arch §9.2).
        engine.set_panic_on_eval(base_id);
        let _ = engine.eval(&compiled);

        // Package into EvalState with matching hash so next call uses eval_cached.
        let mut state = EvalState::new();
        state.engine = engine;
        state.last_content_hash = Some(compiled.content_hash);
        state.version_counter = 2;

        let uri = test_uri();
        let result = compute_diagnostics_with_state(&mut state, source, &uri);

        let pending_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code
                    == Some(lsp_types::NumberOrString::String(
                        "computation-pending".to_string(),
                    ))
            })
            .collect();

        // (i) At least one pending diagnostic must exist.
        assert!(
            !pending_diags.is_empty(),
            "expected at least one 'computation-pending' diagnostic \
             (S.derived is Pending because S.base failed); \
             got zero; all diagnostics: {:#?}",
            result.diagnostics
        );

        for d in &pending_diags {
            // (ii) Prefix must be preserved for backward compatibility.
            assert!(
                d.message
                    .starts_with("computation pending: upstream dependency failed"),
                "computation-pending message must start with the static prefix; \
                 got: {:?}",
                d.message
            );

            // (iii) The parenthesized "(because S.base failed)" suffix must appear
            //       in the message — pins the exact wording the enrichment introduces.
            assert!(
                d.message.contains("(because S.base failed)"),
                "computation-pending message must contain \"(because S.base failed)\" \
                 (parenthesized upstream cell name); got: {:?}",
                d.message
            );
        }
    }

    /// (c) A normal evaluation (all cells Final) must produce zero freshness-code
    /// diagnostics.  This covers both Final (success) and the Intermediate case
    /// (Intermediate → no emission, arch §7.2): since a completed eval leaves all
    /// cells Final, no computation-* diagnostics should appear.
    ///
    /// This test passes both before and after step-18 (it is a negative assertion
    /// that guards against spurious fresh-start emission).
    #[test]
    fn normal_eval_emits_no_freshness_diagnostics() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let source = reify_test_support::bracket_source();

        let result = compute_diagnostics_with_state(&mut state, source, &uri);

        let freshness_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                matches!(&d.code,
                    Some(lsp_types::NumberOrString::String(s))
                        if s == "computation-failed" || s == "computation-pending"
                )
            })
            .collect();

        assert!(
            freshness_diags.is_empty(),
            "normal (all-Final) eval must produce zero freshness-code diagnostics; \
             got: {:#?}",
            freshness_diags
        );
    }

    // ── specialization-scope LSP regression locks (task 2371) ──────────────
    //
    // Each test builds a ParsedModule with a specialization scope
    // (`MemberDecl::Sub { body: Some(_) }`), drives it through
    // `reify_compiler::compile_with_stdlib`, converts every compiler diagnostic
    // through `convert::convert_diagnostic` (mirroring the post-parse half of
    // `compute_diagnostics`), then filters to
    // `code == "SpecializationForbiddenDecl"` before asserting.
    //
    // AST-builder helpers are imported from
    // `reify_test_support::specialization_fixtures` — the canonical shared
    // module that eliminates the prior duplication across compiler and LSP
    // test files.

    #[test]
    fn find_unique_unconsumed_match_excludes_consumed_indices_and_panics_when_pool_is_exhausted() {
        use std::panic::AssertUnwindSafe;

        // D0: message embeds three quoted tokens ('param', 'foo', 'baz') so that
        // a naive substring scan without a consumed-set would match D0 for both
        // ("param", "foo") and ("param", "baz") queries — simulating the
        // bijection-violation scenario.
        let d0 = lsp_types::Diagnostic {
            message: "'param' decl 'foo' seen alongside 'baz' in scope".to_string(),
            ..Default::default()
        };
        // D1: message embeds 'sub' and 'bar'.
        let d1 = lsp_types::Diagnostic {
            message: "'sub' decl 'bar' is not permitted".to_string(),
            ..Default::default()
        };
        let forbidden = vec![d0, d1];

        // (a) Happy path — empty consumed set: each query returns the unique index.
        let consumed: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let idx0 = find_unique_unconsumed_match(&forbidden, &consumed, "param", "foo");
        assert_eq!(idx0, 0);

        let mut consumed_after = std::collections::HashSet::<usize>::new();
        consumed_after.insert(0);
        let idx1 = find_unique_unconsumed_match(&forbidden, &consumed_after, "sub", "bar");
        assert_eq!(idx1, 1);

        // (b) Bijection-violation path — with index 0 consumed, a second query for
        // ("param", "foo") must panic: no unconsumed diagnostic contains both tokens.
        let forbidden_ref = &forbidden;
        let consumed_ref = consumed_after.clone();
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            find_unique_unconsumed_match(forbidden_ref, &consumed_ref, "param", "foo")
        }));
        assert!(
            result.is_err(),
            "expected panic when the only matching diagnostic is already consumed"
        );
    }

    /// Find the unique index in `forbidden` of an unconsumed diagnostic whose message
    /// contains both `'{kind}'` and `'{name}'`. Panics if zero or more than one such
    /// diagnostic exists in the unconsumed pool.
    ///
    /// Used by `assert_specialization_forbidden` to enforce a bijection between
    /// expected `(kind, name, position)` entries and `SpecializationForbiddenDecl`
    /// diagnostics — each expected entry consumes a distinct diagnostic.
    fn find_unique_unconsumed_match(
        forbidden: &[lsp_types::Diagnostic],
        consumed: &std::collections::HashSet<usize>,
        kind: &str,
        name: &str,
    ) -> usize {
        let matches: Vec<usize> = forbidden
            .iter()
            .enumerate()
            .filter(|(idx, d)| {
                !consumed.contains(idx)
                    && d.message.contains(&format!("'{kind}'"))
                    && d.message.contains(&format!("'{name}'"))
            })
            .map(|(idx, _)| idx)
            .collect();

        assert_eq!(
            matches.len(),
            1,
            "expected exactly one unconsumed SpecializationForbiddenDecl diagnostic \
             containing kind='{kind}' and name='{name}'; found {}; \
             matched indices: {matches:#?}; consumed: {consumed:?}; \
             all diagnostics: {forbidden:#?}",
            matches.len()
        );

        matches[0]
    }

    /// Drive the specialization-scope pipeline for `body` (the contents of a
    /// `sub scope : Foo { body }` node inside structure S) and assert that the
    /// resulting LSP diagnostics with code `"SpecializationForbiddenDecl"` match
    /// `expected` exactly.
    ///
    /// For each `(kind, name)` pair the helper asserts that the compiler
    /// message contains both `"'{kind}'"` and `"'{name}'"` as substrings
    /// (mirrors compiler-side substring style established by these tests in
    /// specialization_scope_check.rs:
    /// - `validate_module_emits_forbidden_decl_diagnostic_for_param_inside_specialization_scope`
    /// - `validate_module_emits_forbidden_decl_diagnostic_for_port_inside_specialization_scope`
    /// - `validate_module_emits_forbidden_decl_diagnostic_for_bare_sub_inside_specialization_scope`
    /// - `validate_module_emits_diagnostic_for_each_forbidden_decl_in_nested_specialization_scope`).
    ///
    /// The canonical message
    /// format is pinned solely by compiler-side tests; the LSP layer checks
    /// presence-only, eliminating the dual-edit ratchet that arises from
    /// re-asserting spec-section wording at multiple layers.
    ///
    /// Pairing is by content: for each `(kind, name, expected_pos)` in
    /// `expected` the helper finds the unique diagnostic in `forbidden` whose
    /// message contains both `"'{kind}'"` and `"'{name}'"`.  If no diagnostic
    /// or more than one diagnostic matches, the assertion fails immediately with
    /// a clear message — making any compiler message-format reshuffle that
    /// still embeds both tokens self-evident rather than hiding behind a
    /// sort-key drift.
    ///
    /// Furthermore, each expected entry consumes a distinct diagnostic: once a
    /// diagnostic has been paired to an expected entry, it is excluded from
    /// subsequent iterations' match pool.  Combined with the early
    /// `forbidden.len() == expected.len()` check above, this guarantees a
    /// bijection between `expected` and `forbidden` — no two expected entries
    /// can silently pair to the same diagnostic, and no diagnostic in `forbidden`
    /// can go unmatched.
    ///
    /// Additionally asserts for every matched diagnostic:
    /// - severity `ERROR`, source `"reify"`
    /// - `range.start` equals the expected `Position` for that violation
    ///   (explicit witness that the fixture span was carried through
    ///   `convert_diagnostic` + `offset_to_position` correctly; also
    ///   guarantees distinctness of starts for sibling violations since the
    ///   expected Positions are all distinct)
    /// - non-degenerate range: `range.start != range.end`
    ///
    /// Pass `expected = &[]` to assert that no such diagnostic is emitted.
    fn assert_specialization_forbidden(
        body: Vec<reify_ast::MemberDecl>,
        expected: &[(&str, &str, lsp_types::Position)],
    ) {
        use lsp_types::{DiagnosticSeverity, NumberOrString};
        use reify_test_support::specialization_fixtures::*;

        let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
            "scope",
            dummy_span(),
            body,
        )]);
        let compiled = compile_like_production(&parsed);
        let source = source_stub();
        let uri = test_uri();

        let forbidden: Vec<lsp_types::Diagnostic> = compiled
            .diagnostics
            .iter()
            .map(|d| convert::convert_diagnostic(d, &source, &uri))
            .filter(|d| {
                d.code
                    == Some(NumberOrString::String(
                        "SpecializationForbiddenDecl".to_string(),
                    ))
            })
            .collect();

        assert_eq!(
            forbidden.len(),
            expected.len(),
            "expected {} SpecializationForbiddenDecl diagnostic(s), got {}: {:#?}",
            expected.len(),
            forbidden.len(),
            forbidden
        );

        // Pair by content: for each expected (kind, name, pos) find the unique
        // unconsumed diagnostic whose message contains both quoted tokens.
        // `consumed` tracks which indices have already been paired, enforcing
        // the bijection — a sort-key drift can never silently mispair violations.
        let mut consumed = std::collections::HashSet::<usize>::new();
        for (kind, name, expected_pos) in expected {
            let idx = find_unique_unconsumed_match(&forbidden, &consumed, kind, name);
            consumed.insert(idx);

            let d = &forbidden[idx];

            assert_eq!(
                d.severity,
                Some(DiagnosticSeverity::ERROR),
                "expected ERROR severity; diagnostic: {d:#?}"
            );
            assert_eq!(
                d.source.as_deref(),
                Some("reify"),
                "expected source 'reify'; diagnostic: {d:#?}"
            );
            assert_eq!(
                d.range.start, *expected_pos,
                "range.start must equal expected Position; \
                 got {:?}, want {:?}; diagnostic: {d:#?}",
                d.range.start, expected_pos
            );
            assert_ne!(
                d.range.start, d.range.end,
                "range must be non-degenerate; got start={:?} end={:?}",
                d.range.start, d.range.end
            );
        }
    }

    /// LSP regression lock (task 2371, step-7): a specialization scope containing
    /// only permitted declarations (`let` and `constraint`) must produce ZERO LSP
    /// diagnostics with code `"SpecializationForbiddenDecl"`.
    ///
    /// Pins the converse contract at the LSP layer: permitted decls must never
    /// surface this code, regardless of what unrelated diagnostics the compile
    /// pipeline emits (those are ignored by the code filter).
    #[test]
    fn lsp_compute_diagnostics_emits_no_specialization_forbidden_decl_for_permitted_only_spec_scope()
     {
        use reify_test_support::specialization_fixtures::*;
        assert_specialization_forbidden(vec![make_let("m"), make_constraint()], &[]);
    }

    /// LSP regression lock (task 2371, step-9): a specialization scope with three
    /// sibling forbidden declarations (param, port, bare sub) surfaces exactly THREE
    /// distinct LSP ERROR diagnostics with code `"SpecializationForbiddenDecl"` —
    /// one per violation — each with a distinguishable non-zero range.
    ///
    /// Mirrors the compiler-side test
    /// `validate_module_emits_one_diagnostic_per_sibling_forbidden_decl_in_same_body`
    /// and locks the per-violation 1-LSP-diagnostic contract end-to-end: the LSP
    /// wire form must NOT collapse sibling violations.
    #[test]
    fn lsp_compute_diagnostics_surfaces_one_specialization_forbidden_decl_per_sibling_violation() {
        use reify_test_support::specialization_fixtures::*;
        use tower_lsp::lsp_types::Position;
        // source_stub() = " ".repeat(120) — single-line ASCII, so byte offset N
        // maps to Position::new(0, N) via offset_to_position (UTF-8 + UTF-16
        // collapse for pure ASCII).
        assert_specialization_forbidden(
            vec![
                make_param("x", param_span()),
                make_port("p", port_span()),
                make_sub_bare("child", sub_span()),
            ],
            &[
                ("param", "x", Position::new(0, 30)), // param_span = SourceSpan::new(30, 50)
                ("port", "p", Position::new(0, 60)),  // port_span  = SourceSpan::new(60, 80)
                ("sub", "child", Position::new(0, 90)), // sub_span   = SourceSpan::new(90, 110)
            ],
        );
    }

    /// (d) **Separation regression** — constraint-violation source must NOT produce
    /// any `computation-failed` diagnostics; the existing `constraint <id> violated`
    /// diagnostics must still appear.
    ///
    /// Guards arch §9.3: constraint violations route through `Satisfaction::Violated`,
    /// NOT through `Freshness::Failed`.  This test passes both before and after
    /// step-18, ensuring the implementation never routes violations through the
    /// freshness channel.
    #[test]
    fn constraint_violation_does_not_produce_computation_failed() {
        let mut state = EvalState::new();
        let uri = test_uri();
        let source_violating = reify_test_support::bracket_source_violating();

        let result = compute_diagnostics_with_state(&mut state, &source_violating, &uri);

        // (i) The existing constraint-violated diagnostic must be present.
        let constraint_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.to_lowercase().contains("constraint")
                    && d.message.to_lowercase().contains("violated")
            })
            .collect();
        assert!(
            !constraint_errors.is_empty(),
            "violating source must produce at least one constraint-violated ERROR; \
             got: {:#?}",
            result.diagnostics
        );

        // (ii) Zero `computation-failed` diagnostics — constraint violations
        //      must NEVER route through the freshness channel.
        let computation_failed_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.code
                    == Some(lsp_types::NumberOrString::String(
                        "computation-failed".to_string(),
                    ))
            })
            .collect();
        assert!(
            computation_failed_diags.is_empty(),
            "constraint-violation source must produce ZERO 'computation-failed' diagnostics \
             (arch §9.3 separation); got: {:#?}",
            computation_failed_diags
        );
    }

    /// Every compute target registered by
    /// `reify_eval::compute_targets::register_compute_fns` — that function's
    /// own doc comment calls it "the single place that registers the full
    /// production set of compute trampolines". Exhaustive, not a sample:
    /// used below to probe that the LSP's engine ([`EvalState::new`], which
    /// never calls `register_compute_fns`) carries none of them registered.
    /// If `register_compute_fns` gains or loses a target, update this list
    /// to match — a stale list would silently narrow the probe back to a
    /// sample.
    ///
    /// That instruction is HALF-ENFORCED, deliberately so. A rename or a
    /// removal upstream is caught immediately by
    /// [`assert_all_production_compute_targets_are_registrable`], the positive
    /// counter-probe: without it every `compute_dispatch(target).is_none()`
    /// assertion on a stale name passes trivially and the whole posture lock
    /// goes vacuous without going red. An ADDITION upstream is NOT caught —
    /// closing that half needs a public enumerator over `Engine`'s
    /// `compute_registry.fns` in reify-eval (so the const can be asserted
    /// set-equal to the registry), which is outside this leaf's
    /// "Modules: `reify-lsp`" scope.
    const ALL_PRODUCTION_COMPUTE_TARGETS: &[&str] = &[
        "solver::elastic_static",
        "solver::buckling",
        "solver::form_find",
        "solver::form_find_free",
        "solver::tensegrity_load",
        "solver::membrane_load",
        "solver::multi_case",
        "solver::buckling_multi_case",
        "fdm::as_printed_material_r_fast",
        "fdm::as_printed_material_r0",
        "fdm::slice",
        "modal::free_vibration",
        "modal::transient_response",
        "modal::displacement_at",
        "modal::mechanism_modal",
        "dynamics::inverse_dynamics",
        "trajectory::simulate",
        "trajectory::input_shape",
    ];

    /// Positive counter-probe for [`ALL_PRODUCTION_COMPUTE_TARGETS`]: every
    /// name in the const must actually be registered by
    /// `reify_eval::compute_targets::register_compute_fns`.
    ///
    /// The const is a hand-maintained mirror of that function, and the posture
    /// lock that consumes it only ever asserts `compute_dispatch(t).is_none()`.
    /// Those assertions pass TRIVIALLY on a name that no longer exists, so an
    /// upstream rename or removal would hollow the lock out silently — it
    /// would keep proving that an engine carries no trampoline for eighteen
    /// strings, none of which name a real compute target any more. Asserting
    /// the same names ARE dispatchable on a throwaway registered engine makes
    /// exactly that drift red, at the cost of one extra `Engine`.
    ///
    /// Does NOT catch an upstream ADDITION — see the const's own doc for why
    /// that half is out of this leaf's scope.
    fn assert_all_production_compute_targets_are_registrable() {
        let mut registered_probe = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
        reify_eval::compute_targets::register_compute_fns(&mut registered_probe);
        for target in ALL_PRODUCTION_COMPUTE_TARGETS {
            assert!(
                registered_probe.compute_dispatch(target).is_some(),
                "ALL_PRODUCTION_COMPUTE_TARGETS is STALE: '{target}' is not \
                 registered by reify_eval::compute_targets::register_compute_fns \
                 any more. Every `compute_dispatch('{target}').is_none()` \
                 assertion in the trampoline-free posture lock is therefore \
                 passing trivially. Re-sync this const with \
                 `register_compute_fns`."
            );
        }
    }

    /// Posture lock (PRD `compute-fea-hardening.md` task C1, INV-FEA-1) for
    /// the trampoline-free posture — see [`compute_diagnostics_with_state`]'s
    /// doc comment for the authoritative posture writeup; this test is its
    /// executable contract. Over the FEA-bearing [`FEA_BEARING_SRC`] fixture,
    /// on both LSP entry points, locks that the resulting `Indeterminate`
    /// FEA constraint produces neither a false violation nor a false pass
    /// (see [`assert_no_false_violation_or_pass`]) — the LSP-side analog of
    /// the CLI's `check_fea_violated_constraint_is_not_gated`
    /// (`crates/reify-cli/tests/harness_cli/cli_build_fea.rs`). GREEN before and after:
    /// this locks pre-existing gate behaviour, not new runtime behaviour.
    ///
    /// **Also the BT8 REVERSE lock** (task #6798, PRD
    /// `driver-contract-implementation.md` leaf pi / §5 BT8 / §12 matrix
    /// ruling 2): the compute-trampoline probe below enumerates
    /// [`ALL_PRODUCTION_COMPUTE_TARGETS`] — every target
    /// `register_compute_fns` registers, not a sample — so it doubles as
    /// the lock that injecting the real `SimpleConstraintChecker` at
    /// compile time (this leaf's change) does not disturb the
    /// Leo-ratified trampoline-free posture. Matrix ruling 2 retires the
    /// CLI-side FEA lock but explicitly KEEPS this LSP-side subtraction.
    /// `FEA_BEARING_SRC` has no `auto:` type parameter, so the injected
    /// compile-time checker is never even consulted on it — the checker
    /// swap is a no-op here by construction (see
    /// [`compute_diagnostics_with_state`]'s "## Compile-time checker" doc
    /// section), which is exactly why this stays a posture lock rather
    /// than a BT8-forward-style divergence test, and why it is GREEN
    /// before and after task #6798's impl steps: the compile-time checker
    /// change never touches the eval-time Engine built by
    /// [`EvalState::new`].
    #[test]
    fn fea_bearing_constraint_produces_no_false_violation_or_false_pass() {
        // Anti-vacuity for the probe's own vocabulary: the negative
        // `is_none()` assertions below are only meaningful if every name in
        // ALL_PRODUCTION_COMPUTE_TARGETS still names a real, registrable
        // compute target.
        assert_all_production_compute_targets_are_registrable();

        let uri = test_uri();
        let parsed =
            reify_compiler::parse_with_stdlib(FEA_BEARING_SRC, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);

        // Guard: the fixture must compile with zero errors, so the
        // Indeterminate result asserted below is attributable to
        // trampoline-free body-inlining (the documented posture), not an
        // upstream stdlib-resolution failure that happens to also produce
        // `undef` for the wrong reason.
        let compile_errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            compile_errors.is_empty(),
            "FEA_BEARING_SRC must compile without errors; got: {compile_errors:#?}"
        );

        // --- Stateless surface: compute_diagnostics ---
        //
        // No persistent engine survives the call, so we probe an
        // independently constructed Engine sharing its exact, documented
        // posture (Engine::new(SimpleConstraintChecker, None), no
        // trampoline registration) rather than compute_diagnostics's own
        // internal engine — a redundant belt-and-suspenders check, not the
        // authoritative lock (see `assert_no_false_violation_or_pass`'s doc
        // comment for the full rationale; the stateful surface below is
        // authoritative).
        let stateless_diags = compute_diagnostics(FEA_BEARING_SRC, &uri);

        let checker = SimpleConstraintChecker;
        let mut stateless_probe = reify_eval::Engine::new(Box::new(checker), None);
        for target in ALL_PRODUCTION_COMPUTE_TARGETS {
            assert!(
                stateless_probe.compute_dispatch(target).is_none(),
                "sanity: Engine::new(SimpleConstraintChecker, None) — the \
                 exact construction compute_diagnostics uses internally — \
                 must carry no pre-registered '{target}' compute trampoline"
            );
        }
        let stateless_check_result = stateless_probe.check(&compiled);
        assert_no_false_violation_or_pass(
            &stateless_diags,
            &stateless_check_result,
            "compute_diagnostics",
        );

        // --- Stateful surface: compute_diagnostics_with_state (the live server's path) ---
        //
        // Unlike the stateless surface, EvalState carries the actual
        // persistent Engine the production call used — read its posture and
        // constraint satisfactions directly off `state.engine` (a private
        // field; child-module test access to it is already established
        // elsewhere in this file, e.g. `build_eval_state_with_failed_cell`)
        // rather than a hand-rebuilt one.
        let mut state = EvalState::new();
        let stateful_result = compute_diagnostics_with_state(&mut state, FEA_BEARING_SRC, &uri);
        for target in ALL_PRODUCTION_COMPUTE_TARGETS {
            assert!(
                state.engine.compute_dispatch(target).is_none(),
                "the actual persistent Engine used by \
                 compute_diagnostics_with_state must carry no registered \
                 '{target}' compute trampoline — this is the \
                 trampoline-free posture documented on that function, and \
                 (task #6798) the compile-time checker injection must not \
                 disturb it"
            );
        }
        let stateful_check_result = state.engine.check_snapshot(&compiled).expect(
            "state.engine should hold a snapshot for FEA_BEARING_SRC's content \
             hash immediately after compute_diagnostics_with_state evaluated it",
        );
        assert_no_false_violation_or_pass(
            &stateful_result.diagnostics,
            &stateful_check_result,
            "compute_diagnostics_with_state",
        );
    }

    /// Shared assertion for
    /// [`fea_bearing_constraint_produces_no_false_violation_or_false_pass`]
    /// (see that test's doc for the full rationale). Given one LSP entry
    /// point's diagnostics and the `CheckResult` for the same content,
    /// assert: (a) no diagnostic message equals the exact text
    /// [`constraint_violated_message`] would produce for any constraint (no
    /// false violation, matched via the same formatter production uses, not
    /// a drift-prone substring); (b) zero `Violated` and zero `Satisfied`
    /// constraints, with at least one `Indeterminate` (no false pass on any
    /// individual constraint, and not silently missing or all-satisfied).
    /// The three counts are tallied via an exhaustive `match` (no wildcard
    /// arm) on `Satisfaction`, so this fn fails to *compile* — rather than
    /// silently passing — the moment `Satisfaction` grows a fourth variant.
    ///
    /// Belt-and-suspenders note (applies to both call sites below): the
    /// stateless (`compute_diagnostics`) call site passes a `check_result`
    /// from an independently constructed probe engine, not the exact
    /// internal engine `compute_diagnostics` builds and drops — a redundant
    /// check, not the authoritative lock. The stateful
    /// (`compute_diagnostics_with_state`) call site, which reads
    /// `state.engine` directly, is the authoritative lock for this contract.
    fn assert_no_false_violation_or_pass(
        diagnostics: &[lsp_types::Diagnostic],
        check_result: &reify_eval::CheckResult,
        surface: &str,
    ) {
        let would_be_violated_msgs: Vec<String> = check_result
            .constraint_results
            .iter()
            .map(constraint_violated_message)
            .collect();
        let false_violations: Vec<_> = diagnostics
            .iter()
            .filter(|d| would_be_violated_msgs.contains(&d.message))
            .collect();
        assert!(
            false_violations.is_empty(),
            "{surface} must not report a false violation for an Indeterminate \
             FEA constraint; got: {false_violations:#?}"
        );

        // Exhaustive `match` (no wildcard arm) — see this fn's doc comment
        // for why that alone suffices as the variant-drift guard.
        let (violated_count, satisfied_count, indeterminate_count) =
            check_result.constraint_results.iter().fold(
                (0usize, 0usize, 0usize),
                |(violated, satisfied, indeterminate), e| match e.satisfaction {
                    Satisfaction::Violated => (violated + 1, satisfied, indeterminate),
                    Satisfaction::Satisfied => (violated, satisfied + 1, indeterminate),
                    Satisfaction::Indeterminate => (violated, satisfied, indeterminate + 1),
                },
            );

        assert_eq!(
            violated_count, 0,
            "{surface}: FEA-only fixture must have zero Violated constraints; \
             got constraint_results: {:#?}",
            check_result.constraint_results
        );
        assert_eq!(
            satisfied_count, 0,
            "{surface}: every FEA-result constraint must be Indeterminate \
             under the trampoline-free posture, never Satisfied — otherwise \
             a per-constraint false pass on a subset of the fixture's \
             constraints would slip through; got constraint_results: {:#?}",
            check_result.constraint_results
        );
        assert!(
            indeterminate_count >= 1,
            "{surface}: expected >= 1 Indeterminate constraint (FEA-result \
             constraint under the trampoline-free posture); got 0 — the \
             constraint may be silently Satisfied (false pass) or missing \
             entirely. constraint_results: {:#?}",
            check_result.constraint_results
        );
    }

    /// RED→GREEN driver for task 5078 (PRD `compute-fea-hardening.md` task
    /// C2). Under the trampoline-free posture (see
    /// [`compute_diagnostics_with_state`]'s doc comment), an FEA-result
    /// constraint checks as `Satisfaction::Indeterminate` and — absent this
    /// hint — produces no diagnostic at all, silently indistinguishable
    /// from "no constraint" (neither the `violated_messages` skip-set nor
    /// the span-aware `Satisfaction::Violated` ERROR loop emit anything for
    /// `Indeterminate`). This locks that `compute_diagnostics_with_state`
    /// emits at least one `Severity::Info` hint, and never more than the
    /// fixture's `Indeterminate` count, over the FEA-only
    /// [`FEA_BEARING_SRC`] fixture, anchored to a constraint span, with the
    /// exact wording and source specified by the task.
    ///
    /// Deliberately does NOT assert `hints.len() == indeterminate_count`:
    /// that equivalence only holds because this fixture happens to contain
    /// exclusively FEA-derived constraints, and would spuriously fail if a
    /// future edit added a non-FEA `Indeterminate` control (e.g. an `auto`
    /// param) to it, even though the discriminator would still be behaving
    /// correctly. The exact per-constraint count for THIS fixture (2) is
    /// separately pinned by `fea_hint_two_fea_constraints_each_get_distinct_span_hint`,
    /// and FEA-vs-non-FEA discrimination itself is pinned by
    /// `fea_hint_excludes_auto_param_indeterminate_and_dedups_per_constraint`.
    #[test]
    fn fea_indeterminate_constraint_emits_info_hint() {
        let uri = test_uri();
        let parsed =
            reify_compiler::parse_with_stdlib(FEA_BEARING_SRC, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);

        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, FEA_BEARING_SRC, &uri);

        let hints: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::INFORMATION)
                    && d.code
                        == Some(lsp_types::NumberOrString::String(
                            FEA_NOT_EVALUATED_CODE.to_string(),
                        ))
            })
            .collect();
        for hint in &hints {
            assert_eq!(
                hint.source,
                Some("reify".to_string()),
                "FEA hint must carry source \"reify\"; got: {hint:#?}"
            );
            // The `hints` filter above identifies by `code` (a
            // wording-independent handle — see `FEA_NOT_EVALUATED_CODE`'s
            // doc comment), so it no longer pins the exact message text.
            // Assert it here instead, once each diagnostic is already known
            // to be the FEA hint, so the task's exact-wording requirement
            // stays locked by a real test.
            assert_eq!(
                hint.message, FEA_NOT_EVALUATED_HINT,
                "FEA hint must carry the exact task-specified wording; got: {hint:#?}"
            );
        }

        // Compute the expected count dynamically (not a hardcoded magic
        // number): read the persistent engine's snapshot for the content it
        // just evaluated and count Indeterminate constraints (mirrors the
        // stateful surface in
        // `fea_bearing_constraint_produces_no_false_violation_or_false_pass`).
        let check_result = state.engine.check_snapshot(&compiled).expect(
            "state.engine should hold a snapshot for FEA_BEARING_SRC's content hash \
             immediately after compute_diagnostics_with_state evaluated it",
        );
        let indeterminate_count = check_result
            .constraint_results
            .iter()
            .filter(|e| e.satisfaction == Satisfaction::Indeterminate)
            .count();
        assert!(
            indeterminate_count >= 1,
            "fixture sanity: expected >= 1 Indeterminate constraint; got \
             constraint_results: {:#?}",
            check_result.constraint_results
        );

        // Not `assert_eq!(hints.len(), indeterminate_count)`: that equates
        // "hint count" with "raw Indeterminate count", which only holds
        // because this fixture is FEA-only. Assert existence (not vacuous)
        // and an upper bound (never more hints than Indeterminate entries,
        // since every hint comes from one) instead — see this test's doc
        // comment for why the exact per-fixture count lives elsewhere.
        assert!(
            !hints.is_empty(),
            "expected at least one FEA-not-evaluated hint over the FEA-only \
             fixture; got 0 (constraint_results: {:#?})",
            check_result.constraint_results
        );
        assert!(
            hints.len() <= indeterminate_count,
            "got more FEA-not-evaluated hints ({}) than Indeterminate \
             constraints ({indeterminate_count}) in the fixture — a hint \
             must never fire for a non-Indeterminate constraint; got hints: \
             {:#?}",
            hints.len(),
            hints
        );

        for hint in &hints {
            assert!(
                hint.range.start != hint.range.end,
                "hint range must be anchored to the constraint's span, not a \
                 default/empty range; got: {hint:#?}"
            );
        }
    }

    /// RED→GREEN driver for task 5078 step-3/step-4 (PRD
    /// `compute-fea-hardening.md` task C2): pins that the FEA "not
    /// evaluated in editor" hint (i) collapses to exactly ONE hint per
    /// constraint even when that constraint's expression references the
    /// FEA-derived value more than once (per-constraint, not per-value-ref,
    /// dedup), and (ii) does NOT fire on an `Indeterminate` constraint whose
    /// value is not FEA-derived. A genuinely-unresolved `auto` param has no
    /// solver attached in the LSP's engine (`EvalState::new` never calls
    /// `with_solver`), so it is *also* `Indeterminate` — but for a
    /// different reason than the trampoline-free posture documented on
    /// [`compute_diagnostics_with_state`], and the hint must not confuse the
    /// two causes.
    ///
    /// RED against step-2's naive impl: with no FEA-dependence
    /// discrimination, step-2 emits one hint per `Indeterminate`
    /// `constraint_results` entry, i.e. TWO hints here (one for the
    /// compound FEA constraint, one for the unrelated `gap` auto-param
    /// constraint) — this test requires exactly one.
    #[test]
    fn fea_hint_excludes_auto_param_indeterminate_and_dedups_per_constraint() {
        let uri = test_uri();

        // FEA portion copied verbatim from `FEA_BEARING_SRC`'s known-good
        // body (material / tip_load / mount / solve_elastic_static /
        // peak_stress) so it compiles, collapsed to ONE compound constraint
        // that references `peak_stress` twice, plus an unrelated
        // genuinely-unresolved `auto` param with its own constraint.
        const SRC: &str = r#"structure FeaBearingMixed {
    param length : Length = 1000mm
    param width  : Length = 100mm
    param height : Length = 100mm
    param gap : Length = auto

    let material = Steel_AISI_1045()
    let tip_load = PointLoad(point: "tip", force: 1000.0)
    let mount = FixedSupport(target: "root")

    let result = solve_elastic_static(
        material, length, width, height, [tip_load], [mount], ElasticOptions()
    )

    let peak_stress = result.max_von_mises

    constraint peak_stress < 1MPa && peak_stress < 100MPa
    constraint gap > 1mm
}"#;

        let parsed = reify_compiler::parse_with_stdlib(SRC, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);

        // Guard fixture validity like C1's lock: a fixture typo must fail
        // loudly here rather than making the assertions below vacuous.
        let compile_errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            compile_errors.is_empty(),
            "fixture sanity: SRC must compile without errors; got: {compile_errors:#?}"
        );

        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, SRC, &uri);

        let hints: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::INFORMATION)
                    && d.code
                        == Some(lsp_types::NumberOrString::String(
                            FEA_NOT_EVALUATED_CODE.to_string(),
                        ))
            })
            .collect();

        // Fixture sanity: both constraints must actually be Indeterminate
        // under the LSP's no-solver engine (2 total), so the assertion
        // below on `hints.len() == 1` is a genuine discrimination check,
        // not a vacuous pass because only one constraint was Indeterminate
        // to begin with.
        let check_result = state.engine.check_snapshot(&compiled).expect(
            "state.engine should hold a snapshot for SRC's content hash \
             immediately after compute_diagnostics_with_state evaluated it",
        );
        let indeterminate_count = check_result
            .constraint_results
            .iter()
            .filter(|e| e.satisfaction == Satisfaction::Indeterminate)
            .count();
        assert_eq!(
            indeterminate_count, 2,
            "fixture sanity: expected 2 Indeterminate constraints (the \
             compound FEA constraint + the gap auto-param constraint); got \
             constraint_results: {:#?}",
            check_result.constraint_results
        );

        assert_eq!(
            hints.len(),
            1,
            "expected exactly one FEA hint: the compound FEA constraint's \
             two `peak_stress` refs must dedup to one hint, and the \
             unrelated `gap` auto-param constraint (Indeterminate for a \
             non-FEA reason) must be excluded entirely; got {} hints: {:#?}",
            hints.len(),
            hints
        );

        // Belt-and-suspenders: directly confirm the surviving hint is not
        // anchored to the `gap > 1mm` constraint's span (identify that
        // constraint's span by slicing its source text out of SRC, rather
        // than assuming declaration order).
        let gap_constraint_span = compiled
            .templates
            .iter()
            .flat_map(|t| t.constraints.iter())
            .find(|c| SRC[c.span.start as usize..c.span.end as usize].contains("gap"))
            .map(|c| c.span)
            .expect("fixture sanity: expected a constraint referencing `gap`");
        let gap_range = convert::span_to_range(SRC, gap_constraint_span);
        assert!(
            !hints.iter().any(|h| h.range == gap_range),
            "no FEA hint should be anchored to the auto-param `gap` \
             constraint's span ({gap_range:?}); got hints: {hints:#?}"
        );
    }

    /// Amendment regression lock (task 5078, PRD `compute-fea-hardening.md`
    /// C2): the FEA "not evaluated in editor" Info hint must not double up
    /// with a freshness diagnostic (`computation-pending` /
    /// `computation-failed`, arch §7.1/§9.2, emitted by the loop just below
    /// this hint's emission block) for the same underlying unevaluated FEA
    /// value cell over the [`FEA_BEARING_SRC`] fixture.
    ///
    /// Under the trampoline-free posture, an unregistered `@optimized`
    /// target's handling in `engine_eval.rs` (the "no registered compute
    /// trampoline (falling back to body-inlining)" branch) emits its own
    /// `Severity::Error` diagnostic and falls through to ordinary
    /// expression evaluation — it does NOT call `mark_failed` /
    /// `mark_pending` the way a *registered*-but-failed trampoline dispatch
    /// does. So the FEA-derived value cells (`result`, `peak_stress`)
    /// resolve to `Freshness::Final`, not `Pending`/`Failed`, and the
    /// freshness loop has nothing to say about them. This test locks that
    /// observation so a future change to the unregistered-target fallback
    /// path cannot silently reintroduce duplicated/noisy editor
    /// diagnostics without failing a test.
    #[test]
    fn fea_hint_does_not_duplicate_as_freshness_diagnostic() {
        let uri = test_uri();
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, FEA_BEARING_SRC, &uri);

        let hints: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::INFORMATION)
                    && d.code
                        == Some(lsp_types::NumberOrString::String(
                            FEA_NOT_EVALUATED_CODE.to_string(),
                        ))
            })
            .collect();
        assert!(
            !hints.is_empty(),
            "fixture sanity: expected at least one FEA Info hint; got: {:#?}",
            result.diagnostics
        );

        let freshness_codes = [
            lsp_types::NumberOrString::String("computation-pending".to_string()),
            lsp_types::NumberOrString::String("computation-failed".to_string()),
        ];
        let freshness_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.code.as_ref().is_some_and(|c| freshness_codes.contains(c)))
            .collect();
        assert!(
            freshness_diags.is_empty(),
            "FEA-bearing fixture must not ALSO produce freshness diagnostics \
             (computation-pending/computation-failed) alongside the Info \
             hint — got: {freshness_diags:#?}"
        );
    }

    /// Amendment regression lock (task 5078, PRD `compute-fea-hardening.md`
    /// C2): pins the *other* half of
    /// `constraint_depends_on_unregistered_optimized_compute`'s
    /// discriminator — `engine.compute_dispatch(t).is_none()` — that every
    /// other FEA-hint test leaves unexercised. Every other test builds its
    /// `EvalState` via `EvalState::new()`, which never registers a compute
    /// trampoline, so `compute_dispatch` returns `None` for every target in
    /// every other test; the "registered target ⇒ not FEA-dependent ⇒ no
    /// hint" branch of the discriminator has never actually run.
    ///
    /// Registers a trampoline for `"solver::elastic_static"` that itself
    /// completes with `Value::Undef` — deliberately mirroring the *value*
    /// the unregistered body-inline fallback would have produced, so the
    /// constraint remains genuinely `Satisfaction::Indeterminate` (the fixture
    /// sanity check below confirms this) and the emission loop actually
    /// reaches the discriminator instead of short-circuiting earlier on
    /// `entry.satisfaction != Indeterminate`. Only the *registration status*
    /// differs from the unregistered-path tests above — isolating the
    /// `compute_dispatch(..).is_none()` check as the thing this test can
    /// catch a regression in.
    ///
    /// Registration happens *after* a priming first call, not before: a
    /// freshly-constructed `EvalState` is not yet "initialized"
    /// ([`EvalState::is_engine_initialized`]), so `compute_diagnostics_with_state`'s
    /// cold-start branch would replace `state.engine` wholesale with a fresh
    /// `Engine::new(..)` — silently discarding a trampoline registered
    /// beforehand. Mirrors the priming pattern in
    /// `incremental_path_uses_eval_cached_when_content_unchanged` (above):
    /// call once to initialize the engine, register, then call again with
    /// unchanged content so the `eval_cached` path is taken and `state.engine`
    /// is reused rather than replaced.
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

        let uri = test_uri();
        let mut state = EvalState::new();

        // Priming call: cold-starts and initializes `state.engine` (no
        // trampoline registered yet, so this behaves like the ordinary
        // unregistered/body-inline path — its result is discarded).
        let _ = compute_diagnostics_with_state(&mut state, FEA_BEARING_SRC, &uri);

        // Register on the now-initialized, surviving engine instance.
        state.engine.register_compute_fn(
            "solver::elastic_static",
            undef_result_fn as reify_eval::ComputeFn,
        );
        assert!(
            state
                .engine
                .compute_dispatch("solver::elastic_static")
                .is_some(),
            "test setup sanity: solver::elastic_static must be registered \
             before the second compute_diagnostics_with_state call"
        );

        // Second call, same content: takes the `eval_cached` path (content
        // unchanged + engine now initialized), which does NOT replace
        // `state.engine` — the registered trampoline survives into it.
        let result = compute_diagnostics_with_state(&mut state, FEA_BEARING_SRC, &uri);

        let parsed =
            reify_compiler::parse_with_stdlib(FEA_BEARING_SRC, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);
        let check_result = state.engine.check_snapshot(&compiled).expect(
            "state.engine should hold a snapshot for FEA_BEARING_SRC's content hash \
             immediately after compute_diagnostics_with_state evaluated it",
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

        let hints: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::INFORMATION)
                    && d.code
                        == Some(lsp_types::NumberOrString::String(
                            FEA_NOT_EVALUATED_CODE.to_string(),
                        ))
            })
            .collect();
        assert!(
            hints.is_empty(),
            "no FEA \"not evaluated in editor\" hint should fire once \
             solver::elastic_static has a registered compute trampoline \
             (compute_dispatch(..).is_some()) — got: {hints:#?}"
        );
    }

    /// Amendment (reviewer finding, task 5078): pins that TWO separate
    /// FEA-dependent `Indeterminate` constraints each get their OWN hint,
    /// anchored to their OWN distinct constraint span — not collapsed into
    /// one, and not both anchored to the same span. [`FEA_BEARING_SRC`]
    /// declares exactly two constraints over the same FEA-derived
    /// `peak_stress` cell (`peak_stress < 1MPa` and `peak_stress <
    /// 100MPa`), so this is the natural fixture for it.
    ///
    /// This is the scenario most sensitive to a regression in the upstream
    /// "at most one `constraint_results` entry per constraint id" invariant
    /// that the emission loop's dedup-elision rationale (in
    /// `compute_diagnostics_with_state`, above) relies on instead of an
    /// explicit `(id, label)` guard: a regression there could plausibly
    /// collapse two constraints' entries into one, or misattribute one
    /// hint's span — `fea_indeterminate_constraint_emits_info_hint`'s
    /// count-only assertion would not by itself catch a same-span collapse.
    #[test]
    fn fea_hint_two_fea_constraints_each_get_distinct_span_hint() {
        let uri = test_uri();
        let parsed =
            reify_compiler::parse_with_stdlib(FEA_BEARING_SRC, ModulePath::single("test"));
        let compiled = compile_like_production(&parsed);

        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, FEA_BEARING_SRC, &uri);

        // Locate each of FEA_BEARING_SRC's two known constraints by slicing
        // its own source span (not by assuming declaration order). "< 1MPa"
        // is not a substring of "< 100MPa" (the digits between `<` and `M`
        // differ), so these needles are unambiguous.
        let find_span = |needle: &str| {
            compiled
                .templates
                .iter()
                .flat_map(|t| t.constraints.iter())
                .find(|c| {
                    FEA_BEARING_SRC[c.span.start as usize..c.span.end as usize].contains(needle)
                })
                .map(|c| c.span)
                .unwrap_or_else(|| {
                    panic!("fixture sanity: expected a constraint containing {needle:?}")
                })
        };
        let range_1mpa = convert::span_to_range(FEA_BEARING_SRC, find_span("< 1MPa"));
        let range_100mpa = convert::span_to_range(FEA_BEARING_SRC, find_span("< 100MPa"));
        assert_ne!(
            range_1mpa, range_100mpa,
            "fixture sanity: FEA_BEARING_SRC's two constraints must have \
             distinct spans"
        );

        let hints: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::INFORMATION)
                    && d.code
                        == Some(lsp_types::NumberOrString::String(
                            FEA_NOT_EVALUATED_CODE.to_string(),
                        ))
            })
            .collect();

        assert_eq!(
            hints.len(),
            2,
            "expected exactly two FEA hints, one per FEA-dependent \
             constraint; got {} hints: {:#?}",
            hints.len(),
            hints
        );
        assert!(
            hints.iter().any(|h| h.range == range_1mpa),
            "expected a hint anchored to the `peak_stress < 1MPa` \
             constraint's own span ({range_1mpa:?}); got hints: {hints:#?}"
        );
        assert!(
            hints.iter().any(|h| h.range == range_100mpa),
            "expected a hint anchored to the `peak_stress < 100MPa` \
             constraint's own span ({range_100mpa:?}); got hints: {hints:#?}"
        );
        assert_ne!(
            hints[0].range, hints[1].range,
            "the two hints must be anchored to two distinct spans, not both \
             collapsed onto the same one; got hints: {hints:#?}"
        );
    }
}
