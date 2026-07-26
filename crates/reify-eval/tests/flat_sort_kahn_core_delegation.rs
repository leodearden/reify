//! Task 5045 (eval cell-commit ν): `dirty.rs`'s flat sort IS the one Kahn core.
//!
//! `dirty::topological_sort` used to run its own level-batched Kahn pass
//! (`dirty::compute_levels`), a second scheduling implementation that diverged
//! from `engine_fixpoint::run_unified_pass_seeded` — the single scheduling core
//! shared by the cold/build/edit paths — along two independent axes:
//!
//! 1. **Drain shape.** The level sort emitted *every* in-degree-0 node as one
//!    batch per round; the core pops ONE node at a time off a
//!    `BTreeSet<DebugOrd>` ready set, so a deep chain drains completely before a
//!    `DebugOrd`-larger shallow sibling.
//! 2. **Edge kinds.** The level sort counted in-degree from `trace.reads` only,
//!    and its decrement loop fired only for `NodeId::Value` level members — so a
//!    `realization_reads` consumer could be emitted BEFORE its `Realization`
//!    producer. The core counts and decrements both edge kinds uniformly.
//!
//! These tests pin the delegation STRUCTURALLY: wherever practical they assert
//! against `run_unified_pass_seeded`'s own output rather than a hardcoded
//! literal, so the pin can never drift if `DebugOrd` or the core's tie-breaking
//! changes. A literal-order pin would have to be edited in lockstep with the
//! core; an equality-to-the-core pin cannot rot.

use std::collections::{HashMap, HashSet};

use reify_core::{RealizationNodeId, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::demand::DemandRegistry;
use reify_eval::deps::DependencyTrace;
use reify_eval::dirty::{compute_eval_set, topological_sort};
use reify_eval::engine_fixpoint::run_unified_pass_seeded;

/// `NodeId::Value(S.<member>)`.
fn v(member: &str) -> NodeId {
    NodeId::Value(ValueCellId::new("S", member))
}

/// `NodeId::Realization(S#realization[<index>])`.
fn r(index: u32) -> NodeId {
    NodeId::Realization(RealizationNodeId::new("S", index))
}

/// A trace reading the given `S.<member>` value cells and nothing else.
fn reads(members: &[&str]) -> DependencyTrace {
    DependencyTrace {
        reads: members.iter().map(|m| ValueCellId::new("S", *m)).collect(),
        realization_reads: Vec::new(),
    }
}

/// A trace reading the given `S#realization[i]` producers and no value cells.
fn realization_reads(indices: &[u32]) -> DependencyTrace {
    DependencyTrace {
        reads: Vec::new(),
        realization_reads: indices
            .iter()
            .map(|i| RealizationNodeId::new("S", *i))
            .collect(),
    }
}

/// The chain-plus-shallow-sibling shape on which the two sorts provably diverge:
/// `a = ∅`, `b = reads[a]`, `c = reads[b]`, `z = ∅`.
///
/// Level order is `[a, z, b, c]` (`a` and `z` are both in-degree 0, so they
/// share level 0); the core's order is `[a, b, c, z]` (`a` pops, releasing `b`,
/// which sorts before `z` under `DebugOrd`, and so on down the chain). The same
/// divergence is documented in-tree at `tests/unified_dag_edit_path.rs`.
fn chain_and_sibling_fixture() -> (HashSet<NodeId>, HashMap<NodeId, DependencyTrace>) {
    let nodes: HashSet<NodeId> = [v("a"), v("b"), v("c"), v("z")].into_iter().collect();
    let traces: HashMap<NodeId, DependencyTrace> = [
        (v("a"), reads(&[])),
        (v("b"), reads(&["a"])),
        (v("c"), reads(&["b"])),
        (v("z"), reads(&[])),
    ]
    .into_iter()
    .collect();
    (nodes, traces)
}

/// Index of `node` in `order`, or a panic naming the whole order.
fn position(order: &[NodeId], node: &NodeId) -> usize {
    order
        .iter()
        .position(|n| n == node)
        .unwrap_or_else(|| panic!("{node:?} missing from schedule {order:?}"))
}

#[test]
fn flat_topological_sort_is_the_kahn_core() {
    let (nodes, traces) = chain_and_sibling_fixture();

    let flat = topological_sort(&nodes, &traces);
    let core = run_unified_pass_seeded(&traces, &nodes);

    assert_eq!(
        flat, core,
        "dirty::topological_sort must BE engine_fixpoint::run_unified_pass_seeded, \
         not a second Kahn implementation that happens to agree on easy shapes. \
         flat sort returned {flat:?}, the core returned {core:?} — this fixture \
         (chain a→b→c plus shallow sibling z) is exactly where a level-batched \
         drain diverges from the core's single-pop priority drain."
    );

    // Guard against a vacuous pass: the fixture must actually exercise all four
    // nodes, and the chain must be ordered (a valid linear extension) either way.
    assert_eq!(flat.len(), 4, "all four fixture nodes must be scheduled");
    assert!(position(&flat, &v("a")) < position(&flat, &v("b")));
    assert!(position(&flat, &v("b")) < position(&flat, &v("c")));
}

#[test]
fn flat_sort_orders_realization_producer_before_consumer() {
    // a = ∅ ; b = reads[a] ; Realization(S,0) = reads[b] ; w = realization_reads[R0]
    //
    // The chain depth on the producer side is LOAD-BEARING. A flat one-level
    // fixture would pass vacuously: `Debug` renders the variants as
    // "Realization(..." and "Value(...", and "R" < "V", so `DebugOrd` already
    // happens to place a Realization ahead of a Value inside a single batch.
    // Burying the producer two `reads` edges deep is what makes the missing
    // realization edge observable in the emitted order.
    let nodes: HashSet<NodeId> = [v("a"), v("b"), r(0), v("w")].into_iter().collect();
    let traces: HashMap<NodeId, DependencyTrace> = [
        (v("a"), reads(&[])),
        (v("b"), reads(&["a"])),
        (r(0), reads(&["b"])),
        (v("w"), realization_reads(&[0])),
    ]
    .into_iter()
    .collect();

    let flat = topological_sort(&nodes, &traces);

    assert_eq!(flat.len(), 4, "all four fixture nodes must be scheduled");
    let producer = position(&flat, &r(0));
    let consumer = position(&flat, &v("w"));
    assert!(
        producer < consumer,
        "the realization PRODUCER Realization(S,0) must be scheduled before its \
         realization_reads CONSUMER Value(S.w), but got producer at index \
         {producer} and consumer at index {consumer} in {flat:?}. A reads-only \
         in-degree count ignores `realization_reads` entirely, emitting the \
         consumer against a not-yet-produced handle."
    );

    let core = run_unified_pass_seeded(&traces, &nodes);
    assert_eq!(
        flat, core,
        "the flat sort must be realization-aware BECAUSE it delegates to the \
         core, not because it grew a parallel realization-edge pass: \
         flat {flat:?} vs core {core:?}"
    );
}

#[test]
fn compute_eval_set_is_the_kahn_core() {
    let (nodes, traces) = chain_and_sibling_fixture();

    // Every fixture node is both dirty and demanded, so the dirty ∩ demand
    // intersection `compute_eval_set` sorts is exactly `nodes`. `full_scope` is
    // the cold-path override that makes `is_demanded` total without needing a
    // whole `EvaluationGraph` to `rebuild_cone` against; `add_demand` records
    // the same four nodes as roots so the registry is not merely degenerate.
    let dirty: HashSet<NodeId> = nodes.clone();
    let mut demand = DemandRegistry::new();
    for node in &nodes {
        demand.add_demand(node.clone());
    }
    demand.set_full_scope(true);

    let eval_set = compute_eval_set(&dirty, &demand, &traces);
    let core = run_unified_pass_seeded(&traces, &nodes);

    assert_eq!(
        eval_set, core,
        "compute_eval_set must inherit the core's order transitively through \
         topological_sort — the demand-intersected entry point is what the edit \
         path actually calls. eval_set {eval_set:?} vs core {core:?}"
    );
    assert_eq!(
        eval_set.len(),
        4,
        "all four dirty ∩ demanded nodes must be scheduled"
    );
}
