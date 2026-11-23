// See `reify-types::value::SampledField` for the rationale behind this allow:
// `Value::SampledField` carries an `AtomicBool` (excluded from
// `PartialEq`/`Ord`/`Hash`/`content_hash`) that nonetheless triggers
// `mutable_key_type` on every `BTreeMap<Value, _>` site.
#![allow(clippy::mutable_key_type)]

mod classifier;
mod cpsat;
mod decompose;
mod dependent_reads;
mod discrete_fallback;
mod dual_jacobian;
pub mod hc4;
pub mod interval;
pub mod linalg;
pub mod relate_solve;
mod registry;
pub mod scaling;
pub mod sketch;
mod slvs_sys;
mod solver;
mod solvespace;
pub mod trust_region;

pub use classifier::ConstraintClassifier;
pub use cpsat::{CpSatSolver, SolveAllResult};
pub use decompose::{SubProblem, decompose_into_components};
pub use registry::{ObjectiveConsumption, SolverRegistry, objective_consumption};
// Constrained-2D-sketch solver substrate (docs/prds/v0_6/constrained-2d-sketch.md
// §7 C5).  The typed data model is re-exported at the crate root so consumers
// name `reify_constraints::SketchSystem` rather than reaching into the module;
// the slvs-facing builder stays private.
pub use sketch::{
    SketchBuildError, SketchConstraint, SketchConstraintDef, SketchConstraintId, SketchEntity,
    SketchEntityDef, SketchEntityId, SketchEntityKind, SketchSlotKind, SketchSolveResult,
    SketchSystem, SketchValueField, SolvedSketchEntity,
};
// Task #6672 (solver-unification ε): the forward-mode AD adapter.  The module
// is private and the crate root is the ONLY path to it, so
// `reify_constraints::residual_jacobian` is not merely the preferred spelling
// — it is the reachable one.  `Jacobian` names `reify_expr` types in its
// public shape (`BranchRecord`, `KinkSite`), which a consumer reads from
// reify-expr; `JacobianModel` keeps its `Seeds` private, so no seed type
// crosses this boundary.
pub use dual_jacobian::{Jacobian, JacobianError, JacobianModel, residual_jacobian};
pub use solver::{DimensionalSolver, TrialModel};
// γ cost_robustness_tradeoff (task #4791): re-exported so integration tests can
// compute the λ=0 Chebyshev-centre reference independently of the tradeoff blend
// dispatch under test (see `cost_robustness_tradeoff_blend.rs`).
pub use solver::build_centrality_objective;
pub use solvespace::{SolveSpaceSolver, solve_sketch};

use std::ops::ControlFlow;
use std::sync::OnceLock;

use reify_core::{ConstraintNodeId, Diagnostic, DiagnosticCode, Type};
use reify_ir::{
    CompiledExpr, CompiledExprKind, ComputeDispatch, ComputeDispatchFailure,
    ComputeDispatchOutcome, ConstraintChecker, ConstraintDiagnostics, ConstraintInput,
    ConstraintResult, IndeterminateReason, RefusedOperation, Satisfaction, TransientReason,
    Value,
};

/// Does any leaf operand of `expr` resolve to `Undef` in `values`?
///
/// This is the canonical has-undefined-leaf predicate: collect every leaf
/// `ValueRef` (and `CrossSubGeometryRef` — see the `CrossSubGeometryRef` note
/// on [`classify_undef`]) and ask whether any of them is undefined in
/// `values`. [`classify_undef`] below is built on exactly this check, and
/// `reify-eval`'s `Engine::dispatch_constraints` (task 6169 ζ) calls this
/// function directly so its RepresentationWithin decline stays exact rather
/// than an independently-maintained copy that could silently diverge from
/// `classify_undef`'s has-undef half (task 6480).
///
/// Allocates: walks the expression tree and collects every leaf id per
/// call; intended for diagnostic/decline paths, not per-evaluation hot
/// loops.
pub fn has_undefined_leaf(expr: &reify_ir::CompiledExpr, values: &reify_ir::ValueMap) -> bool {
    any_undef(&expr.collect_value_refs(), values)
}

/// Borrow-only definedness check over an already-collected leaf-id slice;
/// avoids the `get_or_undef` clone since only a boolean is needed here. An
/// absent id counts as undefined, matching `get_or_undef`'s
/// absence-maps-to-`Value::Undef` semantics.
fn any_undef(ids: &[reify_core::ValueCellId], values: &reify_ir::ValueMap) -> bool {
    ids.iter().any(|id| values.get(id).is_none_or(Value::is_undef))
}

/// Classify the `Value::Undef` that `expr` evaluated to under `ctx`.
///
/// Returns:
/// - `UndefInputs { cells }` — at least one leaf is `Undef`; `cells` lists the
///   undefined cells, deduped and ordered by their `ValueCellId::Display` name.
/// - `OperationRefused` — every leaf is defined and the Undef traces to a pure
///   operation refusing plain-data operands (see [`refused_operation`]); the
///   one cause INV-SF-4 counts as unexpected.
/// - `OperatorUndefinedForKinds { kinds }` — every leaf is defined (or the
///   expression has no ValueRefs) but no refusal was proven; `kinds` lists the
///   distinct `value_kind_label` strings of the defined leaf values (deduped,
///   sorted alphabetically).
///
/// ## CrossSubGeometryRef
/// `CompiledExpr::collect_value_refs()` collects both `ValueRef` and
/// `CrossSubGeometryRef` leaf IDs. A `CrossSubGeometryRef` that is absent from the
/// `ValueMap` resolves to `Value::Undef` via `get_or_undef` and is reported as an
/// undefined input cell — the same as any missing ordinary input cell. In practice,
/// constraint expressions evaluated by `SimpleConstraintChecker` in M1 do not contain
/// `CrossSubGeometryRef` leaves (those are emitted exclusively for sub-geometry stamp
/// access and are resolved before constraint checking). The behaviour is therefore
/// correct for the current call sites; this comment documents the contract so future
/// callers are aware that cross-sub geometry refs will surface here if present.
fn classify_undef(expr: &CompiledExpr, ctx: &reify_expr::EvalContext) -> TransientReason {
    let values = ctx.values;
    let leaf_ids = expr.collect_value_refs();

    if !any_undef(&leaf_ids, values) {
        if let Some(refusal) = refused_operation(expr, ctx) {
            return refusal;
        }
        let mut kinds: Vec<String> = leaf_ids
            .iter()
            .map(|id| value_kind_label(&values.get_or_undef(id)))
            .collect();
        kinds.sort();
        kinds.dedup();
        return TransientReason::OperatorUndefinedForKinds { kinds };
    }

    // Ordered as rendered; the id breaks ties because rendering is not injective.
    let cells: std::collections::BTreeSet<(String, reify_core::ValueCellId)> = leaf_ids
        .into_iter()
        .filter(|id| values.get(id).is_none_or(Value::is_undef))
        .map(|id| (id.to_string(), id))
        .collect();
    TransientReason::UndefInputs {
        cells: cells.into_iter().map(|(_, id)| id).collect(),
    }
}

/// The pure operation that refused, when `expr`'s `Undef` traces to one.
///
/// Walks down from `expr` into the first `Undef` child of each BinOp, UnOp or
/// builtin FunctionCall until it reaches the node whose operands are all
/// defined, and reports it when [`refused_its_operands`] holds. Any other node
/// kind is unproven.
///
/// Cost, paid only on the Undef path: each level evaluates its node's children
/// up to the first Undef one, so the subtree `d` levels down is evaluated `d`
/// more times, and a builtin origin once more by [`refused_its_operands`]. A
/// user call would make that a repeated solve, so its presence declines the walk.
fn refused_operation(
    expr: &CompiledExpr,
    ctx: &reify_expr::EvalContext,
) -> Option<TransientReason> {
    if contains_user_function_call(expr) {
        return None;
    }
    let mut node = expr;
    loop {
        let operation = match &node.kind {
            CompiledExprKind::BinOp { op, .. } => RefusedOperation::Binary(*op),
            CompiledExprKind::UnOp { op, .. } => RefusedOperation::Unary(*op),
            CompiledExprKind::FunctionCall { function, .. } => {
                RefusedOperation::Builtin(function.name.clone())
            }
            _ => return None,
        };
        let mut operands = Vec::new();
        let first_undef_child = node.try_for_each_child(&mut |child| {
            let value = reify_expr::eval_expr(child, ctx);
            if value.is_undef() {
                return ControlFlow::Break(child);
            }
            operands.push(value);
            ControlFlow::Continue(())
        });
        if let ControlFlow::Break(child) = first_undef_child {
            node = child;
            continue;
        }
        return refused_its_operands(node, &operands).then(|| TransientReason::OperationRefused {
            operation,
            operands: operands.iter().map(value_kind_label).collect(),
        });
    }
}

/// Whether `node`, whose operands are all defined, itself refused them.
///
/// An operator over plain data refuses on its own. A builtin refuses only when
/// its static type is plain data and a stdlib family claims the call and
/// answers Undef; a name no family claims (a geometry constructor, a selector
/// ctor) is a call `eval_expr` cannot realize, not a refusal. A claimed
/// recognised-name stub also answers Undef (see
/// [`reify_stdlib::try_eval_builtin`]); the plain-data checks are what keep
/// one out.
fn refused_its_operands(node: &CompiledExpr, operands: &[Value]) -> bool {
    if !operands.iter().all(is_plain_data_value) {
        return false;
    }
    match &node.kind {
        CompiledExprKind::FunctionCall { function, .. } => {
            is_plain_data_type(&node.result_type)
                && matches!(
                    reify_stdlib::try_eval_builtin(&function.name, operands),
                    Some(Value::Undef)
                )
        }
        _ => true,
    }
}

/// A user call is never a proven refusal, and since #10033 it may reach the
/// uncached `@optimized` dispatcher, so the diagnostic path must not re-run it.
fn contains_user_function_call(expr: &CompiledExpr) -> bool {
    fn visit(expr: &CompiledExpr) -> ControlFlow<()> {
        if matches!(expr.kind, CompiledExprKind::UserFunctionCall { .. }) {
            return ControlFlow::Break(());
        }
        expr.try_for_each_child(&mut |child| visit(child))
    }
    visit(expr).is_break()
}

/// The closed set of values `eval_expr` treats purely; unknown kinds are not plain.
fn is_plain_data_value(value: &Value) -> bool {
    matches!(
        value,
        Value::Bool(_)
            | Value::Int(_)
            | Value::Real(_)
            | Value::String(_)
            | Value::Scalar { .. }
            | Value::Enum { .. }
    )
}

/// The closed set of static types whose values are plain data; unknown types are not plain.
fn is_plain_data_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Bool | Type::Int | Type::String | Type::Scalar { .. } | Type::Enum(_)
    )
}

/// A short human-readable label for the kind of a defined `Value`.
///
/// Delegates to [`Value::kind_name`] for every plain arm; the two enriched
/// arms below (`Scalar<{dimension}>`, `Enum<{type_name}>`) are the only
/// local special cases this crate needs.
///
/// The `other =>` catch-all means a NEWLY ADDED `Value` variant compiles here
/// silently and gets its bare name — unlike `kind_name` itself, which is
/// exhaustive and breaks to compile. So when a variant lands carrying a
/// payload worth interpolating into a constraint diagnostic, this function
/// must be revisited by hand. `operand_kind_labels_carry_their_enriched_payloads`
/// pins the two existing enrichments against accidental absorption.
fn value_kind_label(v: &Value) -> String {
    match v {
        Value::Scalar { dimension, .. } => format!("Scalar<{}>", dimension),
        Value::Enum { type_name, .. } => format!("Enum<{}>", type_name),
        other => other.kind_name().to_string(),
    }
}

/// Simple constraint checker for M1: evaluates constraint expressions
/// and checks whether they are satisfied (true), violated (false), or
/// indeterminate (undef).
///
/// An indeterminate verdict records the [`classify_undef`] reason; its
/// diagnostic is an error when that reason is unexpected under INV-SF-4
/// ([`IndeterminateReason::is_unexpected`]) and a warning otherwise.
pub struct SimpleConstraintChecker;

impl ConstraintChecker for SimpleConstraintChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        self.check_with_dispatch(input, None)
    }

    fn check_with_dispatch(
        &self,
        input: &ConstraintInput,
        dispatch: Option<&dyn ComputeDispatch>,
    ) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, expr)| check_one(id, expr, input, dispatch))
            .collect()
    }
}

/// Evaluate and judge one constraint. A supplied dispatcher is wrapped for
/// this evaluation only, so a failed `@optimized` call it reaches can
/// attribute an Indeterminate verdict.
fn check_one(
    id: &ConstraintNodeId,
    expr: &CompiledExpr,
    input: &ConstraintInput,
    dispatch: Option<&dyn ComputeDispatch>,
) -> ConstraintResult {
    let recorder = dispatch.map(FailureRecordingDispatch::new);
    let ctx = eval_context(input, recorder.as_ref());
    let value = reify_expr::eval_expr(expr, &ctx);
    let (satisfaction, diagnostics, indeterminate_reason) = match value {
        Value::Bool(true) => (
            Satisfaction::Satisfied,
            ConstraintDiagnostics::default(),
            None,
        ),
        Value::Bool(false) => (
            Satisfaction::Violated,
            ConstraintDiagnostics {
                messages: vec![
                    Diagnostic::error(format!("constraint {} violated", id))
                        .with_code(DiagnosticCode::ConstraintViolated),
                ],
            },
            None,
        ),
        Value::Undef => {
            let classified = classify_undef(expr, &ctx);
            indeterminate_result(
                id,
                classified,
                recorder.and_then(FailureRecordingDispatch::into_first_failure),
            )
        }
        _ => (
            Satisfaction::Violated,
            ConstraintDiagnostics {
                messages: vec![
                    Diagnostic::error(format!("constraint {} evaluated to non-boolean value", id))
                        .with_code(DiagnosticCode::ConstraintViolated),
                ],
            },
            None,
        ),
    };
    ConstraintResult {
        id: id.clone(),
        satisfaction,
        diagnostics,
        indeterminate_reason,
    }
}

fn eval_context<'a>(
    input: &'a ConstraintInput,
    dispatch: Option<&'a FailureRecordingDispatch<'_>>,
) -> reify_expr::EvalContext<'a> {
    let ctx = reify_expr::EvalContext::new(input.values, input.functions);
    let ctx = match input.determinacy {
        Some(det) => ctx.with_determinacy(det),
        None => ctx,
    };
    match dispatch {
        Some(d) => ctx.with_compute_dispatch(d),
        None => ctx,
    }
}

/// The verdict for a constraint that evaluated to `Undef`, whose cause
/// [`classify_undef`] gave as `classified`.
///
/// A recorded dispatch failure (one whose body fallback was itself `Undef`)
/// replaces only the all-leaves-defined guess (`OperatorUndefinedForKinds`):
/// an undefined input is the more upstream cause and keeps precedence. The
/// failure is proven to have contributed an `Undef`, not to be the only cause:
/// in `(f(x) > 1 or true) and <refused operator>` the failed `f` is absorbed
/// and the refusal decides, yet the failure is still named.
///
/// The indeterminate line is an error when the final reason is unexpected
/// under INV-SF-4 ([`IndeterminateReason::is_unexpected`]) and a warning
/// otherwise. The failure's own diagnostics follow it at their
/// producer's severity, each restated as a claim about this constraint (see
/// [`forwarded_claim`]).
fn indeterminate_result(
    id: &ConstraintNodeId,
    classified: TransientReason,
    failure: Option<(String, ComputeDispatchFailure)>,
) -> (
    Satisfaction,
    ConstraintDiagnostics,
    Option<IndeterminateReason>,
) {
    let (reason, forwarded) = match (classified, failure) {
        (TransientReason::OperatorUndefinedForKinds { .. }, Some((target, failure))) => (
            TransientReason::ComputeDispatchFailed {
                target,
                kind: failure.kind,
            },
            failure.diagnostics,
        ),
        (reason, _) => (reason, Vec::new()),
    };
    let reason = IndeterminateReason::Transient(reason);
    let claim = format!("constraint {} indeterminate: {}", id, reason);
    let indeterminate = if reason.is_unexpected() {
        Diagnostic::error(claim.clone())
    } else {
        Diagnostic::warning(claim.clone())
    };
    let mut messages = vec![indeterminate.with_code(DiagnosticCode::ConstraintIndeterminate)];
    messages.extend(
        forwarded
            .into_iter()
            .map(|diagnostic| forwarded_claim(&claim, diagnostic)),
    );
    (
        Satisfaction::Indeterminate,
        ConstraintDiagnostics { messages },
        Some(reason),
    )
}

/// A trampoline diagnostic restated as part of `claim`, keeping its severity
/// and labels. It takes the claim's message prefix and its
/// `ConstraintIndeterminate` code (replacing the producer's), so every pass
/// that retracts a constraint's indeterminate claim once its verdict is
/// upgraded retracts this line with it.
fn forwarded_claim(claim: &str, mut diagnostic: Diagnostic) -> Diagnostic {
    diagnostic.message = format!("{claim}: {}", diagnostic.message);
    diagnostic.with_code(DiagnosticCode::ConstraintIndeterminate)
}

/// Forwards every call to `inner` unchanged and keeps the first failure whose
/// body fallback left `Undef` behind, with its target, so the reason (which
/// names one target) and the forwarded diagnostics describe the same event. A
/// failure the body covered decided nothing and is not kept. Failures nest
/// (a failed call's body can call another), so "first" is in order of
/// fallback completion: the innermost. `OnceLock` because the trait is
/// `Send + Sync`.
struct FailureRecordingDispatch<'a> {
    inner: &'a dyn ComputeDispatch,
    first_failure: OnceLock<(String, ComputeDispatchFailure)>,
}

impl<'a> FailureRecordingDispatch<'a> {
    fn new(inner: &'a dyn ComputeDispatch) -> Self {
        Self {
            inner,
            first_failure: OnceLock::new(),
        }
    }

    fn into_first_failure(self) -> Option<(String, ComputeDispatchFailure)> {
        self.first_failure.into_inner()
    }
}

impl ComputeDispatch for FailureRecordingDispatch<'_> {
    fn dispatch(&self, target: &str, args: &[Value]) -> ComputeDispatchOutcome {
        self.inner.dispatch(target, args)
    }

    fn fell_back_after_failure(
        &self,
        target: &str,
        failure: ComputeDispatchFailure,
        fallback: &Value,
    ) {
        if fallback.is_undef() {
            self.first_failure
                .get_or_init(|| (target.to_owned(), failure.clone()));
        }
        self.inner
            .fell_back_after_failure(target, failure, fallback);
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use reify_core::{ConstraintNodeId, DiagnosticCode, DimensionVector, Severity, Type, ValueCellId};
    use reify_ir::{
        BinOp, CompiledExpr, ComputeDispatchFailure, ComputeDispatchOutcome, ComputeFailureKind,
        IndeterminateReason, RefusedOperation, TransientReason, UnOp, Value, ValueMap,
    };

    fn mm(v: f64) -> Value {
        Value::Scalar {
            si_value: v * 0.001,
            dimension: DimensionVector::LENGTH,
        }
    }

    fn vcid(entity: &str, member: &str) -> ValueCellId {
        ValueCellId::new(entity, member)
    }

    fn cnid(entity: &str, index: u32) -> ConstraintNodeId {
        ConstraintNodeId::new(entity, index)
    }

    fn thickness_gt_2mm() -> CompiledExpr {
        // thickness > 2mm
        let thickness = CompiledExpr::value_ref(vcid("Bracket", "thickness"), Type::length());
        let two_mm = CompiledExpr::literal(mm(2.0), Type::length());
        CompiledExpr::binop(BinOp::Gt, thickness, two_mm, Type::Bool)
    }

    /// Pins the public `has_undefined_leaf` contract directly, independent
    /// of the message-formatting tests that only exercise it transitively
    /// through `classify_undef`. In particular this covers the degenerate
    /// edge — an expression with no ValueRef leaves is vacuously
    /// all-defined — which `classify_undef` routes into its `(false, [])`
    /// arm.
    #[test]
    fn has_undefined_leaf_direct_contract() {
        let literal_only = CompiledExpr::literal(Value::Int(42), Type::Int);
        assert!(!has_undefined_leaf(&literal_only, &ValueMap::new()));

        assert!(has_undefined_leaf(&thickness_gt_2mm(), &ValueMap::new()));
    }

    #[test]
    fn satisfied_constraint() {
        let checker = SimpleConstraintChecker;
        let expr = thickness_gt_2mm();
        let mut values = ValueMap::new();
        values.insert(vcid("Bracket", "thickness"), mm(5.0));

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Satisfied);
    }

    #[test]
    fn violated_constraint() {
        let checker = SimpleConstraintChecker;
        let expr = thickness_gt_2mm();
        let mut values = ValueMap::new();
        values.insert(vcid("Bracket", "thickness"), mm(1.0));

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Violated);
    }

    #[test]
    fn indeterminate_constraint() {
        let checker = SimpleConstraintChecker;
        let expr = thickness_gt_2mm();
        let values = ValueMap::new(); // thickness is Undef

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Indeterminate);
    }

    #[test]
    fn compound_constraint() {
        // thickness < width / 4
        let checker = SimpleConstraintChecker;
        let thickness = CompiledExpr::value_ref(vcid("Bracket", "thickness"), Type::length());
        let width = CompiledExpr::value_ref(vcid("Bracket", "width"), Type::length());
        let four = CompiledExpr::literal(Value::Int(4), Type::Int);
        let width_div_4 = CompiledExpr::binop(BinOp::Div, width, four, Type::length());
        let expr = CompiledExpr::binop(BinOp::Lt, thickness, width_div_4, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(vcid("Bracket", "thickness"), mm(5.0));
        values.insert(vcid("Bracket", "width"), mm(80.0));

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Satisfied);
    }

    #[test]
    fn batch_independent_constraints() {
        let checker = SimpleConstraintChecker;

        // thickness > 2mm (satisfied)
        let expr1 = thickness_gt_2mm();

        // width > 100mm (violated: width = 80mm)
        let width = CompiledExpr::value_ref(vcid("Bracket", "width"), Type::length());
        let hundred_mm = CompiledExpr::literal(mm(100.0), Type::length());
        let expr2 = CompiledExpr::binop(BinOp::Gt, width, hundred_mm, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(vcid("Bracket", "thickness"), mm(5.0));
        values.insert(vcid("Bracket", "width"), mm(80.0));

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![
                (cnid("Bracket", 0), &expr1),
                (cnid("Bracket", 1), &expr2),
            ]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].satisfaction, Satisfaction::Satisfied);
        assert_eq!(results[1].satisfaction, Satisfaction::Violated);
    }

    #[test]
    fn division_by_zero_no_panic() {
        let checker = SimpleConstraintChecker;

        // x > (y / 0)
        let x = CompiledExpr::value_ref(vcid("Bracket", "x"), Type::length());
        let y = CompiledExpr::value_ref(vcid("Bracket", "y"), Type::length());
        let zero = CompiledExpr::literal(Value::Int(0), Type::Int);
        let div = CompiledExpr::binop(BinOp::Div, y, zero, Type::length());
        let expr = CompiledExpr::binop(BinOp::Gt, x, div, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(vcid("Bracket", "x"), mm(5.0));
        values.insert(vcid("Bracket", "y"), mm(10.0));

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        // Should not panic
        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        // Division by zero → Undef → Indeterminate
        assert_eq!(results[0].satisfaction, Satisfaction::Indeterminate);
        assert_eq!(
            results[0].indeterminate_reason,
            refused(RefusedOperation::Binary(BinOp::Div), &["Scalar<m>", "Int"])
        );
    }

    #[test]
    fn indeterminate_constraint_carries_constraint_indeterminate_code() {
        let checker = SimpleConstraintChecker;
        let expr = thickness_gt_2mm();
        let values = ValueMap::new(); // thickness is Undef

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Indeterminate);
        assert_eq!(
            results[0].diagnostics.messages[0].severity,
            Severity::Warning,
        );
        assert_eq!(
            results[0].diagnostics.messages[0].code,
            Some(DiagnosticCode::ConstraintIndeterminate),
        );
    }

    #[test]
    fn indeterminate_names_undefined_input_cells() {
        // thickness_gt_2mm() depends on "Bracket.thickness".
        // With an empty ValueMap, that cell is Undef (undefined input).
        // The diagnostic message must name the cell: "Bracket.thickness".
        let checker = SimpleConstraintChecker;
        let expr = thickness_gt_2mm();
        let values = ValueMap::new(); // thickness is Undef

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Indeterminate);
        let msg = &results[0].diagnostics.messages[0].message;
        assert!(
            msg.contains("undefined inputs"),
            "expected 'undefined inputs' in message: {msg}"
        );
        assert!(
            msg.contains("Bracket.thickness"),
            "expected 'Bracket.thickness' in message: {msg}"
        );
    }

    #[test]
    fn operator_undefined_tensor_operand() {
        // A Tensor value compared to a scalar — eval_cmp falls through to
        // as_f64 which returns None for Tensor → Undef. All leaves are defined,
        // so the message must say "operator undefined" naming "Tensor", not
        // "undefined inputs".
        let checker = SimpleConstraintChecker;
        let tensor_cell = vcid("Obj", "tensor_field");
        let tensor_ref = CompiledExpr::value_ref(tensor_cell.clone(), Type::dimensionless_scalar());
        let one_mm = CompiledExpr::literal(mm(1.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Gt, tensor_ref, one_mm, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(tensor_cell, Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)]));

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Obj", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Indeterminate);
        let msg = &results[0].diagnostics.messages[0].message;
        assert!(
            msg.contains("operator undefined"),
            "expected 'operator undefined' in message: {msg}"
        );
        assert!(
            msg.contains("Tensor"),
            "expected 'Tensor' in message: {msg}"
        );
        assert!(
            !msg.contains("undefined inputs"),
            "expected NO 'undefined inputs' in message: {msg}"
        );
        assert_eq!(
            results[0].diagnostics.messages[0].severity,
            Severity::Warning,
            "a Tensor operand is not plain data, so the Undef stays tolerated"
        );
    }

    #[test]
    fn dimension_mismatched_comparison_is_an_unexpected_refusal() {
        // Length scalar vs mass scalar — eval_cmp returns Undef on dimension
        // mismatch. Both operands are defined plain data, so `>` itself refused.
        let checker = SimpleConstraintChecker;
        let len_cell = vcid("Obj", "len_val");
        let mass_cell = vcid("Obj", "mass_val");
        let len_ref = CompiledExpr::value_ref(len_cell.clone(), Type::length());
        let mass_ref = CompiledExpr::value_ref(
            mass_cell.clone(),
            Type::Scalar { dimension: DimensionVector::MASS },
        );
        let expr = CompiledExpr::binop(BinOp::Gt, len_ref, mass_ref, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(len_cell, mm(1.0));
        values.insert(mass_cell, Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS });

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Obj", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Indeterminate);
        // `DimensionVector::MASS` Displays as its base-unit symbol, "kg".
        assert_eq!(
            results[0].indeterminate_reason,
            refused(
                RefusedOperation::Binary(BinOp::Gt),
                &["Scalar<m>", "Scalar<kg>"]
            )
        );
        assert_eq!(results[0].diagnostics.messages[0].severity, Severity::Error);
    }

    /// The two enriched operand-kind labels — `Scalar<{dimension}>` and
    /// `Enum<{type_name}>` — are all that is left of `value_kind_label` now
    /// that every other arm delegates to `Value::kind_name` (task #6466).
    ///
    /// They are therefore one deletion away from being absorbed by the
    /// `other =>` catch-all, which would silently downgrade every operand
    /// label in an indeterminacy diagnostic from `Scalar<m>` to a bare
    /// `Scalar` and from `Enum<Fit>` to a bare `Enum`. This test asserts the
    /// payload is present, so the catch-all cannot swallow the enrichment
    /// unnoticed.
    #[test]
    fn operand_kind_labels_carry_their_enriched_payloads() {
        // A length scalar compared against an enum: both leaves are defined,
        // but `as_f64` returns None for an Enum, so eval_cmp yields Undef and
        // the diagnostic names the refusing `>` and its operands.
        let checker = SimpleConstraintChecker;
        let len_cell = vcid("Obj", "len_val");
        let fit_cell = vcid("Obj", "fit_val");
        let len_ref = CompiledExpr::value_ref(len_cell.clone(), Type::length());
        let fit_ref = CompiledExpr::value_ref(fit_cell.clone(), Type::Enum("Fit".to_string()));
        let expr = CompiledExpr::binop(BinOp::Gt, len_ref, fit_ref, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(len_cell, mm(1.0));
        values.insert(fit_cell, Value::enum_unit("Fit", "Loose"));

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Obj", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Indeterminate);
        let msg = &results[0].diagnostics.messages[0].message;
        assert!(
            msg.contains("`>` is undefined for these operands"),
            "expected the refusing `>` in message: {msg}"
        );
        // `DimensionVector::LENGTH` Displays as its base-unit symbol, "m".
        assert!(
            msg.contains("Scalar<m>"),
            "expected the dimension-enriched 'Scalar<m>' label, not a bare \
             'Scalar', in message: {msg}"
        );
        assert!(
            msg.contains("Enum<Fit>"),
            "expected the type-name-enriched 'Enum<Fit>' label, not a bare \
             'Enum', in message: {msg}"
        );
    }

    #[test]
    fn violated_constraint_carries_constraint_violated_code() {
        let checker = SimpleConstraintChecker;
        let expr = thickness_gt_2mm();
        let mut values = ValueMap::new();
        values.insert(vcid("Bracket", "thickness"), mm(1.0));

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Violated);
        assert_eq!(results[0].diagnostics.messages[0].severity, Severity::Error,);
        assert_eq!(
            results[0].diagnostics.messages[0].code,
            Some(DiagnosticCode::ConstraintViolated),
        );
    }

    #[test]
    fn non_bool_constraint_carries_constraint_violated_code() {
        let checker = SimpleConstraintChecker;
        // CompiledExpr evaluating to Int(42) — triggers the non-boolean fallback
        let expr = CompiledExpr::literal(Value::Int(42), Type::Int);
        let values = ValueMap::new();

        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
            values: &values,
            functions: &[],
            determinacy: None,
        };

        let results = checker.check(&input);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].satisfaction, Satisfaction::Violated);
        assert_eq!(results[0].diagnostics.messages[0].severity, Severity::Error,);
        assert_eq!(
            results[0].diagnostics.messages[0].code,
            Some(DiagnosticCode::ConstraintViolated),
        );
    }

    fn check_single(id: ConstraintNodeId, expr: &CompiledExpr, values: &ValueMap) -> ConstraintResult {
        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(id, expr)]),
            values,
            functions: &[],
            determinacy: None,
        };
        let mut results = SimpleConstraintChecker.check(&input);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    fn transient(reason: TransientReason) -> Option<IndeterminateReason> {
        Some(IndeterminateReason::Transient(reason))
    }

    fn refused(operation: RefusedOperation, operands: &[&str]) -> Option<IndeterminateReason> {
        transient(TransientReason::OperationRefused {
            operation,
            operands: operands.iter().map(|label| label.to_string()).collect(),
        })
    }

    fn real(v: f64) -> CompiledExpr {
        CompiledExpr::literal(Value::Real(v), Type::dimensionless_scalar())
    }

    /// `1.0 / <cell> > 0.0`.
    fn reciprocal_gt_zero(cell: ValueCellId) -> CompiledExpr {
        let cell = CompiledExpr::value_ref(cell, Type::dimensionless_scalar());
        let reciprocal =
            CompiledExpr::binop(BinOp::Div, real(1.0), cell, Type::dimensionless_scalar());
        CompiledExpr::binop(BinOp::Gt, reciprocal, real(0.0), Type::Bool)
    }

    /// `<name>(<args>) > 0.0`, with the builtin call typed `result_type`; the
    /// evaluator dispatches a builtin by name, so its value ignores `result_type`.
    fn builtin_gt_zero(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
        let call = CompiledExpr {
            content_hash: reify_core::ContentHash::of(format!("{name}_{result_type:?}").as_bytes()),
            result_type,
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: name.to_string(),
                    qualified_name: format!("std::{name}"),
                },
                args,
            },
        };
        CompiledExpr::binop(BinOp::Gt, call, real(0.0), Type::Bool)
    }

    /// `sqrt(-1.0) > 0.0`.
    fn sqrt_of_minus_one_gt_zero(result_type: Type) -> CompiledExpr {
        builtin_gt_zero("sqrt", vec![real(-1.0)], result_type)
    }

    #[test]
    fn division_by_zero_over_reals_is_an_unexpected_refusal() {
        let x = vcid("Root", "x");
        let mut values = ValueMap::new();
        values.insert(x.clone(), Value::Real(0.0));

        let result = check_single(cnid("Root", 0), &reciprocal_gt_zero(x), &values);

        assert_eq!(result.satisfaction, Satisfaction::Indeterminate);
        assert_eq!(
            result.indeterminate_reason,
            refused(RefusedOperation::Binary(BinOp::Div), &["Real", "Real"])
        );
        let [diagnostic] = result.diagnostics.messages.as_slice() else {
            panic!("expected one diagnostic; got {:?}", result.diagnostics);
        };
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(
            diagnostic.code,
            Some(DiagnosticCode::ConstraintIndeterminate)
        );
        assert_eq!(
            diagnostic.message,
            "constraint Root#constraint[0] indeterminate: `/` is undefined for these \
             operands: Real, Real"
        );
    }

    #[test]
    fn refusal_is_found_beneath_a_kleene_operator() {
        let x = vcid("Root", "x");
        let y = vcid("Root", "y");
        let mut values = ValueMap::new();
        values.insert(x.clone(), Value::Real(1.0));
        values.insert(y.clone(), Value::Real(0.0));
        let x_positive = CompiledExpr::binop(
            BinOp::Gt,
            CompiledExpr::value_ref(x, Type::dimensionless_scalar()),
            real(0.0),
            Type::Bool,
        );
        let expr = CompiledExpr::binop(BinOp::And, x_positive, reciprocal_gt_zero(y), Type::Bool);

        let result = check_single(cnid("Root", 0), &expr, &values);

        assert_eq!(result.satisfaction, Satisfaction::Indeterminate);
        assert_eq!(
            result.indeterminate_reason,
            refused(RefusedOperation::Binary(BinOp::Div), &["Real", "Real"])
        );
        assert_eq!(result.diagnostics.messages[0].severity, Severity::Error);
    }

    #[test]
    fn a_builtin_refusal_needs_a_plain_data_result_type() {
        let plain = check_single(
            cnid("Root", 0),
            &sqrt_of_minus_one_gt_zero(Type::dimensionless_scalar()),
            &ValueMap::new(),
        );
        assert_eq!(plain.satisfaction, Satisfaction::Indeterminate);
        assert_eq!(
            plain.indeterminate_reason,
            refused(RefusedOperation::Builtin("sqrt".to_string()), &["Real"])
        );
        assert_eq!(plain.diagnostics.messages[0].severity, Severity::Error);

        let non_plain = check_single(
            cnid("Root", 0),
            &sqrt_of_minus_one_gt_zero(Type::List(Box::new(Type::dimensionless_scalar()))),
            &ValueMap::new(),
        );
        assert_eq!(non_plain.satisfaction, Satisfaction::Indeterminate);
        assert_eq!(
            non_plain.indeterminate_reason,
            transient(TransientReason::OperatorUndefinedForKinds { kinds: vec![] })
        );
        assert_eq!(
            non_plain.diagnostics.messages[0].severity,
            Severity::Warning
        );
    }

    /// No stdlib family claims `box`: its Undef means the evaluator cannot
    /// realize the call, not that it refused, whatever the static type says.
    #[test]
    fn an_unclaimed_builtin_is_not_a_refusal() {
        let side = || CompiledExpr::literal(mm(10.0), Type::length());
        let result = check_single(
            cnid("Root", 0),
            &builtin_gt_zero(
                "box",
                vec![side(), side(), side()],
                Type::dimensionless_scalar(),
            ),
            &ValueMap::new(),
        );
        assert_eq!(result.satisfaction, Satisfaction::Indeterminate);
        assert_eq!(
            result.indeterminate_reason,
            transient(TransientReason::OperatorUndefinedForKinds { kinds: vec![] })
        );
        assert_eq!(result.diagnostics.messages[0].severity, Severity::Warning);
    }

    #[test]
    fn a_unary_operator_over_plain_data_is_an_unexpected_refusal() {
        let s = vcid("Root", "s");
        let n = vcid("Root", "n");
        let mut values = ValueMap::new();
        values.insert(s.clone(), Value::String("abc".to_string()));
        values.insert(n.clone(), Value::Int(3));
        let negated_string_gt_zero = CompiledExpr::binop(
            BinOp::Gt,
            CompiledExpr::unop(
                UnOp::Neg,
                CompiledExpr::value_ref(s, Type::String),
                Type::dimensionless_scalar(),
            ),
            real(0.0),
            Type::Bool,
        );
        let not_an_int =
            CompiledExpr::unop(UnOp::Not, CompiledExpr::value_ref(n, Type::Int), Type::Bool);

        for (expr, operation, operand) in [
            (&negated_string_gt_zero, UnOp::Neg, "String"),
            (&not_an_int, UnOp::Not, "Int"),
        ] {
            let result = check_single(cnid("Root", 0), expr, &values);

            assert_eq!(result.satisfaction, Satisfaction::Indeterminate);
            assert_eq!(
                result.indeterminate_reason,
                refused(RefusedOperation::Unary(operation), &[operand])
            );
            assert_eq!(result.diagnostics.messages[0].severity, Severity::Error);
        }
    }

    /// `(if c then 1.0 / x else 0.0) > 0.0` at `x = 0.0`: the division refuses,
    /// but its Undef reaches the root through a Conditional, which is not a
    /// pure operation, so the descent stops there unproven.
    #[test]
    fn an_undef_beneath_an_ineligible_node_stays_tolerated() {
        let c = vcid("Root", "c");
        let x = vcid("Root", "x");
        let mut values = ValueMap::new();
        values.insert(c.clone(), Value::Bool(true));
        values.insert(x.clone(), Value::Real(0.0));
        let reciprocal = CompiledExpr::binop(
            BinOp::Div,
            real(1.0),
            CompiledExpr::value_ref(x, Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        );
        let conditional = CompiledExpr {
            content_hash: reify_core::ContentHash::of(b"if c then 1.0 / x else 0.0"),
            result_type: Type::dimensionless_scalar(),
            kind: CompiledExprKind::Conditional {
                condition: Box::new(CompiledExpr::value_ref(c, Type::Bool)),
                then_branch: Box::new(reciprocal),
                else_branch: Box::new(real(0.0)),
            },
        };
        let expr = CompiledExpr::binop(BinOp::Gt, conditional, real(0.0), Type::Bool);

        let result = check_single(cnid("Root", 0), &expr, &values);

        assert_eq!(result.satisfaction, Satisfaction::Indeterminate);
        assert_eq!(
            result.indeterminate_reason,
            transient(TransientReason::OperatorUndefinedForKinds {
                kinds: vec!["Bool".to_string(), "Real".to_string()],
            })
        );
        assert_eq!(result.diagnostics.messages[0].severity, Severity::Warning);
    }

    #[test]
    fn undefined_inputs_reason_is_deduped_and_ordered_by_name() {
        // (width + width) > thickness: width leads the leaf order twice, so
        // the recorded cells only come out name-ordered if the checker sorts.
        let width = || CompiledExpr::value_ref(vcid("Bracket", "width"), Type::length());
        let thickness = CompiledExpr::value_ref(vcid("Bracket", "thickness"), Type::length());
        let doubled = CompiledExpr::binop(BinOp::Add, width(), width(), Type::length());
        let expr = CompiledExpr::binop(BinOp::Gt, doubled, thickness, Type::Bool);

        let result = check_single(cnid("Bracket", 0), &expr, &ValueMap::new());

        assert_eq!(result.satisfaction, Satisfaction::Indeterminate);
        assert_eq!(
            result.indeterminate_reason,
            transient(TransientReason::UndefInputs {
                cells: vec![vcid("Bracket", "thickness"), vcid("Bracket", "width")],
            })
        );
        assert_eq!(
            result.diagnostics.messages[0].message,
            "constraint Bracket#constraint[0] indeterminate: undefined inputs: \
             Bracket.thickness, Bracket.width"
        );
    }

    #[test]
    fn operation_refused_reason_records_its_operands_in_position_order() {
        let len_cell = vcid("Obj", "len_val");
        let fit_cell = vcid("Obj", "fit_val");
        let len_ref = CompiledExpr::value_ref(len_cell.clone(), Type::length());
        let fit_ref = CompiledExpr::value_ref(fit_cell.clone(), Type::Enum("Fit".to_string()));
        let expr = CompiledExpr::binop(BinOp::Gt, len_ref, fit_ref, Type::Bool);
        let mut values = ValueMap::new();
        values.insert(len_cell, mm(1.0));
        values.insert(fit_cell, Value::enum_unit("Fit", "Loose"));

        let result = check_single(cnid("Obj", 0), &expr, &values);

        assert_eq!(
            result.indeterminate_reason,
            refused(
                RefusedOperation::Binary(BinOp::Gt),
                &["Scalar<m>", "Enum<Fit>"]
            )
        );
        assert_eq!(
            result.diagnostics.messages[0].message,
            "constraint Obj#constraint[0] indeterminate: `>` is undefined for these \
             operands: Scalar<m>, Enum<Fit>"
        );
    }

    #[test]
    fn operator_undefined_reason_without_cell_operands_records_no_kinds() {
        let tensor = CompiledExpr::literal(
            Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)]),
            Type::dimensionless_scalar(),
        );
        let one_mm = CompiledExpr::literal(mm(1.0), Type::length());
        let expr = CompiledExpr::binop(BinOp::Gt, tensor, one_mm, Type::Bool);

        let result = check_single(cnid("Obj", 0), &expr, &ValueMap::new());

        assert_eq!(result.satisfaction, Satisfaction::Indeterminate);
        assert_eq!(
            result.indeterminate_reason,
            transient(TransientReason::OperatorUndefinedForKinds { kinds: vec![] })
        );
        assert_eq!(
            result.diagnostics.messages[0].message,
            "constraint Obj#constraint[0] indeterminate: operator undefined for these \
             operand kinds"
        );
        assert_eq!(result.diagnostics.messages[0].severity, Severity::Warning);
    }

    #[test]
    fn definite_verdicts_record_no_indeterminate_reason() {
        let mut thick = ValueMap::new();
        thick.insert(vcid("Bracket", "thickness"), mm(5.0));
        let mut thin = ValueMap::new();
        thin.insert(vcid("Bracket", "thickness"), mm(1.0));
        let non_bool = CompiledExpr::literal(Value::Int(42), Type::Int);

        for (expr, values, expected) in [
            (thickness_gt_2mm(), thick, Satisfaction::Satisfied),
            (thickness_gt_2mm(), thin, Satisfaction::Violated),
            (non_bool, ValueMap::new(), Satisfaction::Violated),
        ] {
            let result = check_single(cnid("Bracket", 0), &expr, &values);
            assert_eq!(result.satisfaction, expected);
            assert_eq!(result.indeterminate_reason, None);
        }
    }

    /// An `@optimized` function whose body is a bare `Undef` literal, so
    /// body-eval (no dispatch) is Undef and only a dispatched result can decide
    /// a constraint that calls it. Mirrors reify-expr's #4880 stub fixture.
    fn optimized_stress_fn() -> reify_ir::CompiledFunction {
        optimized_stress_fn_with_body(Value::Undef)
    }

    /// `optimized_stress_fn` with a body that evaluates to `body`.
    fn optimized_stress_fn_with_body(body: Value) -> reify_ir::CompiledFunction {
        let params = vec![("load".to_string(), Type::dimensionless_scalar())];
        reify_ir::CompiledFunction {
            name: "stress".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: reify_ir::CompiledFunction::no_defaults_for(&params),
            params,
            return_type: Type::dimensionless_scalar(),
            body: reify_ir::CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::literal(body.clone(), Type::dimensionless_scalar()),
            },
            content_hash: reify_core::ContentHash::of(
                format!("checker_optimized_stress_stub_{body:?}").as_bytes(),
            ),
            annotations: vec![],
            optimized_target: Some("test::stress".to_string()),
            type_params: vec![],
        }
    }

    /// `stress(5.0)`; `tag` keeps each call site's content hash distinct.
    fn stress_call(tag: &str) -> CompiledExpr {
        CompiledExpr {
            content_hash: reify_core::ContentHash::of(format!("stress_call_{tag}").as_bytes()),
            result_type: Type::dimensionless_scalar(),
            kind: reify_ir::CompiledExprKind::UserFunctionCall {
                function_name: "stress".to_string(),
                args: vec![CompiledExpr::literal(
                    Value::Real(5.0),
                    Type::dimensionless_scalar(),
                )],
            },
        }
    }

    /// `stress(5.0) > bound`.
    fn stress_gt(bound: f64) -> CompiledExpr {
        let call = stress_call(&format!("gt_{bound}"));
        let bound = CompiledExpr::literal(Value::Real(bound), Type::dimensionless_scalar());
        CompiledExpr::binop(BinOp::Gt, call, bound, Type::Bool)
    }

    /// Resolves `"test::stress"` to a dimensionless 42; every other target is
    /// `Unregistered`.
    struct StressDispatch;

    impl reify_ir::ComputeDispatch for StressDispatch {
        fn dispatch(&self, target: &str, _args: &[Value]) -> ComputeDispatchOutcome {
            if target != "test::stress" {
                return ComputeDispatchOutcome::Unregistered;
            }
            ComputeDispatchOutcome::Resolved(Value::Scalar {
                si_value: 42.0,
                dimension: DimensionVector::DIMENSIONLESS,
            })
        }
    }

    #[test]
    fn check_with_dispatch_resolves_optimized_calls_inside_constraints() {
        let functions = [optimized_stress_fn()];
        let values = ValueMap::new();
        let above_ten = stress_gt(10.0);
        let above_hundred = stress_gt(100.0);
        let input = ConstraintInput {
            constraints: Cow::Owned(vec![
                (cnid("Probe", 0), &above_ten),
                (cnid("Probe", 1), &above_hundred),
            ]),
            values: &values,
            functions: &functions,
            determinacy: None,
        };

        let unhooked = SimpleConstraintChecker.check(&input);
        assert!(
            unhooked
                .iter()
                .all(|r| r.satisfaction == Satisfaction::Indeterminate),
            "without dispatch the @optimized body evaluates to Undef: {unhooked:?}"
        );

        let dispatched = SimpleConstraintChecker.check_with_dispatch(&input, Some(&StressDispatch));
        assert_eq!(
            dispatched[0].satisfaction,
            Satisfaction::Satisfied,
            "42 > 10: {:?}",
            dispatched[0]
        );
        assert_eq!(dispatched[0].indeterminate_reason, None);
        assert_eq!(
            dispatched[1].satisfaction,
            Satisfaction::Violated,
            "42 > 100 is false: {:?}",
            dispatched[1]
        );
    }

    /// Declines every target and counts how often it was asked.
    struct CountingDeclineDispatch(std::sync::atomic::AtomicUsize);

    impl reify_ir::ComputeDispatch for CountingDeclineDispatch {
        fn dispatch(&self, _target: &str, _args: &[Value]) -> ComputeDispatchOutcome {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ComputeDispatchOutcome::Unregistered
        }
    }

    #[test]
    fn the_refusal_descent_never_re_dispatches_an_optimized_call() {
        let functions = [optimized_stress_fn()];
        let y = vcid("Probe", "y");
        let mut values = ValueMap::new();
        values.insert(y.clone(), Value::Real(0.0));
        let bare = stress_gt(10.0);
        let beside_a_refusal = CompiledExpr::binop(
            BinOp::Or,
            stress_gt(10.0),
            reciprocal_gt_zero(y),
            Type::Bool,
        );

        for expr in [&bare, &beside_a_refusal] {
            let dispatch = CountingDeclineDispatch(std::sync::atomic::AtomicUsize::new(0));
            let input = ConstraintInput {
                constraints: Cow::Owned(vec![(cnid("Probe", 0), expr)]),
                values: &values,
                functions: &functions,
                determinacy: None,
            };

            let results = SimpleConstraintChecker.check_with_dispatch(&input, Some(&dispatch));

            assert_eq!(
                dispatch.0.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "the verdict dispatches once; classifying its Undef must not dispatch again"
            );
            let [result] = results.as_slice() else {
                panic!("expected one result; got {results:?}");
            };
            assert_eq!(result.satisfaction, Satisfaction::Indeterminate);
            let reason = result
                .indeterminate_reason
                .as_ref()
                .expect("an Indeterminate verdict records a reason");
            assert!(
                !reason.is_unexpected(),
                "a user call is never a proven refusal: {reason}"
            );
            assert_eq!(result.diagnostics.messages[0].severity, Severity::Warning);
        }
    }

    /// For `"test::stress"`, a registered implementation that ran and produced
    /// no value; every other target is `Unregistered`.
    struct FailingDispatch {
        kind: ComputeFailureKind,
    }

    impl reify_ir::ComputeDispatch for FailingDispatch {
        fn dispatch(&self, target: &str, _args: &[Value]) -> ComputeDispatchOutcome {
            if target != "test::stress" {
                return ComputeDispatchOutcome::Unregistered;
            }
            let diagnostics = match self.kind {
                ComputeFailureKind::Failed => {
                    vec![Diagnostic::error("stiffness matrix is singular")]
                }
                ComputeFailureKind::Cancelled => vec![],
            };
            ComputeDispatchOutcome::Failed(ComputeDispatchFailure {
                kind: self.kind,
                diagnostics,
            })
        }
    }

    fn check_one_with(
        expr: &CompiledExpr,
        values: &ValueMap,
        functions: &[reify_ir::CompiledFunction],
        dispatch: &dyn reify_ir::ComputeDispatch,
    ) -> ConstraintResult {
        let input = ConstraintInput {
            constraints: Cow::Owned(vec![(cnid("Probe", 0), expr)]),
            values,
            functions,
            determinacy: None,
        };
        let mut results = SimpleConstraintChecker.check_with_dispatch(&input, Some(dispatch));
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    fn compute_failed(kind: ComputeFailureKind) -> Option<IndeterminateReason> {
        Some(IndeterminateReason::Transient(
            TransientReason::ComputeDispatchFailed {
                target: "test::stress".into(),
                kind,
            },
        ))
    }

    fn error_messages(result: &ConstraintResult) -> Vec<&str> {
        result
            .diagnostics
            .messages
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.as_str())
            .collect()
    }

    #[test]
    fn a_failed_dispatch_is_attributed_not_reported_as_an_operator_refusal() {
        let result = check_one_with(
            &stress_gt(10.0),
            &ValueMap::new(),
            &[optimized_stress_fn()],
            &FailingDispatch {
                kind: ComputeFailureKind::Failed,
            },
        );

        assert_eq!(
            result.satisfaction,
            Satisfaction::Indeterminate,
            "{result:?}"
        );
        assert_eq!(
            result.indeterminate_reason,
            compute_failed(ComputeFailureKind::Failed)
        );
        let first = &result.diagnostics.messages[0];
        assert_eq!(first.severity, Severity::Warning, "{result:?}");
        assert_eq!(first.code, Some(DiagnosticCode::ConstraintIndeterminate));
        assert!(
            first.message.contains("test::stress") && !first.message.contains("operator undefined"),
            "the indeterminate line must name the failed target, not guess an operator \
             refusal: {:?}",
            first.message
        );
        assert_eq!(
            error_messages(&result),
            vec![
                "constraint Probe#constraint[0] indeterminate: compute dispatch failed for \
                 @optimized target \"test::stress\": stiffness matrix is singular"
            ],
            "the trampoline's own diagnostic is carried at its producer severity, as a \
             claim about the constraint"
        );
        assert_eq!(
            result.diagnostics.messages[1].code,
            Some(DiagnosticCode::ConstraintIndeterminate),
            "the forwarded line is retracted with the claim it belongs to: {result:?}"
        );
    }

    #[test]
    fn a_cancelled_dispatch_is_attributed_as_cancelled() {
        let result = check_one_with(
            &stress_gt(10.0),
            &ValueMap::new(),
            &[optimized_stress_fn()],
            &FailingDispatch {
                kind: ComputeFailureKind::Cancelled,
            },
        );

        assert_eq!(
            result.satisfaction,
            Satisfaction::Indeterminate,
            "{result:?}"
        );
        assert_eq!(
            result.indeterminate_reason,
            compute_failed(ComputeFailureKind::Cancelled)
        );
        assert_eq!(
            result.diagnostics.messages.len(),
            1,
            "only the indeterminate warning: {result:?}"
        );
        assert_eq!(
            result.diagnostics.messages[0].code,
            Some(DiagnosticCode::ConstraintIndeterminate)
        );
    }

    #[test]
    fn an_undefined_input_keeps_precedence_over_a_dispatch_failure() {
        let bound = CompiledExpr::value_ref(vcid("Probe", "bound"), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Gt, stress_call("vs_bound"), bound, Type::Bool);

        let result = check_one_with(
            &expr,
            &ValueMap::new(),
            &[optimized_stress_fn()],
            &FailingDispatch {
                kind: ComputeFailureKind::Failed,
            },
        );

        assert_eq!(
            result.satisfaction,
            Satisfaction::Indeterminate,
            "{result:?}"
        );
        assert_eq!(
            result.indeterminate_reason,
            Some(IndeterminateReason::Transient(
                TransientReason::UndefInputs {
                    cells: vec![vcid("Probe", "bound")],
                }
            ))
        );
        assert!(
            error_messages(&result).is_empty(),
            "an undefined input is the upstream cause; nothing is forwarded: {result:?}"
        );
    }

    #[test]
    fn a_dispatch_failure_does_not_touch_a_definite_verdict() {
        let result = check_one_with(
            &stress_gt(10.0),
            &ValueMap::new(),
            &[optimized_stress_fn_with_body(Value::Real(42.0))],
            &FailingDispatch {
                kind: ComputeFailureKind::Failed,
            },
        );

        assert_eq!(result.satisfaction, Satisfaction::Satisfied, "{result:?}");
        assert_eq!(result.indeterminate_reason, None);
        assert!(result.diagnostics.messages.is_empty(), "{result:?}");
    }

    /// `tensor > 1mm`: refused for every operand kind, with no leaf to blame.
    fn refused_operator() -> CompiledExpr {
        let tensor = CompiledExpr::literal(
            Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)]),
            Type::dimensionless_scalar(),
        );
        let one_mm = CompiledExpr::literal(mm(1.0), Type::length());
        CompiledExpr::binop(BinOp::Gt, tensor, one_mm, Type::Bool)
    }

    #[test]
    fn a_failure_the_body_covered_is_not_blamed_for_another_operators_refusal() {
        // 42 > 100 is false, so the refusal alone leaves the disjunction Undef.
        let expr = CompiledExpr::binop(BinOp::Or, stress_gt(100.0), refused_operator(), Type::Bool);

        let result = check_one_with(
            &expr,
            &ValueMap::new(),
            &[optimized_stress_fn_with_body(Value::Real(42.0))],
            &FailingDispatch {
                kind: ComputeFailureKind::Failed,
            },
        );

        assert_eq!(
            result.satisfaction,
            Satisfaction::Indeterminate,
            "{result:?}"
        );
        assert_eq!(
            result.indeterminate_reason,
            Some(IndeterminateReason::Transient(
                TransientReason::OperatorUndefinedForKinds { kinds: vec![] }
            ))
        );
        assert!(
            error_messages(&result).is_empty(),
            "the covered failure decided nothing; nothing is forwarded: {result:?}"
        );
    }

    /// `strain`: `stress` under another target, so two failures can be told apart.
    fn optimized_strain_fn() -> reify_ir::CompiledFunction {
        reify_ir::CompiledFunction {
            name: "strain".to_string(),
            content_hash: reify_core::ContentHash::of(b"checker_optimized_strain_stub"),
            optimized_target: Some("test::strain".to_string()),
            ..optimized_stress_fn()
        }
    }

    fn strain_gt(bound: f64) -> CompiledExpr {
        let call = CompiledExpr {
            content_hash: reify_core::ContentHash::of(format!("strain_call_gt_{bound}").as_bytes()),
            result_type: Type::dimensionless_scalar(),
            kind: reify_ir::CompiledExprKind::UserFunctionCall {
                function_name: "strain".to_string(),
                args: vec![CompiledExpr::literal(
                    Value::Real(5.0),
                    Type::dimensionless_scalar(),
                )],
            },
        };
        let bound = CompiledExpr::literal(Value::Real(bound), Type::dimensionless_scalar());
        CompiledExpr::binop(BinOp::Gt, call, bound, Type::Bool)
    }

    /// Fails every target with a diagnostic naming it, and records the order of
    /// the calls it saw.
    #[derive(Default)]
    struct FailEverythingDispatch {
        dispatched: std::sync::Mutex<Vec<String>>,
    }

    impl reify_ir::ComputeDispatch for FailEverythingDispatch {
        fn dispatch(&self, target: &str, _args: &[Value]) -> ComputeDispatchOutcome {
            self.dispatched.lock().unwrap().push(target.to_owned());
            ComputeDispatchOutcome::Failed(ComputeDispatchFailure {
                kind: ComputeFailureKind::Failed,
                diagnostics: vec![Diagnostic::error(format!("{target} diverged"))],
            })
        }
    }

    #[test]
    fn the_first_failure_names_the_reason_and_alone_is_forwarded() {
        let expr = CompiledExpr::binop(BinOp::And, stress_gt(10.0), strain_gt(1.0), Type::Bool);
        let dispatch = FailEverythingDispatch::default();

        let result = check_one_with(
            &expr,
            &ValueMap::new(),
            &[optimized_stress_fn(), optimized_strain_fn()],
            &dispatch,
        );

        assert_eq!(
            *dispatch.dispatched.lock().unwrap(),
            vec!["test::stress".to_string(), "test::strain".to_string()],
            "precondition: both calls failed, stress first"
        );
        assert_eq!(
            result.indeterminate_reason,
            compute_failed(ComputeFailureKind::Failed)
        );
        let errors = error_messages(&result);
        assert!(
            matches!(errors[..], [only] if only.ends_with("test::stress diverged")),
            "only the named failure's diagnostics are forwarded: {errors:?}"
        );
    }
}
