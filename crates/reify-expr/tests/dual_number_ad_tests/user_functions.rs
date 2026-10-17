//! Arbitrary user algebra — the chain rule through user functions.
//!
//! PRD §7.7 gives AD one job the other two derivative sources cannot do:
//! "constraint residuals of arbitrary user algebra".  Arbitrary means the
//! residual may call functions the user wrote, which call functions the user
//! wrote, and the chain rule has to survive the trip.
//!
//! A callee's parameters and its `let` bindings share one namespace of
//! `ValueCellId::new(&func.name, name)` cells, and reify lets a name be bound
//! there more than once: a `let` shadowing a parameter is a `Shadowing`
//! WARNING, and two same-named `let`s carry no diagnostic at all.  `eval_expr`
//! resolves both by overwrite — the later binding wins the primal — so the
//! tangent overlay must resolve them the same way.  A rebind that was skipped
//! because the new tangent is flat would leave the shadowed binding's tangent
//! live and report a FABRICATED non-zero derivative for an expression the
//! primal says does not move at all; the mirror error is a false refusal, a
//! cell poisoned `Tangent::None` staying poisoned after a plain rebind.

use std::sync::Arc;

use reify_core::ValueCellId;
use reify_expr::{BranchRecord, DualEnv, EvalContext, Seeds, Tangent, eval_dual, eval_expr};
use reify_ir::{BinOp, CompiledExpr, CompiledFunction, FieldSourceKind, Value, ValueMap};
use reify_test_support::builders::expr::{binop, conditional_expr, literal, user_fn_call};
use reify_test_support::builders::function::compiled_function;

use crate::support::{assert_ad_matches_cd_with_fns, calln, cell, dl, pref, probe};

/// A cell inside function `fname`'s own scope — the shape
/// `eval_compiled_function_with_values` binds params under.
fn param_ref(fname: &str, pname: &str) -> CompiledExpr {
    CompiledExpr::value_ref(ValueCellId::new(fname, pname), dl())
}

/// `fn hyp(a, b) = sqrt(a*a + b*b)`
fn hyp_fn() -> CompiledFunction {
    compiled_function(
        "hyp",
        &[("a", dl()), ("b", dl())],
        vec![],
        calln("sqrt", vec![binop(
            BinOp::Add,
            binop(BinOp::Mul, param_ref("hyp", "a"), param_ref("hyp", "a")),
            binop(BinOp::Mul, param_ref("hyp", "b"), param_ref("hyp", "b")),
        )]),
    )
}

// ---------------------------------------------------------------------------
// (1) + (2) A user-function residual: tangent vs central differences, and the
//           primal invariant
// ---------------------------------------------------------------------------

#[test]
fn a_user_function_residual_gradient_agrees_with_central_differences() {
    // r(x, y) = hyp(x, y) − 5, the distance constraint written as user algebra.
    // ∂r/∂x = x/hyp = 0.6, ∂r/∂y = 0.8 at (3, 4).
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let expr = binop(
        BinOp::Sub,
        user_fn_call("hyp", vec![pref("x"), pref("y")], dl()),
        literal(Value::Real(5.0)),
    );
    assert_ad_matches_cd_with_fns("hyp_residual", &expr, &values, &seed_cells, &[hyp_fn()]);
}

#[test]
fn a_user_function_call_keeps_the_primal_invariant_exactly() {
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let fns = [hyp_fn()];
    let ctx = EvalContext::new(&values, &fns);
    let expr = user_fn_call("hyp", vec![pref("x"), pref("y")], dl());
    let seeds = Seeds::new(&seed_cells);
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &seeds, &mut record);
    assert_eq!(dual.value, Value::Real(5.0));
    assert_eq!(dual.value, eval_expr(&expr, &ctx));
}

#[test]
fn a_user_function_let_binding_carries_its_tangent_into_the_result_expression() {
    // fn norm2(a, b) { let s = a*a + b*b; sqrt(s) }
    let f = compiled_function(
        "norm2",
        &[("a", dl()), ("b", dl())],
        vec![(
            "s".to_string(),
            binop(
                BinOp::Add,
                binop(BinOp::Mul, param_ref("norm2", "a"), param_ref("norm2", "a")),
                binop(BinOp::Mul, param_ref("norm2", "b"), param_ref("norm2", "b")),
            ),
        )],
        calln("sqrt", vec![CompiledExpr::value_ref(ValueCellId::new("norm2", "s"), dl())]),
    );
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let expr = user_fn_call("norm2", vec![pref("x"), pref("y")], dl());
    assert_ad_matches_cd_with_fns("norm2_let", &expr, &values, &seed_cells, &[f]);
}

// ---------------------------------------------------------------------------
// (2b) SHADOWING: a cell bound twice, where the LATER binding wins
// ---------------------------------------------------------------------------

/// The tangent a read of `cell` would see: an unbound cell and a cell bound
/// flat are the same answer at the read site, so this asserts what the
/// evaluator observes rather than which of the two the binder chose.
fn visible_tangent(env: &DualEnv, cell: &ValueCellId, width: usize) -> Option<Vec<f64>> {
    match env.get(cell) {
        None => Some(vec![0.0; width]),
        Some(t) => t.materialize(width),
    }
}

#[test]
fn rebinding_a_cell_to_a_flat_tangent_displaces_the_earlier_one() {
    let mut env = DualEnv::new();
    let c = ValueCellId::new("f", "a");
    env.bind(c.clone(), Tangent::from_row(vec![1.0, 2.0]));
    env.bind(c.clone(), Tangent::Zero);
    assert_eq!(
        visible_tangent(&env, &c, 2),
        Some(vec![0.0, 0.0]),
        "a flat rebind must be authoritative, not a no-op that leaves the old tangent live"
    );
}

#[test]
fn rebinding_a_refused_cell_to_a_real_tangent_retires_the_refusal() {
    let mut env = DualEnv::new();
    let c = ValueCellId::new("f", "a");
    env.bind(c.clone(), Tangent::None);
    env.bind(c.clone(), Tangent::from_row(vec![1.0]));
    assert_eq!(
        visible_tangent(&env, &c, 1),
        Some(vec![1.0]),
        "a cell rebound to a real tangent is differentiable again"
    );
}

#[test]
fn a_let_shadowing_a_parameter_flattens_that_parameters_tangent() {
    // fn shadow(a) { let a = 5.0; a * 2.0 } — the body never reads the
    // argument, so the result is 10.0 for every x and ∂/∂x is exactly 0.
    let f = compiled_function(
        "shadow",
        &[("a", dl())],
        vec![("a".to_string(), literal(Value::Real(5.0)))],
        binop(BinOp::Mul, param_ref("shadow", "a"), literal(Value::Real(2.0))),
    );
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let expr = user_fn_call("shadow", vec![pref("x")], dl());
    let fns = [f];
    let ctx = EvalContext::new(&values, &fns);
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &Seeds::new(&seed_cells), &mut record);

    assert_eq!(dual.value, eval_expr(&expr, &ctx), "the primal invariant holds under shadowing");
    assert_eq!(dual.value, Value::Real(10.0));
    assert_eq!(
        dual.tangent.materialize(1),
        Some(vec![0.0]),
        "the shadowing let is seed-independent, so the call is flat in x"
    );
}

#[test]
fn the_later_of_two_same_named_lets_displaces_the_earlier_ones_tangent() {
    // fn two_lets(a) { let s = a * 3.0; let s = 7.0; s + 1.0 }.  Two lets of
    // one name emit NO diagnostic, so this shape reaches the evaluator with
    // nothing upstream having flagged it.
    let f = compiled_function(
        "two_lets",
        &[("a", dl())],
        vec![
            (
                "s".to_string(),
                binop(BinOp::Mul, param_ref("two_lets", "a"), literal(Value::Real(3.0))),
            ),
            ("s".to_string(), literal(Value::Real(7.0))),
        ],
        binop(BinOp::Add, param_ref("two_lets", "s"), literal(Value::Real(1.0))),
    );
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let expr = user_fn_call("two_lets", vec![pref("x")], dl());
    let fns = [f];
    let ctx = EvalContext::new(&values, &fns);
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &Seeds::new(&seed_cells), &mut record);

    assert_eq!(dual.value, eval_expr(&expr, &ctx), "the primal invariant holds under shadowing");
    assert_eq!(dual.value, Value::Real(8.0));
    assert_eq!(
        dual.tangent.materialize(1),
        Some(vec![0.0]),
        "the surviving `s` is the constant one, so the call is flat in x"
    );
}

#[test]
fn a_let_shadowing_a_parameter_leaves_the_rest_of_the_residual_differentiable() {
    // The contrast that makes the flat column meaningful: the shadowed
    // parameter must go to zero WITHOUT the row collapsing or refusing, so
    // the sibling term's real derivative still arrives.  r = shadow(x) + x*x.
    let f = compiled_function(
        "shadow",
        &[("a", dl())],
        vec![("a".to_string(), literal(Value::Real(5.0)))],
        binop(BinOp::Mul, param_ref("shadow", "a"), literal(Value::Real(2.0))),
    );
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let expr = binop(
        BinOp::Add,
        user_fn_call("shadow", vec![pref("x")], dl()),
        binop(BinOp::Mul, pref("x"), pref("x")),
    );
    let fns = [f];
    let ctx = EvalContext::new(&values, &fns);
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &Seeds::new(&seed_cells), &mut record);

    assert_eq!(dual.value, eval_expr(&expr, &ctx), "the primal invariant holds under shadowing");
    assert_eq!(dual.value, Value::Real(19.0));
    assert_eq!(
        dual.tangent.materialize(1),
        Some(vec![6.0]),
        "only the x*x term moves: ∂r/∂x = 2x = 6"
    );
}


// ---------------------------------------------------------------------------
// (3) Nested user-function calls
// ---------------------------------------------------------------------------

#[test]
fn a_user_function_calling_another_user_function_propagates_the_chain_rule() {
    // fn twice_hyp(a, b) = hyp(a, b) * 2
    let outer = compiled_function(
        "twice_hyp",
        &[("a", dl()), ("b", dl())],
        vec![],
        binop(
            BinOp::Mul,
            user_fn_call("hyp", vec![param_ref("twice_hyp", "a"), param_ref("twice_hyp", "b")], dl()),
            literal(Value::Real(2.0)),
        ),
    );
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let expr = user_fn_call("twice_hyp", vec![pref("x"), pref("y")], dl());
    assert_ad_matches_cd_with_fns("nested", &expr, &values, &seed_cells, &[hyp_fn(), outer]);
}

// ---------------------------------------------------------------------------
// (4) A Lambda applied through `apply_lambda`
// ---------------------------------------------------------------------------

/// A `Value::Field` whose source is Analytical and whose lambda is `p ↦ p·p·p`.
/// Registering it at the cell `__field__::cube` makes `cube(x)` in an
/// expression resolve to `apply_lambda`, which is the reachable scalar-in
/// scalar-out lambda-application route in a residual.
fn cube_field_cell() -> (ValueCellId, Value) {
    let p = ValueCellId::new("$lambda_cube", "p");
    let body = binop(
        BinOp::Mul,
        CompiledExpr::value_ref(p.clone(), dl()),
        binop(
            BinOp::Mul,
            CompiledExpr::value_ref(p.clone(), dl()),
            CompiledExpr::value_ref(p.clone(), dl()),
        ),
    );
    let lambda = Value::Lambda {
        params: vec![("p".to_string(), p)],
        body: Box::new(body),
        captures: ValueMap::new(),
    };
    let field = Value::Field {
        domain_type: dl(),
        codomain_type: dl(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };
    (ValueCellId::new(reify_core::FIELD_ENTITY_PREFIX, "cube"), field)
}

#[test]
fn a_lambda_applied_inside_a_residual_propagates_its_argument_tangent() {
    // cube(x) with x = 2 → 8, d/dx = 3x² = 12.
    let (mut values, seed_cells) = probe(&[("x", 2.0)]);
    let (cell_id, field) = cube_field_cell();
    values.insert(cell_id, field);
    let expr = calln("cube", vec![pref("x")]);
    assert_ad_matches_cd_with_fns("lambda_cube", &expr, &values, &seed_cells, &[]);
}

// ---------------------------------------------------------------------------
// (5) A kink inside a user-function body
// ---------------------------------------------------------------------------

#[test]
fn a_kink_inside_a_user_function_body_is_recorded_at_a_site_that_includes_the_call_site() {
    // fn clip(a) = if a > 1 then a*a else a
    let f = compiled_function(
        "clip",
        &[("a", dl())],
        vec![],
        conditional_expr(
            binop(BinOp::Gt, param_ref("clip", "a"), literal(Value::Real(1.0))),
            binop(BinOp::Mul, param_ref("clip", "a"), param_ref("clip", "a")),
            param_ref("clip", "a"),
        ),
    );
    let fns = [f];
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let ctx = EvalContext::new(&values, &fns);
    let seeds = Seeds::new(&seed_cells);

    let expr = user_fn_call("clip", vec![pref("x")], dl());
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &seeds, &mut record);
    assert_eq!(dual.value, Value::Real(9.0));
    assert_eq!(
        dual.tangent.materialize(1).expect("differentiable"),
        vec![6.0],
        "the ACTIVE branch's derivative: d(a²)/da = 2a = 6"
    );

    // The comparison and the conditional are both kinks, and both are inside
    // the callee — so both must descend through `CALLEE_MARKER` explicitly.
    // A mere `!is_empty()` is satisfied by ANY nested path, including one that
    // never pushed the marker at all, which is precisely the scheme that keeps
    // two call sites of the same function distinguishable to λ.
    assert_eq!(record.len(), 2, "the `>` comparison and the `if`");
    for entry in record.entries() {
        assert_eq!(
            entry.site.path().first().copied(),
            Some(reify_expr::CALLEE_MARKER),
            "a kink inside a callee must descend through CALLEE_MARKER, got {:?}",
            entry.site.path()
        );
    }

    // Two distinct call sites of the same function get distinct sites.
    let two_calls = binop(
        BinOp::Add,
        user_fn_call("clip", vec![pref("x")], dl()),
        user_fn_call("clip", vec![pref("x")], dl()),
    );
    let mut rec2 = BranchRecord::new();
    let _ = eval_dual(&two_calls, &ctx, &seeds, &mut rec2);
    assert_eq!(rec2.len(), 4, "two calls × two kinks each");
    let sites: Vec<&[u16]> = rec2.entries().iter().map(|e| e.site.path()).collect();
    // Each call site contributes its own two-segment PREFIX — its argument
    // index under the `+`, then the callee descent — and everything after the
    // marker is identical between the two.  That is the whole namespacing
    // scheme: what distinguishes two calls of one function is the prefix, and
    // nothing else.
    for (k, site) in sites.iter().enumerate() {
        let call_index = (k / 2) as u16;
        assert_eq!(
            site[..2],
            [call_index, reify_expr::CALLEE_MARKER],
            "entry {k} must sit under call site {call_index}'s own descent, got {site:?}"
        );
    }
    assert_eq!(sites[0][2..], sites[2][2..], "the same kink, at the same place inside the callee");
    assert_ne!(sites[0], sites[2], "the same kink at two call sites must not collide");
    assert_ne!(sites[1], sites[3]);
}

// ---------------------------------------------------------------------------
// (6) The recursion guard
// ---------------------------------------------------------------------------

#[test]
fn unbounded_user_function_recursion_yields_undef_and_no_tangent_rather_than_a_stack_overflow() {
    // fn recur(a) = recur(a) — `eval_user_function_call` stops at
    // MAX_RECURSION_DEPTH (256) and returns Undef.  The dual evaluator must
    // hit the SAME guard, not run deeper and blow the 3 MiB test-thread stack.
    //
    // THE STACK SIZE IS PINNED, NOT "GENEROUS" — the same discipline as
    // `eval_user_fn_recursion_depth_exceeded` in `reify-expr/src/lib.rs`, which
    // this test is the dual-path sibling of.  Rust's test harness gives a
    // SPAWNED test thread only 2 MiB, which is already short of what plain
    // `eval_expr` needs at depth 256, so the wrapper is mandatory rather than
    // decorative.  `eval_dual`'s own frames sit on top of that budget, so this
    // figure is larger than the evaluator's 3 MiB — and it stays a working pin
    // for the dual path's per-frame budget only while it is tight enough that a
    // frame-size regression actually overflows it.
    //
    // MEASURED on this tree at MAX_RECURSION_DEPTH (256), debug profile, by
    // bisecting this constant: overflows at 2048 KiB, passes at 2176 KiB.  That
    // is within noise of the evaluator's own measured 2304 KiB, i.e. the dual
    // traversal costs no more per level than `eval_expr` does — which is the
    // whole point of the `#[inline(never)]` split around `eval_dual_fn_body`
    // and `eval_dual_lambda_apply`, and is what this test exists to keep true.
    //
    // 3 MiB = ~41% over the measured 2176 KiB requirement: enough that ordinary
    // codegen drift does not redden the gate, tight enough that a further
    // ~900 KiB (~3.5 KiB/level) regression does.  If a toolchain bump reddens
    // this, RE-MEASURE by bisecting the constant and update the figures here —
    // do not just raise it for headroom, which would silently retire the pin.
    let handle = std::thread::Builder::new()
        .stack_size(3 * 1024 * 1024)
        .spawn(|| {
            let f = compiled_function("recur", &[("a", dl())], vec![], user_fn_call(
                "recur",
                vec![param_ref("recur", "a")],
                dl(),
            ));
            let fns = [f];
            let (values, seed_cells) = probe(&[("x", 2.0)]);
            let ctx = EvalContext::new(&values, &fns);
            let seeds = Seeds::new(&seed_cells);
            let expr = user_fn_call("recur", vec![pref("x")], dl());

            assert_eq!(eval_expr(&expr, &ctx), Value::Undef, "the existing guard returns Undef");
            let mut record = BranchRecord::new();
            let dual = eval_dual(&expr, &ctx, &seeds, &mut record);
            assert_eq!(dual.value, Value::Undef, "the dual path must hit the same guard");
            assert_eq!(
                dual.tangent,
                Tangent::None,
                "an Undef primal has no derivative — least of all a zero one"
            );
        })
        .expect("spawn");
    handle.join().expect("the recursion guard must fire before the stack runs out");
}

// ---------------------------------------------------------------------------
// The callee overlay is the INNER scope: it must shadow the seed columns.
// ---------------------------------------------------------------------------

#[test]
fn a_callee_parameter_cell_colliding_with_a_seed_column_uses_the_call_site_tangent() {
    // `ValueCellId` has two namespaces that are not disjoint by construction:
    // a seed is `(entity, auto_param)` while a callee cell is
    // `(function_name, param_or_let_name)`.  Here the fixture entity is `part`
    // and the function is also named `part` with a param `x`, so the callee's
    // parameter cell IS the seed cell — the same collapse onto one
    // `ValueCellId` as the shadowing defect, one scope out.
    //
    // `fn part(x) = x * 2.0` called as `part(3.0 * x)` is 6x, so ∂/∂x = 6.
    // Resolving the body's `ValueRef` through the SEED column instead of the
    // overlay yields the unit row and reports 2 — the derivative of the
    // function, with the argument's chain rule dropped on the floor.
    let f = compiled_function("part", &[("x", dl())], vec![], binop(
        BinOp::Mul,
        param_ref("part", "x"),
        literal(Value::Real(2.0)),
    ));
    assert_eq!(
        param_ref("part", "x").collect_value_refs(),
        vec![cell("x")],
        "the premise: the callee's parameter cell and the seed cell are the same cell"
    );

    let (values, seed_cells) = probe(&[("x", 1.5)]);
    let expr = user_fn_call("part", vec![binop(
        BinOp::Mul,
        literal(Value::Real(3.0)),
        pref("x"),
    )], dl());
    assert_ad_matches_cd_with_fns(
        "part(3x) where part's param cell collides with the seed",
        &expr,
        &values,
        &seed_cells,
        std::slice::from_ref(&f),
    );
}

// ---------------------------------------------------------------------------
// A kink the fast path cannot see: inside a callee body.
// ---------------------------------------------------------------------------

#[test]
fn a_kink_inside_a_seed_independent_callee_body_is_still_recorded() {
    use reify_expr::KinkKind;

    // `Seeds::subtree_has_kink` walks the CompiledExpr tree, and `walk` cannot
    // descend into a `UserFunctionCall`'s callee body — a bare `CompiledExpr`
    // has no access to the function table.  So a seed-independent call whose
    // body branches satisfied all three fast-path predicates and was handed
    // wholesale to `eval_expr`, contributing zero entries while a branch
    // really was selected.  That is the "we did not look" vs "there is no
    // kink" confusion λ (#6679) may never be handed.
    let jump = compiled_function(
        "jump",
        &[("q", dl())],
        vec![],
        conditional_expr(
            binop(BinOp::Gt, param_ref("jump", "q"), literal(Value::Real(5.0))),
            literal(Value::Real(100.0)),
            literal(Value::Real(200.0)),
        ),
    );
    // `c` is present but NOT seeded, so `jump(c)` is provably seed-independent
    // and the whole subtree is fast-path eligible on tangents alone.
    let mut values = ValueMap::new();
    values.insert(cell("x"), Value::Real(2.0));
    values.insert(cell("c"), Value::Real(9.0));
    let seed_cells = vec![cell("x")];
    let expr = binop(BinOp::Add, pref("x"), user_fn_call("jump", vec![pref("c")], dl()));

    let ctx = EvalContext::new(&values, std::slice::from_ref(&jump));
    let seeds = Seeds::new(&seed_cells);
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &seeds, &mut record);

    assert_eq!(dual.value, eval_expr(&expr, &ctx), "the primal invariant");
    assert_eq!(
        dual.tangent.materialize(1).unwrap_or_else(|| panic!("differentiable in x")),
        vec![1.0],
        "the flat-but-jumping callee contributes no tangent, and vetoes none either"
    );
    assert!(
        record.entries().iter().any(|e| e.kind == KinkKind::Conditional),
        "the branch inside the callee body must reach the record: {:?}",
        record.entries()
    );
}
