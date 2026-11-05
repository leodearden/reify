//! [`NodeId`], the key of every cache entry, and its rendered grammar:
//! [`Display`](fmt::Display) and the [`FromStr`] that inverts it.

use std::fmt;
use std::str::FromStr;

use reify_core::identity::{IndexedNodeIdParseError, IndexedNodeKind, ValueCellIdParseError};
use reify_core::{
    ComputeNodeId, ConstraintNodeId, RealizationNodeId, ResolutionNodeId, ValueCellId,
};

/// Unified identifier for any node in the evaluation graph.
/// Used as the key in the cache store.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeId {
    Value(ValueCellId),
    Constraint(ConstraintNodeId),
    Realization(RealizationNodeId),
    Resolution(ResolutionNodeId),
    /// P3.3: a ComputeNode (e.g. an @optimized FEA/solver computation).
    /// Added so the reverse-dependency index can register VC→Compute and
    /// Realization→Compute edges as `Set<NodeId>` dependents, and so the
    /// dirty-cone / freshness walks can propagate through ComputeNodes.
    Compute(ComputeNodeId),
}

impl NodeId {
    /// Human-readable, kind-naming description of this node (PRD §3.4).
    ///
    /// Produces `"<kind> <id-Display>"` for each variant, naming every kind
    /// distinctly:
    /// - `NodeId::Value`       → `"value cell Bracket.width"`
    /// - `NodeId::Constraint`  → `"constraint Bracket#constraint[0]"`
    /// - `NodeId::Realization` → `"realization E#realization[0]"`
    /// - `NodeId::Resolution`  → `"resolution A#resolution[0]"`
    /// - `NodeId::Compute`     → `"compute A#computation[0]"`
    ///
    /// Used by [`crate::engine_fixpoint::run_unified_pass`] to name graph
    /// members in `E_EVAL_CYCLE` / `E_EVAL_UNRESOLVED` diagnostic messages,
    /// mirroring the ordered-members `[a, b, c]` shape of the legacy
    /// `detect_let_cycle` diagnostic.
    pub fn describe(&self) -> String {
        match self {
            NodeId::Value(v) => format!("value cell {v}"),
            NodeId::Constraint(c) => format!("constraint {c}"),
            NodeId::Realization(r) => format!("realization {r}"),
            NodeId::Resolution(s) => format!("resolution {s}"),
            NodeId::Compute(c) => format!("compute {c}"),
        }
    }
}

impl From<ValueCellId> for NodeId {
    fn from(id: ValueCellId) -> Self {
        NodeId::Value(id)
    }
}

impl From<ConstraintNodeId> for NodeId {
    fn from(id: ConstraintNodeId) -> Self {
        NodeId::Constraint(id)
    }
}

impl From<RealizationNodeId> for NodeId {
    fn from(id: RealizationNodeId) -> Self {
        NodeId::Realization(id)
    }
}

impl From<ResolutionNodeId> for NodeId {
    fn from(id: ResolutionNodeId) -> Self {
        NodeId::Resolution(id)
    }
}

impl From<ComputeNodeId> for NodeId {
    fn from(id: ComputeNodeId) -> Self {
        NodeId::Compute(id)
    }
}

/// Bridge from [`NodeId`] to the canonical [`reify_types::NodeKind`] discriminant.
///
/// This impl lives in `reify-eval` (not `reify-runtime` or `reify-types`) because
/// it is the unique orphan-rule-clean host: `NodeId` is local to this crate, and
/// RFC 2451 permits `impl From<&LocalType> for ForeignType` when the local type
/// appears in the `From` argument. See `docs/prds/v0_3/node-traits-unification.md §4`.
impl From<&NodeId> for reify_ir::NodeKind {
    fn from(node_id: &NodeId) -> Self {
        match node_id {
            NodeId::Value(_) => Self::Value,
            NodeId::Constraint(_) => Self::Constraint,
            NodeId::Realization(_) => Self::Realization,
            NodeId::Resolution(_) => Self::Resolution,
            NodeId::Compute(_) => Self::Compute,
        }
    }
}

/// Project a [`NodeId`] to its [`reify_types::NodeKind`] discriminant via the
/// [`reify_types::HasNodeKind`] trait.
///
/// Sibling bridge to the `From<&NodeId> for NodeKind` impl above; both live in
/// `reify-eval` for the same orphan-rule reason (`NodeId` is local to this crate,
/// the destination trait/type is foreign). The body delegates to that existing `From`
/// impl to avoid duplicating match arms. See PRD §5 B1.
impl reify_ir::HasNodeKind for NodeId {
    fn node_kind(&self) -> reify_ir::NodeKind {
        reify_ir::NodeKind::from(self)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeId::Value(v) => v.fmt(f),
            NodeId::Constraint(c) => c.fmt(f),
            NodeId::Realization(r) => r.fmt(f),
            NodeId::Resolution(s) => s.fmt(f),
            NodeId::Compute(c) => c.fmt(f),
        }
    }
}

/// Error returned by [`NodeId`]'s [`FromStr`] impl: the typed error of the
/// grammar that judged the input — the indexed kind's when the input names
/// that kind's marker, otherwise the value-cell grammar's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeIdParseError {
    Value(ValueCellIdParseError),
    Indexed(IndexedNodeIdParseError),
}

impl fmt::Display for NodeIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeIdParseError::Value(e) => e.fmt(f),
            NodeIdParseError::Indexed(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for NodeIdParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NodeIdParseError::Value(e) => Some(e),
            NodeIdParseError::Indexed(e) => Some(e),
        }
    }
}

impl From<ValueCellIdParseError> for NodeIdParseError {
    fn from(e: ValueCellIdParseError) -> Self {
        NodeIdParseError::Value(e)
    }
}

impl From<IndexedNodeIdParseError> for NodeIdParseError {
    fn from(e: IndexedNodeIdParseError) -> Self {
        NodeIdParseError::Indexed(e)
    }
}

impl FromStr for NodeId {
    type Err = NodeIdParseError;

    /// The exact inverse of [`fmt::Display`] on every id it admits.
    ///
    /// An id the [`IndexedNodeKind`] grammar admits, for the kind its last
    /// `'#'` segment names, is that indexed node. This reading must win:
    /// an indexed entity may contain `'.'`, so its rendering is also a
    /// single-dot value-cell id. Every other id is read as a value cell,
    /// keeping [`ValueCellId`]'s refusal of ambiguous multi-dot ids — this
    /// includes a keyed member that merely mentions a marker, such as
    /// `Widget.vents["k#computation[0]"]`. When an id names a marker but
    /// neither reading admits it, the indexed kind's error is reported.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some(kind) = IndexedNodeKind::of_rendered_id(s) else {
            return Ok(NodeId::Value(s.parse()?));
        };
        parse_indexed(kind, s).or_else(|indexed_error| {
            s.parse()
                .map(NodeId::Value)
                .map_err(|_| NodeIdParseError::Indexed(indexed_error))
        })
    }
}

fn parse_indexed(kind: IndexedNodeKind, s: &str) -> Result<NodeId, IndexedNodeIdParseError> {
    match kind {
        IndexedNodeKind::Constraint => s.parse().map(NodeId::Constraint),
        IndexedNodeKind::Realization => s.parse().map(NodeId::Realization),
        IndexedNodeKind::Resolution => s.parse().map(NodeId::Resolution),
        IndexedNodeKind::Compute => s.parse().map(NodeId::Compute),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use reify_core::identity::IndexedNodeIdFault;

    use super::*;

    /// Task 4357 δ (step-1): `NodeId::describe()` must return a distinct,
    /// human-readable, kind-naming string for each of the five variants —
    /// each containing its kind word AND the underlying id's Display. The
    /// five outputs must be pairwise distinct. Used by `run_unified_pass`'s
    /// `E_EVAL_CYCLE`/`E_EVAL_UNRESOLVED` messages to name graph members.
    ///
    /// RED until step-2 adds `NodeId::describe()`.
    #[test]
    fn node_id_describe_names_every_kind_distinctly() {
        use reify_core::{ComputeNodeId, ResolutionNodeId};

        let value = NodeId::Value(ValueCellId::new("A", "x"));
        let constraint = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        let realization = NodeId::Realization(RealizationNodeId::new("A", 0));
        let resolution = NodeId::Resolution(ResolutionNodeId::new("A", 0));
        let compute = NodeId::Compute(ComputeNodeId::new("A", 0));

        let dv = value.describe();
        let dc = constraint.describe();
        let dr = realization.describe();
        let ds = resolution.describe();
        let dk = compute.describe();

        // Each description names its kind word.
        assert!(dv.contains("value"), "value describe must name kind: {dv}");
        assert!(
            dc.contains("constraint"),
            "constraint describe must name kind: {dc}"
        );
        assert!(
            dr.contains("realization"),
            "realization describe must name kind: {dr}"
        );
        assert!(
            ds.contains("resolution"),
            "resolution describe must name kind: {ds}"
        );
        assert!(
            dk.contains("compute"),
            "compute describe must name kind: {dk}"
        );

        // Each description embeds the underlying id's Display.
        assert!(
            dv.contains(&ValueCellId::new("A", "x").to_string()),
            "value describe must embed id Display: {dv}"
        );
        assert!(
            dc.contains(&ConstraintNodeId::new("A", 0).to_string()),
            "constraint describe must embed id Display: {dc}"
        );
        assert!(
            dr.contains(&RealizationNodeId::new("A", 0).to_string()),
            "realization describe must embed id Display: {dr}"
        );
        assert!(
            ds.contains(&ResolutionNodeId::new("A", 0).to_string()),
            "resolution describe must embed id Display: {ds}"
        );
        assert!(
            dk.contains(&ComputeNodeId::new("A", 0).to_string()),
            "compute describe must embed id Display: {dk}"
        );

        // Pairwise distinct across all five variants.
        let all = [&dv, &dc, &dr, &ds, &dk];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(
                    all[i], all[j],
                    "describe outputs must be pairwise distinct: {} vs {}",
                    all[i], all[j]
                );
            }
        }
    }

    /// Exhaustive (no `_` arm), so a sixth `NodeId` variant fails to compile
    /// here until the round-trip table below covers it.
    fn node_id_variant_index(id: &NodeId) -> usize {
        match id {
            NodeId::Value(_) => 0,
            NodeId::Constraint(_) => 1,
            NodeId::Realization(_) => 2,
            NodeId::Resolution(_) => 3,
            NodeId::Compute(_) => 4,
        }
    }

    #[test]
    fn node_id_from_str_inverts_display_for_every_kind() {
        let ids = [
            NodeId::Value(ValueCellId::new("Bracket", "width")),
            NodeId::Value(ValueCellId::new("__field", "gravity")),
            NodeId::Value(ValueCellId::new("Widget", r#"vents["intake"]"#)),
            NodeId::Value(ValueCellId::new("Widget", r#"vents["k#computation[0]"]"#)),
            NodeId::Constraint(ConstraintNodeId::new("Rig.bolts", 2)),
            NodeId::Constraint(ConstraintNodeId::new("forall_connect:v@Rig.bolts#3", 0)),
            NodeId::Realization(RealizationNodeId::new("box-2658-smoke", 1)),
            NodeId::Resolution(ResolutionNodeId::new("A", 0)),
            NodeId::Compute(ComputeNodeId::new("Bracket", u32::MAX)),
        ];
        let kinds: BTreeSet<usize> = ids.iter().map(node_id_variant_index).collect();
        assert_eq!(kinds, (0..5).collect(), "the table must cover every kind");

        for id in ids {
            assert_eq!(
                id.to_string().parse::<NodeId>(),
                Ok(id.clone()),
                "{id} must round-trip"
            );
        }
    }

    #[test]
    fn node_id_from_str_prefers_the_indexed_reading() {
        let rendered = "Rig.bolts#constraint[2]";
        assert!(
            rendered.parse::<ValueCellId>().is_ok(),
            "the single-dot value-cell reading (Rig, bolts#constraint[2]) exists, \
             so precedence decides which node this string names"
        );
        assert_eq!(
            rendered.parse::<NodeId>(),
            Ok(NodeId::Constraint(ConstraintNodeId::new("Rig.bolts", 2)))
        );
    }

    #[test]
    fn node_id_from_str_reads_an_id_the_indexed_grammar_refuses_as_a_value_cell() {
        for (input, entity, member) in [
            (
                r#"Widget.vents["k#computation[0]"]"#,
                "Widget",
                r#"vents["k#computation[0]"]"#,
            ),
            ("Rig.bolts#resolution[", "Rig", "bolts#resolution["),
        ] {
            assert!(
                IndexedNodeKind::of_rendered_id(input).is_some(),
                "{input:?} must name a marker for this case to test the fallback"
            );
            assert_eq!(
                input.parse::<NodeId>(),
                Ok(NodeId::Value(ValueCellId::new(entity, member))),
                "{input:?} has no indexed reading, so it names the value cell"
            );
        }
    }

    #[test]
    fn node_id_from_str_routes_typed_errors_by_kind() {
        for (input, kind, fault) in [
            (
                "Foo#computation[x]",
                IndexedNodeKind::Compute,
                IndexedNodeIdFault::InvalidIndex,
            ),
            (
                "Foo#constraint",
                IndexedNodeKind::Constraint,
                IndexedNodeIdFault::MissingMarker,
            ),
            (
                "Rig#resolution[",
                IndexedNodeKind::Resolution,
                IndexedNodeIdFault::MissingTerminator,
            ),
            (
                "A.b.c#realization[0",
                IndexedNodeKind::Realization,
                IndexedNodeIdFault::MissingTerminator,
            ),
        ] {
            assert_eq!(
                input.parse::<NodeId>(),
                Err(NodeIdParseError::Indexed(IndexedNodeIdParseError {
                    kind,
                    fault
                })),
                "{input:?} names a marker and neither reading admits it, so the \
                 indexed kind's error is reported"
            );
        }
        for (input, fault) in [
            ("Compute(foo)", ValueCellIdParseError::MissingSeparator),
            ("Foo#bogus[0]", ValueCellIdParseError::MissingSeparator),
            ("A.b.c", ValueCellIdParseError::Ambiguous),
        ] {
            assert_eq!(
                input.parse::<NodeId>(),
                Err(NodeIdParseError::Value(fault)),
                "{input:?} names no indexed marker, so the value-cell grammar judges it"
            );
        }
    }

    #[test]
    fn node_id_from_value_cell_id() {
        let vcid = ValueCellId::new("Bracket", "width");
        let node: NodeId = NodeId::from(vcid.clone());
        assert_eq!(node, NodeId::Value(vcid));
    }

    #[test]
    fn node_id_from_constraint_node_id() {
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let node: NodeId = NodeId::from(cnid.clone());
        assert_eq!(node, NodeId::Constraint(cnid));
    }

    #[test]
    fn node_id_from_realization_node_id() {
        let rnid = RealizationNodeId::new("Bracket", 0);
        let node: NodeId = NodeId::from(rnid.clone());
        assert_eq!(node, NodeId::Realization(rnid));
    }

    #[test]
    fn node_id_resolution_variant() {
        use reify_core::ResolutionNodeId;

        let res_id = ResolutionNodeId::new("A", 0);
        let res_node = NodeId::Resolution(res_id.clone());

        // Equality with itself
        assert_eq!(res_node, NodeId::Resolution(ResolutionNodeId::new("A", 0)));

        // Differs from other variants
        assert_ne!(res_node, NodeId::Value(ValueCellId::new("A", "x")));
        assert_ne!(res_node, NodeId::Constraint(ConstraintNodeId::new("A", 0)));
        assert_ne!(
            res_node,
            NodeId::Realization(RealizationNodeId::new("A", 0))
        );

        // From<ResolutionNodeId> conversion
        let from_node: NodeId = NodeId::from(res_id.clone());
        assert_eq!(from_node, res_node);
    }

    /// P3.3 step-1: Pin the new `NodeId::Compute(ComputeNodeId)` variant.
    ///
    /// Mirrors `node_id_resolution_variant`. Asserts:
    ///   (a) construction + equality/hash round-trip via `HashMap` key
    ///   (b) `From<ComputeNodeId> for NodeId` produces `NodeId::Compute(_)`
    ///   (c) the variant differs from every existing variant even with
    ///       overlapping entity strings
    #[test]
    fn node_id_compute_variant() {
        use reify_core::{ComputeNodeId, ResolutionNodeId};

        let cn_id = ComputeNodeId::new("E", 0);
        let cn_node = NodeId::Compute(cn_id.clone());

        // (a) Equality with itself
        assert_eq!(cn_node, NodeId::Compute(ComputeNodeId::new("E", 0)));

        // (a) Hash round-trip through HashMap (mirrors node_id_hash_as_map_key)
        let mut map = HashMap::new();
        map.insert(cn_node.clone(), "compute");
        assert_eq!(map.get(&NodeId::Compute(cn_id.clone())), Some(&"compute"));

        // (c) Differs from other variants
        assert_ne!(cn_node, NodeId::Value(ValueCellId::new("E", "x")));
        assert_ne!(cn_node, NodeId::Constraint(ConstraintNodeId::new("E", 0)));
        assert_ne!(cn_node, NodeId::Realization(RealizationNodeId::new("E", 0)));
        assert_ne!(cn_node, NodeId::Resolution(ResolutionNodeId::new("E", 0)));

        // (b) From<ComputeNodeId> conversion
        let from_node: NodeId = NodeId::from(cn_id.clone());
        assert_eq!(from_node, cn_node);
    }

    /// P3.3 step-1: Pin `Display for NodeId::Compute(id)` forwarding to
    /// `id.fmt(f)`. The contract this test enforces is delegation: the
    /// wrapper's Display impl must produce the same bytes as the inner
    /// `ComputeNodeId`'s impl. We deliberately do NOT pin the literal
    /// format produced by `ComputeNodeId::fmt` here — that lives in
    /// reify_types and may be retuned independently of NodeId's
    /// pass-through contract.
    #[test]
    fn node_id_display_compute_forwards_to_inner_variant() {
        use reify_core::ComputeNodeId;

        let inner = ComputeNodeId::new("E", 0);
        let node = NodeId::Compute(inner.clone());
        assert_eq!(format!("{}", node), format!("{}", inner));
    }

    #[test]
    fn node_id_variants_not_equal_even_with_overlapping_strings() {
        let vcid = ValueCellId::new("Bracket", "width");
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let rnid = RealizationNodeId::new("Bracket", 0);

        let v = NodeId::Value(vcid);
        let c = NodeId::Constraint(cnid);
        let r = NodeId::Realization(rnid);

        assert_ne!(v, c);
        assert_ne!(v, r);
        assert_ne!(c, r);
    }

    #[test]
    fn node_id_clone_and_debug() {
        let vcid = ValueCellId::new("Bracket", "width");
        let node = NodeId::Value(vcid);
        let cloned = node.clone();
        assert_eq!(node, cloned);

        let debug = format!("{:?}", node);
        assert!(debug.contains("Value"));
    }

    #[test]
    fn node_id_hash_as_map_key() {
        use std::collections::HashMap;
        let vcid = ValueCellId::new("Bracket", "width");
        let cnid = ConstraintNodeId::new("Bracket", 0);

        let mut map = HashMap::new();
        map.insert(NodeId::Value(vcid.clone()), "value");
        map.insert(NodeId::Constraint(cnid.clone()), "constraint");

        assert_eq!(map.get(&NodeId::Value(vcid)), Some(&"value"));
        assert_eq!(map.get(&NodeId::Constraint(cnid)), Some(&"constraint"));
    }

    #[test]
    fn node_id_display_forwards_to_inner_variant() {
        use reify_core::ResolutionNodeId;

        // Value variant
        let inner_v = ValueCellId::new("Bracket", "x");
        let node_v = NodeId::Value(inner_v.clone());
        assert_eq!(format!("{}", node_v), format!("{}", inner_v));

        // Constraint variant
        let inner_c = ConstraintNodeId::new("Bracket", 0);
        let node_c = NodeId::Constraint(inner_c.clone());
        assert_eq!(format!("{}", node_c), format!("{}", inner_c));

        // Realization variant
        let inner_r = RealizationNodeId::new("Bracket", 1);
        let node_r = NodeId::Realization(inner_r.clone());
        assert_eq!(format!("{}", node_r), format!("{}", inner_r));

        // Resolution variant
        let inner_s = ResolutionNodeId::new("Bracket", 2);
        let node_s = NodeId::Resolution(inner_s.clone());
        assert_eq!(format!("{}", node_s), format!("{}", inner_s));
    }
}
