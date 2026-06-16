//! Reserved-name lint tests (task 4591 — W_RESERVED_TYPE_NAME).
//!
//! The lint walks the top-level declarations once and emits a Warning
//! diagnostic with [`DiagnosticCode::ReservedTypeName`] whenever a user
//! `enum`, `structure`, `occurrence`, or `trait` declaration uses a name
//! that is resolvable by the builtin type resolver (`resolve_type_name`).
//! The builtin still wins in type-annotation position; the warning exists
//! to alert the author.

use reify_test_support::{compile_source, warnings_only};
use reify_core::{DiagnosticCode, Severity};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Collect only `ReservedTypeName` warnings from a compiled module.
fn reserved_name_warnings<'a>(
    warnings: &'a [&'a reify_core::Diagnostic],
) -> Vec<&'a reify_core::Diagnostic> {
    warnings
        .iter()
        .copied()
        .filter(|d| d.code == Some(DiagnosticCode::ReservedTypeName))
        .collect()
}

/// Step-3 (RED): compiling `enum Direction { In, Out }` must emit exactly
/// one `ReservedTypeName` warning because `Direction` is a builtin datum type.
///
/// RED: the lint is not wired yet — 0 warnings instead of 1. Turns GREEN
/// after step-4 creates `reserved_name_lint.rs` and wires it in `lib.rs`.
#[test]
fn enum_named_after_builtin_emits_reserved_type_name_warning() {
    let source = r#"
enum Direction { In, Out }
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let reserved: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ReservedTypeName))
        .collect();

    assert_eq!(
        reserved.len(),
        1,
        "expected exactly 1 ReservedTypeName warning for `enum Direction`, got {}: {:?}",
        reserved.len(),
        reserved
            .iter()
            .map(|d| (&d.message, &d.labels))
            .collect::<Vec<_>>()
    );

    let warning = reserved[0];
    assert_eq!(
        warning.severity,
        Severity::Warning,
        "ReservedTypeName diagnostic must be Severity::Warning, got {:?}",
        warning.severity
    );

    assert!(
        !warning.labels.is_empty(),
        "ReservedTypeName warning must carry at least one label, got none"
    );
    let l0 = &warning.labels[0];
    assert!(
        !l0.span.is_empty(),
        "first label span must be non-empty, got: {:?}",
        l0.span
    );
}

// ── Step-5: extended policy coverage ─────────────────────────────────────────

/// (a.1) `structure Frame {}` — `Frame` is a datum-receiver builtin.
/// Must emit exactly one ReservedTypeName warning.
#[test]
fn structure_named_frame_emits_reserved_type_name_warning() {
    let source = "structure Frame {}";
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let reserved = reserved_name_warnings(&warnings);
    assert_eq!(
        reserved.len(),
        1,
        "expected 1 ReservedTypeName warning for `structure Frame`, got {}: {:?}",
        reserved.len(),
        reserved.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert_eq!(reserved[0].severity, Severity::Warning);
    assert!(!reserved[0].labels.is_empty());
    assert!(!reserved[0].labels[0].span.is_empty());
}

/// (a.2) `trait Axis {}` — `Axis` is a datum-receiver builtin.
/// Must emit exactly one ReservedTypeName warning.
#[test]
fn trait_named_axis_emits_reserved_type_name_warning() {
    let source = "trait Axis {}";
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let reserved = reserved_name_warnings(&warnings);
    assert_eq!(
        reserved.len(),
        1,
        "expected 1 ReservedTypeName warning for `trait Axis`, got {}: {:?}",
        reserved.len(),
        reserved.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert_eq!(reserved[0].severity, Severity::Warning);
    assert!(!reserved[0].labels.is_empty());
    assert!(!reserved[0].labels[0].span.is_empty());
}

/// (a.3) An occurrence named after a builtin (`Solid` = geometry-handle alias).
/// Must emit exactly one ReservedTypeName warning.
#[test]
fn occurrence_named_solid_emits_reserved_type_name_warning() {
    let source = "occurrence def Solid {}";
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let reserved = reserved_name_warnings(&warnings);
    assert_eq!(
        reserved.len(),
        1,
        "expected 1 ReservedTypeName warning for `occurrence def Solid`, got {}: {:?}",
        reserved.len(),
        reserved.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert_eq!(reserved[0].severity, Severity::Warning);
    assert!(!reserved[0].labels.is_empty());
    assert!(!reserved[0].labels[0].span.is_empty());
}

/// (a.4) Named physical dimension: `enum Force { A, B }` — `Force` is in NAMED_DIMENSIONS.
/// Must emit exactly one ReservedTypeName warning.
#[test]
fn enum_named_force_dimension_emits_reserved_type_name_warning() {
    let source = "enum Force { A, B }";
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let reserved = reserved_name_warnings(&warnings);
    assert_eq!(
        reserved.len(),
        1,
        "expected 1 ReservedTypeName warning for `enum Force`, got {}: {:?}",
        reserved.len(),
        reserved.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert_eq!(reserved[0].severity, Severity::Warning);
}

/// (b.1) NEGATIVE: `enum Polarity { In, Out }` — `Polarity` is not a builtin.
/// Must emit zero ReservedTypeName warnings.
#[test]
fn enum_polarity_emits_no_reserved_type_name_warning() {
    let source = "enum Polarity { In, Out }";
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let reserved = reserved_name_warnings(&warnings);
    assert_eq!(
        reserved.len(),
        0,
        "expected 0 ReservedTypeName warnings for `enum Polarity`, got {}: {:?}",
        reserved.len(),
        reserved.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// (b.2) NEGATIVE: `structure Bracket {}` — `Bracket` is not a builtin.
/// Must emit zero ReservedTypeName warnings.
#[test]
fn structure_bracket_emits_no_reserved_type_name_warning() {
    let source = "structure Bracket {}";
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let reserved = reserved_name_warnings(&warnings);
    assert_eq!(
        reserved.len(),
        0,
        "expected 0 ReservedTypeName warnings for `structure Bracket`, got {}: {:?}",
        reserved.len(),
        reserved.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// (c) PRECEDENCE / warning-only guarantee: a module declaring `enum Direction`
/// plus a structure that uses `Direction` as a type annotation produces no
/// Error-severity diagnostic attributable to the collision. The collision yields
/// only the Warning; the builtin still resolves.
///
/// This confirms the lint is warning-only and type resolution is unchanged.
#[test]
fn builtin_still_resolves_when_user_enum_collides() {
    let source = r#"
enum Direction { In, Out }
structure Beam {
    param d : Direction = Direction.In
}
"#;
    let module = compile_source(source);

    // There must be no Error-severity diagnostics.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error-severity diagnostics when user enum collides with builtin, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // The ReservedTypeName Warning must be present (from the `enum Direction` decl).
    let warnings = warnings_only(&module);
    let reserved = reserved_name_warnings(&warnings);
    assert_eq!(
        reserved.len(),
        1,
        "expected exactly 1 ReservedTypeName warning, got {}: {:?}",
        reserved.len(),
        reserved.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert_eq!(reserved[0].severity, Severity::Warning);
}
