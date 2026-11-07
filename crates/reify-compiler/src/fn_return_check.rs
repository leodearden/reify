//! Reconciles a fn's declared (or defaulted-to-`Real`) return type against the
//! type its compiled body produces. Call sites type a user-fn call strictly
//! from the signature, and evaluation returns the body value unconverted, so a
//! body the signature contradicts hands every caller a mistyped value.

use std::collections::HashMap;

use reify_core::{
    Diagnostic, DiagnosticCode, DiagnosticLabel, Severity, SourceSpan, Type, ValueCellId,
};
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
pub(crate) enum FnOwner {
    /// A module fn, or a trait-static fn compiled under its namespaced name.
    Free,
    /// A trait associated fn compiled for a conformer: its override, or a trait
    /// default body injected into it.
    Conformer,
}

impl FnOwner {
    /// How diagnostics name the fn. A report describes a body, not one
    /// conformer's compile of it, so no conformer is named.
    fn subject(self, fn_name: &str) -> String {
        match self {
            FnOwner::Free => format!("function '{fn_name}'"),
            FnOwner::Conformer => format!("associated function '{fn_name}'"),
        }
    }
}

/// Report a compiled fn whose body's result type contradicts its return type.
///
/// An explicit annotation is checked by the annotated arm; an absent one means
/// call sites type the result as the defaulted `Real`, so a body producing
/// anything else is reported as un-annotated.
///
/// A trait default body is compiled once per conformer, so it is reconciled
/// only once some structure conforms (before that, nothing can call it), and
/// it is reported once however many conformers compile it.
pub(crate) fn reconcile_fn_return(
    fn_def: &reify_ast::FnDef,
    owner: FnOwner,
    compiled: &CompiledFunction,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(body) = &fn_def.body else {
        return;
    };
    if matches!(owner, FnOwner::Conformer) && reads_receiver(fn_def, compiled) {
        return;
    }
    let declared = &compiled.return_type;
    let Some(produced) = judged_body_type(compiled) else {
        return;
    };
    let body_span = body.result_expr.span;
    match &fn_def.return_type {
        None => {
            if body_contradicts(declared, &produced) {
                push_once_per_body(
                    diagnostics,
                    unannotated_return(
                        &owner.subject(&compiled.name),
                        declared,
                        &produced,
                        body_span,
                    ),
                );
            }
        }
        Some(annotation) => {
            if annotation_is_reconciled(declared) && body_contradicts(declared, &produced) {
                push_once_per_body(
                    diagnostics,
                    mismatched_return(
                        &owner.subject(&compiled.name),
                        declared,
                        annotation.span,
                        &produced,
                        body_span,
                    ),
                );
            }
        }
    }
}

/// Push `report` unless the same body has already been reported: same code,
/// and labels at the same spans saying the same thing.
///
/// A trait default body is compiled for every conformer, and a trait-static
/// one also as its namespaced fn, but the user fixes it in one place.
fn push_once_per_body(diagnostics: &mut Vec<Diagnostic>, report: Diagnostic) {
    let same_body = |existing: &Diagnostic| {
        existing.code == report.code
            && existing.labels.len() == report.labels.len()
            && existing
                .labels
                .iter()
                .zip(&report.labels)
                .all(|(a, b)| a.span == b.span && a.message == b.message)
    };
    if !diagnostics.iter().any(same_body) {
        diagnostics.push(report);
    }
}

/// Report a bodyless required trait assoc fn that has no return annotation.
///
/// With no body that could ever reconcile it, callers and conformers would
/// type its result as the defaulted `Real`.
pub(crate) fn require_bodyless_return_annotation(
    fn_def: &reify_ast::FnDef,
    trait_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if fn_def.body.is_some() || fn_def.return_type.is_some() {
        return;
    }
    diagnostics.push(
        diag_at(
            FN_RETURN_RECONCILE_SEVERITY,
            format!(
                "required associated function '{trait_name}::{}' has no return type \
                 annotation; with no body to check against, callers and conformers type \
                 its result as `Real`; annotate its return type",
                fn_def.name
            ),
        )
        .with_code(DiagnosticCode::FnReturnTypeUnannotated)
        .with_label(DiagnosticLabel::new(
            fn_def.span,
            "required associated function without a return type annotation",
        )),
    );
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

/// The type a body is judged to produce, or `None` when it cannot be judged.
///
/// A bare generic static type is trusted only when the body hands back an input
/// of that type unchanged: arithmetic on a type parameter keeps the parameter's
/// type whatever the true result (`x * x` is T², `a / b` is dimensionless).
fn judged_body_type(compiled: &CompiledFunction) -> Option<Type> {
    let static_type = &compiled.body.result_expr.result_type;
    if is_bare_generic(static_type) {
        exact_body_type(compiled)
    } else {
        Some(static_type.clone())
    }
}

fn is_bare_generic(ty: &Type) -> bool {
    matches!(
        ty,
        Type::TypeParam(_) | Type::ScalarParam(_) | Type::Projection { .. }
    )
}

/// The id a reference in the body to the fn's param or let `name` carries.
///
/// Both fn compile paths build the body scope under the fn's own name, which is
/// `compiled.name`; #8239 will hand the reconciler the scope's entity instead of
/// assuming it. A nested binder (match arm, lambda, quantifier) mints another
/// entity, so a reference to one never aliases a param of the same name.
fn binding_id(compiled: &CompiledFunction, name: &str) -> ValueCellId {
    ValueCellId::new(&compiled.name, name)
}

/// The fn's inputs a body can hand back unchanged (a param, or a let that returns
/// one unchanged), keyed by the id a reference to each carries, with its type.
type ExactBindings = HashMap<ValueCellId, Type>;

/// The type of the input a body hands back unchanged, if it does.
///
/// Lets are folded in order, so a let that shadows a param (or an earlier let)
/// replaces its binding, and drops it when the let is not itself exact.
fn exact_body_type(compiled: &CompiledFunction) -> Option<Type> {
    let mut bindings: ExactBindings = compiled
        .params
        .iter()
        .map(|(name, ty)| (binding_id(compiled, name), ty.clone()))
        .collect();
    for (name, value) in &compiled.body.let_bindings {
        let id = binding_id(compiled, name);
        match exact_type(value, &bindings) {
            Some(ty) => {
                bindings.insert(id, ty);
            }
            None => {
                bindings.remove(&id);
            }
        }
    }
    exact_type(&compiled.body.result_expr, &bindings)
}

/// The type of the input `expr` evaluates to unchanged: a param or let
/// reference, or a conditional or match whose every branch is one and all agree.
fn exact_type(expr: &CompiledExpr, bindings: &ExactBindings) -> Option<Type> {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => bindings.get(id).cloned(),
        CompiledExprKind::Conditional {
            then_branch,
            else_branch,
            ..
        } => agreed([then_branch, else_branch].map(|branch| exact_type(branch, bindings))),
        CompiledExprKind::Match { arms, .. } => {
            agreed(arms.iter().map(|arm| exact_type(&arm.body, bindings)))
        }
        _ => None,
    }
}

/// The one type every entry is, or `None` when there are no entries, any entry
/// is `None`, or two differ.
fn agreed(types: impl IntoIterator<Item = Option<Type>>) -> Option<Type> {
    let mut types = types.into_iter();
    let first = types.next()??;
    types.all(|ty| ty.as_ref() == Some(&first)).then_some(first)
}

/// Whether a body producing `body` contradicts a signature declaring
/// `declared`.
///
/// Never true when either side is `Type::Error`: its root cause is already
/// reported.
fn body_contradicts(declared: &Type, body: &Type) -> bool {
    if declared.is_error() || body.is_error() {
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

#[cfg(test)]
mod tests {
    //! The `Projection` arm of the bare-generic gate. A fn signature cannot name
    //! an associated type (`x: P::Assoc` resolves to `Error`), so no `.ri` source
    //! yields a `Projection`-typed input and the harness tables cannot express
    //! this arm; it runs through `reconcile_fn_return` on hand-built inputs.
    use reify_ast::{Expr, ExprKind, FnBody, FnDef, TypeExpr, TypeExprKind};
    use reify_ir::BinOp;
    use reify_test_support::compiled_function;

    use super::*;

    fn projection() -> Type {
        Type::projection(Type::TypeParam("P".to_string()), "Assoc")
    }

    /// A reference to the param `x : P::Assoc` of `fn f`.
    fn x_ref() -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new("f", "x"), projection())
    }

    /// `x * x`, keeping its operand's type as arithmetic on a generic does.
    fn x_squared() -> CompiledExpr {
        CompiledExpr::binop(BinOp::Mul, x_ref(), x_ref(), projection())
    }

    fn real_annotation() -> TypeExpr {
        TypeExpr {
            kind: TypeExprKind::Named {
                name: "Real".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::new(0, 0),
        }
    }

    /// `fn f(x: P::Assoc) <return_type> { x }`: reconciliation reads only the
    /// return annotation and the body's span.
    fn fn_def(return_type: Option<TypeExpr>) -> FnDef {
        FnDef {
            name: "f".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            params: vec![],
            return_type,
            body: Some(FnBody {
                let_bindings: vec![],
                result_expr: Expr {
                    kind: ExprKind::Ident("x".to_string()),
                    span: SourceSpan::new(0, 0),
                },
            }),
            span: SourceSpan::new(0, 0),
            content_hash: reify_core::ContentHash::of_str("f"),
            annotations: vec![],
        }
    }

    /// The compiled `fn f(x: P::Assoc) { body }`, returning `Real`: the
    /// un-annotated default, and what an `-> Real` annotation resolves to.
    fn compiled(body: CompiledExpr) -> CompiledFunction {
        CompiledFunction {
            return_type: Type::dimensionless_scalar(),
            ..compiled_function("f", &[("x", projection())], vec![], body)
        }
    }

    fn reported(fn_def: &FnDef, compiled: &CompiledFunction) -> Vec<DiagnosticCode> {
        let mut diagnostics = Vec::new();
        reconcile_fn_return(fn_def, FnOwner::Free, compiled, &mut diagnostics);
        diagnostics.iter().filter_map(|d| d.code).collect()
    }

    #[test]
    fn unannotated_fn_handing_back_a_projection_typed_input_is_reported() {
        assert_eq!(
            reported(&fn_def(None), &compiled(x_ref())),
            [DiagnosticCode::FnReturnTypeUnannotated]
        );
    }

    #[test]
    fn annotated_fn_handing_back_a_projection_typed_input_is_reported() {
        assert_eq!(
            reported(&fn_def(Some(real_annotation())), &compiled(x_ref())),
            [DiagnosticCode::FnReturnTypeMismatch]
        );
    }

    #[test]
    fn projection_typed_arithmetic_placeholder_is_not_judged() {
        for (arm, declaration) in [
            ("un-annotated", fn_def(None)),
            ("annotated", fn_def(Some(real_annotation()))),
        ] {
            let codes = reported(&declaration, &compiled(x_squared()));
            assert!(
                codes.is_empty(),
                "expected no report on the {arm} arm, got: {codes:?}"
            );
        }
    }
}
