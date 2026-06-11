//! Compiler integration tests for **task 4119 δ**, selector composition algebra
//! (`union`/`intersect`/`difference`) — the E_SELECTOR_KIND_MISMATCH diagnostic
//! (BT1) and same-kind composition result type (Type::Selector(k)).
//!
//! RED until step-4 wires the `selector_composition_result_type` arm in
//! `crates/reify-compiler/src/expr.rs`.
//!
//! Two test groups:
//!   (a) Mixed-kind composition: `union`/`intersect`/`difference` over Face and
//!       Edge selectors each produce EXACTLY ONE `E_SELECTOR_KIND_MISMATCH`
//!       diagnostic (Error severity) naming both kinds.  BT1.
//!   (b) Same-kind composition: `union`/`intersect`/`difference` over two Face
//!       selectors compile with no errors and the binding's inferred type is
//!       `Type::Selector(Face)`.

use reify_core::{DiagnosticCode, Severity, ty::SelectorKind};
use reify_core::Type;
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ── Test sources ─────────────────────────────────────────────────────────────

/// Mixed-kind: `union(faces(b), edges(b))` — Face ∪ Edge → E_SELECTOR_KIND_MISMATCH.
const SOURCE_UNION_MIXED: &str = r#"
structure def UnionMixed {
    let b = box(10mm, 10mm, 10mm)
    let sel = union(faces(b), edges(b))
}
"#;

/// Mixed-kind: `intersect(faces(b), edges(b))` — Face ∩ Edge → E_SELECTOR_KIND_MISMATCH.
const SOURCE_INTERSECT_MIXED: &str = r#"
structure def IntersectMixed {
    let b = box(10mm, 10mm, 10mm)
    let sel = intersect(faces(b), edges(b))
}
"#;

/// Mixed-kind: `difference(faces(b), edges(b))` — Face ∖ Edge → E_SELECTOR_KIND_MISMATCH.
const SOURCE_DIFFERENCE_MIXED: &str = r#"
structure def DifferenceMixed {
    let b = box(10mm, 10mm, 10mm)
    let sel = difference(faces(b), edges(b))
}
"#;

/// Same-kind: `union(faces(b), faces(c))` — Face ∪ Face → Type::Selector(Face).
const SOURCE_UNION_SAME_KIND: &str = r#"
structure def UnionSameKind {
    let b = box(10mm, 10mm, 10mm)
    let c = box(20mm, 20mm, 20mm)
    let sel = union(faces(b), faces(c))
}
"#;

/// Same-kind: `intersect(faces(b), faces(c))` — Face ∩ Face → Type::Selector(Face).
const SOURCE_INTERSECT_SAME_KIND: &str = r#"
structure def IntersectSameKind {
    let b = box(10mm, 10mm, 10mm)
    let c = box(20mm, 20mm, 20mm)
    let sel = intersect(faces(b), faces(c))
}
"#;

/// Same-kind: `difference(faces(b), faces_by_normal(b, ...))` — Face ∖ Face → Selector(Face).
const SOURCE_DIFFERENCE_SAME_KIND: &str = r#"
structure def DifferenceSameKind {
    let b = box(10mm, 10mm, 10mm)
    let sel = difference(faces(b), faces_by_normal(b, [0, 0, 1], 1deg))
}
"#;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Locate the `default_expr` of the named value cell in the first template.
fn cell_default_expr<'a>(
    compiled: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a reify_ir::CompiledExpr {
    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("expected '{member}' value cell"))
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("expected '{member}' cell to have a default_expr"))
}

// ── (a) Mixed-kind: exactly one E_SELECTOR_KIND_MISMATCH per composition ─────

/// `union(faces(b), edges(b))` must emit EXACTLY ONE `E_SELECTOR_KIND_MISMATCH`
/// Error diagnostic whose message names BOTH kinds (Face and Edge). BT1.
#[test]
fn union_mixed_kind_emits_exactly_one_kind_mismatch_error() {
    let compiled = compile_source_with_stdlib(SOURCE_UNION_MIXED);
    let mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SelectorKindMismatch))
        .collect();

    assert_eq!(
        mismatches.len(),
        1,
        "union(faces, edges): expected exactly 1 E_SELECTOR_KIND_MISMATCH, got {}: {:#?}",
        mismatches.len(),
        mismatches
    );
    let d = mismatches[0];
    assert_eq!(
        d.severity,
        Severity::Error,
        "E_SELECTOR_KIND_MISMATCH must be Error severity"
    );
    // Message must name both kinds (case-insensitive).
    let msg = d.message.to_lowercase();
    assert!(
        msg.contains("face"),
        "message must name the Face kind, got: {:?}",
        d.message
    );
    assert!(
        msg.contains("edge"),
        "message must name the Edge kind, got: {:?}",
        d.message
    );
    // Must have at least one label (the call-site span).
    assert!(
        !d.labels.is_empty(),
        "E_SELECTOR_KIND_MISMATCH must carry a call-site label, got: {:#?}",
        d
    );
}

/// `intersect(faces(b), edges(b))` must emit EXACTLY ONE `E_SELECTOR_KIND_MISMATCH`.
#[test]
fn intersect_mixed_kind_emits_exactly_one_kind_mismatch_error() {
    let compiled = compile_source_with_stdlib(SOURCE_INTERSECT_MIXED);
    let mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SelectorKindMismatch))
        .collect();

    assert_eq!(
        mismatches.len(),
        1,
        "intersect(faces, edges): expected exactly 1 E_SELECTOR_KIND_MISMATCH, got {}: {:#?}",
        mismatches.len(),
        mismatches
    );
    let d = mismatches[0];
    assert_eq!(d.severity, Severity::Error, "must be Error severity");
    let msg = d.message.to_lowercase();
    assert!(msg.contains("face"), "message must name Face kind");
    assert!(msg.contains("edge"), "message must name Edge kind");
    assert!(!d.labels.is_empty(), "must carry a call-site label");
}

/// `difference(faces(b), edges(b))` must emit EXACTLY ONE `E_SELECTOR_KIND_MISMATCH`.
#[test]
fn difference_mixed_kind_emits_exactly_one_kind_mismatch_error() {
    let compiled = compile_source_with_stdlib(SOURCE_DIFFERENCE_MIXED);
    let mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SelectorKindMismatch))
        .collect();

    assert_eq!(
        mismatches.len(),
        1,
        "difference(faces, edges): expected exactly 1 E_SELECTOR_KIND_MISMATCH, got {}: {:#?}",
        mismatches.len(),
        mismatches
    );
    let d = mismatches[0];
    assert_eq!(d.severity, Severity::Error, "must be Error severity");
    let msg = d.message.to_lowercase();
    assert!(msg.contains("face"), "message must name Face kind");
    assert!(msg.contains("edge"), "message must name Edge kind");
    assert!(!d.labels.is_empty(), "must carry a call-site label");
}

// ── (b) Same-kind: no error, result type is Type::Selector(Face) ──────────────

/// `union(faces(b), faces(c))` must compile without any error-severity diagnostic
/// and the `sel` binding's inferred type must be `Type::Selector(Face)`.
#[test]
fn union_same_kind_compiles_clean_with_face_selector_type() {
    let compiled = compile_source_with_stdlib(SOURCE_UNION_SAME_KIND);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "union(faces, faces): must compile without errors; got: {errors:#?}"
    );

    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "union(faces(b), faces(c)) must infer Type::Selector(Face), got {:?}",
        sel_expr.result_type
    );
}

/// `intersect(faces(b), faces(c))` must compile without errors and the result
/// type must be `Type::Selector(Face)`.
#[test]
fn intersect_same_kind_compiles_clean_with_face_selector_type() {
    let compiled = compile_source_with_stdlib(SOURCE_INTERSECT_SAME_KIND);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "intersect(faces, faces): must compile without errors; got: {errors:#?}"
    );

    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "intersect(faces(b), faces(c)) must infer Type::Selector(Face), got {:?}",
        sel_expr.result_type
    );
}

/// `difference(faces(b), faces_by_normal(b,...))` must compile without errors
/// and the result type must be `Type::Selector(Face)`.
#[test]
fn difference_same_kind_compiles_clean_with_face_selector_type() {
    let compiled = compile_source_with_stdlib(SOURCE_DIFFERENCE_SAME_KIND);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "difference(faces, faces_by_normal): must compile without errors; got: {errors:#?}"
    );

    let sel_expr = cell_default_expr(&compiled, "sel");
    assert_eq!(
        sel_expr.result_type,
        Type::Selector(SelectorKind::Face),
        "difference(faces(b), faces_by_normal(b,...)) must infer Type::Selector(Face), got {:?}",
        sel_expr.result_type
    );
}
