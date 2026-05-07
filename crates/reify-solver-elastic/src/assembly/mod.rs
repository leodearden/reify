//! Element-level stiffness assembly for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #8. This module
//! ships the per-element stiffness assembly machinery — the dense
//! `K_e = ∫_Ω_e BᵀDB dV` integrand — for both P1 and P2 tetrahedra. Global
//! sparse-matrix assembly via faer-rs is PRD task #9's job and consumes
//! [`ElementStiffness`] row-major.

pub mod tet;

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
