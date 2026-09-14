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

use reify_ast::{
    Declaration, MAX_MEMBER_NESTING_DEPTH, MemberDecl, ParsedModule,
    walk_specialization_scope_members,
};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};

/// Pre-pass entry point: walk every specialization scope in `parsed`.
///
/// Iterates entity-style top-level declarations (Structure, Occurrence,
/// Trait, Purpose) and hands every `MemberDecl::Sub` to
/// [`walk_specialization_scope_members`], which owns the decision of whether
/// that sub opens a spec §8.7 specialization scope at all. It itself recurses
/// into nested specialization scopes and `where { … } else { … }` branches.
///
/// For each member visited inside a specialization scope, if
/// [`forbidden_decl_info`] returns `Some((kind, name, span))`, an
/// [`DiagnosticCode::SpecializationForbiddenDecl`] error is pushed.
pub(crate) fn validate_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
    for_each_specialization_member(parsed, &mut |member| {
        // # Traversal ordering
        //
        // `for_each_specialization_member` delegates each sub to
        // `walk_specialization_scope_members` (reify-ast), which uses a
        // parent-before-children depth-first traversal (`walk_members`).
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
/// each `MemberDecl::Sub`, [`walk_specialization_scope_members`] is invoked
/// with `visitor`; it is a no-op for a sub that opens no scope.
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
            | Declaration::Import(_)
            | Declaration::Module(_)
            | Declaration::Default(_)
            // Grammar producer only (task α 4395). Joint bodies are Vec<Expr>,
            // not Vec<MemberDecl>, so they cannot host a specialization scope.
            // Semantics deferred to task β.
            | Declaration::Joint(_) => continue,
        };
        find_specialization_scopes(members, visitor, 0);
    }
}

/// Recursively scan a member list for every `MemberDecl::Sub`, invoking
/// [`walk_specialization_scope_members`] on each one.
///
/// The scope-root decision is deliberately NOT made here: every
/// `MemberDecl::Sub` goes to [`walk_specialization_scope_members`], which owns
/// it. This function used to re-derive it as `body.is_some()` — false by
/// construction for the keyed form, which is how a keyed sub stayed invisible
/// to this pass until task 6958. Dropping that guard is observably identical
/// for every non-keyed shape: a sub with no overrides previously fell through
/// to `_ => {}` and now reaches a walker that visits nothing.
///
/// We descend into `MemberDecl::GuardedGroup.{members, else_members}` so a
/// specialization scope that lives inside a top-level
/// `where { … } else { … }` is still discovered (spec §6.4 +
/// shadow_lint.rs:39-43 — guarded-group branches are siblings in the
/// enclosing scope).
///
/// We do NOT descend into a sub's overrides here — that is the job of
/// [`walk_specialization_scope_members`] itself (which recurses through nested
/// specialization scopes and inner guarded groups under the same depth bound).
/// Splitting the responsibility keeps the outer "find scope roots" pass
/// distinct from the inner "walk a scope's members" pass.
fn find_specialization_scopes<F>(members: &[MemberDecl], visitor: &mut F, depth: usize)
where
    F: FnMut(&MemberDecl),
{
    if depth > MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    for member in members {
        match member {
            MemberDecl::Sub(s) => {
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
    use reify_core::{Diagnostic, DiagnosticCode, ModulePath, Severity};
    use reify_test_support::specialization_fixtures::*;

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
                param x : Length = 5mm
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

    // ── task 6958: a keyed entry's overrides IS a specialization scope ───────
    //
    // These use REAL `.ri` source through `parse_module` rather than hand-built
    // AST. The older tests above had no choice — they predate the
    // `sub … { body }` grammar. The keyed grammar exists and parses cleanly
    // today, so the end-to-end path is available and is the stronger signal: it
    // also pins that `lower_sub` really routes these members into
    // `keyed_members[].overrides` rather than somewhere a hand-built fixture
    // merely asserts.

    /// Parse `source` (asserting it is error-free) and run `validate_module`.
    ///
    /// Hands back the module too, so a test can read real decl spans out of the
    /// AST instead of hardcoding byte offsets that any edit to the fixture
    /// string would silently invalidate.
    fn parse_and_validate(source: &str) -> (ParsedModule, Vec<Diagnostic>) {
        let parsed = parse_module(source);
        assert!(
            parsed.errors.is_empty(),
            "fixture must parse cleanly, got {:?} for source:\n{source}",
            parsed.errors
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        validate_module(&parsed, &mut diagnostics);
        (parsed, diagnostics)
    }

    /// The keyed entries of the single top-level `sub` in the first structure,
    /// asserting the keyed lowering on the way through.
    ///
    /// NON-VACUITY guard shared by the keyed tests below: a decl the parser
    /// hoisted into `SubDecl.body` — or up to the structure's top level — would
    /// be reported by the pre-existing body-form path, so it would discriminate
    /// nothing.
    fn keyed_entries(parsed: &ParsedModule) -> &[reify_ast::KeyedSubMemberEntry] {
        let members: &[MemberDecl] = match &parsed.declarations[0] {
            Declaration::Structure(s) => &s.members,
            other => panic!("expected a Structure declaration, got {other:?}"),
        };
        let subs: Vec<_> = members
            .iter()
            .filter_map(|m| match m {
                MemberDecl::Sub(sub) => Some(sub),
                _ => None,
            })
            .collect();
        let sub = match subs.as_slice() {
            [sub] => *sub,
            other => panic!("fixture must contain exactly one top-level Sub, got {other:#?}"),
        };
        assert!(
            sub.body.is_none(),
            "the keyed form must lower with `body: None`, got {:#?}",
            sub.body
        );
        assert!(
            !sub.keyed_members.is_empty(),
            "fixture must lower to at least one keyed entry, got {sub:#?}"
        );
        &sub.keyed_members
    }

    /// The span an AST node reports for itself.
    ///
    /// Read straight off the decl rather than through `forbidden_decl_info`, so
    /// the span assertions below check the diagnostic against the AST rather
    /// than against the very function that produced it.
    fn declared_span(member: &MemberDecl) -> SourceSpan {
        match member {
            MemberDecl::Param(p) => p.span,
            MemberDecl::Port(p) => p.span,
            MemberDecl::Sub(s) => s.span,
            other => panic!("fixture member carries no forbidden-decl span: {other:#?}"),
        }
    }

    /// The single override member of the single keyed entry.
    fn only_override(parsed: &ParsedModule) -> &MemberDecl {
        match keyed_entries(parsed) {
            [entry] => match entry.overrides.as_slice() {
                [member] => member,
                other => panic!("expected exactly one override member, got {other:#?}"),
            },
            other => panic!("expected exactly one keyed entry, got {other:#?}"),
        }
    }

    /// `param`, `port`, and `sub` inside a keyed entry's overrides each fire
    /// exactly one `SpecializationForbiddenDecl` Error naming the kind and the
    /// decl, with the primary label on the decl's own span.
    ///
    /// One table rather than three near-identical tests: the assertions are
    /// identical per kind, and only the kind/name/source triple varies.
    #[test]
    fn validate_module_reports_each_forbidden_decl_kind_inside_a_keyed_entry() {
        for (kind, name, decl) in [
            ("param", "q", "param q : Real = 1"),
            ("port", "q", "port q : MyPort"),
            ("sub", "c", "sub c : Bar"),
        ] {
            let source = format!("structure S {{ sub p : Foo {{ \"a\" => {{ {decl} }} }} }}");
            let (parsed, diagnostics) = parse_and_validate(&source);
            let declared = only_override(&parsed);

            assert_eq!(
                diagnostics.len(),
                1,
                "expected exactly one diagnostic for `{decl}` in a keyed entry, got: {diagnostics:?}"
            );
            let d = &diagnostics[0];
            assert_eq!(d.severity, Severity::Error);
            assert_eq!(d.code, Some(DiagnosticCode::SpecializationForbiddenDecl));
            assert!(
                d.message.contains(&format!("'{kind}'")),
                "message must name the kind '{kind}', got: {:?}",
                d.message
            );
            assert!(
                d.message.contains(&format!("'{name}'")),
                "message must name the decl '{name}', got: {:?}",
                d.message
            );
            assert!(!d.labels.is_empty());
            assert_eq!(
                d.labels[0].span,
                declared_span(declared),
                "primary label span must equal the decl's own span for `{decl}`"
            );
        }
    }

    /// A keyed sub with TWO entries, each holding a forbidden decl, fires TWO
    /// diagnostics — one per entry. A first-entry-only fix fails here.
    #[test]
    fn validate_module_reports_a_forbidden_decl_in_every_keyed_entry() {
        let source = r#"
structure S {
    sub p : Foo {
        "a" => { param q : Real = 1 }
        "b" => { param r : Real = 2 }
    }
}
"#;
        let (parsed, diagnostics) = parse_and_validate(source);
        assert_eq!(
            keyed_entries(&parsed).len(),
            2,
            "fixture must lower to two keyed entries"
        );
        assert_eq!(
            diagnostics.len(),
            2,
            "expected one diagnostic per keyed entry, got: {diagnostics:?}"
        );
        assert!(
            diagnostics[0].message.contains("'q'"),
            "first diagnostic must name the first entry's param, got: {:?}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[1].message.contains("'r'"),
            "second diagnostic must name the second entry's param, got: {:?}",
            diagnostics[1].message
        );
    }

    /// A `where … { … } else { … }` guarded group inside a keyed entry is
    /// recursed into — guarded branches are unconditional in every recursion
    /// set, so both branches report.
    #[test]
    fn validate_module_reports_forbidden_decls_inside_a_guarded_group_in_a_keyed_entry() {
        let source = r#"
structure S {
    param flag : Real = 1
    sub p : Foo {
        "a" => {
            where flag > 0 { param q : Real = 1 } else { port r : MyPort }
        }
    }
}
"#;
        let (parsed, diagnostics) = parse_and_validate(source);
        assert!(
            matches!(only_override(&parsed), MemberDecl::GuardedGroup(_)),
            "NON-VACUITY: the entry's single override must be the guarded group itself"
        );
        assert_eq!(
            diagnostics.len(),
            2,
            "expected the then-branch param AND the else-branch port, got: {diagnostics:?}"
        );
        let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(
            messages
                .iter()
                .any(|m| m.contains("'param'") && m.contains("'q'")),
            "then-branch param must report, got {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("'port'") && m.contains("'r'")),
            "else-branch port must report, got {messages:?}"
        );
    }

    /// `let` and `constraint` inside a keyed entry are PERMITTED (spec §8.7) and
    /// fire nothing — the widening changes WHERE forbidden decls are looked for,
    /// never WHICH kinds are forbidden.
    #[test]
    fn validate_module_reports_nothing_for_permitted_decls_inside_a_keyed_entry() {
        let source = r#"
structure S {
    param t : Real = 1
    sub p : Foo {
        "a" => {
            let y = 1
            constraint t > 0
        }
    }
}
"#;
        let (parsed, diagnostics) = parse_and_validate(source);
        let overrides = match keyed_entries(&parsed) {
            [entry] => &entry.overrides,
            other => panic!("expected exactly one keyed entry, got {other:#?}"),
        };
        assert_eq!(
            overrides.len(),
            2,
            "NON-VACUITY: both permitted members must really be in the entry's overrides, \
             got {overrides:#?}"
        );
        assert!(
            diagnostics.is_empty(),
            "let and constraint are permitted in a specialization scope, got: {diagnostics:?}"
        );
    }

    /// The shape every shipped `.ri` keyed sub actually uses — a per-key param
    /// ASSIGNMENT — fires nothing.
    ///
    /// `lower_sub` routes `"intake" => { area = 5mm }` to the entry's
    /// `param_overrides`, never to a `MemberDecl::Param`, and §8.7 lists
    /// parameter assignments as permitted. This is the assertion that keeps the
    /// widening from reddening `examples/keyed_vents.ri` and its siblings.
    #[test]
    fn validate_module_reports_nothing_for_a_keyed_param_assignment() {
        let source = r#"structure S { sub p : Foo { "intake" => { area = 5mm } } }"#;
        let (parsed, diagnostics) = parse_and_validate(source);
        let entry = match keyed_entries(&parsed) {
            [entry] => entry,
            other => panic!("expected exactly one keyed entry, got {other:#?}"),
        };
        // NON-VACUITY in both directions: the assignment really landed in
        // `param_overrides`, and really produced no `MemberDecl` at all — so
        // "zero diagnostics" reports the routing, not an empty fixture.
        assert_eq!(
            entry.param_overrides.len(),
            1,
            "the `area = 5mm` assignment must lower to param_overrides, got {entry:#?}"
        );
        assert!(
            entry.overrides.is_empty(),
            "a param ASSIGNMENT must produce no MemberDecl, got {:#?}",
            entry.overrides
        );
        assert!(
            diagnostics.is_empty(),
            "a per-key param assignment is permitted (spec §8.7), got: {diagnostics:?}"
        );
    }

    /// A keyed sub NESTED inside a body-form scope reports the inner `sub`
    /// itself AND the forbidden decl inside its keyed overrides.
    ///
    /// Green since `walk_members`' recursive `Sub` arm was widened; pinned here
    /// so the whole keyed story is asserted in one place.
    #[test]
    fn validate_module_reports_through_a_keyed_sub_nested_in_a_body_form_scope() {
        let source = r#"
structure S {
    sub outer : Foo {
        sub inner : Bar {
            "a" => { param q : Real = 1 }
        }
    }
}
"#;
        let (_, diagnostics) = parse_and_validate(source);
        assert_eq!(
            diagnostics.len(),
            2,
            "expected the inner `sub` AND the param inside its keyed overrides, got: {diagnostics:?}"
        );
        assert!(
            diagnostics[0].message.contains("'sub'") && diagnostics[0].message.contains("'inner'"),
            "first diagnostic must be the inner sub, got: {:?}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[1].message.contains("'param'") && diagnostics[1].message.contains("'q'"),
            "second diagnostic must be the leaf param, got: {:?}",
            diagnostics[1].message
        );
    }

    /// A nested `sub c : Bar { param q }` written INSIDE a keyed entry reports
    /// two diagnostics, outer-first — the same parent-before-children ordering
    /// the body-form nested test pins.
    #[test]
    fn validate_module_reports_both_levels_of_a_sub_nested_inside_a_keyed_entry() {
        let source = r#"
structure S {
    sub p : Foo {
        "a" => {
            sub c : Bar { param q : Real = 1 }
        }
    }
}
"#;
        let (_, diagnostics) = parse_and_validate(source);
        assert_eq!(
            diagnostics.len(),
            2,
            "expected the nested `sub` AND its leaf param, got: {diagnostics:?}"
        );
        assert!(
            diagnostics[0].message.contains("'sub'") && diagnostics[0].message.contains("'c'"),
            "first diagnostic must be the nested sub (outer-first), got: {:?}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[1].message.contains("'param'") && diagnostics[1].message.contains("'q'"),
            "second diagnostic must be the leaf param, got: {:?}",
            diagnostics[1].message
        );
    }
}
