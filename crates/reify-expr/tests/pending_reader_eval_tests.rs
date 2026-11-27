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

use reify_core::{Type, ValueCellId};
use reify_expr::{CellRead, EvalContext, Evaluated, PendingReader, eval_expr, eval_expr_or_pending};
use reify_ir::{BinOp, CompiledExpr, CompiledMatchArm, CompiledPattern, Value, ValueMap};
use reify_test_support::builders::{list_expr, std_call};

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
