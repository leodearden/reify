//! Tests for import declaration parsing with dot-path syntax.

use reify_syntax::{ImportKind, ImportDecl};

// ── Step 1: Basic dot-path module import ──────────────────────────

#[test]
fn parse_basic_module_import() {
    let source = "import std.math";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let imports: Vec<&ImportDecl> = parsed
        .declarations
        .iter()
        .filter_map(|d| {
            if let reify_syntax::Declaration::Import(i) = d {
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
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let import = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_syntax::Declaration::Import(i) = d {
                Some(i)
            } else {
                None
            }
        })
        .expect("should have an import");

    assert_eq!(import.path, "std.mechanical.fasteners");
    assert_eq!(import.kind, ImportKind::Module);
}
