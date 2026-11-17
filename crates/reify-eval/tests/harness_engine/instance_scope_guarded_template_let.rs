//! A cold `Engine::eval` resolves a sub instance's read of a where-guarded
//! template let at the template's value, in every collection slot (task
//! #10015). No instance holds a cell of its own for a guarded-group member, so
//! a grown slot, an ungraphed slot and a dynamic index must all read the same
//! value as the template.

use reify_core::ValueCellId;
use reify_eval::EvalResult;
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

const GUARDED_LET: &str = r#"
    structure LInner {
        param len : Length = 10mm
        param on : Bool = true
        where on {
            let half = len / 2
        }
        let q = half * 3
        let r = len * 3
    }

    structure LRack {
        param n : Int = 2
        sub items : List<LInner>
        constraint items.count == n
    }
"#;

fn cell<'a>(result: &'a EvalResult, entity: &str, member: &str) -> Option<&'a Value> {
    result.values.get(&ValueCellId::new(entity, member))
}

/// Cold eval of `source` after each `set_param_and_invalidate` in `overrides`.
fn cold_eval(source: &str, overrides: &[(ValueCellId, Value)]) -> EvalResult {
    let mut engine = make_simple_engine();
    for (param, value) in overrides {
        engine.set_param_and_invalidate(param, value.clone());
    }
    engine.eval(&parse_and_compile_with_stdlib(source))
}

/// The template's own `LInner.q`: the same expression over the same
/// `LInner.half`. Asserted present and determined.
fn template_q(result: &EvalResult) -> &Value {
    let reference = cell(result, "LInner", "q").expect("template cell LInner.q not found");
    assert_ne!(
        reference,
        &Value::Undef,
        "premise: template LInner.q must be determined, or the slot comparison measures nothing"
    );
    reference
}

/// Every slot was elaborated (its `r` is determined) and its `q` equals the
/// template's `LInner.q`.
fn assert_slots_read_the_template_let(result: &EvalResult, slots: &[&str]) {
    let reference = template_q(result);
    for slot in slots {
        let r = cell(result, slot, "r");
        assert!(
            r.is_some_and(|v| *v != Value::Undef),
            "premise: {slot}.r must be determined, so the slot was elaborated; got {r:?}"
        );
        let q = cell(result, slot, "q");
        assert_eq!(
            q,
            Some(reference),
            "{slot}.q reads the guarded template let `half`: expected the template's \
             LInner.q = {reference:?}, got {q:?}"
        );
    }
}

/// A slot grown past the count's static default has no graph cell, so it
/// must get the template value at elaboration, like the original slots.
#[test]
fn a_slot_grown_by_a_count_override_reads_a_guarded_template_let_like_the_original_slots() {
    let result = cold_eval(
        GUARDED_LET,
        &[
            (ValueCellId::new("LInner", "len"), Value::length(0.03)),
            (ValueCellId::new("LRack", "n"), Value::Int(3)),
        ],
    );

    assert_slots_read_the_template_let(
        &result,
        &["LRack.items[0]", "LRack.items[1]", "LRack.items[2]"],
    );
}

/// A count default that is not a literal stamps no slot in the graph, so
/// every slot relies on elaboration alone.
#[test]
fn every_slot_of_a_collection_with_a_non_literal_count_reads_a_guarded_template_let() {
    let source = GUARDED_LET.replace(
        "param n : Int = 2",
        "param m : Int = 1\n        param n : Int = m + 1",
    );
    assert_ne!(
        source, GUARDED_LET,
        "premise: the count default must be replaced by a non-literal"
    );

    let result = cold_eval(&source, &[]);

    assert_slots_read_the_template_let(&result, &["LRack.items[0]", "LRack.items[1]"]);
}

/// A non-literal index reads the member list assembled at elaboration time,
/// so that list must carry the slot's value, not an elaboration-time Undef.
#[test]
fn a_dynamic_index_reads_a_slots_let_over_a_guarded_template_let_at_the_slots_value() {
    let source = GUARDED_LET.replace(
        "constraint items.count == n",
        "constraint items.count == n\n        \
         param k : Int = 1\n        \
         let picked = items[k].q\n        \
         let picked_r = items[k].r",
    );
    assert_ne!(
        source, GUARDED_LET,
        "premise: LRack must gain the dynamic-index lets"
    );

    let result = cold_eval(&source, &[]);

    let picked_r = cell(&result, "LRack", "picked_r");
    let slot_r = cell(&result, "LRack.items[1]", "r");
    assert!(
        slot_r.is_some_and(|v| *v != Value::Undef),
        "premise: LRack.items[1].r must be determined; got {slot_r:?}"
    );
    assert_eq!(
        picked_r, slot_r,
        "control: a dynamic index over a plain let must read the slot's value"
    );

    let reference = template_q(&result);
    let slot_q = cell(&result, "LRack.items[1]", "q");
    assert_eq!(
        slot_q,
        Some(reference),
        "premise: LRack.items[1].q must equal the template's LInner.q = {reference:?}; \
         got {slot_q:?}"
    );

    let picked = cell(&result, "LRack", "picked");
    assert_eq!(
        picked, slot_q,
        "LRack.picked = items[k].q must read the slot's value {slot_q:?}; got {picked:?}"
    );
}
