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

use reify_core::{Diagnostic, DiagnosticLabel};
use std::sync::OnceLock;

/// The unit-ladder table, built once and cached for the process lifetime.
///
/// `reify_core::unit_ladders()` allocates a fresh `Vec<DimensionLadder>` (with
/// nested `Vec`s and `String`s) on every call, but the table is static. Since
/// `validate_display_dimension` runs once per `param`/`let` binding at compile
/// time, we build the table once here and hand out a shared `'static` slice to
/// avoid rebuilding it for every annotated binding.
fn ladders() -> &'static [reify_core::DimensionLadder] {
    static LADDERS: OnceLock<Vec<reify_core::DimensionLadder>> = OnceLock::new();
    LADDERS.get_or_init(reify_core::unit_ladders).as_slice()
}

/// Extract the well-formed single string-literal label from a `@display`
/// annotation. Returns `Some(label)` ONLY for exactly one `String` argument;
/// any other shape (no args, a non-string arg, or extra args) returns `None`
/// and is left to `schema::check_display_args` to report as a Warning. Mirrors
/// `super::is_valid_optimized`'s well-formedness gate, but requires EXACTLY one
/// argument so a shape violation is not silently treated as a valid label.
fn display_label(ann: &reify_ir::Annotation) -> Option<&str> {
    match ann.args.as_slice() {
        [
            reify_ir::AnnotationArg {
                value: reify_ir::AnnotationArgValue::String(label),
                ..
            },
        ] => Some(label.as_str()),
        _ => None,
    }
}

/// Validate that each well-formed `@display("<label>")` names a rung in the
/// binding dimension's unit ladder (`reify_core::unit_ladders`).
///
/// For every `@display` annotation with a well-formed single string label:
/// - skip non-scalar bindings and dimensionless scalars (no ladder concept —
///   deferred, see the module header);
/// - resolve the ladder for the binding dimension via `canonical_name()`;
/// - if the label is not a rung in that ladder (or the dimension has no
///   ladder at all), push an Error naming both the label and the binding's
///   dimension, plus — when the label's ASCII-normalized spelling IS a rung —
///   a "did you mean" hint for task λ's relabel (the normalizer is
///   [`reify_core::display_units::ascii_label_spelling`], which lives beside
///   the tables it describes; the accept-set is unchanged).
///
/// Malformed `@display` shapes are skipped here (they have no well-formed label
/// via `display_label`); their Warning is emitted by `check_display_args`.
pub(crate) fn validate_display_dimension(
    annotations: &[reify_ir::Annotation],
    cell_type: &reify_core::Type,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for ann in annotations {
        if ann.name != reify_core::DISPLAY_ANNOTATION {
            continue;
        }
        // Only a well-formed single-string label is checked here; malformed
        // shapes are reported (as Warnings) by check_display_args.
        let Some(label) = display_label(ann) else {
            continue;
        };
        // A display-unit preference is only meaningful for a dimensioned
        // scalar. Non-scalar and dimensionless bindings have no ladder concept
        // and are deferred.
        let dimension = match cell_type {
            reify_core::Type::Scalar { dimension } if !dimension.is_dimensionless() => *dimension,
            _ => continue,
        };
        // Valid iff the binding dimension's ladder exists AND lists the label.
        let ladder = ladders()
            .iter()
            .find(|l| Some(l.dimension.as_str()) == dimension.canonical_name());
        let is_rung = |candidate: &str| {
            ladder
                .map(|l| l.units.iter().any(|u| u.label == candidate))
                .unwrap_or(false)
        };
        if !is_rung(label) {
            // Name the binding's dimension user-visibly: canonical_name when
            // present, else the composed base-SI Display fallback (never a raw
            // exponent vector — per money-dimension.md's mismatch rule). The
            // dimensionless case is already filtered out above.
            let dim_name = dimension
                .canonical_name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("{dimension}"));
            // Migration hint for task λ's ASCII relabel: suggest the caret
            // spelling ONLY when it is itself a rung of THIS dimension's
            // ladder, so the hint can never advertise a unit the binding could
            // not display anyway. A label with no superscript, one whose
            // normalized form is still unknown here, and a dimension with no
            // ladder at all (`is_rung` is then always false) get the bare
            // message. The mapping itself is stated once, next to the curated
            // tables it normalizes, rather than restated here.
            let message = match reify_core::display_units::ascii_label_spelling(label)
                .filter(|a| is_rung(a))
            {
                Some(ascii) => format!(
                    "@display(\"{label}\") is not a valid display unit for dimension \
                     {dim_name}; did you mean \"{ascii}\"?"
                ),
                None => format!(
                    "@display(\"{label}\") is not a valid display unit for dimension {dim_name}"
                ),
            };
            diagnostics.push(Diagnostic::error(message).with_label(DiagnosticLabel::new(
                ann.span,
                "display unit does not match binding dimension",
            )));
        }
    }
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
        // Assert on the QUOTED label token (`"L"`) so this is not satisfied by
        // the 'L' inside the dimension name "Length" — the check must actually
        // prove the label survives into the message.
        assert!(
            errs[0].message.contains("\"L\""),
            "message should name the label \"L\": {}",
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

    /// The `@display` label vocabulary follows the curated ladders' ASCII
    /// relabel (task λ, #5788; addendum L4).
    ///
    /// This pass validates a label by EXACT STRING EQUALITY against
    /// `reify_core::display_units::unit_ladders()`, so relabelling the curated
    /// tables from `mm²` to `mm^2` silently flips which annotations this
    /// compiler accepts: `@display("mm^2")` starts working and
    /// `@display("mm²")` starts erroring. That is a user-visible surface change
    /// with no corpus impact — `@display` uses across the whole repo are 0, so
    /// nothing needs migrating — but "no corpus impact" is not "no change", and
    /// the addendum is explicit that it must not ship silently. Pinning it here
    /// is the part λ can own; the prose belongs to task ξ.
    ///
    /// This test owns only the POSITIVE direction — that the caret spelling
    /// starts working. The negative direction (the superscript spelling now
    /// errors) is owned SOLELY by
    /// `superscript_display_label_error_hints_the_ascii_spelling` case (a),
    /// which drives the identical input through the identical dimension and
    /// asserts strictly more: the error count AND the migration hint. Restating
    /// it here would only make two tests fail together on every regression of
    /// one path, and leave a reader diffing them to discover they are the same
    /// case.
    #[test]
    fn display_label_vocabulary_is_ascii_after_the_curated_relabel() {
        // ASCII spelling: a real Area rung, so no diagnostic.
        let diags = run(
            std::slice::from_ref(&disp("mm^2")),
            &scalar(reify_core::DimensionVector::AREA),
        );
        assert!(
            diags.is_empty(),
            "@display(\"mm^2\") on an Area cell must be valid, got: {:?}",
            diags
        );
    }

    // ── (g) the relabel's migration seam: a superscript label is self-explaining ─

    /// The λ relabel flips `@display` acceptance, and a user whose file predates
    /// the flip gets an Error naming neither the accepted spelling nor the fact
    /// that the vocabulary moved. The GUI got an explicit migration seam for
    /// this same relabel (`normalizeUnitLabel`, #6028); this is the compiler's
    /// one-line equivalent.
    ///
    /// The hint NEVER widens the accept-set — it is computed only after the
    /// exact match has already failed, and only when the ASCII-normalized
    /// spelling is a rung of the SAME dimension's ladder, so it can never claim
    /// a unit the binding could not display anyway.
    #[test]
    fn superscript_display_label_error_hints_the_ascii_spelling() {
        // (a) `mm²` on Area: `mm^2` IS an Area rung, so the error carries the
        // migration hint alongside the existing label/dimension naming.
        let diags = run(
            std::slice::from_ref(&disp("mm\u{00B2}")),
            &scalar(reify_core::DimensionVector::AREA),
        );
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1, "expected exactly 1 error, got: {:?}", diags);
        assert!(
            errs[0].message.contains("did you mean \"mm^2\""),
            "message should hint the ASCII spelling: {}",
            errs[0].message
        );

        // (b) `kg/m\u{00B3}` on Density — the other relabelled glyph, and one
        // where the caret spelling sits mid-label rather than at the end.
        let diags = run(
            std::slice::from_ref(&disp("kg/m\u{00B3}")),
            &scalar(reify_core::DimensionVector::MASS_DENSITY),
        );
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1, "expected exactly 1 error, got: {:?}", diags);
        assert!(
            errs[0].message.contains("did you mean \"kg/m^3\""),
            "message should hint the ASCII spelling: {}",
            errs[0].message
        );
    }

    /// The hint must not fire when normalizing does NOT reach a real rung —
    /// otherwise it would advertise a unit the binding cannot display, which is
    /// worse than no hint at all.
    #[test]
    fn hint_is_withheld_when_the_ascii_spelling_is_not_a_rung_either() {
        // (a) `mm²` on a LENGTH cell: `mm^2` is an Area rung, not a Length one,
        // so there is nothing truthful to suggest.
        let diags = run(
            std::slice::from_ref(&disp("mm\u{00B2}")),
            &scalar(reify_core::DimensionVector::LENGTH),
        );
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1, "expected exactly 1 error, got: {:?}", diags);
        assert!(
            !errs[0].message.contains("did you mean"),
            "no rung matches the normalized spelling, so no hint: {}",
            errs[0].message
        );

        // (b) a label with no superscript at all is untouched by the seam.
        let diags = run(
            std::slice::from_ref(&disp("furlong")),
            &scalar(reify_core::DimensionVector::VOLUME),
        );
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1, "expected exactly 1 error, got: {:?}", diags);
        assert!(
            !errs[0].message.contains("did you mean"),
            "a non-superscript unknown label gets no hint: {}",
            errs[0].message
        );

        // (c) the `ladder == None` branch: TORQUE is a named dimension with NO
        // curated ladder at all (`unit_ladders()` covers Length/Area/Volume/
        // Angle/Mass/Pressure/Density/Force/Energy/Power). `is_rung` is then
        // false for EVERY candidate, so the accept check errors and the hint's
        // `.filter(is_rung)` withholds — the two call sites of the shared
        // closure agree, which is the whole reason it is shared. Cases (a)/(b)
        // and every other test in this module use a dimension that HAS a
        // ladder, so this arm is otherwise unexercised.
        let diags = run(
            std::slice::from_ref(&disp("mm\u{00B2}")),
            &scalar(reify_core::DimensionVector::TORQUE),
        );
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1, "expected exactly 1 error, got: {:?}", diags);
        assert!(
            errs[0].message.contains("Torque"),
            "message should name the dimension Torque via canonical_name: {}",
            errs[0].message
        );
        assert!(
            !errs[0].message.contains("did you mean"),
            "a dimension with no ladder has no rung to suggest: {}",
            errs[0].message
        );
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
