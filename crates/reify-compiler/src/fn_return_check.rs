//! Reconciles a fn's declared (or defaulted-to-`Real`) return type against the
//! type its compiled body produces. Call sites type a user-fn call strictly
//! from the signature, and evaluation returns the body value unconverted, so a
//! body the signature contradicts hands every caller a mistyped value.

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, Severity, SourceSpan, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, CompiledFunction};

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

/// What a compiled fn belongs to.
#[derive(Clone, Copy)]
pub(crate) enum FnOwner<'a> {
    /// A module fn, or a trait-static fn compiled under its namespaced name.
    Free,
    /// A trait associated fn compiled for the named conformer.
    Conformer(&'a str),
}

impl FnOwner<'_> {
    /// How diagnostics name the fn. An injected default body is compiled once
    /// per conformer, so naming the conformer keeps same-span reports distinct.
    fn subject(self, fn_name: &str) -> String {
        match self {
            FnOwner::Free => format!("function '{fn_name}'"),
            FnOwner::Conformer(conformer) => {
                format!("associated function '{fn_name}' (conformer '{conformer}')")
            }
        }
    }
}

/// Report a compiled fn whose body's result type contradicts its return type.
///
/// An explicit annotation is checked by the annotated arm; an absent one means
/// call sites type the result as the defaulted `Real`, so a body producing
/// anything else is reported as un-annotated.
pub(crate) fn reconcile_fn_return(
    fn_def: &reify_ast::FnDef,
    owner: FnOwner<'_>,
    compiled: &CompiledFunction,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(body) = &fn_def.body else {
        return;
    };
    if matches!(owner, FnOwner::Conformer(_)) && reads_receiver(fn_def, compiled) {
        return;
    }
    let declared = &compiled.return_type;
    let produced = &compiled.body.result_expr.result_type;
    let body_span = body.result_expr.span;
    match &fn_def.return_type {
        None => {
            if body_contradicts(declared, produced) {
                diagnostics.push(unannotated_return(
                    &owner.subject(&compiled.name),
                    declared,
                    produced,
                    body_span,
                ));
            }
        }
        Some(annotation) => {
            if annotation_is_reconciled(declared) && body_contradicts(declared, produced) {
                diagnostics.push(mismatched_return(
                    &owner.subject(&compiled.name),
                    declared,
                    annotation.span,
                    produced,
                    body_span,
                ));
            }
        }
    }
}

/// Whether an assoc-fn body reads its `self` receiver.
///
/// Such a conformer body is not reconciled: `compile_assoc_function`'s body
/// scope has no conformer template, so a receiver-member read types as the
/// dimensionless `Real` fallback rather than the member's declared type, and
/// reconciling it would report correct code. Task #8118 types those reads from
/// the conformer, after which this gate can go.
fn reads_receiver(fn_def: &reify_ast::FnDef, compiled: &CompiledFunction) -> bool {
    let Some(receiver) = fn_def.params.iter().find(|p| p.is_self) else {
        return false;
    };
    let mut found = false;
    let mut visit = |expr: &CompiledExpr| {
        if let CompiledExprKind::ValueRef(id) = &expr.kind
            && id.member == receiver.name
        {
            found = true;
        }
    };
    for (_, let_expr) in &compiled.body.let_bindings {
        let_expr.walk(&mut visit);
    }
    compiled.body.result_expr.walk(&mut visit);
    found
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
    subject: &str,
    defaulted: &Type,
    produced: &Type,
    body_span: SourceSpan,
) -> Diagnostic {
    diag_at(
        FN_RETURN_RECONCILE_SEVERITY,
        format!(
            "{subject} has no return type annotation, so callers type its result as \
             `{defaulted}`, but its body produces `{produced}`; annotate its return type"
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
    subject: &str,
    declared: &Type,
    annotation_span: SourceSpan,
    produced: &Type,
    body_span: SourceSpan,
) -> Diagnostic {
    diag_at(
        FN_RETURN_RECONCILE_SEVERITY,
        format!("{subject} declares return type `{declared}` but its body produces `{produced}`"),
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
