//! Unified build-DAG fixpoint driver (task 4357 δ).
//!
//! This module holds `run_unified_pass` — an online Kahn topological worklist
//! over α's existing forward dependency-trace graph (O(V+E)) — plus the cycle
//! contract (Stage A hang-proof Kahn residue + Stage B Tarjan-SCC discriminator
//! → `E_EVAL_CYCLE`) and the geometry-backed-constraint-on-auto guard
//! (→ `E_EVAL_UNRESOLVED`).
//!
//! The driver is a PURE STRUCTURAL PLANNER: it returns a `(schedule, residue,
//! diagnostics)` triple and does NOT execute nodes (no kernel calls, no handle
//! inserts, no value writes). Node execution and the runtime `Determined`
//! readiness gate are layered on by the ε executors that consume the schedule.
//!
//! See `docs/prds/v0_6/engine-unified-build-dag.md` for the full design.
//!
//! The module and `run_unified_pass` compile unconditionally so the cycle
//! contract is always unit-testable; the `unified-dag` Cargo feature +
//! `REIFY_BUILD_SCHEDULER` env var gate ONLY the production activation of the
//! driver inside `Engine::build()`.

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 4357 δ (step-5): `BuildScheduler::from_env_value` is the PURE
    /// (no real env read) string→scheduler parser. Default is `LegacyMultiPass`;
    /// `"unified"` parses to `UnifiedDag` (feature-independent at the parser
    /// layer); case-insensitive + trimmed; any unrecognized/garbage value
    /// defaults to `LegacyMultiPass`. Pure ⇒ parallel-safe.
    ///
    /// RED until step-6 adds the enum + parser.
    #[test]
    fn build_scheduler_from_env_value_parsing() {
        // Default: absent env → Legacy.
        assert_eq!(
            BuildScheduler::from_env_value(None),
            BuildScheduler::LegacyMultiPass
        );
        // Explicit legacy.
        assert_eq!(
            BuildScheduler::from_env_value(Some("legacy")),
            BuildScheduler::LegacyMultiPass
        );
        // Explicit unified (pure parser — feature-independent).
        assert_eq!(
            BuildScheduler::from_env_value(Some("unified")),
            BuildScheduler::UnifiedDag
        );
        // Case-insensitive + surrounding whitespace tolerated.
        assert_eq!(
            BuildScheduler::from_env_value(Some("  UNIFIED ")),
            BuildScheduler::UnifiedDag
        );
        assert_eq!(
            BuildScheduler::from_env_value(Some("Legacy")),
            BuildScheduler::LegacyMultiPass
        );
        // Garbage / empty → default Legacy.
        assert_eq!(
            BuildScheduler::from_env_value(Some("garbage")),
            BuildScheduler::LegacyMultiPass
        );
        assert_eq!(
            BuildScheduler::from_env_value(Some("")),
            BuildScheduler::LegacyMultiPass
        );
    }

    /// Task 4357 δ (step-5): the `Default` impl must be `LegacyMultiPass` so an
    /// un-configured engine keeps byte-identical legacy behaviour.
    #[test]
    fn build_scheduler_default_is_legacy() {
        assert_eq!(BuildScheduler::default(), BuildScheduler::LegacyMultiPass);
    }
}
