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
use reify_ir::{Value, ValueMap};

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
