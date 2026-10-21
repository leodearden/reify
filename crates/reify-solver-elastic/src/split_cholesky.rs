//! A sparse Cholesky factor `A = G·Gᵀ` whose two halves, `G⁻¹` and `G⁻ᵀ`,
//! can be applied SEPARATELY.
//!
//! faer's `Llt` keeps its `L` factor private and exposes only the composed
//! solve `A⁻¹ = G⁻ᵀ·G⁻¹`. A Cholesky-symmetrized Lanczos needs the two halves
//! on either side of an operator, so this type factors through the crate's
//! shared Cholesky driver (`linear_solve::direct`) and keeps the factor.
//!
//! Convention: faer factors `A = Pᵀ·L·Lᵀ·P` with a fill-reducing permutation
//! `P`, so `G = Pᵀ·L`, `G⁻¹·x = L⁻¹·(P·x)` and `G⁻ᵀ·y = Pᵀ·(L⁻ᵀ·y)`.

use faer::dyn_stack::{MemBuffer, MemStack};
use faer::mat::MatMut;
use faer::sparse::linalg::LltError;
use faer::sparse::linalg::cholesky::SymbolicCholeskyRaw;
use faer::sparse::linalg::cholesky::supernodal::SupernodalLltRef;
use faer::sparse::linalg::triangular_solve;
use faer::sparse::{FaerError, SparseColMatRef, SparseRowMat};
use faer::{Conj, Mat, Par};

use crate::linear_solve::direct::{self, Factor, FactorizeError};

/// The Cholesky factor `G` of a symmetric positive definite `A = G·Gᵀ`.
pub struct SplitCholesky {
    factor: Factor,
}

impl SplitCholesky {
    /// Factor `a`, reading its lower triangle.
    ///
    /// Fails exactly as `a.sp_cholesky(Side::Lower)` does: a non-positive
    /// pivot is `LltError::Numeric`, a resource failure `LltError::Generic`.
    pub fn try_new(a: &SparseRowMat<usize, f64>) -> Result<Self, LltError> {
        let a_csc = a.to_col_major()?;
        Self::try_new_col_major(a_csc.as_ref())
    }

    pub(crate) fn try_new_col_major(a: SparseColMatRef<'_, usize, f64>) -> Result<Self, LltError> {
        let symbolic = direct::analyse(a.symbolic())?;
        let values = direct::factorize(&symbolic, a).map_err(|error| match error {
            FactorizeError::OutOfMemory { .. } => LltError::Generic(FaerError::OutOfMemory),
            FactorizeError::NonPositivePivot { raw, .. } => LltError::Numeric(raw),
        })?;
        Ok(Self {
            factor: Factor::new(symbolic, values),
        })
    }

    /// The dimension of the factored matrix.
    pub fn n(&self) -> usize {
        self.factor.symbolic().nrows()
    }

    /// `out ← G⁻¹·out`, column by column.
    pub fn solve_factor_in_place(&self, mut out: MatMut<'_, f64>) {
        assert_eq!(out.nrows(), self.n(), "SplitCholesky: rhs row count");
        match self.factor.symbolic().perm() {
            Some(perm) => {
                let fwd = perm.arrays().0;
                let mut permuted = Mat::from_fn(out.nrows(), out.ncols(), |i, j| out[(fwd[i], j)]);
                self.solve_l_in_place(permuted.as_mut());
                out.copy_from(&permuted);
            }
            None => self.solve_l_in_place(out),
        }
    }

    /// `out ← G⁻ᵀ·out`, column by column.
    pub fn solve_factor_transpose_in_place(&self, mut out: MatMut<'_, f64>) {
        assert_eq!(out.nrows(), self.n(), "SplitCholesky: rhs row count");
        match self.factor.symbolic().perm() {
            Some(perm) => {
                let inv = perm.arrays().1;
                let mut solved = out.to_owned();
                self.solve_l_transpose_in_place(solved.as_mut());
                for j in 0..out.ncols() {
                    for (i, &src) in inv.iter().enumerate() {
                        out[(i, j)] = solved[(src, j)];
                    }
                }
            }
            None => self.solve_l_transpose_in_place(out),
        }
    }

    fn solve_l_in_place(&self, rhs: MatMut<'_, f64>) {
        let values = self.factor.values();
        match self.factor.symbolic().raw() {
            SymbolicCholeskyRaw::Simplicial(s) => {
                triangular_solve::solve_lower_triangular_in_place(
                    SparseColMatRef::new(s.factor(), values),
                    Conj::No,
                    rhs,
                    Par::Seq,
                )
            }
            SymbolicCholeskyRaw::Supernodal(s) => {
                let mut scratch =
                    MemBuffer::new(s.solve_in_place_scratch::<f64>(rhs.ncols(), Par::Seq));
                SupernodalLltRef::new(s, values).l_solve_with_conj(
                    Conj::No,
                    rhs,
                    Par::Seq,
                    MemStack::new(&mut scratch),
                );
            }
        }
    }

    fn solve_l_transpose_in_place(&self, rhs: MatMut<'_, f64>) {
        let values = self.factor.values();
        match self.factor.symbolic().raw() {
            SymbolicCholeskyRaw::Simplicial(s) => {
                triangular_solve::solve_lower_triangular_transpose_in_place(
                    SparseColMatRef::new(s.factor(), values),
                    Conj::No,
                    rhs,
                    Par::Seq,
                )
            }
            SymbolicCholeskyRaw::Supernodal(s) => {
                let mut scratch =
                    MemBuffer::new(s.solve_in_place_scratch::<f64>(rhs.ncols(), Par::Seq));
                SupernodalLltRef::new(s, values).l_transpose_solve_with_conj(
                    Conj::No,
                    rhs,
                    Par::Seq,
                    MemStack::new(&mut scratch),
                );
            }
        }
    }
}
