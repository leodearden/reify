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
//!   e2e_result_or_else_err_with_stdlib  †  Err{error:"e"}         `{ r }`
//!   e2e_map_err_err_with_stdlib            Err{error:3mm}         `{ r }`
//!   e2e_ok_or_some_with_stdlib             String("e")            `{ err }`
//!   e2e_ok_or_none_with_stdlib             String("e")            `{ err }`
//!   e2e_get_or_absent_key_with_stdlib      0mm                    `{ dflt }`  <-- PASSES
//!
//! † THE MATCHED CANDIDATE IS result.ri's RESULT OVERLOAD. `unwrap_or`,
//! `or_else` and `fallback` are each declared in BOTH stdlib files, and these
//! three fixtures' `arg[0].result_type` is `Enum("Result")` — the erased form
//! variant construction produces, which never exact-equals result.ri's
//! `Applied{name:"Result", args:[T,E]}`. Before #5685 that made pass 1 miss
//! both candidates and left the wildcard pass to take the first in table order
//! — option_recovery.ri's `Option<T>` overload, since `std.option_recovery`
//! loads before `std.result`. #5685 gave the eval matcher the constructor-head
//! narrowing tier the compile side already had, so the erased subject now
//! selects result.ri's signature. Full statement: the OVERLOAD NOTE in
//! crates/reify-expr/tests/result_combinator_eval_tests.rs. Pinned per row
//! below by `census_pins_which_overload_the_matcher_selects` so a future
//! resolver change flips a test RED instead of silently rewriting what this
//! table means.
//!
//! The observed-value column above is unmoved by that reselection, and the
//! `from` column only partly: `unwrap_or` and `fallback` ship the SAME
//! `{ dflt }` body in both stdlib files, and `or_else` returns its subject
//! either way — result.ri's `{ r }` where option_recovery.ri has `{ o }`. Both
//! members of each pair return the same positional argument, which is why the
//! reselection is value-invisible and why the census below pins the selected
//! candidate directly rather than inferring it from an evaluated value.
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
/// Dispatch works because the broadest tier of `find_matching_compiled_function`
/// — the wildcard pass, gated on `!f.type_params.is_empty()` — treats any param
/// carrying a type/dim param as a wildcard, where the exact-equality tier ahead
/// of it cannot bind anything generic. That is what lets the generic
/// `through<T>(x: T)` signature bind a concrete `Scalar[m]` argument, and it is
/// the mechanism this whole harness depends on. (The full tier list and their
/// ordering live on `find_matching_compiled_function`'s own doc comment; this
/// harness only depends on the wildcard tier existing.)
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
    /// The `e2e_*_with_stdlib` guard this row mirrors, used together with `pair`
    /// as the failure label so a census failure names the guard to go read.
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
/// so the census measures the SAME call sites the guards evaluate.
///
/// The `source` is what the census tests below actually compile, via
/// `compile_source_with_stdlib` — so a row that drifts from its guard still
/// measures a real, self-consistent call site, and `matched_family` stays a
/// measurement of THAT source. The `guard` name is a provenance pointer for the
/// reader, not a mechanically enforced invariant.
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
        // MEASURED: the `Enum("Result")` subject selects result.ri's
        // `fallback<T, E>(Result<T, E>, T)`. Was "Option" until #5685 gave the
        // eval matcher the constructor-head narrowing tier.
        matched_family: "Result",
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
/// EMPTY since #5685. `fallback/2` was the sole entry: it is the one census row
/// whose fixture has a `Result` subject AND a same-name/arity Option overload
/// ahead of it in table order, so before the head-narrowing tier the wildcard
/// pass took the Option candidate on table order alone. It now selects
/// result.ri's, so every census row's selected overload agrees with its
/// subject's family.
///
/// Kept (rather than deleted along with its last entry) as a standing
/// two-directional pin: a future resolver change that reintroduces a
/// family-crossing selection anywhere in the census fails here by name.
const OVERLOAD_MISRESOLVED: &[&str] = &[];

/// Collapse a `Type` to the OVERLOAD FAMILY that decides which stdlib module's
/// candidate ought to win: `Option`, `Result`, `Map`, …
///
/// `Enum("Result")` — how an `Ok{..}`/`Err{..}` literal's `result_type` is
/// spelled at a call site — and `Applied{name:"Result", ..}` — how result.ri's
/// signatures spell it — are the SAME family here. Collapsing them is what lets
/// this test compare a call site's subject against the selected candidate's
/// `params[0]` at all: they are never `==`, so the exact-equality tier of
/// `find_matching_compiled_function` can never resolve a Result subject, and
/// which candidate wins is decided further down the tier list.
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
/// `matched_family` stays a per-row MEASURED constant rather than something
/// derived from the fixture, because the derivation would beg the question this
/// test exists to answer. `fallback/2` is the row that proves it: its subject is
/// `Enum("Result")`, and which of the two same-name/arity candidates that erased
/// subject selects is decided entirely inside the resolver's tier ordering — it
/// selected option_recovery.ri's `fallback<T>(Option<T>, T)` until #5685 added
/// the constructor-head narrowing tier, and result.ri's
/// `fallback<T, E>(Result<T, E>, T)` since. A derived expectation would have
/// silently tracked that flip; a pinned one reports it, which is what happened
/// when #5685 landed and this test went RED by design.
///
/// Pinning is the ONLY thing that reports such a flip, because it is BENIGN for
/// the guards: each overload pair returns the same positional argument, so the
/// observable placeholder value is identical either way and no `e2e_*` guard
/// moves. See the OVERLOAD NOTE in
/// crates/reify-expr/tests/result_combinator_eval_tests.rs.
///
/// The test fails in BOTH directions: `matched_family` mismatches if any row's
/// selected overload moves, and `OVERLOAD_MISRESOLVED` mismatches if the set of
/// rows whose selection crosses their subject's family changes — in either
/// direction, so a regression back to family-crossing selection is caught too.
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
         subject's family changed. Empty since #5685 gave the eval matcher the \
         constructor-head narrowing tier; a NON-empty left side means the \
         resolver regressed to picking a candidate from the wrong stdlib module \
         on table order alone. If the change was deliberate, update \
         OVERLOAD_MISRESOLVED and the `†` footnote in this file's module doc, \
         and re-measure the guards' placeholder values"
    );
}
