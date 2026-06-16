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
