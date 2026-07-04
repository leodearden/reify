//! Task α (PRD `docs/prds/v0_6/eval-uniform-dependency-handling.md` §6.1):
//! the no-stale-Undef invariant checker.
//!
//! `check_no_stale_undef` is a pure function over retained post-eval state
//! (graph + values + trace_map + functions) that reports every value cell
//! which is currently `Value::Undef` despite having a `default_expr` and
//! every static dependency resolved — the "causeless staleness" §6.1 exists
//! to catch. A thin `Engine::check_no_stale_undef` wrapper (added in a later
//! step) threads the Engine's own retained `eval_state()` into this free
//! function for the debug-gate corpus harness.
//!
//! Lives in-crate (not in an integration test) because `Engine.functions` is
//! a private field — the future `@optimized` exclusion (§6.1 clause 5) needs
//! in-crate access to thread it through.

use std::collections::HashMap;

use reify_core::{RealizationNodeId, ValueCellId};
use reify_ir::{CompiledFunction, DeterminacyState, PersistentMap, Value};

use crate::cache::NodeId;
use crate::deps::DependencyTrace;
use crate::graph::EvaluationGraph;

/// A single no-stale-Undef violation: `cell` is `Value::Undef` even though
/// every one of its static dependencies is resolved (§6.1 clause 4) and no
/// exclusion (auto, `@optimized`, guard-inactive, undef-literal) applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleUndefViolation {
    pub cell: ValueCellId,
    pub detail: String,
}

/// §6.1: report every value cell that is a stale-Undef violation.
///
/// A violation is a value cell `c` such that ALL of:
/// 1. `c` is not an auto cell;
/// 2. `c` has a `default_expr`;
/// 3. `values[c]` is `Value::Undef` (or absent from `values` entirely);
/// 4. every static dep in `trace(c).reads ∪ trace(c).realization_reads` is
///    resolved (present and non-Undef) — a missing-producer read, or a read
///    that is itself Undef, makes `c` EXEMPT, not violating.
///
/// `functions` is threaded through for the (not-yet-implemented) §6.1
/// clause-5 `@optimized` exclusion.
pub fn check_no_stale_undef(
    graph: &EvaluationGraph,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    trace_map: &HashMap<NodeId, DependencyTrace>,
    _functions: &[CompiledFunction],
) -> Vec<StaleUndefViolation> {
    let empty_trace = DependencyTrace::default();
    let mut violations = Vec::new();

    for (id, cell) in graph.value_cells.iter() {
        // Clause 1: auto cells are solver-owned; not candidates.
        if cell.kind.is_auto() {
            continue;
        }
        // Clause 2: no default_expr => nothing to have gone stale.
        if cell.default_expr.is_none() {
            continue;
        }
        // Clause 3: only a currently-Undef cell can be stale.
        if !matches!(cell_value_or_undef(values, id), Value::Undef) {
            continue;
        }

        // Clause 4: EXEMPT (not a violation) if any static dep is missing
        // or itself unresolved.
        let trace = trace_map
            .get(&NodeId::Value(id.clone()))
            .unwrap_or(&empty_trace);
        let reads_resolved = trace
            .reads
            .iter()
            .all(|dep| value_cell_dep_is_resolved(graph, values, dep));
        let realization_reads_resolved = trace
            .realization_reads
            .iter()
            .all(|rid| realization_dep_is_resolved(graph, values, rid));
        if !reads_resolved || !realization_reads_resolved {
            continue;
        }

        violations.push(StaleUndefViolation {
            cell: id.clone(),
            detail: format!(
                "value cell {id:?} is Undef but all {} static dependency(ies) \
                 (reads + realization_reads) are resolved",
                trace.reads.len() + trace.realization_reads.len()
            ),
        });
    }

    violations
}

/// Look up `id`'s stored value in the retained post-eval values map,
/// treating an absent entry the same as `Value::Undef` (§6.1 clause 3
/// explicitly includes "or absent"). Mirrors `ValueMap::get_or_undef`
/// (`reify_ir::value::ValueMap`) for this crate's
/// `PersistentMap<ValueCellId, (Value, DeterminacyState)>` snapshot shape,
/// which — being a bare tuple map, not a `ValueMap` — has no such method of
/// its own.
fn cell_value_or_undef(
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    id: &ValueCellId,
) -> Value {
    values
        .get(id)
        .map(|(value, _determinacy)| value.clone())
        .unwrap_or(Value::Undef)
}

/// A `trace.reads` dependency is resolved iff its producer is present in
/// the graph AND its stored value is non-Undef. A missing producer or an
/// Undef producer both make the exemption fire (erring EXEMPT, never
/// falsely violating) — §6.1 clause 4.
fn value_cell_dep_is_resolved(
    graph: &EvaluationGraph,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    dep: &ValueCellId,
) -> bool {
    if !graph.value_cells.contains_key(dep) {
        return false; // missing-producer
    }
    !matches!(cell_value_or_undef(values, dep), Value::Undef)
}

/// A `trace.realization_reads` dependency is resolved iff the realization is
/// present in the graph AND it names a backing geometry value cell that is
/// itself present and non-Undef. A realization with no backing
/// `geometry_cell` link cannot be checked for Undef-ness at all — treated as
/// unresolved (erring EXEMPT on ambiguity, per the same clause-4 policy).
fn realization_dep_is_resolved(
    graph: &EvaluationGraph,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    rid: &RealizationNodeId,
) -> bool {
    let Some(rnode) = graph.realizations.get(rid) else {
        return false; // missing-producer
    };
    match &rnode.geometry_cell {
        Some(cell) => value_cell_dep_is_resolved(graph, values, cell),
        None => false,
    }
}
