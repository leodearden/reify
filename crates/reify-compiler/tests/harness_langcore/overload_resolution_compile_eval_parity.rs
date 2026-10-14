//! Compile-vs-eval overload-resolution parity test (task #5992).
//!
//! Ties together the two independently-authored implementations of ONE
//! predicate — "which user function does this call select?" — so they cannot
//! silently drift:
//! - **compile side**: `resolve_function_overload`
//!   (`crates/reify-compiler/src/type_compat.rs:1162`) — a `matches` tier of
//!   per-param disjuncts, narrowed by two tie-break tiers (`exact_matches`,
//!   then `head_matches` / `heads_unifiable`), classified into
//!   `Resolved` / `NoMatch` / `Ambiguous`.
//! - **eval side**: `find_matching_compiled_function`
//!   (`crates/reify-expr/src/lib.rs:1809`) — the same three tiers in the same
//!   priority order (exact, then head, then wildcard), resolved first-match-
//!   wins within each and yielding `Option<&CompiledFunction>`. The head tier
//!   is the newest of the three (#5685); before it landed, this file's
//!   exemption ledger carried a `"head-tier"` row for its absence.
//!
//! ## Contract
//!
//! The two resolvers must agree on both axes this file asserts:
//! 1. **accept/reject** — compile `Resolved`|`Ambiguous` iff eval `Some`.
//!    Driven over SINGLE-candidate overload sets so the tie-break tiers
//!    cannot confound the per-param predicate.
//! 2. **selection identity** — when both accept a MULTI-candidate set, they
//!    must select the SAME overload. This axis is not redundant with the
//!    first: the worst failure mode is that both sides "accept" and pick
//!    DIFFERENT functions, which axis 1 is structurally blind to.
//!
//! Divergences are allowed ONLY via the commented exemption ledger; an
//! unledgered divergence panics, and — the self-cleaning half — a ledgered
//! row that has started AGREEING also panics, so a ledger row cannot outlive
//! the drift it documents.
//!
//! ## Why this matters (the INV-SF-1 miss path)
//!
//! This is the **INV-COMP-2 shape**: two subsystems independently deriving a
//! fact that must agree. (The shape, not the row — INV-COMP-2's row in
//! `docs/invariants.md` is already `enforced(test)` for a different subject.)
//! When the two resolvers drift in the accept direction, the failure is
//! SILENT rather than diagnosed: the compiler resolves the call fine, then
//! `eval_user_function_call` (`crates/reify-expr/src/lib.rs:1943`) fails to
//! find the same function and returns a bare `Value::Undef` carrying NO
//! `UndefCause` (`:1963`) — an undiagnosed hole, not an error message.
//!
//! This has already happened once for real: esc-4093-152, where the
//! compile-side trait-object wildcard applied to every candidate but the
//! eval-side one did not, so a non-generic
//! `solve_elastic_static(loads: List<Load>, ..)` resolved at compile time and
//! then silently failed its `@optimized` ComputeNode dispatch at eval.
//!
//! ## Access to compiler internals
//!
//! Both `resolve_function_overload` and `OverloadResolution` are
//! `pub(crate)`, unreachable from an integration test (a separate external
//! crate). This test reaches the resolver through the `test-support`-gated
//! `reify_compiler::__resolve_function_overload_for_parity_test` shim, which
//! mirrors `__infer_mul_div_result_for_parity_test` (`lib.rs:180`); the
//! self-pull dev-dependency that activates `feature = "test-support"` for
//! the `harness_langcore` integration-test target this module belongs to
//! already exists (`Cargo.toml`), so no manifest change was needed. The
//! eval-side `find_matching_compiled_function` is already `pub`.
//!
//! Modelled on `crates/reify-compiler/tests/mul_div_static_runtime_parity.rs`
//! (INV-COMP-3 β3), whose table + shared-engine + exemption-ledger shape this
//! reuses by pattern (sibling integration-test crates cannot import each
//! other's `tests/` modules).
//!
//! ## Deferred alternative
//!
//! Parity-by-test is the *detection* fix, not the *durable* one. The durable
//! fix is algorithm dedup: a SINGLE shared resolver both crates call, so
//! there is no second implementation to drift. That needs a crate seam
//! visible to both `reify-compiler` and `reify-expr` (the predicate helpers
//! `type_carries_trait_object` / `type_carries_type_param` /
//! `type_carries_dim_param` are today independently duplicated across the two
//! crates as well — `type_compat.rs:417/459/647` vs
//! `reify-expr/src/lib.rs:1332/1372/1465`). Introducing that seam is a
//! scope-level decision rather than a bug fix, and is a candidate for the
//! Wave-3 scope discussion in `docs/prds/v0_6/builtin-signature-registry.md`;
//! task #5689 is the filed hoist-to-reify-core follow-up, and eval's own
//! resolver says the same at `reify-expr/src/lib.rs:1887`.
//! Until such a seam exists, driving BOTH resolvers over one table is what
//! catches predicate drift and resolver drift alike, with no new machinery.

#![cfg(feature = "test-support")]

use reify_compiler::ParityOverloadVerdict;
use reify_core::{ContentHash, Type};
use reify_expr::find_matching_compiled_function;
use reify_ir::{CompiledExpr, CompiledFnBody, CompiledFunction, TypeParam, Value};

// ── Fixture builders ────────────────────────────────────────────────────────

/// Minimal stub body returning `Real(0.0)`.
///
/// Neither resolver ever inspects the body — both dispatch purely on
/// `name` / `params` / `type_params` — so a constant stub is sufficient.
fn stub_body() -> CompiledFnBody {
    CompiledFnBody {
        let_bindings: vec![],
        result_expr: CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar()),
    }
}

/// Build a minimal `CompiledFunction`.
///
/// Mirrors `type_compat.rs:2340`'s in-crate `make_fn` plus a `type_params`
/// argument, because genericity is load-bearing on BOTH sides here: the
/// type-param and dim-param wildcards are gated on `!type_params.is_empty()`,
/// while the trait-object wildcard is not. Only EMPTINESS of `type_params` is
/// consulted by either resolver, so the `bounds` / `default` fields of each
/// `reify_ir::TypeParam` are left empty — they cannot affect a row's outcome.
fn make_fn(name: &str, params: Vec<(&str, Type)>, type_params: Vec<&str>) -> CompiledFunction {
    let params: Vec<(String, Type)> = params
        .into_iter()
        .map(|(n, t)| (n.to_string(), t))
        .collect();
    let param_defaults = CompiledFunction::no_defaults_for(&params);
    CompiledFunction {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        params,
        param_defaults,
        return_type: Type::dimensionless_scalar(),
        body: stub_body(),
        content_hash: ContentHash::of_str(name),
        annotations: vec![],
        optimized_target: None,
        type_params: type_params
            .into_iter()
            .map(|name| TypeParam {
                name: name.to_string(),
                bounds: vec![],
                default: None,
            })
            .collect(),
    }
}

/// Build a call argument carrying `ty` as its `result_type`.
///
/// The eval-side resolver matches on `CompiledExpr::result_type` ONLY, never
/// on the payload `Value`, so a constant `Int(0)` payload is valid for every
/// row regardless of the declared type. This is what makes the eval side
/// directly comparable to the compile side's bare `&[Type]`.
fn arg(ty: Type) -> CompiledExpr {
    CompiledExpr::literal(Value::Int(0), ty)
}

/// Position of `f` within `fns`, by identity — the eval side returns a
/// `&CompiledFunction` while the compile-side shim returns an index, so this
/// maps eval's answer onto the shim's coordinate system.
fn index_of(fns: &[CompiledFunction], f: &CompiledFunction) -> usize {
    fns.iter()
        .position(|c| std::ptr::eq(c, f))
        .expect("eval-selected candidate must be an element of `fns`")
}

// ── Smoke: the simplest possible row agrees on both axes ────────────────────

/// The degenerate case — one non-generic candidate `f(x: Int)` called with
/// `Int` — must resolve identically through both resolvers. If this fails,
/// the harness itself is miswired and no other row in this file means
/// anything.
#[test]
fn smoke_exact_concrete_overload_agrees() {
    let fns = vec![make_fn("f", vec![("x", Type::Int)], vec![])];

    let verdict =
        reify_compiler::__resolve_function_overload_for_parity_test("f", &[Type::Int], &fns);
    assert_eq!(
        verdict,
        ParityOverloadVerdict::Resolved { index: 0 },
        "compile side must resolve `f(Int)` called with `Int` to the sole candidate"
    );

    let selected = find_matching_compiled_function(&fns, "f", &[arg(Type::Int)])
        .expect("eval side must resolve `f(Int)` called with `Int`");
    assert_eq!(
        index_of(&fns, selected),
        0,
        "eval side must select the same sole candidate"
    );
}

// ── Ledger machinery + shared assertion engine ──────────────────────────────

/// Per-row expectation. `Diverge` names the responsible disjunct/tier so the
/// exemption ledger above the tables can be read alongside the row.
#[derive(Debug, Clone)]
enum Expect {
    /// The two resolvers must reach the same answer.
    Agree,
    /// A documented, ledgered divergence — see the exemption ledger.
    Diverge { ledger: &'static str },
}

/// One accept/reject row: a SINGLE-candidate overload set.
///
/// Single-candidate is deliberate. `resolve_function_overload`'s two
/// narrowing tiers (`exact_matches`, `head_matches`) can only ever SELECT
/// among candidates that already passed the `matches` tier, and an empty
/// narrowing tier falls through to `matches`. So with exactly one candidate,
/// compile-accept ⟺ that candidate satisfies the per-param predicate — the
/// tie-break tiers cannot confound the axis. Selection identity, which the
/// tiers *do* govern, is asserted separately over multi-candidate sets.
struct Row {
    label: &'static str,
    params: Vec<(&'static str, Type)>,
    type_params: Vec<&'static str>,
    call_name: &'static str,
    arg_types: Vec<Type>,
    expect: Expect,
}

/// Non-panicking core of the accept/reject engine: `Ok(())` when the row
/// behaves as its `expect` declares, `Err(message)` otherwise.
///
/// Split out from [`assert_accept_reject_parity`] so the table driver can
/// report EVERY offending row in one run rather than aborting at the first —
/// a table this size is much easier to diagnose whole.
fn classify_accept_reject_parity(row: &Row) -> Result<(), String> {
    let fns = vec![make_fn("f", row.params.clone(), row.type_params.clone())];

    let verdict = reify_compiler::__resolve_function_overload_for_parity_test(
        row.call_name,
        &row.arg_types,
        &fns,
    );
    let args: Vec<CompiledExpr> = row.arg_types.iter().cloned().map(arg).collect();
    let eval_result = find_matching_compiled_function(&fns, row.call_name, &args);

    // Compile-accept is `Resolved | Ambiguous`: both mean "the call selected
    // at least one candidate". `NoMatch` and `NoUserFunctions` are rejects.
    let compile_accepts = matches!(
        verdict,
        ParityOverloadVerdict::Resolved { .. } | ParityOverloadVerdict::Ambiguous { .. }
    );
    let eval_accepts = eval_result.is_some();

    match (compile_accepts == eval_accepts, &row.expect) {
        (true, Expect::Agree) => Ok(()),
        (false, Expect::Diverge { .. }) => Ok(()),
        (false, Expect::Agree) => Err(format!(
            "{}: UNLEDGERED accept/reject divergence (compile={:?} accepts={}, eval accepts={}) \
             — add a commented EXEMPTION LEDGER row naming the responsible disjunct, \
             or fix the mismatch",
            row.label, verdict, compile_accepts, eval_accepts
        )),
        (true, Expect::Diverge { ledger }) => Err(format!(
            "{}: STALE ledger row '{}' — the two resolvers now AGREE \
             (compile={:?} accepts={}, eval accepts={}); remove the ledger row and \
             reclassify this row to Expect::Agree",
            row.label, ledger, verdict, compile_accepts, eval_accepts
        )),
    }
}

/// Panicking single-row form of the accept/reject engine.
///
/// # TEETH
///
/// Both engines panic rather than merely reporting, and the ledger is
/// load-bearing in BOTH directions. Proven by temporary mutation, not by a
/// committed failing assertion — both recipes below were run manually while
/// authoring this file, each confirmed to panic, then reverted:
///
/// (a) UNLEDGERED fires: flip a ledgered row (e.g. the `"D4"` row
///     `D4 bare: f(Int) <- TypeParam(T)`, or the `"ambiguity-model"`
///     selection row) to `Expect::Agree` and re-run. Its table driver goes
///     RED with "UNLEDGERED ... divergence", proving a NEW drift cannot slip
///     in undocumented.
///
/// (b) STALE fires: flip a clean row (e.g. `concrete exact: f(Int) <- Int`,
///     or the `exact-wins (concrete first)` selection row) to
///     `Expect::Diverge { ledger: "bogus" }` and re-run. Its driver goes RED
///     with "STALE ledger row 'bogus' — the two resolvers now AGREE",
///     proving a ledger entry cannot outlive the drift it documents.
///
/// (b) is what makes this file self-cleaning: when #5686 lands the two `"D4"`
/// rows start agreeing and go RED as STALE, so the ledger entry is removed by
/// the test failing, not by anyone remembering.
///
/// That is not a prediction — it has already happened once here. #5685 landed
/// the eval-side head-narrowing tier while this file was in flight; the
/// `"head-tier"` row started agreeing, `multi_candidate_selection_parity` went
/// RED as STALE, and the entry was retired (see the RETIRED note in the
/// exemption ledger). Recipe (b) is the mechanism that caught it.
fn assert_accept_reject_parity(row: &Row) {
    if let Err(msg) = classify_accept_reject_parity(row) {
        panic!("{msg}");
    }
}

// ── Exemption ledger ────────────────────────────────────────────────────────
//
// Documented, intentional compile-vs-eval divergences. Every row a table
// declares `Expect::Diverge { ledger: "<key>" }` must have an entry here
// naming the responsible disjunct/tier; an undocumented divergence panics
// UNLEDGERED, and an entry whose rows have started agreeing panics STALE, so
// this block cannot drift out of sync with the tables in either direction.
//
// ── RETIRED: "head-tier" — the ledger's self-cleaning half, demonstrated ────
//
// This block once carried a second temporary entry. Compile-side
// `resolve_function_overload` narrows `matches` twice — by `exact_matches`,
// then by `head_matches` / `heads_unifiable` — which discriminates two
// GENERIC overloads differing only by constructor head; eval-side
// `find_matching_compiled_function` had an exact pass and a wildcard pass and
// nothing else, so `Option<T>` vs `Result<T,E>` selected DIFFERENT functions
// on the two sides. Task #5685 (now landed) added the missing eval-side
// head-narrowing tier, and the entry was retired.
//
// It is recorded here rather than deleted outright because of HOW it was
// retired: not by anyone remembering, but by the STALE arm. #5685 landed
// while this file was in flight, the head-tier row started agreeing, and
// `multi_candidate_selection_parity` plus its anti-vacuity companion went RED
// with a message naming the exact remedy. That is the self-cleaning property
// working end-to-end on a real landing, and it is the reason the four-arm
// engine is worth its extra arm. The row itself is KEPT (reclassified to
// `Expect::Agree`) and its companion repurposed into a positive parity pin,
// so retiring the entry cost no coverage.
//
// ── "ambiguity-model" — architecturally PERMANENT, not a bug ────────────────
//
// Compile-side resolution is a three-way classification
// (`Resolved` / `NoMatch` / `Ambiguous`) and reports `Ambiguous` as a
// call-site DIAGNOSTIC. Eval-side resolution is an `Option` lookup with
// first-match-wins semantics and no ambiguity concept at all — there is no
// `Option` inhabitant meaning "two candidates matched".
//
// This is a deliberate asymmetry, not drift: in practice the compiler
// rejects an ambiguous call with a diagnostic before eval ever sees it, so
// eval's arbitrary pick is unreachable in a well-formed program. It must NOT
// be "fixed" by teaching eval about ambiguity — that would add a runtime
// concept whose only job is to re-detect what the compiler already rejected.
//
// The class is reached by ANY wildcard admitting two or more survivors that
// no narrower tier separates — a trait-object pair, or a bare user type-param
// arg over concrete overloads (the selection row "ambiguity: bare user
// type-param arg over two concrete overloads").
//
// Deliberately carries NO task cite: it is permanent, so there is no owner to
// cite and no fix that would ever close it.

// ── Single-candidate accept/reject table ────────────────────────────────────

/// Every row below is a ONE-candidate overload set. Types are built fresh per
/// row rather than shared, so a row can never be perturbed by another's edits.
#[rustfmt::skip]
fn single_candidate_rows() -> Vec<Row> {
    let trait_obj = |n: &str| Type::TraitObject(n.to_string());
    let tp        = |n: &str| Type::TypeParam(n.to_string());
    let sp        = |n: &str| Type::ScalarParam(n.to_string());
    let result_of = |args: Vec<Type>| Type::Applied { name: "Result".to_string(), args };
    let ph        = |n: &str| Type::unbound_placeholder(n);
    let list_of   = |t: Type| Type::List(Box::new(t));

    vec![
        // ── Concrete exact match / plain rejects ─────────────────────────────
        Row { label: "concrete exact: f(Int) <- Int",             params: vec![("x", Type::Int)],                    type_params: vec![],    call_name: "f", arg_types: vec![Type::Int],                                    expect: Expect::Agree },
        Row { label: "concrete mismatch: f(Real) <- Int",         params: vec![("x", Type::dimensionless_scalar())], type_params: vec![],    call_name: "f", arg_types: vec![Type::Int],                                    expect: Expect::Agree },
        Row { label: "arity mismatch: f(Int, Int) <- (Int)",      params: vec![("x", Type::Int), ("y", Type::Int)],  type_params: vec![],    call_name: "f", arg_types: vec![Type::Int],                                    expect: Expect::Agree },
        Row { label: "name mismatch: f(Int) called as g",         params: vec![("x", Type::Int)],                    type_params: vec![],    call_name: "g", arg_types: vec![Type::Int],                                    expect: Expect::Agree },

        // ── Trait-object wildcard: UNGATED on genericity on both sides ───────
        // (the esc-4093-152 shape — pinned here as a positive, non-generic.)
        Row { label: "trait-obj param <- StructureRef arg",       params: vec![("j", trait_obj("DrivingJoint"))],    type_params: vec![],    call_name: "f", arg_types: vec![Type::StructureRef("X".to_string())],          expect: Expect::Agree },
        Row { label: "trait-obj param <- other trait-obj arg",    params: vec![("j", trait_obj("DrivingJoint"))],    type_params: vec![],    call_name: "f", arg_types: vec![trait_obj("Other")],                          expect: Expect::Agree },
        Row { label: "List<trait-obj> param <- List<StructRef>",  params: vec![("l", Type::List(Box::new(trait_obj("Load"))))], type_params: vec![], call_name: "f", arg_types: vec![Type::List(Box::new(Type::StructureRef("X".to_string())))], expect: Expect::Agree },

        // ── Type-param wildcard: GATED on genericity on both sides ───────────
        Row { label: "generic f<T>(T) <- Int",                    params: vec![("x", tp("T"))],                      type_params: vec!["T"], call_name: "f", arg_types: vec![Type::Int],                                    expect: Expect::Agree },
        Row { label: "GATE: non-generic f(T) <- Int rejects",     params: vec![("x", tp("T"))],                      type_params: vec![],    call_name: "f", arg_types: vec![Type::Int],                                    expect: Expect::Agree },

        // ── Dim-param wildcard: GATED on genericity on both sides ────────────
        Row { label: "generic f<D>(Scalar<D>) <- Length",         params: vec![("x", sp("D"))],                      type_params: vec!["D"], call_name: "f", arg_types: vec![Type::length()],                                expect: Expect::Agree },
        Row { label: "GATE: non-generic f(Scalar<D>) <- Length",  params: vec![("x", sp("D"))],                      type_params: vec![],    call_name: "f", arg_types: vec![Type::length()],                                expect: Expect::Agree },

        // ── Wrapper / enum shapes carrying a nested type param ───────────────
        Row { label: "generic f<T>(Option<T>) <- Option<Int>",    params: vec![("o", Type::Option(Box::new(tp("T"))))], type_params: vec!["T"], call_name: "f", arg_types: vec![Type::Option(Box::new(Type::Int))],        expect: Expect::Agree },
        Row { label: "generic f<T,E>(Result<T,E>) <- Result<Int,String>", params: vec![("r", result_of(vec![tp("T"), tp("E")]))], type_params: vec!["T", "E"], call_name: "f", arg_types: vec![result_of(vec![Type::Int, Type::String])], expect: Expect::Agree },
        // Erased subject: variant construction type-erases `Ok{..}` to `Enum("Result")`.
        Row { label: "generic f<T,E>(Result<T,E>) <- Enum(Result)", params: vec![("r", result_of(vec![tp("T"), tp("E")]))], type_params: vec!["T", "E"], call_name: "f", arg_types: vec![Type::Enum("Result".to_string())], expect: Expect::Agree },

        // ── Type-param-carrying ARG: a bare user `TypeParam` arg wildcards; a
        //    headed one admits only where heads unify (normative text:
        //    `reify_core::overload::slot_matches_wildcard_tier`). ─────────────
        Row { label: "D4 bare user type-param arg: f(Int) <- T accepts",                  params: vec![("x", Type::Int)], type_params: vec![], call_name: "f", arg_types: vec![tp("T")],                           expect: Expect::Agree },
        Row { label: "C-4 headed type-param arg, head mismatch: f(Int) <- Result<T,E> rejects", params: vec![("x", Type::Int)], type_params: vec![], call_name: "f", arg_types: vec![result_of(vec![tp("T"), tp("E")])], expect: Expect::Agree },

        // ── R1 placeholder arg (an undetermined generic-enum type argument):
        //    no arg-side admission; only a param-side wildcard accepts it. ────
        Row { label: "R1 bare placeholder arg vs concrete: f(Int) <- ?T rejects",            params: vec![("x", Type::Int)],          type_params: vec![],    call_name: "f", arg_types: vec![ph("T")], expect: Expect::Agree },
        Row { label: "R1 bare placeholder arg vs generic: f<T>(T) <- ?T accepts",            params: vec![("x", tp("T"))],            type_params: vec!["T"], call_name: "f", arg_types: vec![ph("T")], expect: Expect::Agree },
        Row { label: "R1 bare placeholder arg vs trait-obj: f(TraitObject A) <- ?T accepts", params: vec![("x", trait_obj("A"))],     type_params: vec![],    call_name: "f", arg_types: vec![ph("T")], expect: Expect::Agree },

        // ── C-4 headed args: admitted only where heads unify. The candidate is
        //    non-generic, so tier 2 is EMPTY and acceptance requires the
        //    empty-tier fall-through to tier 3 on both sides. ──────────────────
        Row { label: "C-4 placeholder slot, same head: f(Result<Length,String>) <- Result<Length,?E> accepts", params: vec![("r", result_of(vec![Type::length(), Type::String]))], type_params: vec![], call_name: "f", arg_types: vec![result_of(vec![Type::length(), ph("E")])], expect: Expect::Agree },
        Row { label: "C-4 headed user type-param arg, inner mismatch: f(List<Int>) <- List<T> rejects",         params: vec![("l", list_of(Type::Int))],                             type_params: vec![], call_name: "f", arg_types: vec![list_of(tp("T"))],                         expect: Expect::Agree },

        // ── Tier 2 is a FILTER over tier 3, never a standalone pass:
        //    heads_unifiable's erased-subject arm admits this pair at tier 2,
        //    tier 3 rejects it, so a resolver running tier 2 standalone would
        //    ACCEPT (counterexample source: reify_core::overload's
        //    `slot_matches_head_tier_is_not_a_subset_of_the_wildcard_tier`). ──
        Row { label: "tier 2 is a filter: generic f<T>(Result<Int,String>) <- Enum(Result) rejects", params: vec![("r", result_of(vec![Type::Int, Type::String]))], type_params: vec!["T"], call_name: "f", arg_types: vec![Type::Enum("Result".to_string())], expect: Expect::Agree },
    ]
}

/// Axis 1 — accept/reject parity over every single-candidate row.
#[test]
fn single_candidate_accept_reject_parity() {
    let rows = single_candidate_rows();
    let failures: Vec<String> = rows
        .iter()
        .filter_map(|r| classify_accept_reject_parity(r).err())
        .collect();

    assert!(
        failures.is_empty(),
        "compile-vs-eval accept/reject parity failed for {}/{} row(s):\n  - {}",
        failures.len(),
        rows.len(),
        failures.join("\n  - ")
    );
}

/// Positive two-sided pin for type-param-carrying ARGS against a concrete
/// param — the shape that, until #5686, compile accepted and eval rejected.
///
/// Both sides now apply one tier-3 predicate,
/// `reify_core::overload::slot_matches_wildcard_tier`, whose arg side
/// distinguishes two cases, asserted first-hand on each side:
/// - a BARE user type-param arg (`T`, task-4232 γ) wildcards, so a generic fn
///   body may pass a `T`-typed value to a concrete-param callee;
/// - a HEADED arg carrying a type param (`Result<T,E>`) admits only where the
///   heads unify (PRD C-4, #8014), so it does not reach an `Int` param.
#[test]
fn type_param_carrying_args_resolve_identically_on_both_sides() {
    let fns = vec![make_fn("f", vec![("x", Type::Int)], vec![])];
    let bare = Type::TypeParam("T".to_string());
    let headed = Type::Applied {
        name: "Result".to_string(),
        args: vec![
            Type::TypeParam("T".to_string()),
            Type::TypeParam("E".to_string()),
        ],
    };

    assert_eq!(
        reify_compiler::__resolve_function_overload_for_parity_test(
            "f",
            std::slice::from_ref(&bare),
            &fns
        ),
        ParityOverloadVerdict::Resolved { index: 0 },
        "bare type-param arg: compile side must RESOLVE `f(x: Int)` called with `T` — \
         if not, the shared tier-3 arg side, \
         reify_core::overload::slot_matches_wildcard_tier, has regressed or \
         resolve_function_overload stopped calling it"
    );
    let selected = find_matching_compiled_function(&fns, "f", &[arg(bare.clone())]).expect(
        "bare type-param arg: eval returns None again — the shared tier-3 arg side, \
         reify_core::overload::slot_matches_wildcard_tier, has regressed or \
         find_matching_compiled_function stopped calling it",
    );
    assert_eq!(index_of(&fns, selected), 0);

    assert_eq!(
        reify_compiler::__resolve_function_overload_for_parity_test(
            "f",
            std::slice::from_ref(&headed),
            &fns
        ),
        ParityOverloadVerdict::NoMatch,
        "headed type-param arg: compile side must REJECT `f(x: Int)` called with \
         `Result<T,E>` — if it resolves, the C-4 heads-unify gate on the arg side \
         of slot_matches_wildcard_tier (#8014) has regressed"
    );
    assert!(
        find_matching_compiled_function(&fns, "f", &[arg(headed.clone())]).is_none(),
        "headed type-param arg: eval side must REJECT `f(x: Int)` called with \
         `Result<T,E>` — if it resolves, the C-4 heads-unify gate on the arg side \
         of slot_matches_wildcard_tier (#8014) has regressed"
    );

    for (label, arg_ty) in [
        ("bare type-param arg (companion)", bare),
        ("headed type-param arg (companion)", headed),
    ] {
        assert_accept_reject_parity(&Row {
            label,
            params: vec![("x", Type::Int)],
            type_params: vec![],
            call_name: "f",
            arg_types: vec![arg_ty],
            expect: Expect::Agree,
        });
    }
}

// ── Axis 2: selection identity over MULTI-candidate overload sets ───────────

/// One selection-identity row: a multi-candidate overload set where both
/// resolvers are expected to accept, so the question is *which* candidate
/// each one picks.
///
/// This axis is not redundant with axis 1. Accept/reject is structurally
/// blind to the worst failure mode there is here — both sides accept, but
/// select DIFFERENT overloads, so eval silently runs the wrong function
/// body. Declaration ORDER is load-bearing in several rows below, because
/// eval's passes are first-match-wins while compile's are set-narrowing.
struct SelectionRow {
    label: &'static str,
    fns: Vec<CompiledFunction>,
    call_name: &'static str,
    arg_types: Vec<Type>,
    expect: Expect,
}

/// Non-panicking core of the selection-identity engine.
///
/// "Selection" is the single candidate a side committed to: compile's
/// `Resolved { index }`, or eval's `Some(f)` mapped back to its position.
/// `NoMatch` / `NoUserFunctions` / `Ambiguous` all mean compile committed to
/// no single candidate, which agrees with eval only when eval also selected
/// nothing. Reuses axis 1's four-arm `Expect` classification verbatim, so
/// both axes share one ledger discipline.
fn classify_selection_parity(row: &SelectionRow) -> Result<(), String> {
    let verdict = reify_compiler::__resolve_function_overload_for_parity_test(
        row.call_name,
        &row.arg_types,
        &row.fns,
    );
    let args: Vec<CompiledExpr> = row.arg_types.iter().cloned().map(arg).collect();
    let eval_index = find_matching_compiled_function(&row.fns, row.call_name, &args)
        .map(|f| index_of(&row.fns, f));

    let compile_index = match verdict {
        ParityOverloadVerdict::Resolved { index } => Some(index),
        // No single committed candidate on the compile side.
        ParityOverloadVerdict::NoUserFunctions
        | ParityOverloadVerdict::NoMatch
        | ParityOverloadVerdict::Ambiguous { .. } => None,
    };

    match (compile_index == eval_index, &row.expect) {
        (true, Expect::Agree) => Ok(()),
        (false, Expect::Diverge { .. }) => Ok(()),
        (false, Expect::Agree) => Err(format!(
            "{}: UNLEDGERED selection divergence (compile={:?} -> {:?}, eval -> {:?}) \
             — both sides may even 'accept', but they picked DIFFERENT overloads; \
             add a commented EXEMPTION LEDGER row naming the responsible tier, \
             or fix the mismatch",
            row.label, verdict, compile_index, eval_index
        )),
        (true, Expect::Diverge { ledger }) => Err(format!(
            "{}: STALE ledger row '{}' — the two resolvers now AGREE \
             (compile={:?} -> {:?}, eval -> {:?}); remove the ledger row and \
             reclassify this row to Expect::Agree",
            row.label, ledger, verdict, compile_index, eval_index
        )),
    }
}

/// Panicking single-row form of the selection-identity engine.
///
/// # TEETH
///
/// Both engines panic rather than merely reporting, and the ledger is
/// load-bearing in BOTH directions. Proven by temporary mutation, not by a
/// committed failing assertion — both recipes below were run manually while
/// authoring this file, each confirmed to panic, then reverted:
///
/// (a) UNLEDGERED fires: flip a ledgered row (e.g. the `"D4"` row
///     `D4 bare: f(Int) <- TypeParam(T)`, or the `"ambiguity-model"`
///     selection row) to `Expect::Agree` and re-run. Its table driver goes
///     RED with "UNLEDGERED ... divergence", proving a NEW drift cannot slip
///     in undocumented.
///
/// (b) STALE fires: flip a clean row (e.g. `concrete exact: f(Int) <- Int`,
///     or the `exact-wins (concrete first)` selection row) to
///     `Expect::Diverge { ledger: "bogus" }` and re-run. Its driver goes RED
///     with "STALE ledger row 'bogus' — the two resolvers now AGREE",
///     proving a ledger entry cannot outlive the drift it documents.
///
/// (b) is what makes this file self-cleaning: when #5686 lands the two `"D4"`
/// rows start agreeing and go RED as STALE, so the ledger entry is removed by
/// the test failing, not by anyone remembering.
///
/// That is not a prediction — it has already happened once here. #5685 landed
/// the eval-side head-narrowing tier while this file was in flight; the
/// `"head-tier"` row started agreeing, `multi_candidate_selection_parity` went
/// RED as STALE, and the entry was retired (see the RETIRED note in the
/// exemption ledger). Recipe (b) is the mechanism that caught it.
fn assert_selection_parity(row: &SelectionRow) {
    if let Err(msg) = classify_selection_parity(row) {
        panic!("{msg}");
    }
}

#[rustfmt::skip]
fn selection_rows() -> Vec<SelectionRow> {
    let trait_obj = |n: &str| Type::TraitObject(n.to_string());
    let tp        = |n: &str| Type::TypeParam(n.to_string());
    let result_of = |args: Vec<Type>| Type::Applied { name: "Result".to_string(), args };
    let list_of   = |t: Type| Type::List(Box::new(t));
    let option_of = |t: Type| Type::Option(Box::new(t));
    let ph        = |n: &str| Type::unbound_placeholder(n);

    vec![
        // ── Exact-match-wins tie-break: BOTH sides implement it ──────────────
        // Compile discards wildcard matches via `exact_matches`; eval's pass-1
        // finds the exact candidate before the wildcard pass runs. Both
        // declaration orders are listed so the row cannot be satisfied by
        // accident through eval's first-match-wins.
        SelectionRow {
            label: "exact-wins (generic first): [h<T>(T), h(Int)] <- Int",
            fns: vec![make_fn("h", vec![("x", tp("T"))], vec!["T"]),
                      make_fn("h", vec![("x", Type::Int)], vec![])],
            call_name: "h", arg_types: vec![Type::Int], expect: Expect::Agree,
        },
        SelectionRow {
            label: "exact-wins (concrete first): [h(Int), h<T>(T)] <- Int",
            fns: vec![make_fn("h", vec![("x", Type::Int)], vec![]),
                      make_fn("h", vec![("x", tp("T"))], vec!["T"])],
            call_name: "h", arg_types: vec![Type::Int], expect: Expect::Agree,
        },

        // ── Trait-object wildcard on a NON-generic candidate, with a
        //    non-matching sibling present (the esc-4093-152 shape, pinned as
        //    a positive: both sides must pick the trait-object overload). ────
        SelectionRow {
            label: "trait-obj wildcard beats non-matching sibling",
            fns: vec![make_fn("solve", vec![("loads", list_of(trait_obj("Load")))], vec![]),
                      make_fn("solve", vec![("loads", Type::Int)], vec![])],
            call_name: "solve",
            arg_types: vec![list_of(Type::StructureRef("X".to_string()))],
            expect: Expect::Agree,
        },

        // ── Head tier (the #5685 shape, now AGREEING) ────────────────────────
        // Declaration order is LOAD-BEARING and stays so even though this row
        // is now clean: the Option overload MUST be listed first, or eval's
        // first-match-wins would pick the Result one for the wrong reason and
        // the row would agree vacuously, pinning nothing.
        //
        // Compile: both enter `matches`; `exact` is empty; `head_matches` keeps
        // ONLY the Result candidate — `heads_unifiable(Option<T>, Result<..>)`
        // is false via its catch-all, while
        // `heads_unifiable(Result<T,E>, Result<Int,String>)` is true by
        // same-name/same-arity recursion over `(TypeParam(_), _) => true`
        // leaves. Eval now runs the same discrimination in its own `head` tier
        // (#5685), selected via `first_head.or(first_wildcard)`, so it reaches
        // index 1 too rather than falling back on table order.
        SelectionRow {
            label: "head-tier: [unwrap_or<T>(Option<T>,T), unwrap_or<T,E>(Result<T,E>,T)] <- (Result<Int,String>, Int)",
            fns: vec![make_fn("unwrap_or", vec![("o", option_of(tp("T"))), ("d", tp("T"))], vec!["T"]),
                      make_fn("unwrap_or", vec![("r", result_of(vec![tp("T"), tp("E")])), ("d", tp("T"))], vec!["T", "E"])],
            call_name: "unwrap_or",
            arg_types: vec![result_of(vec![Type::Int, Type::String]), Type::Int],
            expect: Expect::Agree,
        },

        // ── Mixed set: tier 2 drops a head-mismatched GENERIC candidate and so
        //    promotes a non-generic one declared after it. Declaration order is
        //    LOAD-BEARING: the generic is listed FIRST, so a first-match-wins
        //    resolver without tier 2 would pick index 0. Two-sided twin of
        //    reify-expr's eval-only
        //    `mixed_set_head_mismatched_generic_yields_to_non_generic_trait_object`.
        SelectionRow {
            label: "mixed set: [g<T>(Option<T>), g(TraitObject X)] <- Result<Int,String>",
            fns: vec![make_fn("g", vec![("x", option_of(tp("T")))], vec!["T"]),
                      make_fn("g", vec![("x", trait_obj("X"))], vec![])],
            call_name: "g",
            arg_types: vec![result_of(vec![Type::Int, Type::String])],
            expect: Expect::Agree,
        },

        // ── Head tier drops a NON-generic candidate whose heads unify only
        //    through an R1 placeholder slot: both candidates pass tier 3, only
        //    the generic passes tier 2. Declaration order is LOAD-BEARING: the
        //    concrete candidate is listed FIRST, so index 1 is reachable only
        //    through tier 2. ─────────────────────────────────────────────────
        SelectionRow {
            label: "head tier drops a non-generic placeholder-unified candidate: [f(Result<Length,String>), f<T,E>(Result<T,E>)] <- Result<Length,?E>",
            fns: vec![make_fn("f", vec![("r", result_of(vec![Type::length(), Type::String]))], vec![]),
                      make_fn("f", vec![("r", result_of(vec![tp("T"), tp("E")]))], vec!["T", "E"])],
            call_name: "f",
            arg_types: vec![result_of(vec![Type::length(), ph("E")])],
            expect: Expect::Agree,
        },

        // ── A bare R1 placeholder arg is admitted only by the generic's
        //    param-side wildcard. Declaration order is LOAD-BEARING: the
        //    concrete candidate is listed FIRST, so a resolver granting the
        //    placeholder arg-side admission would pick index 0 (or report
        //    Ambiguous). ──────────────────────────────────────────────────────
        SelectionRow {
            label: "bare placeholder selects only the generic: [f(Int), f<T>(T)] <- ?T",
            fns: vec![make_fn("f", vec![("x", Type::Int)], vec![]),
                      make_fn("f", vec![("x", tp("T"))], vec!["T"])],
            call_name: "f",
            arg_types: vec![ph("T")],
            expect: Expect::Agree,
        },

        // ── Ambiguity model ──────────────────────────────────────────────────
        // Two trait-object overloads both wildcard-match; compile reports
        // `Ambiguous` (a call-site diagnostic), eval has no ambiguity concept.
        SelectionRow {
            label: "ambiguity: [g(TraitObject A), g(TraitObject B)] <- StructureRef",
            fns: vec![make_fn("g", vec![("x", trait_obj("A"))], vec![]),
                      make_fn("g", vec![("x", trait_obj("B"))], vec![])],
            call_name: "g",
            arg_types: vec![Type::StructureRef("X".to_string())],
            expect: Expect::Diverge { ledger: "ambiguity-model" },
        },
        // A bare user type-param arg wildcards against BOTH concrete overloads
        // (tier 3's arg side) and tier 2's bare-user-`TypeParam` disjunct keeps
        // both, so compile reports `Ambiguous`; eval takes the first.
        SelectionRow {
            label: "ambiguity: bare user type-param arg over two concrete overloads: [f(Int), f(String)] <- T",
            fns: vec![make_fn("f", vec![("x", Type::Int)], vec![]),
                      make_fn("f", vec![("x", Type::String)], vec![])],
            call_name: "f",
            arg_types: vec![tp("T")],
            expect: Expect::Diverge { ledger: "ambiguity-model" },
        },
    ]
}

/// Axis 2 — selection-identity parity over every multi-candidate row.
#[test]
fn multi_candidate_selection_parity() {
    let rows = selection_rows();
    let failures: Vec<String> = rows
        .iter()
        .filter_map(|r| classify_selection_parity(r).err())
        .collect();

    assert!(
        failures.is_empty(),
        "compile-vs-eval selection-identity parity failed for {}/{} row(s):\n  - {}",
        failures.len(),
        rows.len(),
        failures.join("\n  - ")
    );
}

/// Positive parity pin for the head tier — the strongest row in this file,
/// because it is the one that has already caught a real divergence.
///
/// Formerly the anti-vacuity guard for a `"head-tier"` ledger row: compile
/// discriminated `Option<T>` from `Result<T,E>` by constructor head while
/// eval had no head tier and fell back on declaration order, so both sides
/// "accepted" and ran DIFFERENT function bodies. Task #5685 landed the
/// eval-side head-narrowing tier, this test went RED on its own premise
/// assert, and the ledger entry was retired.
///
/// It is REPURPOSED rather than deleted, so the coverage survives the fix:
/// the shape that used to diverge is now pinned as an explicit AGREEMENT,
/// both sides asserted first-hand to select index 1. Deleting it would have
/// left the very shape #5685 fixed unguarded against re-divergence.
///
/// The `Option`-first declaration order remains load-bearing (see
/// `selection_rows`): with `Result` first, eval's first-match-wins would
/// reach index 0... but so would a head tier, so the row could pass without
/// the head tier existing at all. Option-first is what makes index 1 provable
/// ONLY via head narrowing.
#[test]
fn head_tier_selection_agrees_both_sides_select_result_overload() {
    let tp = |n: &str| Type::TypeParam(n.to_string());
    let result_of = |args: Vec<Type>| Type::Applied {
        name: "Result".to_string(),
        args,
    };
    // Option overload FIRST — load-bearing, see `selection_rows`.
    let fns = vec![
        make_fn(
            "unwrap_or",
            vec![("o", Type::Option(Box::new(tp("T")))), ("d", tp("T"))],
            vec!["T"],
        ),
        make_fn(
            "unwrap_or",
            vec![("r", result_of(vec![tp("T"), tp("E")])), ("d", tp("T"))],
            vec!["T", "E"],
        ),
    ];
    let arg_types = vec![result_of(vec![Type::Int, Type::String]), Type::Int];

    // Compile side picks the Result overload, via `head_matches`.
    let verdict =
        reify_compiler::__resolve_function_overload_for_parity_test("unwrap_or", &arg_types, &fns);
    assert_eq!(
        verdict,
        ParityOverloadVerdict::Resolved { index: 1 },
        "head-tier: compile side must select the Result overload via heads_unifiable"
    );

    // Eval side reaches the SAME overload, and can only do so through its own
    // head tier: the Option candidate is declared first and passes `wildcard`,
    // so a wildcard-only resolver would stop at index 0.
    let args: Vec<CompiledExpr> = arg_types.iter().cloned().map(arg).collect();
    let selected = find_matching_compiled_function(&fns, "unwrap_or", &args)
        .expect("head-tier: eval side must resolve the call");
    assert_eq!(
        index_of(&fns, selected),
        1,
        "head-tier: eval side must select the Result overload (index 1), not the \
         first-declared Option one — if this is 0, the eval-side head-narrowing \
         tier landed by #5685 has regressed and compile/eval now run DIFFERENT \
         function bodies for the same call"
    );

    // ...and only then: the engine classifies the row as agreeing.
    assert_selection_parity(&SelectionRow {
        label: "head-tier (companion)",
        fns,
        call_name: "unwrap_or",
        arg_types,
        expect: Expect::Agree,
    });
}

/// Anti-vacuity guard for the "ambiguity-model" ledger row.
///
/// Unlike the other two ledger entries this one is PERMANENT, so this test
/// pins a deliberate architectural asymmetry rather than a countdown to a
/// fix: compile classifies three ways and reports `Ambiguous` as a
/// diagnostic; eval is an `Option` lookup with no ambiguity concept.
#[test]
fn ambiguity_model_divergence_is_genuine_and_ledgered() {
    let trait_obj = |n: &str| Type::TraitObject(n.to_string());
    let fns = vec![
        make_fn("g", vec![("x", trait_obj("A"))], vec![]),
        make_fn("g", vec![("x", trait_obj("B"))], vec![]),
    ];
    let arg_types = vec![Type::StructureRef("X".to_string())];

    // Compile side: both trait-object params wildcard-match, no exact match
    // narrows them, and the head tier keeps both.
    let verdict =
        reify_compiler::__resolve_function_overload_for_parity_test("g", &arg_types, &fns);
    assert_eq!(
        verdict,
        ParityOverloadVerdict::Ambiguous { count: 2 },
        "ambiguity premise: compile side must still report Ambiguous over two \
         trait-object overloads"
    );

    // Eval side: first-match-wins, no ambiguity concept.
    let args: Vec<CompiledExpr> = arg_types.iter().cloned().map(arg).collect();
    let selected = find_matching_compiled_function(&fns, "g", &args)
        .expect("ambiguity premise: eval side commits to a candidate regardless");
    assert_eq!(
        index_of(&fns, selected),
        0,
        "ambiguity premise: eval side must still take the first candidate"
    );

    // ...and only then: the engine classifies the ledgered row as OK.
    assert_selection_parity(&SelectionRow {
        label: "ambiguity-model (companion)",
        fns,
        call_name: "g",
        arg_types,
        expect: Expect::Diverge {
            ledger: "ambiguity-model",
        },
    });
}
