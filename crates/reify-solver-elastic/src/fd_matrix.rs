//! Dense force-density linear algebra shared by the tensegrity kernels
//! ([`crate::form_find`], [`crate::form_find_free`],
//! [`crate::prestress_stability`]): the line-member matrix `D = CᵀQC` and the
//! one classifier of a real symmetric matrix's spectrum.

#[cfg(test)]
mod tests {
    use super::*;
    use faer::Mat;
    use faer::mat::MatRef;

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
