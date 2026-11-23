//! Dense Householder linear algebra for small least-squares problems.
//!
//! [`RowSpaceQr`] factorizes the rows of a matrix `A` in the order they are
//! pushed — a QR of `Aᵀ` that spends a reflector only on a row that widens the
//! span of the rows before it. One factorization therefore answers three
//! questions: which rows are independent (and in what order they became so), the
//! rank, and an orthonormal basis of the null space (the trailing columns of `Q`).
//! [`RowSpaceQr::least_squares`] adds the minimum-norm least-squares solution of
//! `A x ≈ b`, which has no component in that null space.
//!
//! Vectors are plain `f64` slices; every operation is `O(m·n²)` or better, sized
//! for the tens of unknowns a geometric solve carries.

/// The two cutoffs a [`RowSpaceQr`] applies to each pushed row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankTolerance {
    /// A row widens the span iff its component orthogonal to the span so far,
    /// divided by the row's own norm, exceeds this.
    pub independence: f64,
    /// A row whose norm is below this is numerically zero and never widens the
    /// span.
    pub zero_row: f64,
}

/// An ordered, rank-revealing Householder factorization of the rows pushed so
/// far.
#[derive(Debug, Clone)]
pub struct RowSpaceQr {
    dim: usize,
    tolerance: RankTolerance,
    /// `H₁ … H_r`, one per independent row; `Q = H₁·H₂·…·H_r`.
    reflectors: Vec<Reflector>,
    /// Each pushed row's coordinates `Q₁ᵀa` in the row-space basis, truncated to
    /// the rank reached by its push (empty for a zero row). Later reflectors act
    /// only on later coordinates, so these never change after the push.
    coordinates: Vec<Vec<f64>>,
}

impl RowSpaceQr {
    /// An empty factorization of rows of length `dim`.
    pub fn new(dim: usize, tolerance: RankTolerance) -> Self {
        Self {
            dim,
            tolerance,
            reflectors: Vec::new(),
            coordinates: Vec::new(),
        }
    }

    /// Push one row (of the length given to [`new`](Self::new)); `true` iff it
    /// widened the span.
    pub fn push(&mut self, row: &[f64]) -> bool {
        assert_eq!(row.len(), self.dim, "row length must equal the dimension");
        let row_norm = norm(row);
        if !row_norm.is_finite() || row_norm < self.tolerance.zero_row {
            self.coordinates.push(Vec::new());
            return false;
        }
        let mut w = row.to_vec();
        self.apply_qt(&mut w);
        let rank = self.rank();
        let widened = norm(&w[rank..]) > self.tolerance.independence * row_norm;
        if widened {
            let (reflector, image) = Reflector::annihilating(rank, &w[rank..]);
            w[rank] = image;
            self.reflectors.push(reflector);
        }
        w.truncate(self.rank());
        self.coordinates.push(w);
        widened
    }

    /// Push `rows` in order, returning how many widened the span.
    pub fn push_rows<R: AsRef<[f64]>>(&mut self, rows: impl IntoIterator<Item = R>) -> usize {
        rows.into_iter()
            .filter(|row| self.push(row.as_ref()))
            .count()
    }

    /// The number of independent rows pushed.
    pub fn rank(&self) -> usize {
        self.reflectors.len()
    }

    /// An orthonormal basis of the vectors every pushed row annihilates —
    /// exactly `dim − rank` of them.
    pub fn null_space(&self) -> Vec<Vec<f64>> {
        (self.rank()..self.dim)
            .map(|j| {
                let mut e = vec![0.0; self.dim];
                e[j] = 1.0;
                self.apply_q(&mut e);
                e
            })
            .collect()
    }

    /// The minimum-norm `x` minimizing `‖A x − rhs‖`, where `A`'s rows are the
    /// pushed rows in push order (`rhs` has one entry per pushed row). Rows judged
    /// dependent count with their projection onto the span; `x` has no component
    /// in the [`null_space`](Self::null_space).
    pub fn least_squares(&self, rhs: &[f64]) -> Vec<f64> {
        assert_eq!(
            rhs.len(),
            self.coordinates.len(),
            "one right-hand side per pushed row"
        );
        let rank = self.rank();
        let columns: Vec<Vec<f64>> = (0..rank)
            .map(|k| {
                self.coordinates
                    .iter()
                    .map(|c| c.get(k).copied().unwrap_or(0.0))
                    .collect()
            })
            .collect();
        let mut x = solve_full_column_rank(columns, rhs.to_vec());
        x.resize(self.dim, 0.0);
        self.apply_q(&mut x);
        x
    }

    /// `y ← Qᵀ y`.
    fn apply_qt(&self, y: &mut [f64]) {
        for h in &self.reflectors {
            h.apply(y);
        }
    }

    /// `y ← Q y`.
    fn apply_q(&self, y: &mut [f64]) {
        for h in self.reflectors.iter().rev() {
            h.apply(y);
        }
    }
}

/// `a · b`.
pub fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The Euclidean norm of `a`.
pub fn norm(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}

/// `A v`, for `A` given by its rows.
pub fn mat_vec<R: AsRef<[f64]>>(rows: &[R], v: &[f64]) -> Vec<f64> {
    rows.iter().map(|row| dot(row.as_ref(), v)).collect()
}

/// `Aᵀ w`, for `A` given by its rows (of length `dim`).
pub fn mat_t_vec<R: AsRef<[f64]>>(rows: &[R], w: &[f64], dim: usize) -> Vec<f64> {
    let mut out = vec![0.0; dim];
    for (row, wi) in rows.iter().zip(w) {
        for (o, a) in out.iter_mut().zip(row.as_ref()) {
            *o += a * wi;
        }
    }
    out
}

/// A Householder reflector `H = I − β·v·vᵀ` acting on coordinates `offset..`.
#[derive(Debug, Clone)]
struct Reflector {
    offset: usize,
    v: Vec<f64>,
    beta: f64,
}

impl Reflector {
    /// The reflector at `offset` mapping the nonzero `x` onto `image·e₀`, with
    /// `|image| = ‖x‖` and the sign chosen against `x₀` so nothing cancels.
    fn annihilating(offset: usize, x: &[f64]) -> (Self, f64) {
        let alpha = norm(x);
        let sign = if x[0] >= 0.0 { 1.0 } else { -1.0 };
        let mut v = x.to_vec();
        v[0] += sign * alpha;
        let beta = 2.0 / dot(&v, &v);
        (Self { offset, v, beta }, -sign * alpha)
    }

    /// `y ← H y`.
    fn apply(&self, y: &mut [f64]) {
        let tail = &mut y[self.offset..];
        let s = self.beta * dot(&self.v, tail);
        for (t, vi) in tail.iter_mut().zip(&self.v) {
            *t -= s * vi;
        }
    }
}

/// The least-squares solution of `C y ≈ b` for a full-column-rank `C`, given as
/// its columns, by an unpivoted Householder QR of `C`.
fn solve_full_column_rank(mut columns: Vec<Vec<f64>>, mut b: Vec<f64>) -> Vec<f64> {
    let r = columns.len();
    for k in 0..r {
        let (reflector, image) = Reflector::annihilating(k, &columns[k][k..]);
        columns[k][k] = image;
        for column in &mut columns[k + 1..] {
            reflector.apply(column);
        }
        reflector.apply(&mut b);
    }
    let mut y = vec![0.0; r];
    for k in (0..r).rev() {
        let known: f64 = (k + 1..r).map(|j| columns[j][k] * y[j]).sum();
        y[k] = (b[k] - known) / columns[k][k];
    }
    y
}
