//! Dimension-mismatch validation for the `@display("unit")` annotation.
//!
//! A well-formed `@display("<label>")` names a preferred display unit for a
//! `param`/`let` binding (task 5233, PRD `display-unit-preference.md` §7b).
//! This module validates — in a pass SEPARATE from the schema-level arg-shape
//! check (`schema::check_display_args`) — that the label is a rung in the
//! binding dimension's unit ladder (`reify_core::unit_ladders`). Per PRD §2a
//! the schema `arg_check` hook has no binding-type context, so this dimension
//! check cannot be a same-hook extension; it runs at the entity.rs / guards.rs
//! AST-lowering sites, once the binding's `Type::Scalar { dimension }` is known.
//!
//! Shape violations (wrong arg count/type) are NOT this pass's concern — they
//! are reported as Warnings by `check_display_args`; a malformed `@display` has
//! no well-formed label and is skipped here.

use reify_core::Diagnostic;

/// STEP-5 STUB — empty body so the unit tests below compile and RED-fail; the
/// real implementation lands in step-6.
pub(crate) fn validate_display_dimension(
    _annotations: &[reify_ir::Annotation],
    _cell_type: &reify_core::Type,
    _diagnostics: &mut Vec<Diagnostic>,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test helpers ─────────────────────────────────────────────────────────

    /// `@display("<label>")` — one positional string-literal arg.
    fn disp(label: &str) -> reify_ir::Annotation {
        reify_ir::Annotation {
            name: reify_core::DISPLAY_ANNOTATION.to_string(),
            args: vec![reify_ir::AnnotationArg::positional(
                reify_ir::AnnotationArgValue::String(label.to_string()),
            )],
            span: reify_core::SourceSpan::empty(0),
        }
    }

    fn scalar(dim: reify_core::DimensionVector) -> reify_core::Type {
        reify_core::Type::Scalar { dimension: dim }
    }

    fn run(
        anns: &[reify_ir::Annotation],
        ty: &reify_core::Type,
    ) -> Vec<reify_core::Diagnostic> {
        let mut diags: Vec<reify_core::Diagnostic> = vec![];
        validate_display_dimension(anns, ty, &mut diags);
        diags
    }

    fn errors(diags: &[reify_core::Diagnostic]) -> Vec<&reify_core::Diagnostic> {
        diags
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Error)
            .collect()
    }

    // ── (a) well-formed match → no diagnostic ────────────────────────────────

    #[test]
    fn display_l_on_volume_is_valid() {
        let diags = run(
            std::slice::from_ref(&disp("L")),
            &scalar(reify_core::DimensionVector::VOLUME),
        );
        assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);
    }

    // ── (b) wrong-ladder rung → Error naming label + dimension ───────────────

    #[test]
    fn display_l_on_length_errors_naming_l_and_length() {
        let diags = run(
            std::slice::from_ref(&disp("L")),
            &scalar(reify_core::DimensionVector::LENGTH),
        );
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1, "expected exactly 1 error, got: {:?}", diags);
        assert!(
            errs[0].message.contains('L'),
            "message should name the label L: {}",
            errs[0].message
        );
        assert!(
            errs[0].message.contains("Length"),
            "message should name the dimension Length: {}",
            errs[0].message
        );
    }

    // ── (c) unknown label anywhere → Error naming label + dimension ──────────

    #[test]
    fn display_furlong_on_volume_errors_naming_furlong_and_volume() {
        let diags = run(
            std::slice::from_ref(&disp("furlong")),
            &scalar(reify_core::DimensionVector::VOLUME),
        );
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1, "expected exactly 1 error, got: {:?}", diags);
        assert!(
            errs[0].message.contains("furlong"),
            "message should name the label furlong: {}",
            errs[0].message
        );
        assert!(
            errs[0].message.contains("Volume"),
            "message should name the dimension Volume: {}",
            errs[0].message
        );
    }

    // ── (d)(e) matching rungs on other ladders → no diagnostic ───────────────

    #[test]
    fn display_mm_on_length_is_valid() {
        let diags = run(
            std::slice::from_ref(&disp("mm")),
            &scalar(reify_core::DimensionVector::LENGTH),
        );
        assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);
    }

    #[test]
    fn display_pa_on_pressure_is_valid() {
        let diags = run(
            std::slice::from_ref(&disp("Pa")),
            &scalar(reify_core::DimensionVector::PRESSURE),
        );
        assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);
    }

    // ── (f) malformed @display → silent here (shape is arg_check's job) ───────

    #[test]
    fn malformed_display_int_arg_is_silent_in_this_pass() {
        let a = reify_ir::Annotation {
            name: reify_core::DISPLAY_ANNOTATION.to_string(),
            args: vec![reify_ir::AnnotationArg::positional(
                reify_ir::AnnotationArgValue::Int(5),
            )],
            span: reify_core::SourceSpan::empty(0),
        };
        let diags = run(
            std::slice::from_ref(&a),
            &scalar(reify_core::DimensionVector::VOLUME),
        );
        assert!(
            diags.is_empty(),
            "shape is arg_check's job; this pass must stay silent, got: {:?}",
            diags
        );
    }

    // ── (g) non-scalar + dimensionless bindings → skipped (deferred) ─────────

    #[test]
    fn display_on_non_scalar_and_dimensionless_is_skipped() {
        // Non-scalar binding (e.g. Bool) — no ladder concept, skipped.
        let diags = run(std::slice::from_ref(&disp("L")), &reify_core::Type::Bool);
        assert!(
            diags.is_empty(),
            "non-scalar binding must be skipped, got: {:?}",
            diags
        );
        // Dimensionless scalar binding — canonical_name() is None, skipped.
        let diags = run(
            std::slice::from_ref(&disp("L")),
            &scalar(reify_core::DimensionVector::DIMENSIONLESS),
        );
        assert!(
            diags.is_empty(),
            "dimensionless binding must be skipped, got: {:?}",
            diags
        );
    }

    // ── (h) no @display annotation present → no diagnostic ───────────────────

    #[test]
    fn no_display_annotation_produces_no_diagnostic() {
        // A non-@display annotation on a Length binding is untouched.
        let other = reify_ir::Annotation {
            name: reify_core::SOLVER_HINT_ANNOTATION.to_string(),
            args: vec![],
            span: reify_core::SourceSpan::empty(0),
        };
        let diags = run(
            std::slice::from_ref(&other),
            &scalar(reify_core::DimensionVector::LENGTH),
        );
        assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);
        // An empty annotation slice likewise yields nothing.
        let diags = run(&[], &scalar(reify_core::DimensionVector::LENGTH));
        assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);
    }
}
