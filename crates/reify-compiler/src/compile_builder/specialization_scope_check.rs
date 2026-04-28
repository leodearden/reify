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
//
// NOTE: `validate_module` is intentionally added in step 11 — step 10's
// inline tests below reference it before it exists, which is the failing
// test that drives step 11's implementation.

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
