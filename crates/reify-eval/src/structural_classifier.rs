//! Stage A of the mesh-morphing pipeline — design-tree structural classifier.
//!
//! Implements the pre-flight classifier described in
//! `docs/prds/v0_3/mesh-morphing.md` (Stage A, lines 30–37). This is
//! "Stage A" of the morphing pipeline: before any geometry-kernel work it
//! inspects the design-tree (evaluation graph + runtime value map) and
//! decides whether the parameter edit is *eligible* for mesh morphing.
//!
//! Three public entry points:
//!
//! * [`realization_graph_shape_hash`] — hashes the feature DAG ignoring
//!   runtime leaf parameter values.
//! * [`classify_cell`] — classifies one value-cell as
//!   [`ParameterClass::Dimensional`] or [`ParameterClass::Structural`],
//!   **in isolation**. It does NOT answer the eligibility question: Stage
//!   A's own per-cell predicate is the private `stage_a_cell_vetoes`,
//!   which scopes the type whitelist to leaves.
//! * [`stage_a_eligible`] — the top-level predicate: `true` iff (a) the
//!   graph shape is unchanged, (b) every differing leaf is dimensional,
//!   and (c) no feature was added, removed, or reordered. Its
//!   "# The value-diff walk is LEAF-SCOPED" note is the single canonical
//!   statement of the composed contract; every other note in this file
//!   points at it rather than restating it.
//!
//! ## Purity
//!
//! This module is pure Rust and does **not** call any geometry kernel.
//! It operates solely on [`EvaluationGraph`] and [`reify_types::ValueMap`].

use std::collections::HashSet;

use reify_compiler::ValueCellKind;
use reify_core::{ContentHash, Type, ValueCellId};
use reify_ir::ValueMap;

use crate::graph::EvaluationGraph;

// ── Public types ──────────────────────────────────────────────────────────────

/// Classification of a design-tree value cell for Stage A mesh-morphing
/// eligibility.
///
/// The conservative default is `Structural` — anything that is not clearly a
/// dimensioned scalar, real, or integer, or a `Type::Geometry` realization
/// reference (the task-6635 exception), is treated as structural. This biases
/// Stage A toward false-rejection (one extra remesh) rather than
/// false-eligibility (a topology-changing edit slipping through to Stage B).
///
/// The canonical rationale for the whitelist — including the `Type::Geometry`
/// exception and its measured evidence — lives in ONE place: the
/// `## Type::Geometry and Rule 4` note on [`classify_cell`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterClass {
    /// The cell holds a quantity whose change cannot by itself affect feature
    /// topology: `Type::Scalar { .. }`, `Type::dimensionless_scalar()`,
    /// `Type::Int`, and `Type::Geometry`. All of these remain subject to the
    /// `structure_controlling` and `collection_subs` overrides in
    /// [`classify_cell`].
    Dimensional,
    /// The cell controls topology — feature suppression toggles, pattern
    /// counts, enum-typed mode selectors, or any type not whitelisted as
    /// Dimensional. A differing Structural cell makes the edit Stage-A
    /// ineligible.
    Structural,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Compute the shape hash of a realization graph.
///
/// This is Stage A's *shape primitive* (PRD line 33). It produces a
/// deterministic hash of the feature DAG structure — value cells, constraints,
/// realizations, resolutions, guarded groups, connections, and auto-type
/// substitution — **excluding** the runtime [`reify_types::ValueMap`].
///
/// The runtime value map is precisely what the PRD means by "leaf parameter
/// values": it lives in the engine's snapshot, not in the graph. Two graphs
/// with identical shape hashes have identical structural topology, so a
/// differing value map reflects only parameter-value changes (which Stage A
/// then audits cell-by-cell via [`classify_cell`]).
///
/// # Implementation
///
/// Delegates to [`EvaluationGraph::topology_fingerprint`]. Using the existing
/// fingerprint ensures Stage A and the realization cache key over the same
/// hash, so a future fingerprint bucket addition automatically applies to
/// Stage A too.
pub fn realization_graph_shape_hash(graph: &EvaluationGraph) -> ContentHash {
    graph.topology_fingerprint()
}

/// Classify a single value cell as [`ParameterClass::Dimensional`] or
/// [`ParameterClass::Structural`].
///
/// Resolution order (first matching rule wins):
///
/// 1. **Cell absent** — cell not in `graph.value_cells` → `Structural`
///    (conservative: unknown cells are assumed topology-controlling).
/// 2. **`structure_controlling`** — cell is in `graph.structure_controlling`
///    → `Structural`. This catches feature-suppression toggles (guard cells
///    registered by `EvaluationGraph::from_templates`'s guard-group
///    construction path).
/// 3. **Collection count** — cell appears as `count_cell` of any entry in
///    `graph.collection_subs` → `Structural`. Pattern/array counts have
///    `Type::Int` but drive topology via the collection-elaboration path in
///    `EvaluationGraph::from_templates`.
///
/// 3b. **Keyed-sub count** *(speculative, task 3932 δ)* — cell appears as
///    `count_cell` of any entry in `graph.keyed_subs` → `Structural`. Mirrors
///    Rule 3 for `Keyed<Structure>`. Currently unreachable in the real pipeline
///    because entity.rs does not backfill `count_cell` for keyed subs. Correct
///    by construction and unit-tested; wiring the backfill path activates it.
/// 4. **Type dispatch** — `Type::Scalar { .. } | Type::dimensionless_scalar() | Type::Int`
///    → `Dimensional`; `Type::Geometry` → `Dimensional` (task 6635, see below);
///    everything else → `Structural`.
///
/// ## Leaf scoping lives in the walk, not here
///
/// This function classifies a cell **in isolation** and applies Rule 4 to a
/// cell of ANY [`reify_compiler::ValueCellKind`], so a derived (`Let`) cell of
/// a non-whitelisted type returns `Structural` here even though it does not
/// veto a tick. The composed Stage A contract is therefore NOT "run
/// `classify_cell` over every differing cell" — Stage A's own per-cell
/// predicate is the private `stage_a_cell_vetoes`. Read [`stage_a_eligible`]'s
/// "# The value-diff walk is LEAF-SCOPED" note for that contract.
///
/// The divergence is deliberate and is pinned by
/// `tests::classify_cell_derived_let_non_whitelisted_type_stays_structural`.
/// `classify_cell` is the crate's per-cell type/override classifier and has no
/// production caller today; keeping it kind-agnostic keeps "is this type
/// Dimensional?" separable from "does this cell veto a tick?", which is what a
/// per-cell diagnostic needs in order to NAME a vetoing cell. Do not leaf-scope
/// it to match the walk.
///
/// ## Type::Geometry and Rule 4
///
/// A `Type::Geometry` cell's value is *produced by realization* — a handle to a
/// computed B-rep. No production caller edits such a cell directly;
/// `Engine::edit_param` is driven at the numeric leaves. Its value therefore
/// necessarily changes whenever **any** upstream cell changes: a dimensional
/// leaf tick recomputes it just as surely as a topology-changing one does.
/// Classifying it `Structural` via Rule
/// 4's conservative `_` default meant [`stage_a_eligible`]'s union-walk hit the
/// derived geometry cell on *every* real edit and rejected, so the engine's
/// morph arm was 100% dormant in production — measured as
/// `ineligible_structural_change: 1` on every tick, with the morph arm never
/// once reaching Stage B.
///
/// This contradicted the PRD. `docs/prds/v0_3/mesh-morphing.md` line 33 scopes
/// Stage A to **leaf** parameters ("classify each leaf parameter ... the only
/// differing leaves are dimensional"), and line 34 makes Stage B the gate for
/// value-driven topology changes ("Even when Stage A passes, continuous
/// parameter changes can cross topology-changing thresholds").
///
/// ### The arm keys on the TYPE, not on derivedness
///
/// Rule 4 matches `Type::Geometry` regardless of `ValueCellNode::kind`. Most
/// `Type::Geometry` cells are `ValueCellKind::Let` — `reify-compiler`'s
/// `entity.rs` geometry-*let* path — but a solid-typed param with a
/// geometry-call default (`param body: Geometry = box(...)`) is registered
/// `Type::Geometry` + `ValueCellKind::Param` by the same file's param path, and
/// is therefore Dimensional here too. That is deliberate, not an oversight:
/// entity.rs itself treats such a param "symmetrically to geometry lets", in
/// practice its value within a fixed graph diverges only via upstream recompute
/// (`Engine::edit_param` is driven at the numeric leaves, not at the realization
/// handle — but see "convention, not invariant" below), and narrowing the arm to
/// `kind == Let` would re-introduce the every-tick veto for any design that
/// authors its body as a param. Stage B is the net either way — see (iii) below.
/// Do not read this arm as "derived cells are Dimensional"; read it as "a
/// `Type::Geometry` value is realization output, which carries no independent
/// structural signal".
///
/// That `param body: Geometry = box(...)` case is why this arm stays
/// load-bearing under leaf scoping: such a cell IS a leaf, so
/// [`stage_a_eligible`]'s walk still runs Rule 4 over it and only the
/// `Type::Geometry` entry keeps it Dimensional.
///
/// ### Convention, not invariant: nothing forbids editing a Geometry cell
///
/// "No production caller edits a Geometry-typed cell directly" is an observed
/// convention, NOT something the type system or the engine enforces. Verified:
/// `Engine::edit_param` (`engine_edit.rs`) validates an override only for
/// type-kind/dimension compatibility via `validate_param_override`, so an API
/// caller *can* assign a different `Value::GeometryHandle` to a `Type::Geometry`
/// param cell. After this relaxation Stage A admits such a whole-body swap as
/// "dimensional", leaving Stage B and the morph quality gate as the only nets.
/// The residual risk is low — a genuinely different B-rep will almost always
/// fail the naming bijection — but it is a real gap, so reason from the
/// enforceable claim (Stage B is the net), not from the convention.
///
/// Nothing is lost by the relaxation:
///
/// (i)   [`stage_a_eligible`]'s shape-hash gate still catches every feature
///       added, removed, or reordered — that is a graph-shape change, not a
///       value change, and it short-circuits before any per-cell work.
/// (ii)  Any structure-controlling, collection-count, or keyed-count leaf
///       feeding the geometry is still classified `Structural`, either by Rules
///       2/3/3b above or by Rule 4 on its OWN cell (an `Enum` mode selector, a
///       `Bool` suppression toggle). The relaxation widens the whitelist by
///       exactly one type; it does not make the cells *behind* the geometry
///       invisible.
/// (iii) Stage B's persistent-naming bijection check remains the safety net for
///       a dimensional tick that crosses a topology threshold. What is MEASURED
///       about the `cut_z` structural fixture in
///       `reify-eval/tests/morph_arm_e2e.rs` is the Stage A half: after this
///       change it no longer vetoes (`ineligible_structural_change: 0`) and the
///       reject moves downstream to Stage B. Read no more than that into it —
///       the bucket it lands in is `ineligible_naming_error: 1`, i.e. Stage B
///       could not EVALUATE the bijection on that fixture's boolean-cut B-rep
///       (`NamingLayerErrorReason` is only `Imported`/`Partial`), so it is not a
///       demonstration that the topology-threshold net fires. That
///       demonstration is the in-crate fixture test
///       `reify_mesh_morph::eligibility::tests::`
///       `morph_eligible_stage_a_admits_geometry_diff_stage_b_rejects_count_mismatch`,
///       where Stage A admits a differing Geometry cell and Stage B rejects with
///       a real `BijectionFailure::CountMismatch`.
///
/// Placement is load-bearing: the `Type::Geometry` arm lives inside Rule 4,
/// *after* the Rule 1/2/3/3b early-returns, so a structure-controlling or
/// count-cell Geometry cell still classifies `Structural`. Do not hoist it to
/// an early `if node.cell_type == Type::Geometry { return Dimensional }` — the
/// mutation guards `classify_cell_geometry_in_structure_controlling_returns_structural`,
/// `classify_cell_geometry_as_collection_count_returns_structural` and
/// `stage_a_eligible_structure_controlling_geometry_diff_returns_false` exist to
/// catch exactly that.
///
/// ### PARTIALLY-OPEN GAP — `List<Geometry>` (#7016)
///
/// The task-6635 whitelist relaxation covers the BARE `Type::Geometry` variant
/// only. Handle-LIST cells — `Type::List(Box::new(Type::Geometry))` — still
/// fall into Rule 4's `_ => Structural` default, so `classify_cell` still
/// reports one as `Structural`.
///
/// [`stage_a_eligible`]'s walk consults Rule 4 only for LEAF cells, so the
/// overwhelmingly common `List<Geometry>` shape — `let faces =
/// adjacent_faces(...)`, a resolved selector, all `ValueCellKind::Let` — does
/// NOT veto a tick. What #7016 still owns is:
///
/// * a `Param`-kind `List<Geometry>` cell, which IS a leaf and so still meets
///   Rule 4's whitelist and still vetoes; and
/// * the undecided design question below — whether a change to the list's
///   LENGTH should stay Structural.
///
/// Verified reach (2026-08-29): `adjacent_faces`, `shared_edges`,
/// `siblings_of_face`, `ancestor_faces_of_edge` and `split` are typed
/// `List<Geometry>` in `reify-compiler/src/units.rs`, and so is any `Selector`
/// cell wrapped in `ResolveSelector` (`single(...)`, index access). Their values
/// are `Value::List`s of `Value::GeometryHandle` built by
/// `topology_selectors::make_sub_handle`, whose `upstream_values_hash` is
/// composed from the PARENT's hash; since `Value::GeometryHandle`'s `PartialEq`
/// keys on `(realization_ref, upstream_values_hash)`, those cells differ on
/// every tick. Bare `Type::Selector` cells are NOT affected — `SelectorValue`
/// equality is content-hash based and excludes `kernel_handle`.
///
/// Deliberately left for a follow-up rather than folded in here, because it is
/// not a pure restatement of the bare-Geometry argument: a `List<Geometry>`
/// carries one signal a bare handle does not — its LENGTH — and whether a
/// length change should stay Structural (finer rule) or defer to Stage B like
/// everything else (widen `classify_by_type` to
/// `Type::List(inner) if **inner == Type::Geometry`) is a design decision that
/// wants its own measured RED→GREEN, not a drive-by amendment. Leaf scoping is
/// orthogonal to that question. Do not read this note's absence of a fix as
/// evidence the question was overlooked.
pub fn classify_cell(graph: &EvaluationGraph, cell_id: &ValueCellId) -> ParameterClass {
    // Rule 1: missing cell → Structural.
    let Some(node) = graph.value_cells.get(cell_id) else {
        return ParameterClass::Structural;
    };

    // Rule 2: structure-controlling override (feature-suppression toggles,
    // guard cells). Checked before type dispatch so a Bool guard that also
    // happens to be Scalar-shaped is still classified Structural.
    if graph.structure_controlling.contains(cell_id) {
        return ParameterClass::Structural;
    }

    // Rule 3: collection-count override. Pattern/array counts have Type::Int
    // (which would otherwise be Dimensional) but structurally drive collection
    // elaboration (see `EvaluationGraph::collection_subs` and the
    // collection-count wiring in `EvaluationGraph::from_templates`).
    if graph
        .collection_subs
        .iter()
        .any(|sub| &sub.count_cell == cell_id)
    {
        return ParameterClass::Structural;
    }

    // Rule 3b: keyed-sub count-cell override (task 3932 δ). A keyed sub's
    // (optional) count cell has Type::Int but controls Keyed<Structure>
    // elaboration — same structural role as a positional collection count.
    // Recognises `Keyed<Structure>` alongside `List<Structure>` for
    // count-controlled re-elaboration.
    //
    // SPECULATIVE / FORWARD-LOOKING (verified 2026-06-27): entity.rs currently
    // leaves count_cell:None for keyed subs even when a `<sub>.count == N`
    // constraint is present — the reconciliation backfill is not gated on
    // is_collection in the code, but in practice KeyedSubInfo.count_cell is
    // always None in real compiled modules. Rule 3b is structurally correct and
    // unit-tested (see classify_cell_keyed_sub_count_returns_structural), but
    // is unreachable via the real pipeline until the backfill path is wired.
    // See from_templates_keyed_sub_count_cell_is_none_backfill_not_yet_wired
    // for the canary test that will reveal when the backfill becomes active.
    if graph
        .keyed_subs
        .iter()
        .any(|sub| sub.count_cell.as_ref() == Some(cell_id))
    {
        return ParameterClass::Structural;
    }

    // Rule 4: type-based dispatch (shared with `stage_a_cell_vetoes`).
    classify_by_type(&node.cell_type)
}

/// Rule 4 in isolation: the type-only half of the classification, with no
/// graph context.
///
/// Extracted so the whitelist itself cannot drift between its two callers.
/// They consult it at different SCOPES — [`classify_cell`] for a cell of any
/// [`reify_compiler::ValueCellKind`], [`stage_a_cell_vetoes`] only for LEAF
/// cells (`Param` / `Auto`) — but whichever one asks gets the same answer, so
/// widening the whitelist stays a one-site edit. That difference in scope is
/// also why "is this type on the whitelist?", which is all this function
/// answers, is not the same question as "does a differing cell of this type
/// veto a tick?" — see [`stage_a_eligible`]'s "# The value-diff walk is
/// LEAF-SCOPED".
///
/// `Type::Geometry` is on the whitelist as of task 6635; the `_ => Structural`
/// conservative default is unchanged — note in particular that
/// `Type::List(Type::Geometry)` is still caught by it, which is a KNOWN,
/// partially-open dormancy gap. Rationale, measured evidence and that gap: the
/// `## Type::Geometry and Rule 4` note on [`classify_cell`].
///
/// This is deliberately NOT a public entry point: callers must go through
/// [`classify_cell`], which applies the `structure_controlling` /
/// `collection_subs` / `keyed_subs` overrides FIRST. Rule 4 alone would
/// misclassify a structure-controlling or count cell.
fn classify_by_type(cell_type: &Type) -> ParameterClass {
    match cell_type {
        Type::Scalar { .. } | Type::Int | Type::Geometry => ParameterClass::Dimensional,
        _ => ParameterClass::Structural,
    }
}

/// Does this differing cell VETO Stage-A eligibility?
///
/// The classifier [`stage_a_eligible`]'s value-diff walk actually calls. It
/// accepts a pre-computed set of count-cell IDs (built once before the per-cell
/// loop) so the walk runs in O(N) rather than O(N·C) — N differing cells × C
/// collection/keyed subs.
///
/// It differs from the public [`classify_cell`] in TWO ways, and only one of
/// them is a performance detail:
///
/// * Rules 3/3b are an O(1) set lookup instead of a linear scan (performance).
/// * Rule 4 is **LEAF-SCOPED** — consulted only for `ValueCellKind::Param` and
///   `ValueCellKind::Auto`, never for a derived `Let` cell (task 6643,
///   semantics).
///
/// # Rationale lives in ONE place
///
/// All of it — why Rule 4 is leaf-scoped, why Rules 1/2/3/3b must stay
/// kind-agnostic, why `Auto` counts as a leaf, the Stage B backstop, the
/// soundness of reading `kind` from `new_graph` — is the "# The value-diff walk
/// is LEAF-SCOPED" note on [`stage_a_eligible`], the public entry point this
/// predicate implements. Do not restate it here.
///
/// Two implementation facts that belong with the code rather than the contract:
///
/// * The Rule 1/2/3/3b early-returns are ORDERED BEFORE the `node.kind` match.
///   That ordering is what makes those rules kind-agnostic *by construction*
///   rather than by convention; do not reorder it.
/// * The `match` on `node.kind` is deliberately EXHAUSTIVE — no `_` arm — so a
///   future `ValueCellKind` variant forces an explicit leaf/derived ruling here
///   rather than silently defaulting.
fn stage_a_cell_vetoes(
    graph: &EvaluationGraph,
    cell_id: &ValueCellId,
    count_cells: &HashSet<&ValueCellId>,
) -> bool {
    // Rule 1: missing cell → veto (conservative). With no ValueCellNode there
    // is no `kind`, so leaf scoping cannot apply.
    let Some(node) = graph.value_cells.get(cell_id) else {
        return true;
    };

    // Rules 2 / 3 / 3b: structure-controlling and count-cell overrides.
    // KIND-AGNOSTIC BY CONSTRUCTION — evaluated before the kind match below.
    // `count_cells` unions the collection-sub and keyed-sub count cells.
    if graph.structure_controlling.contains(cell_id) || count_cells.contains(cell_id) {
        return true;
    }

    // Rule 4: type-based dispatch, LEAF-SCOPED. Uses the SAME
    // [`classify_by_type`] the public `classify_cell` calls, so the whitelist
    // cannot drift between the two.
    match node.kind {
        ValueCellKind::Param | ValueCellKind::Auto { .. } => {
            classify_by_type(&node.cell_type) == ParameterClass::Structural
        }
        ValueCellKind::Let => false,
    }
}

/// Stage A top-level eligibility predicate.
///
/// Returns `true` iff the parameter edit from `(old_graph, old_values)` to
/// `(new_graph, new_values)` is eligible for mesh morphing:
///
/// 1. **Shape gate** — `realization_graph_shape_hash(old_graph) ==
///    realization_graph_shape_hash(new_graph)`. This covers PRD criteria (a)
///    and (c): graph structure unchanged, no features added/removed/reordered.
///    Short-circuits before any per-cell work if shapes differ.
///
/// 2. **Value diff** — walk the union of cell IDs in `old_values` and
///    `new_values`. For each cell where the old and new values differ (or the
///    cell is present on only one side), ask the private `stage_a_cell_vetoes`
///    predicate against `new_graph` (which equals `old_graph` structurally
///    after the shape gate passes). The first vetoing cell makes the edit
///    ineligible → `false`.
///
/// # The value-diff walk is LEAF-SCOPED (task 6643)
///
/// The walk does **not** simply run [`classify_cell`] over every differing
/// cell. Its rules split into two groups, and the split is the whole contract:
///
/// * **Rules 1/2/3/3b apply to EVERY differing cell, regardless of
///   [`reify_compiler::ValueCellKind`]** — an unknown cell, a
///   `structure_controlling` cell, a collection count cell, a keyed-sub count
///   cell. Kind-agnostic *by construction*: they are evaluated before the kind
///   match, not by convention.
/// * **Rule 4's type whitelist is consulted only for LEAF cells** —
///   `ValueCellKind::Param` and `ValueCellKind::Auto { .. }`. This is what PRD
///   `docs/prds/v0_3/mesh-morphing.md` line 33 actually says: "classify each
///   **leaf parameter** … the only differing **leaves** are dimensional". A
///   derived (`Let`) cell's value is a pure function of its upstream leaves, so
///   it necessarily changes on every dimensional tick; classifying it by type
///   vetoed 100% of ticks for any design holding one non-whitelisted derived
///   cell. Task 6635 closed that for bare `Type::Geometry` by widening the
///   whitelist; task 6643 closed the whole class — `StructureRef`,
///   `List<Geometry>`, `Bool`, `String`, `Enum`, … — by scoping the rule.
///
/// `Auto { .. }` is classified as a LEAF, not as derived: an `auto` param is a
/// *declared* leaf whose value the constraint solver supplies, not a derived
/// expression, so the whitelist still applies to it in full.
///
/// ## Why this is not "skip all `Let` cells" — the evidence
///
/// The compiler's block/where `__guard_N` feature-suppression cells are
/// constructed as `kind: ValueCellKind::Let` + `cell_type: Type::Bool`
/// (`crates/reify-eval/src/graph.rs:599-605`; allocated in
/// `reify-compiler/src/guards.rs:297,667,700`) and inserted into
/// `structure_controlling` at the same site. A naive "skip all `Let` cells"
/// would therefore have made every guarded design's suppression toggles
/// invisible to Stage A — a real regression, reachable in every guarded design,
/// not a hypothetical one. Rule 2 firing before the kind match is what prevents
/// it; likewise a `let n = base + extra` pattern count stays Structural via
/// Rules 3/3b. Both are pinned by
/// `tests::stage_a_eligible_derived_let_guard_cell_diff_returns_false` and its
/// two count-cell siblings; that the production compiler really routes a
/// `where` guard into `structure_controlling` — the seam those hand-built
/// graphs can only assume — is pinned end-to-end by `tests/morph_arm_e2e.rs`'s
/// `stage_a_vetoes_dimensional_tick_that_flips_a_compiled_guard_cell`.
///
/// Beyond those overrides, Stage B's persistent-naming bijection remains the
/// net for a dimensional tick that crosses a topology threshold — PRD line 34
/// assigns it exactly that role, and
/// `reify_mesh_morph::eligibility::tests::morph_eligible_stage_a_admits_geometry_diff_stage_b_rejects_count_mismatch`
/// demonstrates the composition.
///
/// ## Soundness of reading `kind` from `new_graph`
///
/// The walk already read `node.cell_type` from `new_graph` on the stated
/// grounds that the two graphs are structurally identical once the shape gate
/// passes; reading `node.kind` from the same node adds no new assumption.
///
/// Recorded caveat, PRE-EXISTING and unchanged by task 6643:
/// `ValueCellNode::content_hash` is `id_hash.combine(expr_hash)` — it covers
/// neither `kind` nor `cell_type` — so the shape gate would not by itself catch
/// a cell whose kind or type flipped between the two graphs.
///
/// # Why four arguments?
///
/// The PRD (line 33) writes `stage_a_eligible(old_graph, new_graph)` as
/// shorthand, but runtime values live in [`ValueMap`] (maintained by
/// `Engine::edit_param`), not in the graph. Without both ValueMaps there is no
/// way to detect which cells changed. See design decision in plan.json.
pub fn stage_a_eligible(
    old_graph: &EvaluationGraph,
    new_graph: &EvaluationGraph,
    old_values: &ValueMap,
    new_values: &ValueMap,
) -> bool {
    // 1. Shape gate (PRD criterion a + c). Cheap; short-circuits feature
    //    add/remove/reorder before any per-cell classification work.
    if realization_graph_shape_hash(old_graph) != realization_graph_shape_hash(new_graph) {
        return false;
    }

    // 2. Value diff over the union of cell IDs from both maps.
    //
    // Precompute the count-cell set once so the per-cell loop runs in O(N)
    // rather than O(N·C) (N differing cells × C collection/keyed subs).
    // new_graph == old_graph structurally after the shape gate above.
    let mut count_cells: HashSet<&ValueCellId> = new_graph
        .collection_subs
        .iter()
        .map(|s| &s.count_cell)
        .collect();
    // Also include keyed-sub count cells (task 3932 δ): the O(1) lookup path
    // must cover Keyed<Structure> count cells alongside positional ones.
    count_cells.extend(
        new_graph
            .keyed_subs
            .iter()
            .filter_map(|s| s.count_cell.as_ref()),
    );

    let mut union_ids: HashSet<&ValueCellId> = HashSet::new();
    union_ids.extend(old_values.iter().map(|(id, _)| id));
    union_ids.extend(new_values.iter().map(|(id, _)| id));

    for id in union_ids {
        // Use get_or_undef so that Some(Value::Undef) and None compare equal —
        // the engine treats an absent entry as Undef (see ValueMap::get_or_undef
        // and EvalResult::values doc). Without this alignment, a cell that flips
        // between Some(Undef) and None would look like a change and could cause
        // a spurious structural rejection.
        if old_values.get_or_undef(id) == new_values.get_or_undef(id) {
            continue;
        }
        // Values differ (including Some vs None for non-Undef values).
        // Use new_graph for classification — structurally identical to
        // old_graph after the shape gate.
        if stage_a_cell_vetoes(new_graph, id, &count_cells) {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{ContentHash, Type, ValueCellId};

    use reify_ir::MemberKey;

    use crate::graph::{CollectionSubInfo, EvaluationGraph, KeyedSubInfo, ValueCellNode};

    // ── Minimal-graph builder helpers ─────────────────────────────────────

    /// Build an `EvaluationGraph` with a single value cell of the given type.
    fn graph_with_cell(id: &ValueCellId, cell_type: Type) -> EvaluationGraph {
        let mut g = EvaluationGraph::default();
        g.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Param,
                cell_type,
                default_expr: None,
                content_hash: ContentHash::of_str(&format!("{}", id)),
            },
        );
        g
    }

    /// Build an `EvaluationGraph` with a single value cell of the given type
    /// AND the given [`ValueCellKind`].
    ///
    /// A SIBLING of [`graph_with_cell`], not a widening of it: that helper
    /// hardcodes `ValueCellKind::Param` and 30+ tests in this module depend on
    /// that, so it must not change. Task 6643 made `kind` load-bearing inside
    /// [`stage_a_eligible`]'s value-diff walk — Rule 4's type whitelist is
    /// consulted only for LEAF cells (`Param` / `Auto`) — so every test about
    /// that scoping has to pin the kind explicitly rather than inherit it.
    fn graph_with_cell_kind(
        id: &ValueCellId,
        cell_type: Type,
        kind: ValueCellKind,
    ) -> EvaluationGraph {
        let mut g = EvaluationGraph::default();
        g.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind,
                cell_type,
                default_expr: None,
                content_hash: ContentHash::of_str(&format!("{}", id)),
            },
        );
        g
    }

    /// The dimensional `Param` LEAF every task-6643 [`stage_a_eligible`] test
    /// ticks: `MorphDerivedLet.width`, mirroring the leaf of the
    /// `crates/reify-eval/tests/fixtures/morph_derived_let.ri` fixture.
    fn dim_leaf_id() -> ValueCellId {
        ValueCellId::new("MorphDerivedLet", "width")
    }

    /// Build a graph holding the dimensional `Param` leaf [`dim_leaf_id`]
    /// (`Type::length()`) alongside ONE caller-specified cell.
    ///
    /// Every task-6643 [`stage_a_eligible`] assertion needs a genuine
    /// dimensional leaf tick beside the second cell's diff. Without it the test
    /// would not model a real edit (`Engine::edit_param` is driven at the
    /// numeric leaves), and a `true` result could not distinguish "the second
    /// cell's diff was admitted" from "nothing differed at all".
    fn graph_with_dim_leaf_and(
        other_id: &ValueCellId,
        other_type: Type,
        other_kind: ValueCellKind,
    ) -> EvaluationGraph {
        let mut g = graph_with_cell_kind(&dim_leaf_id(), Type::length(), ValueCellKind::Param);
        g.value_cells.insert(
            other_id.clone(),
            ValueCellNode {
                id: other_id.clone(),
                kind: other_kind,
                cell_type: other_type,
                default_expr: None,
                content_hash: ContentHash::of_str(&format!("{}", other_id)),
            },
        );
        g
    }

    /// Build the `(old_values, new_values)` pair for a purely DIMENSIONAL leaf
    /// tick — `width` 10mm → 10.5mm, the same tick `tests/morph_arm_e2e.rs`
    /// drives — that ALSO carries a diff on `other_id`.
    fn dim_tick_values(
        other_id: &ValueCellId,
        other_old: reify_ir::Value,
        other_new: reify_ir::Value,
    ) -> (reify_ir::ValueMap, reify_ir::ValueMap) {
        use reify_ir::{Value, ValueMap};
        let mut old = ValueMap::new();
        old.insert(dim_leaf_id(), Value::length(0.010));
        old.insert(other_id.clone(), other_old);
        let mut new = ValueMap::new();
        new.insert(dim_leaf_id(), Value::length(0.0105));
        new.insert(other_id.clone(), other_new);
        (old, new)
    }

    // ── Step-1: classify_cell baseline behavior ────────────────────────────

    #[test]
    fn classify_cell_scalar_length_returns_dimensional() {
        let id = ValueCellId::new("Part", "width");
        let g = graph_with_cell(&id, Type::length());
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_scalar_angle_returns_dimensional() {
        let id = ValueCellId::new("Part", "twist");
        let g = graph_with_cell(&id, Type::angle());
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_scalar_dimensionless_returns_dimensional() {
        let id = ValueCellId::new("Part", "ratio");
        let g = graph_with_cell(&id, Type::dimensionless_scalar());
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_real_returns_dimensional() {
        let id = ValueCellId::new("Part", "scale");
        let g = graph_with_cell(&id, Type::dimensionless_scalar());
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_int_returns_dimensional() {
        let id = ValueCellId::new("Part", "sides");
        let g = graph_with_cell(&id, Type::Int);
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_geometry_returns_dimensional() {
        // Task 6635: a `Type::Geometry` cell is a *derived* reference to a
        // realization, never an authored leaf parameter. Classifying it
        // Structural via Rule 4's conservative default vetoed every real
        // dimensional edit. See `classify_cell`'s "Type::Geometry and Rule 4".
        let id = ValueCellId::new("Part", "body");
        let g = graph_with_cell(&id, Type::Geometry);
        assert_eq!(classify_cell(&g, &id), ParameterClass::Dimensional);
    }

    #[test]
    fn classify_cell_enum_returns_structural() {
        let id = ValueCellId::new("Part", "mode");
        let g = graph_with_cell(&id, Type::Enum("Mode".to_string()));
        assert_eq!(classify_cell(&g, &id), ParameterClass::Structural);
    }

    #[test]
    fn classify_cell_bool_returns_structural() {
        // Bool NOT in structure_controlling — still Structural via conservative default.
        let id = ValueCellId::new("Part", "mirrored");
        let g = graph_with_cell(&id, Type::Bool);
        assert_eq!(classify_cell(&g, &id), ParameterClass::Structural);
    }

    #[test]
    fn classify_cell_string_returns_structural() {
        let id = ValueCellId::new("Part", "label");
        let g = graph_with_cell(&id, Type::String);
        assert_eq!(classify_cell(&g, &id), ParameterClass::Structural);
    }

    #[test]
    fn classify_cell_missing_cell_returns_structural() {
        // Cell not present in graph.value_cells → Structural (conservative).
        let g = EvaluationGraph::default();
        let unknown_id = ValueCellId::new("Part", "does_not_exist");
        assert_eq!(classify_cell(&g, &unknown_id), ParameterClass::Structural);
    }

    // ── Step-15: stage_a_eligible asymmetric ValueMap membership ──────────

    #[test]
    fn stage_a_eligible_dimensional_cell_only_in_new_returns_true() {
        use reify_ir::{Value, ValueMap};

        // Graphs are identical (same shape hash). new_values contains a
        // Scalar[LENGTH] entry absent from old_values. Dimensional cell →
        // diff allowed → eligible.
        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let g2 = g1.clone();
        let v1 = ValueMap::new(); // cell absent from old
        let mut v2 = ValueMap::new();
        v2.insert(id.clone(), Value::length(0.10)); // present only in new
        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "dimensional cell only in new_values must be eligible (Some vs None → Dimensional)"
        );
    }

    #[test]
    fn stage_a_eligible_structural_cell_only_in_new_returns_false() {
        use reify_ir::{Value, ValueMap};

        let id = ValueCellId::new("Part", "mode");
        let g1 = graph_with_cell(&id, Type::Enum("Mode".to_string()));
        let g2 = g1.clone();
        let v1 = ValueMap::new();
        let mut v2 = ValueMap::new();
        v2.insert(
            id.clone(),
            Value::Enum {
                type_name: "Mode".to_string(),
                variant: "loft".to_string(),
                payload: vec![],
            },
        );
        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "structural cell only in new_values must not be eligible"
        );
    }

    #[test]
    fn stage_a_eligible_cell_only_in_old_dimensional_returns_true() {
        use reify_ir::{Value, ValueMap};

        let id = ValueCellId::new("Part", "height");
        let g1 = graph_with_cell(&id, Type::length());
        let g2 = g1.clone();
        let mut v1 = ValueMap::new();
        v1.insert(id.clone(), Value::length(0.05)); // present only in old
        let v2 = ValueMap::new(); // absent from new
        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "dimensional cell only in old_values must be eligible"
        );
    }

    #[test]
    fn stage_a_eligible_cell_only_in_old_structural_returns_false() {
        use reify_ir::{Value, ValueMap};

        let id = ValueCellId::new("Part", "mode");
        let g1 = graph_with_cell(&id, Type::Enum("Mode".to_string()));
        let g2 = g1.clone();
        let mut v1 = ValueMap::new();
        v1.insert(
            id.clone(),
            Value::Enum {
                type_name: "Mode".to_string(),
                variant: "sketch".to_string(),
                payload: vec![],
            },
        );
        let v2 = ValueMap::new();
        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "structural cell only in old_values must not be eligible"
        );
    }

    // ── Step-13: stage_a_eligible shape-gate dominance ────────────────────

    #[test]
    fn stage_a_eligible_shape_hash_differs_returns_false() {
        use crate::graph::RealizationNodeData;
        use reify_core::RealizationNodeId;
        use reify_ir::{ReprKind, ValueMap};

        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let mut g2 = g1.clone();
        // Insert an extra realization into g2 — diverges the shape hash.
        let rid = RealizationNodeId::new("Part", 42);
        g2.realizations.insert(
            rid.clone(),
            RealizationNodeData {
                geometry_cell: None,
                id: rid,
                operations: vec![],
                content_hash: ContentHash::of_str("extra-shape"),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
                input_cone_hash: None,
            },
        );
        // Identical (empty) ValueMaps.
        let v = ValueMap::new();
        assert!(
            !stage_a_eligible(&g1, &g2, &v, &v),
            "differing graph shapes must not be stage-A eligible (shape gate short-circuits)"
        );
    }

    #[test]
    fn stage_a_eligible_shape_hash_differs_with_dimensional_value_diff_still_returns_false() {
        use crate::graph::RealizationNodeData;
        use reify_core::RealizationNodeId;
        use reify_ir::{ReprKind, Value, ValueMap};

        let width_id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&width_id, Type::length());
        let mut g2 = g1.clone();
        // Diverge shape.
        let rid = RealizationNodeId::new("Part", 99);
        g2.realizations.insert(
            rid.clone(),
            RealizationNodeData {
                geometry_cell: None,
                id: rid,
                operations: vec![],
                content_hash: ContentHash::of_str("extra-shape-2"),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
                input_cone_hash: None,
            },
        );
        // A dimensional value diff alongside the shape mismatch.
        let mut v1 = ValueMap::new();
        v1.insert(width_id.clone(), Value::length(0.05));
        let mut v2 = ValueMap::new();
        v2.insert(width_id.clone(), Value::length(0.10));
        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "shape gate dominates: permissible dimensional diff cannot rescue a shape mismatch"
        );
    }

    // ── Step-11: stage_a_eligible classifier-driven decisions ─────────────

    #[test]
    fn stage_a_eligible_only_dimensional_value_differs_returns_true() {
        use reify_ir::{Value, ValueMap};

        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let g2 = g1.clone();
        let mut v1 = ValueMap::new();
        v1.insert(id.clone(), Value::length(0.05));
        let mut v2 = ValueMap::new();
        v2.insert(id.clone(), Value::length(0.10)); // dimensional diff — allowed
        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "a dimensional-only diff must be stage-A eligible"
        );
    }

    #[test]
    fn stage_a_eligible_structural_value_differs_returns_false() {
        use reify_ir::{Value, ValueMap};

        let id = ValueCellId::new("Part", "mode");
        let g1 = graph_with_cell(&id, Type::Enum("Mode".to_string()));
        let g2 = g1.clone();
        let mut v1 = ValueMap::new();
        v1.insert(
            id.clone(),
            Value::Enum {
                type_name: "Mode".to_string(),
                variant: "sketch".to_string(),
                payload: vec![],
            },
        );
        let mut v2 = ValueMap::new();
        v2.insert(
            id.clone(),
            Value::Enum {
                type_name: "Mode".to_string(),
                variant: "loft".to_string(),
                payload: vec![],
            },
        ); // structural diff
        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "a structural (Enum) diff must not be stage-A eligible"
        );
    }

    #[test]
    fn stage_a_eligible_structure_controlling_value_differs_returns_false() {
        use reify_ir::{Value, ValueMap};

        // A Bool cell in structure_controlling must be Structural even
        // though Bool is otherwise handled by the conservative default.
        let id = ValueCellId::new("Part", "has_hole");
        let mut g1 = graph_with_cell(&id, Type::Bool);
        g1.structure_controlling.insert(id.clone());
        let g2 = g1.clone();
        let mut v1 = ValueMap::new();
        v1.insert(id.clone(), Value::Bool(true));
        let mut v2 = ValueMap::new();
        v2.insert(id.clone(), Value::Bool(false));
        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "a structure_controlling guard-cell diff must not be stage-A eligible"
        );
    }

    #[test]
    fn stage_a_eligible_collection_count_value_differs_returns_false() {
        use reify_ir::{Value, ValueMap};

        let id = ValueCellId::new("Part", "__count_bolts");
        let mut g1 = graph_with_cell(&id, Type::Int);
        g1.collection_subs.push(CollectionSubInfo {
            parent_entity: "Part".to_string(),
            sub_name: "bolts".to_string(),
            structure_name: "Bolt".to_string(),
            count_cell: id.clone(),
            child_value_cells: vec![],
        });
        let g2 = g1.clone();
        let mut v1 = ValueMap::new();
        v1.insert(id.clone(), Value::Int(3));
        let mut v2 = ValueMap::new();
        v2.insert(id.clone(), Value::Int(5)); // pattern count diff — structural
        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "a collection-count diff must not be stage-A eligible"
        );
    }

    #[test]
    fn stage_a_eligible_mixed_dimensional_and_structural_diff_returns_false() {
        use reify_ir::{Value, ValueMap};

        // Both a Length cell AND an Enum cell differ. The Enum makes it ineligible
        // even though the Length diff would be allowed in isolation.
        let len_id = ValueCellId::new("Part", "width");
        let enum_id = ValueCellId::new("Part", "mode");
        let mut g1 = EvaluationGraph::default();
        for (id, ty) in [
            (len_id.clone(), Type::length()),
            (enum_id.clone(), Type::Enum("Mode".to_string())),
        ] {
            g1.value_cells.insert(
                id.clone(),
                ValueCellNode {
                    id: id.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: ty,
                    default_expr: None,
                    content_hash: ContentHash::of_str(&format!("{}", id)),
                },
            );
        }
        let g2 = g1.clone();
        let mut v1 = ValueMap::new();
        v1.insert(len_id.clone(), Value::length(0.05));
        v1.insert(
            enum_id.clone(),
            Value::Enum {
                type_name: "Mode".to_string(),
                variant: "sketch".to_string(),
                payload: vec![],
            },
        );
        let mut v2 = ValueMap::new();
        v2.insert(len_id.clone(), Value::length(0.10)); // dimensional diff
        v2.insert(
            enum_id.clone(),
            Value::Enum {
                type_name: "Mode".to_string(),
                variant: "loft".to_string(),
                payload: vec![],
            },
        ); // structural diff
        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "any structural diff must not be stage-A eligible, even alongside dimensional diffs"
        );
    }

    // ── Task 6635: derived Type::Geometry cells must not veto a tick ───────

    #[test]
    fn stage_a_eligible_dimensional_tick_with_derived_geometry_diff_returns_true() {
        use reify_core::RealizationNodeId;
        use reify_ir::{GeometryHandleId, Value, ValueMap};

        // Reproduces the measured production shape from
        // `reify-eval/tests/morph_arm_e2e.rs`: a dimensional leaf (`width`)
        // ticks, and the DERIVED `Type::Geometry` cell (`body`) is recomputed
        // downstream, so its value necessarily differs too. The tick must stay
        // Stage-A eligible.
        let width_id = ValueCellId::new("MorphBox", "width");
        let body_id = ValueCellId::new("MorphBox", "body");
        // The dimensional leaf, via the file's shared builder.
        let mut g1 = graph_with_cell(&width_id, Type::length());
        // …plus the derived geometry cell it feeds. `ValueCellKind::Let` is the
        // kind `reify-compiler`'s `entity.rs` geometry-*let* path registers, so
        // the fixture matches the production shape this test's narrative claims
        // (`graph_with_cell` hardcodes `Param`, which is why the body cell is
        // inserted explicitly rather than through it). Rule 4 is deliberately
        // kind-blind — see `classify_cell`'s "The arm keys on the TYPE, not on
        // derivedness" — and pinning the production kind here is what makes that
        // claim exercised rather than merely asserted in prose.
        g1.value_cells.insert(
            body_id.clone(),
            ValueCellNode {
                id: body_id.clone(),
                kind: ValueCellKind::Let,
                cell_type: Type::Geometry,
                default_expr: None,
                content_hash: ContentHash::of_str(&format!("{}", body_id)),
            },
        );
        // Clone so the shape hashes match and the shape gate passes.
        let g2 = g1.clone();

        let mut v1 = ValueMap::new();
        v1.insert(width_id.clone(), Value::length(0.010));
        v1.insert(
            body_id.clone(),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("MorphBox", 0),
                upstream_values_hash: [1u8; 32],
                kernel_handle: Some(GeometryHandleId(1)),
            },
        );

        let mut v2 = ValueMap::new();
        v2.insert(width_id.clone(), Value::length(0.0105)); // the dimensional tick
        v2.insert(
            body_id.clone(),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("MorphBox", 0),
                upstream_values_hash: [2u8; 32], // recomputed downstream
                kernel_handle: Some(GeometryHandleId(1)),
            },
        );

        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6635: a dimensional-only tick must stay Stage-A eligible even \
             though the derived Type::Geometry cell's value also changed — a \
             derived geometry cell is a realization reference, never an authored \
             leaf, and carries no independent structural signal"
        );
    }

    // ── Step-9: stage_a_eligible – identical graph and values ─────────────

    #[test]
    fn stage_a_eligible_identical_graph_and_values_returns_true() {
        use reify_ir::ValueMap;

        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let g2 = g1.clone(); // O(1) structural-sharing clone
        let mut v1 = ValueMap::new();
        v1.insert(id.clone(), reify_ir::Value::length(0.08));
        let v2 = v1.clone();
        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "identical graph and values must be stage-A eligible"
        );
    }

    // ── Step-7: realization_graph_shape_hash ──────────────────────────────

    #[test]
    fn realization_graph_shape_hash_two_identical_graphs_produce_equal_hashes() {
        // Two graphs built with the same content must hash identically.
        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let g2 = graph_with_cell(&id, Type::length());
        assert_eq!(
            realization_graph_shape_hash(&g1),
            realization_graph_shape_hash(&g2),
            "identical graphs must produce equal hashes"
        );
    }

    #[test]
    fn realization_graph_shape_hash_added_realization_diverges() {
        use crate::graph::RealizationNodeData;
        use reify_core::RealizationNodeId;
        use reify_ir::ReprKind;

        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let mut g2 = g1.clone();
        // Insert an extra realization node into g2.
        let rid = RealizationNodeId::new("Part", 99);
        g2.realizations.insert(
            rid.clone(),
            RealizationNodeData {
                geometry_cell: None,
                id: rid,
                operations: vec![],
                content_hash: reify_core::ContentHash::of_str("extra-realization"),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
                input_cone_hash: None,
            },
        );
        assert_ne!(
            realization_graph_shape_hash(&g1),
            realization_graph_shape_hash(&g2),
            "adding a realization must change the shape hash"
        );
    }

    #[test]
    fn realization_graph_shape_hash_added_value_cell_diverges() {
        let id = ValueCellId::new("Part", "width");
        let g1 = graph_with_cell(&id, Type::length());
        let mut g2 = g1.clone();
        // Insert an additional value cell into g2.
        let extra_id = ValueCellId::new("Part", "height");
        g2.value_cells.insert(
            extra_id.clone(),
            ValueCellNode {
                id: extra_id.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: ContentHash::of_str(&format!("{}", extra_id)),
            },
        );
        assert_ne!(
            realization_graph_shape_hash(&g1),
            realization_graph_shape_hash(&g2),
            "adding a value cell must change the shape hash"
        );
    }

    #[test]
    fn realization_graph_shape_hash_matches_topology_fingerprint() {
        // Locks the delegation contract: realization_graph_shape_hash must
        // exactly equal graph.topology_fingerprint() — no forked implementation.
        let id = ValueCellId::new("Part", "width");
        let g = graph_with_cell(&id, Type::length());
        assert_eq!(
            realization_graph_shape_hash(&g),
            g.topology_fingerprint(),
            "realization_graph_shape_hash must delegate to topology_fingerprint"
        );
    }

    // ── Step-5: collection_subs count_cell overrides Int → Dimensional ────

    #[test]
    fn classify_cell_collection_count_returns_structural() {
        // An Int-typed cell registered as a collection count (count_cell) must
        // return Structural, overriding the default Int → Dimensional path.
        let id = ValueCellId::new("Part", "__count_bolts");
        let mut g = graph_with_cell(&id, Type::Int);
        // Register this cell as the count_cell of a CollectionSubInfo entry.
        g.collection_subs.push(CollectionSubInfo {
            parent_entity: "Part".to_string(),
            sub_name: "bolts".to_string(),
            structure_name: "Bolt".to_string(),
            count_cell: id.clone(),
            child_value_cells: vec![],
        });
        assert_eq!(
            classify_cell(&g, &id),
            ParameterClass::Structural,
            "collection count_cell must be Structural even though its Type is Int"
        );
    }

    #[test]
    fn classify_cell_int_not_in_collection_subs_remains_dimensional() {
        // Regression guard: an Int cell that is NOT registered as any
        // collection's count_cell must still return Dimensional.  This proves
        // the collection-count check is targeted (count_cell match), not
        // over-broad (all Int cells → Structural).
        let id = ValueCellId::new("Part", "sides");
        let other_id = ValueCellId::new("Part", "__count_bolts");
        let mut g = graph_with_cell(&id, Type::Int);
        // Add a collection_subs entry whose count_cell is a DIFFERENT cell.
        g.collection_subs.push(CollectionSubInfo {
            parent_entity: "Part".to_string(),
            sub_name: "bolts".to_string(),
            structure_name: "Bolt".to_string(),
            count_cell: other_id,
            child_value_cells: vec![],
        });
        assert_eq!(
            classify_cell(&g, &id),
            ParameterClass::Dimensional,
            "Int cell NOT in collection_subs.count_cell must remain Dimensional"
        );
    }

    // ── Suggestion-1: Undef ≡ absent convention ───────────────────────────
    //
    // `ValueMap::get_or_undef` returns Value::Undef for absent entries,
    // matching the engine's convention (see EvalResult::values doc comment:
    // "callers iterating values for Param cells MUST guard lookups via
    // get_or_undef").  A cell that flips between Some(Undef) and None is a
    // no-op edit and must not trigger a structural rejection.

    #[test]
    fn stage_a_eligible_some_undef_and_absent_are_equivalent_no_spurious_rejection() {
        use reify_ir::{Value, ValueMap};

        // Use an Enum cell (structural type) so that a *real* value diff on
        // this cell would produce false.  The Undef↔absent asymmetry must NOT
        // cause a false rejection — both sides resolve to Value::Undef.
        let id = ValueCellId::new("Part", "mode");
        let g = graph_with_cell(&id, Type::Enum("Mode".to_string()));
        let g2 = g.clone();

        // Case A: old = Some(Undef), new = None (absent).
        let mut v_undef = ValueMap::new();
        v_undef.insert(id.clone(), Value::Undef);
        let v_absent = ValueMap::new();
        assert!(
            stage_a_eligible(&g, &g2, &v_undef, &v_absent),
            "Some(Undef) vs absent must not cause a spurious structural rejection"
        );

        // Case B: old = None (absent), new = Some(Undef).
        assert!(
            stage_a_eligible(&g, &g2, &v_absent, &v_undef),
            "absent vs Some(Undef) must not cause a spurious structural rejection"
        );
    }

    // ── Step-3 (task 3932 δ): keyed_subs count_cell overrides Int → Structural ──

    /// A keyed sub's count cell (Type::Int) must return Structural, mirroring the
    /// collection_subs count-cell rule but for the `keyed_subs` registry.
    ///
    /// RED today (step-3): classify_cell ignores keyed_subs → Rule 4 → Int →
    /// Dimensional. Flips GREEN after step-4.
    #[test]
    fn classify_cell_keyed_sub_count_returns_structural() {
        let id = ValueCellId::new("Manifold", "__count_vents");
        let mut g = graph_with_cell(&id, Type::Int);
        // Register this cell as the count_cell of a KeyedSubInfo entry.
        g.keyed_subs.push(KeyedSubInfo {
            parent_entity: "Manifold".to_string(),
            sub_name: "vents".to_string(),
            structure_name: "Vent".to_string(),
            count_cell: Some(id.clone()),
            member_keys: vec![MemberKey::new("intake"), MemberKey::new("exhaust")],
        });
        assert_eq!(
            classify_cell(&g, &id),
            ParameterClass::Structural,
            "keyed sub count_cell must be Structural even though its Type is Int"
        );
    }

    /// Regression guard: an Int cell that is NOT any keyed sub's count_cell must
    /// still return Dimensional. Proves the keyed-sub rule is targeted — only the
    /// registered count_cell is Structural, not every Int cell in the graph.
    ///
    /// Strengthened beyond symmetry: registers TWO keyed subs (both with a
    /// count_cell) and asserts each count_cell is Structural while an unrelated Int
    /// cell stays Dimensional. A future over-broad predicate (e.g. all Int cells in
    /// the graph, or all cells whose entity matches a keyed-sub parent_entity) would
    /// fail the `sides → Dimensional` assertion. A predicate that only fires on the
    /// first sub's structure_name would fail the second count_cell assertion.
    #[test]
    fn classify_cell_int_not_in_keyed_subs_remains_dimensional() {
        // Cell under test: an unrelated Int cell (design-author-visible parameter,
        // not any sub's count).
        let id = ValueCellId::new("Manifold", "sides");
        // Two distinct count_cells, one per keyed sub.
        let count_id_1 = ValueCellId::new("Manifold", "__count_vents");
        let count_id_2 = ValueCellId::new("Manifold", "__count_ports");

        let mut g = graph_with_cell(&id, Type::Int);
        // Also insert both count_cells into the graph so the Rule 3b lookup can
        // resolve them (Rule 1 returns Structural for absent cells, but we want
        // to exercise Rule 3b specifically — insert them as Int so Rule 4 would
        // otherwise return Dimensional).
        for cid in [&count_id_1, &count_id_2] {
            g.value_cells.insert(
                cid.clone(),
                ValueCellNode {
                    id: cid.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::Int,
                    default_expr: None,
                    content_hash: ContentHash::of_str(&format!("{}", cid)),
                },
            );
        }

        // Sub 1: "vents" (Vent structure)
        g.keyed_subs.push(KeyedSubInfo {
            parent_entity: "Manifold".to_string(),
            sub_name: "vents".to_string(),
            structure_name: "Vent".to_string(),
            count_cell: Some(count_id_1.clone()),
            member_keys: vec![MemberKey::new("intake"), MemberKey::new("exhaust")],
        });
        // Sub 2: "ports" (different structure name to catch structure_name-based
        // over-broad predicates)
        g.keyed_subs.push(KeyedSubInfo {
            parent_entity: "Manifold".to_string(),
            sub_name: "ports".to_string(),
            structure_name: "Port".to_string(),
            count_cell: Some(count_id_2.clone()),
            member_keys: vec![MemberKey::new("inlet"), MemberKey::new("outlet")],
        });

        // Unrelated Int cell stays Dimensional — over-broad rule would fail here.
        assert_eq!(
            classify_cell(&g, &id),
            ParameterClass::Dimensional,
            "Int cell NOT in any keyed_subs.count_cell must remain Dimensional"
        );
        // Both count_cells are Structural — per-sub predicate would fail on count_id_2.
        assert_eq!(
            classify_cell(&g, &count_id_1),
            ParameterClass::Structural,
            "first keyed sub's count_cell must be Structural"
        );
        assert_eq!(
            classify_cell(&g, &count_id_2),
            ParameterClass::Structural,
            "second keyed sub's count_cell (different structure_name) must also be Structural"
        );
    }

    // ── Step-3: structure_controlling overrides dimensional type ───────────

    #[test]
    fn classify_cell_structure_controlling_overrides_dimensional_type() {
        // A cell whose Type is Scalar { LENGTH } would normally classify as
        // Dimensional — but if it's in graph.structure_controlling it must
        // return Structural. This covers feature-suppression guard cells whose
        // concrete type might be Bool or even a dimensioned scalar in unusual
        // designs.
        let id = ValueCellId::new("Part", "guard");
        let mut g = graph_with_cell(&id, Type::length());
        // Insert the cell's id into structure_controlling.
        g.structure_controlling.insert(id.clone());
        assert_eq!(
            classify_cell(&g, &id),
            ParameterClass::Structural,
            "structure_controlling must override the Dimensional type dispatch"
        );
    }

    // ── Task 6635: the Type::Geometry relaxation is SUBORDINATE to Rules 2/3 ──
    //
    // These three guard the ORDERING, not the relaxation itself. The single
    // most likely way a future refactor silently reopens the Stage A hole is by
    // hoisting the Geometry arm to an early
    // `if node.cell_type == Type::Geometry { return Dimensional }` above the
    // structural overrides. Mutating in that hoisted form must fail all three.

    #[test]
    fn classify_cell_geometry_in_structure_controlling_returns_structural() {
        // Mirrors classify_cell_structure_controlling_overrides_dimensional_type
        // for Type::Geometry: Rule 2 must still win over Rule 4.
        let id = ValueCellId::new("Part", "gated_body");
        let mut g = graph_with_cell(&id, Type::Geometry);
        g.structure_controlling.insert(id.clone());
        assert_eq!(
            classify_cell(&g, &id),
            ParameterClass::Structural,
            "task 6635: structure_controlling (Rule 2) must override the \
             Type::Geometry → Dimensional dispatch (Rule 4)"
        );
    }

    #[test]
    fn classify_cell_geometry_as_collection_count_returns_structural() {
        // Contrived by design: the point is that Rule 3 wins over Rule 4 for
        // EVERY type, not just Type::Int.
        let id = ValueCellId::new("Part", "__count_bodies");
        let mut g = graph_with_cell(&id, Type::Geometry);
        g.collection_subs.push(CollectionSubInfo {
            parent_entity: "Part".to_string(),
            sub_name: "bodies".to_string(),
            structure_name: "Body".to_string(),
            count_cell: id.clone(),
            child_value_cells: vec![],
        });
        assert_eq!(
            classify_cell(&g, &id),
            ParameterClass::Structural,
            "task 6635: a collection count_cell (Rule 3) must be Structural for \
             every cell type, including Type::Geometry"
        );
    }

    #[test]
    fn stage_a_eligible_structure_controlling_geometry_diff_returns_false() {
        use reify_core::RealizationNodeId;
        use reify_ir::{GeometryHandleId, Value, ValueMap};

        // Covers `stage_a_cell_vetoes`, the private predicate
        // `stage_a_eligible` actually calls. Without this, a future edit that
        // relaxes ONLY that predicate would pass the two `classify_cell`
        // guards above and still be broken.
        let id = ValueCellId::new("Part", "gated_body");
        let mut g1 = graph_with_cell(&id, Type::Geometry);
        g1.structure_controlling.insert(id.clone());
        let g2 = g1.clone();

        let mut v1 = ValueMap::new();
        v1.insert(
            id.clone(),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("Part", 0),
                upstream_values_hash: [1u8; 32],
                kernel_handle: Some(GeometryHandleId(1)),
            },
        );
        let mut v2 = ValueMap::new();
        v2.insert(
            id.clone(),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("Part", 0),
                upstream_values_hash: [2u8; 32],
                kernel_handle: Some(GeometryHandleId(1)),
            },
        );

        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6635: a structure-controlling Type::Geometry cell must still \
             veto the tick end-to-end through stage_a_eligible — the Rule 2 \
             override must hold in stage_a_cell_vetoes too"
        );
    }

    // ══ Task 6643: Rule 4 is LEAF-SCOPED inside stage_a_eligible's walk ═════
    //
    // Rationale in ONE place: `stage_a_eligible`'s "# The value-diff walk is
    // LEAF-SCOPED" note. The tests below come in two halves, read together:
    //
    //   * RED (task 6643) — a differing derived cell of a NON-whitelisted type,
    //     alongside a real dimensional leaf tick, must be ADMITTED. One test per
    //     type so a partial fix is visible in the failure list.
    //   * GREEN-LOCK — Rules 1/2/3/3b are KIND-AGNOSTIC and must keep vetoing.
    //     These pass BEFORE the leaf-scoping change and must still pass after;
    //     they are the enforceable form of the task's critical constraint and
    //     exist specifically to catch a naive "skip all Let cells".

    /// RED (6643): the headline parametric-FEA case. `let result =
    /// solve_elastic_static(...)` registers a `ValueCellKind::Let` cell of type
    /// `Type::StructureRef("ElasticResult")`, whose value
    /// (`compute_targets/elastic_static.rs:1399` builds it as a
    /// `Value::StructureInstance` under the `StructureTypeId(u32::MAX)`
    /// sentinel) is recomputed on every tick. One such cell anywhere in the
    /// design made the morph arm fully dormant before this change.
    #[test]
    fn stage_a_eligible_dimensional_tick_with_derived_structure_ref_diff_returns_true() {
        use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

        let derived = ValueCellId::new("MorphDerivedLet", "fea");
        let g1 = graph_with_dim_leaf_and(
            &derived,
            Type::StructureRef("ElasticResult".to_string()),
            ValueCellKind::Let,
        );
        let g2 = g1.clone();

        let elastic_result = |max_disp: f64| {
            let mut fields = PersistentMap::new();
            fields.insert("max_displacement".to_string(), Value::length(max_disp));
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_id: StructureTypeId(u32::MAX),
                type_name: "ElasticResult".to_string(),
                version: 1,
                fields,
            }))
        };
        let (v1, v2) = dim_tick_values(&derived, elastic_result(1.0e-4), elastic_result(1.1e-4));

        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643: a dimensional-only tick must stay Stage-A eligible even \
             though a DERIVED StructureRef(\"ElasticResult\") cell was recomputed \
             — PRD line 33 scopes Stage A to LEAF parameters, and a solver-output \
             cell is derived, not a leaf"
        );
    }

    /// RED (6643): `Type::Bool`. This is the exact shape MEASURED in the real
    /// compiled graph for `tests/fixtures/morph_derived_let.ri`'s
    /// `let is_wide = width > depth` (kind=Let, type=Bool, value flipping
    /// `false` → `true` on the 10mm → 10.5mm width tick).
    ///
    /// Pairs deliberately with
    /// [`stage_a_eligible_derived_let_guard_cell_diff_returns_false`] below,
    /// which is the SAME `Let` + `Bool` shape but IS in `structure_controlling`:
    /// membership in that set, not the cell's kind or type, is the discriminator.
    #[test]
    fn stage_a_eligible_dimensional_tick_with_derived_bool_diff_returns_true() {
        use reify_ir::Value;

        let derived = ValueCellId::new("MorphDerivedLet", "is_wide");
        let g1 = graph_with_dim_leaf_and(&derived, Type::Bool, ValueCellKind::Let);
        let g2 = g1.clone();
        let (v1, v2) = dim_tick_values(&derived, Value::Bool(false), Value::Bool(true));

        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643: a derived Bool `let` that is NOT structure_controlling \
             must not veto a dimensional tick — it carries no structural signal \
             of its own, only its upstream leaves' signal"
        );
    }

    /// RED (6643): `Type::String` — e.g. a derived label/report line.
    #[test]
    fn stage_a_eligible_dimensional_tick_with_derived_string_diff_returns_true() {
        use reify_ir::Value;

        let derived = ValueCellId::new("MorphDerivedLet", "label");
        let g1 = graph_with_dim_leaf_and(&derived, Type::String, ValueCellKind::Let);
        let g2 = g1.clone();
        let (v1, v2) = dim_tick_values(
            &derived,
            Value::String("10.0mm".to_string()),
            Value::String("10.5mm".to_string()),
        );

        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643: a derived String `let` must not veto a dimensional tick"
        );
    }

    /// RED (6643): `Type::Enum` — a derived mode SELECTOR is still derived. A
    /// mode selector that genuinely drives topology reaches Stage A as an
    /// authored `param` LEAF (covered by
    /// [`stage_a_eligible_param_leaf_non_whitelisted_type_diff_returns_false`])
    /// or via `structure_controlling`, both of which still veto.
    #[test]
    fn stage_a_eligible_dimensional_tick_with_derived_enum_diff_returns_true() {
        use reify_ir::Value;

        let derived = ValueCellId::new("MorphDerivedLet", "size_class");
        let g1 =
            graph_with_dim_leaf_and(&derived, Type::Enum("Mode".to_string()), ValueCellKind::Let);
        let g2 = g1.clone();
        let mode = |variant: &str| Value::Enum {
            type_name: "Mode".to_string(),
            variant: variant.to_string(),
            payload: vec![],
        };
        let (v1, v2) = dim_tick_values(&derived, mode("narrow"), mode("wide"));

        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643: a derived Enum `let` must not veto a dimensional tick — \
             an authored enum LEAF still does (Rule 4 is unchanged for leaves), \
             and a topology-driving one is also in structure_controlling"
        );
    }

    /// RED (6643): `Type::List(Type::Geometry)` — the sibling shape #7016 owns.
    ///
    /// SCOPE: this closes only the `ValueCellKind::Let` half — the
    /// `let faces = adjacent_faces(...)` / resolved-selector shape, which is the
    /// overwhelmingly common one. #7016 still owns (a) a `Param`-kind
    /// `List<Geometry>` cell, which is a LEAF and so still meets Rule 4's
    /// whitelist, and (b) the undecided design question of whether a LENGTH
    /// change to such a list should stay Structural. Task 6643 does NOT close
    /// #7016.
    #[test]
    fn stage_a_eligible_dimensional_tick_with_derived_list_geometry_diff_returns_true() {
        use reify_core::RealizationNodeId;
        use reify_ir::{GeometryHandleId, Value};

        let derived = ValueCellId::new("MorphDerivedLet", "faces");
        let g1 = graph_with_dim_leaf_and(
            &derived,
            Type::List(Box::new(Type::Geometry)),
            ValueCellKind::Let,
        );
        let g2 = g1.clone();
        // `make_sub_handle` composes each sub-handle's `upstream_values_hash`
        // from the PARENT's, and `Value::GeometryHandle`'s `PartialEq` keys on
        // `(realization_ref, upstream_values_hash)` — so the list differs on
        // every tick even when the selected faces are unchanged.
        let faces = |tag: u8| {
            Value::List(vec![Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("MorphDerivedLet", 0),
                upstream_values_hash: [tag; 32],
                kernel_handle: Some(GeometryHandleId(1)),
            }])
        };
        let (v1, v2) = dim_tick_values(&derived, faces(1), faces(2));

        assert!(
            stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643: a derived List<Geometry> `let` (resolved selector / \
             adjacent_faces) must not veto a dimensional tick; the Param-kind \
             half of that shape and the list-LENGTH question remain #7016's"
        );
    }

    /// GREEN-LOCK: Rule 2 dominates the kind match.
    ///
    /// Built at the REAL compiler guard-cell shape — `kind: ValueCellKind::Let`,
    /// `cell_type: Type::Bool`, inserted into `graph.structure_controlling` —
    /// which is exactly how `crates/reify-eval/src/graph.rs:599-605` constructs
    /// a block/where `__guard_N` cell (allocated in
    /// `reify-compiler/src/guards.rs:297,667,700`).
    ///
    /// This test hand-builds that shape. That the production compiler really
    /// routes a `where` guard into `structure_controlling` is pinned separately
    /// by `tests/morph_arm_e2e.rs`'s
    /// `stage_a_vetoes_dimensional_tick_that_flips_a_compiled_guard_cell`.
    #[test]
    fn stage_a_eligible_derived_let_guard_cell_diff_returns_false() {
        use reify_ir::Value;

        let guard = ValueCellId::new("MorphDerivedLet", "__guard_0");
        let mut g1 = graph_with_dim_leaf_and(&guard, Type::Bool, ValueCellKind::Let);
        g1.structure_controlling.insert(guard.clone());
        let g2 = g1.clone();
        let (v1, v2) = dim_tick_values(&guard, Value::Bool(true), Value::Bool(false));

        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643 CRITICAL CONSTRAINT: a structure_controlling cell must \
             veto REGARDLESS of kind — a compiler `__guard_N` feature-suppression \
             toggle is `Let` + `Bool`, so leaf scoping must evaluate Rule 2 \
             BEFORE the kind match, never skip Let cells wholesale"
        );
    }

    /// GREEN-LOCK: Rule 3 dominates the kind match — the `let n = base + extra`
    /// pattern-count case. A count cell computed by a `let` is still a count.
    #[test]
    fn stage_a_eligible_derived_let_collection_count_diff_returns_false() {
        use reify_ir::Value;

        let count = ValueCellId::new("MorphDerivedLet", "n_bolts");
        let mut g1 = graph_with_dim_leaf_and(&count, Type::Int, ValueCellKind::Let);
        g1.collection_subs.push(CollectionSubInfo {
            parent_entity: "MorphDerivedLet".to_string(),
            sub_name: "bolts".to_string(),
            structure_name: "Bolt".to_string(),
            count_cell: count.clone(),
            child_value_cells: vec![],
        });
        let g2 = g1.clone();
        let (v1, v2) = dim_tick_values(&count, Value::Int(3), Value::Int(5));

        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643 CRITICAL CONSTRAINT: a collection count_cell must veto \
             REGARDLESS of kind — `let n = base + extra` is a Let-kind cell that \
             drives collection elaboration"
        );
    }

    /// GREEN-LOCK: Rule 3b dominates the kind match, mirroring the Rule 3 case
    /// above for `Keyed<Structure>` count cells.
    #[test]
    fn stage_a_eligible_derived_let_keyed_sub_count_diff_returns_false() {
        use reify_ir::Value;

        let count = ValueCellId::new("MorphDerivedLet", "n_vents");
        let mut g1 = graph_with_dim_leaf_and(&count, Type::Int, ValueCellKind::Let);
        g1.keyed_subs.push(KeyedSubInfo {
            parent_entity: "MorphDerivedLet".to_string(),
            sub_name: "vents".to_string(),
            structure_name: "Vent".to_string(),
            count_cell: Some(count.clone()),
            member_keys: vec![MemberKey::new("intake"), MemberKey::new("exhaust")],
        });
        let g2 = g1.clone();
        let (v1, v2) = dim_tick_values(&count, Value::Int(2), Value::Int(4));

        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643 CRITICAL CONSTRAINT: a keyed-sub count_cell (Rule 3b) must \
             veto REGARDLESS of kind"
        );
    }

    /// GREEN-LOCK: Rule 1 dominates the kind match. A cell present in both
    /// ValueMaps but ABSENT from `graph.value_cells` has no knowable kind, so
    /// leaf scoping cannot apply and the conservative veto must stand.
    #[test]
    fn stage_a_eligible_unknown_cell_diff_returns_false() {
        use reify_ir::Value;

        // Deliberately NOT inserted into the graph: `graph_with_cell_kind`
        // registers only the dimensional leaf.
        let unknown = ValueCellId::new("MorphDerivedLet", "does_not_exist");
        let g1 = graph_with_cell_kind(&dim_leaf_id(), Type::length(), ValueCellKind::Param);
        let g2 = g1.clone();
        let (v1, v2) = dim_tick_values(&unknown, Value::Bool(false), Value::Bool(true));

        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643 CRITICAL CONSTRAINT: an unknown cell (Rule 1) must still \
             veto — with no ValueCellNode there is no `kind`, so it cannot be \
             shown to be derived"
        );
    }

    /// GREEN-LOCK: leaf scoping must NOT widen the LEAF whitelist. A `param`
    /// of a non-whitelisted type is a leaf, and Rule 4 still applies to it in
    /// full.
    #[test]
    fn stage_a_eligible_param_leaf_non_whitelisted_type_diff_returns_false() {
        use reify_ir::Value;

        let leaf = ValueCellId::new("MorphDerivedLet", "mirrored");
        let g1 = graph_with_dim_leaf_and(&leaf, Type::Bool, ValueCellKind::Param);
        let g2 = g1.clone();
        let (v1, v2) = dim_tick_values(&leaf, Value::Bool(false), Value::Bool(true));

        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643: leaf scoping must not widen the LEAF whitelist — an \
             authored `param mirrored: Bool` is a leaf, so Rule 4 still applies"
        );
    }

    /// GREEN-LOCK: an `auto` param is a declared LEAF whose value the constraint
    /// solver supplies — not a derived expression — so Rule 4 still applies to
    /// it. `ValueCellKind::Auto { free }` covers both `auto` and `auto(free)`.
    #[test]
    fn stage_a_eligible_auto_leaf_non_whitelisted_type_diff_returns_false() {
        use reify_ir::Value;

        let leaf = ValueCellId::new("MorphDerivedLet", "mode");
        let g1 = graph_with_dim_leaf_and(
            &leaf,
            Type::Enum("Mode".to_string()),
            ValueCellKind::Auto { free: false },
        );
        let g2 = g1.clone();
        let mode = |variant: &str| Value::Enum {
            type_name: "Mode".to_string(),
            variant: variant.to_string(),
            payload: vec![],
        };
        let (v1, v2) = dim_tick_values(&leaf, mode("sketch"), mode("loft"));

        assert!(
            !stage_a_eligible(&g1, &g2, &v1, &v2),
            "task 6643: `auto` is a declared LEAF (the solver supplies its value, \
             it is not a derived expression), so Rule 4 must still apply to it"
        );
    }

    /// GREEN-LOCK: `classify_cell` is deliberately NOT leaf-scoped. It answers
    /// "what class is THIS cell, in isolation", so a derived non-whitelisted
    /// cell still classifies `Structural` here even though the same cell no
    /// longer vetoes a tick.
    ///
    /// Pairs with
    /// [`stage_a_eligible_dimensional_tick_with_derived_bool_diff_returns_true`],
    /// which drives the SAME cell shape through the walk and gets `true`. The
    /// divergence is the contract; without this test, "fixing" `classify_cell`
    /// to match `stage_a_cell_vetoes` would leave the suite fully green (every
    /// other `classify_cell` test builds its graph with `graph_with_cell`,
    /// which hardcodes `ValueCellKind::Param`).
    #[test]
    fn classify_cell_derived_let_non_whitelisted_type_stays_structural() {
        let id = ValueCellId::new("MorphDerivedLet", "is_wide");
        let graph = graph_with_cell_kind(&id, Type::Bool, ValueCellKind::Let);

        assert_eq!(
            classify_cell(&graph, &id),
            ParameterClass::Structural,
            "task 6643: leaf scoping lives in `stage_a_cell_vetoes`, not here — \
             `classify_cell` must keep reporting a derived non-whitelisted cell as \
             Structural so \"is this type Dimensional?\" stays a separable question \
             from \"does this cell veto a tick?\""
        );
    }
}
