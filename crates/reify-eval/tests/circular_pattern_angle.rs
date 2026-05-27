//! Integration tests for circular_pattern angle unit handling.
//!
//! Verifies that:
//! - A bare numeric angle (`360`) is interpreted as degrees and emits a
//!   deprecation warning in the build diagnostics.
//! - An explicit angle unit (`360deg`) passes through without any warning.

use reify_eval::{BuildResult, Engine};
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, parse_and_compile};
use reify_core::Severity;
use reify_ir::ExportFormat;

/// Source shared by both tests: a plate structure with a cylindrical hole
/// patterned around the Z-axis.  The angle argument differs between tests.
fn plate_source(angle_expr: &str) -> String {
    format!(
        r#"
        structure def Plate {{
            let hole = cylinder(5mm, 10mm)
            let holes = circular_pattern(hole, 0, 0, 0, 0, 0, 1, 6, {angle_expr})
        }}
        "#
    )
}

/// Build a plate from the given source using a MockGeometryKernel so that
/// compile_geometry_op is exercised.  Returns the full BuildResult so callers
/// can verify both that the build succeeded and what diagnostics it produced.
fn build_plate(source: &str) -> BuildResult {
    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    engine.build(&compiled, ExportFormat::Step)
}

// ── step-6 ───────────────────────────────────────────────────────────────────

/// `circular_pattern` with a bare numeric angle (`360`) should emit a
/// deprecation warning informing the user that the value is treated as degrees.
#[test]
fn circular_pattern_bare_360_emits_deprecation_warning() {
    let source = plate_source("360");
    let result = build_plate(&source);

    // Guard: ensure the build did not fail with hard errors before reaching
    // the angle-conversion code (which would make the diagnostic check vacuous).
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "build produced unexpected errors before angle conversion: {:?}",
        errors
    );

    let degree_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && (d.message.contains("deg") || d.message.contains("degrees"))
        })
        .collect();

    assert!(
        !degree_warnings.is_empty(),
        "expected at least one Warning diagnostic about implicit degree conversion, \
         but got: {:?}",
        result.diagnostics
    );
}

// ── step-7 ───────────────────────────────────────────────────────────────────

/// `circular_pattern` with an explicit angle unit (`360deg`) should NOT emit
/// any deprecation warning — the explicit unit path must be warning-free.
#[test]
fn circular_pattern_360deg_no_deprecation_warning() {
    let source = plate_source("360deg");
    let result = build_plate(&source);

    // Guard: ensure the build did not fail with hard errors (which would make
    // the "no warning" assertion vacuously true).
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "build produced unexpected errors: {:?}",
        errors
    );

    let degree_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && (d.message.contains("deg") || d.message.contains("degrees"))
        })
        .collect();

    assert!(
        degree_warnings.is_empty(),
        "expected no deprecation warning when explicit `360deg` is used, \
         but got warnings: {:?}",
        degree_warnings
    );
}
