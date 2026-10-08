//! Which realizations an edit made stale, and which `RealizationCache`
//! families that obliges the edit to evict (selective-realization-eviction
//! PRD, `docs/prds/v0_6/selective-realization-eviction.md`).
//!
//! Pure functions over the evaluation graph: no engine state is read or
//! written here. `engine_edit` runs them at its two compare sites and applies
//! the result to the cache.

use std::collections::{HashMap, HashSet};

use reify_core::RealizationNodeId;
use reify_ir::PersistentMap;

use crate::cache::NodeId;
use crate::graph::{EvaluationGraph, RealizationNodeData};

/// Classify every realization in `new_graph` as CHANGED or UNCHANGED by
/// recomputing its INPUT-cone hash against the post-edit context and
/// comparing it to the prior node's stored `input_cone_hash`
/// (selective-realization-eviction PRD task β, #4729).
///
/// # One helper, two compare sites (PRD §11.3)
///
/// This is the single comparison helper shared by both edit entry points,
/// parameterised only by *where the prior hash lives*:
///
/// - `edit_param` does not rebuild the graph, so the persisting graph is
///   both `prior_realizations` and `new_graph`.
/// - `edit_source` does rebuild it, so `prior_realizations` is the OLD
///   graph's realization map and `new_graph` is the new one.
///
/// # Why NOT `RealizationNodeData::content_hash` (design §5.2)
///
/// [`diff_realizations`](crate::engine_edit::diff_realizations) keys on
/// `content_hash`, which
/// `EvaluationGraph::from_templates` builds (graph.rs:371-396) as
/// `of_str(id) ⊕ combine_all(of_str(format!("{:?}", op)))` — a `Debug`
/// render of the compiled op IR. A `Primitive{Box, args:[("width",
/// ValueRef(width))]}` renders identically no matter what `width`
/// *evaluates to*, so that hash provably never moves on a value-driven
/// change. Keying eviction on it would compile, run, and silently evict
/// nothing — the 4317-class trap design §5.2 warns about. This helper
/// therefore uses the GHR-β INPUT-cone fold instead: the same canonical
/// `compute_realization_upstream_values_hash_from_ops` (PRD D1 — never a
/// second fold) that α's stored hash, the value-cell early cutoff, and the
/// tag-28 in-memory geometry cache key all agree on.
///
/// The two are complementary, not alternatives: an ops-level source change
/// is a real change that the input-cone fold could in principle miss, so
/// `edit_source` UNIONs this result with `diff_realizations`' changed∪added
/// sets rather than replacing them.
///
/// # Read-only with respect to `input_cone_hash`
///
/// This helper never writes the stored hash. That field means "the input
/// cone **as of the last EXECUTION**" — owned by α's write inside
/// `execute_realization_ops` and re-stamped by the build-time gate
/// `refresh_and_gate_demanded_realizations`. Re-stamping it here, at EDIT
/// time, would make that gate observe `stored == current` for a realization
/// whose geometry is stale, mark it exempt from re-dispatch, and serve
/// stale geometry — a textbook 4317-class stale.
///
/// # Conservative direction
///
/// A missing prior hash — `None` (never executed, demand-pruned, or
/// un-hydrated) or no prior entry at all (newly added by a recompile) —
/// classifies as CHANGED (PRD §11.2). Over-eviction is merely wasted work;
/// under-eviction serves stale geometry, so every uncertain case rounds
/// towards CHANGED.
pub(crate) fn compute_changed_realizations(
    prior_realizations: &PersistentMap<RealizationNodeId, RealizationNodeData>,
    new_graph: &EvaluationGraph,
    ctx: &reify_expr::EvalContext<'_>,
) -> HashSet<RealizationNodeId> {
    new_graph
        .realizations
        .iter()
        .filter(|(rid, node)| {
            let current = crate::engine_build::compute_realization_upstream_values_hash_from_ops(
                &node.operations,
                ctx,
            );
            let prior = prior_realizations
                .get(*rid)
                .and_then(|prior_node| prior_node.input_cone_hash);
            // This single `!=` expresses all three §11.2 cases at once:
            // a `None` prior — never executed, demand-pruned, or newly
            // added — can never equal `Some(current)`, so it is CHANGED.
            prior != Some(current)
        })
        .map(|(rid, _)| rid.clone())
        .collect()
}

/// The entities whose `RealizationCache` family an edit made stale (γ #4730,
/// PRD D4) — what the edit must hand to `RealizationCache::evict_family`.
///
/// Four terms, each closing a distinct stale-geometry hole:
///
/// - `changed`, the seeds [`compute_changed_realizations`] reported. They are
///   unioned in explicitly because `dirty::compute_dirty_cone_with_realizations`
///   never puts its seeds in its own result.
/// - every `NodeId::Realization` in `realization_cone`, the transitive dirty
///   cone of those seeds. The input-cone fold sees only a realization's own op
///   args, so a `GeomRef::Sub` consumer of a moved body never becomes a seed
///   itself; only the cone reaches it.
/// - `removed`, realizations the edit dropped. The cache is keyed by entity
///   alone, so a dropped realization's cached terminal would otherwise stand
///   in for whatever realization of that entity remains.
/// - once any of those is non-empty, every [cross-entity `Sub`
///   consumer](cross_entity_sub_consumers) in `graph`, the post-edit graph.
///   The reverse index behind the cone resolves a `"<sub>.<member>"` operand
///   by member name alone and drops it when two entities export that member
///   (`A.body` and `B.body`), so the cone cannot be trusted to reach these
///   consumers. Evicting all of them over-evicts a consumer that does not
///   read the stale body; that is wasted work, never stale geometry.
///
/// Value, compute and constraint members of the cone name no family and are
/// ignored.
pub(crate) fn stale_realization_entities<'a>(
    changed: &HashSet<RealizationNodeId>,
    removed: impl IntoIterator<Item = &'a RealizationNodeId>,
    realization_cone: &HashSet<NodeId>,
    graph: &EvaluationGraph,
) -> HashSet<String> {
    let entity = |rid: &RealizationNodeId| rid.entity.clone();
    let reached = realization_cone.iter().filter_map(|node| match node {
        NodeId::Realization(rid) => Some(entity(rid)),
        _ => None,
    });
    let mut stale: HashSet<String> = changed
        .iter()
        .map(entity)
        .chain(removed.into_iter().map(entity))
        .chain(reached)
        .collect();
    if !stale.is_empty() {
        stale.extend(cross_entity_sub_consumers(graph));
    }
    stale
}

/// The entities owning a realization that reads ANOTHER entity's geometry: a
/// `GeomRef::Sub` operand that names no realization of its own entity.
///
/// A sibling operand names the geometry member of a same-entity realization
/// (`union(base, hole)`); it needs no term of its own, because the family is
/// keyed by entity and is evicted exactly when the sibling is.
fn cross_entity_sub_consumers(graph: &EvaluationGraph) -> HashSet<String> {
    let mut members_by_entity: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (rid, node) in graph.realizations.iter() {
        if let Some(cell) = &node.geometry_cell {
            members_by_entity
                .entry(rid.entity.as_str())
                .or_default()
                .insert(cell.member.as_str());
        }
    }
    graph
        .realizations
        .iter()
        .filter(|(rid, node)| {
            let siblings = members_by_entity.get(rid.entity.as_str());
            node.operations
                .iter()
                .flat_map(crate::engine_build::sub_refs_in_op)
                .any(|name| !siblings.is_some_and(|members| members.contains(name)))
        })
        .map(|(rid, _)| rid.entity.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use reify_core::{
        ComputeNodeId, ConstraintNodeId, ContentHash, RealizationNodeId, Type, ValueCellId,
    };
    use reify_ir::{CompiledExpr, DeterminacyState, PersistentMap, Value, ValueMap};

    use super::{
        ScopedClassification, compute_changed_realizations, compute_changed_realizations_scoped,
        stale_realization_entities,
    };
    use crate::cache::NodeId;
    use crate::graph::EvaluationGraph;

    fn rid(entity: &str) -> RealizationNodeId {
        RealizationNodeId::new(entity, 0)
    }

    fn entities(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn no_graph() -> EvaluationGraph {
        EvaluationGraph::default()
    }

    /// A graph of realizations `(entity, index, geometry member, ops)`.
    fn graph_of(
        realizations: &[(&str, u32, &str, Vec<reify_compiler::CompiledGeometryOp>)],
    ) -> EvaluationGraph {
        use crate::graph::RealizationNodeData;
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        for (entity, index, member, operations) in realizations {
            let id = RealizationNodeId::new(*entity, *index);
            graph.realizations.insert(
                id.clone(),
                RealizationNodeData {
                    id: id.clone(),
                    operations: operations.clone(),
                    content_hash: ContentHash::of_str(&id.to_string()),
                    produced_repr: ReprKind::BRep,
                    produced_kernel: None,
                    geometry_cell: Some(ValueCellId::new(*entity, *member)),
                    input_cone_hash: None,
                },
            );
        }
        graph
    }

    fn a_box() -> Vec<reify_compiler::CompiledGeometryOp> {
        vec![reify_compiler::CompiledGeometryOp::Primitive {
            kind: reify_compiler::PrimitiveKind::Box,
            args: vec![],
        }]
    }

    fn union_of(left: &str, right: &str) -> Vec<reify_compiler::CompiledGeometryOp> {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Sub(left.to_string()),
            right: GeomRef::Sub(right.to_string()),
        }]
    }

    /// The 4317-class trap behind δ S7: `A.body` and `B.body` export the same
    /// member name, so the reverse index cannot resolve `C`'s `a.body`
    /// operand and β's cone never reaches `C`. Any cross-entity `Sub`
    /// consumer is therefore stale whenever anything is.
    #[test]
    fn includes_every_cross_entity_sub_consumer_once_anything_is_stale() {
        let graph = graph_of(&[
            ("A", 0, "body", a_box()),
            ("B", 0, "body", a_box()),
            ("C", 0, "combined", union_of("a.body", "b.body")),
        ]);
        let changed = HashSet::from([rid("A")]);

        let stale = stale_realization_entities(&changed, [], &HashSet::new(), &graph);

        assert_eq!(stale, entities(&["A", "C"]));
    }

    /// A `Sub` operand naming a realization of the SAME entity is a sibling
    /// reference: the family is keyed by entity, so it is already evicted
    /// exactly when the sibling is.
    #[test]
    fn a_sibling_sub_reference_does_not_make_a_cross_entity_consumer() {
        let graph = graph_of(&[
            ("E", 0, "base", a_box()),
            ("E", 1, "hole", a_box()),
            ("E", 2, "body", union_of("base", "hole")),
            ("X", 0, "body", a_box()),
        ]);
        let changed = HashSet::from([rid("X")]);

        let stale = stale_realization_entities(&changed, [], &HashSet::new(), &graph);

        assert_eq!(stale, entities(&["X"]));
    }

    /// PRD §6 zero-eviction row, with a cross-entity consumer present.
    #[test]
    fn a_cross_entity_sub_consumer_survives_an_edit_that_stales_nothing() {
        let graph = graph_of(&[
            ("A", 0, "body", a_box()),
            ("C", 0, "combined", union_of("a.body", "a.body")),
        ]);

        let stale = stale_realization_entities(&HashSet::new(), [], &HashSet::new(), &graph);

        assert!(
            stale.is_empty(),
            "nothing moved, nothing is evicted: {stale:?}"
        );
    }

    #[test]
    fn includes_the_entity_of_every_changed_seed() {
        let changed = HashSet::from([rid("PartA"), rid("PartB")]);

        let stale = stale_realization_entities(&changed, [], &HashSet::new(), &no_graph());

        assert_eq!(stale, entities(&["PartA", "PartB"]));
    }

    /// `Asm` is a `GeomRef::Sub` consumer of the seed `PartA`: its own
    /// input-cone fold never moves, so only the transitive dirty cone reaches
    /// it. `dirty::compute_dirty_cone_with_realizations` never puts the SEEDS
    /// in its result, so the seed's entity must come from `changed`.
    #[test]
    fn includes_the_entity_of_every_realization_reached_through_the_dirty_cone() {
        let changed = HashSet::from([rid("PartA")]);
        let cone = HashSet::from([NodeId::Realization(rid("Asm"))]);

        let stale = stale_realization_entities(&changed, [], &cone, &no_graph());

        assert_eq!(stale, entities(&["PartA", "Asm"]));
    }

    /// A removed realization (collection shrink, recompile removal) leaves
    /// its entity's cached terminal behind, where it could now stand in for a
    /// different realization.
    #[test]
    fn includes_the_entity_of_every_removed_realization() {
        let removed = [rid("Dropped")];

        let stale =
            stale_realization_entities(&HashSet::new(), &removed, &HashSet::new(), &no_graph());

        assert_eq!(stale, entities(&["Dropped"]));
    }

    #[test]
    fn ignores_value_compute_and_constraint_members_of_the_cone() {
        let cone = HashSet::from([
            NodeId::Value(ValueCellId::new("PartA", "w")),
            NodeId::Compute(ComputeNodeId::new("Fea", 0)),
            NodeId::Constraint(ConstraintNodeId::new("PartA", 0)),
        ]);

        let stale = stale_realization_entities(&HashSet::new(), [], &cone, &no_graph());

        assert!(
            stale.is_empty(),
            "only realization members name a family: {stale:?}"
        );
    }

    /// PRD §6 zero-eviction row: an edit that moves no realization evicts
    /// nothing.
    #[test]
    fn is_empty_for_a_no_realization_edit() {
        let stale = stale_realization_entities(&HashSet::new(), [], &HashSet::new(), &no_graph());

        assert!(stale.is_empty());
    }

    // ------------------------------------------------------------------
    // selective-realization-eviction task β (#4729): the shared
    // recompute-then-compare helper `compute_changed_realizations`.
    //
    // These are pure graph-level unit tests — no Engine, no kernel, no
    // `.ri` source. The fixture style is copied from `dirty.rs:658-735`
    // (literal `RealizationNodeData` / `ValueCellNode` construction over a
    // hand-built `EvaluationGraph`), which is what makes the core §11.2
    // contract testable without OCCT.
    //
    // Every expected hash is DERIVED by calling the canonical fold
    // (`engine_build::compute_realization_upstream_values_hash_from_ops`,
    // widened to `pub(crate)` by this task's prerequisite) rather than
    // hard-coded — PRD D1 forbids a second fold, so the test must key on
    // the same identity the production compare does. These are exact
    // 32-byte equalities over a deterministic XXH3 fold; there is no
    // tolerance anywhere.
    // ------------------------------------------------------------------

    /// Build a one-realization `EvaluationGraph` whose single
    /// `Primitive{Box}` op reads `E.<cell>` through a `ValueRef` arg, so the
    /// input-cone fold is non-trivial and moves with the `EvalContext`.
    ///
    /// `input_cone_hash` is whatever the caller supplies, standing in for
    /// α's production write inside `execute_realization_ops`.
    fn realization_graph_reading_cell(
        rid: &reify_core::RealizationNodeId,
        cell: &ValueCellId,
        input_cone_hash: Option<[u8; 32]>,
    ) -> EvaluationGraph {
        let mut graph = EvaluationGraph::default();
        insert_realization_reading(&mut graph, rid, std::slice::from_ref(cell), input_cone_hash);
        graph
    }

    /// Insert `rid` into `graph` with one `Primitive{Box}` op per cell in
    /// `cells`, each reading its cell through a `ValueRef` width arg.
    fn insert_realization_reading(
        graph: &mut EvaluationGraph,
        rid: &reify_core::RealizationNodeId,
        cells: &[ValueCellId],
        input_cone_hash: Option<[u8; 32]>,
    ) {
        use crate::graph::RealizationNodeData;
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_ir::ReprKind;

        let operations = cells
            .iter()
            .map(|cell| CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![(
                    "width".to_string(),
                    CompiledExpr::value_ref(cell.clone(), Type::dimensionless_scalar()),
                )],
            })
            .collect();
        graph.realizations.insert(
            rid.clone(),
            RealizationNodeData {
                id: rid.clone(),
                operations,
                content_hash: ContentHash::of_str(&rid.to_string()),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
                geometry_cell: None,
                input_cone_hash,
            },
        );
    }

    /// Long-lived empty meta-map for the β helper tests.
    ///
    /// `eval_ctx_with_meta` borrows the map for the whole lifetime of the
    /// returned `EvalContext`, so it cannot be a `&HashMap::new()`
    /// temporary. Same shape as `deps.rs:73`'s `EMPTY_SET`.
    static NO_META: std::sync::LazyLock<HashMap<String, HashMap<String, String>>> =
        std::sync::LazyLock::new(HashMap::new);

    /// A `ValueMap` binding `cell` to the real number `v`.
    fn values_binding(cell: &ValueCellId, v: f64) -> ValueMap {
        let mut values = ValueMap::default();
        values.insert(cell.clone(), Value::Real(v));
        values
    }

    /// §11.2 case (a): the stored `input_cone_hash` equals the hash
    /// recomputed over the SAME context → the realization is UNCHANGED and
    /// must be absent from the returned set.
    ///
    /// This is the case that makes β selective at all: if it were ever to
    /// regress into "always changed", γ's keyed eviction degenerates back
    /// into the wholesale flush it is meant to replace.
    #[test]
    fn compute_changed_realizations_omits_realization_whose_input_cone_is_unmoved() {
        use crate::engine_build::compute_realization_upstream_values_hash_from_ops;

        let rid = reify_core::RealizationNodeId::new("E", 0);
        let cell = ValueCellId::new("E", "wa");
        let values = values_binding(&cell, 10.0);
        let ctx = crate::eval_ctx_with_meta(&values, &[], &NO_META);

        // Derive the "as of last execution" hash from the canonical fold
        // over the very same ctx, exactly as α's production write does.
        let probe = realization_graph_reading_cell(&rid, &cell, None);
        let stored = compute_realization_upstream_values_hash_from_ops(
            &probe.realizations.get(&rid).unwrap().operations,
            &ctx,
        );

        let graph = realization_graph_reading_cell(&rid, &cell, Some(stored));
        let changed = super::compute_changed_realizations(&graph.realizations, &graph, &ctx);

        assert!(
            changed.is_empty(),
            "an unmoved input cone must not be reported changed, got: {changed:?}"
        );
    }

    /// §11.2 case (b): the context moved, so the recomputed fold differs
    /// from the stored hash → the realization IS in the set.
    #[test]
    fn compute_changed_realizations_reports_realization_whose_input_cone_moved() {
        use crate::engine_build::compute_realization_upstream_values_hash_from_ops;

        let rid = reify_core::RealizationNodeId::new("E", 0);
        let cell = ValueCellId::new("E", "wa");

        // "Last execution" was at wa = 10.
        let old_values = values_binding(&cell, 10.0);
        let old_ctx = crate::eval_ctx_with_meta(&old_values, &[], &NO_META);
        let probe = realization_graph_reading_cell(&rid, &cell, None);
        let stored = compute_realization_upstream_values_hash_from_ops(
            &probe.realizations.get(&rid).unwrap().operations,
            &old_ctx,
        );

        // The edit moved wa to 20.
        let new_values = values_binding(&cell, 20.0);
        let new_ctx = crate::eval_ctx_with_meta(&new_values, &[], &NO_META);

        // Premise lock: the fold genuinely moves with the context. Without
        // this the test could pass for the wrong reason (e.g. a fold that
        // ignores its args would make EVERY realization look changed).
        let recomputed = compute_realization_upstream_values_hash_from_ops(
            &probe.realizations.get(&rid).unwrap().operations,
            &new_ctx,
        );
        assert_ne!(
            stored, recomputed,
            "premise: the input-cone fold must move when a ValueRef arg's value moves"
        );

        let graph = realization_graph_reading_cell(&rid, &cell, Some(stored));
        let changed = super::compute_changed_realizations(&graph.realizations, &graph, &new_ctx);

        assert_eq!(
            changed,
            HashSet::from([rid.clone()]),
            "a moved input cone must be reported changed"
        );
    }

    /// §11.2 case (c), sub-case 1: the prior entry EXISTS but its
    /// `input_cone_hash` is `None` — never executed, demand-pruned, or
    /// un-hydrated. Conservatively CHANGED.
    #[test]
    fn compute_changed_realizations_reports_realization_with_no_stored_hash() {
        let rid = reify_core::RealizationNodeId::new("E", 0);
        let cell = ValueCellId::new("E", "wa");
        let values = values_binding(&cell, 10.0);
        let ctx = crate::eval_ctx_with_meta(&values, &[], &NO_META);

        let graph = realization_graph_reading_cell(&rid, &cell, None);
        let changed = super::compute_changed_realizations(&graph.realizations, &graph, &ctx);

        assert_eq!(
            changed,
            HashSet::from([rid.clone()]),
            "a realization that has never executed (input_cone_hash == None) must be \
             conservatively reported changed (PRD §11.2)"
        );
    }

    /// §11.2 case (c), sub-case 2: the realization is present in
    /// `new_graph` but has NO prior entry at all — newly added by an
    /// `edit_source` recompile. Conservatively CHANGED.
    #[test]
    fn compute_changed_realizations_reports_realization_absent_from_prior_map() {
        use crate::engine_build::compute_realization_upstream_values_hash_from_ops;

        let rid = reify_core::RealizationNodeId::new("E", 0);
        let cell = ValueCellId::new("E", "wa");
        let values = values_binding(&cell, 10.0);
        let ctx = crate::eval_ctx_with_meta(&values, &[], &NO_META);

        // The new graph's node even carries a matching stored hash — the
        // point is that the PRIOR map is what is consulted, and it is empty.
        let probe = realization_graph_reading_cell(&rid, &cell, None);
        let stored = compute_realization_upstream_values_hash_from_ops(
            &probe.realizations.get(&rid).unwrap().operations,
            &ctx,
        );
        let new_graph = realization_graph_reading_cell(&rid, &cell, Some(stored));

        let empty_prior: PersistentMap<
            reify_core::RealizationNodeId,
            crate::graph::RealizationNodeData,
        > = PersistentMap::default();
        let changed = super::compute_changed_realizations(&empty_prior, &new_graph, &ctx);

        assert_eq!(
            changed,
            HashSet::from([rid.clone()]),
            "a realization with no entry in the PRIOR map (newly added by a recompile) \
             must be conservatively reported changed — the prior map, not the new \
             node's own field, is the comparison source"
        );
    }

    /// CHARACTERIZATION LOCK, not a behavioural assertion: the canonical
    /// input-cone fold is BLIND to a boolean-kind flip.
    ///
    /// `compute_realization_upstream_values_hash_from_ops` folds only
    /// `(arg_name, evaluated value)` pairs, and its match arm for a boolean is
    /// `CompiledGeometryOp::Boolean { .. } => &[]` in `engine_build.rs` — so
    /// the arg loop never runs and the op contributes NOTHING to the hash. No
    /// arm mixes in the op discriminant, the op kind, the op count, or the
    /// operand `GeomRef`s either. Two op vectors that differ only in a
    /// `BooleanOp::Union` vs `BooleanOp::Difference` therefore fold to the
    /// byte-identical 32-byte hash.
    ///
    /// This PASSES from the start. It is pinned as a first-class executable
    /// fact because it is the load-bearing premise for the `edit_source`
    /// carry-forward restriction: the input-cone fold cannot detect an
    /// ops-only change, so the carry-forward must NOT rely on "the recomputed
    /// fold will differ anyway" and must instead key on
    /// `RealizationNodeData::content_hash`, which sees the op IR.
    ///
    /// If this test ever FAILS, the fold has gained op-identity coverage and
    /// the `!changed_realizations.contains(rid)` skip in `edit_source` may be
    /// relaxed. Do NOT "fix" it by weakening the assertion — the failure is
    /// the signal.
    ///
    /// Exact byte equality over a deterministic XXH3 fold; no tolerance.
    #[test]
    fn input_cone_fold_is_blind_to_a_boolean_kind_flip() {
        use crate::engine_build::compute_realization_upstream_values_hash_from_ops;
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};

        let cell = ValueCellId::new("E", "wa");
        let values = values_binding(&cell, 10.0);
        let ctx = crate::eval_ctx_with_meta(&values, &[], &NO_META);

        // Two boxes, then a boolean over them — the shape `compile_boolean`
        // emits (`left_ops ++ right_ops ++ [Boolean{op,left,right}]`,
        // geometry_boolean.rs:165-171).
        let ops_with = |op: BooleanOp| -> Vec<CompiledGeometryOp> {
            vec![
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    args: vec![(
                        "width".to_string(),
                        CompiledExpr::value_ref(cell.clone(), Type::dimensionless_scalar()),
                    )],
                },
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    args: vec![(
                        "width".to_string(),
                        CompiledExpr::value_ref(cell.clone(), Type::dimensionless_scalar()),
                    )],
                },
                CompiledGeometryOp::Boolean {
                    op,
                    // Identical operands on both sides: the ONLY difference
                    // between the two vectors is the `op` field itself.
                    left: GeomRef::Step(0),
                    right: GeomRef::Step(1),
                },
            ]
        };

        let union_hash =
            compute_realization_upstream_values_hash_from_ops(&ops_with(BooleanOp::Union), &ctx);
        let difference_hash = compute_realization_upstream_values_hash_from_ops(
            &ops_with(BooleanOp::Difference),
            &ctx,
        );

        assert_eq!(
            union_hash, difference_hash,
            "the input-cone fold must be blind to a union→difference flip — it folds \
             only (arg_name, value) pairs and matches `Boolean {{ .. }} => &[]`. If this \
             now DIFFERS the fold gained op-identity coverage; relax the edit_source \
             carry-forward skip rather than weakening this lock."
        );
    }

    // ------------------------------------------------------------------
    // #6086: `compute_changed_realizations_scoped`, edit_param's fold
    // restricted to the realizations the edit can have moved. Its contract is
    // EQUALITY with the unscoped fold: a realization it skips but the full
    // fold reports is under-eviction, which serves stale geometry.
    // ------------------------------------------------------------------

    fn cell(member: &str) -> ValueCellId {
        ValueCellId::new("E", member)
    }

    fn values_of(bindings: &[(&ValueCellId, f64)]) -> ValueMap {
        let mut values = ValueMap::default();
        for (cell, v) in bindings {
            values.insert((*cell).clone(), Value::Real(*v));
        }
        values
    }

    /// The pre-edit snapshot values `values` stand for.
    fn snapshot_of(values: &ValueMap) -> PersistentMap<ValueCellId, (Value, DeterminacyState)> {
        let mut snapshot = PersistentMap::default();
        for (cell, value) in values.iter() {
            snapshot.insert(cell.clone(), (value.clone(), DeterminacyState::Determined));
        }
        snapshot
    }

    /// α's write at execution: every realization's stored hash becomes the
    /// fold over `values`.
    fn stamp_all(graph: &mut EvaluationGraph, values: &ValueMap) {
        let ctx = crate::eval_ctx_with_meta(values, &[], &NO_META);
        let ids: Vec<RealizationNodeId> =
            graph.realizations.iter().map(|(r, _)| r.clone()).collect();
        for rid in ids {
            let node = graph.realizations.get_mut(&rid).unwrap();
            node.input_cone_hash = Some(
                crate::engine_build::compute_realization_upstream_values_hash_from_ops(
                    &node.operations,
                    &ctx,
                ),
            );
        }
    }

    fn scoped(
        graph: &EvaluationGraph,
        values: &ValueMap,
        prior: &ValueMap,
        pending: &HashSet<RealizationNodeId>,
    ) -> ScopedClassification {
        let ctx = crate::eval_ctx_with_meta(values, &[], &NO_META);
        compute_changed_realizations_scoped(graph, &ctx, values, &snapshot_of(prior), pending)
    }

    #[test]
    fn a_realization_whose_read_cell_value_changed_is_folded_and_reported() {
        let wa = cell("wa");
        let before = values_of(&[(&wa, 10.0)]);
        let mut graph = EvaluationGraph::default();
        insert_realization_reading(&mut graph, &rid("A"), &[wa.clone()], None);
        stamp_all(&mut graph, &before);

        let after = values_of(&[(&wa, 20.0)]);
        let result = scoped(&graph, &after, &before, &HashSet::new());

        assert_eq!(result.changed, HashSet::from([rid("A")]));
        assert_eq!(result.folded, 1);
    }

    /// The cost claim: a realization none of whose read cells changed value
    /// is neither reported nor folded.
    #[test]
    fn a_realization_whose_read_cells_are_value_identical_is_not_folded() {
        let (wa, wb) = (cell("wa"), cell("wb"));
        let before = values_of(&[(&wa, 10.0), (&wb, 20.0)]);
        let mut graph = EvaluationGraph::default();
        insert_realization_reading(&mut graph, &rid("A"), &[wa.clone()], None);
        insert_realization_reading(&mut graph, &rid("B"), &[wb.clone()], None);
        stamp_all(&mut graph, &before);

        let after = values_of(&[(&wa, 30.0), (&wb, 20.0)]);
        let result = scoped(&graph, &after, &before, &HashSet::new());

        assert_eq!(result.changed, HashSet::from([rid("A")]));
        assert_eq!(
            result.folded, 1,
            "B's read cell is value-identical: no fold"
        );
    }

    /// PRD §11.2: never executed means conservatively CHANGED, and there is
    /// no stored hash to compare a fold against.
    #[test]
    fn a_realization_with_no_stored_hash_is_changed_without_a_fold() {
        let wa = cell("wa");
        let values = values_of(&[(&wa, 10.0)]);
        let mut graph = EvaluationGraph::default();
        insert_realization_reading(&mut graph, &rid("A"), &[wa.clone()], None);

        let result = scoped(&graph, &values, &values, &HashSet::new());

        assert_eq!(result.changed, HashSet::from([rid("A")]));
        assert_eq!(result.folded, 0);
    }

    /// β's cumulative "since the last EXECUTION" semantics: a realization an
    /// earlier edit moved stays reported through an edit that touches none of
    /// its cells, and drops out once a build re-stamps its hash.
    #[test]
    fn a_pending_realization_is_refolded_even_if_this_edit_did_not_touch_it() {
        let wa = cell("wa");
        let executed = values_of(&[(&wa, 10.0)]);
        let mut graph = EvaluationGraph::default();
        insert_realization_reading(&mut graph, &rid("A"), &[wa.clone()], None);
        stamp_all(&mut graph, &executed);
        let moved = values_of(&[(&wa, 20.0)]);
        let pending = HashSet::from([rid("A")]);

        let still_stale = scoped(&graph, &moved, &moved, &pending);

        assert_eq!(still_stale.changed, pending, "not rebuilt since wa moved");
        assert_eq!(still_stale.folded, 1);

        stamp_all(&mut graph, &moved);
        let rebuilt = scoped(&graph, &moved, &moved, &pending);

        assert!(rebuilt.changed.is_empty(), "the rebuild retired it");
    }

    #[test]
    fn a_pending_id_absent_from_the_graph_is_dropped() {
        let wa = cell("wa");
        let values = values_of(&[(&wa, 10.0)]);
        let mut graph = EvaluationGraph::default();
        insert_realization_reading(&mut graph, &rid("A"), &[wa.clone()], None);
        stamp_all(&mut graph, &values);

        let result = scoped(&graph, &values, &values, &HashSet::from([rid("Ghost")]));

        assert!(result.changed.is_empty(), "got {:?}", result.changed);
    }

    /// The safety contract as a differential over an edit sequence: at every
    /// step the scoped set equals the unscoped fold. `D` starts never
    /// executed; `C` reads two cells.
    #[test]
    fn scoped_equals_unscoped_over_an_edit_sequence() {
        let cells = [cell("c0"), cell("c1"), cell("c2"), cell("c3")];
        let mut graph = EvaluationGraph::default();
        insert_realization_reading(&mut graph, &rid("A"), &[cells[0].clone()], None);
        insert_realization_reading(&mut graph, &rid("B"), &[cells[1].clone()], None);
        insert_realization_reading(
            &mut graph,
            &rid("C"),
            &[cells[0].clone(), cells[2].clone()],
            None,
        );
        let mut values = values_of(&[
            (&cells[0], 1.0),
            (&cells[1], 2.0),
            (&cells[2], 3.0),
            (&cells[3], 4.0),
        ]);
        stamp_all(&mut graph, &values);
        insert_realization_reading(&mut graph, &rid("D"), &[cells[3].clone()], None);

        // (cell index, new value, build after the edit)
        let steps: [(usize, f64, bool); 9] = [
            (0, 5.0, false),
            (1, 2.0, false),
            (2, 9.0, false),
            (3, 4.0, false),
            (1, 6.0, true),
            (0, 5.0, false),
            (3, 8.0, false),
            (2, 9.0, true),
            (0, 1.0, false),
        ];
        let mut pending = HashSet::new();
        let mut skipped_a_fold = false;
        for (step, (index, value, build_after)) in steps.into_iter().enumerate() {
            let prior = values.clone();
            values.insert(cells[index].clone(), Value::Real(value));

            let result = scoped(&graph, &values, &prior, &pending);
            let ctx = crate::eval_ctx_with_meta(&values, &[], &NO_META);
            let unscoped = compute_changed_realizations(&graph.realizations, &graph, &ctx);

            assert_eq!(result.changed, unscoped, "step {step}: scoped ≠ unscoped");
            skipped_a_fold |= result.folded < graph.realizations.len();
            pending = result.changed;
            if build_after {
                stamp_all(&mut graph, &values);
            }
        }
        assert!(skipped_a_fold, "the sequence must exercise a skipped fold");
    }
}
