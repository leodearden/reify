//! Connected-component decomposition for constraint problems.
//!
//! Builds a bipartite graph of constraints ↔ auto params and uses
//! union-find to identify independent sub-problems.

use crate::classifier::ConstraintClassifier;
use reify_core::{ConstraintNodeId, ValueCellId};
use reify_ir::{AutoParam, CompiledExpr, CompiledExprKind, ConstraintDomain};
use std::collections::{HashMap, HashSet};

/// An independent sub-problem extracted from a larger constraint problem.
#[derive(Debug)]
pub struct SubProblem {
    /// The auto parameters in this sub-problem.
    pub auto_params: HashSet<ValueCellId>,
    /// The constraints in this sub-problem (id + expression).
    pub constraints: Vec<(ConstraintNodeId, CompiledExpr)>,
    /// The domain classification for this sub-problem.
    pub domain: ConstraintDomain,
}

// --- Union-Find ---

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]]; // path splitting
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        // Union by rank
        if self.rank[ra] < self.rank[rb] {
            self.parent[ra] = rb;
        } else if self.rank[ra] > self.rank[rb] {
            self.parent[rb] = ra;
        } else {
            self.parent[rb] = ra;
            self.rank[ra] += 1;
        }
    }
}

// --- Expression tree walk to collect ValueCellIds ---

/// Collect all ValueCellIds referenced in an expression tree (public for registry).
pub(crate) fn collect_value_refs_pub(expr: &CompiledExpr, out: &mut HashSet<ValueCellId>) {
    collect_value_refs(expr, out);
}

/// Collect all ValueCellIds referenced in an expression tree.
///
/// Delegates child traversal to `CompiledExpr::walk` — when new
/// `CompiledExprKind` variants are added, only `walk()` needs updating.
fn collect_value_refs(expr: &CompiledExpr, out: &mut HashSet<ValueCellId>) {
    expr.walk(&mut |node| {
        if let CompiledExprKind::ValueRef(id) = &node.kind {
            out.insert(id.clone());
        }
    });
}

/// For each dependent cell, the set of auto-param ids it reads TRANSITIVELY —
/// following `ValueRef`s through OTHER dependent cells, not just its own
/// expression.
///
/// # Why this exists (task #5720)
///
/// [`decompose_into_components`] unions the auto params an objective references
/// SYNTACTICALLY. The canonical joint-drive shape (task #5189 β) is an objective
/// that reads a bare DERIVED cell and no auto at all, so that union step sees an
/// empty set and two autos coupled only through the derived cell land in
/// SEPARATE components. `SolverRegistry::solve_inner` feeds this map back into
/// its `obj_refs` before decomposing, so decomposition follows `dependent_cells`
/// and the coupled autos are solved jointly. It also uses the map as the
/// per-component fold filter: a component folds a cell only when it OWNS every
/// auto that cell transitively reads, which is what makes a cross-component
/// `Undef` fold structurally impossible.
///
/// # Why a reachability DFS, not a single forward pass
///
/// `dependent_cells` arrives topologically sorted (reify-eval's
/// `build_dependent_cells`), so a single forward pass would be cheaper. Its
/// failure mode is catastrophic and SILENT: were any cell to read a later
/// entry, the pass would under-approximate that cell's auto set, the registry's
/// subset filter would wrongly KEEP the cell in a component missing one of its
/// autos, and the `Undef` fold would come straight back. A reachability DFS is
/// order-independent, still linear, and cannot regress that way.
///
/// This computes REACHABILITY ONLY and never reorders `dependent_cells`, so PRD
/// §6.3's single-authority-on-order invariant is untouched: the stored order
/// remains the one authority, produced once upstream and consumed unchanged.
///
/// # INVARIANTS
///
/// - Cycle-safe, and FAIL-SAFE on a cycle: a cell that closes a back edge — or
///   that transitively reads one — is OMITTED from the returned map entirely
///   rather than published with the partial set the DFS accumulated. Publishing
///   a partial set would be the exact under-approximation this function exists
///   to prevent: the registry's subset filter would wrongly KEEP such a cell in
///   a component missing one of its autos and the `Undef` fold would come
///   straight back. ABSENCE is the safe direction — the filter drops a cell it
///   has no entry for. reify-eval's `build_dependent_cells` already drops
///   cycles, so this costs nothing on a well-formed problem and removes the
///   dependency on that upstream guarantee.
/// - Iterative (explicit stack), so a deep dependent-cell chain cannot blow the
///   native stack.
/// - A ref that is neither an auto nor another dependent cell is ignored: it is
///   a plain value that carries no auto dependence.
/// - A duplicate cell id resolves to the UNION over ALL of its occurrences —
///   both as a child edge (a ref to that id inherits every occurrence's set)
///   and in the returned map. First-occurrence-wins would be unsafe in this
///   map's PRIMARY consumer: the registry filter keys on id, so every
///   occurrence of a duplicated cell is retained or dropped TOGETHER. Were a
///   later occurrence to read a strictly larger auto set, first-wins would keep
///   both in a component that does not own one of those autos and the fold
///   would read it unbound. Unioning is the drop-side-safe direction, matching
///   how every other unknown here resolves.
pub(crate) fn dependent_cell_auto_reads(
    dependent_cells: &[(ValueCellId, CompiledExpr)],
    auto_params: &[AutoParam],
) -> HashMap<ValueCellId, HashSet<ValueCellId>> {
    let n = dependent_cells.len();
    if n == 0 {
        return HashMap::new();
    }

    let auto_ids: HashSet<&ValueCellId> = auto_params.iter().map(|ap| &ap.id).collect();

    // id → EVERY index carrying that id, not just the first. A ref to a
    // duplicated cell inherits the union of all of its occurrences' auto sets:
    // the fold overwrites the cell in stored order, so any occurrence can be
    // the value a later reader observes.
    let mut cell_index: HashMap<&ValueCellId, Vec<usize>> = HashMap::with_capacity(n);
    for (i, (id, _)) in dependent_cells.iter().enumerate() {
        cell_index.entry(id).or_default().push(i);
    }

    // Split each cell's direct refs into (a) autos it reads outright and (b)
    // other dependent cells whose own auto sets it inherits.
    let mut direct_autos: Vec<HashSet<ValueCellId>> = Vec::with_capacity(n);
    let mut child_cells: Vec<Vec<usize>> = Vec::with_capacity(n);
    for (_id, expr) in dependent_cells {
        let mut refs = HashSet::new();
        collect_value_refs(expr, &mut refs);

        let mut autos = HashSet::new();
        let mut children = Vec::new();
        for r in refs {
            if auto_ids.contains(&r) {
                autos.insert(r);
            } else if let Some(indices) = cell_index.get(&r) {
                children.extend(indices.iter().copied());
            }
            // else: a plain value with no auto dependence → ignored.
        }
        direct_autos.push(autos);
        child_cells.push(children);
    }

    // Iterative post-order DFS with memoization. `state`: 0 = unvisited,
    // 1 = on the current stack (in progress), 2 = resolved.
    let mut memo: Vec<Option<HashSet<ValueCellId>>> = vec![None; n];
    // `incomplete[i]`: frame `i` closed a back edge, or inherited one from a
    // child, so `memo[i]` is a STRICT UNDER-APPROXIMATION of that cell's auto
    // reads. Such a cell is omitted from the returned map entirely rather than
    // published partial — see the cycle invariant above.
    let mut incomplete: Vec<bool> = vec![false; n];
    let mut state: Vec<u8> = vec![0; n];
    let mut stack: Vec<usize> = Vec::new();

    for start in 0..n {
        if state[start] == 2 {
            continue;
        }
        stack.push(start);
        while let Some(&top) = stack.last() {
            match state[top] {
                0 => {
                    state[top] = 1;
                    for &child in &child_cells[top] {
                        // Skip children already resolved (2) or already on this
                        // stack (1) — the latter is the cycle guard.
                        if state[child] == 0 {
                            stack.push(child);
                        }
                    }
                }
                1 => {
                    // Every child has either resolved or is an in-progress
                    // ancestor (a cycle). Union the resolved ones and RECORD
                    // whether anything was missed, so a partial set is never
                    // published as if it were complete.
                    let mut set = direct_autos[top].clone();
                    let mut partial = false;
                    for &child in &child_cells[top] {
                        match &memo[child] {
                            Some(child_set) => {
                                set.extend(child_set.iter().cloned());
                                // A resolved-but-partial child taints us too:
                                // our union inherits its shortfall.
                                partial |= incomplete[child];
                            }
                            // Still unresolved at our own resolution point ⇒ an
                            // in-progress ancestor ⇒ a back edge we skipped.
                            None => partial = true,
                        }
                    }
                    memo[top] = Some(set);
                    incomplete[top] = partial;
                    state[top] = 2;
                    stack.pop();
                }
                // Already resolved — this frame is a duplicate push.
                _ => {
                    stack.pop();
                }
            }
        }
    }

    // Materialise. `take()` MOVES each memoised set out — every index is
    // materialised exactly once — so the map never holds a second copy of the
    // DFS's working sets.
    let mut out: HashMap<ValueCellId, HashSet<ValueCellId>> = HashMap::with_capacity(n);
    for (i, (id, _)) in dependent_cells.iter().enumerate() {
        if incomplete[i] {
            continue;
        }
        // UNION across every occurrence of a duplicated id, matching the
        // all-occurrences child edges above.
        out.entry(id.clone())
            .or_default()
            .extend(memo[i].take().unwrap_or_default());
    }
    // An id is only as sound as its WEAKEST occurrence: if ANY occurrence is
    // incomplete, drop the id outright rather than publish a partial union that
    // the registry's subset filter would read as authoritative.
    for (i, (id, _)) in dependent_cells.iter().enumerate() {
        if incomplete[i] {
            out.remove(id);
        }
    }
    out
}

/// Fold each ref's TRANSITIVE auto reads into `refs`, in place.
///
/// This is the ONE expansion body shared by the constraint side and the
/// objective side of the decomposition, and by `SolverRegistry::solve_inner`'s
/// `objective_component` lookup (task #5467 / PRD2 α, layer 2). A ref to a
/// derived cell also means every auto that cell transitively drives, so a
/// constraint reading only `let s = a + b` must be seen to reference `a` and
/// `b`. `auto_reads` is already transitive, so ONE pass closes the set — the
/// expansion is idempotent and may safely be applied to an already-expanded
/// set.
///
/// D1/B2 IDENTITY is structural, not incidental: an empty `auto_reads` (which
/// is exactly what `dependent_cell_auto_reads` returns for an empty
/// `dependent_cells`) inserts nothing, so every downstream ref set, union edge
/// and `referenced_params` list is byte-identical to the pre-α behaviour.
///
/// Returns the ids the expansion REACHED — i.e. exactly the autos that were
/// invisible to the caller's own syntactic walk. The constraint side needs that
/// delta (and not just the widened set) to widen the DOMAIN classification the
/// same way it widens the connectivity; returning it from here keeps that a
/// by-product of the ONE expansion body instead of a second, drifting walk (G7).
/// The list may contain ids the caller already held — re-reaching an already
/// present auto is a no-op for both consumers. Callers that only want the
/// widened set may drop the return value.
pub(crate) fn expand_refs_through_dependent_cells(
    refs: &mut HashSet<ValueCellId>,
    auto_reads: &HashMap<ValueCellId, HashSet<ValueCellId>>,
) -> Vec<ValueCellId> {
    let reached = dependent_cell_reach_delta(refs, auto_reads);
    refs.extend(reached.iter().cloned());
    reached
}

/// The REACH half of [`expand_refs_through_dependent_cells`], without the
/// in-place widening: the autos `refs` reaches only THROUGH a dependent cell.
///
/// Split out so a caller that needs nothing but the delta — the objective-union
/// step of [`decompose_into_components_with_reads`], which only ever maps the
/// widened set down to auto-param INDICES — can skip cloning the whole ref set
/// just to widen a copy of it. Both callers share this ONE body, so the two
/// cannot drift out of lock-step (G7), which is the property the single
/// expansion body exists to hold.
///
/// `auto_reads` is already transitive, so one pass closes the set, and the
/// result may contain duplicates (and ids the caller already holds) — every
/// consumer is duplicate-tolerant (`HashSet::extend`, `UnionFind::union`,
/// `widen_domain`).
pub(crate) fn dependent_cell_reach_delta(
    refs: &HashSet<ValueCellId>,
    auto_reads: &HashMap<ValueCellId, HashSet<ValueCellId>>,
) -> Vec<ValueCellId> {
    if auto_reads.is_empty() {
        return Vec::new();
    }
    refs.iter()
        .filter_map(|id| auto_reads.get(id))
        .flat_map(|autos| autos.iter().cloned())
        .collect()
}

/// The domain flag a bare auto param contributes when it was reached only
/// THROUGH a derived cell.
///
/// `None` means "contributes nothing", which is NOT the same as `Dimensional`:
/// `Dimensional` doubles as the classifier's empty-default, so folding an auto
/// in as `Dimensional` fabricates a numeric flag the classifier itself never
/// set. That is a real cost, so `None` is reserved for the cases where it is
/// provably harmless.
///
/// # The routing question this answers, and why it is SAFE-BY-DEFAULT
///
/// This deliberately does NOT mirror `ConstraintClassifier`'s `ValueRef` arm
/// (`Type::is_numeric()` = `Int | Scalar`). The mirror would be wrong because
/// this function answers a ROUTING question, not a what-does-the-syntax-look-like
/// question: `SolverRegistry::solver_for` routes on `SubProblem.domain`, and a
/// `Logical` verdict hands the whole component to whatever occupies the
/// `Logical` slot — `CpSatSolver`, once PRD2 γ wires it. If that solver cannot
/// build a domain for one of the component's autos, `build_variable_domain`
/// returns `Err` and `solve_inner` fails the ENTIRE component with
/// `NoProgress`.
///
/// So the safe default is "contribute a flag that cannot leave the component
/// at a solver which rejects one of its autos". `None` — the answer that lets a
/// component stay `Logical` — is returned only for autos
/// [`crate::cpsat::can_enumerate`] says CP-SAT can actually enumerate.
/// Consulting that predicate rather than re-deriving a type list here is the
/// point: the routing decision and the enumeration capability are now the same
/// fact, so a new rejection in `build_variable_domain` re-routes in the same
/// commit (the same G7 no-lockstep-duplication argument
/// `fold_dependent_cells` and `expand_refs_through_dependent_cells` already
/// make).
///
/// # Why the non-`None` answers are the auto's OWN domain, not a blanket flag
///
/// A blanket `Dimensional` is safe ONLY when the base classification is
/// `Logical`, because `widen_domain(Logical, Dimensional) == CrossDomain`. It
/// is a NO-OP against a `Dimensional` base — and `Dimensional` is also the
/// classifier's EMPTY DEFAULT for a flagless expression (see the
/// FLAGLESS-EXPRESSION CAVEAT at the widening site). So
/// `let w = if fit == Fit::Tight { 1.0mm } else { 2.0mm }; constraint w == 1.0mm`
/// over an `Enum` auto `fit` classifies `Dimensional`, reaches `fit`,
/// contributes `Dimensional`, stays `Dimensional`, and routes the whole
/// component at `DimensionalSolver` — which maps any non-`Type::Scalar` param
/// to `DimensionVector::DIMENSIONLESS` and writes a `Value::Scalar` back for a
/// `String`/`Enum`/`Geometry` auto. That is the same misrouting class this
/// function exists to close, just on the other side of the base classification.
///
/// Answering with the auto's own domain makes the widening a forcing function
/// regardless of the base:
///   * `Bool` → `Logical` (see below);
///   * `Geometry`/`Feature` → `Geometric`, which `widen_domain` already has
///     arms for, rather than fabricating the numeric flag this doc block opens
///     by warning against;
///   * `Int`/`Scalar` → `Dimensional`, genuinely numeric;
///   * everything else → `CrossDomain`, which NO base classification can
///     absorb, so an auto no solver slot can represent always reaches the
///     fallback.
///
/// Pinned in the `Dimensional`-base direction by
/// `a_numeric_cell_over_a_string_auto_does_not_stay_dimensional`.
///
/// Concretely, the shapes this now routes to the fallback that the earlier
/// hand-written `_ => None` catch-all silently routed at a solver that rejects
/// them:
///   * `Type::Int` with `bounds: None` — NOT hypothetical: `build_auto_param_list`
///     (`reify-eval/src/engine_eval.rs`) hard-codes `bounds: None` for EVERY
///     auto param the engine produces, so an engine-produced `Int` auto is
///     always exactly the shape `build_variable_domain` rejects.
///   * `Type::Int` whose bounds are non-finite, out of `i64` range, or span
///     more than `MAX_INT_DOMAIN` values.
///   * `Type::Enum` with no variant literal anywhere in `constraints`.
///   * `Type::String`/`List`/`Set`/`Map`/`Option`/`Keyed`/`StructureRef`/
///     `TraitObject`/`Field`/… — everything under `build_variable_domain`'s
///     `other =>` catch-all except the two geometry types, which answer
///     `Geometric` (the domain they actually belong to) instead.
///
/// # Why `Type::Bool` is checked BEFORE the predicate
///
/// CP-SAT enumerates `Bool` natively, so the predicate alone would answer
/// `None` for it. But a `Bool` auto reached through a derived cell must still
/// contribute its LOGICAL flag: a constraint the classifier called
/// `Dimensional` that reaches a `Bool` auto is genuinely cross-domain, and
/// leaving it `Dimensional` would route a `Bool` auto at `DimensionalSolver`,
/// which cannot enumerate it. The `Bool` arm therefore answers the domain
/// question, not the capability question, and is matched first.
///
/// # KNOWN APPROXIMATION, accepted: `Type::Enum` sees the WHOLE problem
///
/// `can_enumerate`'s enum arm scans `constraints` for variant literals, and the
/// slice passed here is the decomposition's FULL input, not the component the
/// auto will end up in. A variant literal that lands in a DIFFERENT component
/// therefore still counts as enumerable. This over-approximates in the unsafe
/// direction — but only relative to a perfect answer, never relative to what
/// stood here before: the old code contributed `None` for `Type::Enum`
/// UNCONDITIONALLY, so every enum auto routed `Logical` regardless. Component
/// membership is not known at this point in the loop (the union-find is still
/// being built), so a per-component answer would need a second pass.
///
/// # KNOWN ASYMMETRY, accepted
///
/// A SYNTACTICALLY visible `Int` next to a `Bool` still classifies
/// `CrossDomain` via the classifier's own `is_numeric()` arm, untouched here
/// and outside this task's lock set. This function governs only the
/// invisible-auto delta, where the routing-safety reading is the useful one.
///
/// Pinned by `an_int_auto_behind_a_bool_cell_stays_logical` (the enumerable
/// case) and its `..._unbounded_...` / `..._a_string_auto_...` /
/// `..._a_variantless_enum_...` siblings (the rejected cases).
fn domain_of_auto(
    param: &AutoParam,
    constraints: &[(ConstraintNodeId, CompiledExpr)],
) -> Option<ConstraintDomain> {
    match &param.param_type {
        // Answers the DOMAIN question, so it precedes the capability probe.
        reify_core::Type::Bool => Some(ConstraintDomain::Logical),
        // CP-SAT can build a domain for this auto, so letting the component
        // stay `Logical` is safe — contribute nothing.
        _ if crate::cpsat::can_enumerate(param, constraints) => None,
        // A geometry handle is GEOMETRIC, not numeric. `widen_domain` already
        // carries both `Geometric` arms; answering `Dimensional` here would
        // fabricate exactly the numeric flag this function's doc opens by
        // refusing to fabricate.
        reify_core::Type::Geometry | reify_core::Type::Feature => {
            Some(ConstraintDomain::Geometric)
        }
        // `Int` (unbounded, out of `i64` range, or spanning more than
        // `MAX_INT_DOMAIN`) and `Scalar`: genuinely numeric, so `Dimensional`
        // IS this auto's own domain rather than a stand-in for one.
        t if t.is_numeric() => Some(ConstraintDomain::Dimensional),
        // Everything else — `String`/`Enum`/`List`/`Set`/`Map`/`Option`/
        // `Keyed`/`StructureRef`/`TraitObject`/`Field`/… — is representable by
        // NO solver slot. `CrossDomain` is the only answer no base
        // classification can absorb, so it forces the fallback even when the
        // constraint classified `Dimensional` (including via the classifier's
        // flagless empty default).
        _ => Some(ConstraintDomain::CrossDomain),
    }
}

/// Least upper bound of two constraint domains, reproducing
/// `ConstraintClassifier`'s own flag algebra on the collapsed enum: reconstruct
/// each side's flags (`Dimensional` → numeric, `Logical` → logical, `Geometric`
/// → geometric, `CrossDomain` → geometric+numeric+logical), OR them, and
/// re-apply the classifier's `into_domain` rules — geometric wins over numeric,
/// and logical mixed with ANYTHING else is `CrossDomain`.
///
/// Kept here rather than in `classifier.rs` because it exists only to widen an
/// ALREADY-classified constraint with autos the classifier could not see; the
/// classifier's own single-expression contract is unchanged.
///
/// # Why the match is EXHAUSTIVE rather than `_ => Dimensional`
///
/// The two equal-pair arms at the bottom are unreachable at runtime (the
/// `a == b` fast path returns first), and a catch-all would be shorter. But a
/// catch-all makes a future `ConstraintDomain` variant collapse SILENTLY to
/// `Dimensional` — the LEAST conservative answer — and route an unrepresentable
/// component straight at `DimensionalSolver`. That is precisely the
/// latent-misrouting class the rest of this module exists to close. Spelling
/// every pair out turns a new variant into a COMPILE ERROR instead. The fast
/// path is a runtime early-return, so the compiler still requires those arms
/// and never warns `unreachable_patterns`.
fn widen_domain(a: ConstraintDomain, b: ConstraintDomain) -> ConstraintDomain {
    use ConstraintDomain::{CrossDomain, Dimensional, Geometric, Logical};
    if a == b {
        return a;
    }
    match (a, b) {
        (CrossDomain, _) | (_, CrossDomain) => CrossDomain,
        // logical + (numeric | geometric) → mixed
        (Logical, _) | (_, Logical) => CrossDomain,
        // geometric absorbs numeric (the classifier reports `Geometric` for a
        // geometry call over numeric leaves)
        (Geometric, Dimensional) | (Dimensional, Geometric) => Geometric,
        // Unreachable at runtime via the `a == b` fast path above, but required
        // for exhaustiveness — see the doc comment.
        (Dimensional, Dimensional) => Dimensional,
        (Geometric, Geometric) => Geometric,
    }
}

/// Decompose a constraint problem into independent connected components.
///
/// Each component groups constraints that share auto parameters (directly
/// or transitively). Constraints that reference no auto parameters are
/// excluded from the decomposition.
///
/// The domain for each component is determined by classifying each
/// constraint's expression: unanimous domain → that domain, mixed → CrossDomain.
///
/// Connectivity FOLLOWS `dependent_cells` (task #5467 / PRD2 α, layer 2).
/// `collect_value_refs ∩ param_index` is ONE HOP: for
/// `let s = a + b; constraint s == 10.0` the constraint's ref set is `{s}`,
/// which intersects the auto params in NOTHING — so before α the constraint
/// was skipped entirely and the decomposition came back EMPTY, which
/// `solve_inner` reads as "all auto params are unconstrained".
///
/// This is a thin wrapper: it builds the transitive map and delegates. Callers
/// that ALREADY hold the map (notably `SolverRegistry::solve_inner`, which
/// needs it for its per-component fold filter and its `objective_component`
/// lookup) should call [`decompose_into_components_with_reads`] directly rather
/// than pay for a second walk on the solve hot path.
pub fn decompose_into_components(
    auto_params: &[AutoParam],
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    objective_refs: Option<&HashSet<ValueCellId>>,
    dependent_cells: &[(ValueCellId, CompiledExpr)],
) -> Vec<SubProblem> {
    let auto_reads = dependent_cell_auto_reads(dependent_cells, auto_params);
    decompose_into_components_with_reads(
        auto_params,
        constraints,
        objective_refs,
        None,
        &auto_reads,
    )
}

/// [`decompose_into_components`] over an ALREADY-BUILT
/// `dependent-cell id → transitive auto set` map.
///
/// See [`dependent_cell_auto_reads`] for the map's construction and its
/// fail-safe cycle semantics (a cell on or downstream of a back edge is
/// OMITTED rather than published with a partial set).
///
/// # `objective_refs` is expanded HERE — unless the caller already did it
///
/// Callers may pass their RAW objective ref set and let this function widen it
/// through `auto_reads`, so pre-expanding is never required for the
/// decomposition's sake. `SolverRegistry::solve_inner` pre-expands anyway,
/// because its own `objective_component` first-match lookup needs the widened
/// set.
///
/// `objective_reach` is how such a caller AVOIDS PAYING TWICE (task #5467
/// amendment): it is the reach delta `objective_refs` gets through
/// `auto_reads`, when the caller has already computed it — typically as the
/// return value of [`expand_refs_through_dependent_cells`], in which case it is
/// also already folded into `objective_refs`. `None` means "not computed;
/// compute it here". Re-deriving it instead is not the "handful of hash lookups
/// that find nothing new" this comment used to claim: every dependent-cell id
/// is still present in the widened `refs` (the expansion only ADDS), so the
/// second pass re-clones the WHOLE delta — |delta| `ValueCellId` allocations,
/// each two `String`s. Behaviourally the two are interchangeable (`UnionFind`
/// is idempotent and the chain below tolerates duplicates), so the parameter is
/// a pure cost knob.
pub(crate) fn decompose_into_components_with_reads(
    auto_params: &[AutoParam],
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    objective_refs: Option<&HashSet<ValueCellId>>,
    objective_reach: Option<&[ValueCellId]>,
    auto_reads: &HashMap<ValueCellId, HashSet<ValueCellId>>,
) -> Vec<SubProblem> {
    if constraints.is_empty() {
        return vec![];
    }

    // Build a mapping from ValueCellId → index for auto params only
    let param_ids: Vec<ValueCellId> = auto_params.iter().map(|ap| ap.id.clone()).collect();
    let param_index: HashMap<&ValueCellId, usize> = param_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id, i))
        .collect();

    let n_params = auto_params.len();
    let mut uf = UnionFind::new(n_params);

    // MEMOIZED `domain_of_auto` verdict, one slot per auto param (task #5467
    // amendment). `Some(v)` = probed, `None` = not probed yet; the inner
    // `Option` is the verdict itself.
    //
    // Both of `domain_of_auto`'s arguments — `auto_params[pi]` and the whole
    // `constraints` slice — are invariant across the loop below, so the answer
    // is a pure function of the param INDEX. Without this cache the probe runs
    // once per (constraint × dependent cell read × auto behind it): C × K × A
    // calls, and `dependent_cell_reach_delta` explicitly may return DUPLICATES,
    // so even a single constraint can probe one auto repeatedly. Each call
    // delegates to `cpsat::can_enumerate` → `build_variable_domain`, whose
    // `Type::Enum` arm walks EVERY expression tree in `constraints` — making
    // the uncached form O(C² × K × A × tree_size) on the solve hot path, for a
    // verdict that cannot change between calls. Cached, it is at most A probes
    // per decomposition.
    //
    // D1/B2 IDENTITY: with an empty `auto_reads` no constraint reaches
    // anything, so no slot is ever filled and the `vec![None; n_params]`
    // allocation is the only cost — the same order as the `param_ids` vector
    // already built above.
    let mut auto_domain: Vec<Option<Option<ConstraintDomain>>> = vec![None; n_params];

    // For each constraint, find which auto params it references
    // and union them together. Also track the constraint→params mapping.
    struct ConstraintInfo {
        constraint_idx: usize,
        referenced_params: Vec<usize>, // indices into auto_params
        domain: ConstraintDomain,
    }

    let mut constraint_infos: Vec<ConstraintInfo> = Vec::new();

    for (ci, (_cid, expr)) in constraints.iter().enumerate() {
        let mut refs = HashSet::new();
        collect_value_refs(expr, &mut refs);
        // LAYER 2 (task #5467 / PRD2 α): a constraint that reads a derived
        // cell references every auto that cell transitively drives. With an
        // empty `auto_reads` this inserts nothing and the ref set — hence the
        // union edges and `referenced_params` below — is byte-identical to
        // pre-α.
        let reached = expand_refs_through_dependent_cells(&mut refs, auto_reads);

        // Filter to only auto params
        let referenced: Vec<usize> = refs
            .iter()
            .filter_map(|id| param_index.get(id).copied())
            .collect();

        if referenced.is_empty() {
            // Constraint doesn't reference any auto param → skip
            continue;
        }

        // Union all referenced params
        for i in 1..referenced.len() {
            uf.union(referenced[0], referenced[i]);
        }

        // Domain classification must be widened WHEREVER connectivity was
        // widened, or the two disagree about the same component (task #5467
        // amendment). `ConstraintClassifier::classify` reads the SYNTACTIC
        // expression only, and each `ValueRef` contributes its own
        // `result_type`; an auto reached only THROUGH a derived cell has no
        // `ValueRef` node here at all, so its type is invisible to the walk.
        //
        // Worked case: `let ok = a > 5.0; constraint ok == true` with a `Real`
        // auto `a`. Post-α the union step pulls `a` into this component, but
        // the classifier sees `{ok: Bool, literal true}` and reports `Logical`.
        // A registry that wires a Bool/Enum-only solver into the `Logical` slot
        // (`CpSatSolver`, once PRD2 γ wires it) would then hand a `Real` auto to
        // `build_variable_domain`, get `Err("does not support param type …")`,
        // and fail the WHOLE component with `NoProgress`. Widening to
        // `CrossDomain` routes it to the fallback slot instead, which is what a
        // component holding both a Bool cell and a Real auto actually is.
        // `SolverRegistry::production()` leaves both `logical` and `fallback`
        // `None`, so both spellings fall back to `DimensionalSolver` and
        // production routing is unchanged today.
        //
        // SCOPE of the fold: only the EXPANSION'S OWN reach (`reached`), never
        // the whole widened `refs` set. `reached` is NOT disjoint from the
        // syntactically-visible autos, though — `expand_refs_through_dependent_cells`
        // derives it from each derived cell's TRANSITIVE auto set, which may
        // contain an auto the constraint also references directly and whose
        // type the classifier therefore already folded in. That overlap is
        // harmless rather than merely tolerated: `widen_domain` is idempotent on
        // an already-present flag (`a == b` fast path; `Geometric` absorbs
        // `Dimensional`; `CrossDomain` absorbs everything), so re-widening with
        // an already-seen type is a no-op by construction.
        //
        // FLAGLESS-EXPRESSION CAVEAT: `ConstraintClassifier` collapses its
        // internal `DomainFlags` to the enum before returning, and `Dimensional`
        // is BOTH "saw a numeric leaf" and the empty default (classifier.rs
        // `into_domain`). A constraint expression that sets no flag at all
        // therefore arrives here as `Dimensional`, and a reached `Bool` auto
        // widens it to `CrossDomain` where `Logical` would be exact. That is a
        // deliberate CONSERVATIVE over-approximation: `CrossDomain` routes to
        // the fallback slot, never to a solver that cannot represent a param.
        // Making it exact means propagating `DomainFlags` (or an
        // `Option<ConstraintDomain>` meaning "no flags") out of `classify`, and
        // `classifier.rs` is outside this task's lock set.
        //
        // D1/B2 IDENTITY: an empty `auto_reads` returns an empty `reached`, so
        // this loop never runs and the domain is bit-identical to pre-α.
        let mut domain = ConstraintClassifier::classify(expr);
        for id in &reached {
            if let Some(&pi) = param_index.get(id) {
                let verdict = *auto_domain[pi]
                    .get_or_insert_with(|| domain_of_auto(&auto_params[pi], constraints));
                if let Some(d) = verdict {
                    domain = widen_domain(domain, d);
                }
            }
        }

        constraint_infos.push(ConstraintInfo {
            constraint_idx: ci,
            referenced_params: referenced,
            domain,
        });
    }

    // If objective value-refs are provided (pre-collected from all terms),
    // union all auto params they reference. This ensures all objective-referenced
    // params land in the same component, even if the constraints alone don't
    // connect them. Single-term reduces to prior single-expr behavior identically.
    if let Some(refs) = objective_refs {
        // The OBJECTIVE-side twin of the constraint expansion above. Leaving
        // this direct-only while constraint refs go transitive would be a G7
        // half-fix: the same union-find would receive transitive edges from one
        // source and one-hop edges from the other.
        //
        // Consumes the reach DELTA directly instead of materialising a widened
        // clone of `refs` (task #5467 amendment). The union step's only use for
        // the widened set is mapping it down to auto-param INDICES, and
        // `UnionFind::union` is idempotent, so chaining the delta onto the
        // borrowed original is equivalent and allocates ONE `Vec<ValueCellId>`
        // instead of a whole `HashSet` clone. The old `Cow::Borrowed` fast path
        // was never taken from `SolverRegistry::solve_inner`, whose `auto_reads`
        // is non-empty exactly when the clone was most expensive; an empty
        // `auto_reads` now yields an empty delta and allocates nothing either
        // way, so the D1/B2 path is still zero-cost.
        //
        // And when the caller ALREADY computed that delta — `solve_inner` does,
        // to widen `refs` for its own `objective_component` lookup — it hands it
        // over rather than making this side re-clone the whole thing (see
        // `objective_reach` in the fn doc). `computed` exists only to own the
        // fallback `Vec` for the borrow below.
        let computed;
        let reached: &[ValueCellId] = match objective_reach {
            Some(delta) => delta,
            None => {
                computed = dependent_cell_reach_delta(refs, auto_reads);
                &computed
            }
        };

        let obj_param_indices: Vec<usize> = refs
            .iter()
            .chain(reached.iter())
            .filter_map(|id| param_index.get(id).copied())
            .collect();

        if !obj_param_indices.is_empty() {
            for i in 1..obj_param_indices.len() {
                uf.union(obj_param_indices[0], obj_param_indices[i]);
            }
        }
    }

    if constraint_infos.is_empty() {
        return vec![];
    }

    // Group constraints by their component root
    let mut component_map: HashMap<usize, Vec<usize>> = HashMap::new(); // root → [info_idx]
    for (info_idx, info) in constraint_infos.iter().enumerate() {
        let root = uf.find(info.referenced_params[0]);
        component_map.entry(root).or_default().push(info_idx);
    }

    // Build SubProblem for each component
    let mut result: Vec<SubProblem> = Vec::new();
    for (_root, info_indices) in component_map {
        let mut params = HashSet::new();
        let mut sub_constraints = Vec::new();
        let mut domains: Vec<ConstraintDomain> = Vec::new();

        for &info_idx in &info_indices {
            let info = &constraint_infos[info_idx];
            let (cid, expr) = &constraints[info.constraint_idx];
            sub_constraints.push((cid.clone(), expr.clone()));
            domains.push(info.domain);

            for &pi in &info.referenced_params {
                // Find the root and collect all params in this component
                params.insert(param_ids[pi].clone());
            }
        }

        // Also add any params that are in this component but not directly
        // referenced by any constraint in our list (transitive through union-find)
        for (pi, pid) in param_ids.iter().enumerate() {
            let root = uf.find(pi);
            // Check if this param's root matches any constraint's param root
            if info_indices.iter().any(|&ii| {
                constraint_infos[ii]
                    .referenced_params
                    .iter()
                    .any(|&rp| uf.find(rp) == root)
            }) {
                params.insert(pid.clone());
            }
        }

        // Determine component domain: unanimous → that domain, mixed → CrossDomain
        let first_domain = domains[0];
        let domain = if domains.iter().all(|d| *d == first_domain) {
            first_domain
        } else {
            ConstraintDomain::CrossDomain
        };

        result.push(SubProblem {
            auto_params: params,
            constraints: sub_constraints,
            domain,
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::Type;
    use reify_ir::{BinOp, Value};

    #[test]
    fn collect_refs_from_value_ref() {
        let expr = CompiledExpr::value_ref(ValueCellId::new("Part", "x"), Type::length());
        let mut refs = HashSet::new();
        collect_value_refs(&expr, &mut refs);
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&ValueCellId::new("Part", "x")));
    }

    #[test]
    fn collect_refs_from_binop() {
        let left = CompiledExpr::value_ref(ValueCellId::new("P", "a"), Type::length());
        let right = CompiledExpr::value_ref(ValueCellId::new("P", "b"), Type::length());
        let expr = CompiledExpr::binop(BinOp::Gt, left, right, Type::Bool);
        let mut refs = HashSet::new();
        collect_value_refs(&expr, &mut refs);
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn collect_refs_from_literal_is_empty() {
        let expr = CompiledExpr::literal(Value::Int(42), Type::Int);
        let mut refs = HashSet::new();
        collect_value_refs(&expr, &mut refs);
        assert!(refs.is_empty());
    }

    #[test]
    fn union_find_basic() {
        let mut uf = UnionFind::new(5);
        uf.union(0, 1);
        uf.union(2, 3);
        assert_eq!(uf.find(0), uf.find(1));
        assert_eq!(uf.find(2), uf.find(3));
        assert_ne!(uf.find(0), uf.find(2));

        uf.union(1, 3);
        assert_eq!(uf.find(0), uf.find(3));
    }

    // -----------------------------------------------------------------------
    // LAYER 2 — union-find edges must follow `dependent_cells`
    // (task #5467 / PRD2 α, step-7 RED)
    //
    // `collect_value_refs ∩ param_index` is ONE HOP. For
    // `let s = a + b; constraint s == 10.0` the constraint's ref set is `{s}`,
    // which intersects the auto params in NOTHING — so the constraint is
    // skipped entirely and the decomposition comes back EMPTY, exactly the
    // shape `solve_inner` reads as "all auto params are unconstrained".
    // `ResolutionProblem.dependent_cells` is the id→expr map that closes the
    // gap; it is already topologically ordered (deps precede readers) and its
    // documented membership is precisely "non-auto cells that transitively
    // read ≥1 auto_param".
    // -----------------------------------------------------------------------

    fn alpha_auto(entity: &str, member: &str) -> AutoParam {
        AutoParam {
            id: ValueCellId::new(entity, member),
            param_type: Type::length(),
            bounds: None,
            free: false,
        }
    }

    fn alpha_vref(entity: &str, member: &str) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new(entity, member), Type::length())
    }

    fn add(l: CompiledExpr, r: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(BinOp::Add, l, r, Type::length())
    }

    fn sub(l: CompiledExpr, r: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(BinOp::Sub, l, r, Type::length())
    }

    fn eq_lit(l: CompiledExpr, v: f64) -> CompiledExpr {
        CompiledExpr::binop(
            BinOp::Eq,
            l,
            CompiledExpr::literal(Value::Real(v), Type::length()),
            Type::Bool,
        )
    }

    /// Owned forms of the three slices `decompose_into_components` takes:
    /// auto params, `(id, expr)` constraints, `(id, expr)` dependent cells.
    type AlphaFixture = (
        Vec<AutoParam>,
        Vec<(ConstraintNodeId, CompiledExpr)>,
        Vec<(ValueCellId, CompiledExpr)>,
    );

    /// The α fixture: `let s = a + b`, `let d = a - b`, and two constraints
    /// that read ONLY the lets.
    fn alpha_fixture() -> AlphaFixture {
        let params = vec![alpha_auto("S", "a"), alpha_auto("S", "b")];
        let constraints = vec![
            (ConstraintNodeId::new("S", 0), eq_lit(alpha_vref("S", "s"), 10.0)),
            (ConstraintNodeId::new("S", 1), eq_lit(alpha_vref("S", "d"), 2.0)),
        ];
        let dependent_cells = vec![
            (
                ValueCellId::new("S", "s"),
                add(alpha_vref("S", "a"), alpha_vref("S", "b")),
            ),
            (
                ValueCellId::new("S", "d"),
                sub(alpha_vref("S", "a"), alpha_vref("S", "b")),
            ),
        ];
        (params, constraints, dependent_cells)
    }

    /// THE α FIX — two constraints reading only `let`s must land in ONE
    /// component holding BOTH autos and BOTH constraints. A direct-only ref
    /// intersection yields no referenced params at all and returns an empty
    /// decomposition, which `solve_inner` then reads as "unconstrained".
    #[test]
    fn constraints_reading_only_lets_form_one_component_with_both_autos() {
        let (params, constraints, dependent_cells) = alpha_fixture();

        let components = decompose_into_components(&params, &constraints, None, &dependent_cells);

        assert_eq!(
            components.len(),
            1,
            "both constraints transitively read `S.a` and `S.b` through the \
             `let`s, so they belong to ONE component — a direct-only ref \
             intersection finds no auto params and returns an EMPTY \
             decomposition; got {components:?}",
        );
        let c = &components[0];
        for id in [ValueCellId::new("S", "a"), ValueCellId::new("S", "b")] {
            assert!(
                c.auto_params.contains(&id),
                "the single component must hold {id}; got {:?}",
                c.auto_params,
            );
        }
        assert_eq!(
            c.constraints.len(),
            2,
            "BOTH let-indirected constraints must reach the sub-problem; got \
             {:?}",
            c.constraints,
        );
    }

    /// The OBJECTIVE-side twin. Leaving objective-ref expansion direct-only
    /// while constraint refs go transitive would be a G7 half-fix: the same
    /// union-find would receive transitive edges from one source and one-hop
    /// edges from the other. Here the objective reads ONLY `S.s`, and must
    /// still pull `S.a` and `S.b` into one component.
    #[test]
    fn objective_refs_expand_transitively_through_dependent_cells() {
        let (params, _c, dependent_cells) = alpha_fixture();
        // Two constraints, each pinning ONE auto directly — without the
        // objective they decompose into TWO independent components.
        let constraints = vec![
            (ConstraintNodeId::new("S", 0), eq_lit(alpha_vref("S", "a"), 6.0)),
            (ConstraintNodeId::new("S", 1), eq_lit(alpha_vref("S", "b"), 4.0)),
        ];
        let obj_refs: HashSet<ValueCellId> = [ValueCellId::new("S", "s")].into_iter().collect();

        let split = decompose_into_components(&params, &constraints, None, &dependent_cells);
        assert_eq!(
            split.len(),
            2,
            "fixture integrity: without the objective these two constraints \
             are independent, or the assertion below would pass vacuously; \
             got {split:?}",
        );

        let merged =
            decompose_into_components(&params, &constraints, Some(&obj_refs), &dependent_cells);
        assert_eq!(
            merged.len(),
            1,
            "an objective reading only `S.s` transitively couples `S.a` and \
             `S.b`, so the two components must MERGE into one — the same \
             expansion the constraint side gets; got {merged:?}",
        );
    }

    /// D1/B2 IDENTITY — the SAME call with an EMPTY `dependent_cells` must
    /// reproduce today's partition exactly: no ref reaches an auto param, so
    /// both constraints are skipped and the result is empty.
    #[test]
    fn empty_dependent_cells_reproduces_the_direct_only_partition() {
        let (params, constraints, _dc) = alpha_fixture();

        let components = decompose_into_components(&params, &constraints, None, &[]);

        assert!(
            components.is_empty(),
            "with an EMPTY `dependent_cells` the expansion adds zero edges, so \
             both let-reading constraints reference no auto param and the \
             decomposition is EMPTY — exactly today's behaviour. Anything else \
             means the widening leaked into the D1 identity branch; got \
             {components:?}",
        );
    }

    /// D1/B2 IDENTITY, positive half — an existing DIRECT-ref decomposition is
    /// unaffected by an empty `dependent_cells`. Two independent constraints
    /// stay two components; the shared-param pair stays one.
    #[test]
    fn a_direct_ref_decomposition_is_unaffected_by_empty_dependent_cells() {
        let params = vec![alpha_auto("S", "a"), alpha_auto("S", "b")];
        let independent = vec![
            (ConstraintNodeId::new("S", 0), eq_lit(alpha_vref("S", "a"), 6.0)),
            (ConstraintNodeId::new("S", 1), eq_lit(alpha_vref("S", "b"), 4.0)),
        ];
        assert_eq!(
            decompose_into_components(&params, &independent, None, &[]).len(),
            2,
            "two constraints each reading ONE auto directly stay TWO \
             independent components",
        );

        let shared = vec![(
            ConstraintNodeId::new("S", 0),
            eq_lit(add(alpha_vref("S", "a"), alpha_vref("S", "b")), 10.0),
        )];
        let got = decompose_into_components(&params, &shared, None, &[]);
        assert_eq!(
            got.len(),
            1,
            "one constraint reading BOTH autos directly stays ONE component",
        );
        assert_eq!(
            got[0].auto_params.len(),
            2,
            "…holding both autos; got {:?}",
            got[0].auto_params,
        );
    }

    // -----------------------------------------------------------------------
    // LAYER 2 — the DOMAIN classification must widen wherever connectivity did
    //
    // Widening connectivity without widening classification leaves the two
    // disagreeing about the same component: the union step pulls in an auto the
    // classifier never saw a `ValueRef` for, so `SubProblem.domain` describes
    // only the constraint's syntax while `SubProblem.auto_params` describes the
    // widened reality. `SolverRegistry::solver_for` routes on `domain`, so the
    // disagreement is a mis-ROUTING, latent only because `production()` leaves
    // both the `Logical` and the `CrossDomain` slot `None`.
    // -----------------------------------------------------------------------

    fn bool_cell_ref(entity: &str, member: &str) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new(entity, member), Type::Bool)
    }

    fn gt_lit(l: CompiledExpr, v: f64) -> CompiledExpr {
        CompiledExpr::binop(
            BinOp::Gt,
            l,
            CompiledExpr::literal(Value::Real(v), Type::length()),
            Type::Bool,
        )
    }

    fn eq_true(e: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(
            BinOp::Eq,
            e,
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
            Type::Bool,
        )
    }

    /// A `Bool`-typed derived cell over a NUMERIC auto: `let ok = a > 5.0`,
    /// `constraint ok == true`.
    ///
    /// The syntactic classification is `Logical` — the walk sees only
    /// `{ok: Bool, literal true}`. Post-α the auto `S.a` (a dimensioned
    /// `Scalar`) is nonetheless unioned into this component, so a `Logical`
    /// verdict would route a numeric auto to whatever sits in the `Logical`
    /// slot. `CpSatSolver` — the intended occupant once PRD2 γ wires it —
    /// answers `Err("CpSatSolver does not support param type …")` from
    /// `build_variable_domain` and fails the entire component with
    /// `NoProgress`. `CrossDomain` is the honest description and routes to the
    /// fallback slot instead.
    #[test]
    fn a_bool_cell_over_a_numeric_auto_widens_the_domain_to_cross_domain() {
        let params = vec![alpha_auto("S", "a")];
        let constraints = vec![(
            ConstraintNodeId::new("S", 0),
            eq_true(bool_cell_ref("S", "ok")),
        )];
        let dependent_cells = vec![(
            ValueCellId::new("S", "ok"),
            gt_lit(alpha_vref("S", "a"), 5.0),
        )];

        let components = decompose_into_components(&params, &constraints, None, &dependent_cells);

        assert_eq!(
            components.len(),
            1,
            "fixture integrity: `constraint ok == true` must reach `S.a` \
             through `let ok = a > 5.0`, or the domain assertion below is \
             vacuous; got {components:?}",
        );
        assert!(
            components[0].auto_params.contains(&ValueCellId::new("S", "a")),
            "fixture integrity: the component must actually hold the numeric \
             auto whose routing is under test; got {:?}",
            components[0].auto_params,
        );
        assert_eq!(
            components[0].domain,
            ConstraintDomain::CrossDomain,
            "the component couples a Bool-typed derived cell with a NUMERIC \
             auto, so it is cross-domain. Getting `Logical` means the \
             classification stayed syntactic while connectivity went \
             transitive — and a registry with `CpSatSolver` in the `Logical` \
             slot then fails the whole component with `NoProgress` because \
             `build_variable_domain` rejects a Scalar param type",
        );
    }

    /// The all-numeric α fixture must stay `Dimensional`. The widening folds in
    /// the reached autos' OWN types, and `S.a`/`S.b` are dimensioned scalars,
    /// so there is nothing non-numeric to mix in — this is the guard that the
    /// widening does not smear every let-indirected component to `CrossDomain`
    /// and route the PRD2 α leaf signal away from `DimensionalSolver`.
    #[test]
    fn the_all_numeric_alpha_fixture_stays_dimensional() {
        let (params, constraints, dependent_cells) = alpha_fixture();

        let components = decompose_into_components(&params, &constraints, None, &dependent_cells);

        assert_eq!(components.len(), 1, "fixture integrity; got {components:?}");
        assert_eq!(
            components[0].domain,
            ConstraintDomain::Dimensional,
            "`let s = a + b; constraint s == 10.0` over numeric autos is \
             purely dimensional — widening must add a flag only when a reached \
             auto's type actually differs from what the classifier already saw",
        );
    }

    /// A `Bool` cell over a `Bool` auto stays `Logical`. Paired with the
    /// numeric case deliberately: alone, either could pass on a widening that
    /// unconditionally returned `CrossDomain` for any let-indirected
    /// constraint.
    #[test]
    fn a_bool_cell_over_a_bool_auto_stays_logical() {
        let params = vec![AutoParam {
            id: ValueCellId::new("S", "a"),
            param_type: Type::Bool,
            bounds: None,
            free: true,
        }];
        let constraints = vec![(
            ConstraintNodeId::new("S", 0),
            eq_true(bool_cell_ref("S", "ok")),
        )];
        let dependent_cells = vec![(
            ValueCellId::new("S", "ok"),
            CompiledExpr::unop(reify_ir::UnOp::Not, bool_cell_ref("S", "a"), Type::Bool),
        )];

        let components = decompose_into_components(&params, &constraints, None, &dependent_cells);

        assert_eq!(components.len(), 1, "fixture integrity; got {components:?}");
        assert_eq!(
            components[0].domain,
            ConstraintDomain::Logical,
            "`let ok = not(a); constraint ok == true` over a Bool auto is \
             purely logical — the reached auto contributes the SAME flag the \
             classifier already had, so the widening must be a no-op here",
        );
    }

    /// A BOUNDED `Int` auto behind a `Bool` derived cell stays `Logical` —
    /// `let ok = n > 5; constraint ok == true`.
    ///
    /// `Type::is_numeric()` is `Int | Scalar`, so mirroring the classifier's
    /// `ValueRef` arm verbatim would widen this component to `CrossDomain` and
    /// route it to the FALLBACK slot. But the component is PURELY DISCRETE:
    /// `build_variable_domain` enumerates `0..=10` natively, as it does the
    /// `Bool` cell, so `Logical` is the routing that can actually solve it
    /// while `DimensionalSolver` cannot enumerate the `Bool` side at all.
    ///
    /// This is the POSITIVE discriminator for `domain_of_auto`'s
    /// `can_enumerate` probe: it is the only domain unit in this module whose
    /// auto CP-SAT accepts but whose type is not `Bool`, so an implementation
    /// that answered the routing question from the type alone
    /// (`param_type.is_numeric()`, or a blanket `_ => Some(Dimensional)`)
    /// turns exactly this one red.
    ///
    /// Read together with the three NEGATIVE siblings below, which pin that
    /// every shape `build_variable_domain` REJECTS routes to the fallback.
    /// Deliberately paired: this test alone would pass on a `_ => None`
    /// catch-all, and the siblings alone would pass on `_ => Some(Dimensional)`.
    #[test]
    fn an_int_auto_behind_a_bool_cell_stays_logical() {
        // Shares `bool_cell_over` with its three NEGATIVE siblings below (items
        // are order-free inside a module, so the helper's later declaration is
        // immaterial): identical scaffolding, so the ONLY thing that differs
        // between the positive and negative cases is the auto itself.
        let components = bool_cell_over(
            AutoParam {
                id: ValueCellId::new("S", "n"),
                param_type: Type::Int,
                bounds: Some((0.0, 10.0)),
                free: true,
            },
            CompiledExpr::binop(
                BinOp::Gt,
                CompiledExpr::value_ref(ValueCellId::new("S", "n"), Type::Int),
                CompiledExpr::literal(Value::Int(5), Type::Int),
                Type::Bool,
            ),
        );

        assert_eq!(
            components[0].domain,
            ConstraintDomain::Logical,
            "a Bool derived cell over a BOUNDED Int auto is a purely discrete \
             component and must route to the `Logical` slot, where \
             `CpSatSolver::build_variable_domain` enumerates both the Bool cell \
             and the Int domain natively. Getting `CrossDomain` means \
             `domain_of_auto` stopped consulting `cpsat::can_enumerate` and \
             widened on the type alone, which sends a component CP-SAT could \
             have solved to the fallback `DimensionalSolver` — a solver that \
             cannot enumerate the Bool side at all",
        );
    }

    /// The three NEGATIVE siblings of `an_int_auto_behind_a_bool_cell_stays_logical`.
    ///
    /// Each builds the SAME `let ok = <predicate over the auto>; constraint ok
    /// == true` shape, differing only in the auto's type/bounds, and each picks
    /// a shape `CpSatSolver::build_variable_domain` REJECTS. A rejected auto in
    /// a `Logical` component is `solve_inner` failing the WHOLE component with
    /// `NoProgress`, so the honest verdict is `CrossDomain` (the fallback slot).
    ///
    /// All three go red against a `_ => None` catch-all — the shape that stood
    /// here before `domain_of_auto` consulted `cpsat::can_enumerate`.
    fn bool_cell_over(auto: AutoParam, predicate: CompiledExpr) -> Vec<SubProblem> {
        let auto_id = auto.id.clone();
        let params = vec![auto];
        let constraints = vec![(
            ConstraintNodeId::new("S", 0),
            eq_true(bool_cell_ref("S", "ok")),
        )];
        let dependent_cells = vec![(ValueCellId::new("S", "ok"), predicate)];

        let components = decompose_into_components(&params, &constraints, None, &dependent_cells);

        assert_eq!(
            components.len(),
            1,
            "fixture integrity: `constraint ok == true` must reach `{auto_id:?}` \
             through `let ok = …`, or the domain assertion is vacuous; got \
             {components:?}",
        );
        assert!(
            components[0].auto_params.contains(&auto_id),
            "fixture integrity: the component must actually hold the auto \
             whose routing is under test; got {:?}",
            components[0].auto_params,
        );
        components
    }

    /// An UNBOUNDED `Int` auto behind a `Bool` cell must route to the FALLBACK.
    ///
    /// Not a hypothetical shape: `build_auto_param_list`
    /// (`reify-eval/src/engine_eval.rs`) hard-codes `bounds: None` for EVERY
    /// auto param the engine produces, so this — not the bounded sibling above
    /// — is what an engine-produced `Int` auto actually looks like by the time
    /// it reaches decomposition. `build_variable_domain` answers
    /// `Err("integer auto param … has no bounds; cannot enumerate domain")`.
    #[test]
    fn an_unbounded_int_auto_behind_a_bool_cell_goes_cross_domain() {
        let components = bool_cell_over(
            AutoParam {
                id: ValueCellId::new("S", "n"),
                param_type: Type::Int,
                bounds: None,
                free: true,
            },
            CompiledExpr::binop(
                BinOp::Gt,
                CompiledExpr::value_ref(ValueCellId::new("S", "n"), Type::Int),
                CompiledExpr::literal(Value::Int(5), Type::Int),
                Type::Bool,
            ),
        );

        assert_eq!(
            components[0].domain,
            ConstraintDomain::CrossDomain,
            "an UNBOUNDED Int auto is exactly the shape `build_variable_domain` \
             rejects, and it is the ONLY shape the engine ever mints. Getting \
             `Logical` routes it at a solver that answers `Err(no bounds)` and \
             fails the whole component with `NoProgress`",
        );
    }

    /// A `Type::String` auto behind a `Bool` cell must route to the FALLBACK.
    ///
    /// `String` falls under `build_variable_domain`'s `other =>` catch-all
    /// (`"CpSatSolver does not support param type …"`), together with `List`,
    /// `Set`, `Map`, `Option`, `Geometry`, `Feature`, `StructureRef`,
    /// `TraitObject` and `Field`. This case stands in for that whole arm: a
    /// `domain_of_auto` that enumerated accepted types by hand instead of
    /// asking `can_enumerate` would have to list every one of them.
    #[test]
    fn a_string_auto_behind_a_bool_cell_goes_cross_domain() {
        let components = bool_cell_over(
            AutoParam {
                id: ValueCellId::new("S", "name"),
                param_type: Type::String,
                bounds: None,
                free: true,
            },
            CompiledExpr::binop(
                BinOp::Eq,
                CompiledExpr::value_ref(ValueCellId::new("S", "name"), Type::String),
                CompiledExpr::literal(Value::String("m5".to_string()), Type::String),
                Type::Bool,
            ),
        );

        assert_eq!(
            components[0].domain,
            ConstraintDomain::CrossDomain,
            "`Type::String` is under `build_variable_domain`'s `other =>` \
             catch-all, so a `Logical` verdict hands CP-SAT a param type it \
             answers `Err(does not support param type …)` for",
        );
    }

    /// A `Type::Enum` auto with NO variant literal in the constraints must
    /// route to the FALLBACK.
    ///
    /// `build_variable_domain` derives an enum's domain by scanning the
    /// constraint expressions for `Value::Enum` literals of the matching type
    /// name; with none present it answers `Err("… has no variant literals in
    /// constraints")`. The fixture's `let ok = fit == fit` compares the auto
    /// with ITSELF precisely so no literal appears anywhere.
    ///
    /// This is also the case that pins the enum half of `domain_of_auto`'s
    /// documented approximation: the answer depends on the constraint slice,
    /// not on the type, so it cannot be reached from `param_type` alone.
    #[test]
    fn a_variantless_enum_auto_behind_a_bool_cell_goes_cross_domain() {
        let enum_ref =
            || CompiledExpr::value_ref(ValueCellId::new("S", "fit"), Type::Enum("Fit".to_string()));
        let components = bool_cell_over(
            AutoParam {
                id: ValueCellId::new("S", "fit"),
                param_type: Type::Enum("Fit".to_string()),
                bounds: None,
                free: true,
            },
            CompiledExpr::binop(BinOp::Eq, enum_ref(), enum_ref(), Type::Bool),
        );

        assert_eq!(
            components[0].domain,
            ConstraintDomain::CrossDomain,
            "an enum auto whose variants appear NOWHERE in the constraints has \
             no enumerable domain, so `Logical` routing fails the component \
             with `NoProgress`. Getting `Logical` means `domain_of_auto` \
             answered from `Type::Enum` alone instead of asking \
             `cpsat::can_enumerate`, which needs the constraints to answer",
        );
    }

    /// A `Dimensional`-BASE constraint reaching a non-enumerable auto must not
    /// stay `Dimensional` — `let w = 1.0mm + f(name); constraint w == 1.0mm`
    /// over a `Type::String` auto `S.name`.
    ///
    /// Every other domain unit in this module builds its constraint as
    /// `eq_true(bool_cell_ref(..))`, i.e. a `Logical` base — and against a
    /// `Logical` base ANY numeric-flavoured contribution widens to
    /// `CrossDomain`, so all of them pass on a `domain_of_auto` that answers a
    /// blanket `Dimensional`. This is the case that does not:
    /// `widen_domain(Dimensional, Dimensional) == Dimensional` (the `a == b`
    /// fast path), so a blanket answer is a NO-OP here and the component keeps
    /// the classifier's numeric verdict.
    ///
    /// That verdict routes the whole component at `DimensionalSolver`, which
    /// maps any non-`Type::Scalar` param to `DimensionVector::DIMENSIONLESS`
    /// and writes a `Value::Scalar` back — for a `String` auto. Latent today
    /// (`production()` leaves both the `Logical` and the `CrossDomain` slot
    /// `None`, so every spelling lands on `DimensionalSolver` anyway) and live
    /// at PRD2 γ; the classification is wrong either way, which is what this
    /// pins.
    ///
    /// The dependent cell's `default_expr` is built as IR directly rather than
    /// compiled, so its node types are declarative: what the decomposition
    /// actually consumes is (i) the CONSTRAINT's syntactic types, which drive
    /// `ConstraintClassifier`, and (ii) the `ValueRef` edge from `S.w` to
    /// `S.name`, which drives the reach. A real model would spell the cell
    /// `let w = if name == "m5" { 1.0mm } else { 2.0mm }`.
    #[test]
    fn a_numeric_cell_over_a_string_auto_does_not_stay_dimensional() {
        let params = vec![AutoParam {
            id: ValueCellId::new("S", "name"),
            param_type: Type::String,
            bounds: None,
            free: true,
        }];
        let constraints = vec![(
            ConstraintNodeId::new("S", 0),
            eq_lit(alpha_vref("S", "w"), 1.0),
        )];
        let dependent_cells = vec![(
            ValueCellId::new("S", "w"),
            add(
                CompiledExpr::literal(Value::Real(1.0), Type::length()),
                CompiledExpr::value_ref(ValueCellId::new("S", "name"), Type::String),
            ),
        )];

        let components = decompose_into_components(&params, &constraints, None, &dependent_cells);

        assert_eq!(
            components.len(),
            1,
            "fixture integrity: `constraint w == 1.0` must reach `S.name` \
             through `let w = …`, or the domain assertion below is vacuous; \
             got {components:?}",
        );
        assert!(
            components[0].auto_params.contains(&ValueCellId::new("S", "name")),
            "fixture integrity: the component must actually hold the String \
             auto whose routing is under test; got {:?}",
            components[0].auto_params,
        );
        assert_eq!(
            components[0].domain,
            ConstraintDomain::CrossDomain,
            "the constraint classifies `Dimensional` (a length cell compared \
             with a length literal) and reaches a `Type::String` auto that NO \
             solver slot can represent. Getting `Dimensional` means \
             `domain_of_auto` answered a blanket numeric flag, which \
             `widen_domain` absorbs into an already-`Dimensional` base — \
             leaving a String auto routed at `DimensionalSolver`, which writes \
             a `Value::Scalar` back for it",
        );
    }

    /// A `Type::Geometry` auto reached through a derived cell contributes
    /// `Geometric`, not a fabricated numeric flag.
    ///
    /// Same `Dimensional` base as the sibling above, so the contribution is
    /// visible rather than absorbed. `Geometric` is what `widen_domain`'s
    /// `(Geometric, Dimensional)` arm exists for — and the arm had no coverage
    /// at all before this test and
    /// `widen_domain_absorbs_dimensional_into_geometric` below.
    #[test]
    fn a_geometry_auto_behind_a_numeric_cell_widens_to_geometric() {
        let params = vec![AutoParam {
            id: ValueCellId::new("S", "solid"),
            param_type: Type::Geometry,
            bounds: None,
            free: true,
        }];
        let constraints = vec![(
            ConstraintNodeId::new("S", 0),
            eq_lit(alpha_vref("S", "v"), 1.0),
        )];
        let dependent_cells = vec![(
            ValueCellId::new("S", "v"),
            add(
                CompiledExpr::literal(Value::Real(1.0), Type::length()),
                CompiledExpr::value_ref(ValueCellId::new("S", "solid"), Type::Geometry),
            ),
        )];

        let components = decompose_into_components(&params, &constraints, None, &dependent_cells);

        assert_eq!(components.len(), 1, "fixture integrity; got {components:?}");
        assert_eq!(
            components[0].domain,
            ConstraintDomain::Geometric,
            "a geometry handle is GEOMETRIC. Getting `Dimensional` means \
             `domain_of_auto` fabricated the numeric flag its own doc refuses \
             to fabricate; getting `CrossDomain` means it fell through to the \
             no-solver-can-represent-this arm instead of naming the domain \
             `widen_domain` already has arms for",
        );
    }

    /// Direct coverage of `widen_domain`'s two `Geometric` arms.
    ///
    /// The exhaustive-match doc argues at length that a catch-all would let a
    /// future `ConstraintDomain` variant collapse SILENTLY to `Dimensional`.
    /// That argument is only worth the verbosity if the arms it forces are
    /// actually exercised — and until this unit, 2 of the 5 were dead in the
    /// suite. Both ORDERS of every mixed pair are asserted: `widen_domain` is
    /// documented as a least-upper-bound, so asymmetry would be a bug.
    #[test]
    fn widen_domain_absorbs_dimensional_into_geometric() {
        use ConstraintDomain::{CrossDomain, Dimensional, Geometric, Logical};

        // Geometric absorbs numeric (the classifier reports `Geometric` for a
        // geometry call over numeric leaves).
        assert_eq!(widen_domain(Geometric, Dimensional), Geometric);
        assert_eq!(widen_domain(Dimensional, Geometric), Geometric);
        // The `a == b` fast path, which is what makes the equal-pair arms
        // unreachable at runtime.
        assert_eq!(widen_domain(Geometric, Geometric), Geometric);
        // Logical mixed with ANYTHING else is cross-domain.
        assert_eq!(widen_domain(Geometric, Logical), CrossDomain);
        assert_eq!(widen_domain(Logical, Geometric), CrossDomain);
        // CrossDomain is the top of the lattice and absorbs everything.
        assert_eq!(widen_domain(Geometric, CrossDomain), CrossDomain);
        assert_eq!(widen_domain(CrossDomain, Geometric), CrossDomain);
    }

    // --- dependent_cell_auto_reads (task #5720) ---

    fn auto(name: &str) -> AutoParam {
        AutoParam {
            id: ValueCellId::new("P", name),
            param_type: Type::length(),
            bounds: Some((0.0, 1.0)),
            free: true,
        }
    }

    fn vref(name: &str) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new("P", name), Type::length())
    }

    #[test]
    fn dependent_cell_auto_reads_direct_auto() {
        let cells = vec![(ValueCellId::new("P", "total"), vref("a"))];
        let map = dependent_cell_auto_reads(&cells, &[auto("a")]);
        assert_eq!(
            map.get(&ValueCellId::new("P", "total")),
            Some(&HashSet::from([ValueCellId::new("P", "a")]))
        );
    }

    #[test]
    fn dependent_cell_auto_reads_two_hop_chain_is_transitive() {
        // total = subtotal + a; subtotal = b. `total` must report BOTH autos.
        let cells = vec![
            (ValueCellId::new("P", "subtotal"), vref("b")),
            (
                ValueCellId::new("P", "total"),
                CompiledExpr::binop(BinOp::Add, vref("subtotal"), vref("a"), Type::length()),
            ),
        ];
        let map = dependent_cell_auto_reads(&cells, &[auto("a"), auto("b")]);
        assert_eq!(
            map.get(&ValueCellId::new("P", "total")),
            Some(&HashSet::from([
                ValueCellId::new("P", "a"),
                ValueCellId::new("P", "b"),
            ])),
            "`total` reads `b` only through `subtotal`; a non-transitive walk \
             would miss it and the registry's subset filter would then keep \
             `total` in a component that does not own `b`"
        );
    }

    #[test]
    fn dependent_cell_auto_reads_is_order_independent() {
        // Same graph as above but with the chain stored BACKWARDS (a cell
        // reading a LATER entry). A single forward pass would under-approximate;
        // the reachability DFS must not.
        let cells = vec![
            (
                ValueCellId::new("P", "total"),
                CompiledExpr::binop(BinOp::Add, vref("subtotal"), vref("a"), Type::length()),
            ),
            (ValueCellId::new("P", "subtotal"), vref("b")),
        ];
        let map = dependent_cell_auto_reads(&cells, &[auto("a"), auto("b")]);
        assert_eq!(
            map.get(&ValueCellId::new("P", "total")),
            Some(&HashSet::from([
                ValueCellId::new("P", "a"),
                ValueCellId::new("P", "b"),
            ]))
        );
    }

    #[test]
    fn dependent_cell_auto_reads_ignores_non_auto_non_dependent_refs() {
        let cells = vec![(ValueCellId::new("P", "total"), vref("plain"))];
        let map = dependent_cell_auto_reads(&cells, &[auto("a")]);
        assert_eq!(
            map.get(&ValueCellId::new("P", "total")),
            Some(&HashSet::new()),
            "a ref that is neither an auto nor another dependent cell carries \
             no auto dependence"
        );
    }

    #[test]
    fn dependent_cell_auto_reads_terminates_on_a_cycle() {
        // x = y + a; y = x + b. Self-reachable, so a naive recursion would hang.
        let cells = vec![
            (
                ValueCellId::new("P", "x"),
                CompiledExpr::binop(BinOp::Add, vref("y"), vref("a"), Type::length()),
            ),
            (
                ValueCellId::new("P", "y"),
                CompiledExpr::binop(BinOp::Add, vref("x"), vref("b"), Type::length()),
            ),
        ];
        let map = dependent_cell_auto_reads(&cells, &[auto("a"), auto("b")]);

        // Terminating at all is half the assertion. The other half is that
        // each cycle member is COMPLETE-OR-ABSENT, never partial. `x` and `y`
        // each transitively read {a, b}; whichever resolves FIRST can only see
        // the children already off the stack, so a partial set is what the DFS
        // naturally accumulates. Publishing it would let the registry's subset
        // filter keep `y` (apparent reads {b}) in a component owning only `b`,
        // where folding it reads the unowned auto `a` → `Undef` — precisely the
        // failure the filter is documented to make structurally impossible.
        let both = HashSet::from([ValueCellId::new("P", "a"), ValueCellId::new("P", "b")]);
        for name in ["x", "y"] {
            let id = ValueCellId::new("P", name);
            match map.get(&id) {
                None => {} // Fail-safe: absent, so the filter drops the cell.
                Some(set) => assert_eq!(
                    set, &both,
                    "`{name}` is on a cycle and transitively reads BOTH autos. \
                     A published set MUST be complete; got the partial {set:?}. \
                     Omitting the id entirely is the other acceptable answer — \
                     the registry filter drops a cell it has no entry for."
                ),
            }
        }
    }

    #[test]
    fn dependent_cell_auto_reads_unions_duplicate_ids() {
        // The SAME id twice, the second occurrence reading a strictly larger
        // auto set. The registry filter keys on id, so both occurrences are
        // retained or dropped together — the map must therefore report the
        // UNION. First-occurrence-wins would report {a}, the filter would keep
        // BOTH occurrences in a component owning only `a`, and folding the
        // second would read the unowned auto `b` → `Undef`.
        let dup = ValueCellId::new("P", "total");
        let cells = vec![
            (dup.clone(), vref("a")),
            (
                dup.clone(),
                CompiledExpr::binop(BinOp::Add, vref("a"), vref("b"), Type::length()),
            ),
        ];
        let map = dependent_cell_auto_reads(&cells, &[auto("a"), auto("b")]);
        assert_eq!(
            map.get(&dup),
            Some(&HashSet::from([
                ValueCellId::new("P", "a"),
                ValueCellId::new("P", "b"),
            ])),
            "a duplicated cell id must resolve to the union over ALL of its \
             occurrences — the drop-side-safe direction"
        );
    }

    #[test]
    fn dependent_cell_auto_reads_empty_input_is_empty() {
        assert!(dependent_cell_auto_reads(&[], &[auto("a")]).is_empty());
    }
}
