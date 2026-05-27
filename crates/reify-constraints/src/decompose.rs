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
    objective: Option<&CompiledExpr>,
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

    // If an objective expression is provided, union all auto params it
    // references. This ensures all objective-referenced params land in the
    // same component, even if the constraints alone don't connect them.
    if let Some(obj_expr) = objective {
        let mut obj_refs = HashSet::new();
        collect_value_refs(obj_expr, &mut obj_refs);

        let obj_param_indices: Vec<usize> = obj_refs
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
}
