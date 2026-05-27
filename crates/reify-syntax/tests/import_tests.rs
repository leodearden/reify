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
