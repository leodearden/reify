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
pub(crate) fn expand_refs_through_dependent_cells(
    refs: &mut HashSet<ValueCellId>,
    auto_reads: &HashMap<ValueCellId, HashSet<ValueCellId>>,
) {
    if auto_reads.is_empty() {
        return;
    }
    let reached: Vec<ValueCellId> = refs
        .iter()
        .filter_map(|id| auto_reads.get(id))
        .flat_map(|autos| autos.iter().cloned())
        .collect();
    refs.extend(reached);
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
    decompose_into_components_with_reads(auto_params, constraints, objective_refs, &auto_reads)
}

/// [`decompose_into_components`] over an ALREADY-BUILT
/// `dependent-cell id → transitive auto set` map.
///
/// See [`dependent_cell_auto_reads`] for the map's construction and its
/// fail-safe cycle semantics (a cell on or downstream of a back edge is
/// OMITTED rather than published with a partial set).
pub(crate) fn decompose_into_components_with_reads(
    auto_params: &[AutoParam],
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    objective_refs: Option<&HashSet<ValueCellId>>,
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
        expand_refs_through_dependent_cells(&mut refs, auto_reads);

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

        let domain = ConstraintClassifier::classify(expr);

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
        // source and one-hop edges from the other. Borrowed unchanged when
        // there is nothing to expand, so the direct path allocates nothing.
        let refs: std::borrow::Cow<'_, HashSet<ValueCellId>> = if auto_reads.is_empty() {
            std::borrow::Cow::Borrowed(refs)
        } else {
            let mut owned = refs.clone();
            expand_refs_through_dependent_cells(&mut owned, auto_reads);
            std::borrow::Cow::Owned(owned)
        };

        let obj_param_indices: Vec<usize> = refs
            .iter()
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

    /// The α fixture: `let s = a + b`, `let d = a - b`, and two constraints
    /// that read ONLY the lets.
    fn alpha_fixture() -> (
        Vec<AutoParam>,
        Vec<(ConstraintNodeId, CompiledExpr)>,
        Vec<(ValueCellId, CompiledExpr)>,
    ) {
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
