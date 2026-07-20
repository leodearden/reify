//! End-to-end compile-level tests for the `@display("unit")` annotation
//! (task 5233, PRD `display-unit-preference.md` §7b).
//!
//! These prove the two validation passes reach REAL bindings through the
//! entity.rs / guards.rs AST-lowering sites:
//!   - a well-formed label matching the binding dimension's ladder compiles
//!     clean (PRD §1 G1 shape: liters on a Volume capacity);
//!   - a well-formed label that is not a rung in the binding dimension's ladder
//!     (`@display("L")` on a Length binding) is an Error naming label +
//!     dimension — via a plain `param` AND via a `where {}`-guarded `param`
//!     (guards.rs wiring parity);
//!   - a shape violation (`@display("L","cm")`) is a Warning only, with no
//!     spurious dimension Error.
//!
//! The runtime rendering effect is out of scope (task 5235 / L4).

use reify_core::{Diagnostic, Severity};
use reify_test_support::compile_source;

// ── Helpers (mirror optimized_annotation_tests.rs) ───────────────────────────

fn error_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn warning_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

/// Error-severity diagnostics from the dimension-mismatch pass (their message
/// contains the phrase "display unit"; the shape Warnings do not).
fn dimension_errors(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    error_diags(diags)
        .into_iter()
        .filter(|d| d.message.contains("display unit"))
        .collect()
}

/// Warning-severity diagnostics from the shape check (their message names
/// `@display`).
fn display_shape_warnings(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    warning_diags(diags)
        .into_iter()
        .filter(|d| d.message.contains("@display"))
        .collect()
}

// ── (a) well-formed label matching the ladder → no display diagnostics ───────

#[test]
fn display_matching_volume_rung_compiles_without_display_diagnostics() {
    // PRD §1 G1 shape: `L` (liters) IS a rung in the Volume ladder.
    let source = r#"
structure Tank {
    @display("L")
    let capacity : Volume = 2mm * 3mm * 4mm
}
"#;
    let module = compile_source(source);

    assert!(
        dimension_errors(&module.diagnostics).is_empty(),
        "well-formed @display(\"L\") on a Volume binding must not error, got: {:?}",
        error_diags(&module.diagnostics)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        display_shape_warnings(&module.diagnostics).is_empty(),
        "well-formed @display(\"L\") must not warn, got: {:?}",
        warning_diags(&module.diagnostics)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ── (b) wrong-ladder rung on a param binding → dimension Error (entity.rs) ────

#[test]
fn display_wrong_ladder_on_param_errors_naming_label_and_dimension() {
    // `L` is a Volume rung, NOT a Length rung → Error on a Length param.
    let source = r#"
structure Rod {
    @display("L")
    param x : Length = 1mm
}
"#;
    let module = compile_source(source);

    let errs = dimension_errors(&module.diagnostics);
    assert_eq!(
        errs.len(),
        1,
        "expected exactly 1 display dimension error, got all errors: {:?}",
        error_diags(&module.diagnostics)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        errs[0].message.contains('L') && errs[0].message.contains("Length"),
        "error should name label L and dimension Length: {}",
        errs[0].message
    );
}

// ── (c) shape violation → Warning only, no dimension Error ────────────────────

#[test]
fn display_shape_violation_warns_without_dimension_error() {
    let source = r#"
structure Tank {
    @display("L", "cm")
    let v : Volume = 2mm * 3mm * 4mm
}
"#;
    let module = compile_source(source);

    let warns = display_shape_warnings(&module.diagnostics);
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 @display shape warning, got: {:?}",
        warning_diags(&module.diagnostics)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        warns[0].message.contains("exactly one argument"),
        "unexpected shape-warning message: {}",
        warns[0].message
    );
    assert!(
        dimension_errors(&module.diagnostics).is_empty(),
        "a shape violation must not also emit a dimension Error, got: {:?}",
        dimension_errors(&module.diagnostics)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ── (d) wrong-ladder rung on a guarded param → dimension Error (guards.rs) ────

#[test]
fn display_wrong_ladder_on_guarded_param_errors() {
    // A `where {}`-guarded param proves the guards.rs wiring parity.
    let source = r#"
structure Gate {
    param active : Bool = true
    where active {
        @display("L")
        param p : Length = 5mm
    }
}
"#;
    let module = compile_source(source);

    let errs = dimension_errors(&module.diagnostics);
    assert_eq!(
        errs.len(),
        1,
        "expected exactly 1 display dimension error from the guarded param, got all errors: {:?}",
        error_diags(&module.diagnostics)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        errs[0].message.contains("Length"),
        "error should name dimension Length: {}",
        errs[0].message
    );
}
