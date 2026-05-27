//! Integration tests for the specialization-scope validator (task 2370).
//!
//! These tests verify two things:
//!
//! 1. **Pipeline wiring** (`compile_pipeline_invokes_specialization_scope_validator`):
//!    a smoke test confirming that `reify_compiler::compile()` invokes the
//!    specialization-scope validator and surfaces
//!    `DiagnosticCode::SpecializationForbiddenDecl` through the public
//!    `CompiledModule::diagnostics` field.  Per-kind, combination, permitted-only,
//!    and nested coverage lives in the validator's own unit tests in
//!    `crates/reify-compiler/src/compile_builder/specialization_scope_check.rs`.
//!
//! 2. **`match`-arm scenario** (`forbidden_decl_in_match_arm_sub_body_emits_diagnostic`):
//!    a `MatchArmDeclGroup` nested inside an outer `sub`-with-body must be walked
//!    by `walk_specialization_scope_members`, which recurses into
//!    `MemberDecl::MatchArmDeclGroup` arms.  This scenario is not covered by the
//!    unit tests in `specialization_scope_check.rs`.
//!
//! All tests hand-construct `ParsedModule` AST nodes (no tree-sitter parsing)
//! because the grammar update for the `sub name : Type { body }` form is
//! intentionally deferred (see `sub_decl_specialization_tests.rs:14-17`).
//!
//! Tests filter `compiled.diagnostics` by `DiagnosticCode::SpecializationForbiddenDecl`
//! to isolate the relevant diagnostics from unrelated noise (e.g. unresolved-name
//! diagnostics from stub types like `"Foo"`).

use reify_ast::{Expr, ExprKind, MatchArmDeclArmDecl, MatchArmDeclGroupDecl, MemberDecl};
use reify_test_support::specialization_fixtures::*;
use reify_core::{DiagnosticCode, Severity, SourceSpan};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Filter a slice of diagnostics to only those with
/// `code == DiagnosticCode::SpecializationForbiddenDecl`.
fn forbidden_diagnostics(diagnostics: &[reify_core::Diagnostic]) -> Vec<&reify_core::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SpecializationForbiddenDecl))
        .collect()
}

fn make_match_arm_decl(pattern: &str, member: MemberDecl) -> MatchArmDeclArmDecl {
    MatchArmDeclArmDecl {
        patterns: vec![pattern.to_string()],
        member: Box::new(member),
        span: zero_span(),
    }
}

fn make_match_arm_decl_group(
    discriminant_name: &str,
    arms: Vec<MatchArmDeclArmDecl>,
) -> MemberDecl {
    MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: Expr {
            kind: ExprKind::Ident(discriminant_name.to_string()),
            span: zero_span(),
        },
        arms,
        span: zero_span(),
        content_hash: dummy_hash(),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Smoke test verifying that `reify_compiler::compile()` is wired to invoke the
/// specialization-scope validator.  A `param` declaration inside a
/// `sub`-with-body must surface at least one `SpecializationForbiddenDecl`
/// diagnostic through the public `CompiledModule::diagnostics` field.
///
/// Per-kind, combination, permitted-only, and nested coverage lives in the
/// validator's own unit tests in
/// `crates/reify-compiler/src/compile_builder/specialization_scope_check.rs`.
///
/// Shape: `structure S { sub scope : Foo { param x } }`
#[test]
fn compile_pipeline_invokes_specialization_scope_validator() {
    let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
        "scope",
        zero_span(),
        vec![make_param("x", zero_span())],
    )]);

    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert!(
        !diags.is_empty(),
        "expected at least one SpecializationForbiddenDecl diagnostic confirming \
         the validator is wired into the compile pipeline, got none"
    );
    assert_eq!(
        diags[0].severity,
        Severity::Error,
        "SpecializationForbiddenDecl must be Error severity"
    );
}

/// A `MatchArmDeclGroup` nested inside an outer `sub`-with-body must be walked
/// by `walk_specialization_scope_members`, which recurses into
/// `MemberDecl::MatchArmDeclGroup` arms.  A `sub` arm that itself has a body
/// containing a `param` must produce exactly two diagnostics:
///   - one for the arm's `sub head` (forbidden bare-sub in outer scope), and
///   - one for the leaf `param x` inside the arm's sub body (forbidden in inner scope).
///
/// This scenario is not covered by the unit tests in `specialization_scope_check.rs`.
///
/// The test uses `iter().any(...)` rather than positional indexing because the
/// exact ordering of diagnostics through `MatchArmDeclGroup` traversal is an
/// internal detail of `walk_members_depth` not pinned by prior tests.
///
/// Shape (nested for validator reachability — see design decisions in plan.json):
/// ```text
/// structure S {
///   sub motor : Foo {
///     match head_type {
///       Hex => sub head : Foo { param x }
///     }
///   }
/// }
/// ```
///
/// The `MatchArmDeclGroup` is placed inside an outer `sub motor`-with-body so
/// the validator's top-level walker (`find_specialization_scopes`) reaches it.
/// Placing the match-arm-group at top-level would silently miss the inner
/// forbidden decls (see plan.json design decisions for rationale).
#[test]
fn forbidden_decl_in_match_arm_sub_body_emits_diagnostic() {
    let arm_sub_span = SourceSpan::new(90, 110);
    let leaf_param_span = SourceSpan::new(30, 50);

    // Inner arm: `sub head : Foo { param x }`
    let arm_sub = make_sub_with_body("head", arm_sub_span, vec![make_param("x", leaf_param_span)]);
    let match_group =
        make_match_arm_decl_group("head_type", vec![make_match_arm_decl("Hex", arm_sub)]);

    // Outer specialization scope: `sub motor : Foo { <match_group> }`
    let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
        "motor",
        zero_span(),
        vec![match_group],
    )]);

    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert_eq!(
        diags.len(),
        2,
        "expected exactly two SpecializationForbiddenDecl diagnostics \
         (arm sub + leaf param), got: {:#?}",
        diags
    );

    // One diagnostic must be for the arm's Sub itself (forbidden in outer scope).
    let has_sub_head = diags
        .iter()
        .any(|d| d.message.contains("'sub'") && d.message.contains("'head'"));
    assert!(
        has_sub_head,
        "expected a diagnostic for forbidden 'sub' named 'head', got: {:#?}",
        diags
    );

    // One diagnostic must be for the leaf Param inside the arm's sub body.
    let has_param_x = diags
        .iter()
        .any(|d| d.message.contains("'param'") && d.message.contains("'x'"));
    assert!(
        has_param_x,
        "expected a diagnostic for forbidden 'param' named 'x', got: {:#?}",
        diags
    );
}
