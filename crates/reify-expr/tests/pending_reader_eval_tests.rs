//! Evaluation under a reader that may answer "not here yet".
//!
//! Every test drives the public seam the new engine calls:
//! [`eval_expr_or_pending`] with a [`PendingReader`]. The [`TestReader`]
//! double holds the cells that are here, the cells that are not here yet, and
//! a log of every cell it was asked for; a ref it was told nothing about reads
//! `Here(Undef)`, which is the trait's contract (the walk's composed-field
//! probe asks for `__field.<name>` at every stdlib call outside a sealed
//! frame).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

use reify_ast::QuantifierKind;
use reify_core::{Diagnostic, DimensionVector, Type, ValueCellId};
use reify_expr::{CellRead, EvalContext, Evaluated, PendingReader, eval_expr, eval_expr_or_pending};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFunction, CompiledMatchArm, CompiledPattern,
    ComputeDispatch, ComputeDispatchOutcome, Value, ValueMap,
};
use reify_test_support::builders::{compiled_function, conditional_expr, list_expr, std_call};

// ── The test reader ─────────────────────────────────────────────────────────

/// A store for one round: some cells here, some not here yet, every ask logged.
#[derive(Default)]
struct TestReader {
    here: HashMap<ValueCellId, Value>,
    not_here_yet: BTreeSet<ValueCellId>,
    asks: RefCell<Vec<ValueCellId>>,
}

impl TestReader {
    fn new() -> Self {
        Self::default()
    }

    fn with(mut self, id: &ValueCellId, value: Value) -> Self {
        self.here.insert(id.clone(), value);
        self
    }

    fn not_yet(mut self, id: &ValueCellId) -> Self {
        self.not_here_yet.insert(id.clone());
        self
    }

    /// The old engine's store holding the same cells that are here.
    fn as_value_map(&self) -> ValueMap {
        let mut store = ValueMap::new();
        for (id, value) in &self.here {
            store.insert(id.clone(), value.clone());
        }
        store
    }
}

impl PendingReader for TestReader {
    fn read(&self, id: &ValueCellId) -> CellRead {
        self.asks.borrow_mut().push(id.clone());
        if self.not_here_yet.contains(id) {
            return CellRead::NotHereYet;
        }
        CellRead::Here(self.here.get(id).cloned().unwrap_or(Value::Undef))
    }
}

/// Every cell `reader` was asked for, whatever it answered.
fn asked(reader: &TestReader) -> BTreeSet<ValueCellId> {
    reader.asks.borrow().iter().cloned().collect()
}

fn ids(cells: &[&ValueCellId]) -> BTreeSet<ValueCellId> {
    cells.iter().map(|id| (*id).clone()).collect()
}

// ── Builders ────────────────────────────────────────────────────────────────

fn cell(member: &str) -> ValueCellId {
    ValueCellId::new("Probe", member)
}

fn read(id: &ValueCellId, ty: Type) -> CompiledExpr {
    CompiledExpr::value_ref(id.clone(), ty)
}

fn read_int(id: &ValueCellId) -> CompiledExpr {
    read(id, Type::Int)
}

fn int(n: i64) -> CompiledExpr {
    CompiledExpr::literal(Value::Int(n), Type::Int)
}

fn binop(op: BinOp, left: CompiledExpr, right: CompiledExpr, ty: Type) -> CompiledExpr {
    CompiledExpr::binop(op, left, right, ty)
}

fn enum_value(type_name: &str, variant: &str) -> Value {
    Value::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        payload: vec![],
    }
}

fn arm(pattern: CompiledPattern, body: CompiledExpr) -> CompiledMatchArm {
    CompiledMatchArm {
        patterns: vec![pattern],
        body,
    }
}

// ── Evaluation ──────────────────────────────────────────────────────────────

/// `expr` under the new entry, over `reader`, with no function or attachment.
fn pending_eval(expr: &CompiledExpr, reader: &TestReader) -> Evaluated {
    let empty = ValueMap::new();
    eval_expr_or_pending(expr, &EvalContext::simple(&empty), reader)
}

/// `expr` under the old entry, over a store holding the cells `reader` has here.
fn old_eval(expr: &CompiledExpr, reader: &TestReader) -> Value {
    let store = reader.as_value_map();
    eval_expr(expr, &EvalContext::simple(&store))
}

// ── (a) a read that is not here yet makes the evaluation pending ────────────

#[test]
fn an_expression_whose_only_read_is_not_here_yet_is_pending() {
    let a = cell("a");
    let reader = TestReader::new().not_yet(&a);
    assert_eq!(pending_eval(&read_int(&a), &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&a]));
}

// ── (b) with every cell here, the value is the old entry's ─────────────────

#[test]
fn with_every_cell_here_the_value_is_the_old_entrys_value() {
    let a = cell("a");
    let b = cell("b");
    let d = cell("d");
    let reader = TestReader::new()
        .with(&a, Value::Int(-3))
        .with(&b, Value::Int(4))
        .with(&d, enum_value("Color", "Green"));

    let color = Type::Enum("Color".to_string());
    let table: Vec<(&str, CompiledExpr)> = vec![
        (
            "arithmetic over two refs",
            binop(BinOp::Add, read_int(&a), read_int(&b), Type::Int),
        ),
        (
            "a list literal",
            list_expr(vec![read_int(&a), read_int(&b), int(9)]),
        ),
        ("a stdlib call", std_call("abs", vec![read_int(&a)], Type::Int)),
        (
            "a match on a stored enum",
            CompiledExpr::match_expr(
                read(&d, color),
                vec![
                    arm(CompiledPattern::variant("Red"), int(1)),
                    arm(CompiledPattern::variant("Green"), read_int(&b)),
                    arm(CompiledPattern::wildcard(), int(3)),
                ],
                Type::Int,
            ),
        ),
    ];

    for (name, expr) in &table {
        let expected = old_eval(expr, &reader);
        assert!(!expected.is_undef(), "{name}: the old entry gives a value");
        assert_eq!(
            pending_eval(expr, &reader),
            Evaluated::Value(expected),
            "{name}"
        );
    }
}

// ── (c) a node whose operand is pending still evaluates its other operands ──

#[test]
fn a_sum_with_a_pending_operand_asks_for_both_operands_and_is_pending() {
    let a = cell("a");
    let b = cell("b");
    let reader = TestReader::new().not_yet(&a).with(&b, Value::Int(2));
    let expr = binop(BinOp::Add, read_int(&a), read_int(&b), Type::Int);
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&a, &b]));
}

#[test]
fn a_list_with_a_pending_element_asks_for_every_element_and_is_pending() {
    let a = cell("a");
    let b = cell("b");
    let reader = TestReader::new().not_yet(&a).with(&b, Value::Int(2));
    let expr = list_expr(vec![read_int(&a), read_int(&b)]);
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&a, &b]));
}

// ── The logical operators read both operands ───────────────────────────────

fn read_bool(id: &ValueCellId) -> CompiledExpr {
    read(id, Type::Bool)
}

/// Each logical operator with each left value that decides it today: its
/// absorbing element and a non-Bool.
fn deciding_left_values() -> Vec<(BinOp, Value)> {
    vec![
        (BinOp::And, Value::Bool(false)),
        (BinOp::And, Value::Int(3)),
        (BinOp::Or, Value::Bool(true)),
        (BinOp::Or, Value::Int(3)),
        (BinOp::Implies, Value::Bool(false)),
        (BinOp::Implies, Value::Int(3)),
    ]
}

/// `op(read(x), read(y))` and `op(read(y), read(x))`.
fn both_orders(op: BinOp, x: &ValueCellId, y: &ValueCellId) -> [(&'static str, CompiledExpr); 2] {
    [
        (
            "x first",
            binop(op, read_bool(x), read_bool(y), Type::Bool),
        ),
        (
            "y first",
            binop(op, read_bool(y), read_bool(x), Type::Bool),
        ),
    ]
}

#[test]
fn a_logical_operator_whose_left_operand_decides_still_reads_the_right_and_keeps_its_value() {
    let x = cell("x");
    let y = cell("y");
    for (op, left) in deciding_left_values() {
        for (order, expr) in both_orders(op, &x, &y) {
            let reader = TestReader::new()
                .with(&x, left.clone())
                .with(&y, Value::Bool(true));
            let expected = old_eval(&expr, &reader);
            assert_eq!(
                pending_eval(&expr, &reader),
                Evaluated::Value(expected),
                "{op:?} with x = {left:?}, {order}"
            );
            assert_eq!(asked(&reader), ids(&[&x, &y]), "{op:?} with x = {left:?}, {order}");
        }
    }
}

#[test]
fn a_logical_operator_whose_left_operand_decides_is_pending_when_the_other_is_not_here_yet() {
    let x = cell("x");
    let y = cell("y");
    for (op, left) in deciding_left_values() {
        for (order, expr) in both_orders(op, &x, &y) {
            let reader = TestReader::new().with(&x, left.clone()).not_yet(&y);
            assert_eq!(
                pending_eval(&expr, &reader),
                Evaluated::Pending,
                "{op:?} with x = {left:?}, {order}"
            );
            assert_eq!(asked(&reader), ids(&[&x, &y]), "{op:?} with x = {left:?}, {order}");
        }
    }
}

// ── An `if` or a `match` whose condition is pending evaluates no arm ───────

fn if_then_else(condition: CompiledExpr, a: &ValueCellId, b: &ValueCellId) -> CompiledExpr {
    conditional_expr(condition, read_int(a), read_int(b))
}

/// `match discriminant { Red => read(a), _ => read(b) }`.
fn match_red_or_other(discriminant: CompiledExpr, a: &ValueCellId, b: &ValueCellId) -> CompiledExpr {
    CompiledExpr::match_expr(
        discriminant,
        vec![
            arm(CompiledPattern::variant("Red"), read_int(a)),
            arm(CompiledPattern::wildcard(), read_int(b)),
        ],
        Type::Int,
    )
}

/// `Color.Red { shade: payload }`.
fn red_with_shade(payload: CompiledExpr) -> CompiledExpr {
    CompiledExpr::variant_ctor(
        "Color".to_string(),
        "Red".to_string(),
        vec![("shade".to_string(), payload)],
        Type::Enum("Color".to_string()),
    )
}

#[test]
fn an_if_whose_condition_is_not_here_yet_evaluates_no_arm() {
    let (a, b, c) = (cell("a"), cell("b"), cell("c"));
    let reader = TestReader::new()
        .not_yet(&c)
        .with(&a, Value::Int(1))
        .with(&b, Value::Int(2));
    let expr = if_then_else(read_bool(&c), &a, &b);
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&c]));
}

#[test]
fn an_if_whose_condition_is_defined_but_read_a_pending_cell_evaluates_no_arm() {
    let (a, b, c) = (cell("a"), cell("b"), cell("c"));
    let reader = TestReader::new()
        .not_yet(&c)
        .with(&a, Value::Int(1))
        .with(&b, Value::Int(2));
    let condition = binop(
        BinOp::And,
        CompiledExpr::literal(Value::Bool(false), Type::Bool),
        read_bool(&c),
        Type::Bool,
    );
    let expr = if_then_else(condition, &a, &b);
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&c]), "the else arm is not evaluated");
}

#[test]
fn a_match_whose_discriminant_is_not_here_yet_evaluates_no_arm() {
    let (a, b, d) = (cell("a"), cell("b"), cell("d"));
    let reader = TestReader::new()
        .not_yet(&d)
        .with(&a, Value::Int(1))
        .with(&b, Value::Int(2));
    let expr = match_red_or_other(read(&d, Type::Enum("Color".to_string())), &a, &b);
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&d]));
}

#[test]
fn a_match_on_a_variant_whose_payload_is_not_here_yet_evaluates_no_arm() {
    let (a, b, c) = (cell("a"), cell("b"), cell("c"));
    let reader = TestReader::new()
        .not_yet(&c)
        .with(&a, Value::Int(1))
        .with(&b, Value::Int(2));
    let expr = match_red_or_other(red_with_shade(read_int(&c)), &a, &b);
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&c]), "the tag is known, but no arm is evaluated");
}

#[test]
fn an_if_or_a_match_whose_condition_is_here_evaluates_only_the_taken_arm() {
    let (a, b, c, d) = (cell("a"), cell("b"), cell("c"), cell("d"));
    let cases: Vec<(&str, CompiledExpr, BTreeSet<ValueCellId>)> = vec![
        (
            "if, then arm",
            if_then_else(read_bool(&c), &a, &b),
            ids(&[&c, &a]),
        ),
        (
            "match, Red arm",
            match_red_or_other(red_with_shade(read_int(&d)), &a, &b),
            ids(&[&d, &a]),
        ),
        (
            "match, wildcard arm",
            match_red_or_other(read(&d, Type::Enum("Color".to_string())), &a, &b),
            ids(&[&d, &b]),
        ),
    ];
    for (name, expr, expected_asks) in cases {
        let reader = TestReader::new()
            .with(&c, Value::Bool(true))
            .with(&d, enum_value("Color", "Green"))
            .with(&a, Value::Int(1))
            .with(&b, Value::Int(2));
        let expected = old_eval(&expr, &reader);
        assert!(!expected.is_undef(), "{name}: the old entry gives a value");
        assert_eq!(pending_eval(&expr, &reader), Evaluated::Value(expected), "{name}");
        assert_eq!(asked(&reader), expected_asks, "{name}: only the taken arm is evaluated");
    }
}

// ── A quantifier evaluates its body for every element ──────────────────────

const MANY: usize = 10_000;

fn element(i: usize) -> ValueCellId {
    cell(&format!("e{i}"))
}

fn loop_var() -> ValueCellId {
    ValueCellId::new("$quant0.S", "p")
}

fn quantifier(kind: QuantifierKind, collection: CompiledExpr, predicate: CompiledExpr) -> CompiledExpr {
    CompiledExpr::quantifier(kind, "p".to_string(), loop_var(), collection, predicate)
}

/// `<kind> p in <the MANY element cells, reflectively>: p > 0`.
fn over_many_cells(kind: QuantifierKind) -> CompiledExpr {
    let cells: Vec<CompiledExpr> = (0..MANY).map(|i| read_int(&element(i))).collect();
    let collection = CompiledExpr::reflective_cell_list(cells, Type::List(Box::new(Type::Int)));
    quantifier(
        kind,
        collection,
        binop(BinOp::Gt, read_int(&loop_var()), int(0), Type::Bool),
    )
}

/// `reader` was asked for each of the MANY element cells and nothing else.
fn assert_asked_every_element(reader: &TestReader, kind: QuantifierKind) {
    let every_element: BTreeSet<ValueCellId> = (0..MANY).map(element).collect();
    let asked = asked(reader);
    assert!(
        asked == every_element,
        "{kind:?}: asked for {} cells, {} of them among the {MANY} elements",
        asked.len(),
        asked.intersection(&every_element).count(),
    );
}

/// For each quantifier, the first element's value that decides it (`p > 0`
/// false for `forall`, true for `exists`) and a value that does not.
fn deciding_first_elements() -> [(QuantifierKind, i64, i64, bool); 2] {
    [
        (QuantifierKind::ForAll, -1, 1, false),
        (QuantifierKind::Exists, 1, -1, true),
    ]
}

#[test]
fn a_quantifier_over_ten_thousand_cells_asks_for_every_one_in_one_round() {
    for (kind, deciding, _, _) in deciding_first_elements() {
        let mut reader = TestReader::new().with(&element(0), Value::Int(deciding));
        for i in 1..MANY {
            reader = reader.not_yet(&element(i));
        }
        let expr = over_many_cells(kind);
        assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending, "{kind:?}");
        assert_asked_every_element(&reader, kind);
    }
}

#[test]
fn a_quantifier_over_ten_thousand_cells_that_are_here_asks_for_every_one_and_keeps_its_value() {
    for (kind, deciding, other, decided) in deciding_first_elements() {
        let mut reader = TestReader::new().with(&element(0), Value::Int(deciding));
        for i in 1..MANY {
            reader = reader.with(&element(i), Value::Int(other));
        }
        let expr = over_many_cells(kind);
        assert_eq!(old_eval(&expr, &reader), Value::Bool(decided), "{kind:?}");
        assert_eq!(
            pending_eval(&expr, &reader),
            Evaluated::Value(Value::Bool(decided)),
            "{kind:?}"
        );
        assert_asked_every_element(&reader, kind);
    }
}

/// `forall p in [0, 1]: if p == 0 then false else read(other)`, after corpus
/// entry `quantifier-later-element-read-is-cycle`.
fn later_element_reads_other(other: &ValueCellId) -> CompiledExpr {
    let body = conditional_expr(
        binop(BinOp::Eq, read_int(&loop_var()), int(0), Type::Bool),
        CompiledExpr::literal(Value::Bool(false), Type::Bool),
        read_bool(other),
    );
    quantifier(QuantifierKind::ForAll, list_expr(vec![int(0), int(1)]), body)
}

#[test]
fn a_value_quantifier_decided_by_its_first_element_still_evaluates_the_later_ones() {
    let other = cell("other");

    let reader = TestReader::new().not_yet(&other);
    assert_eq!(pending_eval(&later_element_reads_other(&other), &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&other]));

    let reader = TestReader::new().with(&other, Value::Bool(true));
    let expr = later_element_reads_other(&other);
    assert_eq!(old_eval(&expr, &reader), Value::Bool(false));
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Value(Value::Bool(false)));
    assert_eq!(asked(&reader), ids(&[&other]));
}

#[test]
fn a_quantifier_whose_collection_holds_a_pending_read_evaluates_no_body() {
    let (a, b, c) = (cell("a"), cell("b"), cell("c"));
    let reader = TestReader::new()
        .not_yet(&c)
        .with(&a, Value::Bool(true))
        .with(&b, Value::Bool(true));
    let body = conditional_expr(
        binop(BinOp::Eq, read_int(&loop_var()), int(1), Type::Bool),
        read_bool(&a),
        read_bool(&b),
    );
    let expr = quantifier(
        QuantifierKind::ForAll,
        list_expr(vec![read_int(&c), int(1)]),
        body,
    );
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&c]));
}

#[test]
fn a_quantifier_over_an_empty_list_asks_nothing_and_keeps_its_value() {
    let a = cell("a");
    for kind in [QuantifierKind::ForAll, QuantifierKind::Exists] {
        let reader = TestReader::new().with(&a, Value::Bool(true));
        let empty = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
        let expr = quantifier(kind, empty, read_bool(&a));
        let expected = old_eval(&expr, &reader);
        assert!(!expected.is_undef(), "{kind:?}: the old entry gives a value");
        assert_eq!(pending_eval(&expr, &reader), Evaluated::Value(expected), "{kind:?}");
        assert_eq!(asked(&reader), BTreeSet::new(), "{kind:?}");
    }
}

// ── Calls ───────────────────────────────────────────────────────────────────

/// An expression that panics when evaluated: `MetaAccess` with no meta context.
fn panic_on_eval_sentinel() -> CompiledExpr {
    CompiledExpr::meta_access("__sentinel".into(), "should_not_evaluate".into())
}

fn millimetres(n: f64) -> Value {
    Value::Scalar {
        si_value: n / 1000.0,
        dimension: DimensionVector::LENGTH,
    }
}

/// `fn <name>(x: Length) -> <body's type> { body }`.
fn unary_fn(name: &str, body: CompiledExpr) -> CompiledFunction {
    compiled_function(name, &[("x", Type::length())], vec![], body)
}

fn call(function: &CompiledFunction, args: Vec<CompiledExpr>) -> CompiledExpr {
    CompiledExpr::user_function_call(function.name.clone(), args, function.return_type.clone())
}

#[test]
fn a_user_function_called_with_an_undefined_argument_is_entered() {
    let u = cell("u");
    let five = unary_fn(
        "five",
        CompiledExpr::literal(millimetres(5.0), Type::length()),
    );
    let expr = call(&five, vec![read(&u, Type::length())]);
    let functions = [five];
    let empty = ValueMap::new();
    let ctx = EvalContext::new(&empty, &functions);

    let reader = TestReader::new().with(&u, Value::Undef);
    assert_eq!(
        eval_expr_or_pending(&expr, &ctx, &reader),
        Evaluated::Value(millimetres(5.0))
    );
    assert_eq!(eval_expr(&expr, &ctx), Value::Undef, "the old entry does not enter it");
}

#[test]
fn a_user_function_called_with_a_pending_argument_is_not_entered() {
    let c = cell("c");
    let boom = unary_fn("boom", panic_on_eval_sentinel());
    let expr = call(&boom, vec![read(&c, Type::length())]);
    let functions = [boom];
    let empty = ValueMap::new();
    let ctx = EvalContext::new(&empty, &functions);

    let reader = TestReader::new().not_yet(&c);
    assert_eq!(eval_expr_or_pending(&expr, &ctx, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&c]));
}

/// A selector hook that builds `Int 7` for a call named `probe`.
fn probe_hook(expr: &CompiledExpr, _: &ValueMap, _: &mut Vec<Diagnostic>) -> Option<Value> {
    match &expr.kind {
        CompiledExprKind::FunctionCall { function, .. } if function.name == "probe" => {
            Some(Value::Int(7))
        }
        _ => None,
    }
}

fn panicking_hook(expr: &CompiledExpr, _: &ValueMap, _: &mut Vec<Diagnostic>) -> Option<Value> {
    panic!("the built-in was run: {:?}", expr.kind)
}

#[test]
fn a_built_in_called_with_an_undefined_argument_is_not_run() {
    let u = cell("u");
    let expr = std_call("probe", vec![read_int(&u)], Type::Int);
    let empty = ValueMap::new();
    let ctx = EvalContext::simple(&empty).with_symbolic_selector_ctor(probe_hook);

    let reader = TestReader::new().with(&u, Value::Undef);
    assert_eq!(
        eval_expr_or_pending(&expr, &ctx, &reader),
        Evaluated::Value(Value::Undef)
    );

    let reader = TestReader::new().with(&u, Value::Int(1));
    assert_eq!(
        eval_expr_or_pending(&expr, &ctx, &reader),
        Evaluated::Value(Value::Int(7)),
        "with a defined argument the built-in is run"
    );
}

#[test]
fn a_built_in_whose_argument_holds_a_pending_read_in_a_container_is_not_run() {
    let c = cell("c");
    let expr = std_call(
        "probe",
        vec![list_expr(vec![read_int(&c)])],
        Type::Int,
    );
    let empty = ValueMap::new();
    let ctx = EvalContext::simple(&empty).with_symbolic_selector_ctor(panicking_hook);

    let reader = TestReader::new().not_yet(&c);
    assert_eq!(eval_expr_or_pending(&expr, &ctx, &reader), Evaluated::Pending);
}

/// A compute dispatcher that panics if anything is dispatched to it.
struct PanickingDispatch;

impl ComputeDispatch for PanickingDispatch {
    fn dispatch(&self, target: &str, args: &[Value]) -> ComputeDispatchOutcome {
        panic!("dispatched {target} with {args:?}")
    }
}

#[test]
fn an_optimized_function_called_with_an_undefined_argument_evaluates_its_body_without_dispatch() {
    let u = cell("u");
    let five = CompiledFunction {
        optimized_target: Some("solver::five".to_string()),
        ..unary_fn("five", CompiledExpr::literal(millimetres(5.0), Type::length()))
    };
    let expr = call(&five, vec![read(&u, Type::length())]);
    let functions = [five];
    let empty = ValueMap::new();
    let ctx = EvalContext::new(&empty, &functions).with_compute_dispatch(&PanickingDispatch);

    let reader = TestReader::new().with(&u, Value::Undef);
    assert_eq!(
        eval_expr_or_pending(&expr, &ctx, &reader),
        Evaluated::Value(millimetres(5.0))
    );
}
