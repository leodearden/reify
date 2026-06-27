//! Eval-layer pin for `forall v in <keyed_coll>: constraint ...` (task 3933 ε).
//!
//! Confirms that the compile-emitted per-member constraints carry through
//! the eval engine and apply to every keyed member. This is the closest
//! unit-testable proxy for the CLI-eval consumer — GREEN-on-arrival after
//! step-2's compiler fix (same mechanism, higher layer).
//!
//! Asserts:
//!   (a) snapshot.graph.constraints carries exactly 2 forall@v[*] constraints
//!   (b) each constraint's ValueRef LHS references the correct keyed entity
//!       in insertion order (runtime determinism)
//!   (c) the keyed member cells resolve to their per-key override values
//!   (d) eval does not panic
//!
//! User-observable signal:
//!   cargo test -p reify-eval --test keyed_forall_eval

use reify_core::ValueCellId;
use reify_ir::{BinOp, CompiledExprKind, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Source fixture: Manifold with a 2-key Keyed<Vent> sub and a value-binder
/// forall constraint. Both keys satisfy the constraint (5mm > 1mm, 8mm > 1mm).
const KEYED_FORALL_SRC: &str = r#"
structure def Vent {
    param area : Length = 1mm
}
structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake"  => { area = 5mm }
        "exhaust" => { area = 8mm }
    }
    forall v in vents: constraint v.area > 1mm
    let a = vents["intake"].area
    let b = vents["exhaust"].area
}
"#;

/// Eval-layer pin: `forall v in vents: constraint v.area > 1mm` over a
/// 2-key `Keyed<Vent>` sub materialises exactly 2 `forall@v[*]` constraints
/// in the snapshot graph, each referencing the correct keyed entity's area
/// cell in insertion order, and both keyed cells resolve to their override
/// values.
#[test]
fn keyed_forall_eval_emits_per_member_constraints_and_resolves_cells() {
    let module = parse_and_compile_with_stdlib(KEYED_FORALL_SRC);
    let mut engine = make_simple_engine();
    let result = engine.eval(&module);

    // (d) no panic — reaching this line proves eval completed.

    // (a) Exactly 2 forall@v[*] constraints in the snapshot graph.
    let snap = engine.snapshot().expect("snapshot must be available after eval");
    let mut forall_labels: Vec<String> = snap
        .graph
        .constraints
        .iter()
        .filter_map(|(_, n)| n.label.clone())
        .filter(|s| s.starts_with("forall@v["))
        .collect();
    // Sort before comparing: assertion (a) only verifies that the label SET is
    // exactly {forall@v[0], forall@v[1]}, not the iteration order. Ordering is
    // the subject of assertion (b) below (each label is looked up by name and
    // its LHS ValueRef entity is checked in insertion order). The sort here
    // makes (a) resilient to any non-determinism in `constraints` map
    // iteration while keeping the count/membership signal clear.
    forall_labels.sort();
    assert_eq!(
        forall_labels,
        vec!["forall@v[0]".to_string(), "forall@v[1]".to_string()],
        "expected exactly forall@v[0] and forall@v[1] in snapshot, got {:?}",
        forall_labels
    );

    // (b) Each constraint's LHS ValueRef points to the correct keyed entity
    //     in insertion order (intake → exhaust).
    let expected = [
        ("forall@v[0]", "Manifold.vents[\"intake\"]"),
        ("forall@v[1]", "Manifold.vents[\"exhaust\"]"),
    ];
    for (label, expected_entity) in &expected {
        let constraint = snap
            .graph
            .constraints
            .iter()
            .find(|(_, n)| n.label.as_deref() == Some(*label))
            .unwrap_or_else(|| panic!("missing constraint with label {label}"));

        let CompiledExprKind::BinOp { op, left, .. } = &constraint.1.expr.kind else {
            panic!(
                "expected BinOp at root of {label}.expr, got {:?}",
                constraint.1.expr.kind
            );
        };
        assert_eq!(
            *op,
            BinOp::Gt,
            "{label}: expected BinOp::Gt, got {op:?}"
        );

        let CompiledExprKind::ValueRef(id) = &left.kind else {
            panic!("expected ValueRef on LHS of {label}, got {:?}", left.kind);
        };
        assert_eq!(
            id.entity, *expected_entity,
            "{label}: LHS entity mismatch (expected {expected_entity}, got {})",
            id.entity
        );
        assert_eq!(
            id.member, "area",
            "{label}: LHS member mismatch (expected area, got {})",
            id.member
        );
    }

    // (c) Keyed member cells resolve to their per-key override values.
    let intake_area = result
        .values
        .get(&ValueCellId::new("Manifold.vents[\"intake\"]", "area"));
    assert_eq!(
        intake_area,
        Some(&Value::length(0.005)),
        "intake area must resolve to 5mm (0.005 m), got {intake_area:?}"
    );

    let exhaust_area = result
        .values
        .get(&ValueCellId::new("Manifold.vents[\"exhaust\"]", "area"));
    assert_eq!(
        exhaust_area,
        Some(&Value::length(0.008)),
        "exhaust area must resolve to 8mm (0.008 m), got {exhaust_area:?}"
    );

    // Lets a/b must also resolve correctly.
    let a = result.values.get(&ValueCellId::new("Manifold", "a"));
    assert_eq!(
        a,
        Some(&Value::length(0.005)),
        "Manifold.a (= vents[\"intake\"].area) must be 5mm, got {a:?}"
    );
    let b = result.values.get(&ValueCellId::new("Manifold", "b"));
    assert_eq!(
        b,
        Some(&Value::length(0.008)),
        "Manifold.b (= vents[\"exhaust\"].area) must be 8mm, got {b:?}"
    );
}
