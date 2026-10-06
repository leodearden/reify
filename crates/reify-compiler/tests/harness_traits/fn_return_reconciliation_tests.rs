//! Fn body type vs declared or defaulted return type reconciliation (task #5991).

use reify_compiler::CompiledModule;
use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::compile_source;

fn with_code(module: &CompiledModule, code: DiagnosticCode) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(code))
        .collect()
}

fn assert_no_errors(module: &CompiledModule) {
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {errors:?}"
    );
}

fn has_label_at(diagnostic: &Diagnostic, start: usize, len: usize) -> bool {
    let (start, end) = (start as u32, (start + len) as u32);
    diagnostic
        .labels
        .iter()
        .any(|l| l.span.start == start && l.span.end == end)
}

fn has_label_spanning(diagnostic: &Diagnostic, source: &str, needle: &str) -> bool {
    let start = source
        .find(needle)
        .unwrap_or_else(|| panic!("fixture must contain `{needle}`"));
    has_label_at(diagnostic, start, needle.len())
}

// ── Un-annotated free fns ───────────────────────────────────────────────────

/// An un-annotated fn whose body is dimensioned is typed `Real` at every call
/// site; the contradiction is reported as a warning anchored on the body.
#[test]
fn unannotated_fn_whose_body_is_dimensioned_warns() {
    let source = r#"
fn area(w: Length, h: Length) { w * h }
structure Plate {
    param w : Length = 2mm
    let a = area(w, w)
    constraint a > 5.0
}
"#;
    let module = compile_source(source);

    let unannotated = with_code(&module, DiagnosticCode::FnReturnTypeUnannotated);
    assert_eq!(
        unannotated.len(),
        1,
        "expected exactly one FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
    assert_eq!(unannotated[0].severity, Severity::Warning);
    assert_no_errors(&module);
    assert!(
        has_label_spanning(unannotated[0], source, "w * h"),
        "expected a label spanning the body `w * h`, got: {:?}",
        unannotated[0].labels
    );
}

#[test]
fn unannotated_fn_with_bool_body_warns() {
    let module = compile_source("fn is_long(x: Length) { x > 5mm }\n");

    let unannotated = with_code(&module, DiagnosticCode::FnReturnTypeUnannotated);
    assert_eq!(
        unannotated.len(),
        1,
        "expected exactly one FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}

/// Trait-static fns are compiled through `compile_function` under the
/// namespaced name `Defaults::default_len`.
#[test]
fn unannotated_trait_static_fn_with_dimensioned_body_warns() {
    let module = compile_source("trait Defaults { fn default_len() { 10mm } }\n");

    let unannotated = with_code(&module, DiagnosticCode::FnReturnTypeUnannotated);
    assert_eq!(
        unannotated.len(),
        1,
        "expected exactly one FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}

/// Bodies that agree with the `Real` default stay silent: a dimensionless
/// body and an `Int` body (which widens to the default).
#[test]
fn unannotated_fn_whose_body_agrees_with_real_is_silent() {
    for source in ["fn half(x: Real) { x / 2.0 }\n", "fn three() { 3 }\n"] {
        let module = compile_source(source);
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
            "expected no FnReturnTypeUnannotated for {source:?}, got: {:?}",
            module.diagnostics
        );
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
            "expected no FnReturnTypeMismatch for {source:?}, got: {:?}",
            module.diagnostics
        );
    }
}

/// Callers type an un-annotated fn as `Real`, but a body that hands back a
/// type-param-typed input unchanged produces exactly that parameter, which an
/// annotation can state.
#[test]
fn unannotated_generic_fn_whose_body_passes_its_type_param_through_warns() {
    for source in [
        "fn id<T>(x: T) { x }\n",
        "fn id_q<Q: Dimension>(x: Scalar<Q>) { x }\n",
        "fn relay<T>(x: T) { let y = x; y }\n",
        "fn pick<T>(c: Bool, a: T, b: T) { if c then a else b }\n",
        "enum Side { L, R }\nfn choose<T>(s: Side, a: T, b: T) { match s { L => a, R => b } }\n",
    ] {
        let module = compile_source(source);

        let unannotated = with_code(&module, DiagnosticCode::FnReturnTypeUnannotated);
        assert_eq!(
            unannotated.len(),
            1,
            "expected exactly one FnReturnTypeUnannotated for {source:?}, got: {:?}",
            module.diagnostics
        );
        assert_eq!(unannotated[0].severity, Severity::Warning);
        assert_no_errors(&module);
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
            "expected no FnReturnTypeMismatch for {source:?}, got: {:?}",
            module.diagnostics
        );
    }
}

/// The report names the annotation to write, the bare parameter the body hands
/// back, and anchors on the body expression rather than on the param it reads.
#[test]
fn unannotated_generic_fn_report_names_the_type_param_and_anchors_on_the_body() {
    let source = "fn id<T>(x: T) { x }\n";
    let module = compile_source(source);

    let unannotated = with_code(&module, DiagnosticCode::FnReturnTypeUnannotated);
    assert_eq!(
        unannotated.len(),
        1,
        "expected exactly one FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
    let body_start = source.find("{ x }").unwrap() + "{ ".len();
    assert!(
        has_label_at(unannotated[0], body_start, "x".len()),
        "expected a label on the body `x`, got: {:?}",
        unannotated[0].labels
    );
    assert!(
        unannotated[0].message.contains("`T`"),
        "expected the report to name `T`, got: {}",
        unannotated[0].message
    );

    let module = compile_source("fn id_q<Q: Dimension>(x: Scalar<Q>) { x }\n");

    let unannotated = with_code(&module, DiagnosticCode::FnReturnTypeUnannotated);
    assert_eq!(
        unannotated.len(),
        1,
        "expected exactly one FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
    assert!(
        unannotated[0].message.contains("`Scalar<Q>`"),
        "expected the report to name `Scalar<Q>`, got: {}",
        unannotated[0].message
    );
}

/// Arithmetic on a type param keeps the param's static type whatever the real
/// result (`x * x` is T², `a / b` is dimensionless, where `Real` is correct),
/// so such bodies are not judged.
#[test]
fn unannotated_generic_fn_whose_body_type_is_an_arithmetic_placeholder_is_silent() {
    for source in [
        "fn sq<T>(x: T) { x * x }\n",
        "fn ratio<T>(a: T, b: T) { a / b }\n",
        "fn area_q<Q: Dimension>(x: Scalar<Q>) { x * x }\n",
        "fn either<T>(c: Bool, a: T) { if c then a else a * 2 }\n",
        "fn shadow<T>(x: T) { let x = x * x; x }\n",
    ] {
        let module = compile_source(source);
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
            "expected no FnReturnTypeUnannotated for {source:?}, got: {:?}",
            module.diagnostics
        );
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
            "expected no FnReturnTypeMismatch for {source:?}, got: {:?}",
            module.diagnostics
        );
    }
}

/// A body that only sometimes hands back an input is not judged: an arm that
/// computes, branches of different types, or a match binder that merely shares
/// a param's name (in the last source the first arm's `v` is the payload, not
/// the param `v`).
#[test]
fn unannotated_generic_fn_whose_body_only_sometimes_hands_back_an_input_is_silent() {
    for source in [
        "enum Side { L, R }\nfn mixed<T>(s: Side, a: T, b: T) { match s { L => a, R => b * 2 } }\n",
        "fn differ<T>(c: Bool, a: T, n: Int) { if c then a else n }\n",
        "enum Slot<A> { Full { a: A }, Vacant }\nfn payload_or<T, U>(v: T, s: Slot<U>) { match s { Full { a: v } => v, Vacant => v } }\n",
    ] {
        let module = compile_source(source);
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
            "expected no FnReturnTypeUnannotated for {source:?}, got: {:?}",
            module.diagnostics
        );
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
            "expected no FnReturnTypeMismatch for {source:?}, got: {:?}",
            module.diagnostics
        );
    }
}

#[test]
fn unannotated_fn_with_erroring_body_reports_only_the_root_cause() {
    let module = compile_source("fn broken() { no_such_name }\n");

    assert!(
        module
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "expected the unresolved name to be reported as an Error, got: {:?}",
        module.diagnostics
    );
    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
        "expected no cascading FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}

// ── Annotated free fns ──────────────────────────────────────────────────────

/// A body that contradicts an explicit scalar annotation is reported against
/// both the annotation and the body.
#[test]
fn annotated_scalar_return_contradicted_by_body_warns() {
    let source = "fn sq(x: Length) -> Length { x * x }\n";
    let module = compile_source(source);

    let mismatches = with_code(&module, DiagnosticCode::FnReturnTypeMismatch);
    assert_eq!(
        mismatches.len(),
        1,
        "expected exactly one FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
    assert_eq!(mismatches[0].severity, Severity::Warning);
    assert_no_errors(&module);

    let annotation_start = source.find("-> Length").unwrap() + "-> ".len();
    assert!(
        has_label_at(mismatches[0], annotation_start, "Length".len()),
        "expected a label spanning the return annotation `Length`, got: {:?}",
        mismatches[0].labels
    );
    assert!(
        has_label_spanning(mismatches[0], source, "x * x"),
        "expected a label spanning the body `x * x`, got: {:?}",
        mismatches[0].labels
    );
    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
        "an annotated fn must not report FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}

/// Evaluation returns the body value unconverted, so a bare literal body hands
/// a `Real` to a `Length`-typed call site: no numeric-literal exemption.
#[test]
fn annotated_dimensioned_return_with_bare_literal_body_warns() {
    let module = compile_source("fn unit_len() -> Length { 1.0 }\n");

    assert_eq!(
        with_code(&module, DiagnosticCode::FnReturnTypeMismatch).len(),
        1,
        "expected exactly one FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
}

#[test]
fn annotated_int_return_with_real_body_warns() {
    let module = compile_source("fn trunc(x: Real) -> Int { x }\n");

    assert_eq!(
        with_code(&module, DiagnosticCode::FnReturnTypeMismatch).len(),
        1,
        "expected exactly one FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
}

/// A body that hands back a type-param-typed input unchanged contradicts a
/// concrete annotation whatever the instantiation.
#[test]
fn annotated_real_return_with_type_param_body_warns() {
    let module = compile_source("fn f<T>(x: T) -> Real { x }\n");

    let mismatches = with_code(&module, DiagnosticCode::FnReturnTypeMismatch);
    assert_eq!(
        mismatches.len(),
        1,
        "expected exactly one FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
    assert_eq!(mismatches[0].severity, Severity::Warning);
    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
        "an annotated fn must not report FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}

/// Agreeing bodies stay silent, including `Int` widening to `Real` and generic
/// declared types outside the `Int | Scalar` reconciliation scope.
#[test]
fn annotated_return_agreeing_with_body_is_silent() {
    for source in [
        "fn sq(x: Length) -> Area { x * x }\n",
        "fn one() -> Real { 1 }\n",
        "fn n() -> Int { 3 }\n",
        "fn id<T>(x: T) -> T { x }\n",
        "fn scale_q<Q: Dimension>(x: Scalar<Q>, k: Real) -> Scalar<Q> { x * k }\n",
    ] {
        let module = compile_source(source);
        assert!(
            with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
            "expected no FnReturnTypeMismatch for {source:?}, got: {:?}",
            module.diagnostics
        );
    }
}

#[test]
fn annotated_fn_with_erroring_body_reports_only_the_root_cause() {
    let module = compile_source("fn broken() -> Length { no_such_name }\n");

    assert!(
        module
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "expected the unresolved name to be reported as an Error, got: {:?}",
        module.diagnostics
    );
    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
        "expected no cascading FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
}

#[test]
fn unresolved_return_annotation_reports_only_the_root_cause() {
    let module = compile_source("fn f(x: Length) -> Bogus { x }\n");

    assert!(
        !with_code(&module, DiagnosticCode::UnresolvedType).is_empty(),
        "expected the unresolved annotation to be reported, got: {:?}",
        module.diagnostics
    );
    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
        "expected no cascading FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
}

// ── Conformer twin (trait assoc fns compiled per conformer) ─────────────────

/// An injected default body is compiled for each conformer; one that never
/// reads the receiver is reconciled like a free fn.
#[test]
fn conformer_default_fn_with_unannotated_receiver_free_body_warns() {
    let module = compile_source(
        r#"
trait Tagged {
    param d : Length
    fn flag(self) { true }
}
structure def Rod : Tagged { param d : Length = 5mm }
"#,
    );

    assert_eq!(
        with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).len(),
        1,
        "expected exactly one FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
    assert_no_errors(&module);
}

/// A default body is reported once, however many conformers inherit it, while
/// each override is its own body and reports separately. A trait-static default
/// is compiled both as a namespaced static fn and per conformer; it too is
/// reported once.
#[test]
fn conformer_default_body_is_reported_once_per_body() {
    let module = compile_source(
        r#"
trait Tagged {
    param d : Length
    fn flag(self) { true }
    fn default_len() { 10mm }
}
structure def Rod : Tagged { param d : Length = 5mm }
structure def Bar : Tagged { param d : Length = 9mm }
structure def Pin : Tagged {
    param d : Length = 2mm
    fn flag(self) { 2mm }
}
"#,
    );

    assert_eq!(
        with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).len(),
        3,
        "expected one FnReturnTypeUnannotated per offending body (default `flag`, \
         override `flag`, static `default_len`), got: {:?}",
        module.diagnostics
    );
    assert_no_errors(&module);
}

/// A receiver-taking default is compiled only per conformer. With no conformer
/// nothing can call it, so no caller can receive its defaulted `Real`, and it
/// is deliberately not reconciled.
#[test]
fn conformer_default_without_conformers_is_not_reconciled() {
    let module = compile_source(
        r#"
trait Tagged {
    param d : Length
    fn flag(self) { true }
}
"#,
    );

    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
        "expected no FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}

#[test]
fn conformer_override_contradicting_its_annotation_warns() {
    let module = compile_source(
        r#"
trait Sized {
    param d : Length
    fn unit(self) -> Length
}
structure def Rod : Sized {
    param d : Length = 5mm
    fn unit(self) -> Length { 1.0 }
}
"#,
    );

    assert_eq!(
        with_code(&module, DiagnosticCode::FnReturnTypeMismatch).len(),
        1,
        "expected exactly one FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
    assert!(
        with_code(&module, DiagnosticCode::TraitFnSignatureMismatch).is_empty(),
        "the override's signature agrees with the requirement, got: {:?}",
        module.diagnostics
    );
}

/// Correct bodies that read the receiver's members stay silent. Both bodies
/// agree with their signatures once receiver members are typed from the
/// conformer (#8118), so this holds before and after that fix; until then each
/// one's member-read fallback type contradicts its signature.
#[test]
fn conformer_body_reading_self_members_is_not_reconciled() {
    let module = compile_source(
        r#"
trait Cylindrical {
    param diameter : Length
    param length : Length
    fn lateral_area(self) -> Scalar<Area> { pi * diameter * length }
    fn length_in_mm(self) { length / 1mm }
}
structure def Pin : Cylindrical {
    param diameter : Length = 8mm
    param length : Length = 40mm
}
"#,
    );

    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeMismatch).is_empty(),
        "expected no FnReturnTypeMismatch, got: {:?}",
        module.diagnostics
    );
    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
        "expected no FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}

// ── Bodyless required trait fns ─────────────────────────────────────────────

/// A bodyless required fn has no body that could ever reconcile its defaulted
/// `Real`, so it must carry an annotation.
#[test]
fn bodyless_required_fn_without_return_annotation_warns() {
    let source = "trait Measured { fn measure(self) }\n";
    let module = compile_source(source);

    let unannotated = with_code(&module, DiagnosticCode::FnReturnTypeUnannotated);
    assert_eq!(
        unannotated.len(),
        1,
        "expected exactly one FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
    assert_eq!(unannotated[0].severity, Severity::Warning);
    assert_no_errors(&module);

    assert!(
        has_label_spanning(unannotated[0], source, "fn measure(self)"),
        "expected a label spanning the declaration `fn measure(self)`, got: {:?}",
        unannotated[0].labels
    );
}

#[test]
fn bodyless_required_fn_with_return_annotation_is_silent() {
    let module = compile_source("trait Measured { fn measure(self) -> Length }\n");

    assert!(
        with_code(&module, DiagnosticCode::FnReturnTypeUnannotated).is_empty(),
        "expected no FnReturnTypeUnannotated, got: {:?}",
        module.diagnostics
    );
}
