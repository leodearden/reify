// Split from lib.rs (task 2032) — eval methods.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::panic;
use std::sync::Arc;
use std::time::Instant;

use reify_compiler::{
    CompiledModule, TopologyTemplate, ValueCellDecl, ValueCellKind, find_template,
};
use reify_core::{
    ContentHash, Diagnostic, DiagnosticCode, DiagnosticLabel, FIELD_ENTITY_PREFIX, SnapshotId,
    SourceSpan, ValueCellId, VersionId,
};
use reify_ir::sampled::{LinspaceError, linspace_inclusive};
use reify_ir::{
    AutoParam, CompiledExpr, CompiledExprKind, CompiledFunction, DeterminacyState, ErrorRef,
    Freshness, InterpolationKind, ObjectiveProvenance, ObjectiveSense, ObjectiveSet, PersistentMap,
    ResolutionProblem, SampledField, SampledGridKind, SelectorKind, SnapshotProvenance,
    SolveResult, TermContribution, Value, ValueMap,
};

use crate::cache::{CachedResult, EvalOutcome, NodeId};
use crate::demand::DemandRegistry;
use crate::deps::{DependencyTrace, ReverseDependencyIndex, extract_dependency_trace, take_trace};
use crate::dirty::topological_sort;
use crate::engine_admin::{ParamOverrideRejection, validate_param_override};
use crate::engine_helpers::collect_member_list;
use crate::journal::{EvalEvent, EventKind, EventPayload};
use crate::snapshot::Snapshot;
use crate::unfold::{elaborate_child_instance, unfold_recursive_sub};
use crate::{
    CacheStats, CachedEvalResult, Engine, EvalResult, EvaluationState, GuardLookup, build_meta_map,
    eval_ctx_with_meta, guard_state_fingerprint, merge_functions,
};

/// Resolve a sub-component's `structure_name` against the user module's
/// templates first (module definitions shadow the prelude), then fall back to
/// the engine's compiled stdlib prelude modules.
///
/// Stdlib occurrence templates (e.g. `STLOutput`, `stdlib/io.ri`) live in
/// `Engine::prelude` and are deliberately NOT merged into
/// `CompiledModule::templates`, so a module-only lookup reports them as
/// unknown structures and `sub o = STLOutput(...)` never elaborates
/// (io-export δ / esc-4287-15). Used by BOTH resolver sites — the elaborating
/// sub-component loop in `eval()` and the validation-only mirror in
/// `eval_cached()` — which must agree on which names are unknown, or cached
/// re-evals emit false "unknown structure" errors.
///
/// `pub(crate)` so the io-export δ occurrence-driven export driver
/// ([`Engine::build_outputs`], `engine_build.rs`) resolves each `sub`'s
/// occurrence template through the SAME module-first/prelude-fallback rule the
/// evaluator uses — stdlib `Output` occurrence templates (`STLOutput` et al.)
/// live in the prelude, not `CompiledModule::templates`.
pub(crate) fn find_template_with_prelude<'a>(
    module: &'a CompiledModule,
    prelude: &'a [CompiledModule],
    name: &str,
) -> Option<&'a TopologyTemplate> {
    find_template(&module.templates, name).or_else(|| {
        prelude
            .iter()
            .find_map(|pm| find_template(&pm.templates, name))
    })
}

/// Sentinel substring included in every panic raised by
/// [`assert_value_cell_types_representable`].  Used by the unit test
/// (`invariant_tests::panics_on_unrepresentable_cell_types`) and integration
/// tests (via `reify_eval::ASSERT_MSG_PREFIX`) to assert the correct panic
/// path fired without relying on an exact message match.
///
/// The constant is intentionally always-available (no `cfg(debug_assertions)`)
/// so that `tests/value_cell_type_invariants.rs`, which runs in both debug and
/// release builds, can reference it as a single source of truth.
pub const ASSERT_MSG_PREFIX: &str = "unrepresentable cell_type";

/// Returns `true` when `ty` may legitimately appear as the `cell_type` of a
/// `ValueCellDecl` post-compilation. The variant the predicate *rejects* is
/// `Type::TypeParam(_)` — any non-Undef value supplied to a cell of that type
/// would fall through `value_type_kind_matches` (lib.rs) and trigger
/// `EngineError::TypeKindMismatch`.
///
/// Single source of truth shared by the runtime invariant
/// `assert_value_cell_types_representable` (this file) and the CI regression
/// walker `assert_template_cells_representable`
/// (`crates/reify-eval/tests/value_cell_type_invariants.rs`). Adding a third
/// rejected variant requires updating only this function.
///
/// `Type::StructureRef` (task 1876) and `Type::TraitObject` (task 2287) are
/// intentionally permitted despite also having no corresponding `Value` arm:
/// defaults for these cells evaluate to `Value::Undef`, which passes the
/// kind-match for any type (the `Value::Undef => true` arm of
/// `value_type_kind_matches`). If a `Value` variant is ever added for either,
/// add a matching arm in `value_type_kind_matches` and relax the runtime
/// assertion so the compiler enforces completeness.
///
/// Re-exported from the crate root with `#[doc(hidden)] pub use` so the
/// integration test crate can reach it; not part of the documented public API.
pub fn is_representable_cell_type(ty: &reify_core::Type) -> bool {
    use reify_core::Type;
    match ty {
        // Unrepresentable: no corresponding `Value` variant.
        Type::TypeParam(_) => false,
        // Compile-time-only — dimension-param scalar; erased before eval (D7/D1);
        // no `Value::ScalarParam` exists (task 4234 ε).
        Type::ScalarParam(_) => false,
        // Compile-time-only union — value cells must hold a single concrete
        // arm type post-narrowing (task 2373).
        Type::Union(_) => false,
        // Keyed sub-collection kind (task 3930 / β): structural — a `Keyed<T>`
        // sub lowers to a `SubComponentDecl` and is never held in a value cell;
        // no `Value::Keyed` exists. γ may revisit if it introduces a Value form.
        Type::Keyed(_) => false,
        // Assoc-type projection (task 4602 β): compile-time only — non-concrete
        // until base is resolved by normalize_type (leaf δ); no runtime form.
        Type::Projection { .. } => false,
        // Representable: every other variant that has (or may have) a
        // corresponding `Value`. Listed explicitly so that adding a new
        // `Type` variant to `reify_types` requires a conscious decision here
        // rather than silently inheriting `true`.
        Type::Bool
        | Type::Int
        | Type::String
        | Type::Scalar { .. }
        | Type::Enum(_)
        | Type::List(_)
        | Type::Set(_)
        | Type::Map(_, _)
        | Type::Option(_)
        | Type::Function { .. }
        | Type::StructureRef(_) // task 1876: struct-typed params are permitted
        | Type::TraitObject(_)
        | Type::Field { .. }
        | Type::Point { .. }
        | Type::Vector { .. }
        | Type::Tensor { .. }
        | Type::Complex(_)
        | Type::Orientation(_)
        | Type::Frame(_)
        | Type::Transform(_)
        | Type::AffineMap(_) // task 3958 / α: Value::AffineMap now exists
        | Type::Selector(_) // task 4116 / α: Value::Selector now exists
        | Type::AnySelector // task 4369 / A2: kind-agnostic selector cell (value is Value::Selector(k))
        | Type::Range(_)
        | Type::Plane
        | Type::Axis
        | Type::Direction
        // geometric-relations γ (task 4383): Undef-backed directive type — no
        // Value::Relation; relation cells default to Value::Undef (accepted by
        // value_type_kind_matches for any type) until ζ supplies relate-solve.
        | Type::Relation
        | Type::BoundingBox
        | Type::Matrix { .. }
        | Type::Geometry // task 3604 / GHR-β: Value::GeometryHandle now exists
        // Generic-applied type (task 4602 β): phantom args — runtime cell holds
        // a Value::StructureInstance identified by name (args erased at eval).
        | Type::Applied { .. }
        | Type::Error => true,
    }
}

/// Debug-only invariant check: assert that every `ValueCellNode` in the
/// evaluation graph has a representable `cell_type`. The rejected variant is
/// `Type::TypeParam` — any non-Undef value against such a cell triggers
/// `TypeKindMismatch`; see `value_type_kind_matches` in lib.rs.
///
/// `Type::StructureRef` (task 1876) and `Type::TraitObject` (task 2287) are
/// intentionally permitted despite also having no corresponding `Value` arm.
/// For StructureRef: user code may declare
/// `param material : Material = Material(...)` where `Material` is a
/// canonical struct. The struct-call default evaluates to `Value::Undef`
/// (structure constructors are not builtins; `reify_stdlib::eval_builtin`
/// returns Undef for unknown names), and Undef is accepted by the kind-match
/// against any `Type` variant. Trait-typed params are safe for the same
/// reason: their defaults evaluate to `Value::Undef`. If a future
/// `Value::TraitObjectInstance` or `Value::StructureInstance` variant is
/// added, add a matching arm in `value_type_kind_matches` and relax the
/// runtime assertion so the compiler enforces completeness.
///
/// Fully elided in release builds (cfg-gated, not debug_assert!-wrapped) to
/// avoid the HashMap walk on the hot eval() path.  Tests run under
/// cfg(debug_assertions) by default so the four unit tests in
/// `invariant_tests` below see this function normally.
///
/// Enforcement points: `engine_eval.rs` (eval cold-start), `engine_edit.rs:1207-1208`
/// (edit-time recompile), and `crates/reify-eval/tests/value_cell_type_invariants.rs`
/// (CI regression lock).
#[cfg(debug_assertions)]
pub(crate) fn assert_value_cell_types_representable(graph: &crate::graph::EvaluationGraph) {
    for (id, node) in graph.value_cells.iter() {
        assert!(
            is_representable_cell_type(&node.cell_type),
            "{}: value cell `{}` has cell_type {:?} post-compilation; \
             value_type_kind_matches treats these variants as having no Value counterpart — \
             see crates/reify-eval/tests/value_cell_type_invariants.rs",
            ASSERT_MSG_PREFIX,
            id,
            node.cell_type,
        );
    }
}

/// Populate a fresh `DemandRegistry` with the full per-node-kind demand
/// set for a graph and rebuild its cone.
///
/// This is the demand-initialization block shared by `Engine::eval` and
/// `Engine::edit_source`: every value cell, constraint, and realization
/// node in the graph is marked always-demanded, then `rebuild_cone` is
/// called so `is_demanded` reflects the transitive closure. Kept as a
/// single helper so a future node kind (e.g. Resolution, once it
/// participates in demand) is added once rather than drifting between
/// the two call sites.
pub(crate) fn build_demand_for_graph(graph: &crate::graph::EvaluationGraph) -> DemandRegistry {
    let mut demand = DemandRegistry::new();
    for (_, node) in graph.value_cells.iter() {
        demand.add_demand(NodeId::Value(node.id.clone()));
    }
    for (_, cnode) in graph.constraints.iter() {
        demand.add_demand(NodeId::Constraint(cnode.id.clone()));
    }
    for (_, rnode) in graph.realizations.iter() {
        demand.add_demand(NodeId::Realization(rnode.id.clone()));
    }
    demand.rebuild_cone(graph);
    demand
}

/// Re-evaluate a guard-group cell list in the post-solver pass.
///
/// For each cell:
/// - **Active branch** (`is_active_branch = true`): Param/Let cells are
///   re-evaluated from their `default_expr` (or set to `(Undef, Undetermined)`
///   when there is no default). Auto cells are skipped — the solver already
///   resolved them to concrete values; overwriting would destroy solver work.
/// - **Inactive branch** (`is_active_branch = false`): Non-Auto cells are
///   written to `(Undef, Undetermined)`. Auto cells are **skipped** — their
///   lifecycle is owned by the solver. See the canonical rule in
///   `engine_edit.rs`'s module-level `//!` doc and `deactivate_if_not_auto`.
///
/// Called twice per guarded group — once for `members` (active when
/// `guard = true`) and once for `else_members` (active when `guard = false`).
fn post_solver_re_eval_guard_cells(
    cells: &[ValueCellDecl],
    is_active_branch: bool,
    values: &mut ValueMap,
    snapshot_values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
) {
    for cell in cells {
        if is_active_branch {
            match cell.kind {
                ValueCellKind::Param | ValueCellKind::Let => {
                    if let Some(ref expr) = cell.default_expr {
                        let val = reify_expr::eval_expr(
                            expr,
                            &eval_ctx_with_meta(values, functions, meta_map)
                                .with_determinacy(snapshot_values),
                        );
                        values.insert(cell.id.clone(), val.clone());
                        snapshot_values
                            .insert(cell.id.clone(), (val, DeterminacyState::Determined));
                    } else {
                        values.insert(cell.id.clone(), Value::Undef);
                        snapshot_values.insert(
                            cell.id.clone(),
                            (Value::Undef, DeterminacyState::Undetermined),
                        );
                    }
                }
                ValueCellKind::Auto { .. } => {
                    // Active-branch Auto: skip.
                    // The solver already resolved these to concrete values;
                    // overwriting with Undef here would destroy solver work.
                    // An exhaustive match (rather than an implicit else-skip)
                    // ensures a future ValueCellKind variant triggers a
                    // compile error at this site, forcing a reviewed decision
                    // instead of a silent skip.
                }
            }
        } else if !cell.kind.is_auto() {
            // Inactive non-Auto: write (Undef, Undetermined).
            // Auto cells: skip — lifecycle owned by the solver.
            // Canonical rule: engine_edit.rs module-level doc / deactivate_if_not_auto.
            values.insert(cell.id.clone(), Value::Undef);
            snapshot_values.insert(
                cell.id.clone(),
                (Value::Undef, DeterminacyState::Undetermined),
            );
        }
    }
}

/// Engine-scoped state shared by [`eval_guarded_group_param_cell`] callers within `Engine::eval`'s third pass.
struct GuardedParamCtx<'a> {
    journal: &'a mut crate::journal::EventJournal,
    cache: &'a mut crate::cache::CacheStore,
    functions: &'a [CompiledFunction],
    meta_map: &'a HashMap<String, HashMap<String, String>>,
    version: VersionId,
    /// Mutable reference into `Engine::last_param_override_type_kind_rejections`.
    /// Incremented by `emit_param_override_rejection_warning` for the `TypeKindMismatch` arm.
    /// Disjoint borrow of a separate Engine field — coexists with the `journal`
    /// and `cache` mutable borrows above.
    type_kind_counter: &'a mut usize,
    /// Mutable reference into `Engine::last_param_override_dimension_rejections`.
    /// Incremented by `emit_param_override_rejection_warning` for the `ScalarDimensionMismatch` arm.
    dimension_counter: &'a mut usize,
}

/// Emit a `cache.record_evaluation` + journal `Completed` event pair for a
/// single Param-cell write path.  Centralises the five-line closing idiom
/// shared by `eval_guarded_group_param_cell` (four arms) and the top-level
/// Param S4 branch — extracted to eliminate future-drift risk (task-2195
/// amendment, reviewer suggestion).
///
/// `node_id` is consumed: cloned once into the cache call and moved into the
/// journal event.  `start` is the `Instant` captured before the matching
/// `EventKind::Started` record so that `Duration` spans the full resolution.
///
/// **Cache / journal**: the `outcome` field of the emitted
/// `EventKind::Completed` is forwarded directly from
/// `cache.record_evaluation`'s return value — `New` on a first record,
/// `Unchanged` on an identical re-record, and any other variant the cache may
/// produce.  Callers do not control the outcome; they only supply the
/// `CachedResult` value being stored.
#[inline]
fn record_eval_completed(
    journal: &mut crate::journal::EventJournal,
    cache: &mut crate::cache::CacheStore,
    node_id: NodeId,
    cached_result: CachedResult,
    version: VersionId,
    start: Instant,
) {
    let outcome = cache.record_evaluation(
        node_id.clone(),
        cached_result,
        version,
        DependencyTrace::default(),
    );
    journal.record(EvalEvent {
        timestamp: Instant::now(),
        node_id,
        kind: EventKind::Completed { outcome },
        version,
        payload: Some(EventPayload::Duration(start.elapsed())),
    });
}

/// Builds and topologically sorts the let-cell dependency graph for `template`,
/// pushes a `Diagnostic::error` if a cycle is detected, and returns
/// `(let_cells, let_traces, sorted_lets)`.
///
/// When a cycle exists, `sorted_lets` is the cycle-free topological prefix;
/// callers iterate it to evaluate the cycle-free portion.
fn detect_let_cycle<'a>(
    template: &'a reify_compiler::TopologyTemplate,
    diagnostics: &mut Vec<Diagnostic>,
) -> (
    HashMap<NodeId, &'a reify_ir::CompiledExpr>,
    HashMap<NodeId, DependencyTrace>,
    Vec<NodeId>,
) {
    let let_cells: HashMap<NodeId, &'a reify_ir::CompiledExpr> = template
        .value_cells
        .iter()
        .filter(|c| c.kind == ValueCellKind::Let)
        .filter_map(|c| {
            c.default_expr
                .as_ref()
                .map(|expr| (NodeId::Value(c.id.clone()), expr))
        })
        .collect();

    let let_node_ids: HashSet<NodeId> = let_cells.keys().cloned().collect();
    let let_traces: HashMap<NodeId, DependencyTrace> = let_cells
        .iter()
        .map(|(nid, expr)| (nid.clone(), extract_dependency_trace(expr)))
        .collect();

    let sorted_lets = topological_sort(&let_node_ids, &let_traces);

    // Detect cyclic let-binding dependencies: if topological_sort dropped nodes
    // (Kahn's algorithm silently omits nodes in cycles), report them.
    if sorted_lets.len() < let_node_ids.len() {
        // Build the set lazily here — sorted_lets is still iterable below.
        let sorted_set: HashSet<&NodeId> = sorted_lets.iter().collect();
        let mut cyclic_members: Vec<&str> = let_node_ids
            .iter()
            .filter(|nid| !sorted_set.contains(nid))
            .filter_map(|nid| match nid {
                NodeId::Value(vcid) => Some(vcid.member.as_str()),
                _ => None,
            })
            .collect();
        cyclic_members.sort();
        diagnostics.push(Diagnostic::error(format!(
            "circular let-binding dependency in template {}: [{}]",
            template.name,
            cyclic_members.join(", "),
        )));
    }

    (let_cells, let_traces, sorted_lets)
}

/// Build the combined Param+Let dependency graph for `template`, topologically
/// sort it, and emit two-tier cycle diagnostics (4a let-only, 4b cross-kind).
///
/// Shared between `evaluate_params_and_lets_unified` (fresh eval) and the
/// `eval_cached` unified pass. The `partial_map_skip` flag selects between the
/// two inclusion rules for Param cells:
///
/// * `true`  — eval path: skip Param cells where `!has_override && !has_default`
///   (PARTIAL-MAP invariant; those cells are intentionally absent from `values`).
/// * `false` — eval_cached path: include ALL Param cells (even
///   no-override/no-default ones), giving them an empty trace so the "always
///   writes a result" contract is preserved.
///
/// Returns `(combined_nodes, combined_traces, sorted_combined)` where
/// `sorted_combined` is in dependency order; nodes dropped by Kahn's algorithm
/// (cycle members) are absent from `sorted_combined` but present in
/// `combined_nodes`.
fn build_combined_param_let_graph(
    template: &reify_compiler::TopologyTemplate,
    param_overrides: &HashMap<ValueCellId, Value>,
    partial_map_skip: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> (
    HashSet<NodeId>,
    HashMap<NodeId, DependencyTrace>,
    Vec<NodeId>,
) {
    let mut combined_nodes: HashSet<NodeId> = HashSet::new();
    let mut combined_traces: HashMap<NodeId, DependencyTrace> = HashMap::new();

    for cell in &template.value_cells {
        if cell.kind.is_auto() {
            continue;
        }
        let node_id = NodeId::Value(cell.id.clone());
        match cell.kind {
            ValueCellKind::Param => {
                if partial_map_skip
                    && !param_overrides.contains_key(&cell.id)
                    && cell.default_expr.is_none()
                {
                    // PARTIAL-MAP: silently absent from the graph.
                    continue;
                }
                let trace = cell
                    .default_expr
                    .as_ref()
                    .map(extract_dependency_trace)
                    .unwrap_or_default();
                combined_nodes.insert(node_id.clone());
                combined_traces.insert(node_id, trace);
            }
            ValueCellKind::Let => {
                if let Some(ref expr) = cell.default_expr {
                    combined_nodes.insert(node_id.clone());
                    combined_traces.insert(node_id, extract_dependency_trace(expr));
                }
            }
            _ => {}
        }
    }

    let sorted_combined = topological_sort(&combined_nodes, &combined_traces);

    // ── Two-tier cycle diagnostics ──────────────────────────────────────────
    if sorted_combined.len() < combined_nodes.len() {
        let sorted_combined_set: HashSet<&NodeId> = sorted_combined.iter().collect();

        // 4a — let-only subgraph cycle check (preserves existing diagnostic shape).
        let let_node_ids: HashSet<NodeId> = template
            .value_cells
            .iter()
            .filter(|c| c.kind == ValueCellKind::Let && c.default_expr.is_some())
            .map(|c| NodeId::Value(c.id.clone()))
            .collect();
        let mut let_only_cyclic: HashSet<&NodeId> = HashSet::new();
        if !let_node_ids.is_empty() {
            let let_traces_check: HashMap<NodeId, DependencyTrace> = combined_traces
                .iter()
                .filter(|(k, _)| let_node_ids.contains(*k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let sorted_lets_check = topological_sort(&let_node_ids, &let_traces_check);
            if sorted_lets_check.len() < let_node_ids.len() {
                let sorted_let_set: HashSet<&NodeId> = sorted_lets_check.iter().collect();
                let mut cyclic_members: Vec<&str> = let_node_ids
                    .iter()
                    .filter(|nid| !sorted_let_set.contains(nid))
                    .filter_map(|nid| match nid {
                        NodeId::Value(vcid) => Some(vcid.member.as_str()),
                        _ => None,
                    })
                    .collect();
                for nid in let_node_ids.iter() {
                    if !sorted_let_set.contains(nid) {
                        let_only_cyclic.insert(nid);
                    }
                }
                cyclic_members.sort();
                diagnostics.push(Diagnostic::error(format!(
                    "circular let-binding dependency in template {}: [{}]",
                    template.name,
                    cyclic_members.join(", "),
                )));
            }
        }

        // 4b — cross-kind (param↔let) cycle.
        let cross_kind_cyclic: Vec<&str> = combined_nodes
            .iter()
            .filter(|nid| !sorted_combined_set.contains(nid) && !let_only_cyclic.contains(nid))
            .filter_map(|nid| match nid {
                NodeId::Value(vcid) => Some(vcid.member.as_str()),
                _ => None,
            })
            .collect();
        if !cross_kind_cyclic.is_empty() {
            let mut members = cross_kind_cyclic;
            members.sort();
            diagnostics.push(Diagnostic::error(format!(
                "circular dependency in template {}: [{}]",
                template.name,
                members.join(", "),
            )));
        }
    }

    (combined_nodes, combined_traces, sorted_combined)
}

/// Static coupling detection pass: walk `templates` in iteration order (which
/// mirrors the per-scope resolution order) and emit
/// [`DiagnosticCode::ScopeCoupling`] when a later scope's constraint or
/// objective reads an auto cell that was already frozen by an earlier scope.
///
/// Called in `Engine::eval` AFTER the resolution loop and OUTSIDE the
/// `has_active_solver` gate so the warning surfaces on `reify check` even
/// when no constraint solver is attached.
///
/// Algorithm:
/// - `frozen: HashMap<ValueCellId, String>` accumulates the auto-cell ids of
///   ALREADY-processed scopes, mapped to their owning scope name.
/// - For each scope B (in walk order): collect B's full read-set from
///   `template.constraints` (via `extract_dependency_trace`) — full, not
///   the auto-filtered ResolutionProblem set, because coupling edges are by
///   definition constraints that read a *different* scope's auto cell (which
///   `build_solver_problem`'s own-scope filter drops).
/// - For each read `r` where `frozen[r] == Some(A)` and `A != B.name`,
///   emit a warning (deduped per (A, B, r) triple via `seen`).
/// - THEN insert B's own auto cells into `frozen` (populate-after-process
///   ensures the earlier→later direction; a scope cannot couple to itself
///   or to a not-yet-frozen scope).
fn detect_scope_coupling(templates: &[reify_compiler::TopologyTemplate]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    // auto-cell id → owning scope name for ALREADY-processed scopes.
    let mut frozen: HashMap<ValueCellId, String> = HashMap::new();
    // Dedup set: (frozen_scope_name, later_scope_name, crossing_cell_id).
    let mut seen: HashSet<(String, String, ValueCellId)> = HashSet::new();

    for template in templates {
        let b_name = &template.name;

        // Shared helper: check `reads` against `frozen`, deduplicate per
        // (owner, B, crossing) triple, and push a W_SCOPE_COUPLING warning.
        // `span` is `Some(constraint.span)` for constraint-sourced crossings
        // (attaches a source label pointing at the crossing read site) and
        // `None` for objective-sourced crossings (ObjectiveTerm carries no
        // span).  The message literal lives here once to prevent drift.
        let mut emit_for_reads = |reads: Vec<ValueCellId>, span| {
            for r in reads {
                if let Some(owner) = frozen.get(&r)
                    && owner != b_name
                {
                    let key = (owner.clone(), b_name.clone(), r.clone());
                    if seen.insert(key) {
                        let msg = format!(
                            "W_SCOPE_COUPLING: scope '{b_name}' reads auto cell '{r}' \
                                 owned by already-resolved scope '{owner}'; \
                                 bottom-up resolution may be approximate"
                        );
                        let diag =
                            Diagnostic::warning(msg).with_code(DiagnosticCode::ScopeCoupling);
                        diagnostics.push(if let Some(s) = span {
                            diag.with_label(DiagnosticLabel::new(s, "scope coupling read site"))
                        } else {
                            diag
                        });
                    }
                }
            }
        };

        // Constraint read-sets: attach the constraint span as a source label.
        for constraint in &template.constraints {
            let reads = extract_dependency_trace(&constraint.expr).reads;
            emit_for_reads(reads, Some(constraint.span));
        }

        // Objective read-sets: ObjectiveTerm carries no span.
        if let Some(obj) = &template.objective {
            for term in &obj.terms {
                let reads = extract_dependency_trace(&term.expr).reads;
                emit_for_reads(reads, None);
            }
        }

        // After processing B, freeze B's own auto cells.
        for cell in &template.value_cells {
            if cell.kind.is_auto() {
                frozen.insert(cell.id.clone(), b_name.clone());
            }
        }
    }

    diagnostics
}

/// Shared implementation for scanning the top-level evaluated value-map for
/// error-Map diagnostics and emitting one [`Diagnostic`] per distinct error.
///
/// Called by [`detect_mechanism_errors`] and [`detect_nondriving_joint_errors`]
/// so the filter → sort → dedup → emit logic lives in one place.  Any future
/// provenance fix (e.g. attaching a source span or recording the originating
/// cell id in the error Map) only needs to be made here.
///
/// **Algorithm:**
/// 1. Filter `values` for `Value::Map` entries whose `error` field equals
///    `discriminator` (the cross-crate contract string produced by the
///    corresponding `make_*_error` function in `reify-stdlib`).  When `kind`
///    is `Some(k)`, the Map's `"kind"` field must additionally equal `k`.
///    This distinguishes producers that decorate a typed Map (e.g.
///    `make_duplicate_solid_error` clones a `kind="mechanism"` Map and stamps
///    `error="duplicate_solid"`) from producers that build a fresh, kind-less
///    error Map (`make_nondriving_joint_error`, which passes `None`).  Without
///    the guard a future error-Map type that happened to reuse a discriminator
///    string would be misattributed to the wrong detector.
/// 2. Sort by [`ValueCellId`] for deterministic ordering across hash-based
///    [`ValueMap`] iteration.
/// 3. Dedup by structural [`Value`] equality — one diagnostic per distinct
///    error Map.  An error event propagated verbatim to multiple cells collapses
///    to a single diagnostic; two independently-produced Maps that happen to be
///    structurally identical also collapse (under-reports by one).  Fixing this
///    would require per-call provenance tracking in the error Map; accepted as a
///    v0.1 limitation shared by all error-Map detectors.
///
/// Collect the [`SourceSpan`]s that the compiler already flagged with
/// [`DiagnosticCode::MechanismNonDrivingJoint`].
///
/// Used by [`detect_nondriving_joint_errors`] to build the suppression set:
/// if the compiler emitted a labelled `E_MECHANISM_NONDRIVING_JOINT` at a
/// given source span, the eval pass can skip re-emitting the same diagnostic
/// for any value cell whose `ValueCellDecl.span` matches that span.
///
/// Only diagnostics whose `code == Some(MechanismNonDrivingJoint)` *and* that
/// carry at least one [`DiagnosticLabel`] contribute to the set — unlabelled
/// diagnostics and diagnostics with a different code are ignored.  This is the
/// exact-span join key described in the task analysis.
fn nondriving_joint_compile_spans(diagnostics: &[Diagnostic]) -> HashSet<SourceSpan> {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::MechanismNonDrivingJoint))
        .flat_map(|d| d.labels.iter().map(|l| l.span))
        .collect()
}

/// 4. Emit `Diagnostic::error(msg).with_code(code)` per surviving entry,
///    using `error_message` from the Map or `fallback_msg` as a constant.
///
/// **No source span / [`DiagnosticLabel`]:** [`ValueCellId`] carries only
/// `{entity, member}` strings with no source-span, so no label is attached.
///
/// **Span-scoped suppression:** the optional `suppress` predicate is applied
/// BEFORE structural dedup (step 3).  When `Some(pred)` and `pred(cell_id)`
/// is `true`, that hit is skipped entirely.  This lets [`detect_nondriving_joint_errors`]
/// suppress cells whose `ValueCellDecl.span` already appears in the
/// compile-time `MechanismNonDrivingJoint` diagnostic set — implementing the
/// exact-span join described in task 4364 — while leaving
/// [`detect_mechanism_errors`] (which passes `None`) byte-for-byte unchanged.
fn detect_error_map_diagnostics(
    values: &ValueMap,
    kind: Option<&str>,
    discriminator: &str,
    code: DiagnosticCode,
    fallback_msg: &str,
    suppress: Option<&dyn Fn(&ValueCellId) -> bool>,
) -> Vec<Diagnostic> {
    let mut hits: Vec<(&ValueCellId, &Value)> = values
        .iter()
        .filter(|(_, v)| {
            let Value::Map(m) = v else {
                return false;
            };
            // Optional kind guard: when the producing `make_*_error` decorates
            // a typed Map (duplicate_solid clones a `kind="mechanism"` Map),
            // require the kind to match so an unrelated Map that happens to
            // reuse the `error` discriminator is not picked up by this detector.
            if let Some(kind) = kind {
                let kind_key = Value::String("kind".to_string());
                if !matches!(m.get(&kind_key), Some(Value::String(k)) if k == kind) {
                    return false;
                }
            }
            let error_key = Value::String("error".to_string());
            matches!(m.get(&error_key), Some(Value::String(e)) if e == discriminator)
        })
        .collect();
    // Sort by ValueCellId for deterministic ordering across hash-based ValueMap iteration.
    hits.sort_by_key(|(a, _)| *a);

    // Dedup by structural Value equality — one diagnostic per distinct error Map.
    let msg_key = Value::String("error_message".to_string());
    let mut seen = std::collections::BTreeSet::new();
    let mut diagnostics = Vec::new();
    for (cid, value) in hits {
        // Span-scoped suppression: skip this hit BEFORE the structural dedup when
        // the cell was already flagged at compile time at the exact same source span.
        if let Some(pred) = suppress
            && pred(cid)
        {
            continue;
        }
        if seen.insert(value.clone()) {
            let msg = match value {
                Value::Map(m) => match m.get(&msg_key) {
                    Some(Value::String(msg)) => msg.clone(),
                    _ => fallback_msg.to_string(),
                },
                _ => fallback_msg.to_string(),
            };
            diagnostics.push(Diagnostic::error(msg).with_code(code));
        }
    }
    diagnostics
}

/// Scan the evaluated value-map for duplicate-solid mechanism errors and emit
/// a typed [`Diagnostic`] for each distinct errored mechanism.
///
/// Called in both `eval` and `eval_cached` (eval/eval_cached diagnostic-parity
/// discipline) OUTSIDE the solver gate so the error surfaces on kernel-less
/// `reify check` and in the GUI panel.
///
/// Delegates to [`detect_error_map_diagnostics`] with `kind = Some("mechanism")`
/// and `discriminator = "duplicate_solid"`.  `make_duplicate_solid_error`
/// (mechanism.rs) clones a `kind="mechanism"` Map and stamps
/// `error="duplicate_solid"`, so BOTH fields are required to match — preserving
/// the original `is_duplicate_solid_mechanism` predicate's two-field contract.
///
/// **No source span:** [`ValueCellId`] carries no source-span; the `error_path1`
/// / `error_path2` fields in the error Map are empty `Value::List`s by the v0.1
/// convention.  A follow-up task could resolve the originating `body()` call's
/// span via the compiler's span table.
fn detect_mechanism_errors(values: &ValueMap) -> Vec<Diagnostic> {
    detect_error_map_diagnostics(
        values,
        Some("mechanism"),
        "duplicate_solid",
        DiagnosticCode::MechanismDuplicateSolid,
        "duplicate solid in mechanism",
        None,
    )
}

/// Scan top-level eval cells for `E_MECHANISM_NONDRIVING_JOINT` errors
/// (task 4309 — α), suppressing cells whose compile-time span was already
/// flagged by the compiler (task 4364).
///
/// Called in both `eval` and `eval_cached` outside the solver gate, mirroring
/// [`detect_mechanism_errors`].  Delegates to [`detect_error_map_diagnostics`]
/// with `kind = None` and `discriminator = "nondriving_joint"` — the `error`
/// field value produced by `make_nondriving_joint_error` in `joints.rs` and
/// returned by `bind`/`dim`/`sweep`/`sweep_grid` when a coupling or fixed joint
/// is passed.  `kind` is `None` because that producer builds a fresh error Map
/// with no `"kind"` field (only `error`/`error_message`/`joint`).
///
/// **Suppression predicate (task 4364):** builds the set of source spans the
/// compiler already flagged via [`nondriving_joint_compile_spans`], then builds
/// a `ValueCellId → vc.span` map from `module.templates[*].value_cells`.  A
/// cell whose `ValueCellDecl.span` is in the compile-span set is suppressed —
/// it was statically caught at compile time and re-emitting would produce a
/// duplicate diagnostic for the user.
///
/// Cells whose id is absent from `value_cells` (synthetic / sub-component
/// cells) and cells whose static type the compiler could not resolve to
/// `Coupling` (e.g. loop-bound `List<Joint>` elements) have no matching span
/// in the compile-span set and fall through to emit — preserving the runtime
/// defense-in-depth guard for cases the compiler cannot see statically.
fn detect_nondriving_joint_errors(values: &ValueMap, module: &CompiledModule) -> Vec<Diagnostic> {
    let compile_spans = nondriving_joint_compile_spans(&module.diagnostics);

    // Short-circuit: when no compile-time spans are flagged, nothing can be
    // suppressed.  Skip building the `cell_span` HashMap — an allocation that
    // walks every template's value_cells — and delegate directly with
    // `suppress = None`.  This is the common path for LSP/GUI incremental
    // eval (eval_cached runs per-keystroke; most edits don't involve a
    // non-driving-joint diagnostic).
    if compile_spans.is_empty() {
        return detect_error_map_diagnostics(
            values,
            None,
            "nondriving_joint",
            DiagnosticCode::MechanismNonDrivingJoint,
            "joint has no free motion variable (coupling or fixed)",
            None,
        );
    }

    let cell_span: HashMap<&ValueCellId, SourceSpan> = module
        .templates
        .iter()
        .flat_map(|t| t.value_cells.iter())
        .map(|vc| (&vc.id, vc.span))
        .collect();
    let pred = |cid: &ValueCellId| {
        cell_span
            .get(cid)
            .is_some_and(|s| compile_spans.contains(s))
    };
    detect_error_map_diagnostics(
        values,
        None,
        "nondriving_joint",
        DiagnosticCode::MechanismNonDrivingJoint,
        "joint has no free motion variable (coupling or fixed)",
        Some(&pred),
    )
}

/// Scan the post-evaluation value map for `@face` / `@edge` ad-hoc selector
/// cells that remain `Value::Undef`, and emit a `Diagnostic::warning` for each.
///
/// **Why this detector exists (task 250):** `engine.eval()` and `engine.eval_cached()`
/// are the geometry-free evaluation path — they never execute realizations and
/// therefore never call `post_process_ad_hoc_selectors` (the build-path resolver
/// in engine_build.rs/geometry_ops.rs).  As a result, any `@face` or `@edge`
/// cell whose compiled `default_expr` is `AdHocSelector{Face|Edge}` remains at
/// the `Value::Undef` placeholder that `eval_ad_hoc_selector` (reify-expr)
/// leaves behind, with no accompanying diagnostic.  This violates the task spec:
/// "If selector fails … port frame becomes undef, **diagnostic emitted**."
///
/// **Scope:** only `SelectorKind::Face` and `SelectorKind::Edge` are checked.
/// `@point` cells resolve to a `Value::Frame` in Layer-1 (no kernel) and never
/// match the `Undef` filter.  Body/other kinds are skipped.
///
/// **Severity: Warning** — matches the build-path failure arms in the
/// `extract_faces`/`extract_edges` error arms of `try_eval_ad_hoc_selector`
/// (geometry_ops.rs) and does not trip the many `errors.is_empty()` /
/// `severity == Error` guards in sibling tests, minimising fallout.
///
/// **Limitation — top-level exprs only:** the detector inspects only the
/// top-level `cell.default_expr.kind`.  An `@face`/`@edge` selector nested
/// inside a conditional, list literal, or method call is not detected.  This
/// is an accepted coverage gap for the current task scope; prefer documenting
/// over recursing via a full `CompiledExpr` traversal.
///
/// Called in both `eval()` and `eval_cached()` immediately before `EvalResult`
/// construction, mirroring the `detect_mechanism_errors` /
/// `detect_nondriving_joint_errors` placement.
fn detect_unresolved_ad_hoc_selectors(
    templates: &[reify_compiler::TopologyTemplate],
    values: &ValueMap,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for template in templates {
        for cell in &template.value_cells {
            // Only care about @face and @edge selectors.
            let selector_kind = match &cell.default_expr {
                Some(expr) => match &expr.kind {
                    CompiledExprKind::AdHocSelector { selector_kind, .. } => match selector_kind {
                        SelectorKind::Face | SelectorKind::Edge => *selector_kind,
                        // @point resolves in Layer-1 (no kernel) — skip.
                        // Body / other kinds are not selector-frame kinds — skip.
                        _ => continue,
                    },
                    _ => continue,
                },
                None => continue,
            };

            // Only emit when the cell is explicitly present in the value map AND
            // equals Value::Undef.  Absent cells may belong to nested / instantiated
            // sub-component templates whose values are keyed under instance-qualified
            // ids; treating absence as Undef would produce spurious warnings there.
            if !matches!(values.get(&cell.id), Some(Value::Undef)) {
                continue;
            }

            let kind_str = match selector_kind {
                SelectorKind::Face => "@face",
                SelectorKind::Edge => "@edge",
                // Already filtered above; unreachable.
                _ => continue,
            };

            let msg = format!(
                "{kind_str} selector could not be resolved to a frame during evaluation: \
                 selectors are only resolved on the build()/tessellate() path \
                 (selector frame is undef during eval)"
            );
            diagnostics.push(
                Diagnostic::warning(msg)
                    .with_label(DiagnosticLabel::new(cell.span, "selector frame is undef")),
            );
        }
    }

    diagnostics
}

/// Emit `DiagnosticCode::EvalUnresolved` (at Error severity) for every
/// **geometry-consumer** value cell that remains `Value::Undef` after a
/// kernel-less `eval()` or `eval_cached()`.
///
/// ## What fires
///
/// A "geometry consumer" is a builtin FunctionCall whose name is in the
/// `geometry_ops::is_geometry_consumer_call` allow-list (e.g. `adjacent_faces`,
/// `normal`, `closest_point`, `centroid`, `volume`, `area`, …).  These builtins
/// require a realized geometry kernel; on the pure value-eval surface they fall
/// through to `None`, leaving the cell at `Value::Undef`.  The error makes this
/// silent failure class loud (task #4651 R1a).
///
/// ## What does NOT fire
///
/// - Construction sites: `box()`, `cylinder()`, and similar geometry-constructor
///   names (GEOMETRY_FUNCTION_NAMES).
/// - Kernel-free leaf selector ctors: `faces`, `edges`, `faces_by_normal`, …
///   (the 9 R2b names that mint a symbolic `Value::Selector`).
/// - Composition/named-leaf ctors: `union`, `face`, `edge`, `solid_body`, …
/// - List/selection helpers: `single`.
/// - Non-FunctionCall cells, absent cells, and cells whose value is not Undef.
///
/// These are excluded by the positive allow-list in `is_geometry_consumer_call`
/// (derived from the TopologySelectorHelper consumer map and the R2b leaf set).
///
/// ## Relationship to `detect_unresolved_ad_hoc_selectors`
///
/// Structurally mirrors `detect_unresolved_ad_hoc_selectors` (same value_cells
/// iteration, same "present-in-map AND Value::Undef" guard, same cell.span
/// labelling, same dual call-site placement immediately after that call).
/// Diverges in severity: emits ERROR + EvalUnresolved (DD-4 / task #4651) rather
/// than Warning, because geometry-consumer Undefs are genuine errors, not editor
/// incompleteness.
///
/// ## Build-path disjointness (§6 no-double-fire)
///
/// `engine.build()` calls `check()` which calls `eval()` internally.  When
/// called from the build path, a geometry kernel IS registered (the caller used
/// `Engine::with_registered_kernel`), so the consumers will be realized by the
/// kernel after `eval()` returns — they are not errors.  The `kernel_less` flag
/// is `true` only when NO kernel is registered (the pure value-eval surface, e.g.
/// `reify check` or a test using `Engine::new(checker, None)`), which is the
/// sole correct firing context for this detector.  Passing `kernel_less: false`
/// suppresses the scan entirely, preserving §6 no-double-fire on the build path.
fn detect_unresolved_geometry_consumers(
    templates: &[reify_compiler::TopologyTemplate],
    values: &ValueMap,
    kernel_less: bool,
) -> Vec<Diagnostic> {
    // Only fire on the pure value-eval (kernel-less) surface.  When a geometry
    // kernel is registered the consumers will be resolved by build()/tessellate()
    // after eval() returns; emitting errors here would be false positives.
    if !kernel_less {
        return Vec::new();
    }
    let mut diagnostics = Vec::new();

    for template in templates {
        for cell in &template.value_cells {
            // Only care about geometry CONSUMER function calls.
            let expr = match &cell.default_expr {
                Some(expr) => expr,
                None => continue,
            };
            if !crate::geometry_ops::is_geometry_consumer_call(expr) {
                continue;
            }

            // Only emit when the cell is explicitly present in the value map AND
            // equals Value::Undef.  Absent cells may belong to nested/instantiated
            // sub-component templates whose values are keyed under instance-qualified
            // ids; treating absence as Undef would produce spurious errors there.
            if !matches!(values.get(&cell.id), Some(Value::Undef)) {
                continue;
            }

            // Extract the consumer function name for the error message.
            let consumer_name = match &expr.kind {
                CompiledExprKind::FunctionCall { function, .. } => function.name.as_str(),
                // is_geometry_consumer_call guarantees FunctionCall; unreachable.
                _ => "<geometry consumer>",
            };

            let msg = format!(
                "`{consumer_name}` could not be resolved: geometry-consumer builtins require a \
                 realized geometry kernel and are only resolvable on the build()/tessellate() \
                 path \u{2014} not on the pure value-eval surface (Engine::eval / eval_cached)"
            );
            diagnostics.push(
                Diagnostic::error(msg)
                    .with_code(DiagnosticCode::EvalUnresolved)
                    .with_label(DiagnosticLabel::new(
                        cell.span,
                        "geometry-derived input could not be resolved",
                    )),
            );
        }
    }

    diagnostics
}

/// Builds the `ResolutionProblem` for the constraint solver from `template`'s
/// auto-param cells and constraints, returning `None` when there are no auto
/// cells (signalling "skip solver invocation").
fn build_solver_problem(
    template: &reify_compiler::TopologyTemplate,
    values: &ValueMap,
    functions: Arc<[CompiledFunction]>,
) -> Option<ResolutionProblem> {
    // Collect auto cells once; derive both the id-set (for constraint
    // filtering) and the AutoParam list from the same filtered slice to
    // avoid walking value_cells twice.
    let auto_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|cell| cell.kind.is_auto())
        .collect();

    if auto_cells.is_empty() {
        return None;
    }

    let auto_ids: HashSet<&ValueCellId> = auto_cells.iter().map(|cell| &cell.id).collect();

    let filtered_constraints: Vec<_> = template
        .constraints
        .iter()
        .filter(|c| {
            let trace = extract_dependency_trace(&c.expr);
            trace.reads.iter().any(|r| auto_ids.contains(r))
        })
        .map(|c| (c.id.clone(), c.expr.clone()))
        .collect();

    let auto_param_list: Vec<AutoParam> = auto_cells
        .iter()
        .map(|cell| AutoParam {
            id: cell.id.clone(),
            param_type: cell.cell_type.clone(),
            bounds: None,
            free: cell.kind.is_auto_free(),
        })
        .collect();

    Some(ResolutionProblem {
        auto_params: auto_param_list,
        constraints: filtered_constraints,
        current_values: values.clone(),
        objective: template.objective.clone(),
        // Moved in by value — callers pass Arc::clone, so this is O(1).
        // The merged table is shared with Engine.functions (tasks #1997, #2286).
        functions,
    })
}

/// Recursively check whether a compiled expression contains any inequality
/// sub-expression (Ge/Gt/Le/Lt) at the top level or nested under BinOp::And.
///
/// **Intentional duplication**: `solver.rs::collect_slack_terms` applies the same
/// rule (same ops, same And-recursion, same skips).  The two cannot share a helper
/// because reify-eval src does not depend on reify-constraints (only a dev-dep).
/// If you change which ops decompose (e.g. add Or handling, treat Eq as two
/// inequalities), apply the matching change to `collect_slack_terms` as well.
fn has_inequality_slack(expr: &reify_ir::CompiledExpr) -> bool {
    match &expr.kind {
        reify_ir::CompiledExprKind::BinOp { op, left, right } => match op {
            reify_ir::BinOp::Ge
            | reify_ir::BinOp::Gt
            | reify_ir::BinOp::Le
            | reify_ir::BinOp::Lt => true,
            reify_ir::BinOp::And => has_inequality_slack(left) || has_inequality_slack(right),
            _ => false,
        },
        _ => false,
    }
}

/// Return `true` when a template qualifies for the default Chebyshev-centre
/// centrality objective synthesis (PRD η, I5 provenance hook).
///
/// The predicate mirrors `solver.rs::build_centrality_objective`'s gate:
///   1. At least one auto cell (otherwise `build_solver_problem` returns `None`).
///   2. **Continuous-only guard (B7)**: every auto cell has `Type::Scalar { .. }`.
///   3. At least one constraint contains an inequality slack (Ge/Gt/Le/Lt,
///      possibly nested under BinOp::And) — checked across ALL constraints,
///      NOT filtered by whether the constraint reads an auto cell.
///
/// **Why no auto-read filter (alignment with solver)**: `build_centrality_objective`
/// collects slacks from ALL constraints regardless of auto-cell involvement.
/// An earlier engine version filtered by `trace.reads ∩ auto_ids`, but this diverged
/// from the solver: a scope whose only inequality constraint involves no auto cell
/// would get a synthetic objective from the solver but be absent from
/// `centrality_synthesized_scopes` (under-reporting).  Removing the filter aligns
/// the two predicates.
///
/// **Known limitation (finite-bounds)**: `build_centrality_objective` also returns
/// `None` when any auto param has non-finite (NaN/Inf) effective bounds.  This check
/// cannot be replicated here because `ValueCellDecl` does not carry numeric bounds
/// (they are derived from runtime values in `build_solver_problem`).  In the rare
/// case where bounds are degenerate the engine over-reports (records the scope as
/// centrality-synthesized even though the solver returns `None`), but this is a
/// benign inaccuracy and the scope is otherwise a degenerate problem.
///
/// Cross-reference: `solver.rs::build_centrality_objective`.
fn scope_qualifies_for_centrality(template: &reify_compiler::TopologyTemplate) -> bool {
    let auto_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|cell| cell.kind.is_auto())
        .collect();

    if auto_cells.is_empty() {
        return false;
    }

    // Continuous-only guard: all auto cells must have a Scalar type.
    if !auto_cells
        .iter()
        .all(|cell| matches!(cell.cell_type, reify_core::Type::Scalar { .. }))
    {
        return false;
    }

    // At least one constraint (anywhere in the scope) must contain an inequality.
    // We do NOT filter by auto-cell reads — see the doc comment above.
    template
        .constraints
        .iter()
        .any(|c| has_inequality_slack(&c.expr))
}

/// Pushes the appropriate `Diagnostic::warning` for `rejection` and bumps the
/// corresponding test counter (`type_kind_counter` for `TypeKindMismatch`,
/// `dimension_counter` for `ScalarDimensionMismatch`).
///
/// Call whenever `validate_param_override` returns `Err(rejection)`.
fn emit_param_override_rejection_warning(
    diagnostics: &mut Vec<Diagnostic>,
    cell_id: &ValueCellId,
    cell_type: &reify_core::Type,
    override_val: &Value,
    rejection: &ParamOverrideRejection,
    type_kind_counter: &mut usize,
    dimension_counter: &mut usize,
) {
    match rejection {
        ParamOverrideRejection::TypeKindMismatch => {
            *type_kind_counter += 1;
            diagnostics.push(Diagnostic::warning(format!(
                "param_override for `{}` skipped: type-kind mismatch (expected {}, got value {})",
                cell_id, cell_type, override_val
            )));
        }
        ParamOverrideRejection::ScalarDimensionMismatch { expected, got } => {
            *dimension_counter += 1;
            diagnostics.push(Diagnostic::warning(format!(
                "param_override for `{}` skipped: dimension mismatch (expected {}, got {})",
                cell_id, expected, got
            )));
        }
    }
}

/// Resolve and write the effective value for a guarded-group Param cell,
/// consulting `param_overrides`, validating any stored override, falling back
/// to `cell.default_expr`, and handling the no-override / rejected-override
/// cases symmetrically with the inactive-member write-Undef treatment.
///
/// Called from the `members` loop (when `guard_is_true`) and the
/// `else_members` loop (when `guard_is_false`) inside the third pass of
/// [`Engine::eval`].  Centralising the resolution block here means any future
/// change to the validation policy (a new [`ParamOverrideRejection`] variant,
/// a different Diagnostic shape, a journal/cache hook) only has to be made in
/// one place — the triple-copy divergence that produced the guarded-group
/// override bug (task 2154) cannot recur.
///
/// **Cache / journal**: every value-write path in this helper records a
/// `Started` event before resolution and a `Completed { outcome }` event after
/// calling `cache.record_evaluation`, mirroring the top-level Param branch in
/// `Engine::eval`'s first pass — the symmetric S4 arm is tagged
/// `REJECTED-OVERRIDE-NO-DEFAULT`, and both sites funnel through the shared
/// `record_eval_completed` helper. Task-2195 added journal+cache recording
/// here to make guarded-group Param evals fully visible to tooling that joins
/// journal events against cache state.
fn eval_guarded_group_param_cell(
    cell: &ValueCellDecl,
    param_overrides: &HashMap<ValueCellId, Value>,
    registry: &reify_ir::StructureRegistry,
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    diagnostics: &mut Vec<Diagnostic>,
    ctx: &mut GuardedParamCtx<'_>,
) {
    let node_id = NodeId::Value(cell.id.clone());
    let start = Instant::now();
    ctx.journal.record(EvalEvent {
        timestamp: start,
        node_id: node_id.clone(),
        kind: EventKind::Started,
        version: ctx.version,
        payload: None,
    });

    let override_val = match param_overrides.get(&cell.id) {
        None => {
            // No override stored AND no default_expr: write (Undef, Undetermined)
            // and return early.  This mirrors the inactive-member treatment a few
            // lines below in the calling loop (the `else` arm that writes Undef
            // for deactivated cells) — NOT the top-level Param branch's
            // no-override-no-default `None` arm (search `PARTIAL-MAP INVARIANT`
            // in this file), which bare-continues without inserting into `values`
            // (a pre-task-2017 baseline preserved to avoid a cross-cutting
            // behaviour change). Guarded-group cells always write Undef so all
            // cells appear in EvalResult.values regardless of override presence.
            if cell.default_expr.is_none() {
                values.insert(cell.id.clone(), Value::Undef);
                snapshot.values.insert(
                    cell.id.clone(),
                    (Value::Undef, DeterminacyState::Undetermined),
                );
                record_eval_completed(
                    ctx.journal,
                    ctx.cache,
                    node_id,
                    CachedResult::Value(Value::Undef, DeterminacyState::Undetermined),
                    ctx.version,
                    start,
                );
                return;
            }
            None
        }
        Some(v) => match validate_param_override(v, &cell.cell_type, registry) {
            Ok(()) => Some(v.clone()),
            Err(ref rejection) => {
                emit_param_override_rejection_warning(
                    diagnostics,
                    &cell.id,
                    &cell.cell_type,
                    v,
                    rejection,
                    ctx.type_kind_counter,
                    ctx.dimension_counter,
                );
                None
            }
        },
    };

    let val = if let Some(v) = override_val {
        v
    } else if let Some(ref expr) = cell.default_expr {
        reify_expr::eval_expr(
            expr,
            &eval_ctx_with_meta(values, ctx.functions, ctx.meta_map)
                .with_determinacy(&snapshot.values),
        )
    } else {
        // Override existed but was rejected AND no default_expr.
        // Write (Undef, Undetermined) into both maps so external readers of
        // EvalResult.values see a well-defined Undef instead of a missing key.
        // Record in cache + journal — mirrors the top-level S4 arm (task-2195).
        values.insert(cell.id.clone(), Value::Undef);
        snapshot.values.insert(
            cell.id.clone(),
            (Value::Undef, DeterminacyState::Undetermined),
        );
        record_eval_completed(
            ctx.journal,
            ctx.cache,
            node_id,
            CachedResult::Value(Value::Undef, DeterminacyState::Undetermined),
            ctx.version,
            start,
        );
        return;
    };

    // Override-accepted or default-eval path: write determined value.
    values.insert(cell.id.clone(), val.clone());
    snapshot
        .values
        .insert(cell.id.clone(), (val.clone(), DeterminacyState::Determined));
    record_eval_completed(
        ctx.journal,
        ctx.cache,
        node_id,
        CachedResult::Value(val, DeterminacyState::Determined),
        ctx.version,
        start,
    );
}

/// Elaborate a single compiled field into its `Value::Field` runtime
/// representation against the supplied `values`/`functions`/`meta_map`.
///
/// Used by both the cold-start field-elaboration loop in `Engine::eval` and
/// the incremental composed-field re-elaboration in `Engine::edit_param`.
/// Centralising the build keeps the two call sites bit-identical: the
/// `Value::Field { lambda, source, domain_type, codomain_type }` produced
/// at edit time is exactly what the cold path would have produced for the
/// same `field` and `values`.
///
/// Both `Analytical` and `Composed` sources evaluate the lambda expression
/// once against the current eval context, producing a `Value::Lambda` whose
/// captures are taken from the supplied `values` map.
///
/// `Sampled` fields (task 2341, Option B 5-key surface): each compiled
/// config expression (`grid` / `bounds` / `spacing` / `interpolation` /
/// `data`) is evaluated against the current context, the resulting
/// `Value`s are parsed into a `SampledField`, and that `SampledField` is
/// wrapped as `Arc::new(Value::SampledField(sf))`. On any parse failure
/// the field's lambda becomes `Arc::new(Value::Undef)` — a poisoned
/// no-op that produces `Undef` at every sample point.
///
/// `Imported` fields (task 3576 step-8): call `reify_kernel_openvdb::read_vdb_file`
/// with the compiled path and grid name.  On success, wrap the resulting
/// `SampledField` as `Value::SampledField`; on error push a
/// `DiagnosticCode::FieldImportFailed` runtime error into `runtime_sink` and
/// return `Value::Undef`.  IngestOutcome warnings are forwarded to `runtime_sink`.
/// Elaborate a compiled field declaration into a runtime [`Value::Field`].
///
/// ## Return value
///
/// Returns `(value, content_hash)` where `content_hash` is `Some` for
/// `Imported` sources whose file was successfully read for hashing, and
/// `None` for all other source kinds (or when the hash read fails).  The
/// caller (typically `Engine::eval`) stores this hash into the
/// `CacheStore` imported-file side-table so subsequent evals can detect
/// file-content changes without a second `fs::read` call — see
/// `CacheStore::record_imported_file_hash`.
///
/// Computing the hash here (inside the same elaboration step that calls
/// `read_vdb_file`) avoids a redundant `fs::read` in the caller.  The
/// VDB library issues its own file I/O internally through the C++ boundary
/// so the bytes cannot be shared, but the separate top-level `fs::read`
/// that the caller would otherwise perform is eliminated.
pub(crate) fn elaborate_field(
    field: &reify_compiler::CompiledField,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    runtime_sink: Option<&RefCell<Vec<Diagnostic>>>,
) -> (Value, Option<ContentHash>) {
    // For Imported sources, the hash is computed alongside the VDB read so the
    // caller can record it without issuing a separate fs::read.
    let mut imported_hash: Option<ContentHash> = None;

    let lambda_value = match &field.source {
        reify_compiler::CompiledFieldSource::Analytical { expr }
        | reify_compiler::CompiledFieldSource::Composed { expr } => {
            let mut ctx = eval_ctx_with_meta(values, functions, meta_map);
            if let Some(sink) = runtime_sink {
                ctx = ctx.with_runtime_diagnostics(sink);
            }
            let val = reify_expr::eval_expr(expr, &ctx);
            Arc::new(val)
        }
        reify_compiler::CompiledFieldSource::Sampled { config } => {
            let mut ctx = eval_ctx_with_meta(values, functions, meta_map);
            if let Some(sink) = runtime_sink {
                ctx = ctx.with_runtime_diagnostics(sink);
            }
            match build_sampled_field(&field.name, config, &ctx) {
                Some(sf) => Arc::new(Value::SampledField(sf)),
                None => Arc::new(Value::Undef),
            }
        }
        reify_compiler::CompiledFieldSource::Imported { path, grid, .. } => {
            // Call read_vdb_file with the compiled path and grid name.
            // Both cfg branches (real FFI and stub) return Result<IngestOutcome, IngestError>;
            // errors surface as FieldImportFailed runtime diagnostics + Value::Undef.
            match (path, grid) {
                (Some(p), Some(g)) => {
                    // Hash the raw file bytes here so the caller has the hash without
                    // issuing a separate fs::read after elaboration returns.  IO errors
                    // (e.g. file not found) leave imported_hash as None; the VDB read
                    // below will then also fail and emit FieldImportFailed.
                    imported_hash = hash_imported_file_content(p).ok();
                    match reify_kernel_openvdb::read_vdb_file(p, g, &field.codomain_type) {
                        Ok(outcome) => {
                            // Surface any ingest warnings (e.g. unit mismatch) into the sink.
                            if !outcome.warnings.is_empty()
                                && let Some(sink) = runtime_sink
                            {
                                sink.borrow_mut().extend(outcome.warnings);
                            }
                            Arc::new(Value::SampledField(outcome.field))
                        }
                        Err(e) => {
                            if let Some(sink) = runtime_sink {
                                sink.borrow_mut().push(
                                    reify_core::Diagnostic::error(format!(
                                        "field '{}': failed to import VDB file: {}",
                                        field.name, e
                                    ))
                                    .with_code(reify_core::DiagnosticCode::FieldImportFailed),
                                );
                            }
                            Arc::new(Value::Undef)
                        }
                    }
                }
                // Missing path or grid: a compiler error was already emitted;
                // silently produce Undef at eval time (compiler error is the user-visible signal).
                _ => Arc::new(Value::Undef),
            }
        }
    };

    let source_kind = match &field.source {
        reify_compiler::CompiledFieldSource::Analytical { .. } => {
            reify_ir::FieldSourceKind::Analytical
        }
        reify_compiler::CompiledFieldSource::Sampled { .. } => reify_ir::FieldSourceKind::Sampled,
        reify_compiler::CompiledFieldSource::Composed { .. } => reify_ir::FieldSourceKind::Composed,
        reify_compiler::CompiledFieldSource::Imported { .. } => reify_ir::FieldSourceKind::Imported,
    };

    (
        Value::Field {
            domain_type: field.domain_type.clone(),
            codomain_type: field.codomain_type.clone(),
            source: source_kind,
            lambda: lambda_value,
        },
        imported_hash,
    )
}

/// Hash the raw bytes of an imported field source file.
///
/// ## Contract
///
/// Hashes file **bytes only** — the path string is intentionally NOT mixed
/// into the hash domain, so two distinct paths whose contents are byte-identical
/// produce the same [`reify_types::ContentHash`].  This directly implements the
/// PRD acceptance property "file-path change with same content → cache hit"
/// (PRD task 4 / task 2668, `docs/prds/v0_2/imported-field-source.md`).
///
/// The three-contract pin for this function lives in
/// `imported_file_hash_tests::hash_imported_file_content_byte_round_trip_path_independent_and_propagates_io_error`.
///
/// ## IO error propagation
///
/// Any [`std::io::Error`] from [`std::fs::read`] is propagated as-is, preserving
/// the original [`std::io::ErrorKind`] (e.g. `NotFound`, `PermissionDenied`).
/// PRD task 5's wire site in `elaborate_field` is responsible for converting
/// these into user-visible [`reify_types::Diagnostic`] errors with the appropriate
/// `DiagnosticCode` — this helper stays as a minimal kernel.
///
/// ## PRD acceptance properties
///
/// - File-content change → different hash → `CacheStore::imported_file_hash_changed` returns
///   `true` → cache invalidation signal (wired by PRD task 5).
/// - File-path change with same content → same hash → `imported_file_hash_changed` returns
///   `false` → cache hit.
pub(crate) fn hash_imported_file_content(path: &str) -> std::io::Result<reify_core::ContentHash> {
    // Perf note: `fs::read` allocates a `Vec<u8>` sized to the full file before
    // hashing.  For multi-MB .vdb assets on the hot evaluation path this is a noticeable
    // allocation per call.  If `ContentHash` (or `xxhash_rust::xxh3`) later exposes an
    // incremental/streaming constructor, replace this with `BufReader` + chunk-by-chunk
    // update to avoid the temporary buffer.  The wire site in PRD task 5 is the natural
    // place to evaluate whether the allocation cost is measurable in practice.
    Ok(reify_core::ContentHash::of(&std::fs::read(path)?))
}

/// Look up a config entry by key.  `compile_field` validated that all five
/// required keys are present, so a missing key here would indicate a bug
/// upstream — the helper returns `None` rather than panicking so callers
/// can degrade gracefully.
fn lookup_config<'a>(
    config: &'a [(String, reify_ir::CompiledExpr)],
    key: &str,
) -> Option<&'a reify_ir::CompiledExpr> {
    config.iter().find(|(k, _)| k == key).map(|(_, e)| e)
}

/// Build a runtime `SampledField` from the five-key compiled config. Returns
/// `None` if any value fails to parse — caller wraps `None` as
/// `Arc::new(Value::Undef)` (poisoned no-op).
///
/// On every parse failure a `W_FIELD_SAMPLED_INVALID_CONFIG` warning is
/// pushed via [`push_invalid_config`] into `ctx.diagnostics` before
/// returning `None` — the user sees a concrete message naming the field,
/// the offending key, and (where applicable) the allowed-set hint or
/// the unexpected `Value` shape rendered by [`short_value`]. Parsers
/// (Option B per esc-2341-149):
///
/// * `grid` — `Value::String` matching `"RegularGrid1"|"RegularGrid2"|"RegularGrid3"`
/// * `bounds` — `Value::BoundingBox` with `Point3` corners; per-axis count is
///   `1`/`2`/`3` for `Regular1D`/`2D`/`3D` (the parser projects extra
///   z/y components for lower-dimensional fields, matching how `bbox(...)`
///   today only constructs 3D bounding boxes).
/// * `spacing` — `Value::Scalar { Length }` for `Regular1D`; `Value::List` of
///   `N` Length scalars for higher dimensions.
/// * `interpolation` — `Value::String` matching one of `"Linear"`,
///   `"NearestNeighbor"`, `"Cubic"`, `"Rbf"`, `"Kriging"`.
/// * `data` — `Value::List` whose elements are `Value::Real`, `Value::Int`,
///   or any `Value::Scalar` (its `si_value` is taken as-is and reinterpreted
///   as the field's codomain at sample time — no per-element dimension
///   check is performed here); flattened into row-major SI.
///
/// Missing-key short-circuits (`?` on `lookup_config`) remain silent: the
/// compile-time validator already emits a hard error for any missing
/// required key, so reaching this branch indicates an upstream bug rather
/// than a user-visible config error.
///
/// Once all five values parse, three runtime-invariant checks gate
/// `Some(SampledField{…})` construction.  Each violation emits a
/// `W_FIELD_SAMPLED_INVALID_CONFIG` warning and returns `None`,
/// short-circuiting before any `interp::interpolate_Nd` call could
/// trip its assertions:
///
/// 1. **Spacing is positive and finite per axis.**  Rejects
///    `0.0`, negative, `NaN`, and `±∞` spacings up front so
///    `linspace_inclusive`'s defensive 1-node fallback never fires.
/// 2. **Each axis grid has ≥ 2 nodes.**  Pre-empts
///    `interp::interpolate_Nd`'s `assert!(grid.len() >= 2)` on
///    degenerate axes (e.g. zero-length bounds span).
/// 3. **`data.len() == product(axis_grids[i].len())`.**  Pre-empts
///    `interp::interpolate_Nd`'s grid-vs-values length-equality
///    `assert!` on flatten/shape mismatches.
fn build_sampled_field(
    name: &str,
    config: &[(String, reify_ir::CompiledExpr)],
    ctx: &reify_expr::EvalContext<'_>,
) -> Option<SampledField> {
    let grid_expr = lookup_config(config, "grid")?;
    let bounds_expr = lookup_config(config, "bounds")?;
    let spacing_expr = lookup_config(config, "spacing")?;
    let interp_expr = lookup_config(config, "interpolation")?;
    let data_expr = lookup_config(config, "data")?;

    let grid_val = reify_expr::eval_expr(grid_expr, ctx);
    let bounds_val = reify_expr::eval_expr(bounds_expr, ctx);
    let spacing_val = reify_expr::eval_expr(spacing_expr, ctx);
    let interp_val = reify_expr::eval_expr(interp_expr, ctx);
    let data_val = reify_expr::eval_expr(data_expr, ctx);

    let kind = match parse_grid_kind(&grid_val) {
        Some(k) => k,
        None => {
            push_invalid_config(
                ctx,
                format!(
                    "sampled field '{name}': invalid grid kind: expected 'RegularGrid1' | 'RegularGrid2' | 'RegularGrid3', got {}",
                    short_value(&grid_val)
                ),
            );
            return None;
        }
    };
    let (bounds_min, bounds_max) = match parse_bounds(&bounds_val, kind) {
        Some(b) => b,
        None => {
            push_invalid_config(
                ctx,
                format!(
                    "sampled field '{name}': invalid bounds: expected BoundingBox with Point3 corners (min, max) carrying at least {} component(s), got {}",
                    axis_count_for(kind),
                    short_value(&bounds_val)
                ),
            );
            return None;
        }
    };
    let spacing = match parse_spacing(&spacing_val, kind) {
        Some(s) => s,
        None => {
            push_invalid_config(
                ctx,
                format!(
                    "sampled field '{name}': invalid spacing: expected {}, got {}",
                    spacing_hint_for(kind),
                    short_value(&spacing_val)
                ),
            );
            return None;
        }
    };
    let interpolation = match parse_interpolation(&interp_val) {
        Some(i) => i,
        None => {
            push_invalid_config(
                ctx,
                format!(
                    "sampled field '{name}': invalid interpolation: expected one of 'Linear' | 'NearestNeighbor' | 'Cubic' | 'Rbf' | 'Kriging', got {}",
                    short_value(&interp_val)
                ),
            );
            return None;
        }
    };
    let data = match parse_data(&data_val) {
        Some(d) => d,
        None => {
            push_invalid_config(
                ctx,
                format!(
                    "sampled field '{name}': invalid data: expected list of Real / Int / Scalar elements, got {}",
                    short_value(&data_val)
                ),
            );
            return None;
        }
    };

    // Runtime-invariant checks (step-24): three pre-flight guards that
    // prevent `interp::interpolate_Nd`'s `assert!`s from panicking the
    // eval loop on a malformed config that parsed clean but violates
    // the interpolation primitives' contracts.  Each guard emits a
    // `W_FIELD_SAMPLED_INVALID_CONFIG` warning naming the offending
    // axis / slot and returns `None` so the field's lambda becomes
    // `Value::Undef`.  Tests in
    // `crates/reify-eval/tests/field_eval_tests.rs` (step-23) pin
    // each of the three failure modes.

    // (1) Spacing must be positive and finite per axis.  The
    //     `linspace_inclusive` helper has a defensive
    //     `spacing <= 0.0 || !spacing.is_finite()` branch that
    //     collapses bad spacings to a 1-node grid; downstream
    //     `interp::interpolate_Nd` would then trip
    //     `assert!(grid.len() >= 2)`.  Reject up front so the user
    //     gets a precise message naming the offending axis.
    for (i, s) in spacing.iter().enumerate() {
        if !(*s > 0.0 && s.is_finite()) {
            push_invalid_config(
                ctx,
                format!(
                    "sampled field '{name}': invalid spacing: axis {i} spacing must be positive and finite, got {s}"
                ),
            );
            return None;
        }
    }

    let mut axis_grids: Vec<Vec<f64>> = Vec::with_capacity(bounds_min.len());
    for i in 0..bounds_min.len() {
        match linspace_inclusive(bounds_min[i], bounds_max[i], spacing[i]) {
            Ok(g) => axis_grids.push(g),
            Err(LinspaceError::Excessive { n_intervals }) => {
                push_invalid_config(
                    ctx,
                    format!(
                        "sampled field '{name}': axis {i} requires {n_intervals} grid intervals, \
                         exceeds the {} interval cap \
                         (bounds_min={} bounds_max={} spacing={}); \
                         reduce the span or increase the spacing",
                        reify_ir::sampled::LINSPACE_MAX_INTERVALS,
                        bounds_min[i],
                        bounds_max[i],
                        spacing[i],
                    ),
                );
                return None;
            }
            Err(LinspaceError::Overflow) => {
                push_invalid_config(
                    ctx,
                    format!(
                        "sampled field '{name}': axis {i} requires more intervals than usize \
                         can represent \
                         (bounds_min={} bounds_max={} spacing={}); \
                         reduce the span or increase the spacing",
                        bounds_min[i], bounds_max[i], spacing[i],
                    ),
                );
                return None;
            }
        }
    }

    // (2) Each axis grid must have ≥ 2 nodes.  A degenerate axis
    //     (e.g. zero-length bounds span, or `bounds_min > bounds_max`
    //     hitting `linspace_inclusive`'s `span < 0.0` branch) collapses
    //     to a single node and would later trip
    //     `assert!(grid.len() >= 2)` inside `interp::interpolate_Nd`.
    for (i, axis) in axis_grids.iter().enumerate() {
        if axis.len() < 2 {
            push_invalid_config(
                ctx,
                format!(
                    "sampled field '{name}': axis {i} grid has only {} node(s); need at least 2 (check bounds and spacing — bounds_min={} bounds_max={} spacing={})",
                    axis.len(),
                    bounds_min[i],
                    bounds_max[i],
                    spacing[i]
                ),
            );
            return None;
        }
    }

    // (3) `data.len()` must equal the product of per-axis grid sizes
    //     (row-major flattening).  A mismatch would later trip the
    //     length-equality `assert!` inside
    //     `interp::interpolate_Nd`.  The shape rendering uses ‘×’ so
    //     a 3-D grid prints as e.g. `4×5×6`.
    let expected: usize = axis_grids.iter().map(|g| g.len()).product();
    if data.len() != expected {
        let shape = axis_grids
            .iter()
            .map(|g| g.len().to_string())
            .collect::<Vec<_>>()
            .join("×");
        push_invalid_config(
            ctx,
            format!(
                "sampled field '{name}': data length {} does not match grid shape ({shape}); expected {expected} elements (row-major, axis-0 outermost)",
                data.len()
            ),
        );
        return None;
    }

    Some(SampledField {
        name: name.to_string(),
        kind,
        bounds_min,
        bounds_max,
        spacing,
        axis_grids,
        interpolation,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    })
}

/// Push a `W_FIELD_SAMPLED_INVALID_CONFIG` warning into `ctx.diagnostics`.
/// Silent-drop when the runtime sink is `None` (matching the OOB and
/// interpolation-deferred warning emission contract).
fn push_invalid_config(ctx: &reify_expr::EvalContext<'_>, msg: String) {
    if let Some(sink) = ctx.diagnostics {
        let diag = Diagnostic::warning(msg).with_code(DiagnosticCode::FieldSampledInvalidConfig);
        sink.borrow_mut().push(diag);
    }
}

/// Concise rendering of a `Value` for `W_FIELD_SAMPLED_INVALID_CONFIG`
/// diagnostic messages.  Output is bounded (≤80 bytes total, ellipsis
/// included) so a runaway `List`/`Tensor`/`Map` doesn't flood the
/// diagnostic stream.
///
/// Wraps the value's `Debug` rendering (variant + content) and elides the
/// tail with `…` past the byte budget.  For `Value::String` the rendering
/// keeps the literal so the user can see the offending tag (e.g.
/// `String("RegularGrid42")`).
fn short_value(v: &Value) -> String {
    const MAX: usize = 80;
    const ELLIPSIS: &str = "…";
    let raw = format!("{v:?}");
    if raw.len() <= MAX {
        raw
    } else {
        // UTF-8 safe truncation: reserve room for the ellipsis suffix so
        // the total output stays within MAX bytes, then walk back to a
        // char boundary.
        let mut cut = MAX.saturating_sub(ELLIPSIS.len());
        while cut > 0 && !raw.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}{ELLIPSIS}", &raw[..cut])
    }
}

/// Number of axes carried by the given `SampledGridKind` — used to render
/// "expected at least N component(s)" hints in the bounds parse-failure
/// diagnostic.
fn axis_count_for(kind: SampledGridKind) -> usize {
    match kind {
        SampledGridKind::Regular1D => 1,
        SampledGridKind::Regular2D => 2,
        SampledGridKind::Regular3D => 3,
    }
}

/// Allowed-shape hint for the `spacing = …` parse-failure diagnostic.
fn spacing_hint_for(kind: SampledGridKind) -> &'static str {
    match kind {
        SampledGridKind::Regular1D => "Length scalar (e.g. 1.0m)",
        SampledGridKind::Regular2D => "list of 2 Length scalars (e.g. [1.0m, 1.0m])",
        SampledGridKind::Regular3D => "list of 3 Length scalars (e.g. [1.0m, 1.0m, 1.0m])",
    }
}

/// Map a `grid = …` value to the spatial-grid kind tag.  Accepts
/// `Value::String` matching `"RegularGrid1"|"RegularGrid2"|"RegularGrid3"`
/// (case-sensitive).
fn parse_grid_kind(grid_val: &Value) -> Option<SampledGridKind> {
    match grid_val {
        Value::String(s) => match s.as_str() {
            "RegularGrid1" => Some(SampledGridKind::Regular1D),
            "RegularGrid2" => Some(SampledGridKind::Regular2D),
            "RegularGrid3" => Some(SampledGridKind::Regular3D),
            _ => None,
        },
        _ => None,
    }
}

/// Map a `bounds = …` value (a `Value::BoundingBox`) to `(bounds_min, bounds_max)`
/// per-axis SI coordinates.  For `Regular1D`/`2D`, projects extra components
/// of the 3-component `Point3` corners.
fn parse_bounds(bounds_val: &Value, kind: SampledGridKind) -> Option<(Vec<f64>, Vec<f64>)> {
    let (min_pt, max_pt) = match bounds_val {
        Value::BoundingBox { min, max } => (min.as_ref(), max.as_ref()),
        _ => return None,
    };
    let axis_count = match kind {
        SampledGridKind::Regular1D => 1,
        SampledGridKind::Regular2D => 2,
        SampledGridKind::Regular3D => 3,
    };
    let min_axes = extract_point_axes(min_pt, axis_count)?;
    let max_axes = extract_point_axes(max_pt, axis_count)?;
    Some((min_axes, max_axes))
}

/// Extract the first `axis_count` SI scalar coordinates from a `Value::Point`.
/// Each component must be a `Value::Scalar` (returns its `si_value`) or
/// `Value::Real` (returned as-is; bare-Real points are dimensionless).
fn extract_point_axes(point: &Value, axis_count: usize) -> Option<Vec<f64>> {
    let items = match point {
        Value::Point(items) => items,
        _ => return None,
    };
    if items.len() < axis_count {
        return None;
    }
    let mut axes = Vec::with_capacity(axis_count);
    for item in items.iter().take(axis_count) {
        match item {
            Value::Scalar { si_value, .. } => axes.push(*si_value),
            Value::Real(v) => axes.push(*v),
            _ => return None,
        }
    }
    Some(axes)
}

/// Map a `spacing = …` value to per-axis SI scalars. `Regular1D` accepts
/// a `Value::Scalar` (Length-dimensioned); higher dimensions accept a
/// `Value::List` of `N` length scalars.
fn parse_spacing(spacing_val: &Value, kind: SampledGridKind) -> Option<Vec<f64>> {
    match kind {
        SampledGridKind::Regular1D => match spacing_val {
            Value::Scalar { si_value, .. } => Some(vec![*si_value]),
            Value::Real(v) => Some(vec![*v]),
            _ => None,
        },
        SampledGridKind::Regular2D | SampledGridKind::Regular3D => {
            let n = if matches!(kind, SampledGridKind::Regular2D) {
                2
            } else {
                3
            };
            let items = match spacing_val {
                Value::List(items) => items,
                _ => return None,
            };
            if items.len() != n {
                return None;
            }
            let mut spacing = Vec::with_capacity(n);
            for item in items {
                match item {
                    Value::Scalar { si_value, .. } => spacing.push(*si_value),
                    Value::Real(v) => spacing.push(*v),
                    _ => return None,
                }
            }
            Some(spacing)
        }
    }
}

/// Map an `interpolation = …` value to the language-level kind tag.
fn parse_interpolation(interp_val: &Value) -> Option<InterpolationKind> {
    match interp_val {
        Value::String(s) => match s.as_str() {
            "Linear" => Some(InterpolationKind::Linear),
            "NearestNeighbor" => Some(InterpolationKind::NearestNeighbor),
            "Cubic" => Some(InterpolationKind::Cubic),
            "Rbf" => Some(InterpolationKind::Rbf),
            "Kriging" => Some(InterpolationKind::Kriging),
            _ => None,
        },
        _ => None,
    }
}

/// Map a `data = …` value to a flat row-major `Vec<f64>` in SI units.
/// Accepts a `Value::List` whose elements are `Value::Real`, `Value::Int`
/// (whole-number literals like `0.0` may collapse to `Int` per
/// `expr.rs:257-258`), or any `Value::Scalar` (its `si_value` is taken
/// as-is and reinterpreted as the field's codomain at sample time —
/// no per-element dimension check against the field's codomain is
/// performed here).
fn parse_data(data_val: &Value) -> Option<Vec<f64>> {
    let items = match data_val {
        Value::List(items) => items,
        _ => return None,
    };
    let mut data = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::Real(v) => data.push(*v),
            Value::Int(n) => data.push(*n as f64),
            Value::Scalar { si_value, .. } => data.push(*si_value),
            _ => return None,
        }
    }
    Some(data)
}

// ── undef-self-describing α (task 4321): UndefCause classifier ───────────────

/// Populate `map` with `detail` for every auto param in `auto_params`.
///
/// Called from the `Infeasible` and `NoProgress` resolution-loop arms so the
/// two near-identical capture loops don't drift independently.
fn record_failed_autos(
    map: &mut HashMap<ValueCellId, String>,
    auto_params: &[AutoParam],
    detail: &str,
) {
    for ap in auto_params {
        map.insert(ap.id.clone(), detail.to_owned());
    }
}

/// γ (task 4323): post-eval pass — record `OpContractFailed` causes for undef cells
/// whose `default_expr` returned `Value::Undef` with ALL inputs determined.
///
/// Called immediately after [`classify_undef_origins`] inside the
/// `if self.capture_undef_causes` block.  For each cell that:
///
/// - is undef in `snap_values`,
/// - has **no** cause yet recorded by α (`!causes.contains_key(id)`),
/// - has a `decl` in `decls`, and
/// - has `Some(default_expr)`,
///
/// it re-evaluates `default_expr` via `reify_expr::eval_expr` with a fresh
/// undef-cause sink attached (no diagnostics sink — existing diagnostics must
/// not double-emit).  If the sink receives any `OpContractFailed` push (from
/// `push_op_contract_failure` in reify-expr), we insert
/// `UndefCause::OpContractFailed { code, span: decl.span }` into `causes` —
/// re-stamping the span with the cell's declaration span because
/// `CompiledExpr` carries no span (spans are lost at compile).
///
/// **Re-eval faithfulness**: value cells are already evaluated via
/// `reify_expr::eval_expr` in `evaluate_params_and_lets_unified`, so re-eval
/// reproduces main-eval exactly for value cells.  Geometry occurrences are not
/// value cells and are NOT reached here (OUT OF SCOPE for γ; they would need
/// a separate capture-during-geometry-eval mechanism).
///
/// **A1/G3 structural transparency**: `snap_values` and `decls` are read-only;
/// this function only modifies `causes`.  The hot eval loop remains completely
/// untouched; all pushes are no-ops when no sink is attached (the normal path).
fn record_op_contract_failures(
    causes: &mut HashMap<ValueCellId, reify_ir::UndefCause>,
    snap_values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    decls: &HashMap<&ValueCellId, &reify_compiler::ValueCellDecl>,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
) {
    use reify_ir::UndefCause;

    for (id, (val, _)) in snap_values.iter() {
        // Skip determined cells.
        if !val.is_undef() {
            continue;
        }
        // Skip cells already classified by α (Unbound, UserUndef, AwaitingSolve, SolveFailed).
        if causes.contains_key(id) {
            continue;
        }
        // Skip cells with no decl (synthetic/guard/list/sub-elaborated cells).
        let Some(decl) = decls.get(id) else {
            continue;
        };
        // Skip cells with no default_expr (required params — α would have
        // recorded Unbound for them; if somehow missed, skip gracefully).
        let Some(default_expr) = &decl.default_expr else {
            continue;
        };

        // Re-evaluate the default_expr with a fresh undef-cause sink to detect
        // genuine op/builtin contract failures with ALL inputs determined.
        // No diagnostics sink — α's existing diagnostics must not double-emit.
        let sink: RefCell<Vec<UndefCause>> = RefCell::new(Vec::new());
        let ctx = eval_ctx_with_meta(values, functions, meta_map)
            .with_determinacy(snap_values)
            .with_undef_cause_sink(&sink);
        let _ = reify_expr::eval_expr(default_expr, &ctx);

        // If the sink has any OpContractFailed, record the first one for this
        // cell — re-stamping the span with decl.span (CompiledExpr has no span).
        let sink_borrow = sink.borrow();
        for cause in sink_borrow.iter() {
            if let UndefCause::OpContractFailed { code, .. } = cause {
                causes.insert(
                    id.clone(),
                    UndefCause::OpContractFailed {
                        code: *code,
                        span: decl.span,
                    },
                );
                break;
            }
        }
    }
}

/// Classify the origin of every undef cell in `snap_values`, returning a map
/// from originating-cell id to its `UndefCause`.
///
/// # Classification rules (A2)
///
/// For each cell `(id, (val, det))` in `snap_values` where `val.is_undef()`:
///
/// 1. **No matching decl** — synthetic guard/list/sub-elaborated cells have no
///    entry in `decls`.  Skipped silently to avoid false `Unbound` causes.
///
/// 2. **Propagated undef** (A3) — if any direct input cell (from
///    `extract_value_deps(default_expr)`) is undef or absent in `snap_values`,
///    the cell's undef status is fully explained by its inputs.  Record nothing.
///
/// 3. **Originating undef** — all direct inputs are determined:
///    - `det == Auto | Provisional`: solver variable.
///      - id in `solve_failed_autos` → [`UndefCause::SolveFailed`] with the
///        coarse detail string captured from the actual `SolveResult`.
///      - otherwise → [`UndefCause::AwaitingSolve`].
///    - `default_expr == Some(Literal(Value::Undef))` → [`UndefCause::UserUndef`].
///    - `default_expr == None` → [`UndefCause::Unbound`].
///    - `default_expr` is non-undef and all inputs are determined → the
///      `OpContractFailed` case owned by task γ; record nothing.
///
/// # A1 guarantee
///
/// This function only reads `snap_values` and `decls`; it never modifies them.
/// The caller's `snapshot.values` is untouched, making capture-on == capture-off
/// for every `(Value, DeterminacyState)` and content-hash structural.
fn classify_undef_origins(
    snap_values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    decls: &HashMap<&ValueCellId, &reify_compiler::ValueCellDecl>,
    solve_failed_autos: &HashMap<ValueCellId, String>,
) -> HashMap<ValueCellId, reify_ir::UndefCause> {
    use reify_ir::{CompiledExprKind, DeterminacyState, UndefCause, Value};

    let mut causes = HashMap::new();

    for (id, (val, det)) in snap_values.iter() {
        if !val.is_undef() {
            continue;
        }

        // (1) Skip cells with no matching decl (synthetic guard/list/sub cells).
        let Some(decl) = decls.get(id) else {
            continue;
        };

        // (2) A3: check whether any direct input is undef → purely propagated.
        let propagated = if let Some(default_expr) = &decl.default_expr {
            let inputs = crate::deps::extract_value_deps(default_expr);
            inputs.iter().any(|input_id| {
                match snap_values.get(input_id) {
                    Some((v, _)) => v.is_undef(),
                    None => true, // absent counts as undef
                }
            })
        } else {
            // No default_expr → no input edges → cannot be propagated.
            false
        };

        if propagated {
            // A3: propagated undef — record nothing.
            continue;
        }

        // (3) Originating undef: classify.
        let cause = match det {
            DeterminacyState::Auto | DeterminacyState::Provisional => {
                // Solver variable.
                if let Some(detail) = solve_failed_autos.get(id) {
                    UndefCause::SolveFailed {
                        detail: detail.clone(),
                    }
                } else {
                    UndefCause::AwaitingSolve { param: id.clone() }
                }
            }
            _ => {
                match &decl.default_expr {
                    // `= undef` literal.
                    Some(expr) if matches!(&expr.kind, CompiledExprKind::Literal(Value::Undef)) => {
                        UndefCause::UserUndef { span: decl.span }
                    }
                    // No default expression → required param with no value.
                    None => UndefCause::Unbound {
                        param: id.clone(),
                        span: decl.span,
                    },
                    // Non-undef default_expr with all determined inputs:
                    // the OpContractFailed case owned by task γ — record nothing.
                    _ => continue,
                }
            }
        };

        causes.insert(id.clone(), cause);
    }

    causes
}

/// Containment query dispatch for `sample(restrict(field, region), point)`.
///
/// `Engine` implements `reify_expr::ContainmentQuery` so `cell_eval_ctx`
/// can coerce `&'a self` to `&'a dyn ContainmentQuery` and pass it into
/// `EvalContext::with_containment` (task 4222 δ, PRD §5.3 option (b)).
///
/// Dispatch mirrors `geometry_ops.rs::contains(solid, point)` exactly:
/// uses `DEFAULT_CONTAINS_TOLERANCE_M` and the default kernel.
impl reify_expr::ContainmentQuery for Engine {
    fn contains(&self, region: &Value, point: &Value) -> Option<bool> {
        // Extract the kernel_handle from a GeometryHandle region.
        let kernel_handle = match region {
            Value::GeometryHandle { kernel_handle, .. } => *kernel_handle,
            _ => return None,
        };
        // Extract 3 finite f64 coordinates from a Point3<Length>.
        // Mirrors `geometry_ops::point3_components` (private to geometry_ops).
        let [px, py, pz] = match point {
            Value::Point(c) if c.len() == 3 => {
                let a = c[0].as_f64().filter(|v| v.is_finite())?;
                let b = c[1].as_f64().filter(|v| v.is_finite())?;
                let cc = c[2].as_f64().filter(|v| v.is_finite())?;
                [a, b, cc]
            }
            _ => return None,
        };
        // Build the Contains query and dispatch to the default kernel.
        let q = reify_ir::GeometryQuery::Contains {
            handle: kernel_handle?,
            px,
            py,
            pz,
            tolerance: reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
        };
        match self.default_query_kernel()?.query(&q) {
            Ok(Value::Bool(b)) => Some(b),
            _ => None,
        }
    }
}

impl Engine {
    /// Compute `TermContribution` records for each term in `objective` by evaluating
    /// each term's expression against the post-solve `values` map.
    ///
    /// Called once per resolved scope when the scope has an explicit `ObjectiveSet`.
    /// Uses `eval_ctx_with_meta` + `reify_expr::eval_expr` to evaluate term expressions.
    ///
    /// A non-Scalar or failed eval records `realized_value = f64::NAN` (noisy sentinel).
    /// Contribution is `weight × σ(sense) × realized_value` with σ(Minimize)=+1,
    /// σ(Maximize)=−1 (PRD §6.2 invariant I3).
    ///
    /// **Note — I3 fold duplicated:** The σ(sense) sign convention and the
    /// `weight × σ × realized_value` fold are intentionally re-implemented here.
    /// The canonical instances live in `reify-constraints/src/solver.rs` (see
    /// `eval_objective_set`) and `reify-constraints/src/registry.rs`.  Those are
    /// across a crate boundary that `reify-eval` does not import for this path.
    /// If PRD §6.2 invariant I3 changes, update all three call sites.
    fn objective_term_contributions(
        &self,
        objective: &ObjectiveSet,
        values: &ValueMap,
    ) -> Vec<TermContribution> {
        let ctx = eval_ctx_with_meta(values, &self.functions, &self.meta_map);
        objective
            .terms
            .iter()
            .map(|term| {
                let realized_value = match reify_expr::eval_expr(&term.expr, &ctx) {
                    Value::Scalar { si_value, .. } => si_value,
                    _ => f64::NAN,
                };
                let sigma = match term.sense {
                    ObjectiveSense::Minimize => 1.0_f64,
                    ObjectiveSense::Maximize => -1.0_f64,
                };
                let contribution = term.weight * sigma * realized_value;
                TermContribution {
                    sense: term.sense,
                    weight: term.weight,
                    realized_value,
                    contribution,
                }
            })
            .collect()
    }

    /// Evaluate a compiled module, returning computed values.
    ///
    /// This is a cold-start evaluation that builds a new Snapshot and
    /// dependency structures. Subsequent calls to edit_param() can perform
    /// incremental re-evaluation using these structures.
    pub fn eval(&mut self, module: &CompiledModule) -> EvalResult {
        // Build the merged function table (user functions first, then prelude —
        // SHADOWING INVARIANT) and seal it in an Arc so clones are O(1).
        // See `merge_functions` in lib.rs for the full contract.
        self.functions = merge_functions(module, &self.prelude_functions);
        // Incrementally refresh the structure side-table from the user
        // module's `structure def` templates (task 3540 / SIR-α step-12).
        // `intern` is idempotent on name: prelude-seeded structures keep
        // their stable `StructureTypeId`; user structures are added (or their
        // meta overwritten on re-eval after an edit).
        crate::engine_admin::populate_structure_registry(
            &mut self.structure_registry,
            std::slice::from_ref(module),
        );
        self.compiled_purposes = module.compiled_purposes.clone();
        // Snapshot the field declarations so `Engine::edit_param` can
        // re-elaborate composed fields incrementally when their tracked
        // dependencies change (task 2343 step-8).
        self.compiled_fields = Arc::new(module.fields.clone());
        // Preserve user-intent purpose bindings across eval() (task 3103).
        // `active_purpose_bindings` (purpose_name → Vec<(param, entity)>) is
        // pure user intent and does not reference any snapshot data, so it can
        // be carried across a fresh eval() losslessly.  We snapshot it here via
        // mem::take (leaving the field empty) so the derived-state clears below
        // are safe, then re-apply each binding via
        // activate_purpose_constraints_with_bindings_inner() AFTER the new
        // eval_state is stored at the end of this function.
        //
        // `active_purposes`, `active_objective_map`, and `active_tolerance_scope`
        // are *derived* state — they hold ConstraintNodeIds and value-cell
        // references tied to the OLD snapshot.  These must be rebuilt against the
        // fresh graph, which activate_purpose() does for us.
        let mut preserved_bindings: Vec<(String, Vec<(String, String)>)> =
            std::mem::take(&mut self.active_purpose_bindings)
                .into_iter()
                .collect();
        // Sort by purpose name for deterministic re-injection order (S2, reviewer).
        // HashMap iteration is non-deterministic across runs; sorting here makes
        // diagnostics, demand-cone rebuild order, and any future order-sensitive
        // instrumentation reproducible.  Correctness is unaffected (constraint IDs
        // are keyed by purpose name; tolerance-scope merge is min-commutative).
        preserved_bindings.sort_by(|a, b| a.0.cmp(&b.0));
        self.active_purposes.clear();
        self.active_objective_map.clear();
        // Discard stale tolerance-scope state (task 2647) — rebuilt below by
        // activate_purpose() against the fresh value_cells.
        self.active_tolerance_scope.clear();
        // Build meta_map: template name → meta key/value pairs.
        // Only includes templates with non-empty meta blocks.
        self.meta_map = build_meta_map(module);
        // Use the merged function table (user functions prepended before prelude
        // functions) so that EvalContext has the full dispatch set — both user-defined
        // overloads AND non-shadowed prelude functions. This matches the SHADOWING
        // INVARIANT: first-match-wins linear scan means user functions take precedence
        // when signatures collide, while prelude functions with distinct
        // (name, arity, param types) triples remain callable.
        //
        // Arc::clone is O(1) — a single refcount increment. The merged table was
        // built and sealed by `merge_functions` (see lib.rs) at the assignment above;
        // this local binding lets `evaluate_let_bindings` borrow `self` mutably
        // (it takes &mut self) without conflicting with any immutable borrow of
        // `self.functions`. The Arc keeps the table alive for the lifetime of this
        // local binding even after `self.functions` is reassigned on a future call.
        //
        // PERFORMANCE NOTE (task-2195): eval_guarded_group_param_cell's determined-
        // value path clones `val` twice (once into `values`, once into
        // `snapshot.values`) and moves the third copy into `CachedResult::Value`.
        // The same triple-clone applies to the top-level Param success arm here.
        // Arc-ifying `ValueMap` values would reduce all three to O(1) pointer copies.
        let functions = Arc::clone(&self.functions);

        let mut values = ValueMap::new();
        let mut diagnostics = Vec::new();

        // Runtime diagnostics sink (task 2341 step-16): collects warnings
        // emitted by `reify_expr::eval_expr` during user-expression
        // evaluation — primarily `W_FIELD_OUT_OF_BOUNDS` from sampled-field
        // OOB queries and `W_INTERPOLATION_DEFERRED` from RBF/Kriging
        // fallback. Wired into every `EvalContext` constructed below via
        // `.with_runtime_diagnostics(&runtime_sink)`. Drained into
        // `diagnostics` immediately before returning.
        let runtime_sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());

        // Reset per-call test-instrumentation counters. These are always-present
        // fields (no cfg-gate) so the resets need no conditional compilation.
        self.last_param_override_type_kind_rejections = 0;
        self.last_param_override_dimension_rejections = 0;
        self.last_sub_component_unknown_structure_errors = 0;
        // undef-self-describing α (task 4321): clear the cause map unconditionally
        // so stale data from a prior eval() call never leaks through, even when
        // capture_undef_causes is toggled off between calls.
        self.last_undef_causes.clear();

        // Build Snapshot from CompiledModule (creates EvaluationGraph internally)
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;
        let version = VersionId(version_id);

        let mut snapshot = Snapshot::from_compiled_module(module);
        #[cfg(debug_assertions)]
        assert_value_cell_types_representable(&snapshot.graph);
        snapshot.id = SnapshotId(snapshot_id);
        snapshot.version = version;
        snapshot.provenance = SnapshotProvenance::Initial;

        // Purge orphaned param_overrides entries BEFORE the per-cell Param
        // loop populates `values` from the override map. A dormant override
        // on a cell that no longer exists (or whose kind changed from Param
        // to Let/Auto) would otherwise zombie-resurrect if a future edit
        // re-adds a cell with the same ValueCellId. See
        // `Engine::prune_param_overrides_against` for the shared helper
        // (task 2017 amend-pass); `engine_edit.rs` still inlines the same
        // retain predicate and will migrate onto the helper in a follow-up
        // — until then the two predicates must stay behaviourally identical.
        self.prune_param_overrides_against(&snapshot.graph);

        // Build dependency structures from the graph plus the module's
        // composed fields. Field-to-field deps land via the augmented
        // `Lambda { captures, .. }` injected by the compiler's
        // `phase_augment_composed_captures` post-pass.
        let reverse_index =
            ReverseDependencyIndex::build_from_graph_and_fields(&snapshot.graph, &module.fields);
        let trace_map = crate::deps::build_trace_map_and_fields(&snapshot.graph, &module.fields);

        // Set up demand registry: demand all value cells, constraints, and
        // realizations, then rebuild the cone. Shared helper keeps this in
        // sync with the matching block in `Engine::edit_source`.
        let demand = build_demand_for_graph(&snapshot.graph);

        // Evaluate field declarations first: they must be available in the
        // values map before templates are evaluated, because structure
        // expressions may reference fields (e.g., `sample(my_field, point)`).
        // The same `elaborate_field` helper is reused by `Engine::edit_param`
        // to refresh composed fields when their tracked dependencies change
        // — see `engine_edit.rs` for the incremental call site.
        for field in &module.fields {
            let (field_value, imported_hash) = elaborate_field(
                field,
                &values,
                &functions,
                &self.meta_map,
                Some(&runtime_sink),
            );
            let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, &field.name);
            values.insert(field_id.clone(), field_value.clone());
            snapshot
                .values
                .insert(field_id, (field_value, DeterminacyState::Determined));

            // Record the file content-hash and provenance for Imported field
            // sources so the cache side-tables stay current.
            // The hash was already computed inside elaborate_field alongside the
            // VDB read, so no separate fs::read is needed here.
            // Provenance is recorded whenever the file is readable (hash
            // available), regardless of VDB-parse success (task 2669 §DD).
            if let reify_compiler::CompiledFieldSource::Imported {
                path: Some(ref p),
                ref format,
                ..
            } = field.source
                && let Some(h) = imported_hash
            {
                self.cache.record_imported_file_hash(p, h);
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let prov = crate::field_import_provenance::build_field_import_provenance(
                    p,
                    // `format` is `None` when the compiler omits the optional
                    // `format` keyword (OpenVDB is the only path-importable format
                    // in v0.2, so the default is always correct today).  If/when a
                    // second path-importable format is added, replace this default
                    // with an explicit per-format branch to avoid silently
                    // mislabelling provenance records.
                    format.as_deref().unwrap_or("OpenVDB"),
                    h,
                    None,
                    now_secs,
                );
                // NOTE: provenance is re-recorded on every eval (the timestamp
                // reflects the most-recent observed ingestion, not a stable
                // first-ingest time).  Consumers of
                // `Engine::imported_field_provenance` / `CacheStore::get_field_import_provenance`
                // must not rely on the timestamp for cache-equality reasoning.
                self.cache.record_field_import_provenance(p, prov);
            }
        }

        // Unified single-pass evaluation of Param+Let cells in dependency order (§8.2)
        for template in &module.templates {
            // Unified single-pass: Auto cells pre-seeded, then Param and Let cells
            // evaluated in combined dependency order (spec §8.2 order-independence).
            {
                let meta_map = Arc::clone(&self.meta_map);
                self.evaluate_params_and_lets_unified(
                    template,
                    &mut values,
                    &mut snapshot,
                    version_id,
                    &functions,
                    &meta_map,
                    &mut diagnostics,
                    &runtime_sink,
                );
            }

            // Third pass: evaluate guarded groups.
            // Guard cells are Let-kind synthetic cells — evaluate their expressions,
            // then conditionally evaluate members based on guard truth value.
            for group in &template.guarded_groups {
                // Evaluate the guard cell expression
                let guard_val = reify_expr::eval_expr(
                    &group.guard_expr,
                    &eval_ctx_with_meta(&values, &functions, &self.meta_map)
                        .with_determinacy(&snapshot.values),
                );
                values.insert(group.guard_value_cell.clone(), guard_val.clone());

                let guard_determinacy = match &guard_val {
                    Value::Bool(_) => DeterminacyState::Determined,
                    _ => DeterminacyState::Undetermined,
                };
                snapshot.values.insert(
                    group.guard_value_cell.clone(),
                    (guard_val.clone(), guard_determinacy),
                );

                let guard_is_true = matches!(&guard_val, Value::Bool(true));
                let guard_is_false = matches!(&guard_val, Value::Bool(false));

                // Single ctx reused by both the members and else_members loops —
                // the mutable borrows on journal/cache are released when each loop
                // ends, so a single GuardedParamCtx compiles cleanly across both.
                let mut param_ctx = GuardedParamCtx {
                    journal: &mut self.journal,
                    cache: &mut self.cache,
                    functions: &functions,
                    meta_map: &self.meta_map,
                    version,
                    type_kind_counter: &mut self.last_param_override_type_kind_rejections,
                    dimension_counter: &mut self.last_param_override_dimension_rejections,
                };

                // Evaluate members (active when guard is true)
                for cell in &group.members {
                    if guard_is_true {
                        // Evaluate normally
                        if cell.kind == ValueCellKind::Param {
                            eval_guarded_group_param_cell(
                                cell,
                                &self.param_overrides,
                                &self.structure_registry,
                                &mut values,
                                &mut snapshot,
                                &mut diagnostics,
                                &mut param_ctx,
                            );
                        } else if cell.kind == ValueCellKind::Let {
                            if let Some(ref expr) = cell.default_expr {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &eval_ctx_with_meta(&values, &functions, &self.meta_map)
                                        .with_determinacy(&snapshot.values),
                                );
                                values.insert(cell.id.clone(), val.clone());
                                snapshot
                                    .values
                                    .insert(cell.id.clone(), (val, DeterminacyState::Determined));
                            } else {
                                values.insert(cell.id.clone(), Value::Undef);
                                snapshot.values.insert(
                                    cell.id.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        } else if cell.kind.is_auto() {
                            values.insert(cell.id.clone(), Value::Undef);
                            snapshot
                                .values
                                .insert(cell.id.clone(), (Value::Undef, DeterminacyState::Auto));
                        }
                    } else {
                        // Guard is false or Undef — member is inactive
                        values.insert(cell.id.clone(), Value::Undef);
                        let det = if cell.kind.is_auto() {
                            DeterminacyState::Auto
                        } else {
                            DeterminacyState::Undetermined
                        };
                        snapshot.values.insert(cell.id.clone(), (Value::Undef, det));
                    }
                }

                // Evaluate else_members (active when guard is false)
                for cell in &group.else_members {
                    if guard_is_false {
                        // Mirror the top-level Param branch and the members loop above:
                        // consult param_overrides for Param cells, validate, warn-and-retain
                        // on rejection, fall back to default_expr.
                        if cell.kind == ValueCellKind::Param {
                            eval_guarded_group_param_cell(
                                cell,
                                &self.param_overrides,
                                &self.structure_registry,
                                &mut values,
                                &mut snapshot,
                                &mut diagnostics,
                                &mut param_ctx,
                            );
                        } else if cell.kind == ValueCellKind::Let {
                            if let Some(ref expr) = cell.default_expr {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &eval_ctx_with_meta(&values, &functions, &self.meta_map)
                                        .with_determinacy(&snapshot.values),
                                );
                                values.insert(cell.id.clone(), val.clone());
                                snapshot
                                    .values
                                    .insert(cell.id.clone(), (val, DeterminacyState::Determined));
                            } else {
                                values.insert(cell.id.clone(), Value::Undef);
                                snapshot.values.insert(
                                    cell.id.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        } else if cell.kind.is_auto() {
                            values.insert(cell.id.clone(), Value::Undef);
                            snapshot
                                .values
                                .insert(cell.id.clone(), (Value::Undef, DeterminacyState::Auto));
                        }
                    } else {
                        // Guard is true or Undef — else member is inactive
                        values.insert(cell.id.clone(), Value::Undef);
                        let det = if cell.kind.is_auto() {
                            DeterminacyState::Auto
                        } else {
                            DeterminacyState::Undetermined
                        };
                        snapshot.values.insert(cell.id.clone(), (Value::Undef, det));
                    }
                }
            }
        }

        // Sub-component elaboration: evaluate child template params/lets
        // for each sub_component in each template.
        for template in &module.templates {
            for sub in &template.sub_components {
                // Find the referenced child template by name — module
                // templates first, then the stdlib prelude (esc-4287-15).
                let child_template =
                    match find_template_with_prelude(module, self.prelude, &sub.structure_name) {
                        Some(t) => t,
                        None => {
                            self.last_sub_component_unknown_structure_errors += 1;
                            diagnostics.push(Diagnostic::error(format!(
                                "sub-component \"{}\" references unknown structure \"{}\"",
                                sub.name, sub.structure_name
                            )));
                            continue;
                        }
                    };

                // Collection sub: determine count, then elaborate N instances
                if sub.is_collection {
                    let count = if let Some(ref count_cell_id) = sub.count_cell {
                        // The count cell value should already be evaluated (it's a let binding)
                        match values.get(count_cell_id) {
                            Some(Value::Int(n)) => Some(*n),
                            _ => None,
                        }
                    } else {
                        None
                    };

                    if let Some(n) = count {
                        for idx in 0..n {
                            let scoped_entity = format!("{}.{}[{}]", template.name, sub.name, idx);
                            elaborate_child_instance(
                                &mut values,
                                &mut snapshot,
                                &functions,
                                &mut self.journal,
                                &mut self.cache,
                                version_id,
                                child_template,
                                &scoped_entity,
                                &sub.args,
                                &self.meta_map,
                                &mut diagnostics,
                            );
                        }

                        // Create per-member synthetic lists: __list_{name}__{member} for each value cell
                        for child_cell in &child_template.value_cells {
                            let member_list_val = collect_member_list(
                                &values,
                                &template.name,
                                &sub.name,
                                &child_cell.id.member,
                                n,
                            );
                            let member_list_id = ValueCellId::new(
                                &template.name,
                                format!("__list_{}__{}", sub.name, child_cell.id.member),
                            );
                            values.insert(member_list_id.clone(), member_list_val.clone());
                            snapshot.values.insert(
                                member_list_id,
                                (member_list_val, DeterminacyState::Determined),
                            );
                        }
                    }
                    // If count is None (Undef), no instances are created
                    continue;
                }

                // Guarded sub on a non-recursive template: do NOT elaborate.
                // A guard on a sub is meaningful only in a recursive context. The
                // child-frame analog is the `child_template.is_recursive` filter
                // inside `unfold_recursive_sub` (see the recursive-subs collection
                // below). This branch provides the symmetric root-frame gate: if
                // the top-level template is non-recursive and the sub is guarded,
                // Phase 2 must not materialise the sub's entity.
                if !template.is_recursive && sub.guard_state.is_compiled() {
                    continue;
                }

                // Recursive sub: evaluate guard before elaborating, then unfold recursively.
                if template.is_recursive && sub.guard_state.is_compiled() {
                    let mut unfold_budget = self.max_unfold_nodes;
                    unfold_recursive_sub(
                        &mut values,
                        &mut snapshot,
                        &functions,
                        &mut self.journal,
                        &mut self.cache,
                        version_id,
                        template,       // scope_template: owns `sub` (guard/arg refs match)
                        child_template, // target template for Phase 1 instantiation
                        sub,
                        &template.name,
                        0,
                        self.max_unfold_depth,
                        &self.meta_map,
                        &mut diagnostics,
                        &module.templates,
                        &mut unfold_budget,
                    );
                    continue;
                }

                // Build scoped entity prefix: "ParentName.sub_name"
                let scoped_entity = format!("{}.{}", template.name, sub.name);

                elaborate_child_instance(
                    &mut values,
                    &mut snapshot,
                    &functions,
                    &mut self.journal,
                    &mut self.cache,
                    version_id,
                    child_template,
                    &scoped_entity,
                    &sub.args,
                    &self.meta_map,
                    &mut diagnostics,
                );

                // task 3540 (SIR-α), handler esc-3540-182 (A): expose the
                // elaborated non-collection sub as a single
                // `Value::StructureInstance` at `ValueCellId(parent, sub.name)`.
                // The per-member scoped cells `ValueCellId(parent.sub, member)`
                // are left intact (the existing `self.<sub>.<member>` cross-sub
                // access path still reads them); this adds the collapsed value
                // so `self.<sub>` member-access chains — and direct inspection
                // of the sub cell — see a structure-shaped Value rather than a
                // missing cell. Fields are gathered from the just-populated
                // scoped cells in the child template's declaration order.
                // `type_id` is the ephemeral `StructureTypeId(0)` placeholder
                // (identity is name+version per esc-3540-173); `version` is the
                // child structure-def's `@version(N)` (esc-3540-176).
                {
                    let mut fields: PersistentMap<String, Value> = PersistentMap::new();
                    for cell in &child_template.value_cells {
                        if let Some(v) =
                            values.get(&ValueCellId::new(&scoped_entity, &cell.id.member))
                        {
                            fields.insert(cell.id.member.clone(), v.clone());
                        }
                    }
                    let si = Value::StructureInstance(Box::new(reify_ir::StructureInstanceData {
                        type_id: reify_ir::StructureTypeId(0),
                        type_name: sub.structure_name.clone(),
                        version: child_template.version(),
                        fields,
                    }));
                    let sub_id = ValueCellId::new(&template.name, &sub.name);
                    values.insert(sub_id.clone(), si.clone());
                    snapshot
                        .values
                        .insert(sub_id, (si, DeterminacyState::Determined));
                }
            }

            // Re-evaluate let bindings that may depend on sub-component cells:
            // - collection subs create __list_* synthetic cells
            // - regular subs create {parent}.{sub}.{member} cells via elaborate_child_instance
            // Both become available only after elaboration, so re-evaluate if any subs exist.
            if !template.sub_components.is_empty() {
                let meta_map = Arc::clone(&self.meta_map);
                self.evaluate_let_bindings(
                    template,
                    &mut values,
                    &mut snapshot,
                    version_id,
                    &functions,
                    &meta_map,
                    &mut diagnostics,
                    &runtime_sink,
                );
            }
        }

        // Structural-query expansion pass (task 3985, β).
        //
        // After sub-component elaboration, all `__count_{sub}` collection-count
        // cells are populated.  Now replace any `self.children` / `self.members`
        // MethodCall placeholder with a concrete list expression and re-evaluate
        // the containing Let cell, writing the result into both `values` and
        // `snapshot.values` as (value, Determined).
        //
        // Mirrors `expand_purpose_reflective_placeholders` (engine_purposes.rs:809)
        // — rewrite before eval, no generic-evaluator context threading.
        // `descendants` placeholders are left unexpanded (task γ scope).
        for template in &module.templates {
            for cell in &template.value_cells {
                if !matches!(cell.kind, ValueCellKind::Let) {
                    continue;
                }
                let expr = match &cell.default_expr {
                    Some(e) => e,
                    None => continue,
                };
                if !crate::structural_query::contains_structural_query(expr) {
                    continue;
                }
                let mut expanded = expr.clone();
                crate::structural_query::expand_structural_query(
                    &mut expanded,
                    template,
                    &values,
                );
                let val = reify_expr::eval_expr(
                    &expanded,
                    &eval_ctx_with_meta(&values, &functions, &self.meta_map)
                        .with_determinacy(&snapshot.values),
                );
                values.insert(cell.id.clone(), val.clone());
                snapshot.values.insert(
                    cell.id.clone(),
                    (val, DeterminacyState::Determined),
                );
            }
        }

        // Resolution phase: resolve auto params using the constraint solver.
        //
        // `resolve_solver_for_module` consults `module.solver_pragma` against the
        // named-solver registry (`Engine::register_solver`, Task 2300) and falls
        // back to `self.solver` if the named back-end isn't registered. It is
        // called once before the template loop so the "not registered" warning
        // is emitted at most once per eval call. The inner loop re-looks-up the
        // active solver via `lookup_solver_for_module` (no warning, single
        // expression) so the &self borrow doesn't extend across the &mut self
        // mutations (`self.next_snapshot_id`, etc.) inside the loop body.
        let mut resolved_params = HashMap::new();
        // θ (task 4015): per-auto-cell objective provenance; populated in Solved arm below.
        let mut objective_provenance: HashMap<ValueCellId, ObjectiveProvenance> = HashMap::new();
        // undef-self-describing α (task 4321): side-channel for the cells whose
        // template solve failed (Infeasible or NoProgress).  The HashMap<id, detail>
        // shape lets classify_undef_origins emit the coarse SolveResult string
        // verbatim (§8.3 — no fabricated solver detail). Populated only when
        // capture_undef_causes is true (gated inside the match arms below).
        let mut solve_failed_autos: HashMap<ValueCellId, String> = HashMap::new();
        let has_active_solver = self
            .resolve_solver_for_module(module, &mut diagnostics)
            .is_some();
        if has_active_solver {
            // Refresh template-native objectives so edit_param() can access them.
            // Clear centrality tracking alongside objectives — both are per-eval state.
            self.objectives.clear();
            self.centrality_synthesized_scopes.clear();
            for template in &module.templates {
                if let Some(obj) = &template.objective {
                    self.objectives.insert(template.name.clone(), obj.clone());
                } else if scope_qualifies_for_centrality(template) {
                    // No explicit user objective AND the scope meets the Scalar + inequality
                    // gate: the DimensionalSolver will synthesise a Chebyshev-centre
                    // objective for it.  Record the scope name for the I5 provenance hook
                    // (task θ) and the η integration test.
                    //
                    // This mirrors solver.rs::build_centrality_objective's gate predicate;
                    // cross-reference that function when updating either site.
                    self.centrality_synthesized_scopes
                        .insert(template.name.clone());
                }
            }
            for template in &module.templates {
                // Build the ResolutionProblem; returns None when there are no auto cells.
                // `build_solver_problem` Arc::clones `functions` — O(1) refcount bump,
                // not a deep copy (task #2286).
                let Some(problem) = build_solver_problem(template, &values, Arc::clone(&functions))
                else {
                    continue;
                };

                let parent_snap_id = snapshot.id;
                // Use a temporary borrow of the resolved solver so the
                // reference doesn't outlive the solve() call — this allows
                // &mut self for evaluate_let_bindings and snapshot ID bumps
                // below. `lookup_solver_for_module` re-runs the named-vs-default
                // routing without re-emitting the warning.
                //
                // Cost: per-iteration this is one `solver_pragma.as_ref()` match
                // plus at most one `HashMap::get` plus an `or(self.solver.as_deref())`
                // — negligible relative to the `.solve(&problem)` call that follows.
                // Hoisting the resolved name outside the loop would require either
                // (a) a slimmer inner helper taking `Option<&str>`, or
                // (b) duplicating the routing logic at the call site, both of
                // which trade a minor speedup for an extra surface that must
                // stay in lock-step with `lookup_solver_for_module`. Given typical
                // template counts (single-digit per module), the current shape is
                // the better trade. (Task 2300 reviewer comment.)
                let solve_result = self
                    .lookup_solver_for_module(module)
                    .expect("has_active_solver is true => solver lookup returns Some")
                    .solve(&problem);

                match solve_result {
                    SolveResult::Solved {
                        values: solver_values,
                        unique,
                    } => {
                        // Allocate new snapshot/version IDs BEFORE recording cache
                        // entries so all resolution-phase entries share the same
                        // basis_version as the snapshot. This preserves the invariant
                        // that try_fast_path relies on for incremental evaluation.
                        let res_snapshot_id = self.next_snapshot_id;
                        self.next_snapshot_id += 1;
                        let res_version_id = self.next_version_id;
                        self.next_version_id += 1;

                        // Update values map with resolved values
                        let mut resolved_ids = std::collections::HashSet::new();
                        for (id, val) in &solver_values {
                            let node_id = NodeId::Value(id.clone());
                            let start = Instant::now();
                            self.journal.record(EvalEvent {
                                timestamp: start,
                                node_id: node_id.clone(),
                                kind: EventKind::Started,
                                version: VersionId(res_version_id),
                                payload: None,
                            });

                            values.insert(id.clone(), val.clone());
                            resolved_params.insert(id.clone(), val.clone());
                            resolved_ids.insert(id.clone());

                            // Update snapshot values with (resolved_val, Determined)
                            snapshot
                                .values
                                .insert(id.clone(), (val.clone(), DeterminacyState::Determined));

                            // Update cache with res_version_id (matches snapshot)
                            let trace = DependencyTrace::default();
                            let cached_result =
                                CachedResult::Value(val.clone(), DeterminacyState::Determined);
                            let outcome = self.cache.record_evaluation(
                                node_id.clone(),
                                cached_result,
                                VersionId(res_version_id),
                                trace,
                            );

                            self.journal.record(EvalEvent {
                                timestamp: Instant::now(),
                                node_id,
                                kind: EventKind::Completed { outcome },
                                version: VersionId(res_version_id),
                                payload: Some(EventPayload::Duration(start.elapsed())),
                            });
                        }

                        // Emit warning for free auto params when solution is non-unique
                        if !unique {
                            for ap in &problem.auto_params {
                                if ap.free {
                                    diagnostics.push(Diagnostic::warning(format!(
                                        "Parameter `{}` resolved via auto(free) \
                                         -- result is not uniquely determined.",
                                        ap.id.member
                                    )));
                                }
                            }
                        }

                        // θ (task 4015): record ObjectiveProvenance for each resolved cell.
                        // Capture `is_synth` as a bool so the immutable borrow of
                        // `self.centrality_synthesized_scopes` releases before the &mut self
                        // mutations (`evaluate_let_bindings`) that follow.
                        // Iterate `&resolved_ids` (borrow) so it is still available for the
                        // `SnapshotProvenance::Resolution { resolved: resolved_ids }` move below.
                        //
                        // Performance: wrap `objective` and `term_contributions` in `Arc` once
                        // per scope so the per-cell loop does O(1) refcount bumps rather than
                        // O(N × |terms|) deep clones.  The `ObjectiveProvenance` field docs
                        // explain the sharing contract to consumers.
                        {
                            let is_synth = self
                                .centrality_synthesized_scopes
                                .contains(template.name.as_str());
                            // One deep clone of the ObjectiveSet per scope (not per cell).
                            let objective_arc: Option<Arc<ObjectiveSet>> =
                                problem.objective.as_ref().map(|o| Arc::new(o.clone()));
                            let combination = objective_arc.as_ref().map(|o| o.combination);
                            // Compute per-term contributions once per scope; share via Arc.
                            let term_contributions: Arc<Vec<TermContribution>> = Arc::new(
                                objective_arc
                                    .as_ref()
                                    .map(|obj| self.objective_term_contributions(obj, &values))
                                    .unwrap_or_default(),
                            );
                            for id in &resolved_ids {
                                objective_provenance.insert(
                                    id.clone(),
                                    ObjectiveProvenance {
                                        scope: template.name.clone(),
                                        objective: objective_arc.clone(), // Arc refcount bump
                                        combination,
                                        term_contributions: Arc::clone(&term_contributions),
                                        synthetic_centrality: is_synth,
                                    },
                                );
                            }
                        }

                        // Set child snapshot with Resolution provenance
                        snapshot.id = SnapshotId(res_snapshot_id);
                        snapshot.version = VersionId(res_version_id);
                        snapshot.provenance = SnapshotProvenance::Resolution {
                            scope: template.name.clone(),
                            resolved: resolved_ids,
                            parent: parent_snap_id,
                        };

                        // Re-run let binding evaluation in topological order
                        let meta_map = Arc::clone(&self.meta_map);
                        self.evaluate_let_bindings(
                            template,
                            &mut values,
                            &mut snapshot,
                            res_version_id,
                            &functions,
                            &meta_map,
                            &mut diagnostics,
                            &runtime_sink,
                        );
                    }
                    SolveResult::Infeasible {
                        diagnostics: solver_diags,
                    } => {
                        diagnostics.extend(solver_diags);
                        // undef-self-describing α: record every auto-param that
                        // failed to solve so classify_undef_origins can emit
                        // SolveFailed instead of AwaitingSolve.  Gated by the
                        // capture flag so the resolution loop is byte-identical
                        // when capture is off (A1 / BT8).
                        if self.capture_undef_causes {
                            record_failed_autos(
                                &mut solve_failed_autos,
                                &problem.auto_params,
                                "infeasible",
                            );
                        }
                    }
                    SolveResult::NoProgress { reason } => {
                        diagnostics.push(Diagnostic::warning(format!(
                            "Constraint solver made no progress: {}",
                            reason
                        )));
                        // undef-self-describing α: same as Infeasible arm — record
                        // failed autos with a coarse "no progress: <reason>" detail
                        // string (§8.3 — no fabricated solver detail).
                        if self.capture_undef_causes {
                            let detail = format!("no progress: {reason}");
                            record_failed_autos(
                                &mut solve_failed_autos,
                                &problem.auto_params,
                                &detail,
                            );
                        }
                    }
                }
            }
        }

        // ── Post-solver guard re-evaluation ───────────────────────────────────
        // The "Third pass: guarded groups" loop above runs BEFORE the solver,
        // so any guard whose expression reads an auto param evaluates to Undef
        // at that point (auto params start as Undef). After the solver resolves
        // auto params to concrete values, we must re-evaluate guard cells and
        // member cells to get the correct activated/deactivated state.
        //
        // This pass is idempotent with the earlier pass for guards that don't
        // depend on auto params — they will re-evaluate to the same value.
        // For auto-param-dependent guards and members that reference auto params,
        // this corrects what the earlier pass could not compute.
        //
        // Inactive-branch Auto-cell policy: skip (preserve solver value).
        // Canonical rule documented on `engine_edit.rs`'s module-level `//!` doc
        // and `deactivate_if_not_auto`.
        if self.solver.is_some() {
            for template in &module.templates {
                for group in &template.guarded_groups {
                    let guard_val = reify_expr::eval_expr(
                        &group.guard_expr,
                        &eval_ctx_with_meta(&values, &functions, &self.meta_map)
                            .with_determinacy(&snapshot.values),
                    );
                    values.insert(group.guard_value_cell.clone(), guard_val.clone());
                    let guard_determinacy = match &guard_val {
                        Value::Bool(_) => DeterminacyState::Determined,
                        _ => DeterminacyState::Undetermined,
                    };
                    snapshot.values.insert(
                        group.guard_value_cell.clone(),
                        (guard_val.clone(), guard_determinacy),
                    );

                    let guard_is_true = matches!(&guard_val, Value::Bool(true));
                    let guard_is_false = matches!(&guard_val, Value::Bool(false));

                    post_solver_re_eval_guard_cells(
                        &group.members,
                        guard_is_true,
                        &mut values,
                        &mut snapshot.values,
                        &functions,
                        &self.meta_map,
                    );
                    post_solver_re_eval_guard_cells(
                        &group.else_members,
                        guard_is_false,
                        &mut values,
                        &mut snapshot.values,
                        &functions,
                        &self.meta_map,
                    );
                }
            }
        }

        // ── Guard-state fingerprinting ──────────────────────────────
        // Include guard-cell boolean states in the topology fingerprint so that
        // eval() and edit_param() produce identical fingerprints for the same
        // logical guard configuration.
        if !snapshot.graph.guarded_groups.is_empty() {
            let guard_state_hash = guard_state_fingerprint(
                &snapshot.graph.guarded_groups,
                &values,
                GuardLookup::Lenient,
            );
            snapshot.topology_fingerprint = snapshot
                .graph
                .topology_fingerprint()
                .combine(guard_state_hash);
        }

        // GHR-δ S10: augment each geometry cell's CACHED trace with its backing
        // Realization. Geometry params are recorded above with an empty
        // `DependencyTrace` (the `record_eval_completed` param path can't see the
        // implicit Realization→ValueCell edge), so without this post-pass the GH
        // cell's freshness derivation would never fold in its Realization's
        // freshness (PRD §5/§7.1). The links come from the same single source of
        // truth the reverse index / trace map use, folded per-cell by
        // (`geometry_cell_realization_reads`) so the cached trace carries the
        // SAME accumulated `realization_reads` as `build_trace_map_and_fields`
        // (which `push`-accumulates) even if the 1:1 cell↔realization invariant
        // is ever violated — see that helper's docs. `snapshot.graph` is
        // read-only here and `self.cache` is a disjoint field. Each build
        // re-folds from scratch, so the replace-semantics setter stays
        // idempotent across re-eval rounds.
        for (cell, reads) in crate::deps::geometry_cell_realization_reads(&snapshot.graph) {
            let _ = self
                .cache
                .set_realization_reads(&NodeId::Value(cell), reads);
        }

        // ── RBD-α (task 3822): MassProperties PSD inertia validation ─────────────
        // Post-eval pass: for every cell whose value is a StructureInstance with
        // type_name == "MassProperties", extract the `inertia` field, compute the
        // symmetric-3×3 eigenvalues analytically, and replace the cell with
        // Value::Undef (Determined) when the matrix is non-PSD or malformed.
        //
        // Design rationale: `reify-expr::eval_structure_instance_ctor` is
        // intentionally registry-free and diagnostic-free (SIR-α design decision
        // 2), so the diagnostic-emitting + value-replacing hook belongs here in
        // reify-eval, where the diagnostics sink and value maps are both accessible.
        //
        // The immutable `values` borrow is released before any mutable insert by
        // collecting target pairs first.
        //
        // Performance: the scan is guarded by a fast any() check so designs that
        // never instantiate MassProperties skip the extraction pass entirely.
        {
            // Fast early-out: skip the O(n) extraction pass when no MassProperties
            // cell exists (the common case when std.dynamics is unused).
            let has_mass_props = values.iter().any(|(_, v)| {
                matches!(v, Value::StructureInstance(d) if d.type_name == "MassProperties")
            });

            if has_mass_props {
                // Classify each MassProperties cell's inertia field.
                enum InertiaResult {
                    /// Field is absent or already Undef — leave untouched (no false positives).
                    Skip,
                    /// Field is present but could not be parsed as a 3×3 numeric matrix.
                    Malformed,
                    /// Field parsed successfully — run PSD check.
                    Valid([[f64; 3]; 3]),
                }

                let mass_props_cells: Vec<(ValueCellId, InertiaResult)> = values
                    .iter()
                    .filter_map(|(id, val)| {
                        if let Value::StructureInstance(data) = val
                            && data.type_name == "MassProperties"
                        {
                            let result = match data.fields.get("inertia") {
                                None | Some(Value::Undef) => InertiaResult::Skip,
                                Some(v) => match crate::dynamics_psd::inertia_3x3_from_value(v) {
                                    Some(m) => InertiaResult::Valid(m),
                                    None => InertiaResult::Malformed,
                                },
                            };
                            // Only collect cells that need attention.
                            match result {
                                InertiaResult::Skip => None,
                                other => Some((id.clone(), other)),
                            }
                        } else {
                            None
                        }
                    })
                    .collect();

                for (id, result) in mass_props_cells {
                    match result {
                        InertiaResult::Skip => unreachable!("Skip filtered above"),
                        InertiaResult::Malformed => {
                            // A present-but-unparseable inertia field (wrong shape, non-numeric
                            // cell) is surfaced as E_DynamicsInertiaNotPSD so malformed tensors
                            // never silently flow to dynamics consumers.
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "MassProperties '{}': inertia field cannot be parsed as \
                                     a 3×3 numeric matrix",
                                    id,
                                ))
                                .with_code(DiagnosticCode::DynamicsInertiaNotPSD),
                            );
                            values.insert(id.clone(), Value::Undef);
                            snapshot
                                .values
                                .insert(id.clone(), (Value::Undef, DeterminacyState::Determined));
                        }
                        InertiaResult::Valid(m) => {
                            let tol = crate::dynamics_psd::psd_tol(&m);
                            if !crate::dynamics_psd::is_symmetric_psd(&m, tol) {
                                let min_eig = crate::dynamics_psd::min_eigenvalue(&m);
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "MassProperties '{}': inertia tensor is not positive \
                                         semi-definite (min eigenvalue ≈ {:.3e})",
                                        id, min_eig,
                                    ))
                                    .with_code(DiagnosticCode::DynamicsInertiaNotPSD),
                                );
                                values.insert(id.clone(), Value::Undef);
                                snapshot.values.insert(
                                    id.clone(),
                                    (Value::Undef, DeterminacyState::Determined),
                                );
                            }
                        }
                    }
                }
            }
        }

        // undef-self-describing α (task 4321): post-eval UndefCause classification pass.
        //
        // Runs HERE — after snapshot.values is fully finalized (resolution phase +
        // post-solver guard re-eval + sub-component elaboration + MassProperties PSD
        // check) and BEFORE snapshot is moved into EvaluationState. Reads snapshot
        // read-only and writes only `self.last_undef_causes` (never snapshot.values),
        // so A1 (capture-on == capture-off for every (Value,DeterminacyState) and
        // content-hash) is STRUCTURAL.
        //
        // Only allocates and classifies when the caller has opted in (BT8 toggle).
        if self.capture_undef_causes {
            // Build decl table: id → &ValueCellDecl from all templates' value_cells.
            // Includes only top-level named cells; synthetic guard/list/sub-elaborated
            // cells have no entry and are silently skipped in the classifier.
            let decls: HashMap<&ValueCellId, &reify_compiler::ValueCellDecl> = module
                .templates
                .iter()
                .flat_map(|t| t.value_cells.iter())
                .map(|d| (&d.id, d))
                .collect();
            self.last_undef_causes =
                classify_undef_origins(&snapshot.values, &decls, &solve_failed_autos);
            // γ (task 4323): fill in OpContractFailed for cells α left unclassified
            // (the `_ => continue` arm in classify_undef_origins: non-undef default_expr
            // with all determined inputs). Re-evaluates each such cell's default_expr with
            // a fresh undef-cause sink; the first OpContractFailed found is recorded.
            record_op_contract_failures(
                &mut self.last_undef_causes,
                &snapshot.values,
                &decls,
                &values,
                &functions,
                &self.meta_map,
            );
        }

        // Store internal state for incremental evaluation
        self.eval_state = Some(EvaluationState {
            snapshot,
            reverse_index,
            trace_map,
        });
        self.demand = demand;
        self.last_eval_set = Vec::new(); // Cold start: no incremental eval set

        // Re-apply preserved purpose bindings against the fresh snapshot (task 3103).
        // activate_purpose_constraints_with_bindings_inner() requires eval_state to
        // be Some — satisfied by the assignment above.  For each captured
        // (purpose_name, Vec<(param, entity)>) it injects constraints into the new
        // graph, restores the optimization objective, and records the bindings in
        // active_purpose_bindings.  If a purpose was removed by the re-eval
        // (different module), the inner returns false silently — the stale binding
        // is dropped automatically.  The already-active guard
        // (active_purposes.contains_key) is NOT hit because active_purposes was
        // cleared above; re-injection is safe.
        //
        // Performance (S1, reviewer): a single rebuild_purpose_infrastructure() call
        // after the loop amortises the O(graph) reverse_index/trace_map/rebuild_cone/
        // recompute_tolerance_scope cost into one pass regardless of N preserved
        // purposes.  Pre-3103 preserved_bindings was always empty (zero cost);
        // post-3103 we pay O(graph) at most once per eval() call — exactly once when
        // at least one preserved binding re-injects, zero times when all preserved
        // bindings target purposes absent from the new module (task 3260).
        // When `any_injected` is false, the resulting engine state matches the
        // empty-`preserved_bindings` path: derived caches were freshly built
        // upstream and `active_tolerance_scope` is already cleared.
        if !preserved_bindings.is_empty() {
            let mut any_injected = false;
            for (purpose_name, param_bindings) in &preserved_bindings {
                // Use the multi-param inner directly: it accepts any bindings slice
                // (single- or multi-param), performs injection, and records the
                // bindings in active_purpose_bindings. The single-entity shim
                // activate_purpose_constraints refuses purposes with params.len()!=1,
                // so it cannot round-trip multi-param purposes.
                any_injected |= self
                    .activate_purpose_constraints_with_bindings_inner(purpose_name, param_bindings);
            }
            if any_injected {
                self.rebuild_purpose_infrastructure();
            }
        }

        // Drain runtime diagnostics (task 2341 step-16) into the result
        // diagnostics vec. The sink was populated by `eval_expr` calls
        // above whenever sampled-field OOB queries or RBF/Kriging
        // fallbacks emitted warnings via `EvalContext::diagnostics`.
        diagnostics.append(&mut runtime_sink.borrow_mut());

        // R2a symbolic-mint pass (task #4652, step-4): for each named realization
        // in `module`, mint `Value::GeometryHandle { kernel_handle: None }` into
        // `values` when the cell is not yet realized.  Runs AFTER the scalar
        // value-cell pass (so params like `width` are resolved in `values` for
        // the upstream_values_hash fold) and BEFORE diagnostic passes.
        Engine::mint_symbolic_geometry_handles_into_values(
            module,
            &mut values,
            &functions,
            &self.meta_map,
        );

        // R2b symbolic selector-mint pass (task #4653, step-6): for each
        // topology-selector cell in `module`, mint `Value::Selector` into
        // `values` when the cell is currently Undef and the expr is a
        // recognised kernel-free leaf constructor over a symbolic target.
        // Runs immediately AFTER the handle-mint (above) so the symbolic
        // body handle is already present in `values`.
        crate::geometry_ops::mint_symbolic_topology_selectors_into_values(
            module,
            &mut values,
            &mut diagnostics,
        );

        // Static coupling detection (task 4020 — W_SCOPE_COUPLING, PRD λ §3.7).
        // Placed OUTSIDE the `has_active_solver` gate so the warning surfaces on
        // `reify check` (which attaches no solver). Detection is purely structural
        // and needs no solved values.
        diagnostics.extend(detect_scope_coupling(&module.templates));

        // Mechanism error diagnostics (task 4308 — E_MECHANISM_DUPLICATE_SOLID).
        // Placed OUTSIDE the `has_active_solver` gate so the error surfaces on
        // `reify check` (no kernel needed — duplicate-solid detection is at
        // mechanism-builder eval).  Mirrors the detect_scope_coupling seam.
        diagnostics.extend(detect_mechanism_errors(&values));
        // Non-driving-joint diagnostics (task 4309 — E_MECHANISM_NONDRIVING_JOINT).
        // Same gate placement and rationale as detect_mechanism_errors above.
        // Passes `module` so the compile-span suppression predicate (task 4364)
        // can skip cells already flagged by the compiler at the same source span.
        diagnostics.extend(detect_nondriving_joint_errors(&values, module));
        // Ad-hoc selector Undef diagnostics (task 250).  @face/@edge cells left at
        // Value::Undef by the geometry-free eval path surface a warning here so the
        // eval/check path is behaviorally consistent with the build() path (which
        // emits a warning via geometry_ops.rs when a selector cannot be resolved).
        diagnostics.extend(detect_unresolved_ad_hoc_selectors(
            &module.templates,
            &values,
        ));
        // Geometry-consumer Undef diagnostics (task #4651 R1a).  Geometry-typed
        // builtins (adjacent_faces, normal, closest_point, centroid, …) require a
        // realized kernel and stay Value::Undef on the pure value-eval surface;
        // emitting E_EVAL_UNRESOLVED at Error severity makes this class loud.
        // `kernel_less` gates the scan to the no-kernel path only: build() calls
        // check() → eval() with a kernel registered, so consumers will be resolved
        // afterwards — firing here would be false positives (task #4651 fix).
        diagnostics.extend(detect_unresolved_geometry_consumers(
            &module.templates,
            &values,
            self.default_query_kernel().is_none(),
        ));

        EvalResult {
            values,
            diagnostics,
            resolved_params,
            objective_provenance,
        }
    }

    /// Resolve the default geometry kernel for single-handle ops (export,
    /// tessellate, containment queries).
    ///
    /// Returns `None` when no kernel is registered (no-kernel engine).
    /// The returned `dyn GeometryKernel` is the same kernel used by
    /// `geometry_ops.rs`'s `contains(solid, point)` dispatch.
    pub(crate) fn default_query_kernel(&self) -> Option<&dyn reify_ir::GeometryKernel> {
        self.default_kernel_name
            .as_deref()
            .and_then(|name| self.geometry_kernels.get(name))
            .map(|k| k.as_ref())
    }

    /// Build an `EvalContext` that ALWAYS carries `.with_meta + .with_determinacy +
    /// .with_runtime_diagnostics + .with_containment(self)`.
    ///
    /// Three of the five warm/edit cell-eval sites route through this constructor
    /// directly: `edit_param` Let loop, concurrent wave-2, and `eval_cached` Let
    /// branch.  The `eval_cached` Param-default closure (`default_or`) and the
    /// `edit_source` Let loop instead build the context inline for borrow-scope
    /// reasons, but they MUST keep both `.with_determinacy` and
    /// `.with_runtime_diagnostics` — dropping either silently makes
    /// `DeterminacyPredicate` cells return `Value::Undef` (task 4356).
    ///
    /// `.with_containment(self)` is added here (task 4222 δ) so that
    /// `sample(restrict(field, region), point)` calls receive the live OCCT
    /// containment hook.  Sites that build inline (eval_cached Param-default /
    /// edit_source Let loop) should also add `.with_containment(self)` where
    /// borrow scopes allow — omitting it only causes restricted-field samples
    /// on those paths to return `Value::Undef` instead of the inner value.
    ///
    /// Declared `pub(crate)` so `engine_edit.rs` and `concurrent.rs` (which live
    /// in separate modules in the same crate) can call it.
    pub(crate) fn cell_eval_ctx<'a>(
        &'a self,
        values: &'a ValueMap,
        snapshot_values: &'a PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        runtime_sink: &'a RefCell<Vec<Diagnostic>>,
    ) -> reify_expr::EvalContext<'a> {
        eval_ctx_with_meta(values, &self.functions, &self.meta_map)
            .with_determinacy(snapshot_values)
            .with_runtime_diagnostics(runtime_sink)
            .with_containment(self)
    }

    /// Evaluate a compiled module with caching and early cutoff.
    ///
    /// On first call (cold start), behaves like eval() but populates the cache.
    /// On subsequent calls with the same version, uses version fast path.
    /// On calls with a new version after invalidation, re-evaluates dirty nodes
    /// and uses early cutoff to avoid propagating unchanged results.
    pub fn eval_cached(&mut self, module: &CompiledModule, version: VersionId) -> CachedEvalResult {
        let mut values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let mut stats = CacheStats::default();
        // Determinacy accumulator for cell_eval_ctx (task 4356): mirrors the
        // snapshot_values approach in eval() so DeterminacyPredicate cells see
        // the correct DeterminacyState for every referent cell.
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        // Runtime-diagnostics sink for cell_eval_ctx (field-OOB warnings etc.).
        // Drained into `diagnostics` after the template loop (parity with eval()).
        let runtime_sink: RefCell<Vec<Diagnostic>> = RefCell::new(Vec::new());

        // Reset per-call test-instrumentation counters (same as eval()).
        self.last_param_override_type_kind_rejections = 0;
        self.last_param_override_dimension_rejections = 0;
        self.last_sub_component_unknown_structure_errors = 0;

        // Build meta_map from module templates (same logic as eval()).
        // This ensures MetaAccess expressions resolve correctly even when
        // eval_cached is called without a prior eval().
        self.meta_map = build_meta_map(module);

        // Resolve the active solver once per call so the named-vs-default
        // routing (Task 2300) is identical to eval(): `resolve_solver_for_module`
        // consults `module.solver_pragma` against the named-solver registry
        // (`Engine::register_solver`) and emits the "not registered" warning at
        // most once per eval_cached call (rather than once per template).
        // Inside the template loop, `lookup_solver_for_module` re-runs the
        // unwarned lookup so the &self borrow only spans the `.solve(&problem)`
        // expression and doesn't conflict with `&mut self` mutations elsewhere
        // in the loop body (e.g. `self.last_sub_component_unknown_structure_errors`).
        let has_active_solver = self
            .resolve_solver_for_module(module, &mut diagnostics)
            .is_some();

        for template in &module.templates {
            // Pre-seed Auto cells (unchanged; processed separately before the
            // unified Param+Let pass so Auto leaves are visible to topo-ordered
            // Param/Let expression evaluation — mirrors eval()'s Auto pre-seed).
            for cell in &template.value_cells {
                if cell.kind.is_auto() {
                    let node_id = NodeId::Value(cell.id.clone());

                    // Check version fast path
                    if let Some(CachedResult::Value(val, det)) =
                        self.cache.try_fast_path(&node_id, version)
                    {
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        snapshot_values.insert(cell.id.clone(), (val.clone(), det));
                        values.insert(cell.id.clone(), val);
                        stats.cache_hits += 1;
                        continue;
                    }

                    // Check cache reuse (not dirty, no override)
                    // Preserve existing freshness (Failed/Pending) — see the
                    // analogous let-cell block comment for rationale (arch §7.1/§9.2).
                    if !self.param_overrides.contains_key(&cell.id)
                        && !self.cache.is_dirty(&node_id)
                        && let Some(entry) = self.cache.get(&node_id)
                        && let CachedResult::Value(ref val, det) = entry.result
                    {
                        let val = val.clone();
                        let preserved_freshness = entry.freshness.clone();
                        snapshot_values.insert(cell.id.clone(), (val.clone(), det));
                        values.insert(cell.id.clone(), val);
                        let trace = entry.dependency_trace.clone();
                        let result = entry.result.clone();
                        self.cache.record_evaluation_with_freshness(
                            node_id.clone(),
                            result,
                            version,
                            trace,
                            preserved_freshness,
                        );
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        stats.cache_hits += 1;
                        continue;
                    }

                    stats.cache_misses += 1;

                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version,
                        payload: None,
                    });

                    // Use override if available, otherwise Undef with Auto determinacy
                    let (val, det) = if let Some(override_val) = self.param_overrides.get(&cell.id)
                    {
                        (override_val.clone(), DeterminacyState::Determined)
                    } else {
                        (reify_ir::Value::Undef, DeterminacyState::Auto)
                    };

                    let trace = DependencyTrace::default();
                    let cached_result = CachedResult::Value(val.clone(), det);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        version,
                        trace,
                    );

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version,
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });

                    if outcome == EvalOutcome::Unchanged {
                        stats.early_cutoffs += 1;
                    }

                    snapshot_values.insert(cell.id.clone(), (val.clone(), det));
                    values.insert(cell.id.clone(), val);
                }
            }

            // Unified single-pass evaluation of Param+Let cells in dependency order (§8.2).
            // Mirrors evaluate_params_and_lets_unified but preserves eval_cached's per-cell
            // cache fast-path / try_fast_path / is_dirty / record_evaluation_with_freshness
            // semantics while reordering Param+Let evaluation by data dependency over Auto
            // leaves (pre-seeded above).
            //
            // This replaces the old kind-partitioned two-pass:
            //   - pass 1: Param cells evaluated in declaration order (before lets in values)
            //   - pass 2: detect_let_cycle + let cells in topo order
            // with a single combined-graph topological sort so a param default correctly
            // observes any sibling let it reads (spec §8.2 order-independence).
            {
                // ── Pre-check: emit override-rejection warnings for ALL Param cells ───
                // Runs unconditionally before the topological sort so that Param cells
                // dropped from sorted_combined (cycle members) still surface their
                // rejection warnings. The topo loop's Param arm still calls
                // validate_param_override to determine the effective value but does NOT
                // re-emit the warning (avoiding double-emission for non-cyclic params).
                for cell in template
                    .value_cells
                    .iter()
                    .filter(|c| matches!(c.kind, ValueCellKind::Param))
                {
                    if let Some(v) = self.param_overrides.get(&cell.id)
                        && let Err(ref rejection) =
                            validate_param_override(v, &cell.cell_type, &self.structure_registry)
                    {
                        emit_param_override_rejection_warning(
                            &mut diagnostics,
                            &cell.id,
                            &cell.cell_type,
                            v,
                            rejection,
                            &mut self.last_param_override_type_kind_rejections,
                            &mut self.last_param_override_dimension_rejections,
                        );
                    }
                }

                // ── Steps 2–4: Build combined graph, topo-sort, emit cycle diagnostics ─
                // Delegates to build_combined_param_let_graph so the same graph-build
                // and cycle-detection logic is not duplicated between this path and
                // evaluate_params_and_lets_unified. partial_map_skip=false: ALL params
                // enter the graph (eval_cached "always writes a result" contract).
                let (_combined_nodes, combined_traces, sorted_combined) =
                    build_combined_param_let_graph(
                        template,
                        &self.param_overrides,
                        false,
                        &mut diagnostics,
                    );

                // ── Evaluate in topological order with cache fast-paths ────────────────
                for node_id in sorted_combined {
                    let cell_id = match &node_id {
                        NodeId::Value(vcid) => vcid.clone(),
                        _ => continue,
                    };
                    let cell = match template.value_cells.iter().find(|c| c.id == cell_id) {
                        Some(c) => c,
                        None => continue,
                    };

                    match cell.kind {
                        // ── Param cell ────────────────────────────────────────────────
                        ValueCellKind::Param => {
                            // Validate the override once, storing both the &Value borrow and the
                            // Result — no unconditional clone.
                            //
                            // The amend bdf65905d (task 2267) previously hoisted both the clone
                            // and the validation into a single `override_check: Option<(Value,
                            // Result<...>)>` binding so that the diagnostic pre-check and the
                            // cache-miss match could share the validated result. That sharing was
                            // correct in intent, but it unconditionally cloned the Value on EVERY
                            // Param cell visit — including the LSP-keystroke fast-path where the
                            // cloned value is immediately dropped.
                            //
                            // Task 2273 separates the two concerns:
                            //   • validation result  → stored in `override_entry.1` (no clone)
                            //   • value access       → `override_entry.0` holds a &Value borrow,
                            //     valid for the duration of this Param branch (param_overrides is
                            //     Ok(()) arm — the previous shape cloned on every Param
                            //     cell visit even on the LSP fast-path.
                            let override_entry: Option<(
                                &Value,
                                Result<(), ParamOverrideRejection>,
                            )> = self.param_overrides.get(&cell.id).map(|v| {
                                (
                                    v,
                                    validate_param_override(
                                        v,
                                        &cell.cell_type,
                                        &self.structure_registry,
                                    ),
                                )
                            });

                            // Override-rejection warning was already emitted in the
                            // pre-check loop above (before the topological sort) so it
                            // surfaces even for cycle-dropped Param cells that never
                            // appear in sorted_combined.

                            // Cache fast-path (same-version result is always fresh).
                            if let Some(CachedResult::Value(val, det)) =
                                self.cache.try_fast_path(&node_id, version)
                            {
                                self.journal.record(EvalEvent {
                                    timestamp: Instant::now(),
                                    node_id,
                                    kind: EventKind::CacheHit,
                                    version,
                                    payload: None,
                                });
                                snapshot_values.insert(cell.id.clone(), (val.clone(), det));
                                values.insert(cell.id.clone(), val);
                                stats.cache_hits += 1;
                                continue;
                            }

                            // Cache-reuse: not dirty + entry exists (no override).
                            // Preserve existing freshness (Failed/Pending) — arch §7.1/§9.2.
                            if !self.param_overrides.contains_key(&cell.id)
                                && !self.cache.is_dirty(&node_id)
                                && let Some(entry) = self.cache.get(&node_id)
                                && let CachedResult::Value(ref val, det) = entry.result
                            {
                                let val = val.clone();
                                let preserved_freshness = entry.freshness.clone();
                                snapshot_values.insert(cell.id.clone(), (val.clone(), det));
                                values.insert(cell.id.clone(), val);
                                let trace = entry.dependency_trace.clone();
                                let result = entry.result.clone();
                                self.cache.record_evaluation_with_freshness(
                                    node_id.clone(),
                                    result,
                                    version,
                                    trace,
                                    preserved_freshness,
                                );
                                self.journal.record(EvalEvent {
                                    timestamp: Instant::now(),
                                    node_id,
                                    kind: EventKind::CacheHit,
                                    version,
                                    payload: None,
                                });
                                stats.cache_hits += 1;
                                continue;
                            }

                            // Cache miss: evaluate and record.
                            stats.cache_misses += 1;

                            let start = Instant::now();
                            self.journal.record(EvalEvent {
                                timestamp: start,
                                node_id: node_id.clone(),
                                kind: EventKind::Started,
                                version,
                                payload: None,
                            });

                            // Evaluate default expression; mirrors the old Param branch
                            // (task 2273 validate-once / default_or invariant).
                            // Uses cell_eval_ctx so DeterminacyPredicate defaults see
                            // the determinacy map (task 4356).
                            let default_or =
                                |no_default_state: DeterminacyState| -> (Value, DeterminacyState) {
                                    if let Some(ref expr) = cell.default_expr {
                                        (
                                            reify_expr::eval_expr(
                                                expr,
                                                &eval_ctx_with_meta(
                                                    &values,
                                                    &self.functions,
                                                    &self.meta_map,
                                                )
                                                .with_determinacy(&snapshot_values)
                                                .with_runtime_diagnostics(&runtime_sink),
                                            ),
                                            DeterminacyState::Determined,
                                        )
                                    } else {
                                        (reify_ir::Value::Undef, no_default_state)
                                    }
                                };
                            let (val, det) = match override_entry {
                                Some((override_val, Ok(()))) => {
                                    (override_val.clone(), DeterminacyState::Determined)
                                }
                                Some((_, Err(_))) => default_or(DeterminacyState::Undetermined),
                                None => default_or(DeterminacyState::Determined),
                            };
                            // drop the closure to release borrows of &values, &snapshot_values,
                            // &runtime_sink before the mutable snapshot_values.insert below.
                            let _ = default_or;

                            // Use the actual dependency trace from combined_traces so that
                            // dirty-cone propagation marks dependents when an upstream let
                            // changes. The old two-pass used DependencyTrace::default()
                            // (empty) for all Params, silently breaking incremental
                            // invalidation for param defaults that read sibling lets.
                            let trace = combined_traces.get(&node_id).cloned().unwrap_or_default();

                            let cached_result = CachedResult::Value(val.clone(), det);
                            let outcome = self.cache.record_evaluation(
                                node_id.clone(),
                                cached_result,
                                version,
                                trace,
                            );

                            self.journal.record(EvalEvent {
                                timestamp: Instant::now(),
                                node_id,
                                kind: EventKind::Completed { outcome },
                                version,
                                payload: Some(EventPayload::Duration(start.elapsed())),
                            });

                            if outcome == EvalOutcome::Unchanged {
                                stats.early_cutoffs += 1;
                            }

                            snapshot_values.insert(cell.id.clone(), (val.clone(), det));
                            values.insert(cell.id.clone(), val);
                        }

                        // ── Let cell ──────────────────────────────────────────────────────
                        ValueCellKind::Let => {
                            let expr = match &cell.default_expr {
                                Some(e) => e,
                                None => continue,
                            };

                            // Cache fast-path.
                            if let Some(CachedResult::Value(val, det)) =
                                self.cache.try_fast_path(&node_id, version)
                            {
                                self.journal.record(EvalEvent {
                                    timestamp: Instant::now(),
                                    node_id,
                                    kind: EventKind::CacheHit,
                                    version,
                                    payload: None,
                                });
                                snapshot_values.insert(cell.id.clone(), (val.clone(), det));
                                values.insert(cell.id.clone(), val);
                                stats.cache_hits += 1;
                                continue;
                            }

                            // Cache-reuse: not dirty + entry exists.
                            // Preserve existing freshness (Failed/Pending) — arch §7.1/§9.2.
                            // See the detailed rationale in the old second-pass let-cell block.
                            if !self.cache.is_dirty(&node_id)
                                && let Some(entry) = self.cache.get(&node_id)
                                && let CachedResult::Value(ref val, det) = entry.result
                            {
                                let val = val.clone();
                                let preserved_freshness = entry.freshness.clone();
                                snapshot_values.insert(cell.id.clone(), (val.clone(), det));
                                values.insert(cell.id.clone(), val);
                                let trace = entry.dependency_trace.clone();
                                let result = entry.result.clone();
                                self.cache.record_evaluation_with_freshness(
                                    node_id.clone(),
                                    result,
                                    version,
                                    trace,
                                    preserved_freshness,
                                );
                                self.journal.record(EvalEvent {
                                    timestamp: Instant::now(),
                                    node_id,
                                    kind: EventKind::CacheHit,
                                    version,
                                    payload: None,
                                });
                                stats.cache_hits += 1;
                                continue;
                            }

                            // Cache miss: evaluate and record.
                            stats.cache_misses += 1;
                            self.cache.clear_dirty(&node_id);

                            let start = Instant::now();
                            self.journal.record(EvalEvent {
                                timestamp: start,
                                node_id: node_id.clone(),
                                kind: EventKind::Started,
                                version,
                                payload: None,
                            });

                            // Use cell_eval_ctx so DeterminacyPredicate cells (e.g.
                            // `let r = determined(x)`) see the determinacy map (task 4356).
                            let val = reify_expr::eval_expr(
                                expr,
                                &self.cell_eval_ctx(&values, &snapshot_values, &runtime_sink),
                            );

                            // Use the actual trace from combined_traces (same as the eval()
                            // unified pass; replaces the old let_traces from detect_let_cycle).
                            let trace = combined_traces
                                .get(&node_id)
                                .cloned()
                                .expect("sorted_combined ⊆ combined_traces.keys() by construction");

                            let cached_result =
                                CachedResult::Value(val.clone(), DeterminacyState::Determined);
                            let outcome = self.cache.record_evaluation(
                                node_id.clone(),
                                cached_result,
                                version,
                                trace,
                            );

                            self.journal.record(EvalEvent {
                                timestamp: Instant::now(),
                                node_id,
                                kind: EventKind::Completed { outcome },
                                version,
                                payload: Some(EventPayload::Duration(start.elapsed())),
                            });

                            if outcome == EvalOutcome::Unchanged {
                                stats.early_cutoffs += 1;
                                self.cache.clear_dependents_dirty(&cell.id);
                            }

                            snapshot_values.insert(
                                cell.id.clone(),
                                (val.clone(), DeterminacyState::Determined),
                            );
                            values.insert(cell.id.clone(), val);
                        }

                        _ => {}
                    }
                }
            }

            // Sub-component validation pass: emit "unknown structure" error for any
            // sub_component whose structure_name has no matching template in the
            // module OR the stdlib prelude (the lookup set must match eval()'s
            // elaborating loop, esc-4287-15). Mirrors eval()'s resolver. We do NOT
            // elaborate child instances here — this is lookup-only by design (see
            // design decision in plan).
            for sub in &template.sub_components {
                if find_template_with_prelude(module, self.prelude, &sub.structure_name).is_none() {
                    self.last_sub_component_unknown_structure_errors += 1;
                    diagnostics.push(Diagnostic::error(format!(
                        "sub-component \"{}\" references unknown structure \"{}\"",
                        sub.name, sub.structure_name
                    )));
                }
            }

            // Solver pass: invoke constraint solver for diagnostic purposes.
            // Mirrors eval() lines 1006-1158, but with an intentionally empty Solved arm —
            // value/snapshot updates in eval_cached are a separate gap (see design decision:
            // "Solver Solved arm in eval_cached is intentionally empty"). Only the
            // Infeasible and NoProgress arms matter for this task's diagnostic-emission goal.
            //
            // `has_active_solver` was computed once before the template loop via
            // `resolve_solver_for_module` so the "not registered" warning is emitted
            // at most once. Inside the loop we re-run `lookup_solver_for_module`
            // (no warning, single expression) to obtain the solver reference for
            // the `.solve(&problem)` call without holding the &self borrow across
            // the surrounding &mut self mutations.
            if has_active_solver {
                // Build the ResolutionProblem; returns None when there are no auto cells.
                // `build_solver_problem` centralises construction so both eval() and
                // eval_cached() build identical inputs to the solver (pinned by the
                // `eval_and_eval_cached_emit_byte_identical_solver_no_progress_warning` test).
                // The solver must run on every eval_cached call — even when all auto cells hit
                // the cache — so that Infeasible/NoProgress diagnostics surface on every LSP
                // keystroke. See step-10/step-11 regression tests.
                if let Some(problem) =
                    build_solver_problem(template, &values, Arc::clone(&self.functions))
                {
                    // Per-iteration cost of `lookup_solver_for_module`: one
                    // `solver_pragma.as_ref()` match plus at most one
                    // `HashMap::get`, negligible vs. `.solve(&problem)`. See
                    // the matching note at the eval() site (~line 1235) for
                    // why this is preferred over hoisting the name outside
                    // the loop. (Task 2300 reviewer comment.)
                    let solve_result = self
                        .lookup_solver_for_module(module)
                        .expect("has_active_solver is true => solver lookup returns Some")
                        .solve(&problem);

                    match solve_result {
                        SolveResult::Solved {
                            values: solver_values,
                            unique,
                        } => {
                            // θ (task 4361) step-4: back-prop solved autos into
                            // values/snapshot_values/cache and re-evaluate downstream
                            // let cells, mirroring cold eval() (:2728) and edit_param
                            // (engine_edit.rs:1360). The four warm-resolution sites
                            // (eval_cached, eval, edit_param, concurrent) must stay in sync.
                            //
                            // VERSION / TRACE NOTE (for future readers):
                            //
                            // Unlike cold eval() which allocates a fresh internal
                            // `res_version_id` for resolution-phase cache entries (so all
                            // entries share one snapshot basis; see eval() :2780-2783), this
                            // warm path records entries under the caller-supplied `version`.
                            //
                            // This is intentional and safe for the following reasons:
                            //
                            // 1. eval_cached operates in CALLER-VERSION space (the version
                            //    token supplied by the caller — e.g. an edit serial).  Cold
                            //    eval() operates in INTERNAL-VERSION space (engine-owned
                            //    `next_version_id` counter).  These are separate namespaces;
                            //    `try_fast_path` compares `entry.basis_version ==
                            //    current_version` using whichever namespace was recorded.
                            //
                            // 2. After cold eval() records entries under `VersionId(N)` (an
                            //    internal counter), the first `eval_cached(module, VersionId(V))`
                            //    call will see mismatches (N ≠ V) → cache misses → re-runs the
                            //    solver → records under `VersionId(V)`.  A second call with the
                            //    same `VersionId(V)` then hits the fast path.  This is correct
                            //    incremental behavior: the caller controls what "current" means.
                            //
                            // 3. `DependencyTrace::default()` (empty trace) is also correct for
                            //    the warm path.  Cold eval() uses the same empty trace at :2812.
                            //    An empty trace means the cache entry has NO inter-cell
                            //    dependencies recorded; it is invalidated purely by version
                            //    change (the caller bumping `version`), which is the right
                            //    semantic for solver-resolved auto params — they are
                            //    re-evaluated whenever the caller signals a new version, not
                            //    when a specific dependency changes.
                            //
                            // There is therefore no incremental fast-path desync between
                            // cold eval() and eval_cached: they record in separate version
                            // spaces, each consistent with the path that wrote them.
                            let mut resolved_ids: HashSet<ValueCellId> = HashSet::new();

                            for (id, val) in &solver_values {
                                values.insert(id.clone(), val.clone());
                                resolved_ids.insert(id.clone());
                                snapshot_values.insert(
                                    id.clone(),
                                    (val.clone(), DeterminacyState::Determined),
                                );

                                let node_id = NodeId::Value(id.clone());
                                let cached_result = CachedResult::Value(
                                    val.clone(),
                                    DeterminacyState::Determined,
                                );
                                self.cache.record_evaluation(
                                    node_id,
                                    cached_result,
                                    version,
                                    DependencyTrace::default(),
                                );
                            }

                            if !unique {
                                for ap in &problem.auto_params {
                                    if ap.free {
                                        diagnostics.push(Diagnostic::warning(format!(
                                            "Parameter `{}` resolved via auto(free) \
                                             -- result is not uniquely determined.",
                                            ap.id.member
                                        )));
                                    }
                                }
                            }

                            // Second wave: re-evaluate downstream let cells that read
                            // the resolved autos. Collect nodes+exprs while holding
                            // the immutable eval_state borrow; release before &mut self ops.
                            // Pattern mirrors edit_param (engine_edit.rs:1423) and
                            // concurrent (concurrent.rs:463).
                            if !resolved_ids.is_empty() {
                                let nodes_to_reeval: Vec<(NodeId, CompiledExpr)> =
                                    if let Some(es) = self.eval_state.as_ref() {
                                        let wave2_dirty = crate::dirty::compute_dirty_cone(
                                            &resolved_ids,
                                            &es.reverse_index,
                                            &es.snapshot.graph,
                                        );
                                        let wave2_eval = crate::dirty::compute_eval_set(
                                            &wave2_dirty,
                                            &self.demand,
                                            &es.trace_map,
                                        );
                                        wave2_eval
                                            .into_iter()
                                            .filter_map(|node_id| {
                                                if let NodeId::Value(vcid) = &node_id
                                                    && let Some(node) =
                                                        es.snapshot.graph.value_cells.get(vcid)
                                                    && let Some(ref expr) = node.default_expr
                                                {
                                                    return Some((node_id, expr.clone()));
                                                }
                                                None
                                            })
                                            .collect()
                                    } else {
                                        Vec::new()
                                    };

                                for (node_id, expr) in nodes_to_reeval {
                                    let val = reify_expr::eval_expr(
                                        &expr,
                                        &self.cell_eval_ctx(
                                            &values,
                                            &snapshot_values,
                                            &runtime_sink,
                                        ),
                                    );
                                    if let NodeId::Value(vcid) = &node_id {
                                        values.insert(vcid.clone(), val.clone());
                                        snapshot_values.insert(
                                            vcid.clone(),
                                            (val.clone(), DeterminacyState::Determined),
                                        );
                                    }
                                    let trace = extract_dependency_trace(&expr);
                                    let cached_result =
                                        CachedResult::Value(val, DeterminacyState::Determined);
                                    self.cache.record_evaluation(
                                        node_id,
                                        cached_result,
                                        version,
                                        trace,
                                    );
                                }
                            }
                        }
                        SolveResult::Infeasible {
                            diagnostics: solver_diags,
                        } => {
                            diagnostics.extend(solver_diags);
                        }
                        SolveResult::NoProgress { reason } => {
                            diagnostics.push(Diagnostic::warning(format!(
                                "Constraint solver made no progress: {}",
                                reason
                            )));
                        }
                    }
                }
            }
        }

        // Drain runtime diagnostics (field-OOB warnings, etc.) collected via
        // cell_eval_ctx during the template pass — parity with eval() (task 4356).
        diagnostics.append(&mut runtime_sink.borrow_mut());

        // R2a symbolic-mint pass (task #4652, step-4): mirrors eval() call above.
        // Runs AFTER scalar template evaluation and BEFORE diagnostic passes so
        // the LSP/GUI incremental path also sees symbolic GeometryHandles.
        Engine::mint_symbolic_geometry_handles_into_values(
            module,
            &mut values,
            &self.functions,
            &self.meta_map,
        );

        // R2b symbolic selector-mint pass (task #4653, step-6): mirrors the
        // eval() call above so the LSP/GUI incremental path also sees
        // symbolic topology selectors.  Runs immediately after handle-mint.
        crate::geometry_ops::mint_symbolic_topology_selectors_into_values(
            module,
            &mut values,
            &mut diagnostics,
        );

        // Mechanism error diagnostics (task 4308 — E_MECHANISM_DUPLICATE_SOLID).
        // Mirrors the eval() call site (above detect_scope_coupling).  eval_cached
        // is the incremental LSP path; without this call the GUI/LSP would drop the
        // MechanismDuplicateSolid diagnostic on every incremental re-evaluation.
        diagnostics.extend(detect_mechanism_errors(&values));
        // Non-driving-joint diagnostics (task 4309 — E_MECHANISM_NONDRIVING_JOINT).
        // Mirrors eval() call site; eval_cached is the LSP/GUI incremental path.
        // Passes `module` for compile-span suppression parity with eval() (task 4364).
        diagnostics.extend(detect_nondriving_joint_errors(&values, module));
        // Ad-hoc selector Undef diagnostics (task 250).  Mirrors eval() call site so
        // the LSP/GUI incremental path surfaces the same selector-frame-is-undef
        // warning as the cold-eval path.
        diagnostics.extend(detect_unresolved_ad_hoc_selectors(
            &module.templates,
            &values,
        ));
        // Geometry-consumer Undef diagnostics (task #4651 R1a).  Mirrors eval()
        // call site; eval_cached is the LSP/GUI incremental path and must surface
        // the same E_EVAL_UNRESOLVED errors as the cold-eval path (diagnostic
        // parity).  `kernel_less` gates the scan to the no-kernel path only
        // (same rationale as the eval() call site above).
        diagnostics.extend(detect_unresolved_geometry_consumers(
            &module.templates,
            &values,
            self.default_query_kernel().is_none(),
        ));

        // Build and store a snapshot so that engine.snapshot() returns Some after
        // eval_cached() — preserving cross-path parity with eval() (spec §8.2,
        // task 4317 step-6).
        //
        // snapshot.values is populated from the evaluated `values` map; the
        // DeterminacyState for each cell is read from the cache entry that
        // record_evaluation / record_evaluation_with_freshness wrote during the
        // unified Param+Let topological pass above.  Cells absent from `values`
        // (cells that were short-circuited by a fast-path or that have no entry
        // yet) keep the (Undef, Undetermined/Auto) initialised by
        // Snapshot::from_compiled_module.
        {
            let snapshot_id = self.next_snapshot_id;
            self.next_snapshot_id += 1;
            let mut snapshot = Snapshot::from_compiled_module(module);
            snapshot.id = SnapshotId(snapshot_id);
            snapshot.version = version;
            snapshot.provenance = SnapshotProvenance::Initial;

            for (cell_id, val) in values.iter() {
                let node_id = NodeId::Value(cell_id.clone());
                let det = self
                    .cache
                    .get(&node_id)
                    .and_then(|entry| match &entry.result {
                        CachedResult::Value(_, det) => Some(*det),
                        _ => None,
                    })
                    .unwrap_or(DeterminacyState::Undetermined);
                snapshot.values.insert(cell_id.clone(), (val.clone(), det));
            }

            self.eval_state = Some(EvaluationState {
                snapshot,
                reverse_index: ReverseDependencyIndex::default(),
                trace_map: HashMap::new(),
            });
        }

        // Re-apply active purpose bindings against the fresh snapshot (task 3260 / task 3103).
        // The snapshot created above has no purpose-injected constraints; re-injection
        // rebuilds them against the new graph.  Mirrors the eval() pattern: save bindings via
        // mem::take (so activate_purpose_constraints_with_bindings_inner's already-active guard
        // is not hit), clear derived state (old constraint node IDs are invalid in the new
        // snapshot), then re-inject.  activate_purpose_constraints_with_bindings_inner
        // repopulates active_purpose_bindings, active_purposes, active_objective_map, and
        // active_tolerance_scope from scratch so the engine state is consistent after this block.
        {
            let mut preserved_bindings: Vec<(String, Vec<(String, String)>)> =
                std::mem::take(&mut self.active_purpose_bindings)
                    .into_iter()
                    .collect();
            // Sort for deterministic re-injection order (matches eval() line 1654).
            preserved_bindings.sort_by(|a, b| a.0.cmp(&b.0));
            if !preserved_bindings.is_empty() {
                self.active_purposes.clear();
                self.active_objective_map.clear();
                self.active_tolerance_scope.clear();
                let mut any_injected = false;
                for (purpose_name, param_bindings) in &preserved_bindings {
                    any_injected |= self.activate_purpose_constraints_with_bindings_inner(
                        purpose_name,
                        param_bindings,
                    );
                }
                if any_injected {
                    self.rebuild_purpose_infrastructure();
                }
            }
        }

        CachedEvalResult {
            eval_result: EvalResult {
                values,
                diagnostics,
                resolved_params: HashMap::new(),
                objective_provenance: HashMap::new(),
            },
            stats,
        }
    }

    /// Persistent-cache key for a solver `ComputeNode`: the structural
    /// [`compute_cache_key`](crate::compute_cache_key::compute_cache_key) (value /
    /// realization inputs + target + `options_hash`) COMBINED with a content hash
    /// of the fully-evaluated `arg_values`.
    ///
    /// The structural key ALONE is incomplete for persistent caching. The γ-slice
    /// shallow walk that builds `value_inputs` keeps only *direct* `ValueRef` args,
    /// so the boundary conditions — `loads`/`supports` passed as list literals
    /// (`[tip_load]` / `[mount]`) — plus the `ElasticOptions` ctor are dropped, and
    /// `options_hash` is currently `ContentHash(0)`. All of these change the FEA
    /// result, so a structural-only key produces FALSE persistent-cache HITS (two
    /// solves differing only in load magnitude collide). Folding a content hash of
    /// the evaluated `arg_values` captures every result-affecting input regardless
    /// of how it is expressed. It errs toward spurious MISSES on execution-only
    /// option changes (e.g. thread count) — safe — but never a false HIT. A future
    /// deep input walk / `ElasticOptions::cacheable_hash`
    /// (`docs/prds/v0_3/structural-analysis-fea.md` task #4) can tighten this to
    /// permit cross-config hits.
    fn persistent_cache_key(
        node: &crate::graph::ComputeNodeData,
        graph: &crate::graph::EvaluationGraph,
        arg_values: &[Value],
    ) -> reify_core::ContentHash {
        crate::compute_cache_key::compute_cache_key(node, graph).combine(
            reify_core::ContentHash::combine_all(arg_values.iter().map(|v| v.content_hash())),
        )
    }

    /// task 3594/δ step-12: on the shell route, insert + dispatch an upstream
    /// `shell-extract::extract` ComputeNode feeding the `solver::elastic_static`
    /// FEA node, returning its synthetic output [`ValueCellId`] so the caller can
    /// splice it into the FEA node's `value_inputs` (the upstream→downstream
    /// segmentation edge the e2e asserts on).
    ///
    /// Gated by classifying the evaluated options arg (`arg_values[6]`) + dims
    /// (`arg_values[1..=3]`) via the *same* `extract_shell_route_params` +
    /// `classify_shell` helpers the FEA trampoline uses, so the graph wiring and
    /// the trampoline's own Shell/Tet routing always agree. Returns `None` (no
    /// upstream node, no edge) for the Tet route, malformed args, or a failed
    /// extraction under the soft-fallback policy.
    ///
    /// Per PRD §11 OQ-2 the upstream node is fed a *synthetic* slab SDF
    /// (`build_slab_sdf` from the body dims): it exists to satisfy the
    /// graph/segmentation contract and `Complete` cleanly, NOT as the geometry
    /// source for the v0.4 flat-plate stress solve (that mesh is synthesized
    /// inside `solve_flat_plate_shell`).
    ///
    /// Failure policy (`resolve_extraction_failure`): under `ShellForce::On`
    /// (`HardError`) a Failed extraction surfaces the trampoline's Error
    /// diagnostics; under `Auto`/`Off` (`TetFallbackWithWarning`) it is
    /// downgraded to a single Warning and the edge is dropped (the FEA node
    /// proceeds — its own classification still drives the actual solve).
    fn insert_shell_extract_upstream(
        &mut self,
        snapshot: &mut Snapshot,
        entity: &str,
        arg_values: &[Value],
        version_id: u64,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<reify_core::ValueCellId> {
        use crate::compute_targets::elastic_static::extract_shell_route_params;
        use crate::compute_targets::shell_solve::{
            FailurePolicy, ShellRoute, build_slab_sdf, classify_shell, resolve_extraction_failure,
        };

        // FEA fn signature: (material, length, width, height, loads, supports,
        // options). The shell route reads dims [1..=3] + options [6].
        if arg_values.len() < 7 {
            return None;
        }

        // Non-panicking scalar read: a malformed dim (not a `Value::Scalar`)
        // skips the shell route rather than crashing the lowering.
        fn scalar_si(v: &Value) -> Option<f64> {
            match v {
                Value::Scalar { si_value, .. } => Some(*si_value),
                _ => None,
            }
        }
        let length = scalar_si(&arg_values[1])?;
        let width = scalar_si(&arg_values[2])?;
        let height = scalar_si(&arg_values[3])?;

        let (shell_force, shell_threshold) = extract_shell_route_params(&arg_values[6]);
        if classify_shell(shell_force, length, width, height, shell_threshold) != ShellRoute::Shell
        {
            return None;
        }

        // Allocate a fresh per-entity ComputeNodeId index (`max(index)+1`,
        // matching the FEA node's own allocation) + a distinct synthetic output
        // cell. The angle-bracketed member is not a valid DSL identifier, so it
        // can never collide with a real value cell.
        let extract_index: u32 = snapshot
            .graph
            .compute_nodes
            .iter()
            .filter(|(id, _)| id.entity.as_str() == entity)
            .map(|(id, _)| id.index)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        let extract_c_id = reify_core::ComputeNodeId::new(entity, extract_index);
        let extract_output_cell = reify_core::ValueCellId {
            entity: entity.to_string(),
            member: format!("<shell-extract-{extract_index}>"),
        };

        // Trampoline inputs (γ-only seam): value_inputs[0]=options,
        // value_inputs[1]=synthetic slab SDF (a `Value::SampledField`).
        let extract_args = vec![arg_values[6].clone(), build_slab_sdf(height)];

        let extract_cancel = crate::graph::CancellationHandle::new();
        // task #3428 step-2: populate cache_key via compute_cache_key before
        // insertion. Even though this node has no graph-tracked value/realization
        // inputs (its args are synthetic, not ValueCellId-addressed), the key
        // still encodes the target string — making it non-zero and deterministic.
        let mut extract_node = crate::graph::ComputeNodeData {
            computation_id: extract_c_id.clone(),
            target: "shell-extract::extract".to_string(),
            // The upstream node's inputs are the synthetic options + slab SDF
            // passed directly to dispatch; it has no value-cell inputs.
            value_inputs: vec![],
            realization_inputs: vec![],
            options_hash: reify_core::ContentHash(0),
            cache_key: reify_core::ContentHash(0),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: Some(extract_cancel.clone()),
            output_value_cells: vec![extract_output_cell.clone()],
        };
        let ck = crate::compute_cache_key::compute_cache_key(&extract_node, &snapshot.graph);
        extract_node.cache_key = ck;
        snapshot.graph.insert_compute_node(extract_node);

        let outcome = self.run_compute_dispatch(
            &extract_c_id,
            std::slice::from_ref(&extract_output_cell),
            "shell-extract::extract",
            &extract_args,
            &[],
            &Value::Undef,
            &extract_cancel,
            VersionId(version_id),
            ck, // task #3428 step-6: persistent-cache input key
        );
        // Clear the running slot on every terminal outcome (mirrors the FEA path).
        if let Some(n) = snapshot.graph.get_compute_node_mut(&extract_c_id) {
            n.running = None;
        }

        match outcome {
            Ok((result, diags)) => {
                // Completed — surface any (normally empty) diagnostics and wire
                // the upstream→downstream edge.
                diagnostics.extend(diags);
                // task #3428: register the synthetic shell-extract output cell in
                // graph.value_cells so the DOWNSTREAM FEA node's compute_cache_key
                // can resolve it (the cell is pushed into that node's value_inputs
                // as `shell_extract_feed`). The content_hash is the extract result's
                // hash, so the downstream persistent-cache key soundly depends on the
                // shell-extract output — a different extraction yields a different key
                // (no false cache hits). cell_type is inferred from the result Value;
                // the representable fallback keeps assert_value_cell_types_representable
                // satisfied for the synthetic, expr-less cell.
                let cell_type = result
                    .try_infer_type()
                    .unwrap_or_else(reify_core::Type::dimensionless_scalar);
                snapshot.graph.value_cells.insert(
                    extract_output_cell.clone(),
                    crate::graph::ValueCellNode {
                        id: extract_output_cell.clone(),
                        kind: ValueCellKind::Let,
                        cell_type,
                        default_expr: None,
                        content_hash: result.content_hash(),
                    },
                );
                Some(extract_output_cell)
            }
            Err(crate::engine_compute::DispatchError::Failed(diags)) => {
                match resolve_extraction_failure(shell_force) {
                    FailurePolicy::HardError => {
                        // ShellForce::On — surface the extraction Error diagnostics.
                        diagnostics.extend(diags);
                    }
                    FailurePolicy::TetFallbackWithWarning => {
                        // Auto/Off — downgrade to a single Warning and proceed.
                        diagnostics.push(Diagnostic::warning(
                            "shell-extract::extract failed; falling back to tet \
                             meshing (ShellForce::Auto/Off soft fallback)",
                        ));
                    }
                }
                // No usable extraction output → drop the edge.
                None
            }
            Err(crate::engine_compute::DispatchError::Cancelled) => {
                // Cancelled mid-flight — leave the output cell Pending, no edge.
                None
            }
        }
    }

    /// Unified single-pass evaluation for a template's Auto, Param, and Let cells.
    ///
    /// Replaces the kind-partitioned two-pass (pass-1: Param+Auto in cell order;
    /// pass-2: `evaluate_let_bindings` for Lets only) with:
    ///
    /// 1. Pre-seed every Auto cell as `(Undef, DeterminacyState::Auto)` leaf.
    /// 2. Build a combined Param+Let dependency graph and topologically sort it.
    /// 3. Evaluate cells in that order, dispatching to the existing per-cell
    ///    Param logic (override precedence, PARTIAL-MAP skip, REJECTED-OVERRIDE
    ///    Undef insert, journal/cache recording) and the existing Let logic
    ///    (pre-eval Pending gate, @optimized dispatch, panic boundary,
    ///    `record_evaluation_propagating_freshness`).
    ///
    /// A param whose default reads a sibling let now evaluates AFTER that let,
    /// fixing the §8.2 order-independence bug (task 4317).
    ///
    /// Let-only cycle detection: emits a `circular let-binding dependency`
    /// `Diagnostic::error` for cycles within the Let-only subgraph (step-2
    /// scope). Cross-kind param↔let cycles are detected as dropped nodes in the
    /// combined topological sort but not yet diagnosed (step-4 extends this).
    ///
    /// The subsequent passes (guarded groups, sub-component elaboration,
    /// post-solver evaluate_let_bindings) are UNCHANGED.
    #[allow(clippy::too_many_arguments)]
    fn evaluate_params_and_lets_unified(
        &mut self,
        template: &reify_compiler::TopologyTemplate,
        values: &mut ValueMap,
        snapshot: &mut Snapshot,
        version_id: u64,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        diagnostics: &mut Vec<Diagnostic>,
        runtime_sink: &RefCell<Vec<Diagnostic>>,
    ) {
        let version = VersionId(version_id);

        // ── Step 1: Pre-seed Auto cells as (Undef, Auto) leaves ──────────────
        // Identical to the old first-pass Auto arm; must run before the unified
        // Param+Let pass so that downstream expressions can read Undef for
        // auto-param slots (solver fills them later).
        for cell in &template.value_cells {
            if cell.kind.is_auto() {
                let node_id = NodeId::Value(cell.id.clone());
                let start = Instant::now();
                self.journal.record(EvalEvent {
                    timestamp: start,
                    node_id: node_id.clone(),
                    kind: EventKind::Started,
                    version,
                    payload: None,
                });

                values.insert(cell.id.clone(), reify_ir::Value::Undef);
                snapshot.values.insert(
                    cell.id.clone(),
                    (reify_ir::Value::Undef, DeterminacyState::Auto),
                );

                // Record in cache
                let trace = DependencyTrace::default();
                let cached_result =
                    CachedResult::Value(reify_ir::Value::Undef, DeterminacyState::Auto);
                let outcome =
                    self.cache
                        .record_evaluation(node_id.clone(), cached_result, version, trace);

                self.journal.record(EvalEvent {
                    timestamp: Instant::now(),
                    node_id,
                    kind: EventKind::Completed { outcome },
                    version,
                    payload: Some(EventPayload::Duration(start.elapsed())),
                });
            }
        }

        // ── Pre-check: emit override-rejection warnings for ALL Param cells ───────
        // Runs unconditionally BEFORE the topological sort so that Param cells
        // dropped from sorted_combined (cycle members) still surface their
        // rejection warnings. The topo loop's Param arm still calls
        // validate_param_override to determine the effective value but does NOT
        // re-emit the warning (avoiding double-emission for non-cyclic params).
        for cell in template
            .value_cells
            .iter()
            .filter(|c| matches!(c.kind, ValueCellKind::Param))
        {
            if let Some(v) = self.param_overrides.get(&cell.id)
                && let Err(ref rejection) =
                    validate_param_override(v, &cell.cell_type, &self.structure_registry)
            {
                emit_param_override_rejection_warning(
                    diagnostics,
                    &cell.id,
                    &cell.cell_type,
                    v,
                    rejection,
                    &mut self.last_param_override_type_kind_rejections,
                    &mut self.last_param_override_dimension_rejections,
                );
            }
        }

        // ── Steps 2–4: Build combined graph, topo-sort, emit cycle diagnostics ───
        // Extracted into build_combined_param_let_graph so the same logic is not
        // duplicated in the eval_cached path. partial_map_skip=true preserves the
        // eval() PARTIAL-MAP invariant (params with no override AND no default are
        // absent from values by design).
        let (_combined_nodes, mut combined_traces, sorted_combined) =
            build_combined_param_let_graph(template, &self.param_overrides, true, diagnostics);

        // ── Step 5: Unified evaluation in topological order ───────────────────
        // Cells dropped from `sorted_combined` (cycles) remain Undef — the cycle
        // diagnostic above is the only effect. The cell-order lookup below is
        // O(n) per cell; for typical template sizes (single-digit to low dozens)
        // this is negligible.
        for node_id in sorted_combined {
            let cell_id = match &node_id {
                NodeId::Value(vcid) => vcid.clone(),
                _ => continue,
            };
            let cell = match template.value_cells.iter().find(|c| c.id == cell_id) {
                Some(c) => c,
                None => continue,
            };

            match cell.kind {
                // ── Param cell (verbatim from old pass-1 Param branch) ────────
                ValueCellKind::Param => {
                    let override_val = match self.param_overrides.get(&cell.id) {
                        None => {
                            // PARTIAL-MAP INVARIANT — see Engine::eval pass-1 comment.
                            // Should not reach here (excluded from combined_nodes above),
                            // but guard defensively.
                            if cell.default_expr.is_none() {
                                continue;
                            }
                            None
                        }
                        Some(v) => match validate_param_override(
                            v,
                            &cell.cell_type,
                            &self.structure_registry,
                        ) {
                            Ok(()) => Some(v.clone()),
                            Err(_) => {
                                // Rejection warning already emitted in the pre-check
                                // loop before the topological sort; do not re-emit here.
                                None
                            }
                        },
                    };

                    let node_id_p = NodeId::Value(cell.id.clone());
                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id_p.clone(),
                        kind: EventKind::Started,
                        version,
                        payload: None,
                    });

                    let val = if let Some(v) = override_val {
                        v
                    } else if let Some(ref expr) = cell.default_expr {
                        reify_expr::eval_expr(
                            expr,
                            &eval_ctx_with_meta(values, functions, meta_map)
                                .with_determinacy(&snapshot.values),
                        )
                    } else {
                        // REJECTED-OVERRIDE-NO-DEFAULT: write (Undef, Undetermined).
                        values.insert(cell.id.clone(), Value::Undef);
                        snapshot.values.insert(
                            cell.id.clone(),
                            (Value::Undef, DeterminacyState::Undetermined),
                        );
                        record_eval_completed(
                            &mut self.journal,
                            &mut self.cache,
                            node_id_p,
                            CachedResult::Value(Value::Undef, DeterminacyState::Undetermined),
                            version,
                            start,
                        );
                        continue;
                    };
                    values.insert(cell.id.clone(), val.clone());
                    snapshot
                        .values
                        .insert(cell.id.clone(), (val.clone(), DeterminacyState::Determined));
                    record_eval_completed(
                        &mut self.journal,
                        &mut self.cache,
                        node_id_p,
                        CachedResult::Value(val, DeterminacyState::Determined),
                        version,
                        start,
                    );
                }

                // ── Let cell (adapted from evaluate_let_bindings loop body) ───
                // Uses `combined_traces` (mutable) in place of `let_traces`.
                // All other logic is identical to evaluate_let_bindings.
                ValueCellKind::Let => {
                    let expr = match &cell.default_expr {
                        Some(e) => e,
                        None => continue, // Should not happen (excluded above).
                    };

                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version: VersionId(version_id),
                        payload: None,
                    });

                    // Snapshot test-instrumentation panic-injection state
                    #[cfg(any(test, feature = "test-instrumentation"))]
                    let force_panic = self.panic_on_eval_cells.contains(&cell_id);

                    // Arch §7.2 / §9.2 pre-eval Pending gate (same as
                    // evaluate_let_bindings:3633-3659 but keyed on
                    // combined_traces instead of let_traces).
                    {
                        let trace_peek = combined_traces
                            .get(&node_id)
                            .expect("sorted_combined ⊆ combined_traces.keys() by construction");
                        let (gate_freshness, gate_cause) =
                            self.cache.derive_output_freshness_from_trace_with_cause(
                                trace_peek, false, version_id,
                            );
                        if matches!(gate_freshness, Freshness::Pending { .. })
                            && let Some(cause) = gate_cause
                            && self.cache.mark_pending_with_cause(&node_id, cause)
                        {
                            let _ = take_trace(
                                &mut combined_traces,
                                &node_id,
                                "sorted_combined",
                                "combined_traces",
                            );
                            self.journal.record(EvalEvent {
                                timestamp: Instant::now(),
                                node_id: node_id.clone(),
                                kind: EventKind::Completed {
                                    outcome: EvalOutcome::Unchanged,
                                },
                                version: VersionId(version_id),
                                payload: Some(EventPayload::Duration(start.elapsed())),
                            });
                            continue;
                        }
                    }

                    // @optimized UserFunctionCall dispatch (same as
                    // evaluate_let_bindings:3662-4054, combined_traces replaces
                    // let_traces in every take_trace call).
                    if let reify_ir::CompiledExprKind::UserFunctionCall {
                        function_name,
                        args,
                    } = &expr.kind
                    {
                        let maybe_target: Option<String> =
                            reify_expr::find_matching_compiled_function(
                                functions,
                                function_name,
                                args,
                            )
                            .and_then(|f| f.optimized_target.clone());

                        if let Some(target) = maybe_target {
                            {
                                let output_node_id = NodeId::Value(cell_id.clone());
                                let prior_had_compute_node =
                                    self.eval_state.as_ref().is_some_and(|prior| {
                                        prior.snapshot.graph.compute_nodes.iter().any(|(_, cn)| {
                                            cn.target == target
                                                && cn.output_value_cells.contains(&cell_id)
                                        })
                                    });
                                if prior_had_compute_node
                                    && self.cache.freshness(&output_node_id) == Freshness::Final
                                    && let Some(entry) = self.cache.get(&output_node_id)
                                    && let CachedResult::Value(cached_val, det) =
                                        entry.result.clone()
                                {
                                    values.insert(cell_id.clone(), cached_val.clone());
                                    snapshot.values.insert(cell_id.clone(), (cached_val, det));
                                    let _trace = take_trace(
                                        &mut combined_traces,
                                        &node_id,
                                        "sorted_combined",
                                        "combined_traces",
                                    );
                                    self.journal.record(EvalEvent {
                                        timestamp: Instant::now(),
                                        node_id,
                                        kind: EventKind::Completed {
                                            outcome: EvalOutcome::Unchanged,
                                        },
                                        version: VersionId(version_id),
                                        payload: Some(EventPayload::Duration(start.elapsed())),
                                    });
                                    continue;
                                }
                            }

                            if self.compute_dispatch(&target).is_some() {
                                let arg_values: Vec<Value> = {
                                    let eval_ctx = eval_ctx_with_meta(values, functions, meta_map)
                                        .with_determinacy(&snapshot.values)
                                        .with_runtime_diagnostics(runtime_sink);
                                    args.iter()
                                        .map(|a| reify_expr::eval_expr(a, &eval_ctx))
                                        .collect()
                                };

                                let shell_extract_feed: Option<reify_core::ValueCellId> =
                                    if target == "solver::elastic_static" {
                                        self.insert_shell_extract_upstream(
                                            snapshot,
                                            cell_id.entity.as_str(),
                                            &arg_values,
                                            version_id,
                                            diagnostics,
                                        )
                                    } else {
                                        None
                                    };

                                let next_index: u32 = snapshot
                                    .graph
                                    .compute_nodes
                                    .iter()
                                    .filter(|(id, _)| id.entity == cell_id.entity)
                                    .map(|(id, _)| id.index)
                                    .max()
                                    .map(|m| m + 1)
                                    .unwrap_or(0);

                                // task #4726: filter out geometry-typed ValueRefs
                                // (e.g. `body` from a geometry let like
                                // `let body = box(...)`).  Geometry lets create NO
                                // value cell in the compiler (entity.rs:1664-1665), so
                                // they are absent from `snapshot.graph.value_cells`.
                                // `compute_cache_key` asserts that every `value_input`
                                // is present in the graph — including a geometry-typed
                                // cell would panic there.  Geometry inputs flow through
                                // `realization_inputs` instead (via
                                // `build_compute_realization_inputs`).
                                let mut value_inputs: Vec<reify_core::ValueCellId> = args
                                    .iter()
                                    .filter_map(|arg| match &arg.kind {
                                        reify_ir::CompiledExprKind::ValueRef(target_cell) => {
                                            // Exclude geometry-let refs: not in the graph.
                                            if snapshot.graph.value_cells.contains_key(target_cell) {
                                                Some(target_cell.clone())
                                            } else {
                                                None
                                            }
                                        }
                                        _ => None,
                                    })
                                    .collect();

                                if let Some(feed) = shell_extract_feed {
                                    value_inputs.push(feed);
                                }

                                let c_id = reify_core::ComputeNodeId::new(
                                    cell_id.entity.as_str(),
                                    next_index,
                                );

                                if let Some(prev) = snapshot.graph.get_compute_node_mut(&c_id)
                                    && let Some(old) = prev.running.take()
                                {
                                    old.cancel();
                                }
                                let cancel = crate::graph::CancellationHandle::new();

                                let (realization_inputs, realization_read_handles, proj_diags) =
                                    self.build_compute_realization_inputs(
                                        &arg_values,
                                        &snapshot.graph,
                                    );
                                diagnostics.extend(proj_diags);

                                // task #3428 step-2: populate cache_key via
                                // compute_cache_key before insertion. All
                                // value_inputs/realization_inputs are already in
                                // snapshot.graph at this point (topological order).
                                let mut node = crate::graph::ComputeNodeData {
                                    computation_id: c_id.clone(),
                                    target: target.clone(),
                                    value_inputs,
                                    realization_inputs,
                                    options_hash: reify_core::ContentHash(0),
                                    cache_key: reify_core::ContentHash(0),
                                    cached_result: None,
                                    result_content_hash: None,
                                    opaque_state: None,
                                    running: Some(cancel.clone()),
                                    output_value_cells: vec![cell_id.clone()],
                                };
                                // task #3428: fold the evaluated arg_values into the
                                // persistent key so loads/supports/options (dropped by
                                // the shallow value_inputs walk) can't cause a false
                                // cache hit. See Self::persistent_cache_key.
                                let ck = Self::persistent_cache_key(
                                    &node,
                                    &snapshot.graph,
                                    &arg_values,
                                );
                                node.cache_key = ck;
                                snapshot.graph.insert_compute_node(node);

                                match self.run_compute_dispatch(
                                    &c_id,
                                    std::slice::from_ref(&cell_id),
                                    &target,
                                    &arg_values,
                                    &realization_read_handles,
                                    &Value::Undef,
                                    &cancel,
                                    VersionId(version_id),
                                    ck, // task #3428 step-6: persistent-cache input key
                                ) {
                                    Ok((result, diags)) => {
                                        diagnostics.extend(diags);
                                        values.insert(cell_id.clone(), result.clone());
                                        snapshot.values.insert(
                                            cell_id.clone(),
                                            (result.clone(), DeterminacyState::Determined),
                                        );
                                        let _trace = take_trace(
                                            &mut combined_traces,
                                            &node_id,
                                            "sorted_combined",
                                            "combined_traces",
                                        );
                                        if let Some(n) = snapshot.graph.get_compute_node_mut(&c_id)
                                        {
                                            n.running = None;
                                        }
                                        self.journal.record(EvalEvent {
                                            timestamp: Instant::now(),
                                            node_id,
                                            kind: EventKind::Completed {
                                                outcome: EvalOutcome::Changed,
                                            },
                                            version: VersionId(version_id),
                                            payload: Some(EventPayload::Duration(start.elapsed())),
                                        });
                                        continue;
                                    }
                                    Err(crate::engine_compute::DispatchError::Cancelled) => {
                                        if let Some(n) = snapshot.graph.get_compute_node_mut(&c_id)
                                        {
                                            n.running = None;
                                        }
                                        let _trace = take_trace(
                                            &mut combined_traces,
                                            &node_id,
                                            "sorted_combined",
                                            "combined_traces",
                                        );
                                        self.journal.record(EvalEvent {
                                            timestamp: Instant::now(),
                                            node_id,
                                            kind: EventKind::Completed {
                                                outcome: EvalOutcome::Unchanged,
                                            },
                                            version: VersionId(version_id),
                                            payload: Some(EventPayload::Duration(start.elapsed())),
                                        });
                                        continue;
                                    }
                                    Err(crate::engine_compute::DispatchError::Failed(diags)) => {
                                        if let Some(n) = snapshot.graph.get_compute_node_mut(&c_id)
                                        {
                                            n.running = None;
                                        }
                                        diagnostics.extend(diags);
                                        let error = ErrorRef::new(format!(
                                            "@optimized target {:?}: compute trampoline \
                                             returned Failed",
                                            target
                                        ));
                                        let trace = take_trace(
                                            &mut combined_traces,
                                            &node_id,
                                            "sorted_combined",
                                            "combined_traces",
                                        );
                                        self.cache.record_evaluation_propagating_freshness(
                                            node_id.clone(),
                                            CachedResult::Value(
                                                Value::Undef,
                                                DeterminacyState::Determined,
                                            ),
                                            VersionId(version_id),
                                            trace,
                                            false,
                                        );
                                        let _ = self.cache.mark_failed(&node_id, error.clone());
                                        self.journal.record(EvalEvent {
                                            timestamp: Instant::now(),
                                            node_id: node_id.clone(),
                                            kind: EventKind::Failed { error },
                                            version: VersionId(version_id),
                                            payload: Some(EventPayload::Duration(start.elapsed())),
                                        });
                                        continue;
                                    }
                                }
                            } else {
                                // Unregistered @optimized target: emit Error, fall through
                                // to body-inlining.
                                diagnostics.push(Diagnostic::error(format!(
                                    "@optimized target {:?}: no registered compute trampoline \
                                     (falling back to body-inlining)",
                                    target
                                )));
                            }
                        }
                    }

                    // Normal eval: panic boundary (arch §9.1 / evaluate_let_bindings:4056).
                    let eval_ctx = eval_ctx_with_meta(values, functions, meta_map)
                        .with_determinacy(&snapshot.values)
                        .with_runtime_diagnostics(runtime_sink);
                    let panic_result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                        #[cfg(any(test, feature = "test-instrumentation"))]
                        if force_panic {
                            panic!("test-instrumentation forced panic for {:?}", cell_id);
                        }
                        reify_expr::eval_expr(expr, &eval_ctx)
                    }));

                    let val = match panic_result {
                        Ok(v) => v,
                        Err(payload) => {
                            let msg = payload
                                .downcast_ref::<String>()
                                .map(|s| s.as_str())
                                .or_else(|| payload.downcast_ref::<&str>().copied())
                                .unwrap_or("<non-string panic>")
                                .to_string();
                            let error = ErrorRef::new(msg);
                            let trace = take_trace(
                                &mut combined_traces,
                                &node_id,
                                "sorted_combined",
                                "combined_traces",
                            );
                            self.cache.record_evaluation_propagating_freshness(
                                node_id.clone(),
                                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                                VersionId(version_id),
                                trace,
                                false,
                            );
                            let _ = self.cache.mark_failed(&node_id, error.clone());
                            self.journal.record(EvalEvent {
                                timestamp: Instant::now(),
                                node_id: node_id.clone(),
                                kind: EventKind::Failed { error },
                                version: VersionId(version_id),
                                payload: Some(EventPayload::Duration(start.elapsed())),
                            });
                            continue;
                        }
                    };
                    values.insert(cell_id.clone(), val.clone());
                    snapshot
                        .values
                        .insert(cell_id.clone(), (val.clone(), DeterminacyState::Determined));

                    let trace = take_trace(
                        &mut combined_traces,
                        &node_id,
                        "sorted_combined",
                        "combined_traces",
                    );
                    let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
                    let outcome = self.cache.record_evaluation_propagating_freshness(
                        node_id.clone(),
                        cached_result,
                        VersionId(version_id),
                        trace,
                        false,
                    );
                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version: VersionId(version_id),
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });
                }

                _ => {} // Auto cells pre-seeded above; no other kinds expected.
            }
        }
    }

    /// Evaluate let bindings from a template in topological order.
    ///
    /// Collects let cells with expressions, builds dependency traces,
    /// topologically sorts, and evaluates each in order — recording
    /// journal events and cache entries. Used by both the initial eval()
    /// pass and the post-resolution re-evaluation pass.
    #[allow(clippy::too_many_arguments)]
    fn evaluate_let_bindings(
        &mut self,
        template: &reify_compiler::TopologyTemplate,
        values: &mut ValueMap,
        snapshot: &mut Snapshot,
        version_id: u64,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        diagnostics: &mut Vec<Diagnostic>,
        runtime_sink: &RefCell<Vec<Diagnostic>>,
    ) {
        let (let_cells, mut let_traces, sorted_lets) = detect_let_cycle(template, diagnostics);

        for node_id in sorted_lets {
            let expr = let_cells[&node_id];
            // let_cells is keyed exclusively by NodeId::Value; topological_sort returns
            // only keys from that set — so this assertion holds in all correct code paths.
            // In debug/test builds it fires loud; in release the diagnostic+continue handles
            // any accidental invariant violation gracefully.
            debug_assert!(
                matches!(node_id, NodeId::Value(_)),
                "evaluate_let_bindings: sorted_lets produced a non-Value NodeId: {:?}; construction invariant violated",
                node_id,
            );
            let cell_id = match &node_id {
                NodeId::Value(vcid) => vcid,
                _ => {
                    diagnostics.push(Diagnostic::error(format!(
                        "let-binding evaluation: expected NodeId::Value, got {:?}; skipping",
                        node_id,
                    )));
                    continue;
                }
            };

            let start = Instant::now();
            self.journal.record(EvalEvent {
                timestamp: start,
                node_id: node_id.clone(),
                kind: EventKind::Started,
                version: VersionId(version_id),
                payload: None,
            });

            // Snapshot test-instrumentation panic-injection state for this cell
            // so the closure does not need to borrow `self`. The field and
            // this read site are both `#[cfg(any(test, feature =
            // "test-instrumentation"))]`-gated — absent in production builds.
            #[cfg(any(test, feature = "test-instrumentation"))]
            let force_panic = self.panic_on_eval_cells.contains(cell_id);

            // Arch §7.2 line 748 / §9.2 line 890 — pre-eval Pending gate.
            //
            // Before invoking eval_expr, peek at this node's input freshness via
            // the freshly-built dependency trace.  If any input is Failed (§9.2
            // carve-out: Failed input → Pending output) or Pending (§7.2 line 748:
            // Pending forwards the chain), the value computation MUST be quieted
            // — running eval_expr with a missing/Undef upstream value would
            // overwrite the cached `last_substantive` with poisoned output and
            // defeat the "preserve previous best" semantic.
            //
            // Implementation:
            //   1. Peek at the trace for this node (without removing it from
            //      `let_traces`).
            //   2. Derive `(Freshness, Option<NodeId>)` via the cause-bearing
            //      helper: Failed input contributes its own NodeId as cause,
            //      Pending input forwards the upstream entry's `pending_cause`.
            //   3. If the derivation is `Pending` AND the cache entry already
            //      exists (steady-state re-eval), call `mark_pending_with_cause`
            //      — this preserves the existing `result_hash` as
            //      `last_substantive` and bumps `pending_transition_count`.
            //   4. Drain the trace from `let_traces` (keeping the
            //      sorted_lets ⊆ let_traces.keys() invariant honoured for the
            //      remainder of the loop) and emit `Completed { Unchanged }`
            //      so the journal still records the visit.
            //   5. `continue` — skip the panic-bounded eval and the normal
            //      record_evaluation_propagating_freshness write.
            //
            // Cold-start fallback (entry absent → `mark_pending_with_cause`
            // returns false): fall through to normal eval.  eval_expr will see
            // missing reads via the value map and produce `Value::Undef` per
            // the existing Kleene-undef semantics; the §9.2 derivation in
            // `record_evaluation_propagating_freshness` still yields
            // `Pending { last_substantive: ResultRef::none() }` (just without
            // the chain cause attached, since there is no prior entry to
            // store it on).  No test currently exercises this corner case.
            //
            // The gate intentionally does NOT short-circuit when `force_panic`
            // is set — letting the gate win over test instrumentation matches
            // the spec semantics ("Pending naturally quiets the downstream
            // subtree") and keeps the test-only hook from overriding the
            // production propagation contract.
            {
                let trace_peek = let_traces
                    .get(&node_id)
                    .expect("sorted_lets ⊆ let_traces.keys() by detect_let_cycle invariant");
                let (gate_freshness, gate_cause) = self
                    .cache
                    .derive_output_freshness_from_trace_with_cause(trace_peek, false, version_id);
                if matches!(gate_freshness, Freshness::Pending { .. })
                    && let Some(cause) = gate_cause
                    && self.cache.mark_pending_with_cause(&node_id, cause)
                {
                    // Drain the trace so the rest of the loop body is
                    // never reached for this iteration; the existing
                    // entry's `dependency_trace` is preserved (stable
                    // structure invariant during incremental re-eval).
                    let _ = take_trace(&mut let_traces, &node_id, "sorted_lets", "let_traces");
                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id: node_id.clone(),
                        kind: EventKind::Completed {
                            outcome: EvalOutcome::Unchanged,
                        },
                        version: VersionId(version_id),
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });
                    continue;
                }
            }

            // @optimized UserFunctionCall → ComputeNode lowering (task γ / 3422, PRD §8).
            // When a let-cell's expr is a UserFunctionCall whose CompiledFunction carries
            // `optimized_target == Some(t)` AND a compute trampoline is registered for `t`,
            // we insert a ComputeNode into the graph and invoke the trampoline synchronously
            // instead of body-inlining. Runs after the Pending gate; unregistered targets
            // fall through to the eval_expr path below (step-8 adds the fallback diagnostic).
            if let reify_ir::CompiledExprKind::UserFunctionCall {
                function_name,
                args,
            } = &expr.kind
            {
                let maybe_target: Option<String> =
                    reify_expr::find_matching_compiled_function(functions, function_name, args)
                        .and_then(|f| f.optimized_target.clone());

                if let Some(target) = maybe_target {
                    // §8-η / §3 Final-gate: if the output VC is already
                    // `Freshness::Final` in the cache from a prior eval() that
                    // dispatched via the @optimized path, the trampoline result
                    // is unchanged — skip re-dispatch and return the cached
                    // value directly.
                    //
                    // Guard: we only fire this gate when the prior snapshot
                    // (`self.eval_state`) had a ComputeNode for the same target
                    // pointing to this output cell.  Without this guard, a
                    // body-inline cache entry written by an intervening
                    // `edit_source` that replaced the @optimized call with a
                    // plain expression would suppress ComputeNode creation on
                    // the next eval() call — the test
                    // `remove_and_reinsert_via_edit_source_preserves_counter`
                    // (opaque_state_lifecycle.rs) pins this invariant.
                    //
                    // Uses `NodeId::Value(cell_id.clone())` — the same key that
                    // `complete_compute_dispatch_atomically` writes under on the
                    // first dispatch (matching the post-dispatch store site).
                    {
                        let output_node_id = NodeId::Value(cell_id.clone());
                        let prior_had_compute_node =
                            self.eval_state.as_ref().is_some_and(|prior| {
                                prior.snapshot.graph.compute_nodes.iter().any(|(_, cn)| {
                                    cn.target == target && cn.output_value_cells.contains(cell_id)
                                })
                            });
                        if prior_had_compute_node
                            && self.cache.freshness(&output_node_id) == Freshness::Final
                            && let Some(entry) = self.cache.get(&output_node_id)
                            && let CachedResult::Value(cached_val, det) = entry.result.clone()
                        {
                            values.insert(cell_id.clone(), cached_val.clone());
                            snapshot.values.insert(cell_id.clone(), (cached_val, det));
                            let _trace =
                                take_trace(&mut let_traces, &node_id, "sorted_lets", "let_traces");
                            self.journal.record(EvalEvent {
                                timestamp: Instant::now(),
                                node_id,
                                kind: EventKind::Completed {
                                    outcome: EvalOutcome::Unchanged,
                                },
                                version: VersionId(version_id),
                                payload: Some(EventPayload::Duration(start.elapsed())),
                            });
                            continue;
                        }
                    }

                    if self.compute_dispatch(&target).is_some() {
                        // Evaluate args in a scoped block so the eval_ctx borrow on
                        // `snapshot.values` ends before we mutably access `snapshot`.
                        let arg_values: Vec<Value> = {
                            let eval_ctx = eval_ctx_with_meta(values, functions, meta_map)
                                .with_determinacy(&snapshot.values)
                                .with_runtime_diagnostics(runtime_sink);
                            args.iter()
                                .map(|a| reify_expr::eval_expr(a, &eval_ctx))
                                .collect()
                        };

                        // task 3594/δ step-12: shell-route upstream wiring. When
                        // this is the `solver::elastic_static` FEA target AND the
                        // evaluated options arg + dims classify as a SHELL body,
                        // insert + dispatch an upstream `shell-extract::extract`
                        // ComputeNode first, then splice its synthetic output
                        // cell into the FEA node's `value_inputs` below (the
                        // upstream→downstream segmentation edge). `None` on the
                        // Tet route / malformed args / soft-fallback failure —
                        // every non-shell target stays byte-identical to before.
                        // Inserting the upstream node here (before the index
                        // computation below) means the FEA node's `max(index)+1`
                        // allocation naturally accounts for it (no collision).
                        let shell_extract_feed: Option<reify_core::ValueCellId> =
                            if target == "solver::elastic_static" {
                                self.insert_shell_extract_upstream(
                                    snapshot,
                                    cell_id.entity.as_str(),
                                    &arg_values,
                                    version_id,
                                    diagnostics,
                                )
                            } else {
                                None
                            };

                        // Derive a unique per-entity ComputeNodeId index by
                        // taking `max(existing.index) + 1` over already-inserted
                        // ComputeNodes in the same entity. `insert_compute_node`
                        // does NOT dedupe (graph.rs:565-571 "Duplicate targets"
                        // doc), so this caller-side counter discharges the
                        // unique-ID contract. Without it, two `@optimized` calls
                        // in the same entity would collide on
                        // `PersistentMap<ComputeNodeId, _>`, silently overwriting
                        // the first node.
                        //
                        // `max(index) + 1` is preferred over `count()` because
                        // the latter assumes densely-allocated indices [0..N).
                        // That happens to be true today (this is the only
                        // insertion site), but any future code path that removes
                        // a ComputeNode or reserves an index out-of-order would
                        // let `count()` collide with a still-present id.
                        // `max + 1` discharges the unique-ID contract without
                        // relying on an insertion-only / no-gaps invariant.
                        let next_index: u32 = snapshot
                            .graph
                            .compute_nodes
                            .iter()
                            .filter(|(id, _)| id.entity == cell_id.entity)
                            .map(|(id, _)| id.index)
                            .max()
                            .map(|m| m + 1)
                            .unwrap_or(0);

                        // Extract `value_inputs` via a shallow walk over the
                        // call args: each direct `ValueRef(cell)` arg
                        // contributes the referenced cell. Literals, BinOps,
                        // and other complex sub-expressions contribute no
                        // entries in the γ slice — transitive-dependency
                        // walking is deferred to P3.2 (the cache-key
                        // composition slice that consumes `value_inputs`).
                        // Crucially, the OUTPUT cell (`cell_id`) is NOT in
                        // this list — that would be a graph self-loop.
                        // Contract pinned by:
                        //   tests/compute_dispatch_registry.rs::e2e_optimized_non_valueref_arg_yields_empty_value_inputs
                        // task #4726: mirror of the primary dispatch site — exclude
                        // geometry-let ValueRefs from value_inputs (no value cell,
                        // not in the graph; see the primary site comment for details).
                        let mut value_inputs: Vec<reify_core::ValueCellId> = args
                            .iter()
                            .filter_map(|arg| match &arg.kind {
                                reify_ir::CompiledExprKind::ValueRef(target_cell) => {
                                    if snapshot.graph.value_cells.contains_key(target_cell) {
                                        Some(target_cell.clone())
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            })
                            .collect();

                        // task 3594/δ step-12: on the shell route, the upstream
                        // `shell-extract::extract` node's synthetic output cell
                        // joins this FEA node's `value_inputs` — the graph edge
                        // the e2e (`shell_solve_e2e.rs`) asserts on. The FEA
                        // trampoline reads only value_inputs[0..=6]; this extra
                        // input is a pure dependency edge it ignores.
                        if let Some(feed) = shell_extract_feed {
                            value_inputs.push(feed);
                        }

                        // task δ / 3423 (PRD §3 atomic completion, §8 task δ):
                        // insert the ComputeNode FIRST — BEFORE dispatch — so
                        // that mid-flight an observer reading
                        // `cache.pending_cause(output_vc) ==
                        // Some(NodeId::Compute(c_id))` can resolve `c_id` to a
                        // live graph node (the "computing badge" rendering
                        // policy, PRD §3). `cached_result: None` because the
                        // CacheStore is now the canonical at-rest store for the
                        // result (PRD §5); the prior γ wiring set it
                        // post-dispatch.
                        let c_id =
                            reify_core::ComputeNodeId::new(cell_id.entity.as_str(), next_index);

                        // ε / §5 step-3: create the cancellation handle.
                        //
                        // The block below defensively cancels any prior handle on
                        // the same c_id, but is UNREACHABLE today: `c_id` is
                        // allocated as `max(index) + 1` from the current snapshot,
                        // so it is always a fresh identifier and the lookup will
                        // always return `None`.  Two freshness gates above guard
                        // this code: the Pending gate (lines 2771-2789) fires when
                        // inputs are Pending, and the Final-gate (lines 2808-2861)
                        // fires when the output VC is already Final from a prior
                        // eval().  Together they short-circuit any re-eval of a
                        // node whose inputs or output make re-dispatch unnecessary.
                        //
                        // The guard is kept for the future async-driver slice where
                        // a same-`ComputeNodeId` re-dispatch might carry a live
                        // `running` handle (PRD §2 one-in-flight invariant / design
                        // decision in task ε/3424).  Until then the branch is dead
                        // and exists only to make the invariant explicit in code.
                        if let Some(prev) = snapshot.graph.get_compute_node_mut(&c_id)
                            && let Some(old) = prev.running.take()
                        {
                            old.cancel();
                        }
                        let cancel = crate::graph::CancellationHandle::new();

                        let (realization_inputs, realization_read_handles, proj_diags) =
                            self.build_compute_realization_inputs(&arg_values, &snapshot.graph);
                        diagnostics.extend(proj_diags);

                        // task #3428 step-2: populate cache_key via
                        // compute_cache_key before insertion. All
                        // value_inputs/realization_inputs are already in
                        // snapshot.graph at this point (topological order).
                        let mut node = crate::graph::ComputeNodeData {
                            computation_id: c_id.clone(),
                            target: target.clone(),
                            value_inputs,
                            realization_inputs,
                            options_hash: reify_core::ContentHash(0),
                            cache_key: reify_core::ContentHash(0),
                            cached_result: None,
                            result_content_hash: None,
                            opaque_state: None,
                            // ε: the same Arc<AtomicBool> is both stored here
                            // (so a future async driver can cancel via `running`)
                            // and passed to run_compute_dispatch below (so the
                            // trampoline's cooperative poll sees the signal).
                            running: Some(cancel.clone()),
                            output_value_cells: vec![cell_id.clone()],
                        };
                        // task #3428: fold the evaluated arg_values into the
                        // persistent key so loads/supports/options (dropped by the
                        // shallow value_inputs walk) can't cause a false cache hit.
                        // See Self::persistent_cache_key.
                        let ck = Self::persistent_cache_key(&node, &snapshot.graph, &arg_values);
                        node.cache_key = ck;
                        snapshot.graph.insert_compute_node(node);

                        // ε / §8-ε: the begin → invoke trampoline →
                        // atomic-complete-or-leave-Pending lifecycle is owned by
                        // `Engine::run_compute_dispatch` (PRD §3 / §8 task δ).
                        // The SAME `cancel` handle is both stored in `running`
                        // (so a future async driver can fire it) and passed here
                        // (so the trampoline's cooperative poll sees the signal).
                        // Cancelled leaves the VC Pending; Failed owns mark_failed.
                        // PRD §2: "cancelled dispatch leaves prior best on display,
                        // prior cache untouched, Pending until next dispatch."
                        match self.run_compute_dispatch(
                            &c_id,
                            std::slice::from_ref(cell_id),
                            &target,
                            &arg_values,
                            &realization_read_handles,
                            &Value::Undef,
                            &cancel,
                            VersionId(version_id),
                            ck, // task #3428 step-6: persistent-cache input key
                        ) {
                            Ok((result, diags)) => {
                                diagnostics.extend(diags);

                                values.insert(cell_id.clone(), result.clone());
                                snapshot.values.insert(
                                    cell_id.clone(),
                                    (result.clone(), DeterminacyState::Determined),
                                );
                                // Preserve `let_traces` consumption so the
                                // per-node trace map stays drained consistently
                                // with every other let-cell visit. The trace is
                                // no longer threaded into a cache write here —
                                // `run_compute_dispatch` →
                                // `complete_compute_dispatch_atomically` is the
                                // canonical writer (it writes with an empty
                                // DependencyTrace; the ComputeNode's
                                // `value_inputs` drive the cache key in P3.2,
                                // not the output VC's trace).
                                //
                                // **Intermediate freshness propagation is
                                // intentionally dropped for @optimized cells in
                                // δ scope.** The pre-`run_compute_dispatch`
                                // wiring threaded `take_trace(...)` into
                                // `record_evaluation_propagating_freshness`, so
                                // the output cell would inherit derived
                                // freshness (e.g. Intermediate when any input
                                // was Intermediate, §7.2). The δ contract
                                // (PRD §3) flips the output Pending→Final on
                                // successful completion — period. Restoring
                                // derived-Intermediate propagation when inputs
                                // are partial is a separate concern and is
                                // deferred to a future slice (the upstream
                                // Pending gate already short-circuits
                                // Failed/Pending inputs before reaching here).
                                let _trace = take_trace(
                                    &mut let_traces,
                                    &node_id,
                                    "sorted_lets",
                                    "let_traces",
                                );
                                // `run_compute_dispatch` bundles
                                // write+flip+clear and does not surface an
                                // EvalOutcome (the atomicity-as-API contract);
                                // a completed dispatch (re)computed a value, so
                                // the journal records `Changed`.
                                //
                                // ε §5 step-3: clear the running slot on terminal
                                // outcome (PRD §2 / design decision task ε/3424).
                                if let Some(n) = snapshot.graph.get_compute_node_mut(&c_id) {
                                    n.running = None;
                                }
                                self.journal.record(EvalEvent {
                                    timestamp: Instant::now(),
                                    node_id,
                                    kind: EventKind::Completed {
                                        outcome: EvalOutcome::Changed,
                                    },
                                    version: VersionId(version_id),
                                    payload: Some(EventPayload::Duration(start.elapsed())),
                                });
                                continue;
                            }
                            Err(crate::engine_compute::DispatchError::Cancelled) => {
                                // ε / PRD §2 / §7.1: CANCELLED — the output VC is
                                // already `Pending{last_substantive: prior}` from
                                // `begin_compute_dispatch`; that IS the correct
                                // cancelled postcondition.  Do NOT insert a value
                                // and do NOT call `mark_failed` — the prior best
                                // stays on display and the prior cache is untouched
                                // until the next dispatch completes.
                                //
                                // ε §5 step-3: clear the running slot.
                                if let Some(n) = snapshot.graph.get_compute_node_mut(&c_id) {
                                    n.running = None;
                                }
                                // Drain let_traces consistently with the Ok arm
                                // (mirrors the `take_trace` call above so the
                                // per-node trace map stays drained on every path).
                                let _trace = take_trace(
                                    &mut let_traces,
                                    &node_id,
                                    "sorted_lets",
                                    "let_traces",
                                );
                                // Journal a non-Changed event: the dispatch was
                                // attempted but did not produce a new value.
                                self.journal.record(EvalEvent {
                                    timestamp: Instant::now(),
                                    node_id,
                                    kind: EventKind::Completed {
                                        outcome: EvalOutcome::Unchanged,
                                    },
                                    version: VersionId(version_id),
                                    payload: Some(EventPayload::Duration(start.elapsed())),
                                });
                                continue;
                            }
                            Err(crate::engine_compute::DispatchError::Failed(diags)) => {
                                // Registered trampoline returned Failed — do NOT
                                // body-inline. The user explicitly registered a
                                // trampoline for this target, so a failure there is
                                // a genuine compute error (review feedback #1,
                                // suggestion 1).
                                //
                                // The ComputeNode is INTENTIONALLY left in the graph
                                // so `pending_cause` resolution can chase the chain
                                // root to a live node (PRD §3 / §5 rationale below).
                                //
                                // ε §5 step-3: clear the running slot on terminal
                                // outcome.
                                if let Some(n) = snapshot.graph.get_compute_node_mut(&c_id) {
                                    n.running = None;
                                }
                                diagnostics.extend(diags);
                                let error = ErrorRef::new(format!(
                                    "@optimized target {:?}: compute trampoline \
                                     returned Failed",
                                    target
                                ));
                                let trace = take_trace(
                                    &mut let_traces,
                                    &node_id,
                                    "sorted_lets",
                                    "let_traces",
                                );
                                self.cache.record_evaluation_propagating_freshness(
                                    node_id.clone(),
                                    CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                                    VersionId(version_id),
                                    trace,
                                    false,
                                );
                                let _ = self.cache.mark_failed(&node_id, error.clone());
                                self.journal.record(EvalEvent {
                                    timestamp: Instant::now(),
                                    node_id: node_id.clone(),
                                    kind: EventKind::Failed { error },
                                    version: VersionId(version_id),
                                    payload: Some(EventPayload::Duration(start.elapsed())),
                                });
                                continue;
                            }
                        }
                    } else {
                        // Unregistered target (PRD §9 Q1, task γ): emit Error
                        // diagnostic, then fall through to body-inlining.
                        // Release-hard-error is deferred to slice η.
                        diagnostics.push(Diagnostic::error(format!(
                            "@optimized target {:?}: no registered compute trampoline \
                             (falling back to body-inlining)",
                            target
                        )));
                    }
                }
            }

            // Arch §9.1 panic boundary (lines 868–877): wrap `reify_expr::eval_expr`
            // in `catch_unwind` so a panic inside expression evaluation becomes
            // `Freshness::Failed { error }` on the cell plus a single
            // `EventKind::Failed` event — rather than crashing the engine.
            //
            // The test-instrumentation hook (`panic_on_eval_cells` /
            // `set_panic_on_eval`, `force_panic` above) panics with a known
            // sentinel BEFORE calling `eval_expr`, so the same boundary serves
            // both the production path (a panic raised inside `eval_expr`
            // itself) and the test path. The `force_panic` variable and the
            // `if force_panic { panic!(…) }` branch are both
            // `#[cfg(any(test, feature = "test-instrumentation"))]`-gated and
            // are absent in production builds.
            let eval_ctx = eval_ctx_with_meta(values, functions, meta_map)
                .with_determinacy(&snapshot.values)
                .with_runtime_diagnostics(runtime_sink);
            let panic_result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                #[cfg(any(test, feature = "test-instrumentation"))]
                if force_panic {
                    panic!("test-instrumentation forced panic for {:?}", cell_id);
                }
                reify_expr::eval_expr(expr, &eval_ctx)
            }));

            let val = match panic_result {
                Ok(v) => v,
                Err(payload) => {
                    // Downcast the panic payload to a string message — same
                    // pattern as `invariant_tests::panics_on_unrepresentable_cell_types`
                    // (engine_eval.rs:2098-2107).
                    let msg = payload
                        .downcast_ref::<String>()
                        .map(|s| s.as_str())
                        .or_else(|| payload.downcast_ref::<&str>().copied())
                        .unwrap_or("<non-string panic>")
                        .to_string();
                    let error = ErrorRef::new(msg);
                    // Ensure the cache entry exists so `mark_failed` can flip
                    // its freshness in place. We write a stub Undef result so
                    // the entry exists; mark_failed then overrides freshness
                    // to Failed { error }.
                    let trace = take_trace(&mut let_traces, &node_id, "sorted_lets", "let_traces");
                    self.cache.record_evaluation_propagating_freshness(
                        node_id.clone(),
                        CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                        VersionId(version_id),
                        trace,
                        false,
                    );
                    let _ = self.cache.mark_failed(&node_id, error.clone());
                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id: node_id.clone(),
                        kind: EventKind::Failed { error },
                        version: VersionId(version_id),
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });
                    continue;
                }
            };
            values.insert(cell_id.clone(), val.clone());

            snapshot
                .values
                .insert(cell_id.clone(), (val.clone(), DeterminacyState::Determined));

            // sorted_lets and let_traces are built from the same key set, so remove() cannot fail.
            let trace = take_trace(&mut let_traces, &node_id, "sorted_lets", "let_traces");
            let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
            // Arch §7.2 propagation rule (docs/reify-implementation-architecture.md lines 730-749):
            // output freshness = derive(still_refining=false, input_freshnesses, generation=version_id).
            // Uses the freshly-computed `trace` (not the old cached trace) so derivation is
            // always keyed off the current reads.  `still_refining=false` is the only valid
            // value today — no progressive nodes exist yet (that is PRD task 4+ scope).
            // `generation` is derived from `VersionId(version_id).0` inside the method per §7.1
            // (single source of truth — no need to pass both `VersionId` and bare `u64`).
            let outcome = self.cache.record_evaluation_propagating_freshness(
                node_id.clone(),
                cached_result,
                VersionId(version_id),
                trace,
                false,
            );

            self.journal.record(EvalEvent {
                timestamp: Instant::now(),
                node_id,
                kind: EventKind::Completed { outcome },
                version: VersionId(version_id),
                payload: Some(EventPayload::Duration(start.elapsed())),
            });
        }
    }

    /// GHR-δ §5 lazy revalidation read entry point: read a value cell's stored
    /// `Value`, revalidating any `Value::GeometryHandle` against the Engine's
    /// current `realization_ref → handle` map ([`Engine::realization_handles`])
    /// before returning it.
    ///
    /// - STALE handle (the cell's `kernel_handle` no longer matches the Engine's
    ///   current handle for its `realization_ref`): the re-resolved value is
    ///   written back into `snapshot.values` so the next read of the same cell
    ///   hits the fast path, and the slow-path counter is bumped.
    /// - ABSENT realization (no longer in the map): returns [`Value::Undef`] and
    ///   bumps the counter.
    /// - FAST path (handle already matches, or the value is not a geometry
    ///   handle, or the cell is absent): returns the value verbatim (Undef for
    ///   an absent cell) with no write-back and no counter bump.
    ///
    /// Takes `&self` (not `&mut self`): the validity map is read-only here and
    /// the counter is an `AtomicUsize`, so the only mutation is to the
    /// caller-owned `snapshot`. A caller can therefore clone the Engine's
    /// snapshot (`engine.snapshot().clone()`) and revalidate against it without
    /// a borrow conflict with `&self`. Per PRD §9 Q4 the per-read HashMap lookup
    /// is acceptable for v0.3.
    ///
    /// **Consumers / production wiring (deferred).** As of GHR-δ this entry
    /// point (and its sibling counter [`Engine::geometry_revalidation_slow_path_count`])
    /// has NO production caller — only the in-crate integration suite
    /// (`tests/geometry_handle_freshness.rs`) exercises it. Lazy revalidation is
    /// therefore NOT yet active on any real read path: GUI value reads and other
    /// consumers still read `snapshot.values` directly and bypass it. Routing the
    /// real read boundary (e.g. the GUI engine's value-read path) through this
    /// method is intentionally left to a follow-up task; until then a stale
    /// handle is only re-resolved when a caller opts in. Do not assume
    /// revalidation is live on every read just because this method exists.
    pub fn read_value_revalidated(&self, snapshot: &mut Snapshot, cell: &ValueCellId) -> Value {
        // Clone the (value, determinacy) pair out so the immutable borrow on
        // `snapshot.values` ends before the possible write-back below. A missing
        // cell reads as Undef (no entry to revalidate).
        let Some((value, det)) = snapshot.values.get(cell).cloned() else {
            return Value::Undef;
        };
        match revalidate_geometry_handle(&value, &self.realization_handles) {
            RevalidationOutcome::Fresh => value,
            RevalidationOutcome::Resolved(resolved) => {
                self.geometry_revalidation_slow_path
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Preserve the cell's DeterminacyState; only the handle changes.
                snapshot
                    .values
                    .insert(cell.clone(), (resolved.clone(), det));
                resolved
            }
            RevalidationOutcome::Undef => {
                self.geometry_revalidation_slow_path
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Value::Undef
            }
        }
    }
}

#[cfg(all(test, debug_assertions))]
mod invariant_tests {
    use reify_compiler::ValueCellKind;
    use reify_core::{ContentHash, Type, ValueCellId};

    use crate::graph::{EvaluationGraph, ValueCellNode};

    fn bad_node(cell_type: Type) -> (ValueCellId, ValueCellNode) {
        let id = ValueCellId::new("E", "t");
        let node = ValueCellNode {
            id: id.clone(),
            kind: ValueCellKind::Param,
            cell_type,
            default_expr: None,
            content_hash: ContentHash(0),
        };
        (id, node)
    }

    /// `Type::Geometry` is representable as of GHR-β (task 3604): a
    /// `Value::GeometryHandle` exists and `is_representable_cell_type` must
    /// admit it.
    #[test]
    fn is_representable_cell_type_admits_geometry() {
        assert!(
            super::is_representable_cell_type(&Type::Geometry),
            "Type::Geometry must be representable post GHR-β (Value::GeometryHandle exists)"
        );
    }

    /// `Type::Relation` is representable as a value cell_type (geometric-relations
    /// γ, task 4383): it is an Undef-backed compile-time directive type with no
    /// `Value::Relation`, admitted alongside StructureRef/TraitObject. Relation
    /// calls type-check to Type::Relation but evaluate to Value::Undef until ζ
    /// supplies the relate-solve. RED until step-2 adds the arm.
    #[test]
    fn is_representable_cell_type_admits_relation() {
        assert!(
            super::is_representable_cell_type(&Type::Relation),
            "Type::Relation must be representable (Undef-backed directive type, γ)"
        );
    }

    /// Verify that `assert_value_cell_types_representable` panics with the
    /// expected message for every unrepresentable `Type` variant.  Uses
    /// `catch_unwind` to check all variants in a single test run, avoiding
    /// nearly-identical `#[should_panic]` tests that would pass even if the
    /// variant list diverged from the function under test.
    ///
    /// `Type::StructureRef` is intentionally absent from this list (task
    /// 1876): it is permitted on value cells so that user params like
    /// `material : Material = Material(...)` can be represented; the
    /// default expression evaluates to `Value::Undef`, which passes the
    /// kind-match check for any type.
    ///
    /// `Type::Geometry` is intentionally absent from this list (task 3604 /
    /// GHR-β): it is now representable as `Value::GeometryHandle`; see
    /// `is_representable_cell_type_admits_geometry`.
    #[test]
    #[allow(clippy::single_element_loop)]
    fn panics_on_unrepresentable_cell_types() {
        use std::panic;
        {
            let ty = Type::TypeParam("T".into());
            let mut graph = EvaluationGraph::default();
            let (id, node) = bad_node(ty);
            graph.value_cells.insert(id, node);
            let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                super::assert_value_cell_types_representable(&graph);
            }));
            assert!(result.is_err(), "expected panic but helper returned Ok");
            let err = result.unwrap_err();
            let msg = err
                .downcast_ref::<String>()
                .map(|s| s.as_str())
                .or_else(|| err.downcast_ref::<&str>().copied())
                .unwrap_or("<non-string panic>");
            assert!(
                msg.starts_with(&format!("{}:", super::ASSERT_MSG_PREFIX)),
                "panic message did not start with {:?} followed by colon: {msg}",
                super::ASSERT_MSG_PREFIX,
            );
            assert!(
                msg.contains("has cell_type "),
                "panic message did not contain \"has cell_type \": {msg}",
            );
        }
    }

    #[test]
    fn accepts_representable_types() {
        let mut graph = EvaluationGraph::default();
        for (entity, member, ty) in [
            ("E", "a", Type::Int),
            ("E", "b", Type::dimensionless_scalar()),
            ("E", "c", Type::Bool),
            ("E", "d", Type::List(Box::new(Type::Int))),
            // StructureRef is permitted (task 1876): struct-typed params like
            // `material : Material` are valid; their default evaluates to Undef
            // which passes the kind-match for any type.
            ("E", "e", Type::StructureRef("Material".into())),
            // Geometry is now representable (task 3604 / GHR-β): Value::GeometryHandle
            // was added so Geometry cells must not trigger the invariant assertion.
            ("E", "f", Type::Geometry),
        ] {
            let id = ValueCellId::new(entity, member);
            let node = ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Param,
                cell_type: ty,
                default_expr: None,
                content_hash: ContentHash(0),
            };
            graph.value_cells.insert(id, node);
        }
        super::assert_value_cell_types_representable(&graph);
    }

    /// Task 2373: `Type::Union` is a compile-time-only union over
    /// guarded-decl-group arm types — no Value counterpart exists, so cells
    /// must hold a single concrete arm type post-narrowing. The
    /// `is_representable_cell_type` predicate must reject it alongside
    /// `TypeParam`.
    #[test]
    fn is_representable_cell_type_rejects_union() {
        assert!(!super::is_representable_cell_type(&Type::Union(vec![
            Type::StructureRef("X".to_string())
        ])));
    }

    /// Task 3930 / β: `Type::Keyed` is the keyed sub-collection kind. It is
    /// structural — a `Keyed<T>` sub lowers to a `SubComponentDecl` and is never
    /// held in a value cell, and no `Value::Keyed` variant exists — so the
    /// `is_representable_cell_type` predicate must reject it alongside `TypeParam`
    /// and `Union`. This pins the eval-layer backstop for the case where a
    /// `Keyed<T>` is (mis)used in a value position such as `param x : Keyed<Vent>`:
    /// the compile-time value-position guard is deferred to γ/δ, and until then
    /// this predicate (and the runtime/CI invariants it backs) is what keeps such a
    /// cell from silently slipping through. γ may revisit if it introduces a
    /// `Value::Keyed` form.
    #[test]
    fn is_representable_cell_type_rejects_keyed() {
        assert!(
            !super::is_representable_cell_type(&Type::Keyed(Box::new(Type::StructureRef(
                "Vent".to_string()
            )))),
            "Type::Keyed must be non-representable as a value cell_type (β: no Value::Keyed; \
             keyed subs lower to SubComponentDecl)"
        );
    }

    // ── Applied / Projection representability (step-1 RED / task 4602 β) ────
    // RED until step-2 adds Type::Applied and Type::Projection variants.
    // Compile failure IS the RED signal.

    /// β: Type::Applied is representable — a phantom-args cell at runtime holds
    /// a `Value::StructureInstance` identified by name, ignoring type args.
    /// RED until step-2.
    #[test]
    fn is_representable_cell_type_admits_applied() {
        assert!(
            super::is_representable_cell_type(&Type::Applied {
                name: "Coupling".to_string(),
                args: vec![Type::StructureRef("Prismatic".to_string())],
            }),
            "Type::Applied must be representable (phantom-args cell, β)"
        );
    }

    /// β: Type::Projection is NOT representable — compile-time-only assoc-type
    /// access; no runtime value form exists until the base is concrete (δ).
    /// RED until step-2.
    #[test]
    fn is_representable_cell_type_rejects_projection() {
        assert!(
            !super::is_representable_cell_type(&Type::Projection {
                base: Box::new(Type::StructureRef("Prismatic".to_string())),
                member: "MotionValue".to_string(),
            }),
            "Type::Projection must be non-representable (compile-time only, β)"
        );
    }
}

/// GHR-δ §5 lazy revalidation: the classification of a cell's stored `Value`
/// after checking its geometry handle against the Engine's current
/// `realization_ref → handle` map ([`crate::Engine::realization_handles`]).
///
/// Returned by the pure oracle [`revalidate_geometry_handle`] and consumed by
/// the Engine read entry point (S16), which decides whether to write a
/// re-resolved value back into the snapshot and whether to bump the slow-path
/// counter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RevalidationOutcome {
    /// No change needed: the value is not a `GeometryHandle`, or its
    /// `kernel_handle` already matches the Engine's current handle for its
    /// `realization_ref`. The caller returns the value verbatim — no
    /// write-back, no slow-path bump (the §9 Q4 fast path).
    Fresh,
    /// The value was a `GeometryHandle` whose `kernel_handle` was stale. The
    /// carried `Value` keeps the same `realization_ref` + `upstream_values_hash`
    /// with `kernel_handle` re-resolved to the Engine's current handle (so it
    /// still compares `==` to the original — `kernel_handle` is excluded from
    /// `Value` equality). The caller writes it back and bumps the slow path.
    Resolved(Value),
    /// The value was a `GeometryHandle` whose `realization_ref` is ABSENT from
    /// the Engine's map (the backing realization no longer exists). The caller
    /// returns `Value::Undef` and bumps the slow path.
    Undef,
}

/// Pure validity oracle for a `Value::GeometryHandle` (PRD §5 lazy
/// revalidation). Compares the handle's `kernel_handle` against
/// `realization_handles[realization_ref]` and classifies the result into a
/// [`RevalidationOutcome`]:
///
/// - non-`GeometryHandle` value → `Fresh` (passthrough; nothing to revalidate)
/// - `realization_ref` present, handle EQUAL → `Fresh` (fast path)
/// - `realization_ref` present, handle DIFFERENT → `Resolved` with the current
///   handle spliced in (`realization_ref` + `upstream_values_hash` preserved)
/// - `realization_ref` ABSENT → `Undef` (the backing realization was removed)
///
/// No kernel coupling and no Engine borrow: the handle map is the entire
/// validity source (the GeometryKernel trait has no `is_valid` API and
/// snapshots carry no kernel reference — see plan.json design decision), so
/// this is unit-testable in isolation.
pub(crate) fn revalidate_geometry_handle(
    value: &Value,
    realization_handles: &HashMap<reify_core::RealizationNodeId, reify_ir::GeometryHandleId>,
) -> RevalidationOutcome {
    let (realization_ref, upstream_values_hash, kernel_handle) = match value {
        Value::GeometryHandle {
            realization_ref,
            upstream_values_hash,
            kernel_handle,
        } => (realization_ref, upstream_values_hash, kernel_handle),
        // Not a geometry handle — nothing to revalidate.
        _ => return RevalidationOutcome::Fresh,
    };

    match realization_handles.get(realization_ref) {
        // Fast path: the handle still matches the Engine's current resolution.
        Some(current) if Some(*current) == *kernel_handle => RevalidationOutcome::Fresh,
        // Slow path: stale handle — re-resolve to the current one, preserving
        // identity (realization_ref + upstream_values_hash) so the re-resolved
        // value remains `==` to the original.
        Some(current) => RevalidationOutcome::Resolved(Value::GeometryHandle {
            realization_ref: realization_ref.clone(),
            upstream_values_hash: *upstream_values_hash,
            kernel_handle: Some(*current),
        }),
        // Slow path: the backing realization is gone.
        None => RevalidationOutcome::Undef,
    }
}

/// Tests for `hash_imported_file_content`.
///
/// Deliberately NOT inside `#[cfg(all(test, debug_assertions))] mod invariant_tests`
/// because file-content hashing is not a debug-only invariant — it must run in
/// release-mode test builds too (PRD task 4 / task 2668).
///
/// Each test uses `tempfile::tempdir()` so cleanup is guaranteed even on panic —
/// the `TempDir` guard's `Drop` impl removes the directory unconditionally.
#[cfg(test)]
mod imported_file_hash_tests {
    use reify_core::ContentHash;
    use std::fs;

    use super::hash_imported_file_content;

    /// (a) Round-trip: `hash_imported_file_content` returns
    /// `Ok(ContentHash::of(&bytes))` for known byte content.
    #[test]
    fn hash_imported_file_content_round_trip() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let bytes: &[u8] = b"some vdb file bytes \x00\x01\x02";
        let path = dir.path().join("file_a.vdb");
        fs::write(&path, bytes).expect("write file_a");
        let path_str = path.to_str().expect("path utf8");

        let result = hash_imported_file_content(path_str)
            .expect("hash_imported_file_content should succeed");
        assert_eq!(
            result,
            ContentHash::of(bytes),
            "round-trip: hash must equal ContentHash::of(&bytes)"
        );
    }

    /// (b) Path-independence: two distinct paths with identical byte content
    /// produce the same `ContentHash` — the path string is intentionally
    /// NOT mixed into the hash domain (PRD acceptance: "file-path change
    /// with same content → cache hit").
    #[test]
    fn hash_imported_file_content_path_independent() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let bytes: &[u8] = b"identical content at two paths";

        let path1 = dir.path().join("file_a.vdb");
        let path2 = dir.path().join("file_b.vdb");
        fs::write(&path1, bytes).expect("write file_a");
        fs::write(&path2, bytes).expect("write file_b");

        let hash1 =
            hash_imported_file_content(path1.to_str().expect("path1 utf8")).expect("hash file_a");
        let hash2 =
            hash_imported_file_content(path2.to_str().expect("path2 utf8")).expect("hash file_b");
        assert_eq!(
            hash1, hash2,
            "path-independence: same content at different paths must yield the same ContentHash"
        );
    }

    /// (c) IO-error propagation: calling the helper on a guaranteed-nonexistent
    /// path returns `Err(e)` with `e.kind() == std::io::ErrorKind::NotFound`.
    #[test]
    fn hash_imported_file_content_propagates_not_found() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let missing = dir.path().join("does_not_exist.vdb");
        let missing_str = missing.to_str().expect("missing utf8");

        let err = hash_imported_file_content(missing_str)
            .expect_err("must return Err for nonexistent path");
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::NotFound,
            "IO error kind must be NotFound for missing file"
        );
    }
}

/// GHR-δ §5 lazy-revalidation pure-helper unit tests (S13).
///
/// `revalidate_geometry_handle` is the kernel-free validity oracle: given a
/// cell's stored `Value` and the Engine's `realization_ref → current handle`
/// map, it classifies the value into a [`RevalidationOutcome`] without touching
/// the kernel or borrowing the Engine. These cases pin the PRD §5 truth table:
/// equal handle → Fresh (fast path); stale handle → Resolved with the current
/// handle spliced in (identity preserved so it still `==` the original); absent
/// realization → Undef; non-handle → Fresh passthrough.
///
/// RED until S14 defines `RevalidationOutcome` + `revalidate_geometry_handle`.
#[cfg(test)]
mod revalidation_tests {
    use super::{RevalidationOutcome, revalidate_geometry_handle};
    use reify_core::RealizationNodeId;
    use reify_ir::{GeometryHandleId, Value};
    use std::collections::HashMap;

    /// A `Value::GeometryHandle` backed by `realization` with the given kernel id
    /// and a fixed upstream hash (so identity preservation is observable).
    fn handle(realization: &RealizationNodeId, id: u64) -> Value {
        Value::GeometryHandle {
            realization_ref: realization.clone(),
            upstream_values_hash: [7u8; 32],
            kernel_handle: Some(GeometryHandleId(id)),
        }
    }

    /// (a) realization_ref present and kernel_handle EQUAL → `Fresh` (fast path,
    /// no write-back).
    #[test]
    fn revalidate_equal_handle_is_fresh() {
        let r0 = RealizationNodeId::new("Widget", 0);
        let mut map = HashMap::new();
        map.insert(r0.clone(), GeometryHandleId(42));
        assert_eq!(
            revalidate_geometry_handle(&handle(&r0, 42), &map),
            RevalidationOutcome::Fresh
        );
    }

    /// (b) realization_ref present but kernel_handle DIFFERENT → `Resolved` whose
    /// Value carries the SAME realization_ref + upstream_values_hash with
    /// kernel_handle re-resolved to the current handle. The re-resolved value
    /// must compare `==` to the original (kernel_handle is excluded from Value
    /// equality per the GeometryHandle variant contract).
    #[test]
    fn revalidate_stale_handle_resolves_to_current() {
        let r0 = RealizationNodeId::new("Widget", 0);
        let mut map = HashMap::new();
        map.insert(r0.clone(), GeometryHandleId(99));
        let stale = handle(&r0, 42);

        match revalidate_geometry_handle(&stale, &map) {
            RevalidationOutcome::Resolved(resolved) => {
                match &resolved {
                    Value::GeometryHandle {
                        realization_ref,
                        upstream_values_hash,
                        kernel_handle,
                    } => {
                        assert_eq!(
                            *kernel_handle,
                            Some(GeometryHandleId(99)),
                            "kernel_handle must be re-resolved to the current handle"
                        );
                        assert_eq!(realization_ref, &r0, "realization_ref must be preserved");
                        assert_eq!(
                            upstream_values_hash, &[7u8; 32],
                            "upstream_values_hash must be preserved"
                        );
                    }
                    other => panic!("expected GeometryHandle, got {:?}", other),
                }
                assert_eq!(
                    resolved, stale,
                    "re-resolved value must == original under Value PartialEq (kernel_handle excluded)"
                );
            }
            other => panic!("expected Resolved, got {:?}", other),
        }
    }

    /// (c) realization_ref ABSENT from the map → `Undef` (backing realization
    /// removed; the read returns Value::Undef).
    #[test]
    fn revalidate_absent_realization_is_undef() {
        let r0 = RealizationNodeId::new("Widget", 0);
        let map: HashMap<RealizationNodeId, GeometryHandleId> = HashMap::new();
        assert_eq!(
            revalidate_geometry_handle(&handle(&r0, 42), &map),
            RevalidationOutcome::Undef
        );
    }

    /// (d) non-GeometryHandle value → `Fresh` passthrough (nothing to
    /// revalidate, regardless of map contents).
    #[test]
    fn revalidate_non_handle_is_fresh() {
        let map: HashMap<RealizationNodeId, GeometryHandleId> = HashMap::new();
        assert_eq!(
            revalidate_geometry_handle(&Value::Int(7), &map),
            RevalidationOutcome::Fresh
        );
        assert_eq!(
            revalidate_geometry_handle(&Value::Undef, &map),
            RevalidationOutcome::Fresh
        );
    }
}

// --- Task 4364: suppress eval-side E_MECHANISM_NONDRIVING_JOINT double-emission ---
//
// Unit tests for the pure helper `nondriving_joint_compile_spans` (step-1/2)
// and the `detect_error_map_diagnostics` suppress-predicate seam (step-3/4).
// Mirror the `revalidation_tests` pattern: hand-built fixtures, `use super::...`,
// no .ri files or CompiledModule construction needed.
#[cfg(test)]
mod nondriving_joint_suppression_tests {
    use super::{detect_error_map_diagnostics, nondriving_joint_compile_spans};
    use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan, ValueCellId};
    use reify_ir::{Value, ValueMap};
    use std::collections::{BTreeMap, HashSet};

    /// `nondriving_joint_compile_spans` must collect only the label spans of
    /// `MechanismNonDrivingJoint`-coded diagnostics, ignoring:
    /// - `MechanismNonDrivingJoint` diagnostics that carry NO label,
    /// - diagnostics with a different code that DO carry a label.
    ///
    /// This pins the helper's filtering contract: only matching-code labelled
    /// spans reach the eval-side suppression predicate.
    #[test]
    fn compile_spans_collects_only_matching_code_labelled_spans() {
        let span = SourceSpan::new(10, 20);

        let diagnostics = vec![
            // (a) matching code + label → span is collected
            Diagnostic::error("a")
                .with_code(DiagnosticCode::MechanismNonDrivingJoint)
                .with_label(DiagnosticLabel::new(span, "x")),
            // (b) matching code but NO label → span is NOT collected
            Diagnostic::error("b").with_code(DiagnosticCode::MechanismNonDrivingJoint),
            // (c) different code but HAS a label → span is NOT collected
            Diagnostic::error("c")
                .with_code(DiagnosticCode::MechanismDuplicateSolid)
                .with_label(DiagnosticLabel::new(SourceSpan::new(100, 200), "y")),
        ];

        let result = nondriving_joint_compile_spans(&diagnostics);
        let expected: HashSet<SourceSpan> = [span].iter().copied().collect();
        assert_eq!(
            result, expected,
            "must collect exactly the label spans of MechanismNonDrivingJoint diagnostics; \
             unlabelled same-code and labelled different-code diagnostics must be excluded"
        );
    }

    /// Helper: build a `Value::Map` that looks like a `make_nondriving_joint_error`
    /// result — `error="nondriving_joint"` with a distinct `error_message` so two
    /// such maps are structurally unequal and the dedup step does NOT merge them.
    fn nondriving_error_map(msg: &str) -> Value {
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("error".to_string()),
            Value::String("nondriving_joint".to_string()),
        );
        m.insert(
            Value::String("error_message".to_string()),
            Value::String(msg.to_string()),
        );
        Value::Map(m)
    }

    /// `detect_error_map_diagnostics` with a `suppress` predicate must skip the
    /// suppressed cell while STILL emitting for the non-suppressed cell.
    ///
    /// Pins the "span-scoped, not blanket" contract: the defense-in-depth
    /// guarantee that a compile-caught cell is suppressed but a distinct
    /// loop-bound cell (whose id is NOT in the compile-span set) still emits.
    ///
    /// RED until the `suppress` parameter is added to `detect_error_map_diagnostics`
    /// (step-4).
    #[test]
    fn detect_error_map_suppresses_one_cell_emits_other() {
        let keep_id = ValueCellId::new("E", "keep");
        let drop_id = ValueCellId::new("E", "drop");

        // Two structurally distinct error Maps so dedup does NOT collapse them.
        let mut values = ValueMap::new();
        values.insert(keep_id.clone(), nondriving_error_map("keep msg"));
        values.insert(drop_id.clone(), nondriving_error_map("drop msg"));

        // Suppress only the "drop" cell; "keep" must still emit.
        let result = detect_error_map_diagnostics(
            &values,
            None,
            "nondriving_joint",
            DiagnosticCode::MechanismNonDrivingJoint,
            "fallback",
            Some(&|cid: &ValueCellId| *cid == ValueCellId::new("E", "drop")),
        );

        assert_eq!(
            result.len(),
            1,
            "exactly one diagnostic must be emitted for the unsuppressed 'keep' cell; \
             got {} diagnostic(s)",
            result.len(),
        );
        assert_eq!(
            result[0].code,
            Some(DiagnosticCode::MechanismNonDrivingJoint),
            "the surviving diagnostic must carry MechanismNonDrivingJoint"
        );
    }
}
