//! Diagnostic for generic-enum payload field type naming an undeclared/out-of-scope
//! identifier (task 4992).
//!
//! Verifies that:
//!   (A) A generic enum's variant payload field type naming an identifier that
//!       resolves to nothing valid (not a builtin/alias/structure/trait/in-scope
//!       enum, nor one of the enum's own declared type parameters) emits
//!       `DiagnosticCode::EnumUnknownTypeParam` — mirrors `FnUnknownTypeParam` for
//!       generic function signatures.
//!   (B) A non-generic enum's unknown payload field type stays silent (gating pin
//!       — the "declared type parameter" concept only exists for generic enums).
//!   (C) A generic enum's own declared type parameter, and an in-scope sibling
//!       enum, both resolve cleanly — no false positive.
//!   (D) A bare `Scalar` payload field (anti-cascade case, already returns
//!       `Some(Type::Error)` with its own `BareScalarType` diagnostic) does NOT
//!       also trigger `EnumUnknownTypeParam` — single root-cause, no double-report.
//!
//! All tests use `compile_source` (no stdlib) — none of the sources below need
//! prelude symbols.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::compile_source;

// ────────────────────────────────────────────────────────────────────────────
// (A) PRIMARY RED — undeclared/out-of-scope identifier in a generic enum payload
// ────────────────────────────────────────────────────────────────────────────

/// A generic enum's variant payload field naming an unresolvable identifier
/// emits exactly one `DiagnosticCode::EnumUnknownTypeParam` Error, naming the
/// enum, variant, and offending type.
///
/// RED until step-2: today `Nonexistent` silently resolves to `Type::Error` via
/// the `.unwrap_or(Type::Error)` fallback in `enums_phase.rs`, with NO diagnostic.
#[test]
fn generic_enum_undeclared_payload_type_emits_enum_unknown_type_param() {
    let source = r#"
        enum Box<T> {
            Wrap { value: Nonexistent },
        }
    "#;
    let module = compile_source(source);

    let matches: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::EnumUnknownTypeParam))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one EnumUnknownTypeParam diagnostic for undeclared \
         payload type 'Nonexistent', got: {:?}",
        module.diagnostics
    );

    let diag = matches[0];
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        diag.message.contains("Box"),
        "message should name the enum 'Box', got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("Wrap"),
        "message should name the variant 'Wrap', got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("Nonexistent"),
        "message should name the offending type 'Nonexistent', got: {:?}",
        diag.message
    );
}

// ────────────────────────────────────────────────────────────────────────────
// (B) GATING PIN — non-generic enum stays silent
// ────────────────────────────────────────────────────────────────────────────

/// A non-generic enum with an unknown payload field type must NOT emit
/// `EnumUnknownTypeParam` — the "declared type parameter" concept only exists
/// for generic enums (pre-existing silent-`Type::Error` behavior, unchanged).
#[test]
fn nongeneric_enum_unknown_payload_type_stays_silent() {
    let source = r#"
        enum Shape {
            Circle { radius: Nonexistent },
        }
    "#;
    let module = compile_source(source);

    let found = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::EnumUnknownTypeParam));
    assert!(
        found.is_none(),
        "non-generic enum must not emit EnumUnknownTypeParam, got: {:?}",
        found
    );
}

// ────────────────────────────────────────────────────────────────────────────
// (C) NO-FALSE-POSITIVE PIN — declared type param + in-scope sibling enum
// ────────────────────────────────────────────────────────────────────────────

/// A generic enum's payload field referencing its own declared type parameter,
/// and a sibling generic enum's payload field referencing an in-scope sibling
/// enum, both resolve cleanly — no `EnumUnknownTypeParam` and no Error
/// diagnostics at all.
#[test]
fn generic_enum_valid_payload_types_emit_no_diagnostics() {
    let source = r#"
        enum Box<T> {
            Wrap { value: T },
        }
        enum Sib {
            A,
        }
        enum Holder<T> {
            H { s: Sib },
        }
    "#;
    let module = compile_source(source);

    let enum_unknown = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::EnumUnknownTypeParam));
    assert!(
        enum_unknown.is_none(),
        "declared type param and in-scope sibling enum must not emit \
         EnumUnknownTypeParam, got: {:?}",
        enum_unknown
    );

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );
}

// ────────────────────────────────────────────────────────────────────────────
// (D) ANTI-CASCADE PIN — bare `Scalar` payload keeps single root-cause diagnostic
// ────────────────────────────────────────────────────────────────────────────

/// A bare `Scalar` payload field (an anti-cascade case that already returns
/// `Some(Type::Error)` with its own `BareScalarType` diagnostic) must NOT also
/// trigger `EnumUnknownTypeParam` — exactly one root-cause diagnostic, no
/// double-report.
#[test]
fn generic_enum_bare_scalar_payload_emits_only_bare_scalar_type() {
    let source = r#"
        enum Box<T> {
            Wrap { value: Scalar },
        }
    "#;
    let module = compile_source(source);

    let bare_scalar: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::BareScalarType))
        .collect();
    assert_eq!(
        bare_scalar.len(),
        1,
        "expected exactly one BareScalarType diagnostic for bare `Scalar` payload, \
         got: {:?}",
        module.diagnostics
    );

    let enum_unknown = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::EnumUnknownTypeParam));
    assert!(
        enum_unknown.is_none(),
        "bare `Scalar` payload must not also emit EnumUnknownTypeParam \
         (anti-cascade — single root-cause diagnostic), got: {:?}",
        enum_unknown
    );
}
