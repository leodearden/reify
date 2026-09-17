//! Integration tests for circular_pattern angle unit handling.
//!
//! Verifies that:
//! - A bare numeric angle (`360`) is REJECTED (PRD 3 leaf ε reverses task
//!   #1763, which had made this the one builtin reading bare as degrees).
//! - An explicit angle unit (`360deg`) passes through without any warning.

use reify_core::Severity;
use reify_eval::{BuildResult, Engine};
use reify_ir::ExportFormat;
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, parse_and_compile};

/// Source shared by both tests: a plate structure with a cylindrical hole
/// patterned around the Z-axis.  The angle argument differs between tests.
///
/// The axis ORIGIN is dimensioned (`0mm`) — it is length-semantic and gated as
/// a Length since task 5350. The axis DIRECTION `0, 0, 1` stays bare (a
/// dimensionless unit vector). The `angle_expr` is the variable under test:
/// one case passes it bare (rejected since ε) and one dimensioned (accepted).
fn plate_source(angle_expr: &str) -> String {
    format!(
        r#"
        structure def Plate {{
            let hole = cylinder(5mm, 10mm)
            let holes = circular_pattern(hole, 0mm, 0mm, 0mm, 0, 0, 1, 6, {angle_expr})
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

/// `circular_pattern` with a bare numeric angle (`360`) is REJECTED.
///
/// INVERTED by PRD 3 leaf ε (task 5781), reversing task #1763. This asserted
/// the opposite — that a bare angle warns and is coerced from degrees — and
/// that mechanism is exactly what ε deletes. Flipping it IS the fix.
#[test]
fn circular_pattern_bare_360_is_rejected() {
    let source = plate_source("360");
    let result = build_plate(&source);

    let rejection = result
        .diagnostics
        .iter()
        .find(|d| d.message.contains("angle argument expects Angle, got "))
        .unwrap_or_else(|| {
            panic!(
                "a bare circular_pattern angle must be rejected by name; got: {:?}",
                result.diagnostics
            )
        });
    assert_eq!(
        rejection.severity,
        Severity::Error,
        "this was a Warning with exit 0 before ε; got: {rejection:?}"
    );
    assert!(
        rejection
            .message
            .contains(reify_core::units::ANGLE_MIGRATION_HINT),
        "the rejection must tell the author how to fix it; got: {:?}",
        rejection.message
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("bare numeric angle")),
        "the deprecation warning must be GONE, not merely joined by a \
         rejection; got: {:?}",
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
