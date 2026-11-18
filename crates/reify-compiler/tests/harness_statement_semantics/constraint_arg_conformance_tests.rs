//! Binding-site conformance of constraint-instantiation arguments: every test
//! asserts on `DiagnosticCode::ConstraintArgTypeMismatch`, which
//! `expand_constraint_inst` (src/entity.rs) emits via `constraint_arg_type_conforms`
//! (src/type_compat.rs). This file is the one home for the helpers that count that code.

use reify_core::*;
use reify_test_support::{compile_source, compile_template};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Collect only error diagnostics.
fn error_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Collect only the diagnostics carrying `DiagnosticCode::ConstraintArgTypeMismatch`.
fn constraint_arg_type_mismatches(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ConstraintArgTypeMismatch))
        .collect()
}

/// Assert the binding-site rejection is the module's ONLY error: exactly one
/// `ConstraintArgTypeMismatch`, at Error severity, and no other Error. The
/// sole-error clause also proves the fixture is otherwise well-formed.
fn assert_sole_error_is_constraint_arg_type_mismatch(diags: &[Diagnostic]) {
    let mismatches = constraint_arg_type_mismatches(diags);
    assert_eq!(
        mismatches.len(),
        1,
        "expected exactly 1 ConstraintArgTypeMismatch, got {}; diagnostics: {:?}",
        mismatches.len(),
        diags
    );
    assert_eq!(
        mismatches[0].severity,
        Severity::Error,
        "ConstraintArgTypeMismatch must be Error severity; diagnostics: {:?}",
        diags
    );
    let errors = error_diags(diags);
    assert_eq!(
        errors.len(),
        1,
        "the binding-site ConstraintArgTypeMismatch must be the only error, got {}; \
         diagnostics: {:?}",
        errors.len(),
        diags
    );
}

// ── Task 4546: code-pinning test for ConstraintArgTypeMismatch ────────────────

/// Pin that exactly ONE diagnostic carries `DiagnosticCode::ConstraintArgTypeMismatch`
/// when a Bool literal is passed where a Length param is declared.
///
/// The filter is by code (not message substring), so the count is about this
/// code alone. Since task 5762 the rejected binding short-circuits the
/// instantiation, so the substituted predicate `true > 0mm` is no longer
/// compiled and task 4490's `CmpOperandKind` is not co-emitted.
///
/// `DiagnosticCode` is in scope via `use reify_core::*` at the top of this file.
///
/// RED (before step-6): `ConstraintArgTypeMismatch` variant does not yet exist
/// on `DiagnosticCode` — this test won't compile. After step-6 adds the variant
/// AND wires `.with_code(DiagnosticCode::ConstraintArgTypeMismatch)` at the
/// emission site in entity.rs, the test will be GREEN.
#[test]
fn constraint_arg_type_mismatch_carries_code() {
    let source = r#"
constraint def MinWall {
    param w: Length
    w > 0mm
}
structure S {
    constraint MinWall(w: true)
}
"#;
    let module = compile_source(source);
    let code_count = constraint_arg_type_mismatches(&module.diagnostics).len();
    assert_eq!(
        code_count, 1,
        "expected exactly 1 diagnostic with code ConstraintArgTypeMismatch, \
         got {}; diagnostics: {:?}",
        code_count, module.diagnostics
    );
}

/// Guard: a Length-typed arg passed where a Length param is declared must NOT
/// produce any `ConstraintArgTypeMismatch` diagnostic (the happy-path
/// integration counterpart to `constraint_def_compile_tests.rs`'s
/// `type_mismatch_bool_for_length`).
///
/// This integration test exercises `expand_constraint_inst`'s type-check loop
/// end-to-end and confirms that Rule 3 (type_compatible identity) suppresses
/// the check for a correctly-typed arg.
#[test]
fn valid_length_arg_no_constraint_arg_type_mismatch() {
    let source = r#"
constraint def MinWall {
    param w: Length
    w > 0mm
}
structure S {
    constraint MinWall(w: 5mm)
}
"#;
    let module = compile_source(source);
    let code_count = constraint_arg_type_mismatches(&module.diagnostics).len();
    assert_eq!(
        code_count, 0,
        "expected zero ConstraintArgTypeMismatch diagnostics for a valid 5mm arg, \
         got {}; diagnostics: {:?}",
        code_count, module.diagnostics
    );
}

// ── Task 5762 (δ₂): cross-dimension constraint args are rejected at the binding site ──

/// A Mass arg at a Length param whose predicate does not compare dimensions
/// (`determined(v)`) is rejected at the binding site — no predicate-side guard
/// could catch it.
#[test]
fn cross_dimension_arg_at_determinacy_predicate_is_constraint_arg_type_mismatch() {
    let source = r#"
constraint def Determined {
    param v : Length
    determined(v)
}
structure S {
    param m : Mass = 3kg
    constraint Determined(v: m)
}
"#;
    let module = compile_source(source);
    assert_sole_error_is_constraint_arg_type_mismatch(&module.diagnostics);
}

/// A Mass arg at a Length param compared against a bare zero (`w > 0`, which
/// adopts the arg's dimension) is rejected at the binding site.
#[test]
fn cross_dimension_arg_at_bare_zero_comparison_is_constraint_arg_type_mismatch() {
    let source = r#"
constraint def Positive {
    param w : Length
    w > 0
}
structure S {
    param m : Mass = 3kg
    constraint Positive(w: m)
}
"#;
    let module = compile_source(source);
    assert_sole_error_is_constraint_arg_type_mismatch(&module.diagnostics);
}

/// A dimensionless literal arg at a Length param is rejected at the binding site.
#[test]
fn dimensionless_literal_arg_for_length_param_is_constraint_arg_type_mismatch() {
    let source = r#"
constraint def Positive {
    param w : Length
    w > 0
}
structure S {
    constraint Positive(w: 2.5)
}
"#;
    let module = compile_source(source);
    assert_sole_error_is_constraint_arg_type_mismatch(&module.diagnostics);
}

/// Vacuity guard and clean twin of the three rejections above: a same-dimension
/// arg at both defs compiles with no mismatch and no error.
#[test]
fn same_dimension_arg_at_constraint_def_param_compiles_clean() {
    let source = r#"
constraint def Determined {
    param v : Length
    determined(v)
}
constraint def Positive {
    param w : Length
    w > 0
}
structure S {
    param t : Length = 5mm
    constraint Determined(v: t)
    constraint Positive(w: t)
}
"#;
    let module = compile_source(source);
    let mismatches = constraint_arg_type_mismatches(&module.diagnostics);
    assert!(
        mismatches.is_empty(),
        "expected zero ConstraintArgTypeMismatch for same-dimension args, got: {:?}",
        mismatches
    );
    let errors = error_diags(&module.diagnostics);
    assert!(
        errors.is_empty(),
        "expected zero errors for same-dimension args, got: {:?}",
        errors
    );
}

/// A rejected binding REPLACES the predicate-body diagnostic rather than adding
/// to it: the mistake is reported once, at the argument, and the instantiation
/// is dropped exactly as a missing argument drops it.
///
/// Both masked fixtures would otherwise also be rejected inside the def body
/// (`w > 0mm`): a Mass arg by the comparison's dimension check, a Bool arg by
/// task 4490's comparison-operand guard.
#[test]
fn rejected_constraint_arg_is_reported_once_at_the_binding_site() {
    let cases = [
        (
            "Mass arg",
            "param m : Mass = 3kg\n    constraint MinWall(w: m)",
        ),
        ("Bool arg", "constraint MinWall(w: true)"),
    ];
    for (case, body) in cases {
        let source = format!(
            r#"
constraint def MinWall {{
    param w : Length
    w > 0mm
}}
structure S {{
    {body}
}}
"#
        );
        let (tmpl, diags) = compile_template(&source, "S");
        let errors = error_diags(&diags);
        assert_eq!(
            errors.len(),
            1,
            "[{case}] expected exactly 1 error (the binding-site rejection), got {}; \
             diagnostics: {:?}",
            errors.len(),
            diags
        );
        assert_eq!(
            errors[0].code,
            Some(DiagnosticCode::ConstraintArgTypeMismatch),
            "[{case}] the sole error must be ConstraintArgTypeMismatch; diagnostics: {:?}",
            diags
        );
        let emitted: Vec<&Option<String>> = tmpl
            .constraints
            .iter()
            .map(|c| &c.label)
            .filter(|label| label.as_deref().is_some_and(|l| l.starts_with("MinWall#")))
            .collect();
        assert!(
            emitted.is_empty(),
            "[{case}] a rejected instantiation must contribute no CompiledConstraint, \
             got labels: {:?}",
            emitted
        );
    }
}

/// Guard for the throwaway-sink flush: rejecting `w` must not swallow the
/// diagnostics of a sibling arg (`lim`) that was compiled on the promise that
/// predicate compilation would report them.
#[test]
fn rejected_constraint_arg_does_not_swallow_a_sibling_args_diagnostics() {
    let source = r#"
constraint def Within {
    param w : Length
    param lim : Length
    w < lim
}
structure S {
    param m : Mass = 3kg
    constraint Within(w: m, lim: no_such_name)
}
"#;
    let module = compile_source(source);
    let mismatches = constraint_arg_type_mismatches(&module.diagnostics);
    assert_eq!(
        mismatches.len(),
        1,
        "expected exactly 1 ConstraintArgTypeMismatch (for `w`), got {}; diagnostics: {:?}",
        mismatches.len(),
        module.diagnostics
    );
    assert!(
        module
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::UnresolvedName)),
        "the sibling arg's unresolved name must still be reported; diagnostics: {:?}",
        module.diagnostics
    );
}

/// Guard for the short-circuit's `where` compile: rejecting `w` drops the guard
/// group, but must not swallow the diagnostics of the instantiation's own
/// `where` condition.
#[test]
fn rejected_constraint_arg_does_not_swallow_its_where_clauses_diagnostics() {
    let source = r#"
constraint def MinWall {
    param w : Length
    w > 0mm
}
structure S {
    param m : Mass = 3kg
    constraint MinWall(w: m) where no_such_flag
}
"#;
    let module = compile_source(source);
    let mismatches = constraint_arg_type_mismatches(&module.diagnostics);
    assert_eq!(
        mismatches.len(),
        1,
        "expected exactly 1 ConstraintArgTypeMismatch (for `w`), got {}; diagnostics: {:?}",
        mismatches.len(),
        module.diagnostics
    );
    assert!(
        module
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::UnresolvedName)),
        "the where clause's unresolved name must still be reported; diagnostics: {:?}",
        module.diagnostics
    );
}

// ── Task 10029: Int↔Scalar constraint args are rejected at the binding site ──

/// An Int literal at a Length param is rejected at the binding site (task 10029).
///
/// This REVERSES a deliberate ruling, restated here rather than deleted per PRD
/// docs/prds/v0_6/dimensioned-construction-strictness.md §0.3 and §13 Open
/// question 1. Rule 4 of `constraint_arg_type_conforms` once accepted any
/// numeric pairing (task 4546). Task 5762 (δ₂) removed only its cross-dimension
/// half and kept Int-for-Length tolerated, pinned by this test's predecessor
/// `int_literal_for_length_param_no_constraint_arg_type_mismatch`. Task 10029
/// rejects it too: δ₁ (task 5646) already made `param t : Length = 1` an error,
/// and 5762 left `MinWall(w: 3.0)` rejected while `MinWall(w: 3)` was accepted.
///
/// In this masked fixture (`w > 0mm`) the predicate-body error `incompatible
/// types in comparison: Int vs Scalar[m]` is REPLACED by the binding-site code,
/// not doubled: the rejection short-circuits the instantiation (task 5762's
/// REPLACE ruling), so no `MinWall#0[0]` is emitted.
#[test]
fn int_literal_for_length_param_is_constraint_arg_type_mismatch() {
    let source = r#"
constraint def MinWall {
    param w: Length
    w > 0mm
}
structure S {
    constraint MinWall(w: 3)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");
    assert_sole_error_is_constraint_arg_type_mismatch(&diags);
    let labels: Vec<&Option<String>> = tmpl.constraints.iter().map(|c| &c.label).collect();
    assert!(
        !labels
            .iter()
            .any(|label| label.as_deref() == Some("MinWall#0[0]")),
        "the rejected binding must not emit MinWall#0[0]; got labels: {:?}",
        labels
    );
}

/// An Int literal at a Length param compared against a bare zero (`w > 0`,
/// which adopts the arg's type) is rejected at the binding site.
#[test]
fn int_literal_for_length_param_at_bare_zero_comparison_is_constraint_arg_type_mismatch() {
    let source = r#"
constraint def Positive {
    param w : Length
    w > 0
}
structure S {
    constraint Positive(w: 3)
}
"#;
    let module = compile_source(source);
    assert_sole_error_is_constraint_arg_type_mismatch(&module.diagnostics);
}

/// An Int cell at a Length param whose predicate does not compare dimensions
/// (`determined(v)`) is rejected at the binding site. The arg is a cell
/// because `determined` rejects a non-cell argument.
#[test]
fn int_cell_arg_at_determinacy_predicate_is_constraint_arg_type_mismatch() {
    let source = r#"
constraint def Determined {
    param v : Length
    determined(v)
}
structure S {
    param k : Int = 3
    constraint Determined(v: k)
}
"#;
    let module = compile_source(source);
    assert_sole_error_is_constraint_arg_type_mismatch(&module.diagnostics);
}

/// A Length literal at an Int param is rejected at the binding site. The bare
/// zero in `n > 0` adopts the arg's dimension, so nothing else catches it.
#[test]
fn length_arg_for_int_param_is_constraint_arg_type_mismatch() {
    let source = r#"
constraint def AtLeastOne {
    param n : Int
    n > 0
}
structure S {
    constraint AtLeastOne(n: 5mm)
}
"#;
    let module = compile_source(source);
    assert_sole_error_is_constraint_arg_type_mismatch(&module.diagnostics);
}

/// A dimensionless literal at an Int param is rejected at the binding site:
/// the Int→dimensionless widening is one-way.
#[test]
fn dimensionless_literal_arg_for_int_param_is_constraint_arg_type_mismatch() {
    let source = r#"
constraint def AtLeastOne {
    param n : Int
    n > 0
}
structure S {
    constraint AtLeastOne(n: 2.5)
}
"#;
    let module = compile_source(source);
    assert_sole_error_is_constraint_arg_type_mismatch(&module.diagnostics);
}

/// Vacuity guard and clean twin of the rejections above: an Int literal at an
/// Int param, and at a dimensionless `Real` param (the Int widening), binds
/// with no mismatch and no error, and both instantiations are emitted.
#[test]
fn int_literal_at_int_and_dimensionless_params_compiles_clean() {
    let source = r#"
constraint def AtLeastOne {
    param n : Int
    n > 0
}
constraint def PositiveRatio {
    param r : Real
    r > 0
}
structure S {
    constraint AtLeastOne(n: 3)
    constraint PositiveRatio(r: 3)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");
    let mismatches = constraint_arg_type_mismatches(&diags);
    assert!(
        mismatches.is_empty(),
        "expected zero ConstraintArgTypeMismatch for Int at Int and Real params, got: {:?}",
        mismatches
    );
    let errors = error_diags(&diags);
    assert!(
        errors.is_empty(),
        "expected zero errors for Int at Int and Real params, got: {:?}",
        errors
    );
    let labels: Vec<&Option<String>> = tmpl.constraints.iter().map(|c| &c.label).collect();
    for expected in ["AtLeastOne#0[0]", "PositiveRatio#0[0]"] {
        assert!(
            labels
                .iter()
                .any(|label| label.as_deref() == Some(expected)),
            "the accepted binding must emit {expected}; got labels: {:?}",
            labels
        );
    }
}
