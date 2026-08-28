//! Forward-mode dual-number evaluation of a `CompiledExpr`.
//!
//! Task #6672 (solver-unification ε).  Design reference:
//! `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.7.
//!
//! # The one rule that shapes this whole module
//!
//! **This is not a second evaluator.**  [`eval_dual`] computes *only tangents*.
//! Every primal it returns comes from the existing implementation:
//!
//! - a subtree that references no seed cell is handed wholesale to
//!   [`crate::eval_expr`] and lifted with [`Tangent::Zero`];
//! - a node that does depend on a seed recurses on its children and obtains its
//!   own primal from the crate-private value-semantics helpers
//!   ([`crate::eval_add`], [`crate::eval_sub`], [`crate::eval_mul`],
//!   [`crate::eval_div`], [`crate::eval_pow`], [`crate::eval_mod`],
//!   [`crate::negate_value`]).
//!
//! So reify's value semantics — dimension algebra, strict-`Undef` propagation,
//! `sanitize`, the Invariant V `Scalar{DIMENSIONLESS} → Real` collapse,
//! `Point + Point` rejection — have exactly ONE implementation, and
//! `eval_dual(e).value == eval_expr(e)` holds by construction rather than by
//! diligence.
//!
//! # The `Undef` cliff
//!
//! When the primal is `Undef`, the tangent is [`Tangent::None`], never a
//! finite row.  `Undef` means the value semantics refused to produce a number;
//! attaching a derivative to that refusal would be inventing one.

use std::cell::RefCell;
use std::collections::HashMap;

use reify_core::{ContentHash, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, UnOp, Value};

use crate::EvalContext;
use crate::branch_signature::BranchRecord;
use crate::dual::{DualValue, Tangent};

/// The ordered column vector a dual evaluation is differentiated against.
///
/// Column `j` of every tangent row corresponds to `columns()[j]`.  Consumers
/// (η's Jacobian, μ's reduced gradient) rely on that ordering being *theirs* —
/// it is the caller's `auto_params` order, not an internal one.
pub struct Seeds {
    columns: Vec<ValueCellId>,
    index: HashMap<ValueCellId, usize>,
    /// Memo for the seed-dependence predicate, keyed on `CompiledExpr::content_hash`.
    ///
    /// Without it, asking "does this subtree touch a seed?" at every node walks
    /// each node's whole subtree — quadratic in depth.  The predicate is a pure
    /// function of `(subtree, seed set)`, and a `Seeds` *is* the seed set, so
    /// this is the correct place to cache it.
    depends: RefCell<HashMap<ContentHash, bool>>,
}

impl Seeds {
    /// Build a seed set from an ordered column vector.
    ///
    /// Duplicate cells keep their FIRST column: a repeated auto param would
    /// otherwise split one variable's derivative across two columns.
    pub fn new(columns: &[ValueCellId]) -> Self {
        let mut index = HashMap::with_capacity(columns.len());
        for (j, id) in columns.iter().enumerate() {
            index.entry(id.clone()).or_insert(j);
        }
        Seeds { columns: columns.to_vec(), index, depends: RefCell::new(HashMap::new()) }
    }

    /// Number of columns — the width of every tangent row.
    pub fn width(&self) -> usize {
        self.columns.len()
    }

    /// The column vector, in order.
    pub fn columns(&self) -> &[ValueCellId] {
        &self.columns
    }

    /// The column index of `id`, or `None` when `id` is not a seed.
    pub fn column_of(&self, id: &ValueCellId) -> Option<usize> {
        self.index.get(id).copied()
    }

    /// True when `expr`'s subtree references at least one seed cell.
    ///
    /// A `false` answer is a *proof* that the whole subtree is constant with
    /// respect to every seed, which lets [`eval_dual`] evaluate it with the
    /// existing evaluator in one call and lift it with [`Tangent::Zero`].
    pub fn depends_on_seed(&self, expr: &CompiledExpr) -> bool {
        if let Some(hit) = self.depends.borrow().get(&expr.content_hash) {
            return *hit;
        }
        let answer = expr.collect_value_refs().iter().any(|id| self.index.contains_key(id));
        self.depends.borrow_mut().insert(expr.content_hash, answer);
        answer
    }
}

/// Forward-mode dual evaluation of `expr` at the point held in `ctx`.
///
/// Returns the primal (identical to `eval_expr(expr, ctx)`) paired with
/// `∂primal/∂x_j` for each seed column `j`.  Non-smooth nodes traversed on the
/// way are appended to `record`.
pub fn eval_dual(
    expr: &CompiledExpr,
    ctx: &EvalContext,
    seeds: &Seeds,
    record: &mut BranchRecord,
) -> DualValue {
    // Fast path and correctness path in one: a subtree that cannot reach a seed
    // has a provably zero derivative, so evaluate it exactly as the rest of the
    // system would and lift it.
    if !seeds.depends_on_seed(expr) {
        return DualValue::constant(crate::eval_expr(expr, ctx));
    }

    match &expr.kind {
        // A literal never depends on a seed, so it is unreachable here; keep the
        // arm anyway so the fast path is an optimisation, not a load-bearing
        // precondition.
        CompiledExprKind::Literal(v) => DualValue::constant(v.clone()),

        CompiledExprKind::ValueRef(id) | CompiledExprKind::CrossSubGeometryRef(id) => {
            let value = ctx.values.get_or_undef(id);
            match seeds.column_of(id) {
                Some(j) if value.as_f64().is_some() => {
                    let mut row = vec![0.0; seeds.width()];
                    row[j] = 1.0;
                    DualValue { value, tangent: Tangent::Scalar(row) }
                }
                // A seeded cell holding a NON-scalar (or Undef) value cannot
                // carry a scalar tangent.  Refuse loudly rather than emit a
                // zero row, which would claim the residual is flat in a
                // variable it may well depend on.
                Some(_) => DualValue::opaque(value),
                None => DualValue::constant(value),
            }
        }

        CompiledExprKind::UnOp { op, operand } => {
            let inner = eval_dual(operand, ctx, seeds, record);
            match op {
                UnOp::Neg => {
                    let value = crate::negate_value(inner.value.clone());
                    finish_unary(value, &inner, |_| -1.0)
                }
                // `not` is Bool-valued: there is no scalar derivative to take.
                UnOp::Not => DualValue::opaque(crate::eval_expr(expr, ctx)),
            }
        }

        CompiledExprKind::BinOp { op, left, right } => {
            eval_dual_binop(expr, *op, left, right, ctx, seeds, record)
        }

        // Everything else is a kind this step does not yet differentiate.  The
        // primal still comes from the real evaluator; the tangent is a loud
        // refusal carrying the offending kind's name.
        _ => DualValue::opaque(crate::eval_expr(expr, ctx)),
    }
}

/// Complete a unary node: compute the tangent as `df/dx · inner_tangent`,
/// subject to the `Undef` cliff.
fn finish_unary(value: Value, inner: &DualValue, dfdx: impl Fn(f64) -> f64) -> DualValue {
    if value.is_undef() {
        return DualValue::opaque(value);
    }
    match &inner.tangent {
        Tangent::Zero => DualValue::constant(value),
        Tangent::None => DualValue::opaque(value),
        Tangent::Scalar(row) => {
            let Some(x) = inner.value.as_f64() else {
                return DualValue::opaque(value);
            };
            let k = dfdx(x);
            DualValue::with_row(value, row.iter().map(|t| t * k).collect())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_dual_binop(
    expr: &CompiledExpr,
    op: BinOp,
    left: &CompiledExpr,
    right: &CompiledExpr,
    ctx: &EvalContext,
    seeds: &Seeds,
    record: &mut BranchRecord,
) -> DualValue {
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow | BinOp::Mod => {}
        // Comparisons and Kleene connectives are Bool-valued kinks; they land
        // with the branch-signature arms.
        _ => return DualValue::opaque(crate::eval_expr(expr, ctx)),
    }

    let ld = eval_dual(left, ctx, seeds, record);
    let rd = eval_dual(right, ctx, seeds, record);

    // THE PRIMAL COMES FROM THE EXISTING CODE.
    let value = match op {
        BinOp::Add => crate::eval_add(&ld.value, &rd.value),
        BinOp::Sub => crate::eval_sub(&ld.value, &rd.value),
        BinOp::Mul => crate::eval_mul(&ld.value, &rd.value),
        BinOp::Div => crate::eval_div(&ld.value, &rd.value),
        BinOp::Pow => crate::eval_pow(&ld.value, &rd.value),
        BinOp::Mod => crate::eval_mod(&ld.value, &rd.value),
        _ => unreachable!("guarded above"),
    };

    // The Undef cliff: no derivative is attached to a refusal.
    if value.is_undef() {
        return DualValue::opaque(value);
    }
    if ld.tangent.is_none() || rd.tangent.is_none() {
        return DualValue::opaque(value);
    }
    if ld.tangent.is_zero() && rd.tangent.is_zero() {
        return DualValue::constant(value);
    }

    let width = seeds.width();
    let (Some(lrow), Some(rrow)) = (ld.tangent.materialize(width), rd.tangent.materialize(width))
    else {
        return DualValue::opaque(value);
    };
    // Join the three numeric scalar variants (`Int`, `Real`, `Scalar`) through
    // `as_f64` — NOT by matching `Value::Scalar`, which `from_real_scalar`
    // collapses to `Real` whenever the dimension cancels (Invariant V).
    let (Some(a), Some(b)) = (ld.value.as_f64(), rd.value.as_f64()) else {
        return DualValue::opaque(value);
    };

    let row: Vec<f64> = (0..width)
        .map(|j| {
            let (ap, bp) = (lrow[j], rrow[j]);
            match op {
                BinOp::Add => ap + bp,
                BinOp::Sub => ap - bp,
                BinOp::Mul => ap * b + a * bp,
                BinOp::Div => (ap * b - a * bp) / (b * b),
                BinOp::Pow => pow_tangent(a, b, ap, bp, rd.tangent.is_zero()),
                // `a % b = a − trunc(a/b)·b`, so away from the wrap points
                // ∂/∂a = 1 and ∂/∂b = −trunc(a/b).  The wrap points themselves
                // are a kink, recorded with the other kinks.
                BinOp::Mod => ap - (a / b).trunc() * bp,
                _ => unreachable!("guarded above"),
            }
        })
        .collect();

    DualValue::with_row(value, row)
}

/// One column of `d(a^b)`.
///
/// With a *constant* exponent the identity `b·a^(b−1)·a'` is used, which stays
/// valid for a NEGATIVE base at integral `b` — the common case in reify, where
/// `Scalar ^ Int` is the only dimensioned power the value layer accepts.  Only
/// a seed-dependent exponent needs the general form, which requires `ln a` and
/// is therefore restricted to a positive base.
fn pow_tangent(a: f64, b: f64, ap: f64, bp: f64, exponent_is_constant: bool) -> f64 {
    if exponent_is_constant {
        if b == 0.0 {
            // Exactly zero, rather than 0·a^(−1)·a' (NaN at a == 0).
            return 0.0;
        }
        return b * a.powf(b - 1.0) * ap;
    }
    a.powf(b) * (bp * a.ln() + b * ap / a)
}
