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

use reify_core::{Diagnostic, ValueCellId};
use reify_ir::{DeterminacyState, PersistentMap, Value, ValueMap};

/// The minimal UNIVERSAL mutable state a detector reads/mutates, bundled as
/// disjoint `&mut` borrows — mirrors [`crate::cell_commit::CommitLegs`].
///
/// All three fields are present on every eval path: `Engine::eval` and
/// `Engine::eval_cached` both hold a `values` map, a `snapshot.values` map,
/// and a `diagnostics` sink at their post-pass point. A detector operating
/// only on this bundle therefore runs identically regardless of which path
/// assembled the state — INV-EVAL-3.
///
/// Detectors needing heavier READ-ONLY context (e.g. the annotation-args
/// driver's module/prelude/functions + `EvalContext`) are out of scope for
/// this task; see the module doc's "Known scope gaps for task μ". Task μ
/// may extend this struct if a migrated detector needs more.
#[allow(dead_code)] // wired in by task μ; exercised by tests until then
pub(crate) struct PostPassState<'a> {
    pub(crate) values: &'a mut ValueMap,
    pub(crate) snapshot_values: &'a mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    pub(crate) diagnostics: &'a mut Vec<Diagnostic>,
}

/// A single post-pass check runnable identically on any eval path.
///
/// Contract: equal input [`PostPassState`] → equal diagnostic sequence +
/// equal state mutations (INV-EVAL-3). Implementations should treat `state`
/// as the only source of truth — no hidden global/interior state.
#[allow(dead_code)] // wired in by task μ; exercised by tests until then
pub(crate) trait PostPassDetector {
    /// A stable kebab-case slug identifying this detector — used for
    /// ordering introspection ([`DetectorRegistry::ids`]) and debugging.
    fn id(&self) -> &'static str;

    /// Runs the check against `state`, mutating it in place (pushing
    /// diagnostics, replacing values) as needed.
    fn run(&self, state: &mut PostPassState<'_>);
}

/// An ordered collection of [`PostPassDetector`]s — registration order IS
/// run order.
///
/// This is the single owner of post-pass ordering (INV-EVAL-3), replacing
/// the scattered "must run before …" / "MUST run AFTER …" ordering-
/// convention comments (e.g. `engine_eval.rs:6312`,
/// `structural_query.rs:531,610`, `significance_filter.rs:1025,1032`) with
/// one readable, explicit `Vec`.
#[allow(dead_code)] // wired in by task μ; exercised by tests until then
#[derive(Default)]
pub(crate) struct DetectorRegistry {
    detectors: Vec<Box<dyn PostPassDetector>>,
}

#[allow(dead_code)] // wired in by task μ; exercised by tests until then
impl DetectorRegistry {
    /// An empty registry.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Appends `detector` to the registry. Registration order is run order.
    pub(crate) fn register(&mut self, detector: Box<dyn PostPassDetector>) {
        self.detectors.push(detector);
    }

    /// Runs every registered detector, in registration order, against
    /// `state`.
    pub(crate) fn run_all(&self, state: &mut PostPassState<'_>) {
        for detector in &self.detectors {
            detector.run(state);
        }
    }

    /// The registered detectors' ids, in registration (= run) order — the
    /// fixed, introspectable order task μ relies on.
    pub(crate) fn ids(&self) -> Vec<&'static str> {
        self.detectors.iter().map(|d| d.id()).collect()
    }
}

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

    /// Registration order IS run order — the single ordering core that
    /// replaces the scattered "must run before …" / "MUST run AFTER …"
    /// convention comments (e.g. `engine_eval.rs:6312`,
    /// `structural_query.rs:531,610`, `significance_filter.rs:1025,1032`).
    /// Order is caller-controlled registration order, not source-file
    /// scatter: the SAME three detectors registered in two DIFFERENT orders
    /// each run — and report via `ids()` — in their own registration order.
    #[test]
    fn run_all_executes_in_registration_order() {
        fn detector(slug: &'static str, code: DiagnosticCode) -> Box<dyn PostPassDetector> {
            Box::new(FixedMarkerDetector {
                slug,
                code,
                message: "fired",
                marker_cell: ValueCellId::new("Body", slug),
            })
        }

        // Registered in source order A, B, C.
        let mut registry_abc = DetectorRegistry::new();
        registry_abc.register(detector("a", DiagnosticCode::ConstraintViolated));
        registry_abc.register(detector("b", DiagnosticCode::SelectorKindMismatch));
        registry_abc.register(detector("c", DiagnosticCode::ConstraintIndeterminate));

        assert_eq!(registry_abc.ids(), vec!["a", "b", "c"]);

        let mut state_abc = OwnedState::empty();
        registry_abc.run_all(&mut state_abc.as_state());
        assert_eq!(
            diag_keys(&state_abc.diagnostics),
            vec![
                (Some(DiagnosticCode::ConstraintViolated), "fired".to_string()),
                (Some(DiagnosticCode::SelectorKindMismatch), "fired".to_string()),
                (
                    Some(DiagnosticCode::ConstraintIndeterminate),
                    "fired".to_string()
                ),
            ]
        );

        // The SAME three detectors, registered in a DIFFERENT order
        // (C, A, B): both the effect order and ids() follow the new
        // registration order, not the order above.
        let mut registry_cab = DetectorRegistry::new();
        registry_cab.register(detector("c", DiagnosticCode::ConstraintIndeterminate));
        registry_cab.register(detector("a", DiagnosticCode::ConstraintViolated));
        registry_cab.register(detector("b", DiagnosticCode::SelectorKindMismatch));

        assert_eq!(registry_cab.ids(), vec!["c", "a", "b"]);

        let mut state_cab = OwnedState::empty();
        registry_cab.run_all(&mut state_cab.as_state());
        assert_eq!(
            diag_keys(&state_cab.diagnostics),
            vec![
                (
                    Some(DiagnosticCode::ConstraintIndeterminate),
                    "fired".to_string()
                ),
                (Some(DiagnosticCode::ConstraintViolated), "fired".to_string()),
                (Some(DiagnosticCode::SelectorKindMismatch), "fired".to_string()),
            ]
        );
    }
}
