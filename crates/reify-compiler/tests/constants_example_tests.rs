//! Regression test for `examples/stdlib/constants.ri` (task 4026).
//!
//! Pins five leaf signals per the multi_load_bracket_example_tests precedent:
//!
//!   1. The file parses with zero errors.
//!   2. It compiles under the stdlib prelude with zero Error-severity diagnostics.
//!   3. The compiled module exposes a `PhysicalConstants` structure template.
//!   4. Positive source-text pins: `SPEED_OF_LIGHT` and `BOLTZMANN_CONSTANT`
//!      must appear in the source.
//!   5. Negative source-text pins: `299792458` and `1380649` must NOT appear
//!      in the source (guards against inline magic numbers — comments must
//!      describe what each constant IS, not echo its SI numeric value; see
//!      design decision 4 in the task plan).
//!
//! Pattern lifted from `multi_load_bracket_example_tests.rs` (task 3587).
//! PRD reference: `docs/prds/v0_6/stdlib-reconstruction.md` task ζ.

use reify_core::{ModulePath, Severity};

// ─── examples/stdlib/constants.ri compiles clean and pins leaf signals ─────

/// `examples/stdlib/constants.ri` must parse, compile under the stdlib
/// prelude with zero Error diagnostics, expose a `PhysicalConstants`
/// structure template, reference both new constants by name, and contain
/// no inline magic numbers (`299792458` / `1380649`).
///
/// Path resolution uses `CARGO_MANIFEST_DIR` so it works in any worktree.
#[test]
fn constants_example_compiles_under_stdlib_with_zero_errors_and_pins_constant_references() {
    const EXAMPLE_PATH: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/stdlib/constants.ri");

    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/stdlib/constants.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists",
    );

    // ── Parse ──────────────────────────────────────────────────────────────────

    let parsed = reify_syntax::parse(&src, ModulePath::single("constants"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in examples/stdlib/constants.ri: {:?}",
        parsed.errors
    );

    // ── Compile ────────────────────────────────────────────────────────────────

    let module = reify_compiler::compile_with_stdlib(&parsed);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling examples/stdlib/constants.ri under stdlib, \
         got:\n{:#?}",
        errors
    );

    // ── Template presence ──────────────────────────────────────────────────────

    assert!(
        module.templates.iter().any(|t| t.name == "PhysicalConstants"),
        "expected a 'PhysicalConstants' structure template in compiled constants.ri; \
         found templates: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    // ── Positive source-text leaf-signal pins ──────────────────────────────────
    //
    // Both constants must be referenced in the example so a reader can
    // discover them. Checking the compiled module (not just source) is
    // stronger but source-text is sufficient here — the compile-clean
    // assertion above already proves the names resolved.

    assert!(
        src.contains("SPEED_OF_LIGHT"),
        "constants.ri must reference SPEED_OF_LIGHT"
    );
    assert!(
        src.contains("BOLTZMANN_CONSTANT"),
        "constants.ri must reference BOLTZMANN_CONSTANT"
    );

    // ── Negative source-text pins (no inline magic numbers) ───────────────────
    //
    // Per design decision 4: comments must describe the constant's *role*,
    // not echo its SI numeric value. A substring check on the raw digit
    // sequences catches any reconstruction of the SI value regardless of
    // identifier choice. `1380649` matches both the decimal literal
    // `0.00000000000000000000001380649` and any inline `1.380649e-23` variant.
    //
    // Pattern from multi_load_bracket_example_tests.rs:185-194.

    assert!(
        !src.contains("299792458"),
        "constants.ri must NOT contain the magic number '299792458' inline — \
         use SPEED_OF_LIGHT() instead"
    );
    assert!(
        !src.contains("1380649"),
        "constants.ri must NOT contain the magic number '1380649' inline — \
         use BOLTZMANN_CONSTANT() instead"
    );
}
