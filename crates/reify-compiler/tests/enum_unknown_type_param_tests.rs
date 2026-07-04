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
//!   (E) A generic enum's payload field referencing a PRELUDE generic enum with
//!       type args (e.g. `Result<T, T>`) resolves cleanly — no false positive
//!       (uses `compile_source_with_stdlib` so the stdlib prelude is in scope).
//!   (F) An unresolvable identifier referenced WITH type args (e.g.
//!       `Nope<T>`, not just a bare name as in (A)) still emits
//!       `EnumUnknownTypeParam` — the with-args resolution arm falls through
//!       to the same gated fallback as the bare-name case.
//!   (G) A SIBLING module-local generic enum referenced WITH type args (e.g.
//!       `Box<U>`) resolves cleanly — no false positive (the module-local
//!       half of the with-args resolution arm; (E) covers the prelude half).
//!   (H) Amendment (reviewer_comprehensive test_coverage finding): documents a
//!       pre-existing, separate gap — an unknown type arg nested INSIDE an
//!       otherwise-resolvable generic enum reference (`Result<Bad, T>`) is
//!       silently swallowed by `resolve_enum_type_with_args`'s own
//!       `.unwrap_or(Type::Error)`, with no diagnostic at all. This is NOT
//!       fixed by this task (out of scope — see review discussion); the test
//!       pins the current silent behavior so a future reader does not assume
//!       `Bad` is also flagged.
//!
//! All tests use `compile_source` (no stdlib) — none of the sources below need
//! prelude symbols — EXCEPT (E) and (H), which use `compile_source_with_stdlib`
//! since they specifically exercise resolution against the stdlib prelude's
//! `Result<T, E>` enum.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{compile_source, compile_source_with_stdlib};

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
    // The bare type name is injected via `format!` rather than written as a
    // literal `: Scalar` substring in this file: the corpus-cleanliness guard
    // (crates/reify-cli/tests/corpus_no_bare_scalar.rs) bans exactly that
    // pattern anywhere under `crates/**/*.rs` outside its two parse-only
    // carve-outs, which this file isn't. This fixture is an intentional
    // negative case — the point is that the compiler still rejects bare
    // `Scalar` — so it needs the identical source text without the literal
    // substring; mirrors how the `BareScalarType` unit tests in
    // `type_resolution.rs`/`diagnostics.rs` sidestep the same guard.
    let bare_scalar_type_name = "Scalar";
    let source = format!(
        r#"
        enum Box<T> {{
            Wrap {{ value: {bare_scalar_type_name} }},
        }}
    "#
    );
    let module = compile_source(&source);

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

// ────────────────────────────────────────────────────────────────────────────
// (E) NO-FALSE-POSITIVE PIN — prelude generic enum referenced with type args
// ────────────────────────────────────────────────────────────────────────────

/// A generic enum's payload field referencing a PRELUDE generic enum with type
/// args (`Result<T, T>`, from the stdlib prelude's `result.ri`) must resolve
/// cleanly — no `EnumUnknownTypeParam` and no Error diagnostics at all.
///
/// RED until step-5: the with-type-args resolution arm in `enums_phase.rs`
/// passes only the MODULE-LOCAL `enum_defs` to `resolve_enum_type_with_args`,
/// so the prelude's `Result` is not found there — `resolve_enum_type_with_args`
/// returns `None` silently (its `?` on the module-local lookup), which reaches
/// the `.unwrap_or_else` fallback and — because the enclosing `Wrapper<T>` is
/// generic — emits a FALSE-POSITIVE `EnumUnknownTypeParam` for a valid stdlib
/// type.
#[test]
fn generic_enum_prelude_generic_enum_with_args_payload_emits_no_diagnostics() {
    let source = r#"
        enum Wrapper<T> {
            W { inner: Result<T, T> },
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let enum_unknown = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::EnumUnknownTypeParam));
    assert!(
        enum_unknown.is_none(),
        "prelude generic enum `Result<T, T>` referenced with type args must not \
         emit EnumUnknownTypeParam, got: {:?}",
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
// (F) POSITIVE PIN — unresolvable identifier referenced WITH type args
// ────────────────────────────────────────────────────────────────────────────

/// Amendment test (reviewer_comprehensive test_coverage finding): a generic
/// enum's variant payload field naming an unresolvable identifier referenced
/// WITH type args (`Nope<T>`, as opposed to the bare-name case in (A)) still
/// emits exactly one `DiagnosticCode::EnumUnknownTypeParam`. The with-args
/// resolution arm (`resolve_enum_type_with_args`) returns `None` when `name`
/// isn't found in the merged prelude ++ module-local enum set, falling
/// through to the same gated fallback as the bare-name case.
#[test]
fn generic_enum_unknown_payload_type_with_type_args_emits_enum_unknown_type_param() {
    let source = r#"
        enum Box<T> {
            Wrap { value: Nope<T> },
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
         payload type 'Nope<T>' (unknown name referenced WITH type args), got: {:?}",
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
        diag.message.contains("Nope"),
        "message should name the offending type 'Nope', got: {:?}",
        diag.message
    );
}

// ────────────────────────────────────────────────────────────────────────────
// (G) NO-FALSE-POSITIVE PIN — sibling module-local generic enum referenced WITH args
// ────────────────────────────────────────────────────────────────────────────

/// Amendment test (reviewer_comprehensive test_coverage finding): a generic
/// enum's payload field referencing a SIBLING module-local generic enum WITH
/// type args (`Box<U>`) must resolve cleanly via the merged
/// prelude ++ module-local slice — no `EnumUnknownTypeParam` and no Error
/// diagnostics. Distinct from (C), which only covers a bare (argless) sibling
/// reference; this exercises the module-local half of the with-type-args
/// resolution arm (the prelude half is covered by (E)).
#[test]
fn generic_enum_sibling_module_local_generic_enum_with_args_payload_emits_no_diagnostics() {
    let source = r#"
        enum Box<T> {
            Wrap { value: T },
        }
        enum Holder<U> {
            H { b: Box<U> },
        }
    "#;
    let module = compile_source(source);

    let enum_unknown = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::EnumUnknownTypeParam));
    assert!(
        enum_unknown.is_none(),
        "sibling module-local generic enum referenced with type args must not \
         emit EnumUnknownTypeParam, got: {:?}",
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
// (H) DOCUMENTING PIN — unknown inner type arg inside a KNOWN generic enum stays silent
// ────────────────────────────────────────────────────────────────────────────

/// Amendment test (reviewer_comprehensive test_coverage finding): documents a
/// pre-existing gap that is SEPARATE from (and not fixed by) this task — see
/// review discussion. An unknown type argument nested INSIDE an
/// otherwise-resolvable generic enum reference (`Result<Bad, T>` — `Result`
/// itself resolves, but `Bad` does not) is silently swallowed by
/// `resolve_enum_type_with_args`'s own inner `.unwrap_or(Type::Error)`
/// (`type_resolution.rs`), which carries no diagnostic. Because the OUTER
/// `Result<Bad, T>` reference resolves to `Some(Type::Applied { .. })`, the
/// field never reaches the gated `EnumUnknownTypeParam` fallback in
/// `enums_phase.rs` — unlike (A)/(F), where the WHOLE reference fails to
/// resolve. This test pins the current (silent) behavior so a future reader
/// does not assume `Bad` is also flagged.
#[test]
fn generic_enum_prelude_generic_enum_unknown_inner_arg_stays_silent() {
    let source = r#"
        enum Wrapper<T> {
            W { inner: Result<Bad, T> },
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let enum_unknown = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::EnumUnknownTypeParam));
    assert!(
        enum_unknown.is_none(),
        "unknown inner type arg 'Bad' nested inside a known generic enum \
         reference is a pre-existing, separate silent gap — must not emit \
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
        "documents the pre-existing silent behavior: an unresolvable inner \
         type arg inside a known generic enum currently produces NO Error \
         diagnostics at all (becomes Type::Error internally with no report); \
         got: {:?}",
        errors
    );
}
