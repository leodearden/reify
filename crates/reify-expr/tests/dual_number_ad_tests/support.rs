//! The probe vocabulary shared by every group: dimensionless seeded cells
//! under entity `part`, the `jacobian_row` driver, and the closed-form and
//! central-difference assertions.

use reify_core::{Type, ValueCellId};
use reify_expr::{
    BranchRecord, EvalContext, NonDifferentiable, Seeds, eval_dual, eval_expr, jacobian_row,
};
use reify_ir::{CompiledExpr, CompiledFunction, Value, ValueMap};
use reify_test_support::builders::expr::{std_call, value_ref_typed};
use reify_test_support::finite_difference::{self, assert_column_matches_central_difference};

pub(crate) const ENT: &str = "part";

pub(crate) fn cell(member: &str) -> ValueCellId {
    ValueCellId::new(ENT, member)
}

pub(crate) fn dl() -> Type {
    Type::dimensionless_scalar()
}

/// Build a fixture of dimensionless `Real` cells, all seeded, in order.
pub(crate) fn probe(cells: &[(&str, f64)]) -> (ValueMap, Vec<ValueCellId>) {
    let mut values = ValueMap::new();
    let mut seed_cells = Vec::new();
    for (name, v) in cells {
        values.insert(cell(name), Value::Real(*v));
        seed_cells.push(cell(name));
    }
    (values, seed_cells)
}

pub(crate) fn pref(member: &str) -> CompiledExpr {
    value_ref_typed(ENT, member, dl())
}

pub(crate) fn calln(name: &str, args: Vec<CompiledExpr>) -> CompiledExpr {
    std_call(name, args, dl())
}

pub(crate) fn jrow(
    expr: &CompiledExpr,
    values: &ValueMap,
    seed_cells: &[ValueCellId],
) -> Result<(f64, Vec<f64>), NonDifferentiable> {
    let ctx = EvalContext::simple(values);
    let seeds = Seeds::new(seed_cells);
    let mut record = BranchRecord::new();
    jacobian_row(expr, &ctx, &seeds, &mut record)
}

/// Tolerance for a hand-computed algebraic identity: a closed-form comparison
/// of two f64 expression *orderings*, not a finite-difference comparison, so
/// the only error present is last-place rounding; 1e-15 relative is roughly
/// 4 ulp.  Finite-difference agreement uses `assert_ad_matches_cd` instead,
/// whose tolerance is set by the reference's own noise floor.
pub(crate) fn assert_close(actual: f64, expected: f64, what: &str) {
    let tol = 1e-15 * expected.abs() + 1e-300;
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: got {actual:?}, expected {expected:?} (tol {tol:?})"
    );
}

pub(crate) fn assert_row_close(actual: &[f64], expected: &[f64], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: row width");
    for (j, (&got, &want)) in actual.iter().zip(expected.iter()).enumerate() {
        let tol = 1e-15 * want.abs() + 1e-300;
        assert!(
            (got - want).abs() <= tol,
            "{label} column {j}: got {got:?}, expected {want:?}"
        );
    }
}

/// Central-difference reference for `∂expr/∂cell`, computed only through
/// `eval_expr` on a perturbed copy of the value map.
///
/// `functions` is threaded rather than assumed empty so that a residual
/// containing user algebra is differenced by the SAME reference as one that is
/// not — a second finite-difference implementation for the with-functions case
/// is a second thing to keep right.
pub(crate) fn central_difference(
    expr: &CompiledExpr,
    values: &ValueMap,
    target: &ValueCellId,
    functions: &[CompiledFunction],
) -> f64 {
    let base = values.get(target).cloned().expect("probe cell must exist");
    let x = base.as_f64().expect("probe cell must be numeric");
    let dim = base.dimension();
    finite_difference::central_difference(x, |v| {
        let mut perturbed = values.clone();
        perturbed.insert(target.clone(), Value::from_real_scalar(v, dim));
        let ctx = EvalContext::new(&perturbed, functions);
        eval_expr(expr, &ctx).as_f64().expect("perturbed evaluation must stay numeric")
    })
}

/// The suite's single assertion shape: one dual traversal produces the whole
/// gradient row, and every column is checked against its own central
/// difference.
///
/// `EvalContext::simple(v)` IS `EvalContext::new(v, &[])`, so the no-functions
/// case is this same driver with an empty slice rather than a second copy of
/// it.
pub(crate) fn assert_ad_matches_cd(
    label: &str,
    expr: &CompiledExpr,
    values: &ValueMap,
    seed_cells: &[ValueCellId],
) {
    assert_ad_matches_cd_with_fns(label, expr, values, seed_cells, &[]);
}

/// [`assert_ad_matches_cd`] with user functions in scope.
pub(crate) fn assert_ad_matches_cd_with_fns(
    label: &str,
    expr: &CompiledExpr,
    values: &ValueMap,
    seed_cells: &[ValueCellId],
    functions: &[CompiledFunction],
) {
    let ctx = EvalContext::new(values, functions);
    let seeds = Seeds::new(seed_cells);
    let mut record = BranchRecord::new();
    let dual = eval_dual(expr, &ctx, &seeds, &mut record);

    assert_eq!(
        dual.value,
        eval_expr(expr, &ctx),
        "{label}: the primal invariant must hold for builtins too"
    );

    let ad = dual
        .tangent
        .materialize(seed_cells.len())
        .unwrap_or_else(|| panic!("{label}: expected a differentiable tangent, got Tangent::None"));

    for (j, target) in seed_cells.iter().enumerate() {
        let cd = central_difference(expr, values, target, functions);
        assert_column_matches_central_difference(label, j, ad[j], cd);
    }
}
