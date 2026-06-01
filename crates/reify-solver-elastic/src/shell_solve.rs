//! Flat-plate MITC3 cantilever shell driver (PRD
//! `docs/prds/v0_4/shell-extract-engine-bridge.md` task δ, §3/§5/§7).
//!
//! `solve_flat_plate_shell` synthesizes a structured triangulated mid-surface
//! mesh of an `length × width` rectangle in the XY plane (thickness `t`),
//! clamps the `x == 0` root edge, distributes a `-Z` tip load across the
//! `x == length` free edge, assembles + solves the MITC3 shell system, and
//! recovers per-element [`ShellElementStress`] + per-element [`ShellFrame`].
//!
//! This is the neutral-types kernel driver: it returns only solver-elastic
//! types (no `ShellChannels` — that glue lives in reify-eval, which depends on
//! this crate; see PRD §11 OQ-2 and the task δ design decisions). The recipe
//! is lifted from the proven end-to-end shell solve in
//! `tests/shell_benchmarks.rs` (the flat-plate cantilever sanity test).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitutive::IsotropicElastic;

    /// Local von Mises from a 3×3 Cauchy tensor (rotation-invariant, so it is
    /// correct to evaluate on the element-LOCAL stress without rotating to
    /// global first — see PRD §11 OQ-1 and the task δ design decision).
    fn von_mises(m: &[[f64; 3]; 3]) -> f64 {
        let (sxx, syy, szz) = (m[0][0], m[1][1], m[2][2]);
        let (sxy, syz, szx) = (m[0][1], m[1][2], m[2][0]);
        (0.5
            * ((sxx - syy).powi(2)
                + (syy - szz).powi(2)
                + (szz - sxx).powi(2)
                + 6.0 * (sxy * sxy + syz * syz + szx * szx)))
        .sqrt()
    }

    /// Max absolute component of a 3×3 tensor.
    fn max_abs(m: &[[f64; 3]; 3]) -> f64 {
        let mut mx = 0.0_f64;
        for row in m {
            for &v in row {
                mx = mx.max(v.abs());
            }
        }
        mx
    }

    /// RED (task δ step-1): pin the flat-plate MITC3 cantilever shell driver.
    ///
    /// Fixture mirrors `examples/fea_shell_flexure.ri`: a 50 mm × 10 mm × 1 mm
    /// steel flexure (E=205 GPa, ν=0.29) with a 10 N transverse tip load.
    ///
    /// # Accuracy basis (esc-3594-10 bare-MITC3 honest band)
    ///
    /// A flat-facet cantilever is the BENIGN MITC3 case: it has NO curvature,
    /// so none of the membrane-locking that drives the 1.7×–2200× errors on the
    /// curved MacNeal-Harder benchmarks (`shell_benchmarks.rs`) applies. The
    /// assertion is therefore a ONE-ORDER-OF-MAGNITUDE band [3e7, 3e9] Pa around
    /// the analytical σ = 6PL/(bh²) = 3e8 Pa — a 10× window, far wider than the
    /// flat-facet method error. No tight (5%) tolerance is asserted.
    #[test]
    fn flat_plate_shell_cantilever_top_von_mises_within_one_oom_of_analytical() {
        let length = 0.05_f64;
        let width = 0.01_f64;
        let thickness = 0.001_f64;
        let material = IsotropicElastic {
            youngs_modulus: 205e9,
            poisson_ratio: 0.29,
        };
        let tip_force = 10.0_f64;

        let solve = solve_flat_plate_shell(length, width, thickness, &material, tip_force);

        assert!(solve.converged, "flat-plate shell CG must converge");
        assert!(
            solve.iterations >= 1,
            "a cold-start CG solve must take at least one iteration, got {}",
            solve.iterations,
        );
        assert!(
            !solve.stresses.is_empty(),
            "driver must recover per-element stresses"
        );
        assert_eq!(
            solve.stresses.len(),
            solve.frames.len(),
            "exactly one local→global frame per element"
        );

        // Analytical reference: σ = 6PL/(bh²) = 6·10·0.05/(0.01·1e-6) = 3e8 Pa.
        let sigma_ref = 6.0 * tip_force * length / (width * thickness.powi(2));
        let lower = 0.1 * sigma_ref; // 3e7 Pa
        let upper = 10.0 * sigma_ref; // 3e9 Pa

        // Peak bending lives at the clamped root; assert max-over-elements (not a
        // fragile root-element index) of the .top layer von Mises.
        let max_top_vm = solve
            .stresses
            .iter()
            .map(|s| von_mises(&s.top))
            .fold(0.0_f64, f64::max);

        assert!(
            max_top_vm.is_finite(),
            "max top von Mises must be finite, got {max_top_vm}"
        );
        assert!(max_top_vm > 0.0, "max top von Mises must be > 0, got {max_top_vm}");
        assert!(
            (lower..=upper).contains(&max_top_vm),
            "max top von Mises {max_top_vm:.4e} Pa outside one-OOM band \
             [{lower:.1e}, {upper:.1e}] around σ=6PL/(bh²)={sigma_ref:.4e} Pa"
        );

        // Real through-thickness bending gradient: the top fibre must carry more
        // stress than the mid (neutral-plane) layer.
        let max_top_abs = solve
            .stresses
            .iter()
            .map(|s| max_abs(&s.top))
            .fold(0.0_f64, f64::max);
        let max_mid_abs = solve
            .stresses
            .iter()
            .map(|s| max_abs(&s.mid))
            .fold(0.0_f64, f64::max);
        assert!(
            max_top_abs > max_mid_abs,
            "expected a through-thickness bending gradient (max|top|={max_top_abs:.4e} \
             > max|mid|={max_mid_abs:.4e}); mid should sit near the neutral plane"
        );
    }
}

