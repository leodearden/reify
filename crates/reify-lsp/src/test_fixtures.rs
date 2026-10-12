//! Test fixtures and helpers shared by more than one `reify-lsp` module's
//! private `tests` module.
//!
//! Compiled only under `cfg(test)`, so no production code carries this data.
//! Everything here has two or more consumers; a fixture with one consumer
//! belongs in that module's own `tests`. The point of the single home is that
//! the verbatim copies it replaced had to be edited in lockstep on every
//! grammar change.
//!
//! Declared from `analysis.rs` rather than the crate root, so the path is
//! `crate::analysis::test_fixtures`.

use reify_ast::{Declaration, MemberDecl, ParsedModule, SubDecl};
use reify_core::{ModulePath, SourceSpan};

/// One verified-parseable snippet per NAMED `Declaration` variant, paired with
/// the name that variant declares — the shared fixture behind
/// [`crate::analysis::decl_name_and_span`]'s wildcard-free match.
///
/// Consumers, each a per-kind loop over this table except where noted:
/// - `analysis::tests::decl_name_and_span_returns_name_and_span_for_every_named_kind`
/// - `analysis::tests::named_decl_snippets_cover_every_named_kind` — also asserts
///   this table is row-for-row aligned with `OUTLINE_SYMBOL_KIND_BY_NAME`, so a
///   new row needs a per-kind `SymbolKind` verdict too
/// - `analysis::tests::every_named_decl_snippet_yields_one_symbol_agreeing_with_decl_name_and_span`
/// - `analysis::tests::document_symbols_map_every_named_kind_to_its_symbol_kind` — looks
///   rows up BY NAME via `named_decl_snippet`, so renaming a row's declared name
///   panics there rather than silently shrinking its loop
/// - `goto_def::tests::goto_def_cursor_on_declaration_name_resolves_for_every_kind`
/// - `references::cross_file::tests::cross_file_declaration_kind_admission_tracks_use_site_coverage`
///
/// ADDING A ROW is what a newly-named `Declaration` variant needs after its
/// `decl_name_and_span` and `kind_index` arms: the loop consumers then cover it
/// with no edit of their own. `named_decl_snippets_cover_every_named_kind` is
/// what reds if the row is missing.
///
/// Every snippet is lifted (verbatim or near-verbatim) from an existing passing
/// source — `crates/reify-syntax/tests/harness_syntax/*` or
/// `tree-sitter-reify/test/corpus/*` — rather than invented, so a RED assertion
/// can never be doomed by a surface-syntax guess. The `field def` codomain is
/// the one deliberate divergence: the lifted original reads `-> Scalar`, which
/// the workspace-wide `corpus_has_zero_bare_scalar` guard forbids outside its
/// excluded `crates/reify-syntax/tests` dir. Do NOT restore it — that re-reds
/// the guard.
pub(crate) const NAMED_DECL_SNIPPETS: &[(&str, &str)] = &[
    ("structure S { param x : Length = 5mm }", "S"),
    (
        "occurrence def Welding { param method : Length }",
        "Welding",
    ),
    ("enum Dir { In, Out }", "Dir"),
    ("fn id_length(x: Length) -> Length { x }", "id_length"),
    ("trait Rigid { param mass : Mass }", "Rigid"),
    (
        "field def temp : Point3 -> Real { source = analytical { |p| p } }",
        "temp",
    ),
    (
        "purpose lightweight(subject : Structure) { minimize subject.mass }",
        "lightweight",
    ),
    ("constraint def Foo { x > 0 }", "Foo"),
    ("unit meter : Length", "meter"),
    ("type Pressure = Force", "Pressure"),
    (
        "joint ball(c: Point, d: Point) with orientation: Orientation = coincident(c, d)",
        "ball",
    ),
];

/// Byte offsets of every occurrence of `needle` in `source`, ascending.
pub(crate) fn occurrences(source: &str, needle: &str) -> Vec<usize> {
    source.match_indices(needle).map(|(i, _)| i).collect()
}

/// Parse a single-declaration fixture, asserting that it parses clean and
/// yields exactly one declaration.
///
/// Cleanliness is asserted BEFORE the count so grammar drift fails loudly here
/// rather than silently yielding zero declarations — which would turn a
/// caller's per-kind loop, and every negative assertion inside it, into a
/// vacuous pass.
pub(crate) fn parse_one_clean(source: &str, module: &str) -> ParsedModule {
    let parsed = reify_syntax::parse(source, ModulePath::single(module));
    assert!(
        parsed.errors.is_empty(),
        "fixture must parse clean, got {:?} for: {source}",
        parsed.errors
    );
    assert_eq!(
        parsed.declarations.len(),
        1,
        "fixture must hold exactly one declaration: {source}"
    );
    parsed
}

/// Build the name-token span `[start, start + text.len())`.
pub(crate) fn span_of(start: usize, text: &str) -> SourceSpan {
    SourceSpan::new(start as u32, (start + text.len()) as u32)
}

/// Locate the `MemberDecl::Sub` named `sub_name` among the parsed
/// module's top-level structure members. Used to assert directly on an
/// AST field (e.g. `index_domain.is_some()`) instead of on an interim
/// parser diagnostic's wording, which is not the behavior under test and
/// would break for an unrelated reason once that diagnostic is retired.
pub(crate) fn find_sub<'a>(parsed: &'a ParsedModule, sub_name: &str) -> &'a SubDecl {
    parsed
        .declarations
        .iter()
        .find_map(|decl| match decl {
            Declaration::Structure(s) => s.members.iter().find_map(|m| match m {
                MemberDecl::Sub(sub) if sub.name == sub_name => Some(sub),
                _ => None,
            }),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no `sub {sub_name}` found in parsed module"))
}
