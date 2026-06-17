//! Undef-cause tracer: all-causes DAG walk (PRD undef-self-describing β, task 4322).
//!
//! Reconstructs the **complete, deduplicated** set of root [`UndefCause`]s for an
//! undef cell by following forward dependency edges through undef cells only,
//! cycle-guarded by a visited-cell set.
//!
//! # Design decisions (see plan.json)
//!
//! * **Free function `trace_undef_causes`**: takes explicit state (origins,
//!   dep_map, values, start) for synthetic-input unit-testing — no solver, no
//!   real eval required to exercise cycle / dedup / order invariants.
//! * **Thin `Engine::trace_undef_causes` wrapper** (in `engine_admin.rs`): the
//!   consumer-facing API for δ (CLI) / ε (GUI) / ζ (LSP).
//! * **Dedup by originating cell** (via the visited-cell set), NOT by
//!   `UndefCause` value — so two independent cells with an identical
//!   `SolveFailed{detail}` are both returned (PRD Q4).
//! * **Order-stability (B1)**: `DependencyMap::forward_reachable` returns cells
//!   sorted by `ValueCellId`; the result `Vec<UndefCause>` therefore inherits
//!   that order.

use std::collections::HashMap;

use reify_core::ValueCellId;
use reify_ir::{DeterminacyState, PersistentMap, UndefCause, Value};

use crate::deps::DependencyMap;

/// Reconstruct the complete set of root [`UndefCause`]s for `start`.
///
/// Walks forward dependencies from `start`, expanding only cells whose value is
/// undef (recurse predicate: `values.get(c) → v.is_undef()`; absent ⇒ treat as
/// undef per α's convention).  Collects each visited cell's recorded origin from
/// `origins`; cells with no recorded origin contribute nothing (they are
/// propagated undef cells — PRD A3).
///
/// # Deduplication
///
/// Dedup is by **originating cell** — each cell appears in the traversal at most
/// once (visited-set).  Two independent cells carrying an identical
/// `SolveFailed{detail}` are both returned.
///
/// # Order
///
/// Output is ordered by originating `ValueCellId` ascending, matching
/// `forward_reachable`'s sorted output (B1).
///
/// # Cycle safety
///
/// Cycles terminate via the visited-cell set inside `forward_reachable` (BT7).
pub fn trace_undef_causes(
    origins: &HashMap<ValueCellId, UndefCause>,
    dep_map: &DependencyMap,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    start: &ValueCellId,
) -> Vec<UndefCause> {
    // Walk forward through undef cells only.
    // absent-from-values ⇒ treat as undef (α convention: pre-seeded Unbound params
    // are (Undef, Undetermined) in snapshot.values but may be absent from the
    // engine-side map before first eval).
    let reachable = dep_map.forward_reachable(start, |c| {
        values.get(c).map(|(v, _)| v.is_undef()).unwrap_or(true)
    });

    // Collect origins for each reachable UNDEF cell.
    // `reachable` is sorted by ValueCellId, so the result is order-stable (B1).
    // Dedup is by originating cell (visited-set in forward_reachable) — each cell
    // contributes at most one origin; same-detail SolveFailed on distinct cells
    // are both returned.
    //
    // Extra guard: `forward_reachable` includes boundary cells (determined cells
    // that were visited but not expanded past). Filter those out — only undef
    // cells can be undef-cause originators (B4: determined inputs never appear).
    //
    // Note on the double `values.get(c)...unwrap_or(true)` lookup: the same
    // predicate runs inside `forward_reachable`'s `should_expand` closure and
    // again in the `.filter(...)` below. This is intentional (belt-and-suspenders):
    // `forward_reachable` visits boundary cells without expanding past them, so
    // the second filter is the correct place to exclude them from cause
    // collection. The double read-only lookup is negligible on this tooling path.
    reachable
        .iter()
        .filter(|c| values.get(*c).map(|(v, _)| v.is_undef()).unwrap_or(true))
        .filter_map(|c| origins.get(c).cloned())
        .collect()
}

/// Render the complete root-cause set as a terse body string, or `None` when empty.
///
/// Maps each [`UndefCause`] variant to a short phrase and joins them with `", "`,
/// preserving input order (the tracer already returns a sorted, deduplicated
/// `Vec<UndefCause>` — see [β's order-stability invariant B1]).
///
/// # Return value
///
/// - `None` — the slice is empty (cell is determined, or capture was off).
/// - `Some(body)` — e.g. `"outer_d unbound, wall_ratio unbound"`.
///
/// **Surfaces wrap**: callers (LSP, CLI, GUI) prepend their own framing
/// (e.g. `"undef because: <body>"`); this function owns only the body so
/// all surfaces produce byte-identical cause sets (PRD S4 / §11 Q5).
pub fn format_undef_causes(causes: &[reify_ir::UndefCause]) -> Option<String> {
    if causes.is_empty() {
        return None;
    }
    let phrases: Vec<String> = causes
        .iter()
        .map(|cause| match cause {
            reify_ir::UndefCause::Unbound { param, .. } => {
                format!("{} unbound", param.member)
            }
            reify_ir::UndefCause::AwaitingSolve { param } => {
                format!("{} awaiting solve", param.member)
            }
            reify_ir::UndefCause::SolveFailed { detail } => {
                format!("solve failed: {detail}")
            }
            reify_ir::UndefCause::OpContractFailed { code, .. } => {
                format!("op contract failed ({code:?})")
            }
            reify_ir::UndefCause::UserUndef { .. } => "explicit undef".to_string(),
        })
        .collect();
    Some(phrases.join(", "))
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use reify_core::{SourceSpan, ValueCellId};
    use reify_ir::{DeterminacyState, PersistentMap, UndefCause, Value};

    use super::{format_undef_causes, trace_undef_causes};
    use crate::deps::DependencyMap;

    // ── format_undef_causes tests ─────────────────────────────────────────────

    /// (a) empty slice → None.
    #[test]
    fn format_empty_is_none() {
        assert_eq!(format_undef_causes(&[]), None);
    }

    /// (b) Two Unbound causes → joined with ", " in input order.
    #[test]
    fn format_two_unbound_joined_in_order() {
        let a = ValueCellId::new("S", "a");
        let b = ValueCellId::new("S", "b");
        let causes = vec![
            UndefCause::Unbound { param: a.clone(), span: SourceSpan::empty(0) },
            UndefCause::Unbound { param: b.clone(), span: SourceSpan::empty(0) },
        ];
        let body = format_undef_causes(&causes).expect("non-empty causes must return Some");
        // Both member names must appear with " unbound"
        assert!(
            body.contains("a unbound"),
            "expected 'a unbound' in {body:?}"
        );
        assert!(
            body.contains("b unbound"),
            "expected 'b unbound' in {body:?}"
        );
        // Joined by ", " and in input order (a before b)
        let a_pos = body.find("a unbound").unwrap();
        let b_pos = body.find("b unbound").unwrap();
        assert!(a_pos < b_pos, "expected 'a unbound' before 'b unbound' in {body:?}");
    }

    /// (c) UserUndef renders a non-empty phrase containing "undef".
    #[test]
    fn format_user_undef_contains_undef() {
        let causes = vec![UndefCause::UserUndef { span: SourceSpan::empty(0) }];
        let body = format_undef_causes(&causes).expect("non-empty causes must return Some");
        assert!(
            body.to_lowercase().contains("undef"),
            "expected 'undef' phrase in {body:?}"
        );
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn cell(entity: &str, field: &str) -> ValueCellId {
        ValueCellId::new(entity, field)
    }

    /// Build a DependencyMap from forward edges only (reverse is derived).
    fn make_dep_map(edges: &[(ValueCellId, Vec<ValueCellId>)]) -> DependencyMap {
        let mut forward: HashMap<ValueCellId, Vec<ValueCellId>> = HashMap::new();
        let mut reverse: HashMap<ValueCellId, Vec<ValueCellId>> = HashMap::new();

        for (c, deps) in edges {
            for d in deps {
                forward.entry(d.clone()).or_default();
                reverse.entry(d.clone()).or_default().push(c.clone());
            }
            forward.insert(c.clone(), deps.clone());
            reverse.entry(c.clone()).or_default();
        }

        DependencyMap { forward, reverse }
    }

    /// Insert a cell as undef (Value::Undef, Undetermined).
    fn undef_cell(
        values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        c: ValueCellId,
    ) {
        values.insert(c, (Value::Undef, DeterminacyState::Undetermined));
    }

    /// Insert a cell as determined (Value::Int(1), Determined).
    fn det_cell(
        values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        c: ValueCellId,
    ) {
        values.insert(c, (Value::Int(1), DeterminacyState::Determined));
    }

    // ── BT1: single root ──────────────────────────────────────────────────────

    /// BT1: forward={c:[a]}, a undef w/ origin Unbound → trace(c)=[Unbound a].
    #[test]
    fn single_root_unbound() {
        let a = cell("s", "a");
        let c = cell("s", "c");

        let dep_map = make_dep_map(&[(c.clone(), vec![a.clone()])]);

        let mut origins = HashMap::new();
        origins.insert(
            a.clone(),
            UndefCause::Unbound { param: a.clone(), span: SourceSpan::new(0, 3) },
        );

        let mut values = PersistentMap::new();
        undef_cell(&mut values, a.clone());
        undef_cell(&mut values, c.clone());

        let result = trace_undef_causes(&origins, &dep_map, &values, &c);
        assert_eq!(result.len(), 1, "BT1: expected 1 cause, got {:?}", result);
        assert!(
            matches!(&result[0], UndefCause::Unbound { param, .. } if param == &a),
            "BT1: expected Unbound(a), got {:?}",
            result
        );
    }

    // ── BT2 + diamond dedup ───────────────────────────────────────────────────

    /// Diamond dedup: forward={e:[c,d], c:[a,b], d:[a,b]}, a/b origins →
    /// trace(e)=[Unbound a, Unbound b] (a reached via both c and d, deduplicated).
    #[test]
    fn diamond_dedup() {
        let a = cell("s", "a");
        let b = cell("s", "b");
        let c = cell("s", "c");
        let d = cell("s", "d");
        let e = cell("s", "e");

        let dep_map = make_dep_map(&[
            (e.clone(), vec![c.clone(), d.clone()]),
            (c.clone(), vec![a.clone(), b.clone()]),
            (d.clone(), vec![a.clone(), b.clone()]),
        ]);

        let mut origins = HashMap::new();
        origins.insert(
            a.clone(),
            UndefCause::Unbound { param: a.clone(), span: SourceSpan::new(0, 1) },
        );
        origins.insert(
            b.clone(),
            UndefCause::Unbound { param: b.clone(), span: SourceSpan::new(2, 3) },
        );

        let mut values = PersistentMap::new();
        for c_id in [&a, &b, &c, &d, &e] {
            undef_cell(&mut values, c_id.clone());
        }

        let result = trace_undef_causes(&origins, &dep_map, &values, &e);

        assert_eq!(result.len(), 2, "diamond: expected 2 causes (deduped), got {:?}", result);
        assert!(
            result.iter().any(|r| matches!(r, UndefCause::Unbound { param, .. } if param == &a)),
            "diamond: must contain Unbound(a): {:?}",
            result
        );
        assert!(
            result.iter().any(|r| matches!(r, UndefCause::Unbound { param, .. } if param == &b)),
            "diamond: must contain Unbound(b): {:?}",
            result
        );
    }

    // ── BT3: chain collapse ───────────────────────────────────────────────────

    /// BT3: chain z→y→x, only x has origin Unbound → trace(z)=[Unbound x].
    #[test]
    fn chain_collapse() {
        let x = cell("s", "x");
        let y = cell("s", "y");
        let z = cell("s", "z");

        let dep_map = make_dep_map(&[
            (z.clone(), vec![y.clone()]),
            (y.clone(), vec![x.clone()]),
        ]);

        let mut origins = HashMap::new();
        origins.insert(
            x.clone(),
            UndefCause::Unbound { param: x.clone(), span: SourceSpan::new(0, 1) },
        );

        let mut values = PersistentMap::new();
        for c in [&x, &y, &z] {
            undef_cell(&mut values, c.clone());
        }

        let result = trace_undef_causes(&origins, &dep_map, &values, &z);

        assert_eq!(result.len(), 1, "chain: expected [Unbound x], got {:?}", result);
        assert!(
            matches!(&result[0], UndefCause::Unbound { param, .. } if param == &x),
            "chain: expected Unbound(x), got {:?}",
            result
        );
    }

    // ── B4: determined cell not reported ─────────────────────────────────────

    /// B4: a determined input is not expanded and never appears in the trace.
    #[test]
    fn determined_not_reported() {
        let a = cell("s", "a"); // determined input
        let c = cell("s", "c"); // undef propagated via a

        let dep_map = make_dep_map(&[(c.clone(), vec![a.clone()])]);

        let mut origins = HashMap::new();
        // 'a' has an origin but is determined — should NOT be reported.
        origins.insert(
            a.clone(),
            UndefCause::Unbound { param: a.clone(), span: SourceSpan::new(0, 1) },
        );

        let mut values = PersistentMap::new();
        det_cell(&mut values, a.clone()); // determined
        undef_cell(&mut values, c.clone()); // undef start

        // Trace from c: a is determined so not expanded → empty result.
        let result = trace_undef_causes(&origins, &dep_map, &values, &c);
        assert!(
            result.is_empty(),
            "B4: determined dep must not appear in trace, got {:?}",
            result
        );
    }

    /// B4: tracing a determined START cell → empty result.
    #[test]
    fn determined_start_empty() {
        let a = cell("s", "a");
        let dep_map = make_dep_map(&[]);

        let origins = HashMap::new();
        let mut values = PersistentMap::new();
        det_cell(&mut values, a.clone());

        let result = trace_undef_causes(&origins, &dep_map, &values, &a);
        assert!(result.is_empty(), "determined start must yield empty trace: {:?}", result);
    }

    // ── BT7: cycle terminates ─────────────────────────────────────────────────

    /// BT7: cycle a→b→a, both undef, origin on a → terminates, returns a's cause.
    #[test]
    fn cycle_terminates() {
        let a = cell("s", "a");
        let b = cell("s", "b");

        let dep_map = make_dep_map(&[
            (a.clone(), vec![b.clone()]),
            (b.clone(), vec![a.clone()]),
        ]);

        let mut origins = HashMap::new();
        origins.insert(
            a.clone(),
            UndefCause::Unbound { param: a.clone(), span: SourceSpan::new(0, 1) },
        );

        let mut values = PersistentMap::new();
        undef_cell(&mut values, a.clone());
        undef_cell(&mut values, b.clone());

        let result = trace_undef_causes(&origins, &dep_map, &values, &a);

        // Must terminate and return at least a's cause.
        assert!(
            result.iter().any(|r| matches!(r, UndefCause::Unbound { param, .. } if param == &a)),
            "BT7: must return a's cause: {:?}",
            result
        );
    }

    // ── INDEPENDENCE: same-detail SolveFailed on distinct cells ──────────────

    /// INDEPENDENCE: two independent cells each with SolveFailed{detail:"inf"} →
    /// BOTH returned, not collapsed (dedup by cell, not by cause value).
    #[test]
    fn independence_same_detail_solve_failed() {
        let x = cell("s", "x");
        let y = cell("s", "y");
        let z = cell("s", "z"); // z depends on x and y

        let dep_map = make_dep_map(&[(z.clone(), vec![x.clone(), y.clone()])]);

        let mut origins = HashMap::new();
        origins.insert(x.clone(), UndefCause::SolveFailed { detail: "infeasible".to_string() });
        origins.insert(y.clone(), UndefCause::SolveFailed { detail: "infeasible".to_string() });

        let mut values = PersistentMap::new();
        undef_cell(&mut values, x.clone());
        undef_cell(&mut values, y.clone());
        undef_cell(&mut values, z.clone());

        let result = trace_undef_causes(&origins, &dep_map, &values, &z);

        assert_eq!(result.len(), 2, "INDEPENDENCE: both SolveFailed cells must be returned: {:?}", result);
        assert!(
            result.iter().all(|r| matches!(r, UndefCause::SolveFailed { .. })),
            "INDEPENDENCE: both must be SolveFailed: {:?}",
            result
        );
    }

    // ── absent-from-values ⇒ treat as undef ──────────────────────────────────

    /// absent-from-values: a cell with a recorded origin that is entirely absent
    /// from `values` is still traversed and reported.
    ///
    /// This directly exercises the `unwrap_or(true)` fallback that upholds α's
    /// convention — pre-seeded Unbound params may be absent from the engine-side
    /// `snapshot.values` before first evaluation.  A future refactor flipping the
    /// default to `false` would silently break tracing of those params; this test
    /// guards against that regression.
    #[test]
    fn absent_from_values_treated_as_undef() {
        // Topology: top → mid → leaf
        // `leaf` has a recorded origin but is ABSENT from `values`.
        // `mid` and `top` are also absent — all must be treated as undef.
        let top = cell("s", "top");
        let mid = cell("s", "mid");
        let leaf = cell("s", "leaf");

        let dep_map = make_dep_map(&[
            (top.clone(), vec![mid.clone()]),
            (mid.clone(), vec![leaf.clone()]),
        ]);

        let mut origins = HashMap::new();
        origins.insert(
            leaf.clone(),
            UndefCause::Unbound { param: leaf.clone(), span: SourceSpan::new(0, 1) },
        );

        // Intentionally empty — no cell is seeded into `values`.
        // Every cell is absent; the unwrap_or(true) path must fire for each.
        let values: PersistentMap<ValueCellId, (Value, DeterminacyState)> = PersistentMap::new();

        let result = trace_undef_causes(&origins, &dep_map, &values, &top);

        assert_eq!(
            result.len(),
            1,
            "absent-from-values must be treated as undef and reported: got {:?}",
            result
        );
        assert!(
            matches!(&result[0], UndefCause::Unbound { param, .. } if param == &leaf),
            "absent-from-values: expected Unbound(leaf), got {:?}",
            result
        );
    }

    // ── ORDER-STABILITY: output ordered by ValueCellId ascending ─────────────

    /// ORDER-STABILITY: output ordered by ValueCellId ascending.
    #[test]
    fn order_stability() {
        // Three leaf cells with origins; one collector depending on all.
        let a = cell("s", "a");
        let b = cell("s", "b");
        let c_node = cell("s", "c");
        let top = cell("s", "top");

        let dep_map = make_dep_map(&[
            (top.clone(), vec![a.clone(), b.clone(), c_node.clone()]),
        ]);

        let mut origins = HashMap::new();
        origins.insert(
            a.clone(),
            UndefCause::Unbound { param: a.clone(), span: SourceSpan::new(0, 1) },
        );
        origins.insert(
            b.clone(),
            UndefCause::Unbound { param: b.clone(), span: SourceSpan::new(2, 3) },
        );
        origins.insert(
            c_node.clone(),
            UndefCause::Unbound { param: c_node.clone(), span: SourceSpan::new(4, 5) },
        );

        let mut values = PersistentMap::new();
        for c in [&a, &b, &c_node, &top] {
            undef_cell(&mut values, c.clone());
        }

        let result = trace_undef_causes(&origins, &dep_map, &values, &top);

        // Result must contain 3 causes.
        assert_eq!(result.len(), 3, "ORDER: expected 3 causes: {:?}", result);

        // The originating cells for the result must be in ascending order.
        // Extract params from Unbound variants (which store the cell id).
        let params: Vec<ValueCellId> = result
            .iter()
            .filter_map(|r| {
                if let UndefCause::Unbound { param, .. } = r { Some(param.clone()) } else { None }
            })
            .collect();
        let mut sorted = params.clone();
        sorted.sort();
        assert_eq!(params, sorted, "ORDER: result must be sorted by originating cell: {:?}", result);
    }
}
