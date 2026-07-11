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

use std::collections::{HashMap, HashSet};

use reify_core::{RealizationNodeId, Type, ValueCellId};
use reify_ir::{CompiledExprKind, CompiledFunction, DeterminacyState, PersistentMap, Value};

use crate::cache::NodeId;
use crate::deps::DependencyTrace;
use crate::graph::EvaluationGraph;

/// A single no-stale-Undef violation: `cell` is `Value::Undef` even though
/// every one of its static dependencies is resolved (§6.1 clause 4) and no
/// exclusion (auto, `@optimized`, guard-inactive, undef-literal, build-only
/// geometry-consumer) applies.
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
///    that is itself Undef, makes `c` EXEMPT, not violating;
/// 5. `c` is not `@optimized`-dispatched (a `UserFunctionCall` default_expr
///    whose matching `CompiledFunction` has `optimized_target: Some(..)` —
///    the exact predicate R3e uses, `engine_eval.rs`'s
///    `re_eval_consumers_of_in_walk_mints`, to avoid clobbering a
///    compute-dispatch result) and `c` is not a guard-inactive member of a
///    `GuardedGroupInfo` (mirroring `EvaluationGraph::active_constraint_ids`'s
///    guard-value dispatch, but over `members`/`else_members` rather than
///    `constraints`/`else_constraints`);
/// 6. `c`'s `default_expr` is not a `Literal(Value::Undef)` (the UserUndef
///    `param u = undef` / `let x = undef` shape) — α's sanctioned
///    exclusion-set refinement per §11 Q2/Q4, structural rather than via
///    the `undef_causes` map, beyond §6.1's two literal clause-5
///    exclusions. Deliberately does NOT exclude a pure op-contract-failure
///    cell (all deps resolved, the op itself returns `Undef`) — only task γ
///    (the op-sink) can identify that case (`UndefCause::OpContractFailed`,
///    `value.rs:3733-3788`); α honestly cannot distinguish it from genuine
///    staleness.
/// 7. `c`'s `cell_type` is not `Type::Geometry` (which also covers the
///    `DatumRef` alias, `type_resolution.rs:551`). A `Type::Geometry` cell's
///    resolved value is exclusively `Value::GeometryHandle`, hydrated by
///    `engine_build.rs`'s `post_process_geometry_handle_cells` — which
///    writes ONLY into `build()`'s transient local `ValueMap` /
///    `BuildResult`, never back into the retained `Engine::eval_state()`
///    snapshot this checker inspects (confirmed: `engine_build.rs` writes
///    `state.snapshot.values` explicitly in exactly one place,
///    `redispatch_geometry_consuming_compute_nodes`'s narrow compute-node
///    re-dispatch — NOT the geometry-handle hydration itself). So every
///    `Type::Geometry` cell is `Value::Undef` in `eval_state()` by
///    construction, whether or not `build()` was ever called — a standing
///    surface gap, not staleness;
/// 8. `c`'s `default_expr` does not require build()-only resolution
///    anywhere in its tree. Recognised via two complementary mechanisms
///    (see [`is_build_only_dispatch_call`]). The primary one is a DERIVED
///    STRUCTURAL rule (task γ / #4954): a call that *consumes* a geometry/
///    selector value (`geometry_ops::consumes_geometry_or_selector` — keyed
///    on argument static types) but is NOT one of the kernel-free symbolic-
///    eval-wired selector ctors
///    (`geometry_ops::is_symbolic_eval_wired_selector_ctor`, the single
///    EVAL_WIRED map) resolves only against a realized kernel; likewise a
///    feature→datum projection (`geometry_ops::is_feature_datum_projection_call`)
///    or an ad-hoc topology selector (`CompiledExprKind::AdHocSelector`,
///    e.g. `body @ face("top")`). This rule CLOSES the class the old name
///    lists missed — `face`/`edge`/`solid_body`/`vertex` named-leaf ctors,
///    composition `union`/`intersect`/`difference`, GD&T `max_deviation`,
///    `split`, `feature`, the conformance queries, and any FUTURE geometry-
///    arg consumer — with no per-name upkeep, while the eval-wired guard
///    keeps the net tight (a well-formed `faces_by_normal` &c. that stays
///    `Undef` is a real bug, not exempt). The kernel geometry-query family
///    (`is_geometry_consumer_call`: `volume`/`centroid`/…, `angle`) and the
///    kinematic/dynamics queries stay REGISTRY-keyed: they are build()-only
///    but consume ABSTRACTED handles (direction vectors, mechanism snapshots,
///    body-ids), not a geometry-typed arg the structural rule can see — a
///    bounded, stable set, unlike the open-ended geometry-consumer class the
///    structural rule absorbs. Checked anywhere in the expr tree AND,
///    transitively, inside the body of any `UserFunctionCall` callee (e.g. a
///    stdlib helper like `body_mass_props` that calls `volume(...)`
///    internally — the call-site expr alone doesn't show it). Such calls are
///    documented (`engine_eval.rs`'s `detect_unresolved_geometry_consumers`,
///    task #4651 R1a; `ad_hoc_face_selector.ri`'s header comment) to require
///    a realized geometry kernel and resolve only via `build()`/
///    `tessellate()`, never on the kernel-less `eval()`/`eval_cached()`
///    surface this checker runs against.
pub fn check_no_stale_undef(
    graph: &EvaluationGraph,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    trace_map: &HashMap<NodeId, DependencyTrace>,
    functions: &[CompiledFunction],
) -> Vec<StaleUndefViolation> {
    let empty_trace = DependencyTrace::default();
    let guard_inactive_members = guard_inactive_members(graph, values);
    let mut violations = Vec::new();

    for (id, cell) in graph.value_cells.iter() {
        // Clause 1: auto cells are solver-owned; not candidates.
        if cell.kind.is_auto() {
            continue;
        }
        // Clause 2: no default_expr => nothing to have gone stale.
        let Some(expr) = cell.default_expr.as_ref() else {
            continue;
        };
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

        // Clause 5a: `@optimized` UserFunctionCall — the EXACT predicate
        // R3e uses (engine_eval.rs's re_eval_consumers_of_in_walk_mints,
        // ~line 6759+) to avoid misclassifying a compute-dispatched cell
        // (evaluated outside the plain expr-eval path) as stale.
        if let CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } = &expr.kind
            && reify_expr::find_matching_compiled_function(functions, function_name, args)
                .and_then(|f| f.optimized_target.clone())
                .is_some()
        {
            continue;
        }

        // Clause 5b: guard-inactive member — this cell belongs to the
        // branch of a `GuardedGroupInfo` that the current guard value does
        // NOT select, so its Undef is expected (unevaluated branch), not
        // stale.
        if guard_inactive_members.contains(id) {
            continue;
        }

        // Clause 6: UserUndef structural exclusion (§11 Q2/Q4 — α's
        // sanctioned exclusion-set refinement beyond §6.1's two literal
        // clause-5 exclusions). A `param u = undef` / `let x = undef`
        // default is author-intended, not stale: clause 4 cannot exempt it
        // (zero deps is vacuously "all resolved"), so it is excluded
        // structurally — by the *shape* of `default_expr` — instead of via
        // the `undef_causes` map (this checker stays a pure function of
        // graph/values/trace_map/functions; the harness never calls
        // `set_capture_undef_causes`). Deliberately NOT excluded here: a
        // pure op-contract-failure cell (all deps resolved, yet the op
        // itself returns `Undef`, e.g. `sqrt` of a resolved-negative) — α
        // cannot distinguish that from genuine staleness;
        // `UndefCause::OpContractFailed` is recorded by task γ (the
        // op-sink) only (value.rs:3733-3788), never by α. Unbound/
        // propagated Undefs are already exempt via clause 4;
        // AwaitingSolve/SolveFailed auto cells via clause 1.
        if matches!(&expr.kind, CompiledExprKind::Literal(Value::Undef)) {
            continue;
        }

        // Clause 7: `Type::Geometry` (incl. the `DatumRef` alias) cells are
        // hydrated to `Value::GeometryHandle` ONLY inside `build()`'s local
        // `ValueMap` — never written back into the retained `eval_state()`
        // snapshot this checker inspects. Undef here is a standing
        // surface gap, not staleness (see the type-level doc comment above
        // for the engine_build.rs evidence trail).
        if cell.cell_type == Type::Geometry {
            continue;
        }

        // Clause 8: build()-only dependency (broad-sweep residual, §11 Q4's
        // "err broad" corpus-scope call). A cell whose `default_expr`
        // requires build()-only resolution ANYWHERE in its tree — bare
        // (`let centroid = centroid(geometry)`), nested (`let mass =
        // volume(geometry) * material.density`, `structural_physical.ri:46`),
        // behind a plain (non-`@optimized`) stdlib `UserFunctionCall`
        // wrapper (`let mp = body_mass_props(geometry, rho)`,
        // `ambient_default_surface.ri`), a named-leaf selector constructor
        // over a plain geometry target (`let s = face(b, "tag")`,
        // `bt8_named_leaf_interim.ri` — task γ / #4954 fallout), or an
        // ad-hoc selector (`let
        // top_frame = body @ face("top")`, `ad_hoc_face_selector.ri`) —
        // cannot be resolved on the kernel-less `eval()`/`eval_cached()`
        // surface at all: these require a realized geometry kernel and are
        // only resolvable via `build()`/`tessellate()`. This is the exact
        // class `engine_eval.rs`'s `detect_unresolved_geometry_consumers`
        // (task #4651 R1a / DD-4) already diagnoses at `EvalUnresolved` for
        // the direct-call case; generalized here to nested/UserFunctionCall
        // -body/ad-hoc-selector occurrences (which #4651's detector — by
        // design, top-level-builtin-only — does not catch, matching
        // `geometry_ops::try_eval_geometry_query`'s own DIRECT/NESTED split
        // for the build-path fold). A documented, standing eval-surface
        // limitation, not staleness.
        if expr_requires_build_only_resolution(expr, functions) {
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

impl crate::Engine {
    /// Debug-gate corpus-harness wrapper: threads this Engine's own retained
    /// post-eval state (`eval_state()`) and function table into the
    /// free-function [`check_no_stale_undef`]. Returns an empty `Vec` when no
    /// eval has run yet (`eval_state()` is `None`) — there is no retained
    /// state to check.
    ///
    /// Task ε (PRD §6.1 "extends it graph-natively to edit_param/
    /// edit_source"): this wrapper is reused UNCHANGED across all four
    /// state-producing entry points — `eval`, `eval_cached` (engine_eval.rs)
    /// and `edit_param`, `edit_source` (engine_edit.rs). Every one of them
    /// installs its own post-run snapshot/reverse_index/trace_map into
    /// `self.eval_state` immediately before returning an `EvalResult`/
    /// `CachedEvalResult` whose `values` match that snapshot, so this
    /// wrapper always inspects exactly the state the calling entry point
    /// just produced. No edit-path-specific checker variant or
    /// `TopologyTemplate` dependency is needed; see
    /// `crates/reify-eval/tests/no_stale_undef_edit_path_gate.rs` for the
    /// edit-path regression coverage.
    pub fn check_no_stale_undef(&self) -> Vec<StaleUndefViolation> {
        let Some(state) = self.eval_state() else {
            return Vec::new();
        };
        check_no_stale_undef(
            &state.snapshot.graph,
            &state.snapshot.values,
            &state.trace_map,
            &self.functions,
        )
    }
}

/// Query-family name registries for build()-only ops that consume ABSTRACTED
/// handles (mechanism snapshots, body-ids), NOT a raw geometry/selector value
/// — so the structural [`geometry_ops::consumes_geometry_or_selector`] rule
/// cannot see them and they are keyed by name. Mirror the build-path dispatch
/// tables (`reify_compiler::units::DYNAMICS_QUERY_NAMES`, units.rs:870;
/// `try_eval_kinematic_query`'s `KinematicHelper` arms) — kept as small local
/// literals rather than threading cross-crate `pub` exports for a handful of
/// names.
const DYNAMICS_QUERY_NAMES: &[&str] = &["body_mass_props"];
const KINEMATIC_QUERY_NAMES: &[&str] = &["interferes", "interferes_with", "min_clearance"];

/// `true` iff `expr` (a single node, NOT a tree — callers walk the tree) is
/// itself a call that can only be resolved by a build()-integrated
/// post-process pass. Clause 8 recognises the build-only class via two
/// complementary mechanisms:
///
/// 1. Structural node-kinds: an ad-hoc topology selector
///    (`CompiledExprKind::AdHocSelector`, e.g. `body @ face("top")`) and a
///    feature→datum projection method call
///    (`geometry_ops::is_feature_datum_projection_call`, e.g. `feature.axis`/
///    `.plane`/`.point`/`.dir`, gated on the receiver's static type).
///
/// 2. The DERIVED STRUCTURAL RULE (task γ / #4954): a call that *consumes* a
///    geometry/selector value ([`geometry_ops::consumes_geometry_or_selector`],
///    keyed on argument static types) but is NOT one of the kernel-free
///    symbolic-eval-wired selector ctors
///    ([`geometry_ops::is_symbolic_eval_wired_selector_ctor`], the single
///    EVAL_WIRED map). This CLOSES the class the old clause-8 name lists
///    missed — composition `union`/`intersect`/`difference`, the `face`/
///    `edge`/`solid_body`/`vertex` named-leaf ctors, GD&T `max_deviation`,
///    `split`, `feature`, the conformance queries, AND any future geometry-arg
///    consumer — with no per-name upkeep. The eval-wired guard keeps the α net
///    tight: a well-formed leaf ctor (`faces_by_normal` &c.) still `Undef`
///    post-eval with all deps resolved is a genuine arity/scheduling bug and
///    stays a violation, NOT an exemption.
///
/// 3. Registry recognition for the kernel geometry-query family
///    (`geometry_ops::is_geometry_consumer_call` — `volume`/`centroid`/… and
///    `angle` over direction vectors) and the kinematic/dynamics queries
///    (`KINEMATIC_QUERY_NAMES`/`DYNAMICS_QUERY_NAMES`). These are build()-only
///    but consume ABSTRACTED handles (direction vectors, mechanism snapshots,
///    body-ids) — NOT a geometry-typed arg — so the structural rule (2) cannot
///    see them; they are a bounded, stable set keyed by their dispatch
///    registries. (The whack-a-mole risk — an open-ended stream of NEW
///    geometry consumers — is what rule (2) structurally absorbs; these
///    families do not grow that way.)
///
/// Before task γ (#4954) a selector/query consumer of a top-level geometry
/// LET was exempted upstream via clause 4's "missing producer" path (geometry
/// lets had no value cell of their own); pairing every geometry let with a
/// real value cell (γ) made the dependency genuinely resolved (via the R3d
/// symbolic-handle mint), exposing this whole build-only class to clause 8
/// for the first time.
fn is_build_only_dispatch_call(expr: &reify_ir::CompiledExpr) -> bool {
    // (1) Structural node-kinds: always build()-only.
    if matches!(expr.kind, CompiledExprKind::AdHocSelector { .. })
        || crate::geometry_ops::is_feature_datum_projection_call(expr)
    {
        return true;
    }
    // (2) Derived structural rule: consumes a geometry/selector value, yet is
    // not resolved by the kernel-free symbolic-eval pass ⇒ build()-only.
    if crate::geometry_ops::consumes_geometry_or_selector(expr)
        && !crate::geometry_ops::is_symbolic_eval_wired_selector_ctor(expr)
    {
        return true;
    }
    // (3) Registry-keyed query families over abstracted handles (no
    // geometry-typed arg for (2) to see): kernel geometry queries (incl.
    // `angle`), kinematics, dynamics.
    if crate::geometry_ops::is_geometry_consumer_call(expr) {
        return true;
    }
    matches!(
        &expr.kind,
        CompiledExprKind::FunctionCall { function, .. }
            if DYNAMICS_QUERY_NAMES.contains(&function.name.as_str())
                || KINEMATIC_QUERY_NAMES.contains(&function.name.as_str())
    )
}

/// `true` iff `expr` requires build()-only resolution anywhere in its tree —
/// [`is_build_only_dispatch_call`] anywhere in the tree — clause 8's
/// build-only-dependency exclusion. Uses `CompiledExpr::walk` (the
/// sanctioned traversal — see its doc comment, `reify-ir/src/expr.rs`) so
/// every variant (bare call, `BinOp`, nested `FunctionCall` args, …) is
/// covered without re-deriving a match over `CompiledExprKind` here.
///
/// Also recurses into the body of any `UserFunctionCall` callee found along
/// the way (via `functions`), so a plain (non-`@optimized`) stdlib wrapper
/// like `body_mass_props(geometry, rho)` — whose OWN call-site args are just
/// `ValueRef`s, with the actual `volume(...)` call hidden inside the
/// callee's body — is still caught. `visited` bounds the recursion against
/// (mutually) recursive function bodies; reify functions are not expected
/// to recurse, but this is a cheap defensive bound rather than an
/// assumption baked into the traversal.
fn expr_requires_build_only_resolution(
    expr: &reify_ir::CompiledExpr,
    functions: &[CompiledFunction],
) -> bool {
    let mut visited = HashSet::new();
    expr_requires_build_only_resolution_inner(expr, functions, &mut visited)
}

fn expr_requires_build_only_resolution_inner(
    expr: &reify_ir::CompiledExpr,
    functions: &[CompiledFunction],
    visited: &mut HashSet<String>,
) -> bool {
    let mut found = false;
    expr.walk(&mut |node| {
        if found {
            return;
        }
        if is_build_only_dispatch_call(node) {
            found = true;
            return;
        }
        if let CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } = &node.kind
            && visited.insert(function_name.clone())
            && let Some(f) =
                reify_expr::find_matching_compiled_function(functions, function_name, args)
        {
            let body_has_it = expr_requires_build_only_resolution_inner(
                &f.body.result_expr,
                functions,
                visited,
            ) || f.body.let_bindings.iter().any(|(_, let_expr)| {
                expr_requires_build_only_resolution_inner(let_expr, functions, visited)
            });
            if body_has_it {
                found = true;
            }
        }
    });
    found
}

/// Computes the set of value cells that are members of the currently
/// INACTIVE branch of some `GuardedGroupInfo` in `graph.guarded_groups`.
/// Mirrors `EvaluationGraph::active_constraint_ids`'s guard-value dispatch
/// (`graph.rs`), but over `members`/`else_members` (`ValueCellId`) rather
/// than `constraints`/`else_constraints` (`ConstraintNodeId`):
/// - guard is `Bool(true)`  => `else_members` are inactive;
/// - guard is `Bool(false)` => `members` are inactive;
/// - guard is `Undef`/non-`Bool` (or missing from `values`) => BOTH
///   `members` and `else_members` are inactive (neither branch selected).
fn guard_inactive_members(
    graph: &EvaluationGraph,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
) -> HashSet<ValueCellId> {
    let mut inactive = HashSet::new();
    for group in &graph.guarded_groups {
        match cell_value_or_undef(values, &group.guard_cell) {
            Value::Bool(true) => inactive.extend(group.else_members.iter().cloned()),
            Value::Bool(false) => inactive.extend(group.members.iter().cloned()),
            _ => {
                inactive.extend(group.members.iter().cloned());
                inactive.extend(group.else_members.iter().cloned());
            }
        }
    }
    inactive
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
/// the graph AND its stored value is neither `Value::Undef` nor a
/// collection that recursively contains one ([`value_is_or_contains_undef`]).
/// A missing producer or an unresolved producer both make the exemption
/// fire (erring EXEMPT, never falsely violating) — §6.1 clause 4.
fn value_cell_dep_is_resolved(
    graph: &EvaluationGraph,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    dep: &ValueCellId,
) -> bool {
    if !graph.value_cells.contains_key(dep) {
        return false; // missing-producer
    }
    !value_is_or_contains_undef(&cell_value_or_undef(values, dep))
}

/// `true` iff `value` is `Value::Undef`, or is a `List`/`Set`/`Map`/`Option`
/// that recursively contains `Value::Undef` anywhere inside — clause 4's
/// dependency-resolution exemption, extended one level into the handful of
/// collection shapes the corpus's element-wise aggregate ops (`.sum`,
/// `forall`) actually iterate over.
///
/// Without this, a dependency like `let all_masses = [self.b01.mass, ...]`
/// (`large_assembly.ri`) or `let xs: List<Bool> = [true, a, true]`
/// (`kleene_e2e.ri`) reads as "resolved" under a plain top-level
/// `Value::Undef` check — it IS a concrete `List`, just with an Undef
/// element — so clause 4 could not exempt `total_mass = all_masses.sum` /
/// `p4 = forall x in xs: x`. This is exactly the "op returns Undef from a
/// resolved-looking input" shape that §6.1/§11 defer past α: only task γ
/// (the op-sink) can tell a genuine op-contract failure from correct
/// Kleene-style Undef propagation through an aggregate; α erring EXEMPT
/// here mirrors clause 4's existing dependency-resolution policy one level
/// deeper, rather than guessing.
///
/// Deliberately does NOT recurse into other composite `Value` variants
/// (`StructureInstance`, `Tensor`, `Point`, `Frame`, …): those represent a
/// single composite mathematical/geometric object, not a collection of
/// independently-sourced sub-results, and no corpus fixture forces that
/// case — extending to them would be speculative, not evidence-driven.
///
/// `pub(crate)`: also reused by
/// [`crate::engine_build::Engine::post_process_derived_lets`] (task 4725) as
/// its Undef-Let candidate-selection filter, for the identical reason —
/// `all_masses`'s stored value is a concrete `List` with `Undef` elements
/// (not top-level `Value::Undef`), so the shallow `Value::is_undef` check
/// used there previously never re-selected it once its cross-sub `mass`
/// dependencies folded, leaving `total_mass = all_masses.sum` permanently
/// stuck at `Value::Undef`.
pub(crate) fn value_is_or_contains_undef(value: &Value) -> bool {
    match value {
        Value::Undef => true,
        Value::List(items) => items.iter().any(value_is_or_contains_undef),
        Value::Set(items) => items.iter().any(value_is_or_contains_undef),
        Value::Map(entries) => entries
            .iter()
            .any(|(k, v)| value_is_or_contains_undef(k) || value_is_or_contains_undef(v)),
        Value::Option(inner) => inner.as_deref().is_some_and(value_is_or_contains_undef),
        _ => false,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::ValueCellKind;
    use reify_core::{ContentHash, Type};
    use reify_ir::{
        CompiledExpr, CompiledExprKind, CompiledFnBody, StructureInstanceData, StructureTypeId,
    };

    use crate::graph::{GuardedGroupInfo, ValueCellNode};

    /// Fabricates a `Let` value cell with the given `default_expr`, registers
    /// it in `graph.value_cells`, and stamps its `(value, determinacy)` into
    /// `values`. Returns the cell's `ValueCellId`.
    #[allow(clippy::too_many_arguments)]
    fn seed_cell(
        graph: &mut EvaluationGraph,
        values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        entity: &str,
        member: &str,
        default_expr: CompiledExpr,
        value: Value,
        determinacy: DeterminacyState,
    ) -> ValueCellId {
        let id = ValueCellId::new(entity, member);
        graph.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Let,
                cell_type: Type::length(),
                default_expr: Some(default_expr),
                content_hash: ContentHash::of_str(&format!("{entity}.{member}")),
            },
        );
        values.insert(id.clone(), (value, determinacy));
        id
    }

    /// A zero-arg `CompiledFunction` whose `optimized_target` is `Some(target)`
    /// — matched by `find_matching_compiled_function` against a zero-arg
    /// `UserFunctionCall { function_name: name, args: vec![] }` (arity 0 == 0,
    /// vacuously "all params match").
    fn zero_arg_optimized_function(name: &str, target: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            params: vec![],
            param_defaults: vec![],
            return_type: Type::length(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::literal(Value::length(0.0), Type::length()),
            },
            content_hash: ContentHash::of_str(name),
            annotations: vec![],
            optimized_target: Some(target.to_string()),
            type_params: vec![],
        }
    }

    /// §6.1 clause-5 exclusions (RED until step-4 implements them): an
    /// `@optimized` `UserFunctionCall` cell and a guard-inactive member must
    /// NOT be reported, even though both are otherwise indistinguishable from
    /// a genuine stale-Undef violation under clauses 1-4 alone.
    #[test]
    fn optimized_and_guard_inactive_cells_are_excluded() {
        let mut graph = EvaluationGraph::default();
        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        let mut trace_map: HashMap<NodeId, DependencyTrace> = HashMap::new();

        // A shared, resolved producer so every candidate below has "all deps
        // resolved" — isolating clause-5 as the only thing under test.
        let producer_id = seed_cell(
            &mut graph,
            &mut values,
            "Clause5",
            "producer",
            CompiledExpr::literal(Value::length(1.0), Type::length()),
            Value::length(1.0),
            DeterminacyState::Determined,
        );

        // The genuine violation: causeless staleness, no exclusion applies.
        let genuine_id = seed_cell(
            &mut graph,
            &mut values,
            "Clause5",
            "genuine",
            CompiledExpr::value_ref(producer_id.clone(), Type::length()),
            Value::Undef,
            DeterminacyState::Undetermined,
        );
        trace_map.insert(
            NodeId::Value(genuine_id.clone()),
            DependencyTrace {
                reads: vec![producer_id.clone()],
                realization_reads: Vec::new(),
            },
        );

        // (a) @optimized UserFunctionCall — zero-arg call matching a
        // fabricated CompiledFunction whose optimized_target is Some(..). No
        // trace_map entry => empty trace => vacuously "all deps resolved".
        let optimized_id = seed_cell(
            &mut graph,
            &mut values,
            "Clause5",
            "optimized",
            CompiledExpr {
                kind: CompiledExprKind::UserFunctionCall {
                    function_name: "opt_fn".to_string(),
                    args: vec![],
                },
                result_type: Type::length(),
                content_hash: ContentHash::of_str("optimized-call"),
            },
            Value::Undef,
            DeterminacyState::Undetermined,
        );
        let functions = vec![zero_arg_optimized_function(
            "opt_fn",
            "test::optimized_target",
        )];

        // (b) guard-inactive member: guard resolves Bool(false), so
        // `members` (not `else_members`) are inactive.
        let guard_id = ValueCellId::new("Clause5", "guard");
        values.insert(
            guard_id.clone(),
            (Value::Bool(false), DeterminacyState::Determined),
        );
        let guard_member_id = seed_cell(
            &mut graph,
            &mut values,
            "Clause5",
            "guard_member",
            CompiledExpr::value_ref(producer_id.clone(), Type::length()),
            Value::Undef,
            DeterminacyState::Undetermined,
        );
        trace_map.insert(
            NodeId::Value(guard_member_id.clone()),
            DependencyTrace {
                reads: vec![producer_id.clone()],
                realization_reads: Vec::new(),
            },
        );
        graph.guarded_groups.push(GuardedGroupInfo {
            guard_cell: guard_id,
            members: vec![guard_member_id.clone()],
            constraints: vec![],
            else_members: vec![],
            else_constraints: vec![],
        });

        let violations = check_no_stale_undef(&graph, &values, &trace_map, &functions);

        assert_eq!(
            violations.len(),
            1,
            "expected ONLY the genuine violation; @optimized cell {optimized_id:?} \
             and guard-inactive member {guard_member_id:?} must be excluded, got \
             {violations:?}"
        );
        assert_eq!(violations[0].cell, genuine_id);
    }

    /// §11 Q2/Q4 UserUndef structural exclusion (RED until step-6): a
    /// `param u = undef` / `let x = undef` cell — `default_expr` is a
    /// `Literal(Value::Undef)`, value `Undef`, ZERO deps — is the one case
    /// clause 4 CANNOT exempt (an empty dep set is vacuously "all resolved").
    /// Without a structural skip, every author-intended undef default would
    /// be misreported as a causeless violation.
    #[test]
    fn undef_literal_default_is_excluded() {
        let mut graph = EvaluationGraph::default();
        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        let mut trace_map: HashMap<NodeId, DependencyTrace> = HashMap::new();

        // A shared, resolved producer so the genuine violation has "all deps
        // resolved" — isolating the undef-literal skip as the only thing
        // under test.
        let producer_id = seed_cell(
            &mut graph,
            &mut values,
            "UndefLiteral",
            "producer",
            CompiledExpr::literal(Value::length(1.0), Type::length()),
            Value::length(1.0),
            DeterminacyState::Determined,
        );

        // The genuine violation: causeless staleness, no exclusion applies.
        let genuine_id = seed_cell(
            &mut graph,
            &mut values,
            "UndefLiteral",
            "genuine",
            CompiledExpr::value_ref(producer_id.clone(), Type::length()),
            Value::Undef,
            DeterminacyState::Undetermined,
        );
        trace_map.insert(
            NodeId::Value(genuine_id.clone()),
            DependencyTrace {
                reads: vec![producer_id.clone()],
                realization_reads: Vec::new(),
            },
        );

        // UserUndef: `param u = undef` shape — default_expr is Literal(Undef),
        // value Undef, ZERO deps (no trace_map entry => empty trace, so
        // clause 4's "every dep resolved" is vacuously true).
        let user_undef_id = seed_cell(
            &mut graph,
            &mut values,
            "UndefLiteral",
            "user_undef",
            CompiledExpr::literal(Value::Undef, Type::length()),
            Value::Undef,
            DeterminacyState::Undetermined,
        );

        let violations = check_no_stale_undef(&graph, &values, &trace_map, &[]);

        assert_eq!(
            violations.len(),
            1,
            "expected ONLY the genuine violation; the undef-literal (UserUndef) \
             cell {user_undef_id:?} must be excluded, got {violations:?}"
        );
        assert_eq!(violations[0].cell, genuine_id);
    }

    /// Boundary-pinning test (code-review amendment, task 4952): documents
    /// `value_is_or_contains_undef`'s deliberate choice NOT to recurse into
    /// `StructureInstance` (nor `Tensor`/`Point`/`Frame`) — see its doc
    /// comment. A producer that resolves to a `StructureInstance` with one
    /// Undef field (e.g. a legitimately-absent optional param, propagated
    /// Kleene-style into a struct field rather than surfacing at the
    /// top level) still reads as "resolved" by clause 4, because only the
    /// TOP-LEVEL value is checked against `Value::Undef`. So a consumer of
    /// that producer which is itself genuinely Undef is NOT exempted here
    /// and IS reported — pinning that as an intentional, tested outcome
    /// rather than an incidental one. No corpus fixture currently forces
    /// this shape (hence no false positive observed in the broad sweep,
    /// step 9/10), but if `value_is_or_contains_undef` is ever extended to
    /// composite variants, this test should fail loudly and be updated
    /// deliberately rather than the boundary silently shifting.
    #[test]
    fn structure_instance_with_undef_field_is_not_exempted() {
        let mut graph = EvaluationGraph::default();
        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        let mut trace_map: HashMap<NodeId, DependencyTrace> = HashMap::new();

        let mut fields = PersistentMap::new();
        fields.insert("gap".to_string(), Value::Undef);
        let producer_id = seed_cell(
            &mut graph,
            &mut values,
            "StructBoundary",
            "producer",
            CompiledExpr::literal(Value::length(1.0), Type::length()),
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_id: StructureTypeId(0),
                type_name: "Widget".to_string(),
                version: 1,
                fields,
            })),
            DeterminacyState::Determined,
        );

        // Consumer reads the producer and is itself Undef — the shape a
        // recursing exemption would treat as exempt (propagated through a
        // struct field), but the current, documented boundary does not.
        let consumer_id = seed_cell(
            &mut graph,
            &mut values,
            "StructBoundary",
            "consumer",
            CompiledExpr::value_ref(producer_id.clone(), Type::length()),
            Value::Undef,
            DeterminacyState::Undetermined,
        );
        trace_map.insert(
            NodeId::Value(consumer_id.clone()),
            DependencyTrace {
                reads: vec![producer_id.clone()],
                realization_reads: Vec::new(),
            },
        );

        let violations = check_no_stale_undef(&graph, &values, &trace_map, &[]);

        assert_eq!(
            violations.len(),
            1,
            "pinning the documented StructureInstance boundary: a producer \
             whose value is a StructureInstance with an Undef field reads as \
             top-level resolved (not recursed into), so consumer \
             {consumer_id:?} is NOT exempted by clause 4 and IS reported — \
             got {violations:?}"
        );
        assert_eq!(violations[0].cell, consumer_id);
    }

    // ── Clause 8: derived structural build-only classifier (task γ / #4954) ───

    /// A `FunctionCall` node named `name` whose args are `ValueRef`s of the
    /// given `arg_types` (the predicate keys on arg *types*, not values).
    fn fn_call(name: &str, arg_types: &[Type]) -> CompiledExpr {
        let args = arg_types
            .iter()
            .enumerate()
            .map(|(i, ty)| {
                CompiledExpr::value_ref(ValueCellId::new("T", format!("a{i}")), ty.clone())
            })
            .collect();
        CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: name.to_string(),
                    qualified_name: format!("std::{name}"),
                },
                args,
            },
            result_type: Type::Bool,
            content_hash: ContentHash::of_str(name),
        }
    }

    /// A `MethodCall` node `<receiver>.<method>()` whose receiver is a
    /// `ValueRef` of `recv_type`.
    fn method_call(recv_type: Type, method: &str) -> CompiledExpr {
        CompiledExpr {
            kind: CompiledExprKind::MethodCall {
                object: Box::new(CompiledExpr::value_ref(
                    ValueCellId::new("T", "recv"),
                    recv_type,
                )),
                method: method.to_string(),
                args: vec![],
            },
            result_type: Type::Bool,
            content_hash: ContentHash::of_str(method),
        }
    }

    /// Pins clause 8's two-mechanism build-only classifier (task γ / #4954):
    /// (2) the DERIVED STRUCTURAL rule catches geometry/selector-arg consumers
    /// that are not eval-wired — closing the open-ended class the old name
    /// lists missed; (3) the REGISTRY path still catches build-only query
    /// families that consume abstracted handles (no geometry-typed arg). Also
    /// pins that the α net stays TIGHT for eval-wired ctors and scalar ops.
    #[test]
    fn clause8_build_only_classifier() {
        let geom = Type::Geometry;
        let sel = Type::Selector(reify_core::ty::SelectorKind::Face);
        // A deliberately NON-geometry arg type, to prove a classification is
        // NOT relying on the structural "consumes geometry/selector" rule.
        let scalar = Type::length();

        // (2) STRUCTURAL — geometry-arg consumers, keyed by arg type. These
        // close the class the old clause-8 name lists kept missing.
        for name in [
            // query family that takes a geometry arg
            "volume",
            "area",
            "centroid",
            "bounding_box",
            "adjacent_faces",
            "normal",
            "closest_point",
            "shared_edges",
            "siblings_of_face",
            "ancestor_faces_of_edge",
            "length",
            "perimeter",
            "curvature",
            "center_of_mass",
            "moment_of_inertia",
            "distance",
            "contains",
            "intersects",
            "geo_equiv",
            "is_on",
            "angle_between_surfaces",
            // named-leaf ctors (the interim WIP name list — now structural)
            "face",
            "edge",
            "solid_body",
            "vertex",
            // GD&T + set-ops + conformance the name lists never covered
            "max_deviation",
            "split",
            "feature",
            "is_watertight",
            "is_manifold",
            "is_orientable",
            "is_closed",
            "is_connected",
            "is_bounded",
        ] {
            assert!(
                is_build_only_dispatch_call(&fn_call(name, std::slice::from_ref(&geom))),
                "{name}(geometry) must classify build-only (structural rule)"
            );
        }
        // (2) STRUCTURAL — composition selectors, keyed by selector-typed args.
        for name in ["union", "intersect", "difference"] {
            assert!(
                is_build_only_dispatch_call(&fn_call(name, &[sel.clone(), sel.clone()])),
                "{name}(selector, selector) must classify build-only (structural)"
            );
        }

        // (3) REGISTRY — build-only query families that consume ABSTRACTED
        // handles (direction vectors, mechanism snapshots, body-ids), NOT a
        // geometry-typed arg. Tested with a non-geometry arg on purpose: the
        // structural rule cannot see them, so the registry path must.
        for name in [
            "angle",           // over direction vectors
            "body_mass_props", // dynamics
            "interferes",      // kinematic — over a mechanism snapshot
            "interferes_with",
            "min_clearance",
        ] {
            assert!(
                is_build_only_dispatch_call(&fn_call(name, std::slice::from_ref(&scalar))),
                "{name}(non-geometry) must classify build-only (registry)"
            );
        }

        // NET STAYS TIGHT: kernel-free symbolic-eval-wired leaf ctors are NOT
        // build-only — a well-formed one still Undef is a genuine arity/
        // scheduling bug that must remain a violation, not be exempted.
        for name in [
            "faces",
            "edges",
            "faces_by_normal",
            "faces_by_area",
            "edges_by_length",
            "vertices",
            "mid_surface",
            "created_by_feature",
        ] {
            assert!(
                !is_build_only_dispatch_call(&fn_call(name, std::slice::from_ref(&geom))),
                "{name}(geometry) is eval-wired — must NOT be exempted"
            );
        }

        // Non-geometry, non-registry calls are never build-only (α net for
        // scalar ops — a real stale Undef here must still be reported).
        assert!(
            !is_build_only_dispatch_call(&fn_call("sqrt", std::slice::from_ref(&scalar))),
            "a scalar op with no geometry/selector arg must NOT be exempted"
        );
        assert!(
            !is_build_only_dispatch_call(&fn_call("single", &[])),
            "a zero-arg non-registry call consumes nothing — not build-only"
        );

        // Feature→datum projection: a MethodCall on a geometry/selector receiver
        // is build-only (the narrow, receiver-type-gated structural node-kind).
        assert!(
            is_build_only_dispatch_call(&method_call(geom.clone(), "axis")),
            "feature.axis on a geometry receiver must classify build-only"
        );
        // A pure datum→datum projection (non-geometry receiver) is NOT build-only.
        assert!(
            !is_build_only_dispatch_call(&method_call(scalar.clone(), "dir")),
            "a method call on a non-geometry receiver must NOT be exempted"
        );
    }
}
