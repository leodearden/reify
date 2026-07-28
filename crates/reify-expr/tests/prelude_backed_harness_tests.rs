//! Prelude-backed eval-harness tests — task 5593, PRD
//! docs/prds/v0_6/placeholder-type-eradication-ratchet.md §8 BT10 / INV-SF-5.
//!
//! WHY THIS FILE EXISTS
//!
//! `reify_test_support::compile_source_with_stdlib` returns a `CompiledModule`
//! whose `.functions` is USER-SOURCE-ONLY by construction: the compile pipeline
//! pushes only this module's own AST fns into `ctx.functions`
//! (compile_builder/functions_phase.rs:79-92), and routes prelude fns to a
//! DIFFERENT field, `ctx.resolution_functions` (functions_phase.rs:101-105),
//! which `ctx.rs:201` then drops when building the `CompiledModule`.
//!
//! Consequence: evaluating under `EvalContext::new(&values, &module.functions)`
//! uses a table in which NO stdlib `.ri` body is registered. Every stdlib call
//! misses `find_matching_compiled_function` and returns `Value::Undef` from the
//! lookup-miss arm at reify-expr/src/lib.rs:1625. That harness therefore cannot
//! observe a `.ri` placeholder body at all — a guard written against it proves
//! only "the intercept fires", never "the intercept beats the placeholder".
//!
//! `reify_test_support::prelude_backed_functions` fixes that by reproducing the
//! PRODUCTION runtime dispatch table (`reify_eval::merge_functions`,
//! reify-eval/src/lib.rs:1425-1432). This file pins that the fix is real.
//!
//! THE LIVENESS WITNESS AND WHY IT USES `through`
//!
//! All 15 combinators in stdlib/option_recovery.ri and stdlib/result.ri are
//! intercept-shadowed (reify-expr/src/lib.rs:735, 755, 774), so none of them can
//! serve as its own witness: while the intercepts are live their `.ri` body can
//! never run. The control is therefore `pub fn through<T>(x: T) -> T { x }`
//! (crates/reify-compiler/stdlib/fields.ri:68) — generic, `pub`, trivially
//! bodied, and with NO intercept anywhere in crates/reify-expr/src or
//! crates/reify-eval/src. Same expression, same `ValueMap`, only the function
//! slice differs; so a non-`Undef` result can ONLY have come from executing the
//! `.ri` body.

use reify_core::DimensionVector;
use reify_ir::{CompiledExpr, CompiledExprKind, Value, ValueMap};

/// 5mm as the evaluator represents it.
fn val_5mm() -> Value {
    Value::Scalar {
        si_value: 0.005,
        dimension: DimensionVector::LENGTH,
    }
}

/// The harness-liveness witness: a prelude-backed function table actually
/// EXECUTES stdlib `.ri` bodies, where the user-source-only table cannot.
///
/// Both halves evaluate the SAME `CompiledExpr` against the SAME (empty)
/// `ValueMap`. The only thing that varies is the function slice handed to
/// `EvalContext::new`, which isolates the claim to table construction:
///
/// (a) under `module.functions` — the user-source-only table — `through(5mm)`
///     misses the lookup and yields `Value::Undef` (reify-expr/src/lib.rs:1625);
/// (b) under `prelude_backed_functions(&module)` it yields 5mm, which is
///     `through`'s `.ri` body `{ x }` having genuinely run.
///
/// Dispatch works because `find_matching_compiled_function`
/// (reify-expr/src/lib.rs:1533-1574) is a two-pass resolver: pass 1 requires
/// exact `Type` equality; pass 2, gated on `!f.type_params.is_empty()`, treats
/// any param carrying a type/dim param as an unconditional wildcard. That
/// second pass is what lets the generic `through<T>(x: T)` signature bind a
/// concrete `Scalar[m]` argument — and it is the mechanism this whole harness
/// depends on.
///
/// If a future change regresses the guards back to a user-source-only table,
/// half (b) breaks loudly here rather than silently degrading the 17
/// `e2e_*_with_stdlib` guards back to `Undef`-only signal.
#[test]
fn prelude_backed_table_executes_stdlib_ri_bodies() {
    let module =
        reify_test_support::compile_source_with_stdlib("structure S { let v = through(5mm) }");
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();

    // (a) user-source-only table: no stdlib body is registered, so the call
    //     misses lookup entirely.
    let user_only_ctx = reify_expr::EvalContext::new(&values, &module.functions);
    assert_eq!(
        reify_expr::eval_expr(expr, &user_only_ctx),
        Value::Undef,
        "control: under module.functions (user-source-only) the stdlib fn \
         `through` is not registered, so eval_user_function_call's lookup-miss \
         arm (reify-expr/src/lib.rs:1625) must return Undef"
    );

    // (b) prelude-backed table: the `.ri` body is registered AND executes.
    let prelude_backed = reify_test_support::prelude_backed_functions(&module);
    let prelude_ctx = reify_expr::EvalContext::new(&values, &prelude_backed);
    assert_eq!(
        reify_expr::eval_expr(expr, &prelude_ctx),
        val_5mm(),
        "liveness witness: under the prelude-backed table `through(5mm)` must \
         return 5mm — `through` has NO intercept, so this value can only have \
         been produced by executing its stdlib `.ri` body `{{ x }}`"
    );
}

// ── competitor census ────────────────────────────────────────────────────────

/// The fixture source of each guarded `e2e_*_with_stdlib` test, reused verbatim
/// so the census measures the SAME call sites the guards evaluate.
///
/// `(pair label, guard test name, fixture source)`. One entry per intercepted
/// `(name, arity)` pair — `is_combinator`'s ten
/// (crates/reify-expr/src/option_recovery.rs:119-133) plus `map_or`/3 and
/// `map_err`/2, which have their own gates at reify-expr/src/lib.rs:755 and 774.
const CENSUS: &[(&str, &str, &str)] = &[
    (
        "unwrap_or/2",
        "e2e_unwrap_or_some_5mm_with_stdlib",
        "structure S { let v = unwrap_or(some(5mm), 0mm) }",
    ),
    (
        "or_default/2",
        "e2e_or_default_some_with_stdlib",
        "structure S { let v = or_default(some(5mm), 0mm) }",
    ),
    (
        "or_else/2",
        "e2e_or_else_none_subject_with_stdlib",
        "structure S { param o : Option<Length> = none  let v = or_else(o, some(3mm)) }",
    ),
    (
        "is_some/1",
        "e2e_is_some_none_with_stdlib",
        "structure S { let v = is_some(none) }",
    ),
    (
        "is_none/1",
        "e2e_is_none_none_with_stdlib",
        "structure S { let v = is_none(none) }",
    ),
    (
        "get_or/3",
        "e2e_get_or_absent_key_with_stdlib",
        r#"structure S { let v = get_or(map{"k" => 1mm}, "absent", 0mm) }"#,
    ),
    (
        "fallback/2",
        "e2e_result_fallback_ok_with_stdlib",
        "structure S { let v = fallback(Ok { value: 5mm }, 0mm) }",
    ),
    (
        "is_ok/1",
        "e2e_result_is_ok_err_with_stdlib",
        r#"structure S { let v = is_ok(Err { error: "e" }) }"#,
    ),
    (
        "is_err/1",
        "e2e_result_is_err_err_with_stdlib",
        r#"structure S { let v = is_err(Err { error: "e" }) }"#,
    ),
    (
        "ok_or/2",
        "e2e_ok_or_some_with_stdlib",
        r#"structure S { let v = ok_or(some(5mm), "e") }"#,
    ),
    (
        "map_or/3",
        "e2e_map_or_some_with_stdlib",
        "structure S { let v = map_or(some(5mm), 0mm, |x: Length| x * 2) }",
    ),
    (
        "map_err/2",
        "e2e_map_err_err_with_stdlib",
        "structure S { let v = map_err(Err { error: 3mm }, |e: Length| e * 2) }",
    ),
];

/// Destructure a compiled cell expr into the `(function_name, args)` a call site
/// hands to `find_matching_compiled_function`.
fn as_call(expr: &CompiledExpr) -> (&str, &[CompiledExpr]) {
    match &expr.kind {
        CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => (function_name.as_str(), args.as_slice()),
        other => panic!(
            "census fixture must compile to a UserFunctionCall (that is the node \
             the reify-expr intercepts gate on); got {other:?}"
        ),
    }
}

/// The census: for every intercepted `(name, arity)` pair, the stdlib `.ri`
/// placeholder is a LIVE competing candidate under the prelude-backed table and
/// was NOT a candidate at all under the old user-source-only one.
///
/// This is the automatable half of the "genuinely compete" claim in this task's
/// title. Paired with `prelude_backed_table_executes_stdlib_ri_bodies` above —
/// which proves `.ri` bodies EXECUTE under this table — it decomposes the claim
/// into two permanently CI-enforced halves:
///
///   executes (liveness witness) + is-matched (this census) = competes.
///
/// Neither half can be observed directly on the combinators themselves while the
/// intercepts are live, which is exactly why the claim is split this way rather
/// than tested by an intercept-removal edit that must never be committed.
///
/// Note this is also the first coverage of pass 2 of
/// `find_matching_compiled_function`: the matcher's own unit-test file,
/// crates/reify-expr/tests/find_matching_compiled_function_tests.rs, builds every
/// fixture with `type_params: vec![]` and so never reaches the wildcard pass.
///
/// MEASURED RESULT: all 12 pairs match, including `map_or`/3 and `map_err`/2,
/// whose lambda argument passes through the wildcard as a
/// `Function{params, return_type}` param. No pair had to be excluded as a
/// measured limitation.
///
/// On assertion (a) being trivially satisfiable: none of these fixtures defines
/// a user `fn`, so `module.functions` is empty and (a) can only hold. That is
/// deliberate and is asserted explicitly below rather than left implicit — an
/// empty user table is precisely the state the 17 guards were evaluating in, and
/// pinning it is what makes (b) meaningful: with no user fn in play, ANY match
/// under the prelude-backed table necessarily came from a stdlib `.ri` fn.
#[test]
fn stdlib_placeholders_are_live_candidates_under_the_prelude_backed_table() {
    // Collect every failure and report them together — a census that aborts on
    // the first mismatch hides how many pairs are affected.
    let mut failures: Vec<String> = Vec::new();
    let mut matched_count = 0usize;

    for (pair, guard, source) in CENSUS {
        let module = reify_test_support::compile_source_with_stdlib(source);
        let expr = reify_test_support::get_let_expr(&module, "v");
        let (name, args) = as_call(expr);
        let prelude_backed = reify_test_support::prelude_backed_functions(&module);

        // Anti-vacuity: the fixture must contribute no user fn, so that (a)'s
        // `None` is a statement about the HARNESS (the prelude was dropped) and
        // not about the fixture having shadowed something.
        if !module.functions.is_empty() {
            failures.push(format!(
                "{pair} ({guard}): fixture unexpectedly defines user fn(s) {:?} — \
                 the census assumes an empty user table",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            ));
        }

        // (a) the OLD harness had no competitor at all.
        if let Some(f) = reify_expr::find_matching_compiled_function(&module.functions, name, args)
        {
            failures.push(format!(
                "{pair} ({guard}): expected NO candidate under module.functions \
                 (user-source-only), but matched {}/{}",
                f.name,
                f.params.len()
            ));
        }

        // (b) the placeholder IS a live competing candidate under the new table.
        let Some(matched) =
            reify_expr::find_matching_compiled_function(&prelude_backed, name, args)
        else {
            // MEASURED-LIMITATION path: report the shapes rather than weakening
            // the assertion into vacuity.
            let candidates: Vec<String> = prelude_backed
                .iter()
                .filter(|f| f.name == name && f.params.len() == args.len())
                .map(|f| {
                    format!(
                        "{}<{:?}>({:?})",
                        f.name,
                        f.type_params.iter().map(|p| &p.name).collect::<Vec<_>>(),
                        f.params.iter().map(|(_, t)| t).collect::<Vec<_>>()
                    )
                })
                .collect();
            let arg_types: Vec<_> = args.iter().map(|a| &a.result_type).collect();
            failures.push(format!(
                "{pair} ({guard}): NO match under the prelude-backed table.\n    \
                 call-site arg result_types: {arg_types:?}\n    \
                 same-name/arity candidates: {candidates:?}"
            ));
            continue;
        };

        matched_count += 1;

        // (c) the match is a prelude placeholder, not a user fn.
        if matched.type_params.is_empty() {
            failures.push(format!(
                "{pair} ({guard}): matched {} but it is NOT generic \
                 (type_params empty) — the guarded stdlib combinators are all \
                 generic, so a non-generic match means the census resolved \
                 something else",
                matched.name
            ));
        }
        if module
            .functions
            .iter()
            .any(|f| f.name == matched.name && f.params.len() == matched.params.len())
        {
            failures.push(format!(
                "{pair} ({guard}): matched {} but a same-signature fn also exists \
                 in module.functions — the match must come from the prelude",
                matched.name
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "competitor census found {} problem(s):\n  - {}",
        failures.len(),
        failures.join("\n  - ")
    );
    assert_eq!(
        matched_count,
        CENSUS.len(),
        "every intercepted (name, arity) pair must have a live prelude candidate; \
         this pins the census against silently shrinking to a subset"
    );
}
