//! Eval tests for `self.descendants` structural-query accessor (task 3988, γ).
//!
//! These tests verify end-to-end evaluation of the `self.descendants` accessor,
//! which is expanded from MethodCall placeholders into concrete list expressions
//! by the `structural_query` post-pass in `engine_eval.rs`.
//!
//! Fixture (headline / step-1a): 3-level nesting Arm → Motor → (Shaft + aux
//! Bushing).  Expected descendants: pre-order DFS, aux included, declaration
//! order at each level.
//!
//! Fixture (step-1b): self-referential Node with depth param guard.  The
//! schema walk does NOT evaluate the `where` guard, so a depth cap is
//! mandatory; the guard ships with the first recursion impl.
//!
//! Step numbering mirrors plan.json step IDs.

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_eval::Engine;

// ─── step-1a: descendants pre-order nesting with aux (RED) ───

/// Fixture: Arm → Motor → {Shaft (plain sub), Bushing (aux sub)}.
///
/// `self.descendants` should yield pre-order DFS in declaration order,
/// aux subs included:
/// [Arm.motor, Arm.motor.shaft, Arm.motor.bushing]
///
/// `self.descendants.count` should yield Int(3).
///
/// RED today: the MethodCall placeholder for `self.descendants` is unexpanded
/// and evaluates to Undef, so both `d` and `n` are Undef.
#[test]
fn descendants_pre_order_nesting_with_aux() {
    let source = r#"
        structure Shaft {}
        structure Bushing {}
        structure Motor {
            sub shaft = Shaft()
            aux sub bushing : Bushing
        }
        structure Arm {
            sub motor = Motor()
            let d = self.descendants
            let n = self.descendants.count
        }
    "#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // (a) n == Int(3): Arm.motor + Arm.motor.shaft + Arm.motor.bushing
    let n_id = ValueCellId::new("Arm", "n");
    assert_eq!(
        result.values.get(&n_id),
        Some(&Value::Int(3)),
        "Arm.n (self.descendants.count) should be Int(3); got: {:?}",
        result.values.get(&n_id)
    );

    // (b) d is a 3-element list in pre-order, declaration order, aux included
    let d_id = ValueCellId::new("Arm", "d");
    match result.values.get(&d_id) {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                3,
                "Arm.d should have 3 elements; got: {:?}",
                items
            );
            assert_eq!(
                items[0],
                Value::String("Arm.motor".to_string()),
                "Arm.d[0] should be Arm.motor (pre-order: parent before children); got: {:?}",
                items[0]
            );
            assert_eq!(
                items[1],
                Value::String("Arm.motor.shaft".to_string()),
                "Arm.d[1] should be Arm.motor.shaft (declaration order); got: {:?}",
                items[1]
            );
            assert_eq!(
                items[2],
                Value::String("Arm.motor.bushing".to_string()),
                "Arm.d[2] should be Arm.motor.bushing (aux sub included); got: {:?}",
                items[2]
            );
        }
        other => panic!(
            "Arm.d should be Value::List; got: {:?}",
            other
        ),
    }
}

// ─── step-3: descendants flattens collection sub (RED) ───

/// Fixture: Arm with a non-collection sub (Motor → Shaft) AND a root-level
/// collection sub (Bolts, count=3).
///
/// Expected pre-order traversal (declaration order, motor subtree precedes bolts):
/// [Arm.motor, Arm.motor.shaft, Arm.bolts[0], Arm.bolts[1], Arm.bolts[2]]
/// count == 5.
///
/// RED today: step-2 skips collection subs (continue), so bolts[0..2] are
/// absent; d has 2 elements (motor + shaft) and n == Int(2), not Int(5).
#[test]
fn descendants_flattens_collection_sub() {
    let source = r#"
        structure Shaft {}
        structure Motor {
            sub shaft = Shaft()
        }
        structure Bolt {}
        structure Arm {
            sub motor = Motor()
            sub bolts : List<Bolt>
            constraint bolts.count == 3
            let d = self.descendants
            let n = self.descendants.count
        }
    "#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // n == Int(5): motor + motor.shaft + bolts[0] + bolts[1] + bolts[2]
    let n_id = ValueCellId::new("Arm", "n");
    assert_eq!(
        result.values.get(&n_id),
        Some(&Value::Int(5)),
        "Arm.n should be Int(5); got: {:?}",
        result.values.get(&n_id)
    );

    // d is exactly [Arm.motor, Arm.motor.shaft, Arm.bolts[0], Arm.bolts[1], Arm.bolts[2]]
    let d_id = ValueCellId::new("Arm", "d");
    match result.values.get(&d_id) {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                5,
                "Arm.d should have 5 elements; got: {:?}",
                items
            );
            let expected = [
                "Arm.motor",
                "Arm.motor.shaft",
                "Arm.bolts[0]",
                "Arm.bolts[1]",
                "Arm.bolts[2]",
            ];
            for (i, (got, exp)) in items.iter().zip(expected.iter()).enumerate() {
                assert_eq!(
                    *got,
                    Value::String(exp.to_string()),
                    "Arm.d[{}] should be {}; got: {:?}",
                    i,
                    exp,
                    got
                );
            }
        }
        other => panic!(
            "Arm.d should be Value::List; got: {:?}",
            other
        ),
    }
}

// ─── step-1b: self-referential structure terminates with bounded depth (RED) ───

/// Self-referential fixture: Node has `sub child = Node(depth: depth - 1)
/// where depth > 0`.  The schema walk ignores the `where` guard — without a
/// depth cap it would recurse forever.
///
/// With `engine.set_max_unfold_depth(5)`:
/// - eval must NOT panic/hang (depth guard terminates the schema walk).
/// - Node.d must be a bounded Value::List (len <= 6).
/// - result.diagnostics must contain a Severity::Error whose message contains
///   "depth" (truncation diagnostic from the depth guard).
///
/// RED today: `descendants` is unexpanded → Undef; no truncation diagnostic.
#[test]
fn descendants_self_reference_terminates_bounded() {
    let source = r#"
        structure Node {
            param depth : Int = 3
            sub child = Node(depth: depth - 1) where depth > 0
            let d = self.descendants
            let n = self.descendants.count
        }
    "#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.set_max_unfold_depth(5);
    // eval must return without panic; the depth guard terminates the walk.
    let result = engine.eval(&compiled);

    // Node.d should be a bounded list (len <= 6) — truncated by depth guard.
    let d_id = ValueCellId::new("Node", "d");
    match result.values.get(&d_id) {
        Some(Value::List(items)) => {
            assert!(
                items.len() <= 6,
                "Node.d should be bounded (len <= 6); got len={}; items: {:?}",
                items.len(),
                items
            );
        }
        other => panic!(
            "Node.d should be a bounded Value::List; got: {:?}",
            other
        ),
    }

    // A Severity::Error diagnostic mentioning "depth" must be emitted.
    let has_depth_error = result.diagnostics.iter().any(|d| {
        d.severity == Severity::Error && d.message.contains("depth")
    });
    assert!(
        has_depth_error,
        "expected a Severity::Error with 'depth' in message; diagnostics: {:?}",
        result.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── step-5: example file eval (RED until step-6 creates the file) ───

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/structural_query_descendants.ri"
);

/// Reads `examples/structural_query_descendants.ri`, parses, compiles, and
/// evaluates it.  Asserts zero Error diagnostics at both stages and that
/// `Arm.descendant_count == Int(3)`.
///
/// RED until step-6 creates the file (read_to_string fails / file missing).
#[test]
fn example_structural_query_descendants_ri_evals_clean() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/structural_query_descendants.ri should exist (created by step-6)");

    let parsed = reify_syntax::parse(&source, ModulePath::single("structural_query_descendants_example"));
    assert!(
        parsed.errors.is_empty(),
        "example parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "example compile errors: {:?}",
        compile_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "example eval errors: {:?}",
        eval_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let count_id = ValueCellId::new("Arm", "descendant_count");
    assert_eq!(
        result.values.get(&count_id),
        Some(&Value::Int(3)),
        "Arm.descendant_count should be Int(3); got: {:?}",
        result.values.get(&count_id)
    );
}
