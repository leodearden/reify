//! Tests for import declaration parsing with dot-path syntax.

use reify_ast::{ImportDecl, ImportKind};

use crate::parse_error_lookup::only_error_starting_with;

// ── Step 1: Basic dot-path module import ──────────────────────────

#[test]
fn parse_basic_module_import() {
    let source = "import std.math";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let imports: Vec<&ImportDecl> = parsed
        .declarations
        .iter()
        .filter_map(|d| {
            if let reify_ast::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].path, "std.math");
    assert_eq!(imports[0].kind, ImportKind::Module);
    assert!(!imports[0].is_pub);
}

#[test]
fn parse_deep_module_import() {
    let source = "import std.mechanical.fasteners";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let import = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_ast::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .expect("should have an import");

    assert_eq!(import.path, "std.mechanical.fasteners");
    assert_eq!(import.kind, ImportKind::Module);
}

// ── Step 3: Entity import ─────────────────────────────────────────

/// Entity import: last segment starts with uppercase → Entity kind.
/// Module path = everything except the last segment.
#[test]
fn parse_entity_import() {
    let source = "import std.math.Sqrt";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let import = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_ast::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .expect("should have an import");

    assert_eq!(import.path, "std.math");
    assert_eq!(import.kind, ImportKind::Entity("Sqrt".to_string()));
}

// ── Step 5: Destructured import ───────────────────────────────────

#[test]
fn parse_destructured_import() {
    let source = "import std.mech.{Bolt, Nut}";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let import = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_ast::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .expect("should have an import");

    assert_eq!(import.path, "std.mech");
    assert_eq!(
        import.kind,
        ImportKind::Destructured(vec!["Bolt".to_string(), "Nut".to_string()])
    );
}

#[test]
fn parse_destructured_import_single_item() {
    let source = "import std.mech.{Bolt}";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let import = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_ast::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .expect("should have an import");

    assert_eq!(
        import.kind,
        ImportKind::Destructured(vec!["Bolt".to_string()])
    );
}

/// The SPACED form `import a.b {C, D}` is NOT Reify and must be a parse error.
///
/// The canonical destructured form is the DOTTED `import a.b.{C, D}`, per the
/// `import_path` production in `docs/reify-language-spec.md` §15 "Grammar
/// Summary", which makes the `'.'` an explicit terminal before the brace list
/// (#5931).
///
/// An ERROR nested inside `import_declaration` is refused as `invalid import: …`,
/// so the `errors.is_empty()` assertions in the two tests above pin the dotted
/// separator at AST level; the CST-level pins live in
/// tree-sitter-reify/tests/import_items_grammar_tests.rs. The spaced form's
/// stray `{...}` is a sibling ERROR at `source_file` level.
#[test]
fn spaced_destructured_import_is_rejected() {
    let source = "import std.mech {Bolt, Nut}";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        !parsed.errors.is_empty(),
        "`{source}` (space instead of `.`) must be a parse error — the canonical \
         destructured form is `import a.b.{{C, D}}` per \
         docs/reify-language-spec.md §15's `import_path`; got declarations: {:?}",
        parsed.declarations
    );
}

/// The empty and trailing-comma item lists are DELIBERATE LATITUDE at the
/// grammar level — §15's EBNF (`'{' IDENT (',' IDENT)* '}'`) describes neither,
/// but `commaSep` in grammar.js and the Lezer port's
/// `(Identifier ("," Identifier)* ","?)?` both admit them, and
/// `empty_and_trailing_comma_item_lists_are_deliberate_latitude` in
/// tree-sitter-reify/tests/import_items_grammar_tests.rs records why that is
/// kept rather than tightened.
///
/// This pins what the two shapes LOWER to, which no grammar test can see: an
/// empty list stays `Destructured` with no names — a vacuous import, NOT
/// `ImportKind::Module` and not a parse error — and a trailing comma
/// contributes no phantom name. Both follow from `lower_import` keeping only
/// the `identifier` children of the `items` field, so a change to that loop
/// (or to which field selects the kind) shows up here.
#[test]
fn empty_and_trailing_comma_destructured_imports_lower_as_written() {
    // The single `ImportDecl` in `source`, with the parse asserted clean.
    fn single_import(source: &str) -> ImportDecl {
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
        assert!(
            parsed.errors.is_empty(),
            "`{source}` must parse cleanly; parse errors: {:?}",
            parsed.errors
        );
        parsed
            .declarations
            .iter()
            .find_map(|d| {
                if let reify_ast::Declaration::Import(i) = d {
                    Some(i.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                panic!(
                    "`{source}` should have an import; got declarations: {:?}",
                    parsed.declarations
                )
            })
    }

    let empty = single_import("import a.{}");
    assert_eq!(empty.path, "a");
    assert_eq!(
        empty.kind,
        ImportKind::Destructured(vec![]),
        "an empty item list stays Destructured — a vacuous import, not Module"
    );

    let trailing = single_import("import a.{Foo,}");
    assert_eq!(
        trailing.kind,
        ImportKind::Destructured(vec!["Foo".to_string()]),
        "a trailing comma must not contribute a phantom name"
    );
}

// ── Step 7: Aliased module import ─────────────────────────────────

#[test]
fn parse_aliased_module_import() {
    let source = "import std.mech as m";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let import = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_ast::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .expect("should have an import");

    assert_eq!(import.path, "std.mech");
    assert_eq!(
        import.kind,
        ImportKind::Aliased {
            alias: "m".to_string()
        }
    );
}

// ── Step 9: Entity aliased import ─────────────────────────────────

#[test]
fn parse_entity_aliased_import() {
    let source = "import std.mech.Bolt as StdBolt";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let import = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_ast::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .expect("should have an import");

    assert_eq!(import.path, "std.mech");
    assert_eq!(
        import.kind,
        ImportKind::EntityAliased {
            entity: "Bolt".to_string(),
            alias: "StdBolt".to_string(),
        }
    );
}

// ── Step 11: Pub import (re-export) ───────────────────────────────

#[test]
fn parse_pub_import() {
    let source = "pub import internal.Helper";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let import = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_ast::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .expect("should have an import");

    assert!(import.is_pub);
    assert_eq!(import.path, "internal");
    assert_eq!(import.kind, ImportKind::Entity("Helper".to_string()));
}

#[test]
fn parse_pub_module_import() {
    let source = "pub import std.math";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let import = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_ast::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .expect("should have an import");

    assert!(import.is_pub);
    assert_eq!(import.path, "std.math");
    assert_eq!(import.kind, ImportKind::Module);
}

// ── Content hash ──────────────────────────────────────────────────

#[test]
fn import_has_content_hash() {
    let source = "import std.math";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let import = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_ast::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .expect("should have an import");

    // Content hash should be non-zero (not default)
    let zero = reify_core::ContentHash::of_str("");
    assert_ne!(import.content_hash, zero, "content_hash should be computed");
}

// ── Malformed imports are refused ─────────────────────────────────

/// The `ImportDecl`s lowered from `parsed`, in source order.
fn imports_of(parsed: &reify_ast::ParsedModule) -> Vec<&ImportDecl> {
    parsed
        .declarations
        .iter()
        .filter_map(|d| match d {
            reify_ast::Declaration::Import(i) => Some(i),
            _ => None,
        })
        .collect()
}

/// An import whose CST carries a nested ERROR or MISSING node no longer matches its source
/// once lowered (e.g. `import a.b.{C D}` would lower to `Destructured([C])`, dropping `D`), so
/// it is refused with one `invalid import: ` diagnostic located at its first fault.
#[test]
fn import_with_a_nested_fault_is_refused_at_its_first_fault() {
    let stray_item = "import a.b.{C D}";
    let unclosed_items = "import a.b.{C, D";
    let missing_segment = "import a.b.";
    let missing_path = "pub import";
    let cases = [
        (stray_item, stray_item.find(" D").unwrap() + 1),
        (unclosed_items, unclosed_items.len()),
        (missing_segment, missing_segment.len()),
        (missing_path, missing_path.len()),
    ];
    for (source, fault_offset) in cases {
        let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
        let error = only_error_starting_with(&parsed.errors, "invalid import: ");
        assert_eq!(
            error.span.start as usize, fault_offset,
            "`{source}`: expected the diagnostic at byte {fault_offset}, got: {error:?}"
        );
        assert!(
            imports_of(&parsed).is_empty(),
            "`{source}`: a refused import must not be lowered, got declarations: {:?}",
            parsed.declarations
        );
    }
}

#[test]
fn a_refused_import_does_not_take_its_well_formed_neighbour_with_it() {
    let source = "import a.b.{C D}\nimport c.d";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    only_error_starting_with(&parsed.errors, "invalid import: ");

    let imports = imports_of(&parsed);
    assert_eq!(
        imports.len(),
        1,
        "`{source}`: expected only the well-formed import, got declarations: {:?}",
        parsed.declarations
    );
    assert_eq!(imports[0].path, "c.d", "`{source}`");
    assert_eq!(imports[0].kind, ImportKind::Module, "`{source}`");
}
