//! Element-level stiffness assembly for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #8. This module
//! ships the per-element stiffness assembly machinery — the dense
//! `K_e = ∫_Ω_e BᵀDB dV` integrand — for both P1 and P2 tetrahedra. Global
//! sparse-matrix assembly via faer-rs is PRD task #9's job and consumes
//! [`ElementStiffness`] row-major.
//!
//! Mixed-element (tet + shell) global assembly was added per PRD
//! `docs/prds/v0_4/structural-analysis-shells.md` task T11; the
//! `assemble_global_stiffness` entry point is now D-agnostic, deriving each
//! element's `dofs_per_node` from `k_e.n_dofs / connectivity.len()`.

pub mod global;
pub mod hex;
pub mod tet;
pub mod wedge;

pub use global::{
    AssemblyElement, AssemblyMode, OrphanDofsSummary, assemble_global_stiffness,
    detect_orphan_dofs,
};

#[cfg(test)]
pub(crate) mod test_support;

use crate::constitutive::IsotropicElastic;

/// Tetrahedral element interpolation order — P1 (linear, 4 nodes) or
/// P2 (quadratic, 10 nodes).
///
/// This is the **Rust-side** order enum local to `reify-solver-elastic`.
/// Bridging to the stdlib-side `ElementOrder` enum (in
/// `crates/reify-compiler/stdlib/solver_elastic.ri`) is PRD task #16's
/// job and lives in the engine-integration layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementOrder {
    /// 4-node linear (P1) tetrahedron; `n_dofs = 12`.
    P1,
    /// 10-node quadratic (P2) tetrahedron with edge-midpoint nodes;
    /// `n_dofs = 30`.
    P2,
}

/// Public dispatch entry point: compute the element-stiffness matrix
/// for a single tetrahedron of the given interpolation order.
///
/// Routes to [`tet::element_stiffness_p1`] for `ElementOrder::P1`
/// (requires `phys_nodes.len() == 4`) or [`tet::element_stiffness_p2`]
/// for `ElementOrder::P2` (requires `phys_nodes.len() == 10`).
///
/// The slice-shaped signature lets a single dispatcher accept either
/// node count without const-generic gymnastics. Length is checked at
/// runtime; a mismatch panics with a descriptive message naming the
/// expected `ElementOrder` variant and the observed length.
///
/// # Panics
///
/// Panics if `phys_nodes.len()` does not match the expected node count
/// for `order` (4 for `P1`, 10 for `P2`).
pub fn element_stiffness(
    order: ElementOrder,
    phys_nodes: &[[f64; 3]],
    material: &IsotropicElastic,
) -> ElementStiffness {
    match order {
        ElementOrder::P1 => {
            assert_eq!(
                phys_nodes.len(),
                4,
                "ElementOrder::P1 requires 4 phys_nodes, got {}",
                phys_nodes.len(),
            );
            // Length checked above, so the conversion is infallible.
            let arr: &[[f64; 3]; 4] = phys_nodes
                .try_into()
                .expect("phys_nodes.len() == 4 just asserted");
            tet::element_stiffness_p1(arr, material)
        }
        ElementOrder::P2 => {
            assert_eq!(
                phys_nodes.len(),
                10,
                "ElementOrder::P2 requires 10 phys_nodes, got {}",
                phys_nodes.len(),
            );
            let arr: &[[f64; 3]; 10] = phys_nodes
                .try_into()
                .expect("phys_nodes.len() == 10 just asserted");
            tet::element_stiffness_p2(arr, material)
        }
    }
}

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
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::assembly::test_support::{dimensionless_steel_like, scaled_p2_phys_nodes};

    /// Canonical 4-node P1 phys layout (unit reference tet).
    const UNIT_TET_P1_NODES: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    #[test]
    fn dispatch_p1_matches_direct_p1_call_bit_for_bit() {
        let mat = dimensionless_steel_like();
        let direct = tet::element_stiffness_p1(&UNIT_TET_P1_NODES, &mat);
        let dispatched = element_stiffness(ElementOrder::P1, &UNIT_TET_P1_NODES[..], &mat);
        assert_eq!(dispatched.n_dofs, 12);
        assert_eq!(dispatched.data.len(), 144);
        // Bit-for-bit match: same inputs through the same generic helper
        // means the floating-point operations are identical.
        assert_eq!(dispatched.data, direct.data);
    }

    #[test]
    fn dispatch_p2_matches_direct_p2_call_bit_for_bit() {
        let mat = dimensionless_steel_like();
        let phys = scaled_p2_phys_nodes(1.0);
        let direct = tet::element_stiffness_p2(&phys, &mat);
        let dispatched = element_stiffness(ElementOrder::P2, &phys[..], &mat);
        assert_eq!(dispatched.n_dofs, 30);
        assert_eq!(dispatched.data.len(), 900);
        assert_eq!(dispatched.data, direct.data);
    }

    #[test]
    #[should_panic(expected = "P1")]
    fn dispatch_p1_with_10_node_slice_panics() {
        let mat = dimensionless_steel_like();
        let phys = scaled_p2_phys_nodes(1.0);
        let _ = element_stiffness(ElementOrder::P1, &phys[..], &mat);
    }

    #[test]
    #[should_panic(expected = "P2")]
    fn dispatch_p2_with_4_node_slice_panics() {
        let mat = dimensionless_steel_like();
        let _ = element_stiffness(ElementOrder::P2, &UNIT_TET_P1_NODES[..], &mat);
    }

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
