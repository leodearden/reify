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

/// Build-time scheduler selection (task 4357 δ).
///
/// Selects between the legacy multi-pass build loop and the unified build-DAG
/// Kahn worklist driver. Defaults to [`BuildScheduler::LegacyMultiPass`] so an
/// un-configured engine keeps byte-identical legacy behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildScheduler {
    /// Legacy multi-pass build loop (default; byte-preserving).
    #[default]
    LegacyMultiPass,
    /// Unified build-DAG: `run_unified_pass` Kahn worklist + cycle contract.
    UnifiedDag,
}

impl BuildScheduler {
    /// Environment variable consulted by [`BuildScheduler::from_env`].
    pub const ENV_VAR: &'static str = "REIFY_BUILD_SCHEDULER";

    /// Pure parser: map an optional configuration string to a scheduler.
    ///
    /// Feature-INDEPENDENT — `Some("unified")` always parses to `UnifiedDag` so
    /// the parser stays unit-testable without the `unified-dag` Cargo feature.
    /// Matching is case-insensitive and tolerates surrounding whitespace. Any
    /// unrecognized value — including `None`, empty, or garbage — defaults to
    /// `LegacyMultiPass`.
    ///
    /// The production [`BuildScheduler::from_env`] layers the `unified-dag`
    /// feature gate on top of this parser.
    pub fn from_env_value(value: Option<&str>) -> Self {
        let normalized = value.map(|v| v.trim().to_ascii_lowercase());
        match normalized.as_deref() {
            Some("unified") => BuildScheduler::UnifiedDag,
            _ => BuildScheduler::LegacyMultiPass,
        }
    }

    /// Production selection: read `REIFY_BUILD_SCHEDULER` and apply the
    /// `unified-dag` feature gate.
    ///
    /// `UnifiedDag` is selectable ONLY when the `unified-dag` Cargo feature is
    /// enabled. When the feature is disabled (the default), this always returns
    /// `LegacyMultiPass` regardless of the env value — the env gate is inert
    /// without the feature, so production builds opt in deliberately.
    pub fn from_env() -> Self {
        #[cfg(feature = "unified-dag")]
        {
            Self::from_env_value(std::env::var(Self::ENV_VAR).ok().as_deref())
        }
        #[cfg(not(feature = "unified-dag"))]
        {
            BuildScheduler::LegacyMultiPass
        }
    }
}

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

    // --- run_unified_pass driver tests (step-7+) ---

    use crate::cache::NodeId;
    use crate::deps::DependencyTrace;
    use crate::graph::EvaluationGraph;
    use reify_core::{ConstraintNodeId, RealizationNodeId, ResolutionNodeId, ValueCellId};
    use std::collections::{HashMap, HashSet};

    /// Build a `DependencyTrace` from explicit reads + realization_reads.
    fn trace(reads: Vec<ValueCellId>, realization_reads: Vec<RealizationNodeId>) -> DependencyTrace {
        DependencyTrace {
            reads,
            realization_reads,
        }
    }

    /// Map each scheduled node to its position for ordering assertions.
    fn positions(schedule: &[NodeId]) -> HashMap<NodeId, usize> {
        schedule
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, n)| (n, i))
            .collect()
    }

    /// Assert `schedule` is a valid topological order over `traces`: every node
    /// appears after ALL of its in-set `reads` (→Value) and `realization_reads`
    /// (→Realization) predecessors.
    fn assert_topo_valid(schedule: &[NodeId], traces: &HashMap<NodeId, DependencyTrace>) {
        let pos = positions(schedule);
        for (node, tr) in traces {
            let npos = pos[node];
            for r in &tr.reads {
                let p = NodeId::Value(r.clone());
                if let Some(&pp) = pos.get(&p) {
                    assert!(pp < npos, "Value pred {p} must precede {node}");
                }
            }
            for rr in &tr.realization_reads {
                let p = NodeId::Realization(rr.clone());
                if let Some(&pp) = pos.get(&p) {
                    assert!(pp < npos, "Realization pred {p} must precede {node}");
                }
            }
        }
    }

    /// Task 4357 δ (step-7): a synthetic ACYCLIC graph spanning every
    /// forward-trace edge kind — a param VC, a realization reading it
    /// (VC→Realization), a geometry VC backed by that realization
    /// (Realization→Value), a constraint reading the geometry
    /// (Constraint→Realization), a realization→realization GeomRef::Sub edge
    /// (Realization→Realization), and a Resolution whose reads = auto_params
    /// (Resolution→Value). `run_unified_pass` must produce a valid topological
    /// schedule covering EXACTLY the trace-map keys, with empty residue and zero
    /// diagnostics. The realization→realization edge pins that `realization_reads`
    /// participates in in-degree (which `compute_levels` ignores).
    ///
    /// RED until step-8 implements `run_unified_pass`.
    #[test]
    fn unified_pass_acyclic_all_edge_kinds_schedules_everything() {
        let e = "E";
        let p = ValueCellId::new(e, "p");
        let g = ValueCellId::new(e, "g");
        let a = ValueCellId::new(e, "a");
        // Producer index 1, consumer index 0: the consumer reads the producer
        // via realization_reads, so honoring that edge forces producer(idx1)
        // BEFORE consumer(idx0) — contradicting DebugOrd's natural "0 < 1" order.
        let r_prod = RealizationNodeId::new(e, 1);
        let r_cons = RealizationNodeId::new(e, 0);
        let c0 = ConstraintNodeId::new(e, 0);
        let s0 = ResolutionNodeId::new(e, 0);

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // Roots.
        traces.insert(NodeId::Value(p.clone()), trace(vec![], vec![]));
        traces.insert(NodeId::Value(a.clone()), trace(vec![], vec![]));
        // VC → Realization (producer reads param p).
        traces.insert(
            NodeId::Realization(r_prod.clone()),
            trace(vec![p.clone()], vec![]),
        );
        // Realization → Realization (consumer reads producer via GeomRef::Sub).
        traces.insert(
            NodeId::Realization(r_cons.clone()),
            trace(vec![], vec![r_prod.clone()]),
        );
        // Realization → Value (geometry cell backed by producer).
        traces.insert(NodeId::Value(g.clone()), trace(vec![], vec![r_prod.clone()]));
        // Constraint → Realization (constraint reads geometry/producer).
        traces.insert(
            NodeId::Constraint(c0.clone()),
            trace(vec![], vec![r_prod.clone()]),
        );
        // Resolution → Value (resolution reads auto param a).
        traces.insert(NodeId::Resolution(s0.clone()), trace(vec![a.clone()], vec![]));

        let graph = EvaluationGraph::default();
        let result = run_unified_pass(&graph, &traces);

        // (a) valid topological order over all edge kinds.
        assert_topo_valid(&result.schedule, &traces);
        // realization_reads participates: producer(idx1) before consumer(idx0).
        let pos = positions(&result.schedule);
        assert!(
            pos[&NodeId::Realization(r_prod.clone())] < pos[&NodeId::Realization(r_cons.clone())],
            "producer realization must precede consumer despite lower DebugOrd; schedule={:?}",
            result.schedule
        );

        // (b) schedule covers EXACTLY the trace-map keys (no Compute nodes here).
        let scheduled: HashSet<NodeId> = result.schedule.iter().cloned().collect();
        let keys: HashSet<NodeId> = traces.keys().cloned().collect();
        assert_eq!(
            scheduled, keys,
            "schedule must cover exactly the trace-map keys"
        );
        assert_eq!(
            result.schedule.len(),
            traces.len(),
            "no node scheduled twice"
        );

        // (c) residue empty.
        assert!(
            result.residue.is_empty(),
            "acyclic graph must leave empty residue, got {:?}",
            result.residue
        );
        // (d) zero diagnostics.
        assert!(
            result.diagnostics.is_empty(),
            "acyclic graph must emit zero diagnostics, got {}",
            result.diagnostics.len()
        );
    }
}
