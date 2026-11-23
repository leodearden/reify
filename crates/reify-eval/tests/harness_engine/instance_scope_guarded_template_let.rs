//! A cold `Engine::eval` resolves a sub instance's read of a where-guarded
//! template let at the template's value, in every collection slot (task
//! #10015). No instance holds a cell of its own for a guarded-group member, so
//! a grown slot, an ungraphed slot and a dynamic index must all read the same
//! value as the template.
//!
//! That is the template-value rule. The per-instance alternative, which
//! evaluates each instance's guarded groups against its own guard and inputs,
//! is owned by #7992. Only
//! `a_slot_grown_by_a_count_override_reads_a_guarded_template_let_like_the_original_slots`
//! separates the two rules: it overrides the template's `len` alone, so its
//! slots read the template's `q` rather than one computed from their own
//! `len`. The other tests keep template and slot inputs equal, so their
//! literals hold under either rule.

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

/// The two slots of the count's static default, plus one grown by `n = 3`.
const GROWN_SLOTS: &[&str] = &["LRack.items[0]", "LRack.items[1]", "LRack.items[2]"];

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

/// The template's own `LInner.<member>`, asserted present and determined.
fn template_value<'a>(result: &'a EvalResult, member: &str) -> &'a Value {
    let reference = cell(result, "LInner", member)
        .unwrap_or_else(|| panic!("template cell LInner.{member} not found"));
    assert_ne!(
        reference,
        &Value::Undef,
        "premise: template LInner.{member} must be determined, or the slot comparison \
         measures nothing"
    );
    reference
}

/// Every slot was elaborated (its `r` is determined) and its `member` equals
/// `expected`.
fn assert_slots_hold(result: &EvalResult, slots: &[&str], member: &str, expected: &Value) {
    for slot in slots {
        let r = cell(result, slot, "r");
        assert!(
            r.is_some_and(|v| *v != Value::Undef),
            "premise: {slot}.r must be determined, so the slot was elaborated; got {r:?}"
        );
        let actual = cell(result, slot, member);
        assert_eq!(
            actual,
            Some(expected),
            "{slot}.{member} reads a guarded template let: expected {expected:?}, got {actual:?}"
        );
    }
}

/// Every slot's `q` equals the template's `LInner.q`: the same expression over
/// the same `LInner.half`.
fn assert_slots_read_the_template_let(result: &EvalResult, slots: &[&str]) {
    assert_slots_hold(result, slots, "q", template_value(result, "q"));
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

    assert_eq!(
        template_value(&result, "q"),
        &Value::length(0.045),
        "premise: LInner.q = 30mm / 2 * 3; slot equality alone also holds when both \
         scopes degrade together"
    );
    for slot in GROWN_SLOTS {
        let len = cell(&result, slot, "len");
        assert_eq!(
            len,
            Some(&Value::length(0.01)),
            "premise: {slot} keeps its own len = 10mm, not the template's override; equal \
             inputs would stop this test separating the template-value rule from the \
             per-instance rule; got {len:?}"
        );
    }
    assert_slots_read_the_template_let(&result, GROWN_SLOTS);
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

    let reference = template_value(&result, "q");
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

fn grow_to_three_slots() -> (ValueCellId, Value) {
    (ValueCellId::new("LRack", "n"), Value::Int(3))
}

/// Phase 1 evaluates a param default before phase 2 commits any let, reading a
/// let it depends on as a scratch value. The guarded-group seed must already be
/// in phase 1's scope, or the grown slot's scratch `q`, and so its `p`, is Undef.
#[test]
fn a_param_default_reading_a_let_over_a_guarded_template_let_resolves_in_a_grown_slot() {
    let source = GUARDED_LET.replace(
        "let r = len * 3",
        "let r = len * 3\n        param p : Length = q",
    );
    assert_ne!(
        source, GUARDED_LET,
        "premise: LInner must gain the param `p`"
    );

    let result = cold_eval(&source, &[grow_to_three_slots()]);

    let reference = template_value(&result, "q");
    assert_eq!(
        reference,
        &Value::length(0.015),
        "premise: LInner.q = 10mm / 2 * 3"
    );
    assert_slots_hold(&result, GROWN_SLOTS, "p", reference);
}

/// Phase 1.5 evaluates a nested sub's ctor arg against phase 1's scope plus
/// scratch lets, so the guarded-group seed must reach it too.
#[test]
fn a_nested_ctor_arg_reading_a_let_over_a_guarded_template_let_resolves_in_a_grown_slot() {
    let source = format!(
        "structure LProbe {{ param v : Length = 0mm }}\n{}",
        GUARDED_LET.replace(
            "let r = len * 3",
            "let r = len * 3\n        sub probe = LProbe(v: q)",
        )
    );
    assert!(
        source.contains("sub probe = LProbe(v: q)"),
        "premise: LInner must gain the nested sub `probe`"
    );

    let result = cold_eval(&source, &[grow_to_three_slots()]);

    assert_slots_read_the_template_let(&result, GROWN_SLOTS);
    let reference = template_value(&result, "q");
    for slot in GROWN_SLOTS {
        let v = cell(&result, &format!("{slot}.probe"), "v");
        assert_eq!(
            v,
            Some(reference),
            "{slot}.probe.v is the ctor arg `q`: expected the template's LInner.q = \
             {reference:?}, got {v:?}"
        );
    }
}

/// A false guard selects the else branch: a same-name member (`half`) and an
/// else-only member (`full`) both resolve at the template's else value.
#[test]
fn a_grown_slot_reads_a_guarded_template_lets_else_branch() {
    let source = GUARDED_LET
        .replace("param on : Bool = true", "param on : Bool = false")
        .replace(
            "let half = len / 2\n        }",
            "let half = len / 2\n        } else {\n            \
             let half = len\n            \
             let full = len * 2\n        }\n        \
             let s = full * 3",
        );
    assert!(
        source.contains("param on : Bool = false") && source.contains("let s = full * 3"),
        "premise: LInner must gain a false guard and an else branch"
    );

    let result = cold_eval(&source, &[grow_to_three_slots()]);

    for (member, expected) in [("q", 0.03), ("s", 0.06)] {
        let reference = template_value(&result, member);
        assert_eq!(
            reference,
            &Value::length(expected),
            "premise: template LInner.{member} must take the else branch"
        );
        assert_slots_hold(&result, GROWN_SLOTS, member, reference);
    }
}
