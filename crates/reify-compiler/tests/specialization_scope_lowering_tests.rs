//! Source-driven rewrite of the leaf-signal test from
//! phase-3-grammar-fiction-triage-log.md §B3 (task 3571).
//!
//! User-observable leaf signal: existing `compile_pipeline_invokes_specialization_scope_validator`
//! test (hand-built AST) rewritten to start from `.ri` source and continues to pass.
//!
//! These tests parse `.ri` source through the full
//! `reify_syntax::parse → reify_compiler::compile` pipeline and verify that
//! `SubDecl.body` is correctly populated (`Some(...)`) when the
//! `specialization_body` CST node is present (task 3571).
//!
//! Tests filter `compiled.diagnostics` by `DiagnosticCode::SpecializationForbiddenDecl`
//! to isolate the relevant diagnostics from unrelated noise (e.g. unresolved-name
//! diagnostics from stub types like `"Foo"`).

use reify_core::{DiagnosticCode, ModulePath, Severity};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Filter a slice of diagnostics to only those with
/// `code == DiagnosticCode::SpecializationForbiddenDecl`.
fn forbidden_diagnostics(diagnostics: &[reify_core::Diagnostic]) -> Vec<&reify_core::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SpecializationForbiddenDecl))
        .collect()
}

// ── Leaf-signal test (RED before step-2) ─────────────────────────────────────

/// Source-driven rewrite of `compile_pipeline_invokes_specialization_scope_validator`.
///
/// Parses `.ri` source `structure S { sub scope : Foo { sub inner : Bar } }`, runs
/// the full compile pipeline, and asserts that the specialization-scope validator
/// fires `SpecializationForbiddenDecl` for the forbidden nested `sub` declaration —
/// which requires `lower_sub` to populate `body: Some([MemberDecl::Sub(...)])`.
///
/// Using `sub inner : Bar` (instead of the `param x` in the AST-shape pin below)
/// ensures each test covers a distinct forbidden-decl variant: Sub here, Param in
/// the shape-pin test.
///
/// This test is RED before step-2 because `lower_sub` currently hardcodes
/// `body: None` — the validator's walker is a no-op when `body.is_none()`, so
/// `sub inner` is never visited and no `SpecializationForbiddenDecl` is emitted.
///
/// Leaf signal from phase-3-grammar-fiction-triage-log.md §B3.
#[test]
fn compile_pipeline_invokes_specialization_scope_validator_from_source() {
    let source = "structure S { sub scope : Foo { sub inner : Bar } }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_spec_scope_validator"));

    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors (grammar from task 3569 must accept this form), got: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert!(
        !diags.is_empty(),
        "expected at least one SpecializationForbiddenDecl diagnostic confirming the validator \
         fires when body is populated — lower_sub must set body: Some([Param(x)]) for the \
         validator to visit `param x` and emit the diagnostic; got none.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );

    assert_eq!(
        diags[0].severity,
        Severity::Error,
        "SpecializationForbiddenDecl must be Error severity"
    );
}

// ── Regression and shape pins (step-3) ───────────────────────────────────────

/// (a) AST-shape pin: `structure S { sub scope : Foo { param x } }` lowers to a
/// `SubDecl` whose `body == Some(vec)` with `vec.len() == 1` and `vec[0]` is
/// `MemberDecl::Param` named `"x"`.
#[test]
fn specialization_body_with_param_lowers_to_ast_body_some() {
    let source = "structure S { sub scope : Foo { param x } }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_body_some_param"));

    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    let s = parsed
        .declarations
        .iter()
        .find_map(|d| match d {
            reify_ast::Declaration::Structure(s) if s.name == "S" => Some(s),
            _ => None,
        })
        .expect("structure S should be in parsed declarations");

    let sub_decl = s
        .members
        .iter()
        .find_map(|m| match m {
            reify_ast::MemberDecl::Sub(sub) if sub.name == "scope" => Some(sub),
            _ => None,
        })
        .expect("sub 'scope' should be lowered into S.members");

    let body = sub_decl
        .body
        .as_ref()
        .expect("body should be Some(...) — lower_sub must wire the specialization_body field");

    assert_eq!(
        body.len(),
        1,
        "expected body.len() == 1 (one param x), got {}",
        body.len()
    );

    match &body[0] {
        reify_ast::MemberDecl::Param(p) => {
            assert_eq!(p.name, "x", "expected param named 'x', got '{}'", p.name);
        }
        other => panic!("expected body[0] to be MemberDecl::Param, got: {:?}", other),
    }
}

/// (b) Permitted-only body emits zero `SpecializationForbiddenDecl` diagnostics.
///
/// `structure S { sub motor : Foo { let m = 1.0  constraint m > 0.0 } }` lowers
/// with `body == Some([Let, Constraint])` (length 2) and produces zero
/// `SpecializationForbiddenDecl` diagnostics (the validator's wildcard arm lets
/// `let` and `constraint` through).
#[test]
fn permitted_only_body_emits_zero_forbidden_diagnostics() {
    let source = "structure S { sub motor : Foo { let m = 1.0  constraint m > 0.0 } }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_permitted_body"));

    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    // AST-shape: body should be Some with 2 members (let + constraint).
    let s = parsed
        .declarations
        .iter()
        .find_map(|d| match d {
            reify_ast::Declaration::Structure(s) if s.name == "S" => Some(s),
            _ => None,
        })
        .expect("structure S should be in parsed declarations");

    let sub_decl = s
        .members
        .iter()
        .find_map(|m| match m {
            reify_ast::MemberDecl::Sub(sub) if sub.name == "motor" => Some(sub),
            _ => None,
        })
        .expect("sub 'motor' should be lowered into S.members");

    let body = sub_decl
        .body
        .as_ref()
        .expect("body should be Some for permitted-only body");

    assert_eq!(
        body.len(),
        2,
        "expected body.len() == 2 (let + constraint), got {}",
        body.len()
    );

    assert!(
        matches!(&body[0], reify_ast::MemberDecl::Let(_)),
        "body[0] should be MemberDecl::Let, got: {:?}",
        &body[0]
    );

    assert!(
        matches!(&body[1], reify_ast::MemberDecl::Constraint(_)),
        "body[1] should be MemberDecl::Constraint, got: {:?}",
        &body[1]
    );

    // Compile: zero SpecializationForbiddenDecl diagnostics.
    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert!(
        diags.is_empty(),
        "expected zero SpecializationForbiddenDecl diagnostics for permitted body, got: {:#?}",
        diags
    );
}

/// (c) Where-guard-before-body preserves both `where_clause` and `body`.
///
/// `structure S { sub left : TreeBracket where depth > 0 { depth = depth - 1 } }`
/// lowers to a `SubDecl` with `where_clause: Some(_)` AND `body: Some(_)`.
///
/// The `param_assignment` inside the body is dropped per the design decision
/// (task 3573 follow-up), so `body` may be empty — this test only asserts that
/// the `where_clause` was NOT consumed by body lowering, and that `body` is Some.
#[test]
fn where_guard_before_body_preserves_both_fields() {
    let source = "structure S { sub left : TreeBracket where depth > 0 { depth = depth - 1 } }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_where_guard_body"));

    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    let s = parsed
        .declarations
        .iter()
        .find_map(|d| match d {
            reify_ast::Declaration::Structure(s) if s.name == "S" => Some(s),
            _ => None,
        })
        .expect("structure S should be in parsed declarations");

    let sub_decl = s
        .members
        .iter()
        .find_map(|m| match m {
            reify_ast::MemberDecl::Sub(sub) if sub.name == "left" => Some(sub),
            _ => None,
        })
        .expect("sub 'left' should be lowered into S.members");

    assert!(
        sub_decl.where_clause.is_some(),
        "where_clause should be Some — body lowering must not consume the where_clause field"
    );

    assert!(
        sub_decl.body.is_some(),
        "body should be Some — the {{ depth = depth - 1 }} block should be recognised as a body"
    );

    // The body contains only a `param_assignment` (`depth = depth - 1`), which is
    // silently dropped during lowering per the design decision (task 3573 follow-up).
    // Pin the empty-vec outcome explicitly so task 3573 must consciously update this
    // test when it lowers `param_assignment` to an actual MemberDecl variant.
    assert!(
        sub_decl.body.as_ref().unwrap().is_empty(),
        "param_assignment is dropped per task 3573 follow-up; expected empty body, got: {:?}",
        sub_decl.body.as_ref().unwrap()
    );
}

/// (d) Bare-colon-no-body regression: `structure S { sub a : Foo }` lowers to `body: None`.
#[test]
fn bare_colon_no_body_lowers_to_body_none() {
    let source = "structure S { sub a : Foo }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_bare_colon_no_body"));

    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    let s = parsed
        .declarations
        .iter()
        .find_map(|d| match d {
            reify_ast::Declaration::Structure(s) if s.name == "S" => Some(s),
            _ => None,
        })
        .expect("structure S should be in parsed declarations");

    let sub_decl = s
        .members
        .iter()
        .find_map(|m| match m {
            reify_ast::MemberDecl::Sub(sub) if sub.name == "a" => Some(sub),
            _ => None,
        })
        .expect("sub 'a' should be lowered into S.members");

    assert!(
        sub_decl.body.is_none(),
        "bare-colon-no-body form should lower to body: None, got: {:?}",
        sub_decl.body
    );
}

/// (e) Instantiation-form regression: `structure S { sub a = Foo() }` still lowers to `body: None`.
#[test]
fn instantiation_form_lowers_to_body_none() {
    let source = "structure S { sub a = Foo() }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_instantiation_form"));

    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    let s = parsed
        .declarations
        .iter()
        .find_map(|d| match d {
            reify_ast::Declaration::Structure(s) if s.name == "S" => Some(s),
            _ => None,
        })
        .expect("structure S should be in parsed declarations");

    let sub_decl = s
        .members
        .iter()
        .find_map(|m| match m {
            reify_ast::MemberDecl::Sub(sub) if sub.name == "a" => Some(sub),
            _ => None,
        })
        .expect("sub 'a' should be lowered into S.members");

    assert!(
        sub_decl.body.is_none(),
        "instantiation form should lower to body: None, got: {:?}",
        sub_decl.body
    );
}

/// (f) Collection-form regression: `structure S { sub a : List<Foo> }` still lowers to `body: None`.
#[test]
fn collection_form_lowers_to_body_none() {
    let source = "structure S { sub a : List<Foo> }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_collection_form"));

    assert!(
        parsed.errors.is_empty(),
        "expected no parse errors, got: {:?}",
        parsed.errors
    );

    let s = parsed
        .declarations
        .iter()
        .find_map(|d| match d {
            reify_ast::Declaration::Structure(s) if s.name == "S" => Some(s),
            _ => None,
        })
        .expect("structure S should be in parsed declarations");

    let sub_decl = s
        .members
        .iter()
        .find_map(|m| match m {
            reify_ast::MemberDecl::Sub(sub) if sub.name == "a" => Some(sub),
            _ => None,
        })
        .expect("sub 'a' should be lowered into S.members");

    assert!(
        sub_decl.body.is_none(),
        "collection form should lower to body: None, got: {:?}",
        sub_decl.body
    );
}
