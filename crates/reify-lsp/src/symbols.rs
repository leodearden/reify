//! The purely syntactic `textDocument/documentSymbol` outline built from a
//! parsed module: one symbol per named declaration, with members, enum
//! variants and purpose-nested structures as its children.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl_oracle::test_fixtures::{NAMED_DECL_SNIPPETS, parse_one_clean};

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

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
