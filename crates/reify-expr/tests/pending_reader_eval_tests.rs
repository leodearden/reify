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
use reify_core::{Diagnostic, DiagnosticCode, DimensionVector, Type, ValueCellId};
use reify_expr::{
    CellRead, EvalContext, Evaluated, PendingReader, eval_expr, eval_expr_or_pending,
};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFunction, CompiledMatchArm, CompiledPattern,
    ComputeDispatch, ComputeDispatchOutcome, SelectorKind, Value, ValueMap,
};
use reify_test_support::builders::{
    compiled_function, conditional_expr, list_expr, map_expr, std_call,
};

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
        (
            "a stdlib call",
            std_call("abs", vec![read_int(&a)], Type::Int),
        ),
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
        ("x first", binop(op, read_bool(x), read_bool(y), Type::Bool)),
        ("y first", binop(op, read_bool(y), read_bool(x), Type::Bool)),
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
            assert_eq!(
                asked(&reader),
                ids(&[&x, &y]),
                "{op:?} with x = {left:?}, {order}"
            );
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
            assert_eq!(
                asked(&reader),
                ids(&[&x, &y]),
                "{op:?} with x = {left:?}, {order}"
            );
        }
    }
}

// ── An `if` or a `match` whose condition is pending evaluates no arm ───────

fn if_then_else(condition: CompiledExpr, a: &ValueCellId, b: &ValueCellId) -> CompiledExpr {
    conditional_expr(condition, read_int(a), read_int(b))
}

/// `match discriminant { Red => read(a), _ => read(b) }`.
fn match_red_or_other(
    discriminant: CompiledExpr,
    a: &ValueCellId,
    b: &ValueCellId,
) -> CompiledExpr {
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
    assert_eq!(
        asked(&reader),
        ids(&[&c]),
        "the tag is known, but no arm is evaluated"
    );
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
        assert_eq!(
            pending_eval(&expr, &reader),
            Evaluated::Value(expected),
            "{name}"
        );
        assert_eq!(
            asked(&reader),
            expected_asks,
            "{name}: only the taken arm is evaluated"
        );
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

fn quantifier(
    kind: QuantifierKind,
    collection: CompiledExpr,
    predicate: CompiledExpr,
) -> CompiledExpr {
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
    quantifier(
        QuantifierKind::ForAll,
        list_expr(vec![int(0), int(1)]),
        body,
    )
}

#[test]
fn a_value_quantifier_decided_by_its_first_element_still_evaluates_the_later_ones() {
    let other = cell("other");

    let reader = TestReader::new().not_yet(&other);
    assert_eq!(
        pending_eval(&later_element_reads_other(&other), &reader),
        Evaluated::Pending
    );
    assert_eq!(asked(&reader), ids(&[&other]));

    let reader = TestReader::new().with(&other, Value::Bool(true));
    let expr = later_element_reads_other(&other);
    assert_eq!(old_eval(&expr, &reader), Value::Bool(false));
    assert_eq!(
        pending_eval(&expr, &reader),
        Evaluated::Value(Value::Bool(false))
    );
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
        assert!(
            !expected.is_undef(),
            "{kind:?}: the old entry gives a value"
        );
        assert_eq!(
            pending_eval(&expr, &reader),
            Evaluated::Value(expected),
            "{kind:?}"
        );
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
    assert_eq!(
        eval_expr(&expr, &ctx),
        Value::Undef,
        "the old entry does not enter it"
    );
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
    assert_eq!(
        eval_expr_or_pending(&expr, &ctx, &reader),
        Evaluated::Pending
    );
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
    let expr = std_call("probe", vec![list_expr(vec![read_int(&c)])], Type::Int);
    let empty = ValueMap::new();
    let ctx = EvalContext::simple(&empty).with_symbolic_selector_ctor(panicking_hook);

    let reader = TestReader::new().not_yet(&c);
    assert_eq!(
        eval_expr_or_pending(&expr, &ctx, &reader),
        Evaluated::Pending
    );
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
        ..unary_fn(
            "five",
            CompiledExpr::literal(millimetres(5.0), Type::length()),
        )
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

// ── map_or and map_err evaluate every argument ─────────────────────────────

fn option_int() -> Type {
    Type::Option(Box::new(Type::Int))
}

fn result_type() -> Type {
    Type::Enum("Result".to_string())
}

/// `|x| body`, capturing `captures`.
fn lambda(body: CompiledExpr, captures: &[&ValueCellId]) -> CompiledExpr {
    let ty = Type::Function {
        params: vec![Type::Int],
        return_type: Box::new(body.result_type.clone()),
    };
    CompiledExpr::lambda(
        vec![("x".to_string(), Some(Type::Int))],
        vec![lambda_param()],
        body,
        captures.iter().map(|id| (*id).clone()).collect(),
        ty,
    )
}

fn lambda_param() -> ValueCellId {
    ValueCellId::new("__lambda", "x")
}

fn identity(captures: &[&ValueCellId]) -> CompiledExpr {
    lambda(read_int(&lambda_param()), captures)
}

fn map_or(subject: CompiledExpr, default: CompiledExpr, f: CompiledExpr) -> CompiledExpr {
    CompiledExpr::user_function_call("map_or".to_string(), vec![subject, default, f], Type::Int)
}

fn map_err(subject: CompiledExpr, f: CompiledExpr) -> CompiledExpr {
    CompiledExpr::user_function_call("map_err".to_string(), vec![subject, f], result_type())
}

fn result_value(variant: &str, field: &str, n: i64) -> Value {
    Value::Enum {
        type_name: "Result".to_string(),
        variant: variant.to_string(),
        payload: vec![(field.to_string(), Value::Int(n))],
    }
}

#[test]
fn map_or_on_some_evaluates_its_default_too_and_keeps_its_value() {
    let d = cell("d");
    let reader = TestReader::new().with(&d, Value::Int(9));
    let subject = CompiledExpr::option_some(int(1), option_int());
    let expr = map_or(subject, read_int(&d), identity(&[]));
    let expected = old_eval(&expr, &reader);
    assert_eq!(expected, Value::Int(1));
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Value(expected));
    assert_eq!(asked(&reader), ids(&[&d]));
}

#[test]
fn map_or_on_none_evaluates_its_function_expression_too_and_gives_the_default() {
    let (c, d) = (cell("c"), cell("d"));
    let reader = TestReader::new()
        .with(&c, Value::Int(5))
        .with(&d, Value::Int(9));
    let expr = map_or(
        CompiledExpr::option_none(option_int()),
        read_int(&d),
        identity(&[&c]),
    );
    assert_eq!(old_eval(&expr, &reader), Value::Int(9));
    assert_eq!(
        pending_eval(&expr, &reader),
        Evaluated::Value(Value::Int(9))
    );
    assert_eq!(asked(&reader), ids(&[&c, &d]));
}

#[test]
fn map_or_on_a_pending_subject_evaluates_every_argument_and_is_pending() {
    let (c, d, s) = (cell("c"), cell("d"), cell("s"));
    let reader = TestReader::new()
        .not_yet(&s)
        .with(&c, Value::Int(5))
        .with(&d, Value::Int(9));
    let expr = map_or(read(&s, option_int()), read_int(&d), identity(&[&c]));
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&s, &d, &c]));
}

#[test]
fn map_err_on_ok_evaluates_its_function_expression_but_does_not_apply_it() {
    let c = cell("c");
    let reader = TestReader::new().with(&c, Value::Int(5));
    let ok = result_value("Ok", "value", 4);
    let expr = map_err(
        CompiledExpr::literal(ok.clone(), result_type()),
        lambda(panic_on_eval_sentinel(), &[&c]),
    );
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Value(ok));
    assert_eq!(asked(&reader), ids(&[&c]));
}

#[test]
fn map_err_on_err_applies_its_function_as_the_old_entry_does() {
    let reader = TestReader::new();
    let err = result_value("Err", "error", 4);
    let expr = map_err(CompiledExpr::literal(err, result_type()), identity(&[]));
    let expected = old_eval(&expr, &reader);
    assert!(!expected.is_undef(), "the old entry gives a value");
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Value(expected));
}

#[test]
fn map_or_on_some_with_a_pending_default_does_not_apply_its_function() {
    let d = cell("d");
    let reader = TestReader::new().not_yet(&d);
    let subject = CompiledExpr::option_some(int(1), option_int());
    let expr = map_or(subject, read_int(&d), lambda(panic_on_eval_sentinel(), &[]));
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&d]));
}

#[test]
fn map_err_on_err_with_a_pending_capture_does_not_apply_its_function() {
    let c = cell("c");
    let reader = TestReader::new().not_yet(&c);
    let err = result_value("Err", "error", 4);
    let expr = map_err(
        CompiledExpr::literal(err, result_type()),
        lambda(panic_on_eval_sentinel(), &[&c]),
    );
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Pending);
    assert_eq!(asked(&reader), ids(&[&c]));
}

// ── solve_load_cases ────────────────────────────────────────────────────────

/// `solve_load_cases([read(m)], 1, 1, 1, [])`: every argument is defined
/// whatever `m` reads, and the solve, given no load case, reports
/// `MultiLoadEmptyCases` to the runtime sink.
fn solve_with_no_cases(m: &ValueCellId) -> CompiledExpr {
    let no_cases = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    CompiledExpr::user_function_call(
        "solve_load_cases".to_string(),
        vec![
            list_expr(vec![read_int(m)]),
            int(1),
            int(1),
            int(1),
            no_cases,
        ],
        Type::Int,
    )
}

/// `expr` under the new entry over `reader`, and the codes of the runtime
/// diagnostics it pushed.
fn pending_eval_with_diagnostics(
    expr: &CompiledExpr,
    reader: &TestReader,
) -> (Evaluated, Vec<Option<DiagnosticCode>>) {
    let empty = ValueMap::new();
    let sink = RefCell::new(Vec::new());
    let ctx = EvalContext::simple(&empty).with_runtime_diagnostics(&sink);
    let evaluated = eval_expr_or_pending(expr, &ctx, reader);
    let codes = sink.into_inner().iter().map(|d| d.code).collect();
    (evaluated, codes)
}

#[test]
fn solve_load_cases_is_not_run_when_an_argument_holds_a_pending_read() {
    let m = cell("m");
    let expr = solve_with_no_cases(&m);

    let reader = TestReader::new().with(&m, Value::Int(1));
    let (evaluated, codes) = pending_eval_with_diagnostics(&expr, &reader);
    assert_eq!(evaluated, Evaluated::Value(Value::Undef));
    assert_eq!(
        codes,
        vec![Some(DiagnosticCode::MultiLoadEmptyCases)],
        "with every cell here the solve is run"
    );

    let reader = TestReader::new().not_yet(&m);
    let (evaluated, codes) = pending_eval_with_diagnostics(&expr, &reader);
    assert_eq!(evaluated, Evaluated::Pending);
    assert_eq!(codes, vec![], "the solve is not run");
}

// ── flat_map, worst_case, method calls and ad-hoc selectors ────────────────

/// `|x| if x == 1 then 0 else <panic sentinel>`: reaching any element but 1
/// panics.
fn panics_unless_one() -> CompiledExpr {
    lambda(
        conditional_expr(
            binop(BinOp::Eq, read_int(&lambda_param()), int(1), Type::Bool),
            int(0),
            panic_on_eval_sentinel(),
        ),
        &[],
    )
}

/// `flat_map([1, 2], panics_unless_one)`: the first element's result is not
/// a list, which decides the call.
fn flat_map_decided_by_its_first_element() -> CompiledExpr {
    std_call(
        "flat_map",
        vec![list_expr(vec![int(1), int(2)]), panics_unless_one()],
        Type::List(Box::new(Type::Int)),
    )
}

#[test]
#[should_panic(expected = "MetaAccess evaluation requires meta context")]
fn flat_map_applies_its_function_to_every_element_under_the_new_entry() {
    pending_eval(&flat_map_decided_by_its_first_element(), &TestReader::new());
}

#[test]
fn flat_map_stops_at_the_first_non_list_result_under_the_old_entry() {
    let expr = flat_map_decided_by_its_first_element();
    assert_eq!(old_eval(&expr, &TestReader::new()), Value::Undef);
}

/// `worst_case({ "cases": { 0: 1, "a": 2 } }, panics_unless_one)`: `Int`
/// sorts before `String` in `Value`'s order, so the first case's key is not
/// a String, which decides the call; reaching case "a" panics.
fn worst_case_decided_by_its_first_key() -> CompiledExpr {
    let cases = map_expr(vec![(int(0), int(1)), (string("a"), int(2))]);
    let outer = map_expr(vec![(string("cases"), cases)]);
    std_call("worst_case", vec![outer, panics_unless_one()], Type::String)
}

fn string(s: &str) -> CompiledExpr {
    CompiledExpr::literal(Value::String(s.to_string()), Type::String)
}

#[test]
#[should_panic(expected = "MetaAccess evaluation requires meta context")]
fn worst_case_applies_its_function_to_every_case_under_the_new_entry() {
    pending_eval(&worst_case_decided_by_its_first_key(), &TestReader::new());
}

#[test]
fn worst_case_stops_at_the_first_non_string_key_under_the_old_entry() {
    let expr = worst_case_decided_by_its_first_key();
    assert_eq!(old_eval(&expr, &TestReader::new()), Value::Undef);
}

#[test]
fn a_method_call_with_an_argument_evaluates_its_object_and_argument() {
    let (a, o) = (cell("a"), cell("o"));
    let reader = TestReader::new()
        .with(&o, Value::List(vec![Value::Int(1)]))
        .with(&a, Value::Int(0));
    let expr = CompiledExpr::method_call(
        read(&o, Type::List(Box::new(Type::Int))),
        "len".to_string(),
        vec![read_int(&a)],
        Type::Int,
    );
    assert_eq!(old_eval(&expr, &reader), Value::Undef);
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Value(Value::Undef));
    assert_eq!(asked(&reader), ids(&[&o, &a]));
}

#[test]
fn a_point_selector_evaluates_every_coordinate() {
    let (y, z) = (cell("y"), cell("z"));
    let reader = TestReader::new()
        .with(&y, millimetres(1.0))
        .with(&z, millimetres(2.0));
    let expr = CompiledExpr::ad_hoc_selector(
        int(0),
        SelectorKind::Point,
        vec![int(7), read(&y, Type::length()), read(&z, Type::length())],
    );
    assert_eq!(old_eval(&expr, &reader), Value::Undef);
    assert_eq!(pending_eval(&expr, &reader), Evaluated::Value(Value::Undef));
    assert_eq!(asked(&reader), ids(&[&y, &z]));
}

#[test]
fn a_face_or_edge_selector_evaluates_its_arguments() {
    let n = cell("n");
    for kind in [SelectorKind::Face, SelectorKind::Edge] {
        let reader = TestReader::new().with(&n, Value::String("top".to_string()));
        let expr = CompiledExpr::ad_hoc_selector(int(0), kind, vec![read(&n, Type::String)]);
        assert_eq!(old_eval(&expr, &reader), Value::Undef);
        assert_eq!(pending_eval(&expr, &reader), Evaluated::Value(Value::Undef));
        assert_eq!(asked(&reader), ids(&[&n]));
    }
}
