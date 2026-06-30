//! Compiler-side field-set checks for named-field enum-variant MATCH PATTERNS (task ε #3944).
//!
//! Drives the pattern-matching side of data-carrying enums:
//!   - Unknown-field check: a VariantBind pattern names a field not declared by
//!     the variant → `PatternUnknownField` (step-3 RED / step-4 GREEN).
//!   - Missing-field check: a VariantBind pattern omits a declared field
//!     → `PatternMissingField` (step-5 RED / step-6 GREEN).
//!
//! Diagnostic assertions match on `Diagnostic.code` (typed `DiagnosticCode`)
//! rather than message substrings, per the codebase convention.

mod common;

use common::compile_with_stdlib_helper;
use reify_core::ty::Type;
use reify_core::{DiagnosticCode, Severity};

/// The shared `Shape` enum used by the pattern-check tests: one
/// single-field variant (`Circle`), one two-field variant (`Rect`), and one
/// bare variant (`Point`).
const SHAPE_ENUM: &str = "\
enum Shape {
    Circle { radius: Length },
    Rect { width: Length, height: Length },
    Point,
}
";

/// True if compiling `source` yields at least one Error-severity diagnostic
/// carrying `code`.
fn has_error_code(source: &str, code: DiagnosticCode) -> bool {
    compile_with_stdlib_helper(source)
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.code == Some(code))
}

/// Collect all Error-severity diagnostic codes (for assertion failure messages).
fn error_codes(source: &str) -> Vec<Option<DiagnosticCode>> {
    compile_with_stdlib_helper(source)
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code)
        .collect()
}

/// Build a Reify source that declares SHAPE_ENUM and a structure whose `let area`
/// is a `match` on a `Shape`-typed let binding, using the supplied arm text.
///
/// `arms` is pasted verbatim inside `match shape { <arms> }`.
fn shape_match_source(arms: &str) -> String {
    format!(
        "{SHAPE_ENUM}\nstructure def Widget {{\n    let shape = Shape.Point\n    let area : Length = match shape {{\n{arms}\n    }}\n}}\n"
    )
}

/// Same as `shape_match_source` but without a type annotation on `area`, so
/// the compiled result type can be inspected directly.
fn shape_match_source_untyped(arms: &str) -> String {
    format!(
        "{SHAPE_ENUM}\nstructure def Widget {{\n    let shape = Shape.Point\n    let area = match shape {{\n{arms}\n    }}\n}}\n"
    )
}

/// Return the compiled result type of the `area` let binding in a Widget
/// structure compiled from `source`.
fn area_result_type(source: &str) -> Type {
    let module = compile_with_stdlib_helper(source);
    let widget = module
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template should be present");
    let cell = widget
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "area")
        .expect("Widget should have an 'area' value cell");
    let expr = cell
        .default_expr
        .as_ref()
        .expect("area let should have a compiled default_expr");
    expr.result_type.clone()
}

/// True if compiling `source` yields at least one Error-severity diagnostic
/// whose message contains `substring`.
fn has_error_containing(source: &str, substring: &str) -> bool {
    compile_with_stdlib_helper(source)
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.message.contains(substring))
}

// ── step-3 RED: unknown-field pattern diagnostic ──────────────────────────────

/// A pattern that binds a field `diameter` which `Circle` does not declare
/// (Circle declares only `radius`) must emit `PatternUnknownField`.
///
/// RED today: no pattern field-set check exists; VariantBind binders were dropped.
#[test]
fn pattern_unknown_field_emits_diagnostic() {
    let source = shape_match_source(
        "        Circle { diameter: d } => 0.0mm,\n        _ => 0.0mm,",
    );
    assert!(
        has_error_code(&source, DiagnosticCode::PatternUnknownField),
        "expected PatternUnknownField for 'Circle {{ diameter: d }}' (Circle has no 'diameter'); \
         actual error codes: {:?}",
        error_codes(&source),
    );
}

// ── step-5 RED: missing-field pattern diagnostic ──────────────────────────────

/// A pattern that omits declared field `height` of `Rect` (Rect declares both
/// `width` and `height`) must emit `PatternMissingField`.
///
/// RED today: no field-set completeness check exists.
#[test]
fn pattern_missing_field_emits_diagnostic() {
    let source = shape_match_source(
        "        Rect { width: w } => w,\n        _ => 0.0mm,",
    );
    assert!(
        has_error_code(&source, DiagnosticCode::PatternMissingField),
        "expected PatternMissingField for 'Rect {{ width: w }}' (Rect also declares 'height'); \
         actual error codes: {:?}",
        error_codes(&source),
    );
}

// ── step-7 capstone: preservation pins ───────────────────────────────────────

/// (a) PRD §1 exhaustive fully-bound match: Circle{radius:r}=>r, Rect{width:w,height:h}=>w,
/// Point=>0.0mm produces ZERO Error diagnostics AND the match result type is
/// Length (proves binders carry the declared Length type, not Type::Error).
#[test]
fn valid_exhaustive_variantbind_match_is_clean() {
    let source = shape_match_source_untyped(
        "        Circle { radius: r } => r,\n        Rect { width: w, height: h } => w,\n        Point => 0.0mm,",
    );
    let module = compile_with_stdlib_helper(&source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "valid exhaustive match should produce ZERO Error diagnostics; got: {:?}",
        errors,
    );
    let ty = area_result_type(&source);
    assert_eq!(
        ty,
        Type::length(),
        "area result type must be Length (binders carry declared type, not Error); got: {:?}",
        ty,
    );
}

/// (b) A bare-variant arm against a Named variant is legal — no Pattern* diagnostic.
/// `Rect => 0.0mm` ignores the payload entirely; the field-set check must NOT fire.
#[test]
fn bare_variant_arm_against_named_variant_is_legal() {
    let source = shape_match_source_untyped(
        "        Rect => 0.0mm,\n        Circle { radius: r } => r,\n        Point => 0.0mm,",
    );
    assert!(
        !has_error_code(&source, DiagnosticCode::PatternUnknownField),
        "bare-variant arm must NOT emit PatternUnknownField",
    );
    assert!(
        !has_error_code(&source, DiagnosticCode::PatternMissingField),
        "bare-variant arm must NOT emit PatternMissingField",
    );
}

/// (c) A wildcard arm is legal — no Pattern* diagnostic.
#[test]
fn wildcard_arm_is_legal() {
    let source = shape_match_source_untyped(
        "        Circle { radius: r } => r,\n        _ => 0.0mm,",
    );
    assert!(
        !has_error_code(&source, DiagnosticCode::PatternUnknownField),
        "wildcard arm must NOT emit PatternUnknownField",
    );
    assert!(
        !has_error_code(&source, DiagnosticCode::PatternMissingField),
        "wildcard arm must NOT emit PatternMissingField",
    );
}

// ── message token checks (less brittle than exact-wording assertions) ─────────

/// PatternUnknownField message must mention the variant name ('Circle') and
/// the unknown field name ('diameter') — tests dynamic content without
/// pinning exact English prose or whitespace.
#[test]
fn pattern_unknown_field_message_contains_tokens() {
    let source = shape_match_source(
        "        Circle { diameter: d } => 0.0mm,\n        _ => 0.0mm,",
    );
    assert!(
        has_error_containing(&source, "'Circle'") && has_error_containing(&source, "'diameter'"),
        "PatternUnknownField message must mention variant 'Circle' and field 'diameter'",
    );
}

/// PatternMissingField message must mention the variant name ('Rect') and
/// the missing field name ('height') — tests dynamic content without
/// pinning exact English prose or whitespace.
#[test]
fn pattern_missing_field_message_contains_tokens() {
    let source = shape_match_source(
        "        Rect { width: w } => w,\n        _ => 0.0mm,",
    );
    assert!(
        has_error_containing(&source, "'Rect'") && has_error_containing(&source, "'height'"),
        "PatternMissingField message must mention variant 'Rect' and missing field 'height'",
    );
}

/// (d) D4 preserved: a non-exhaustive payload-enum match (missing a tag, no _)
/// STILL emits the existing non-exhaustive-match diagnostic.
#[test]
fn non_exhaustive_payload_match_still_flagged() {
    // Circle and Rect covered; Point missing; no wildcard.
    let source = shape_match_source_untyped(
        "        Circle { radius: r } => r,\n        Rect { width: w, height: h } => w,",
    );
    assert!(
        has_error_containing(&source, "non-exhaustive"),
        "non-exhaustive match on Shape (Point missing) should still emit the \
         non-exhaustive diagnostic; error codes: {:?}",
        error_codes(&source),
    );
}

// ── anti-cascade coverage ─────────────────────────────────────────────────────

/// When the match discriminant is NOT a known enum type, VariantBind patterns
/// must NOT emit PatternUnknownField or PatternMissingField.
///
/// The `resolved_variant.is_some()` guard skips field-set checks entirely
/// when `resolved_enum` is None (non-enum discriminant), preventing spurious
/// diagnostics on ill-typed match expressions.
#[test]
fn non_enum_discriminant_no_pattern_field_diagnostics() {
    // Discriminant is a plain Length scalar — not an enum type.
    // `Foo { bar: b }` is a syntactically valid VariantBind pattern but the
    // discriminant is not a known enum, so resolved_enum is None and the
    // field-set check must NOT fire.
    let source = "\
structure def Widget {
    let x : Length = 5.0mm
    let area = match x {
        Foo { bar: b } => b,
        _ => 0.0mm,
    }
}
";
    assert!(
        !has_error_code(source, DiagnosticCode::PatternUnknownField),
        "non-enum discriminant must NOT emit PatternUnknownField",
    );
    assert!(
        !has_error_code(source, DiagnosticCode::PatternMissingField),
        "non-enum discriminant must NOT emit PatternMissingField",
    );
}

/// An unknown-field binder is bound with Type::Error so that arm-body
/// references to it resolve as a ValueRef rather than emitting a cascade
/// "unresolved name" error on top of PatternUnknownField.
#[test]
fn unknown_field_binder_body_reference_no_cascade() {
    // `d` is bound as an unknown-field binder (diameter ∉ Circle's fields).
    // Referencing `d` in the body must not add extra errors beyond
    // PatternUnknownField — the binder is inserted with Type::Error so the
    // body resolves `d` as a ValueRef, not an unresolved name.
    let source_with_ref = shape_match_source_untyped(
        "        Circle { diameter: d } => d,\n        _ => 0.0mm,",
    );
    let source_no_ref = shape_match_source_untyped(
        "        Circle { diameter: d } => 0.0mm,\n        _ => 0.0mm,",
    );
    // Baseline: PatternUnknownField fires for the unknown 'diameter' field.
    assert!(
        has_error_code(&source_no_ref, DiagnosticCode::PatternUnknownField),
        "baseline: expected PatternUnknownField for unknown 'diameter' field",
    );
    // Referencing the binder in the body must not increase the error count.
    let errs_with_ref = compile_with_stdlib_helper(&source_with_ref)
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let errs_no_ref = compile_with_stdlib_helper(&source_no_ref)
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    assert_eq!(
        errs_with_ref,
        errs_no_ref,
        "referencing unknown-field binder 'd' in arm body must not cascade into \
         extra errors (binder is bound with Type::Error so body resolves 'd' as a ValueRef)",
    );
}
