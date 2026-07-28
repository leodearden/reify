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
//!
//! MEASURED: WHAT THE 17 GUARDS OBSERVE UNDER INTERCEPT REMOVAL
//!
//! The signal this task exists to create. Measured by locally disabling all
//! three intercept gates (`if false && …` at reify-expr/src/lib.rs:735, 755,
//! 774 — an edit that must NEVER be committed) and reading each guard's `left:`
//! value. Both runs are on this branch; only the harness differs.
//!
//! BEFORE (guards on `module.functions`): 17 of 17 fail with `left: Undef`.
//! Not one placeholder value. The `.ri` bodies were not in the table at all, so
//! the guards could only ever have proven "the intercept fires" — never "the
//! intercept beats the placeholder".
//!
//! AFTER (guards on `prelude_backed_functions`): 0 of 17 report `Undef`; 16
//! report the stdlib placeholder's WRONG VALUE, and the 17th passes.
//!
//!   guard                                  placeholder observed   from
//!   e2e_unwrap_or_some_5mm_with_stdlib     0mm                    `{ dflt }`
//!   e2e_or_default_some_with_stdlib        0mm                    `{ dflt }`
//!   e2e_or_else_none_subject_with_stdlib   Option(None)           `{ o }`
//!   e2e_is_some_none_with_stdlib           Bool(true)             `{ true }`
//!   e2e_is_none_none_with_stdlib           Bool(false)            `{ false }`
//!   e2e_get_or_present_key_with_stdlib     0mm                    `{ dflt }`
//!   e2e_get_or_undef_map_with_stdlib (`w`) 0mm                    `{ dflt }`
//!   e2e_map_or_some_with_stdlib            0mm                    `{ dflt }`
//!   e2e_result_unwrap_or_ok_with_stdlib †  0mm                    `{ dflt }`
//!   e2e_result_fallback_ok_with_stdlib  †  0mm                    `{ dflt }`
//!   e2e_result_is_ok_err_with_stdlib       Bool(true)             `{ true }`
//!   e2e_result_is_err_err_with_stdlib      Bool(false)            `{ false }`
//!   e2e_result_or_else_err_with_stdlib  †  Err{error:"e"}         `{ o }`
//!   e2e_map_err_err_with_stdlib            Err{error:3mm}         `{ r }`
//!   e2e_ok_or_some_with_stdlib             String("e")            `{ err }`
//!   e2e_ok_or_none_with_stdlib             String("e")            `{ err }`
//!   e2e_get_or_absent_key_with_stdlib      0mm                    `{ dflt }`  <-- PASSES
//!
//! † THE MATCHED CANDIDATE IS option_recovery.ri's OPTION OVERLOAD, NOT
//! result.ri's. `unwrap_or`, `or_else` and `fallback` are each declared in BOTH
//! stdlib files, and these three fixtures' `arg[0].result_type` is
//! `Enum("Result")` — which never exact-equals result.ri's
//! `Applied{name:"Result", args:[T,E]}`, so pass 1 of
//! `find_matching_compiled_function` misses both candidates and pass 2's
//! wildcard takes the first in table order. `std.option_recovery` loads before
//! `std.result`, so the Option overload wins. MEASURED: matched
//! `params[0] == Option(TypeParam("T"))` for all three; result.ri's own
//! `unwrap_or`/`or_else`/`fallback` placeholders are never the selected
//! candidate anywhere in this suite. The `from` column above is therefore the
//! Option overload's body, and the observable value only coincides with
//! result.ri's because each overload pair returns the same positional argument.
//! Full statement: the OVERLOAD NOTE in
//! crates/reify-expr/tests/result_combinator_eval_tests.rs. This is pre-existing
//! reify-expr resolution behaviour, out of scope for #5593, and is pinned below
//! by `census_pins_which_overload_the_matcher_selects` so a future resolver
//! change flips a test RED instead of silently rewriting what this table means.
//!
//! TWO OF THE 17 GUARDS ARE COINCIDENCE-LIMITED even under this harness —
//! `e2e_get_or_absent_key_with_stdlib` (the one that PASSES above) and the `v`
//! half of `e2e_get_or_undef_map_with_stdlib`. Why each is limited, and what
//! preserves `get_or` coverage regardless, is stated ONCE in the CANONICAL
//! MECHANISM NOTE above `e2e_or_default_some_with_stdlib` in
//! crates/reify-expr/tests/option_recovery_eval_tests.rs; this file defers to it
//! rather than restating it.

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

/// One census row: an intercepted `(name, arity)` pair, the `e2e_*_with_stdlib`
/// guard that exercises it, that guard's fixture source, and the MEASURED
/// overload family the matcher actually selects for it.
struct CensusRow {
    /// `name/arity` of the intercepted pair, used as the failure label.
    pair: &'static str,
    /// The `e2e_*_with_stdlib` guard this row mirrors. Kept in sync with the
    /// guard's real fixture by `census_fixture_sources_match_the_guards`.
    guard: &'static str,
    /// Copied verbatim from `guard`'s body so the census measures the SAME call
    /// site the guard evaluates.
    source: &'static str,
    /// MEASURED `overload_family` of the matched candidate's `params[0]`.
    ///
    /// Pinned per row rather than derived, because for three pairs it does NOT
    /// equal the call site's own subject family — see `OVERLOAD_MISRESOLVED`.
    matched_family: &'static str,
}

/// The fixture source of each guarded `e2e_*_with_stdlib` test, reused verbatim
/// so the census measures the SAME call sites the guards evaluate. That reuse is
/// a hand-maintained copy, so `census_fixture_sources_match_the_guards` below
/// enforces it against the guard files rather than trusting this comment.
///
/// One entry per intercepted `(name, arity)` pair — `is_combinator`'s ten
/// (crates/reify-expr/src/option_recovery.rs:119-133) plus `map_or`/3 and
/// `map_err`/2, which have their own gates at reify-expr/src/lib.rs:755 and 774.
const CENSUS: &[CensusRow] = &[
    CensusRow {
        pair: "unwrap_or/2",
        guard: "e2e_unwrap_or_some_5mm_with_stdlib",
        source: "structure S { let v = unwrap_or(some(5mm), 0mm) }",
        matched_family: "Option",
    },
    CensusRow {
        pair: "or_default/2",
        guard: "e2e_or_default_some_with_stdlib",
        source: "structure S { let v = or_default(some(5mm), 0mm) }",
        matched_family: "Option",
    },
    CensusRow {
        pair: "or_else/2",
        guard: "e2e_or_else_none_subject_with_stdlib",
        source: "structure S { param o : Option<Length> = none  let v = or_else(o, some(3mm)) }",
        matched_family: "Option",
    },
    CensusRow {
        pair: "is_some/1",
        guard: "e2e_is_some_none_with_stdlib",
        source: "structure S { let v = is_some(none) }",
        matched_family: "Option",
    },
    CensusRow {
        pair: "is_none/1",
        guard: "e2e_is_none_none_with_stdlib",
        source: "structure S { let v = is_none(none) }",
        matched_family: "Option",
    },
    CensusRow {
        pair: "get_or/3",
        guard: "e2e_get_or_absent_key_with_stdlib",
        source: r#"structure S { let v = get_or(map{"k" => 1mm}, "absent", 0mm) }"#,
        matched_family: "Map",
    },
    CensusRow {
        pair: "fallback/2",
        guard: "e2e_result_fallback_ok_with_stdlib",
        source: "structure S { let v = fallback(Ok { value: 5mm }, 0mm) }",
        // MEASURED mis-resolution: the subject is `Enum("Result")` but the
        // selected candidate is option_recovery.ri's `fallback<T>(Option<T>, T)`.
        matched_family: "Option",
    },
    CensusRow {
        pair: "is_ok/1",
        guard: "e2e_result_is_ok_err_with_stdlib",
        source: r#"structure S { let v = is_ok(Err { error: "e" }) }"#,
        matched_family: "Result",
    },
    CensusRow {
        pair: "is_err/1",
        guard: "e2e_result_is_err_err_with_stdlib",
        source: r#"structure S { let v = is_err(Err { error: "e" }) }"#,
        matched_family: "Result",
    },
    CensusRow {
        pair: "ok_or/2",
        guard: "e2e_ok_or_some_with_stdlib",
        source: r#"structure S { let v = ok_or(some(5mm), "e") }"#,
        matched_family: "Option",
    },
    CensusRow {
        pair: "map_or/3",
        guard: "e2e_map_or_some_with_stdlib",
        source: "structure S { let v = map_or(some(5mm), 0mm, |x: Length| x * 2) }",
        matched_family: "Option",
    },
    CensusRow {
        pair: "map_err/2",
        guard: "e2e_map_err_err_with_stdlib",
        source: "structure S { let v = map_err(Err { error: 3mm }, |e: Length| e * 2) }",
        matched_family: "Result",
    },
];

/// The census pairs whose selected candidate does NOT belong to the same
/// overload family as the call site's own subject — MEASURED, exhaustive.
///
/// Only `fallback/2` qualifies in the census, because it is the one row here
/// whose fixture has a `Result` subject AND a same-name/arity Option overload
/// ahead of it in table order. The two other guards in that position —
/// `e2e_result_unwrap_or_ok_with_stdlib` and `e2e_result_or_else_err_with_stdlib`
/// — are represented in the census by their OPTION-subject siblings, so the
/// census cannot observe them; the module-doc table's `†` rows record them.
const OVERLOAD_MISRESOLVED: &[&str] = &["fallback/2"];

/// Collapse a `Type` to the OVERLOAD FAMILY that decides which stdlib module's
/// candidate ought to win: `Option`, `Result`, `Map`, …
///
/// `Enum("Result")` — how an `Ok{..}`/`Err{..}` literal's `result_type` is
/// spelled at a call site — and `Applied{name:"Result", ..}` — how result.ri's
/// signatures spell it — are the SAME family here. That they are not `==` is
/// exactly why pass 1 of `find_matching_compiled_function` can never resolve a
/// Result subject, which is what makes the `fallback/2` mis-resolution possible.
fn overload_family(t: &reify_core::Type) -> String {
    use reify_core::Type;
    match t {
        Type::Option(_) => "Option".to_string(),
        Type::Map(_, _) => "Map".to_string(),
        Type::Enum(name) => name.clone(),
        Type::Applied { name, .. } => name.clone(),
        other => format!("{other:?}"),
    }
}

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

    for CensusRow {
        pair,
        guard,
        source,
        ..
    } in CENSUS
    {
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

/// WHICH overload the matcher selects, pinned per census row.
///
/// The census above proves only that SOME generic prelude candidate matches. For
/// `unwrap_or`, `or_else` and `fallback` — each declared in BOTH
/// stdlib/option_recovery.ri and stdlib/result.ri — "some candidate" is not
/// enough: the census would stay green while silently measuring a different
/// stdlib module's placeholder than the one its own doc names. That is precisely
/// the prove-more-than-you-measure failure #5593 exists to eliminate, so the
/// selected overload is pinned rather than left implicit.
///
/// MEASURED, and the reason `matched_family` is a per-row constant rather than
/// something derived from the fixture: `fallback/2`'s subject is `Enum("Result")`
/// yet the selected candidate is option_recovery.ri's
/// `fallback<T>(Option<T>, T)`. Pass 1 of `find_matching_compiled_function`
/// requires exact `Type` equality and `Enum("Result")` never equals
/// `Applied{name:"Result", args:[T,E]}`, so BOTH candidates miss it; pass 2's
/// wildcard then takes the first in table order, and `std.option_recovery` loads
/// before `std.result`. Pre-existing reify-expr resolution behaviour, out of
/// scope for #5593 — see the OVERLOAD NOTE in
/// crates/reify-expr/tests/result_combinator_eval_tests.rs, and task #5685,
/// which is fixing it. This test pins the CURRENT behaviour, so #5685 landing
/// turns it RED by design: that is the notification, and the assertion message
/// below says what to update.
///
/// The mis-resolution is BENIGN for the guards (each overload pair returns the
/// same positional argument, so the observable placeholder value is identical),
/// which is exactly why it needs a test: nothing else would go RED if the
/// resolution flipped. This test fails in BOTH directions — if the matcher
/// starts preferring result.ri, `matched_family` mismatches; if the
/// mis-resolution is fixed, `OVERLOAD_MISRESOLVED` no longer matches.
#[test]
fn census_pins_which_overload_the_matcher_selects() {
    let mut failures: Vec<String> = Vec::new();
    let mut observed_misresolved: Vec<&str> = Vec::new();

    for row in CENSUS {
        let module = reify_test_support::compile_source_with_stdlib(row.source);
        let expr = reify_test_support::get_let_expr(&module, "v");
        let (name, args) = as_call(expr);
        let prelude_backed = reify_test_support::prelude_backed_functions(&module);

        let Some(matched) =
            reify_expr::find_matching_compiled_function(&prelude_backed, name, args)
        else {
            // Already reported in full by the census test above; nothing to pin.
            failures.push(format!(
                "{} ({}): no match at all — see the competitor census failure",
                row.pair, row.guard
            ));
            continue;
        };

        let Some((_, param0)) = matched.params.first() else {
            failures.push(format!(
                "{} ({}): matched {} has zero params — every intercepted \
                 combinator takes a subject",
                row.pair, row.guard, matched.name
            ));
            continue;
        };
        let matched_family = overload_family(param0);
        if matched_family != row.matched_family {
            failures.push(format!(
                "{} ({}): matched candidate's params[0] family is {matched_family:?}, \
                 but the census pins {:?}. The resolver's overload choice moved — \
                 update the pin AND the `from` column of the module-doc table, \
                 which names the stdlib file whose body this row measures.",
                row.pair, row.guard, row.matched_family
            ));
        }

        // Does the selected overload actually belong to the subject's family?
        let subject_family = args
            .first()
            .map(|a| overload_family(&a.result_type))
            .unwrap_or_else(|| "<no args>".to_string());
        if subject_family != matched_family {
            observed_misresolved.push(row.pair);
        }
    }

    assert!(
        failures.is_empty(),
        "overload pinning found {} problem(s):\n  - {}",
        failures.len(),
        failures.join("\n  - ")
    );
    assert_eq!(
        observed_misresolved, OVERLOAD_MISRESOLVED,
        "the set of census pairs whose selected overload does NOT match their \
         subject's family changed. This is the pre-existing reify-expr \
         Option-before-Result resolution behaviour; if it was deliberately fixed, \
         update OVERLOAD_MISRESOLVED and the `†` footnote in this file's module \
         doc, and re-measure the guards' placeholder values"
    );
}

// ── census / guard sync-drift check ──────────────────────────────────────────

/// The guard test files the census copies its fixture sources from.
const GUARD_FILES: &[(&str, &str)] = &[
    (
        "option_recovery_eval_tests.rs",
        include_str!("option_recovery_eval_tests.rs"),
    ),
    (
        "result_combinator_eval_tests.rs",
        include_str!("result_combinator_eval_tests.rs"),
    ),
    (
        "result_fallback_eval_tests.rs",
        include_str!("result_fallback_eval_tests.rs"),
    ),
];

/// Sync-drift check: every `CENSUS.source` is still the literal fixture source of
/// the `CENSUS.guard` it claims to mirror.
///
/// `CENSUS` hard-codes a COPY of each guard's fixture. Nothing about a copy is
/// self-enforcing, so without this check editing a guard's fixture — a plausible
/// follow-up, since the coincidence-limited guards invite strengthening — would
/// leave the census silently measuring the retired call site while its doc still
/// claimed the sources are "reused verbatim". Same shape as this repo's existing
/// `sync_drift_check_all_combinators_recognized` /
/// `sync_drift_check_result_combinators_recognized`.
///
/// Deliberately a SOURCE-TEXT check rather than a shared constant: hoisting the
/// fixtures into a shared `const` would satisfy the coupling but would also move
/// each guard's fixture out of the guard, where a reader needs to see it. This
/// keeps the fixture inline in the guard and makes the copy loud when it drifts.
///
/// Guard files are pulled in with `include_str!`, so a renamed or deleted file is
/// a COMPILE error rather than a silently-skipped check.
#[test]
fn census_fixture_sources_match_the_guards() {
    let mut failures: Vec<String> = Vec::new();

    for row in CENSUS {
        let needle = format!("fn {}(", row.guard);
        let hits: Vec<&(&str, &str)> = GUARD_FILES
            .iter()
            .filter(|(_, src)| src.contains(&needle))
            .collect();

        let [(file, src)] = hits.as_slice() else {
            failures.push(format!(
                "{} : guard `{}` was found in {} of the {} guard files (expected \
                 exactly 1) — it was renamed, deleted or duplicated, so the census \
                 row no longer mirrors anything",
                row.pair,
                row.guard,
                hits.len(),
                GUARD_FILES.len()
            ));
            continue;
        };

        // Window = the guard's own body and nothing else, so a fixture belonging
        // to a NEIGHBOURING test (or to that test's doc comment) cannot satisfy
        // this row. Bounded by whichever comes first: the fn item's closing brace
        // at column 0, or the next `#[test]`.
        let start = src.find(&needle).expect("contains() just succeeded");
        let tail = &src[start..];
        let end = [
            tail.find("\n}\n").map(|off| start + off + 3),
            tail.find("\n#[test]").map(|off| start + off),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(src.len());
        let body = &src[start..end];

        if !body.contains(row.source) {
            failures.push(format!(
                "{} : CENSUS source has drifted from `{}` in {file}.\n      \
                 census holds: {}\n      \
                 but that literal no longer appears in the guard's body — the \
                 census is measuring a call site the guard no longer evaluates. \
                 Re-copy the fixture, then RE-MEASURE `matched_family` for this \
                 row and the module-doc placeholder table.",
                row.pair, row.guard, row.source
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "census/guard fixture drift — {} row(s):\n  - {}",
        failures.len(),
        failures.join("\n  - ")
    );
}
