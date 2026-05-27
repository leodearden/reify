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

use reify_ast::{Declaration, MAX_MEMBER_NESTING_DEPTH, MemberDecl, ParsedModule, walk_specialization_scope_members};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};

/// Pre-pass entry point: walk every specialization scope in `parsed`.
///
/// Iterates entity-style top-level declarations (Structure, Occurrence,
/// Trait, Purpose) and visits every `MemberDecl::Sub` whose `body.is_some()`
/// — those are the spec §8.7 specialization scopes. Each scope is delegated
/// to [`walk_specialization_scope_members`], which itself recurses into
/// nested specialization scopes and `where { … } else { … }` branches.
///
/// For each member visited inside a specialization scope, if
/// [`forbidden_decl_info`] returns `Some((kind, name, span))`, an
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
        if let Some((kind, name, span)) = forbidden_decl_info(member) {
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

/// Returns `(kind, name, span)` for the three forbidden specialization-scope
/// member variants, or `None` for permitted variants.
///
/// Returns `Some(("param"|"port"|"sub", decl_name, decl_span))` for
/// `MemberDecl::Param`, `::Port`, and `::Sub` (spec §8.7 "Not permitted: New
/// param, port, or sub declarations"). Returns `None` for all other variants
/// (let, constraint, connect, chain, etc.), which are permitted.
///
/// # Load-bearing wildcard
///
/// The explicit `_ => None` arm is intentional. A future `MemberDecl` variant
/// that should be *permitted* must not silently become forbidden because of a
/// missing arm here. The test
/// `validate_module_emits_no_diagnostic_for_permitted_decls_inside_specialization_scope`
/// guards against accidental broadening — it will catch any new arm that
/// erroneously returns `Some`.
fn forbidden_decl_info(member: &MemberDecl) -> Option<(&'static str, &str, SourceSpan)> {
    match member {
        MemberDecl::Param(p) => Some(("param", &p.name, p.span)),
        MemberDecl::Port(p) => Some(("port", &p.name, p.span)),
        MemberDecl::Sub(s) => Some(("sub", &s.name, s.span)),
        // LOAD-BEARING: this wildcard arm must stay `None`. A future
        // MemberDecl variant that should be *permitted* must NOT get an arm
        // returning `Some` here — the test
        // `validate_module_emits_no_diagnostic_for_permitted_decls_inside_specialization_scope`
        // catches any accidental broadening.
        _ => None,
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
    use reify_ast::{GuardedGroupDecl, MemberDecl};
    use reify_test_support::specialization_fixtures::*;
    use reify_core::{Diagnostic, DiagnosticCode, ModulePath, Severity};

    fn parse_module(source: &str) -> ParsedModule {
        reify_syntax::parse(source, ModulePath::single("test"))
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

    // ── suggestion 2a: GuardedGroup inside specialization scope ─────────────

    /// A `GuardedGroup` (`where cond { … } else { … }`) directly inside a
    /// specialization-scope body is recursed into by
    /// `walk_specialization_scope_members`. Forbidden decls in both the
    /// `members` branch and the `else_members` branch must each fire a
    /// diagnostic.
    #[test]
    fn validate_module_emits_diagnostic_for_forbidden_decl_in_guarded_group_inside_specialization_scope()
     {
        let members_param_span = param_span();
        let else_members_port_span = port_span();
        // Structure S {
        //   sub scope : Foo {
        //     where (true) { param x } else { port p : SomePort }
        //   }
        // }
        let guarded = MemberDecl::GuardedGroup(GuardedGroupDecl {
            condition: dummy_expr(),
            members: vec![make_param("x", members_param_span)],
            else_members: vec![make_port("p", else_members_port_span)],
            span: dummy_span(),
            content_hash: dummy_hash(),
        });
        let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
            "scope",
            dummy_span(),
            vec![guarded],
        )]);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        validate_module(&parsed, &mut diagnostics);

        assert_eq!(
            diagnostics.len(),
            2,
            "expected two diagnostics (param in members + port in else_members), got: {diagnostics:?}"
        );
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code == Some(DiagnosticCode::SpecializationForbiddenDecl)),
            "all diagnostics must have code SpecializationForbiddenDecl"
        );
        let spans: Vec<_> = diagnostics.iter().map(|d| d.labels[0].span).collect();
        assert!(
            spans.contains(&members_param_span),
            "members param span must appear in diagnostics"
        );
        assert!(
            spans.contains(&else_members_port_span),
            "else_members port span must appear in diagnostics"
        );
    }

    // ── suggestion 2b: multiple sibling forbidden decls ──────────────────────

    /// All three sibling forbidden decls in the same spec-scope body each
    /// produce their own diagnostic in source order. Pins emission count and
    /// ordering stability.
    #[test]
    fn validate_module_emits_one_diagnostic_per_sibling_forbidden_decl_in_same_body() {
        let p_span = param_span();
        let po_span = port_span();
        let s_span = sub_span();
        // Structure S { sub scope : Foo { param x; port p; sub child : Foo } }
        let parsed = parsed_module_with_structure_members(vec![make_sub_with_body(
            "scope",
            dummy_span(),
            vec![
                make_param("x", p_span),
                make_port("p", po_span),
                make_sub_bare("child", s_span),
            ],
        )]);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        validate_module(&parsed, &mut diagnostics);

        assert_eq!(
            diagnostics.len(),
            3,
            "expected three diagnostics (param + port + sub), got: {diagnostics:?}"
        );
        assert!(diagnostics[0].message.contains("'param'"));
        assert_eq!(diagnostics[0].labels[0].span, p_span);
        assert!(diagnostics[1].message.contains("'port'"));
        assert_eq!(diagnostics[1].labels[0].span, po_span);
        assert!(diagnostics[2].message.contains("'sub'"));
        assert_eq!(diagnostics[2].labels[0].span, s_span);
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
            d0.labels[0].span, inner_sub_span,
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
            d1.labels[0].span, leaf_param_span,
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

        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic, got: {diagnostics:?}"
        );
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
            d.labels[0].span, s_span,
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

        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic, got: {diagnostics:?}"
        );
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
        assert_eq!(
            d.labels[0].span, p_span,
            "primary label span must equal the PortDecl's span"
        );
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

        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic, got: {diagnostics:?}"
        );
        let d = &diagnostics[0];
        assert_eq!(
            d.severity,
            Severity::Error,
            "diagnostic must be Error severity"
        );
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
        assert!(
            !d.labels.is_empty(),
            "diagnostic must have at least one label"
        );
        assert_eq!(
            d.labels[0].span, p_span,
            "primary label span must equal the ParamDecl's span"
        );
        assert!(
            !d.labels[0].message.is_empty(),
            "primary label message must be non-empty"
        );
    }
}
