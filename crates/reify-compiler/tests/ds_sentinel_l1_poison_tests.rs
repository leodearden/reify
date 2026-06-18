//! L1 ds-sentinel poison tests (task #4646).
//!
//! Pins the surface-reachable producer sites where an unresolved type NAME in a
//! function- or trait-signature position must resolve the producer's RESOLVED
//! TYPE to `Type::Error` (poison), NOT `Type::dimensionless_scalar()`.
//!
//! Rationale (PRD docs/prds/dimensionless-scalar-sentinel-stampout.md §9 L1): a
//! `dimensionless_scalar()` fallback at a resolution-failure site leaks a silent
//! `Real` into downstream scope/body/overload/conformance resolution and spawns a
//! secondary mis-typed cascade. Returning `Type::Error` engages the existing
//! anti-cascade guards (`implicitly_converts_to(Error, _) => true` in
//! type_compat.rs; `is_error()` short-circuits) so the root-cause diagnostic
//! stands alone.
//!
//! DISCRIMINATOR: each test asserts `.is_error()` on the producer-returned
//! resolved Type — the precise effect of the L1 fix (dimensionless == not-error
//! pre-fix -> Error == is-error post-fix). A diagnostic-count test would be GREEN
//! pre-fix (the resolved=false gate already suppresses the cascade) and thus a
//! doomed RED; `.is_error()` is genuinely RED pre-fix.
//!
//! The parse-unreachable Tier-2 field arms + pub(crate)-only assoc-fn sites are
//! tested by direct producer construction inside the crate (functions.rs /
//! traits.rs `#[cfg(test)] mod tests`); this file covers only the
//! surface-reachable unknown-NAME fixtures via `compile_source`.

use reify_test_support::compile_source;

/// functions.rs compile_function return-type position (site :202): an unresolved
/// return-type NAME `Bogus` must make the compiled function's `return_type`
/// poison (`Type::Error`), not a silent dimensionless `Real`.
#[test]
fn fn_return_unresolved_name_resolves_to_error() {
    let module = compile_source("module m\nfn f() -> Bogus { 0 }");
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "f")
        .expect("function f should be compiled");
    assert!(
        func.return_type.is_error(),
        "unresolved return type `Bogus` must resolve to Type::Error (poison), got: {:?}",
        func.return_type
    );
}

/// functions.rs compile_function param position (site :99): an unresolved param
/// type NAME `Bogus` must make the compiled param's Type poison (`Type::Error`),
/// not a silent dimensionless `Real`.
#[test]
fn fn_param_unresolved_name_resolves_to_error() {
    let module = compile_source("module m\nfn g(x : Bogus) -> Real { 0.0 }");
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "g")
        .expect("function g should be compiled");
    let (_, x_ty) = func
        .params
        .iter()
        .find(|(name, _)| name == "x")
        .expect("function g should have a param named x");
    assert!(
        x_ty.is_error(),
        "unresolved param type `Bogus` must resolve to Type::Error (poison), got: {:?}",
        x_ty
    );
}

/// traits.rs resolve_trait_member_type_annotation final unresolved-Named fallback
/// (site :96): an unresolved type NAME in a trait `param` member must make the
/// required member's Type poison (`Type::Error`), not a silent dimensionless `Real`.
#[test]
fn trait_member_unresolved_name_resolves_to_error() {
    use reify_compiler::RequirementKind;
    let module = compile_source("module m\ntrait T { param x : Bogus }");
    let t = module
        .trait_defs
        .iter()
        .find(|t| t.name == "T")
        .expect("trait T should be compiled");
    let x = t
        .required_members
        .iter()
        .find(|r| r.name == "x")
        .expect("trait T should have a required member x");
    match &x.kind {
        RequirementKind::Param(ty) => assert!(
            ty.is_error(),
            "unresolved trait-member type `Bogus` must resolve to Type::Error (poison), got: {:?}",
            ty
        ),
        other => panic!("expected RequirementKind::Param, got: {:?}", other),
    }
}

/// Anti-cascade closure (reviewer follow-up): the producer-side `.is_error()` tests
/// above prove an unresolved return-type NAME poisons the function's `return_type`,
/// but not that the poison actually SUPPRESSES the secondary cascade at a *use* site
/// — which is the whole motivation of PRD §9 L1 ("each emit exactly ONE error").
///
/// This pins the PRD's headline signal end-to-end: `fn f() -> Bogus { 0 }` is the
/// root-cause (one "unresolved return type" error), and *using* `f()` in a BinOp
/// against a dimensioned literal (`+ 5mm`) would, if the unresolved return type
/// leaked a silent dimensionless `Real` (the pre-fix bug), spawn a SECOND "dimension
/// mismatch" diagnostic on top. With the L1 `Type::Error` fix the consumer BinOp
/// short-circuits (`infer_binop_type` accepts an `is_error()` operand → poison), so
/// exactly the root-cause error survives — no mis-typed cascade.
#[test]
fn unresolved_fn_return_use_does_not_cascade() {
    use reify_core::Type;
    use reify_test_support::{assert_no_type_cascade, collect_errors, get_let_expr_in};

    let module = compile_source("fn f() -> Bogus { 0 }\nstructure S { let broken = f() + 5mm }");

    // (a) The consumer BinOp short-circuits to Type::Error once the poisoned call
    // result flows in — direct proof the poison propagated rather than leaking a
    // dimensionless `Real` into downstream type-checking.
    let broken = get_let_expr_in(&module, "S", "broken");
    assert_eq!(
        broken.result_type,
        Type::Error,
        "`f() + 5mm` must short-circuit to Type::Error (anti-cascade), got: {:?}",
        broken.result_type
    );

    // (b) Exactly ONE error — the root-cause "unresolved return type" — with NO
    // cascaded dimension/type-mismatch diagnostic on top (the PRD L1 signal). A
    // regression to `dimensionless_scalar()` would re-introduce the second error.
    let errors = collect_errors(&module.diagnostics);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error (root cause only, no cascade), got: {:?}",
        errors
    );
    assert_no_type_cascade(&module.diagnostics, &["unresolved return type"]);
}
