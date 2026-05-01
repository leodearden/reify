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
    ConstraintDecl, Declaration, Expr, ExprKind, LetDecl, MatchArmDeclArmDecl,
    MatchArmDeclGroupDecl, MemberDecl, ParsedModule, ParamDecl, PortDecl, StructureDef, SubDecl,
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

/// PRD acceptance criterion 3: a bare `sub` declaration (body=None) directly
/// inside a specialization-scope body must produce exactly one Error diagnostic
/// with `DiagnosticCode::SpecializationForbiddenDecl`, a message containing
/// `'sub'` and the sub name, and a primary label whose span equals the
/// `SubDecl`'s span.
///
/// Shape: `structure S { sub scope : Foo { sub child : Foo } }`
#[test]
fn bare_sub_inside_specialization_scope_emits_forbidden_decl_diagnostic() {
    let s = sub_span();
    let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
        "scope",
        zero_span(),
        vec![make_sub_bare("child", s)],
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
        d.message.contains("'sub'"),
        "message must contain \"'sub'\", got: {:?}",
        d.message
    );
    assert!(
        d.message.contains("'child'"),
        "message must contain \"'child'\", got: {:?}",
        d.message
    );
    assert!(!d.labels.is_empty(), "diagnostic must have at least one label");
    assert_eq!(
        d.labels[0].span,
        s,
        "primary label span must equal the inner SubDecl's span"
    );
}

/// Combination test: all three forbidden kinds in a single specialization scope
/// must each emit an independent diagnostic, in source order (param → port → sub).
///
/// Shape: `structure S { sub scope : Foo { param x; port p : SomePort; sub child : Foo } }`
#[test]
fn all_three_forbidden_kinds_in_same_specialization_scope_each_emit_diagnostic() {
    let p_span = param_span();
    let po_span = port_span();
    let s_span = sub_span();
    let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
        "scope",
        zero_span(),
        vec![
            make_param("x", p_span),
            make_port("p", po_span),
            make_sub_bare("child", s_span),
        ],
    )]);

    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert_eq!(
        diags.len(),
        3,
        "expected exactly three SpecializationForbiddenDecl diagnostics (param + port + sub), got: {:#?}",
        diags
    );

    // First diagnostic: param x
    let d0 = diags[0];
    assert_eq!(d0.severity, Severity::Error);
    assert!(
        d0.message.contains("'param'"),
        "first diagnostic must be for 'param', got: {:?}",
        d0.message
    );
    assert!(
        d0.message.contains("'x'"),
        "first diagnostic must name 'x', got: {:?}",
        d0.message
    );
    assert_eq!(d0.labels[0].span, p_span, "first span must equal param span");

    // Second diagnostic: port p
    let d1 = diags[1];
    assert_eq!(d1.severity, Severity::Error);
    assert!(
        d1.message.contains("'port'"),
        "second diagnostic must be for 'port', got: {:?}",
        d1.message
    );
    assert!(
        d1.message.contains("'p'"),
        "second diagnostic must name 'p', got: {:?}",
        d1.message
    );
    assert_eq!(d1.labels[0].span, po_span, "second span must equal port span");

    // Third diagnostic: sub child
    let d2 = diags[2];
    assert_eq!(d2.severity, Severity::Error);
    assert!(
        d2.message.contains("'sub'"),
        "third diagnostic must be for 'sub', got: {:?}",
        d2.message
    );
    assert!(
        d2.message.contains("'child'"),
        "third diagnostic must name 'child', got: {:?}",
        d2.message
    );
    assert_eq!(d2.labels[0].span, s_span, "third span must equal sub span");
}

/// PRD acceptance criterion 4 (negative test): a specialization-scope body
/// containing only permitted member kinds (`let`, `constraint`) must produce
/// zero `SpecializationForbiddenDecl` diagnostics.
///
/// Pins the converse contract: `Let` and `Constraint` (and any future-permitted
/// variant) must not be misclassified as forbidden.
///
/// Shape: `structure S { sub scope : Foo { let v = true; constraint <expr> } }`
#[test]
fn permitted_only_specialization_scope_body_emits_no_forbidden_decl_diagnostic() {
    let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
        "scope",
        zero_span(),
        vec![make_let("v"), make_constraint()],
    )]);

    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert!(
        diags.is_empty(),
        "let and constraint inside a specialization scope must not produce \
         SpecializationForbiddenDecl diagnostics, got: {:#?}",
        diags
    );
}

/// Nested specialization scope: an inner `sub` with its own body inside an outer
/// specialization scope must produce TWO diagnostics:
///   1. One for the inner Sub itself (forbidden bare-sub in outer scope).
///   2. One for the leaf Param inside the inner Sub's body (forbidden in inner scope).
///
/// Order is outer-first per `walk_members_depth`'s parent-before-children traversal.
/// Locks in the "applies anywhere a specialization scope appears" PRD clause.
///
/// Shape: `structure S { sub outer : Foo { sub inner : Foo { param x } } }`
#[test]
fn forbidden_decl_in_nested_specialization_scope_body_emits_two_diagnostics() {
    let inner_sub_span = sub_span();
    let leaf_param_span = param_span();
    let inner_sub = make_sub_with_body("inner", inner_sub_span, vec![make_param("x", leaf_param_span)]);
    let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
        "outer",
        zero_span(),
        vec![inner_sub],
    )]);

    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert_eq!(
        diags.len(),
        2,
        "expected two SpecializationForbiddenDecl diagnostics (inner sub + leaf param), got: {:#?}",
        diags
    );

    // First diagnostic: the inner Sub itself (forbidden in outer scope).
    let d0 = diags[0];
    assert_eq!(d0.severity, Severity::Error);
    assert!(
        d0.message.contains("'sub'"),
        "first diagnostic must be for 'sub', got: {:?}",
        d0.message
    );
    assert!(
        d0.message.contains("'inner'"),
        "first diagnostic must name 'inner', got: {:?}",
        d0.message
    );
    assert_eq!(
        d0.labels[0].span,
        inner_sub_span,
        "first diagnostic span must equal inner SubDecl's span"
    );

    // Second diagnostic: the leaf Param inside the inner Sub's body.
    let d1 = diags[1];
    assert_eq!(d1.severity, Severity::Error);
    assert!(
        d1.message.contains("'param'"),
        "second diagnostic must be for 'param', got: {:?}",
        d1.message
    );
    assert!(
        d1.message.contains("'x'"),
        "second diagnostic must name 'x', got: {:?}",
        d1.message
    );
    assert_eq!(
        d1.labels[0].span,
        leaf_param_span,
        "second diagnostic span must equal leaf ParamDecl's span"
    );
}

// ── match-arm helpers (step-7) ────────────────────────────────────────────────

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

/// PRD scenario 5 (`match`-arm `sub` bodies with forbidden inside):
/// a `MatchArmDeclGroup` nested inside an outer `sub`-with-body must be walked
/// by `walk_specialization_scope_members`, which recurses into
/// `MemberDecl::MatchArmDeclGroup` arms.  A `sub` arm that itself has a body
/// containing a `param` must produce AT LEAST two diagnostics:
///   - one for the arm's `sub head : HexHead` (forbidden bare-sub in outer scope), and
///   - one for the leaf `param x` inside the arm's sub body (forbidden in inner scope).
///
/// The test uses `iter().any(...)` rather than positional indexing because the
/// exact ordering of diagnostics through `MatchArmDeclGroup` traversal is an
/// internal detail of `walk_members_depth` not pinned by prior tests.
///
/// Shape (nested for validator reachability — see design decision):
/// ```text
/// structure S {
///   sub motor : ElectricMotor {
///     match head_type {
///       Hex => sub head : HexHead { param x }
///     }
///   }
/// }
/// ```
///
/// The `MatchArmDeclGroup` is nested inside an outer `sub motor`-with-body so
/// the validator's top-level walker (`find_specialization_scopes`) reaches it.
/// Placing the match-arm-group at top-level would silently miss the inner
/// forbidden decls (see design decision for rationale).
#[test]
fn forbidden_decl_in_match_arm_sub_body_emits_diagnostic() {
    let arm_sub_span = sub_span();
    let leaf_param_span = param_span();

    // Inner arm: `sub head : HexHead { param x }`
    // The sub has a body → it opens its own specialization scope.
    let arm_sub = make_sub_with_body("head", arm_sub_span, vec![make_param("x", leaf_param_span)]);
    let match_group = make_match_arm_decl_group("head_type", vec![make_match_arm_decl("Hex", arm_sub)]);

    // Outer specialization scope: `sub motor : ElectricMotor { <match_group> }`
    let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
        "motor",
        zero_span(),
        vec![match_group],
    )]);

    let compiled = reify_compiler::compile(&parsed);
    let diags = forbidden_diagnostics(&compiled.diagnostics);

    assert!(
        diags.len() >= 2,
        "expected at least two SpecializationForbiddenDecl diagnostics \
         (arm sub + leaf param), got: {:#?}",
        diags
    );

    // One diagnostic must be for the arm's Sub itself (forbidden in outer scope).
    let has_sub_head = diags.iter().any(|d| {
        d.message.contains("'sub'") && d.message.contains("'head'")
    });
    assert!(
        has_sub_head,
        "expected a diagnostic for forbidden 'sub' named 'head', got: {:#?}",
        diags
    );

    // One diagnostic must be for the leaf Param inside the arm's sub body.
    let has_param_x = diags.iter().any(|d| {
        d.message.contains("'param'") && d.message.contains("'x'")
    });
    assert!(
        has_param_x,
        "expected a diagnostic for forbidden 'param' named 'x', got: {:#?}",
        diags
    );
}
