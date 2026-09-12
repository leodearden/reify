//! Compiler lowering tests for `forall v in <keyed_coll>: constraint ...`
//! (task 3933 ε).
//!
//! Pins: per-member emission, BinOp::Gt, key-addressed scoped ValueRef, and
//! insertion-order determinism for a `Keyed<T>` sub.
//!
//! RED until step-2 adds the keyed branch in `resolve_forall_elements`.
//!
//! User-observable signal:
//!   cargo test -p reify-compiler --test keyed_forall_lower_tests

use reify_ir::{BinOp, CompiledExprKind};
use reify_test_support::{compile_source, errors_only};

/// Source fixture: a `Keyed<Vent>` sub with two author-assigned keys and a
/// value-binder forall constraint over them.
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
}
"#;

/// `forall v in vents: constraint v.area > 1mm` over a 2-key `Keyed<Vent>` sub
/// must emit EXACTLY 2 `forall@v[*]` constraints in declaration order:
///   - forall@v[0]: `vents["intake"].area > 1mm`
///     (BinOp::Gt, ValueRef entity `Manifold.vents["intake"]`, member "area")
///   - forall@v[1]: `vents["exhaust"].area > 1mm`
///     (BinOp::Gt, ValueRef entity `Manifold.vents["exhaust"]`, member "area")
///
/// Also asserts a clean compile (errors_only is empty).
///
/// RED today: `resolve_forall_elements` has no keyed branch (keyed subs carry
/// `is_collection==false` so the positional `.find(|s| s.is_collection)` lookup
/// never matches them); zero `forall@` constraints are emitted, making the
/// `assert_eq!(forall_constraints.len(), 2, ...)` fail.
///
/// Mirrors `forall_constraint_over_collection_sub_with_known_count_emits_per_element_constraints`
/// in `forall_statement_lower_tests.rs`, swapping the positional `S.vents[i]`
/// entity for the keyed `Manifold.vents["intake"]`/`["exhaust"]` entities and
/// `BinOp::Lt → BinOp::Gt`.
#[test]
fn forall_constraint_over_keyed_sub_emits_per_member_constraints() {
    let module = compile_source(KEYED_FORALL_SRC);

    // Clean compile — no errors.
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Manifold")
        .expect("Manifold template should compile");

    // Exactly 2 forall@v[*] constraints.
    let forall_constraints: Vec<_> = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@v["))
        })
        .collect();

    assert_eq!(
        forall_constraints.len(),
        2,
        "expected exactly 2 forall@v[*] constraints, got {}: labels = {:?}",
        forall_constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| c.label.as_deref())
            .collect::<Vec<_>>()
    );

    // Labels must be in insertion order (intake before exhaust).
    assert_eq!(
        forall_constraints[0].label.as_deref(),
        Some("forall@v[0]"),
        "first constraint must have label forall@v[0]"
    );
    assert_eq!(
        forall_constraints[1].label.as_deref(),
        Some("forall@v[1]"),
        "second constraint must have label forall@v[1]"
    );

    // Each constraint is `vents["key"].area > 1mm`:
    //   BinOp::Gt whose left is ValueRef { entity: "Manifold.vents[\"key\"]", member: "area" }.
    let expected_keys = ["intake", "exhaust"];
    for (i, key) in expected_keys.iter().enumerate() {
        let c = forall_constraints[i];
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, .. } => {
                assert_eq!(
                    *op,
                    BinOp::Gt,
                    "expected BinOp::Gt in element {} (key={}) body, got {:?}",
                    i,
                    key,
                    op
                );
                match &left.kind {
                    CompiledExprKind::ValueRef(id) => {
                        let expected_entity = format!("Manifold.vents[\"{key}\"]");
                        assert_eq!(
                            id.entity, expected_entity,
                            "element {} (key={}) must reference entity {}, got {}",
                            i, key, expected_entity, id.entity
                        );
                        assert_eq!(
                            id.member, "area",
                            "element {} (key={}) must reference member 'area', got {}",
                            i, key, id.member
                        );
                    }
                    other => panic!(
                        "expected ValueRef on LHS of element {} (key={}), got {:?}",
                        i, key, other
                    ),
                }
            }
            other => panic!(
                "expected BinOp(Gt) for element {} (key={}), got {:?}",
                i, key, other
            ),
        }
    }
}
