#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::direction::DIRECTION_DEGENERACY_FLOOR;
    use std::f64::consts::FRAC_1_SQRT_2;

    fn bits(u: UnitVec3) -> [u64; 3] {
        u.components().map(f64::to_bits)
    }

    fn unit(v: [f64; 3]) -> UnitVec3 {
        UnitVec3::from_vector(v).expect("test vector is non-degenerate")
    }

    fn assert_close(got: [f64; 3], want: [f64; 3], tol: f64) {
        for i in 0..3 {
            assert!(
                (got[i] - want[i]).abs() <= tol,
                "component {i}: got {got:?}, want {want:?} (tol {tol:e})"
            );
        }
    }

    fn norm(c: [f64; 3]) -> f64 {
        (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
    }

    /// splitmix64: a fixed-seed generator so the sample is reproducible
    /// without a dependency.
    struct SplitMix64(u64);

    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        /// Uniform in [-1, 1).
        fn symmetric(&mut self) -> f64 {
            let unit_interval = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
            2.0 * unit_interval - 1.0
        }
    }

    const SAMPLE_SIZE: usize = 200_000;

    /// `SAMPLE_SIZE` non-degenerate vectors with components uniform in [-1, 1];
    /// every third one is rescaled by 10^k, k in [-10, 10].
    fn seeded_sample() -> Vec<[f64; 3]> {
        let mut rng = SplitMix64(0x5EED_D1EC_7104_0001);
        let mut out = Vec::with_capacity(SAMPLE_SIZE);
        let mut i = 0usize;
        while out.len() < SAMPLE_SIZE {
            let mut v = [rng.symmetric(), rng.symmetric(), rng.symmetric()];
            if i % 3 == 0 {
                let k = (rng.next_u64() % 21) as i32 - 10;
                let scale = 10f64.powi(k);
                v = v.map(|c| c * scale);
            }
            i += 1;
            if norm(v) > DIRECTION_DEGENERACY_FLOOR {
                out.push(v);
            }
        }
        out
    }

    #[test]
    fn normalises_a_non_unit_vector() {
        assert_eq!(unit([0.0, 0.0, 2.0]).components(), [0.0, 0.0, 1.0]);
        assert_close(unit([3.0, 0.0, 4.0]).components(), [0.6, 0.0, 0.8], 1e-15);
    }

    #[test]
    fn zero_vector_is_degenerate() {
        assert_eq!(
            UnitVec3::from_vector([0.0; 3]),
            Err(DirectionFault::Degenerate { magnitude: 0.0 })
        );
    }

    #[test]
    fn floor_boundary() {
        for v in [[1e-13, 0.0, 0.0], [DIRECTION_DEGENERACY_FLOOR, 0.0, 0.0]] {
            assert!(
                matches!(
                    UnitVec3::from_vector(v),
                    Err(DirectionFault::Degenerate { .. })
                ),
                "{v:?} is at or below the floor, so it must be Degenerate"
            );
        }
        assert_eq!(unit([2e-12, 0.0, 0.0]).components(), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn non_finite_component_is_non_finite() {
        for v in [
            [f64::NAN, 0.0, 0.0],
            [f64::INFINITY, 0.0, 0.0],
            [0.0, f64::NEG_INFINITY, 1.0],
        ] {
            assert_eq!(
                UnitVec3::from_vector(v),
                Err(DirectionFault::NonFinite),
                "{v:?}"
            );
        }
    }

    #[test]
    fn huge_finite_vector_normalises() {
        let u = UnitVec3::from_vector([1e300, 1e300, 0.0])
            .expect("finite components overflow only in the squared sum");
        assert_close(u.components(), [FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0], 1e-15);
    }

    #[test]
    fn from_vector_is_bit_idempotent() {
        let fixed = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [0.6, 0.8, 0.0],
            [1.0, 2.0, 3.0],
        ];
        for v in fixed {
            let u = unit(v);
            assert_eq!(bits(unit(u.components())), bits(u), "{v:?}");
        }

        let mut failures = 0usize;
        let mut first: Option<([f64; 3], [f64; 3], [f64; 3])> = None;
        for v in seeded_sample() {
            let u = unit(v);
            let again = unit(u.components());
            if bits(again) != bits(u) {
                failures += 1;
                first.get_or_insert((v, u.components(), again.components()));
            }
        }
        assert_eq!(
            failures, 0,
            "from_vector is not a fixed point on {failures} of {SAMPLE_SIZE} vectors; \
             first (input, once, twice): {first:?}"
        );
    }

    #[test]
    fn output_is_unit_to_four_ulp() {
        for v in seeded_sample() {
            let c = unit(v).components();
            assert!(
                (norm(c) - 1.0).abs() <= 4.0 * f64::EPSILON,
                "from_vector({v:?}) = {c:?} has magnitude {}",
                norm(c)
            );
        }
    }

    #[test]
    fn negative_zero_is_canonicalised() {
        assert_eq!(bits(unit([-0.0, -0.0, 1.0])), bits(unit([0.0, 0.0, 1.0])));
        assert_eq!(
            bits(unit([-0.0, 0.0, -2.0])),
            [0.0f64.to_bits(), 0.0f64.to_bits(), (-1.0f64).to_bits()]
        );
    }

    #[test]
    fn negated_is_exact_and_canonical() {
        let minus_z = unit([0.0, 0.0, 1.0]).negated();
        assert_eq!(bits(minus_z), bits(unit([0.0, 0.0, -1.0])));
        assert!(
            minus_z.components().iter().all(|c| c.to_bits() != (-0.0f64).to_bits()),
            "negated() must not leave a -0.0 slot: {:?}",
            minus_z.components()
        );

        assert_eq!(
            bits(unit([0.6, 0.8, 0.0]).negated()),
            [(-0.6f64).to_bits(), (-0.8f64).to_bits(), 0.0f64.to_bits()]
        );

        for v in seeded_sample().into_iter().take(1000) {
            let u = unit(v);
            assert_eq!(bits(u.negated().negated()), bits(u), "{v:?}");
            assert_eq!(bits(unit(u.negated().components())), bits(u.negated()), "{v:?}");
        }
    }

    #[test]
    fn degenerate_fault_reports_its_magnitude() {
        let fault = UnitVec3::from_vector([1e-13, 0.0, 0.0]).unwrap_err();
        let DirectionFault::Degenerate { magnitude } = fault else {
            panic!("expected Degenerate, got {fault:?}");
        };
        assert!((magnitude - 1e-13).abs() <= 1e-25, "magnitude {magnitude:e}");

        let message = fault.to_string();
        assert!(message.contains(&format!("{magnitude:e}")), "{message}");
        assert!(
            message.contains(&format!("{DIRECTION_DEGENERACY_FLOOR:e}")),
            "{message}"
        );
        assert!(DirectionFault::NonFinite.to_string().contains("non-finite"));
    }
}
