// Step-1 stub: no implementation — only the test module so that
// `cargo check --tests` reports undefined `DirichletBc` and
// `apply_dirichlet_row_elimination` (RED).

#[cfg(test)]
mod tests {
    use super::{DirichletBc, apply_dirichlet_row_elimination};

    use faer::sparse::SparseRowMat;

    use crate::assembly::{
        AssemblyElement, AssemblyMode, assemble_global_stiffness,
    };
    use crate::assembly::tet::element_stiffness_p1;
    use crate::constitutive::IsotropicElastic;

    /// Steel-like dimensionless material reused across boundary tests.
    fn dimensionless_steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        }
    }

    /// Canonical 4-node P1 phys layout (unit reference tet).
    const UNIT_TET_P1_NODES: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    /// Read entry `(i, j)` of a `SparseRowMat<usize, f64>` as a plain `f64`,
    /// returning `0.0` if the entry is not stored.
    fn read(k: &SparseRowMat<usize, f64>, i: usize, j: usize) -> f64 {
        k.get(i, j).copied().unwrap_or(0.0)
    }

    /// Empty BC list → K and f are bit-identical to their pre-call snapshots.
    ///
    /// Pins the no-op contract: passing `bcs = &[]` must be a perfect
    /// identity operation — no stored value in K is touched, no `f[j]`
    /// changes.  Regression guard for future refactors that, for example,
    /// allocate and write a scratch buffer unconditionally.
    #[test]
    fn apply_dirichlet_bcs_with_empty_slice_leaves_k_and_f_unchanged() {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1_NODES, &mat);
        let connectivity = [0usize, 1, 2, 3];
        let element = AssemblyElement {
            id: 0,
            connectivity: &connectivity,
            k_e: &k_e,
        };
        let mut k = assemble_global_stiffness(4, &[element], AssemblyMode::Deterministic);
        let mut f: Vec<f64> = (0..12).map(|i| i as f64).collect();

        // Snapshot K (densified) and f before the call.
        let k_before: Vec<Vec<f64>> = (0..12)
            .map(|i| (0..12).map(|j| read(&k, i, j)).collect())
            .collect();
        let f_before = f.clone();

        // Apply empty BC list — must be a no-op.
        apply_dirichlet_row_elimination(&mut k, &mut f, &[]);

        // Verify bit-exact identity.
        for i in 0..12 {
            for j in 0..12 {
                let after = read(&k, i, j);
                assert_eq!(
                    after.to_bits(),
                    k_before[i][j].to_bits(),
                    "K[{i}][{j}] changed after empty-BC call: was {}, now {}",
                    k_before[i][j],
                    after,
                );
            }
            assert_eq!(
                f[i].to_bits(),
                f_before[i].to_bits(),
                "f[{i}] changed after empty-BC call: was {}, now {}",
                f_before[i],
                f[i],
            );
        }
    }
}
