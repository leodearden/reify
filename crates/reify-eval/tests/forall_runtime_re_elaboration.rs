//! Runtime re-elaboration of statement-form `forall` over deferred-count
//! collection subs (task 2629; PRD criterion 7 second-half).
//!
//! Pins the runtime contract that supersedes the compile-time silent-skip half
//! of PRD criterion 7 — see also `forall_constraint_over_undef_count_collection_sub_emits_no_decls_no_error`
//! in `crates/reify-compiler/tests/forall_statement_lower_tests.rs`. When a
//! `forall v in <coll_sub>` declaration is compiled over a collection sub
//! whose count cell is initially undef/non-literal, the compiler emits zero
//! per-element constraints/connections and stashes a `CompiledForallTemplate`
//! describing the per-element body. Once `Engine::edit_param` makes the count
//! known, this test module asserts that per-element constraints / connections
//! materialise in the snapshot's graph, with the correct cell-id rewriting
//! (`v → coll_sub[i]`) and removal of stale prior emissions on count decrease.
//!
//! Tests in this module follow the lifecycle Undef → known-count and the
//! reverse, exercising the `EvaluationGraph::forall_templates` carrier and
//! the `engine_edit::edit_param` collection-count re-elaboration block that
//! drives the runtime emission.

use std::collections::{HashMap, HashSet};

use reify_compiler::CompiledModule;
use reify_eval::cache::NodeId;
use reify_eval::snapshot::Snapshot;
use reify_eval::Engine;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;
use reify_types::{CompiledExprKind, ConstraintNodeId, Value, ValueCellId};

/// Convenience: parse + compile a single-source string via the shared
/// test-support helper. Mirrors the `compile_source` helper in
/// `eval_param_overrides.rs`.
fn compile_source(source: &str) -> CompiledModule {
    parse_and_compile(source)
}

/// Build an Engine with an empty prelude for self-contained forall-runtime tests.
fn fresh_engine() -> Engine {
    Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
}

/// Canonical fixture source for the runtime re-elaboration tests.
///
/// `S.n` has no default — the synthesized `__count_vents` cell is therefore
/// initially Undef so the count is unknown at first eval and the compile-time
/// `forall_templates` capture path applies. After `edit_param(n, Int(N))`,
/// `__count_vents` becomes Int(N) and the runtime re-elaboration must emit
/// `N` per-element `forall@v[i]`-labelled constraints into the snapshot's
/// graph, each referencing `S.vents[i].mass`.
const FORALL_FIXTURE_SRC: &str = r#"
structure Vent {
    param mass : Scalar = 10kg
}
structure S {
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == n
    forall v in vents: constraint v.mass < 50kg
}
"#;

/// task-2629 step-8: pins that `Engine::edit_param` re-elaborates per-element
/// `forall` constraints when a deferred count cell becomes known.
///
/// Sequence:
/// 1. Compile + initial `eval()` — count is Undef ⇒ zero `forall@*` constraints.
/// 2. `edit_param(S.n, Int(3))` — count becomes 3.
/// 3. Assert exactly 3 ConstraintNodeData entries with labels
///    `forall@v[0]`, `forall@v[1]`, `forall@v[2]`.
/// 4. Each constraint's `expr` (a `BinOp { left: ValueRef(id), .. }` shape)
///    has `id.entity == "S.vents[i]"` for the matching `i`.
///
/// RED before step-9 wires the runtime re-emission block in `engine_edit.rs`.
#[test]
fn edit_param_count_undef_to_known_emits_per_element_forall_constraints() {
    let module = compile_source(FORALL_FIXTURE_SRC);
    let mut engine = fresh_engine();

    // (1) Initial eval: count cell is Undef ⇒ zero forall@* constraints.
    let _initial = engine.eval(&module);
    let initial_snapshot = engine.snapshot().expect("snapshot after initial eval");
    let initial_forall_count = initial_snapshot
        .graph
        .constraints
        .iter()
        .filter(|(_, n)| {
            n.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@"))
        })
        .count();
    assert_eq!(
        initial_forall_count, 0,
        "expected zero forall@* constraints when count is Undef, got {}",
        initial_forall_count
    );

    // (2) Edit param `S.n` to 3 — count cell becomes Int(3).
    let n_id = ValueCellId::new("S", "n");
    let _ = engine
        .edit_param(n_id, Value::Int(3))
        .expect("edit_param should succeed");

    // (3) Snapshot now carries exactly 3 forall@v[i] constraints.
    let snap = engine.snapshot().expect("snapshot after edit_param");
    let mut forall_labels: Vec<String> = snap
        .graph
        .constraints
        .iter()
        .filter_map(|(_, n)| n.label.clone())
        .filter(|s| s.starts_with("forall@"))
        .collect();
    forall_labels.sort();
    assert_eq!(
        forall_labels,
        vec![
            "forall@v[0]".to_string(),
            "forall@v[1]".to_string(),
            "forall@v[2]".to_string(),
        ],
        "expected exactly forall@v[0..2] labels after edit_param to Int(3)"
    );

    // (4) Each forall@v[i] constraint references S.vents[i].mass on its
    //     left-hand side (BinOp { left: ValueRef(id), .. }).
    for i in 0..3 {
        let label = format!("forall@v[{}]", i);
        let constraint = snap
            .graph
            .constraints
            .iter()
            .find(|(_, n)| n.label.as_deref() == Some(label.as_str()))
            .unwrap_or_else(|| panic!("missing constraint with label {}", label));

        let CompiledExprKind::BinOp { left, .. } = &constraint.1.expr.kind else {
            panic!(
                "expected BinOp at root of forall@v[{}].expr, got {:?}",
                i, constraint.1.expr.kind
            );
        };

        let CompiledExprKind::ValueRef(id) = &left.kind else {
            panic!(
                "expected ValueRef on LHS of forall@v[{}].expr, got {:?}",
                i, left.kind
            );
        };

        assert_eq!(
            id.entity,
            format!("S.vents[{}]", i),
            "forall@v[{}] LHS entity mismatch (expected S.vents[{}], got {})",
            i,
            i,
            id.entity
        );
        assert_eq!(
            id.member, "mass",
            "forall@v[{}] LHS member mismatch (expected mass, got {})",
            i, id.member
        );
    }
}

/// Helper: collect sorted `forall@*` labels from a snapshot's graph.
fn collect_forall_labels(snap: &Snapshot) -> Vec<String> {
    let mut labels: Vec<String> = snap
        .graph
        .constraints
        .iter()
        .filter_map(|(_, n)| n.label.clone())
        .filter(|s| s.starts_with("forall@"))
        .collect();
    labels.sort();
    labels
}

/// task-2629 step-10: pins that count-decrease removes stale per-element
/// constraints (not just overwrites them) and that `topology_fingerprint`
/// changes across each count transition.
///
/// Sequence:
/// 1. Compile + initial `eval()` (count=Undef ⇒ zero `forall@*` constraints).
/// 2. `edit_param(S.n, Int(3))` — capture `fingerprint_3`, assert exactly 3
///    `forall@v[0..2]` labels.
/// 3. `edit_param(S.n, Int(1))` — assert exactly `forall@v[0]` remains
///    (verify `forall@v[1]` and `forall@v[2]` are absent from the
///    `constraints` PersistentMap). Capture `fingerprint_1`; assert
///    `fingerprint_3 != fingerprint_1`.
/// 4. `edit_param(S.n, Int(0))` — assert zero `forall@*` labels remain;
///    capture `fingerprint_0`; assert `fingerprint_0 != fingerprint_1`.
#[test]
fn edit_param_count_decrease_removes_stale_forall_constraints_and_changes_fingerprint() {
    let module = compile_source(FORALL_FIXTURE_SRC);
    let mut engine = fresh_engine();
    let n_id = ValueCellId::new("S", "n");

    // (1) Initial eval — count Undef ⇒ zero forall@* constraints.
    let _ = engine.eval(&module);
    let initial_snap = engine.snapshot().expect("snapshot after initial eval");
    assert!(
        collect_forall_labels(initial_snap).is_empty(),
        "expected zero forall@* constraints when count is Undef"
    );

    // (2) edit_param(n, 3) ⇒ 3 per-element constraints.
    let _ = engine
        .edit_param(n_id.clone(), Value::Int(3))
        .expect("edit_param to 3 should succeed");
    let snap_3 = engine.snapshot().expect("snapshot after edit n=3");
    let fingerprint_3 = snap_3.topology_fingerprint;
    assert_eq!(
        collect_forall_labels(snap_3),
        vec![
            "forall@v[0]".to_string(),
            "forall@v[1]".to_string(),
            "forall@v[2]".to_string(),
        ],
        "expected forall@v[0..2] after edit_param to Int(3)"
    );

    // (3) edit_param(n, 1) ⇒ only forall@v[0] remains.
    let _ = engine
        .edit_param(n_id.clone(), Value::Int(1))
        .expect("edit_param to 1 should succeed");
    let snap_1 = engine.snapshot().expect("snapshot after edit n=1");
    let labels_1 = collect_forall_labels(snap_1);
    assert_eq!(
        labels_1,
        vec!["forall@v[0]".to_string()],
        "expected exactly forall@v[0] after count decrease to Int(1) (got {:?})",
        labels_1
    );
    // Verify forall@v[1] and forall@v[2] are *gone*, not just overwritten —
    // their absence in the `forall_labels` Vec already implies removal,
    // since each forall constraint is keyed by a unique ConstraintNodeId.
    let absent: Vec<&'static str> = ["forall@v[1]", "forall@v[2]"]
        .iter()
        .filter(|missing| {
            snap_1
                .graph
                .constraints
                .iter()
                .any(|(_, n)| n.label.as_deref() == Some(**missing))
        })
        .copied()
        .collect();
    assert!(
        absent.is_empty(),
        "stale forall labels should be removed but found {:?}",
        absent
    );
    let fingerprint_1 = snap_1.topology_fingerprint;
    assert_ne!(
        fingerprint_3, fingerprint_1,
        "topology_fingerprint must change across count transition 3 -> 1"
    );

    // (4) edit_param(n, 0) ⇒ zero forall@* constraints.
    let _ = engine
        .edit_param(n_id, Value::Int(0))
        .expect("edit_param to 0 should succeed");
    let snap_0 = engine.snapshot().expect("snapshot after edit n=0");
    let labels_0 = collect_forall_labels(snap_0);
    assert!(
        labels_0.is_empty(),
        "expected zero forall@* constraints after edit_param to Int(0) (got {:?})",
        labels_0
    );
    let fingerprint_0 = snap_0.topology_fingerprint;
    assert_ne!(
        fingerprint_1, fingerprint_0,
        "topology_fingerprint must change across count transition 1 -> 0"
    );
}

/// Helper: collect the `ConstraintNodeId`s of constraints whose label matches
/// `forall@<var>[<i>]` for the given variable, sorted by `i`. Used in tests
/// that need to capture the live IDs and re-check them after a count edit.
fn collect_forall_ids(snap: &Snapshot, variable: &str) -> Vec<ConstraintNodeId> {
    let prefix = format!("forall@{}[", variable);
    let mut entries: Vec<(usize, ConstraintNodeId)> = snap
        .graph
        .constraints
        .iter()
        .filter_map(|(id, n)| {
            let label = n.label.as_deref()?;
            let rest = label.strip_prefix(&prefix)?;
            let idx_str = rest.strip_suffix(']')?;
            let i: usize = idx_str.parse().ok()?;
            Some((i, id.clone()))
        })
        .collect();
    entries.sort_by_key(|(i, _)| *i);
    entries.into_iter().map(|(_, id)| id).collect()
}

/// task-2629 step-14: pins that count-decrease invalidates the engine cache
/// entries for prior per-element forall constraints (mirrors task 2184's
/// per-instance value-cell invalidation contract for `NodeId::Value`).
///
/// Sequence:
/// 1. Compile + initial `eval()` (count=Undef).
/// 2. `edit_param(S.n, Int(3))` — record the `ConstraintNodeId`s of the 3
///    emitted forall constraints (forall@v[0], [1], [2]).
/// 3. Inject a synthetic cache entry for the constraint id that will be
///    REMOVED on the next edit (forall@v[2]) via `cache_store_mut().put(...)`.
///    Confirm `cache_store().get(...)` finds it.
/// 4. `edit_param(S.n, Int(1))` — the runtime re-elaboration drains prior
///    emissions and calls `cache.invalidate(&NodeId::Constraint(id))` for
///    every drained id. Assert that for the 2 `ConstraintNodeId`s that were
///    removed (forall@v[1], forall@v[2]),
///    `engine.cache_store().get(&NodeId::Constraint(id))` returns `None` —
///    confirming the synthetic cache entry from step (3) has been cleared,
///    not just stale-replayable.
///
/// This pins the invalidation contract concretely: an actually-present cache
/// entry on a removed forall constraint id must be cleared, not preserved.
/// The synthetic-injection pattern is necessary because the eval pipeline
/// does not currently materialise `NodeId::Constraint` cache entries at
/// `edit_param` time — that doesn't change the contract, and a future
/// change that DOES populate them must still invalidate.
#[test]
fn edit_param_count_change_invalidates_prior_forall_constraint_cache() {
    use reify_eval::cache::{CachedResult, NodeCache};
    use reify_eval::deps::DependencyTrace;
    use reify_types::{DeterminacyState, Freshness, VersionId};

    let module = compile_source(FORALL_FIXTURE_SRC);
    let mut engine = fresh_engine();
    let n_id = ValueCellId::new("S", "n");

    // (1) Initial eval: count Undef ⇒ zero forall@* constraints.
    let _ = engine.eval(&module);

    // (2) Edit n=3, capture the 3 emitted ConstraintNodeIds in order.
    let _ = engine
        .edit_param(n_id.clone(), Value::Int(3))
        .expect("edit_param to 3 should succeed");
    let snap_3 = engine.snapshot().expect("snapshot after edit n=3");
    let ids_3 = collect_forall_ids(snap_3, "v");
    assert_eq!(
        ids_3.len(),
        3,
        "expected 3 forall@v[*] constraint ids after n=3 (got {})",
        ids_3.len()
    );

    // (3) Inject synthetic cache entries for the 2 ids that will be removed
    //     (forall@v[1] and forall@v[2]). Use a trivial CachedResult::Value(Bool(true))
    //     to give the test something concrete to observe being cleared.
    let removed_ids = vec![ids_3[1].clone(), ids_3[2].clone()];
    for id in &removed_ids {
        let entry = NodeCache::new(
            CachedResult::Value(Value::Bool(true), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace { reads: Vec::new() },
            VersionId(0),
        );
        engine
            .cache_store_mut()
            .put(NodeId::Constraint(id.clone()), entry);
    }
    // Confirm the synthetic entries are in place before the next edit.
    for id in &removed_ids {
        assert!(
            engine
                .cache_store()
                .get(&NodeId::Constraint(id.clone()))
                .is_some(),
            "synthetic cache entry for {} should be present before edit_param to 1",
            id
        );
    }

    // (4) Edit n=1: forall@v[1] and forall@v[2] are removed from the graph;
    //     their cache entries must be cleared by the runtime invalidation
    //     loop in engine_edit.rs.
    let _ = engine
        .edit_param(n_id, Value::Int(1))
        .expect("edit_param to 1 should succeed");

    for (i_in_3, id) in removed_ids.iter().enumerate() {
        let label_idx = i_in_3 + 1; // 1, 2 — the labels that were removed
        assert!(
            engine
                .cache_store()
                .get(&NodeId::Constraint(id.clone()))
                .is_none(),
            "expected cache entry for forall@v[{}] (id={}) to be invalidated after count decrease, but it was still present",
            label_idx,
            id
        );
    }
}

/// Fixture variant with an unrelated `other` param to exercise the precision
/// contract pinned by step-16. `other` is a top-level Param with no
/// dependency on the count cell or the forall body, so editing it must
/// leave the forall emission ledger untouched.
const FORALL_FIXTURE_SRC_WITH_UNRELATED_PARAM: &str = r#"
structure Vent {
    param mass : Scalar = 10kg
}
structure S {
    sub vents : List<Vent>
    param n : Int
    param other : Int = 5
    constraint vents.count == n
    forall v in vents: constraint v.mass < 50kg
}
"#;

/// task-2629 step-16: pins that editing a param UNRELATED to the
/// collection-count cell does NOT re-emit forall constraints. The existing
/// gating at `engine_edit.rs:1409` (`if new_count_val == old_count_val
/// { continue; }`) is the precision contract being pinned: only count-cell
/// changes drive forall runtime re-elaboration.
///
/// Sequence:
/// 1. Compile + initial `eval()` (count=Undef ⇒ zero forall emissions).
/// 2. `edit_param(S.n, Int(3))` — capture the 3 emitted ConstraintNodeIds
///    AND the snapshot's `forall_emitted` ledger.
/// 3. `edit_param(S.other, Int(7))` — an unrelated param edit.
/// 4. Assert (a) the 3 forall ConstraintNodeIds are still present in the
///    new snapshot's graph AND identical to the pre-edit ids (id stability
///    across edits — captured by id-equality, not just count-equality), and
///    (b) the `forall_emitted` ledger is unchanged.
#[test]
fn edit_param_unrelated_param_does_not_re_emit_forall_constraints() {
    let module = compile_source(FORALL_FIXTURE_SRC_WITH_UNRELATED_PARAM);
    let mut engine = fresh_engine();
    let n_id = ValueCellId::new("S", "n");
    let other_id = ValueCellId::new("S", "other");

    // (1) Initial eval — count Undef, zero forall emissions.
    let _ = engine.eval(&module);

    // (2) Edit n=3 — capture the 3 emitted ConstraintNodeIds and ledger.
    let _ = engine
        .edit_param(n_id, Value::Int(3))
        .expect("edit_param(n, 3) should succeed");
    let snap_after_n3 = engine.snapshot().expect("snapshot after edit n=3");
    let ids_after_n3 = collect_forall_ids(snap_after_n3, "v");
    assert_eq!(
        ids_after_n3.len(),
        3,
        "expected 3 forall@v[*] constraint ids after n=3 (got {})",
        ids_after_n3.len()
    );
    let ledger_after_n3 = snap_after_n3.forall_emitted.clone();

    // (3) Edit an unrelated param — must NOT trigger forall re-emission.
    let _ = engine
        .edit_param(other_id, Value::Int(7))
        .expect("edit_param(other, 7) should succeed");

    // (4a) The 3 forall constraint ids are still present, identical (id
    //      stability proves "not re-emitted", not just "count preserved").
    let snap_after_other = engine
        .snapshot()
        .expect("snapshot after edit other");
    let ids_after_other = collect_forall_ids(snap_after_other, "v");
    assert_eq!(
        ids_after_other, ids_after_n3,
        "forall constraint ids must be identical across an unrelated param edit (was {:?}, now {:?})",
        ids_after_n3, ids_after_other
    );

    // (4b) The forall_emitted ledger is unchanged.
    assert_eq!(
        snap_after_other.forall_emitted, ledger_after_n3,
        "forall_emitted ledger must be unchanged across an unrelated param edit"
    );
}

/// task-2629 step-20: end-to-end lifecycle test that pins the full
/// Undef → known → Undef-equivalent (count=0) → known cycle. Mirrors the
/// task-958 `edit_param_count_int_undef_undef_int_transition` regression
/// coverage in `collection_sub_eval.rs` for value cells, lifted to the
/// per-element forall constraint emission ledger.
///
/// Sequence: Undef → 3 → 0 → 2. At each step the exact set of
/// `forall@v[*]` labels must match:
///   1. Undef ⇒ {}
///   2. Int(3) ⇒ {forall@v[0], forall@v[1], forall@v[2]}
///   3. Int(0) ⇒ {}
///   4. Int(2) ⇒ {forall@v[0], forall@v[1]}
///
/// Confirms (a) Int(0) clears prior emissions just as Undef would; (b) a
/// re-grow after a count-0 still works (forall_emitted ledger must be
/// drained, then re-populated with the new fresh ids); (c) the
/// idempotency of the drain-then-emit pair across a full lifecycle.
#[test]
fn full_lifecycle_undef_to_three_to_zero_to_two_per_element_constraints() {
    let module = compile_source(FORALL_FIXTURE_SRC);
    let mut engine = fresh_engine();
    let n_id = ValueCellId::new("S", "n");

    // (1) Initial eval — count Undef.
    let _ = engine.eval(&module);
    let snap_initial = engine.snapshot().expect("initial snapshot");
    assert!(
        collect_forall_labels(snap_initial).is_empty(),
        "expected zero forall@v[*] when count is Undef"
    );

    // (2) edit n=3 ⇒ 3 emissions.
    let _ = engine
        .edit_param(n_id.clone(), Value::Int(3))
        .expect("edit_param(n, 3) should succeed");
    let snap_3 = engine.snapshot().expect("snapshot after edit n=3");
    assert_eq!(
        collect_forall_labels(snap_3),
        vec![
            "forall@v[0]".to_string(),
            "forall@v[1]".to_string(),
            "forall@v[2]".to_string(),
        ],
        "expected forall@v[0..2] after edit_param to Int(3)"
    );

    // (3) edit n=0 ⇒ all emissions cleared.
    let _ = engine
        .edit_param(n_id.clone(), Value::Int(0))
        .expect("edit_param(n, 0) should succeed");
    let snap_0 = engine.snapshot().expect("snapshot after edit n=0");
    assert!(
        collect_forall_labels(snap_0).is_empty(),
        "expected zero forall@v[*] after edit_param to Int(0) (got {:?})",
        collect_forall_labels(snap_0)
    );

    // (4) edit n=2 ⇒ exactly forall@v[0..1] re-emitted from the cleared state.
    let _ = engine
        .edit_param(n_id, Value::Int(2))
        .expect("edit_param(n, 2) should succeed");
    let snap_2 = engine.snapshot().expect("snapshot after edit n=2");
    assert_eq!(
        collect_forall_labels(snap_2),
        vec!["forall@v[0]".to_string(), "forall@v[1]".to_string()],
        "expected forall@v[0..1] after edit_param to Int(2) following Int(0)"
    );
}

/// Fixture with TWO `forall` declarations sharing the same `(variable,
/// parent_entity, collection_sub_name)` triple — `(v, S, vents)` — but
/// referring to different members in their bodies (`v.mass` vs
/// `v.length`). Used to pin the reviewer-flagged ID-collision concern
/// (reviewer_comprehensive correctness/id_collision).
const FORALL_FIXTURE_SRC_TWO_FORALLS_SAME_TRIPLE: &str = r#"
structure Vent {
    param mass : Scalar = 10kg
    param length : Scalar = 1m
}
structure S {
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == n
    forall v in vents: constraint v.mass < 50kg
    forall v in vents: constraint v.length < 5m
}
"#;

/// task-2629 step-25: pin the reviewer-flagged ID-collision concern in the
/// runtime forall re-emission code in `engine_edit.rs`. Two
/// `CompiledForallTemplate`s sharing the same
/// `(variable, parent_entity, collection_sub_name)` triple currently produce
/// identical `ConstraintNodeId`s because `cnid_entity` is built only from
/// that triple, so the second template's emissions silently overwrite the
/// first's in `graph.constraints` (and the first template's
/// `forall_emitted` ledger holds IDs that are also owned by the second
/// template, breaking drain-on-decrease).
///
/// Sequence + assertions:
/// 1. Initial `engine.eval()` — count is Undef ⇒ zero `forall@*` constraints.
/// 2. `edit_param(S.n, Int(2))` — assert exactly **4 distinct
///    `ConstraintNodeId`s** whose label starts with `forall@v[`. With the
///    bug present, only 2 IDs would exist (the second template overwrites
///    the first at the same IDs).
/// 3. Partition the 4 constraints by the `member` field of the LHS
///    `ValueRef` extracted from each constraint's `BinOp`: the partition
///    must be `{mass: 2, length: 2}` (each forall contributes its full
///    set of per-element emissions, with no cross-pollution).
/// 4. `edit_param(S.n, Int(1))` — assert exactly **2 distinct
///    `ConstraintNodeId`s** remain in `graph.constraints` with `forall@v[*]`
///    labels, one per template (both at element index 0). With the bug
///    present, the drain-on-decrease path would misfire because the first
///    template's `forall_emitted[0]` ledger holds IDs also owned by the
///    second template — the assertion would fail with either fewer than 2
///    (over-cleanup) or stale IDs leaking through from the prior
///    4-element snapshot.
/// 5. `edit_param(S.n, Int(2))` again — assert 4 distinct IDs again with
///    the `{mass: 2, length: 2}` partition restored, pinning that the
///    cleanup-then-re-emit cycle is symmetric across both templates with
///    no leakage between their `forall_emitted` ledgers.
///
/// RED before step-26 disambiguates `cnid_entity` with the per-template
/// `t_idx`.
#[test]
fn edit_param_two_foralls_same_variable_same_collection_sub_emit_distinct_constraint_ids() {
    let module = compile_source(FORALL_FIXTURE_SRC_TWO_FORALLS_SAME_TRIPLE);
    let mut engine = fresh_engine();
    let n_id = ValueCellId::new("S", "n");

    // Helper: collect all (id, member) pairs for forall@v[*] constraints,
    // pulling the LHS member from `BinOp { left: ValueRef(id), .. }`.
    let collect_id_member_pairs = |snap: &Snapshot| -> Vec<(ConstraintNodeId, String)> {
        snap.graph
            .constraints
            .iter()
            .filter_map(|(id, n)| {
                let label = n.label.as_deref()?;
                if !label.starts_with("forall@v[") {
                    return None;
                }
                let CompiledExprKind::BinOp { left, .. } = &n.expr.kind else {
                    return None;
                };
                let CompiledExprKind::ValueRef(vid) = &left.kind else {
                    return None;
                };
                Some((id.clone(), vid.member.clone()))
            })
            .collect()
    };

    // (1) Initial eval: count Undef ⇒ zero forall@v[*] constraints.
    let _ = engine.eval(&module);
    let initial_snap = engine.snapshot().expect("initial snapshot");
    assert!(
        collect_forall_labels(initial_snap).is_empty(),
        "expected zero forall@v[*] when count is Undef"
    );

    // (2) edit n=2 ⇒ 4 distinct ConstraintNodeIds (2 per template × 2 templates).
    let _ = engine
        .edit_param(n_id.clone(), Value::Int(2))
        .expect("edit_param(n, 2) should succeed");
    let snap_2 = engine.snapshot().expect("snapshot after edit n=2");
    let pairs_2 = collect_id_member_pairs(snap_2);
    let ids_2: HashSet<ConstraintNodeId> = pairs_2.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(
        ids_2.len(),
        4,
        "expected 4 distinct ConstraintNodeIds for two foralls × 2 elements (got {} — id-collision bug present?)",
        ids_2.len()
    );

    // (3) Partition by member: each forall contributed its 2 emissions.
    let mut by_member: HashMap<String, usize> = HashMap::new();
    for (_, member) in &pairs_2 {
        *by_member.entry(member.clone()).or_insert(0) += 1;
    }
    let mut expected: HashMap<String, usize> = HashMap::new();
    expected.insert("mass".to_string(), 2);
    expected.insert("length".to_string(), 2);
    assert_eq!(
        by_member, expected,
        "expected per-member partition {{mass: 2, length: 2}}, got {:?} (cross-pollution between foralls?)",
        by_member
    );

    // (4) edit n=1 ⇒ exactly 2 distinct ConstraintNodeIds remain (one per
    //     template, both at element index 0). Drain-on-decrease must not
    //     leak across the per-template ledgers.
    let _ = engine
        .edit_param(n_id.clone(), Value::Int(1))
        .expect("edit_param(n, 1) should succeed");
    let snap_1 = engine.snapshot().expect("snapshot after edit n=1");
    let pairs_1 = collect_id_member_pairs(snap_1);
    let ids_1: HashSet<ConstraintNodeId> = pairs_1.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(
        ids_1.len(),
        2,
        "expected exactly 2 distinct ConstraintNodeIds after count decrease 2 → 1 (got {} — drain-on-decrease misfired?)",
        ids_1.len()
    );
    let mut by_member_1: HashMap<String, usize> = HashMap::new();
    for (_, member) in &pairs_1 {
        *by_member_1.entry(member.clone()).or_insert(0) += 1;
    }
    let mut expected_1: HashMap<String, usize> = HashMap::new();
    expected_1.insert("mass".to_string(), 1);
    expected_1.insert("length".to_string(), 1);
    assert_eq!(
        by_member_1, expected_1,
        "expected per-member partition {{mass: 1, length: 1}} after count decrease, got {:?}",
        by_member_1
    );

    // (5) edit n=2 again ⇒ 4 distinct IDs restored with the {mass:2, length:2}
    //     partition. Pins symmetry of cleanup-then-re-emit across both templates.
    let _ = engine
        .edit_param(n_id, Value::Int(2))
        .expect("second edit_param(n, 2) should succeed");
    let snap_2_again = engine.snapshot().expect("snapshot after second edit n=2");
    let pairs_2_again = collect_id_member_pairs(snap_2_again);
    let ids_2_again: HashSet<ConstraintNodeId> =
        pairs_2_again.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(
        ids_2_again.len(),
        4,
        "expected 4 distinct ConstraintNodeIds after re-grow 1 → 2 (got {} — ledger leak between templates?)",
        ids_2_again.len()
    );
    let mut by_member_2_again: HashMap<String, usize> = HashMap::new();
    for (_, member) in &pairs_2_again {
        *by_member_2_again.entry(member.clone()).or_insert(0) += 1;
    }
    assert_eq!(
        by_member_2_again, expected,
        "expected per-member partition {{mass: 2, length: 2}} after re-grow, got {:?}",
        by_member_2_again
    );
}
