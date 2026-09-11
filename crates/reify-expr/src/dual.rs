//! Forward-mode dual numbers for automatic differentiation over `CompiledExpr`.
//!
//! Task #6672 (solver-unification ε).  Design reference:
//! `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.7.
//!
//! # What a dual value is
//!
//! A [`DualValue`] carries a primal `Value` plus a [`Tangent`], the gradient row
//! `tangent[j] = ∂value/∂x_j` for the j-th *seed* column.  Propagating one
//! `DualValue` through an expression tree therefore yields that expression's
//! whole gradient row in a **single traversal** — which is exactly the shape η
//! (#6675) needs to assemble a Jacobian and μ (#6680) needs for a reduced
//! gradient.
//!
//! The primal is a whole `Value` rather than an `f64` because reify's numeric
//! scalars are *three* variants (`Int`, `Real`, `Scalar { si_value, dimension }`)
//! and the primal must keep its dimension and its `Undef` provenance intact.
//!
//! This module is the data, not the rules: every chain rule is stated exactly
//! once, in [`crate::dual_eval`], beside the traversal that applies it.  A
//! second statement of `(f·g)' = f'g + fg'` here — over a type nothing on the
//! evaluation path constructs — would be one more place for the two to drift.
//!
//! The tangent row is `Vec`-backed rather than a compile-time `[f64; N]`, per
//! PRD §12 Q6: cluster dimension is *dynamic* (and ≤ 12 today), so a
//! const-generic width would force either monomorphisation over every observed
//! cluster size or a boxed fallback.  A `Vec` of ≤ 12 f64 is the cheaper answer.
//!
//! # No third-party dependency
//!
//! `reify-expr` has zero third-party dependencies, and this module keeps it that
//! way: the arithmetic in `dual_eval` is hand-rolled chain rule rather than a
//! `num-dual` / `autodiff` crate edge.

use reify_ir::Value;

/// The tangent attached to a [`DualValue`] as it flows through the evaluator.
///
/// The three states are deliberately distinct, and the third is the reason this
/// is not just an `Option<Vec<f64>>` with a zero-vector convention:
///
/// - [`Tangent::Zero`] — the node provably does not depend on any seed (its
///   whole subtree was seed-independent).  The derivative *is* zero.
/// - [`Tangent::Scalar`] — a real gradient row, one entry per seed column.
/// - [`Tangent::None`] — **non-differentiable here**.  The node either produced
///   a non-scalar value (a `Point`, `Frame`, `Matrix`, …), reached an
///   unsupported kind, or hit `Undef`, *while* carrying a dependence on a seed.
///
/// `Tangent::None` must **never** be silently coerced to zero by any caller.
/// A zero row says "this residual does not move when you move this variable",
/// which is a *claim*, and a false one; a solver that believes it will step in
/// a direction the residual does not actually reward.  Callers convert `None`
/// into a typed refusal (`dual_eval::NonDifferentiable`) instead — see
/// PRD §7.7 ("Non-numeric operands stop contributing phantom gradients") and
/// INV-SF-7 ("a well-typed wrong value is the worst shape").
#[derive(Debug, Clone, PartialEq)]
pub enum Tangent {
    /// Provably seed-independent: the derivative is exactly zero.
    Zero,
    /// A gradient row of `width` components.
    Scalar(Vec<f64>),
    /// Non-differentiable at this node.  Never coerce to zero.
    None,
}

impl Tangent {
    /// Materialise this tangent as a concrete gradient row of `width`
    /// components, or `Option::None` when the node is non-differentiable.
    ///
    /// # Panics
    ///
    /// If a [`Tangent::Scalar`] carries a row of the wrong length — that is an
    /// internal invariant violation, not a user-reachable condition.
    pub fn materialize(&self, width: usize) -> Option<Vec<f64>> {
        match self {
            Tangent::Zero => Some(vec![0.0; width]),
            Tangent::Scalar(row) => {
                assert!(
                    row.len() == width,
                    "dual tangent width mismatch: {} vs {width}",
                    row.len()
                );
                Some(row.clone())
            }
            Tangent::None => Option::None,
        }
    }

    /// True for [`Tangent::None`] — i.e. "the derivative is unavailable here",
    /// which is emphatically not the same as "the derivative is zero".
    pub fn is_none(&self) -> bool {
        matches!(self, Tangent::None)
    }

    /// True for [`Tangent::Zero`].
    pub fn is_zero(&self) -> bool {
        matches!(self, Tangent::Zero)
    }

    /// Build a tangent from a gradient row, collapsing an all-zero row to
    /// [`Tangent::Zero`] so downstream `is_zero` fast paths stay effective.
    pub fn from_row(row: Vec<f64>) -> Tangent {
        if row.iter().all(|t| *t == 0.0) { Tangent::Zero } else { Tangent::Scalar(row) }
    }
}

/// A reify [`Value`] paired with its tangent — the unit the evaluator carries.
///
/// The primal half is always produced by the existing value semantics; only the
/// tangent half is new.
#[derive(Debug, Clone, PartialEq)]
pub struct DualValue {
    /// The primal, produced by the existing value semantics.
    pub value: Value,
    /// The derivative of `value` with respect to each seed column.
    pub tangent: Tangent,
}

impl DualValue {
    /// A value that provably depends on no seed.
    pub fn constant(value: Value) -> Self {
        DualValue { value, tangent: Tangent::Zero }
    }

    /// A value whose derivative is unavailable.  See [`Tangent::None`].
    pub fn opaque(value: Value) -> Self {
        DualValue { value, tangent: Tangent::None }
    }

    /// A value with an explicit gradient row.
    pub fn with_row(value: Value, row: Vec<f64>) -> Self {
        DualValue { value, tangent: Tangent::from_row(row) }
    }
}
