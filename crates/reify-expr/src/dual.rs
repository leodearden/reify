//! Forward-mode dual numbers for automatic differentiation over `CompiledExpr`.
//!
//! Task #6672 (solver-unification ε).  Design reference:
//! `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.7.
//!
//! # What a `Dual` is
//!
//! A [`Dual`] carries a primal `value` plus an n-vector `tangent`, where
//! `tangent[j] = ∂value/∂x_j` for the j-th *seed* column.  Propagating one
//! `Dual` through an expression tree therefore yields that expression's whole
//! gradient row in a **single traversal** — which is exactly the shape η
//! (#6675) needs to assemble a Jacobian and μ (#6680) needs for a reduced
//! gradient.
//!
//! The tangent is `Vec`-backed rather than a compile-time `[f64; N]`, per PRD
//! §12 Q6: cluster dimension is *dynamic* (and ≤ 12 today), so a const-generic
//! width would force either monomorphisation over every observed cluster size
//! or a boxed fallback.  A `Vec` of ≤ 12 f64 is the cheaper answer.
//!
//! # No third-party dependency
//!
//! `reify-expr` has zero third-party dependencies, and this module keeps it
//! that way: the arithmetic below is ~100 lines of hand-rolled chain rule
//! rather than a `num-dual` / `autodiff` crate edge.

use reify_ir::Value;

/// Panic message shared by every binary operation, so callers (and tests) can
/// match on one stable string.
const WIDTH_MISMATCH: &str = "dual width mismatch";

/// A forward-mode dual number: a primal value plus its gradient row.
///
/// All binary operations require both operands to have the **same width**.
/// Two duals of differing width were seeded against different column vectors,
/// so combining them cannot yield a meaningful gradient row — silently
/// truncating or zero-extending would hand the solver a well-typed *wrong*
/// Jacobian, which is the worst possible shape.  Every binary operation
/// therefore panics with [`WIDTH_MISMATCH`] instead.
#[derive(Debug, Clone, PartialEq)]
pub struct Dual {
    /// The primal value, `f(x)`.
    pub value: f64,
    /// The gradient row: `tangent[j] = ∂f/∂x_j`, one entry per seed column.
    pub tangent: Vec<f64>,
}

impl Dual {
    /// A dual with an explicit value and tangent.
    pub fn new(value: f64, tangent: Vec<f64>) -> Self {
        Dual { value, tangent }
    }

    /// A constant of the given width: it depends on no seed, so its tangent is
    /// all zeros.
    pub fn constant(value: f64, width: usize) -> Self {
        Dual { value, tangent: vec![0.0; width] }
    }

    /// The `index`-th seed direction: tangent `e_index` of the given `width`,
    /// with a primal of `0.0`.  Compose with [`Dual::with_value`] to attach the
    /// point at which the derivative is being taken:
    ///
    /// ```ignore
    /// let x = Dual::seed(j, n).with_value(x_j);
    /// ```
    ///
    /// # Panics
    ///
    /// If `index >= width`.  There is no basis vector to return, and yielding
    /// an all-zero tangent instead would silently produce a zero Jacobian
    /// column for a variable the residual really does depend on.
    pub fn seed(index: usize, width: usize) -> Self {
        assert!(index < width, "seed index {index} out of range for width {width}");
        let mut tangent = vec![0.0; width];
        tangent[index] = 1.0;
        Dual { value: 0.0, tangent }
    }

    /// Replace the primal, keeping the tangent.
    pub fn with_value(mut self, value: f64) -> Self {
        self.value = value;
        self
    }

    /// Number of seed columns this dual is differentiated against.
    pub fn width(&self) -> usize {
        self.tangent.len()
    }

    /// True when every tangent component is finite.  A non-finite tangent means
    /// the derivative blew up (a division by zero, `ln` of a non-positive
    /// number, …) and must be reported, never handed to a linear solve.
    pub fn tangent_is_finite(&self) -> bool {
        self.tangent.iter().all(|t| t.is_finite())
    }

    fn check_width(&self, other: &Dual) {
        assert!(
            self.width() == other.width(),
            "{WIDTH_MISMATCH}: {} vs {}",
            self.width(),
            other.width()
        );
    }

    /// Sum rule: `(f + g)' = f' + g'`.
    #[allow(clippy::should_implement_trait)] // named for the rule, not `std::ops::Add`
    pub fn add(&self, other: &Dual) -> Dual {
        self.check_width(other);
        Dual {
            value: self.value + other.value,
            tangent: zip_map(&self.tangent, &other.tangent, |a, b| a + b),
        }
    }

    /// Difference rule: `(f − g)' = f' − g'`.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(&self, other: &Dual) -> Dual {
        self.check_width(other);
        Dual {
            value: self.value - other.value,
            tangent: zip_map(&self.tangent, &other.tangent, |a, b| a - b),
        }
    }

    /// Product rule: `(f·g)' = f'·g + f·g'`.
    #[allow(clippy::should_implement_trait)]
    pub fn mul(&self, other: &Dual) -> Dual {
        self.check_width(other);
        let (fv, gv) = (self.value, other.value);
        Dual {
            value: fv * gv,
            tangent: zip_map(&self.tangent, &other.tangent, |fp, gp| fp * gv + fv * gp),
        }
    }

    /// Quotient rule: `(f/g)' = (f'·g − f·g')/g²`.
    #[allow(clippy::should_implement_trait)]
    pub fn div(&self, other: &Dual) -> Dual {
        self.check_width(other);
        let (fv, gv) = (self.value, other.value);
        let g2 = gv * gv;
        Dual {
            value: fv / gv,
            tangent: zip_map(&self.tangent, &other.tangent, |fp, gp| (fp * gv - fv * gp) / g2),
        }
    }

    /// Negation: `(−f)' = −f'`.
    #[allow(clippy::should_implement_trait)]
    pub fn neg(&self) -> Dual {
        Dual { value: -self.value, tangent: self.tangent.iter().map(|t| -t).collect() }
    }

    /// Integer power rule: `(f^k)' = k·f^(k−1)·f'`.
    ///
    /// `k == 0` is special-cased to an exact constant `1` with an exactly-zero
    /// tangent, rather than evaluating `0·f^(−1)·f'` (which would be `NaN` at
    /// `f == 0` and could yield `−0.0` elsewhere).
    pub fn powi(&self, k: i32) -> Dual {
        if k == 0 {
            return Dual::constant(1.0, self.width());
        }
        let dfdx = f64::from(k) * self.value.powi(k - 1);
        self.chain(dfdx).with_value(self.value.powi(k))
    }

    /// General power rule for a positive base:
    /// `(f^g)' = f^g·(g'·ln f + g·f'/f)`.
    ///
    /// For `f <= 0` the identity is not defined (`ln f` is not real), and the
    /// tangent comes out non-finite — [`Dual::tangent_is_finite`] is how a
    /// caller detects that; it must never be silently rounded to zero.
    pub fn powf(&self, other: &Dual) -> Dual {
        self.check_width(other);
        let (fv, gv) = (self.value, other.value);
        let pow = fv.powf(gv);
        let ln_f = fv.ln();
        Dual {
            value: pow,
            tangent: zip_map(&self.tangent, &other.tangent, |fp, gp| {
                pow * (gp * ln_f + gv * fp / fv)
            }),
        }
    }

    /// Chain rule for any smooth single-argument function: scale the tangent by
    /// `df/dx` evaluated at this dual's primal.  The caller attaches the new
    /// primal with [`Dual::with_value`]:
    ///
    /// ```ignore
    /// // sin(x)
    /// x.chain(x.value.cos()).with_value(x.value.sin())
    /// ```
    pub fn chain(&self, dfdx: f64) -> Dual {
        Dual { value: self.value, tangent: self.tangent.iter().map(|t| t * dfdx).collect() }
    }
}

fn zip_map(a: &[f64], b: &[f64], f: impl Fn(f64, f64) -> f64) -> Vec<f64> {
    a.iter().zip(b.iter()).map(|(&x, &y)| f(x, y)).collect()
}

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
                assert!(row.len() == width, "{WIDTH_MISMATCH}: {} vs {width}", row.len());
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

/// A reify [`Value`] paired with its tangent.
///
/// The evaluator carries this, not a bare [`Dual`], because reify's numeric
/// scalars are *three* `Value` variants (`Int`, `Real`, `Scalar { si_value,
/// dimension }`) and the primal must keep its dimension and its `Undef`
/// provenance intact.  The primal half is always produced by the existing
/// evaluator; only the tangent half is new.
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
