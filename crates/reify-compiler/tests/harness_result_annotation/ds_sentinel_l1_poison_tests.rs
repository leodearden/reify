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

// ── ds-sentinel residual gap: field arrow-type domain/codomain arms (#4657) ──
//
// RESIDUAL GAP from L1 (#4646), ratified out-of-scope for L1 per esc-4646-3 and
// re-opened by #4657 (reverses the KEEP for these two arms). Unlike the other
// Tier-2 field arms, the `TypeExprKind::Function` domain/codomain arms in
// compile_field are PARSE-REACHABLE: `function_type` is a valid field
// domain/codomain choice (ts_parser lower_field → lower_function_type, task
// 4595). Pre-fix they pushed the root-cause "function type not allowed in this
// position" diagnostic but KEPT returning `Type::dimensionless_scalar()`.
//
// For the codomain arm this is a real secondary cascade: returning a silent
// `Real` (rather than `Type::Error`) does NOT make `codomain_type.is_error()`
// true, so the analytical-source codomain check (`field_codomain_compatible`,
// gated on `is_error()`) still runs. An arrow-typed codomain with a dimensioned
// lambda body (e.g. a `Length`) then spawns a SECOND `FieldCodomainMismatch`
// on top of the root cause. The #4657 fix returns `Type::Error` from both arms.

/// functions.rs codomain Function arm (#4657): an arrow-typed field codomain
/// `(Real) -> Real` with a dimensioned lambda body (`1.0m` = Length) must emit
/// exactly ONE error — the root-cause "function type not allowed" — with NO
/// secondary `FieldCodomainMismatch`. Pre-fix the codomain resolved to
/// dimensionless `Real`, so the body's `Length` mis-matched and a second error
/// cascaded; post-fix the codomain is `Type::Error` and the check short-circuits.
#[test]
fn field_arrow_codomain_resolves_to_error_no_cascade() {
    use reify_test_support::{assert_no_type_cascade, collect_errors};

    let module = compile_source(
        "field def f : Point3 -> (Real) -> Real { source = analytical { |p| 1.0m } }",
    );
    let field = &module.fields[0];

    // (a) The producer-returned codomain type is poison (Type::Error), not a
    // silent dimensionless `Real` — the precise effect of the #4657 fix.
    assert!(
        field.codomain_type.is_error(),
        "arrow-typed field codomain must resolve to Type::Error (poison), got: {:?}",
        field.codomain_type
    );

    // (b) Exactly ONE error — the root-cause — with NO cascaded
    // FieldCodomainMismatch. A regression to `dimensionless_scalar()` would
    // re-introduce the second error.
    let errors = collect_errors(&module.diagnostics);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error (root cause only, no FieldCodomainMismatch cascade), got: {:?}",
        errors
    );
    assert_no_type_cascade(&module.diagnostics, &["function type not allowed"]);
}

/// functions.rs `TypeExprKind::Function` domain arm in `compile_field` (#4657,
/// symmetric): an arrow-typed field domain `(Real) -> Real` must resolve to
/// `Type::Error` (poison).
///
/// The sole RED/GREEN discriminator is `domain_type.is_error()` — unlike the
/// codomain arm, the domain arm does not feed `field_codomain_compatible`, so
/// no secondary cascade fires and the error count is 1 both pre- and post-fix
/// (not a cascade discriminator). The error-count and no-cascade assertions
/// below are regression guards only.
#[test]
fn field_arrow_domain_resolves_to_error() {
    use reify_test_support::{assert_no_type_cascade, collect_errors};

    let module = compile_source(
        "field def g : (Real) -> Real -> Length { source = analytical { |p| 1.0m } }",
    );
    let field = &module.fields[0];

    // Primary discriminator: pre-fix the domain resolves to dimensionless_scalar()
    // (is_error() == false); post-fix it is Type::Error (is_error() == true).
    assert!(
        field.domain_type.is_error(),
        "arrow-typed field domain must resolve to Type::Error (poison), got: {:?}",
        field.domain_type
    );

    // Regression guards — NOT cascade discriminators. The domain arm does not feed
    // `field_codomain_compatible`, so this count is 1 both pre- and post-fix and
    // is not a RED/GREEN signal. Kept as a guard against future regressions.
    let errors = collect_errors(&module.diagnostics);
    assert_eq!(
        errors.len(),
        1,
        "regression guard: expected exactly one error (root cause only), got: {:?}",
        errors
    );
    assert_no_type_cascade(&module.diagnostics, &["function type not allowed"]);
}
