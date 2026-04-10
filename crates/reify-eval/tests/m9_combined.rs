//! M9 combined integration tests.
//!
//! Exercises all M9 milestone features in a single cohesive example:
//! trait conformance with defaults, constraint definitions, determinacy predicates,
//! recursive structures, custom unit declarations, meta block access, and doc comments.
//! Uses examples/m9_combined.ri as the source file.

use reify_constraints::SimpleConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m9_combined.ri"
);

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Parse source, assert no parse errors, compile, assert no compile errors.
/// Returns the compiled module.
fn parse_and_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    compiled
}

// ── Test 1: file parses without errors ──────────────────────────────────────

/// Read m9_combined.ri and verify it parses without errors.
#[test]
fn m9_combined_ri_parses() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}
