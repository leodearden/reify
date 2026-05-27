// See `reify-types::value::SampledField` for the rationale behind this allow:
// `Value::SampledField` carries an `AtomicBool` (excluded from
// `PartialEq`/`Ord`/`Hash`/`content_hash`) that nonetheless triggers
// `mutable_key_type` on every `BTreeMap<Value, _>` site.
#![allow(clippy::mutable_key_type)]

mod classifier;
mod cpsat;
mod decompose;
mod registry;
mod slvs_sys;
mod solver;
mod solvespace;

pub use classifier::ConstraintClassifier;
pub use cpsat::CpSatSolver;
pub use decompose::{SubProblem, decompose_into_components};
// Loop-closure Newton solver was relocated to reify-stdlib (task 2678) to
// resolve a would-be cycle: `reify_stdlib::snapshot` needs to invoke the
// solver on closed-chain mechanisms, but `reify-stdlib` cannot depend on
// `reify-constraints` (the latter already depends on the former for FK
// primitives).  These re-exports preserve the original
// `reify_constraints::{NewtonConfig, ...}` paths so downstream callers
// (reify-eval tests, reify-constraints integration tests) compile unchanged.
pub use registry::SolverRegistry;
pub use reify_stdlib::loop_closure_solver::{
    LoopClosureChain, LoopClosureReport, NewtonConfig, NewtonOutcome, StartStrategy,
    mechanism_loop_closure_chains, newton_solve, solve_loop_closure,
    solve_loop_closure_with_diagnostics,
};
// γ-widening re-export: callers that need to construct `vals_a` / `vals_b_initial`
// for `solve_loop_closure(...)` or `solve_loop_closure_with_diagnostics(...)`
// require `JointValue` to wrap scalar / planar / spherical / cylindrical
// per-joint storage.  Re-exported here so downstream crates (e.g. `reify-eval`
// integration tests) don't need a direct `reify-stdlib` dev-dep to use the
// loop-closure API.  See `JointValue` in `crates/reify-stdlib/src/loop_closure_value.rs`.
pub use reify_stdlib::loop_closure_value::{JointKind, JointValue};
pub use solver::DimensionalSolver;
pub use solvespace::SolveSpaceSolver;

use reify_types::{
    ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Diagnostic,
    DiagnosticCode, Satisfaction, Value,
};

/// Simple constraint checker for M1: evaluates constraint expressions
/// and checks whether they are satisfied (true), violated (false), or
/// indeterminate (undef input).
pub struct SimpleConstraintChecker;

impl ConstraintChecker for SimpleConstraintChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, expr)| {
                let ctx = reify_expr::EvalContext::new(input.values, input.functions);
                let ctx = if let Some(det) = input.determinacy {
                    ctx.with_determinacy(det)
                } else {
                    ctx
                };
                let value = reify_expr::eval_expr(expr, &ctx);
                let (satisfaction, diagnostics) = match value {
                    Value::Bool(true) => {
                        (Satisfaction::Satisfied, ConstraintDiagnostics::default())
                    }
                    Value::Bool(false) => (
                        Satisfaction::Violated,
                        ConstraintDiagnostics {
                            messages: vec![
                                Diagnostic::error(format!("constraint {} violated", id))
                                    .with_code(DiagnosticCode::ConstraintViolated),
                            ],
                        },
                    ),
                    Value::Undef => (
                        Satisfaction::Indeterminate,
                        ConstraintDiagnostics {
                            messages: vec![
                                Diagnostic::warning(format!(
                                    "constraint {} indeterminate: undefined inputs",
                                    id
                                ))
                                .with_code(DiagnosticCode::ConstraintIndeterminate),
                            ],
                        },
                    ),
                    _ => (
                        Satisfaction::Violated,
                        ConstraintDiagnostics {
                            messages: vec![
                                Diagnostic::error(format!(
                                    "constraint {} evaluated to non-boolean value",
                                    id
                                ))
                                .with_code(DiagnosticCode::ConstraintViolated),
                            ],
                        },
                    ),
                };

                ConstraintResult {
                    id: id.clone(),
                    satisfaction,
                    diagnostics,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use reify_types::{
        BinOp, CompiledExpr, ConstraintNodeId, DiagnosticCode, DimensionVector, Severity, Type,
        Value, ValueCellId, ValueMap,
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
}
