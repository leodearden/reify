//! Tests for import declaration parsing with dot-path syntax.

use reify_ast::{ImportDecl, ImportKind};

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
/// The two tests above cannot pin that separator: an ERROR nested inside
/// `import_declaration` never becomes a diagnostic (see the note in
/// `lower_import`), so they would stay green if the grammar merely
/// error-recovered the `.` — the CST-level pins live in
/// tree-sitter-reify/tests/import_items_grammar_tests.rs. The spaced form's
/// stray `{...}`, by contrast, is a sibling ERROR at `source_file` level, which
/// the source_file dispatch loop does surface, so this rejection is observable
/// here.
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
