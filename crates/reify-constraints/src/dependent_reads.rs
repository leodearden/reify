//! Which auto params each dependent cell reads, TRANSITIVELY.
//!
//! A dependent cell is a derived value the solver re-derives from the auto
//! params on every trial. The decomposition's connectivity, the registry's
//! per-component fold filter and the bound-derivation guard all need the autos
//! a cell reads through OTHER cells, not just its own expression; this module
//! is the one analysis they share.
//!
//! It is the bottom layer: it depends on the IR alone and on none of the
//! solver modules that consume it, so any of them can import it without
//! closing a module cycle.

use reify_core::ValueCellId;
use reify_ir::{AutoParam, CompiledExpr, CompiledExprKind};
use std::collections::{HashMap, HashSet};

/// Collect all ValueCellIds referenced in an expression tree.
///
/// `ValueRef` ids ONLY. Not `CompiledExpr::collect_value_refs`, which also
/// collects `CrossSubGeometryRef` ids and would change which refs count.
///
/// Delegates child traversal to `CompiledExpr::walk` — when new
/// `CompiledExprKind` variants are added, only `walk()` needs updating.
pub(crate) fn collect_value_refs(expr: &CompiledExpr, out: &mut HashSet<ValueCellId>) {
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
/// [`crate::decompose_into_components`] unions the auto params an objective
/// references SYNTACTICALLY. The canonical joint-drive shape (task #5189 β) is
/// an objective that reads a bare DERIVED cell and no auto at all, so that
/// union step sees an empty set and two autos coupled only through the derived
/// cell land in SEPARATE components. `SolverRegistry::solve_inner` feeds this map back into
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

/// The autos `refs` reaches THROUGH dependent cells, borrowed from
/// `auto_reads`.
///
/// `auto_reads` is already transitive, so one pass closes the set. The result
/// may hold duplicates and ids `refs` already contains; every consumer
/// tolerates both. D1/B2 IDENTITY: an empty `auto_reads` — what
/// [`dependent_cell_auto_reads`] returns for an empty `dependent_cells` —
/// reaches nothing, so every ref set, union edge and `referenced_params` list
/// downstream stays exactly the direct-only one.
pub(crate) fn reach_of<'m>(
    refs: &HashSet<ValueCellId>,
    auto_reads: &'m HashMap<ValueCellId, HashSet<ValueCellId>>,
) -> Vec<&'m ValueCellId> {
    if auto_reads.is_empty() {
        return Vec::new();
    }
    refs.iter()
        .filter_map(|id| auto_reads.get(id))
        .flatten()
        .collect()
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
