//! Sound closed-interval arithmetic over f64 SI magnitudes (task #6655).
//!
//! Every operation encloses the exact real result over its operand boxes.
//! Rounding is directed through error-free transforms: the round-to-nearest
//! result is kept when its exact residual shows it already lies on the safe
//! side, and moved one ulp outward otherwise. An operation that is exact in
//! f64 therefore stays exact, and an inexact one is one ulp wide.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// A non-empty closed interval `[lo, hi]` of the extended reals.
///
/// Invariant: no NaN endpoint, `lo <= hi`, `lo != +inf`, `hi != -inf`. It is
/// established by [`checked`] alone, which every constructor and every
/// operation result goes through. An empty set is never an `Interval`; the
/// operations that can produce one return `Option`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    lo: f64,
    hi: f64,
}

fn checked(lo: f64, hi: f64) -> Option<Interval> {
    let well_formed = lo <= hi && lo != f64::INFINITY && hi != f64::NEG_INFINITY;
    well_formed.then_some(Interval { lo, hi })
}

/// The result of an operation on well-formed operands, which is well formed by
/// construction. The `ENTIRE` fallback is sound and is never expected to run.
fn enclosure(lo: f64, hi: f64) -> Interval {
    let result = checked(lo, hi);
    debug_assert!(result.is_some(), "malformed enclosure [{lo}, {hi}]");
    result.unwrap_or(Interval::ENTIRE)
}

impl Interval {
    pub const ENTIRE: Interval = Interval {
        lo: f64::NEG_INFINITY,
        hi: f64::INFINITY,
    };

    pub fn new(lo: f64, hi: f64) -> Option<Interval> {
        checked(lo, hi)
    }

    /// The degenerate interval `[v, v]`; `None` for a non-finite `v`.
    pub fn point(v: f64) -> Option<Interval> {
        if v.is_finite() { checked(v, v) } else { None }
    }

    pub fn lo(self) -> f64 {
        self.lo
    }

    pub fn hi(self) -> f64 {
        self.hi
    }

    pub fn is_entire(self) -> bool {
        self == Interval::ENTIRE
    }

    pub fn contains(self, v: f64) -> bool {
        self.lo <= v && v <= self.hi
    }

    pub fn intersect(self, other: Interval) -> Option<Interval> {
        checked(self.lo.max(other.lo), self.hi.min(other.hi))
    }

    /// The smallest interval enclosing both operands.
    pub fn hull(self, other: Interval) -> Interval {
        enclosure(self.lo.min(other.lo), self.hi.max(other.hi))
    }

    /// `x^n`; `x^0` is `[1, 1]`, including at `0`.
    pub fn powi(self, n: u32) -> Interval {
        if n == 0 {
            return enclosure(1.0, 1.0);
        }
        if n % 2 == 0 {
            let magnitude = self.abs();
            return enclosure(
                power_dir(magnitude.lo, n, Dir::Down),
                power_dir(magnitude.hi, n, Dir::Up),
            );
        }
        enclosure(
            power_dir(self.lo, n, Dir::Down),
            power_dir(self.hi, n, Dir::Up),
        )
    }

    /// `√x` over the part of `x` inside the domain `[0, +inf]`; `None` when
    /// `x` lies wholly below zero.
    pub fn sqrt(self) -> Option<Interval> {
        let domain = self.intersect(Interval::non_negative())?;
        Some(enclosure(
            sqrt_dir(domain.lo, Dir::Down),
            sqrt_dir(domain.hi, Dir::Up),
        ))
    }

    pub fn abs(self) -> Interval {
        if self.lo >= 0.0 {
            self
        } else if self.hi <= 0.0 {
            -self
        } else {
            enclosure(0.0, (-self.lo).max(self.hi))
        }
    }

    pub fn min(self, other: Interval) -> Interval {
        enclosure(self.lo.min(other.lo), self.hi.min(other.hi))
    }

    pub fn max(self, other: Interval) -> Interval {
        enclosure(self.lo.max(other.lo), self.hi.max(other.hi))
    }

    fn contains_zero(self) -> bool {
        self.contains(0.0)
    }

    fn non_negative() -> Interval {
        enclosure(0.0, f64::INFINITY)
    }
}

/// Relational inverses, the backward half of HC4. Each narrows the CURRENT
/// domain of one operand given that the operation's result lies in `z`;
/// `None` means no point of that domain can produce a result in `z`.
impl Interval {
    /// The `x` with `x^n ∈ z`.
    pub fn powi_preimage(z: Interval, x: Interval, n: u32) -> Option<Interval> {
        if n == 0 {
            return z.contains(1.0).then_some(x);
        }
        if n % 2 == 0 {
            let magnitude = z.intersect(Interval::non_negative())?;
            let roots = enclosure(
                root_dir(magnitude.lo, n, Dir::Down),
                root_dir(magnitude.hi, n, Dir::Up),
            );
            return symmetric_preimage(roots, x);
        }
        x.intersect(enclosure(
            signed_root_dir(z.lo, n, Dir::Down),
            signed_root_dir(z.hi, n, Dir::Up),
        ))
    }

    /// The `x` with `√x ∈ z`.
    pub fn sqrt_preimage(z: Interval, x: Interval) -> Option<Interval> {
        let roots = z.intersect(Interval::non_negative())?;
        x.intersect(roots.powi(2))
    }

    /// The `x` with `|x| ∈ z`.
    pub fn abs_preimage(z: Interval, x: Interval) -> Option<Interval> {
        symmetric_preimage(z.intersect(Interval::non_negative())?, x)
    }

    /// The `x` with `x · y ∈ z`. A factor `y` containing 0 leaves `x`
    /// unchanged, since [`Div`] then gives [`Interval::ENTIRE`].
    pub fn mul_preimage(z: Interval, x: Interval, y: Interval) -> Option<Interval> {
        x.intersect(z / y)
    }

    /// The numerator `x` with `x / y ∈ z`.
    pub fn div_numerator_preimage(z: Interval, x: Interval, y: Interval) -> Option<Interval> {
        x.intersect(z * y)
    }

    /// The denominator `y` with `x / y ∈ z`. A range `z` containing 0 leaves
    /// `y` unchanged.
    pub fn div_denominator_preimage(z: Interval, y: Interval, x: Interval) -> Option<Interval> {
        y.intersect(x / z)
    }

    /// The `x` with `min(x, y) ∈ z`: `x` is at least `z.lo`, and lies in `z`
    /// outright when `y` is too large to be the minimum.
    pub fn min_preimage(z: Interval, x: Interval, y: Interval) -> Option<Interval> {
        let floored = x.intersect(enclosure(z.lo, f64::INFINITY))?;
        if y.lo > z.hi {
            floored.intersect(z)
        } else {
            Some(floored)
        }
    }

    /// The `x` with `max(x, y) ∈ z`; the mirror of [`Interval::min_preimage`].
    pub fn max_preimage(z: Interval, x: Interval, y: Interval) -> Option<Interval> {
        let capped = x.intersect(enclosure(f64::NEG_INFINITY, z.hi))?;
        if y.hi < z.lo {
            capped.intersect(z)
        } else {
            Some(capped)
        }
    }
}

/// The `x` whose magnitude lies in `magnitude` (non-negative): the hull of the
/// positive and negative branches that survive in `x`. Shared by the
/// even-power and `abs` inverses.
fn symmetric_preimage(magnitude: Interval, x: Interval) -> Option<Interval> {
    match (x.intersect(magnitude), x.intersect(-magnitude)) {
        (Some(positive), Some(negative)) => Some(positive.hull(negative)),
        (positive, negative) => positive.or(negative),
    }
}

impl Neg for Interval {
    type Output = Interval;

    fn neg(self) -> Interval {
        enclosure(-self.hi, -self.lo)
    }
}

impl Add for Interval {
    type Output = Interval;

    fn add(self, other: Interval) -> Interval {
        enclosure(
            add_dir(self.lo, other.lo, Dir::Down),
            add_dir(self.hi, other.hi, Dir::Up),
        )
    }
}

impl Sub for Interval {
    type Output = Interval;

    fn sub(self, other: Interval) -> Interval {
        self + -other
    }
}

impl Mul for Interval {
    type Output = Interval;

    fn mul(self, other: Interval) -> Interval {
        corner_hull(self, other, mul_dir)
    }
}

impl Div for Interval {
    type Output = Interval;

    /// A divisor containing zero gives [`Interval::ENTIRE`].
    fn div(self, other: Interval) -> Interval {
        if other.contains_zero() {
            return Interval::ENTIRE;
        }
        corner_hull(self, other, div_dir)
    }
}

/// The hull of `op` over the four endpoint pairs, for an `op` monotone in each
/// operand over the box. A NaN corner (`inf / inf`) is skipped by `f64::min` /
/// `f64::max`; the adjacent corners already reach the extremes it stands for.
fn corner_hull(a: Interval, b: Interval, op: fn(f64, f64, Dir) -> f64) -> Interval {
    let corners = [(a.lo, b.lo), (a.lo, b.hi), (a.hi, b.lo), (a.hi, b.hi)];
    let lo = corners
        .iter()
        .fold(f64::INFINITY, |acc, &(x, y)| acc.min(op(x, y, Dir::Down)));
    let hi = corners
        .iter()
        .fold(f64::NEG_INFINITY, |acc, &(x, y)| acc.max(op(x, y, Dir::Up)));
    enclosure(lo, hi)
}

// ── Directed rounding ───────────────────────────────────────────────────────

/// Which bound a directed operation computes: `Down` never exceeds the exact
/// result, `Up` never falls below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Down,
    Up,
}

impl Dir {
    fn flipped(self) -> Dir {
        match self {
            Dir::Down => Dir::Up,
            Dir::Up => Dir::Down,
        }
    }

    fn unbounded(self) -> f64 {
        match self {
            Dir::Down => f64::NEG_INFINITY,
            Dir::Up => f64::INFINITY,
        }
    }
}

/// Below this magnitude a product or quotient residual can itself underflow,
/// so results there are widened unconditionally instead of corrected.
const EFT_EXACT_MIN: f64 = f64::MIN_POSITIVE * (1u64 << 54) as f64;

/// Keep `nearest`, or step it one ulp outward when `error` (the sign of the
/// exact result minus `nearest`) shows it lies on the wrong side.
fn correct(nearest: f64, error: f64, dir: Dir) -> f64 {
    match dir {
        Dir::Down if error < 0.0 => nearest.next_down(),
        Dir::Up if error > 0.0 => nearest.next_up(),
        _ => nearest,
    }
}

fn widen(nearest: f64, dir: Dir) -> f64 {
    match dir {
        Dir::Down => nearest.next_down(),
        Dir::Up => nearest.next_up(),
    }
}

/// An infinite result. From finite operands it is an overflow whose exact
/// value is finite, so a bound on the far side of it is the largest finite
/// value; from an infinite operand it is exact.
fn infinite_result(nearest: f64, operands_finite: bool, dir: Dir) -> f64 {
    match (operands_finite, dir) {
        (true, Dir::Down) if nearest == f64::INFINITY => f64::MAX,
        (true, Dir::Up) if nearest == f64::NEG_INFINITY => f64::MIN,
        _ => nearest,
    }
}

fn add_dir(a: f64, b: f64, dir: Dir) -> f64 {
    let sum = a + b;
    if sum.is_nan() {
        return dir.unbounded();
    }
    if sum.is_infinite() {
        return infinite_result(sum, a.is_finite() && b.is_finite(), dir);
    }
    let b_virtual = sum - a;
    let error = (a - (sum - b_virtual)) + (b - b_virtual);
    correct(sum, error, dir)
}

fn mul_dir(a: f64, b: f64, dir: Dir) -> f64 {
    if a == 0.0 || b == 0.0 {
        return 0.0;
    }
    let product = a * b;
    if product.is_infinite() {
        return infinite_result(product, a.is_finite() && b.is_finite(), dir);
    }
    if product.abs() < EFT_EXACT_MIN {
        return widen(product, dir);
    }
    correct(product, a.mul_add(b, -product), dir)
}

/// `a / b` for a non-zero `b`. `inf / inf` is NaN, which [`corner_hull`] skips.
fn div_dir(a: f64, b: f64, dir: Dir) -> f64 {
    let quotient = a / b;
    if quotient.is_nan() {
        return quotient;
    }
    if quotient.is_infinite() {
        return infinite_result(quotient, a.is_finite() && b.is_finite(), dir);
    }
    if a == 0.0 || b.is_infinite() {
        return quotient;
    }
    if quotient.abs() < EFT_EXACT_MIN || a.abs() < EFT_EXACT_MIN {
        return widen(quotient, dir);
    }
    let remainder = (-quotient).mul_add(b, a);
    correct(quotient, remainder * b.signum(), dir)
}

/// `√x` for `x >= 0`.
fn sqrt_dir(x: f64, dir: Dir) -> f64 {
    let root = x.sqrt();
    if x == 0.0 || x.is_infinite() {
        return root;
    }
    if x < EFT_EXACT_MIN {
        return widen(root, dir).max(0.0);
    }
    correct(root, (-root).mul_add(root, x), dir)
}

/// `x^n` for `n >= 1`, as a chain of directed products on the magnitude. Each
/// partial product of non-negative factors is monotone in its inputs, so a
/// chain rounded one way bounds the exact power that way; an odd power of a
/// negative base is the negated magnitude power rounded the other way.
fn power_dir(x: f64, n: u32, dir: Dir) -> f64 {
    if x < 0.0 && n % 2 == 1 {
        return -magnitude_power_dir(-x, n, dir.flipped());
    }
    magnitude_power_dir(x.abs(), n, dir)
}

fn magnitude_power_dir(m: f64, n: u32, dir: Dir) -> f64 {
    (1..n).fold(m, |acc, _| mul_dir(acc, m, dir))
}

/// Bound on the outward and inward ulp steps [`root_dir`] takes from its guess.
const ROOT_REFINE_STEPS: usize = 8;

/// `m^(1/n)` for `m >= 0` and `n >= 1`.
///
/// Above `n = 2` there is no correctly rounded root to correct, so a guess is
/// VERIFIED against [`magnitude_power_dir`] instead, which keeps this on the
/// one rounding mechanism: a lower bound is a candidate whose power rounded up
/// is still `<= m`, an upper bound one whose power rounded down is `>= m`. The
/// guess steps outward until it verifies, then inward while its neighbour
/// still does. A guess that never verifies falls back to the trivial sound
/// bound (`0` below, `+inf` above).
fn root_dir(m: f64, n: u32, dir: Dir) -> f64 {
    if n == 1 || m == 0.0 || m.is_infinite() {
        return m;
    }
    if n == 2 {
        return sqrt_dir(m, dir);
    }
    let bounds_root = |r: f64| match dir {
        Dir::Down => r >= 0.0 && magnitude_power_dir(r, n, Dir::Up) <= m,
        Dir::Up => magnitude_power_dir(r, n, Dir::Down) >= m,
    };
    let mut root = newton_root_guess(m, n);
    for _ in 0..ROOT_REFINE_STEPS {
        if bounds_root(root) {
            break;
        }
        root = widen(root, dir);
    }
    if !bounds_root(root) {
        return match dir {
            Dir::Down => 0.0,
            Dir::Up => f64::INFINITY,
        };
    }
    for _ in 0..ROOT_REFINE_STEPS {
        let inner = widen(root, dir.flipped());
        if !bounds_root(inner) {
            break;
        }
        root = inner;
    }
    root
}

/// A round-to-nearest estimate of `m^(1/n)`: `powf`, then one Newton step,
/// since `powf(m, 1/n)` inherits the rounding of `1/n` amplified by `ln m`.
fn newton_root_guess(m: f64, n: u32) -> f64 {
    let guess = m.powf(1.0 / f64::from(n));
    let exponent = i32::try_from(n).unwrap_or(i32::MAX);
    let refined = guess - (guess.powi(exponent) - m) / (f64::from(n) * guess.powi(exponent - 1));
    if refined.is_finite() && refined > 0.0 {
        refined
    } else {
        guess
    }
}

/// The real `n`-th root of a signed `v`, for odd `n`.
fn signed_root_dir(v: f64, n: u32, dir: Dir) -> f64 {
    if v < 0.0 {
        -root_dir(-v, n, dir.flipped())
    } else {
        root_dir(v, n, dir)
    }
}
