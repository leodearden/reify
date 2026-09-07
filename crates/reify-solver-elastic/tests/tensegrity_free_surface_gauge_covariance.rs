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
//! shift, no mantissa rounding) — the bit-exact covariance assertions below
//! are therefore checkable with `assert_eq!`, not a tolerance.

use reify_solver_elastic::{
    ForceDensitySpec, FreeFormResult, MemberKind, form_find_free_surfaces,
};

// ---------------------------------------------------------------------------
// Fixture — the complete triplex (same as the δ combined golden)
// ---------------------------------------------------------------------------

fn triplex_topology() -> (Vec<(usize, usize)>, Vec<MemberKind>) {
    let members = vec![
        (0, 4), (1, 5), (2, 3), // struts
        (0, 1), (1, 2), (2, 0), // top horizontals
        (3, 4), (4, 5), (5, 3), // bottom horizontals
        (0, 3), (1, 4), (2, 5), // verticals
    ];
    let kinds = vec![
        MemberKind::Strut, MemberKind::Strut, MemberKind::Strut,
        MemberKind::Cable, MemberKind::Cable, MemberKind::Cable,
        MemberKind::Cable, MemberKind::Cable, MemberKind::Cable,
        MemberKind::Cable, MemberKind::Cable, MemberKind::Cable,
    ];
    (members, kinds)
}

fn triplex_group_ids() -> Vec<usize> {
    vec![0, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2]
}

/// Canonical symmetric T-prism.
fn canonical_prism() -> Vec<[f64; 3]> {
    let s = 3.0_f64.sqrt() / 2.0;
    let h = 1.0_f64;
    vec![
        [1.0, 0.0, h], [-0.5, s, h], [-0.5, -s, h],
        [-0.5, -s, -h], [1.0, 0.0, -h], [-0.5, s, -h],
    ]
}

/// Perturbed prism guess, with the fixed `PERTURB` displacement table scaled
/// by `k` — lets the D3 grid below (see
/// `free_surfaces_explicit_convergence_from_perturbed_guess_is_gauge_independent`)
/// probe how far off-symmetry a starting guess can be while still converging.
/// Mirrors `tensegrity_delta_combined_form_find.rs`'s helper of the same
/// name/shape. `perturbed_prism_guess()` delegates here at `k = 1.0`, so the
/// existing gauge-covariance golden above is byte-identical to before this
/// generalisation.
fn perturbed_prism_guess_scaled(k: f64) -> Vec<[f64; 3]> {
    const PERTURB: [[f64; 3]; 6] = [
        [0.0009, -0.0011, 0.0007],
        [-0.0013, 0.0006, 0.0010],
        [0.0012, 0.0008, -0.0009],
        [-0.0007, -0.0012, 0.0011],
        [0.0010, -0.0008, -0.0013],
        [-0.0011, 0.0013, 0.0006],
    ];
    canonical_prism()
        .iter()
        .zip(PERTURB.iter())
        .map(|(p, d)| [p[0] + k * d[0], p[1] + k * d[1], p[2] + k * d[2]])
        .collect()
}

fn perturbed_prism_guess() -> Vec<[f64; 3]> {
    perturbed_prism_guess_scaled(1.0)
}

/// Top {0,1,2} and bottom {3,4,5} membrane triangles.
fn prism_surfaces() -> Vec<(usize, usize, usize)> {
    vec![(0, 1, 2), (3, 4, 5)]
}

/// Closed-form COMBINED self-stress for the triplex + two equilateral membrane
/// triangles (same derivation, same closed form, as
/// `tensegrity_delta_combined_form_find.rs::analytic_combined_q`): every
/// cotangent in the surface stencil is cot(60°) = 1/√3 at the symmetric
/// realisation, so `Σ_T σ_T·L_T` collapses to a uniform extra edge weight
/// `w = σ·cot(60°)/2 = σ/(2√3)` on the six horizontal cables, giving
/// `q_strut = -(√3 + σ/2)`, `q_horiz = 1`, `q_vert = +(√3 + σ/2)`.
fn analytic_combined_q(sigma: f64) -> Vec<f64> {
    let a = 3.0_f64.sqrt() + sigma / 2.0;
    vec![
        -a, -a, -a, // struts
        1.0, 1.0, 1.0, // top horizontals
        1.0, 1.0, 1.0, // bottom horizontals
        a, a, a, // verticals
    ]
}

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
/// this test additionally asserts bit-exact agreement (`max|Δx| == 0.0`,
/// `assert_eq!`, no tolerance) between each scaled-λ solve and the λ=1 solve
/// — legitimate because λ is a power of two, an exponent-field shift that
/// faer's self-adjoint EVD reproduces exactly (identical eigenvectors,
/// exactly λ-scaled eigenvalues).
///
/// MEASURED RED at two altitudes (task 6413 premise verification): (i)
/// compile — the crate does not build at all today (E0425 at
/// `form_find_free.rs:423`, `cannot find function all_node_equilibrium_residual`
/// — task #6537 added this second (2b) call site after this task's other four
/// commits were authored, and it still names the renamed-away function); (ii)
/// semantic — with the pre-existing raw residual restored at :423 (crate
/// builds, but the (2b) fix not yet applied) this test still fails,
/// `Err(SearchDidNotConverge)` at λ=2^20 in every one of the 9 cells.
/// MEASURED GREEN with the one-line (2b) fix: all 9 cells × the 2 non-unity λ
/// converge with nullity 4 and max|Δx| exactly 0.0.
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
                for (a, b) in r1.nodes.iter().zip(r.nodes.iter()) {
                    for axis in 0..3 {
                        max_dx = max_dx.max((a[axis] - b[axis]).abs());
                    }
                }
                assert_eq!(
                    max_dx, 0.0,
                    "[{cell}] {label} recovered geometry must be exactly \
                     gauge-invariant: max|Δx| = {max_dx:e}",
                );
            }
        }
    }
}
