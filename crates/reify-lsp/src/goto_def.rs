use reify_ast::ImportKind;
use reify_core::{ModulePath, SourceSpan};
use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::analysis::{
    enclosing_decl_at, find_named_member_span, module_name_from_uri, name_token_span,
};
use crate::convert::{find_word_at_offset, position_to_offset, span_to_range};

/// Compute go-to-definition for the symbol at the given position.
///
/// Returns the `Location` of the symbol's declaration, or `None` if the
/// position is not on a navigable identifier. A top-level declaration NAME
/// resolves to its own name token (task 6388); keywords, and words matching
/// neither a member nor a top-level declaration, return `None`.
// G-allow: LSP public API entry point; production caller uses the _in_context/_with_parsed/_from_parsed variant
pub fn compute_goto_definition(source: &str, uri: &Url, position: Position) -> Option<Location> {
    // Only needs ParsedModule for declaration spans (compiler discards them).
    // Use prelude-aware parse for AST-shape consistency with the rest of
    // reify-lsp (diagnostics + analysis); see task 2525.
    let module_name = module_name_from_uri(uri);
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));
    compute_goto_definition_with_parsed(&parsed, source, uri, position)
}

/// Compute go-to-definition using a pre-built [`ParsedModule`].
///
/// This is the injectable core shared by the per-request wrapper
/// [`compute_goto_definition`] (which parses internally) and the server's
/// cache-fed path (which supplies the per-document cached parse — one parse
/// per edit). Only declaration spans are needed, so no compile/check runs.
pub fn compute_goto_definition_with_parsed(
    parsed: &reify_ast::ParsedModule,
    source: &str,
    uri: &Url,
    position: Position,
) -> Option<Location> {
    let offset = position_to_offset(source, position);
    let (_word_start, word) = find_word_at_offset(source, offset)?;

    // Phase A (task 6388): the cursor is ON a top-level declaration's OWN name
    // token. Resolved FIRST because a declaration's own name token can never be
    // a reference to a member, so answering it here removes any chance that a
    // pathologically same-named member shadows the definition — and returning
    // the definition when the cursor is already on it is standard LSP
    // behaviour. Pinned by
    // `goto_def_cursor_on_declaration_name_beats_same_named_member`.
    if let Some(loc) = resolve_decl_name(parsed, source, uri, word, Some(offset)) {
        return Some(loc);
    }

    // Try to find the enclosing declaration by checking if the cursor offset
    // falls within a declaration's span. If found, search only that declaration
    // first for scoped resolution.
    if let Some(enclosing) = enclosing_decl_at(&parsed.declarations, offset) {
        let members: &[_] = match enclosing {
            reify_ast::Declaration::Structure(s) => &s.members,
            reify_ast::Declaration::Occurrence(o) => &o.members,
            reify_ast::Declaration::Trait(t) => &t.members,
            reify_ast::Declaration::Purpose(p) => &p.members,
            _ => &[], // Variants without members (Import, Enum, Function, etc.)
        };
        if let Some(info) = find_named_member_span(members, word) {
            return Some(Location {
                uri: uri.clone(),
                range: span_to_range(source, info.span),
            });
        }
        // Member not found in enclosing declaration; fall through to
        // the all-declarations search below.
    }

    // Fallback: search all declarations (cursor outside any declaration,
    // or enclosing declaration didn't contain the member).
    for decl in &parsed.declarations {
        let members = match decl {
            reify_ast::Declaration::Structure(s) => &s.members,
            reify_ast::Declaration::Occurrence(o) => &o.members,
            reify_ast::Declaration::Trait(t) => &t.members,
            reify_ast::Declaration::Purpose(p) => &p.members,
            _ => continue,
        };
        if let Some(info) = find_named_member_span(members, word) {
            return Some(Location {
                uri: uri.clone(),
                range: span_to_range(source, info.span),
            });
        }
    }

    // Phase C (task 6388): the word NAMES a top-level declaration — a use site
    // such as `sub b = Bracket()`, `let v = area(2mm)` or `param p : Pressure`.
    //
    // Ordering is load-bearing:
    // - AFTER both member phases, so every pre-existing member resolution keeps
    //   byte-identical behaviour and a member never loses to a same-named
    //   declaration. Pinned by
    //   `goto_def_member_use_wins_over_same_named_top_level_declaration`.
    // - BEFORE cross-file Phase 2. `compute_goto_definition_cross_file_with_parsed`
    //   delegates to this core at its Phase 1 slot, so placing Phase C here
    //   makes a LOCAL declaration win over an IMPORT of the same name — pinned
    //   by `goto_def_local_declaration_wins_over_same_named_import`.
    // - Cross-file Phase 0 (cursor inside an `import` span) still runs first, so
    //   the cursor-on-import contract is untouched.
    //
    // The returned range is the NAME TOKEN — deliberately the same shape the
    // cross-file path returns via `find_declaration_name_span`; closing that
    // asymmetry is why task 6388 exists. Member resolution above keeps returning
    // the full member statement span, unchanged.
    resolve_decl_name(parsed, source, uri, word, None)
}

/// Resolve `word` to the NAME TOKEN of the top-level declaration it names.
///
/// The single body behind task 6388's two same-file phases; `cursor` is what
/// distinguishes them:
/// - `Some(offset)` — Phase A, the cursor sits ON a declaration's own name
///   token: the match must additionally CONTAIN `offset`.
/// - `None` — Phase C, `word` merely NAMES a declaration from a use site
///   anywhere in the file: no containment filter.
///
/// Where the two calls sit relative to the member phases is the load-bearing
/// decision, and it lives at the call sites rather than here.
///
/// CHEAP DISCRIMINATORS FIRST: name equality, then (Phase A only) cursor
/// containment in the declaration's statement span — both O(1)-ish — before
/// paying for [`decl_name_token`], a bounded scan over the declaration's text.
/// Pre-filtering on the statement span cannot change which declaration matches:
/// `name_token_span` returns a sub-span of the span it is given, so `offset`
/// inside the name token always implies `offset` inside the statement span.
///
/// When two declarations share a name (already a semantic error) the FIRST in
/// source order wins.
fn resolve_decl_name(
    parsed: &reify_ast::ParsedModule,
    source: &str,
    uri: &Url,
    word: &str,
    cursor: Option<usize>,
) -> Option<Location> {
    for decl in &parsed.declarations {
        let Some((name, span)) = crate::analysis::decl_name_and_span(decl) else {
            continue;
        };
        if name != word {
            continue;
        }
        if let Some(offset) = cursor
            && (offset < span.start as usize || offset >= span.end as usize)
        {
            continue;
        }
        let Some(tok) = decl_name_token(source, name, span) else {
            continue;
        };
        if let Some(offset) = cursor
            && (offset < tok.start as usize || offset >= tok.end as usize)
        {
            continue;
        }
        return Some(Location {
            uri: uri.clone(),
            range: span_to_range(source, tok),
        });
    }
    None
}

/// The byte span of a top-level declaration's own NAME TOKEN, narrowed from the
/// `(name, statement span)` pair its kind yields — or `None` when that token
/// cannot be located inside the declaration's span.
///
/// Callers obtain `(name, decl_span)` from
/// [`crate::analysis::decl_name_and_span`], whose wildcard-free exhaustive match
/// is what makes same-file go-to-definition uniform across declaration kinds
/// instead of a per-kind allowlist.
///
/// Narrowing uses [`crate::analysis::name_token_span`] — whole-word, bounded to
/// the declaration's own span, UTF-8-boundary-snapping. Deliberately NOT
/// [`find_name_offset_in_decl`], a bare substring search that matches the `n` of
/// `fn` for a declaration named `n`. That sibling is on the RENAME write path,
/// where the same bug rewrites a keyword byte (`structure s` renames to
/// `Xtructure s`); migrating it onto `name_token_span` changes rename output, so
/// it is filed as a follow-up rather than fixed under task 6388's
/// no-rename-change constraint.
///
/// `name_token_span` falls back to a ZERO-WIDTH span at `span.start` when the
/// name is absent within the declaration span (e.g. a recovered AST node). A
/// zero-width `Location` is never a useful jump target, so that case becomes
/// `None` here.
///
/// SEPARATE BY DESIGN from [`find_declaration_name_span`], which also feeds
/// rename/references. Two non-goals follow from keeping them apart:
/// - a `structure def` nested inside a `purpose` body lives in
///   `PurposeDef.structures`, is not a top-level declaration, and is not
///   resolved (see `goto_def_purpose_nested_structure_is_not_top_level`);
/// - CROSS-file goto-def still covers only the narrower
///   [`find_declaration_name_span`] kind list, so Purpose/Constraint/Unit/
///   TypeAlias/Joint remain same-file only.
fn decl_name_token(source: &str, name: &str, decl_span: SourceSpan) -> Option<SourceSpan> {
    let tok = crate::analysis::name_token_span(source, decl_span, name);
    (tok.start != tok.end).then_some(tok)
}

/// Compute go-to-definition with cross-file import resolution.
///
/// First tries single-file resolution (same logic as [`compute_goto_definition`]).
/// On failure, checks if the word matches an imported name and resolves it to
/// the target file using the provided resolver closure.
///
/// `resolve_import` maps an import dot-path (e.g., "parts") to
/// `(target_uri, target_source_text)`, or returns `None` if the module can't be found.
// G-allow: LSP public API entry point; production caller uses the _in_context/_with_parsed/_from_parsed variant
pub fn compute_goto_definition_cross_file(
    source: &str,
    uri: &Url,
    position: Position,
    resolve_import: &dyn Fn(&str) -> Option<(Url, String)>,
) -> Option<Location> {
    let module_name = module_name_from_uri(uri);
    // Prelude-aware parse for AST-shape consistency across reify-lsp;
    // see task 2525.
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));
    compute_goto_definition_cross_file_with_parsed(&parsed, source, uri, position, resolve_import)
}

/// Compute cross-file go-to-definition using a pre-built [`ParsedModule`].
///
/// Like [`compute_goto_definition_cross_file`] but takes the primary
/// document's parse instead of parsing internally — the server supplies the
/// per-document cached parse (one parse per edit). Its single-file phase
/// delegates to [`compute_goto_definition_with_parsed`] with the same parse,
/// so the primary document is parsed at most once (the previous wrapper
/// re-parsed it inside the single-file phase).
pub fn compute_goto_definition_cross_file_with_parsed(
    parsed: &reify_ast::ParsedModule,
    source: &str,
    uri: &Url,
    position: Position,
    resolve_import: &dyn Fn(&str) -> Option<(Url, String)>,
) -> Option<Location> {
    let offset = position_to_offset(source, position);
    let (_word_start, word) = find_word_at_offset(source, offset)?;

    let offset_u32 = offset as u32;

    // Phase 0: Check if cursor is within an import statement's span.
    // This takes priority — when the cursor is on an import, navigate to the target.
    for decl in &parsed.declarations {
        if let reify_ast::Declaration::Import(import) = decl
            && offset_u32 >= import.span.start
            && offset_u32 < import.span.end
            && let Some((target_uri, target_source)) = resolve_import(&import.path)
        {
            // Determine what entity to look for in the target
            let entity_name = match &import.kind {
                ImportKind::Entity(name) => Some(name.as_str()),
                ImportKind::EntityAliased { entity, .. } => Some(entity.as_str()),
                ImportKind::Destructured(names) => {
                    // Find which name the cursor is on
                    names
                        .iter()
                        .find(|n| n.as_str() == word)
                        .map(|n| n.as_str())
                }
                ImportKind::Module | ImportKind::Aliased { .. } => None,
            };

            if let Some(name) = entity_name
                && let Some(loc) = find_declaration_in_source(&target_source, name, &target_uri)
            {
                return Some(loc);
            }
            // For module imports or unresolved entity, navigate to file start
            return Some(Location {
                uri: target_uri,
                range: Range::default(),
            });
        }
    }

    // Phase 1 + 1b: Single-file resolution (delegate to the single-file core
    // with the SAME parse — avoids re-parsing the primary document, which the
    // previous `compute_goto_definition` call did).
    if let Some(loc) = compute_goto_definition_with_parsed(parsed, source, uri, position) {
        return Some(loc);
    }

    // Phase 2: Cross-file import resolution.
    // Check if the word matches an imported name.
    for decl in &parsed.declarations {
        if let reify_ast::Declaration::Import(import) = decl {
            let target_name = match &import.kind {
                ImportKind::Entity(name) if name == word => Some(name.as_str()),
                ImportKind::EntityAliased { entity, alias } if alias == word => {
                    Some(entity.as_str())
                }
                ImportKind::Destructured(names) => names
                    .iter()
                    .find(|n| n.as_str() == word)
                    .map(|n| n.as_str()),
                _ => None,
            };

            if let Some(target_entity) = target_name
                && let Some((target_uri, target_source)) = resolve_import(&import.path)
                && let Some(loc) =
                    find_declaration_in_source(&target_source, target_entity, &target_uri)
            {
                return Some(loc);
            }
        }
    }

    None
}

/// Find a top-level declaration by name in a source string and return its Location.
///
/// Parses `source` ONCE and scans it via [`decl_name_span_in`] with
/// `include_aliases = true`, so cross-file goto-def also resolves a `type`
/// alias (#6341), then pairs the located name-token span with `uri` as an LSP
/// [`Location`].
///
/// The single parse is load-bearing, not incidental: the cross-file caller
/// probes each import's target in turn, so a MISS is the common case, and the
/// target file is not covered by the server's per-document parse cache (that
/// cache holds only the primary document). Chaining an alias-only second pass
/// behind `.or_else` would therefore re-run a full tree-sitter parse + AST
/// lowering of the same string on every miss, doubling the cost of an
/// interactive, per-keystroke-adjacent path.
fn find_declaration_in_source(source: &str, name: &str, uri: &Url) -> Option<Location> {
    // Prelude-aware parse for AST-shape consistency across reify-lsp;
    // see task 2525.
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("_target"));
    let span = decl_name_span_in(&parsed, source, name, true)?;
    Some(Location {
        uri: uri.clone(),
        range: span_to_range(source, span),
    })
}

/// Find the **name-token span** of a top-level declaration named `name`.
///
/// Parses `source` (prelude-aware, for AST-shape consistency across reify-lsp;
/// see task 2525) and delegates to [`decl_name_span_in`] with
/// `include_aliases = false`. Returns `None` when no declaration matches.
///
/// Factored from [`find_declaration_in_source`] so the cross-file
/// reference/rename collectors (task κ, 4210) can obtain a renamed structure's
/// home declaration token uniformly as a `SourceSpan`, independent of the
/// `Location`/`uri` packaging that goto-def needs.
///
/// # This helper feeds REFERENCES, not just cross-file goto-def
///
/// It serves CROSS-FILE go-to-definition *and* is consumed by three points in
/// `references.rs`: the `references::collect_structure_name_spans` home token,
/// `references::resolve_cross_file_home` step 2 → `CrossFileHome::Structure`,
/// and the cross-file rename producer.
///
/// Its kind list is therefore DELIBERATELY NARROWER than
/// [`crate::analysis::decl_name_and_span`], which task 6388 introduced as the
/// uniform, wildcard-free source of declaration names for the SAME-FILE path.
/// The two are separate on purpose, and this one must not be "unified" onto the
/// other.
///
/// **Adding a kind here changes what the REFERENCE SET reports.** The use-site
/// collectors `collect_uses` / `collect_idents_in_expr` walk `ExprKind::Ident`
/// in EXPRESSIONS only — never type expressions — and
/// `collect_structure_name_spans` adds only `sub _ = Name` construction sites.
/// So a type-position-only kind (TypeAlias / Unit / Constraint / Joint /
/// Purpose) admitted here makes `compute_references_cross_file` report the
/// DECLARATION token ALONE, with every type-position use absent. Measured, not
/// assumed: flipping this call's `include_aliases` to `true` turns that set on
/// `type Pressure = Force` / `param p : Pressure` from `None` into exactly one
/// location, the declaration token.
///
/// RENAME is gated SEPARATELY — by `references::classify_top_level_decl`'s own
/// allowlist, consulted through `references::is_renameable_cross_file`, which
/// never consults this helper. Admitting a kind here therefore does not by
/// itself grant rename; it makes the reference set the incomplete input a later
/// rename would trust, and Invariant 5 would not catch the result because the
/// edited buffer still re-parses clean.
///
/// Both halves are guarded by `references::tests::
/// rename_and_references_unaffected_by_same_file_goto_def_declaration_names`:
/// its per-kind refusals pin `classify_top_level_decl`, and its coupling
/// assertion reds when the reference set becomes non-empty for a kind whose
/// type-position uses it does not cover.
pub(crate) fn find_declaration_name_span(source: &str, name: &str) -> Option<SourceSpan> {
    // Prelude-aware parse for AST-shape consistency across reify-lsp;
    // see task 2525.
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("_target"));
    decl_name_span_in(&parsed, source, name, false)
}

/// Scan an already-parsed module for the name-token span of the declaration
/// named `name`, returning the byte [`SourceSpan`] of just the NAME identifier
/// (located via [`crate::analysis::name_token_span`], which matches
/// whole-word and only within the declaration's own span — a declaration span
/// starts at its keyword, so an unbounded substring search finds the `s` of
/// `structure` before the `s` of `structure s`).
///
/// Takes `&ParsedModule` rather than `&str` so a caller that needs more than one
/// declaration shape pays for exactly one parse; `source` is still required
/// because the AST carries whole-declaration spans, not name-token spans.
///
/// `include_aliases` gates the `TypeAlias` arm, and the gate is a safety
/// boundary, not a convenience. The shared [`find_declaration_name_span`] passes
/// `false` because the cross-file rename/reference collectors use it to decide
/// what is renameable: classifying an alias as a renameable home declaration
/// would move the declaration token while silently missing every
/// `param x : Alias` use site — those collectors walk *expressions*, not type
/// expressions — corrupting the user's file. Goto-def is read-only and has no
/// such hazard, so only [`find_declaration_in_source`] passes `true`.
/// Task #6341.
///
/// Declarations are scanned in source order, so in the (ill-formed) case of an
/// alias and a structure sharing one name, the earlier declaration wins.
///
/// A matched declaration whose span does not actually contain its own name
/// token is REFUSED — `None`, and without resuming the scan. The previous
/// locator instead fell back to a `name.len()`-wide span anchored at the
/// declaration start; that bogus WIDE span flowed into the `references.rs`
/// rename write path and emitted a destructive edit over the declaration's
/// leading keyword. Resuming the scan would be the mirror-image hazard, letting
/// a later same-named declaration donate its token — exactly what bounding the
/// search to the declaration's own span exists to prevent.
///
/// What a refusal costs is enumerated per consumer rather than summarised,
/// because they do not all behave alike — three are inert and one is not:
/// - [`find_declaration_in_source`] (goto-def) is read-only: no jump. Inert.
/// - `references.rs::resolve_cross_file_home` step 2 tests only `.is_some()`,
///   so a structure declared in the primary document stops being recognised as
///   the home and the query falls through to the import arm — which, in the
///   home document itself, resolves to nothing. A wholesale `None`. Inert.
/// - `references.rs::compute_references_cross_file` uses the value only to drop
///   the declaration token when `include_declaration = false`; with no token to
///   drop, that filter is a no-op. Inert.
/// - `references.rs::collect_structure_name_spans` pushes this token into the
///   span set that `compute_rename_cross_file` turns into edits. A refusal
///   silently OMITS it, so a rename driven from an IMPORTING document (where
///   the home resolves through the import arm, never consulting this function)
///   rewrites every construction site and import token but leaves the
///   declaration behind — a partial rename. Still strictly better than the old
///   locator, which rewrote the declaration's leading keyword, but not free.
///   Making the rename path refuse wholesale when the home token cannot be
///   located is a follow-up; it is not reachable today, because no parse
///   observed so far yields a surviving declaration whose span excludes its own
///   name token (error recovery either keeps the name inside the span or emits
///   no declaration at all, which this function already answers with `None`).
fn decl_name_span_in(
    parsed: &reify_ast::ParsedModule,
    source: &str,
    name: &str,
    include_aliases: bool,
) -> Option<SourceSpan> {
    for decl in &parsed.declarations {
        let (decl_name, span) = match decl {
            reify_ast::Declaration::Structure(s) => (s.name.as_str(), s.span),
            reify_ast::Declaration::Occurrence(o) => (o.name.as_str(), o.span),
            reify_ast::Declaration::Function(f) => (f.name.as_str(), f.span),
            reify_ast::Declaration::Enum(e) => (e.name.as_str(), e.span),
            reify_ast::Declaration::Trait(t) => (t.name.as_str(), t.span),
            reify_ast::Declaration::Field(f) => (f.name.as_str(), f.span),
            reify_ast::Declaration::TypeAlias(t) if include_aliases => (t.name.as_str(), t.span),
            _ => continue,
        };
        if decl_name == name {
            // Point to the name within the declaration, not the entire span.
            // An empty span is `name_token_span`'s documented not-found
            // fallback, and an exact discriminator here because a declaration
            // name is never the empty string.
            let token = name_token_span(source, span, name);
            return (!token.is_empty()).then_some(token);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    /// Test helper: return the UTF-16 code unit count of the Nth line of `source`.
    ///
    /// LSP `Position.character` is defined in UTF-16 code units
    /// (`PositionEncodingKind::UTF16`), matching the convention already used in
    /// `convert.rs` (`offset_to_position` / `position_to_offset`).
    ///
    /// Every declaration in these tests is single-line with `range.end`
    /// pinned to the end of that line. Computing the expected end from
    /// the source keeps assertions self-consistent if the declaration
    /// text ever changes (e.g. renaming a param or widening a literal),
    /// avoiding manual recompute of hard-coded character offsets.
    fn line_end_char(source: &str, line: u32) -> u32 {
        source
            .lines()
            .nth(line as usize)
            .expect("line index out of range in test source")
            .encode_utf16()
            .count() as u32
    }

    #[test]
    fn line_end_char_returns_utf16_units() {
        // Supplementary-plane emoji U+1F600 is 1 `char` but 2 UTF-16 code units.
        // "abc\u{1F600}" → 3 ASCII chars + 1 emoji = 4 chars but 5 UTF-16 code units.
        // LSP Position.character is defined in UTF-16 code units (PositionEncodingKind::UTF16),
        // so line_end_char must return 5, not 4.
        let source = "abc\u{1F600}";
        assert_eq!(
            line_end_char(source, 0),
            5,
            "line_end_char must return UTF-16 code unit count, not char count"
        );
    }

    // --- step-7: go-to-definition tests ---

    #[test]
    fn goto_def_thickness_in_constraint_returns_param_location() {
        let source = reify_test_support::bracket_source();
        // 'thickness' in 'constraint thickness > 2mm' is on line 9
        let position = Position::new(9, 15);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for thickness ref should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to the param declaration:
        // "    param thickness: Length = 5mm" (line 3)
        assert_eq!(loc.range.start.line, 3);
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 3, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 3),
            "end should cover full 'param thickness: Length = 5mm'"
        );
    }

    #[test]
    fn goto_def_width_in_constraint_expr_returns_param_location() {
        let source = reify_test_support::bracket_source();
        // 'width' in 'constraint thickness < width / 4' is on line 10
        // "    constraint thickness < width / 4"
        //                            ^-- char 30
        let position = Position::new(10, 30);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for width ref should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to param width on line 1:
        // "    param width: Length = 80mm"
        assert_eq!(loc.range.start.line, 1);
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 1, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 1),
            "end should cover full 'param width: Length = 80mm'"
        );
    }

    #[test]
    fn goto_def_volume_returns_let_location() {
        let source = reify_test_support::bracket_source();
        // 'volume' in "let volume = ..." on line 7
        let position = Position::new(7, 8);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for volume should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to itself (the let declaration) on line 7:
        // "    let volume = width * height * thickness"
        assert_eq!(loc.range.start.line, 7);
        assert_eq!(
            loc.range.start.character, 4,
            "let keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 7, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 7),
            "end should cover full 'let volume = width * height * thickness'"
        );
    }

    #[test]
    fn goto_def_occurrence_param_returns_location() {
        let source = "occurrence def Joint {\n    param diameter: Length = 10mm\n    constraint diameter > 5mm\n}";
        // 'diameter' in the constraint is on line 2, col 15
        let position = Position::new(2, 15);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for diameter ref in occurrence should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to param declaration on line 1:
        // "    param diameter: Length = 10mm"
        assert_eq!(loc.range.start.line, 1);
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 1, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 1),
            "end should cover full 'param diameter: Length = 10mm'"
        );
    }

    #[test]
    fn goto_def_occurrence_let_returns_location() {
        let source = "occurrence def Joint {\n    param diameter: Length = 10mm\n    let radius = diameter / 2\n}";
        // 'radius' on line 2, col 8
        let position = Position::new(2, 8);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for let member in occurrence should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to let declaration on line 2:
        // "    let radius = diameter / 2"
        assert_eq!(loc.range.start.line, 2);
        assert_eq!(
            loc.range.start.character, 4,
            "let keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 2, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 2),
            "end should cover full 'let radius = diameter / 2'"
        );
    }

    #[test]
    fn goto_def_keyword_returns_none() {
        let source = reify_test_support::bracket_source();
        // 'param' keyword on line 1
        let position = Position::new(1, 6);
        assert!(
            compute_goto_definition(source, &test_uri(), position).is_none(),
            "goto-def on keyword should return None"
        );
    }

    #[test]
    fn goto_def_structure_name_resolves_to_its_own_name_token() {
        // Task 6388: goto-def on a top-level declaration NAME used to return
        // None — a deliberate non-goal, now lifted. Standard LSP behaviour is
        // that goto-def on a definition returns that definition, and the
        // cross-file path (find_declaration_name_span) has always returned the
        // NAME TOKEN for the very same symbol; this closes that asymmetry.
        let source = reify_test_support::bracket_source();
        // 'Bracket' on line 0: "structure def Bracket {"
        let position = Position::new(0, 16);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def on a structure name should resolve to its own name token");
        assert_eq!(loc.uri, test_uri());

        // Derive the expected columns from the fixture rather than hard-coding
        // 14/21, so the assertion survives a fixture edit.
        let name_start = source.find("Bracket").expect("fixture declares Bracket");
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, name_start as u32);
        assert_eq!(loc.range.end.line, 0);
        assert_eq!(
            loc.range.end.character,
            (name_start + "Bracket".len()) as u32
        );
    }

    // --- guarded group go-to-definition tests ---

    #[test]
    fn goto_def_param_inside_where_block() {
        // Source with guarded_x declared inside a where block,
        // referenced by let ref_x = guarded_x on line 5.
        let source = "structure S {\n    param cond : Bool = true\n    where cond {\n        param guarded_x : Length = 5mm\n    }\n    let ref_x = guarded_x\n}";
        // Line 5: "    let ref_x = guarded_x"
        //                          ^-- char 16 = start of 'guarded_x' reference
        let position = Position::new(5, 16);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for guarded_x ref should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to the param declaration on line 3:
        // "        param guarded_x : Length = 5mm"
        assert_eq!(loc.range.start.line, 3);
        assert_eq!(
            loc.range.start.character, 8,
            "param keyword starts after 8-space indent"
        );
        // Assert range.end covers the full declaration line.
        assert_eq!(loc.range.end.line, 3, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 3),
            "end should cover full 'param guarded_x : Length = 5mm'"
        );
    }

    #[test]
    fn goto_def_let_inside_else_block() {
        // Source with fallback declared inside an else block,
        // referenced by let use_fb = fallback on line 7.
        let source = "structure S {\n    param cond : Bool = true\n    where cond {\n        param a : Length = 1mm\n    } else {\n        let fallback = 10\n    }\n    let use_fb = fallback\n}";
        // Line 7: "    let use_fb = fallback"
        //                           ^-- char 17 = start of 'fallback' reference
        let position = Position::new(7, 17);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for fallback ref should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to the let declaration on line 5:
        // "        let fallback = 10"
        assert_eq!(loc.range.start.line, 5);
        assert_eq!(
            loc.range.start.character, 8,
            "let keyword starts after 8-space indent"
        );
        // Assert range.end covers the full declaration line.
        assert_eq!(loc.range.end.line, 5, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 5),
            "end should cover full 'let fallback = 10'"
        );
    }

    // --- enclosing-declaration scoping tests ---

    #[test]
    fn goto_def_cursor_in_second_decl_scopes_to_enclosing() {
        // Two structures with identically-named param x.
        // Cursor on 'x' in B's `let y = x` should jump to B's param x, not A's.
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param x: Bool = true\n    let y = x\n}";
        // Line 5: "    let y = x"
        //                      ^ col 12 = 'x' reference
        let position = Position::new(5, 12);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for x in B should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to B's param x on line 4, NOT A's on line 1:
        // "    param x: Bool = true"
        assert_eq!(
            loc.range.start.line, 4,
            "expected B's param x (line 4), got line {}",
            loc.range.start.line
        );
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 4, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 4),
            "end should cover full 'param x: Bool = true'"
        );
    }

    #[test]
    fn goto_def_cursor_in_occurrence_scopes_to_enclosing() {
        // Structure A and occurrence B both have param diameter.
        // Cursor on 'diameter' in B's constraint should jump to B's param, not A's.
        let source = "structure A {\n    param diameter: Length = 10mm\n}\noccurrence def B {\n    param diameter: Length = 20mm\n    constraint diameter > 5mm\n}";
        // Line 5: "    constraint diameter > 5mm"
        //                        ^ col 15 = 'diameter' reference
        let position = Position::new(5, 15);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for diameter in B should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to B's param diameter on line 4, NOT A's on line 1:
        // "    param diameter: Length = 20mm"
        assert_eq!(
            loc.range.start.line, 4,
            "expected B's param diameter (line 4), got line {}",
            loc.range.start.line
        );
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 4, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 4),
            "end should cover full 'param diameter: Length = 20mm'"
        );
    }

    #[test]
    fn goto_def_existing_single_decl_unchanged() {
        // Verify that all existing single-declaration goto_def behavior still
        // works after the enclosing-declaration scoping refactoring.
        let source = reify_test_support::bracket_source();
        // Test 1: thickness ref in constraint → param declaration
        let loc = compute_goto_definition(source, &test_uri(), Position::new(9, 15))
            .expect("thickness ref should resolve");
        assert_eq!(loc.range.start.line, 3);
        // Test 2: width ref in constraint expr → param declaration
        let loc = compute_goto_definition(source, &test_uri(), Position::new(10, 30))
            .expect("width ref should resolve");
        assert_eq!(loc.range.start.line, 1);
        // Test 3: volume let → itself
        let loc = compute_goto_definition(source, &test_uri(), Position::new(7, 8))
            .expect("volume should resolve");
        assert_eq!(loc.range.start.line, 7);
    }

    #[test]
    fn goto_def_cursor_in_first_decl_still_finds_own_member() {
        // When cursor is inside the first declaration, scoped search should
        // still find members (not accidentally skip them).
        let source = "structure A {\n    param x: Length = 5mm\n    let y = x\n}\nstructure B {\n    param x: Bool = true\n}";
        // Line 2: "    let y = x"
        //                      ^ col 12 = 'x' reference inside A
        let position = Position::new(2, 12);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for x in A should return location");
        // Should point to A's param x on line 1
        assert_eq!(
            loc.range.start.line, 1,
            "expected A's param x (line 1), got line {}",
            loc.range.start.line
        );
    }

    #[test]
    fn goto_def_enclosing_decl_member_not_found_falls_back() {
        // Cursor on 'y' in A's `let z = y`. Phase 1 finds enclosing A, but 'y'
        // is not a member of A → break. Phase 2 fallback searches all declarations
        // and finds 'y' as a param in B.
        let source = "structure A {\n    param x: Length = 5mm\n    let z = y\n}\nstructure B {\n    param y: Length = 20mm\n}";
        // Line 2: "    let z = y"
        //                      ^ col 12 = 'y' reference inside A's span
        let position = Position::new(2, 12);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for y inside A should fall back and find y in B");
        assert_eq!(loc.uri, test_uri());
        // Should point to B's param y on line 5, proving Phase 2 fallback fired:
        // "    param y: Length = 20mm"
        assert_eq!(
            loc.range.start.line, 5,
            "expected B's param y (line 5), got line {}",
            loc.range.start.line
        );
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 5, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 5),
            "end should cover full 'param y: Length = 20mm'"
        );
    }

    #[test]
    fn goto_def_cursor_outside_declarations_falls_back_to_first() {
        // Standalone 'x' between two declarations, outside both spans.
        // Phase 1 loop finds no enclosing declaration.
        // Phase 2 fallback searches all declarations and finds 'x' in A.
        let source = "structure A {\n    param x: Length = 5mm\n}\nx\nstructure B {\n    param y: Length = 20mm\n}";
        // Line 3: "x" — standalone word between declarations
        //          ^ col 0
        let position = Position::new(3, 0);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for x outside declarations should fall back and find x in A");
        assert_eq!(loc.uri, test_uri());
        // Should point to A's param x on line 1
        assert_eq!(
            loc.range.start.line, 1,
            "expected A's param x (line 1), got line {}",
            loc.range.start.line
        );
    }

    #[test]
    fn goto_def_declaration_name_resolves_to_itself() {
        // Task 6388: this source/position pair used to be asserted as None by a
        // test misleadingly named `goto_def_unknown_word_returns_none` — 'Foo'
        // is not an unknown word, it is the structure's own name. It now
        // resolves to its own name token.
        let source = "structure Foo {\n  param x: Length = 5mm\n}";
        let position = Position::new(0, 12); // on 'Foo'
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def on a declaration name should resolve to itself");
        assert_eq!(loc.uri, test_uri());
        let name_start = source.find("Foo").expect("source declares Foo");
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, name_start as u32);
        assert_eq!(loc.range.end.line, 0);
        assert_eq!(loc.range.end.character, (name_start + "Foo".len()) as u32);
    }

    #[test]
    fn goto_def_unknown_word_returns_none() {
        // The genuine no-match contract (task 6388 keeps this covered, with an
        // honest fixture): a word naming neither a member nor a top-level
        // declaration still returns None.
        let source = "structure Foo {\n    let y = nowhere\n}";
        let unknown = source.find("nowhere").expect("source mentions nowhere");
        let position = crate::convert::offset_to_position(source, unknown as u32 + 1);
        assert!(
            compute_goto_definition(source, &test_uri(), position).is_none(),
            "goto-def on a word matching no member and no declaration should return None"
        );
    }

    // --- task 6388: uniform same-file declaration-name resolution ---

    /// The snippet table is SHARED with `analysis::tests`, not mirrored:
    /// `crate::analysis::NAMED_DECL_SNIPPETS` is a `#[cfg(test)] pub(crate)`
    /// const sitting next to the `decl_name_and_span` oracle it enumerates.
    /// Two verbatim copies used to live here and there, and had to be edited in
    /// lockstep on every grammar change (9d56ba5485 touched both).
    use crate::analysis::NAMED_DECL_SNIPPETS;

    /// Byte offsets of every occurrence of `needle` in `source`, ascending.
    fn occurrences(source: &str, needle: &str) -> Vec<usize> {
        source.match_indices(needle).map(|(i, _)| i).collect()
    }

    /// Convert an LSP range back to the `[start, end)` byte range it covers.
    fn range_to_byte_range(source: &str, range: Range) -> (usize, usize) {
        (
            position_to_offset(source, range.start),
            position_to_offset(source, range.end),
        )
    }

    #[test]
    fn goto_def_cursor_on_declaration_name_resolves_for_every_kind() {
        for (source, name) in NAMED_DECL_SNIPPETS {
            let name_offset = source
                .find(name)
                .unwrap_or_else(|| panic!("snippet must contain {name:?}: {source}"));
            // A byte INSIDE the name token, so `find_word_at_offset` yields it.
            // The midpoint rather than `name_offset + 1`: the latter overshoots
            // a ONE-character name (e.g. `structure S`, where the token is
            // `[10, 11)` and offset 11 is already the following space).
            let position =
                crate::convert::offset_to_position(source, (name_offset + name.len() / 2) as u32);

            let loc = compute_goto_definition(source, &test_uri(), position)
                .unwrap_or_else(|| panic!("goto-def on {name:?} returned None for: {source}"));
            assert_eq!(loc.uri, test_uri(), "uri mismatch for: {source}");

            let (lo, hi) = range_to_byte_range(source, loc.range);
            assert_eq!(
                &source[lo..hi],
                *name,
                "goto-def range {:?} sliced to {:?}, expected exactly {name:?} for: {source}",
                loc.range,
                &source[lo..hi]
            );
            assert_eq!(
                lo, name_offset,
                "goto-def should land on the DECLARATION's own name token for: {source}"
            );
        }
    }

    #[test]
    fn goto_def_unnamed_declaration_kinds_return_none() {
        // The three `Declaration` variants that declare no name of their own.
        // Driven through the SINGLE-FILE entry point on purpose: the cross-file
        // Phase 0 would navigate an `import` to its target file, which is a
        // different contract.
        //
        // Each row's WORD is chosen to be exactly what a hypothetical name arm
        // for that variant would return, so the row actually fails if the arm is
        // ever flipped from None to Some. Pointing the import row at the module
        // PATH segment `parts`, or the module row at a single letter of a dotted
        // `a.b.c` path, would leave both rows green under such a change.
        let cases = [
            // The ENTITY name an `Import` binds — what an Import name arm would
            // yield. Correctly None today: `Import` declares no name of its own,
            // there is no local `Hole` declaration, and no member named `Hole`.
            ("import parts.Hole", "Hole"),
            // Single-segment `module a` (the shape every `examples/*.ri` uses),
            // so the word under the cursor IS the whole module path.
            ("module a", "a"),
            ("default Material = steel", "Material"),
        ];
        for (source, word) in cases {
            let offset = source
                .find(word)
                .unwrap_or_else(|| panic!("snippet must contain {word:?}: {source}"));
            // Midpoint, not `offset + 1`: the latter overshoots a ONE-character
            // word such as `module a`, landing past the end of the source where
            // `find_word_at_offset` returns None and the assertion goes vacuous.
            let position =
                crate::convert::offset_to_position(source, (offset + word.len() / 2) as u32);
            assert!(
                compute_goto_definition(source, &test_uri(), position).is_none(),
                "unnamed declaration kind should not resolve {word:?} for: {source}"
            );
        }
    }

    /// Parse a test fixture the same way the goto-def entry point does, and
    /// assert it parses clean — a fixture that silently fails to parse would
    /// make an ordering pin below pass without exercising the conflict it
    /// claims to set up.
    fn parse_clean(source: &str) -> reify_ast::ParsedModule {
        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
        assert!(
            parsed.errors.is_empty(),
            "fixture must parse clean, got {:?} for: {source}",
            parsed.errors
        );
        parsed
    }

    #[test]
    fn goto_def_cursor_on_declaration_name_beats_same_named_member() {
        // ORDERING PIN for Phase A's placement BEFORE both member phases.
        //
        // `Bar` holds a member pathologically named `Foo` — the same name as the
        // structure declared above it. With the cursor on `structure Foo`'s own
        // name token, Phase A answers first. Merging Phase A into Phase C, or
        // moving it after the member phases, would let the all-declarations
        // member fallback find `param Foo` inside `Bar` and jump THERE instead;
        // this test is what makes that refactor red.
        let source = "structure Foo {\n    param x : Length = 5mm\n}\nstructure Bar {\n    param Foo : Length = 1mm\n}";
        let parsed = parse_clean(source);

        // Fixture guard: the shadowing member must really exist, or the pin is
        // vacuous.
        let bar = parsed
            .declarations
            .iter()
            .find_map(|d| match d {
                reify_ast::Declaration::Structure(s) if s.name == "Bar" => Some(s),
                _ => None,
            })
            .expect("fixture declares structure Bar");
        let shadow = find_named_member_span(&bar.members, "Foo")
            .expect("fixture: Bar must hold a member named Foo for this pin to bite");

        let decl_name = source.find("Foo").expect("source declares Foo");
        let position = crate::convert::offset_to_position(source, decl_name as u32 + 1);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("cursor on a declaration's own name token must resolve");

        let (lo, hi) = range_to_byte_range(source, loc.range);
        assert_eq!(
            &source[lo..hi],
            "Foo",
            "expected the declaration name token"
        );
        assert_eq!(
            lo, decl_name,
            "must land on `structure Foo`'s own name token, not on the same-named \
             member at offset {}",
            shadow.span.start
        );
    }

    #[test]
    fn goto_def_member_use_wins_over_same_named_top_level_declaration() {
        // ORDERING PIN for Phase C's placement AFTER both member phases — the
        // "every pre-existing member resolution stays byte-identical" claim.
        //
        // A top-level `fn width` shares its name with `S`'s `param width`. A use
        // of the member must still resolve to the MEMBER statement span; hoisting
        // Phase C above the member phases would jump to the fn's name token.
        let source = "fn width(x: Length) -> Length { x }\nstructure S {\n    param width : Length = 5mm\n    let v = width\n}";
        let parsed = parse_clean(source);

        // Fixture guard: the conflicting top-level declaration must really exist.
        assert!(
            parsed.declarations.iter().any(|d| matches!(
                d,
                reify_ast::Declaration::Function(f) if f.name == "width"
            )),
            "fixture must declare a top-level `fn width` for this pin to bite"
        );

        let fn_name = source.find("width").expect("fixture declares fn width");
        let member = source
            .find("param width")
            .expect("fixture declares the member");
        let use_site =
            source.find("let v = width").expect("fixture holds a use") + "let v = ".len();
        let position = crate::convert::offset_to_position(source, use_site as u32 + 1);

        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("a member use must resolve");
        let (lo, hi) = range_to_byte_range(source, loc.range);
        assert_eq!(
            lo,
            member,
            "a member use must resolve to the MEMBER statement (offset {member}), \
             not to the same-named `fn width` name token (offset {fn_name}); got {:?}",
            &source[lo..hi]
        );
        assert!(
            source[lo..hi].starts_with("param width"),
            "expected the member statement span, got {:?}",
            &source[lo..hi]
        );
    }

    #[test]
    fn goto_def_purpose_nested_structure_is_not_top_level() {
        // DELIBERATE SCOPE BOUNDARY, not an oversight. A `structure def` nested
        // directly inside a `purpose` body lands in `PurposeDef.structures`, not
        // in `ParsedModule.declarations`, so it is not a TOP-LEVEL declaration
        // and task 6388's uniform declaration-name resolution does not reach it.
        // Whether such a name is even visible outside its enclosing purpose is a
        // language-semantics question this task does not answer; pinning the
        // current None keeps the boundary explicit rather than latent.
        let source = "purpose Exploration() {\n    structure def InPurpose {\n        param x : Length = 5mm\n    }\n}";
        let offset = source.find("InPurpose").expect("source declares InPurpose");
        let position = crate::convert::offset_to_position(source, offset as u32 + 1);
        assert!(
            compute_goto_definition(source, &test_uri(), position).is_none(),
            "a purpose-nested structure name is not a top-level declaration"
        );
    }

    #[test]
    fn goto_def_unit_suffixed_literal_does_not_resolve_to_its_unit_declaration() {
        // BOUNDARY PIN, sibling to `goto_def_purpose_nested_structure_is_not_top_level`.
        //
        // Task 6388's "uniform across declaration kinds" claim holds at
        // DECLARATION sites for all eleven named kinds, but a `unit` has no
        // reachable USE site, and that limit is worth pinning rather than
        // leaving as a silent hole in the use-site table above.
        //
        // The cause is the WORD-SCANNING layer, not the declaration scan:
        // `find_word_at_offset` treats every alphanumeric byte as an identifier
        // byte, so a unit-suffixed literal fuses with its number and the cursor
        // anywhere in `5meter` yields the word `"5meter"` — which never equals
        // the declaration name `meter`. Widening `decl_name_and_span` cannot
        // reach this; teaching the scanner to split a unit suffix would.
        let source = "unit meter : Length\nstructure S {\n    param x : Length = 5meter\n}";
        let parsed = parse_clean(source);

        let suffix = source.rfind("meter").expect("fixture uses a `5meter` literal");
        // Fixture guard: the scanned word really IS the fused literal, so the
        // None below pins the documented boundary rather than an unrelated miss.
        assert_eq!(
            find_word_at_offset(source, suffix).map(|(_, w)| w),
            Some("5meter"),
            "the unit suffix must fuse with the numeric literal, or this pin \
             no longer describes why the use site is unreachable"
        );
        assert!(
            compute_goto_definition_with_parsed(
                &parsed,
                source,
                &test_uri(),
                crate::convert::offset_to_position(source, suffix as u32)
            )
            .is_none(),
            "a unit-suffixed literal is not a use site the word scanner can reach"
        );

        // CONTRAST: the same unit's own DECLARATION name is navigable, so this
        // is a use-site limit, not a missing kind.
        let decl = source.find("meter").expect("fixture declares `unit meter`");
        assert!(
            compute_goto_definition_with_parsed(
                &parsed,
                source,
                &test_uri(),
                crate::convert::offset_to_position(source, (decl + "meter".len() / 2) as u32)
            )
            .is_some(),
            "the unit DECLARATION name must still resolve to itself"
        );
    }

    #[test]
    fn goto_def_use_site_resolves_to_top_level_declaration_name_token() {
        // Task 6388's headline symptom: goto-def on a USE of a top-level
        // declaration (far from the declaration itself) must jump to that
        // declaration's name token. `(source, name, use_occurrence_index)` —
        // occurrence 0 is always the declaration, so the assertion is that the
        // jump LEFT the use site and LANDED on the definition.
        let rows: &[(&str, &str, usize)] = &[
            // structure construction — the `sub b = Bracket()` case.
            (
                "structure def Bracket {\n    param w : Length = 5mm\n}\nstructure Asm {\n    sub b = Bracket()\n}",
                "Bracket",
                1,
            ),
            // fn call.
            (
                "fn area(x: Length) -> Length { x }\nstructure S {\n    let v = area(2mm)\n}",
                "area",
                1,
            ),
            // type alias in a type-annotation position.
            (
                "type Pressure = Force\nstructure S {\n    param p : Pressure = 1.0\n}",
                "Pressure",
                1,
            ),
            // enum name in a type position (occurrence 2 is `Dir.In`).
            (
                "enum Dir { In, Out }\nstructure S {\n    param d : Dir = Dir.In\n}",
                "Dir",
                1,
            ),
            // trait name in a bound.
            (
                "trait Rigid { param mass : Mass }\nstructure S : Rigid {\n    param mass : Mass\n}",
                "Rigid",
                1,
            ),
            // occurrence construction — `sub o = Welding()`.
            (
                "occurrence def Welding {\n    param method : Length = 1mm\n}\nstructure Asm {\n    sub o = Welding()\n}",
                "Welding",
                1,
            ),
            // field name referenced from an expression.
            (
                "field def temp : Point3 -> Real { source = analytical { |p| p } }\nstructure S {\n    let v = temp\n}",
                "temp",
                1,
            ),
        ];

        for (source, name, use_index) in rows {
            let offsets = occurrences(source, name);
            assert!(
                offsets.len() > *use_index,
                "fixture must contain a use of {name:?} at occurrence {use_index}: {source}"
            );
            let decl_offset = offsets[0];
            let use_offset = offsets[*use_index];
            let position =
                crate::convert::offset_to_position(source, (use_offset + name.len() / 2) as u32);

            let loc = compute_goto_definition(source, &test_uri(), position).unwrap_or_else(|| {
                panic!("goto-def on the USE of {name:?} returned None for: {source}")
            });
            assert_eq!(loc.uri, test_uri(), "uri mismatch for {name:?}: {source}");

            let (lo, hi) = range_to_byte_range(source, loc.range);
            assert_eq!(
                &source[lo..hi],
                *name,
                "goto-def range sliced to {:?}, expected exactly {name:?} for: {source}",
                &source[lo..hi]
            );
            assert_eq!(
                lo, decl_offset,
                "goto-def on a use of {name:?} must land on the DECLARATION's name \
                 token (offset {decl_offset}), not on the use site (offset {use_offset}): {source}"
            );
        }
    }

    #[test]
    fn goto_def_local_declaration_wins_over_same_named_import() {
        // Precedence pin: a LOCAL top-level declaration beats an import of the
        // same name. This is the one ordering decision task 6388 makes that a
        // later refactor could silently invert — same-file Phase C runs inside
        // the single-file core, which the cross-file entry point delegates to at
        // its Phase 1 slot, so it necessarily precedes cross-file Phase 2.
        let source = "import parts.Hole\nstructure Hole {\n    param d : Length = 1mm\n}\nstructure Asm {\n    sub h = Hole()\n}";
        let target_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Occurrence 0 = the import, 1 = the local declaration, 2 = the use.
        let offsets = occurrences(source, "Hole");
        assert_eq!(offsets.len(), 3, "fixture should mention Hole three times");
        let position = crate::convert::offset_to_position(source, offsets[2] as u32 + 1);

        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("goto-def on `sub h = Hole()` should resolve");
        assert_eq!(
            loc.uri,
            test_uri(),
            "must resolve to the LOCAL declaration, not the imported target"
        );
        let (lo, hi) = range_to_byte_range(source, loc.range);
        assert_eq!(&source[lo..hi], "Hole");
        assert_eq!(
            lo, offsets[1],
            "must land on the local `structure Hole` name token"
        );
    }

    #[test]
    fn goto_def_cursor_inside_enum_decl_falls_through_to_global() {
        // Enum variant 'x' shares name with param x in structure S.
        // Cursor on 'x' inside enum span → enclosing_decl_at returns Enum,
        // _ => &[] gives empty members, falls through to phase-2 global search,
        // which finds param x in S.
        let source = "enum Foo { x }\nstructure S {\n    param x: Length = 5mm\n}";
        // Line 0: "enum Foo { x }"
        //                     ^ col 11 = 'x' variant
        let position = Position::new(0, 11);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for x inside enum should fall through to S's param x");
        assert_eq!(loc.uri, test_uri());
        // Should point to S's param x on line 2
        assert_eq!(
            loc.range.start.line, 2,
            "expected S's param x (line 2), got line {}",
            loc.range.start.line
        );
    }

    #[test]
    fn goto_def_fallback_finds_trait_member() {
        // Cursor on 'mass' inside structure S, which has no member named 'mass'.
        // Phase 1 scoped lookup returns None (S has member 'y', not 'mass').
        // Phase 2 fallback should find the trait param mass.
        let source =
            "trait Rigid {\n    param mass: Length = 5mm\n}\nstructure S {\n    let y = mass\n}";
        // Line 4: "    let y = mass"
        //                      ^ col 12 = 'mass' reference
        let position = Position::new(4, 12);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for mass in S should fall through to trait param");
        assert_eq!(loc.uri, test_uri());
        // Should point to Rigid's param mass on line 1
        assert_eq!(
            loc.range.start.line, 1,
            "expected trait's param mass (line 1), got line {}",
            loc.range.start.line
        );
    }

    #[test]
    fn goto_def_cursor_in_trait_scopes_to_enclosing() {
        // Structure A and trait T both have param x.
        // Cursor on 'x' in T's `let y = x` should jump to T's param x, not A's.
        let source = "structure A {\n    param x: Length = 5mm\n}\ntrait T {\n    param x: Length = 10mm\n    let y = x\n}";
        // Line 5: "    let y = x"
        //                      ^ col 12 = 'x' reference
        let position = Position::new(5, 12);
        let loc = compute_goto_definition(source, &test_uri(), position)
            .expect("goto-def for x in trait T should return location");
        assert_eq!(loc.uri, test_uri());
        // Should point to T's param x on line 4, NOT A's on line 1:
        // "    param x: Length = 10mm"
        assert_eq!(
            loc.range.start.line, 4,
            "expected T's param x (line 4), got line {}",
            loc.range.start.line
        );
        assert_eq!(
            loc.range.start.character, 4,
            "param keyword starts after 4-space indent"
        );
        assert_eq!(loc.range.end.line, 4, "declaration should be single-line");
        assert_eq!(
            loc.range.end.character,
            line_end_char(source, 4),
            "end should cover full 'param x: Length = 10mm'"
        );
    }

    // --- cross-file goto-definition tests ---

    fn parts_uri() -> Url {
        Url::parse("file:///project/parts.ri").unwrap()
    }

    /// Helper: build a mock resolver from a HashMap of (import_path -> (uri, source))
    fn mock_resolver(
        map: std::collections::HashMap<String, (Url, String)>,
    ) -> impl Fn(&str) -> Option<(Url, String)> {
        move |path: &str| map.get(path).cloned()
    }

    #[test]
    fn cross_file_entity_import_resolves_to_target_structure() {
        // Main source imports 'parts.Hole' and uses it as a sub-component type.
        let source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        // Target file declares 'structure Hole { ... }'
        let target_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Hole' in 'sub hole = Hole' (line 2, col 16)
        let position = Position::new(2, 16);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cross-file goto-def should resolve imported Hole");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        assert_eq!(
            loc.range.start.line, 0,
            "should point to structure Hole declaration on line 0"
        );
    }

    #[test]
    fn cross_file_destructured_import_resolves_to_target_structure() {
        // Main source imports 'parts.{Bolt, Nut}', cursor on 'Bolt' in a constraint.
        let source =
            "import parts.{Bolt, Nut}\nstructure Assembly {\n    sub b = Bolt\n    sub n = Nut\n}";
        // Target file has both structures
        let target_source = "structure Bolt {\n    param length: Length = 20mm\n}\nstructure Nut {\n    param size: Length = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Bolt' in 'sub b = Bolt' (line 2, col 12)
        let position = Position::new(2, 12);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cross-file goto-def should resolve destructured import Bolt");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        assert_eq!(
            loc.range.start.line, 0,
            "should point to structure Bolt declaration on line 0"
        );
    }

    #[test]
    fn cross_file_aliased_entity_import_resolves_to_original_name() {
        // Main source imports 'parts.Bolt as StdBolt', cursor on 'StdBolt' in code.
        let source = "import parts.Bolt as StdBolt\nstructure Assembly {\n    sub b = StdBolt\n}";
        // Target file has 'structure Bolt { ... }' (original name)
        let target_source = "structure Bolt {\n    param length: Length = 20mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'StdBolt' in 'sub b = StdBolt' (line 2, col 12)
        let position = Position::new(2, 12);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cross-file goto-def should resolve aliased import StdBolt -> Bolt");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        assert_eq!(
            loc.range.start.line, 0,
            "should point to structure Bolt (original name) on line 0"
        );
    }

    #[test]
    fn cross_file_function_import_resolves_to_fn_declaration() {
        // Main source imports 'math.Sqrt' (entity import, uppercase) and uses it.
        // In Reify, entity imports use uppercase first letter per convention.
        let source = "import math.Sqrt\nstructure Circle {\n    param r: Length = 5mm\n    let d = Sqrt(r)\n}";
        // Target file declares 'fn Sqrt(...)'
        let target_source = "fn Sqrt(x: Length) -> Length {\n    x\n}";
        let math_uri = Url::parse("file:///project/math.ri").unwrap();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "math".to_string(),
            (math_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Sqrt' in 'let d = Sqrt(r)' (line 3, col 12)
        let position = Position::new(3, 12);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cross-file goto-def should resolve imported fn Sqrt");
        assert_eq!(loc.uri, math_uri, "should point to the math target file");
        assert_eq!(
            loc.range.start.line, 0,
            "should point to fn Sqrt declaration on line 0"
        );
    }

    #[test]
    fn cross_file_cursor_on_import_entity_navigates_to_target() {
        // Cursor on 'Hole' within 'import parts.Hole' (on the import statement itself)
        let source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        let target_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Hole' in 'import parts.Hole' (line 0, col 13)
        let position = Position::new(0, 13);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cursor on import entity name should navigate to target declaration");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        assert_eq!(
            loc.range.start.line, 0,
            "should point to structure Hole in the target file"
        );
    }

    #[test]
    fn cross_file_cursor_on_import_path_navigates_to_target() {
        // Cursor on 'parts' within 'import parts.Hole'
        let source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        let target_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'parts' in 'import parts.Hole' (line 0, col 8)
        let position = Position::new(0, 8);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cursor on import path should navigate to target");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        // For entity import with cursor on path, navigate to the entity declaration
        assert_eq!(
            loc.range.start.line, 0,
            "should point to structure Hole declaration"
        );
    }

    #[test]
    fn cross_file_cursor_on_module_import_navigates_to_file_start() {
        // Module import: 'import utils' (no entity)
        let source = "import utils\nstructure S {\n    param x: Length = 1mm\n}";
        let target_source = "structure Helper {\n    param y: Length = 2mm\n}";
        let utils_uri = Url::parse("file:///project/utils.ri").unwrap();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "utils".to_string(),
            (utils_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'utils' in 'import utils' (line 0, col 8)
        let position = Position::new(0, 8);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cursor on module import should navigate to file start");
        assert_eq!(loc.uri, utils_uri, "should point to the target file");
        assert_eq!(
            loc.range.start.line, 0,
            "module import should navigate to file start"
        );
        assert_eq!(loc.range.start.character, 0);
    }

    // --- verification / regression tests ---

    #[test]
    fn cross_file_sub_component_type_resolves_through_entity_import() {
        // Verify: sub-component type 'sub hole = Hole' resolves through entity import.
        // (This overlaps step-1 but explicitly verifies the sub-component pattern.)
        let source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        let target_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Hole' in 'sub hole = Hole' (line 2, col 16)
        let position = Position::new(2, 16);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("sub-component type should resolve through entity import");
        assert_eq!(loc.uri, target_uri);
        assert_eq!(loc.range.start.line, 0);
    }

    #[test]
    fn cross_file_unresolvable_import_returns_none_without_panic() {
        // Verify: when the resolver returns None, we get None back with no panic.
        let source = "import nonexistent.Foo\nstructure S {\n    sub f = Foo\n}";
        let resolver = |_: &str| -> Option<(Url, String)> { None };

        // Cursor on 'Foo' in 'sub f = Foo'
        let position = Position::new(2, 12);
        let result = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver);
        assert!(
            result.is_none(),
            "unresolvable import should return None, not panic"
        );
    }

    #[test]
    fn find_declaration_in_source_with_multibyte_before_decl() {
        // End-to-end: target source has multi-byte chars before a structure declaration.
        // Parser should still find the declaration, and the name-token locator
        // should handle any tricky offsets gracefully.
        let target_source =
            "// comment with é accent\nstructure Widget {\n    param size: Length = 5mm\n}";
        let target_uri = Url::parse("file:///target.ri").unwrap();
        let result = find_declaration_in_source(target_source, "Widget", &target_uri);
        assert!(
            result.is_some(),
            "should find Widget declaration despite multi-byte chars"
        );
        let loc = result.unwrap();
        assert_eq!(loc.uri, target_uri);
        // Widget declaration is on line 1
        assert_eq!(loc.range.start.line, 1);
    }

    #[test]
    fn cross_file_single_file_behavior_unchanged_with_resolver() {
        // Verify: existing single-file goto-def behavior is unchanged when
        // a cross-file resolver is present.
        let source = reify_test_support::bracket_source();
        let resolver = |_: &str| -> Option<(Url, String)> { None };

        // Test 1: thickness ref in constraint → param declaration
        let loc = compute_goto_definition_cross_file(
            source,
            &test_uri(),
            Position::new(9, 15),
            &resolver,
        )
        .expect("thickness ref should still resolve in single-file mode");
        assert_eq!(loc.range.start.line, 3);

        // Test 2: width ref in constraint expr → param declaration
        let loc = compute_goto_definition_cross_file(
            source,
            &test_uri(),
            Position::new(10, 30),
            &resolver,
        )
        .expect("width ref should still resolve in single-file mode");
        assert_eq!(loc.range.start.line, 1);

        // Test 3: volume let → itself
        let loc =
            compute_goto_definition_cross_file(source, &test_uri(), Position::new(7, 8), &resolver)
                .expect("volume should still resolve in single-file mode");
        assert_eq!(loc.range.start.line, 7);
    }

    // --- enclosing_decl_at integration regression tests ---

    #[test]
    fn enclosing_decl_at_integration_scoped_member_in_second_decl() {
        // Verify that using enclosing_decl_at from goto_def's context
        // correctly identifies the enclosing declaration for scoped member resolution.
        use crate::analysis::enclosing_decl_at;
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param x: Bool = true\n    let y = x\n}";
        let uri = test_uri();
        let module_name = crate::analysis::module_name_from_uri(&uri);
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single(module_name));

        // Offset inside B's 'let y = x'
        let offset = source.find("let y").unwrap();
        let decl = enclosing_decl_at(&parsed.declarations, offset);
        assert!(decl.is_some(), "offset inside B should find enclosing decl");
        match decl.unwrap() {
            reify_ast::Declaration::Structure(s) => {
                assert_eq!(s.name, "B", "enclosing decl should be B");
                // Verify we can extract members from the returned declaration
                assert!(!s.members.is_empty(), "B should have members");
            }
            other => panic!("expected Structure B, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_integration_cursor_outside_returns_none() {
        // Cursor outside all declarations — enclosing_decl_at should return None,
        // and goto_def should fall back to searching all declarations.
        use crate::analysis::enclosing_decl_at;
        let source = "structure A {\n    param x: Length = 5mm\n}\nx\nstructure B {\n    param y: Length = 20mm\n}";
        let uri = test_uri();
        let module_name = crate::analysis::module_name_from_uri(&uri);
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single(module_name));

        // Offset on 'x' standalone between declarations
        let offset = source.find("\nx\n").unwrap() + 1;
        let decl = enclosing_decl_at(&parsed.declarations, offset);
        assert!(
            decl.is_none(),
            "offset outside declarations should return None"
        );

        // But goto_def should still find 'x' via fallback (line 3 is the standalone 'x')
        let loc = compute_goto_definition(source, &test_uri(), Position::new(3, 0))
            .expect("goto-def should fall back to find x in A");
        assert_eq!(loc.range.start.line, 1, "should point to A's param x");
    }

    #[test]
    fn enclosing_decl_at_integration_missing_member_falls_back() {
        // Cursor in A on 'y', which doesn't exist in A but exists in B.
        // enclosing_decl_at finds A, but member not found → falls back.
        use crate::analysis::enclosing_decl_at;
        let source = "structure A {\n    param x: Length = 5mm\n    let z = y\n}\nstructure B {\n    param y: Length = 20mm\n}";
        let uri = test_uri();
        let module_name = crate::analysis::module_name_from_uri(&uri);
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single(module_name));

        // Offset inside A
        let offset = source.find("let z").unwrap();
        let decl = enclosing_decl_at(&parsed.declarations, offset);
        assert!(decl.is_some());
        match decl.unwrap() {
            reify_ast::Declaration::Structure(s) => assert_eq!(s.name, "A"),
            _ => panic!("expected A"),
        }

        // goto_def should fall through and find y in B
        let loc = compute_goto_definition(source, &test_uri(), Position::new(2, 12))
            .expect("goto-def for y should fall back to B");
        assert_eq!(loc.range.start.line, 5, "should find y in B via fallback");
    }

    // --- step-09: injectable parsed cores over a shared ParsedModule ---

    /// `compute_goto_definition_with_parsed`, fed a `ParsedModule` built once by
    /// the caller, must return the same `Location` as the
    /// `compute_goto_definition` wrapper (which parses internally) for an
    /// in-document member reference — proving the cache-fed core is
    /// output-equivalent to the per-request path.
    #[test]
    fn compute_goto_definition_with_parsed_matches_wrapper() {
        let source = reify_test_support::bracket_source();
        let uri = test_uri();
        // 'thickness' in 'constraint thickness > 2mm' (line 9) → param on line 3.
        let position = Position::new(9, 15);

        let parsed = reify_compiler::parse_with_stdlib(
            source,
            reify_core::ModulePath::single(crate::analysis::module_name_from_uri(&uri)),
        );

        let via_parsed = compute_goto_definition_with_parsed(&parsed, source, &uri, position);
        let via_wrapper = compute_goto_definition(source, &uri, position);

        assert!(
            via_parsed.is_some(),
            "with-parsed goto-def should resolve the thickness reference"
        );
        assert_eq!(
            via_parsed, via_wrapper,
            "with-parsed goto-def must match the wrapper output"
        );
    }

    /// `compute_goto_definition_cross_file_with_parsed`, fed a `ParsedModule`
    /// built once by the caller plus an import resolver, must return the same
    /// `Location` as the `compute_goto_definition_cross_file` wrapper (which
    /// parses internally) for an imported-entity reference.
    #[test]
    fn compute_goto_definition_cross_file_with_parsed_matches_wrapper() {
        let source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        let target_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        let uri = test_uri();
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Hole' in 'sub hole = Hole' (line 2, col 16).
        let position = Position::new(2, 16);

        let parsed = reify_compiler::parse_with_stdlib(
            source,
            reify_core::ModulePath::single(crate::analysis::module_name_from_uri(&uri)),
        );

        let via_parsed = compute_goto_definition_cross_file_with_parsed(
            &parsed, source, &uri, position, &resolver,
        );
        let via_wrapper = compute_goto_definition_cross_file(source, &uri, position, &resolver);

        assert!(
            via_parsed.is_some(),
            "cross-file with-parsed goto-def should resolve the imported Hole"
        );
        assert_eq!(
            via_parsed, via_wrapper,
            "cross-file with-parsed goto-def must match the wrapper output"
        );
    }
    // --- task #6341: cross-file goto-def for type aliases ---

    /// Phase 2 must resolve an imported type-alias name to the alias's NAME token
    /// in the target file.
    #[test]
    fn goto_def_cross_file_resolves_type_alias() {
        let source = "import parts.{Speed}\nstructure S {\n    param v: Speed = 1.0\n}";
        let target_source =
            "pub type Speed = Length / Time\nstructure Widget {\n    param w: Length = 5mm\n}";
        let target_uri = parts_uri();

        let mut map = std::collections::HashMap::new();
        map.insert(
            "parts".to_string(),
            (target_uri.clone(), target_source.to_string()),
        );
        let resolver = mock_resolver(map);

        // Cursor on 'Speed' in 'param v: Speed' (line 2, col 14).
        let position = Position::new(2, 14);
        let loc = compute_goto_definition_cross_file(source, &test_uri(), position, &resolver)
            .expect("cross-file goto-def should resolve the imported type alias Speed");
        assert_eq!(loc.uri, target_uri, "should point to the target file");
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(
            loc.range.start.character,
            target_source.find("Speed").unwrap() as u32,
            "should point at the alias NAME token, not the declaration start"
        );
    }

    /// Regression pin: the SHARED helper's behaviour must stay byte-identical.
    ///
    /// `find_declaration_name_span` is `pub(crate)` and the rename/reference
    /// collectors use it to decide what is renameable. Giving it a TypeAlias arm
    /// would classify an alias as a renameable home declaration, but the use-site
    /// collectors walk expressions, not type expressions — so a rename would move
    /// the declaration token and silently miss every `param x : Alias` use.
    #[test]
    fn find_declaration_name_span_still_skips_type_alias() {
        assert!(
            find_declaration_name_span("type Speed = Length / Time\n", "Speed").is_none(),
            "the shared helper must not resolve type aliases (rename safety)"
        );
    }

    /// A declaration whose span does not contain its own name token must yield
    /// `None`, not a span borrowed from elsewhere and not a zero-width span —
    /// and the scan must NOT resume past the refusal.
    ///
    /// Tree-sitter error recovery can produce a declaration node whose span is
    /// truncated short of the name. Any span this function returns reaches the
    /// `references.rs` rename write path, so the only answer that cannot
    /// corrupt a buffer is a refusal.
    ///
    /// The fixture carries TWO same-named declarations on purpose: with a
    /// single declaration, `return None` and `continue` are indistinguishable.
    /// Here they are not — a `continue` would hand back the SECOND
    /// declaration's name token for the FIRST declaration, which is the
    /// borrow-a-token-from-elsewhere hazard that bounding the search to the
    /// declaration's own span exists to prevent.
    #[test]
    fn decl_name_span_in_refuses_without_resuming_the_declaration_scan() {
        let source = "structure Widget {\n}\nstructure Widget {\n}\n";
        let mut parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("_t"));
        assert_eq!(
            parsed.declarations.len(),
            2,
            "fixture: both same-named declarations must survive the parse"
        );

        // Truncate the FIRST declaration's span to cover only `struc`, the shape
        // an error-recovery span can take. Mutating a REAL parse keeps every
        // other field (AST node, content hash) honest.
        let mut truncated = false;
        if let reify_ast::Declaration::Structure(s) = &mut parsed.declarations[0] {
            s.span = SourceSpan::new(0, 5);
            truncated = true;
        }
        assert!(
            truncated,
            "fixture: declarations[0] must be the Structure whose span we truncate"
        );

        // Anti-vacuity: the second declaration's name token IS locatable, so a
        // scan that resumed past the refusal would have something to return.
        let reify_ast::Declaration::Structure(second) = &parsed.declarations[1] else {
            panic!("fixture: declarations[1] must be the second Structure");
        };
        let donor = name_token_span(source, second.span, "Widget");
        assert!(
            !donor.is_empty(),
            "fixture: the second declaration's name token must be locatable, \
             otherwise a resumed scan would return `None` for the wrong reason"
        );

        assert_eq!(
            decl_name_span_in(&parsed, source, "Widget", false),
            None,
            "a declaration span that excludes its own name token must be refused \
             outright: neither a span that would reach the rename write path, nor \
             the later declaration's token at {donor:?}"
        );
    }

    /// A SHORT declaration name must resolve to the declaration's NAME token,
    /// never to a character inside the leading keyword.
    ///
    /// Every declaration span starts at its keyword (`structure`, `fn`, `enum`,
    /// `trait`, `occurrence def`), and the grammar admits one-character
    /// identifiers, so a locator that is not whole-word matches the `s` of
    /// `structure` before the `s` of `structure s`. The resulting span reaches
    /// the `references.rs` rename write path, where it rewrites byte 0 of the
    /// user's file.
    #[test]
    fn find_declaration_name_span_short_name_is_name_token_not_keyword_prefix() {
        // (source, name, byte offset of the NAME token)
        let rows: &[(&str, &str, u32)] = &[
            ("structure s {\n}\n", "s", 10),
            ("fn n() -> Length {\n    1mm\n}\n", "n", 3),
            ("enum e {\n    A,\n}\n", "e", 5),
            ("trait t {\n}\n", "t", 6),
            ("occurrence def o {\n}\n", "o", 15),
            // Control: a name that cannot occur inside its keyword already works.
            ("structure Widget {\n}\n", "Widget", 10),
        ];

        for (source, name, name_offset) in rows {
            let expected = SourceSpan::new(*name_offset, name_offset + name.len() as u32);
            assert_eq!(
                &source[expected.start as usize..expected.end as usize],
                *name,
                "fixture is wrong: offset {name_offset} in {source:?} is not {name:?}"
            );
            assert_eq!(
                find_declaration_name_span(source, name),
                Some(expected),
                "declaration name {name:?} in {source:?} must resolve to its own \
                 NAME token, not to a character inside the leading keyword"
            );
        }
    }
}
