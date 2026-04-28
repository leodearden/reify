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
use reify_types::Diagnostic;

/// Pre-pass entry point: walk every specialization scope in `parsed`.
///
/// Iterates entity-style top-level declarations (Structure, Occurrence,
/// Trait, Purpose) and visits every `MemberDecl::Sub` whose `body.is_some()`
/// — those are the spec §8.7 specialization scopes. Each scope is delegated
/// to [`walk_specialization_scope_members`], which itself recurses into
/// nested specialization scopes and `where { … } else { … }` branches.
///
/// This task ships the visitor as a no-op. Task 2369 populates the actual
/// rejection rule (`Param`/`Port`/`Sub` with body inside a specialization
/// scope) and pushes [`reify_types::DiagnosticCode::E_SPECIALIZATION_FORBIDDEN_DECL`]
/// into the supplied `diagnostics` vector.
// TODO(task-2369): the visitor body below will be replaced with diagnostic
// emission that pushes into `diagnostics`. The `Vec` (not `&mut [_]`)
// signature is the planned shape because 2369 needs `push`; we keep it now
// so the call site in `compile_with_prelude_context` doesn't churn.
#[allow(clippy::ptr_arg)]
pub(crate) fn validate_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
    // Deliberately unused this task — see TODO(task-2369) above. Binding to
    // `_` makes the dead-ness self-evident at the call site without the
    // wider `unused_variables` lint allow.
    let _ = diagnostics;
    for_each_specialization_member(parsed, &mut |_member| {
        // Intentionally a no-op until task 2369. The traversal is wired
        // here so the compile pipeline pays exactly one walk; 2369 will
        // replace this body with the rejection-rule logic.
    });
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
