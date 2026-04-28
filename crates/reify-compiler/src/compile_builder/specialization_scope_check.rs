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
// TODO(task-2369): the visitor will push into `diagnostics`, eliminating the
// need for both allows. The `Vec` (not `&mut [_]`) is the planned signature
// because 2369 needs `push`; we keep it now so the call site in
// `compile_with_prelude_context` doesn't churn when 2369 lands.
#[allow(unused_variables, clippy::ptr_arg)]
pub(crate) fn validate_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
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
        let members: &[MemberDecl] = match decl {
            Declaration::Structure(s) => &s.members,
            Declaration::Occurrence(o) => &o.members,
            Declaration::Trait(t) => &t.members,
            Declaration::Purpose(p) => &p.members,
            // The remaining declaration kinds (Function, Field, Constraint
            // def, Enum, Unit, TypeAlias, Import) cannot host a `sub`
            // declaration and therefore cannot open a specialization scope.
            _ => continue,
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

/// Test-only hook: count the number of times the specialization-scope
/// walker invokes its visitor on `parsed`.
///
/// Mirrors the iteration shape of [`validate_module`] exactly, swapping the
/// no-op visitor for an increment-on-each-call counter. With the current
/// grammar (no `sub a : T { body }` form), `parsed` from `reify_syntax::parse`
/// always yields zero — the test in `tests` below pins that contract.
#[cfg(test)]
fn count_visited_specialization_members(parsed: &ParsedModule) -> usize {
    let mut count = 0usize;
    for_each_specialization_member(parsed, &mut |_m| count += 1);
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_syntax::ParsedModule;
    use reify_types::{Diagnostic, ModulePath};

    fn parse_module(source: &str) -> ParsedModule {
        reify_syntax::parse(source, ModulePath::single("test"))
    }

    #[test]
    fn validate_module_emits_no_diagnostics_on_currently_parseable_module() {
        // The parser today produces only `body: None` SubDecls (the
        // `sub a : T { body }` form awaits a future grammar update). The
        // pre-pass therefore has no specialization-scope bodies to walk
        // and must add zero diagnostics.
        let parsed = parse_module("structure S { sub a = Foo() }");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        validate_module(&parsed, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "validate_module should emit no diagnostics on a currently-parseable module, got: {diagnostics:?}"
        );
    }

    #[test]
    fn validate_module_visits_zero_specialization_scopes_today() {
        // Instrumented testing hook: counts the number of times the
        // walker visitor is called. With every parsed SubDecl having
        // `body: None`, the count must be zero.
        let parsed = parse_module(
            "structure S {
                param x : Scalar = 5mm
                sub a = Foo()
                sub b : List<Bar>
            }",
        );
        let count = count_visited_specialization_members(&parsed);
        assert_eq!(
            count, 0,
            "with the current grammar, no SubDecl has body == Some(_), so the visitor must not be called"
        );
    }
}
