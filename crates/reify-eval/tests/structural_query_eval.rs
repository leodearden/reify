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

// ─── step-3: members enumeration (RED) ───

/// Same fixture as step-1, extended with `let ms = self.members; let m =
/// count(self.members)`.
///
/// `self.members` flattens collection subs: non-collection → 1 element,
/// collection → N elements (N = count).  Aux subs included.
///
/// Expected:
///   - m == Int(6): 1 (Asm.a) + 1 (Asm.jig, aux) + 4 (Asm.bolts[0..3])
///   - ms == ["Asm.a", "Asm.jig", "Asm.bolts[0]", "Asm.bolts[1]",
///            "Asm.bolts[2]", "Asm.bolts[3]"]  (declaration order, flat)
///
/// RED today: the members MethodCall placeholder is un-enumerated.
#[test]
fn members_enumeration_plain_aux_collection() {
    let source = r#"
        structure Leaf {}
        structure Jig {}
        structure Asm {
            sub a = Leaf()
            aux sub jig : Jig
            sub bolts : List<Leaf>
            constraint bolts.count == 4
            let ms = self.members
            let m = self.members.count
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

    // (a) m == Int(6): 1 plain + 1 aux + 4 flattened bolts
    let m_id = ValueCellId::new("Asm", "m");
    assert_eq!(
        result.values.get(&m_id),
        Some(&Value::Int(6)),
        "Asm.m (count(self.members)) should be Int(6); got: {:?}",
        result.values.get(&m_id)
    );

    // (b) ms is a flat list of 6 entity-path strings in declaration + flatten order
    let ms_id = ValueCellId::new("Asm", "ms");
    match result.values.get(&ms_id) {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                6,
                "Asm.ms should have 6 elements; got: {:?}",
                items
            );
            let expected = [
                "Asm.a",
                "Asm.jig",
                "Asm.bolts[0]",
                "Asm.bolts[1]",
                "Asm.bolts[2]",
                "Asm.bolts[3]",
            ];
            for (i, (got, exp)) in items.iter().zip(expected.iter()).enumerate() {
                assert_eq!(
                    *got,
                    Value::String(exp.to_string()),
                    "Asm.ms[{}] should be {}; got: {:?}",
                    i,
                    exp,
                    got
                );
            }
        }
        other => panic!(
            "Asm.ms should be Value::List; got: {:?}",
            other
        ),
    }
}

// ─── step-5: undef-count collection (RED) ───

/// Collection sub with NO count constraint → `__count_vents` cell is absent
/// (no synthetic Let cell is generated).
///
/// `children`: still 2 slots (a + vents), because children counts ONE slot per
/// sub regardless of the count.
/// `members`: 1 (a only) — the undef-count collection flattens to ZERO elements,
/// no panic.
///
/// RED today: `enumerate_members` might panic/unwrap if the count cell is absent.
#[test]
fn members_undef_count_collection_no_panic() {
    let source = r#"
        structure Leaf {}
        structure Asm {
            sub a = Leaf()
            sub vents : List<Leaf>
            let cn = self.children.count
            let mn = self.members.count
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
        "compile errors (undef-count fixture): {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // children: a + vents = 2 slots (undef count doesn't affect children)
    let cn_id = ValueCellId::new("Asm", "cn");
    assert_eq!(
        result.values.get(&cn_id),
        Some(&Value::Int(2)),
        "Asm.cn (count(self.children)) should be Int(2); got: {:?}",
        result.values.get(&cn_id)
    );

    // members: a only = 1 (vents has no count → 0 elements, no panic)
    let mn_id = ValueCellId::new("Asm", "mn");
    assert_eq!(
        result.values.get(&mn_id),
        Some(&Value::Int(1)),
        "Asm.mn (count(self.members)) should be Int(1); got: {:?}",
        result.values.get(&mn_id)
    );
}
