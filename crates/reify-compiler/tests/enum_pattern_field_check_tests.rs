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
