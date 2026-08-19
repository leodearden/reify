//! Compile-vs-eval overload-resolution parity test (task #5992).
//!
//! Ties together the two independently-authored implementations of ONE
//! predicate — "which user function does this call select?" — so they cannot
//! silently drift:
//! - **compile side**: `resolve_function_overload`
//!   (`crates/reify-compiler/src/type_compat.rs:1155`) — a `matches` tier of
//!   per-param disjuncts, narrowed by two tie-break tiers (`exact_matches`,
//!   then `head_matches` / `heads_unifiable`), classified into
//!   `Resolved` / `NoMatch` / `Ambiguous`.
//! - **eval side**: `find_matching_compiled_function`
//!   (`crates/reify-expr/src/lib.rs:1533`) — an exact first-match-wins pass
//!   followed by a wildcard pass, yielding `Option<&CompiledFunction>`.
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
//! `eval_user_function_call` (`crates/reify-expr/src/lib.rs:1625`) fails to
//! find the same function and returns a bare `Value::Undef` carrying NO
//! `UndefCause` — an undiagnosed hole, not an error message.
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
//! mirrors `__infer_mul_div_result_for_parity_test` (`lib.rs:178`); the
//! self-pull dev-dependency that activates `feature = "test-support"` for
//! this integration-test target already exists (`Cargo.toml`), so no
//! manifest change was needed. The eval-side
//! `find_matching_compiled_function` is already `pub`.
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
//! `reify-expr/src/lib.rs:1306/1346/1439`). Introducing that seam is a
//! scope-level decision rather than a bug fix, and is a candidate for the
//! Wave-3 scope discussion in `docs/prds/v0_6/builtin-signature-registry.md`.
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
/// Mirrors `type_compat.rs:2333`'s in-crate `make_fn` plus a `type_params`
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
///     `D4 bare: f(Int) <- TypeParam(T)`, or the `"head-tier"` selection row)
///     to `Expect::Agree` and re-run. Its table driver goes RED with
///     "UNLEDGERED ... divergence", proving a NEW drift cannot slip in
///     undocumented.
///
/// (b) STALE fires: flip a clean row (e.g. `concrete exact: f(Int) <- Int`,
///     or the `exact-wins (concrete first)` selection row) to
///     `Expect::Diverge { ledger: "bogus" }` and re-run. Its driver goes RED
///     with "STALE ledger row 'bogus' — the two resolvers now AGREE",
///     proving a ledger entry cannot outlive the drift it documents.
///
/// (b) is what makes this file self-cleaning: when #5686 lands the two `"D4"`
/// rows start agreeing and go RED as STALE, and when #5685 lands the
/// `"head-tier"` row does the same — so each ledger entry is removed by the
/// test failing, not by anyone remembering.
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
// ── "D4" — type-param-carrying ARG as a compile-only wildcard ───────────────
//
// Compile-side `resolve_function_overload`'s `matches` tier applies FOUR
// per-param disjuncts (type_compat.rs:1195-1202); the fourth,
// `type_carries_type_param(arg_ty)` (task-4232 γ), lets a generic fn body pass
// a `TypeParam`-typed value to a concrete-param callee without a spurious
// NoMatch. Eval-side `find_matching_compiled_function`'s wildcard pass
// (reify-expr/src/lib.rs:1566-1573) has only THREE disjuncts — there is no
// arg-side counterpart at all.
//
// So `P_compile == P_eval || type_carries_type_param(arg_ty)`: for a
// single-candidate overload set, D4 is the ONLY accept/reject divergence
// class, which is why the other 14 rows in the table below are clean.
//
// Consequence when it fires: the compiler resolves the call, then
// `eval_user_function_call` (reify-expr/src/lib.rs:1625) finds nothing and
// returns a bare `Value::Undef` carrying NO `UndefCause` — the silent
// INV-SF-1 miss path, not a diagnostic.
//
// This is TEMPORARY and OWNED. Adding the missing eval-side disjunct is
// task #5686's fix; this task deliberately does NOT duplicate it — it pins
// the divergence so it cannot widen or be forgotten. Covers both the bare
// `TypeParam(T)` arg and the HEADED arg carrying a nested type param
// (`Result<T,E>`), which reach `matches` by the same disjunct but differ in
// the head tier: the bare form is kept by `head_matches`'s
// `matches!(arg_ty, Type::TypeParam(_))` arm, while the headed form leaves
// `head_matches` empty and falls through to `matches`.
//
// TODO(#5686): remove this ledger row and reclassify the two D4 rows to
// Expect::Agree once the eval-side D4 disjunct lands.
//
// ── "head-tier" — compile's second tie-break tier has no eval counterpart ───
//
// Compile-side `resolve_function_overload` narrows `matches` twice: first by
// `exact_matches`, then by `head_matches` / `heads_unifiable`
// (type_compat.rs:1019 and 1245-1290), which discriminates two GENERIC
// overloads that differ only by constructor head. Eval-side
// `find_matching_compiled_function` has an exact pass and a wildcard pass and
// nothing else — no head tier exists anywhere in `crates/reify-expr/`.
//
// So two generic overloads differing only by head (`Option<T>` vs
// `Result<T,E>`) resolve to DIFFERENT functions on the two sides: compile
// picks the head-compatible one, eval picks whichever was declared first.
// Note this divergence is invisible to the accept/reject axis — both sides
// accept. Only selection identity catches it, which is why axis 2 exists.
//
// This one IS owned and IS temporary, with a DIFFERENT owner from D4: task
// #5685 adds the missing eval-side head-narrowing tier. #5686 does NOT close
// this row — it adds only the D4 disjunct to the wildcard pass — so the two
// ledger entries must be retired independently.
//
// TODO(#5685): remove this ledger row and reclassify the head-tier row to
// Expect::Agree once the eval-side head-narrowing tier lands.
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
// Deliberately carries NO task cite: it is permanent, so a `TODO` here would
// be an untracked PTODO with no owner and nothing to close it.

// ── Single-candidate accept/reject table ────────────────────────────────────

/// Every row below is a ONE-candidate overload set. Types are built fresh per
/// row rather than shared, so a row can never be perturbed by another's edits.
#[rustfmt::skip]
fn single_candidate_rows() -> Vec<Row> {
    let trait_obj = |n: &str| Type::TraitObject(n.to_string());
    let tp        = |n: &str| Type::TypeParam(n.to_string());
    let sp        = |n: &str| Type::ScalarParam(n.to_string());
    let result_of = |args: Vec<Type>| Type::Applied { name: "Result".to_string(), args };

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

        // ── D4: type-param-carrying ARG as a compile-only wildcard ───────────
        // Compile's `matches` tier has a fourth disjunct `type_carries_type_param(arg_ty)`
        // that eval's wildcard pass has no counterpart for. Expected to be the
        // ONLY accept/reject divergence class in this table.
        Row { label: "D4 bare: f(Int) <- TypeParam(T)",           params: vec![("x", Type::Int)],                    type_params: vec![],    call_name: "f", arg_types: vec![tp("T")],                                      expect: Expect::Diverge { ledger: "D4" } },
        Row { label: "D4 nested: f(Int) <- Result<T,E>",          params: vec![("x", Type::Int)],                    type_params: vec![],    call_name: "f", arg_types: vec![result_of(vec![tp("T"), tp("E")])],            expect: Expect::Diverge { ledger: "D4" } },
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

/// Anti-vacuity guard for the "D4" ledger row.
///
/// The ledger only earns its keep if the divergence it documents is REAL
/// right now — a ledger row covering a case that never diverged would pass
/// silently forever. So assert the two sides' behaviour FIRST-HAND before
/// asserting that the engine accepts the row as ledgered.
#[test]
fn d4_arg_side_type_param_divergence_is_genuine_and_ledgered() {
    let fns = vec![make_fn("f", vec![("x", Type::Int)], vec![])];
    let arg_ty = Type::TypeParam("T".to_string());

    // Compile side ACCEPTS: `type_carries_type_param(arg_ty)` fires, and the
    // head tier's bare-`TypeParam` arm keeps the sole candidate.
    let verdict = reify_compiler::__resolve_function_overload_for_parity_test(
        "f",
        std::slice::from_ref(&arg_ty),
        &fns,
    );
    assert_eq!(
        verdict,
        ParityOverloadVerdict::Resolved { index: 0 },
        "D4 premise: compile side must still RESOLVE `f(x: Int)` called with a \
         `TypeParam` arg — if this fails, #5686 or a related change altered the \
         compile side and the D4 ledger row needs re-deriving, not deleting"
    );

    // Eval side REJECTS: its wildcard pass has no arg-side disjunct.
    assert!(
        find_matching_compiled_function(&fns, "f", &[arg(arg_ty.clone())]).is_none(),
        "D4 premise: eval side must still return None — if this now resolves, \
         #5686 has landed: delete the D4 ledger row and reclassify both D4 rows \
         to Expect::Agree"
    );

    // ...and only then: the engine classifies the ledgered row as OK.
    assert_accept_reject_parity(&Row {
        label: "D4 bare (companion)",
        params: vec![("x", Type::Int)],
        type_params: vec![],
        call_name: "f",
        arg_types: vec![arg_ty],
        expect: Expect::Diverge { ledger: "D4" },
    });
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
///     `D4 bare: f(Int) <- TypeParam(T)`, or the `"head-tier"` selection row)
///     to `Expect::Agree` and re-run. Its table driver goes RED with
///     "UNLEDGERED ... divergence", proving a NEW drift cannot slip in
///     undocumented.
///
/// (b) STALE fires: flip a clean row (e.g. `concrete exact: f(Int) <- Int`,
///     or the `exact-wins (concrete first)` selection row) to
///     `Expect::Diverge { ledger: "bogus" }` and re-run. Its driver goes RED
///     with "STALE ledger row 'bogus' — the two resolvers now AGREE",
///     proving a ledger entry cannot outlive the drift it documents.
///
/// (b) is what makes this file self-cleaning: when #5686 lands the two `"D4"`
/// rows start agreeing and go RED as STALE, and when #5685 lands the
/// `"head-tier"` row does the same — so each ledger entry is removed by the
/// test failing, not by anyone remembering.
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

        // ── Head tier (the #5685 shape) ──────────────────────────────────────
        // Declaration order is LOAD-BEARING: the Option overload MUST be first,
        // or eval's first-match-wins would coincidentally pick the Result one
        // and mask the divergence.
        //
        // Compile: both enter `matches`; `exact` is empty; `head_matches` keeps
        // ONLY the Result candidate — `heads_unifiable(Option<T>, Result<..>)`
        // is false via its catch-all, while
        // `heads_unifiable(Result<T,E>, Result<Int,String>)` is true by
        // same-name/same-arity recursion over `(TypeParam(_), _) => true`
        // leaves. Eval has no head tier at all, so its wildcard pass takes the
        // first candidate.
        SelectionRow {
            label: "head-tier: [unwrap_or<T>(Option<T>,T), unwrap_or<T,E>(Result<T,E>,T)] <- (Result<Int,String>, Int)",
            fns: vec![make_fn("unwrap_or", vec![("o", option_of(tp("T"))), ("d", tp("T"))], vec!["T"]),
                      make_fn("unwrap_or", vec![("r", result_of(vec![tp("T"), tp("E")])), ("d", tp("T"))], vec!["T", "E"])],
            call_name: "unwrap_or",
            arg_types: vec![result_of(vec![Type::Int, Type::String]), Type::Int],
            expect: Expect::Diverge { ledger: "head-tier" },
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

/// Anti-vacuity guard for the "head-tier" ledger row (see
/// `d4_arg_side_type_param_divergence_is_genuine_and_ledgered` for the same
/// pattern on axis 1): assert the divergence is REAL right now, first-hand,
/// before asserting that the engine accepts the row as ledgered.
#[test]
fn head_tier_selection_divergence_is_genuine_and_ledgered() {
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
        "head-tier premise: compile side must still select the Result overload \
         via heads_unifiable"
    );

    // Eval side picks the FIRST candidate — the Option overload — because it
    // has no head tier. Both sides "accept"; only the index differs.
    let args: Vec<CompiledExpr> = arg_types.iter().cloned().map(arg).collect();
    let selected = find_matching_compiled_function(&fns, "unwrap_or", &args)
        .expect("head-tier premise: eval side accepts (it just picks the wrong one)");
    assert_eq!(
        index_of(&fns, selected),
        0,
        "head-tier premise: eval side must still select the FIRST (Option) \
         overload — if this is now 1, #5685 has landed: delete the head-tier \
         ledger row and reclassify the row to Expect::Agree"
    );

    // ...and only then: the engine classifies the ledgered row as OK.
    assert_selection_parity(&SelectionRow {
        label: "head-tier (companion)",
        fns,
        call_name: "unwrap_or",
        arg_types,
        expect: Expect::Diverge {
            ledger: "head-tier",
        },
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
