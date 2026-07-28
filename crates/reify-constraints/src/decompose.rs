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
/// - Cycle-safe: a cell reachable from itself terminates with the PARTIAL set
///   accumulated so far rather than recursing forever. reify-eval's
///   `build_dependent_cells` already drops cycles, but the visited set costs
///   nothing and removes the dependency on that upstream guarantee.
/// - Iterative (explicit stack), so a deep dependent-cell chain cannot blow the
///   native stack.
/// - A ref that is neither an auto nor another dependent cell is ignored: it is
///   a plain value that carries no auto dependence.
/// - A duplicate cell id resolves to its FIRST occurrence, matching the fold
///   order in `fold_dependent_cells` (stored order, first write wins the
///   subsequent reads).
pub(crate) fn dependent_cell_auto_reads(
    dependent_cells: &[(ValueCellId, CompiledExpr)],
    auto_params: &[AutoParam],
) -> HashMap<ValueCellId, HashSet<ValueCellId>> {
    let n = dependent_cells.len();
    if n == 0 {
        return HashMap::new();
    }

    let auto_ids: HashSet<&ValueCellId> = auto_params.iter().map(|ap| &ap.id).collect();

    let mut cell_index: HashMap<&ValueCellId, usize> = HashMap::with_capacity(n);
    for (i, (id, _)) in dependent_cells.iter().enumerate() {
        cell_index.entry(id).or_insert(i);
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
            } else if let Some(&ci) = cell_index.get(&r) {
                children.push(ci);
            }
            // else: a plain value with no auto dependence → ignored.
        }
        direct_autos.push(autos);
        child_cells.push(children);
    }

    // Iterative post-order DFS with memoization. `state`: 0 = unvisited,
    // 1 = on the current stack (in progress), 2 = resolved.
    let mut memo: Vec<Option<HashSet<ValueCellId>>> = vec![None; n];
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
                    // ancestor (a cycle). Union the resolved ones; a cycle
                    // contributes its partial set via the ancestor's own frame.
                    let mut set = direct_autos[top].clone();
                    for &child in &child_cells[top] {
                        if let Some(child_set) = &memo[child] {
                            set.extend(child_set.iter().cloned());
                        }
                    }
                    memo[top] = Some(set);
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

    let mut out: HashMap<ValueCellId, HashSet<ValueCellId>> = HashMap::with_capacity(n);
    for (i, (id, _)) in dependent_cells.iter().enumerate() {
        let set = memo[i].clone().unwrap_or_default();
        // First occurrence wins, matching `cell_index`.
        out.entry(id.clone()).or_insert(set);
    }
    out
}

/// Decompose a constraint problem into independent connected components.
///
/// Each component groups constraints that share auto parameters (directly
/// or transitively). Constraints that reference no auto parameters are
/// excluded from the decomposition.
///
/// The domain for each component is determined by classifying each
/// constraint's expression: unanimous domain → that domain, mixed → CrossDomain.
pub fn decompose_into_components(
    auto_params: &[AutoParam],
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    objective_refs: Option<&HashSet<ValueCellId>>,
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
        // Terminating at all IS the assertion; each cell must still report at
        // least its own directly-read auto.
        assert!(map.get(&ValueCellId::new("P", "x")).unwrap().contains(&ValueCellId::new("P", "a")));
        assert!(map.get(&ValueCellId::new("P", "y")).unwrap().contains(&ValueCellId::new("P", "b")));
    }

    #[test]
    fn dependent_cell_auto_reads_empty_input_is_empty() {
        assert!(dependent_cell_auto_reads(&[], &[auto("a")]).is_empty());
    }
}
