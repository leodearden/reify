//! Compile-vs-eval overload-resolution parity test (task #5992).
//!
//! Two resolvers answer "which user function does this call select?":
//! compile-side `reify_compiler::type_compat::resolve_function_overload`,
//! reached through the `test-support`-gated
//! `reify_compiler::__resolve_function_overload_for_parity_test` shim because
//! the resolver is `pub(crate)`, and eval-side
//! `reify_expr::find_matching_compiled_function`. Both call the per-slot tier
//! predicates in `reify_core::overload`, which owns the tier semantics. Each
//! still implements the per-candidate composition that module lists under
//! "What stays caller-side policy", and these tables pin the two
//! implementations against each other. Drift in the accept direction is
//! silent: the compiler resolves the call, then eval finds no function and
//! yields a `Value::Undef` with no `UndefCause`.
//!
//! ## Contract
//!
//! Every row pins the compile verdict first-hand, which catches both
//! resolvers regressing the same way, and asserts parity on one axis:
//! 1. **accept/reject** — compile `Resolved`|`Ambiguous` iff eval `Some`,
//!    over SINGLE-candidate sets, so no tie-break tier can confound the
//!    per-param predicate;
//! 2. **selection identity** — over MULTI-candidate sets, both sides commit
//!    to the SAME overload. Axis 1 cannot see both sides accepting and
//!    picking different functions.
//!
//! A row may diverge only by citing a [`Ledger`] entry. An uncited divergence
//! fails as UNLEDGERED, a citing row whose resolvers now agree fails as
//! STALE, and an entry that no row cites fails
//! `every_ledger_entry_is_cited_by_a_row`.
//!
//! A single per-candidate resolver shared by both crates would retire this
//! file. The seam that blocks it is recorded in `reify_core::overload`'s
//! "Why the API is per-SLOT" section.

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
/// `type_params` is load-bearing on BOTH sides: the type-param and dim-param
/// wildcards are gated on `!type_params.is_empty()`, while the trait-object
/// wildcard is not. Only that emptiness is consulted, so each
/// `reify_ir::TypeParam`'s `bounds` / `default` stay empty.
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

// ── Exemption ledger ────────────────────────────────────────────────────────

/// A documented, intentional compile-vs-eval divergence, cited by a row's
/// [`Parity::Diverge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ledger {
    /// Architecturally PERMANENT, not a bug. Compile classifies the surviving
    /// set three ways and reports `Ambiguous` as a call-site diagnostic; eval
    /// is a first-match-wins `Option` lookup with no ambiguity concept. A
    /// well-formed program never reaches eval with an ambiguous call, so eval
    /// must NOT be taught to re-detect what the compiler already rejects.
    /// Reached whenever a wildcard admits two or more candidates that no
    /// narrower tier separates.
    AmbiguityModel,
}

impl Ledger {
    /// Every entry, for `every_ledger_entry_is_cited_by_a_row`. An entry
    /// missing here AND cited by no row is never constructed, which the
    /// `dead_code` lint reports.
    const ALL: [Ledger; 1] = [Ledger::AmbiguityModel];
}

/// What a row asserts about compile-vs-eval agreement on its axis.
#[derive(Debug, Clone, Copy)]
enum Parity {
    /// The two resolvers must reach the same answer.
    Agree,
    /// The two resolvers must DIFFER, for the cited ledger reason.
    Diverge(Ledger),
}

// ── Shared assertion engine ─────────────────────────────────────────────────

/// The ledger discipline both axes share. `evidence` renders both sides'
/// answers for the failure message.
fn check_parity(
    label: &str,
    axis: &str,
    agree: bool,
    parity: Parity,
    evidence: &str,
) -> Result<(), String> {
    match (agree, parity) {
        (true, Parity::Agree) | (false, Parity::Diverge(_)) => Ok(()),
        (false, Parity::Agree) => Err(format!(
            "{label}: UNLEDGERED {axis} divergence ({evidence}) — fix the mismatch, \
             or cite a `Ledger` entry documenting why it is intentional"
        )),
        (true, Parity::Diverge(entry)) => Err(format!(
            "{label}: STALE ledger citation {entry:?} — the two resolvers now AGREE \
             ({evidence}); reclassify the row to `Parity::Agree`"
        )),
    }
}

/// The first-hand half of every row: compile must answer exactly `pinned`.
/// Parity alone cannot see both resolvers regressing the same way.
fn check_compile_verdict(
    label: &str,
    pinned: &ParityOverloadVerdict,
    actual: &ParityOverloadVerdict,
) -> Result<(), String> {
    if actual == pinned {
        Ok(())
    } else {
        Err(format!(
            "{label}: compile verdict is {actual:?}, but the row pins {pinned:?}"
        ))
    }
}

/// Run `check` over every row and report EVERY failing row in one panic,
/// since a table this size is easier to diagnose whole.
fn assert_every_row_passes<R>(axis: &str, rows: &[R], check: impl Fn(&R) -> Result<(), String>) {
    let failures: Vec<String> = rows.iter().filter_map(|r| check(r).err()).collect();
    assert!(
        failures.is_empty(),
        "compile-vs-eval {axis} parity failed for {}/{} row(s):\n  - {}",
        failures.len(),
        rows.len(),
        failures.join("\n  - ")
    );
}

// ── Axis 1: accept/reject over SINGLE-candidate overload sets ───────────────

/// One accept/reject row: the overload set is the single candidate
/// `f(params)` with `type_params`.
struct Row {
    label: &'static str,
    params: Vec<(&'static str, Type)>,
    type_params: Vec<&'static str>,
    call_name: &'static str,
    arg_types: Vec<Type>,
    compile_verdict: ParityOverloadVerdict,
    parity: Parity,
}

fn check_accept_reject_row(row: &Row) -> Result<(), String> {
    let fns = vec![make_fn("f", row.params.clone(), row.type_params.clone())];

    let verdict = reify_compiler::__resolve_function_overload_for_parity_test(
        row.call_name,
        &row.arg_types,
        &fns,
    );
    let args: Vec<CompiledExpr> = row.arg_types.iter().cloned().map(arg).collect();
    let eval_accepts = find_matching_compiled_function(&fns, row.call_name, &args).is_some();

    // `Resolved` and `Ambiguous` both mean the call selected at least one
    // candidate; `NoMatch` and `NoUserFunctions` are rejects.
    let compile_accepts = matches!(
        verdict,
        ParityOverloadVerdict::Resolved { .. } | ParityOverloadVerdict::Ambiguous { .. }
    );

    check_parity(
        row.label,
        "accept/reject",
        compile_accepts == eval_accepts,
        row.parity,
        &format!("compile={verdict:?} accepts={compile_accepts}, eval accepts={eval_accepts}"),
    )?;
    check_compile_verdict(row.label, &row.compile_verdict, &verdict)
}

/// Types are built fresh per row rather than shared, so a row can never be
/// perturbed by another's edits.
#[rustfmt::skip]
fn single_candidate_rows() -> Vec<Row> {
    use ParityOverloadVerdict::{NoMatch, NoUserFunctions, Resolved};
    use Parity::Agree;
    let trait_obj = |n: &str| Type::TraitObject(n.to_string());
    let tp        = |n: &str| Type::TypeParam(n.to_string());
    let sp        = |n: &str| Type::ScalarParam(n.to_string());
    let result_of = |args: Vec<Type>| Type::Applied { name: "Result".to_string(), args };
    let ph        = |n: &str| Type::unbound_placeholder(n);
    let list_of   = |t: Type| Type::List(Box::new(t));

    vec![
        // ── Concrete exact match / plain rejects ─────────────────────────────
        Row { label: "concrete exact: f(Int) <- Int",             params: vec![("x", Type::Int)],                    type_params: vec![],    call_name: "f", arg_types: vec![Type::Int],                                    compile_verdict: Resolved { index: 0 }, parity: Agree },
        Row { label: "concrete mismatch: f(Real) <- Int",         params: vec![("x", Type::dimensionless_scalar())], type_params: vec![],    call_name: "f", arg_types: vec![Type::Int],                                    compile_verdict: NoMatch,               parity: Agree },
        Row { label: "arity mismatch: f(Int, Int) <- (Int)",      params: vec![("x", Type::Int), ("y", Type::Int)],  type_params: vec![],    call_name: "f", arg_types: vec![Type::Int],                                    compile_verdict: NoMatch,               parity: Agree },
        Row { label: "name mismatch: f(Int) called as g",         params: vec![("x", Type::Int)],                    type_params: vec![],    call_name: "g", arg_types: vec![Type::Int],                                    compile_verdict: NoUserFunctions,       parity: Agree },

        // ── Trait-object wildcard: UNGATED on genericity on both sides ───────
        // (the esc-4093-152 shape — pinned here as a positive, non-generic.)
        Row { label: "trait-obj param <- StructureRef arg",       params: vec![("j", trait_obj("DrivingJoint"))],    type_params: vec![],    call_name: "f", arg_types: vec![Type::StructureRef("X".to_string())],          compile_verdict: Resolved { index: 0 }, parity: Agree },
        Row { label: "trait-obj param <- other trait-obj arg",    params: vec![("j", trait_obj("DrivingJoint"))],    type_params: vec![],    call_name: "f", arg_types: vec![trait_obj("Other")],                          compile_verdict: Resolved { index: 0 }, parity: Agree },
        Row { label: "List<trait-obj> param <- List<StructRef>",  params: vec![("l", Type::List(Box::new(trait_obj("Load"))))], type_params: vec![], call_name: "f", arg_types: vec![Type::List(Box::new(Type::StructureRef("X".to_string())))], compile_verdict: Resolved { index: 0 }, parity: Agree },

        // ── Type-param wildcard: GATED on genericity on both sides ───────────
        Row { label: "generic f<T>(T) <- Int",                    params: vec![("x", tp("T"))],                      type_params: vec!["T"], call_name: "f", arg_types: vec![Type::Int],                                    compile_verdict: Resolved { index: 0 }, parity: Agree },
        Row { label: "GATE: non-generic f(T) <- Int rejects",     params: vec![("x", tp("T"))],                      type_params: vec![],    call_name: "f", arg_types: vec![Type::Int],                                    compile_verdict: NoMatch,               parity: Agree },

        // ── Dim-param wildcard: GATED on genericity on both sides ────────────
        Row { label: "generic f<D>(Scalar<D>) <- Length",         params: vec![("x", sp("D"))],                      type_params: vec!["D"], call_name: "f", arg_types: vec![Type::length()],                                compile_verdict: Resolved { index: 0 }, parity: Agree },
        Row { label: "GATE: non-generic f(Scalar<D>) <- Length",  params: vec![("x", sp("D"))],                      type_params: vec![],    call_name: "f", arg_types: vec![Type::length()],                                compile_verdict: NoMatch,               parity: Agree },

        // ── Wrapper / enum shapes carrying a nested type param ───────────────
        Row { label: "generic f<T>(Option<T>) <- Option<Int>",    params: vec![("o", Type::Option(Box::new(tp("T"))))], type_params: vec!["T"], call_name: "f", arg_types: vec![Type::Option(Box::new(Type::Int))],        compile_verdict: Resolved { index: 0 }, parity: Agree },
        Row { label: "generic f<T,E>(Result<T,E>) <- Result<Int,String>", params: vec![("r", result_of(vec![tp("T"), tp("E")]))], type_params: vec!["T", "E"], call_name: "f", arg_types: vec![result_of(vec![Type::Int, Type::String])], compile_verdict: Resolved { index: 0 }, parity: Agree },
        // Erased subject: variant construction type-erases `Ok{..}` to `Enum("Result")`.
        Row { label: "generic f<T,E>(Result<T,E>) <- Enum(Result)", params: vec![("r", result_of(vec![tp("T"), tp("E")]))], type_params: vec!["T", "E"], call_name: "f", arg_types: vec![Type::Enum("Result".to_string())], compile_verdict: Resolved { index: 0 }, parity: Agree },

        // ── Type-param-carrying ARG: a bare user `TypeParam` arg wildcards; a
        //    headed one admits only where heads unify (normative text:
        //    `reify_core::overload::slot_matches_wildcard_tier`). ─────────────
        Row { label: "D4 bare user type-param arg: f(Int) <- T accepts",                  params: vec![("x", Type::Int)], type_params: vec![], call_name: "f", arg_types: vec![tp("T")],                           compile_verdict: Resolved { index: 0 }, parity: Agree },
        Row { label: "C-4 headed type-param arg, head mismatch: f(Int) <- Result<T,E> rejects", params: vec![("x", Type::Int)], type_params: vec![], call_name: "f", arg_types: vec![result_of(vec![tp("T"), tp("E")])], compile_verdict: NoMatch,               parity: Agree },

        // ── R1 placeholder arg (an undetermined generic-enum type argument):
        //    no arg-side admission; only a param-side wildcard accepts it. ────
        Row { label: "R1 bare placeholder arg vs concrete: f(Int) <- ?T rejects",            params: vec![("x", Type::Int)],          type_params: vec![],    call_name: "f", arg_types: vec![ph("T")], compile_verdict: NoMatch,               parity: Agree },
        Row { label: "R1 bare placeholder arg vs generic: f<T>(T) <- ?T accepts",            params: vec![("x", tp("T"))],            type_params: vec!["T"], call_name: "f", arg_types: vec![ph("T")], compile_verdict: Resolved { index: 0 }, parity: Agree },
        Row { label: "R1 bare placeholder arg vs trait-obj: f(TraitObject A) <- ?T accepts", params: vec![("x", trait_obj("A"))],     type_params: vec![],    call_name: "f", arg_types: vec![ph("T")], compile_verdict: Resolved { index: 0 }, parity: Agree },

        // ── C-4 headed args: admitted only where heads unify. The candidate is
        //    non-generic, so tier 2 is EMPTY and acceptance requires the
        //    empty-tier fall-through to tier 3 on both sides. ──────────────────
        Row { label: "C-4 placeholder slot, same head: f(Result<Length,String>) <- Result<Length,?E> accepts", params: vec![("r", result_of(vec![Type::length(), Type::String]))], type_params: vec![], call_name: "f", arg_types: vec![result_of(vec![Type::length(), ph("E")])], compile_verdict: Resolved { index: 0 }, parity: Agree },
        Row { label: "C-4 headed user type-param arg, inner mismatch: f(List<Int>) <- List<T> rejects",         params: vec![("l", list_of(Type::Int))],                             type_params: vec![], call_name: "f", arg_types: vec![list_of(tp("T"))],                         compile_verdict: NoMatch,               parity: Agree },

        // ── Tier 2 is a FILTER over tier 3, never a standalone pass:
        //    heads_unifiable's erased-subject arm admits this pair at tier 2,
        //    tier 3 rejects it, so a resolver running tier 2 standalone would
        //    ACCEPT (counterexample source: reify_core::overload's
        //    `slot_matches_head_tier_is_not_a_subset_of_the_wildcard_tier`). ──
        Row { label: "tier 2 is a filter: generic f<T>(Result<Int,String>) <- Enum(Result) rejects", params: vec![("r", result_of(vec![Type::Int, Type::String]))], type_params: vec!["T"], call_name: "f", arg_types: vec![Type::Enum("Result".to_string())], compile_verdict: NoMatch,               parity: Agree },
    ]
}

#[test]
fn single_candidate_accept_reject_parity() {
    assert_every_row_passes(
        "accept/reject",
        &single_candidate_rows(),
        check_accept_reject_row,
    );
}

// ── Axis 2: selection identity over MULTI-candidate overload sets ───────────

/// One selection-identity row: which candidate of a multi-candidate set each
/// resolver commits to. Declaration ORDER is load-bearing in several rows,
/// because eval's passes are first-match-wins while compile's are
/// set-narrowing.
struct SelectionRow {
    label: &'static str,
    fns: Vec<CompiledFunction>,
    call_name: &'static str,
    arg_types: Vec<Type>,
    compile_verdict: ParityOverloadVerdict,
    parity: Parity,
}

/// "Selection" is the single candidate a side committed to: compile's
/// `Resolved { index }`, or eval's `Some(f)` mapped back to its position.
fn check_selection_row(row: &SelectionRow) -> Result<(), String> {
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

    check_parity(
        row.label,
        "selection-identity",
        compile_index == eval_index,
        row.parity,
        &format!("compile={verdict:?} -> {compile_index:?}, eval -> {eval_index:?}"),
    )?;
    check_compile_verdict(row.label, &row.compile_verdict, &verdict)
}

#[rustfmt::skip]
fn selection_rows() -> Vec<SelectionRow> {
    use ParityOverloadVerdict::{Ambiguous, Resolved};
    use Parity::{Agree, Diverge};
    let trait_obj = |n: &str| Type::TraitObject(n.to_string());
    let tp        = |n: &str| Type::TypeParam(n.to_string());
    let result_of = |args: Vec<Type>| Type::Applied { name: "Result".to_string(), args };
    let list_of   = |t: Type| Type::List(Box::new(t));
    let option_of = |t: Type| Type::Option(Box::new(t));
    let ph        = |n: &str| Type::unbound_placeholder(n);

    vec![
        // ── Exact-match-wins tie-break: BOTH sides implement it ──────────────
        // Compile discards wildcard matches via `exact_matches`; eval's tier 1
        // returns the first exact candidate wherever it appears in table order
        // (one fused scan). Both declaration orders are listed so the row
        // cannot be satisfied by accident through eval's first-match-wins.
        SelectionRow {
            label: "exact-wins (generic first): [h<T>(T), h(Int)] <- Int",
            fns: vec![make_fn("h", vec![("x", tp("T"))], vec!["T"]),
                      make_fn("h", vec![("x", Type::Int)], vec![])],
            call_name: "h", arg_types: vec![Type::Int],
            compile_verdict: Resolved { index: 1 }, parity: Agree,
        },
        SelectionRow {
            label: "exact-wins (concrete first): [h(Int), h<T>(T)] <- Int",
            fns: vec![make_fn("h", vec![("x", Type::Int)], vec![]),
                      make_fn("h", vec![("x", tp("T"))], vec!["T"])],
            call_name: "h", arg_types: vec![Type::Int],
            compile_verdict: Resolved { index: 0 }, parity: Agree,
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
            compile_verdict: Resolved { index: 0 }, parity: Agree,
        },

        // ── Head tier: two GENERIC overloads differing only by constructor
        //    head. Both pass tier 3; both sides narrow via tier 2
        //    (`slot_matches_head_tier`) to the Result candidate alone.
        //    Declaration order is LOAD-BEARING: the Option overload is listed
        //    FIRST, so index 1 is reachable only through tier 2 — with Result
        //    first, first-match-wins would reach it without tier 2 at all. ───
        SelectionRow {
            label: "head-tier: [unwrap_or<T>(Option<T>,T), unwrap_or<T,E>(Result<T,E>,T)] <- (Result<Int,String>, Int)",
            fns: vec![make_fn("unwrap_or", vec![("o", option_of(tp("T"))), ("d", tp("T"))], vec!["T"]),
                      make_fn("unwrap_or", vec![("r", result_of(vec![tp("T"), tp("E")])), ("d", tp("T"))], vec!["T", "E"])],
            call_name: "unwrap_or",
            arg_types: vec![result_of(vec![Type::Int, Type::String]), Type::Int],
            compile_verdict: Resolved { index: 1 }, parity: Agree,
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
            compile_verdict: Resolved { index: 1 }, parity: Agree,
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
            compile_verdict: Resolved { index: 1 }, parity: Agree,
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
            compile_verdict: Resolved { index: 1 }, parity: Agree,
        },

        // ── Ambiguity model: compile reports `Ambiguous`, eval commits to a
        //    candidate anyway. ─────────────────────────────────────────────────
        // Two trait-object overloads both wildcard-match.
        SelectionRow {
            label: "ambiguity: [g(TraitObject A), g(TraitObject B)] <- StructureRef",
            fns: vec![make_fn("g", vec![("x", trait_obj("A"))], vec![]),
                      make_fn("g", vec![("x", trait_obj("B"))], vec![])],
            call_name: "g",
            arg_types: vec![Type::StructureRef("X".to_string())],
            compile_verdict: Ambiguous { count: 2 }, parity: Diverge(Ledger::AmbiguityModel),
        },
        // A bare user type-param arg wildcards against BOTH concrete overloads
        // (tier 3's arg side), and tier 2's bare-user-`TypeParam` disjunct
        // keeps both.
        SelectionRow {
            label: "ambiguity: bare user type-param arg over two concrete overloads: [f(Int), f(String)] <- T",
            fns: vec![make_fn("f", vec![("x", Type::Int)], vec![]),
                      make_fn("f", vec![("x", Type::String)], vec![])],
            call_name: "f",
            arg_types: vec![tp("T")],
            compile_verdict: Ambiguous { count: 2 }, parity: Diverge(Ledger::AmbiguityModel),
        },
    ]
}

#[test]
fn multi_candidate_selection_parity() {
    assert_every_row_passes("selection-identity", &selection_rows(), check_selection_row);
}

#[test]
fn every_ledger_entry_is_cited_by_a_row() {
    let cited: Vec<Ledger> = single_candidate_rows()
        .iter()
        .map(|r| r.parity)
        .chain(selection_rows().iter().map(|r| r.parity))
        .filter_map(|p| match p {
            Parity::Diverge(entry) => Some(entry),
            Parity::Agree => None,
        })
        .collect();
    let uncited: Vec<Ledger> = Ledger::ALL
        .into_iter()
        .filter(|entry| !cited.contains(entry))
        .collect();
    assert!(
        uncited.is_empty(),
        "ledger entries cited by no row: {uncited:?} — nothing diverges for that \
         reason any more, so delete the entry"
    );
}
