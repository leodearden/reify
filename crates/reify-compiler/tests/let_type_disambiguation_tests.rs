//! Tests for DefaultKind::Let cell_type disambiguation (task 366).
//!
//! These tests verify that a trait's `let` binding carries the correct type
//! annotation in `DefaultKind::Let { cell_type, .. }`:
//!   - annotated `let x : Length = …` → `cell_type = Some(Type::length())`
//!   - unannotated `let x = …`        → `cell_type = None`
//!   - explicitly `let x : Real = …`  → `cell_type = Some(Type::Real)`
//!   - unknown annotation `let x : Nonexistent = …` → diagnostic + `Some(Type::Real)` fallback
//!
//! Steps 8 and 9 add integration tests for the conformance check path that
//! produced a false type-mismatch before this fix.

use reify_compiler::DefaultKind;
use reify_test_support::{compile_source, errors_only};
use reify_types::{DimensionVector, Type};

// ── helper ────────────────────────────────────────────────────────────────────

/// Compile `source`, find the named trait, and return the `cell_type` from its
/// first `DefaultKind::Let` default.
///
/// Panics if the trait is not found or has no Let default.
fn extract_let_cell_type(source: &str, trait_name: &str) -> Option<Type> {
    let module = compile_source(source);
    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == trait_name)
        .unwrap_or_else(|| panic!("expected trait {}", trait_name));
    let let_default = trait_def
        .defaults
        .iter()
        .find(|d| matches!(&d.kind, DefaultKind::Let { .. }))
        .unwrap_or_else(|| panic!("expected a Let default in trait {}", trait_name));
    match &let_default.kind {
        DefaultKind::Let { cell_type, .. } => cell_type.clone(),
        other => panic!("expected DefaultKind::Let, got {:?}", other),
    }
}

// ── step-1 (test): DefaultKind::Let carries cell_type ────────────────────────

/// A trait with `let x : Length = 5mm` must produce a DefaultKind::Let whose
/// cell_type is Some(Type::Scalar{LENGTH}).
#[test]
fn let_with_length_annotation_carries_cell_type() {
    let source = r#"
trait HasLength {
    let x : Length = 5mm
}
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "expected no errors, got: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        extract_let_cell_type(source, "HasLength"),
        Some(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        "annotated let x : Length should have cell_type = Some(Type::length())"
    );
}

/// A trait with unannotated `let x = 5.0` must produce a DefaultKind::Let
/// whose cell_type is None.
#[test]
fn let_without_annotation_has_none_cell_type() {
    let source = r#"
trait HasUntyped {
    let x = 5.0
}
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "expected no errors, got: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        extract_let_cell_type(source, "HasUntyped"),
        None,
        "unannotated let should have cell_type = None"
    );
}

/// A trait with `let x : Real = 5.0` must produce a DefaultKind::Let whose
/// cell_type is Some(Type::Real).
#[test]
fn let_with_real_annotation_carries_cell_type_real() {
    let source = r#"
trait HasReal {
    let x : Real = 5.0
}
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "expected no errors, got: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        extract_let_cell_type(source, "HasReal"),
        Some(Type::Real),
        "let x : Real should have cell_type = Some(Type::Real)"
    );
}

/// When the annotation names an unknown type, a diagnostic is emitted and
/// cell_type falls back to Some(Type::Real) for error-recovery (not None).
///
/// This guards against a silent regression where someone changes the fallback
/// from `Some(Type::Real)` to `None`, which would alter conformance semantics.
#[test]
fn let_with_unknown_annotation_emits_diagnostic_and_falls_back_to_real() {
    let source = r#"
trait HasBadType {
    let x : Nonexistent = 5.0
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected a diagnostic for unknown type 'Nonexistent'"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("Nonexistent") || d.message.contains("unresolved")),
        "diagnostic should mention the unknown type, got: {:?}",
        errors
    );
    assert_eq!(
        extract_let_cell_type(source, "HasBadType"),
        Some(Type::Real),
        "error-recovery fallback must be Some(Type::Real), not None"
    );
}

// ── step-8 (test): conformance integration — annotated let satisfies let requirement ──

/// Trait A provides `let x : Length = 5mm`, trait B requires `let x : Length`.
/// Structure S : A + B should compile without errors.
///
/// Before the fix, available_defaults used Type::Real for all Let defaults,
/// so the conformance check compared Real vs Scalar{LENGTH} → false type-mismatch.
#[test]
fn annotated_let_default_satisfies_let_requirement() {
    let source = r#"
trait A {
    let x : Length = 5mm
}
trait B {
    let x : Length
}
structure S : A + B {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "structure S : A + B should compile without type-mismatch errors, got: {:?}",
        errors
    );
}

// ── step-9 (test): scope registration — annotated let injects correctly ───────

/// Trait with `let x : Length = 5mm` injected into structure S (no override).
/// The injected ValueCellDecl for 'x' should exist in the compiled template.
#[test]
fn annotated_let_default_injects_value_cell() {
    let source = r#"
trait HasX {
    let x : Length = 5mm
}
structure S : HasX {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");

    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("expected value_cell 'x' to be injected from trait HasX");

    // The cell should be a Let kind.
    assert_eq!(
        x_cell.kind,
        reify_compiler::ValueCellKind::Let,
        "injected 'x' should be ValueCellKind::Let"
    );
}

// ── negative conformance test: conflicting Let defaults still produces a diagnostic ──

/// Two traits provide `let x` with different expressions (and different annotated types).
/// Structure S implements both without overriding — must produce a "conflicting let
/// bindings" diagnostic.
///
/// Note: the reify trait DSL requires `= expr` for all `let` bindings, so
/// `RequirementKind::Let` is not reachable from source syntax (see trait_merge_tests.rs:277).
/// This test verifies that the conformance engine still reports errors for genuinely
/// conflicting definitions, so the disambiguation fix did not accidentally suppress
/// all error reporting.
#[test]
fn conflicting_let_annotations_produce_diagnostic() {
    let source = r#"
trait ProvidesLength {
    let x : Length = 5mm
}
trait ProvidesArea {
    let x : Area = 1mm * 1mm
}
structure S : ProvidesLength + ProvidesArea {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "structure S : ProvidesLength + ProvidesArea should produce a conflict diagnostic, got none"
    );
    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("conflicting"),
        "diagnostic should mention 'conflicting', got: {}",
        error_msg
    );
}

// ── task 1834 step-1: annotation-vs-expression cross-check emits diagnostic ──

/// Trait with `let x : Length = 5.0` injected into a structure: the annotation
/// says Length, but 5.0 evaluates to Real — they are not compatible via
/// `implicitly_converts_to`, so conformance must emit an error diagnostic whose
/// message mentions "type mismatch".
///
/// Before task 1834 this was silent: the cell_type was taken from the compiled
/// expression (Real), so the annotation had no observable effect.
#[test]
fn annotated_let_expr_type_mismatch_emits_diagnostic() {
    let source = r#"
trait HasX {
    let x : Length = 5.0
}
structure S : HasX {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected a diagnostic for annotation/expression type mismatch, got none"
    );
    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("type mismatch"),
        "diagnostic should mention 'type mismatch', got: {}",
        error_msg
    );
}

// ── task 1834 step-2: injected let uses annotation type, not expr type ───────

/// Trait with `let x : Length = 5mm` injected into structure `S : HasX`.
/// The injected `ValueCellDecl` for `x` must have `cell_type ==
/// Type::Scalar { dimension: DimensionVector::LENGTH }`.
///
/// This pins the "annotation-is-authoritative" semantics of improvement 1:
/// after the fix, even if the expression's inferred type drifts (e.g. via a
/// new implicit-conversion rule that makes expr type and annotation type
/// differ while still being compatible), the annotation stays authoritative
/// on the cell — same invariant as the scope pre-registration, which already
/// uses the annotation via `.unwrap_or`.
#[test]
fn annotated_let_injected_cell_uses_annotation_type() {
    let source = r#"
trait HasX {
    let x : Length = 5mm
}
structure S : HasX {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");

    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("expected value_cell 'x' to be injected from trait HasX");

    assert_eq!(
        x_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "annotation type must be authoritative on the injected cell"
    );
}

// ── task 1834 step-3: compatible annotation+expression stays silent ─────────

/// Happy-path guard: exact-match annotation/expression types — `let x : Real = 5.0`
/// and `let x : Length = 5mm` — must compile without any error diagnostic.
/// Protects against a future over-eager cross-check that would reject the
/// happy path where the expression's inferred type implicitly converts to
/// (or exactly matches) the annotation type.
#[test]
fn annotated_let_compatible_expr_no_diagnostic() {
    // Exact match: Real annotation, Real expression.
    // `5.5` is a fractional literal → `Type::Real` (whole-number `.0` literals
    // are typed as `Int` by the compiler and would trip the cross-check).
    let real_source = r#"
trait HasR {
    let x : Real = 5.5
}
structure S : HasR {
}
    "#;
    let real_module = compile_source(real_source);
    let real_errors = errors_only(&real_module);
    assert!(
        real_errors.is_empty(),
        "let x : Real = 5.5 (exact Real) should not emit any errors, got: {:?}",
        real_errors
    );

    // Exact match: Length annotation, Length expression.
    let len_source = r#"
trait HasL {
    let x : Length = 5mm
}
structure S : HasL {
}
    "#;
    let len_module = compile_source(len_source);
    let len_errors = errors_only(&len_module);
    assert!(
        len_errors.is_empty(),
        "let x : Length = 5mm (exact Length) should not emit any errors, got: {:?}",
        len_errors
    );
}

// ── task 1834 step-5: unannotated let default satisfies typed let requirement ──

/// Trait A provides `let x = 5mm` (no annotation, expression inferred as Length),
/// trait B requires `let x : Length`.  Structure `S : A + B {}` must compile
/// cleanly: A's unannotated let default, once its expression type is inferred,
/// should match B's `Length` requirement.
///
/// Before task 1834, `available_defaults` advertised unannotated let defaults
/// with `Type::Real` (the `.unwrap_or(Type::Real)` fallback), so any Let-kind
/// requirement expecting Length would produce a false type-mismatch.
///
/// NOTE: the reify DSL currently does not syntactically accept
/// `let x : Length` without a value (see trait_merge_tests.rs:280), so trait B
/// here parses as empty and `RequirementKind::Let` is not reachable from source
/// today.  The test therefore passes trivially against the current code AND
/// the post-1834 code; it is retained as a forward-regression guard so that
/// whenever `let` requirement syntax is introduced, the `available_defaults`
/// inference behavior it relies on is already exercised.
#[test]
fn unannotated_let_default_satisfies_typed_let_requirement() {
    let source = r#"
trait A {
    let x = 5mm
}
trait B {
    let x : Length
}
structure S : A + B {
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "structure S : A + B with unannotated `let x = 5mm` default should compile \
         without type-mismatch errors (got: {:?})",
        errors
    );
}

