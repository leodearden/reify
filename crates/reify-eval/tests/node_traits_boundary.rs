//! Boundary tests for [`NodeTraitsMap<NodeId>`] — PRD §5 B1 / §9 T4 (precedence chain).
//!
//! This file is the PRD §9 reserved location for the full T1–T7 boundary test grid.
//! Task β seeds it with the NodeTraitsMap<NodeId> cases that can be tested without
//! any additional scheduler wiring. Later tasks (γ/δ/ζ/η/θ) will append T1–T7 here.
//!
//! All tests use real `reify_eval::cache::NodeId` values so that the
//! `impl HasNodeKind for NodeId` bridge in `reify-eval/src/cache.rs` is exercised
//! against the production type rather than a test stub.

use reify_eval::cache::NodeId;
use reify_types::{
    ComputeNodeId, ConstraintNodeId, NodeKind, NodeTraits, NodeTraitsMap, RealizationNodeId,
    ResolutionNodeId, ValueCellId,
};

// ── helpers ─────────────────────────────────────────────────────────────────

fn value_node() -> NodeId {
    NodeId::Value(ValueCellId::new("E", "x"))
}

fn constraint_node() -> NodeId {
    NodeId::Constraint(ConstraintNodeId::new("E", 0))
}

fn realization_node() -> NodeId {
    NodeId::Realization(RealizationNodeId::new("E", 0))
}

fn resolution_node() -> NodeId {
    NodeId::Resolution(ResolutionNodeId::new("E", 0))
}

fn compute_node(idx: u32) -> NodeId {
    NodeId::Compute(ComputeNodeId::new("E", idx))
}

// ── default-fallback tests ───────────────────────────────────────────────────

#[test]
fn node_traits_map_with_node_id_resolves_value_default() {
    let m = NodeTraitsMap::<NodeId>::default();
    assert_eq!(m.resolve(&value_node()), NodeTraits::IMMEDIATE);
}

#[test]
fn node_traits_map_with_node_id_resolves_constraint_default() {
    let m = NodeTraitsMap::<NodeId>::default();
    assert_eq!(m.resolve(&constraint_node()), NodeTraits::empty());
}

#[test]
fn node_traits_map_with_node_id_resolves_realization_default() {
    let m = NodeTraitsMap::<NodeId>::default();
    assert_eq!(
        m.resolve(&realization_node()),
        NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE)
    );
}

#[test]
fn node_traits_map_with_node_id_resolves_resolution_default() {
    let m = NodeTraitsMap::<NodeId>::default();
    assert_eq!(
        m.resolve(&resolution_node()),
        NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE)
    );
}

#[test]
fn node_traits_map_with_node_id_resolves_compute_default() {
    let m = NodeTraitsMap::<NodeId>::default();
    assert_eq!(
        m.resolve(&compute_node(0)),
        NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE)
    );
}

// ── T4 (lite): instance > kind precedence with real NodeId ───────────────────

#[test]
fn node_traits_map_with_node_id_instance_wins_over_kind() {
    let mut m = NodeTraitsMap::<NodeId>::default();
    // Set a kind-level override for all Compute nodes
    m.set_type(NodeKind::Compute, NodeTraits::PROGRESSIVE);
    // Set an instance-level override for one specific compute node
    let specific = compute_node(42);
    m.set_instance(specific.clone(), NodeTraits::IMMEDIATE);

    // Instance wins for the specific node
    assert_eq!(m.resolve(&specific), NodeTraits::IMMEDIATE);
    // Kind-level applies to other compute nodes
    assert_eq!(m.resolve(&compute_node(99)), NodeTraits::PROGRESSIVE);
    // Value default is unaffected
    assert_eq!(m.resolve(&value_node()), NodeTraits::IMMEDIATE);
}
