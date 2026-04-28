//! Specialization-scope structural check (spec §8.7).
//!
//! Pre-pass that walks every specialization-scope body in the parsed AST
//! and gives downstream rules a single place to inspect them. This task
//! (2368) ships the wiring with a no-op visitor; task 2369 populates the
//! `Param`/`Port`/`Sub` rejection rule and the `E_SPECIALIZATION_FORBIDDEN_DECL`
//! diagnostic.
//!
//! Mirrors the signature shape of `dot_chain_lint::lint_module` and
//! `shadow_lint::lint_module` (both in this directory) so the call site
//! in `compile_with_prelude_context` is uniform.

use reify_syntax::{
    Declaration, MAX_MEMBER_NESTING_DEPTH, MemberDecl, ParsedModule,
    walk_specialization_scope_members,
};
use reify_types::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};

/// Pre-pass entry point: walk every specialization scope in `parsed`.
///
/// Iterates entity-style top-level declarations (Structure, Occurrence,
/// Trait, Purpose) and visits every `MemberDecl::Sub` whose `body.is_some()`
/// — those are the spec §8.7 specialization scopes. Each scope is delegated
/// to [`walk_specialization_scope_members`], which itself recurses into
/// nested specialization scopes and `where { … } else { … }` branches.
///
/// For each member visited inside a specialization scope, if
/// [`forbidden_kind_name`] returns `Some(kind)`, an
/// [`DiagnosticCode::SpecializationForbiddenDecl`] error is pushed.
pub(crate) fn validate_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
    for_each_specialization_member(parsed, &mut |member| {
        // # Traversal ordering
        //
        // `for_each_specialization_member` delegates each scope's body to
        // `walk_specialization_scope_members` (from reify-syntax), which uses
        // a parent-before-children depth-first traversal (`walk_members_depth`).
        // For nested specialization scopes (`sub outer { sub inner { param x } }`):
        //   1. The visitor fires on `inner` (the MemberDecl::Sub) first.
        //   2. The walker then recurses into `inner`'s body and fires on `x`.
        // One diagnostic is emitted per forbidden decl visited, regardless of
        // nesting depth. The test
        // `validate_module_emits_diagnostic_for_each_forbidden_decl_in_nested_specialization_scope`
        // pins this two-diagnostic, outer-first ordering.
        //
        // # Wording pin
        //
        // The format string below is pinned by
        // `validate_module_diagnostic_message_format_is_pinned` in this file's
        // inline tests. The label message `"forbidden in specialization scope"` is
        // pinned by the same test. Any drift between these literals and the test's
        // expectations will surface as a test failure — do not change one without
        // updating the other.
        //
        // - Diagnostic message: `"'<kind>' declaration '<name>' is not permitted in a specialization scope (spec §8.7)"`
        // - Label message: `"forbidden in specialization scope"`
        if let Some(kind) = forbidden_kind_name(member) {
            let name = member_name(member);
            let span = member_span(member);
            diagnostics.push(
                Diagnostic::error(format!(
                    "'{kind}' declaration '{name}' is not permitted in a specialization scope (spec §8.7)"
                ))
                .with_code(DiagnosticCode::SpecializationForbiddenDecl)
                .with_label(DiagnosticLabel::new(span, "forbidden in specialization scope")),
            );
        }
    });
}

/// Returns the kind name string for forbidden specialization-scope member kinds,
/// or `None` for permitted kinds.
///
/// Returns `Some("param")`, `Some("port")`, or `Some("sub")` for the three
/// forbidden variants (spec §8.7 "Not permitted: New param, port, or sub
/// declarations"). Returns `None` for all other variants (let, constraint,
/// connect, chain, etc.), which are permitted inside a specialization scope.
///
/// # Load-bearing wildcard
///
/// The explicit `_ => None` arm is intentional. A future `MemberDecl` variant
/// that should be *permitted* must not silently become forbidden because of a
/// missing arm here. The test `validate_module_emits_no_diagnostic_for_permitted_decls_inside_specialization_scope`
/// guards against accidental broadening — it will catch any new arm that
/// erroneously returns `Some`.
fn forbidden_kind_name(member: &MemberDecl) -> Option<&'static str> {
    match member {
        MemberDecl::Param(_) => Some("param"),
        MemberDecl::Port(_) => Some("port"),
        MemberDecl::Sub(_) => Some("sub"),
        // LOAD-BEARING: this wildcard arm must stay `None`. A future
        // MemberDecl variant that should be *permitted* must NOT get an arm
        // returning `Some` here — the test
        // `validate_module_emits_no_diagnostic_for_permitted_decls_inside_specialization_scope`
        // catches any accidental broadening.
        _ => None,
    }
}

/// Returns the name of the declaration (used in the diagnostic message).
fn member_name(member: &MemberDecl) -> &str {
    match member {
        MemberDecl::Param(p) => &p.name,
        MemberDecl::Port(p) => &p.name,
        MemberDecl::Sub(s) => &s.name,
        // Only Param/Port/Sub are forbidden; other arms are unreachable here
        // (forbidden_kind_name returns None for them). Provide a fallback to
        // keep the match exhaustive.
        _ => "<unknown>",
    }
}

/// Returns the source span of the declaration (used as the primary label span).
fn member_span(member: &MemberDecl) -> SourceSpan {
    match member {
        MemberDecl::Param(p) => p.span,
        MemberDecl::Port(p) => p.span,
        MemberDecl::Sub(s) => s.span,
        // Fallback for completeness (forbidden_kind_name returns None for these).
        _ => SourceSpan::empty(0),
    }
}

/// Iterate every member visited by the specialization-scope walker across
/// the whole module.
///
/// Walks the entity-body member lists of the four declaration kinds that
/// can host specialization scopes (Structure / Occurrence / Trait /
/// Purpose), descending into top-level `where { … } else { … }` branches
/// to find specialization scopes that live inside a guarded group. For
/// each `MemberDecl::Sub` whose `body.is_some()`,
/// [`walk_specialization_scope_members`] is invoked with `visitor`.
///
/// Recursion is bounded by [`MAX_MEMBER_NESTING_DEPTH`] to mirror the
/// convention used elsewhere in the compiler (`shadow_lint`,
/// `find_named_member_span`) and to keep pathological fuzzer inputs from
/// blowing the stack.
fn for_each_specialization_member<F>(parsed: &ParsedModule, visitor: &mut F)
where
    F: FnMut(&MemberDecl),
{
    for decl in &parsed.declarations {
        // Exhaustive match (no `_ =>`) — if a future declaration kind grows
        // a `Vec<MemberDecl>` body, the compiler will force a deliberate
        // decision here instead of silently skipping the new variant.
        let members: &[MemberDecl] = match decl {
            Declaration::Structure(s) => &s.members,
            Declaration::Occurrence(o) => &o.members,
            Declaration::Trait(t) => &t.members,
            Declaration::Purpose(p) => &p.members,
            // The remaining declaration kinds cannot host a `MemberDecl::Sub`
            // today: their bodies (if any) are typed as `FnBody`,
            // `FieldSource`, `Vec<Expr>` predicates, etc. — not
            // `Vec<MemberDecl>`. Therefore none of them can open a
            // specialization scope.
            Declaration::Function(_)
            | Declaration::Field(_)
            | Declaration::Constraint(_)
            | Declaration::Enum(_)
            | Declaration::Unit(_)
            | Declaration::TypeAlias(_)
            | Declaration::Import(_) => continue,
        };
        find_specialization_scopes(members, visitor, 0);
    }
}

/// Recursively scan a member list for `MemberDecl::Sub` with `body.is_some()`,
/// invoking [`walk_specialization_scope_members`] on each one.
///
/// We descend into `MemberDecl::GuardedGroup.{members, else_members}` so a
/// specialization scope that lives inside a top-level
/// `where { … } else { … }` is still discovered (spec §6.4 +
/// shadow_lint.rs:39-43 — guarded-group branches are siblings in the
/// enclosing scope).
///
/// We do NOT descend into `MemberDecl::Sub.body` here — that is the job of
/// [`walk_specialization_scope_members`] itself (which recurses through
/// nested specialization scopes and inner guarded groups under the same
/// depth bound). Splitting the responsibility keeps the outer "find scope
/// roots" pass distinct from the inner "walk a scope's members" pass.
fn find_specialization_scopes<F>(members: &[MemberDecl], visitor: &mut F, depth: usize)
where
    F: FnMut(&MemberDecl),
{
    if depth > MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    for member in members {
        match member {
            MemberDecl::Sub(s) if s.body.is_some() => {
                walk_specialization_scope_members(s, visitor);
            }
            MemberDecl::GuardedGroup(g) => {
                find_specialization_scopes(&g.members, visitor, depth + 1);
                find_specialization_scopes(&g.else_members, visitor, depth + 1);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_syntax::{
        ConstraintDecl, Declaration, Expr, ExprKind, LetDecl, MemberDecl, ParamDecl, PortDecl,
        SubDecl, StructureDef,
    };
    use reify_types::{
        ContentHash, Diagnostic, DiagnosticCode, ModulePath, PortDirection, Severity, SourceSpan,
        SpannedIdent,
    };

    fn parse_module(source: &str) -> ParsedModule {
        reify_syntax::parse(source, ModulePath::single("test"))
    }

    // ── AST helpers ──────────────────────────────────────────────────────────

    fn dummy_span() -> SourceSpan {
        SourceSpan::new(10, 20)
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

    fn dummy_hash() -> ContentHash {
        ContentHash(0)
    }

    fn dummy_expr() -> Expr {
        Expr {
            kind: ExprKind::BoolLiteral(true),
            span: dummy_span(),
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
            span: dummy_span(),
            content_hash: dummy_hash(),
        })
    }

    fn make_constraint() -> MemberDecl {
        MemberDecl::Constraint(ConstraintDecl {
            label: None,
            expr: dummy_expr(),
            where_clause: None,
            span: dummy_span(),
            content_hash: dummy_hash(),
        })
    }

    /// Build a ParsedModule with a single Structure whose top-level members
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
                span: dummy_span(),
                content_hash: dummy_hash(),
                pragmas: Vec::new(),
                annotations: Vec::new(),
            })],
            errors: Vec::new(),
            content_hash: dummy_hash(),
            pragmas: Vec::new(),
        }
    }

    // ── existing regression test ─────────────────────────────────────────────

    #[test]
    fn validate_module_emits_no_diagnostics_on_currently_parseable_module() {
        // The parser today produces only `body: None` SubDecls (the
        // `sub a : T { body }` form awaits a future grammar update). The
        // pre-pass therefore has no specialization-scope bodies to walk
        // and must add zero diagnostics. This single assertion covers the
        // contract: with no body=Some, the visitor is never invoked, and
        // therefore no diagnostics fire.
        let parsed = parse_module(
            "structure S {
                param x : Scalar = 5mm
                sub a = Foo()
                sub b : List<Bar>
            }",
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        validate_module(&parsed, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "validate_module should emit no diagnostics on a currently-parseable module, got: {diagnostics:?}"
        );
    }

    // ── step-11: nested specialization scope ─────────────────────────────────

    /// An inner `sub` with its own body (nested specialization scope) inside an
    /// outer specialization scope must produce TWO diagnostics:
    ///   1. One for the inner Sub itself (forbidden `sub` declaration).
    ///   2. One for the leaf Param inside the inner Sub's body.
    ///
    /// Order is outer-first per `walk_members_depth`'s parent-before-children
    /// traversal. Locks in the "applies anywhere a specialization scope appears"
    /// PRD clause.
    #[test]
    fn validate_module_emits_diagnostic_for_each_forbidden_decl_in_nested_specialization_scope() {
        let inner_sub_span = sub_span();
        let leaf_param_span = param_span();
        // Structure S { sub outer : Foo { sub inner : Foo { param x } } }
        let inner_sub = make_sub_with_body(
            "inner",
            inner_sub_span,
            vec![make_param("x", leaf_param_span)],
        );
        let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
            "outer",
            dummy_span(),
            vec![inner_sub],
        )]);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        validate_module(&parsed, &mut diagnostics);

        assert_eq!(
            diagnostics.len(),
            2,
            "expected two diagnostics (inner Sub + leaf Param), got: {diagnostics:?}"
        );

        // First diagnostic: the inner Sub itself
        let d0 = &diagnostics[0];
        assert_eq!(d0.severity, Severity::Error);
        assert_eq!(d0.code, Some(DiagnosticCode::SpecializationForbiddenDecl));
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

        // Second diagnostic: the leaf Param inside the inner Sub's body
        let d1 = &diagnostics[1];
        assert_eq!(d1.severity, Severity::Error);
        assert_eq!(d1.code, Some(DiagnosticCode::SpecializationForbiddenDecl));
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

    // ── step-9: permitted decls must not fire ────────────────────────────────

    /// `let` and `constraint` declarations inside a specialization-scope body
    /// must produce zero diagnostics. Pins the converse of design decision #5:
    /// only param/port/sub fire — let/constraint/connect/etc. are permitted.
    ///
    /// This test exists to guard against a future change that accidentally broadens
    /// `forbidden_kind_name` (e.g., catching `Let` or `Constraint`). With step-8's
    /// impl in place, this test passes immediately.
    #[test]
    fn validate_module_emits_no_diagnostic_for_permitted_decls_inside_specialization_scope() {
        let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
            "scope",
            dummy_span(),
            vec![make_let("v"), make_constraint()],
        )]);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        validate_module(&parsed, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "let and constraint inside a specialization scope must not fire diagnostics, got: {diagnostics:?}"
        );
    }

    // ── step-7: bare Sub inside specialization scope ─────────────────────────

    /// A bare `sub` declaration (body=None) inside a specialization-scope body must
    /// produce exactly one Error diagnostic with code=SpecializationForbiddenDecl,
    /// a message containing `'sub'` and the decl name, and a label whose span
    /// equals the SubDecl's span.
    ///
    /// Mirrors PRD acceptance criterion 3: `sub motor : ElectricMotor { sub child : Foo }`.
    #[test]
    fn validate_module_emits_forbidden_decl_diagnostic_for_bare_sub_inside_specialization_scope() {
        let s_span = sub_span();
        let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
            "scope",
            dummy_span(),
            vec![make_sub_bare("child", s_span)],
        )]);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        validate_module(&parsed, &mut diagnostics);

        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic, got: {diagnostics:?}");
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::SpecializationForbiddenDecl));
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
        assert!(!d.labels.is_empty());
        assert_eq!(
            d.labels[0].span,
            s_span,
            "primary label span must equal the SubDecl's span"
        );
    }

    // ── step-5: Port inside specialization scope ─────────────────────────────

    /// A `port` declaration directly inside a specialization-scope body must
    /// produce exactly one Error diagnostic with code=SpecializationForbiddenDecl,
    /// a message containing `'port'` and the decl name, and a label whose span
    /// equals the PortDecl's span.
    #[test]
    fn validate_module_emits_forbidden_decl_diagnostic_for_port_inside_specialization_scope() {
        let p_span = port_span();
        let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
            "scope",
            dummy_span(),
            vec![make_port("p", p_span)],
        )]);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        validate_module(&parsed, &mut diagnostics);

        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic, got: {diagnostics:?}");
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::SpecializationForbiddenDecl));
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
        assert!(!d.labels.is_empty());
        assert_eq!(d.labels[0].span, p_span, "primary label span must equal the PortDecl's span");
    }

    // ── step-3: Param inside specialization scope ────────────────────────────

    /// A `param` declaration directly inside a specialization-scope body must
    /// produce exactly one Error diagnostic with code=SpecializationForbiddenDecl,
    /// a message containing `'param'` and the decl name, and a label whose span
    /// equals the ParamDecl's span.
    #[test]
    fn validate_module_emits_forbidden_decl_diagnostic_for_param_inside_specialization_scope() {
        let p_span = param_span();
        // Structure S { sub scope : Foo { param x } }  (hand-built)
        let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
            "scope",
            dummy_span(),
            vec![make_param("x", p_span)],
        )]);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        validate_module(&parsed, &mut diagnostics);

        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic, got: {diagnostics:?}");
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error, "diagnostic must be Error severity");
        assert_eq!(
            d.code,
            Some(DiagnosticCode::SpecializationForbiddenDecl),
            "code must be SpecializationForbiddenDecl"
        );
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
        assert!(
            !d.labels[0].message.is_empty(),
            "primary label message must be non-empty"
        );
    }
}
