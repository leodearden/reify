#[cfg(test)]
mod tests {
    use super::*;

    const TWO_POW_20: f64 = 1_048_576.0;

    /// Chain 0 —soft— 1 —stiff— 2: the force-density matrix of two members.
    fn chain_d(soft: f64, stiff: f64) -> Mat<f64> {
        let mut d = Mat::<f64>::zeros(3, 3);
        for (a, b, q) in [(0, 1, soft), (1, 2, stiff)] {
            d[(a, a)] += q;
            d[(b, b)] += q;
            d[(a, b)] -= q;
            d[(b, a)] -= q;
        }
        d
    }

    /// Nodes 1 and 2 coincide, so the stiff member is balanced; node 0 sits
    /// 2^-20 off along x, so the soft one is not.
    fn chain_nodes() -> Vec<[f64; 3]> {
        vec![
            [1.0 + 1.0 / TWO_POW_20, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ]
    }

    #[test]
    fn an_empty_free_set_is_not_at_equilibrium() {
        let mut d = Mat::<f64>::zeros(2, 2);
        d[(1, 0)] = 8.5;
        let nodes = [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]];
        let residual = StiffnessScale::of(&d, RowSet::Free(&[])).relative_residual(&nodes);
        assert_eq!(residual, f64::INFINITY);
    }

    #[test]
    fn an_all_zero_judged_row_is_not_at_equilibrium() {
        // A node touched by neither a member nor a triangle: its net force is
        // vacuously 0, which must not read as converged.
        let d = Mat::<f64>::zeros(2, 2);
        let nodes = [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]];
        let residual = StiffnessScale::of(&d, RowSet::Free(&[1])).relative_residual(&nodes);
        assert_eq!(residual, f64::INFINITY);
    }

    #[test]
    fn a_nan_judged_row_is_not_at_equilibrium_beside_a_healthy_one() {
        // `f64::max` is NaN-transparent, so a guard on the max over rows would
        // land on the healthy row's 8.5 and miss the NaN row: it is per row.
        let mut d = Mat::<f64>::zeros(3, 3);
        d[(1, 0)] = 8.5;
        d[(2, 0)] = f64::NAN;
        let nodes = [[1.0, 1.0, 1.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let residual = StiffnessScale::of(&d, RowSet::Free(&[1, 2])).relative_residual(&nodes);
        assert_eq!(residual, f64::INFINITY);
    }

    #[test]
    fn a_nan_net_force_on_a_healthy_row_is_not_at_equilibrium() {
        // Row 1 is finite (Σ|D_1j| = 17), but node 1 sits at a NaN coordinate.
        let mut d = Mat::<f64>::zeros(3, 3);
        d[(1, 0)] = 8.5;
        d[(1, 1)] = -8.5;
        let nodes = [[1.0, 1.0, 1.0], [f64::NAN, 2.0, 3.0], [4.0, 5.0, 6.0]];
        let residual = StiffnessScale::of(&d, RowSet::Free(&[1])).relative_residual(&nodes);
        assert_eq!(residual, f64::INFINITY);
    }

    #[test]
    fn only_the_row_set_is_judged() {
        // Row 0 (an anchor's, in the Free case) is all-zero; row 1 is healthy
        // and balanced.
        let mut d = Mat::<f64>::zeros(2, 2);
        d[(1, 0)] = -8.5;
        d[(1, 1)] = 8.5;
        let nodes = [[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]];
        assert_eq!(
            StiffnessScale::of(&d, RowSet::Free(&[1])).relative_residual(&nodes),
            0.0
        );
        assert_eq!(
            StiffnessScale::of(&d, RowSet::All).relative_residual(&nodes),
            f64::INFINITY
        );
    }

    #[test]
    fn each_row_is_judged_against_its_own_stiffness() {
        // The soft row's imbalance 2^-20 over its own Σ|D_0j| = 2 is 2^-21; over
        // the stiff row's 2 + 2^21 it would be ~5e-13, below both kernels'
        // 1e-11 stop tolerance.
        let nodes = chain_nodes();
        let residual =
            StiffnessScale::of(&chain_d(1.0, TWO_POW_20), RowSet::All).relative_residual(&nodes);
        let max_abs_x = 1.0 + 1.0 / TWO_POW_20;
        assert_eq!(residual, (1.0 / TWO_POW_20 / 2.0) / (1.0 + max_abs_x));
        assert!(residual > 1e-11);
    }

    #[test]
    fn the_residual_is_exactly_invariant_under_a_uniform_rescale_of_d() {
        let nodes = chain_nodes();
        let d = chain_d(1.0, TWO_POW_20);
        let base = StiffnessScale::of(&d, RowSet::All).relative_residual(&nodes);
        for lambda in [TWO_POW_20, 1.0 / TWO_POW_20] {
            let scaled = Mat::<f64>::from_fn(3, 3, |i, j| d[(i, j)] * lambda);
            let residual = StiffnessScale::of(&scaled, RowSet::All).relative_residual(&nodes);
            assert_eq!(residual, base, "λ = {lambda:e}");
        }
    }

    #[test]
    fn the_stop_test_is_the_guard_form_at_the_geometrys_own_scale() {
        let nodes = chain_nodes();
        let d = chain_d(1.0, TWO_POW_20);
        let stiffness = StiffnessScale::of(&d, RowSet::All);
        let max_abs_x = 1.0 + 1.0 / TWO_POW_20;
        assert_eq!(max_abs_coordinate(&nodes), max_abs_x);
        assert_eq!(
            stiffness.relative_residual_against(&nodes, max_abs_x),
            stiffness.relative_residual(&nodes),
        );
    }
}
