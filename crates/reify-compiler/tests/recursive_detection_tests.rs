//! Recursive structure detection tests (Task 203).
//!
//! Tests the `detect_recursive_structures()` post-pass behavior:
//! 1. Self-referencing structures are tagged `is_recursive = true`.
//! 2. Non-recursive structures remain `is_recursive = false`.
//! 3. Indirect cycles (2-node, 3-node) tag all participants.
//! 4. Non-cyclic graph patterns (linear chains, diamonds) are correctly excluded.
//! 5. Multiple independent cycles are each detected and diagnosed separately.
//! 6. Warning diagnostics include correct cycle path strings.
//! 7. Collection sub form (`sub items : List<S>`) creates reference edges.
//! 8. References to unknown structures do not crash.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning all templates and diagnostics.
fn compile_all(source: &str) -> (Vec<TopologyTemplate>, Vec<Diagnostic>) {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    (compiled.templates, compiled.diagnostics)
}

/// Helper: find a template by name in a list of templates.
fn find_template<'a>(templates: &'a [TopologyTemplate], name: &str) -> &'a TopologyTemplate {
    templates.iter().find(|t| t.name == name)
        .unwrap_or_else(|| panic!("expected template named '{}'", name))
}

// ─── Step 1: direct self-reference and non-recursive baseline ─────────────────

/// A structure that references itself via a sub should have `is_recursive == true`.
#[test]
fn direct_self_reference_sets_is_recursive() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where n > 0
}
"#;

    let (templates, _diagnostics) = compile_all(source);
    let s = find_template(&templates, "S");

    assert!(
        s.is_recursive,
        "structure S with self-referencing sub should have is_recursive == true"
    );
}

/// A structure with only params and lets (no subs) should NOT be recursive.
#[test]
fn no_subs_not_recursive() {
    let source = r#"
structure S {
    param n : Int = 5
    let doubled : Int = n * 2
}
"#;

    let (templates, _diagnostics) = compile_all(source);
    let s = find_template(&templates, "S");

    assert!(
        !s.is_recursive,
        "structure S with no subs should have is_recursive == false"
    );
}
