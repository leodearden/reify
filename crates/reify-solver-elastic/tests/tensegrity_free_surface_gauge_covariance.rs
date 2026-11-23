//! Integration golden for Tensegrity free-standing GAUGE covariance under both
//! force-density specs, `Explicit` (task 6413) and `GroupRatios` (task 9272) —
//! the free-standing twin of task 6119's anchored-path gauge lock.
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
//! of `λ`, under either spec:
//!
//! - [`ForceDensitySpec::Explicit`] takes the `λ`-scaled `q` as given. Before
//!   task 6413's fix, the absolute equilibrium-residual stop test made its
//!   convergence gauge-DEPENDENT: a large `λ` scaled the raw residual past the
//!   absolute tolerance, so the fixed point exhausted its iteration budget and
//!   returned `Err(SearchDidNotConverge)` for a structure that is physically
//!   identical to one that converges fine at `λ = 1`.
//! - [`ForceDensitySpec::GroupRatios`] SEARCHES `q`, so its whole gauge is the
//!   seed ratios and σ scaled together. Before task 9272's fix, the search
//!   judged its objective, grid and magnitude floors on absolute thresholds:
//!   a small `λ` failed to converge at all, and a large one drifted the
//!   recovered geometry.
//!
//! This test drives [`form_find_free_surfaces`] through its **public
//! crate-root surface** only — it never reaches into the private per-node
//! residual / descent-step internals (those are covered by the crate's own
//! unit tests in `form_find_free.rs`, including the descent-step gauge
//! covariance that has no converging e2e altitude — see that module's tests
//! for why).
//!
//! # Fixtures
//!
//! The fixed-point-start golden re-solves `λ = 1` and `λ = 2^20` from a common
//! baseline `(x*, q*)` obtained by first running the existing δ `GroupRatios`
//! golden (the same fixture as `tensegrity_delta_combined_form_find.rs`) to
//! convergence. The perturbed-guess grid solves every cell of both specs from
//! the shared closed-form fixtures in `common`. `λ` is always a power of two so
//! the gauge change is exact in IEEE-754 (an exponent-field shift, no mantissa
//! rounding). The fixed-point-start golden checks this bit-exactly
//! (`assert_eq!`); the perturbed-guess grid relaxes to a tight relative bound
//! instead — see that test's own doc for why.

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
/// output. The two λ re-solves both go through [`ForceDensitySpec::Explicit`],
/// so this golden probes the stop test alone. `GroupRatios` IS asserted
/// gauge-covariant, by
/// [`free_standing_convergence_from_perturbed_guess_is_gauge_independent_for_both_specs`],
/// under the whole-gauge rescale of its seed ratios and σ together. Scaling σ
/// alone against the reference group is a physically different problem, not a
/// gauge change.
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

/// TASK 6413 (defect D3) / TASK 9272 — free-standing convergence from a
/// PERTURBED guess must be gauge-independent under BOTH force-density specs.
/// Unlike [`free_surfaces_explicit_convergence_is_gauge_independent`] above —
/// which starts AT the bootstrap fixed point, so the outer loop breaks out at
/// iteration 0 and never executes the (2b) null-space trial-move gate or the
/// (3) eigenvalue-gap descent step — every cell here starts off-equilibrium.
/// On the caps rows (2b) and (3) therefore run under a gauge change, and under
/// `GroupRatios` so does the combined search that derives `q`. The line-only
/// rows pass no surfaces, so `form_find_free_surfaces` delegates them to the
/// line-only path: there `GroupRatios` runs the line-only search and
/// `Explicit` the direct null-space recovery.
///
/// Grid: spec ∈ {`Explicit`, `GroupRatios`} × membrane ∈ {line-only, caps at
/// σ ∈ {0.05, 0.2, 1.0}} × guess-perturbation scale `k` ∈ {1, 10, 50}, each
/// re-solved at `λ ∈ {2^20, 2^-20}` against the `λ = 1` solve from the SAME
/// guess. The whole gauge is rescaled together: `Explicit`'s `q` (the closed
/// form [`analytic_combined_q`], σ = 0 on the line-only row), `GroupRatios`'
/// seed ratios, and the caps' σ. Per cell, both solves must converge with
/// nullity 4, and the λ solve must reproduce the `λ = 1` geometry to
/// `max|Δx| <= 1e-12·max|x|` with every force density and member force
/// `λ`-scaled to the same relative bound.
///
/// Deliberately a bound rather than `assert_eq!`: λ a power of two makes the
/// arithmetic bit-reproducible in principle (see `form_find_free.rs`'s unit
/// tests for that bit-exact contract, and the fixed-point-start golden
/// above), but pinning every cell bit-exactly through many third-party dense
/// eigendecompositions would make this golden fragile to an unrelated faer
/// change rather than a real gauge regression. The bound loses no
/// discriminating power: both defects it locks — task 6413's absolute stop
/// test (`Explicit`) and task 9272's absolute search thresholds
/// (`GroupRatios`) — fail with `Err(SearchDidNotConverge)` or drift orders of
/// magnitude past it.
#[test]
fn free_standing_convergence_from_perturbed_guess_is_gauge_independent_for_both_specs() {
    const LAMBDA_UP: f64 = 1_048_576.0; // 2^20
    const LAMBDA_DOWN: f64 = 1.0 / 1_048_576.0; // 2^-20

    for spec in [SpecArm::Explicit, SpecArm::GroupRatios] {
        for membrane in [
            Membrane::LineOnly,
            Membrane::Caps(0.05),
            Membrane::Caps(0.2),
            Membrane::Caps(1.0),
        ] {
            for k in [1.0_f64, 10.0, 50.0] {
                let cell = GaugeCell { spec, membrane, k };
                let r1 = cell.solve(1.0);

                for lambda in [LAMBDA_UP, LAMBDA_DOWN] {
                    let label = cell.label(lambda);
                    let r = cell.solve(lambda);

                    let mut max_dx = 0.0_f64;
                    let mut max_x = 0.0_f64;
                    for (a, b) in r1.nodes.iter().zip(r.nodes.iter()) {
                        for axis in 0..3 {
                            max_dx = max_dx.max((a[axis] - b[axis]).abs());
                            max_x = max_x.max(a[axis].abs());
                        }
                    }
                    assert!(
                        max_dx <= REL_BOUND * max_x,
                        "{label} recovered geometry must be gauge-invariant to within a \
                         tight relative bound: max|Δx| = {max_dx:e}, max|x| = {max_x:e}",
                    );

                    assert_scaled_by_lambda(
                        &r1.force_densities,
                        &r.force_densities,
                        lambda,
                        "force_densities",
                        &label,
                    );
                    assert_scaled_by_lambda(
                        &r1.member_forces,
                        &r.member_forces,
                        lambda,
                        "member_forces",
                        &label,
                    );
                }
            }
        }
    }
}

/// The relative bound every perturbed-guess gauge cell is held to.
const REL_BOUND: f64 = 1e-12;

/// The force-density spec a [`GaugeCell`] is solved under.
#[derive(Clone, Copy, Debug)]
enum SpecArm {
    /// `q = analytic_combined_q(σ)·λ`, echoed back unchanged.
    Explicit,
    /// The δ golden's triplex groups, reference group 1, every seed ratio `λ`-scaled.
    GroupRatios,
}

/// The membrane a [`GaugeCell`] carries.
#[derive(Clone, Copy, Debug)]
enum Membrane {
    /// No surfaces: [`form_find_free_surfaces`] delegates to the line-only path.
    LineOnly,
    /// Both prism caps at isotropic surface stress σ (before the gauge `λ`).
    Caps(f64),
}

/// One cell of the perturbed-guess gauge grid, solvable at any gauge `λ`.
struct GaugeCell {
    spec: SpecArm,
    membrane: Membrane,
    k: f64,
}

impl GaugeCell {
    fn label(&self, lambda: f64) -> String {
        let Self { spec, membrane, k } = self;
        format!("[{spec:?}, {membrane:?}, k={k}, λ={lambda:e}]")
    }

    /// Solve the cell at gauge `λ`, asserting the gauge-free verdict
    /// (`Ok`, converged, nullity 4) and the input echoes: σ always, and
    /// `q` itself under `Explicit`.
    fn solve(&self, lambda: f64) -> FreeFormResult {
        let label = self.label(lambda);
        let (members, kinds) = triplex_topology();
        let guess = perturbed_prism_guess_scaled(self.k);
        let (surfaces, stresses, sigma) = match self.membrane {
            Membrane::LineOnly => (Vec::new(), Vec::new(), 0.0),
            Membrane::Caps(sigma) => (prism_surfaces(), vec![sigma * lambda; 2], sigma),
        };
        let spec = match self.spec {
            SpecArm::Explicit => ForceDensitySpec::Explicit(
                analytic_combined_q(sigma)
                    .iter()
                    .map(|q| q * lambda)
                    .collect(),
            ),
            SpecArm::GroupRatios => ForceDensitySpec::GroupRatios {
                group_ids: triplex_group_ids(),
                seed_ratios: vec![-lambda, lambda, lambda],
                reference_group: 1,
            },
        };

        let result = form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &stresses, &spec)
            .unwrap_or_else(|e| panic!("{label} must form-find, got {e:?}"));

        assert!(result.converged, "{label} must converge");
        assert_eq!(result.nullity, 4, "{label} must recover a nullity-4 form");
        assert_eq!(
            result.surface_stresses, stresses,
            "{label} surface_stresses must echo the λ-scaled σ exactly",
        );
        if let ForceDensitySpec::Explicit(q) = &spec {
            assert_eq!(
                &result.force_densities, q,
                "{label} force_densities must echo the λ-scaled input exactly",
            );
        }

        result
    }
}

/// Assert `scaled[i]` is `λ·base[i]` to [`REL_BOUND`] for every `i`, and that
/// each `base[i]` is nonzero, since at zero the ×λ check is vacuous.
fn assert_scaled_by_lambda(base: &[f64], scaled: &[f64], lambda: f64, field: &str, label: &str) {
    assert_eq!(
        base.len(),
        scaled.len(),
        "{label} {field}: same length at both gauges"
    );
    for (i, (&b, &s)) in base.iter().zip(scaled.iter()).enumerate() {
        assert!(
            b != 0.0,
            "{label} {field}[{i}] = 0 at λ = 1 makes the ×λ check vacuous"
        );
        let expected = b * lambda;
        assert!(
            (s - expected).abs() <= REL_BOUND * expected.abs(),
            "{label} {field}[{i}] must scale by λ: λ·{b:e} = {expected:e}, got {s:e}",
        );
    }
}
