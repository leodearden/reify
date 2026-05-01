// Split from engine_build.rs / engine_purposes.rs (task 2792) — tolerance query methods.

use crate::Engine;
use reify_types::Diagnostic;

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
    /// for a downstream output occurrence; return
    /// `Some(Severity::Warning)` diagnostic when the demand is strictly
    /// tighter than the promise, otherwise `None`.
    ///
    /// Thin chain over
    /// [`Engine::imported_tolerance_promise`] +
    /// [`Engine::demanded_tolerance_for_output`] +
    /// [`crate::tolerance_promise::is_promise_insufficient`] +
    /// [`crate::tolerance_promise::imported_tolerance_promise_diagnostic`]
    /// — see [`crate::tolerance_promise`] for the strict-`<` rationale, the
    /// truth table (pinned by `tests/tolerance_import_promise.rs`), and the
    /// PRD cross-references. Auto-emission from `build()` / `build_snapshot()`
    /// is deferred to the dispatcher (sibling task 2649); this method is
    /// the public query single-entry-point.
    pub fn check_imported_tolerance_promise(
        &self,
        input_template_name: &str,
        subject_entity_ref: &str,
        output_template_name: &str,
    ) -> Option<Diagnostic> {
        let promise = self.imported_tolerance_promise(input_template_name)?;
        let demanded =
            self.demanded_tolerance_for_output(output_template_name, subject_entity_ref)?;
        if crate::tolerance_promise::is_promise_insufficient(demanded, promise) {
            Some(crate::tolerance_promise::imported_tolerance_promise_diagnostic(
                input_template_name,
                demanded,
                promise,
            ))
        } else {
            None
        }
    }
}
