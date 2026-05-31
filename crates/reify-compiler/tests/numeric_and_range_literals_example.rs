//! Focused compile-smoke test for `examples/numeric_and_range_literals.ri`.
//!
//! This is the TDD gate for task η/3915 — integration example exercising all
//! three v0.6 literal slices together (digit separators, hex/binary, single-sided
//! ranges) per docs/prds/v0_6/numeric-and-range-literal-forms.md.
//!
//! Signal: `examples/numeric_and_range_literals.ri` must parse and compile with
//! the stdlib prelude producing zero Error-severity diagnostics.  This mirrors
//! the `smoke_one` body in `examples_smoke.rs` (which also auto-discovers and
//! exercises the file via its recursive walk of `examples/`).
//!
//! Pattern: matches `constants_example_tests.rs` (task 4026) and the
//! `smoke_one` helper in `examples_smoke.rs`.

use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
use reify_core::{ModulePath, Severity};

/// Path to the integration example, resolved at compile time via
/// `CARGO_MANIFEST_DIR` so it works in any worktree.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/numeric_and_range_literals.ri"
);

/// `examples/numeric_and_range_literals.ri` must:
///   (a) parse with zero errors, and
///   (b) compile under the stdlib prelude with zero Error-severity diagnostics.
///
/// The bulk `all_examples_parse_and_compile_with_stdlib` test in
/// `examples_smoke.rs` also auto-discovers and exercises this file via its
/// recursive walk of `examples/`.  The overlap is intentional and permanent:
/// this focused gate provides a targeted, descriptive failure message scoped
/// to this one file, while the bulk harness provides breadth coverage.
#[test]
fn numeric_and_range_literals_example_parses_and_compiles_with_zero_errors() {
    let src = std::fs::read_to_string(EXAMPLE_PATH).unwrap_or_else(|e| {
        panic!(
            "failed to read examples/numeric_and_range_literals.ri — \
             check that the file exists: {}",
            e
        )
    });

    // ── Parse ──────────────────────────────────────────────────────────────
    // Use parse_with_stdlib so the module is seen the same way
    // compile_with_stdlib sees it.
    let parsed = parse_with_stdlib(&src, ModulePath::single("numeric_and_range_literals"));

    assert!(
        parsed.errors.is_empty(),
        "parse errors in examples/numeric_and_range_literals.ri:\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );

    // ── Compile ────────────────────────────────────────────────────────────
    let compiled = compile_with_stdlib(&parsed);

    let errors: Vec<&str> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();

    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling examples/numeric_and_range_literals.ri \
         under stdlib, got:\n{}",
        errors.join("\n")
    );
}
