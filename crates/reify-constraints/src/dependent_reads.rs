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

/// Per dependent cell, the auto params it reads TRANSITIVELY, and whether a
/// stored-order fold can derive its value at all. Built by
/// [`dependent_cell_auto_reads`].
#[derive(Debug, Default)]
pub(crate) struct DependentCellReads {
    cells: HashMap<ValueCellId, CellEntry>,
}

#[derive(Debug)]
struct CellEntry {
    autos: HashSet<ValueCellId>,
    foldable: bool,
}

/// What [`DependentCellReads::lookup`] knows about one id. Each consumer
/// chooses its own safe direction for [`CellReads::Unfoldable`] by name.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CellReads<'a> {
    /// Not a dependent cell: a plain value, or an auto param.
    NotACell,
    /// A cell a stored-order fold derives, and every auto it reads.
    Foldable(&'a HashSet<ValueCellId>),
    /// A cell on or downstream of a cycle: no stored-order fold can derive its
    /// value, though it still reads exactly these autos.
    Unfoldable(&'a HashSet<ValueCellId>),
}

impl DependentCellReads {
    pub(crate) fn lookup(&self, id: &ValueCellId) -> CellReads<'_> {
        match self.cells.get(id) {
            None => CellReads::NotACell,
            Some(entry) if entry.foldable => CellReads::Foldable(&entry.autos),
            Some(entry) => CellReads::Unfoldable(&entry.autos),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// For each dependent cell, the set of auto-param ids it reads TRANSITIVELY —
/// following `ValueRef`s through OTHER dependent cells, not just its own
/// expression — and whether a stored-order fold can derive it.
///
/// # Why this exists (task #5720)
///
/// [`crate::decompose_into_components`] unions the auto params an objective
/// references SYNTACTICALLY. The canonical joint-drive shape (task #5189 β) is
/// an objective that reads a bare DERIVED cell and no auto at all, so that
/// union step sees an empty set and two autos coupled only through the derived
/// cell land in SEPARATE components. `SolverRegistry`'s decomposition prelude
/// expands the objective's refs through this map before decomposing, so
/// decomposition follows `dependent_cells` and the coupled autos are solved
/// jointly. It also uses the map as the per-component fold filter: a component
/// folds a cell only when it OWNS every auto that cell transitively reads,
/// which is what makes a cross-component `Undef` fold structurally impossible.
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
/// - EVERY dependent cell is present, with its EXACT transitive auto set. A
///   cell that closes a back edge, or transitively reads one, is UNFOLDABLE:
///   its value depends on a cycle that no stored-order fold can derive. It is
///   classified, not omitted, because an absence would read as "not a cell"
///   to every consumer, while each needs its own safe direction: the fold
///   filter drops such a cell, connectivity couples through it, and the
///   bound-derivation guard treats it as varying. The DFS memo is partial for
///   such a cell, so a reachability walk re-derives its set; reify-eval's
///   `build_dependent_cells` drops cycles upstream, so on a well-formed
///   problem that walk never runs.
/// - Iterative (explicit stack), so a deep dependent-cell chain cannot blow the
///   native stack.
/// - A ref that is neither an auto nor another dependent cell is ignored: it is
///   a plain value that carries no auto dependence.
/// - A duplicate cell id resolves to the UNION over ALL of its occurrences —
///   both as a child edge (a ref to that id inherits every occurrence's set)
///   and in the returned map — and is UNFOLDABLE if ANY occurrence is.
///   First-occurrence-wins would be unsafe in this map's PRIMARY consumer: the
///   registry filter keys on id, so every occurrence of a duplicated cell is
///   retained or dropped TOGETHER. Were a later occurrence to read a strictly
///   larger auto set, first-wins would keep both in a component that does not
///   own one of those autos and the fold would read it unbound.
pub(crate) fn dependent_cell_auto_reads(
    dependent_cells: &[(ValueCellId, CompiledExpr)],
    auto_params: &[AutoParam],
) -> DependentCellReads {
    let n = dependent_cells.len();
    if n == 0 {
        return DependentCellReads::default();
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
    // reads. Such a cell is UNFOLDABLE, and its exact set is re-derived below.
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

    let exact: Vec<(usize, HashSet<ValueCellId>)> = (0..n)
        .filter(|&i| incomplete[i])
        .map(|i| {
            (
                i,
                exact_reach(i, &direct_autos, &child_cells, &memo, &incomplete),
            )
        })
        .collect();
    for (i, reach) in exact {
        memo[i] = Some(reach);
    }

    // Materialise. `take()` MOVES each memoised set out — every index is
    // materialised exactly once — so the map never holds a second copy of the
    // DFS's working sets.
    let mut cells: HashMap<ValueCellId, CellEntry> = HashMap::with_capacity(n);
    for (i, (id, _)) in dependent_cells.iter().enumerate() {
        let entry = cells.entry(id.clone()).or_insert_with(|| CellEntry {
            autos: HashSet::new(),
            foldable: true,
        });
        // UNION across every occurrence of a duplicated id, matching the
        // all-occurrences child edges above; ANY unfoldable occurrence makes
        // the id unfoldable.
        entry.autos.extend(memo[i].take().unwrap_or_default());
        entry.foldable &= !incomplete[i];
    }
    DependentCellReads { cells }
}

/// The exact transitive auto set of `start`, a cell the DFS left `incomplete`:
/// a cycle-safe reachability walk that takes a COMPLETE cell's memo whole
/// rather than walking beneath it (a complete cell never reaches an incomplete
/// one, or it would have inherited the taint).
fn exact_reach(
    start: usize,
    direct_autos: &[HashSet<ValueCellId>],
    child_cells: &[Vec<usize>],
    memo: &[Option<HashSet<ValueCellId>>],
    incomplete: &[bool],
) -> HashSet<ValueCellId> {
    let mut reach = HashSet::new();
    let mut visited = vec![false; direct_autos.len()];
    visited[start] = true;
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        if !incomplete[node] {
            reach.extend(memo[node].iter().flatten().cloned());
            continue;
        }
        reach.extend(direct_autos[node].iter().cloned());
        for &child in &child_cells[node] {
            if !visited[child] {
                visited[child] = true;
                stack.push(child);
            }
        }
    }
    reach
}

/// The autos `refs` reaches THROUGH dependent cells, split by whether the cell
/// that reaches them is foldable; ids are borrowed from `reads`.
#[derive(Debug, Default)]
pub(crate) struct Reach<'m> {
    /// Reached through cells a stored-order fold derives.
    pub(crate) foldable: Vec<&'m ValueCellId>,
    /// Reached through UNFOLDABLE cells.
    pub(crate) unfoldable: Vec<&'m ValueCellId>,
    /// Some ref is an unfoldable cell — true even when that cell reaches no
    /// auto.
    pub(crate) reads_unfoldable_cell: bool,
}

impl<'m> Reach<'m> {
    /// Every auto reached, through foldable and unfoldable cells alike.
    pub(crate) fn all(&self) -> impl Iterator<Item = &'m ValueCellId> {
        self.foldable.iter().chain(&self.unfoldable).copied()
    }
}

/// The [`Reach`] of `refs`. `reads` is already transitive, so one pass closes
/// the set. The result may hold duplicates and ids `refs` already contains;
/// every consumer tolerates both.
///
/// D1/B2 IDENTITY: an empty `reads` — what [`dependent_cell_auto_reads`]
/// returns for an empty `dependent_cells` — reaches nothing, so every ref set,
/// union edge and `referenced_params` list downstream stays exactly the
/// direct-only one.
pub(crate) fn reach_of<'m>(
    refs: &HashSet<ValueCellId>,
    reads: &'m DependentCellReads,
) -> Reach<'m> {
    let mut reach = Reach::default();
    if reads.is_empty() {
        return reach;
    }
    for id in refs {
        match reads.lookup(id) {
            CellReads::NotACell => {}
            CellReads::Foldable(autos) => reach.foldable.extend(autos),
            CellReads::Unfoldable(autos) => {
                reach.reads_unfoldable_cell = true;
                reach.unfoldable.extend(autos);
            }
        }
    }
    reach
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

    fn autos<const N: usize>(names: [&str; N]) -> HashSet<ValueCellId> {
        names
            .into_iter()
            .map(|n| ValueCellId::new("P", n))
            .collect()
    }

    #[test]
    fn dependent_cell_auto_reads_direct_auto() {
        let cells = vec![(ValueCellId::new("P", "total"), vref("a"))];
        let map = dependent_cell_auto_reads(&cells, &[auto("a")]);
        assert_eq!(
            map.lookup(&ValueCellId::new("P", "total")),
            CellReads::Foldable(&autos(["a"]))
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
            map.lookup(&ValueCellId::new("P", "total")),
            CellReads::Foldable(&autos(["a", "b"])),
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
            map.lookup(&ValueCellId::new("P", "total")),
            CellReads::Foldable(&autos(["a", "b"]))
        );
    }

    #[test]
    fn dependent_cell_auto_reads_ignores_non_auto_non_dependent_refs() {
        let cells = vec![(ValueCellId::new("P", "total"), vref("plain"))];
        let map = dependent_cell_auto_reads(&cells, &[auto("a")]);
        assert_eq!(
            map.lookup(&ValueCellId::new("P", "total")),
            CellReads::Foldable(&HashSet::new()),
            "a ref that is neither an auto nor another dependent cell carries \
             no auto dependence"
        );
        assert_eq!(
            map.lookup(&ValueCellId::new("P", "plain")),
            CellReads::NotACell
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

        // Terminating at all is half the assertion. The other half is that each
        // cycle member is published UNFOLDABLE with its EXACT reach, never the
        // partial set the DFS naturally accumulates for whichever member
        // resolves first. A partial `y` (apparent reads {b}) would let a
        // connectivity consumer leave `a` out of the component that must
        // couple it.
        for name in ["x", "y"] {
            assert_eq!(
                map.lookup(&ValueCellId::new("P", name)),
                CellReads::Unfoldable(&autos(["a", "b"])),
                "`{name}` is on a cycle, so no stored-order fold derives it, \
                 and it transitively reads BOTH autos",
            );
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
            map.lookup(&dup),
            CellReads::Foldable(&autos(["a", "b"])),
            "a duplicated cell id must resolve to the union over ALL of its \
             occurrences — the drop-side-safe direction"
        );
    }

    #[test]
    fn a_duplicate_id_with_one_cyclic_occurrence_is_unfoldable_with_the_union() {
        // `total` twice: first reading `a` (foldable), then `spin + b`, where
        // `spin = spin` is a cycle reaching no auto. The id is only as foldable
        // as its WEAKEST occurrence, and its reach is still the union.
        let dup = ValueCellId::new("P", "total");
        let cells = vec![
            (dup.clone(), vref("a")),
            (
                dup.clone(),
                CompiledExpr::binop(BinOp::Add, vref("spin"), vref("b"), Type::length()),
            ),
            (ValueCellId::new("P", "spin"), vref("spin")),
        ];
        let map = dependent_cell_auto_reads(&cells, &[auto("a"), auto("b")]);
        assert_eq!(
            map.lookup(&dup),
            CellReads::Unfoldable(&autos(["a", "b"])),
            "one cyclic occurrence makes the id unfoldable, and its reach is \
             the union over every occurrence: `Foldable({{a}})` is \
             first-occurrence-wins, `Unfoldable({{b}})` last-occurrence-wins",
        );
        assert_eq!(
            map.lookup(&ValueCellId::new("P", "spin")),
            CellReads::Unfoldable(&HashSet::new()),
            "a cycle that reaches no auto is still unfoldable",
        );
    }

    #[test]
    fn dependent_cell_auto_reads_empty_input_is_empty() {
        assert!(dependent_cell_auto_reads(&[], &[auto("a")]).is_empty());
    }
}
