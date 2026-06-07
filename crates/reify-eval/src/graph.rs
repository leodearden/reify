// EvaluationGraph: typed graph nodes backed by PersistentMap.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use reify_compiler::{
    CompiledConnection, CompiledForallTemplate, CompiledGeometryOp, TopologyTemplate,
    ValueCellKind, find_template,
};
use reify_core::{
    ComputeNodeId, ConstraintNodeId, ContentHash, KernelId, RealizationNodeId, ResolutionNodeId,
    Type, ValueCellId,
};
use reify_ir::{CompiledExpr, OpaqueState, PersistentMap, ReprKind, Value, ValueMap};

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
    /// The repr-kind produced by the kernel adapter that last executed this realization.
    /// Initialized to `ReprKind::BRep` at graph-construction time (v0.2 OCCT-only baseline).
    /// Task ε (3436) will write the per-op dispatcher choice at execution time.
    /// NOT included in `content_hash` — this is dispatcher/cache metadata, not identity.
    pub produced_repr: ReprKind,
    /// GHR-δ (PRD geometry-handle-runtime.md §8 Phase 4): the value cell that
    /// holds this realization's `Value::GeometryHandle`, if any. Populated by
    /// [`EvaluationGraph::from_templates`] when a template has a `Type::Geometry`
    /// value cell whose member name matches this realization's name (the same
    /// rule GHR-γ uses in `post_process_geometry_handle_cells`). `None` for
    /// realizations with no backing geometry cell. Riding this link on the graph
    /// lets the trace builders record the Realization→ValueCell freshness edge in
    /// both directions without re-deriving the cell↔realization correspondence,
    /// which is otherwise lost (RealizationNodeData carries no name/member link).
    /// NOT included in `content_hash` — it is an evaluation-graph wiring detail,
    /// not realization identity.
    pub geometry_cell: Option<ValueCellId>,
    /// The kernel that produced the terminal geometry handle for this
    /// realization (task 4248, piece 3).  `None` until the realization has
    /// been executed at least once; set from the terminal `KernelHandle`
    /// at the two `node.produced_repr = repr` graph-write sites in
    /// `engine_build.rs`.  NOT included in `content_hash` — this is
    /// dispatcher/cache metadata, not realization identity.
    ///
    /// Production counterpart to the `#[cfg(test)]`-gated
    /// `Engine::test_terminal_handle`; see `Engine::realization_kernel_provenance`
    /// for the public read path.
    pub produced_kernel: Option<KernelId>,
}

/// Per-realization kernel provenance entry returned by
/// [`Engine::realization_kernel_provenance`] (task 4248, piece 3).
///
/// Sorted by `realization` id for deterministic CLI output; only realizations
/// whose terminal kernel is `Some` are included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealizationKernelProvenance {
    /// Stable string identifier for this realization (the `RealizationNodeId`
    /// string representation).
    pub realization: String,
    /// The repr-kind produced by the terminal kernel adapter.
    pub repr: ReprKind,
    /// The kernel that produced (and owns) the terminal geometry handle.
    pub kernel: KernelId,
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

/// Cooperative-cancellation handle for an in-flight ComputeNode dispatch.
///
/// A thin wrapper around `Arc<AtomicBool>`. Cloning shares the same
/// underlying flag, so cancelling via any clone propagates to all holders
/// (including graph snapshots taken mid-dispatch). See
/// `docs/prds/v0_3/compute-node-contract.md` §2 for the full contract.
///
/// Both `cancel()` and `is_cancelled()` use `Ordering::Relaxed`: the flag is
/// a one-shot monotonic signal (false → true; never resets within a handle's
/// lifetime). There is no other memory operation whose ordering needs to be
/// enforced relative to this flag, so stronger orderings buy nothing.
///
/// Module-private (not re-exported from `lib.rs`) until task γ (3422) adds
/// the dispatch-registry consumer and export.
#[derive(Debug, Clone)]
pub struct CancellationHandle {
    inner: Arc<AtomicBool>,
}

#[allow(clippy::new_without_default)] // Default intentionally omitted: keeps API minimal and leaves room to swap inner to a non-Default-able primitive (e.g. tokio_util::sync::CancellationToken) — see compute-node-contract.md §2
impl CancellationHandle {
    /// Create a new, non-cancelled handle.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signal cancellation. All clones of this handle will observe the change.
    pub fn cancel(&self) {
        self.inner.store(true, Ordering::Relaxed);
    }

    /// Returns `true` if `cancel()` has been called on this handle or any clone.
    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::Relaxed)
    }
}

/// A compute node in the evaluation graph.
/// Parallel to RealizationNodeData / ResolutionNodeData. See
/// `docs/prds/v0_3/compute-node-infrastructure.md` §"Struct shape" for
/// the field-by-field PRD spec; the exhaustive-destructure test
/// `compute_node_data_fields_match_prd_spec` pins this list at compile time.
#[derive(Debug)]
pub struct ComputeNodeData {
    // Identity
    pub computation_id: ComputeNodeId,
    pub target: String,
    // Inputs (drive cache key in P3.2)
    pub value_inputs: Vec<ValueCellId>,
    pub realization_inputs: Vec<RealizationNodeId>,
    pub options_hash: ContentHash,
    // Cache
    pub cache_key: ContentHash,
    pub cached_result: Option<Value>,
    pub result_content_hash: Option<ContentHash>,
    // Lifecycle
    pub opaque_state: Option<OpaqueState>,
    pub running: Option<CancellationHandle>,
    // Output side
    pub output_value_cells: Vec<ValueCellId>,
}

// Manual Clone mirroring `NodeCache::Clone` (cache.rs:171-183):
// OpaqueState is !Clone (Box<dyn Any + Send>), so we drop the slot to
// None on clone. Warm state is transient — best-effort recovery is the
// existing WarmStatePool contract.
//
// `running` is Arc-shared on clone: `CancellationHandle` wraps `Arc<AtomicBool>`,
// so `self.running.clone()` produces a second handle pointing at the same flag.
// A graph snapshot taken mid-dispatch therefore represents the same in-flight
// operation; cancelling via either snapshot propagates through the shared channel.
// (Decision recorded in task 3421: Arc-sharing is preferred over reset-to-None
// because resetting would silently orphan the cancellation channel, and
// cancel-on-clone is a footgun for callers that snapshot for read-only inspection.)
impl Clone for ComputeNodeData {
    fn clone(&self) -> Self {
        Self {
            computation_id: self.computation_id.clone(),
            target: self.target.clone(),
            value_inputs: self.value_inputs.clone(),
            realization_inputs: self.realization_inputs.clone(),
            options_hash: self.options_hash,
            cache_key: self.cache_key,
            cached_result: self.cached_result.clone(),
            result_content_hash: self.result_content_hash,
            opaque_state: None,
            running: self.running.clone(),
            output_value_cells: self.output_value_cells.clone(),
        }
    }
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
    /// Compute nodes (P3.1+). Keyed by ComputeNodeId.
    pub compute_nodes: PersistentMap<ComputeNodeId, ComputeNodeData>,
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

    /// Resolved `auto:` type-parameter substitution map for this graph.
    ///
    /// Each entry is `(param_name, concrete_template_name)`, e.g.
    /// `("T", "ORingSeal")`. Populated from
    /// `MultiParamResolutionOutcome.substitution` (same `Vec<(String, String)>`
    /// shape, no adapter needed; see `crates/reify-compiler/src/auto_type_param.rs`).
    ///
    /// **Hash stability:** mixed into `topology_fingerprint` as a seventh
    /// per-bucket sub-hash. Entries are sorted by `param_name` before hashing
    /// so the same logical map always produces the same fingerprint regardless
    /// of Vec insertion order (revert-stable: same candidate re-selected after
    /// a parameter edit + revert → same fingerprint → cache reuse).
    ///
    /// **Empty-Vec:** the default empty Vec contributes `ContentHash(0)`; this
    /// is deterministic but is NOT a no-op against the outer `combine_all` —
    /// adding this bucket shifts pre-existing fingerprints, which is acceptable
    /// since `topology_fingerprint` is an in-memory cache key only.
    ///
    /// **Invariant:** param names must be unique; duplicates are a producer bug.
    pub auto_type_substitution: Vec<(String, String)>,
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
                // GHR-δ S2: link this realization to the `Type::Geometry` value
                // cell it backs, if any — the same name-match rule GHR-γ applies
                // in `post_process_geometry_handle_cells`
                // (`cell.id.member == realization.name && cell_type == Geometry`).
                // Riding the link on the graph lets the trace builders record the
                // Realization→ValueCell freshness edge in both directions without
                // re-deriving the cell↔realization correspondence (which the eval
                // graph's RealizationNodeData otherwise drops).
                let geometry_cell = realization.name.as_deref().and_then(|name| {
                    template
                        .value_cells
                        .iter()
                        .find(|c| c.id.member == name && c.cell_type == Type::Geometry)
                        .map(|c| c.id.clone())
                });
                let node = RealizationNodeData {
                    id: realization.id.clone(),
                    operations: realization.operations.clone(),
                    content_hash: id_hash.combine(ops_hash),
                    // v0.2 default — OCCT-only baseline; task ε (3436) writes the
                    // per-op dispatcher choice at execution time.
                    produced_repr: ReprKind::BRep,
                    geometry_cell,
                    // Task 4248 piece 3: populated at execution time from the
                    // terminal KernelHandle; None until first execution.
                    produced_kernel: None,
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
                                if let reify_ir::CompiledExprKind::Literal(Value::Int(n)) =
                                    &expr.kind
                                {
                                    Some(*n)
                                } else {
                                    // For ValueRef expressions, look up the referenced cell's default
                                    if let reify_ir::CompiledExprKind::ValueRef(ref_id) = &expr.kind
                                    {
                                        template
                                            .value_cells
                                            .iter()
                                            .find(|vc| vc.id == *ref_id)
                                            .and_then(|vc| vc.default_expr.as_ref())
                                            .and_then(|e| {
                                                if let reify_ir::CompiledExprKind::Literal(
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

                        // task 3806 (γ) precedence rule: if the parent template
                        // already inserted an override cell for this scoped id
                        // (lines 284-298 above, from the parent's own value_cells
                        // — e.g. a `sub b : Bearing { bore = auto }` override
                        // emitted by entity.rs step-4), let that cell stand.
                        // Overwriting it with the child-default-derived node
                        // would change its `kind` from `Auto` to `Param`, which
                        // would cause `Snapshot::from_compiled_module` to
                        // initialise the cell as `Undetermined` instead of `Auto`
                        // and break incremental-eval cache-key invariants.
                        if graph.value_cells.contains_key(&scoped_id) {
                            continue;
                        }

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

    /// Insert a ComputeNode into the graph. Returns the node's `ComputeNodeId`
    /// (a clone of `data.computation_id`) for caller convenience.
    ///
    /// **Note (P3.1 scope):** ComputeNodes are not produced by any builder in
    /// P3.1 — `from_templates` does not construct them, and the
    /// `topology_fingerprint` does NOT yet include a ComputeNode bucket. P3.2
    /// composes `cache_key` and adds the fingerprint bucket; P3.4 wires
    /// `@optimized` lowering to call this method. See
    /// `docs/prds/v0_3/compute-node-infrastructure.md`.
    ///
    /// **Duplicate targets:** inserting a `ComputeNodeData` whose `target`
    /// matches an already-present node's `target` does NOT error — the
    /// underlying `PersistentMap` is keyed on `ComputeNodeId`, not on the
    /// `target` string. Callers that require target uniqueness must
    /// deduplicate before calling this method. P3.4's `@optimized` lowering
    /// is the natural deduplication point; see
    /// `docs/prds/v0_3/compute-node-infrastructure.md`.
    pub fn insert_compute_node(&mut self, data: ComputeNodeData) -> ComputeNodeId {
        let id = data.computation_id.clone();
        self.compute_nodes.insert(id.clone(), data);
        id
    }

    pub fn get_compute_node(&self, id: &ComputeNodeId) -> Option<&ComputeNodeData> {
        self.compute_nodes.get(id)
    }

    pub fn get_compute_node_mut(&mut self, id: &ComputeNodeId) -> Option<&mut ComputeNodeData> {
        self.compute_nodes.get_mut(id)
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

        // Task 2690/2713: connections bucket. Each connection contributes a
        // composite hash over its compatibility-constraint id, both port
        // names, the operator discriminant, the (sorted) port-mappings,
        // connector_sub, and frame_constraint. Note: span is intentionally
        // omitted (matching the convention used by other buckets). The
        // per-connection hashes are sorted by raw `.0` then combined,
        // mirroring the order-independence treatment used by the other
        // buckets above.
        let conn_hash = {
            let mut per_conn: Vec<ContentHash> = self
                .connections
                .iter()
                .map(|c| {
                    let cnid_hash = ContentHash::of_str(&format!("{}", c.compatibility_constraint));
                    let left_hash = ContentHash::of_str(&c.left_port);
                    let right_hash = ContentHash::of_str(&c.right_port);
                    let op_hash = ContentHash::of_str(&format!("op:{}", c.operator.as_u8()));
                    let mut pm_strs: Vec<String> = c
                        .port_mappings
                        .iter()
                        .map(|(l, r)| format!("{}->{}", l, r))
                        .collect();
                    pm_strs.sort();
                    let pm_hash =
                        ContentHash::combine_all(pm_strs.iter().map(|s| ContentHash::of_str(s)));
                    let connector_sub_hash = ContentHash::of_str(&match &c.connector_sub {
                        Some(s) => format!("connector_sub:some:{}", s),
                        None => "connector_sub:none".to_string(),
                    });
                    let frame_constraint_hash = ContentHash::of_str(&match &c.frame_constraint {
                        Some(id) => format!("frame_constraint:some:{}", id),
                        None => "frame_constraint:none".to_string(),
                    });
                    ContentHash::combine_all([
                        cnid_hash,
                        left_hash,
                        right_hash,
                        op_hash,
                        pm_hash,
                        connector_sub_hash,
                        frame_constraint_hash,
                    ])
                })
                .collect();
            per_conn.sort_by_key(|h| h.0);
            ContentHash::combine_all(per_conn)
        };

        // Task 2388 / PRD task 5 criterion 7: auto type-param substitution
        // bucket. Each (param_name, template_name) pair is hashed via a
        // domain-separated prefix `"auto:<p>=<t>"`, following the same
        // `"key:value"` convention used by the connections bucket above.
        //
        // The Vec is sorted by param_name BEFORE per-pair hashing so that
        // two graphs constructed with the same logical substitution map
        // but different insertion orders produce identical fingerprints.
        // Rationale: PRD criterion 7's second half ("same candidate re-selected
        // after a parameter edit + revert") requires logical map equality to
        // imply fingerprint equality regardless of Vec assembly order.
        // Mirrors the per-bucket sort applied by every other bucket above
        // (value_cells, constraints, realizations, resolutions sort by
        // ContentHash.0; guarded_groups sort by group ContentHash).
        // (revert-stable: same logical map → same fingerprint regardless
        // of source ordering — step-6 rationale for the sort.)
        //
        // step-3 contract: identical input Vecs (in identical order) produce
        // identical bucket hashes — enforced by ContentHash determinism.
        //
        // step-7 back-compat contract: an empty Vec contributes ContentHash(0)
        // deterministically via combine_all([]). This is NOT a no-op against
        // the outer combine_all — combine(x, ContentHash(0)) rehashes a 32-byte
        // concat (hash.rs:32-37), so adding this bucket shifts pre-existing
        // fingerprints. That shift is acceptable: topology_fingerprint is an
        // in-memory cache key only, not a persisted identifier.
        let auto_type_sub_hash = {
            // Invariant: param names must be unique (producer guarantee).
            debug_assert!(
                {
                    let mut seen = std::collections::HashSet::new();
                    self.auto_type_substitution
                        .iter()
                        .all(|(p, _)| seen.insert(p.as_str()))
                },
                "auto_type_substitution: param names must be unique; duplicates are a producer bug"
            );
            // In release builds the assert is elided; duplicate param names
            // produce a deterministic-but-undefined fingerprint (sorted pairs
            // hashed as-is) — callers must not rely on any particular result.
            // Sort input pairs by `param_name`, not by `.0` of the resulting
            // pair hashes (which is the convention used by sibling buckets).
            // Under the param-name-uniqueness invariant debug_assert!ed above,
            // both orderings satisfy the determinism contract (logical map
            // equality → bucket equality); the input-pair sort is preserved
            // as-is to avoid re-shuffling the existing bit-stable bucket output.
            let mut sorted = self.auto_type_substitution.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let pair_hashes: Vec<ContentHash> = sorted
                .iter()
                // `param_name` and `template_name` are identifier tokens
                // validated upstream; they cannot contain '=' or ':', so
                // the "auto:{p}={t}" domain-separated prefix is unambiguous
                // (no aliasing between distinct (param, template) pairs).
                .map(|(p, t)| ContentHash::of_str(&format!("auto:{}={}", p, t)))
                .collect();
            ContentHash::combine_all(pair_hashes)
        };

        ContentHash::combine_all([
            vc_hash,
            cn_hash,
            real_hash,
            res_hash,
            guard_hash,
            conn_hash,
            auto_type_sub_hash,
        ])
    }
}

#[cfg(test)]
mod tests {
    use reify_compiler::{CompiledGeometryOp, PrimitiveKind, ValueCellKind};
    use reify_core::{ConstraintNodeId, ContentHash, RealizationNodeId, Type, ValueCellId};
    use reify_ir::{CompiledExpr, Value};

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
            geometry_cell: None,
            id: id.clone(),
            operations: ops,
            content_hash: hash,
            produced_repr: reify_ir::ReprKind::BRep,
            produced_kernel: None,
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
    fn realization_node_data_carries_produced_repr_brep_default() {
        use reify_ir::ReprKind;
        use reify_test_support::TopologyTemplateBuilder;

        // from_templates must initialize produced_repr to ReprKind::BRep (v0.2 OCCT default).
        let template = TopologyTemplateBuilder::new("A")
            .realization("A", 0, vec![])
            .build();
        let graph = EvaluationGraph::from_templates(&[template]);
        let r_node = graph
            .realizations
            .get(&RealizationNodeId::new("A", 0))
            .unwrap();
        assert_eq!(
            r_node.produced_repr,
            ReprKind::BRep,
            "from_templates must initialize produced_repr to ReprKind::BRep \
             (v0.2 default; task ε (3436) wires the per-op dispatcher choice)"
        );
    }

    /// GHR-δ S1: `EvaluationGraph::from_templates` populates
    /// `RealizationNodeData.geometry_cell` with the `Type::Geometry` value cell
    /// whose member name matches the realization's name (the GHR-γ rule from
    /// `post_process_geometry_handle_cells`: `cell.id.member == realization.name
    /// && cell.cell_type == Type::Geometry`), and leaves it `None` for
    /// realizations with no backing geometry cell.
    ///
    /// RED until S2 wires the population in `from_templates`.
    #[test]
    fn from_templates_populates_realization_geometry_cell() {
        use reify_test_support::{bracket_compiled_module, parse_and_compile};

        // Some(cell): `param body : Solid = box(..)` compiles to a realization
        // named "body" backed by the Type::Geometry value cell Widget.body
        // (same fixture as geometry_handle_value_cell_e2e.rs).
        let module = parse_and_compile(
            r#"structure def Widget {
    param body : Solid = box(10mm, 20mm, 30mm)
}"#,
        );
        let graph = EvaluationGraph::from_templates(&module.templates);
        let widget_r0 = graph
            .realizations
            .get(&RealizationNodeId::new("Widget", 0))
            .expect("Widget realization #0 must exist");
        assert_eq!(
            widget_r0.geometry_cell,
            Some(ValueCellId::new("Widget", "body")),
            "a geometry-backed realization must link its Type::Geometry value cell"
        );

        // None: the bracket fixture has a realization (#0, a box) but no
        // Type::Geometry value cell — every member is Length/Scalar — so the
        // realization has no backing geometry cell.
        let bracket = bracket_compiled_module();
        let bgraph = EvaluationGraph::from_templates(&bracket.templates);
        let bracket_r0 = bgraph
            .realizations
            .get(&RealizationNodeId::new("Bracket", 0))
            .expect("Bracket realization #0 must exist");
        assert_eq!(
            bracket_r0.geometry_cell, None,
            "a realization with no Type::Geometry backing cell must have geometry_cell == None"
        );
    }

    #[test]
    fn resolution_node_data_construction() {
        use reify_core::ResolutionNodeId;

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
    fn compute_node_data_construction() {
        use reify_core::{ComputeNodeId, RealizationNodeId as RnId};

        let computation_id = ComputeNodeId::new("Bracket", 0);
        let data = ComputeNodeData {
            computation_id: computation_id.clone(),
            target: "solver::elastic_static".to_string(),
            value_inputs: vec![ValueCellId::new("Bracket", "load")],
            realization_inputs: vec![RnId::new("Bracket", 0)],
            options_hash: ContentHash::of_str("opts"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: Some(Value::Real(0.0)),
            result_content_hash: Some(ContentHash::of_str("rh")),
            opaque_state: None,
            running: None,
            output_value_cells: vec![ValueCellId::new("Bracket", "stress")],
        };

        assert_eq!(data.computation_id, computation_id);
        assert_eq!(data.target, "solver::elastic_static");
        assert_eq!(data.value_inputs.len(), 1);
        assert_eq!(data.value_inputs[0], ValueCellId::new("Bracket", "load"));
        assert_eq!(data.realization_inputs.len(), 1);
        assert_eq!(data.options_hash, ContentHash::of_str("opts"));
        assert_eq!(data.cache_key, ContentHash::of_str("ck"));
        assert!(data.cached_result.is_some());
        assert!(data.result_content_hash.is_some());
        assert!(data.opaque_state.is_none());
        assert!(data.running.is_none());
        assert_eq!(data.output_value_cells.len(), 1);
        assert_eq!(
            data.output_value_cells[0],
            ValueCellId::new("Bracket", "stress")
        );

        let debug = format!("{:?}", data);
        assert!(debug.contains("ComputeNodeData"));
    }

    #[test]
    fn compute_node_data_clone_drops_opaque_state() {
        use reify_core::{ComputeNodeId, RealizationNodeId as RnId};
        use reify_ir::OpaqueState;

        let data = ComputeNodeData {
            computation_id: ComputeNodeId::new("Bracket", 0),
            target: "solver::elastic_static".to_string(),
            value_inputs: vec![ValueCellId::new("Bracket", "load")],
            realization_inputs: vec![RnId::new("Bracket", 0)],
            options_hash: ContentHash::of_str("opts"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: Some(Value::Real(1.5)),
            result_content_hash: Some(ContentHash::of_str("rh")),
            opaque_state: Some(OpaqueState::new(42i32, 4)),
            running: Some(CancellationHandle::new()),
            output_value_cells: vec![ValueCellId::new("Bracket", "stress")],
        };

        let cloned = data.clone();

        // Manual-Clone contract: opaque_state is transient, dropped to None
        assert!(cloned.opaque_state.is_none());
        // CancellationHandle IS Clone (Arc-shared flag); running is preserved
        assert!(cloned.running.is_some());
        // Other fields are preserved
        assert_eq!(cloned.computation_id, ComputeNodeId::new("Bracket", 0));
        assert_eq!(cloned.target, "solver::elastic_static");
        assert_eq!(
            cloned.value_inputs,
            vec![ValueCellId::new("Bracket", "load")]
        );
        assert_eq!(cloned.options_hash, ContentHash::of_str("opts"));
        assert_eq!(cloned.cache_key, ContentHash::of_str("ck"));
    }

    #[test]
    fn compute_node_data_fields_match_prd_spec() {
        use reify_core::ComputeNodeId;

        let data = ComputeNodeData {
            computation_id: ComputeNodeId::new("Bracket", 0),
            target: "solver::elastic_static".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![],
            options_hash: ContentHash::of_str("opts"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![],
        };
        // Exhaustive destructure: adding/renaming/removing any field breaks compilation
        let ComputeNodeData {
            computation_id: _,
            target: _,
            value_inputs: _,
            realization_inputs: _,
            options_hash: _,
            cache_key: _,
            cached_result: _,
            result_content_hash: _,
            opaque_state: _,
            running: _,
            output_value_cells: _,
        } = data;
    }

    #[test]
    fn evaluation_graph_has_resolutions_map() {
        let graph = EvaluationGraph::default();
        assert!(graph.resolutions.is_empty());
        assert_eq!(graph.resolutions.len(), 0);
    }

    #[test]
    fn evaluation_graph_has_compute_nodes_map() {
        let graph = EvaluationGraph::default();
        assert!(graph.compute_nodes.is_empty());
        assert_eq!(graph.compute_nodes.len(), 0);
    }

    #[test]
    fn evaluation_graph_insert_compute_node_round_trip() {
        use reify_core::{ComputeNodeId, RealizationNodeId as RnId};
        let mut graph = EvaluationGraph::default();
        let computation_id = ComputeNodeId::new("Bracket", 0);
        let data = ComputeNodeData {
            computation_id: computation_id.clone(),
            target: "solver::elastic_static".to_string(),
            value_inputs: vec![ValueCellId::new("Bracket", "load")],
            realization_inputs: vec![RnId::new("Bracket", 0)],
            options_hash: ContentHash::of_str("opts"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![],
        };
        let id = graph.insert_compute_node(data);
        assert_eq!(id, computation_id);
        let got = graph.get_compute_node(&id).unwrap();
        assert_eq!(got.target, "solver::elastic_static");
        assert_eq!(got.value_inputs, vec![ValueCellId::new("Bracket", "load")]);
        assert_eq!(got.options_hash, ContentHash::of_str("opts"));
    }

    #[test]
    fn evaluation_graph_get_compute_node_mut_returns_mutable_reference() {
        use reify_core::ComputeNodeId;
        let mut graph = EvaluationGraph::default();
        let data = ComputeNodeData {
            computation_id: ComputeNodeId::new("Bracket", 0),
            target: "solver::elastic_static".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![],
            options_hash: ContentHash::of_str("opts"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![],
        };
        let id = graph.insert_compute_node(data);
        graph.get_compute_node_mut(&id).unwrap().target = "other::target".to_string();
        assert_eq!(graph.get_compute_node(&id).unwrap().target, "other::target");
    }

    #[test]
    fn evaluation_graph_get_compute_node_missing_returns_none() {
        use reify_core::ComputeNodeId;
        let graph = EvaluationGraph::default();
        assert!(
            graph
                .get_compute_node(&ComputeNodeId::new("Nope", 99))
                .is_none()
        );
    }

    #[test]
    fn evaluation_graph_multiple_compute_nodes_coexist() {
        use reify_core::ComputeNodeId;
        let mut graph = EvaluationGraph::default();

        let id_a = graph.insert_compute_node(ComputeNodeData {
            computation_id: ComputeNodeId::new("Bracket", 0),
            target: "solver::elastic_static".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![],
            options_hash: ContentHash::of_str("opts_a"),
            cache_key: ContentHash::of_str("ck_a"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![],
        });
        let id_b = graph.insert_compute_node(ComputeNodeData {
            computation_id: ComputeNodeId::new("Bracket", 1),
            target: "solver::modal".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![],
            options_hash: ContentHash::of_str("opts_b"),
            cache_key: ContentHash::of_str("ck_b"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![],
        });

        assert_eq!(graph.compute_nodes.len(), 2);
        assert_eq!(
            graph.get_compute_node(&id_a).unwrap().target,
            "solver::elastic_static"
        );
        assert_eq!(
            graph.get_compute_node(&id_b).unwrap().target,
            "solver::modal"
        );
    }

    #[test]
    fn evaluation_graph_clone_preserves_compute_nodes() {
        use reify_core::ComputeNodeId;
        use reify_ir::OpaqueState;
        let mut graph = EvaluationGraph::default();

        let id = graph.insert_compute_node(ComputeNodeData {
            computation_id: ComputeNodeId::new("Bracket", 0),
            target: "solver::elastic_static".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![],
            options_hash: ContentHash::of_str("opts"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: Some(OpaqueState::new(7i32, 4)),
            running: None,
            output_value_cells: vec![],
        });

        let mut cloned = graph.clone();
        // Insert a second node only in clone
        cloned.insert_compute_node(ComputeNodeData {
            computation_id: ComputeNodeId::new("Bracket", 1),
            target: "solver::modal".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![],
            options_hash: ContentHash::of_str("opts2"),
            cache_key: ContentHash::of_str("ck2"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![],
        });

        // Original unchanged
        assert_eq!(graph.compute_nodes.len(), 1);
        // Clone has both
        assert_eq!(cloned.compute_nodes.len(), 2);
        assert!(cloned.get_compute_node(&id).is_some());
        // Manual-Clone contract: opaque_state dropped to None on clone
        assert!(cloned.get_compute_node(&id).unwrap().opaque_state.is_none());
    }

    #[test]
    fn evaluation_graph_compute_nodes_iter_yields_all_inserted() {
        use reify_core::ComputeNodeId;
        use std::collections::HashSet;
        let mut graph = EvaluationGraph::default();

        for (target, idx) in [("solver::a", 0u32), ("solver::b", 1), ("solver::c", 2)] {
            graph.insert_compute_node(ComputeNodeData {
                computation_id: ComputeNodeId::new("Bracket", idx),
                target: target.to_string(),
                value_inputs: vec![],
                realization_inputs: vec![],
                options_hash: ContentHash::of_str(target),
                cache_key: ContentHash::of_str(target),
                cached_result: None,
                result_content_hash: None,
                opaque_state: None,
                running: None,
                output_value_cells: vec![],
            });
        }

        let targets: HashSet<String> = graph
            .compute_nodes
            .values()
            .map(|n| n.target.clone())
            .collect();
        assert_eq!(
            targets,
            ["solver::a", "solver::b", "solver::c"]
                .iter()
                .map(|s| s.to_string())
                .collect::<HashSet<_>>()
        );
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
            geometry_cell: None,
            id: rnid.clone(),
            operations: vec![],
            content_hash: ContentHash::of_str("r0"),
            produced_repr: reify_ir::ReprKind::BRep,
            produced_kernel: None,
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
                geometry_cell: None,
                id: RealizationNodeId::new("X", 0),
                operations: vec![],
                content_hash: hash_h,
                produced_repr: reify_ir::ReprKind::BRep,
                produced_kernel: None,
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
        use reify_core::ResolutionNodeId;

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
        use reify_core::{ResolutionNodeId, Type};
        use reify_ir::{CompiledExpr, Value};
        use reify_test_support::TopologyTemplateBuilder;

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
        use reify_core::ResolutionNodeId;

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
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr, Value};
        use reify_test_support::TopologyTemplateBuilder;

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
        use reify_core::SourceSpan;
        use reify_test_support::TopologyTemplateBuilder;

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
        use reify_ast::ConnectOp;
        use reify_compiler::CompiledConnection;
        use reify_core::SourceSpan;
        use reify_test_support::TopologyTemplateBuilder;

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
            carried.compatibility_constraint,
            conn.compatibility_constraint,
        );
    }

    /// Helper for the connection-bucket fingerprint tests below.
    fn make_connection(
        entity: &str,
        idx: u32,
        left: &str,
        right: &str,
    ) -> reify_compiler::CompiledConnection {
        use reify_ast::ConnectOp;
        use reify_core::SourceSpan;
        reify_compiler::CompiledConnection {
            left_port: left.to_string(),
            operator: ConnectOp::Forward,
            right_port: right.to_string(),
            connector_sub: None,
            compatibility_constraint: ConstraintNodeId::new(entity, idx),
            port_mappings: Vec::new(),
            frame_constraint: None,
            span: SourceSpan::empty(0),
        }
    }

    /// Task 2690 step-7/step-8: adding a `CompiledConnection` to a graph
    /// must change `topology_fingerprint`. Two graphs identical except for
    /// one extra connection must produce distinct fingerprints.
    ///
    /// RED before step-8 (the connections bucket is not yet mixed in).
    #[test]
    fn topology_fingerprint_includes_connections() {
        let mut g_no_conn = EvaluationGraph::default();
        g_no_conn.value_cells.insert(
            ValueCellId::new("X", "a"),
            ValueCellNode {
                id: ValueCellId::new("X", "a"),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: ContentHash::of_str("a"),
            },
        );

        let mut g_with_conn = g_no_conn.clone();
        g_with_conn
            .connections
            .push(make_connection("X", 0, "p", "q"));

        assert_ne!(
            g_no_conn.topology_fingerprint(),
            g_with_conn.topology_fingerprint(),
            "fingerprint must change when a connection is added",
        );
    }

    /// Task 2690 step-7/step-8: domain separation between connection bucket
    /// and other node-type buckets. A graph with a single CompiledConnection
    /// whose hash inputs collide with an unrelated value-cell content_hash
    /// must produce a different fingerprint than a graph with that same hash
    /// on the value-cell only. Mirrors `fingerprint_domain_separates_node_types`.
    ///
    /// RED before step-8 (the connections bucket is not yet mixed in, so
    /// both graphs collapse to the same fingerprint).
    #[test]
    fn fingerprint_domain_separates_connections_from_others() {
        let hash_h = ContentHash::of_str("collide");

        // Graph A: single value cell with hash_h.
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

        // Graph B: same value cell + one CompiledConnection. The
        // connection contributes an additional bucket hash; if connections
        // were not mixed in, both graphs would fingerprint identically.
        let mut graph_b = graph_a.clone();
        graph_b.connections.push(make_connection("X", 0, "p", "q"));

        assert_ne!(
            graph_a.topology_fingerprint(),
            graph_b.topology_fingerprint(),
            "fingerprint must domain-separate connections from value_cells",
        );
    }

    /// Task 2713 step-1/step-2: `connector_sub` on a CompiledConnection must
    /// influence `topology_fingerprint`. Three graphs whose only difference is
    /// `connector_sub` (None, Some("sub_a"), Some("sub_b")) must produce
    /// pairwise distinct fingerprints.
    ///
    /// RED before step-2: the connection-bucket hash chain does not yet
    /// include `connector_sub`, so all three graphs produce identical
    /// fingerprints.
    #[test]
    fn topology_fingerprint_includes_connector_sub() {
        use reify_ast::ConnectOp;
        use reify_core::SourceSpan;

        let base_conn = reify_compiler::CompiledConnection {
            left_port: "p".to_string(),
            operator: ConnectOp::Forward,
            right_port: "q".to_string(),
            connector_sub: None,
            compatibility_constraint: ConstraintNodeId::new("X", 0),
            port_mappings: Vec::new(),
            frame_constraint: None,
            span: SourceSpan::empty(0),
        };

        let mut graph_none = EvaluationGraph::default();
        graph_none.connections.push(base_conn.clone());

        let mut graph_some_a = EvaluationGraph::default();
        graph_some_a
            .connections
            .push(reify_compiler::CompiledConnection {
                connector_sub: Some("sub_a".to_string()),
                ..base_conn.clone()
            });

        let mut graph_some_b = EvaluationGraph::default();
        graph_some_b
            .connections
            .push(reify_compiler::CompiledConnection {
                connector_sub: Some("sub_b".to_string()),
                ..base_conn
            });

        assert_ne!(
            graph_none.topology_fingerprint(),
            graph_some_a.topology_fingerprint(),
            "fingerprint must differ: connector_sub None vs Some(\"sub_a\")",
        );
        assert_ne!(
            graph_some_a.topology_fingerprint(),
            graph_some_b.topology_fingerprint(),
            "fingerprint must differ: connector_sub Some(\"sub_a\") vs Some(\"sub_b\")",
        );
        assert_ne!(
            graph_none.topology_fingerprint(),
            graph_some_b.topology_fingerprint(),
            "fingerprint must differ: connector_sub None vs Some(\"sub_b\")",
        );
    }

    /// Task 2713 step-3/step-4: `frame_constraint` on a CompiledConnection must
    /// influence `topology_fingerprint`. Three graphs whose only difference is
    /// `frame_constraint` (None, Some(ConstraintNodeId("Frame",0)),
    /// Some(ConstraintNodeId("Frame",1))) must produce pairwise distinct
    /// fingerprints.
    ///
    /// RED before step-4: the connection-bucket hash chain does not yet
    /// include `frame_constraint`, so all three graphs produce identical
    /// fingerprints.
    #[test]
    fn topology_fingerprint_includes_frame_constraint() {
        use reify_ast::ConnectOp;
        use reify_core::SourceSpan;

        let base_conn = reify_compiler::CompiledConnection {
            left_port: "p".to_string(),
            operator: ConnectOp::Forward,
            right_port: "q".to_string(),
            connector_sub: None,
            compatibility_constraint: ConstraintNodeId::new("X", 0),
            port_mappings: Vec::new(),
            frame_constraint: None,
            span: SourceSpan::empty(0),
        };

        let mut graph_none = EvaluationGraph::default();
        graph_none.connections.push(base_conn.clone());

        let mut graph_some_a = EvaluationGraph::default();
        graph_some_a
            .connections
            .push(reify_compiler::CompiledConnection {
                frame_constraint: Some(ConstraintNodeId::new("Frame", 0)),
                ..base_conn.clone()
            });

        let mut graph_some_b = EvaluationGraph::default();
        graph_some_b
            .connections
            .push(reify_compiler::CompiledConnection {
                frame_constraint: Some(ConstraintNodeId::new("Frame", 1)),
                ..base_conn
            });

        assert_ne!(
            graph_none.topology_fingerprint(),
            graph_some_a.topology_fingerprint(),
            "fingerprint must differ: frame_constraint None vs Some(id_0)",
        );
        assert_ne!(
            graph_some_a.topology_fingerprint(),
            graph_some_b.topology_fingerprint(),
            "fingerprint must differ: frame_constraint Some(id_0) vs Some(id_1)",
        );
        assert_ne!(
            graph_none.topology_fingerprint(),
            graph_some_b.topology_fingerprint(),
            "fingerprint must differ: frame_constraint None vs Some(id_1)",
        );
    }

    /// Task 2690 step-7/step-8: insertion order of `connections` must NOT
    /// affect `topology_fingerprint`. Mirrors `topology_fingerprint_order_independent`.
    ///
    /// RED before step-8 (since connections aren't mixed in, both graphs
    /// fingerprint identically anyway — this test will only fail meaningfully
    /// once the bucket is wired and the per-connection hashes are sorted).
    /// Even after wiring, this test guards against an accidental
    /// non-deterministic mix (e.g. forgetting to sort the per-connection
    /// hashes before `combine_all`).
    #[test]
    fn topology_fingerprint_connections_order_independent() {
        let conn_a = make_connection("X", 0, "p1", "q1");
        let conn_b = make_connection("X", 1, "p2", "q2");

        let mut g1 = EvaluationGraph::default();
        g1.connections.push(conn_a.clone());
        g1.connections.push(conn_b.clone());

        let mut g2 = EvaluationGraph::default();
        g2.connections.push(conn_b);
        g2.connections.push(conn_a);

        assert_eq!(
            g1.topology_fingerprint(),
            g2.topology_fingerprint(),
            "connection insertion order must not affect fingerprint",
        );
    }

    // --- CancellationHandle API tests (PRD §8 task β observable signal) ---

    #[test]
    fn cancellation_handle_new_is_not_cancelled() {
        let handle = CancellationHandle::new();
        assert!(!handle.is_cancelled(), "fresh handle must not be cancelled");
    }

    #[test]
    fn cancellation_handle_cancel_makes_is_cancelled_true() {
        let handle = CancellationHandle::new();
        handle.cancel();
        assert!(
            handle.is_cancelled(),
            "handle must be cancelled after cancel()"
        );
    }

    #[test]
    fn cancellation_handle_clones_share_cancellation_state() {
        let original = CancellationHandle::new();
        let clone = original.clone();
        clone.cancel();
        assert!(
            original.is_cancelled(),
            "cancelling a clone must be visible on the original (Arc-sharing)"
        );
    }

    #[test]
    fn cancellation_handle_thread_safety_cancel_from_spawned_thread() {
        let handle = CancellationHandle::new();
        let clone = handle.clone();
        let t = std::thread::spawn(move || {
            clone.cancel();
        });
        t.join().expect("spawned thread panicked");
        assert!(
            handle.is_cancelled(),
            "cancellation from spawned thread must be visible on the main-thread handle"
        );
    }
}
