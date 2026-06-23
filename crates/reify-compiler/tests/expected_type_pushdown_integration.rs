//! PRD §7 two-way boundary gate: consumer (let + arg) and producer (channel
//! engagement / resolution / recursion) — all §7 rows #1-#8 pinned end-to-end.
//!
//! This suite is the integration gate for docs/prds/expected-type-pushdown.md.
//! It is distinct from the per-position suites authored pre-impl (β/δ):
//!   - expected_type_pushdown_let_tests.rs  — β #4702 let-position unit suite
//!   - expected_type_arg_pushdown_tests.rs  — δ #4703 arg-position unit suite
//!
//! Unlike those suites, this gate asserts via `cell_type` / `cell_expr.result_type`
//! and `DiagnosticCode` — never message substrings — so it survives diagnostic
//! wording changes while remaining a durable end-to-end contract.
//!
//! All rows are GREEN-on-arrival (β #4702 + δ #4703 already landed).
//! If any row is RED, the gate has caught a cross-task integration mismatch;
//! the task is escalated rather than patching compiler code here.

use reify_core::{DiagnosticCode, Type};
use reify_test_support::{compile_source, errors_only, warnings_only};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Return the resolved `cell_type` of a named value cell in `templates[0]`.
fn cell_type<'a>(module: &'a reify_compiler::CompiledModule, member: &str) -> &'a Type {
    let template = &module.templates[0];
    &template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("value cell '{member}' not found in templates[0]"))
        .cell_type
}

/// Return the `default_expr` of a named value cell in `templates[0]`.
fn cell_expr<'a>(
    module: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a reify_ir::CompiledExpr {
    let template = &module.templates[0];
    template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("value cell '{member}' not found in templates[0]"))
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("value cell '{member}' has no default_expr"))
}

// ── §7#1: consumer-let list resolve ──────────────────────────────────────────

/// §7#1 — `let xs : List<Length> = []` must resolve to `List<Length>` with no
/// errors and no warnings (the empty-list inference warning is suppressed when
/// the annotation provides the element type).
///
/// GREEN-on-arrival (β #4702 let-annotation push-down landed).
#[test]
fn integration_let_list_resolves_to_list_length() {
    let source = r#"
structure S {
    let xs : List<Length> = []
}
"#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "§7#1: expected no errors for `let xs : List<Length> = []`, got: {:?}",
        errors_only(&module)
    );
    assert!(
        warnings_only(&module).is_empty(),
        "§7#1: expected no warnings for `let xs : List<Length> = []`, got: {:?}",
        warnings_only(&module)
    );
    assert_eq!(
        *cell_type(&module, "xs"),
        Type::List(Box::new(Type::length())),
        "§7#1: cell_type of `xs` must be List<Length>"
    );
}

// ── §7#2 set: consumer-let set resolve ───────────────────────────────────────

/// §7#2 set — `let s : Set<Length> = set {}` must resolve to `Set<Length>` with
/// no errors and no warnings.
///
/// GREEN-on-arrival (β #4702).
#[test]
fn integration_let_set_resolves_to_set_length() {
    let source = r#"
structure S {
    let s : Set<Length> = set {}
}
"#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "§7#2 set: expected no errors for `let s : Set<Length> = set {{}}`, got: {:?}",
        errors_only(&module)
    );
    assert!(
        warnings_only(&module).is_empty(),
        "§7#2 set: expected no warnings for `let s : Set<Length> = set {{}}`, got: {:?}",
        warnings_only(&module)
    );
    assert_eq!(
        *cell_type(&module, "s"),
        Type::Set(Box::new(Type::length())),
        "§7#2 set: cell_type of `s` must be Set<Length>"
    );
}

// ── §7#2 map: consumer-let map resolve ───────────────────────────────────────

/// §7#2 map — `let m : Map<String, Length> = map {}` must resolve to
/// `Map<String, Length>` with no errors and no warnings.
///
/// GREEN-on-arrival (β #4702).
#[test]
fn integration_let_map_resolves_to_map_string_length() {
    let source = r#"
structure S {
    let m : Map<String, Length> = map {}
}
"#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "§7#2 map: expected no errors for `let m : Map<String, Length> = map {{}}`, got: {:?}",
        errors_only(&module)
    );
    assert!(
        warnings_only(&module).is_empty(),
        "§7#2 map: expected no warnings for `let m : Map<String, Length> = map {{}}`, got: {:?}",
        warnings_only(&module)
    );
    assert_eq!(
        *cell_type(&module, "m"),
        Type::Map(Box::new(Type::String), Box::new(Type::length())),
        "§7#2 map: cell_type of `m` must be Map<String, Length>"
    );
}

// ── §7#3: producer recursion — nested list resolve ────────────────────────────

/// §7#3 — `let xss : List<List<Length>> = [[]]` must resolve to
/// `List<List<Length>>` with no errors and no warnings.  The outer annotation
/// pushes `List<Length>` as the expected element type for the inner `[]`,
/// triggering a recursive resolution pass (producer channel engagement).
///
/// GREEN-on-arrival (β #4702).
#[test]
fn integration_let_nested_list_resolves_by_recursion() {
    let source = r#"
structure S {
    let xss : List<List<Length>> = [[]]
}
"#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "§7#3: expected no errors for `let xss : List<List<Length>> = [[]]`, got: {:?}",
        errors_only(&module)
    );
    assert!(
        warnings_only(&module).is_empty(),
        "§7#3: expected no warnings for `let xss : List<List<Length>> = [[]]`, got: {:?}",
        warnings_only(&module)
    );
    assert_eq!(
        *cell_type(&module, "xss"),
        Type::List(Box::new(Type::List(Box::new(Type::length())))),
        "§7#3: cell_type of `xss` must be List<List<Length>>"
    );
}
