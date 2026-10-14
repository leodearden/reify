//! Integration golden for Tensegrity free-standing combined-surfaces GAUGE
//! covariance (task 6413) — the free-standing twin of task 6119's anchored-path
//! gauge lock.
//!
//! PRD reference: `docs/prds/v0_6/tensegrity-structures.md` Tier-1 leaf T1b /
//! `docs/prds/v0_6/tensegrity-membrane.md` §4 M1b / D3 (δ).
//!
//! # The user-observable signal
//!
//! `D_combined = CᵀQC + Σ_T σ_T·L_T` is exactly linear in the force densities
//! `q` and the surface stresses `σ`, so a uniform gauge change `q → λ·q`,
//! `σ → λ·σ` at fixed geometry scales `D_combined` by `λ` entrywise and
//! therefore does not change the physical equilibrium — [`form_find_free_surfaces`]
//! must converge to the SAME geometry (and report the SAME nullity) regardless
//! of `λ`. Before task 6413's fix, the absolute equilibrium-residual stop test
//! made convergence gauge-DEPENDENT: a large `λ` scaled the raw residual past
//! the absolute tolerance, so the fixed point exhausted its iteration budget
//! and returned `Err(SearchDidNotConverge)` for a structure that is physically
//! identical to one that converges fine at `λ = 1`.
//!
//! This test drives [`form_find_free_surfaces`] through its **public
//! crate-root surface** only — it never reaches into the private per-node
//! residual / descent-step internals (those are covered by the crate's own
//! unit tests in `form_find_free.rs`, including the descent-step gauge
//! covariance that has no converging e2e altitude — see that module's tests
//! for why).
//!
//! # Fixture — SELF-DERIVING, no magic constants
//!
//! `λ = 1` and `λ = 2^20` are re-solved from a common baseline `(x*, q*)`
//! obtained by first running the existing δ `GroupRatios` golden (the same
//! fixture as `tensegrity_delta_combined_form_find.rs`) to convergence. `λ` is
//! a power of two so the gauge change is exact in IEEE-754 (an exponent-field
//! shift, no mantissa rounding). The fixed-point-start golden below checks
//! this bit-exactly (`assert_eq!`); the perturbed-guess grid relaxes to a
//! tight relative bound instead — see that test's own doc for why.

use reify_solver_elastic::{
    ForceDensitySpec, FreeFormResult, MemberKind, form_find_free_surfaces,
};

mod common;
use common::{
    analytic_combined_q, perturbed_prism_guess, perturbed_prism_guess_scaled, prism_surfaces,
    triplex_group_ids, triplex_topology,
};

// ---------------------------------------------------------------------------
// Gauge covariance golden
// ---------------------------------------------------------------------------

/// Re-solve the combined prism+membrane problem through the `Explicit` path
/// at guess `x_star`, force densities `lambda * q_star`, and surface stresses
/// `lambda * sigma`, asserting the per-lambda invariants (`converged`,
/// `nullity == 4`, and that `force_densities` echoes the scaled input exactly)
/// before returning the result for the cross-lambda comparison.
fn solve_at_gauge(
    members: &[(usize, usize)],
    kinds: &[MemberKind],
    surfaces: &[(usize, usize, usize)],
    x_star: &[[f64; 3]],
    q_star: &[f64],
    sigma: f64,
    lambda: f64,
) -> FreeFormResult {
    let q_scaled: Vec<f64> = q_star.iter().map(|v| v * lambda).collect();
    let sigmas_scaled = vec![sigma * lambda; surfaces.len()];
    let spec = ForceDensitySpec::Explicit(q_scaled.clone());

    let result = form_find_free_surfaces(x_star, members, kinds, surfaces, &sigmas_scaled, &spec)
        .unwrap_or_else(|e| panic!("λ={lambda:e} combined explicit solve must converge, got {e:?}"));

    assert!(result.converged, "λ={lambda:e} must converge");
    assert_eq!(result.nullity, 4, "λ={lambda:e} combined D must have nullity 4");
    assert_eq!(
        result.force_densities, q_scaled,
        "λ={lambda:e} force_densities must echo the λ-scaled input exactly",
    );

    result
}

/// TASK 6413 — [`form_find_free_surfaces`]' combined-surfaces convergence must
/// be gauge-INDEPENDENT: re-solving at `q → λ·q`, `σ → λ·σ` (fixed geometry
/// gauge, λ a power of two) must converge to the bit-identical geometry and
/// exactly `λ`-scaled member forces — never a different iteration outcome.
///
/// Self-deriving: `(x*, q*)` is the δ `GroupRatios` golden's own converged
/// output. `GroupRatios` pins its own gauge via `reference_group` and is
/// deliberately NOT itself asserted gauge-covariant here (see the design
/// decision in this task's plan for why — scaling σ alone against a
/// gauge-fixed reference group is a physically different, and here
/// infeasible, problem). The two λ re-solves both go through
/// [`ForceDensitySpec::Explicit`], which has no internal gauge fix, so it is
/// the mode this task's lock applies to.
///
/// MEASURED RED on pristine (task 6413 premise verification): λ=1 converges;
/// λ=2^20 returns `Err(SearchDidNotConverge)` — the absolute equilibrium-residual
/// stop test is gauge-dependent. MEASURED GREEN with the fix: both converge,
/// nullity 4, and the recovered geometry is bit-identical (max|Δx| = 0.0).
#[test]
fn free_surfaces_explicit_convergence_is_gauge_independent() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_prism_guess();
    let surfaces = prism_surfaces();
    const SIGMA: f64 = 0.2;
    const LAMBDA_UP: f64 = 1_048_576.0; // 2^20

    // Bootstrap: the δ GroupRatios golden's own converged (x*, q*).
    let bootstrap_spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };
    let bootstrap = form_find_free_surfaces(
        &guess,
        &members,
        &kinds,
        &surfaces,
        &[SIGMA; 2],
        &bootstrap_spec,
    )
    .expect("δ GroupRatios bootstrap must form-find the combined prism+membrane");
    assert!(bootstrap.converged, "bootstrap must converge");
    assert_eq!(bootstrap.nullity, 4, "bootstrap combined D must have nullity 4");
    let x_star = bootstrap.nodes;
    let q_star = bootstrap.force_densities;

    // Re-solve through the Explicit path at λ=1 and λ=2^20 from the SAME
    // bootstrap geometry. `form_find_free` cannot displace x* further here
    // (it is already the fixed point), so iteration 0 already satisfies the
    // stop test — a pure probe of the stop test's gauge sensitivity, not of
    // the geometry-descent step.
    let r1 = solve_at_gauge(&members, &kinds, &surfaces, &x_star, &q_star, SIGMA, 1.0);
    let r_up = solve_at_gauge(&members, &kinds, &surfaces, &x_star, &q_star, SIGMA, LAMBDA_UP);

    // Bit-identical recovered geometry across the gauge change.
    let mut max_dx = 0.0_f64;
    for (a, b) in r1.nodes.iter().zip(r_up.nodes.iter()) {
        for axis in 0..3 {
            max_dx = max_dx.max((a[axis] - b[axis]).abs());
        }
    }
    assert_eq!(
        max_dx, 0.0,
        "recovered geometry must be exactly gauge-invariant: max|Δx| = {max_dx:e}",
    );

    // Member forces N = q·L: L is bit-identical (nodes are), q scales exactly
    // by λ (a power of two), so N must scale exactly by λ too.
    for (i, (&n1, &n_up)) in r1.member_forces.iter().zip(r_up.member_forces.iter()).enumerate() {
        assert_eq!(
            n_up,
            n1 * LAMBDA_UP,
            "member {i} force must scale exactly by λ: N(1)={n1:e} N(λ)={n_up:e}",
        );
    }
}

/// TASK 6413 (defect D3) — the (2b) null-space trial-move gate and the (3)
/// eigenvalue-gap descent step must ALSO be gauge-independent. Unlike
/// [`free_surfaces_explicit_convergence_is_gauge_independent`] above — which
/// starts AT the bootstrap fixed point, so the outer loop breaks out at
/// iteration 0 and never executes (2b) or the descent step — this test drives
/// [`form_find_free_surfaces`] on the `Explicit` path from PERTURBED guesses,
/// so both (2b) and (3) actually run under a gauge change.
///
/// Grid: σ ∈ {0.05, 0.2, 1.0} × guess-perturbation scale `k` ∈ {1, 10, 50}
/// (the same grid `tensegrity_delta_combined_form_find.rs`'s
/// `combined_explicit_analytic_q_converges_across_sigma_and_perturbation`
/// regression uses, minus the redundant `canonical`/`x50`-adjacent cells),
/// with `q = analytic_combined_q(σ)` and σ itself both scaled by
/// `λ ∈ {2^20, 2^-20}` against the `λ = 1` solve from the SAME guess. Per
/// cell: `solve_at_gauge` already asserts `converged` and `nullity == 4`;
/// this test additionally asserts each scaled-λ solve agrees with the λ=1
/// solve to within a tight relative bound (`max|Δx| <= 1e-12·max|x|`).
/// Deliberately a bound rather than `assert_eq!`: λ a power of two makes the
/// arithmetic bit-reproducible in principle (see `form_find_free.rs`'s unit
/// tests for that bit-exact contract, and the fixed-point-start golden
/// above), but pinning 18 bit-exact equalities through many third-party
/// dense eigendecompositions here would make this e2e golden fragile to an
/// unrelated faer change rather than a real gauge regression.
///
/// MEASURED RED (task 6413 premise verification): with the pre-existing raw
/// residual at the (2b) call site, `Err(SearchDidNotConverge)` at λ=2^20 in
/// every one of the 9 cells — orders of magnitude away from the bound above,
/// so no discriminating power is lost by relaxing it. MEASURED GREEN with
/// the one-line (2b) fix: all 9 cells × the 2 non-unity λ converge with
/// nullity 4 and max|Δx| exactly 0.0.
#[test]
fn free_surfaces_explicit_convergence_from_perturbed_guess_is_gauge_independent() {
    let (members, kinds) = triplex_topology();
    let surfaces = prism_surfaces();
    const LAMBDA_UP: f64 = 1_048_576.0; // 2^20
    const LAMBDA_DOWN: f64 = 1.0 / 1_048_576.0; // 2^-20

    for &sigma in &[0.05_f64, 0.2, 1.0] {
        let q_star = analytic_combined_q(sigma);

        for &k in &[1.0_f64, 10.0, 50.0] {
            let guess = perturbed_prism_guess_scaled(k);
            let cell = format!("sigma={sigma}, k={k}");

            let r1 = solve_at_gauge(&members, &kinds, &surfaces, &guess, &q_star, sigma, 1.0);

            for (label, lambda) in [("λ=2^20", LAMBDA_UP), ("λ=2^-20", LAMBDA_DOWN)] {
                let r =
                    solve_at_gauge(&members, &kinds, &surfaces, &guess, &q_star, sigma, lambda);

                let mut max_dx = 0.0_f64;
                let mut max_x = 0.0_f64;
                for (a, b) in r1.nodes.iter().zip(r.nodes.iter()) {
                    for axis in 0..3 {
                        max_dx = max_dx.max((a[axis] - b[axis]).abs());
                        max_x = max_x.max(a[axis].abs());
                    }
                }
                const REL_BOUND: f64 = 1e-12;
                assert!(
                    max_dx <= REL_BOUND * max_x,
                    "[{cell}] {label} recovered geometry must be gauge-invariant \
                     to within a tight relative bound: max|Δx| = {max_dx:e}, \
                     max|x| = {max_x:e}",
                );
            }
        }
    }
}
