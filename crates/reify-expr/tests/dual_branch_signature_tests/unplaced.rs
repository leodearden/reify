//! Kinks ε cannot place: the `Unplaced` entry (task #7493).
//!
//! A node the dual traversal hands WHOLE to `eval_expr` never visits the kinks
//! beneath it, so it cannot name their sites.  It records one `Unplaced` entry
//! at its OWN site instead, which keeps "an empty record means no kink" true
//! through such nodes.

use reify_core::Type;
use reify_expr::{BranchChoice, BranchEntry, BranchRecord, KinkKind, KinkSite, Tangent};
use reify_ir::{BinOp, CompiledExpr, Value};
use reify_test_support::builders::expr::{
    binop, conditional_expr, gt, list_expr, literal, not, user_fn_call,
};

use crate::{assert_single_entry, call, dimensionless, record_of, row, run, vref};

fn unplaced_record(choice: BranchChoice) -> BranchRecord {
    let mut record = BranchRecord::new();
    record.push(BranchEntry { site: KinkSite::new(vec![1]), kind: KinkKind::Unplaced, choice });
    record
}

#[test]
fn an_unplaced_entry_s_outcome_hash_is_part_of_the_signature() {
    let ten = || BranchChoice::OutcomeHash(Value::Real(10.0).content_hash());
    let twenty = BranchChoice::OutcomeHash(Value::Real(20.0).content_hash());

    let a = unplaced_record(ten());
    let b = unplaced_record(ten());
    assert_eq!(a, b, "the same outcome is the same entry");
    assert_eq!(a.differs_from(&b), None, "the same outcome is no flip");
    assert_eq!(a.signature_key(), b.signature_key(), "the same outcome is the same signature");

    let flipped = unplaced_record(twenty);
    assert_eq!(
        a.differs_from(&flipped),
        Some(KinkSite::new(vec![1])),
        "a different outcome is a flip at the enclosing node's site"
    );
    assert_ne!(
        a.signature_key(),
        flipped.signature_key(),
        "the outcome hash must reach the key: λ counts alternations BY KEY, so a payload \
         hashed as its discriminant alone would make two signatures read as one"
    );
}

/// `[first, if p then 10.0 else 20.0]`: a list whose second element is a kink
/// the traversal never visits, because it delegates any node enclosing the list
/// to `eval_expr` whole.
fn list_hiding_a_conditional(first: CompiledExpr) -> CompiledExpr {
    list_expr(vec![
        first,
        conditional_expr(vref("p"), literal(Value::Real(10.0)), literal(Value::Real(20.0))),
    ])
}

/// `x + [1.0, if p then 10.0 else 20.0][1]`: with only `x` seeded, the
/// IndexAccess at [1] is seed-independent yet selects through a Conditional.
fn x_plus_a_hidden_selection() -> CompiledExpr {
    binop(
        BinOp::Add,
        vref("x"),
        CompiledExpr::index_access(
            list_hiding_a_conditional(literal(Value::Real(1.0))),
            literal(Value::Int(1)),
            dimensionless(),
        ),
    )
}

#[test]
fn a_kink_inside_a_seed_independent_index_access_is_recorded_as_one_unplaced_entry() {
    let cells = [("x", Value::Real(2.0)), ("p", Value::Bool(true))];
    let (v, t, rec) = run(&x_plus_a_hidden_selection(), &cells, &["x"]);
    assert_eq!(v, Value::Real(12.0));
    assert_eq!(
        row(&t, 1, "hidden selection"),
        vec![1.0],
        "the flat IndexAccess contributes nothing to the row and vetoes nothing"
    );
    assert_single_entry(
        &rec,
        &[1],
        KinkKind::Unplaced,
        BranchChoice::OutcomeHash(Value::Real(10.0).content_hash()),
        "IndexAccess hiding a Conditional",
    );
}

#[test]
fn every_delegating_call_site_records_an_unplaced_entry_for_a_kink_it_encloses() {
    // The catch-all arm is the IndexAccess test above; these are the other two.

    // The `UnOp::Not` arm.
    let negated = not(gt(vref("c"), literal(Value::Real(5.0))));
    let cells = [("c", Value::Real(9.0)), ("x", Value::Real(2.0))];
    let (v, t, rec) = run(&negated, &cells, &["x"]);
    assert_eq!(v, Value::Bool(false));
    assert_eq!(t, Tangent::Zero, "an unseeded `not` is flat, not a refusal");
    assert_single_entry(
        &rec,
        &[],
        KinkKind::Unplaced,
        BranchChoice::OutcomeHash(Value::Bool(false).content_hash()),
        "`not` over a comparison",
    );

    // The intercepted-family gate: `unwrap_or` never runs a `.ri` body, so the
    // whole call is delegated before its arguments are descended.
    let some_conditional = CompiledExpr::option_some(
        conditional_expr(vref("p"), literal(Value::Real(1.0)), literal(Value::Real(2.0))),
        Type::Option(Box::new(dimensionless())),
    );
    let recovered = binop(
        BinOp::Add,
        vref("x"),
        user_fn_call(
            "unwrap_or",
            vec![some_conditional, literal(Value::Real(0.0))],
            dimensionless(),
        ),
    );
    let cells = [("x", Value::Real(2.0)), ("p", Value::Bool(true))];
    let (v, t, rec) = run(&recovered, &cells, &["x"]);
    assert_eq!(v, Value::Real(3.0));
    assert_eq!(row(&t, 1, "unwrap_or"), vec![1.0]);
    assert_single_entry(
        &rec,
        &[1],
        KinkKind::Unplaced,
        BranchChoice::OutcomeHash(Value::Real(1.0).content_hash()),
        "`unwrap_or` over a Conditional",
    );
}

#[test]
fn an_unplaced_entry_reports_a_hidden_flip_and_only_a_hidden_flip() {
    let expr = x_plus_a_hidden_selection();
    let at =
        |x: f64, p: bool| record_of(&expr, &[("x", Value::Real(x)), ("p", Value::Bool(p))], &["x"]);

    let then_side = at(2.0, true);
    let else_side = at(2.0, false);
    assert_eq!(
        then_side.differs_from(&else_side),
        Some(KinkSite::new(vec![1])),
        "the hidden Conditional flipped, so λ must see a flip at the enclosing node"
    );
    assert_ne!(then_side.signature_key(), else_side.signature_key());

    let moved = at(7.0, true);
    assert_eq!(
        then_side.differs_from(&moved),
        None,
        "only the seed moved, so a flip here would be phantom chatter"
    );
    assert_eq!(then_side.signature_key(), moved.signature_key());
}

/// `max(list[0], 100.0)` for a `list` whose element 0 is `x`: the IndexAccess
/// moves with the seed and refuses a tangent, and `max` then discards that
/// refusal by selecting the constant operand.
fn max_discarding_a_refused_index_access(list: CompiledExpr) -> CompiledExpr {
    call("max", vec![
        CompiledExpr::index_access(list, literal(Value::Int(0)), dimensionless()),
        literal(Value::Real(100.0)),
    ])
}

#[test]
fn an_unplaced_entry_over_a_seed_dependent_node_is_stable_in_a_row_that_survives_the_refusal() {
    let expr = max_discarding_a_refused_index_access(list_hiding_a_conditional(vref("x")));
    let expected = [
        BranchEntry {
            site: KinkSite::new(vec![0]),
            kind: KinkKind::Unplaced,
            choice: BranchChoice::Unresolved,
        },
        BranchEntry {
            site: KinkSite::root(),
            kind: KinkKind::Max,
            choice: BranchChoice::Operand(1),
        },
    ];
    // Every x stays below 100, so `max` never flips: nothing λ may count moved.
    let records: Vec<BranchRecord> = [2.0, 3.0, 50.0]
        .into_iter()
        .map(|x| {
            let cells = [("x", Value::Real(x)), ("p", Value::Bool(true))];
            let (v, t, rec) = run(&expr, &cells, &["x"]);
            assert_eq!(v, Value::Real(100.0), "x = {x}");
            assert_eq!(
                row(&t, 1, "max over a refused operand"),
                vec![0.0],
                "x = {x}: the row exists although the IndexAccess refused, so its record is read"
            );
            assert_eq!(rec.entries(), expected.as_slice(), "x = {x}");
            rec
        })
        .collect();
    for later in &records[1..] {
        assert_eq!(
            records[0].differs_from(later),
            None,
            "the Unplaced choice must not move with a value that moves continuously with the \
             seed, or every trial point would read as a flip"
        );
        assert_eq!(records[0].signature_key(), later.signature_key());
    }
}

#[test]
fn a_delegated_node_with_no_kink_beneath_it_records_no_unplaced_entry() {
    // The same delegated, seed-dependent IndexAccess as above, without the
    // Conditional: `max` records its own selection and nothing else.
    let expr = max_discarding_a_refused_index_access(list_expr(vec![
        vref("x"),
        literal(Value::Real(2.0)),
    ]));
    let (v, t, rec) = run(&expr, &[("x", Value::Real(2.0))], &["x"]);
    assert_eq!(v, Value::Real(100.0));
    assert_eq!(row(&t, 1, "max over a refused operand"), vec![0.0]);
    assert_single_entry(
        &rec,
        &[],
        KinkKind::Max,
        BranchChoice::Operand(1),
        "IndexAccess over a kink-free list",
    );
}

#[test]
fn an_unplaced_entry_is_conservative_a_delegated_call_records_one_over_a_smooth_subtree() {
    // A call is opaque to `subtree_has_kink`, so a delegated node that is or
    // encloses one records an `Unplaced` entry with no kink beneath it.  That is
    // the deliberate direction: the opposite error, a kink with no entry, is
    // the one λ cannot detect.  Both routes a call is opaque by are pinned.
    let cells = [("x", Value::Real(2.0)), ("c", Value::Real(0.0))];

    // A `UserFunctionCall`: the intercepted family is the delegated node.
    let recovered = binop(
        BinOp::Add,
        vref("x"),
        user_fn_call(
            "unwrap_or",
            vec![
                CompiledExpr::option_some(vref("c"), Type::Option(Box::new(dimensionless()))),
                literal(Value::Real(5.0)),
            ],
            dimensionless(),
        ),
    );
    let (v, t, rec) = run(&recovered, &cells, &["x"]);
    assert_eq!(v, Value::Real(2.0));
    assert_eq!(row(&t, 1, "unwrap_or"), vec![1.0]);
    assert_single_entry(
        &rec,
        &[1],
        KinkKind::Unplaced,
        BranchChoice::OutcomeHash(Value::Real(0.0).content_hash()),
        "`unwrap_or` over a smooth argument",
    );

    // An arity-1 builtin under a delegated IndexAccess.  Not delegated, the
    // same `sin(c)` records nothing.
    let sine = || call("sin", vec![vref("c")]);
    let indexed = binop(
        BinOp::Add,
        vref("x"),
        CompiledExpr::index_access(
            list_expr(vec![sine()]),
            literal(Value::Int(0)),
            dimensionless(),
        ),
    );
    let (v, _, rec) = run(&indexed, &cells, &["x"]);
    assert_eq!(v, Value::Real(2.0));
    assert_single_entry(
        &rec,
        &[1],
        KinkKind::Unplaced,
        BranchChoice::OutcomeHash(Value::Real(0.0).content_hash()),
        "IndexAccess over `[sin(c)]`",
    );
    let (_, _, rec) = run(&binop(BinOp::Add, vref("x"), sine()), &cells, &["x"]);
    assert!(rec.is_empty(), "`x + sin(c)` is smooth, got {:?}", rec.entries());
}
