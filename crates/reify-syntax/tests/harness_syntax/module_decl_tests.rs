//! Tests for module declaration parsing: round-trip and positional rejection.
//!
//! Step-5 (RED): Tests fail after step-4 because `lower_source_file` has no
//! `module_declaration` arm (the node hits `warn_unexpected_child`, which emits
//! an error) and `ParsedModule.declared_module_path` is hardcoded to `None`.
//!
//! Step-6 (GREEN): Lowering wiring is added; both tests pass.

use reify_ast::{Declaration, ModuleDecl};
use reify_core::ModulePath;

// ── Test A: round-trip ────────────────────────────────────────────────────────

/// Parse a top-of-file `module a.b.c` declaration and assert:
///   1. No parse errors (the grammar accepts `module_declaration` at position 0).
///   2. `declared_module_path` equals the structured ModulePath.
///   3. A `Declaration::Module(ModuleDecl)` with `path == "a.b.c"` is present.
#[test]
fn parse_module_declaration_round_trip() {
    let source = "module a.b.c";
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));

    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    assert_eq!(
        parsed.declared_module_path,
        Some(ModulePath::new(vec!["a".into(), "b".into(), "c".into()])),
        "declared_module_path mismatch"
    );

    let module_decls: Vec<&ModuleDecl> = parsed
        .declarations
        .iter()
        .filter_map(|d| if let Declaration::Module(m) = d { Some(m) } else { None })
        .collect();

    assert_eq!(module_decls.len(), 1, "expected one Declaration::Module");
    assert_eq!(module_decls[0].path, "a.b.c");
}

/// Single-segment `module foo` — verify it also parses and produces the right path.
#[test]
fn parse_module_declaration_single_segment() {
    let source = "module foo";
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));

    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    assert_eq!(
        parsed.declared_module_path,
        Some(ModulePath::new(vec!["foo".into()])),
    );

    let found = parsed
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Module(m) = d { Some(m) } else { None })
        .expect("should have a Declaration::Module");

    assert_eq!(found.path, "foo");
}

// ── Test B: positional rejection ──────────────────────────────────────────────

/// A `module` declaration appearing AFTER another declaration (not at the top)
/// must produce a parse error and leave `declared_module_path` as `None`.
///
/// Grammar: `source_file: seq(optional($.module_declaration), repeat($._declaration))`
/// The `module_declaration` rule is absent from `_declaration`, so `module a.b`
/// after `structure S {}` becomes an ERROR node → `lower_source_file` emits an error.
#[test]
fn module_declaration_after_other_decl_is_an_error() {
    let source = "structure S {}\nmodule a.b";
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));

    assert!(
        !parsed.errors.is_empty(),
        "expected parse error for out-of-position module decl, but got none"
    );
    assert!(
        parsed.declared_module_path.is_none(),
        "declared_module_path should be None when module decl is out-of-position"
    );
}
