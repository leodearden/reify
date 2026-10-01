//! Reconciles a fn's declared (or defaulted-to-`Real`) return type against the
//! type its compiled body produces. Call sites type a user-fn call strictly
//! from the signature, and evaluation returns the body value unconverted, so a
//! body the signature contradicts hands every caller a mistyped value.

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, Severity, SourceSpan, Type};
use reify_ir::CompiledFunction;

use crate::conformance::diag_at;
use crate::type_compat::type_compatible;

/// Severity of every fn-return reconciliation diagnostic: `Warning`, the
/// warn-mode phase of the fail-closed rollout (docs/invariants.md, enforcement
/// posture).
///
/// Every emit site reads this const, never a `Severity` literal, so promoting
/// the check to `Error` (task #7007) is a one-const flip — the same reasoning
/// documented on `CTOR_FIELD_CONFORMANCE_SEVERITY`.
pub(crate) const FN_RETURN_RECONCILE_SEVERITY: Severity = Severity::Warning;

/// Report a compiled fn whose body's result type contradicts its return type.
///
/// An explicit annotation is checked by the annotated arm; an absent one means
/// call sites type the result as the defaulted `Real`, so a body producing
/// anything else is reported as un-annotated.
pub(crate) fn reconcile_fn_return(
    fn_def: &reify_ast::FnDef,
    compiled: &CompiledFunction,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(body) = &fn_def.body else {
        return;
    };
    let declared = &compiled.return_type;
    let produced = &compiled.body.result_expr.result_type;
    let body_span = body.result_expr.span;
    match &fn_def.return_type {
        None => {
            if body_contradicts(declared, produced) {
                diagnostics.push(unannotated_return(
                    &compiled.name,
                    declared,
                    produced,
                    body_span,
                ));
            }
        }
        Some(annotation) => {
            if annotation_is_reconciled(declared) && body_contradicts(declared, produced) {
                diagnostics.push(mismatched_return(
                    &compiled.name,
                    declared,
                    annotation.span,
                    produced,
                    body_span,
                ));
            }
        }
    }
}

/// Whether an explicit return annotation resolving to `declared` is checked.
///
/// Only `Int` and `Scalar` are, mirroring the param and let annotation checks
/// (`check_param_default_type` / `check_let_annotation_type`): other declared
/// types (enums under erasure, `Option`, trait objects, collections) have
/// known body-inference gaps that would report correct code.
fn annotation_is_reconciled(declared: &Type) -> bool {
    matches!(declared, Type::Int | Type::Scalar { .. })
}

/// Whether a body producing `body` contradicts a signature declaring
/// `declared`.
///
/// Never true when either side is `Type::Error` (its root cause is already
/// reported), nor for a bare generic body (`TypeParam` / `ScalarParam` /
/// `Projection`), whose agreement depends on the instantiation (task #7008).
fn body_contradicts(declared: &Type, body: &Type) -> bool {
    if declared.is_error() || body.is_error() {
        return false;
    }
    if matches!(
        body,
        Type::TypeParam(_) | Type::ScalarParam(_) | Type::Projection { .. }
    ) {
        return false;
    }
    !type_compatible(declared, body)
}

fn unannotated_return(
    fn_name: &str,
    defaulted: &Type,
    produced: &Type,
    body_span: SourceSpan,
) -> Diagnostic {
    diag_at(
        FN_RETURN_RECONCILE_SEVERITY,
        format!(
            "function '{fn_name}' has no return type annotation, so callers type its \
             result as `{defaulted}`, but its body produces `{produced}`; annotate its \
             return type"
        ),
    )
    .with_code(DiagnosticCode::FnReturnTypeUnannotated)
    .with_label(DiagnosticLabel::new(
        body_span,
        format!(
            "body produces `{produced}`; the un-annotated return type defaults to `{defaulted}`"
        ),
    ))
}

fn mismatched_return(
    fn_name: &str,
    declared: &Type,
    annotation_span: SourceSpan,
    produced: &Type,
    body_span: SourceSpan,
) -> Diagnostic {
    diag_at(
        FN_RETURN_RECONCILE_SEVERITY,
        format!(
            "function '{fn_name}' declares return type `{declared}` but its body produces \
             `{produced}`"
        ),
    )
    .with_code(DiagnosticCode::FnReturnTypeMismatch)
    .with_label(DiagnosticLabel::new(
        annotation_span,
        "declared return type",
    ))
    .with_label(DiagnosticLabel::new(
        body_span,
        format!("body produces `{produced}`"),
    ))
}
