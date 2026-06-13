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

use reify_core::{Diagnostic, DiagnosticCode};
use reify_ir::{ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction, Value};

/// Classify `Value::Undef` by leaf-ValueRef definedness.
///
/// Returns:
/// - `(true, names)` — at least one leaf is `Undef`; `names` lists the undefined
///   cell names (deduped, sorted alphabetically) via `ValueCellId::Display`.
/// - `(false, kinds)` — all leaves are defined (or the expression has no ValueRefs);
///   `kinds` lists the distinct `value_kind_label` strings of the defined leaf values
///   (deduped, sorted alphabetically).
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
fn classify_undef(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> (bool, Vec<String>) {
    use std::collections::HashSet;

    let leaf_ids = expr.collect_value_refs();
    let mut undef_names: Vec<String> = Vec::new();
    let mut undef_seen: HashSet<String> = HashSet::new();
    let mut defined_kinds: Vec<String> = Vec::new();
    let mut kinds_seen: HashSet<String> = HashSet::new();

    for id in &leaf_ids {
        let v = values.get_or_undef(id);
        if v.is_undef() {
            let name = id.to_string();
            if undef_seen.insert(name.clone()) {
                undef_names.push(name);
            }
        } else {
            let kind = value_kind_label(&v);
            if kinds_seen.insert(kind.clone()) {
                defined_kinds.push(kind);
            }
        }
    }

    if !undef_names.is_empty() {
        undef_names.sort();
        (true, undef_names)
    } else {
        defined_kinds.sort();
        (false, defined_kinds)
    }
}

/// A short human-readable label for the kind of a defined `Value`.
fn value_kind_label(v: &Value) -> String {
    match v {
        Value::Bool(_) => "Bool".to_string(),
        Value::Int(_) => "Int".to_string(),
        Value::Real(_) => "Real".to_string(),
        Value::String(_) => "String".to_string(),
        Value::Scalar { dimension, .. } => format!("Scalar<{}>", dimension),
        Value::Enum { type_name, .. } => format!("Enum<{}>", type_name),
        Value::Tensor(_) => "Tensor".to_string(),
        Value::Matrix(_) => "Matrix".to_string(),
        Value::List(_) => "List".to_string(),
        Value::Set(_) => "Set".to_string(),
        Value::Map(_) => "Map".to_string(),
        Value::Option(_) => "Option".to_string(),
        Value::Point(_) => "Point".to_string(),
        Value::Vector(_) => "Vector".to_string(),
        Value::Complex { .. } => "Complex".to_string(),
        Value::Orientation { .. } => "Orientation".to_string(),
        Value::Frame { .. } => "Frame".to_string(),
        Value::Transform { .. } => "Transform".to_string(),
        Value::Plane { .. } => "Plane".to_string(),
        Value::Axis { .. } => "Axis".to_string(),
        Value::Direction { .. } => "Direction".to_string(),
        Value::BoundingBox { .. } => "BoundingBox".to_string(),
        Value::Range { .. } => "Range".to_string(),
        Value::Field { .. } => "Field".to_string(),
        Value::Lambda { .. } => "Lambda".to_string(),
        Value::SampledField(_) => "SampledField".to_string(),
        Value::StructureInstance(_) => "StructureInstance".to_string(),
        Value::GeometryHandle { .. } => "GeometryHandle".to_string(),
        Value::AffineMap { .. } => "AffineMap".to_string(),
        Value::Selector(_) => "Selector".to_string(),
        Value::Undef => "Undef".to_string(),
    }
}

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
                    Value::Undef => {
                        let (has_undef, items) = classify_undef(expr, input.values);
                        let msg = if has_undef {
                            format!(
                                "constraint {} indeterminate: undefined inputs: {}",
                                id,
                                items.join(", ")
                            )
                        } else {
                            // All leaves are defined (or the expr has no ValueRefs) but
                            // the operator produced Undef; report the distinct operand kinds.
                            if items.is_empty() {
                                format!(
                                    "constraint {} indeterminate: operator undefined for these operand kinds",
                                    id
                                )
                            } else {
                                format!(
                                    "constraint {} indeterminate: operator undefined for these operand kinds: {}",
                                    id,
                                    items.join(", ")
                                )
                            }
                        };
                        (
                            Satisfaction::Indeterminate,
                            ConstraintDiagnostics {
                                messages: vec![
                                    Diagnostic::warning(msg)
                                        .with_code(DiagnosticCode::ConstraintIndeterminate),
                                ],
                            },
                        )
                    }
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
    use reify_core::{ConstraintNodeId, DiagnosticCode, DimensionVector, Severity, Type, ValueCellId};
    use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};

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
    }

    #[test]
    fn operator_undefined_dimension_mismatch() {
        // Length scalar vs mass scalar — eval_cmp returns Undef on dimension
        // mismatch. Both leaves are defined, so the message must say
        // "operator undefined" naming "Scalar" (both operands), not "undefined inputs".
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
        let msg = &results[0].diagnostics.messages[0].message;
        assert!(
            msg.contains("operator undefined"),
            "expected 'operator undefined' in message: {msg}"
        );
        assert!(
            msg.contains("Scalar"),
            "expected 'Scalar' in message: {msg}"
        );
        assert!(
            !msg.contains("undefined inputs"),
            "expected NO 'undefined inputs' in message: {msg}"
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
