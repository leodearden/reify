// Split from lib.rs (task 2032) — purposes methods.

use crate::Engine;
use crate::cache::NodeId;
use crate::deps::ReverseDependencyIndex;
use crate::graph::ValueCellNode;
use reify_compiler::{ResolvedSchemaQuery, ValueCellKind};
use reify_core::{ConstraintNodeId, ContentHash, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, OptimizationObjective, PersistentMap};
use std::sync::Arc;

impl Engine {
    /// Activate a purpose by name against a target entity.
    ///
    /// Looks up the compiled purpose by `purpose_name`, then injects its
    /// constraints into the current evaluation graph. The injected constraint
    /// node IDs use a `"purpose:<purpose_name>@<entity_ref>"` entity prefix to
    /// avoid collisions with structure-level constraints.
    ///
    /// Requires a prior call to `eval()` so an evaluation state exists.
    /// If the purpose is already active, this is a no-op.
    ///
    /// **Persistence across `eval()` (task 3103):** `Engine::eval` preserves
    /// the per-purpose binding set across the call by snapshotting
    /// `active_purpose_bindings` (user intent) before clearing the derived
    /// state (`active_purposes`, `active_objective_map`,
    /// `active_tolerance_scope`), then re-applying each captured binding via
    /// this method after the fresh snapshot is stored. The canonical user flow
    /// `engine.eval → activate_purpose → engine.build` therefore keeps purpose
    /// state intact across `build()`'s internal `check() → eval()` round-trip;
    /// production callers do **not** need to re-activate purposes between
    /// consecutive builds. The snapshot surfaces (`build_snapshot` /
    /// `tessellate_snapshot`) participate in the same contract — bindings
    /// persist across all three flow shapes. See `engine_eval.rs` for the
    /// preservation site.
    ///
    /// **Idempotency:** calling `activate_purpose` twice for the same
    /// `purpose_name` is a no-op via the `active_purposes.contains_key` early
    /// return in `activate_purpose_constraints` — **the first binding wins
    /// regardless of `entity_ref`**. A second call with a different `entity_ref`
    /// is silently ignored; to rebind a purpose to a new entity, call
    /// `deactivate_purpose` first. This means existing callers that manually
    /// re-activate between consecutive builds (with the same entity) continue
    /// to work harmlessly — the second call is silently skipped.
    ///
    /// **Multi-param refusal:** activating a multi-param purpose via this
    /// single-entity shim is refused (no-op + `tracing::warn!`) pending the
    /// per-param bindings API (`activate_purpose_with_bindings`, task γ —
    /// PRD §4.5 C2). `is_purpose_active` returns `false` and no constraints
    /// are injected.
    pub fn activate_purpose(&mut self, purpose_name: &str, entity_ref: &str) {
        // Delegate to the constraint-injection helper; rebuild infrastructure only
        // once rather than once per call.  For the single-activation case (N=1)
        // this is equivalent to the previous inline implementation.
        if self.activate_purpose_constraints(purpose_name, entity_ref) {
            self.rebuild_purpose_infrastructure();
        }
    }

    /// Inject a purpose's constraints, demand entries, and objective into the
    /// current evaluation graph, and record the binding in `active_purposes`,
    /// `active_purpose_bindings`, and `active_objective_map`.
    ///
    /// Returns `true` if the injection was performed, `false` when this is a
    /// no-op (purpose already active, purpose not found in `compiled_purposes`,
    /// no `eval_state` present, or the purpose has more than one param — the
    /// single-entity shim cannot bind a multi-param purpose; callers must use
    /// `activate_purpose_with_bindings`, task γ — PRD §4.5 C2).
    ///
    /// **Does NOT** rebuild `reverse_index`, `trace_map`, `rebuild_cone`, or
    /// `active_tolerance_scope`.  Call `rebuild_purpose_infrastructure()` once
    /// after a batch of calls to amortise those O(graph) passes into a single
    /// pass — this is what `Engine::eval`'s preserved-binding loop does (task
    /// 3103, S1 reviewer suggestion).
    pub(crate) fn activate_purpose_constraints(
        &mut self,
        purpose_name: &str,
        entity_ref: &str,
    ) -> bool {
        // No-op if already active — first binding wins; a different entity_ref
        // is ignored.  Call deactivate_purpose first to rebind to a new entity.
        if self.active_purposes.contains_key(purpose_name) {
            return false;
        }

        // Look up the compiled purpose
        let purpose = match self
            .compiled_purposes
            .iter()
            .find(|p| p.name == purpose_name)
        {
            Some(p) => p.clone(),
            None => return false, // Purpose not found — silently ignore
        };

        // Contract C2 (PRD §4.5): the single-entity `activate_purpose(name, entity_ref)`
        // shim cannot safely bind a multi-param purpose. Applying one `entity_ref` to
        // every per-param `{purpose}::{param}` stamp (the remap loop below) would alias
        // distinct params — `part.length > envelope.length` would collapse to
        // `entity.length > entity.length`, a silently meaningless constraint. Refuse the
        // activation rather than inject a mis-bound constraint. Per-param binding is task
        // γ's `activate_purpose_with_bindings`; until it lands the single-entity path is a
        // refusal (non-silent: warn-logged + observable as no injection / not active),
        // NOT a silent no-op or mis-bind.
        if purpose.params.len() > 1 {
            tracing::warn!(
                purpose = %purpose_name,
                param_count = purpose.params.len(),
                "refusing single-entity activation of multi-param purpose; use activate_purpose_with_bindings (task gamma)"
            );
            return false;
        }

        // Get mutable access to the evaluation state
        let state = match self.eval_state.as_mut() {
            Some(s) => s,
            None => return false, // No eval state — silently ignore
        };

        // Build a unique entity prefix for the purpose-injected constraints
        let purpose_entity = format!("purpose:{}@{}", purpose_name, entity_ref);

        // Rewrite compiled expressions: substitute each per-param stamp
        // `ValueCellId("{purpose}::{param}", member)` with `ValueCellId(entity_ref, member)`
        // so references resolve to existing value cells in the evaluation graph (task-2181 β).
        //
        // By the time control reaches here, `purpose.params.len() == 1` (multi-param
        // purposes are refused above by the C2 guard). The per-param loop therefore runs
        // exactly once — behavior-identical to the pre-β single `remap_entity(purpose_name,
        // entity_ref)`. Task γ replaces the C2 guard-plus-single-binding loop with a
        // per-binding remap via `activate_purpose_with_bindings`; keep the `for param in
        // &purpose.params { … }` loop shape as the seam γ generalizes.
        let mut rewritten_constraints = purpose.constraints.clone();
        for constraint in &mut rewritten_constraints {
            for param in &purpose.params {
                let from_stamp = format!("{}::{}", purpose_name, param.name);
                constraint.expr.remap_entity(&from_stamp, entity_ref);
            }
        }

        let rewritten_objective = purpose.objective.clone().map(|mut obj| {
            match &mut obj {
                OptimizationObjective::Minimize(expr) | OptimizationObjective::Maximize(expr) => {
                    for param in &purpose.params {
                        let from_stamp = format!("{}::{}", purpose_name, param.name);
                        expr.remap_entity(&from_stamp, entity_ref);
                    }
                }
            }
            obj
        });

        // Expand `PurposeReflectiveAggregation` placeholders into populated
        // `ListLiteral([ValueRef(entity_ref, member), ...])` nodes against the
        // bound entity (task-2289). Walks each constraint expression and the
        // objective immediately after the `remap_entity` rewrite. The walk
        // mirrors `remap_entity`'s arm-by-arm structure so future variant
        // additions in `crates/reify-types/src/expr.rs` only need to touch the
        // same places.
        //
        // Captured shape — `&purpose.resolved_queries`, `entity_ref`, and
        // `&state.snapshot.graph.value_cells` — is identical for the
        // constraints loop and the objective rewrite, so a closure keeps the
        // call sites in lockstep (any future arg change only touches one
        // signature).
        let expand_placeholders = |expr: &mut CompiledExpr| {
            expand_purpose_reflective_placeholders(
                expr,
                &purpose.resolved_queries,
                entity_ref,
                &state.snapshot.graph.value_cells,
            );
        };
        for constraint in &mut rewritten_constraints {
            expand_placeholders(&mut constraint.expr);
        }
        let rewritten_objective = rewritten_objective.map(|mut obj| {
            match &mut obj {
                OptimizationObjective::Minimize(expr) | OptimizationObjective::Maximize(expr) => {
                    expand_placeholders(expr);
                }
            }
            obj
        });

        // Inject each of the purpose's compiled constraints into the evaluation graph
        let mut injected_ids = Vec::new();
        for (i, constraint) in rewritten_constraints.iter().enumerate() {
            let constraint_id = ConstraintNodeId::new(&purpose_entity, i as u32);
            let node = crate::graph::ConstraintNodeData {
                id: constraint_id.clone(),
                label: constraint.label.clone(),
                expr: constraint.expr.clone(),
                content_hash: ContentHash::of_str(&format!(
                    "purpose:{}:constraint:{}",
                    purpose_name, i
                )),
                optimized_target: constraint.optimized_target.clone(),
            };
            state
                .snapshot
                .graph
                .constraints
                .insert(constraint_id.clone(), node);
            injected_ids.push(constraint_id);
        }

        // Update demand registry: demand each newly injected constraint node.
        for id in &injected_ids {
            self.demand.add_demand(NodeId::Constraint(id.clone()));
        }

        self.active_purposes
            .insert(purpose_name.to_string(), injected_ids);

        // Inject the optimization objective if the purpose has one.
        if let Some(ref objective) = rewritten_objective {
            self.active_objective_map
                .insert(purpose_name.to_string(), objective.clone());
        }

        // Record the per-purpose entity binding.  The binding insert MUST run
        // before any call to recompute_tolerance_scope() — which happens in the
        // caller via rebuild_purpose_infrastructure() — so this purpose's
        // `RepresentationWithin` contribution is included in the resulting
        // scope; tighter contributions across purposes win via `min`. See
        // `crates/reify-eval/src/tolerance_scope.rs` for the recognition
        // matcher and propagation walk.
        self.active_purpose_bindings
            .insert(purpose_name.to_string(), entity_ref.to_string());

        true
    }

    /// Rebuild the purpose-activation infrastructure (reverse_index, trace_map,
    /// rebuild_cone, and active_tolerance_scope) against the current graph state.
    ///
    /// Called once after `activate_purpose_constraints` (or once after a batch
    /// of such calls) to materialise the incremental-eval routing tables and the
    /// tolerance scope.  When N purposes are re-injected — e.g. in `Engine::eval`'s
    /// preserved-binding loop (task 3103) — a single call here amortises the
    /// O(graph) rebuild cost into one pass instead of N independent passes.
    ///
    /// Compose-field edge preservation (task-2343): passes `&compiled_fields`
    /// into the `_and_fields` builders so composed-field reverse-index edges
    /// (`__field.<dep>` → `Value(__field.<composed>)`) survive the rebuild.
    /// Without this, every composed-field edge would be dropped here, breaking
    /// the cache invariant downstream — pinned by
    /// `purpose_activation_preserves_composed_field_reverse_index` in
    /// `crates/reify-eval/tests/purpose_activation.rs`.  The
    /// `Arc::clone(&self.compiled_fields)` happens BEFORE the
    /// `self.eval_state.as_mut()` reborrow because both go through `&mut self`;
    /// cloning the Arc first into a fully owned local sidesteps the cross-field
    /// borrow.  Mirrors the pattern at `engine_edit.rs:829`.
    pub(crate) fn rebuild_purpose_infrastructure(&mut self) {
        let compiled_fields = Arc::clone(&self.compiled_fields);
        if let Some(state) = self.eval_state.as_mut() {
            state.reverse_index = ReverseDependencyIndex::build_from_graph_and_fields(
                &state.snapshot.graph,
                &compiled_fields,
            );
            state.trace_map =
                crate::deps::build_trace_map_and_fields(&state.snapshot.graph, &compiled_fields);
        }
        if let Some(state) = self.eval_state.as_ref() {
            self.demand.rebuild_cone(&state.snapshot.graph);
        }
        self.recompute_tolerance_scope();
    }

    /// Deactivate a purpose by name.
    ///
    /// Removes the constraints and objectives that were injected by `activate_purpose`.
    /// If the purpose is not active, this is a no-op.
    pub fn deactivate_purpose(&mut self, purpose_name: &str) {
        // Look up and remove the injected constraint IDs
        let injected_ids = match self.active_purposes.remove(purpose_name) {
            Some(ids) => ids,
            None => return, // Not active — no-op
        };

        // Update demand registry: remove demand for each ejected constraint node.
        for id in &injected_ids {
            self.demand.remove_demand(&NodeId::Constraint(id.clone()));
        }

        // Remove each injected constraint from the evaluation graph, then
        // rebuild the infrastructure so subsequent edit_param() calls no longer
        // route through purpose constraint dependencies.
        //
        // Compose-field edge preservation (task-2343): pass `&compiled_fields`
        // into the `_and_fields` builders for the same reason as
        // `activate_purpose` above — composed-field edges must survive every
        // rebuild path or the cache invariant breaks downstream.
        let compiled_fields = Arc::clone(&self.compiled_fields);
        if let Some(state) = self.eval_state.as_mut() {
            for constraint_id in &injected_ids {
                state.snapshot.graph.constraints.remove(constraint_id);
            }
            state.reverse_index = ReverseDependencyIndex::build_from_graph_and_fields(
                &state.snapshot.graph,
                &compiled_fields,
            );
            state.trace_map =
                crate::deps::build_trace_map_and_fields(&state.snapshot.graph, &compiled_fields);
        }
        if let Some(state) = self.eval_state.as_ref() {
            self.demand.rebuild_cone(&state.snapshot.graph);
        }

        // Remove the objective if one was injected
        self.active_objective_map.remove(purpose_name);

        // Drop the per-purpose entity binding then rebuild the tolerance
        // scope (task 2647 / PRD `docs/prds/v0_2/per-purpose-tolerance.md`,
        // "Resolved design decisions" → "Tolerance lives at the purpose").
        // The binding remove MUST run before the recompute so this purpose's
        // contribution is excluded from the resulting scope; surviving
        // contributors fold back to their own `min`. See
        // `crates/reify-eval/src/tolerance_scope.rs` for the recognition
        // matcher and propagation walk.
        self.active_purpose_bindings.remove(purpose_name);
        self.recompute_tolerance_scope();
    }

    /// Check whether a purpose is currently active.
    pub fn is_purpose_active(&self, purpose_name: &str) -> bool {
        self.active_purposes.contains_key(purpose_name)
    }

    /// Returns the currently active optimization objectives (injected by purposes).
    pub fn active_objectives(&self) -> Vec<&OptimizationObjective> {
        self.active_objective_map.values().collect()
    }

    /// Rebuild `active_tolerance_scope` from scratch by walking every
    /// currently-active purpose binding, extracting its tolerance bindings
    /// (RepresentationWithin shape recognition), propagating each subject
    /// to its dotted descendants in the value-cell graph, and combining
    /// contributions across purposes via `min` (tighter satisfies looser).
    ///
    /// Called at the end of both `activate_purpose` and `deactivate_purpose`
    /// after `active_purpose_bindings` has been mutated. Full recompute is
    /// the chosen posture per the design decision in `.task/plan.json` —
    /// active purposes are typically 1-3 and value_cell iteration is already
    /// used by other Engine paths (see `engine_purposes.rs:147-159`).
    ///
    /// TODO(perf): each `propagate_subject_to_descendants` call is an
    /// O(n_value_cells) linear prefix scan, and we run it once per
    /// (purpose × tolerance binding). For the documented 1-3 active
    /// purposes this is fine, but if the dispatcher (sibling tasks 2649/
    /// 2650) ends up calling `active_tolerance_for` on a hot path while
    /// purposes flip, replace the linear scan with a prefix-trie (or
    /// incremental-update strategy keyed on entity_ref) inside this
    /// helper. Keep the recompute entry point so callers don't have to
    /// know the strategy changed.
    ///
    /// **Note on `Engine::realization_cache` interaction (task 2874 step-14 + amendment scope-correction)**:
    /// this helper does NOT explicitly clear or invalidate the realization
    /// cache. The cache's partial-order rule (`cached_tol ≤ requested_tol`,
    /// enforced by `RealizationCache::lookup` → `ToleranceBucket::lookup`)
    /// produces the correct cache-miss behaviour when the recomputed scope
    /// tightens the demanded tolerance for a previously-cached entity, and
    /// the correct cache-hit behaviour when the scope loosens or remains
    /// unchanged. Pinned by the integration test
    /// `cache_lookup_misses_when_purpose_changes_demanded_tolerance` in
    /// `crates/reify-eval/tests/tolerance_wiring_e2e.rs`.
    ///
    /// IMPORTANT: the partial-order rule covers ONLY tolerance-driven
    /// staleness. Handle-stability hazards orthogonal to demanded tolerance
    /// (e.g. parameter edits invalidating the underlying geometry while
    /// keeping `(entity_id, BRep, demanded_tol)` constant — a cache hit
    /// would then return a stale `GeometryHandleId`) are NOT mitigated by
    /// the partial-order rule and require their own invalidation strategy,
    /// which is OUT OF SCOPE for this MVP wiring (follow-up task expected).
    /// See the `Engine::realization_cache` field docstring on `lib.rs` for
    /// the full known-limitation rundown.
    fn recompute_tolerance_scope(&mut self) {
        self.active_tolerance_scope.clear();

        // No eval state ⇒ no value_cells to scan ⇒ nothing to populate.
        // Early-return preserves the empty-scope invariant for engines
        // constructed without `eval()` (or after a fresh `eval()` clears
        // the prior snapshot).
        let value_cells = match self.eval_state.as_ref() {
            Some(state) => &state.snapshot.graph.value_cells,
            None => return,
        };

        for (purpose_name, entity_ref) in &self.active_purpose_bindings {
            let purpose = match self
                .compiled_purposes
                .iter()
                .find(|p| &p.name == purpose_name)
            {
                Some(p) => p,
                None => continue, // Compiled purpose disappeared (e.g. across re-eval) — skip.
            };
            // NOTE: single-binding contract — `entity_ref` is substituted for every matched
            // constraint's subject unambiguously because today's API binds at most one
            // entity-ref per purpose. See `extract_tolerance_bindings` § "Single-binding
            // contract" for what a future multi-param producer must change at this call site.
            let bindings = crate::tolerance_scope::extract_tolerance_bindings(purpose, entity_ref);
            for binding in bindings {
                let descendants = crate::tolerance_scope::propagate_subject_to_descendants(
                    &binding.subject_entity,
                    value_cells,
                );
                let additions = descendants
                    .into_iter()
                    .map(|entity| (entity, binding.si_tolerance));
                crate::tolerance_scope::merge_with_min(&mut self.active_tolerance_scope, additions);
            }
        }
    }
}

// ─── Activation-time reflective-aggregation expansion (task-2289) ────────────

/// Walk the given expression tree and rewrite every
/// `CompiledExprKind::PurposeReflectiveAggregation` placeholder into a
/// populated `CompiledExprKind::ReflectiveCellList` of `ValueRef(entity_ref,
/// member)` elements sourced from the bound entity (task-2458 — distinguishes
/// placeholder-derived lists from user-written `ListLiteral`s for
/// `eval_quantifier`'s cell-iteration trigger). Element `result_type` is taken
/// from the looked-up `ValueCellNode.cell_type` (cell-type lockstep, task-1904
/// cross-reference); the outer list `result_type` adopts
/// `Type::List(Box::new(first_element_type))` when populated, falling back to
/// `Type::List(Box::new(Type::Error))` for the empty-list case (anti-cascade
/// poison — task 3749 tightened the 3639 Shape-A G-allow carve-out).  The
/// `Type::Error` element type is not observed before `eval_quantifier`'s
/// vacuous-true short-circuit fires: the sole caller
/// (`activate_purpose_constraints`) stores the rewritten expr in the constraint
/// graph without reading the outer `result_type` for type-compatibility checks.
///
/// Resolution strategy for the `params` query:
///   1. Prefer the compile-time `ResolvedSchemaQuery` whose `query_kind`
///      matches the placeholder. This is populated for concrete-typed
///      purpose params (`subject : Bracket`) by `compile_purpose` in
///      `reify-compiler/src/traits.rs`.
///   2. Fall back to scanning `value_cells` for cells whose `entity` matches
///      `entity_ref` and whose `kind` is `Param` or `Auto`. This handles the
///      wildcard-subject case (`subject : Structure`) where the compiler
///      cannot pre-resolve queries because the template is not known until
///      activation.
///
/// For other query kinds (`geometric_params`, `material_params`), no compile-
/// time resolution and no fallback heuristic exists yet (task-1904 territory),
/// so the placeholder rewrites to an empty list — matching today's
/// vacuous-true behaviour for those members.
///
/// The walk mirrors `CompiledExpr::remap_entity`'s arm-by-arm traversal so a
/// future variant addition only touches the same places.
///
/// Missing-cell branch (wiring inconsistency): when the resolved query
/// references a `ValueCellId` that is absent from `value_cells`, the
/// lookup misses and the branch:
///   1. Emits `tracing::warn!` (observable at `RUST_LOG=warn` without
///      manual tuning) with structured fields `entity`, `param`,
///      `query`, and `missing_cell` identifying the discrepancy.
///   2. Fires `debug_assert!(false, ...)` — halt loudly in debug builds
///      (same posture as the `PurposeReflectiveAggregation` arm in
///      `eval_expr`, `crates/reify-expr/src/lib.rs`).
///   3. Falls back to `Type::Real` for release-build anti-cascade safety.
///
/// This is qualitatively different from the empty-list case (absent
/// `ResolvedSchemaQuery` — intentional vacuous-true) and the present-cell
/// case (normal cell-type lockstep): a missing cell means the template
/// and the runtime entity diverged, or a wiring bug picked the wrong
/// `entity_ref`. The order — warn first, debug_assert second, fallback
/// third — ensures the tracing event increments before any panic, so a
/// `catch_unwind` caller can still read the counter.
///
/// CONTRACT — content-hash staleness: replacing a placeholder node updates
/// that node's `content_hash` (via `CompiledExpr::reflective_cell_list`), but
/// **does not** rebuild ancestor hashes (e.g. the enclosing `Quantifier`
/// still carries the pre-rewrite hash). This is the same posture as
/// `CompiledExpr::remap_entity` (also called above). Today this is safe
/// because the only consumer of the rewritten expression — constraint
/// injection at lines ~96–113 — reseeds each constraint's `content_hash`
/// from `purpose:<name>:constraint:<i>` independently of the expression
/// hash, so injected nodes are never observed at their stale hash. If a
/// future caller relies on `expr.content_hash` for sub-expression cache
/// lookups inside the rewritten tree, add a bottom-up hash-rebuild pass
/// here (and on `remap_entity`/`remap_cell`).
fn expand_purpose_reflective_placeholders(
    expr: &mut CompiledExpr,
    queries: &[ResolvedSchemaQuery],
    entity_ref: &str,
    value_cells: &PersistentMap<ValueCellId, ValueCellNode>,
) {
    match &mut expr.kind {
        CompiledExprKind::PurposeReflectiveAggregation {
            param_name,
            query_kind,
        } => {
            // Resolve the member list for this placeholder. Prefer compile-
            // time `ResolvedSchemaQuery`; fall back to scanning `value_cells`
            // for the bound entity's params when the query is unresolved
            // (wildcard-subject case).
            //
            // The lookup filters by *both* `param_name` and `query_kind` —
            // the placeholder records which purpose param it was projected
            // from (e.g. `subject` vs a hypothetical `part`), and so does
            // each `ResolvedSchemaQuery`. Today's compiler only accepts a
            // single StructureRef param (task-2201), so collisions are
            // latent — but matching by query_kind alone would silently
            // misbind in any future multi-param purpose. Defending now is
            // cheaper than debugging that misbind later.
            let members: Vec<String> = if let Some(q) = queries
                .iter()
                .find(|q| q.param_name == *param_name && q.query_kind == *query_kind)
            {
                q.resolved_ids.iter().map(|id| id.member.clone()).collect()
            } else if query_kind == "params" {
                let mut members: Vec<String> = value_cells
                    .iter()
                    .filter(|(id, node)| {
                        id.entity == entity_ref
                            && matches!(
                                node.kind,
                                ValueCellKind::Param | ValueCellKind::Auto { .. }
                            )
                    })
                    .map(|(id, _)| id.member.clone())
                    .collect();
                // PersistentMap iteration order is not guaranteed stable
                // across runs; sort for determinism. Downstream tests
                // sort before comparison, so the surface order is not
                // load-bearing — but a stable order keeps logs and
                // hashes reproducible.
                members.sort();
                members
            } else {
                // geometric_params / material_params: no resolution path
                // yet (task-1904). Empty list ⇒ vacuous-true forall, same
                // as before this expansion existed.
                Vec::new()
            };

            // Build ValueRef elements with cell-type lockstep.
            // Extract string slices so the closure below can capture them
            // by shared reference without fighting the &mut bindings from
            // the outer match arm.
            let param_name_str = param_name.as_str();
            let query_kind_str = query_kind.as_str();
            let elements: Vec<CompiledExpr> = members
                .iter()
                .map(|member| {
                    let cell_id = ValueCellId::new(entity_ref, member);
                    let elem_type = match value_cells.get(&cell_id) {
                        Some(node) => node.cell_type.clone(),
                        None => {
                            // Graph-vs-resolved-query wiring inconsistency:
                            // `resolved_ids` is populated by compile_purpose
                            // against the template; the runtime entity_ref is
                            // expected to expose the same members. A miss here
                            // means the template and the runtime entity
                            // diverged, or a wiring bug picked the wrong
                            // entity_ref. Warn first (visible in release
                            // builds), debug_assert second (halts in debug),
                            // then fall back to Type::Real for anti-cascade
                            // safety (matches the Value::Undef posture in the
                            // PurposeReflectiveAggregation arm of eval_expr).
                            tracing::warn!(
                                target: "reify_eval::engine_purposes",
                                entity = %entity_ref,
                                param = %param_name_str,
                                query = %query_kind_str,
                                missing_cell = %cell_id,
                                "graph-vs-resolved-query inconsistency: \
                                 ResolvedSchemaQuery references a value cell \
                                 missing from snapshot.graph.value_cells; \
                                 falling back to Type::Real"
                            );
                            debug_assert!(
                                false,
                                "expand_purpose_reflective_placeholders: \
                                 resolved-query cell {:?} missing from \
                                 snapshot.graph.value_cells \
                                 (purpose param={:?}, query_kind={:?}, \
                                 entity={:?})",
                                cell_id, param_name_str, query_kind_str, entity_ref
                            );
                            Type::Real
                        }
                    };
                    CompiledExpr::value_ref(cell_id, elem_type)
                })
                .collect();

            // Invariant: every element above is a `ValueRef` — enforced by
            // `CompiledExpr::reflective_cell_list` (task-2552, follow-up to task-2544).
            // The `ReflectiveCellList(_)` no-op arm below relies on this.

            // Outer ReflectiveCellList type: inherit first element's type when
            // populated; default to Type::Error on empty (anti-cascade poison).
            // task-2458: emit ReflectiveCellList (not ListLiteral) so that
            // eval_quantifier's cell-iteration trigger fires only on this
            // placeholder-derived shape, not on user-written all-ValueRef
            // ListLiterals that share the same surface structure.
            // An empty ReflectiveCellList still triggers eval_quantifier's
            // vacuous-true short-circuit before the element type is used for any
            // arithmetic, so the Type::Error element type is never observed in the
            // vacuous-true path; it is defense in depth for release-mode safety
            // (task 3749 tightened the 3639 Shape-A G-allow carve-out).
            // Verified: the sole caller (activate_purpose_constraints) stores the
            // rewritten expr in the constraint graph without reading the outer
            // result_type for type-compatibility — no non-eval consumer of the
            // Type::List(Type::Error) shape exists between expansion and eval.
            let element_type = elements
                .first()
                .map(|e| e.result_type.clone())
                .unwrap_or(Type::Error);
            *expr =
                CompiledExpr::reflective_cell_list(elements, Type::List(Box::new(element_type)));
        }
        CompiledExprKind::ValueRef(_)
        | CompiledExprKind::CrossSubGeometryRef(_)
        | CompiledExprKind::Literal(_)
        | CompiledExprKind::OptionNone
        | CompiledExprKind::MetaAccess { .. }
        | CompiledExprKind::DeterminacyPredicate { .. } => {
            // No children carrying potential placeholders.
            // CrossSubGeometryRef is a leaf consumed by entity.rs before
            // activation — it never contains nested placeholders (task-3508).
        }
        CompiledExprKind::BinOp { left, right, .. } => {
            expand_purpose_reflective_placeholders(left, queries, entity_ref, value_cells);
            expand_purpose_reflective_placeholders(right, queries, entity_ref, value_cells);
        }
        CompiledExprKind::UnOp { operand, .. } => {
            expand_purpose_reflective_placeholders(operand, queries, entity_ref, value_cells);
        }
        CompiledExprKind::FunctionCall { args, .. }
        | CompiledExprKind::UserFunctionCall { args, .. } => {
            for arg in args {
                expand_purpose_reflective_placeholders(arg, queries, entity_ref, value_cells);
            }
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            expand_purpose_reflective_placeholders(condition, queries, entity_ref, value_cells);
            expand_purpose_reflective_placeholders(then_branch, queries, entity_ref, value_cells);
            expand_purpose_reflective_placeholders(else_branch, queries, entity_ref, value_cells);
        }
        CompiledExprKind::Match { discriminant, arms } => {
            expand_purpose_reflective_placeholders(discriminant, queries, entity_ref, value_cells);
            for arm in arms {
                expand_purpose_reflective_placeholders(
                    &mut arm.body,
                    queries,
                    entity_ref,
                    value_cells,
                );
            }
        }
        CompiledExprKind::Lambda { body, .. } => {
            expand_purpose_reflective_placeholders(body, queries, entity_ref, value_cells);
        }
        CompiledExprKind::ListLiteral(elements) | CompiledExprKind::SetLiteral(elements) => {
            for elem in elements {
                expand_purpose_reflective_placeholders(elem, queries, entity_ref, value_cells);
            }
        }
        CompiledExprKind::ReflectiveCellList(_) => {
            // Invariant (task-2544): RCL elements are `ValueRef`s by construction —
            // the only emission site is `CompiledExpr::reflective_cell_list(...)`
            // built from `CompiledExpr::value_ref(cell_id, elem_type)` in this same
            // function. Recursion would be a guaranteed no-op (`ValueRef` arm above
            // is empty), so we elide the loop and document the trust explicitly.
            // Originally task-2458 grouped RCL with the list/set traversal "for
            // completeness"; task-2544 reverted that conflation after review.
        }
        CompiledExprKind::MapLiteral(entries) => {
            for (key, val) in entries {
                expand_purpose_reflective_placeholders(key, queries, entity_ref, value_cells);
                expand_purpose_reflective_placeholders(val, queries, entity_ref, value_cells);
            }
        }
        CompiledExprKind::IndexAccess { object, index } => {
            expand_purpose_reflective_placeholders(object, queries, entity_ref, value_cells);
            expand_purpose_reflective_placeholders(index, queries, entity_ref, value_cells);
        }
        CompiledExprKind::MethodCall { object, args, .. } => {
            expand_purpose_reflective_placeholders(object, queries, entity_ref, value_cells);
            for arg in args {
                expand_purpose_reflective_placeholders(arg, queries, entity_ref, value_cells);
            }
        }
        CompiledExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            expand_purpose_reflective_placeholders(collection, queries, entity_ref, value_cells);
            expand_purpose_reflective_placeholders(predicate, queries, entity_ref, value_cells);
        }
        CompiledExprKind::OptionSome(inner) => {
            expand_purpose_reflective_placeholders(inner, queries, entity_ref, value_cells);
        }
        CompiledExprKind::RangeConstructor { lower, upper, .. } => {
            if let Some(lo) = lower {
                expand_purpose_reflective_placeholders(lo, queries, entity_ref, value_cells);
            }
            if let Some(hi) = upper {
                expand_purpose_reflective_placeholders(hi, queries, entity_ref, value_cells);
            }
        }
        CompiledExprKind::AdHocSelector { base, args, .. } => {
            expand_purpose_reflective_placeholders(base, queries, entity_ref, value_cells);
            for arg in args {
                expand_purpose_reflective_placeholders(arg, queries, entity_ref, value_cells);
            }
        }
        // task 3540 (SIR-α): exhaustiveness-forced adapter arm for the new
        // shared-enum variant (step-16). Recurse into the ctor's supplied
        // args + captured defaults so nested purpose-reflective placeholders
        // inside a structure constructor's argument expressions are still
        // expanded — same posture as the FunctionCall/UserFunctionCall arms.
        CompiledExprKind::StructureInstanceCtor {
            ordered_args,
            defaults,
            ..
        } => {
            for (_, arg) in ordered_args {
                expand_purpose_reflective_placeholders(arg, queries, entity_ref, value_cells);
            }
            for (_, def) in defaults {
                expand_purpose_reflective_placeholders(def, queries, entity_ref, value_cells);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests that drive `expand_purpose_reflective_placeholders`
    //! directly, bypassing `compile_purpose`. Useful for pinning contract
    //! invariants whose witness in an integration test would depend on
    //! incidental compiler behaviour (e.g. declaration-order preservation
    //! in `ResolvedSchemaQuery.resolved_ids`).
    use super::*;
    use crate::graph::ValueCellNode;

    /// Reviewer suggestion S2 (amendment round 2): pin the precedence
    /// contract — when a matching `ResolvedSchemaQuery` is supplied, its
    /// `resolved_ids` order wins over the alphabetical fallback scan of
    /// `value_cells`. Hand-crafting the inputs here means the test
    /// doesn't bake in `compile_purpose`'s incidental ordering: a future
    /// refactor that sorts inside the compiler would have made the
    /// integration-level witness vacuous (both paths would have produced
    /// the same alphabetical order), but this test stays sharp.
    #[test]
    fn expand_prefers_resolved_query_over_value_cells_scan() {
        let entity = "Foo";
        let cell_z = ValueCellId::new(entity, "z");
        let cell_a = ValueCellId::new(entity, "a");

        // Hand-crafted ResolvedSchemaQuery — order [z, a], NOT
        // alphabetical. The fallback scan would sort to [a, z], so the
        // two paths disagree on the witness.
        let queries = vec![ResolvedSchemaQuery {
            param_name: "subject".to_string(),
            query_kind: "params".to_string(),
            resolved_ids: vec![cell_z.clone(), cell_a.clone()],
        }];

        let mut value_cells: PersistentMap<ValueCellId, ValueCellNode> = PersistentMap::default();
        for cell in [&cell_a, &cell_z] {
            value_cells.insert(
                cell.clone(),
                ValueCellNode {
                    id: cell.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::Real,
                    default_expr: None,
                    content_hash: ContentHash::of_str(&cell.member),
                },
            );
        }

        let mut expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        expand_purpose_reflective_placeholders(&mut expr, &queries, entity, &value_cells);

        let elements = match &expr.kind {
            CompiledExprKind::ReflectiveCellList(elements) => elements,
            other => panic!(
                "task-2458: expected ReflectiveCellList after expansion, got {:?}",
                other
            ),
        };
        // `expand_purpose_reflective_placeholders` only ever emits `ValueRef`
        // elements inside a ReflectiveCellList (task-3508: CrossSubGeometryRef
        // is consumed by the entity.rs bare-let drop site before any reflective
        // list is built, so it cannot appear here by construction).
        let expanded_order: Vec<&str> = elements
            .iter()
            .map(|e| match &e.kind {
                CompiledExprKind::ValueRef(id) => id.member.as_str(),
                other => panic!("expected ValueRef element, got {:?}", other),
            })
            .collect();
        assert_eq!(
            expanded_order,
            vec!["z", "a"],
            "resolved-query path must preserve resolved_ids order; \
             a [a, z] result would indicate the fallback scan won precedence"
        );
    }

    /// Pin the element-type of the outer `Type::List` when a `ResolvedSchemaQuery` with
    /// an empty `resolved_ids` drives the empty-elements path.
    ///
    /// When `resolved_ids` is empty, `expand_purpose_reflective_placeholders` constructs a
    /// `ReflectiveCellList([])` and must set `expr.result_type` to
    /// `Type::List(Box::new(Type::Error))` — the anti-cascade element type for empty
    /// match-sets (task 3749, tightening of the 3639 Shape-A G-allow carve-out).
    ///
    /// The existing integration test `activate_expands_geometric_params_placeholder_to_empty_list`
    /// covers the same empty-elements path through `compile_purpose`, but only asserts
    /// `elements.is_empty()` — it does not pin the outer `Type::List` element-type contract.
    /// This unit test pins that contract independently of the integration path.
    ///
    /// Test fails RED before step-08 (current impl returns `Type::List(Box::new(Type::Real))`);
    /// passes GREEN after step-08 changes the fallback to `unwrap_or(Type::Error)`.
    #[test]
    fn expand_empty_resolved_query_yields_list_error_element_type() {
        let entity = "Foo";
        let queries = vec![ResolvedSchemaQuery {
            param_name: "subject".to_string(),
            query_kind: "params".to_string(),
            resolved_ids: vec![], // empty — triggers the empty-elements path
        }];
        let value_cells: PersistentMap<ValueCellId, ValueCellNode> = PersistentMap::default();
        let mut expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        expand_purpose_reflective_placeholders(&mut expr, &queries, entity, &value_cells);

        let elements = match &expr.kind {
            CompiledExprKind::ReflectiveCellList(elements) => elements,
            other => panic!(
                "expected ReflectiveCellList after expansion with empty resolved_ids, got {:?}",
                other
            ),
        };
        assert!(
            elements.is_empty(),
            "empty resolved_ids must produce an empty ReflectiveCellList"
        );
        // The anti-cascade element type must be Type::Error after task 3749 tightening.
        assert_eq!(
            expr.result_type,
            Type::List(Box::new(Type::Error)),
            "empty ReflectiveCellList must carry Type::Error element type (anti-cascade poison)"
        );
    }

    /// Shared fixture for the missing-cell branch tests.
    ///
    /// Returns `(entity, queries, value_cells, expr)` where `queries` references
    /// both a `"present"` and an `"absent"` cell, `value_cells` contains only
    /// the former (to trigger the missing-cell branch in
    /// `expand_purpose_reflective_placeholders`), and `expr` is a
    /// `PurposeReflectiveAggregation` placeholder for `("subject", "params",
    /// Type::List(Type::Real))`.
    fn missing_cell_fixture() -> (
        &'static str,
        Vec<ResolvedSchemaQuery>,
        PersistentMap<ValueCellId, ValueCellNode>,
        CompiledExpr,
    ) {
        let entity = "Foo";
        let cell_present = ValueCellId::new(entity, "present");
        let cell_absent = ValueCellId::new(entity, "absent");

        let queries = vec![ResolvedSchemaQuery {
            param_name: "subject".to_string(),
            query_kind: "params".to_string(),
            resolved_ids: vec![cell_present.clone(), cell_absent.clone()],
        }];

        let mut value_cells: PersistentMap<ValueCellId, ValueCellNode> = PersistentMap::default();
        value_cells.insert(
            cell_present.clone(),
            ValueCellNode {
                id: cell_present.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("present"),
            },
        );

        let expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        (entity, queries, value_cells, expr)
    }

    /// Guard: when a `ResolvedSchemaQuery` references a cell that is absent
    /// from `value_cells`, the missing-cell branch must emit exactly one WARN
    /// event scoped to `reify_eval::engine_purposes` (graph-vs-resolved-query
    /// wiring inconsistency signal).
    ///
    /// The test wraps the call in `catch_unwind` so it runs identically in
    /// both debug builds (where `debug_assert!(false)` panics) and release
    /// builds (where it does not). The WARN fires *before* the
    /// `debug_assert!`, so the counter increments regardless of build mode.
    ///
    /// **Dual-mode coverage:** the release-only structural assertions below
    /// (the `#[cfg(not(debug_assertions))]` block) are exercised in CI by
    /// the `cargo test -p reify-eval --release` pass in `orchestrator.yaml`'s
    /// `test_command` (second invocation in the two-pass debug+release pattern).
    /// The *debug-mode posture* (panic + no expr mutation) is pinned by the
    /// sibling test `expand_missing_cell_debug_mode_halts_via_debug_assert`,
    /// which runs automatically under default `cargo test -p reify-eval` in
    /// debug builds.
    #[test]
    fn expand_signals_when_resolved_query_cell_missing_from_value_cells() {
        use reify_test_support::CountingSubscriberBuilder;
        use std::panic::AssertUnwindSafe;
        use std::sync::atomic::Ordering;

        let (entity, queries, value_cells, mut expr) = missing_cell_fixture();

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .target_prefix("reify_eval::engine_purposes")
            .build();
        let warn_arc = counters[&tracing::Level::WARN].clone();

        // Wrap in catch_unwind so debug builds (debug_assert! panics) and
        // release builds both complete and let us read the warn counter.
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tracing::subscriber::with_default(subscriber, || {
                expand_purpose_reflective_placeholders(&mut expr, &queries, entity, &value_cells);
            });
        }));

        assert_eq!(
            warn_arc.load(Ordering::Acquire),
            1,
            "missing-cell wiring signal: expected exactly one WARN from \
             reify_eval::engine_purposes when a resolved-query cell is absent \
             from value_cells"
        );

        // S2 (amendment): pin the release-mode anti-cascade contract. In
        // debug builds the `debug_assert!(false)` inside
        // `expand_purpose_reflective_placeholders`'s missing-cell arm panics
        // during .collect(), so *expr is never reassigned and stays as
        // PurposeReflectiveAggregation — there is no post-call expanded state
        // to assert on. In release builds the expand call completes: verify
        // that both cells (the present one and the absent-fallback one) produce
        // ValueRef elements typed as Type::Real, and that the ListLiteral has
        // exactly 2 elements.
        //
        // CI dependency: this block executes only in release builds; it is
        // exercised by the `cargo test -p reify-eval --release` pass in
        // `orchestrator.yaml`'s `test_command` (second invocation in the
        // two-pass debug+release pattern). The *debug-mode posture* (panic +
        // no expr mutation + warn fires first) is pinned by the sibling test
        // `expand_missing_cell_debug_mode_halts_via_debug_assert`, which runs
        // automatically under default `cargo test -p reify-eval` in debug builds.
        #[cfg(not(debug_assertions))]
        {
            let elements = match &expr.kind {
                CompiledExprKind::ReflectiveCellList(elements) => elements,
                other => panic!(
                    "anti-cascade contract: expected ReflectiveCellList after expansion \
                     (task-2458), got {:?}",
                    other
                ),
            };
            assert_eq!(
                elements.len(),
                2,
                "anti-cascade contract: both resolved-query cells \
                 (present + absent-fallback) produce ValueRef elements"
            );
            // `expand_purpose_reflective_placeholders` only emits `ValueRef`
            // elements inside a ReflectiveCellList (task-3508: CrossSubGeometryRef
            // is consumed by entity.rs before any reflective list is built).
            for elem in elements {
                assert_eq!(
                    elem.result_type,
                    Type::Real,
                    "anti-cascade contract: missing-cell fallback must use \
                     Type::Real for the absent cell (elem kind: {:?})",
                    elem.kind
                );
                assert!(
                    matches!(&elem.kind, CompiledExprKind::ValueRef(_)),
                    "anti-cascade contract: each element must be a ValueRef, \
                     got {:?}",
                    elem.kind
                );
            }
        }
    }

    /// Dual-mode counterpart to `expand_signals_when_resolved_query_cell_missing_from_value_cells`.
    ///
    /// That sibling test asserts the WARN counter unconditionally and gates its
    /// structural (anti-cascade) assertions behind `#[cfg(not(debug_assertions))]`
    /// — those run in CI via the `cargo test -p reify-eval --release` pass in
    /// `orchestrator.yaml`'s `test_command` (second invocation in the two-pass
    /// debug+release pattern).
    ///
    /// This test pins the *debug-mode posture* explicitly:
    /// - `catch_unwind` must return `Err(_)` (the `debug_assert!(false, …)` inside
    ///   `expand_purpose_reflective_placeholders`'s missing-cell arm fires and
    ///   unwinds the thread).
    /// - The WARN counter must equal 1 — `tracing::warn!` fires *before* the
    ///   `debug_assert!`, matching the "warn first, debug_assert second, fallback
    ///   third" order documented in the contract preamble in
    ///   `expand_purpose_reflective_placeholders`.
    /// - `expr.kind` must remain `PurposeReflectiveAggregation` — the panic
    ///   occurs mid-`.collect()`, so `*expr = CompiledExpr::list_literal(…)`
    ///   never runs, and the expression retains its pre-call shape.
    ///
    /// Gated with `#[cfg(debug_assertions)]` because in release builds
    /// `debug_assert!(false)` is a no-op and `catch_unwind` would return `Ok`,
    /// causing the `result.is_err()` assertion to fail.
    ///
    /// Note: `catch_unwind` on the `debug_assert!(false)` panic may emit a panic
    /// stacktrace to stderr in default `cargo test` runs (the libtest panic hook
    /// fires before `catch_unwind` suppresses the unwind). This matches the
    /// sibling test's accepted behavior; correctness is unaffected.
    #[test]
    #[cfg(debug_assertions)]
    fn expand_missing_cell_debug_mode_halts_via_debug_assert() {
        use reify_test_support::CountingSubscriberBuilder;
        use std::panic::AssertUnwindSafe;
        use std::sync::atomic::Ordering;

        let (entity, queries, value_cells, mut expr) = missing_cell_fixture();

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .target_prefix("reify_eval::engine_purposes")
            .build();
        let warn_arc = counters[&tracing::Level::WARN].clone();

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tracing::subscriber::with_default(subscriber, || {
                expand_purpose_reflective_placeholders(&mut expr, &queries, entity, &value_cells);
            });
        }));

        assert!(
            result.is_err(),
            "debug-mode posture: expected a panic from the debug_assert!(false, …) \
             inside expand_purpose_reflective_placeholders's missing-cell arm when \
             a resolved-query cell is absent from value_cells, but catch_unwind \
             returned Ok(_)"
        );

        assert_eq!(
            warn_arc.load(Ordering::Acquire),
            1,
            "debug-mode posture: WARN must fire before the debug_assert! \
             (\"warn first, debug_assert second\" contract from the preamble in \
             expand_purpose_reflective_placeholders); \
             expected counter == 1 regardless of build mode"
        );

        assert!(
            matches!(
                &expr.kind,
                CompiledExprKind::PurposeReflectiveAggregation { .. }
            ),
            "debug-mode posture: panic mid-.collect() must prevent \
             `*expr = CompiledExpr::list_literal(…)` from running; \
             expr.kind must remain PurposeReflectiveAggregation, got {:?}",
            expr.kind
        );
    }

    /// Companion: when no matching `ResolvedSchemaQuery` is supplied
    /// (e.g. the wildcard-subject case), expansion falls back to a
    /// sorted scan of `value_cells` for the bound entity. This locks in
    /// the fallback's stable ordering — a regression that returned
    /// PersistentMap iteration order would break determinism.
    #[test]
    fn expand_falls_back_to_sorted_value_cells_scan_when_query_unresolved() {
        let entity = "Foo";
        let cell_z = ValueCellId::new(entity, "z");
        let cell_a = ValueCellId::new(entity, "a");

        // Empty queries — forces the fallback path for `params`.
        let queries: Vec<ResolvedSchemaQuery> = Vec::new();

        let mut value_cells: PersistentMap<ValueCellId, ValueCellNode> = PersistentMap::default();
        // Insert in non-alphabetical order to exercise the sort.
        for cell in [&cell_z, &cell_a] {
            value_cells.insert(
                cell.clone(),
                ValueCellNode {
                    id: cell.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::Real,
                    default_expr: None,
                    content_hash: ContentHash::of_str(&cell.member),
                },
            );
        }

        let mut expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        expand_purpose_reflective_placeholders(&mut expr, &queries, entity, &value_cells);

        let elements = match &expr.kind {
            CompiledExprKind::ReflectiveCellList(elements) => elements,
            other => panic!(
                "task-2458: expected ReflectiveCellList after expansion, got {:?}",
                other
            ),
        };
        // `expand_purpose_reflective_placeholders` only ever emits `ValueRef`
        // elements (task-3508: CrossSubGeometryRef is consumed by the entity.rs
        // bare-let drop site before any reflective list is built).
        let expanded_order: Vec<&str> = elements
            .iter()
            .map(|e| match &e.kind {
                CompiledExprKind::ValueRef(id) => id.member.as_str(),
                other => panic!("expected ValueRef element, got {:?}", other),
            })
            .collect();
        assert_eq!(
            expanded_order,
            vec!["a", "z"],
            "fallback scan must sort alphabetically for determinism"
        );
    }

    /// Returns `true` if any node anywhere in `expr`'s tree is a
    /// `PurposeReflectiveAggregation` node. Uses the canonical
    /// `CompiledExpr::walk` traversal so the check automatically follows new
    /// variants when the walker is updated.
    ///
    /// Note: this is an exhaustive walk (no short-circuit); trees exercised
    /// here are tiny so the cost is irrelevant in practice.
    fn tree_contains_placeholder(expr: &CompiledExpr) -> bool {
        let mut found = false;
        expr.walk(&mut |e: &CompiledExpr| {
            if matches!(
                e.kind,
                CompiledExprKind::PurposeReflectiveAggregation { .. }
            ) {
                found = true;
            }
        });
        found
    }

    /// Returns `true` if the tree rooted at `expr` contains a `ValueRef` for
    /// `entity/member`. Used to confirm that expansion produced the correct
    /// resolved node and not just some opaque substitution.
    fn tree_contains_value_ref_for(expr: &CompiledExpr, entity: &str, member: &str) -> bool {
        let mut found = false;
        expr.walk(&mut |e: &CompiledExpr| {
            if let CompiledExprKind::ValueRef(id) = &e.kind
                && id.entity == entity
                && id.member == member
            {
                found = true;
            }
        });
        found
    }

    type WrapFn = Box<dyn Fn(CompiledExpr) -> CompiledExpr>;

    /// Shared runner for the recursion regression-pin sweep.
    ///
    /// Builds the standard single-cell fixture (`entity = "Foo"`, resolved
    /// query `("subject", "params")` → `ValueCellId("Foo", "x")`), then for
    /// every `(label, wrap)` in `wrappers`:
    ///
    /// 1. Builds `wrap(placeholder)` where placeholder is a fresh
    ///    `PurposeReflectiveAggregation("subject", "params", List<Real>)`.
    /// 2. Calls `expand_purpose_reflective_placeholders`.
    /// 3. Asserts no `PurposeReflectiveAggregation` node remains anywhere in
    ///    the tree (recursion happened).
    /// 4. Asserts a `ValueRef("Foo", "x")` node is present (the expansion
    ///    produced the correct resolved node, not just some substitution).
    ///
    /// All failures are collected so every wrapper is exercised on a single
    /// run — a panic on the first failure would mask later regressions.
    fn assert_recursive_arms(wrappers: Vec<(&'static str, WrapFn)>) {
        let entity = "Foo";
        let cell_x = ValueCellId::new(entity, "x");
        let queries = vec![ResolvedSchemaQuery {
            param_name: "subject".to_string(),
            query_kind: "params".to_string(),
            resolved_ids: vec![cell_x.clone()],
        }];
        let mut value_cells: PersistentMap<ValueCellId, ValueCellNode> = PersistentMap::default();
        value_cells.insert(
            cell_x.clone(),
            ValueCellNode {
                id: cell_x.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("x"),
            },
        );
        let make_placeholder = || {
            CompiledExpr::purpose_reflective_aggregation(
                "subject".to_string(),
                "params".to_string(),
                Type::List(Box::new(Type::Real)),
            )
        };

        let mut failures: Vec<String> = Vec::new();
        for (label, wrap) in &wrappers {
            let mut wrapped = wrap(make_placeholder());
            expand_purpose_reflective_placeholders(&mut wrapped, &queries, entity, &value_cells);
            if tree_contains_placeholder(&wrapped) {
                failures.push(format!("{label}: placeholder not rewritten"));
            }
            if !tree_contains_value_ref_for(&wrapped, entity, "x") {
                failures.push(format!(
                    "{label}: expected ValueRef(Foo/x) in expanded tree"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "recursion regression failures:\n{}",
            failures.join("\n")
        );
    }

    /// Regression pin: `expand_purpose_reflective_placeholders` must recurse
    /// into every child slot of `Conditional` and `Match` nodes. A future
    /// refactor that drops any of these recursive arms would not be caught by
    /// the existing top-level tests (which only place the placeholder at the
    /// root).
    ///
    /// Acceptance-criterion literal pins: Conditional (all three child slots)
    /// and Match arm body.
    #[test]
    fn expand_recurses_through_branching_wrappers() {
        use reify_test_support::conditional_expr;
        use reify_ir::{CompiledMatchArm, Value};

        let wrappers: Vec<(&'static str, WrapFn)> = vec![
            (
                "Conditional condition",
                Box::new(|ph: CompiledExpr| {
                    conditional_expr(
                        ph,
                        CompiledExpr::literal(Value::Bool(true), Type::Bool),
                        CompiledExpr::literal(Value::Bool(true), Type::Bool),
                    )
                }),
            ),
            (
                "Conditional then_branch",
                Box::new(|ph: CompiledExpr| {
                    conditional_expr(
                        CompiledExpr::literal(Value::Bool(true), Type::Bool),
                        ph,
                        CompiledExpr::literal(Value::Bool(true), Type::Bool),
                    )
                }),
            ),
            (
                "Conditional else_branch",
                Box::new(|ph: CompiledExpr| {
                    conditional_expr(
                        CompiledExpr::literal(Value::Bool(true), Type::Bool),
                        CompiledExpr::literal(Value::Bool(true), Type::Bool),
                        ph,
                    )
                }),
            ),
            (
                "Match discriminant",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::match_expr(
                        ph,
                        vec![CompiledMatchArm {
                            patterns: vec!["_".to_string()],
                            body: CompiledExpr::literal(Value::Int(0), Type::Int),
                        }],
                        Type::Int,
                    )
                }),
            ),
            (
                "Match arm body",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::match_expr(
                        CompiledExpr::literal(Value::Int(0), Type::Int),
                        vec![CompiledMatchArm {
                            patterns: vec!["_".to_string()],
                            body: ph,
                        }],
                        Type::List(Box::new(Type::Real)),
                    )
                }),
            ),
        ];
        assert_recursive_arms(wrappers);
    }

    /// Regression pin: `expand_purpose_reflective_placeholders` must recurse
    /// into both child slots of `Quantifier` nodes (collection and predicate).
    ///
    /// Acceptance-criterion literal pin: nested Quantifier collection.
    /// The predicate position is included for symmetry — both recursive arms
    /// in the `Quantifier` match arm are exercised.
    #[test]
    fn expand_recurses_through_quantifier_components() {
        use reify_ast::QuantifierKind;
        use reify_ir::Value;

        let variable_id = ValueCellId::new("Q", "i");
        let wrappers: Vec<(&'static str, WrapFn)> = vec![
            (
                "Quantifier collection",
                Box::new({
                    let variable_id = variable_id.clone();
                    move |ph: CompiledExpr| {
                        CompiledExpr::quantifier(
                            QuantifierKind::ForAll,
                            "i".to_string(),
                            variable_id.clone(),
                            ph,
                            CompiledExpr::literal(Value::Bool(true), Type::Bool),
                        )
                    }
                }),
            ),
            (
                "Quantifier predicate",
                Box::new({
                    let variable_id = variable_id.clone();
                    move |ph: CompiledExpr| {
                        CompiledExpr::quantifier(
                            QuantifierKind::ForAll,
                            "i".to_string(),
                            variable_id.clone(),
                            CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Real))),
                            ph,
                        )
                    }
                }),
            ),
        ];
        assert_recursive_arms(wrappers);
    }

    /// Regression pin: `expand_purpose_reflective_placeholders` must recurse
    /// into every child slot of compound wrapper nodes — `BinOp` (left,
    /// right), `UnOp` (operand), `UserFunctionCall` (args), `Lambda` (body),
    /// `ListLiteral` (element), `SetLiteral` (element), `MapLiteral` (key,
    /// value), `IndexAccess` (object, index), `MethodCall` (object, args),
    /// `OptionSome` (inner), `RangeConstructor` (lower, upper), and
    /// `AdHocSelector` (base, args).
    ///
    /// Note: `FunctionCall` and `UserFunctionCall` share the exact same
    /// expansion arm (`FunctionCall { args, .. } | UserFunctionCall { args, .. }`),
    /// so the `UserFunctionCall arg` case exercises both code paths.
    #[test]
    fn expand_recurses_through_compound_wrappers() {
        use reify_ir::{BinOp, SelectorKind, UnOp, Value};

        let wrappers: Vec<(&'static str, WrapFn)> = vec![
            (
                "BinOp left",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::binop(
                        BinOp::Add,
                        ph,
                        CompiledExpr::literal(Value::Int(0), Type::Int),
                        Type::Int,
                    )
                }),
            ),
            (
                "BinOp right",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::binop(
                        BinOp::Add,
                        CompiledExpr::literal(Value::Int(0), Type::Int),
                        ph,
                        Type::Int,
                    )
                }),
            ),
            (
                "UnOp operand",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::unop(UnOp::Neg, ph, Type::List(Box::new(Type::Real)))
                }),
            ),
            (
                "UserFunctionCall arg",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::user_function_call("f".to_string(), vec![ph], Type::Bool)
                }),
            ),
            (
                "Lambda body",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::lambda(
                        vec![("x".to_string(), Some(Type::Real))],
                        vec![ValueCellId::new("L", "x")],
                        ph,
                        vec![],
                        Type::List(Box::new(Type::Real)),
                    )
                }),
            ),
            (
                "ListLiteral element",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::list_literal(
                        vec![ph],
                        Type::List(Box::new(Type::List(Box::new(Type::Real)))),
                    )
                }),
            ),
            (
                "SetLiteral element",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::set_literal(
                        vec![ph],
                        Type::Set(Box::new(Type::List(Box::new(Type::Real)))),
                    )
                }),
            ),
            (
                "MapLiteral key",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::map_literal(
                        vec![(ph, CompiledExpr::literal(Value::Int(0), Type::Int))],
                        Type::Map(
                            Box::new(Type::List(Box::new(Type::Real))),
                            Box::new(Type::Int),
                        ),
                    )
                }),
            ),
            (
                "MapLiteral val",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::map_literal(
                        vec![(CompiledExpr::literal(Value::Int(0), Type::Int), ph)],
                        Type::Map(
                            Box::new(Type::Int),
                            Box::new(Type::List(Box::new(Type::Real))),
                        ),
                    )
                }),
            ),
            (
                "IndexAccess object",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::index_access(
                        ph,
                        CompiledExpr::literal(Value::Int(0), Type::Int),
                        Type::Real,
                    )
                }),
            ),
            (
                "IndexAccess index",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::index_access(
                        CompiledExpr::literal(Value::Int(0), Type::Int),
                        ph,
                        Type::Real,
                    )
                }),
            ),
            (
                "MethodCall object",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::method_call(ph, "len".to_string(), vec![], Type::Int)
                }),
            ),
            (
                "MethodCall arg",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::method_call(
                        CompiledExpr::literal(Value::Int(0), Type::Int),
                        "to_str".to_string(),
                        vec![ph],
                        Type::Int,
                    )
                }),
            ),
            (
                "OptionSome inner",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::option_some(
                        ph,
                        Type::Option(Box::new(Type::List(Box::new(Type::Real)))),
                    )
                }),
            ),
            (
                "RangeConstructor lower",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::range_constructor(
                        Some(ph),
                        None,
                        true,
                        false,
                        Type::Range(Box::new(Type::Real)),
                    )
                }),
            ),
            (
                "RangeConstructor upper",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::range_constructor(
                        None,
                        Some(ph),
                        false,
                        true,
                        Type::Range(Box::new(Type::Real)),
                    )
                }),
            ),
            (
                "AdHocSelector base",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::ad_hoc_selector(ph, SelectorKind::Face, vec![])
                }),
            ),
            (
                "AdHocSelector arg",
                Box::new(|ph: CompiledExpr| {
                    CompiledExpr::ad_hoc_selector(
                        CompiledExpr::literal(Value::Int(0), Type::Int),
                        SelectorKind::Face,
                        vec![ph],
                    )
                }),
            ),
        ];
        assert_recursive_arms(wrappers);
    }

    /// task-2458 step-5 (RED): pins that `expand_purpose_reflective_placeholders`
    /// produces a `ReflectiveCellList` (not a `ListLiteral`) for a populated
    /// `subject.params` query. RED until step-6 switches the emission.
    ///
    /// CONTRACT: "task-2458: post-expansion shape must be `ReflectiveCellList`
    /// (not `ListLiteral`) so eval_quantifier's cell-iteration trigger fires only
    /// on placeholder-derived lists, not on user-written all-ValueRef
    /// ListLiterals."
    #[test]
    fn expand_emits_reflective_cell_list_for_subject_params() {
        let entity = "Foo";
        let cell_z = ValueCellId::new(entity, "z");
        let cell_a = ValueCellId::new(entity, "a");

        // Resolved query: [z, a] in that order (matches expand_prefers_resolved_query
        // fixture pattern).
        let queries = vec![ResolvedSchemaQuery {
            param_name: "subject".to_string(),
            query_kind: "params".to_string(),
            resolved_ids: vec![cell_z.clone(), cell_a.clone()],
        }];

        let mut value_cells: PersistentMap<ValueCellId, ValueCellNode> = PersistentMap::default();
        for cell in [&cell_z, &cell_a] {
            value_cells.insert(
                cell.clone(),
                ValueCellNode {
                    id: cell.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::Real,
                    default_expr: None,
                    content_hash: ContentHash::of_str(&cell.member),
                },
            );
        }

        let mut expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        expand_purpose_reflective_placeholders(&mut expr, &queries, entity, &value_cells);

        // task-2458: must be ReflectiveCellList, NOT ListLiteral.
        let elements = match &expr.kind {
            CompiledExprKind::ReflectiveCellList(elements) => elements,
            other => panic!(
                "task-2458: post-expansion shape must be `ReflectiveCellList` \
                 (not `ListLiteral`) so eval_quantifier's cell-iteration trigger \
                 fires only on placeholder-derived lists, not on user-written \
                 all-ValueRef ListLiterals; got {:?}",
                other
            ),
        };

        assert_eq!(
            elements.len(),
            2,
            "expanded ReflectiveCellList must have one element per resolved_id"
        );

        // `expand_purpose_reflective_placeholders` only ever emits `ValueRef`
        // elements (task-3508: CrossSubGeometryRef is consumed by the entity.rs
        // bare-let drop site before any reflective list is built).
        let members: Vec<&str> = elements
            .iter()
            .map(|e| match &e.kind {
                CompiledExprKind::ValueRef(id) => id.member.as_str(),
                other => panic!("expected ValueRef element, got {:?}", other),
            })
            .collect();
        assert_eq!(
            members,
            vec!["z", "a"],
            "resolved-query order must be preserved in the expanded ReflectiveCellList"
        );

        assert_eq!(
            expr.result_type,
            Type::List(Box::new(Type::Real)),
            "outer list result_type must be Type::List(Type::Real)"
        );
    }

    /// task-2458 step-5 (RED): pins that `expand_purpose_reflective_placeholders`
    /// produces an *empty* `ReflectiveCellList` (not a `ListLiteral`) for a
    /// `geometric_params` query where no resolved-query path and no fallback scan
    /// exists. The empty list falls through to value-iteration's vacuous-true
    /// path in `eval_quantifier`. RED until step-6 switches the emission.
    #[test]
    fn expand_emits_empty_reflective_cell_list_for_geometric_params() {
        let entity = "Foo";
        let cell_x = ValueCellId::new(entity, "x");

        // No query for "geometric_params" — geometric_params has no resolution
        // path yet (task-1904). The function emits an empty list.
        let queries: Vec<ResolvedSchemaQuery> = Vec::new();

        let mut value_cells: PersistentMap<ValueCellId, ValueCellNode> = PersistentMap::default();
        value_cells.insert(
            cell_x.clone(),
            ValueCellNode {
                id: cell_x.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("x"),
            },
        );

        let mut expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "geometric_params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        expand_purpose_reflective_placeholders(&mut expr, &queries, entity, &value_cells);

        // task-2458: must be ReflectiveCellList (empty), NOT ListLiteral.
        let elements = match &expr.kind {
            CompiledExprKind::ReflectiveCellList(elements) => elements,
            other => panic!(
                "task-2458: post-expansion shape must be `ReflectiveCellList` \
                 (not `ListLiteral`) even for the empty geometric_params case; \
                 got {:?}",
                other
            ),
        };

        assert!(
            elements.is_empty(),
            "geometric_params expansion must produce an empty ReflectiveCellList \
             (no resolution path exists yet, task-1904)"
        );
    }
}
