//! The purely syntactic `textDocument/documentSymbol` outline built from a
//! parsed module: one symbol per named declaration, with members, enum
//! variants and purpose-nested structures as its children.

use reify_ast::{Declaration, ParsedModule};
use reify_core::{ModulePath, SourceSpan};
use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind, Url};

use crate::convert::{offset_to_position, span_to_range};
use crate::decl_oracle::{decl_name_and_span, module_name_from_uri, name_token_span};

/// Compute a hierarchical [`DocumentSymbol`] tree for `source` (task 4207 η).
///
/// This is a purely SYNTACTIC outline: it parses the module via
/// [`reify_compiler::parse_with_stdlib`] (the same prelude-aware parse used by
/// goto-def) and walks `parsed.declarations` WITHOUT compiling or
/// constraint-checking. It is deliberately kept distinct from the outline's
/// semantic realization tree (`get_entity_tree`) per PRD design decision 5 —
/// the symbol list reflects declaration structure, not evaluation.
///
/// EVERY named top-level declaration is a symbol; [`symbol_kind_for`] owns the
/// per-kind mapping and is wildcard-free over all 14 `Declaration` variants.
/// The skip set is exactly the three kinds that declare no name of their own —
/// Import, Module and Default, the kinds [`decl_name_and_span`] answers `None`
/// for. Before #6533 the skip set was instead whatever a `_ => {}` wildcard
/// happened to catch, which is how Field, Purpose, Constraint, Unit and Joint
/// were silently dropped as the parser grew them.
// G-allow: LSP public API entry point; production caller uses the _in_context/_with_parsed/_from_parsed variant
pub fn compute_document_symbols(source: &str, uri: &Url) -> Vec<DocumentSymbol> {
    let module_name = module_name_from_uri(uri);
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));
    compute_document_symbols_from_parsed(&parsed, source)
}

/// Compute the [`DocumentSymbol`] tree from a pre-built [`ParsedModule`].
///
/// Injectable core shared by the per-request wrapper [`compute_document_symbols`]
/// (which parses internally) and the server's cache-fed path (which supplies the
/// per-document cached parse — one parse per edit). Walks `parsed.declarations`
/// WITHOUT compiling or constraint-checking; see [`compute_document_symbols`] for
/// the declaration→symbol mapping.
pub fn compute_document_symbols_from_parsed(
    parsed: &ParsedModule,
    source: &str,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    for decl in &parsed.declarations {
        // Name and span come from `decl_name_and_span`, the crate's single home
        // for "what name does this top-level declaration declare" — so the
        // outline cannot drift from goto-def about either. `None` is the three
        // kinds that declare no name of their own (Import, Module, Default),
        // which are exactly the kinds with no meaningful outline label.
        let Some((name, span)) = decl_name_and_span(decl) else {
            continue;
        };
        symbols.push(make_symbol(
            name,
            symbol_kind_for(decl),
            span_to_range(source, span),
            name_selection_range(source, span, name),
            children_or_none(symbol_children(source, decl)),
        ));
    }
    symbols
}

/// Map a top-level declaration to its LSP [`SymbolKind`].
///
/// Wildcard-free over all 14 `Declaration` variants, and that is the
/// load-bearing part: a new variant becomes a COMPILE ERROR here, forcing an
/// explicit kind decision. The previous shape — one 14-arm match that computed
/// kind, children, name and span together and ended in `_ => {}` — is how the
/// outline silently lost Field, Purpose, Constraint, Unit and Joint as the
/// parser grew them (#6533).
///
/// The three UNNAMED kinds are unreachable from
/// [`compute_document_symbols_from_parsed`], which skips them on
/// [`decl_name_and_span`]'s `None`. They still get explicit arms rather than a
/// shared wildcard, so admitting one to the outline later is a decision made
/// here rather than a default inherited by accident.
fn symbol_kind_for(decl: &Declaration) -> SymbolKind {
    match decl {
        Declaration::Structure(_) => SymbolKind::STRUCT,
        Declaration::Occurrence(_) => SymbolKind::CLASS,
        Declaration::Trait(_) => SymbolKind::INTERFACE,
        Declaration::Enum(_) => SymbolKind::ENUM,
        Declaration::Function(_) => SymbolKind::FUNCTION,
        // SymbolKind has no TypeAlias member; TYPE_PARAMETER is the conventional
        // LSP mapping for a type alias (rust-analyzer maps its own
        // SymbolKind::TypeAlias the same way). Task #6341.
        Declaration::TypeAlias(_) => SymbolKind::TYPE_PARAMETER,
        // A named mapping over a domain — the closest LSP analogue to a property.
        Declaration::Field(_) => SymbolKind::PROPERTY,
        // A named region enclosing other declarations, which is what NAMESPACE
        // means in LSP — and [`symbol_children`] emits those enclosed
        // declarations, so the container label is not an empty promise.
        Declaration::Purpose(_) => SymbolKind::NAMESPACE,
        // A named relation, not a value.
        Declaration::Constraint(_) => SymbolKind::OPERATOR,
        // A named fixed quantity.
        Declaration::Unit(_) => SymbolKind::CONSTANT,
        // A named parameterized construction.
        Declaration::Joint(_) => SymbolKind::METHOD,
        // Unreachable from the outline (see doc above); NULL is the LSP
        // "no meaningful kind" value, never rendered.
        Declaration::Import(_) => SymbolKind::NULL,
        Declaration::Module(_) => SymbolKind::NULL,
        Declaration::Default(_) => SymbolKind::NULL,
    }
}

/// Build a declaration's child symbols.
///
/// Only the container kinds have any: structure/occurrence/trait expose their
/// members, a purpose exposes both of its child regions, and an enum exposes
/// its variants. Every other kind is a LEAF and returns empty, which
/// [`children_or_none`] turns into `children: None` rather than `Some(vec![])`.
///
/// Deliberately a wildcard match, unlike [`symbol_kind_for`]: "has no children"
/// is the right DEFAULT for a new declaration kind, whereas "has no symbol
/// kind" is not — a new variant silently mapped to some arbitrary kind would
/// never be noticed, while missing children show up as members absent from the
/// outline.
///
/// That default is safe only while the kind's [`symbol_kind_for`] arm does not
/// ADVERTISE children. A `purpose` maps to NAMESPACE — "a named region
/// enclosing other declarations" — so falling through the wildcard rendered it
/// as a container holding nothing while its params, lets and nested
/// `structure def`s vanished from the outline entirely. It has an explicit arm
/// for that reason, and so must any future kind mapped to a container
/// `SymbolKind`.
fn symbol_children(source: &str, decl: &Declaration) -> Vec<DocumentSymbol> {
    match decl {
        Declaration::Structure(s) => members_to_symbols(source, &s.members),
        Declaration::Occurrence(o) => members_to_symbols(source, &o.members),
        Declaration::Trait(t) => members_to_symbols(source, &t.members),
        // A purpose is the one kind whose children live in TWO sibling vecs:
        // `members` (param/let/constraint/minimize) and `structures`, which the
        // parser keeps apart so `members` stays a pure `MemberDecl` list (task
        // 4639). Both are rendered and then re-interleaved by span, so the
        // outline reads in source order rather than vec order.
        Declaration::Purpose(p) => {
            let mut children = members_to_symbols(source, &p.members);
            children.extend(p.structures.iter().map(|s| structure_def_symbol(source, s)));
            children.sort_by_key(|s| (s.range.start.line, s.range.start.character));
            children
        }
        // Each variant becomes an ENUM_MEMBER child. Named-payload variant
        // fields (`Circle { radius: Length }`) are not expanded into
        // grandchildren.
        Declaration::Enum(e) => e
            .variants
            .iter()
            .map(|v| {
                make_symbol(
                    &v.name,
                    SymbolKind::ENUM_MEMBER,
                    span_to_range(source, v.span),
                    name_selection_range(source, v.span, &v.name),
                    None,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Build the [`DocumentSymbol`] for a `structure def` that is NOT a top-level
/// [`Declaration`] — today only the ones nested one level inside a `purpose`
/// body.
///
/// STRUCT is the kind [`symbol_kind_for`] gives a top-level structure, and a
/// purpose-nested one must not be labelled differently: the compiler registers
/// it in the module-level structure namespace and compiles it into the same
/// template table (the evidence is on [`crate::decl_oracle::purpose_nested_decl_names`]), and this
/// crate's navigation already resolves the two alike. Nothing in the type
/// system ties the two spellings of STRUCT together, so
/// `tests::purpose_children_nest_members_and_nested_structures` compares this
/// kind against the one a top-level structure fixture actually produces.
fn structure_def_symbol(source: &str, s: &reify_ast::StructureDef) -> DocumentSymbol {
    make_symbol(
        &s.name,
        SymbolKind::STRUCT,
        span_to_range(source, s.span),
        name_selection_range(source, s.span, &s.name),
        children_or_none(members_to_symbols(source, &s.members)),
    )
}

/// Convert a possibly-empty child list into the `children` field of a
/// [`DocumentSymbol`]: `None` when there are no navigable members, `Some(_)`
/// otherwise. Keeps leaf declarations as `children: None` rather than
/// `Some(vec![])`.
fn children_or_none(children: Vec<DocumentSymbol>) -> Option<Vec<DocumentSymbol>> {
    if children.is_empty() {
        None
    } else {
        Some(children)
    }
}

/// Recursively convert a member list into child [`DocumentSymbol`]s (task 4207 η).
///
/// Mirrors the recursion shape of [`crate::analysis::count_members_recursive`] /
/// [`reify_ast::find_named_member_span`]: `where`/`else` ([`reify_ast::MemberDecl::GuardedGroup`])
/// and match-arm ([`reify_ast::MemberDecl::MatchArmDeclGroup`]) members are
/// FLATTENED up to the owning declaration's children — no synthetic guard nodes —
/// so guarded and match-arm params/lets stay discoverable for symbol-jump. Named
/// members map as: param→FIELD, let→VARIABLE, sub→OBJECT, port→INTERFACE,
/// sketch→NAMESPACE (body nested); subs, ports and sketches form true nested
/// nodes (a sub's specialization body, a port's internal members and a sketch's
/// body become grandchildren — a sketch body is block-scoped, so it is never
/// flattened). Unlabeled constraints, connects, chains, minimize/maximize, and
/// meta blocks have no stable identifier to jump to and are skipped.
///
/// Recursion is bounded by [`reify_ast::MAX_MEMBER_NESTING_DEPTH`] to prevent
/// stack overflow on pathological input, matching the AST member-walk helpers.
fn members_to_symbols(source: &str, members: &[reify_ast::MemberDecl]) -> Vec<DocumentSymbol> {
    members_to_symbols_depth(source, members, 0)
}

fn members_to_symbols_depth(
    source: &str,
    members: &[reify_ast::MemberDecl],
    depth: usize,
) -> Vec<DocumentSymbol> {
    use reify_ast::MemberDecl;
    if depth > reify_ast::MAX_MEMBER_NESTING_DEPTH {
        return Vec::new();
    }
    let mut symbols = Vec::new();
    for member in members {
        match member {
            MemberDecl::Param(p) => symbols.push(make_symbol(
                &p.name,
                SymbolKind::FIELD,
                span_to_range(source, p.span),
                name_selection_range(source, p.span, &p.name),
                None,
            )),
            MemberDecl::Let(l) => symbols.push(make_symbol(
                &l.name,
                SymbolKind::VARIABLE,
                span_to_range(source, l.span),
                name_selection_range(source, l.span, &l.name),
                None,
            )),
            // where/else members flatten up to the owning declaration.
            MemberDecl::GuardedGroup(g) => {
                symbols.extend(members_to_symbols_depth(source, &g.members, depth + 1));
                symbols.extend(members_to_symbols_depth(source, &g.else_members, depth + 1));
            }
            // match-arm members flatten up to the owning declaration.
            MemberDecl::MatchArmDeclGroup(g) => {
                for arm in &g.arms {
                    symbols.extend(members_to_symbols_depth(
                        source,
                        std::slice::from_ref(&*arm.member),
                        depth + 1,
                    ));
                }
            }
            // sub-components nest their specialization-body members (when the
            // `sub` opens a `{ … }` body) as grandchildren; a bare `sub` is a leaf.
            MemberDecl::Sub(s) => {
                let body = s
                    .body
                    .as_ref()
                    .map(|b| members_to_symbols_depth(source, b, depth + 1))
                    .unwrap_or_default();
                symbols.push(make_symbol(
                    &s.name,
                    SymbolKind::OBJECT,
                    span_to_range(source, s.span),
                    name_selection_range(source, s.span, &s.name),
                    children_or_none(body),
                ));
            }
            // ports nest their internal members as grandchildren.
            MemberDecl::Port(port) => symbols.push(make_symbol(
                &port.name,
                SymbolKind::INTERFACE,
                span_to_range(source, port.span),
                name_selection_range(source, port.span, &port.name),
                children_or_none(members_to_symbols_depth(source, &port.members, depth + 1)),
            )),
            // A sketch is a named region enclosing its own declarations; its
            // body nests rather than flattening, because sketch-local names are
            // not visible outside the block.
            MemberDecl::Sketch(sketch) => symbols.push(make_symbol(
                &sketch.name,
                SymbolKind::NAMESPACE,
                span_to_range(source, sketch.span),
                name_selection_range(source, sketch.span, &sketch.name),
                children_or_none(members_to_symbols_depth(source, &sketch.members, depth + 1)),
            )),
            // Constraints/connects/chains/minimize/maximize/meta/relate carry no
            // identifier of their own to jump to; associated types and fns are
            // named but not outlined as members.
            _ => {}
        }
    }
    symbols
}

/// Build a [`DocumentSymbol`] with every field set explicitly.
///
/// `lsp-types` 0.94 marks `DocumentSymbol.deprecated` as `#[deprecated]` and
/// provides no `Default`, so the field cannot be omitted; centralizing the
/// `#[allow(deprecated)]` here keeps the verify/clippy pipeline clean while
/// every construction site stays a one-liner (PRD design decision 6).
#[allow(deprecated)]
fn make_symbol(
    name: &str,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol {
        name: name.to_string(),
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

/// Compute the `selection_range` (the name token) for a declaration whose full
/// span is `span` and whose declared name is `name`.
///
/// Declaration AST nodes carry only `name: String` plus the full declaration
/// span (no separate name-token span), so the name's byte offset must be
/// recovered from the source — via [`name_token_span`], the crate's single
/// name-token locator, which matches whole-word and only within the
/// declaration's own span. LSP requires `selection_range ⊆ range` and ideally on
/// the name token; [`name_token_span`]'s empty-span fallback (no whole-word
/// match inside the span) degrades to `span.start` here, which still satisfies
/// the subset invariant (defensive; should not happen for well-formed
/// declarations).
fn name_selection_range(source: &str, span: SourceSpan, name: &str) -> Range {
    let token = name_token_span(source, span, name);
    let name_offset = if token.is_empty() { span.start } else { token.start };
    // Clamp the selection end to the declaration span's end so the LSP
    // `selection_range ⊆ range` invariant holds even for degenerate or
    // error-recovery spans shorter than the name. When the name is located
    // the match already fits inside the span (`name_offset + name.len() ≤
    // span.end`), so this clamp is a no-op there; it only constrains the
    // `span.start` fallback, where `span.start + name.len()` could otherwise
    // push the selection end past `span.end` (and thus past `range.end`).
    let name_end = (name_offset + name.len() as u32).min(span.end);
    Range {
        start: offset_to_position(source, name_offset),
        end: offset_to_position(source, name_end),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl_oracle::test_fixtures::{NAMED_DECL_SNIPPETS, parse_one_clean, test_uri};

    /// Map a parsed declaration to a dense index over the NAMED `Declaration`
    /// variants — a SECOND wildcard-free match, existing only so that adding a
    /// variant is a compile error here too.
    ///
    /// [`decl_name_and_span`]'s exhaustive match forces a new variant to get an
    /// explicit named-vs-unnamed decision; it does not force the FIXTURE that
    /// exercises it. Pairing it with this index lets
    /// `named_decl_snippets_cover_every_named_kind` detect a DUPLICATE or a GAP
    /// in [`NAMED_DECL_SNIPPETS`]'s coverage, and lets that test assert the two
    /// matches AGREE on which kinds are named.
    ///
    /// What the pair still cannot see: a variant given both a named arm and the
    /// next free index but no snippet row leaves the indices dense, so the
    /// coverage assertion stays green. Closing that needs an enumeration of the
    /// variants themselves, which Rust does not offer without a derive.
    fn kind_index(decl: &Declaration) -> Option<u8> {
        match decl {
            Declaration::Structure(_) => Some(0),
            Declaration::Occurrence(_) => Some(1),
            Declaration::Enum(_) => Some(2),
            Declaration::Function(_) => Some(3),
            Declaration::Trait(_) => Some(4),
            Declaration::Field(_) => Some(5),
            Declaration::Purpose(_) => Some(6),
            Declaration::Constraint(_) => Some(7),
            Declaration::Unit(_) => Some(8),
            Declaration::TypeAlias(_) => Some(9),
            Declaration::Joint(_) => Some(10),
            Declaration::Import(_) => None,
            Declaration::Module(_) => None,
            Declaration::Default(_) => None,
        }
    }

    #[test]
    fn named_decl_snippets_cover_every_named_kind() {
        let mut seen: Vec<u8> = Vec::new();
        for (source, expected_name) in NAMED_DECL_SNIPPETS {
            let parsed = parse_one_clean(source, "test");
            let decl = &parsed.declarations[0];

            // The two wildcard-free matches must agree on which kinds are
            // named. Both force an arm for a new variant, but nothing forces
            // those arms to say the same thing: a kind admitted as named by
            // `decl_name_and_span` and mapped to None here would silently need
            // no snippet row.
            assert_eq!(
                kind_index(decl).is_some(),
                decl_name_and_span(decl).is_some(),
                "`kind_index` and `decl_name_and_span` disagree on whether this \
                 kind is named: {source}"
            );

            let index = kind_index(decl).unwrap_or_else(|| {
                panic!(
                    "row {expected_name:?} parsed to an UNNAMED declaration kind, \
                     so it covers none of the named kinds: {source}"
                )
            });
            assert!(
                !seen.contains(&index),
                "two rows parse to the same kind (index {index}), which leaves \
                 another named kind with no snippet at all: {source}"
            );
            seen.push(index);

            // Close the loop on the per-kind SymbolKind oracle too: a variant
            // newly admitted to `decl_name_and_span` + `kind_index` + this
            // table is forced BY THE COMPILER to pick a `symbol_kind_for` arm,
            // but only this assertion forces a test to state which one.
            assert!(
                OUTLINE_SYMBOL_KIND_BY_NAME
                    .iter()
                    .any(|(n, _)| n == expected_name),
                "every NAMED_DECL_SNIPPETS row needs an OUTLINE_SYMBOL_KIND_BY_NAME \
                 row, or `document_symbols_map_every_named_kind_to_its_symbol_kind` \
                 leaves {expected_name:?}'s SymbolKind unpinned: {source}"
            );
        }

        assert_eq!(
            seen.len(),
            NAMED_DECL_SNIPPETS.len(),
            "every row must contribute a kind index"
        );
        // The converse direction. `named_decl_snippet` already panics on a kind
        // row with no snippet, but only for rows the outline test reaches; this
        // makes the two tables the same SIZE, so neither can carry a row the
        // other lacks.
        assert_eq!(
            OUTLINE_SYMBOL_KIND_BY_NAME.len(),
            NAMED_DECL_SNIPPETS.len(),
            "the snippet table and the SymbolKind oracle must hold exactly one \
             row per named kind each"
        );
        seen.sort_unstable();
        assert_eq!(
            seen,
            (0..seen.len() as u8).collect::<Vec<u8>>(),
            "NAMED_DECL_SNIPPETS must hold exactly one snippet per NAMED \
             Declaration variant, with no gap in `kind_index`'s numbering. A \
             variant newly admitted to `decl_name_and_span` and `kind_index` \
             needs a row here too, or \
             `decl_name_and_span_returns_name_and_span_for_every_named_kind` and \
             `goto_def::tests::goto_def_cursor_on_declaration_name_resolves_for_every_kind` \
             silently under-cover it."
        );
    }

    // --- compute_document_symbols tests (task 4207 η) ---

    #[test]
    fn compute_document_symbols_single_structure_top_level() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = "structure Bracket { param width: Length = 80mm }";
        let symbols = compute_document_symbols(source, &test_uri());
        assert_eq!(
            symbols.len(),
            1,
            "should return exactly one top-level symbol for one structure"
        );
        let bracket = &symbols[0];
        assert_eq!(bracket.name, "Bracket");
        assert_eq!(bracket.kind, SymbolKind::STRUCT);
        // range covers the full declaration, which starts on line 0.
        assert_eq!(bracket.range.start.line, 0);
        // selection_range is the "Bracket" name token: line 0, chars 10..17
        // ("structure " is 10 chars; "Bracket" is 7 chars).
        assert_eq!(bracket.selection_range.start.line, 0);
        assert_eq!(bracket.selection_range.start.character, 10);
        assert_eq!(bracket.selection_range.end.line, 0);
        assert_eq!(bracket.selection_range.end.character, 17);
    }

    /// Regression (task 4207 η amendment): `name_selection_range` must preserve
    /// the LSP `selection_range ⊆ range` invariant even for a degenerate span
    /// shorter than the name. Here the span is 2 bytes wide but the name is 7
    /// bytes, so the word-boundary search fails and the `span.start` fallback
    /// runs; without the end-clamp the selection end would be `0 + 7 = 7`,
    /// pushing `selection_range.end` past `range.end`.
    #[test]
    fn name_selection_range_clamps_end_to_span_for_degenerate_span() {
        let source = "Bracket";
        let span = SourceSpan::new(0, 2);
        let sel = name_selection_range(source, span, "Bracket");
        let range = span_to_range(source, span);
        assert!(
            (sel.start.line, sel.start.character) >= (range.start.line, range.start.character),
            "selection_range.start {:?} must be >= range.start {:?}",
            sel.start,
            range.start,
        );
        assert!(
            (sel.end.line, sel.end.character) <= (range.end.line, range.end.character),
            "selection_range.end {:?} must be clamped within range.end {:?}",
            sel.end,
            range.end,
        );
    }

    /// Assert `sym.selection_range` lies within `sym.range` and covers exactly
    /// `sym.name` in `source`. Shared across the document-symbol tests.
    fn assert_selection_on_name(source: &str, sym: &DocumentSymbol) {
        let r = &sym.range;
        let sr = &sym.selection_range;
        assert!(
            (sr.start.line, sr.start.character) >= (r.start.line, r.start.character),
            "selection_range.start {:?} should be >= range.start {:?} for {}",
            sr.start,
            r.start,
            sym.name
        );
        assert!(
            (sr.end.line, sr.end.character) <= (r.end.line, r.end.character),
            "selection_range.end {:?} should be <= range.end {:?} for {}",
            sr.end,
            r.end,
            sym.name
        );
        let start = crate::convert::position_to_offset(source, sr.start);
        let end = crate::convert::position_to_offset(source, sr.end);
        assert_eq!(
            &source[start..end],
            sym.name,
            "selection_range should cover the name token for {}",
            sym.name
        );
    }

    #[test]
    fn compute_document_symbols_members_as_children() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = reify_test_support::bracket_source();
        let symbols = compute_document_symbols(source, &test_uri());
        assert_eq!(symbols.len(), 1, "bracket_source has one structure");
        let bracket = &symbols[0];
        assert_eq!(bracket.name, "Bracket");

        let children = bracket
            .children
            .as_ref()
            .expect("Bracket should have children");
        // 5 params + 2 lets = 7 named members; the 3 unlabeled constraints
        // are NOT navigable symbols and must be excluded.
        assert_eq!(
            children.len(),
            7,
            "expected 7 named members (constraints excluded), got: {:?}",
            children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>()
        );

        // Source order: the 5 params, then volume, then body.
        let expected: [(&str, SymbolKind); 7] = [
            ("width", SymbolKind::FIELD),
            ("height", SymbolKind::FIELD),
            ("thickness", SymbolKind::FIELD),
            ("fillet_radius", SymbolKind::FIELD),
            ("hole_diameter", SymbolKind::FIELD),
            ("volume", SymbolKind::VARIABLE),
            ("body", SymbolKind::VARIABLE),
        ];
        for (child, (name, kind)) in children.iter().zip(expected.iter()) {
            assert_eq!(&child.name, name, "child name/order mismatch");
            assert_eq!(child.kind, *kind, "child {name} kind mismatch");
            assert!(
                child.children.is_none() || child.children.as_ref().unwrap().is_empty(),
                "leaf member {name} should have no children"
            );
            assert_selection_on_name(source, child);
        }
    }

    #[test]
    fn compute_document_symbols_occurrence_and_trait() {
        use tower_lsp::lsp_types::SymbolKind;

        // --- occurrence → CLASS with param FIELD + let VARIABLE children ---
        let occ_source =
            "occurrence def Joint {\n    param diameter: Length = 10mm\n    let radius = diameter / 2\n}";
        let occ_symbols = compute_document_symbols(occ_source, &test_uri());
        assert_eq!(occ_symbols.len(), 1, "one occurrence → one top-level symbol");
        let joint = &occ_symbols[0];
        assert_eq!(joint.name, "Joint");
        assert_eq!(joint.kind, SymbolKind::CLASS);
        assert_selection_on_name(occ_source, joint);
        let occ_children = joint
            .children
            .as_ref()
            .expect("Joint should have children");
        assert_eq!(occ_children.len(), 2, "Joint has diameter + radius");
        assert_eq!(occ_children[0].name, "diameter");
        assert_eq!(occ_children[0].kind, SymbolKind::FIELD);
        assert_eq!(occ_children[1].name, "radius");
        assert_eq!(occ_children[1].kind, SymbolKind::VARIABLE);
        for c in occ_children {
            assert_selection_on_name(occ_source, c);
        }

        // --- trait → INTERFACE with param FIELD child ---
        let trait_source = "trait Rigid {\n    param mass: Length = 5mm\n}";
        let trait_symbols = compute_document_symbols(trait_source, &test_uri());
        assert_eq!(trait_symbols.len(), 1, "one trait → one top-level symbol");
        let rigid = &trait_symbols[0];
        assert_eq!(rigid.name, "Rigid");
        assert_eq!(rigid.kind, SymbolKind::INTERFACE);
        assert_selection_on_name(trait_source, rigid);
        let trait_children = rigid
            .children
            .as_ref()
            .expect("Rigid should have children");
        assert_eq!(trait_children.len(), 1, "Rigid has mass");
        assert_eq!(trait_children[0].name, "mass");
        assert_eq!(trait_children[0].kind, SymbolKind::FIELD);
        assert_selection_on_name(trait_source, &trait_children[0]);
    }

    #[test]
    fn compute_document_symbols_enum_variants() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = "enum Shape { Point, Circle { radius: Length } }";
        let symbols = compute_document_symbols(source, &test_uri());
        assert_eq!(symbols.len(), 1, "one enum → one top-level symbol");
        let shape = &symbols[0];
        assert_eq!(shape.name, "Shape");
        assert_eq!(shape.kind, SymbolKind::ENUM);
        assert_selection_on_name(source, shape);

        let variants = shape
            .children
            .as_ref()
            .expect("Shape should have variant children");
        assert_eq!(variants.len(), 2, "Shape has Point + Circle in source order");
        assert_eq!(variants[0].name, "Point");
        assert_eq!(variants[0].kind, SymbolKind::ENUM_MEMBER);
        assert_eq!(variants[1].name, "Circle");
        assert_eq!(variants[1].kind, SymbolKind::ENUM_MEMBER);
        for v in variants {
            // range covers the variant span; selection_range sits on the name.
            assert_selection_on_name(source, v);
            assert!(
                v.children.is_none() || v.children.as_ref().unwrap().is_empty(),
                "variant {} should have no grandchildren in this task",
                v.name
            );
        }
    }

    /// RENAMED from `compute_document_symbols_fn_and_excludes_non_symbol_decls`
    /// by #6972, which is also why its `unit` assertion inverted. The old name
    /// and body encoded the pre-#6533 skip set — "import + unit are NOT
    /// navigable symbols" — which was never a rule, only the reach of a `_ => {}`
    /// wildcard. The skip set is now exactly the kinds that declare no name of
    /// their own, which `decl_name_and_span` decides; `import` is one, `unit` is
    /// not.
    #[test]
    fn compute_document_symbols_excludes_only_unnamed_decls() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = "import std.math\nunit hoop : Length\nfn area(w: Length) -> Length { w }";
        let symbols = compute_document_symbols(source, &test_uri());
        assert_eq!(
            symbols
                .iter()
                .map(|s| (s.name.as_str(), s.kind))
                .collect::<Vec<_>>(),
            vec![
                ("hoop", SymbolKind::CONSTANT),
                ("area", SymbolKind::FUNCTION)
            ],
            "the import is skipped as an unnamed kind; the unit and the fn are \
             symbols, in source order"
        );
        let area = &symbols[1];
        assert!(
            area.children.is_none() || area.children.as_ref().unwrap().is_empty(),
            "fn is a leaf symbol (params are not surfaced as children)"
        );
        assert_selection_on_name(source, area);
        assert_selection_on_name(source, &symbols[0]);
    }

    // --- task #6341: type aliases as document symbols ---

    /// A top-level `type` alias is a navigable symbol and a LEAF (no children),
    /// per the `children_or_none` convention.
    #[test]
    fn document_symbols_include_type_alias() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = "type Speed = Length / Time\n";
        let uri = test_uri();
        let parsed = reify_compiler::parse_with_stdlib(
            source,
            ModulePath::single(module_name_from_uri(&uri)),
        );
        let symbols = compute_document_symbols_from_parsed(&parsed, source);
        assert_eq!(
            symbols.len(),
            1,
            "one type alias \u{2192} one top-level symbol, got: {:?}",
            symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
        );
        let speed = &symbols[0];
        assert_eq!(speed.name, "Speed");
        assert_eq!(speed.kind, SymbolKind::TYPE_PARAMETER);
        assert!(
            speed.children.is_none(),
            "a type alias is a leaf symbol, got: {:?}",
            speed.children
        );
        // selection_range covers just the name token, not the whole declaration.
        assert_eq!(speed.selection_range.start.line, 0);
        assert_eq!(
            speed.selection_range.start.character,
            source.find("Speed").unwrap() as u32
        );
        assert_selection_on_name(source, speed);
    }

    #[test]
    fn document_symbols_include_type_alias_alongside_structure() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = "type Speed = Length / Time\nstructure S {\n    param v: Speed = 1.0\n}";
        let uri = test_uri();
        let parsed = reify_compiler::parse_with_stdlib(
            source,
            ModulePath::single(module_name_from_uri(&uri)),
        );
        let symbols = compute_document_symbols_from_parsed(&parsed, source);
        assert_eq!(
            symbols.len(),
            2,
            "alias + structure \u{2192} two top-level symbols, got: {:?}",
            symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
        );
        assert_eq!(
            symbols
                .iter()
                .map(|s| (s.name.as_str(), s.kind))
                .collect::<Vec<_>>(),
            vec![
                ("Speed", SymbolKind::TYPE_PARAMETER),
                ("S", SymbolKind::STRUCT)
            ],
            "symbols must be in source order"
        );
    }

    // --- task #6533: the outline's per-kind SymbolKind mapping ---

    /// The single per-kind `SymbolKind` ORACLE: every one of the eleven named
    /// `Declaration` kinds, keyed by the name its [`NAMED_DECL_SNIPPETS`] row
    /// declares.
    ///
    /// It holds ALL of them, not just the five #6533 restored, because
    /// [`symbol_kind_for`]'s wildcard-free match forces a new variant to PICK a
    /// kind but nothing forces a test to state WHICH — so a silent
    /// STRUCT→OBJECT flip on an already-admitted kind had no oracle to red.
    /// `named_decl_snippets_cover_every_named_kind` asserts this table and the
    /// snippet table stay row-for-row aligned, so a newly-named variant needs a
    /// row in both or that test fails.
    const OUTLINE_SYMBOL_KIND_BY_NAME: &[(&str, SymbolKind)] = &[
        // `structure S { … }` — the archetypal record of members.
        ("S", SymbolKind::STRUCT),
        // `occurrence def Welding { … }` — an instantiable kind of happening.
        ("Welding", SymbolKind::CLASS),
        // `enum Dir { In, Out }` — LSP's own enum.
        ("Dir", SymbolKind::ENUM),
        // `fn id_length(…) -> Length` — LSP's own function.
        ("id_length", SymbolKind::FUNCTION),
        // `trait Rigid { … }` — a named contract over members.
        ("Rigid", SymbolKind::INTERFACE),
        // `field def temp : Point3 -> Real` — a named mapping over a domain, the
        // closest LSP analogue to a property.
        ("temp", SymbolKind::PROPERTY),
        // `purpose lightweight(...)` — a named region enclosing other
        // declarations, which is what NAMESPACE means in LSP.
        ("lightweight", SymbolKind::NAMESPACE),
        // `constraint def Foo { x > 0 }` — a named relation, not a value.
        ("Foo", SymbolKind::OPERATOR),
        // `unit meter : Length` — a named fixed quantity.
        ("meter", SymbolKind::CONSTANT),
        // `type Pressure = Force` — SymbolKind has no TypeAlias member;
        // TYPE_PARAMETER is the conventional LSP mapping (#6341).
        ("Pressure", SymbolKind::TYPE_PARAMETER),
        // `joint ball(...)` — a named parameterized construction.
        ("ball", SymbolKind::METHOD),
    ];

    /// Look up the shared-table snippet that declares `name`.
    ///
    /// Panics rather than returning an Option: a miss means
    /// [`OUTLINE_SYMBOL_KIND_BY_NAME`] and [`NAMED_DECL_SNIPPETS`] have drifted
    /// apart, which must fail loudly here rather than silently shrink the loop
    /// below.
    fn named_decl_snippet(name: &str) -> &'static str {
        NAMED_DECL_SNIPPETS
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(src, _)| *src)
            .unwrap_or_else(|| panic!("no NAMED_DECL_SNIPPETS row declares {name:?}"))
    }

    #[test]
    fn document_symbols_map_every_named_kind_to_its_symbol_kind() {
        for (name, expected_kind) in OUTLINE_SYMBOL_KIND_BY_NAME {
            let source = named_decl_snippet(name);
            let parsed = parse_one_clean(source, "test");
            let symbols = compute_document_symbols_from_parsed(&parsed, source);

            assert_eq!(
                symbols.len(),
                1,
                "one declaration \u{2192} one top-level symbol for {source:?}, got: {:?}",
                symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
            );
            let sym = &symbols[0];
            assert_eq!(sym.name, *name, "symbol name mismatch for {source:?}");
            assert_eq!(
                sym.kind, *expected_kind,
                "symbol kind mismatch for {source:?}"
            );
            // LSP requires selection_range to be contained in range and to sit on
            // the name token; the helper asserts both.
            assert_selection_on_name(source, sym);
        }
    }

    /// A `purpose` is labelled NAMESPACE — "a named region enclosing other
    /// declarations" — so the outline must actually show what it encloses.
    /// Before this, `symbol_children`'s wildcard gave it ZERO children, and both
    /// of its child regions (`members` and the nested `structure def`s this
    /// sweep made file-wide navigable and renameable, #6534) vanished from the
    /// document outline while the container label stayed.
    ///
    /// Also the SPOT guard for the nested structure's kind: STRUCT is spelled
    /// once in `symbol_kind_for` and once in `structure_def_symbol`, with
    /// nothing in the type system tying them together, so the expected kind here
    /// is READ OFF a top-level structure fixture rather than restated.
    #[test]
    fn purpose_children_nest_members_and_nested_structures() {
        // `let`, not `param`: grammar.js's `purpose_member` admits
        // constraint/let/minimize/maximize/guarded/default/pragma/structure —
        // a `param` in a purpose body lowers to no member at all.
        const SRC: &str = "purpose Exploration() {\n    \
                           let budget = 5mm\n    \
                           structure def InPurpose {\n        \
                           param x : Length = 5mm\n    \
                           }\n\
                           }";
        let parsed = parse_one_clean(SRC, "test");
        // Non-vacuity: both child regions must really be populated, or the
        // assertions below could pass against an empty purpose.
        let Declaration::Purpose(p) = &parsed.declarations[0] else {
            panic!("fixture must parse to a purpose");
        };
        assert_eq!(p.members.len(), 1, "fixture must carry one member");
        assert_eq!(p.structures.len(), 1, "fixture must nest one structure");

        let symbols = compute_document_symbols_from_parsed(&parsed, SRC);
        assert_eq!(symbols.len(), 1, "one purpose \u{2192} one top-level symbol");
        let purpose = &symbols[0];
        assert_eq!(purpose.kind, SymbolKind::NAMESPACE);
        let children = purpose
            .children
            .as_ref()
            .expect("a NAMESPACE symbol must not render as an empty container");

        // Source order across the two sibling vecs: the param is declared first.
        assert_eq!(
            children
                .iter()
                .map(|c| (c.name.as_str(), c.kind))
                .collect::<Vec<_>>(),
            vec![
                ("budget", SymbolKind::VARIABLE),
                ("InPurpose", top_level_structure_symbol_kind()),
            ],
            "a purpose's children are its members and its nested structures, in \
             source order"
        );

        // The nested structure is a container in its own right, exactly as a
        // top-level one is.
        let nested = &children[1];
        assert_eq!(
            nested
                .children
                .as_ref()
                .expect("a nested structure exposes its own members")
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["x"]
        );
        assert_selection_on_name(SRC, nested);
    }

    /// The `SymbolKind` a TOP-LEVEL `structure` actually yields, read off the
    /// shared snippet table so `purpose_children_nest_members_and_nested_structures`
    /// compares against the live mapping instead of a second hard-coded STRUCT.
    fn top_level_structure_symbol_kind() -> SymbolKind {
        let source = named_decl_snippet("S");
        let parsed = parse_one_clean(source, "test");
        compute_document_symbols_from_parsed(&parsed, source)[0].kind
    }

    /// The complement. The three kinds `decl_name_and_span` answers `None` for
    /// declare no name of their own, so they must emit NOTHING — admitting one
    /// would put a symbol with no meaningful label in the outline.
    #[test]
    fn document_symbols_exclude_every_unnamed_kind() {
        for source in ["import parts.Hole", "module a.b", "default Material = 1"] {
            let parsed = reify_syntax::parse(source, ModulePath::single("test"));
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
            assert!(
                decl_name_and_span(&parsed.declarations[0]).is_none(),
                "fixture must parse to an UNNAMED kind, or this asserts nothing: {source}"
            );

            let symbols = compute_document_symbols_from_parsed(&parsed, source);
            assert!(
                symbols.is_empty(),
                "an unnamed declaration must emit no symbol for {source:?}, got: {:?}",
                symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
            );
        }
    }

    /// The outline's forcing function, and the reason the kind list cannot
    /// silently rot again.
    ///
    /// Drives the WHOLE shared [`NAMED_DECL_SNIPPETS`] table rather than a
    /// hand-listed subset, so a kind newly admitted to
    /// [`decl_name_and_span`] — which `named_decl_snippets_cover_every_named_kind`
    /// already forces a table row for — is exercised here the moment that row
    /// lands, with no edit to this test. THIS TEST IS WHAT A NEWLY-ADMITTED
    /// VARIANT MUST SATISFY: give it a `symbol_kind_for` arm (the compiler
    /// demands that much) and a `NAMED_DECL_SNIPPETS` row, and the outline is
    /// covered.
    ///
    /// It pins the WIRING, not the per-kind `SymbolKind` choice, which is
    /// `document_symbols_map_every_named_kind_to_its_symbol_kind`'s job: every named
    /// declaration yields exactly one top-level symbol whose name, range and
    /// selection_range are the ones `decl_name_and_span` + `name_selection_range`
    /// produce. A future refactor that recomputes any of those three locally —
    /// the SPOT violation #6533 exists to undo — reds here.
    #[test]
    fn every_named_decl_snippet_yields_one_symbol_agreeing_with_decl_name_and_span() {
        for (source, expected_name) in NAMED_DECL_SNIPPETS {
            let parsed = parse_one_clean(source, "test");
            let decl = &parsed.declarations[0];
            let (name, span) = decl_name_and_span(decl).unwrap_or_else(|| {
                panic!("NAMED_DECL_SNIPPETS row must parse to a NAMED kind: {source}")
            });

            let symbols = compute_document_symbols_from_parsed(&parsed, source);
            assert_eq!(
                symbols.len(),
                1,
                "one named declaration \u{2192} one top-level symbol for {source:?}, got: {:?}",
                symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
            );
            let sym = &symbols[0];
            assert_eq!(sym.name, *expected_name, "symbol name for {source:?}");
            assert_eq!(
                sym.name, name,
                "the symbol's name must be decl_name_and_span's, not a local \
                 recomputation: {source:?}"
            );
            assert_eq!(
                sym.range,
                span_to_range(source, span),
                "the symbol's range must be decl_name_and_span's span: {source:?}"
            );
            assert_eq!(
                sym.selection_range,
                name_selection_range(source, span, name),
                "the symbol's selection_range must be name_selection_range's: {source:?}"
            );
            assert_selection_on_name(source, sym);
        }
    }

    #[test]
    fn compute_document_symbols_sub_port_and_guarded() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = r#"structure Assembly {
    param cond : Bool = true
    sub motor = Motor()
    port intake : MechPort { param d : Length = 5mm }
    where cond {
        param guarded_x : Length = 1mm
    }
}"#;
        let symbols = compute_document_symbols(source, &test_uri());
        assert_eq!(symbols.len(), 1, "one structure → one top-level symbol");
        let assembly = &symbols[0];
        assert_eq!(assembly.name, "Assembly");
        let children = assembly
            .children
            .as_ref()
            .expect("Assembly should have children");
        let find = |name: &str| children.iter().find(|c| c.name == name);

        // sub motor → OBJECT
        let motor = find("motor").expect("motor sub should be a child symbol");
        assert_eq!(motor.kind, SymbolKind::OBJECT);
        assert_selection_on_name(source, motor);

        // port intake → INTERFACE, with port-internal param d → FIELD grandchild
        let intake = find("intake").expect("intake port should be a child symbol");
        assert_eq!(intake.kind, SymbolKind::INTERFACE);
        assert_selection_on_name(source, intake);
        let intake_children = intake
            .children
            .as_ref()
            .expect("intake port should have internal members");
        let d = intake_children
            .iter()
            .find(|c| c.name == "d")
            .expect("port-internal param d should be a grandchild");
        assert_eq!(d.kind, SymbolKind::FIELD);
        assert_selection_on_name(source, d);

        // where-block param guarded_x flattened as a direct child of the structure
        let guarded = find("guarded_x").expect("guarded_x should be flattened as a direct child");
        assert_eq!(guarded.kind, SymbolKind::FIELD);
        assert_selection_on_name(source, guarded);
    }

    /// A `sketch { … }` block is a named region: it appears in the outline as a
    /// NAMESPACE whose body NESTS under it. Sketch-local names are not visible
    /// outside the block (PRD `docs/prds/v0_6/constrained-2d-sketch.md` §7 C1),
    /// so — unlike guarded members — they are not flattened up to the structure.
    #[test]
    fn compute_document_symbols_nests_sketch_body_under_the_sketch() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = "structure def T {\n    param w : Length = 5mm\n    sketch profile {\n        let a = point(0mm, 0mm)\n        fix(a)\n    }\n}";
        let parsed = parse_one_clean(source, "test");
        let symbols = compute_document_symbols_from_parsed(&parsed, source);
        assert_eq!(symbols.len(), 1, "one structure → one top-level symbol");
        let children = symbols[0]
            .children
            .as_ref()
            .expect("T should have children");

        let shape = |syms: &[DocumentSymbol]| -> Vec<(String, SymbolKind)> {
            syms.iter().map(|s| (s.name.clone(), s.kind)).collect()
        };
        assert_eq!(
            shape(children),
            vec![
                ("w".to_string(), SymbolKind::FIELD),
                ("profile".to_string(), SymbolKind::NAMESPACE),
            ],
            "the structure's children, in source order, are the param and the sketch — \
             the sketch-local `a` must not be flattened up to the structure"
        );

        let profile = &children[1];
        assert_selection_on_name(source, profile);
        let profile_children = profile
            .children
            .as_ref()
            .expect("the sketch's body members must nest under it");
        assert_eq!(
            shape(profile_children),
            vec![("a".to_string(), SymbolKind::VARIABLE)],
            "the sketch body's `let a` nests under the sketch; the bare relation `fix(a)` \
             has no identifier and emits nothing"
        );
    }

    // --- step-11: injectable document-symbol core over a shared ParsedModule ---

    /// `compute_document_symbols_from_parsed`, fed a `ParsedModule` built once
    /// by the caller, must yield the same symbol tree as the
    /// `compute_document_symbols` wrapper (which parses internally) — proving
    /// the cache-fed core is output-equivalent to the per-request path.
    #[test]
    fn compute_document_symbols_from_parsed_matches_wrapper() {
        // Multi-declaration source spanning every navigable symbol kind plus
        // members, so the equivalence covers names, kinds, ranges, and the
        // nested children tree — not just the top-level list.
        let source = r#"structure Bracket {
    param width : Length = 80mm
    sub motor = Motor()
}
occurrence def Joint {
    param diameter : Length = 10mm
}
trait Rigid {
    param mass : Length = 5mm
}
enum Shape { Circle, Square }
fn area(w: Length) -> Length { w }"#;
        let uri = test_uri();

        let parsed = reify_compiler::parse_with_stdlib(
            source,
            ModulePath::single(module_name_from_uri(&uri)),
        );

        let via_parsed = compute_document_symbols_from_parsed(&parsed, source);
        let via_wrapper = compute_document_symbols(source, &uri);

        assert!(
            !via_parsed.is_empty(),
            "multi-declaration source should yield symbols"
        );
        assert_eq!(
            via_parsed, via_wrapper,
            "from-parsed document symbols must match the wrapper (names/kinds/ranges/children)"
        );
    }
}
