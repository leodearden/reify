// Split from engine_build.rs / engine_purposes.rs (task 2792) — tolerance query methods.

use crate::Engine;
use reify_compiler::CompiledModule;
use reify_core::Diagnostic;

impl Engine {
    /// Look up the imported-geometry tolerance promise carried by the
    /// `param tolerance : Length = X` declaration on an `Input` occurrence
    /// template (e.g. `STEPInput` / `STLInput`); returns `Some(si_value)`
    /// in metres or `None` when no `eval_state` exists or the cell is
    /// absent/malformed.
    ///
    /// Thin delegator to
    /// [`crate::tolerance_promise::extract_input_tolerance_promise`] — see
    /// that function for the recognition shape, the silent-skip gate audit,
    /// and the PRD cross-references. Sibling demand-side query is
    /// [`Engine::demanded_tolerance_for_output`].
    pub fn imported_tolerance_promise(&self, input_template_name: &str) -> Option<f64> {
        let state = self.eval_state.as_ref()?;
        crate::tolerance_promise::extract_input_tolerance_promise(
            &state.snapshot.values,
            input_template_name,
        )
    }

    /// Compare the imported-geometry tolerance promise against the demand
    /// for a downstream output occurrence; return a `Some(Severity::Warning)`
    /// diagnostic when a mismatch is detected, otherwise `None`.
    ///
    /// ## Dispatch order (task 2833 — mutual exclusivity)
    ///
    /// Two branches fire in priority order:
    ///
    /// 1. **Zero-promise lint** ([`DiagnosticCode::InputTolerancePromiseIsZero`]):
    ///    fires when `promise == 0.0 && demanded > 0.0`. Surfaces the
    ///    placeholder-default footgun where `param tolerance : Length = 0m`
    ///    silently disables the insufficient-promise warning. Builder:
    ///    [`crate::tolerance_promise::input_tolerance_promise_is_zero_diagnostic`].
    ///
    /// 2. **Insufficient-promise lint** ([`DiagnosticCode::ImportedTolerancePromiseInsufficient`]):
    ///    fires when `demanded < promise` (strict-`<`). Builder:
    ///    [`crate::tolerance_promise::imported_tolerance_promise_diagnostic`].
    ///
    /// The two branches are **mutually exclusive**: when `promise == 0.0`,
    /// `is_promise_insufficient(demanded, 0.0)` evaluates `demanded < 0.0`,
    /// which is false for every `demanded >= 0.0`. The strict-`<` branch
    /// therefore never fires when promise is zero, so a single `Option<Diagnostic>`
    /// return remains correct — no caller needs updating.
    ///
    /// Placing the zero-promise check BEFORE the strict-`<` check ensures it
    /// fires on the `(0.0, positive)` row. Placing it after would still be
    /// functionally equivalent (due to mutual exclusivity) but would obscure
    /// dispatch intent.
    ///
    /// ## Degenerate case
    ///
    /// When `promise == 0.0 && demanded == 0.0` (both zero), neither branch
    /// fires — `demanded > 0.0` is false, so the zero-promise guard rejects
    /// the case, and `demanded < 0.0` is false, so the strict-`<` guard also
    /// rejects. Returns `None`. This matches the canonical `(0.0, 0.0) -> false`
    /// row of `is_promise_insufficient`'s truth table ("both zero → sufficient").
    ///
    /// See [`crate::tolerance_promise`] for the strict-`<` rationale, the full
    /// truth table (pinned by `tests/tolerance_import_promise.rs`), and PRD
    /// cross-references. Auto-emission from `build()` / `build_snapshot()` is
    /// deferred to the dispatcher (sibling task 2649); this method is the public
    /// query single-entry-point.
    pub fn check_imported_tolerance_promise(
        &self,
        input_template_name: &str,
        subject_entity_ref: &str,
        output_template_name: &str,
    ) -> Option<Diagnostic> {
        let promise = self.imported_tolerance_promise(input_template_name)?;
        let demanded =
            self.demanded_tolerance_for_output(output_template_name, subject_entity_ref)?;
        // Zero-promise lint (task 2833, option-b continuation): checked BEFORE the
        // strict-`<` insufficient branch. The `f64 == 0.0` comparison is intentional —
        // the upstream extractor's gate (`!si_value.is_finite() || si_value < 0.0`)
        // rejects NaN / ±Inf and **strictly negative finite** values, so any `f64`
        // reaching this point is finite and >= 0.0; equality is well-defined.
        //
        // Signed-zero note: `-0.0` is NOT rejected by the upstream gate because
        // `-0.0 < 0.0` is false in IEEE-754. It therefore reaches this comparator.
        // IEEE-754 specifies `-0.0 == 0.0`, so the zero-promise branch fires
        // correctly for both `+0.0` and `-0.0` promises — behavior is identical and
        // benign. See the Gate-4 audit comment in `tolerance_promise.rs` for the
        // symmetric upstream note.
        if promise == 0.0 && demanded > 0.0 {
            return Some(
                crate::tolerance_promise::input_tolerance_promise_is_zero_diagnostic(
                    input_template_name,
                    demanded,
                ),
            );
        }
        if crate::tolerance_promise::is_promise_insufficient(demanded, promise) {
            Some(
                crate::tolerance_promise::imported_tolerance_promise_diagnostic(
                    input_template_name,
                    demanded,
                    promise,
                ),
            )
        } else {
            None
        }
    }

    /// Look up the active tolerance (SI metres) for `entity_ref`, computed
    /// from the currently active purposes whose subject prefix-scan covers
    /// `entity_ref`. Returns `None` if no active purpose contributes a
    /// tolerance for this entity.
    ///
    /// The returned value is the *minimum* across all active contributors —
    /// tighter satisfies looser, the same partial-order semantics as the
    /// cache-side `ToleranceBucket` (task 2648). This is the demand-side
    /// counterpart that tells the dispatcher (sibling tasks 2649/2650) which
    /// tolerance to ask for when materialising realizations for `entity_ref`.
    ///
    /// Per PRD `docs/prds/v0_2/per-purpose-tolerance.md` ("Resolved design
    /// decisions" → "Tolerance lives at the purpose"), task 2647. The
    /// extraction/propagation/merge primitives live in
    /// `crates/reify-eval/src/tolerance_scope.rs`.
    pub fn active_tolerance_for(&self, entity_ref: &str) -> Option<f64> {
        self.active_tolerance_scope.get(entity_ref).copied()
    }

    /// Look up the demanded tolerance (SI metres) for an output occurrence
    /// instance — the tighter of (a) the output template's own
    /// `RepresentationWithin` bound and (b) the active purpose's tolerance
    /// scope at `subject_entity_ref`. Returns `None` if neither contributor
    /// has a tolerance for this query.
    ///
    /// # Two distinct keys
    ///
    /// The two contributors are keyed differently because they live at
    /// conceptually different scopes (per arch §14.5 vs §14.4):
    ///
    /// - `output_template_name` — the output occurrence's *template* name
    ///   (e.g. `"STEPOutput"`). Output-occurrence body constraints stay
    ///   under their template-name entity scope in the runtime graph
    ///   regardless of how many times the occurrence is sub-instantiated
    ///   (subs duplicate value cells under scoped entity-refs but do NOT
    ///   scope-duplicate constraints — see
    ///   `crate::graph::EvaluationGraph::from_templates`). Resolved via
    ///   [`crate::tolerance_combine::extract_output_tolerance_bound`].
    /// - `subject_entity_ref` — the realization target (e.g. `"MyDesign"`)
    ///   the active purpose's subject prefix-scan covers. Resolved via
    ///   [`Self::active_tolerance_for`].
    ///
    /// Decoupling the two keys keeps the API explicit about which lookup
    /// is which — a single coalesced argument would force callers to pass
    /// the same string for two semantically distinct lookups.
    ///
    /// # Combination rule
    ///
    /// Both bounds are folded by
    /// [`crate::tolerance_combine::combine_demanded_tolerance`]. Each row
    /// is pinned by an integration test in `tests/tolerance_combine.rs`:
    ///
    /// | output_bound | purpose_bound | demanded_tolerance_for_output | scenario       | pinned by                                                     |
    /// |--------------|---------------|-------------------------------|----------------|---------------------------------------------------------------|
    /// | `Some(o)`    | `Some(p)`     | `Some(o.min(p))`              | both-active    | `engine_demanded_tolerance_for_output_combines_via_min_when_both_active` |
    /// | `Some(t)`    | `None`        | `Some(t)`                     | output-only    | `engine_demanded_tolerance_for_output_handles_partial_inputs` (a)        |
    /// | `None`       | `Some(t)`     | `Some(t)`                     | purpose-only   | `engine_demanded_tolerance_for_output_handles_partial_inputs` (b)        |
    /// | `None`       | `None`        | `None`                        | neither        | `engine_demanded_tolerance_for_output_handles_partial_inputs` (c)        |
    ///
    /// "Tighter satisfies looser" — same partial-order semantics as the
    /// cache-side `tolerance_bucket` `<=` rule and the purpose-side
    /// `tolerance_scope::merge_with_min`.
    ///
    /// Pre-eval (`eval_state == None`) the output-bound query naturally
    /// returns `None` (no graph to scan); the combiner then falls back to
    /// whatever the purpose-side contributes. No explicit guard needed.
    ///
    /// Per PRD `docs/prds/v0_2/per-purpose-tolerance.md` ("Resolved design
    /// decisions" → "Tolerance lives at the purpose"), task 2650.
    pub fn demanded_tolerance_for_output(
        &self,
        output_template_name: &str,
        subject_entity_ref: &str,
    ) -> Option<f64> {
        let output_bound = self.eval_state.as_ref().and_then(|state| {
            crate::tolerance_combine::extract_output_tolerance_bound(
                &state.snapshot.graph.constraints,
                output_template_name,
            )
        });
        let purpose_bound = self.active_tolerance_for(subject_entity_ref);
        crate::tolerance_combine::combine_demanded_tolerance(output_bound, purpose_bound)
    }

    /// Walk `module.templates`, identify (Input, Output) occurrence templates,
    /// and emit one imported-tolerance-promise diagnostic per
    /// (Input × Output × active-purpose-binding) triple by forwarding the
    /// `Some(diag)` return of [`Engine::check_imported_tolerance_promise`].
    ///
    /// # Recognition shapes
    ///
    /// - **Input templates** — those whose `(template.name, "tolerance")`
    ///   value-cell entry passes [`crate::tolerance_promise::extract_input_tolerance_promise`]
    ///   (i.e. the promise extractor returns `Some`).
    /// - **Output templates** — those whose constraints contain a
    ///   `RepresentationWithin(<ValueRef typed StructureRef>, <length-literal>)`
    ///   shape recognised by [`crate::tolerance_combine::extract_output_tolerance_bound`]
    ///   (i.e. the bound extractor returns `Some`).
    ///
    /// # Subject entity binding
    ///
    /// `Engine::active_purpose_bindings` is the canonical
    /// `purpose_name → entity_ref` map populated by `activate_purpose`. The
    /// helper iterates the bindings' values (each value is a subject
    /// `entity_ref`) and treats each as the third argument to
    /// [`Engine::check_imported_tolerance_promise`].
    ///
    /// # Code-agnostic forwarding
    ///
    /// The dispatcher `check_imported_tolerance_promise` emits **two** distinct
    /// `DiagnosticCode`s — `ImportedTolerancePromiseInsufficient` (strict-`<`
    /// branch) and `InputTolerancePromiseIsZero` (zero-promise branch). This
    /// helper's contract is to **forward whatever the dispatcher emits**, with
    /// no code-filtering — both branches must reach `BuildResult.diagnostics`
    /// so production-side observability matches the unit-test surface in
    /// `tests/tolerance_import_promise.rs`.
    ///
    /// # Empty cases
    ///
    /// - No `eval_state`: helper is a no-op (no snapshot to scan).
    /// - No active purposes: helper is a no-op (no `(input, output, entity)`
    ///   triple has a subject to demand against).
    /// - No Input templates OR no Output templates: helper is a no-op (no
    ///   pair to dispatch on).
    pub(crate) fn emit_imported_tolerance_promise_diagnostics_for_module(
        &self,
        module: &CompiledModule,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Bail early if no eval state — no snapshot to scan, so neither the
        // promise extractor nor the bound extractor can fire.
        let state = match self.eval_state.as_ref() {
            Some(s) => s,
            None => return,
        };

        // Identify Input occurrence templates — those whose
        // `extract_input_tolerance_promise` returns Some(_) against the
        // post-eval snapshot's value-cell map. The probe is bounded by the
        // existing extractor's complexity (one HashMap lookup + scalar gate).
        let input_template_names: Vec<&str> = module
            .templates
            .iter()
            .filter(|t| {
                crate::tolerance_promise::extract_input_tolerance_promise(
                    &state.snapshot.values,
                    &t.name,
                )
                .is_some()
            })
            .map(|t| t.name.as_str())
            .collect();

        // Identify Output occurrence templates — those whose
        // `extract_output_tolerance_bound` returns Some(_) against the
        // post-eval snapshot's constraint map.
        let output_template_names: Vec<&str> = module
            .templates
            .iter()
            .filter(|t| {
                crate::tolerance_combine::extract_output_tolerance_bound(
                    &state.snapshot.graph.constraints,
                    &t.name,
                )
                .is_some()
            })
            .map(|t| t.name.as_str())
            .collect();

        // For every (Input × Output × active-purpose-binding) triple, forward
        // whatever `check_imported_tolerance_promise` returns. The helper is
        // code-agnostic: both `ImportedTolerancePromiseInsufficient` and
        // `InputTolerancePromiseIsZero` codes flow through unchanged.
        for input_name in &input_template_names {
            for output_name in &output_template_names {
                for entity_ref in self.active_purpose_bindings.values() {
                    if let Some(diag) =
                        self.check_imported_tolerance_promise(input_name, entity_ref, output_name)
                    {
                        diagnostics.push(diag);
                    }
                }
            }
        }
    }
}
