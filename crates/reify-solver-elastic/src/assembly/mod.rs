//! Element-level stiffness assembly for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #8. This module
//! ships the per-element stiffness assembly machinery — the dense
//! `K_e = ∫_Ω_e BᵀDB dV` integrand — for both P1 and P2 tetrahedra. Global
//! sparse-matrix assembly via faer-rs is PRD task #9's job and consumes
//! [`ElementStiffness`] row-major.

pub mod tet;

/// A dense, square element-stiffness matrix `K_e` of size `n_dofs × n_dofs`.
///
/// The DOF index is `3 · node_idx + axis` (node-major, axis-minor; `axis ∈
/// {0, 1, 2}` for `(u_x, u_y, u_z)`). For a P1 tet (4 nodes) `n_dofs = 12`;
/// for a P2 tet (10 nodes) `n_dofs = 30`.
///
/// # Storage
///
/// Backing `Vec<f64>` of length `n_dofs²`, indexed **row-major**:
/// `data[i * n_dofs + j]` is the `(i, j)` entry. This is one heap
/// allocation per element (vs `n_dofs + 1` for a nested `Vec<Vec<f64>>`)
/// and is the layout faer-rs's CSR builders expect — PRD task #9 (global
/// sparse assembly) reads row-major slices directly without any transpose
/// step.
///
/// # Symmetry
///
/// `K_e = ∫ BᵀDB dV` is symmetric whenever `D` is symmetric (which the
/// isotropic-elastic D matrix is by construction). We store the full dense
/// matrix anyway — symmetric-only storage saves 50% memory but doubles
/// the index-arithmetic cost on every `get` and breaks the row-major
/// expectation of consumers. For a 30×30 P2 element the difference is
/// 7200 bytes vs 3600 bytes; assembly memory is dominated by the global
/// sparse matrix (task #9), not the per-element dense buffers.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementStiffness {
    /// Number of DOFs (rows = columns).
    pub n_dofs: usize,
    /// Flat row-major storage of length `n_dofs²`.
    pub data: Vec<f64>,
}

impl ElementStiffness {
    /// Construct an `n_dofs × n_dofs` zero matrix.
    pub fn zeros(n_dofs: usize) -> Self {
        Self {
            n_dofs,
            data: vec![0.0; n_dofs * n_dofs],
        }
    }

    /// Read the `(i, j)` entry. Row-major: `data[i * n_dofs + j]`.
    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.data[i * self.n_dofs + j]
    }

    /// Accumulate `v` into the `(i, j)` entry. Used by the assembly inner
    /// loop to add `(BᵀDB)_{ij} · |det J| · w` contributions per
    /// quadrature point.
    pub(crate) fn add(&mut self, i: usize, j: usize, v: f64) {
        self.data[i * self.n_dofs + j] += v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeros_constructs_n_by_n_dense_with_n_squared_storage() {
        let k = ElementStiffness::zeros(12);
        assert_eq!(k.n_dofs, 12);
        assert_eq!(k.data.len(), 144);
        for v in &k.data {
            assert_eq!(*v, 0.0);
        }
    }

    #[test]
    fn get_reads_row_major_entries() {
        // Build a 4×4 with distinct entries and verify get() reads
        // them in row-major order: data[i*n + j].
        let mut k = ElementStiffness::zeros(4);
        for idx in 0..16 {
            k.data[idx] = idx as f64;
        }
        for i in 0..4 {
            for j in 0..4 {
                assert_eq!(k.get(i, j), (i * 4 + j) as f64);
            }
        }
    }

    #[test]
    fn round_trip_via_direct_data_access_p1_size() {
        // n_dofs = 12 (P1: 4 nodes × 3 axes).
        let mut k = ElementStiffness::zeros(12);
        for i in 0..12 {
            for j in 0..12 {
                let v = (i * 13 + j) as f64;
                k.data[i * 12 + j] = v;
            }
        }
        for i in 0..12 {
            for j in 0..12 {
                let expected = (i * 13 + j) as f64;
                assert_eq!(k.get(i, j), expected, "({i},{j})");
            }
        }
    }

    #[test]
    fn round_trip_via_direct_data_access_p2_size() {
        // n_dofs = 30 (P2: 10 nodes × 3 axes).
        let mut k = ElementStiffness::zeros(30);
        assert_eq!(k.data.len(), 900);
        for i in 0..30 {
            for j in 0..30 {
                let v = (i * 31 + j) as f64;
                k.data[i * 30 + j] = v;
            }
        }
        for i in 0..30 {
            for j in 0..30 {
                let expected = (i * 31 + j) as f64;
                assert_eq!(k.get(i, j), expected, "({i},{j})");
            }
        }
    }
}
