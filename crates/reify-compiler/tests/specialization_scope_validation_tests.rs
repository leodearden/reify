//! Integration tests for the specialization-scope validator (task 2370).
//!
//! These tests exercise the forbidden-decl diagnostic path end-to-end through
//! the public `reify_compiler::compile()` entry point, covering all five
//! PRD-listed scenarios from `docs/prds/specialization-scope.md`:
//!
//! 1. Forbidden `param` inside a specialization scope.
//! 2. Forbidden `port` inside a specialization scope.
//! 3. Forbidden bare `sub` inside a specialization scope.
//! 4. All three forbidden kinds in a single scope.
//! 5. Permitted-only body (negative test — no diagnostics).
//! 6. Nested `sub` with forbidden inside.
//! 7. `match`-arm `sub` body with forbidden inside.
//!
//! All tests hand-construct `ParsedModule` AST nodes (no tree-sitter parsing)
//! because the grammar update for the `sub name : Type { body }` form is
//! intentionally deferred (see `sub_decl_specialization_tests.rs:14-17`).
//!
//! Tests filter `compiled.diagnostics` by `DiagnosticCode::SpecializationForbiddenDecl`
//! to isolate the relevant diagnostics from unrelated noise (e.g. unresolved-name
//! diagnostics from stub types like `"Foo"`, `"SomePort"`).

use reify_syntax::{
    ConstraintDecl, Declaration, Expr, ExprKind, LetDecl, MemberDecl, ParsedModule, ParamDecl,
    PortDecl, StructureDef, SubDecl,
};
use reify_types::{ContentHash, DiagnosticCode, ModulePath, Severity, SourceSpan};

// ── Span helpers ──────────────────────────────────────────────────────────────

fn zero_span() -> SourceSpan {
    SourceSpan::new(0, 0)
}

fn param_span() -> SourceSpan {
    SourceSpan::new(30, 50)
}

fn port_span() -> SourceSpan {
    SourceSpan::new(60, 80)
}

fn sub_span() -> SourceSpan {
    SourceSpan::new(90, 110)
}

// ── AST construction helpers ──────────────────────────────────────────────────

fn dummy_hash() -> ContentHash {
    ContentHash(0)
}

fn dummy_expr() -> Expr {
    Expr {
        kind: ExprKind::BoolLiteral(true),
        span: zero_span(),
    }
}

fn make_param(name: &str, span: SourceSpan) -> MemberDecl {
    MemberDecl::Param(ParamDecl {
        name: name.to_string(),
        doc: None,
        type_expr: None,
        default: None,
        where_clause: None,
        annotations: Vec::new(),
        span,
        content_hash: dummy_hash(),
    })
}

fn make_port(name: &str, span: SourceSpan) -> MemberDecl {
    MemberDecl::Port(PortDecl {
        name: name.to_string(),
        direction: None,
        type_name: "SomePort".to_string(),
        members: Vec::new(),
        frame_expr: None,
        span,
        content_hash: dummy_hash(),
    })
}

fn make_sub_bare(name: &str, span: SourceSpan) -> MemberDecl {
    MemberDecl::Sub(SubDecl {
        name: name.to_string(),
        structure_name: "Foo".to_string(),
        type_args: Vec::new(),
        args: Vec::new(),
        is_collection: false,
        where_clause: None,
        body: None,
        span,
        content_hash: dummy_hash(),
    })
}

fn make_sub_with_body(name: &str, span: SourceSpan, body: Vec<MemberDecl>) -> MemberDecl {
    MemberDecl::Sub(SubDecl {
        name: name.to_string(),
        structure_name: "Foo".to_string(),
        type_args: Vec::new(),
        args: Vec::new(),
        is_collection: false,
        where_clause: None,
        body: Some(body),
        span,
        content_hash: dummy_hash(),
    })
}

fn make_let(name: &str) -> MemberDecl {
    MemberDecl::Let(LetDecl {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        type_expr: None,
        value: dummy_expr(),
        where_clause: None,
        annotations: Vec::new(),
        span: zero_span(),
        content_hash: dummy_hash(),
    })
}

fn make_constraint() -> MemberDecl {
    MemberDecl::Constraint(ConstraintDecl {
        label: None,
        expr: dummy_expr(),
        where_clause: None,
        span: zero_span(),
        content_hash: dummy_hash(),
    })
}

/// Build a `ParsedModule` with a single `Structure` whose top-level members
/// are the supplied `members` slice.
fn parsed_module_with_structure_members(members: Vec<MemberDecl>) -> ParsedModule {
    ParsedModule {
        path: ModulePath::single("test"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: Vec::new(),
            trait_bounds: Vec::new(),
            members,
            span: zero_span(),
            content_hash: dummy_hash(),
            pragmas: Vec::new(),
            annotations: Vec::new(),
        })],
        errors: Vec::new(),
        content_hash: dummy_hash(),
        pragmas: Vec::new(),
    }
}

/// Filter a slice of diagnostics to only those with
/// `code == DiagnosticCode::SpecializationForbiddenDecl`.
fn forbidden_diagnostics(diagnostics: &[reify_types::Diagnostic]) -> Vec<&reify_types::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SpecializationForbiddenDecl))
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// PRD acceptance criterion 1: a `param` declaration directly inside a
/// specialization-scope body must produce exactly one Error diagnostic with
/// `DiagnosticCode::SpecializationForbiddenDecl`, a message containing `'param'`
/// and the param name, and a primary label whose span equals the `ParamDecl`'s span.
///
/// Shape: `structure S { sub scope : Foo { param x } }`
#[test]
fn param_inside_specialization_scope_emits_forbidden_decl_diagnostic() {
    let p_span = param_span();
    let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
        "scope",
        zero_span(),
        vec![make_param("x", p_span)],
    )]);

    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert_eq!(
        diags.len(),
        1,
        "expected exactly one SpecializationForbiddenDecl diagnostic, got: {:#?}",
        diags
    );
    let d = diags[0];
    assert_eq!(d.severity, Severity::Error, "diagnostic must be Error severity");
    assert!(
        d.message.contains("'param'"),
        "message must contain \"'param'\", got: {:?}",
        d.message
    );
    assert!(
        d.message.contains("'x'"),
        "message must contain \"'x'\", got: {:?}",
        d.message
    );
    assert!(!d.labels.is_empty(), "diagnostic must have at least one label");
    assert_eq!(
        d.labels[0].span,
        p_span,
        "primary label span must equal the ParamDecl's span"
    );
}

/// PRD acceptance criterion 2: a `port` declaration directly inside a
/// specialization-scope body must produce exactly one Error diagnostic with
/// `DiagnosticCode::SpecializationForbiddenDecl`, a message containing `'port'`
/// and the port name, and a primary label whose span equals the `PortDecl`'s span.
///
/// Shape: `structure S { sub scope : Foo { port p : SomePort } }`
#[test]
fn port_inside_specialization_scope_emits_forbidden_decl_diagnostic() {
    let p_span = port_span();
    let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
        "scope",
        zero_span(),
        vec![make_port("p", p_span)],
    )]);

    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert_eq!(
        diags.len(),
        1,
        "expected exactly one SpecializationForbiddenDecl diagnostic, got: {:#?}",
        diags
    );
    let d = diags[0];
    assert_eq!(d.severity, Severity::Error, "diagnostic must be Error severity");
    assert!(
        d.message.contains("'port'"),
        "message must contain \"'port'\", got: {:?}",
        d.message
    );
    assert!(
        d.message.contains("'p'"),
        "message must contain \"'p'\", got: {:?}",
        d.message
    );
    assert!(!d.labels.is_empty(), "diagnostic must have at least one label");
    assert_eq!(
        d.labels[0].span,
        p_span,
        "primary label span must equal the PortDecl's span"
    );
}
