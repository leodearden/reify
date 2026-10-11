//! The validated stiffness matrix every tier of the seam solves against.

use std::fmt;

use faer::sparse::SparseRowMat;

use crate::sparse_util::find_in_row;

/// A post-boundary-condition stiffness `K`, both triangles stored, that
/// passed [`SpdSystem::new`]'s structural checks.
pub struct SpdSystem {
    k: SparseRowMat<usize, f64>,
}

/// Why a matrix is not a valid [`SpdSystem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemInvalid {
    NotSquare {
        rows: usize,
        cols: usize,
    },
    /// Some row is stored non-compressed, unsorted, or with a repeated column.
    NonCanonical,
    /// Row `dof` (the lowest such row) has no stored diagonal entry `> 0`.
    NonPositiveDiagonal {
        dof: usize,
    },
}

impl SpdSystem {
    /// Validates `k`: square, then canonical CSR (compressed storage, column
    /// indices strictly increasing in every row), then a stored positive
    /// diagonal on every row. The first failing check is reported.
    pub fn new(k: SparseRowMat<usize, f64>) -> Result<Self, SystemInvalid> {
        let (rows, cols) = (k.nrows(), k.ncols());
        if rows != cols {
            return Err(SystemInvalid::NotSquare { rows, cols });
        }
        let k_ref = k.as_ref();
        let symbolic = k_ref.symbolic();
        let row_ptr = symbolic.row_ptr();
        let col_idx = symbolic.col_idx();
        let row = |i: usize| row_ptr[i]..row_ptr[i + 1];
        let strictly_increasing = |i: usize| col_idx[row(i)].windows(2).all(|w| w[0] < w[1]);
        if symbolic.row_nnz().is_some() || !(0..rows).all(strictly_increasing) {
            return Err(SystemInvalid::NonCanonical);
        }
        let vals = k_ref.val();
        let positive_diagonal = |i: usize| {
            find_in_row(col_idx, row_ptr[i], row_ptr[i + 1], i).is_some_and(|slot| vals[slot] > 0.0)
        };
        if let Some(dof) = (0..rows).find(|&i| !positive_diagonal(i)) {
            return Err(SystemInvalid::NonPositiveDiagonal { dof });
        }
        Ok(Self { k })
    }

    pub fn n_dofs(&self) -> usize {
        self.k.nrows()
    }
}

impl fmt::Display for SystemInvalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSquare { rows, cols } => {
                write!(f, "stiffness matrix must be square, but is {rows}×{cols}")
            }
            Self::NonCanonical => write!(
                f,
                "stiffness matrix must be canonical CSR (compressed, column indices strictly \
                 increasing in every row)"
            ),
            Self::NonPositiveDiagonal { dof } => write!(
                f,
                "stiffness matrix must store a positive diagonal on every row, but row {dof} does not"
            ),
        }
    }
}

impl std::error::Error for SystemInvalid {}
