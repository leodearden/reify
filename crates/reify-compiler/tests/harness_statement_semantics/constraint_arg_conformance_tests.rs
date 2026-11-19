//! Binding-site conformance of constraint-instantiation arguments: every test
//! asserts on `DiagnosticCode::ConstraintArgTypeMismatch`, which
//! `expand_constraint_inst` (src/entity.rs) emits via `constraint_arg_type_conforms`
//! (src/type_compat.rs). This file is the one home for the helpers that count that code.

use reify_core::*;
use reify_test_support::{compile_source, compile_template, diags_with_code, error_diags};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// [`diags_with_code`] specialised to `ConstraintArgTypeMismatch`.
fn constraint_arg_type_mismatches(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags_with_code(diags, DiagnosticCode::ConstraintArgTypeMismatch)
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

/// Assert the `ConstraintArgTypeMismatch` count for `source`, counted via
/// `constraint_arg_type_mismatches`, with a message naming `what` and dumping
/// the diagnostics on failure.
///
/// When `expected == 0` the module is ALSO asserted to compile with zero
/// Error-severity diagnostics. Without that second half every accept-side guard
/// here is VACUOUS: a constraint def that failed to compile, a `Zq.Close` that
/// failed to resolve, a parse error, or a spurious def-site `unknown type` each
/// yield zero mismatch diagnostics too, so a bare count of 0 cannot tell
/// "accepted" from "never checked". With it, 0 means compiled, checked, passed.
fn assert_arg_type_mismatches(source: &str, expected: usize, what: &str) {
    let module = compile_source(source);
    let got = constraint_arg_type_mismatches(&module.diagnostics).len();
    assert_eq!(
        got, expected,
        "expected {} ConstraintArgTypeMismatch diagnostic(s) for {}, got {}; diagnostics: {:?}",
        expected, what, got, module.diagnostics
    );
    if expected == 0 {
        let errors = error_diags(&module.diagnostics);
        assert!(
            errors.is_empty(),
            "expected {} to compile CLEAN, but got error diagnostic(s): {:?}. \
             A zero mismatch count is only meaningful if the arg was actually \
             reached and checked",
            what,
            errors
        );
    }
}

// ── Task 4546: code-pinning test for ConstraintArgTypeMismatch ────────────────

/// Pin that exactly ONE diagnostic carries `DiagnosticCode::ConstraintArgTypeMismatch`
/// when a Bool literal is passed where a Length param is declared.
///
/// The filter is by code (not message substring), so the count is about this
/// code alone. Since task 5762 the rejected binding short-circuits the
/// instantiation, so the substituted predicate `true > 0mm` is no longer
/// compiled and task 4490's `CmpOperandKind` is not co-emitted.
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
    assert_arg_type_mismatches(source, 1, "a Bool literal at a Length param");
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
    assert_arg_type_mismatches(source, 0, "a valid 5mm arg at a Length param");
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
    assert_arg_type_mismatches(source, 0, "same-dimension args");
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

// ── Task 6416: the #4546 arg type check now fires for enum-typed params ──────

/// Before task 6416 an enum-typed param carried `ty: None`, so
/// `expand_constraint_inst` skipped it and an Int arg passed where an enum was
/// declared produced ZERO diagnostics. Now `ty` is `Some(Enum("Zq"))` and Rule 5
/// of `constraint_arg_type_conforms` rejects it.
///
/// This is the user-visible defect in task 6416's title, pinned independently of
/// the def-side structural assertion in `constraint_def_compile_tests.rs`.
#[test]
fn int_arg_for_enum_param_is_rejected() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Zq
    true
}
structure S {
    constraint K(g: 5)
}
"#,
        1,
        "an Int literal passed to an Enum(Zq) param",
    );
}

/// Bool counterpart of `int_arg_for_enum_param_is_rejected` — also measured at
/// zero diagnostics before task 6416.
#[test]
fn bool_arg_for_enum_param_is_rejected() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Zq
    true
}
structure S {
    constraint K(g: true)
}
"#,
        1,
        "a Bool literal passed to an Enum(Zq) param",
    );
}

/// A variant of the WRONG enum must be rejected too — `Enum(Other)` vs
/// `Enum(Zq)` is a Rule-5 rejection, not merely a non-enum-vs-enum one. This is
/// the case most likely to bite in real code, since it still *looks* enum-typed.
#[test]
fn wrong_enum_variant_arg_for_enum_param_is_rejected() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }
enum Other { A, B }

constraint def K {
    param g : Zq
    true
}
structure S {
    constraint K(g: Other.A)
}
"#,
        1,
        "an Other.A variant passed to an Enum(Zq) param",
    );
}

/// Guard: the correct variant literal must NOT be rejected.
///
/// Task 6416 activates a type check against a population it has never run on,
/// so the dominant risk is a FALSE POSITIVE on valid code, not a missed
/// rejection. The three guards below pin that boundary from the accept side —
/// the reject tests above would stay green under an over-broad future change
/// that also began rejecting valid enum args.
#[test]
fn correct_enum_variant_arg_is_accepted() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Zq
    true
}
structure S {
    constraint K(g: Zq.Close)
}
"#,
        0,
        "the correct Zq.Close variant passed to an Enum(Zq) param",
    );
}

/// Guard: an enum param actually REFERENCED by the predicate must still be
/// accepted, exercising the predicate-substitution path rather than the trivial
/// `true` body used by the other cases.
#[test]
fn enum_param_referenced_by_predicate_is_accepted() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Zq
    g == Zq.Close
}
structure S {
    constraint K(g: Zq.Medium)
}
"#,
        0,
        "an enum arg bound to a param referenced by the predicate",
    );
}

/// Guard: an enum-typed STRUCTURE param forwarded as the arg must be accepted.
///
/// This is the shape of the one pre-existing enum-typed constraint-def param in
/// tracked source (`constraint_inst_tests.rs`'s `constraint_inst_match_substitution`),
/// converting an incidental pass into an explicit contract. Structure params
/// already resolved bare enum names before task 6416, so the arg's type is
/// `Enum(Zq)` and Rule 3 (`type_compatible` identity) accepts.
#[test]
fn enum_typed_structure_param_arg_is_accepted() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Zq
    true
}
structure S {
    param q: Zq
    constraint K(g: q)
}
"#,
        0,
        "an enum-typed structure param forwarded to an Enum(Zq) param",
    );
}

// ── Task 6416: the arg type check on the OPTION-WRAPPED enum param ──────────
//
// `param g : Option<Zq>` is the third spelling the `EnumNameScope` install newly
// resolves (`constraint_def_compile_tests.rs` pins the def-side
// `Some(Option(Enum("Zq")))`), and the only one that was user-visibly BROKEN
// before rather than merely under-typed. Resolving it activates #4546's arg
// check on a shape that had none, so the three cases below pin the consequence
// users actually see.

/// The `some(..)` spelling must be accepted for an `Option<Zq>` param.
#[test]
fn some_wrapped_enum_arg_for_option_typed_param_is_accepted() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Option<Zq>
    true
}
structure S {
    constraint K(g: some(Zq.Close))
}
"#,
        0,
        "a some(Zq.Close) arg passed to an Option<Enum(Zq)> param",
    );
}

/// A BARE variant passed to an `Option<Zq>` param must be rejected: there is no
/// `T -> Option<T>` widening in `type_compatible` (`type_compat.rs`), so the
/// `some(..)` wrapper is required — the same rule struct params already follow.
///
/// This direction is deliberate, not incidental, and is pinned so a future
/// widening (or a regression back to `ty: None`, which would skip the check and
/// silently accept) cannot land unnoticed. Before task 6416 this source emitted
/// a spurious def-site `unknown type 'Option'` and zero mismatches.
#[test]
fn bare_enum_arg_for_option_typed_param_is_rejected() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Option<Zq>
    true
}
structure S {
    constraint K(g: Zq.Close)
}
"#,
        1,
        "a bare Zq.Close variant passed to an Option<Enum(Zq)> param",
    );
}

/// A bare `none` is REJECTED for an `Option<Zq>` param. This pins TODAY's
/// behaviour, not a desired contract: `expr.rs` types a bare `none` as
/// `Option<Real>` ("contextual override happens at param/let sites") and the
/// constraint-arg binding site applies no such override, so #4546's check
/// compares `Option<Real>` against `Option<Enum(Zq)>`.
///
/// MEASURED: the cause is enum-independent and pre-existing — `param g :
/// Option<Length>` + `K(g: none)` fails identically ("expected
/// Option<Scalar[m]>, got Option<Real>"). Task 6416 only makes `Option<Zq>`
/// reach the check at all; before it, the def site emitted a spurious `unknown
/// type 'Option'`. Contextual typing of `none` here is filed as follow-up
/// ticket tkt_0RTT1BWK4518B7W6XX74XSE79D — flip the expected count to 0 when it
/// lands.
#[test]
fn bare_none_arg_for_option_typed_enum_param_is_rejected_today() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }

constraint def K {
    param g : Option<Zq>
    true
}
structure S {
    constraint K(g: none)
}
"#,
        1,
        "a bare `none` passed to an Option<Enum(Zq)> param (today's behaviour: \
         `none` defaults to Option<Real>)",
    );
}

// ── Task 6416 / step-5: the arg type check on the ENUM-BODIED ALIAS path ─────
//
// Task 6259's parity harness in `tests/harness_langcore/type_alias_compile_tests.rs`
// compares `alias_ty` against `direct_ty` only, so reverting task 6416's
// `EnumNameScope` install collapses both sides to `None` and leaves it green.
// The two cases below pin ABSOLUTE diagnostic counts through the alias spelling,
// which is what actually detects such a revert.

/// An Int literal passed to an ALIAS-typed enum param must be rejected exactly
/// as it is for the direct spelling — `type AL = Zq` resolves to `Enum(Zq)`, so
/// `expand_constraint_inst` type-checks the arg instead of skipping it.
#[test]
fn int_arg_for_alias_typed_enum_param_is_rejected() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }
type AL = Zq

constraint def K {
    param g : AL
    true
}
structure S {
    constraint K(g: 5)
}
"#,
        1,
        "an Int literal passed to an alias-typed (`type AL = Zq`) Enum param",
    );
}

/// Accept-side guard for the alias spelling: the correct variant must still pass.
///
/// Without this, the reject case above would stay green under an over-broad
/// future change that began rejecting every alias-typed enum arg — the same
/// false-positive risk the direct-spelling guards above cover.
#[test]
fn correct_enum_variant_arg_for_alias_typed_param_is_accepted() {
    assert_arg_type_mismatches(
        r#"
enum Zq { Close, Medium }
type AL = Zq

constraint def K {
    param g : AL
    true
}
structure S {
    constraint K(g: Zq.Close)
}
"#,
        0,
        "the correct Zq.Close variant passed to an alias-typed (`type AL = Zq`) Enum param",
    );
}
