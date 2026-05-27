//! Quantifier evaluation tests.

use reify_expr::{EvalContext, eval_expr};
use reify_ast::QuantifierKind;
use reify_core::{Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, DeterminacyPredicateKind, DeterminacyState, PersistentMap, Value, ValueMap};

/// Helper: create a quantifier CompiledExpr.
fn make_quantifier(
    kind: QuantifierKind,
    var_name: &str,
    var_id: ValueCellId,
    collection: CompiledExpr,
    predicate: CompiledExpr,
) -> CompiledExpr {
    CompiledExpr::quantifier(kind, var_name.to_string(), var_id, collection, predicate)
}

/// step-5: forall over [1,2,3] with x>0 -> true
#[test]
fn forall_all_true() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

/// step-5: forall over [1,-1,3] with x>0 -> false
#[test]
fn forall_has_false() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(-1), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

/// step-5: exists over [1,2,3] with x>2 -> true
#[test]
fn exists_has_true() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::Exists, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

/// step-5: exists over [1,2,3] with x>5 -> false
#[test]
fn exists_all_false() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(5), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::Exists, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

/// step-7: forall over empty list -> true (vacuous truth)
#[test]
fn forall_empty_list_vacuous_truth() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

/// step-9: forall over [1, Undef, 3] with x>0 -> Undef (no false, but undef present)
#[test]
fn forall_with_undef_no_false() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Undef, Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// step-9: exists over [1, Undef, 3] with x>2 -> true (short-circuit on 3>2=true)
#[test]
fn exists_with_undef_has_true() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Undef, Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::Exists, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

// ─── step-11: Integration test: parse + compile + eval quantifier in constraint context ───

/// step-11: End-to-end integration test — parse a structure with a list-typed
/// let binding and a `forall` constraint, compile it, then evaluate the compiled
/// constraint expression with concrete values. Verifies the full pipeline:
/// grammar -> parser -> compiler -> evaluator.
#[test]
fn integration_forall_constraint_parse_compile_eval() {
    // Parse source with a list and a forall constraint
    let source = r#"
structure S {
    let grades = [9.0, 8.8, 9.5]
    constraint forall g in grades: g >= 8.8
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("integ_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = &compiled.templates[0];
    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    // Find the grades value cell and the constraint
    let grades_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "grades")
        .expect("should have 'grades' value cell");
    let constraint_expr = &template.constraints[0].expr;

    // Verify the constraint compiled to a Quantifier
    assert!(
        matches!(&constraint_expr.kind, CompiledExprKind::Quantifier { .. }),
        "expected Quantifier, got {:?}",
        constraint_expr.kind,
    );

    // Evaluate the grades default expression to get the list value
    let empty_values = ValueMap::new();
    let grades_value = eval_expr(
        grades_cell.default_expr.as_ref().unwrap(),
        &EvalContext::simple(&empty_values),
    );
    assert!(
        matches!(&grades_value, Value::List(_)),
        "grades should eval to a list, got {:?}",
        grades_value,
    );

    // Now evaluate the constraint with grades in scope
    let mut values = ValueMap::new();
    values.insert(grades_cell.id.clone(), grades_value);
    let result = eval_expr(constraint_expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Bool(true),
        "all grades >= 8.8 should be true"
    );
}

/// §9.2.6 truth-table matrix: 4 spec-rows × {List, Set} = 8 assertions.
/// Pins Kleene semantics of `exists` for both container kinds.
///
/// Row 1 uses elements `[-1, undef, 2]` with predicate `x > 0`. The adversarial
/// ordering guarantees depend on the container kind:
/// - **List** preserves insertion order → predicate results `[false, undef, true]`.
///   Catches both premature-false (returning early on a false result before scanning
///   the rest) and premature-undef (returning early on undef before seeing a true).
/// - **Set** is backed by `BTreeSet<Value>`; `Value::Undef` carries the lowest
///   type-tag (0) and sorts before `Value::Int(_)` (tag 2), so the BTreeSet
///   iteration order is `[Undef, Int(-1), Int(2)]` → predicate results
///   `[undef, false, true]`. The adversarial property exercised for Set is:
///   "must scan past undef and then false before reaching the determining true";
///   the explicit false→undef→true ordering is only pinned for the List variant.
#[test]
fn exists_kleene_truth_table_over_list_and_set() {
    #[derive(Debug, Clone, Copy)]
    enum CollKind {
        List,
        Set,
    }

    // Rows: (name, elements as Option<i64>, expected result)
    // Some(i) => Value::Int(i); predicate x > 0 yields Bool(i > 0)
    // None    => Value::Undef; predicate x > 0 on Undef yields Undef
    let rows: Vec<(&str, Vec<Option<i64>>, Value)> = vec![
        // Row 1: adversarial ordering.
        // List (insertion order preserved): false → undef → true.
        //   Catches premature-false and premature-undef short-circuit bugs.
        // Set (BTreeSet, Undef tag=0 < Int tag=2): sorts to undef → false → true.
        //   Catches premature-undef bug; the false→undef ordering is List-only.
        (
            "any_true_after_false_and_undef",
            vec![Some(-1), None, Some(2)],
            Value::Bool(true),
        ),
        // Row 2: all predicate results false → Bool(false)
        ("all_false", vec![Some(-1), Some(-2)], Value::Bool(false)),
        // Row 3: no true result, undef present → Undef
        ("no_true_some_undef", vec![Some(-1), None], Value::Undef),
        // Row 4: empty collection → vacuous falsity → Bool(false)
        ("empty", vec![], Value::Bool(false)),
    ];

    // x_id and predicate are invariant across all (kind, row) iterations.
    let x_id = ValueCellId::new("$quant0.S", "x");
    // Predicate: x > 0
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );

    for kind in [CollKind::List, CollKind::Set] {
        for (name, elements, expected) in &rows {
            // Build element expressions: Some(i) → Int literal, None → Undef literal
            let elem_exprs: Vec<CompiledExpr> = elements
                .iter()
                .map(|opt| match opt {
                    Some(i) => CompiledExpr::literal(Value::Int(*i), Type::Int),
                    None => CompiledExpr::literal(Value::Undef, Type::Int),
                })
                .collect();

            // Build collection for the current container kind
            let collection = match kind {
                CollKind::List => {
                    CompiledExpr::list_literal(elem_exprs, Type::List(Box::new(Type::Int)))
                }
                CollKind::Set => {
                    CompiledExpr::set_literal(elem_exprs, Type::Set(Box::new(Type::Int)))
                }
            };

            let expr = make_quantifier(
                QuantifierKind::Exists,
                "x",
                x_id.clone(),
                collection,
                predicate.clone(),
            );

            let values = ValueMap::new();
            let result = eval_expr(&expr, &EvalContext::simple(&values));
            assert_eq!(
                result, *expected,
                "exists({kind:?}, row={name}): expected {expected:?}",
            );
        }
    }
}

/// §9.2.6 truth-table matrix: 4 spec-rows × {List, Set} = 8 assertions.
/// Pins Kleene semantics of `forall` for both container kinds. Cites spec lines
/// 1722–1727.
///
/// Row 2 uses elements `[None, Some(-1), Some(2)]` (i.e. `[undef, -1, 2]`) with
/// predicate `x > 0`. The adversarial ordering ensures `undef` appears **before**
/// the determining `false` on both container kinds:
///
/// - **List** preserves insertion order → predicate results `[undef, false, true]`.
///   Catches a premature-undef short-circuit bug: a broken implementation that
///   returns `Undef` on the first undef result would never reach the determining
///   `false` and would incorrectly yield `Undef` instead of `Bool(false)`.
/// - **Set** is backed by `BTreeSet<Value>`; `Value::Undef` carries the lowest
///   type-tag (0, from `crates/reify-types/src/value.rs` lines 1506–1547) and
///   sorts before `Value::Int(_)` (tag 2), so the BTreeSet iteration order is
///   `[Undef, Int(-1), Int(2)]` → predicate results `[undef, false, true]`.
///   The same adversarial property holds: undef is encountered before the
///   determining false, exercising the "must scan past undef to reach false" path.
#[test]
fn forall_kleene_truth_table_over_list_and_set() {
    #[derive(Debug, Clone, Copy)]
    enum CollKind {
        List,
        Set,
    }

    // Rows: (name, elements as Option<i64>, expected result)
    // Some(i) => Value::Int(i); predicate x > 0 yields Bool(i > 0)
    // None    => Value::Undef; predicate x > 0 on Undef yields Undef
    let rows: Vec<(&str, Vec<Option<i64>>, Value)> = vec![
        // Row 1: all predicate results true → Bool(true)
        (
            "all_true",
            vec![Some(1), Some(2), Some(3)],
            Value::Bool(true),
        ),
        // Row 2: adversarial ordering — undef before the determining false.
        // List (insertion order preserved): undef → false → true.
        //   Catches premature-undef short-circuit bug: must scan past undef to
        //   reach the determining false and return Bool(false).
        // Set (BTreeSet, Undef tag=0 < Int tag=2): sorts to undef → false → true.
        //   Same adversarial property; undef comes before the determining false.
        (
            "any_false_after_undef",
            vec![None, Some(-1), Some(2)],
            Value::Bool(false),
        ),
        // Row 3: no false, undef present → Undef
        // List: [true, undef, true]; Set: [undef, true, true] — both yield Undef
        (
            "no_false_some_undef",
            vec![Some(1), None, Some(3)],
            Value::Undef,
        ),
        // Row 4: empty collection → vacuous truth → Bool(true)
        ("empty", vec![], Value::Bool(true)),
    ];

    for kind in [CollKind::List, CollKind::Set] {
        for (name, elements, expected) in &rows {
            let x_id = ValueCellId::new("$quant0.S", "x");

            // Build element expressions: Some(i) → Int literal, None → Undef literal
            let elem_exprs: Vec<CompiledExpr> = elements
                .iter()
                .map(|opt| match opt {
                    Some(i) => CompiledExpr::literal(Value::Int(*i), Type::Int),
                    None => CompiledExpr::literal(Value::Undef, Type::Int),
                })
                .collect();

            // Build collection for the current container kind
            let collection = match kind {
                CollKind::List => {
                    CompiledExpr::list_literal(elem_exprs, Type::List(Box::new(Type::Int)))
                }
                CollKind::Set => {
                    CompiledExpr::set_literal(elem_exprs, Type::Set(Box::new(Type::Int)))
                }
            };

            // Predicate: x > 0
            let predicate = CompiledExpr::binop(
                BinOp::Gt,
                CompiledExpr::value_ref(x_id.clone(), Type::Int),
                CompiledExpr::literal(Value::Int(0), Type::Int),
                Type::Bool,
            );

            let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

            let values = ValueMap::new();
            let result = eval_expr(&expr, &EvalContext::simple(&values));
            assert_eq!(
                result, *expected,
                "forall({kind:?}, row={name}): expected {expected:?}",
            );
        }
    }
}

/// step-11: Integration test for exists — parse + compile + eval with a false result
#[test]
fn integration_exists_constraint_parse_compile_eval() {
    let source = r#"
structure S {
    let scores = [1, 2, 3, 4, 5]
    let found = exists s in scores: s > 10
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("integ_test2"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = &compiled.templates[0];
    let found_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "found")
        .expect("should have 'found' value cell");
    let found_expr = found_cell.default_expr.as_ref().unwrap();

    // Verify the expression compiled to a Quantifier
    assert!(
        matches!(&found_expr.kind, CompiledExprKind::Quantifier { .. }),
        "expected Quantifier, got {:?}",
        found_expr.kind,
    );

    // Evaluate the scores list first
    let scores_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "scores")
        .expect("should have 'scores' value cell");
    let empty_values = ValueMap::new();
    let scores_value = eval_expr(
        scores_cell.default_expr.as_ref().unwrap(),
        &EvalContext::simple(&empty_values),
    );

    // Now evaluate the exists expression with scores in scope
    let mut values = ValueMap::new();
    values.insert(scores_cell.id.clone(), scores_value);
    let result = eval_expr(found_expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false), "no score > 10, should be false");
}

// ── task-2289 step-8: cell-iteration mode for quantifier evaluation ───────────
//
// When a Quantifier's collection is a `ListLiteral` whose elements are all
// `ValueRef`s (the post-activation shape produced by `activate_purpose`'s
// expansion of `PurposeReflectiveAggregation`), the Quantifier evaluator
// must iterate over the *cell IDs* and rewrite the synthetic loop-var inside
// `DeterminacyPredicate { cell }` to the iterated cell's id. The current
// value-iteration path binds `variable_id → element_value`; that's not enough
// because `DeterminacyPredicate` reads a cell ID, not a value, and the
// synthetic loop-var has no entry in the determinacy snapshot.
//
// These tests exercise the cell-iteration path for both `ForAll` and
// `Exists`, asserting that:
//   (a) the determinacy snapshot is consulted for the iterated cells'
//       actual states (not the synthetic loop-var id);
//   (b) the Kleene short-circuit semantics are preserved (forall: false
//       short-circuits; exists: true short-circuits);
//   (c) NO `debug_assert!` panic fires (i.e. the current "wiring bug" trip
//       in `DeterminacyPredicate` is no longer hit).
//
// RED before step-9 impl: today's quantifier eval falls through to value
// iteration, binds the synthetic loop-var to each `ValueRef`'s cached value,
// then evaluates `DeterminacyPredicate { cell: $loop_var }` — the loop-var
// cell is missing from the determinacy snapshot, so `debug_assert!` fires
// and `Value::Undef` is returned. These tests fail/panic.

/// Build a determinacy snapshot from `(cell, value, state)` triples.
fn make_determinacy_snapshot(
    entries: &[(ValueCellId, Value, DeterminacyState)],
) -> PersistentMap<ValueCellId, (Value, DeterminacyState)> {
    let mut map: PersistentMap<ValueCellId, (Value, DeterminacyState)> = PersistentMap::new();
    for (id, val, state) in entries {
        map.insert(id.clone(), (val.clone(), *state));
    }
    map
}

/// task-2289 step-8 (collection updated to ReflectiveCellList in task-2458):
/// forall over `[ValueRef(E,a), ValueRef(E,b)]` (as `ReflectiveCellList`) with
/// `determined($loop_var)` and `(E,a)=Determined`, `(E,b)=Undetermined`
/// → `Bool(false)` (and NO debug_assert panic).
#[test]
fn forall_cell_iteration_with_determinacy_predicate_returns_false_when_one_undetermined() {
    let cell_a = ValueCellId::new("Bracket", "a");
    let cell_b = ValueCellId::new("Bracket", "b");

    // Synthetic loop-var cell — what the quantifier currently binds.
    let loop_var = ValueCellId::new("$quant0.S", "p");

    // Collection: ReflectiveCellList of two ValueRefs (post-activation shape,
    // task-2458: narrowed from ListLiteral to ReflectiveCellList so the
    // cell-iteration trigger only fires on placeholder-derived lists).
    let collection = CompiledExpr::reflective_cell_list(
        vec![
            CompiledExpr::value_ref(cell_a.clone(), Type::Real),
            CompiledExpr::value_ref(cell_b.clone(), Type::Real),
        ],
        Type::List(Box::new(Type::Real)),
    );

    // Predicate: determined($loop_var)
    let predicate =
        CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, loop_var.clone());

    let expr = make_quantifier(QuantifierKind::ForAll, "p", loop_var, collection, predicate);

    // Determinacy snapshot: (E,a) determined, (E,b) undetermined.
    let snapshot = make_determinacy_snapshot(&[
        (
            cell_a.clone(),
            Value::Real(1.0),
            DeterminacyState::Determined,
        ),
        (cell_b.clone(), Value::Undef, DeterminacyState::Undetermined),
    ]);

    // Provide the same values in the value map so that any value-iteration
    // fallback can still observe a value (we still want to verify the
    // cell-iteration path is the one that fires).
    let mut values = ValueMap::new();
    values.insert(cell_a.clone(), Value::Real(1.0));
    values.insert(cell_b.clone(), Value::Undef);

    let functions: Vec<reify_ir::CompiledFunction> = Vec::new();
    let ctx = EvalContext::new(&values, &functions).with_determinacy(&snapshot);

    let result = eval_expr(&expr, &ctx);
    assert_eq!(
        result,
        Value::Bool(false),
        "forall over [ValueRef(E,a), ValueRef(E,b)] with one Undetermined cell \
         must return Bool(false) under cell-iteration mode (NOT panic on the \
         loop-var debug_assert)"
    );
}

/// task-2289 step-8 (collection updated to ReflectiveCellList in task-2458):
/// exists over `[ValueRef(E,a), ValueRef(E,b)]` (as `ReflectiveCellList`) with
/// `determined($loop_var)` and `(E,a)=Determined`, `(E,b)=Undetermined`
/// → `Bool(true)` (the Determined element short-circuits the exists).
#[test]
fn exists_cell_iteration_with_determinacy_predicate_returns_true_when_one_determined() {
    let cell_a = ValueCellId::new("Bracket", "a");
    let cell_b = ValueCellId::new("Bracket", "b");
    let loop_var = ValueCellId::new("$quant0.S", "p");

    // Collection: ReflectiveCellList (task-2458: narrowed from ListLiteral).
    let collection = CompiledExpr::reflective_cell_list(
        vec![
            CompiledExpr::value_ref(cell_a.clone(), Type::Real),
            CompiledExpr::value_ref(cell_b.clone(), Type::Real),
        ],
        Type::List(Box::new(Type::Real)),
    );

    let predicate =
        CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, loop_var.clone());

    let expr = make_quantifier(QuantifierKind::Exists, "p", loop_var, collection, predicate);

    let snapshot = make_determinacy_snapshot(&[
        (
            cell_a.clone(),
            Value::Real(1.0),
            DeterminacyState::Determined,
        ),
        (cell_b.clone(), Value::Undef, DeterminacyState::Undetermined),
    ]);

    let mut values = ValueMap::new();
    values.insert(cell_a.clone(), Value::Real(1.0));
    values.insert(cell_b.clone(), Value::Undef);

    let functions: Vec<reify_ir::CompiledFunction> = Vec::new();
    let ctx = EvalContext::new(&values, &functions).with_determinacy(&snapshot);

    let result = eval_expr(&expr, &ctx);
    assert_eq!(
        result,
        Value::Bool(true),
        "exists over [ValueRef(E,a), ValueRef(E,b)] with at least one \
         Determined cell must return Bool(true) under cell-iteration mode \
         (NOT panic on the loop-var debug_assert)"
    );
}

/// task-2458 acceptance: user-written `forall p in [E.x, E.y]: p > 0`
/// (collection is a `ListLiteral` of `ValueRef`s) routes through the
/// value-iteration fallback — task-2458 narrowed the cell-iteration trigger
/// to the placeholder-derived `ReflectiveCellList` variant, so this
/// user-source shape no longer matches. The arithmetic-predicate result is
/// observationally equivalent under both paths; the path divergence is
/// exercised separately by
/// `forall_user_written_value_ref_list_uses_value_iteration_not_cell_iteration`.
#[test]
fn forall_user_written_list_of_value_refs_with_arithmetic_predicate_works() {
    let cell_x = ValueCellId::new("E", "x");
    let cell_y = ValueCellId::new("E", "y");
    let loop_var = ValueCellId::new("$quant0.S", "p");

    // Collection: `[E.x, E.y]` — a user could legally write this.
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::value_ref(cell_x.clone(), Type::Real),
            CompiledExpr::value_ref(cell_y.clone(), Type::Real),
        ],
        Type::List(Box::new(Type::Real)),
    );

    // Predicate: `p > 0` — references the loop var via ValueRef. Value-iteration
    // binds `p` to E.x's value / E.y's value per iteration; both are positive.
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(loop_var.clone(), Type::Real),
        CompiledExpr::literal(Value::Real(0.0), Type::Real),
        Type::Bool,
    );

    let expr = make_quantifier(QuantifierKind::ForAll, "p", loop_var, collection, predicate);

    let mut values = ValueMap::new();
    values.insert(cell_x.clone(), Value::Real(1.5));
    values.insert(cell_y.clone(), Value::Real(2.5));

    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Bool(true),
        "user-written `forall p in [E.x, E.y]: p > 0` with positive values \
         in both cells must evaluate to Bool(true) under value-iteration \
         (task-2458: ListLiteral no longer triggers cell-iteration)"
    );

    // Counterpart: flip one cell negative, expect Bool(false).
    let mut values_neg = ValueMap::new();
    values_neg.insert(cell_x.clone(), Value::Real(1.5));
    values_neg.insert(cell_y.clone(), Value::Real(-0.5));

    let predicate2 = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(ValueCellId::new("$quant0.S", "p"), Type::Real),
        CompiledExpr::literal(Value::Real(0.0), Type::Real),
        Type::Bool,
    );
    let collection2 = CompiledExpr::list_literal(
        vec![
            CompiledExpr::value_ref(cell_x.clone(), Type::Real),
            CompiledExpr::value_ref(cell_y.clone(), Type::Real),
        ],
        Type::List(Box::new(Type::Real)),
    );
    let expr2 = make_quantifier(
        QuantifierKind::ForAll,
        "p",
        ValueCellId::new("$quant0.S", "p"),
        collection2,
        predicate2,
    );
    let result2 = eval_expr(&expr2, &EvalContext::simple(&values_neg));
    assert_eq!(
        result2,
        Value::Bool(false),
        "user-written forall with one negative cell must evaluate to Bool(false)"
    );
}

/// task-2458 acceptance: a user-written `forall p in [Bracket.x, Bracket.y]:
/// determined(p)` (collection is a `ListLiteral` of pure `ValueRef`s) must
/// evaluate via the value-iteration fallback — binding `p` to each cell's
/// VALUE — and NOT via cell-iteration (which would rewrite the loop-var's
/// cell-id under the predicate). The two paths diverge for
/// `DeterminacyPredicate { cell: $loop_var }`: cell-iteration would rewrite
/// the cell to `Bracket.x`/`Bracket.y` (both Determined → Bool(true));
/// value-iteration leaves the predicate intact and looks up `$loop_var.p`
/// in the determinacy snapshot, finding the planted `Undetermined` state →
/// `Bool(false)`.
///
/// RED before step-4: current `eval_quantifier` triggers cell-iteration on
/// the ListLiteral-of-ValueRefs shape and returns `Bool(true)`.
#[test]
fn forall_user_written_value_ref_list_uses_value_iteration_not_cell_iteration() {
    let cell_x = ValueCellId::new("Bracket", "x");
    let cell_y = ValueCellId::new("Bracket", "y");
    let loop_var = ValueCellId::new("$quant0.S", "p");

    // Collection: user-authored ListLiteral of ValueRefs (not a ReflectiveCellList).
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::value_ref(cell_x.clone(), Type::Real),
            CompiledExpr::value_ref(cell_y.clone(), Type::Real),
        ],
        Type::List(Box::new(Type::Real)),
    );

    // Predicate: determined($loop_var) — the path-divergence witness.
    // - Cell-iteration: remap_cell rewrites $loop_var → Bracket.x/Bracket.y
    //   (both Determined) → Bool(true).
    // - Value-iteration: $loop_var is left intact; snapshot lookup finds
    //   Undetermined → Bool(false).
    let predicate =
        CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, loop_var.clone());

    let expr = make_quantifier(
        QuantifierKind::ForAll,
        "p",
        loop_var.clone(),
        collection,
        predicate,
    );

    // Determinacy snapshot: Bracket.x and Bracket.y are Determined;
    // $loop_var.p is the path-divergence witness — planted as Undetermined.
    let snapshot = make_determinacy_snapshot(&[
        (
            cell_x.clone(),
            Value::Real(1.0),
            DeterminacyState::Determined,
        ),
        (
            cell_y.clone(),
            Value::Real(2.0),
            DeterminacyState::Determined,
        ),
        // The witness: value-iteration will evaluate determined($loop_var)
        // with this cell intact; it finds Undetermined → false.
        (
            loop_var.clone(),
            Value::Undef,
            DeterminacyState::Undetermined,
        ),
    ]);

    let mut values = ValueMap::new();
    values.insert(cell_x.clone(), Value::Real(1.0));
    values.insert(cell_y.clone(), Value::Real(2.0));
    // No entry for loop_var: value-iteration binds it to the iterated value
    // (Real(1.0) / Real(2.0)), which the DeterminacyPredicate ignores.

    let functions: Vec<reify_ir::CompiledFunction> = Vec::new();
    let ctx = EvalContext::new(&values, &functions).with_determinacy(&snapshot);

    let result = eval_expr(&expr, &ctx);
    assert_eq!(
        result,
        Value::Bool(false),
        "task-2458: user-written ListLiteral-of-ValueRefs must route through \
         value-iteration (not cell-iteration); value-iteration leaves the \
         DeterminacyPredicate cell intact as $loop_var, which is Undetermined \
         in the snapshot → Bool(false). Cell-iteration would incorrectly rewrite \
         the cell to Bracket.x/Bracket.y (both Determined) → Bool(true)."
    );
}
