//! Tests for `LetAnnotationTypeMismatch` — let binding declared-type vs
//! initializer-type mismatch detected at the declaration site.
//!
//! This is the let analogue of `ParamDefaultTypeMismatch` (#4318). A bare or
//! negated numeric literal at a dimensioned Scalar let annotation is rejected
//! like any other dimensionless initializer (see
//! docs/prds/v0_6/dimensioned-construction-strictness.md §0.3).

use reify_core::DiagnosticCode;
use reify_test_support::{assert_single_declaration_error, compile_source, errors_only};

// ─── Step-1 tests ─────────────────────────────────────────────────────────────

/// A structure-body `let a : Length = 5kg` must produce exactly one
/// `LetAnnotationTypeMismatch` anchored at `let a`, whose message also says
/// "declared" and "initializer".
#[test]
fn let_annotation_dimension_mismatch_errors_at_declaration() {
    let source = r#"
structure S {
    let a : Length = 5kg
}
"#;

    let message = assert_single_declaration_error(
        source,
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "a",
    );
    assert!(
        message.contains("declared") && message.contains("initializer"),
        "message must say 'declared' and 'initializer', got: {message:?}"
    );
}

/// A force-dimensioned initializer at a `Length` let produces exactly one
/// `LetAnnotationTypeMismatch` anchored at `let a`. The force is built from
/// built-in units (kg·m·s^-2) because `N` lives only in the stdlib unit
/// registry. Force shares Length's m^1 exponent, so only a full-dimension
/// comparison rejects it.
#[test]
fn let_annotation_force_dimension_mismatch_errors_at_declaration() {
    let message = assert_single_declaration_error(
        "structure S { let a : Length = 5kg * 1m / (1s * 1s) }",
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "a",
    );
    assert!(
        message.contains("`Scalar[m]`") && message.contains("`Scalar[m·kg·s^-2]`"),
        "message must name declared `Scalar[m]` and found force `Scalar[m·kg·s^-2]`, \
         got: {message:?}"
    );
}

/// Valid let bindings with compatible annotations must NOT produce any
/// "declared … initializer" diagnostics.
///
/// - `let v : Length = 6mm`    — exact match (ok)
/// - `let c : Length = b * 2.0` — Length*Real = Length via algebra (ok)
/// - `let r : Real = 8.0`      — exact Real match (ok)
/// - `let n : Real = 8`        — Int→Real widening via type_compatible (ok)
///
/// This positive guard passes throughout all steps.
#[test]
fn valid_let_annotations_do_not_produce_declared_initializer_error() {
    let source = r#"
structure S {
    param b : Length = 3mm
    let v : Length = 6mm
    let c : Length = b * 2.0
    let r : Real = 8.0
    let n : Real = 8
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    // No error should carry both "declared" and "initializer" in its message.
    let false_positive = errors
        .iter()
        .find(|d| d.message.contains("declared") && d.message.contains("initializer"));
    assert!(
        false_positive.is_none(),
        "unexpected 'declared/initializer' error on valid let annotations: {:?}",
        false_positive
    );
}

// ─── Step-3 tests (appended) ──────────────────────────────────────────────────

/// Inverts a deliberate pin of the opposite rule: a bare or negated Int/Real
/// literal at a dimensioned Scalar let is rejected. The literal tolerance was
/// introduced by #4318; see docs/prds/v0_6/dimensioned-construction-strictness.md §0.3.
#[test]
fn let_annotation_int_and_real_literal_on_dimensioned_scalar_errors() {
    for (source, name) in [
        ("structure S { let x : Length = 5 }", "x"),
        ("structure S { let y : Length = 0.5 }", "y"),
        ("structure S { let z : Length = -5.0 }", "z"),
        ("structure S { let t : Length = 1.0 }", "t"),
    ] {
        assert_single_declaration_error(
            source,
            DiagnosticCode::LetAnnotationTypeMismatch,
            "let",
            name,
        );
    }
}

/// A pure numeric-literal-arithmetic initializer is neither a single literal nor
/// a dimensioned expression; at a dimensioned let it is rejected once, naming
/// the declared and found types.
#[test]
fn let_annotation_pure_literal_arithmetic_on_dimensioned_scalar_errors() {
    for (source, name, found) in [
        ("structure S { let a : Length = 2 + 3 }", "a", "`Int`"),
        ("structure S { let b : Length = 2.0 * 3.0 }", "b", "`Real`"),
    ] {
        let message = assert_single_declaration_error(
            source,
            DiagnosticCode::LetAnnotationTypeMismatch,
            "let",
            name,
        );
        assert!(
            message.contains("`Scalar[m]`") && message.contains(found),
            "let '{name}': message must name declared `Scalar[m]` and found {found}, \
             got: {message:?}"
        );
    }
    assert_single_declaration_error(
        "trait T { }\nstructure S { port p : out T { let c : Length = 2 + 3 } }",
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "c",
    );
}

/// A let declared `Int` with a fractional Real initializer (`0.5`) MUST produce
/// `LetAnnotationTypeMismatch`. `type_compatible(Int, Scalar[dimensionless])` is false
/// (one-directional widening: Int→dimensionless, not the reverse).
#[test]
fn let_int_declared_with_real_initializer_errors() {
    assert_single_declaration_error(
        "structure S { let i : Int = 0.5 }",
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "i",
    );
}

/// A let declared `Int` with a dimensioned initializer (`5kg`) MUST produce
/// `LetAnnotationTypeMismatch`. `type_compatible(Int, Scalar[kg])` is false.
#[test]
fn let_int_declared_with_dimensioned_initializer_errors() {
    assert_single_declaration_error(
        "structure S { let j : Int = 5kg }",
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "j",
    );
}

/// A let declared `Length` (scalar) with a Bool initializer (`true`) MUST produce
/// `LetAnnotationTypeMismatch`. `type_compatible(Scalar[m], Bool)` is false.
/// Tests the "wrong-type" path (non-scalar RHS with scalar declared type).
#[test]
fn let_scalar_declared_with_bool_initializer_errors() {
    assert_single_declaration_error(
        "structure S { let a : Length = true }",
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "a",
    );
}

/// Anti-cascade regression lock: `let a : Length = []` must produce EXACTLY ONE error
/// and its code must be `CollectionLiteralKindMismatch` (from β/#4702),
/// NOT `LetAnnotationTypeMismatch`.
///
/// β owns collection-literal-vs-annotation. The `rhs_is_collection_literal` skip in
/// `check_let_annotation_type` prevents the double-fire.
#[test]
fn let_annotation_empty_list_anti_cascade_single_collection_error() {
    let source = r#"
structure S {
    let a : Length = []
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // Must have exactly one error.
    assert!(
        errors.len() == 1,
        "expected exactly one error for 'let a : Length = []'; got: {:?}",
        errors
            .iter()
            .map(|d| (&d.message, &d.code))
            .collect::<Vec<_>>()
    );

    // That error must be CollectionLiteralKindMismatch (β's code), NOT LetAnnotationTypeMismatch.
    assert!(
        errors[0].code == Some(DiagnosticCode::CollectionLiteralKindMismatch),
        "expected CollectionLiteralKindMismatch for 'let a : Length = []'; \
         got code: {:?}, message: {:?}",
        errors[0].code,
        errors[0].message
    );

    let no_let_mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        no_let_mismatch.is_none(),
        "LetAnnotationTypeMismatch must NOT fire for a collection-literal RHS \
         (β owns this via CollectionLiteralKindMismatch); got: {:?}",
        no_let_mismatch
    );
}

/// Scope-guard (bullet-B deferred): `let xs : List<Length> = [1N]` must produce
/// NO `LetAnnotationTypeMismatch`. The declared type is `List<Length>` — not scalar —
/// so the scalar restriction in `check_let_annotation_type` skips the check.
/// Non-empty collection element conformance is the filed follow-up (#4705 §B).
#[test]
fn let_annotation_list_type_declared_no_let_annotation_mismatch() {
    let source = r#"
structure S {
    let xs : List<Length> = [1N]
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let let_mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        let_mismatch.is_none(),
        "LetAnnotationTypeMismatch must NOT fire for 'let xs : List<Length> = [1N]' \
         (declared is List — not scalar; scalar restriction skips); got: {:?}",
        let_mismatch
    );
}

/// Unresolved-annotation guard: `let a : Bogus = 5` must produce NO
/// `LetAnnotationTypeMismatch`. When the annotation type fails to resolve,
/// `expected_ty` is `None` and the check is skipped entirely — preserving
/// current behavior for unresolved let annotations.
#[test]
fn let_annotation_unresolved_type_no_let_annotation_mismatch() {
    let source = r#"
structure S {
    let a : Bogus = 5
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // There may be an UnresolvedType error, but there must be NO LetAnnotationTypeMismatch.
    let let_mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        let_mismatch.is_none(),
        "LetAnnotationTypeMismatch must NOT fire for 'let a : Bogus = 5' \
         (unresolved annotation -> expected_ty=None -> check skipped); got: {:?}",
        let_mismatch
    );
}

// ─── Step-5 tests (appended) ──────────────────────────────────────────────────

/// A port-member let with an explicit scalar annotation whose initializer
/// mismatches must produce `LetAnnotationTypeMismatch` anchored at the
/// port-member let declaration — completing let/param symmetry (param check
/// covers both the structure-body site and the port-member site).
///
/// Mirrors `port_member_param_dimension_mismatch_errors_at_declaration`.
#[test]
fn port_member_let_annotation_dimension_mismatch_errors_at_declaration() {
    let source = r#"
trait T { }
structure S {
    port p : out T {
        let d : Length = 5kg
    }
}
"#;

    assert_single_declaration_error(
        source,
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "d",
    );
}

/// Anti-cascade at site 2: port-member `let d : Length = []` must NOT produce
/// `LetAnnotationTypeMismatch`. β (#4702) owns collection-literal-vs-annotation via
/// `CollectionLiteralKindMismatch`; the `rhs_is_collection_literal` skip in
/// `check_let_annotation_type` prevents double-firing even at the port-member site
/// which resolves the annotation through a fresh throwaway-sink (not β's expected_ty).
#[test]
fn port_member_let_annotation_empty_list_no_let_annotation_mismatch() {
    let source = r#"
trait T { }
structure S {
    port p : out T {
        let d : Length = []
    }
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let let_mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        let_mismatch.is_none(),
        "LetAnnotationTypeMismatch must NOT fire for a collection-literal RHS \
         at the port-member site (β owns via CollectionLiteralKindMismatch); got: {:?}",
        let_mismatch
    );
}

/// Unresolved-annotation guard at site 2: port-member `let d : Bogus = 5` must
/// produce NO `LetAnnotationTypeMismatch`. Site 2 resolves the annotation via a
/// fresh throwaway-sink (`resolve_type_expr_with_aliases` into `Vec::new()`);
/// when resolution fails, `expected_ty` is `None` and the check is skipped —
/// matching site 1's behavior and preserving current tolerance for bad annotations.
#[test]
fn port_member_let_annotation_unresolved_type_no_let_annotation_mismatch() {
    let source = r#"
trait T { }
structure S {
    port p : out T {
        let d : Bogus = 5
    }
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // There may be other errors (e.g. UnresolvedType), but LetAnnotationTypeMismatch
    // must NOT appear — the check is skipped when resolution yields None.
    let let_mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        let_mismatch.is_none(),
        "LetAnnotationTypeMismatch must NOT fire for 'let d : Bogus = 5' at the \
         port-member site (unresolved annotation -> expected_ty=None -> check skipped); \
         got: {:?}",
        let_mismatch
    );
}

/// Valid port-member let bindings with compatible annotations must NOT produce any
/// `LetAnnotationTypeMismatch` diagnostics.
///
/// This is the port-member analogue of
/// `valid_let_annotations_do_not_produce_declared_initializer_error` for site 2.
/// It guards against site 2 (port-member) over-firing on valid bindings — a
/// regression that would slip through the structure-body positive guard alone,
/// because the two sites resolve the annotation through different paths:
/// site 1 reuses β's pre-computed `expected_ty`; site 2 resolves via a fresh
/// throwaway-sink.
///
/// - `let d : Length = 5mm`  — exact match (ok)
/// - `let e : Real = 8`      — Int→Real widening via `type_compatible` (ok)
#[test]
fn valid_port_member_let_annotations_do_not_produce_let_annotation_mismatch() {
    let source = r#"
trait T { }
structure S {
    port p : out T {
        let d : Length = 5mm
        let e : Real = 8
    }
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let let_mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        let_mismatch.is_none(),
        "unexpected LetAnnotationTypeMismatch on valid port-member let annotations; \
         site 2 (port-member) must not over-fire on compatible bindings; got: {:?}",
        let_mismatch
    );
}

/// Inverts a deliberate pin of the opposite rule at the port-member site (site 2):
/// a bare or negated literal at a dimensioned Scalar let is rejected. The
/// tolerance was introduced by #4318; see docs/prds/v0_6/dimensioned-construction-strictness.md §0.3.
#[test]
fn port_member_let_annotation_numeric_literal_on_dimensioned_scalar_errors() {
    for (source, name) in [
        (
            "trait T { }\nstructure S { port p : out T { let d : Length = 5 } }",
            "d",
        ),
        (
            "trait T { }\nstructure S { port p : out T { let e : Length = -0.5 } }",
            "e",
        ),
    ] {
        assert_single_declaration_error(
            source,
            DiagnosticCode::LetAnnotationTypeMismatch,
            "let",
            name,
        );
    }
}
