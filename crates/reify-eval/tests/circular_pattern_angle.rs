//! Integration tests for circular_pattern angle unit handling.
//!
//! Verifies that:
//! - A bare numeric angle (`360`) is REJECTED (PRD 3 leaf ε reverses task
//!   #1763, which had made this the one builtin reading bare as degrees).
//! - An explicit angle unit (`360deg`) passes through without any warning.

use reify_core::Severity;
use reify_eval::{BuildResult, Engine};
use reify_ir::ExportFormat;
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, compile_source};

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
/// compile_geometry_op is exercised.  Returns BOTH layers' output from ONE
/// compile: the compiled module, whose diagnostics are the COMPILE-layer
/// verdict, and the full BuildResult, whose diagnostics are the EVAL-layer one.
///
/// NOT `parse_and_compile` — that helper `assert!`s the compiled module has no
/// Error-severity diagnostics, and the bare-angle fixture below is a deliberate
/// COMPILE-layer rejection since PRD 3 leaf ζ gave `circular_pattern` an ANGLE
/// slot at both its forms. Under the panicking helper the bare case would die
/// inside the helper before reaching a single assertion of its own, and the
/// failure would read as "the fixture is wrong" rather than "the gate fired".
/// Restoring `parse_and_compile` here re-breaks it; the dimensioned case keeps
/// the guarantee it used to get from the helper by asserting the compile
/// module is error-free itself.
fn build_plate(source: &str) -> (reify_compiler::CompiledModule, BuildResult) {
    let compiled = compile_source(source);
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);
    (compiled, result)
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
    let (compiled, result) = build_plate(&source);

    // ── COMPILE layer (PRD 3 leaf ζ) ────────────────────────────────────────
    //
    // Observed independently of the eval assertions below: PRD decision D3
    // says the compile slots COMPLEMENT the eval gate, never replace it, so
    // each layer's verdict is asserted on its own module.
    let compile_rejections: Vec<&str> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(reify_core::DiagnosticCode::ArgTypeMismatch)
                && d.severity == Severity::Error
        })
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        compile_rejections,
        vec![format!(
            "circular_pattern: angle argument expects Angle, got Int; {}",
            reify_core::units::ANGLE_MIGRATION_HINT
        )],
        "the compile layer must reject exactly the bare angle, with the one \
         shared hint; got: {:?}",
        compiled.diagnostics
    );

    // ── EVAL layer (PRD 3 leaf ε) ───────────────────────────────────────────
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
    let (compiled, result) = build_plate(&source);

    // Guard: ensure COMPILE produced no hard errors. `parse_and_compile` used
    // to assert this on our behalf; `compile_source` does not, so the
    // dimensioned case states it itself rather than losing it.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "an explicit `360deg` must satisfy the compile-layer ANGLE slot: {:?}",
        compile_errors
    );

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
