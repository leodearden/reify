// EvaluationGraph: typed graph nodes backed by PersistentMap.

use std::collections::HashSet;

use reify_compiler::{
    CompiledConnection, CompiledForallTemplate, CompiledGeometryOp, TopologyTemplate,
    ValueCellKind, find_template,
};
use reify_types::{
    CompiledExpr, ConstraintNodeId, ContentHash, PersistentMap, RealizationNodeId,
    ResolutionNodeId, Type, Value, ValueCellId, ValueMap,
};

/// A value cell node in the evaluation graph.
/// Corresponds to a param or let binding in the topology.
#[derive(Debug, Clone)]
pub struct ValueCellNode {
    pub id: ValueCellId,
    pub kind: ValueCellKind,
    pub cell_type: Type,
    pub default_expr: Option<CompiledExpr>,
    pub content_hash: ContentHash,
}

/// A constraint node in the evaluation graph.
/// Holds the compiled constraint expression, optional label, and its content hash.
#[derive(Debug, Clone)]
pub struct ConstraintNodeData {
    pub id: ConstraintNodeId,
    pub label: Option<String>,
    pub expr: CompiledExpr,
    pub content_hash: ContentHash,
    /// Target name from an `@optimized("...")` annotation on the originating
    /// constraint def, if any. Used by `Engine::dispatch_constraints` to route
    /// this constraint through a registered `OptimizedImpl` instead of the
    /// language-level `ConstraintChecker` (Task 273).
    pub optimized_target: Option<String>,
}

/// A realization node in the evaluation graph.
/// Holds the compiled geometry operations and content hash.
#[derive(Debug, Clone)]
pub struct RealizationNodeData {
    pub id: RealizationNodeId,
    pub operations: Vec<CompiledGeometryOp>,
    pub content_hash: ContentHash,
}

/// A resolution node in the evaluation graph.
/// Holds references to auto parameters and constraint dependencies
/// for constraint resolution (solving). Dependencies are static (from the template).
#[derive(Debug, Clone)]
pub struct ResolutionNodeData {
    pub id: ResolutionNodeId,
    pub scope: String,
    pub auto_params: Vec<ValueCellId>,
    pub constraint_deps: Vec<ConstraintNodeId>,
    pub content_hash: ContentHash,
}

/// Metadata for a guarded group in the evaluation graph.
/// Tracks which cells and constraints are conditionally active.
#[derive(Debug, Clone)]
pub struct GuardedGroupInfo {
    /// The guard ValueCellId (Bool, Let kind) that controls this group.
    pub guard_cell: ValueCellId,
    /// Members active when guard is true.
    pub members: Vec<ValueCellId>,
    /// Constraints active when guard is true.
    pub constraints: Vec<ConstraintNodeId>,
    /// Members active when guard is false (else branch).
    pub else_members: Vec<ValueCellId>,
    /// Constraints active when guard is false (else branch).
    pub else_constraints: Vec<ConstraintNodeId>,
}

/// Metadata for a collection sub-component in the evaluation graph.
/// Tracks the count cell and child template info needed for re-elaboration.
#[derive(Debug, Clone)]
pub struct CollectionSubInfo {
    /// The parent template/entity name (e.g., "Parent").
    pub parent_entity: String,
    /// The sub-component name (e.g., "bolts").
    pub sub_name: String,
    /// The child structure name (e.g., "Bolt").
    pub structure_name: String,
    /// The count cell ValueCellId (e.g., Parent.__count_bolts).
    pub count_cell: ValueCellId,
    /// The child template's value cell declarations, stored for re-elaboration.
    pub child_value_cells: Vec<(String, ValueCellKind, Type, Option<CompiledExpr>)>,
}

/// The evaluation graph: holds all typed nodes in PersistentMaps
/// for O(1) clone with structural sharing.
#[derive(Debug, Clone, Default)]
pub struct EvaluationGraph {
    pub value_cells: PersistentMap<ValueCellId, ValueCellNode>,
    pub constraints: PersistentMap<ConstraintNodeId, ConstraintNodeData>,
    pub realizations: PersistentMap<RealizationNodeId, RealizationNodeData>,
    pub resolutions: PersistentMap<ResolutionNodeId, ResolutionNodeData>,
    /// Guarded groups with conditional membership.
    pub guarded_groups: Vec<GuardedGroupInfo>,
    /// ValueCellIds whose boolean value controls topology (guard cells).
    pub structure_controlling: HashSet<ValueCellId>,
    /// Collection sub-component metadata for count-based re-elaboration.
    pub collection_subs: Vec<CollectionSubInfo>,
    /// Captured per-element body templates for statement-form `forall`
    /// over deferred-count collection subs (task 2629; PRD criterion 7
    /// second-half). The runtime `Engine::edit_param` collection-count
    /// phase walks these whenever a count cell becomes known and emits
    /// per-element constraints / connections by rewriting `coll_sub[0]`
    /// placeholder cell IDs to `coll_sub[i]`.
    ///
    /// **Hash stability:** intentionally NOT mixed into
    /// `topology_fingerprint`; per-element constraints become observable
    /// in the fingerprint only once they materialize in `constraints` at
    /// runtime emission.
    pub forall_templates: Vec<CompiledForallTemplate>,
    /// Compiled connections carried through from `template.connections`
    /// (task 2690). Mutated at runtime by `Engine::edit_param`'s
    /// collection-count phase to materialise per-element forall-Connect
    /// emissions when a `__count_<sub>` cell becomes known. Each entry's
    /// `compatibility_constraint` ties it to the synthesised
    /// `ConstraintNodeData` in `constraints`; runtime cleanup walks
    /// `Snapshot::forall_emitted` and removes matching entries here as
    /// well as in `constraints`.
    ///
    /// **Hash stability:** mixed into `topology_fingerprint` (a sixth
    /// per-bucket sub-hash with domain separation) so cache keys vary
    /// when the connection set changes.
    pub connections: Vec<CompiledConnection>,
}

impl EvaluationGraph {
    /// Build an EvaluationGraph from compiled topology templates.
    ///
    /// Converts each template's declarations into typed graph nodes:
    /// - ValueCellDecl → ValueCellNode
    /// - CompiledConstraint → ConstraintNodeData
    /// - RealizationDecl → RealizationNodeData
    pub fn from_templates(templates: &[TopologyTemplate]) -> Self {
        let mut graph = EvaluationGraph::default();

        for template in templates {
            // task 2629: carry forall templates (deferred-count statement-form
            // forall body shapes) into the runtime graph. Populated alongside
            // other per-template extraction passes so the runtime
            // `Engine::edit_param` collection-count phase can consume them
            // when a count cell becomes known.
            graph
                .forall_templates
                .extend(template.forall_templates.iter().cloned());

            // task 2690: carry compile-time connections into the runtime
            // graph. The forall-Connect runtime arm in
            // `engine_edit::edit_param` mutates this Vec in lockstep with
            // `graph.constraints` when a deferred-count cell becomes known.
            graph
                .connections
                .extend(template.connections.iter().cloned());

            for cell in &template.value_cells {
                let id_hash = ContentHash::of_str(&format!("{}", cell.id));
                let expr_hash = cell
                    .default_expr
                    .as_ref()
                    .map(|e| e.content_hash)
                    .unwrap_or(ContentHash(0));
                let node = ValueCellNode {
                    id: cell.id.clone(),
                    kind: cell.kind,
                    cell_type: cell.cell_type.clone(),
                    default_expr: cell.default_expr.clone(),
                    content_hash: id_hash.combine(expr_hash),
                };
                graph.value_cells.insert(cell.id.clone(), node);
            }

            for constraint in &template.constraints {
                let id_hash = ContentHash::of_str(&format!("{}", constraint.id));
                let node = ConstraintNodeData {
                    id: constraint.id.clone(),
                    label: constraint.label.clone(),
                    expr: constraint.expr.clone(),
                    content_hash: id_hash.combine(constraint.expr.content_hash),
                    optimized_target: constraint.optimized_target.clone(),
                };
                graph.constraints.insert(constraint.id.clone(), node);
            }

            for realization in &template.realizations {
                let id_hash = ContentHash::of_str(&format!("{}", realization.id));
                let ops_hash = ContentHash::combine_all(
                    realization
                        .operations
                        .iter()
                        .map(|op| ContentHash::of_str(&format!("{:?}", op))),
                );
                let node = RealizationNodeData {
                    id: realization.id.clone(),
                    operations: realization.operations.clone(),
                    content_hash: id_hash.combine(ops_hash),
                };
                graph.realizations.insert(realization.id.clone(), node);
            }

            // Sub-component elaboration: create scoped ValueCellNode entries
            for sub in &template.sub_components {
                let child_template = match find_template(templates, &sub.structure_name) {
                    Some(t) => t,
                    None => continue, // skip unknown structures silently
                };

                if sub.is_collection {
                    // Collection sub: determine count from the count cell's default_expr literal
                    let count = sub.count_cell.as_ref().and_then(|count_id| {
                        // Look up the count cell in this template's value_cells
                        template
                            .value_cells
                            .iter()
                            .find(|vc| vc.id == *count_id)
                            .and_then(|vc| vc.default_expr.as_ref())
                            .and_then(|expr| {
                                // If the count expr is a literal Int, use it directly
                                if let reify_types::CompiledExprKind::Literal(Value::Int(n)) =
                                    &expr.kind
                                {
                                    Some(*n)
                                } else {
                                    // For ValueRef expressions, look up the referenced cell's default
                                    if let reify_types::CompiledExprKind::ValueRef(ref_id) =
                                        &expr.kind
                                    {
                                        template
                                            .value_cells
                                            .iter()
                                            .find(|vc| vc.id == *ref_id)
                                            .and_then(|vc| vc.default_expr.as_ref())
                                            .and_then(|e| {
                                                if let reify_types::CompiledExprKind::Literal(
                                                    Value::Int(n),
                                                ) = &e.kind
                                                {
                                                    Some(*n)
                                                } else {
                                                    None
                                                }
                                            })
                                    } else {
                                        None
                                    }
                                }
                            })
                    });

                    if let Some(n) = count {
                        for i in 0..n {
                            let scoped_entity = format!("{}.{}[{}]", template.name, sub.name, i);
                            for child_cell in &child_template.value_cells {
                                let scoped_id =
                                    ValueCellId::new(&scoped_entity, &child_cell.id.member);
                                let id_hash = ContentHash::of_str(&format!("{}", scoped_id));
                                let expr_hash = child_cell
                                    .default_expr
                                    .as_ref()
                                    .map(|e| e.content_hash)
                                    .unwrap_or(ContentHash(0));
                                let node = ValueCellNode {
                                    id: scoped_id.clone(),
                                    kind: child_cell.kind,
                                    cell_type: child_cell.cell_type.clone(),
                                    default_expr: child_cell.default_expr.clone(),
                                    content_hash: id_hash.combine(expr_hash),
                                };
                                graph.value_cells.insert(scoped_id, node);
                            }
                        }
                    }
                    // Store collection sub info for re-elaboration
                    if let Some(count_id) = &sub.count_cell {
                        graph.collection_subs.push(CollectionSubInfo {
                            parent_entity: template.name.clone(),
                            sub_name: sub.name.clone(),
                            structure_name: sub.structure_name.clone(),
                            count_cell: count_id.clone(),
                            child_value_cells: child_template
                                .value_cells
                                .iter()
                                .map(|vc| {
                                    (
                                        vc.id.member.clone(),
                                        vc.kind,
                                        vc.cell_type.clone(),
                                        vc.default_expr.clone(),
                                    )
                                })
                                .collect(),
                        });
                    }
                    // If count is None (Undef), no instances are created
                } else {
                    // Non-collection sub: single scoped entity
                    let scoped_entity = format!("{}.{}", template.name, sub.name);

                    for child_cell in &child_template.value_cells {
                        let scoped_id = ValueCellId::new(&scoped_entity, &child_cell.id.member);
                        let id_hash = ContentHash::of_str(&format!("{}", scoped_id));
                        let expr_hash = child_cell
                            .default_expr
                            .as_ref()
                            .map(|e| e.content_hash)
                            .unwrap_or(ContentHash(0));
                        let node = ValueCellNode {
                            id: scoped_id.clone(),
                            kind: child_cell.kind,
                            cell_type: child_cell.cell_type.clone(),
                            default_expr: child_cell.default_expr.clone(),
                            content_hash: id_hash.combine(expr_hash),
                        };
                        graph.value_cells.insert(scoped_id, node);
                    }
                }
            }

            // Guarded groups: create guard ValueCell nodes, member/else nodes,
            // constraint/else-constraint nodes, and store GuardedGroupInfo metadata.
            for group in &template.guarded_groups {
                // 1. Create ValueCellNode for the guard cell (Bool, Let kind)
                let guard_id = &group.guard_value_cell;
                let guard_id_hash = ContentHash::of_str(&format!("{}", guard_id));
                let guard_expr_hash = group.guard_expr.content_hash;
                let guard_node = ValueCellNode {
                    id: guard_id.clone(),
                    kind: reify_compiler::ValueCellKind::Let,
                    cell_type: Type::Bool,
                    default_expr: Some(group.guard_expr.clone()),
                    content_hash: guard_id_hash.combine(guard_expr_hash),
                };
                graph.value_cells.insert(guard_id.clone(), guard_node);

                // 2. Create ValueCellNodes for all members
                let mut member_ids = Vec::new();
                for cell in &group.members {
                    let id_hash = ContentHash::of_str(&format!("{}", cell.id));
                    let expr_hash = cell
                        .default_expr
                        .as_ref()
                        .map(|e| e.content_hash)
                        .unwrap_or(ContentHash(0));
                    let node = ValueCellNode {
                        id: cell.id.clone(),
                        kind: cell.kind,
                        cell_type: cell.cell_type.clone(),
                        default_expr: cell.default_expr.clone(),
                        content_hash: id_hash.combine(expr_hash),
                    };
                    graph.value_cells.insert(cell.id.clone(), node);
                    member_ids.push(cell.id.clone());
                }

                // 3. Create ConstraintNodeData for guard-true constraints
                let mut constraint_ids = Vec::new();
                for constraint in &group.constraints {
                    let id_hash = ContentHash::of_str(&format!("{}", constraint.id));
                    let node = ConstraintNodeData {
                        id: constraint.id.clone(),
                        label: constraint.label.clone(),
                        expr: constraint.expr.clone(),
                        content_hash: id_hash.combine(constraint.expr.content_hash),
                        optimized_target: constraint.optimized_target.clone(),
                    };
                    graph.constraints.insert(constraint.id.clone(), node);
                    constraint_ids.push(constraint.id.clone());
                }

                // 4. Create ValueCellNodes for else_members
                let mut else_member_ids = Vec::new();
                for cell in &group.else_members {
                    let id_hash = ContentHash::of_str(&format!("{}", cell.id));
                    let expr_hash = cell
                        .default_expr
                        .as_ref()
                        .map(|e| e.content_hash)
                        .unwrap_or(ContentHash(0));
                    let node = ValueCellNode {
                        id: cell.id.clone(),
                        kind: cell.kind,
                        cell_type: cell.cell_type.clone(),
                        default_expr: cell.default_expr.clone(),
                        content_hash: id_hash.combine(expr_hash),
                    };
                    graph.value_cells.insert(cell.id.clone(), node);
                    else_member_ids.push(cell.id.clone());
                }

                // 5. Create ConstraintNodeData for else constraints
                let mut else_constraint_ids = Vec::new();
                for constraint in &group.else_constraints {
                    let id_hash = ContentHash::of_str(&format!("{}", constraint.id));
                    let node = ConstraintNodeData {
                        id: constraint.id.clone(),
                        label: constraint.label.clone(),
                        expr: constraint.expr.clone(),
                        content_hash: id_hash.combine(constraint.expr.content_hash),
                        optimized_target: constraint.optimized_target.clone(),
                    };
                    graph.constraints.insert(constraint.id.clone(), node);
                    else_constraint_ids.push(constraint.id.clone());
                }

                // 6. Store GuardedGroupInfo
                graph.guarded_groups.push(GuardedGroupInfo {
                    guard_cell: guard_id.clone(),
                    members: member_ids,
                    constraints: constraint_ids,
                    else_members: else_member_ids,
                    else_constraints: else_constraint_ids,
                });

                // 7. Add guard cell to structure_controlling
                graph.structure_controlling.insert(guard_id.clone());
            }
        }

        graph
    }

    /// Returns `true` iff `id` refers to a value cell present in this graph
    /// whose `kind` is `Auto` (strict or free). Returns `false` for missing
    /// cells — callers relying on that "missing → false" branch (e.g. the
    /// guard-deactivation helper) get the same semantics as the prior
    /// `value_cells.get(id).is_some_and(|n| n.kind.is_auto())` form.
    pub(crate) fn is_auto_cell(&self, id: &ValueCellId) -> bool {
        self.value_cells.get(id).is_some_and(|n| n.kind.is_auto())
    }

    /// Determine which constraint IDs are active given the current values.
    ///
    /// For each guarded group, inspects the guard cell's value:
    /// - true: group.constraints are active
    /// - false: group.else_constraints are active
    /// - Undef/other: neither branch is active
    ///
    /// Constraints not referenced in any guarded group are always active.
    pub fn active_constraint_ids(&self, values: &ValueMap) -> HashSet<ConstraintNodeId> {
        // Collect all constraint IDs that are under some guard
        let mut guarded_ids: HashSet<ConstraintNodeId> = HashSet::new();
        let mut active_ids: HashSet<ConstraintNodeId> = HashSet::new();

        for group in &self.guarded_groups {
            for cid in &group.constraints {
                guarded_ids.insert(cid.clone());
            }
            for cid in &group.else_constraints {
                guarded_ids.insert(cid.clone());
            }

            let guard_val = values.get(&group.guard_cell);
            match guard_val {
                Some(Value::Bool(true)) => {
                    for cid in &group.constraints {
                        active_ids.insert(cid.clone());
                    }
                }
                Some(Value::Bool(false)) => {
                    for cid in &group.else_constraints {
                        active_ids.insert(cid.clone());
                    }
                }
                _ => {
                    // Undef or non-Bool: neither branch active
                }
            }
        }

        // All unguarded constraints are always active
        for (cid, _) in self.constraints.iter() {
            if !guarded_ids.contains(cid) {
                active_ids.insert(cid.clone());
            }
        }

        active_ids
    }

    /// Compute a deterministic fingerprint of the graph topology.
    ///
    /// Computes three per-type sub-hashes (value_cells, constraints, realizations)
    /// independently, then combines them in fixed order. This ensures:
    /// - Determinism regardless of PersistentMap iteration order (via sorting)
    /// - Domain separation: a value_cell with hash H won't alias with a constraint of hash H
    pub fn topology_fingerprint(&self) -> ContentHash {
        let vc_hash = {
            let mut hashes: Vec<ContentHash> = self
                .value_cells
                .iter()
                .map(|(_, n)| n.content_hash)
                .collect();
            hashes.sort_by_key(|h| h.0);
            ContentHash::combine_all(hashes)
        };
        let cn_hash = {
            let mut hashes: Vec<ContentHash> = self
                .constraints
                .iter()
                .map(|(_, n)| n.content_hash)
                .collect();
            hashes.sort_by_key(|h| h.0);
            ContentHash::combine_all(hashes)
        };
        let real_hash = {
            let mut hashes: Vec<ContentHash> = self
                .realizations
                .iter()
                .map(|(_, n)| n.content_hash)
                .collect();
            hashes.sort_by_key(|h| h.0);
            ContentHash::combine_all(hashes)
        };
        let res_hash = {
            let mut hashes: Vec<ContentHash> = self
                .resolutions
                .iter()
                .map(|(_, n)| n.content_hash)
                .collect();
            hashes.sort_by_key(|h| h.0);
            ContentHash::combine_all(hashes)
        };

        let guard_hash = {
            let mut per_group: Vec<ContentHash> = self
                .guarded_groups
                .iter()
                .map(|g| {
                    let guard_id_hash = ContentHash::of_str(&format!("{}", g.guard_cell));
                    let mut member_strs: Vec<String> =
                        g.members.iter().map(|m| format!("{}", m)).collect();
                    member_strs.sort();
                    let member_hashes: Vec<ContentHash> =
                        member_strs.iter().map(|s| ContentHash::of_str(s)).collect();
                    let members_hash = ContentHash::combine_all(member_hashes);
                    let mut else_strs: Vec<String> =
                        g.else_members.iter().map(|m| format!("{}", m)).collect();
                    else_strs.sort();
                    let else_hashes: Vec<ContentHash> =
                        else_strs.iter().map(|s| ContentHash::of_str(s)).collect();
                    let else_hash = ContentHash::combine_all(else_hashes);
                    ContentHash::combine_all([guard_id_hash, members_hash, else_hash])
                })
                .collect();
            per_group.sort_by_key(|h| h.0);
            ContentHash::combine_all(per_group)
        };

        ContentHash::combine_all([vc_hash, cn_hash, real_hash, res_hash, guard_hash])
    }
}

#[cfg(test)]
mod tests {
    use reify_compiler::{CompiledGeometryOp, PrimitiveKind, ValueCellKind};
    use reify_types::{
        CompiledExpr, ConstraintNodeId, ContentHash, RealizationNodeId, Type, Value, ValueCellId,
    };

    use super::*;

    #[test]
    fn value_cell_node_construction() {
        let id = ValueCellId::new("Bracket", "width");
        let node = ValueCellNode {
            id: id.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            content_hash: ContentHash::of_str("width"),
        };

        assert_eq!(node.id, id);
        assert_eq!(node.kind, ValueCellKind::Param);
        assert_eq!(node.cell_type, Type::length());
        assert!(node.default_expr.is_some());
        assert_eq!(node.content_hash, ContentHash::of_str("width"));
    }

    #[test]
    fn value_cell_node_let_kind() {
        let id = ValueCellId::new("Bracket", "volume");
        let node = ValueCellNode {
            id: id.clone(),
            kind: ValueCellKind::Let,
            cell_type: Type::Real,
            default_expr: None,
            content_hash: ContentHash::of_str("volume"),
        };

        assert_eq!(node.kind, ValueCellKind::Let);
        assert!(node.default_expr.is_none());
    }

    #[test]
    fn value_cell_node_debug_and_clone() {
        let node = ValueCellNode {
            id: ValueCellId::new("Bracket", "width"),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: None,
            content_hash: ContentHash::of_str("width"),
        };

        let debug = format!("{:?}", node);
        assert!(debug.contains("ValueCellNode"));

        let cloned = node.clone();
        assert_eq!(cloned.id, node.id);
        assert_eq!(cloned.kind, node.kind);
    }

    #[test]
    fn constraint_node_data_construction() {
        let id = ConstraintNodeId::new("Bracket", 0);
        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let hash = ContentHash::of_str("constraint0");

        let node = ConstraintNodeData {
            id: id.clone(),
            label: None,
            expr: expr.clone(),
            content_hash: hash,
            optimized_target: None,
        };

        assert_eq!(node.id, id);
        assert_eq!(node.content_hash, hash);
        let debug = format!("{:?}", node);
        assert!(debug.contains("ConstraintNodeData"));

        let cloned = node.clone();
        assert_eq!(cloned.id, node.id);
        assert_eq!(cloned.content_hash, node.content_hash);
    }

    #[test]
    fn realization_node_data_construction() {
        let id = RealizationNodeId::new("Bracket", 0);
        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                (
                    "width".to_string(),
                    CompiledExpr::literal(Value::length(0.08), Type::length()),
                ),
                (
                    "height".to_string(),
                    CompiledExpr::literal(Value::length(0.10), Type::length()),
                ),
                (
                    "depth".to_string(),
                    CompiledExpr::literal(Value::length(0.005), Type::length()),
                ),
            ],
        }];
        let hash = ContentHash::of_str("realization0");

        let node = RealizationNodeData {
            id: id.clone(),
            operations: ops,
            content_hash: hash,
        };

        assert_eq!(node.id, id);
        assert_eq!(node.operations.len(), 1);
        assert_eq!(node.content_hash, hash);

        let debug = format!("{:?}", node);
        assert!(debug.contains("RealizationNodeData"));

        let cloned = node.clone();
        assert_eq!(cloned.id, node.id);
        assert_eq!(cloned.operations.len(), 1);
    }

    #[test]
    fn resolution_node_data_construction() {
        use reify_types::ResolutionNodeId;

        let id = ResolutionNodeId::new("Bracket", 0);
        let auto_params = vec![ValueCellId::new("Bracket", "x")];
        let constraint_deps = vec![ConstraintNodeId::new("Bracket", 0)];
        let hash = ContentHash::of_str("res0");

        let node = ResolutionNodeData {
            id: id.clone(),
            scope: "Bracket".to_string(),
            auto_params: auto_params.clone(),
            constraint_deps: constraint_deps.clone(),
            content_hash: hash,
        };

        assert_eq!(node.id, id);
        assert_eq!(node.scope, "Bracket");
        assert_eq!(node.auto_params, auto_params);
        assert_eq!(node.constraint_deps, constraint_deps);
        assert_eq!(node.content_hash, hash);

        // Test Debug derive
        let debug = format!("{:?}", node);
        assert!(debug.contains("ResolutionNodeData"));

        // Test Clone derive
        let cloned = node.clone();
        assert_eq!(cloned.id, node.id);
        assert_eq!(cloned.scope, node.scope);
        assert_eq!(cloned.auto_params, node.auto_params);
        assert_eq!(cloned.constraint_deps, node.constraint_deps);
    }

    #[test]
    fn evaluation_graph_has_resolutions_map() {
        let graph = EvaluationGraph::default();
        assert!(graph.resolutions.is_empty());
        assert_eq!(graph.resolutions.len(), 0);
    }

    #[test]
    fn evaluation_graph_empty() {
        let graph = EvaluationGraph::default();
        assert!(graph.value_cells.is_empty());
        assert!(graph.constraints.is_empty());
        assert!(graph.realizations.is_empty());
        assert!(graph.resolutions.is_empty());
        assert_eq!(graph.value_cells.len(), 0);
    }

    #[test]
    fn evaluation_graph_insert_and_get() {
        let mut graph = EvaluationGraph::default();

        let vcid = ValueCellId::new("Bracket", "width");
        let node = ValueCellNode {
            id: vcid.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: None,
            content_hash: ContentHash::of_str("width"),
        };
        graph.value_cells.insert(vcid.clone(), node);
        assert_eq!(graph.value_cells.len(), 1);
        assert!(graph.value_cells.get(&vcid).is_some());

        let cnid = ConstraintNodeId::new("Bracket", 0);
        let cnode = ConstraintNodeData {
            id: cnid.clone(),
            label: None,
            expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
            content_hash: ContentHash::of_str("c0"),
            optimized_target: None,
        };
        graph.constraints.insert(cnid.clone(), cnode);
        assert_eq!(graph.constraints.len(), 1);

        let rnid = RealizationNodeId::new("Bracket", 0);
        let rnode = RealizationNodeData {
            id: rnid.clone(),
            operations: vec![],
            content_hash: ContentHash::of_str("r0"),
        };
        graph.realizations.insert(rnid.clone(), rnode);
        assert_eq!(graph.realizations.len(), 1);
    }

    #[test]
    fn evaluation_graph_clone_independence() {
        let mut graph = EvaluationGraph::default();
        let vcid = ValueCellId::new("Bracket", "width");
        graph.value_cells.insert(
            vcid.clone(),
            ValueCellNode {
                id: vcid.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: ContentHash::of_str("width"),
            },
        );

        let mut cloned = graph.clone();
        let vcid2 = ValueCellId::new("Bracket", "height");
        cloned.value_cells.insert(
            vcid2.clone(),
            ValueCellNode {
                id: vcid2.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: ContentHash::of_str("height"),
            },
        );

        // Original unchanged
        assert_eq!(graph.value_cells.len(), 1);
        assert!(!graph.value_cells.contains_key(&vcid2));

        // Clone has both
        assert_eq!(cloned.value_cells.len(), 2);
        assert!(cloned.value_cells.contains_key(&vcid2));
    }

    #[test]
    fn evaluation_graph_from_templates() {
        use reify_test_support::{TopologyTemplateBuilder, gt, literal, lt, value_ref};

        let template = TopologyTemplateBuilder::new("Bracket")
            .param(
                "Bracket",
                "width",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            )
            .param(
                "Bracket",
                "height",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.10), Type::length())),
            )
            .let_binding(
                "Bracket",
                "volume",
                Type::Real,
                CompiledExpr::literal(Value::Real(0.0), Type::Real),
            )
            .constraint(
                "Bracket",
                0,
                None,
                gt(value_ref("Bracket", "width"), literal(Value::length(0.01))),
            )
            .constraint(
                "Bracket",
                1,
                Some("max_height"),
                lt(value_ref("Bracket", "height"), literal(Value::length(1.0))),
            )
            .build();

        let graph = EvaluationGraph::from_templates(&[template]);

        // 2 params + 1 let = 3 value cells
        assert_eq!(graph.value_cells.len(), 3);
        assert!(
            graph
                .value_cells
                .get(&ValueCellId::new("Bracket", "width"))
                .is_some()
        );
        assert!(
            graph
                .value_cells
                .get(&ValueCellId::new("Bracket", "height"))
                .is_some()
        );
        assert!(
            graph
                .value_cells
                .get(&ValueCellId::new("Bracket", "volume"))
                .is_some()
        );

        // Check kinds
        let width_node = graph
            .value_cells
            .get(&ValueCellId::new("Bracket", "width"))
            .unwrap();
        assert_eq!(width_node.kind, ValueCellKind::Param);
        let vol_node = graph
            .value_cells
            .get(&ValueCellId::new("Bracket", "volume"))
            .unwrap();
        assert_eq!(vol_node.kind, ValueCellKind::Let);

        // 2 constraints
        assert_eq!(graph.constraints.len(), 2);
        assert!(
            graph
                .constraints
                .get(&ConstraintNodeId::new("Bracket", 0))
                .is_some()
        );
        assert!(
            graph
                .constraints
                .get(&ConstraintNodeId::new("Bracket", 1))
                .is_some()
        );

        // 0 realizations (none added via builder)
        assert_eq!(graph.realizations.len(), 0);
    }

    #[test]
    fn topology_fingerprint_same_structure_same_hash() {
        use reify_test_support::{TopologyTemplateBuilder, gt, literal, value_ref};

        let template1 = TopologyTemplateBuilder::new("A")
            .param(
                "A",
                "x",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            )
            .constraint(
                "A",
                0,
                None,
                gt(value_ref("A", "x"), literal(Value::length(0.0))),
            )
            .build();
        let template2 = TopologyTemplateBuilder::new("A")
            .param(
                "A",
                "x",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            )
            .constraint(
                "A",
                0,
                None,
                gt(value_ref("A", "x"), literal(Value::length(0.0))),
            )
            .build();

        let g1 = EvaluationGraph::from_templates(&[template1]);
        let g2 = EvaluationGraph::from_templates(&[template2]);

        assert_eq!(g1.topology_fingerprint(), g2.topology_fingerprint());
    }

    #[test]
    fn topology_fingerprint_different_structure_different_hash() {
        use reify_test_support::{TopologyTemplateBuilder, gt, literal, value_ref};

        let template1 = TopologyTemplateBuilder::new("A")
            .param(
                "A",
                "x",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            )
            .build();
        let template2 = TopologyTemplateBuilder::new("A")
            .param(
                "A",
                "x",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            )
            .constraint(
                "A",
                0,
                None,
                gt(value_ref("A", "x"), literal(Value::length(0.0))),
            )
            .build();

        let g1 = EvaluationGraph::from_templates(&[template1]);
        let g2 = EvaluationGraph::from_templates(&[template2]);

        assert_ne!(g1.topology_fingerprint(), g2.topology_fingerprint());
    }

    #[test]
    fn topology_fingerprint_order_independent() {
        // Insert same nodes in different order, should produce same fingerprint
        let mut g1 = EvaluationGraph::default();
        let mut g2 = EvaluationGraph::default();

        let a = ValueCellId::new("X", "a");
        let b = ValueCellId::new("X", "b");
        let node_a = ValueCellNode {
            id: a.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: None,
            content_hash: ContentHash::of_str("a"),
        };
        let node_b = ValueCellNode {
            id: b.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: None,
            content_hash: ContentHash::of_str("b"),
        };

        // Different insertion order
        g1.value_cells.insert(a.clone(), node_a.clone());
        g1.value_cells.insert(b.clone(), node_b.clone());

        g2.value_cells.insert(b.clone(), node_b);
        g2.value_cells.insert(a.clone(), node_a);

        assert_eq!(g1.topology_fingerprint(), g2.topology_fingerprint());
    }

    #[test]
    fn content_hash_includes_node_id_for_value_cells() {
        use reify_test_support::TopologyTemplateBuilder;

        // Two params with different IDs but identical default expressions
        let template_a = TopologyTemplateBuilder::new("A")
            .param(
                "A",
                "width",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            )
            .build();
        let template_b = TopologyTemplateBuilder::new("A")
            .param(
                "A",
                "height",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            )
            .build();

        let graph_a = EvaluationGraph::from_templates(&[template_a]);
        let graph_b = EvaluationGraph::from_templates(&[template_b]);

        let hash_width = graph_a
            .value_cells
            .get(&ValueCellId::new("A", "width"))
            .unwrap()
            .content_hash;
        let hash_height = graph_b
            .value_cells
            .get(&ValueCellId::new("A", "height"))
            .unwrap()
            .content_hash;

        // Different IDs with same expression must produce different content hashes
        assert_ne!(
            hash_width, hash_height,
            "content_hash must incorporate node ID"
        );
    }

    #[test]
    fn content_hash_includes_node_id_for_constraints() {
        use reify_test_support::{TopologyTemplateBuilder, gt, literal, value_ref};

        let expr = gt(value_ref("A", "x"), literal(Value::length(0.0)));
        let template = TopologyTemplateBuilder::new("A")
            .param("A", "x", Type::length(), None)
            .constraint("A", 0, None, expr.clone())
            .constraint("A", 1, None, expr)
            .build();

        let graph = EvaluationGraph::from_templates(&[template]);

        let hash_0 = graph
            .constraints
            .get(&ConstraintNodeId::new("A", 0))
            .unwrap()
            .content_hash;
        let hash_1 = graph
            .constraints
            .get(&ConstraintNodeId::new("A", 1))
            .unwrap()
            .content_hash;

        // Different constraint IDs with same expression must produce different content hashes
        assert_ne!(
            hash_0, hash_1,
            "content_hash must incorporate constraint node ID"
        );
    }

    #[test]
    fn content_hash_no_expr_value_cell_uses_id() {
        use reify_test_support::TopologyTemplateBuilder;

        // A param with no default_expr should still have a non-zero content_hash derived from its ID
        let template = TopologyTemplateBuilder::new("A")
            .param("A", "x", Type::length(), None)
            .build();

        let graph = EvaluationGraph::from_templates(&[template]);
        let node = graph.value_cells.get(&ValueCellId::new("A", "x")).unwrap();

        assert_ne!(
            node.content_hash,
            ContentHash(0),
            "param without default_expr should have non-zero content_hash"
        );
    }

    #[test]
    fn from_templates_with_realizations() {
        use reify_test_support::TopologyTemplateBuilder;

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                (
                    "width".to_string(),
                    CompiledExpr::literal(Value::length(0.08), Type::length()),
                ),
                (
                    "height".to_string(),
                    CompiledExpr::literal(Value::length(0.10), Type::length()),
                ),
                (
                    "depth".to_string(),
                    CompiledExpr::literal(Value::length(0.005), Type::length()),
                ),
            ],
        }];

        let template = TopologyTemplateBuilder::new("A")
            .param(
                "A",
                "w",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            )
            .realization("A", 0, ops.clone())
            .build();

        let graph = EvaluationGraph::from_templates(&[template]);

        // Realization should be populated
        assert_eq!(graph.realizations.len(), 1);
        let r_node = graph
            .realizations
            .get(&RealizationNodeId::new("A", 0))
            .unwrap();
        assert_eq!(r_node.id, RealizationNodeId::new("A", 0));
        assert_eq!(r_node.operations.len(), 1);

        // Verify content_hash matches manually computed value:
        // id_hash.combine(ops_hash)
        let expected_id_hash = ContentHash::of_str(&format!("{}", RealizationNodeId::new("A", 0)));
        let expected_ops_hash = ContentHash::combine_all(
            ops.iter()
                .map(|op| ContentHash::of_str(&format!("{:?}", op))),
        );
        let expected_hash = expected_id_hash.combine(expected_ops_hash);
        assert_eq!(
            r_node.content_hash, expected_hash,
            "realization content_hash should be id_hash.combine(ops_hash)"
        );
        assert_ne!(r_node.content_hash, ContentHash(0));
    }

    #[test]
    fn fingerprint_domain_separates_node_types() {
        // graph_a has a value_cell with hash H, graph_b has a constraint with hash H
        let hash_h = ContentHash::of_str("same");

        let mut graph_a = EvaluationGraph::default();
        graph_a.value_cells.insert(
            ValueCellId::new("X", "a"),
            ValueCellNode {
                id: ValueCellId::new("X", "a"),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: hash_h,
            },
        );

        let mut graph_b = EvaluationGraph::default();
        graph_b.constraints.insert(
            ConstraintNodeId::new("X", 0),
            ConstraintNodeData {
                id: ConstraintNodeId::new("X", 0),
                label: None,
                expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                content_hash: hash_h,
                optimized_target: None,
            },
        );

        assert_ne!(
            graph_a.topology_fingerprint(),
            graph_b.topology_fingerprint(),
            "fingerprint must domain-separate value_cells from constraints"
        );
    }

    #[test]
    fn fingerprint_domain_separates_all_three_types() {
        let hash_h = ContentHash::of_str("same");

        let mut graph_a = EvaluationGraph::default();
        graph_a.value_cells.insert(
            ValueCellId::new("X", "a"),
            ValueCellNode {
                id: ValueCellId::new("X", "a"),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: hash_h,
            },
        );

        let mut graph_b = EvaluationGraph::default();
        graph_b.constraints.insert(
            ConstraintNodeId::new("X", 0),
            ConstraintNodeData {
                id: ConstraintNodeId::new("X", 0),
                label: None,
                expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                content_hash: hash_h,
                optimized_target: None,
            },
        );

        let mut graph_c = EvaluationGraph::default();
        graph_c.realizations.insert(
            RealizationNodeId::new("X", 0),
            RealizationNodeData {
                id: RealizationNodeId::new("X", 0),
                operations: vec![],
                content_hash: hash_h,
            },
        );

        let fp_a = graph_a.topology_fingerprint();
        let fp_b = graph_b.topology_fingerprint();
        let fp_c = graph_c.topology_fingerprint();

        // All three must be pairwise distinct
        assert_ne!(
            fp_a, fp_b,
            "value_cell vs constraint fingerprints must differ"
        );
        assert_ne!(
            fp_a, fp_c,
            "value_cell vs realization fingerprints must differ"
        );
        assert_ne!(
            fp_b, fp_c,
            "constraint vs realization fingerprints must differ"
        );
    }

    #[test]
    fn evaluation_graph_resolution_clone_independence() {
        use reify_types::ResolutionNodeId;

        let mut graph = EvaluationGraph::default();
        let r0_id = ResolutionNodeId::new("A", 0);
        graph.resolutions.insert(
            r0_id.clone(),
            ResolutionNodeData {
                id: r0_id.clone(),
                scope: "A".to_string(),
                auto_params: vec![ValueCellId::new("A", "x")],
                constraint_deps: vec![],
                content_hash: ContentHash::of_str("r0"),
            },
        );

        let mut cloned = graph.clone();
        let r1_id = ResolutionNodeId::new("A", 1);
        cloned.resolutions.insert(
            r1_id.clone(),
            ResolutionNodeData {
                id: r1_id.clone(),
                scope: "A".to_string(),
                auto_params: vec![ValueCellId::new("A", "y")],
                constraint_deps: vec![],
                content_hash: ContentHash::of_str("r1"),
            },
        );

        // Original unchanged
        assert_eq!(graph.resolutions.len(), 1);
        assert!(!graph.resolutions.contains_key(&r1_id));

        // Clone has both
        assert_eq!(cloned.resolutions.len(), 2);
        assert!(cloned.resolutions.contains_key(&r0_id));
        assert!(cloned.resolutions.contains_key(&r1_id));
    }

    #[test]
    fn topology_fingerprint_includes_resolutions() {
        use reify_test_support::TopologyTemplateBuilder;
        use reify_types::{CompiledExpr, ResolutionNodeId, Type, Value};

        // Build two identical graphs from same template
        let template1 = TopologyTemplateBuilder::new("A")
            .param(
                "A",
                "x",
                Type::Real,
                Some(CompiledExpr::literal(Value::Real(1.0), Type::Real)),
            )
            .build();
        let template2 = TopologyTemplateBuilder::new("A")
            .param(
                "A",
                "x",
                Type::Real,
                Some(CompiledExpr::literal(Value::Real(1.0), Type::Real)),
            )
            .build();

        let g1 = EvaluationGraph::from_templates(&[template1]);
        let mut g2 = EvaluationGraph::from_templates(&[template2]);

        // Before adding resolution, fingerprints should be equal
        assert_eq!(g1.topology_fingerprint(), g2.topology_fingerprint());

        // Add a ResolutionNodeData to g2
        let r0_id = ResolutionNodeId::new("A", 0);
        g2.resolutions.insert(
            r0_id.clone(),
            ResolutionNodeData {
                id: r0_id,
                scope: "A".to_string(),
                auto_params: vec![ValueCellId::new("A", "x")],
                constraint_deps: vec![],
                content_hash: ContentHash::of_str("r0"),
            },
        );

        // After adding resolution, fingerprints must differ
        assert_ne!(
            g1.topology_fingerprint(),
            g2.topology_fingerprint(),
            "fingerprint must change when resolution node is added"
        );

        // Two graphs with identical resolutions should have same fingerprint
        let mut g3 = g1.clone();
        let r0_id2 = ResolutionNodeId::new("A", 0);
        g3.resolutions.insert(
            r0_id2.clone(),
            ResolutionNodeData {
                id: r0_id2,
                scope: "A".to_string(),
                auto_params: vec![ValueCellId::new("A", "x")],
                constraint_deps: vec![],
                content_hash: ContentHash::of_str("r0"),
            },
        );
        assert_eq!(g2.topology_fingerprint(), g3.topology_fingerprint());
    }

    #[test]
    fn fingerprint_domain_separates_resolution_from_others() {
        let hash_h = ContentHash::of_str("same");
        use reify_types::ResolutionNodeId;

        let mut graph_a = EvaluationGraph::default();
        graph_a.value_cells.insert(
            ValueCellId::new("X", "a"),
            ValueCellNode {
                id: ValueCellId::new("X", "a"),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: hash_h,
            },
        );

        let mut graph_d = EvaluationGraph::default();
        graph_d.resolutions.insert(
            ResolutionNodeId::new("X", 0),
            ResolutionNodeData {
                id: ResolutionNodeId::new("X", 0),
                scope: "X".to_string(),
                auto_params: vec![],
                constraint_deps: vec![],
                content_hash: hash_h,
            },
        );

        assert_ne!(
            graph_a.topology_fingerprint(),
            graph_d.topology_fingerprint(),
            "fingerprint must domain-separate value_cells from resolutions"
        );
    }

    #[test]
    fn sub_component_nodes_in_evaluation_graph() {
        use reify_test_support::TopologyTemplateBuilder;
        use reify_types::{BinOp, CompiledExpr, Type, Value};

        // Child: param height, let half_h = height / 2
        let height_ref =
            || CompiledExpr::value_ref(ValueCellId::new("Child", "height"), Type::length());
        let half_h_expr = CompiledExpr::binop(
            BinOp::Div,
            height_ref(),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            Type::length(),
        );
        let child = TopologyTemplateBuilder::new("Child")
            .param(
                "Child",
                "height",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
            )
            .let_binding("Child", "half_h", Type::length(), half_h_expr)
            .build();

        // Parent: param width, sub rib = Child(height: width * 0.5)
        let width_ref =
            || CompiledExpr::value_ref(ValueCellId::new("Parent", "width"), Type::length());
        let arg_expr = CompiledExpr::binop(
            BinOp::Mul,
            width_ref(),
            CompiledExpr::literal(Value::Real(0.5), Type::Real),
            Type::length(),
        );
        let parent = TopologyTemplateBuilder::new("Parent")
            .param(
                "Parent",
                "width",
                Type::length(),
                Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            )
            .sub_component("rib", "Child", vec![("height".to_string(), arg_expr)])
            .build();

        let graph = EvaluationGraph::from_templates(&[child, parent]);

        // Should have scoped entries for sub-component
        let scoped_height = ValueCellId::new("Parent.rib", "height");
        let scoped_half_h = ValueCellId::new("Parent.rib", "half_h");

        assert!(
            graph.value_cells.get(&scoped_height).is_some(),
            "graph should contain scoped Parent.rib.height node"
        );
        assert!(
            graph.value_cells.get(&scoped_half_h).is_some(),
            "graph should contain scoped Parent.rib.half_h node"
        );

        // Verify kinds are preserved
        let h_node = graph.value_cells.get(&scoped_height).unwrap();
        assert_eq!(h_node.kind, ValueCellKind::Param);
        let hh_node = graph.value_cells.get(&scoped_half_h).unwrap();
        assert_eq!(hh_node.kind, ValueCellKind::Let);
    }

    /// Two graphs with identical value_cell nodes but different GuardedGroupInfo
    /// member bindings must produce different topology fingerprints.
    /// Exposes the bug where guard_hash only hashes the guard_cell ID string,
    /// ignoring member bindings.
    #[test]
    fn topology_fingerprint_distinguishes_guard_groupings() {
        let guard_cell = ValueCellId::new("S", "__guard_0");
        let x = ValueCellId::new("S", "x");
        let y = ValueCellId::new("S", "y");

        // Shared nodes: guard, x, y all present in both graphs
        let guard_node = ValueCellNode {
            id: guard_cell.clone(),
            kind: ValueCellKind::Let,
            cell_type: Type::Bool,
            default_expr: Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
            content_hash: ContentHash::of_str("guard_0"),
        };
        let x_node = ValueCellNode {
            id: x.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: Some(CompiledExpr::literal(Value::length(0.005), Type::length())),
            content_hash: ContentHash::of_str("x"),
        };
        let y_node = ValueCellNode {
            id: y.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
            content_hash: ContentHash::of_str("y"),
        };

        // Graph A: guard_cell guards [x], else_members=[]
        let mut graph_a = EvaluationGraph::default();
        graph_a
            .value_cells
            .insert(guard_cell.clone(), guard_node.clone());
        graph_a.value_cells.insert(x.clone(), x_node.clone());
        graph_a.value_cells.insert(y.clone(), y_node.clone());
        graph_a.guarded_groups.push(GuardedGroupInfo {
            guard_cell: guard_cell.clone(),
            members: vec![x.clone()],
            constraints: vec![],
            else_members: vec![],
            else_constraints: vec![],
        });

        // Graph B: guard_cell guards [y], else_members=[]
        let mut graph_b = EvaluationGraph::default();
        graph_b.value_cells.insert(guard_cell.clone(), guard_node);
        graph_b.value_cells.insert(x.clone(), x_node);
        graph_b.value_cells.insert(y.clone(), y_node);
        graph_b.guarded_groups.push(GuardedGroupInfo {
            guard_cell: guard_cell.clone(),
            members: vec![y.clone()],
            constraints: vec![],
            else_members: vec![],
            else_constraints: vec![],
        });

        assert_ne!(
            graph_a.topology_fingerprint(),
            graph_b.topology_fingerprint(),
            "fingerprints must differ when guard groups bind different members"
        );
    }

    #[test]
    fn is_auto_cell_predicate() {
        let auto_strict_id = ValueCellId::new("E", "auto_strict");
        let auto_free_id = ValueCellId::new("E", "auto_free");
        let param_id = ValueCellId::new("E", "param");
        let let_id = ValueCellId::new("E", "let_cell");

        let mut graph = EvaluationGraph::default();

        graph.value_cells.insert(
            auto_strict_id.clone(),
            ValueCellNode {
                id: auto_strict_id.clone(),
                kind: ValueCellKind::Auto { free: false },
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("auto_strict"),
            },
        );
        graph.value_cells.insert(
            auto_free_id.clone(),
            ValueCellNode {
                id: auto_free_id.clone(),
                kind: ValueCellKind::Auto { free: true },
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("auto_free"),
            },
        );
        graph.value_cells.insert(
            param_id.clone(),
            ValueCellNode {
                id: param_id.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("param"),
            },
        );
        graph.value_cells.insert(
            let_id.clone(),
            ValueCellNode {
                id: let_id.clone(),
                kind: ValueCellKind::Let,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("let_cell"),
            },
        );

        assert!(
            graph.is_auto_cell(&auto_strict_id),
            "Auto {{ free: false }} should be auto"
        );
        assert!(
            graph.is_auto_cell(&auto_free_id),
            "Auto {{ free: true }} should be auto"
        );
        assert!(!graph.is_auto_cell(&param_id), "Param should not be auto");
        assert!(!graph.is_auto_cell(&let_id), "Let should not be auto");
        assert!(
            !graph.is_auto_cell(&ValueCellId::new("X", "missing")),
            "Missing cell should return false"
        );
    }

    /// task-2629 step-6: `EvaluationGraph::from_templates` carries
    /// `TopologyTemplate.forall_templates` through to the graph for the
    /// runtime `engine_edit` collection-count phase to consume. Adding a
    /// `CompiledForallTemplate` to a template with no instantiated
    /// constraints does NOT change `topology_fingerprint` — the templates
    /// are runtime-only metadata; per-element constraints get hashed once
    /// they are emitted into `graph.constraints`.
    ///
    /// RED before step-7 (the `forall_templates` field on
    /// `EvaluationGraph` does not yet exist).
    #[test]
    fn evaluation_graph_carries_forall_templates() {
        use reify_compiler::{CompiledForallBody, CompiledForallTemplate};
        use reify_test_support::TopologyTemplateBuilder;
        use reify_types::SourceSpan;

        let body_expr = CompiledExpr::value_ref(
            ValueCellId::new("S.vents[0]", "mass"),
            Type::dimensionless_scalar(),
        );
        let ft = CompiledForallTemplate {
            variable: "v".to_string(),
            parent_entity: "S".to_string(),
            collection_sub_name: "vents".to_string(),
            count_cell: ValueCellId::new("S", "__count_vents"),
            span: SourceSpan::empty(0),
            body: CompiledForallBody::Constraint {
                body_expr: body_expr.clone(),
            },
        };

        let template = TopologyTemplateBuilder::new("S")
            .forall_template(ft.clone())
            .build();
        let graph = EvaluationGraph::from_templates(&[template]);

        // (a) The forall_templates field carries through.
        assert_eq!(
            graph.forall_templates.len(),
            1,
            "expected 1 forall template carried into graph",
        );
        let carried = &graph.forall_templates[0];
        assert_eq!(carried.variable, ft.variable);
        assert_eq!(carried.parent_entity, ft.parent_entity);
        assert_eq!(carried.collection_sub_name, ft.collection_sub_name);
        assert_eq!(carried.count_cell, ft.count_cell);

        // (b) topology_fingerprint is unchanged when only forall_templates
        // differs (the templates alone don't affect the constraint set;
        // per-element constraints become visible only at runtime emission).
        let template_no_ft = TopologyTemplateBuilder::new("S").build();
        let graph_no_ft = EvaluationGraph::from_templates(&[template_no_ft]);
        assert_eq!(
            graph.topology_fingerprint(),
            graph_no_ft.topology_fingerprint(),
            "topology_fingerprint should not change when only forall_templates differs (no instantiated constraints)",
        );
    }

    /// Task 2690 step-5/step-6: `template.connections` must carry through to
    /// `graph.connections` so the runtime forall-Connect re-emission path
    /// can mutate the `Vec<CompiledConnection>` in lockstep with the
    /// synthesised compatibility-constraint nodes in `graph.constraints`.
    ///
    /// RED before step-6 (the `connections` field on `EvaluationGraph` does
    /// not yet exist).
    #[test]
    fn evaluation_graph_carries_connections() {
        use reify_compiler::CompiledConnection;
        use reify_syntax::ConnectOp;
        use reify_test_support::TopologyTemplateBuilder;
        use reify_types::SourceSpan;

        let conn = CompiledConnection {
            left_port: "vents[0].inlet".to_string(),
            operator: ConnectOp::Forward,
            right_port: "air_channel".to_string(),
            connector_sub: None,
            compatibility_constraint: ConstraintNodeId::new("S", 0),
            port_mappings: Vec::new(),
            frame_constraint: None,
            span: SourceSpan::empty(0),
        };

        let template = TopologyTemplateBuilder::new("S")
            .connection(conn.clone())
            .build();
        let graph = EvaluationGraph::from_templates(&[template]);

        assert_eq!(
            graph.connections.len(),
            1,
            "expected 1 connection carried into graph",
        );
        let carried = &graph.connections[0];
        assert_eq!(carried.left_port, conn.left_port);
        assert_eq!(carried.right_port, conn.right_port);
        assert_eq!(carried.operator, conn.operator);
        assert_eq!(
            carried.compatibility_constraint, conn.compatibility_constraint,
        );
    }
}
