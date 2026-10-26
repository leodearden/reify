//! Syntax-only oracles over a parsed module's declarations: the module name a
//! URI implies, the declaration enclosing an offset, the name a declaration
//! declares and where its name token sits. Read by analysis, goto-def,
//! references and the document outline.

use reify_ast::{Declaration, ParsedModule};
use reify_core::SourceSpan;
use tower_lsp::lsp_types::Url;

use crate::convert::is_ident_byte;

/// Fixtures and helpers shared by several modules' `tests`, in their own file
/// so no production module carries test data. Declared here, next to the
/// `decl_name_and_span` oracle whose per-kind table is the largest of them.
#[cfg(test)]
#[path = "test_fixtures.rs"]
pub(crate) mod test_fixtures;

/// Extract a module name from a file URI.
///
/// e.g., `file:///path/to/test.ri` → `"test"`.
/// Returns `"unnamed"` if the URI has no path segments or the file
/// doesn't end with `.ri`.
pub fn module_name_from_uri(uri: &Url) -> &str {
    uri.path_segments()
        .and_then(|mut segs| segs.next_back())
        .and_then(|name| name.strip_suffix(".ri"))
        .unwrap_or("unnamed")
}

/// Return the declaration whose span contains `offset`,
/// or `None` if the offset is outside all declarations.
///
/// This is a free function that operates on `&[Declaration]` directly, so it can
/// be used by callers that only have a `ParsedModule` (e.g., goto-def) without
/// needing a full `AnalysisContext`.
pub fn enclosing_decl_at(declarations: &[Declaration], offset: usize) -> Option<&Declaration> {
    let offset_u32 = offset as u32;
    for decl in declarations {
        let decl_span = match decl {
            Declaration::Structure(s) => s.span,
            Declaration::Occurrence(o) => o.span,
            Declaration::Import(i) => i.span,
            Declaration::Enum(e) => e.span,
            Declaration::Function(f) => f.span,
            Declaration::Trait(t) => t.span,
            Declaration::Field(f) => f.span,
            Declaration::Purpose(p) => p.span,
            Declaration::Constraint(c) => c.span,
            Declaration::Unit(u) => u.span,
            Declaration::TypeAlias(t) => t.span,
            Declaration::Default(d) => d.span,
            Declaration::Module(m) => m.span,
            // Grammar producer only (task α 4395). Joint bodies are not
            // navigable by the LSP yet; included here so the exhaustive match
            // stays complete. Semantics deferred to task β.
            Declaration::Joint(j) => j.span,
        };
        if offset_u32 >= decl_span.start && offset_u32 < decl_span.end {
            return Some(decl);
        }
    }
    None
}

/// The name a top-level [`Declaration`] declares, paired with the
/// declaration's own statement span — or `None` for the kinds that declare
/// no name of their own.
///
/// The match is deliberately **exhaustive with no `_` wildcard arm**, and that
/// is the load-bearing part of the design: a new `Declaration` variant becomes
/// a COMPILE ERROR here, forcing an explicit named-vs-unnamed decision instead
/// of a silent omission. This scan was the first in the crate to be written
/// that way, after every wildcard-terminated one had silently dropped kinds as
/// the parser grew them; #6972 finished the job, so the two rename/references
/// oracles, `goto_def::decl_name_span_in` and
/// `references::cross_file::classify_decl_name_in`, are exhaustive too and a new variant
/// is a compile error at all three.
///
/// The returned span is the whole declaration statement, NOT the name token —
/// narrow it with [`name_token_span`] when a jump target is wanted.
///
/// CONSUMERS — same-file go-to-definition (`goto_def::resolve_decl_name`, task
/// 6388) and the document outline ([`crate::symbols::compute_document_symbols_from_parsed`],
/// migrated onto this pair by #6972, which is what let that function drop its
/// own wildcard). Both are READ-ONLY surfaces, where admitting a kind too
/// generously costs precision, not correctness.
///
/// The deliberate NON-consumers are the two rename/references oracles,
/// `goto_def::decl_name_span_in` and
/// `references::cross_file::classify_decl_name_in`. They answer a stricter question, and
/// must keep answering it separately: they feed rename, whose edit set is the
/// reference set, so admitting a kind whose USE SITES are not collected
/// produces a rename that moves the declaration and leaves every use stale. A
/// kind belongs there only once every use-site form for it is collected. Since
/// #6539 ten of the eleven named kinds qualify; `Unit` does not, and cannot —
/// its one use form is a literal suffix that carries no span. The argument and
/// the measurement behind it live on the guard test
/// `references::cross_file::tests::cross_file_declaration_kind_admission_tracks_use_site_coverage`.
pub(crate) fn decl_name_and_span(decl: &Declaration) -> Option<(&str, SourceSpan)> {
    let named = match decl {
        Declaration::Structure(s) => (s.name.as_str(), s.span),
        Declaration::Occurrence(o) => (o.name.as_str(), o.span),
        Declaration::Enum(e) => (e.name.as_str(), e.span),
        Declaration::Function(f) => (f.name.as_str(), f.span),
        Declaration::Trait(t) => (t.name.as_str(), t.span),
        Declaration::Field(f) => (f.name.as_str(), f.span),
        Declaration::Purpose(p) => (p.name.as_str(), p.span),
        Declaration::Constraint(c) => (c.name.as_str(), c.span),
        Declaration::Unit(u) => (u.name.as_str(), u.span),
        Declaration::TypeAlias(t) => (t.name.as_str(), t.span),
        Declaration::Joint(j) => (j.name.as_str(), j.span),
        // Binds a path/entity, not a new name — goto-def's cross-file Phase 0
        // owns the cursor-in-import case.
        Declaration::Import(_) => return None,
        // A dotted module path, not a declared name.
        Declaration::Module(_) => return None,
        // Binds an EXISTING type to a value; introduces no new name.
        Declaration::Default(_) => return None,
    };
    Some(named)
}

/// The `structure def`s nested inside `decl`'s body, or an empty slice for a
/// declaration kind that has no nested declaration region.
///
/// WILDCARD-FREE over all 14 `Declaration` variants, for the same reason
/// [`decl_name_and_span`] is: "does this kind carry child DECLARATIONS?" is a
/// per-kind question, and answering it with a `_` arm is how a kind that later
/// grows one gets silently skipped. Today exactly one kind does — a `purpose`,
/// whose `structure def` members are kept out of `PurposeDef.members` and
/// collected into `PurposeDef.structures` (task 4639).
fn nested_structures(decl: &Declaration) -> &[reify_ast::StructureDef] {
    match decl {
        Declaration::Purpose(p) => &p.structures,
        Declaration::Structure(_)
        | Declaration::Occurrence(_)
        | Declaration::Enum(_)
        | Declaration::Function(_)
        | Declaration::Trait(_)
        | Declaration::Field(_)
        | Declaration::Constraint(_)
        | Declaration::Unit(_)
        | Declaration::TypeAlias(_)
        | Declaration::Joint(_)
        | Declaration::Import(_)
        | Declaration::Module(_)
        | Declaration::Default(_) => &[],
    }
}

/// Every name declared by a `structure def` nested inside a `purpose` body,
/// paired with that structure's own statement span — the companion to
/// [`decl_name_and_span`] for the names that are NOT `Declaration`s.
///
/// WHY IT IS SEPARATE rather than folded into [`decl_name_and_span`]: that
/// function's contract is precisely "the name a top-level `Declaration`
/// declares", and its compile-error forcing function is keyed to the 14
/// `Declaration` variants. A purpose-nested structure is not one of them, so
/// folding it in would blur a well-defined purpose and buy nothing the
/// wildcard-free [`nested_structures`] does not already give.
///
/// WHY NAVIGATION TREATS THESE AS FILE-WIDE: the compiler puts the name in the
/// MODULE-LEVEL structure namespace. `compile_builder::pre_pass`'s
/// `Declaration::Purpose` arm registers each through
/// `record_or_report_duplicate(…, "structure")` — so it COLLIDES with a
/// same-named top-level structure — and `compile_builder::entities_phase`
/// compiles it into the same `ctx.templates` as a top-level structure, scoping
/// only AMBIENT-DEFAULT resolution to the purpose. Its one documented
/// limitation, that `structure_refs` omits them so no function-signature
/// skeleton is built, is a skeleton gap rather than a visibility rule.
///
/// SINGLE-LEVEL, by construction and not by choice: the grammar allows
/// `structure_definition` only one level under `purpose_member`, so there is no
/// deeper nesting to recurse into. `compile_builder::entities_phase`'s Purpose
/// arm is the INVARIANT OWNER and carries the same note against `grammar.js`;
/// if the permitted nesting depth ever changes, that arm and this one move
/// together.
///
/// The crate's single source of purpose-nested names: the same-file goto-def
/// scan, the cross-file oracle and the rename classifier all read it, so they
/// cannot drift into three independent descents — which is exactly how the
/// per-kind rot this task exists to undo began.
pub(crate) fn purpose_nested_decl_names(
    parsed: &ParsedModule,
) -> impl Iterator<Item = (&str, SourceSpan)> {
    parsed
        .declarations
        .iter()
        .flat_map(|decl| nested_structures(decl).iter().map(|s| (s.name.as_str(), s.span)))
}

/// Narrow a member-statement span down to the span of just its NAME identifier
/// token.
///
/// Searches forward from `member_span.start`, **bounded by `member_span.end`**,
/// for the first **whole-word** occurrence of `name` (a match whose neighbouring
/// bytes are not identifier characters), returning
/// `SourceSpan::new(name_start, name_start + name.len())`. Falls back to an empty
/// span at `member_span.start` when `name` is not found *within the member span*
/// — so a member whose own name token is unexpectedly absent never borrows a
/// same-named token from a sibling member.
///
/// The declaration name always follows its leading keyword
/// (`param`/`let`/`sub`/`port` for a member, `structure`/`fn`/`enum`/`trait`/
/// `occurrence def` for a top-level declaration), so the first whole-word match
/// is the declaration token. Whole-word matching guards against a longer
/// identifier that merely contains `name` as a substring — a bare substring
/// search finds the `a` of `param` and, worse, the `s` of `structure` before
/// the `s` of `structure s`. The UTF-8 char-boundary snap mirrors
/// `convert::offset_to_position`.
///
/// The crate's single name-token locator; no second implementation of this
/// search exists. Consumers disagree only on how they treat the empty-span
/// fallback, which is what makes that fallback the risky thing to change:
/// `goto_def::decl_name_token` maps it to `None` (a zero-width jump target is
/// useless), `name_selection_range` degrades it to the declaration start, and
/// `references/` propagates it into the rename/references span set.
pub fn name_token_span(source: &str, member_span: SourceSpan, name: &str) -> SourceSpan {
    let mut start = (member_span.start as usize).min(source.len());
    // Snap forward to a valid UTF-8 boundary if we landed mid-character.
    while start < source.len() && !source.is_char_boundary(start) {
        start += 1;
    }
    // Bound the search to the member's OWN span. Without this, a member whose
    // name token is unexpectedly absent (e.g. a malformed/recovered AST node
    // whose span does not actually contain the declared name) would match the
    // first same-named whole word anywhere later in the source — silently
    // borrowing a token from a sibling member. Clamping to `member_span.end`
    // makes such a case fall through to the empty-span fallback instead.
    let end = (member_span.end as usize).min(source.len()).max(start);

    let bytes = source.as_bytes();
    let name_len = name.len();
    for (rel, _) in source[start..].match_indices(name) {
        let abs = start + rel;
        // Past the member span: `match_indices` yields ascending offsets, so once
        // a match would extend beyond `end` every later match does too — stop.
        if abs + name_len > end {
            break;
        }
        // Whole-word check: neither the byte before nor the byte after the match
        // may be an identifier character.
        let prev_ok = abs == 0 || !is_ident_byte(bytes[abs - 1]);
        let next = abs + name_len;
        let next_ok = next >= bytes.len() || !is_ident_byte(bytes[next]);
        if prev_ok && next_ok {
            return SourceSpan::new(abs as u32, (abs + name_len) as u32);
        }
    }

    SourceSpan::empty(member_span.start)
}

#[cfg(test)]
mod tests {
    use super::test_fixtures::{NAMED_DECL_SNIPPETS, parse_one_clean};
    use super::*;
    use reify_core::ModulePath;

    // --- module_name_from_uri tests ---

    #[test]
    fn module_name_from_uri_extracts_test() {
        let uri = Url::parse("file:///test.ri").unwrap();
        assert_eq!(module_name_from_uri(&uri), "test");
    }

    #[test]
    fn module_name_from_uri_nested_path() {
        let uri = Url::parse("file:///path/to/bracket.ri").unwrap();
        assert_eq!(module_name_from_uri(&uri), "bracket");
    }

    #[test]
    fn module_name_from_uri_no_ri_suffix() {
        let uri = Url::parse("file:///test.txt").unwrap();
        assert_eq!(module_name_from_uri(&uri), "unnamed");
    }

    // --- enclosing_decl_at free function tests ---

    #[test]
    fn enclosing_decl_at_returns_structure_for_offset_inside() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        // Offset inside A: 'x' in "param x: Length = 5mm"
        let a_x_offset = source.find("param x").unwrap() + 6;
        let decl = enclosing_decl_at(&parsed.declarations, a_x_offset);
        assert!(decl.is_some(), "offset inside A should return Some");
        match decl.unwrap() {
            reify_ast::Declaration::Structure(s) => assert_eq!(s.name, "A"),
            other => panic!("expected Structure A, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_returns_correct_structure_for_second_decl() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        // Offset inside B: 'y' in "param y: Bool = true"
        let b_y_offset = source.find("param y").unwrap() + 6;
        let decl = enclosing_decl_at(&parsed.declarations, b_y_offset);
        assert!(decl.is_some(), "offset inside B should return Some");
        match decl.unwrap() {
            reify_ast::Declaration::Structure(s) => assert_eq!(s.name, "B"),
            other => panic!("expected Structure B, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_returns_occurrence() {
        let source = "occurrence def Joint {\n    param diameter: Length = 10mm\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let offset = source.find("diameter").unwrap();
        let decl = enclosing_decl_at(&parsed.declarations, offset);
        assert!(
            decl.is_some(),
            "offset inside occurrence should return Some"
        );
        match decl.unwrap() {
            reify_ast::Declaration::Occurrence(o) => assert_eq!(o.name, "Joint"),
            other => panic!("expected Occurrence Joint, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_returns_none_between_declarations() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        // Offset between A and B (the newline between them)
        let between_offset = source.find("\nstructure B").unwrap();
        let decl = enclosing_decl_at(&parsed.declarations, between_offset);
        assert!(
            decl.is_none(),
            "offset between declarations should return None"
        );
    }

    #[test]
    fn enclosing_decl_at_returns_none_for_offset_past_end() {
        let source = "structure A {\n    param x: Length = 5mm\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let decl = enclosing_decl_at(&parsed.declarations, source.len() + 100);
        assert!(decl.is_none(), "offset past end should return None");
    }

    #[test]
    fn enclosing_decl_at_empty_declarations() {
        let decl = enclosing_decl_at(&[], 0);
        assert!(decl.is_none(), "empty declarations should return None");
    }

    // --- decl_name_and_span free function tests (task 6388) ---

    #[test]
    fn decl_name_and_span_returns_name_and_span_for_every_named_kind() {
        for (source, expected_name) in NAMED_DECL_SNIPPETS {
            let parsed = parse_one_clean(source, "test");

            let got = decl_name_and_span(&parsed.declarations[0]);
            let (name, span) = got.unwrap_or_else(|| {
                panic!("decl_name_and_span returned None for named kind, source: {source}")
            });
            assert_eq!(name, *expected_name, "name mismatch for source: {source}");
            assert!(
                span.start < span.end,
                "span must be non-empty for source: {source}, got {span:?}"
            );
            let sliced = &source[span.start as usize..span.end as usize];
            assert!(
                sliced.contains(expected_name),
                "declaration span {span:?} sliced to {sliced:?} must contain \
                 {expected_name:?} for source: {source}"
            );
        }
    }

    #[test]
    fn decl_name_and_span_returns_none_for_unnamed_kinds() {
        // The three variants that declare no name of their own: Import binds a
        // path/entity, Module is a dotted path, Default binds an existing type.
        let unnamed = [
            "import parts.Hole",
            "module a.b.c",
            "default Material = steel",
        ];
        for source in unnamed {
            let parsed = parse_one_clean(source, "test");
            assert!(
                decl_name_and_span(&parsed.declarations[0]).is_none(),
                "unnamed declaration kind must yield None for source: {source}"
            );
        }
    }

    // --- name_token_span tests (step-1) ---
    //
    // `name_token_span(source, member_span, name)` narrows a member-statement
    // span down to the span of just the NAME identifier token. Expected offsets
    // are derived from `source.find` so the assertions stay fixture-robust.

    #[test]
    fn name_token_span_param_width_covers_just_the_name() {
        let source = "param width: Length = 80mm";
        let member_span = SourceSpan::new(0, source.len() as u32);
        let span = name_token_span(source, member_span, "width");
        let start = source.find("width").unwrap() as u32;
        let end = start + "width".len() as u32;
        assert_eq!(span, SourceSpan::new(start, end));
        assert_eq!(&source[span.start as usize..span.end as usize], "width");
    }

    #[test]
    fn name_token_span_let_volume_covers_just_the_name() {
        let source = "let volume = width * height * thickness";
        let member_span = SourceSpan::new(0, source.len() as u32);
        let span = name_token_span(source, member_span, "volume");
        let start = source.find("volume").unwrap() as u32;
        let end = start + "volume".len() as u32;
        assert_eq!(span, SourceSpan::new(start, end));
        assert_eq!(&source[span.start as usize..span.end as usize], "volume");
    }

    #[test]
    fn name_token_span_indented_guarded_param_covers_just_the_name() {
        // A guarded member's statement span begins at the indented `param`
        // keyword. name_token_span must still land on the identifier token,
        // searching forward from the (non-zero) member-span start.
        let source = "        param guarded_x : Length = 5mm";
        let decl_start = source.find("param").unwrap() as u32;
        let member_span = SourceSpan::new(decl_start, source.len() as u32);
        let span = name_token_span(source, member_span, "guarded_x");
        let start = source.find("guarded_x").unwrap() as u32;
        let end = start + "guarded_x".len() as u32;
        assert_eq!(span, SourceSpan::new(start, end));
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "guarded_x"
        );
    }

    #[test]
    fn name_token_span_absent_name_falls_back_within_member_not_sibling() {
        // The member span covers ONLY the first statement (`param alpha …`), which
        // does NOT contain the name `beta`. `beta` appears only in the LATER
        // sibling statement. name_token_span must fall back to an empty span at the
        // member start rather than borrowing the sibling's `beta` token — a
        // malformed/recovered node whose span lacks its own name must not match a
        // same-named token elsewhere in the source.
        let source = "param alpha: Length = 1mm\nlet beta = alpha";
        let member_end = source.find('\n').unwrap() as u32; // end of `param alpha …`
        let member_span = SourceSpan::new(0, member_end);

        let span = name_token_span(source, member_span, "beta");
        assert_eq!(
            span,
            SourceSpan::empty(0),
            "name absent from the member span must fall back to empty, not borrow \
             the sibling `beta` token at {:?}",
            source.find("beta"),
        );
        assert!(span.is_empty(), "fallback span must be empty");
    }

    // --- name_token_span robustness: clamp / UTF-8-snap / start==len ---
    //
    // goto_def had its own name-token locator with a parallel set of these
    // tests; task 7529 collapsed the two locators into this one, so the
    // hardening is pinned here, at the primitive that performs it.

    #[test]
    fn name_token_span_start_beyond_source_len_falls_back_without_panic() {
        // A span start past the end of the source must not panic on the slice.
        let source = "structure Foo { }"; // 17 bytes
        let span = name_token_span(source, SourceSpan::new(100, 120), "Foo");
        assert_eq!(span, SourceSpan::empty(100));
        assert!(span.is_empty(), "out-of-range start must fall back to empty");
    }

    #[test]
    fn name_token_span_start_on_continuation_byte_snaps_forward() {
        // "aéb" = [0x61, 0xC3, 0xA9, 0x62]; byte 2 is the continuation byte 0xA9,
        // so the span start lands mid-character and must snap forward rather
        // than panic on a non-boundary slice.
        let source = "a\u{00E9}b Foo";
        assert!(!source.is_char_boundary(2), "fixture: byte 2 must be mid-char");
        let span = name_token_span(source, SourceSpan::new(2, source.len() as u32), "Foo");
        let start = source.find("Foo").unwrap() as u32;
        assert_eq!(span, SourceSpan::new(start, start + 3));
        assert_eq!(&source[span.start as usize..span.end as usize], "Foo");
    }

    #[test]
    fn name_token_span_start_exactly_source_len_falls_back_without_panic() {
        // An empty trailing slice must not panic.
        let source = "structure Foo { }";
        let len = source.len() as u32;
        let span = name_token_span(source, SourceSpan::new(len, len), "Foo");
        assert_eq!(span, SourceSpan::empty(len));
        assert!(span.is_empty(), "start == source.len() must fall back to empty");
    }
}
