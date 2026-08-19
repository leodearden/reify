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
    let params: Vec<(String, Type)> = params.into_iter().map(|(n, t)| (n.to_string(), t)).collect();
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

    let verdict = reify_compiler::__resolve_function_overload_for_parity_test(
        "f",
        &[Type::Int],
        &fns,
    );
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
