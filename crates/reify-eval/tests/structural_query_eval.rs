//! Eval tests for structural-query accessors: `self.children`, `self.members` (task 3985, β).
//!
//! These tests verify end-to-end evaluation of the `self.children` and
//! `self.members` structural-query accessors, which are expanded from
//! MethodCall placeholders into concrete list expressions by the
//! `structural_query` post-pass in `engine_eval.rs`.
//!
//! Fixture: 2 non-collection subs (one plain, one aux) + 1 collection sub (count=4).
//!   - children = 3 (one slot per sub_component entry, aux included, collection = 1 slot)
//!   - members  = 6 (1 plain + 1 aux + 4 flattened bolts)
//!
//! Step numbering mirrors plan.json step IDs.

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_eval::Engine;

// ─── step-1: children enumeration (RED) ───

/// Fixture with 1 plain sub + 1 aux sub + 1 collection sub (count=4).
///
/// `self.children` should yield a list with ONE slot per sub_components entry
/// (collection subs contribute ONE slot, not flattened). `self.children.count`
/// should yield Int(3).
///
/// RED today: the MethodCall placeholder for `self.children` is un-enumerated
/// and evaluates to Undef, so `cs` = Undef and `n` = Undef.
#[test]
fn children_enumeration_plain_aux_collection() {
    let source = r#"
        structure Leaf {}
        structure Jig {}
        structure Asm {
            sub a = Leaf()
            aux sub jig : Jig
            sub bolts : List<Leaf>
            constraint bolts.count == 4
            let cs = self.children
            let n = self.children.count
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

    // (a) n == Int(3): 2 non-collection subs (incl aux) + 1 collection slot
    let n_id = ValueCellId::new("Asm", "n");
    assert_eq!(
        result.values.get(&n_id),
        Some(&Value::Int(3)),
        "Asm.n (count(self.children)) should be Int(3); got: {:?}",
        result.values.get(&n_id)
    );

    // (b) cs is a list of 3 entity-path strings in declaration order
    let cs_id = ValueCellId::new("Asm", "cs");
    match result.values.get(&cs_id) {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                3,
                "Asm.cs should have 3 elements; got: {:?}",
                items
            );
            assert_eq!(
                items[0],
                Value::String("Asm.a".to_string()),
                "Asm.cs[0] should be Asm.a; got: {:?}",
                items[0]
            );
            assert_eq!(
                items[1],
                Value::String("Asm.jig".to_string()),
                "Asm.cs[1] should be Asm.jig (aux included); got: {:?}",
                items[1]
            );
            assert_eq!(
                items[2],
                Value::String("Asm.bolts".to_string()),
                "Asm.cs[2] should be Asm.bolts (collection = one slot); got: {:?}",
                items[2]
            );
        }
        other => panic!(
            "Asm.cs should be Value::List; got: {:?}",
            other
        ),
    }
}
