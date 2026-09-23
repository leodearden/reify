//! Connected-component decomposition for constraint problems.
//!
//! Builds a bipartite graph of constraints ↔ auto params and uses
//! union-find to identify independent sub-problems.

use crate::classifier::ConstraintClassifier;
use crate::dependent_reads::{
    DependentCellReads, collect_value_refs, dependent_cell_auto_reads, reach_of,
};
use reify_core::{ConstraintNodeId, ValueCellId};
use reify_ir::{AutoParam, CompiledExpr, ConstraintDomain};
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

/// An objective's value-refs, ALREADY widened through `dependent_cells`: a ref
/// to a derived cell also names every auto that cell transitively drives.
///
/// [`ExpandedObjectiveRefs::expand`] is the only constructor, so an unexpanded
/// objective cannot reach the decomposition — the shape that split autos
/// coupled only through a derived cell into separate components (task #5720).
///
/// Only FOLDABLE cells widen it, unlike the constraint side, which couples
/// through unfoldable cells too. A cell no stored-order fold derives is never
/// folded, so an objective reaching autos only through one governs nothing;
/// coupling it would turn `FallbackComponentZero` into `Consumed` and silence
/// `E_OBJECTIVE_UNCONSUMED`.
pub(crate) struct ExpandedObjectiveRefs {
    refs: HashSet<ValueCellId>,
}

impl ExpandedObjectiveRefs {
    pub(crate) fn expand(mut refs: HashSet<ValueCellId>, auto_reads: &DependentCellReads) -> Self {
        for id in reach_of(&refs, auto_reads).foldable {
            if !refs.contains(id) {
                refs.insert(id.clone());
            }
        }
        Self { refs }
    }

    /// Does the objective reach any of `autos`, directly or through a cell?
    pub(crate) fn reaches_any(&self, autos: &HashSet<ValueCellId>) -> bool {
        self.refs.iter().any(|r| autos.contains(r))
    }
}

/// The domain flag a bare auto param contributes when the classifier never saw
/// it: reached only THROUGH a derived cell, or coupled in by the objective
/// alone. `constraints` is the slice of the component the auto lives in — the
/// same slice CP-SAT builds its domain from.
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
/// `fold_dependent_cells` and `dependent_reads::reach_of` already make).
///
/// # Why the non-`None` answers are the auto's OWN domain, not a blanket flag
///
/// A blanket `Dimensional` is safe ONLY when the base classification is
/// `Logical`, because `widen_domain(Logical, Dimensional) == CrossDomain`. It
/// is a NO-OP against a `Dimensional` base — and `Dimensional` is also the
/// classifier's EMPTY DEFAULT for a flagless expression (see the
/// FLAGLESS-EXPRESSION CAVEAT on [`component_domain`]). So
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
/// # Why `Type::Bool` and `Type::Enum` are checked BEFORE the predicate
///
/// CP-SAT enumerates `Bool` natively and enumerates an `Enum` whose variants
/// appear as literals, so the predicate alone would answer `None` for both. But
/// a `Bool`/`Enum` auto reached through a derived cell must still contribute
/// its LOGICAL flag: a constraint the classifier called `Dimensional` that
/// reaches one is genuinely cross-domain, and leaving it `Dimensional` routes a
/// `Bool`/`Enum` auto at `DimensionalSolver`, which cannot enumerate it and
/// writes a `Value::Scalar` back for it. Those two arms therefore answer the
/// DOMAIN question, not the capability question, and are matched first.
///
/// `Type::Int` is the only type left for the capability probe, and it is the
/// only one where the two questions coincide — an enumerable `Int` is both
/// numeric and CP-SAT-representable, so leaving a `Logical` base alone is
/// correct. Since `build_variable_domain` accepts exactly `Bool`, bounded-sane
/// `Int` and literal-bearing `Enum` (cpsat.rs), those three arms exhaust the
/// probe's true-set: no `_ if can_enumerate(..)` catch-all is needed, and
/// having one is a live bug (it short-circuits the domain answer for the very
/// types whose domain differs from the base's).
///
/// # PRECONDITION on the `Logical` slot — read before wiring PRD2 γ
///
/// This function is generic ROUTING, but the capability it consults —
/// `crate::cpsat::can_enumerate` — belongs to ONE concrete solver. That is a
/// dependency inversion, and it is sound today only because of a fact outside
/// this function: `SolverRegistry::production()` (registry.rs) leaves both the
/// `logical` and `fallback` slots `None`, so every spelling falls through to
/// `DimensionalSolver` and `CpSatSolver` is the only thing that could ever
/// occupy `Logical`. Nothing in the TYPE system enforces that — the slot is an
/// `Option<Box<dyn ConstraintSolver>>` and accepts any implementor.
///
/// So the anti-drift argument above ("routing and capability cannot disagree")
/// holds only while CP-SAT is the sole `Logical` candidate. The moment PRD2 γ
/// wires a DIFFERENT logical solver into that slot, components get routed by
/// CP-SAT's acceptance set to a solver that never agreed to it, and the
/// argument silently stops holding — with no test failing, because
/// `production()` will still be the thing under test.
///
/// The fix at that point is NOT to patch this arm: it is to move the capability
/// question behind the `ConstraintSolver` trait (e.g. `fn can_enumerate(&self,
/// param, constraints) -> bool` defaulting to `false`) and have
/// `SolverRegistry` ask its ACTUAL `logical` occupant. That refactor was
/// deliberately left out of task #5467, whose lock set is the underdetermined
/// /dependent-cell path, not the solver-capability trait surface.
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
/// `..._a_variantless_enum_...` siblings (the rejected cases), and — for the
/// arm ORDER specifically, which every one of those passes regardless because
/// they all sit on a `Logical` base — by
/// `an_enumerable_enum_auto_behind_a_numeric_cell_does_not_stay_dimensional`.
fn domain_of_auto(
    param: &AutoParam,
    constraints: &[(ConstraintNodeId, CompiledExpr)],
) -> Option<ConstraintDomain> {
    match &param.param_type {
        // Answers the DOMAIN question, so it precedes the capability probe.
        reify_core::Type::Bool => Some(ConstraintDomain::Logical),
        // `Enum` answers the DOMAIN question too, so it ALSO precedes the
        // capability probe (review round 3, suggestion 2). "CP-SAT can build a
        // domain for this auto" and "which solver slot can represent this auto"
        // are DIFFERENT questions, and `widen_domain` is only ever asking the
        // second one. Contributing `None` here left an enum auto reached from a
        // `Dimensional`-classified constraint in a `Dimensional` component,
        // which `solver_for` routes at `DimensionalSolver` — a solver that maps
        // every non-`Type::Scalar` param to `DimensionVector::DIMENSIONLESS`
        // and writes a `Value::Scalar` back. That is precisely the misrouting
        // this fn's doc block opens by promising to close, and it uses an
        // `Enum` auto as its worked example.
        //
        // `Logical` is the enum's OWN domain, so the answer behaves exactly
        // like the `Bool` arm above: a no-op against a `Logical` base
        // (`widen_domain`'s `a == b` fast path) and a forcing function against
        // a `Dimensional` or `Geometric` one. Pinned by
        // `an_enumerable_enum_auto_behind_a_numeric_cell_does_not_stay_dimensional`,
        // whose component holds a `Fit` literal so the probe answers true.
        reify_core::Type::Enum(_) if crate::cpsat::can_enumerate(param, constraints) => {
            Some(ConstraintDomain::Logical)
        }
        // `Int` is the ONE type whose capability answer IS its routing answer:
        // an enumerable `Int` is both genuinely numeric and CP-SAT
        // representable, so a `Logical` component holding it stays solvable and
        // contributing nothing is correct. Keeping the probe in a TYPE-BOUND
        // arm rather than a `_` catch-all is the whole point — a catch-all lets
        // the capability question short-circuit the domain question for every
        // type whose domain is not the base's. `Bool`/`Int`/`Enum` are exactly
        // the three types `build_variable_domain` accepts (cpsat.rs), so these
        // three arms cover the probe's entire true-set with nothing left for a
        // catch-all to absorb.
        reify_core::Type::Int if crate::cpsat::can_enumerate(param, constraints) => None,
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

/// One constraint's contribution to the decomposition.
struct ConstraintInfo {
    constraint_idx: usize,
    /// Indices into `auto_params` of every auto the constraint reads,
    /// syntactically or through a dependent cell: its union-find edges.
    referenced_params: Vec<usize>,
    /// Those reached THROUGH dependent cells, whose types the classifier never
    /// sees.
    reached_params: Vec<usize>,
    /// The classifier's verdict on the syntax alone, or `CrossDomain` for a
    /// constraint reading an unfoldable cell.
    classified: ConstraintDomain,
}

/// The routing domain of one component. Each constraint's `classified`
/// verdict is widened by the own domain of every auto it reaches through a
/// dependent cell; the UNANIMITY rule combines those (mixed → `CrossDomain`);
/// the result is widened by every `objective_only` auto. `own(pi)` is
/// [`domain_of_auto`] over THIS component's constraints.
///
/// The classifier reads syntax only, so it never sees the type of an auto
/// reached through a derived cell, nor of one the objective alone coupled in —
/// yet connectivity puts both in the component, and routing must agree with
/// it. `let ok = a > 5.0; constraint ok == true` over a `Real` auto classifies
/// `Logical`; unwidened, that hands `a` to CP-SAT, whose
/// `build_variable_domain` rejects it and fails the whole component with
/// `NoProgress`.
///
/// The probe reads the COMPONENT's constraints because CP-SAT builds an auto's
/// domain from the sub-problem it is handed. Over the whole problem, an enum
/// variant literal in ANOTHER component made an enum auto look enumerable and
/// left its component `Logical`, for CP-SAT to reject.
///
/// A reached auto may also be referenced syntactically, and so already folded
/// in by the classifier; re-widening it is a no-op, since `widen_domain` is
/// idempotent on a flag already present. A syntactically referenced auto is
/// deliberately NOT re-widened otherwise: that classifier gap is #5416/#6681.
///
/// FLAGLESS-EXPRESSION CAVEAT: `classify` answers `Dimensional` both for a
/// numeric leaf and for an expression with no flag at all, so a flagless
/// constraint reaching a `Bool` auto widens to `CrossDomain` where `Logical`
/// would be exact. That is conservative — `CrossDomain` routes to the fallback
/// slot, never to a solver that cannot represent a param — and making it exact
/// needs `classify` itself to report "no flags".
fn component_domain(
    infos: &[&ConstraintInfo],
    objective_only: &[usize],
    mut own: impl FnMut(usize) -> Option<ConstraintDomain>,
) -> ConstraintDomain {
    let domains: Vec<ConstraintDomain> = infos
        .iter()
        .map(|info| {
            info.reached_params
                .iter()
                .filter_map(|&pi| own(pi))
                .fold(info.classified, widen_domain)
        })
        .collect();

    // Determine component domain: unanimous → that domain, mixed → CrossDomain
    let first_domain = domains[0];
    let unanimous = if domains.iter().all(|d| *d == first_domain) {
        first_domain
    } else {
        ConstraintDomain::CrossDomain
    };

    objective_only
        .iter()
        .filter_map(|&pi| own(pi))
        .fold(unanimous, widen_domain)
}

/// Decompose a constraint problem into independent connected components.
///
/// Each component groups constraints that share auto parameters (directly
/// or transitively). Constraints that reference no auto parameters are
/// excluded from the decomposition.
///
/// The domain for each component is determined by classifying each
/// constraint's expression: unanimous domain → that domain, mixed → CrossDomain.
/// Autos the classifier cannot see — reached through a derived cell, or coupled
/// in by the objective alone — widen it by their own domain.
///
/// Connectivity FOLLOWS `dependent_cells` (task #5467 / PRD2 α, layer 2).
/// `collect_value_refs ∩ param_index` is ONE HOP: for
/// `let s = a + b; constraint s == 10.0` the constraint's ref set is `{s}`,
/// which intersects the auto params in NOTHING — so before α the constraint
/// was skipped entirely and the decomposition came back EMPTY, which
/// `solve_inner` reads as "all auto params are unconstrained".
///
/// This is a thin wrapper: it builds the transitive map, expands the objective
/// through it, and delegates. Callers that ALREADY hold the map (notably
/// `SolverRegistry::solve_inner`, which needs it for its per-component fold
/// filter and its `objective_component` lookup) should call
/// [`decompose_into_components_with_reads`] directly rather than pay for a
/// second walk on the solve hot path.
pub fn decompose_into_components(
    auto_params: &[AutoParam],
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    objective_refs: Option<&HashSet<ValueCellId>>,
    dependent_cells: &[(ValueCellId, CompiledExpr)],
) -> Vec<SubProblem> {
    let auto_reads = dependent_cell_auto_reads(dependent_cells, auto_params);
    let objective =
        objective_refs.map(|refs| ExpandedObjectiveRefs::expand(refs.clone(), &auto_reads));
    decompose_into_components_with_reads(auto_params, constraints, objective.as_ref(), &auto_reads)
}

/// [`decompose_into_components`] over an ALREADY-BUILT dependent-cell reads
/// map (see [`dependent_cell_auto_reads`]).
///
/// A constraint that reads an UNFOLDABLE cell (on or downstream of a cycle) is
/// coupled to every auto that cell reaches and classified `CrossDomain`: no
/// specialized solver can derive a value the registry's per-component fold
/// filter refuses to fold. Dropping it for lack of edges instead could empty
/// the decomposition, which `SolverRegistry` answers with
/// `Solved { unique: true }` and every auto at its default.
///
/// `objective` is already widened through the same `auto_reads` — an
/// [`ExpandedObjectiveRefs`] cannot be built any other way — so its autos are
/// unioned as given.
pub(crate) fn decompose_into_components_with_reads(
    auto_params: &[AutoParam],
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    objective: Option<&ExpandedObjectiveRefs>,
    auto_reads: &DependentCellReads,
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

    // For each constraint, find which auto params it references
    // and union them together. Also track the constraint→params mapping.
    let mut constraint_infos: Vec<ConstraintInfo> = Vec::new();

    for (ci, (_cid, expr)) in constraints.iter().enumerate() {
        let mut refs = HashSet::new();
        collect_value_refs(expr, &mut refs);
        // LAYER 2 (task #5467 / PRD2 α): a constraint that reads a derived
        // cell references every auto that cell transitively drives, foldable
        // or not. With an empty `auto_reads` nothing is reached, so the union
        // edges and `referenced_params` below are exactly pre-α's.
        let reach = reach_of(&refs, auto_reads);

        // Filter to only auto params
        let mut referenced: Vec<usize> = refs
            .iter()
            .chain(reach.all())
            .filter_map(|id| param_index.get(id).copied())
            .collect();
        referenced.sort_unstable();
        referenced.dedup();

        if referenced.is_empty() {
            // Constraint doesn't reference any auto param → skip
            continue;
        }

        // Union all referenced params
        for i in 1..referenced.len() {
            uf.union(referenced[0], referenced[i]);
        }

        let mut reached_params: Vec<usize> = reach
            .all()
            .filter_map(|id| param_index.get(id).copied())
            .collect();
        reached_params.sort_unstable();
        reached_params.dedup();

        // A constraint reading an UNFOLDABLE cell is `CrossDomain` outright —
        // the same top-of-lattice forcing `domain_of_auto`'s `_` arm uses —
        // because no specialized solver can derive a value the per-component
        // fold filter refuses to fold.
        let classified = if reach.reads_unfoldable_cell {
            ConstraintDomain::CrossDomain
        } else {
            ConstraintClassifier::classify(expr)
        };

        constraint_infos.push(ConstraintInfo {
            constraint_idx: ci,
            referenced_params: referenced,
            reached_params,
            classified,
        });
    }

    // Union every auto param the objective reaches (its refs from all terms,
    // already widened through `auto_reads`, the transitive twin of the
    // constraint side above), so they all land in the same component even if
    // the constraints alone don't connect them. Single-term reduces to prior
    // single-expr behavior identically.
    if let Some(objective) = objective {
        let obj_param_indices: Vec<usize> = objective
            .refs
            .iter()
            .filter_map(|id| param_index.get(id).copied())
            .collect();
        for i in 1..obj_param_indices.len() {
            uf.union(obj_param_indices[0], obj_param_indices[i]);
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

    // MEMOIZED `domain_of_auto` verdict, one slot per auto param: `Some(v)` =
    // probed, `None` = not yet; the inner `Option` is the verdict itself. Both
    // arguments are fixed per param — the param, and the constraint slice of
    // the ONE component it lives in — so each slot is filled at most once,
    // always from its own component's slice. Unmemoized, an auto reached by
    // many constraints (or repeatedly by one) would re-run
    // `cpsat::can_enumerate`, whose `Type::Enum` arm walks every expression in
    // the slice. A model whose constraints read every auto directly, with no
    // auto coupled in by the objective alone, never fills a slot.
    let mut auto_domain: Vec<Option<Option<ConstraintDomain>>> = vec![None; n_params];

    // An auto in a component that NO constraint reads, syntactically or
    // through a cell, was put there by the objective's unions alone.
    let mut read_by_a_constraint = vec![false; n_params];
    for info in &constraint_infos {
        for &pi in &info.referenced_params {
            read_by_a_constraint[pi] = true;
        }
    }

    // Build SubProblem for each component
    let mut result: Vec<SubProblem> = Vec::new();
    for (root, info_indices) in component_map {
        let infos: Vec<&ConstraintInfo> =
            info_indices.iter().map(|&i| &constraint_infos[i]).collect();
        let sub_constraints: Vec<(ConstraintNodeId, CompiledExpr)> = infos
            .iter()
            .map(|info| constraints[info.constraint_idx].clone())
            .collect();

        // Every param in this component — which is exactly every param whose
        // union-find root IS the component's root, directly referenced or not.
        //
        // This ONE pass replaces the two that stood here (review round 3,
        // suggestion 4): an insert loop over each constraint's
        // `referenced_params`, then a scan asking, per param, whether ANY
        // constraint in the component referenced ANY param sharing that param's
        // root. Both are subsumed, and the subsumption is exact rather than
        // approximate: the constraint loop above unions every one of a
        // constraint's `referenced_params` together, and `component_map` is
        // keyed on `uf.find(referenced_params[0])`, so `uf.find(rp) == root`
        // holds for EVERY `rp` of EVERY constraint in `info_indices` by
        // construction. The old predicate therefore reduced to `root_of(pi) ==
        // root` — the map key already in hand — and the first loop's inserts
        // were a subset of what this one produces.
        //
        // Not merely tidier: the old form cost O(P x I_c x R) `uf.find` calls
        // where O(P) suffices, and LAYER 2 made `R` grow. Pre-α
        // `referenced_params` held the 1-2 SYNTACTICALLY visible autos; it is
        // now derived from the WIDENED ref set, so one constraint reading a
        // single `let` over the whole model carries `R = |component|` and the
        // scan goes worst-case O(P^2 x C) — on the solve hot path, for exactly
        // the let-indirected models this feature exists to make solvable.
        let mut params = HashSet::new();
        let mut objective_only = Vec::new();
        for (pi, pid) in param_ids.iter().enumerate() {
            if uf.find(pi) == root {
                params.insert(pid.clone());
                if !read_by_a_constraint[pi] {
                    objective_only.push(pi);
                }
            }
        }

        let domain = component_domain(&infos, &objective_only, |pi| {
            *auto_domain[pi]
                .get_or_insert_with(|| domain_of_auto(&auto_params[pi], &sub_constraints))
        });

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
    /// It also pins that the enum answer depends on the constraint slice, not
    /// on the type, so it cannot be reached from `param_type` alone.
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

    /// An ENUMERABLE `Type::Enum` auto reached from a `Dimensional`-base
    /// constraint must not leave the component `Dimensional` (review round 3,
    /// suggestion 2).
    ///
    /// This is the ONE unit in this module that discriminates `domain_of_auto`'s
    /// ARM ORDER. Every other enum/int case here sits on a `Logical` base built
    /// by `bool_cell_over`, and against a `Logical` base a `None` contribution
    /// and a `Some(Logical)` one are indistinguishable — `widen_domain(Logical,
    /// Logical) == Logical` via the `a == b` fast path. Only a `Dimensional`
    /// base separates them: `None` leaves `Dimensional` (the classifier's
    /// verdict AND its flagless empty default), while `Some(Logical)` widens to
    /// `CrossDomain`.
    ///
    /// `S.fit` is reached only through `let w = 1.0mm + fit` from a numeric
    /// constraint. A second constraint, `fit == Fit::Tight`, puts a variant
    /// literal in the SAME component, so `can_enumerate(S.fit)` is true over
    /// that component's own constraints. A `_ if can_enumerate(..)` catch-all
    /// ahead of the `Enum` arm would therefore contribute `None`, and the
    /// component would stay `Dimensional`: the literal's own constraint is
    /// flagless too, which the integrity assertion pins so a classifier change
    /// fails loudly instead of making this test vacuous. `solver_for` would then
    /// hand an `Enum` auto to `DimensionalSolver`, which maps every
    /// non-`Type::Scalar` param to `DIMENSIONLESS` and writes a `Value::Scalar`
    /// back.
    ///
    /// Latent in `production()` today (both the `Logical` and `CrossDomain`
    /// slots are `None`, so every spelling lands on `DimensionalSolver`
    /// regardless) and live at PRD2 γ — the CLASSIFICATION is wrong either way,
    /// which is what this pins.
    #[test]
    fn an_enumerable_enum_auto_behind_a_numeric_cell_does_not_stay_dimensional() {
        let fit = ValueCellId::new("S", "fit");
        let fit_ty = || Type::Enum("Fit".to_string());
        let params = vec![AutoParam {
            id: fit.clone(),
            param_type: fit_ty(),
            bounds: None,
            free: true,
        }];
        let names_a_variant = CompiledExpr::binop(
            BinOp::Eq,
            CompiledExpr::value_ref(fit.clone(), fit_ty()),
            CompiledExpr::literal(Value::enum_unit("Fit", "Tight"), fit_ty()),
            Type::Bool,
        );
        assert_eq!(
            ConstraintClassifier::classify(&names_a_variant),
            ConstraintDomain::Dimensional,
            "fixture integrity: `fit == Fit::Tight` must classify flagless on its \
             own. A `Logical` verdict would mix with the numeric constraint into \
             `CrossDomain` whatever the reached auto contributes, and the \
             assertion below would pass vacuously",
        );
        let constraints = vec![
            // Dimensional base, reaching `S.fit` only through the derived cell.
            (
                ConstraintNodeId::new("S", 0),
                eq_lit(alpha_vref("S", "w"), 1.0),
            ),
            (ConstraintNodeId::new("S", 1), names_a_variant),
        ];
        let dependent_cells = vec![(
            ValueCellId::new("S", "w"),
            add(
                CompiledExpr::literal(Value::Real(1.0), Type::length()),
                CompiledExpr::value_ref(fit.clone(), fit_ty()),
            ),
        )];

        let components = decompose_into_components(&params, &constraints, None, &dependent_cells);

        assert_eq!(
            components.len(),
            1,
            "fixture integrity: both constraints read `S.fit`, one through \
             `let w`, so they form one component; got {components:?}",
        );
        let under_test = &components[0];

        assert_eq!(
            under_test.domain,
            ConstraintDomain::CrossDomain,
            "the constraint classifies `Dimensional` (a length cell compared \
             with a length literal) and reaches an `Enum` auto through the \
             derived cell. Getting `Dimensional` means `domain_of_auto` let the \
             `can_enumerate` CAPABILITY probe short-circuit the DOMAIN answer \
             for `Type::Enum` — and `widen_domain` absorbs a `None` into an \
             already-`Dimensional` base, leaving an `Enum` auto routed at \
             `DimensionalSolver`, which writes a `Value::Scalar` back for it",
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
}
