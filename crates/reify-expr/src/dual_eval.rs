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

use reify_core::{ContentHash, FIELD_ENTITY_PREFIX, ValueCellId};
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

        CompiledExprKind::FunctionCall { function, args } => {
            // Dispatch on the BARE name — that is what `eval_expr` does.  The
            // only `qualified_name` comparison in the evaluator is the
            // `std::__interp_render` intercept, and that name is not in the
            // derivative table, so it falls through to the refusal path below.
            eval_dual_builtin(expr, &function.name, args, ctx, seeds, record)
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

/// A `FunctionCall` node.
///
/// The PRIMAL comes from `reify_stdlib::eval_builtin(name, &primal_args)` —
/// the exact call `eval_expr` makes — so every builtin's dimension handling
/// (`sqrt` → `dimension.root(2)`, `pow` → a bare `Real`, `asin`/`acos`/`atan`/
/// `atan2` → `DimensionVector::ANGLE`) is INHERITED rather than restated here.
/// This function supplies only the local partial derivatives, applied to the
/// argument tangents by the chain rule.
fn eval_dual_builtin(
    expr: &CompiledExpr,
    name: &str,
    args: &[CompiledExpr],
    ctx: &EvalContext,
    seeds: &Seeds,
    record: &mut BranchRecord,
) -> DualValue {
    // Names outside the derivative table (geometry, list, matrix, field ops)
    // are not differentiated.  They can only reach here when they DO depend on
    // a seed — a seed-independent call already short-circuited to
    // `Tangent::Zero` at the top of `eval_dual` — so the honest answer is a
    // loud refusal, not a zero row.
    if !is_differentiable_builtin(name, args.len()) {
        return DualValue::opaque(crate::eval_expr(expr, ctx));
    }
    // `eval_expr` lets a `Value::Field` cell named `__field__::<name>` shadow a
    // builtin of the same name.  Rather than replicate that lookup's semantics,
    // detect the shadow and hand the whole node back to the real evaluator, so
    // the primal invariant cannot be broken by an exotic value map.
    if args.len() == 1
        && matches!(
            ctx.values.get_or_undef(&ValueCellId::new(FIELD_ENTITY_PREFIX, name)),
            Value::Field { .. }
        )
    {
        return DualValue::opaque(crate::eval_expr(expr, ctx));
    }

    let duals: Vec<DualValue> =
        args.iter().map(|a| eval_dual(a, ctx, seeds, record)).collect();
    let primal_args: Vec<Value> = duals.iter().map(|d| d.value.clone()).collect();

    // Strict `Undef` propagation, matching `eval_expr`'s short-circuit.
    if primal_args.iter().any(|v| v.is_undef()) {
        return DualValue::opaque(Value::Undef);
    }

    let value = reify_stdlib::eval_builtin(name, &primal_args);
    if value.is_undef() {
        return DualValue::opaque(value);
    }
    if duals.iter().any(|d| d.tangent.is_none()) {
        return DualValue::opaque(value);
    }
    if duals.iter().all(|d| d.tangent.is_zero()) {
        return DualValue::constant(value);
    }

    // Join the three numeric scalar variants through `as_f64` (Invariant V:
    // a dimensionless `Scalar` is already a `Real` by the time it gets here).
    let Some(xs) = primal_args.iter().map(|v| v.as_f64()).collect::<Option<Vec<f64>>>() else {
        return DualValue::opaque(value);
    };
    let Some(partials) = builtin_partials(name, &xs) else {
        return DualValue::opaque(value);
    };
    // A non-finite local derivative — `sqrt(0)`, `log(0)`, `asin(±1)`, `abs(0)`
    // — is a refusal, never a NaN or Inf smuggled into a Jacobian.
    if partials.iter().any(|p| !p.is_finite()) {
        return DualValue::opaque(value);
    }

    let width = seeds.width();
    let mut row = vec![0.0; width];
    for (i, d) in duals.iter().enumerate() {
        if d.tangent.is_zero() {
            continue;
        }
        let Some(t) = d.tangent.materialize(width) else {
            return DualValue::opaque(value);
        };
        for (j, slot) in row.iter_mut().enumerate() {
            *slot += partials[i] * t[j];
        }
    }
    DualValue::with_row(value, row)
}

/// True when `name` at this arity has an entry in [`builtin_partials`].
fn is_differentiable_builtin(name: &str, arity: usize) -> bool {
    matches!(
        (name, arity),
        ("sqrt" | "exp" | "log" | "log10", 1)
            | ("sin" | "cos" | "tan" | "asin" | "acos" | "atan", 1)
            | ("sinh" | "cosh" | "tanh" | "abs", 1)
            | ("atan2" | "pow", 2)
            | ("lerp", 3)
            | ("remap", 5)
    )
}

/// The local partial derivatives `∂f/∂arg_i` of a smooth builtin at `xs`.
///
/// `None` when the name/arity has no entry.  A derivative that does not exist
/// at this point is returned as `NaN`, which the caller's finiteness guard
/// turns into `Tangent::None`.
fn builtin_partials(name: &str, xs: &[f64]) -> Option<Vec<f64>> {
    Some(match (name, xs.len()) {
        ("sqrt", 1) => vec![0.5 / xs[0].sqrt()],
        ("exp", 1) => vec![xs[0].exp()],
        // `log` IS the natural logarithm — there is no `ln` binding.
        ("log", 1) => vec![1.0 / xs[0]],
        ("log10", 1) => vec![1.0 / (xs[0] * std::f64::consts::LN_10)],
        ("sin", 1) => vec![xs[0].cos()],
        ("cos", 1) => vec![-xs[0].sin()],
        ("tan", 1) => {
            let c = xs[0].cos();
            vec![1.0 / (c * c)]
        }
        ("asin", 1) => vec![1.0 / (1.0 - xs[0] * xs[0]).sqrt()],
        ("acos", 1) => vec![-1.0 / (1.0 - xs[0] * xs[0]).sqrt()],
        ("atan", 1) => vec![1.0 / (1.0 + xs[0] * xs[0])],
        ("sinh", 1) => vec![xs[0].cosh()],
        ("cosh", 1) => vec![xs[0].sinh()],
        ("tanh", 1) => {
            let t = xs[0].tanh();
            vec![1.0 - t * t]
        }
        // `abs` is smooth away from the origin with derivative sign(x).  AT the
        // origin there is no derivative: `signum()` would confidently return
        // ±1, so an explicit NaN is emitted for the finiteness guard to catch.
        ("abs", 1) => vec![if xs[0] == 0.0 { f64::NAN } else { xs[0].signum() }],
        // NOTE the argument order: atan2(y, x).
        ("atan2", 2) => {
            let (y, x) = (xs[0], xs[1]);
            let d = x * x + y * y;
            vec![x / d, -y / d]
        }
        ("pow", 2) => {
            let (x, y) = (xs[0], xs[1]);
            vec![y * x.powf(y - 1.0), x.powf(y) * x.ln()]
        }
        // lerp(a, b, t) = a + t(b − a)
        ("lerp", 3) => {
            let (a, b, t) = (xs[0], xs[1], xs[2]);
            vec![1.0 - t, t, b - a]
        }
        // remap(x, flo, fhi, tlo, thi) = tlo + (x − flo)·(thi − tlo)/(fhi − flo)
        // With s = fhi − flo, d = thi − tlo, u = (x − flo)/s and k = d/s:
        //   ∂/∂x = k, ∂/∂flo = −k + u·k, ∂/∂fhi = −u·k,
        //   ∂/∂tlo = 1 − u, ∂/∂thi = u
        ("remap", 5) => {
            let (x, flo, fhi, tlo, thi) = (xs[0], xs[1], xs[2], xs[3], xs[4]);
            let s = fhi - flo;
            let k = (thi - tlo) / s;
            let u = (x - flo) / s;
            vec![k, -k + u * k, -u * k, 1.0 - u, u]
        }
        _ => return None,
    })
}
