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

use reify_compiler::CompiledModule;
use reify_eval::Engine;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;
use reify_types::{CompiledExprKind, Value, ValueCellId};

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
