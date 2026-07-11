//! Shared post-pass detector registry (task λ, #5043).
//!
//! PRD `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.7, INV-EVAL-3.
//!
//! Today's eval-only post-passes (the `MassProperties` PSD inertia
//! validation and the annotation-args materialization driver, both inline
//! in `Engine::eval`) run on the cold `eval()` path but NOT on
//! `eval_cached()` — a cold-only detector asymmetry — and their relative
//! ordering is encoded only in scattered "must run before …" / "MUST run
//! AFTER …" convention comments (e.g. `engine_eval.rs:6312`,
//! `structural_query.rs:531,610`, `significance_filter.rs:1025,1032`).
//!
//! This module provides the REGISTRY MECHANISM that replaces both: a single
//! shared post-pass detector registry any eval path can run identically,
//! where registration order IS run order — one owner, all paths.
//!
//! **Scope of this task**: the registry mechanism only. Wiring it into
//! `Engine::eval` / `Engine::eval_cached` / `Engine::edit_check`, plus
//! fast-path diagnostic replay, is task μ (#5044, depends on κ, λ, γ, δ) —
//! explicitly OUT of scope here.
//!
//! ## Known scope gaps for task μ
//!
//! - The annotation-args materialization driver (`engine_eval.rs:4400-4557`)
//!   needs heavy READ-ONLY `Engine` context (module/prelude/functions plus
//!   an `EvalContext`) that this module's post-pass state does not carry.
//!   Its per-path hand-off — especially on `edit_check` — is deferred to
//!   task μ (PRD §10 open-question #2).
//! - Per-node diagnostic attribution and `NodeCache` storage/replay
//!   (consuming task κ's per-node diagnostics vec) is also task μ's.
//! - The inline `MassProperties` PSD pass at `engine_eval.rs:4298-4397`
//!   stays in place until task μ removes it and wires this registry into
//!   the three eval paths (foundation-then-migrate, mirroring how task α
//!   shipped `commit_cell_result` ahead of its own migration leaves).

#[cfg(test)]
mod tests {
    use reify_core::{Diagnostic, DiagnosticCode, ValueCellId};
    use reify_ir::{DeterminacyState, PersistentMap, Value, ValueMap};

    use super::*;

    /// Projects a `Diagnostic` to a comparable key. `Diagnostic` derives
    /// only `Debug, Clone` (no `PartialEq`), but `DiagnosticCode` does, so
    /// `(code, message)` is a faithful equality check for these tests.
    fn diag_key(d: &Diagnostic) -> (Option<DiagnosticCode>, String) {
        (d.code, d.message.clone())
    }

    fn diag_keys(diags: &[Diagnostic]) -> Vec<(Option<DiagnosticCode>, String)> {
        diags.iter().map(diag_key).collect()
    }

    /// A deterministic test-double detector: pushes one fixed diagnostic and
    /// inserts one fixed marker cell into `values` — a pure function of its
    /// own fields, reading nothing from the incoming state.
    struct FixedMarkerDetector {
        slug: &'static str,
        code: DiagnosticCode,
        message: &'static str,
        marker_cell: ValueCellId,
    }

    impl PostPassDetector for FixedMarkerDetector {
        fn id(&self) -> &'static str {
            self.slug
        }

        fn run(&self, state: &mut PostPassState<'_>) {
            state
                .diagnostics
                .push(Diagnostic::error(self.message.to_string()).with_code(self.code));
            state
                .values
                .insert(self.marker_cell.clone(), Value::Bool(true));
        }
    }

    fn detector_a() -> Box<dyn PostPassDetector> {
        Box::new(FixedMarkerDetector {
            slug: "test-detector-a",
            code: DiagnosticCode::ConstraintViolated,
            message: "detector A fired",
            marker_cell: ValueCellId::new("Body", "a_marker"),
        })
    }

    fn detector_b() -> Box<dyn PostPassDetector> {
        Box::new(FixedMarkerDetector {
            slug: "test-detector-b",
            code: DiagnosticCode::SelectorKindMismatch,
            message: "detector B fired",
            marker_cell: ValueCellId::new("Body", "b_marker"),
        })
    }

    /// A freshly-built, empty post-pass state triple, matching what every
    /// eval path assembles at its post-pass point.
    struct OwnedState {
        values: ValueMap,
        snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        diagnostics: Vec<Diagnostic>,
    }

    impl OwnedState {
        fn empty() -> Self {
            Self {
                values: ValueMap::new(),
                snapshot_values: PersistentMap::new(),
                diagnostics: Vec::new(),
            }
        }

        fn as_state(&mut self) -> PostPassState<'_> {
            PostPassState {
                values: &mut self.values,
                snapshot_values: &mut self.snapshot_values,
                diagnostics: &mut self.diagnostics,
            }
        }
    }

    /// INV-EVAL-3's core done-criteria: the SAME registry, run against
    /// independently-assembled but EQUAL post-pass state, yields identical
    /// diagnostics — modelling "runs identically on eval / eval_cached /
    /// edit_check": same registry + equal state ⇒ identical output
    /// regardless of which call site assembled the state.
    #[test]
    fn same_post_pass_state_yields_same_diagnostic_set() {
        let mut registry = DetectorRegistry::new();
        registry.register(detector_a());
        registry.register(detector_b());

        // Three INDEPENDENTLY-built, but equal (empty), states — mimicking
        // cold eval / warm eval_cached / edit_check assembling their own
        // post-pass state at three different call sites.
        let mut site_eval = OwnedState::empty();
        let mut site_eval_cached = OwnedState::empty();
        let mut site_edit_check = OwnedState::empty();

        registry.run_all(&mut site_eval.as_state());
        registry.run_all(&mut site_eval_cached.as_state());
        registry.run_all(&mut site_edit_check.as_state());

        let expected = vec![
            (
                Some(DiagnosticCode::ConstraintViolated),
                "detector A fired".to_string(),
            ),
            (
                Some(DiagnosticCode::SelectorKindMismatch),
                "detector B fired".to_string(),
            ),
        ];
        assert_eq!(diag_keys(&site_eval.diagnostics), expected);
        assert_eq!(diag_keys(&site_eval_cached.diagnostics), expected);
        assert_eq!(diag_keys(&site_edit_check.diagnostics), expected);

        // The value-map mutation is also identical across all three sites.
        for site in [&site_eval, &site_eval_cached, &site_edit_check] {
            assert_eq!(
                site.values.get(&ValueCellId::new("Body", "a_marker")),
                Some(&Value::Bool(true))
            );
            assert_eq!(
                site.values.get(&ValueCellId::new("Body", "b_marker")),
                Some(&Value::Bool(true))
            );
        }
    }
}
