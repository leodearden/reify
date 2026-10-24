//! Dense force-density linear algebra shared by the tensegrity kernels
//! ([`crate::form_find`], [`crate::form_find_free`],
//! [`crate::prestress_stability`]): the line-member matrix `D = CᵀQC` and the
//! one classifier of a real symmetric matrix's spectrum.

use faer::mat::MatRef;
use faer::{Mat, Side};

/// Relative null threshold: an eigenvalue is a null direction when
/// `|λ| ≤ NULLITY_REL_TOL · max|λ|`.
pub(crate) const NULLITY_REL_TOL: f64 = 1e-8;

/// Why a matrix has no dense symmetric spectrum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpectrumError {
    /// An entry is NaN or infinite: the first such entry in column-major order.
    NonFinite {
        /// Row of the entry.
        row: usize,
        /// Column of the entry.
        col: usize,
    },
    /// The dense self-adjoint eigendecomposition did not converge.
    NoConvergence,
}

/// The line-member force-density matrix `D = CᵀQC` (`n×n`): for each member
/// `(j, k)` with force density `qᵢ`, `+qᵢ` on the diagonal entries `D[j,j]`,
/// `D[k,k]` and `−qᵢ` on the off-diagonal pair `D[j,k]`, `D[k,j]`.
///
/// Endpoints must index real nodes: callers range-check the members first, and
/// debug builds assert it here, naming the offending member.
pub(crate) fn assemble_d(n: usize, members: &[(usize, usize)], q: &[f64]) -> Mat<f64> {
    let mut d = Mat::<f64>::zeros(n, n);
    for (&(j, k), &qi) in members.iter().zip(q.iter()) {
        debug_assert!(
            j < n && k < n,
            "member ({j}, {k}) indexes past the {n}×{n} force-density matrix; \
             ForceDensityProblem::new must run before assemble_d",
        );
        d[(j, j)] += qi;
        d[(k, k)] += qi;
        d[(j, k)] -= qi;
        d[(k, j)] -= qi;
    }
    d
}

/// The spectrum of a real symmetric matrix: its eigenpairs ascending by `|λ|`
/// with signs kept (a force-density matrix is indefinite), so the null
/// directions come first.
#[derive(Debug)]
pub(crate) struct SymmetricSpectrum {
    eigenvalues: Vec<f64>,
    /// Column `j` is the eigenvector of `eigenvalues[j]`.
    eigenvectors: Mat<f64>,
    max_mag: f64,
}

impl SymmetricSpectrum {
    /// The spectrum of the symmetric matrix `m`, from a dense self-adjoint
    /// eigendecomposition of its lower triangle.
    ///
    /// # Errors
    ///
    /// [`SpectrumError::NonFinite`] names `m`'s first NaN or infinite entry,
    /// checked here because faer's self-adjoint path does not check it;
    /// [`SpectrumError::NoConvergence`] when that eigendecomposition fails.
    ///
    /// # Panics
    ///
    /// If `m` is not square.
    pub(crate) fn of(m: &Mat<f64>) -> Result<SymmetricSpectrum, SpectrumError> {
        let n = m.nrows();
        assert_eq!(
            n,
            m.ncols(),
            "a symmetric spectrum needs a square matrix, got {n}×{}",
            m.ncols(),
        );
        if n == 0 {
            return Ok(SymmetricSpectrum {
                eigenvalues: Vec::new(),
                eigenvectors: Mat::zeros(0, 0),
                max_mag: 0.0,
            });
        }
        if let Some((row, col)) = first_non_finite_entry(m) {
            return Err(SpectrumError::NonFinite { row, col });
        }
        let eig = m
            .self_adjoint_eigen(Side::Lower)
            .map_err(|_| SpectrumError::NoConvergence)?;
        let s = eig.S();
        let u = eig.U();

        // faer returns ascending ALGEBRAIC order; reorder by |λ| so the null
        // directions lead.
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| s[a].abs().total_cmp(&s[b].abs()));

        let eigenvalues: Vec<f64> = order.iter().map(|&i| s[i]).collect();

        let mut eigenvectors = Mat::<f64>::zeros(n, n);
        for (new_col, &src_col) in order.iter().enumerate() {
            for r in 0..n {
                eigenvectors[(r, new_col)] = u[(r, src_col)];
            }
        }

        let max_mag = eigenvalues.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);

        Ok(SymmetricSpectrum {
            eigenvalues,
            eigenvectors,
            max_mag,
        })
    }

    /// Eigenvalues ascending by `|λ|`, signs kept.
    pub(crate) fn eigenvalues(&self) -> &[f64] {
        &self.eigenvalues
    }

    /// Largest eigenvalue magnitude `max|λ|` (`0` for an empty spectrum).
    pub(crate) fn max_mag(&self) -> f64 {
        self.max_mag
    }

    /// Count of eigenvalues with `|λ| ≤ rel_tol · max|λ|`.
    pub(crate) fn nullity(&self, rel_tol: f64) -> usize {
        let threshold = rel_tol * self.max_mag;
        self.eigenvalues
            .iter()
            .filter(|v| v.abs() <= threshold)
            .count()
    }

    /// Count of eigenvalues clear of the null threshold
    /// (see [`nullity`](Self::nullity)).
    pub(crate) fn rank(&self, rel_tol: f64) -> usize {
        self.eigenvalues.len() - self.nullity(rel_tol)
    }

    /// The eigenvectors of the `k` smallest-`|λ|` eigenvalues, as orthonormal
    /// columns.
    pub(crate) fn leading_eigenvectors(&self, k: usize) -> MatRef<'_, f64> {
        self.eigenvectors.subcols(0, k)
    }

    /// Orthonormal basis of the null space under `rel_tol`: the leading
    /// [`nullity`](Self::nullity) eigenvectors.
    pub(crate) fn null_basis(&self, rel_tol: f64) -> MatRef<'_, f64> {
        self.leading_eigenvectors(self.nullity(rel_tol))
    }
}

/// `m`'s first NaN or infinite entry in column-major order, as `(row, col)`.
fn first_non_finite_entry(m: &Mat<f64>) -> Option<(usize, usize)> {
    (0..m.ncols())
        .flat_map(|col| (0..m.nrows()).map(move |row| (row, col)))
        .find(|&(row, col)| !m[(row, col)].is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT_TOL: f64 = 1e-12;

    fn diag(values: &[f64]) -> Mat<f64> {
        let n = values.len();
        Mat::from_fn(n, n, |i, j| if i == j { values[i] } else { 0.0 })
    }

    fn assert_column_is_signed_axis(m: MatRef<'_, f64>, col: usize, axis: usize) {
        for r in 0..m.nrows() {
            let expected = if r == axis { 1.0 } else { 0.0 };
            assert!(
                (m[(r, col)].abs() - expected).abs() <= UNIT_TOL,
                "column {col} should be ±e{axis}, but entry {r} is {}",
                m[(r, col)],
            );
        }
    }

    #[test]
    fn assemble_d_scatters_each_member_as_a_rank_one_update() {
        let d = assemble_d(3, &[(0, 1), (1, 2)], &[2.0, 3.0]);
        let expected = [[2.0, -2.0, 0.0], [-2.0, 5.0, -3.0], [0.0, -3.0, 3.0]];
        assert_eq!((d.nrows(), d.ncols()), (3, 3));
        for (i, row) in expected.iter().enumerate() {
            for (j, &v) in row.iter().enumerate() {
                assert_eq!(d[(i, j)], v, "D[{i},{j}]");
            }
        }
    }

    #[test]
    fn spectrum_orders_an_indefinite_matrix_by_magnitude_keeping_signs() {
        let spectrum = SymmetricSpectrum::of(&diag(&[5.0, -2.0, 0.0])).expect("finite matrix");
        assert_eq!(spectrum.eigenvalues(), &[0.0, -2.0, 5.0]);
        assert_eq!(spectrum.max_mag(), 5.0);
        assert_eq!(spectrum.nullity(NULLITY_REL_TOL), 1);
        assert_eq!(spectrum.rank(NULLITY_REL_TOL), 2);

        let null = spectrum.null_basis(NULLITY_REL_TOL);
        assert_eq!(null.ncols(), 1);
        assert_column_is_signed_axis(null, 0, 2);

        let leading = spectrum.leading_eigenvectors(2);
        assert_eq!(leading.ncols(), 2);
        assert_column_is_signed_axis(leading, 0, 2);
        assert_column_is_signed_axis(leading, 1, 1);
    }

    #[test]
    fn nullity_and_rank_follow_the_relative_tolerance() {
        let spectrum = SymmetricSpectrum::of(&diag(&[1e-6, 1.0, 2.0])).expect("finite matrix");
        assert_eq!(spectrum.nullity(1e-5), 1);
        assert_eq!(spectrum.rank(1e-5), 2);
        assert_eq!(spectrum.nullity(1e-8), 0);
        assert_eq!(spectrum.rank(1e-8), 3);
    }

    #[test]
    fn a_nan_entry_is_a_typed_error_naming_its_position() {
        let mut m = Mat::<f64>::identity(3, 3);
        m[(1, 2)] = f64::NAN;
        assert_eq!(
            SymmetricSpectrum::of(&m).unwrap_err(),
            SpectrumError::NonFinite { row: 1, col: 2 },
        );
    }

    #[test]
    fn an_infinite_diagonal_entry_is_a_typed_error_of_either_sign() {
        for poison in [f64::INFINITY, f64::NEG_INFINITY] {
            let mut m = Mat::<f64>::identity(3, 3);
            m[(0, 0)] = poison;
            assert_eq!(
                SymmetricSpectrum::of(&m).unwrap_err(),
                SpectrumError::NonFinite { row: 0, col: 0 },
                "poison {poison}",
            );
        }
    }

    #[test]
    fn an_empty_matrix_has_an_empty_spectrum() {
        let spectrum = SymmetricSpectrum::of(&Mat::<f64>::zeros(0, 0)).expect("empty matrix");
        assert!(spectrum.eigenvalues().is_empty());
        assert_eq!(spectrum.nullity(NULLITY_REL_TOL), 0);
        assert_eq!(spectrum.rank(NULLITY_REL_TOL), 0);
        assert_eq!(spectrum.null_basis(NULLITY_REL_TOL).ncols(), 0);
    }
}
