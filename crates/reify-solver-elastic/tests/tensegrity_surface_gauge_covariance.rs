//! Integration golden for task 6119 — the surfaces-path `form_find` convergence
//! criterion must be GAUGE-INVARIANT: solving at `(q, σ)` and at `(λ·q, λ·σ)`
//! for any `λ > 0` must converge to the identical geometry and scale every
//! member force by exactly `λ`.
//!
//! # PRD reference
//!
//! `docs/prds/v0_6/dimension-checked-readers.md` §"Deliberately bare" (:206-214):
//! tensegrity `force_densities` are **nullity-invariant relative ratios** —
//! genuinely dimensionless, not a gap — so a uniform rescaling of every `q`
//! (and every `σ`) by `λ > 0` must be a no-op on the solved shape and scale
//! every member force by exactly `λ`.
//!
//! The line-only path (`form_find_anchored`) was already covariant
//! unconditionally: `D_ff x_f = −D_fa x_a` is linear in `D`, so `λ` cancels
//! exactly regardless of the convergence criterion. The surfaces path iterates
//! a cotangent fixed point and judges convergence against
//! `SURFACE_EQUILIBRIUM_REL_TOL`; before task 6119 that criterion was an
//! ABSOLUTE bound on a residual that is itself linear in `q`/`σ`, so a gauge
//! change could change the iteration count, the stop geometry, or whether the
//! solve converged at all. This file is the user-observable signal that the
//! fix restores covariance on BOTH surfaces entry points —
//! [`form_find_anchored_surfaces`] and [`form_find_anchored_surfaces_aniso`],
//! which share the identical criterion.

use reify_solver_elastic::{
    AnisotropicSurfaceStress, MemberKind, form_find_anchored_surfaces,
    form_find_anchored_surfaces_aniso,
};

// ---------------------------------------------------------------------------
// Catenoid-tube fixture (adapted from tensegrity_gamma_membrane_form_find.rs
// at the cheap 8-azimuthal × 2-axial resolution) plus ring line-members so
// `q` genuinely enters `D_ff` — a pure-membrane fixture would leave the line
// contribution trivially zero and under-test the gauge argument.
// ---------------------------------------------------------------------------

/// Catenoid waist parameter `c` in `r(z) = c·cosh(z/c)`.
const C: f64 = 1.0;

/// Half-height: boundary rings sit at `z = ±H` (same stable-branch choice as
/// the γ golden — well below the `t·tanh(t) = 1` existence limit).
const H: f64 = 0.8;

fn catenoid_radius(z: f64) -> f64 {
    C * (z / C).cosh()
}

/// Deterministic, RNG-free perturbation in `[-1, 1]` keyed on two indices (RNG
/// is unavailable to workflow/golden code and would break reproducibility).
fn jitter(a: usize, b: usize) -> f64 {
    ((a as f64) * 12.9898 + (b as f64) * 78.233).sin()
}

/// Build the 8-azimuthal × 2-axial catenoid tube: 3 rings × 8 nodes = 24
/// nodes. The two boundary rings (16 nodes) are anchored exactly on the
/// analytic catenoid; the single interior ring (8 nodes) is free, seeded on
/// the catenoid plus a small deterministic perturbation. 32 triangles (2 axial
/// segments × 8 quads × 2 triangles each).
///
/// Additionally scatters 4 struts across the free ring's diameters and 8 hoop
/// cables around it (struts-then-cables order), so line members genuinely
/// couple into `D_ff` alongside the membrane.
///
/// Returns `(nodes, surfaces, anchors, members, kinds)`.
#[allow(clippy::type_complexity)]
fn build_catenoid_tube_with_ring_members(
    perturb: f64,
) -> (
    Vec<[f64; 3]>,
    Vec<(usize, usize, usize)>,
    Vec<usize>,
    Vec<(usize, usize)>,
    Vec<MemberKind>,
) {
    const N_THETA: usize = 8;
    const N_AXIAL: usize = 2;
    let n_rings = N_AXIAL + 1;
    let node_id = |ring: usize, j: usize| ring * N_THETA + (j % N_THETA);

    let mut nodes = vec![[0.0_f64; 3]; n_rings * N_THETA];
    let mut anchors = Vec::new();

    for ring in 0..n_rings {
        let z = -H + 2.0 * H * (ring as f64) / (N_AXIAL as f64);
        let r_true = catenoid_radius(z);
        let is_boundary = ring == 0 || ring == n_rings - 1;
        for j in 0..N_THETA {
            let theta = 2.0 * std::f64::consts::PI * (j as f64) / (N_THETA as f64);
            let id = node_id(ring, j);
            if is_boundary {
                // Anchors stay EXACTLY on the catenoid — the fixed BVP data.
                nodes[id] = [r_true * theta.cos(), r_true * theta.sin(), z];
                anchors.push(id);
            } else {
                let r = r_true + perturb * jitter(ring, j);
                let dz = 0.5 * perturb * jitter(j, ring);
                nodes[id] = [r * theta.cos(), r * theta.sin(), z + dz];
            }
        }
    }

    // Triangulate each quad between adjacent rings (consistent diagonal split).
    let mut surfaces = Vec::new();
    for ring in 0..N_AXIAL {
        for j in 0..N_THETA {
            let a = node_id(ring, j);
            let b = node_id(ring, j + 1);
            let c = node_id(ring + 1, j);
            let d = node_id(ring + 1, j + 1);
            surfaces.push((a, b, c));
            surfaces.push((b, d, c));
        }
    }

    // The only interior (non-boundary) ring is ring 1 for N_AXIAL=2.
    let ring1 = |j: usize| node_id(1, j);
    let mut members = Vec::new();
    let mut kinds = Vec::new();
    // 4 struts across the ring diameters (opposite nodes, N_THETA/2 apart).
    for j in 0..4 {
        members.push((ring1(j), ring1(j + 4)));
        kinds.push(MemberKind::Strut);
    }
    // 8 hoop cables around the free ring.
    for j in 0..N_THETA {
        members.push((ring1(j), ring1(j + 1)));
        kinds.push(MemberKind::Cable);
    }

    (nodes, surfaces, anchors, members, kinds)
}

/// Force density magnitudes for the ring members: struts compressive, cables
/// tensile (the sign contract [`MemberKind`] enforces).
const STRUT_Q: f64 = -0.05;
const CABLE_Q: f64 = 0.3;

fn ring_member_q(kinds: &[MemberKind]) -> Vec<f64> {
    kinds
        .iter()
        .map(|k| match k {
            MemberKind::Strut => STRUT_Q,
            MemberKind::Cable => CABLE_Q,
        })
        .collect()
}

/// Perturbation off the analytic catenoid — large enough that the solve must
/// do real work, small enough to stay in the basin of attraction (matches the
/// γ golden's choice).
const PERTURB: f64 = 0.02;

/// Gauge factor `λ = 2^20`, chosen as a power of two on purpose: `λ·q` and
/// `λ·σ` are then exact in IEEE-754, so the assembled `D_λ = λ·D` holds
/// entrywise-exactly and the whole covariance claim is an arithmetic identity
/// rather than a rounding accident. A non-power-of-two `λ` would only test the
/// identity to rounding.
const LAMBDA: f64 = 1_048_576.0;

/// Reciprocal gauge factor `λ = 2^-20` — also exact in IEEE-754 (dividing by a
/// power of two is exact, same as multiplying), so the bit-exactness argument
/// above holds equally here. Locks the OTHER half of the pre-fix defect: a
/// LARGE λ inflated the absolute residual so the solve never converged (the
/// measured RED at [`LAMBDA`]); a SMALL λ instead SHRINKS the residual below
/// an absolute tolerance and stops PREMATURELY on an unconverged geometry —
/// the half of the defect a large-λ-only suite cannot see (task 6119).
const LAMBDA_SMALL: f64 = 1.0 / 1_048_576.0;

/// Bound for the relative agreement between the base-gauge and λ-gauge
/// solves. MEASURED agreement is bit-exact (`0.0`) for a power-of-two `λ`;
/// `1e-13` is deliberately ~13 orders of margin above that measurement so a
/// future faer LU implementation change cannot turn a correct kernel red — it
/// is NOT a tuned-to-fit threshold.
const GAUGE_REL_TOL: f64 = 1e-13;

// ---------------------------------------------------------------------------
// Relative-difference helpers
// ---------------------------------------------------------------------------

/// Max componentwise relative difference between two equal-length node lists:
/// `max|Δx| / (1 + max|x_ref|)`.
fn max_coord_rel_diff(a: &[[f64; 3]], b_ref: &[[f64; 3]]) -> f64 {
    let mut num = 0.0_f64;
    let mut scale = 0.0_f64;
    for (pa, pb) in a.iter().zip(b_ref.iter()) {
        for k in 0..3 {
            num = num.max((pa[k] - pb[k]).abs());
            scale = scale.max(pb[k].abs());
        }
    }
    num / (1.0 + scale)
}

/// Max relative difference between `scaled` and `λ·base`:
/// `max|λ·base − scaled| / (1 + max|λ·base|)`. Used to check that member
/// forces / force-density echoes / principal-stress echoes all scale by
/// exactly `λ` under a gauge change.
fn max_rel_diff_scaled(base: &[f64], scaled: &[f64], lambda: f64) -> f64 {
    let mut num = 0.0_f64;
    let mut scale = 0.0_f64;
    for (&nb, &ns) in base.iter().zip(scaled.iter()) {
        let expected = lambda * nb;
        num = num.max((expected - ns).abs());
        scale = scale.max(expected.abs());
    }
    num / (1.0 + scale)
}

/// Per-element floor below which a base-gauge quantity would make its own ×λ
/// covariance check vacuous: `max_rel_diff_scaled` compares `λ·base` against
/// `scaled`, which collapses to `0 == 0·λ` — trivially true for EVERY λ — when
/// `base` is zero. Mirrors the non-vacuity floor task/6095 added to its own
/// gauge-covariance suite (commit f0ec8d5e9f) so a regression that collapsed
/// member forces, density echoes, or stress echoes to zero cannot land green.
const NON_VACUOUS_FLOOR: f64 = 1e-9;

/// Asserts `values` is non-empty and every element clears
/// [`NON_VACUOUS_FLOOR`], so a subsequent `max_rel_diff_scaled(values, ...)`
/// call cannot pass by vacuously comparing `0 == 0·λ`.
fn assert_all_non_vacuous(label: &str, values: &[f64]) {
    assert!(!values.is_empty(), "{label} must be non-empty");
    for (i, &v) in values.iter().enumerate() {
        assert!(
            v.abs() > NON_VACUOUS_FLOOR,
            "{label}[{i}] = {v} at the base gauge makes the ×λ covariance check \
             vacuous — 0 == 0·λ holds for any λ (task 6119)",
        );
    }
}

/// Floor for "the base-gauge solve actually moved off the seed geometry",
/// comparable to [`PERTURB`] but well below it (task 6119 review). Without
/// this check, a regression that made the convergence criterion trivially
/// satisfiable would have BOTH gauges break out of the fixed point at
/// iteration 0 and echo the unperturbed seed back as `converged == true`;
/// every covariance assertion below would then compare the (λ-scaled) seed
/// to itself and pass vacuously — the same "echo the unsolved initial guess
/// back as converged" failure mode [`assert_all_non_vacuous`] does not, by
/// itself, catch.
const MIN_SOLVE_DISPLACEMENT: f64 = 1e-3;

// ---------------------------------------------------------------------------
// Shared assertion body
// ---------------------------------------------------------------------------

/// Shared assertion body for a single gauge-covariance check: both solves
/// converged, the base-gauge solve actually moved off the seed geometry,
/// solved geometry agrees, and each `(label, base, scaled)` quantity in
/// `extra_echoes` scales by exactly `lambda` — on top of the member-force
/// and force-density echoes checked unconditionally. Factored
/// out so the four covariance tests (iso/aniso × [`LAMBDA`]/[`LAMBDA_SMALL`])
/// share one assertion body instead of duplicating it a third and fourth time
/// (task 6119).
#[allow(clippy::too_many_arguments)]
fn assert_gauge_covariant(
    context: &str,
    lambda: f64,
    base_converged: bool,
    scaled_converged: bool,
    seed_nodes: &[[f64; 3]],
    base_nodes: &[[f64; 3]],
    scaled_nodes: &[[f64; 3]],
    base_member_forces: &[f64],
    scaled_member_forces: &[f64],
    base_force_densities: &[f64],
    scaled_force_densities: &[f64],
    extra_echoes: &[(&str, &[f64], &[f64])],
) {
    eprintln!(
        "[{context}] λ={lambda:e} base.converged={base_converged} scaled.converged={scaled_converged}",
    );

    assert!(
        base_converged,
        "[{context}] λ={lambda:e}: base-gauge solve must converge",
    );
    assert!(
        scaled_converged,
        "[{context}] λ={lambda:e}: λ-gauge solve must converge — the criterion must be gauge-invariant (task 6119)",
    );

    // Vacuity guard (task 6119 review): pin that the base-gauge solve
    // actually moved off the seed geometry before trusting any agreement
    // check below — see [`MIN_SOLVE_DISPLACEMENT`] for why.
    let moved = max_coord_rel_diff(base_nodes, seed_nodes);
    assert!(
        moved > MIN_SOLVE_DISPLACEMENT,
        "[{context}] λ={lambda:e}: base-gauge solve barely moved off the seed \
         geometry (rel diff = {moved:e}, expected > {MIN_SOLVE_DISPLACEMENT:e}) \
         — suspiciously close to the 'echoed the unsolved initial guess back \
         as converged' failure mode (task 6119)",
    );

    let node_err = max_coord_rel_diff(base_nodes, scaled_nodes);
    assert!(
        node_err < GAUGE_REL_TOL,
        "[{context}] λ={lambda:e}: solved geometry must be gauge-invariant: rel err = {node_err:e}, expected < {GAUGE_REL_TOL:e}",
    );

    assert_all_non_vacuous(&format!("[{context}] member_forces"), base_member_forces);
    let force_err = max_rel_diff_scaled(base_member_forces, scaled_member_forces, lambda);
    assert!(
        force_err < GAUGE_REL_TOL,
        "[{context}] λ={lambda:e}: member forces must scale by exactly λ: rel err = {force_err:e}, expected < {GAUGE_REL_TOL:e}",
    );

    assert_all_non_vacuous(&format!("[{context}] force_densities"), base_force_densities);
    let q_echo_err = max_rel_diff_scaled(base_force_densities, scaled_force_densities, lambda);
    assert!(
        q_echo_err < GAUGE_REL_TOL,
        "[{context}] λ={lambda:e}: force_densities echo must scale by exactly λ: rel err = {q_echo_err:e}",
    );

    for (label, base_vals, scaled_vals) in extra_echoes {
        assert_all_non_vacuous(&format!("[{context}] {label}"), base_vals);
        let err = max_rel_diff_scaled(base_vals, scaled_vals, lambda);
        assert!(
            err < GAUGE_REL_TOL,
            "[{context}] λ={lambda:e}: {label} echo must scale by exactly λ: rel err = {err:e}",
        );
    }
}

// ---------------------------------------------------------------------------
// Isotropic surfaces path
// ---------------------------------------------------------------------------

/// Solve the catenoid+ring-members fixture at both the base gauge and the
/// `lambda`-scaled gauge via [`form_find_anchored_surfaces`], and assert the
/// shared gauge-covariance invariants. Called at both [`LAMBDA`] (the
/// large-λ / never-converges direction) and [`LAMBDA_SMALL`] (the small-λ /
/// premature-stop direction), so both halves of the pre-fix defect are
/// covered without duplicating the solve-and-assert body (task 6119).
fn check_iso_surfaces_gauge_covariance(lambda: f64) {
    let (nodes, surfaces, anchors, members, kinds) =
        build_catenoid_tube_with_ring_members(PERTURB);
    let q = ring_member_q(&kinds);
    let sigma = 1.0_f64;
    let sigmas = vec![sigma; surfaces.len()];

    let base =
        form_find_anchored_surfaces(&nodes, &members, &kinds, &q, &surfaces, &sigmas, &anchors)
            .expect("base-gauge catenoid+ring-members solve must be feasible");

    let q_scaled: Vec<f64> = q.iter().map(|v| v * lambda).collect();
    let sigmas_scaled: Vec<f64> = sigmas.iter().map(|v| v * lambda).collect();
    let scaled = form_find_anchored_surfaces(
        &nodes, &members, &kinds, &q_scaled, &surfaces, &sigmas_scaled, &anchors,
    )
    .expect("λ-gauge catenoid+ring-members solve must be feasible");

    assert_gauge_covariant(
        "ISO",
        lambda,
        base.converged,
        scaled.converged,
        &nodes,
        &base.nodes,
        &scaled.nodes,
        &base.member_forces,
        &scaled.member_forces,
        &base.force_densities,
        &scaled.force_densities,
        &[(
            "surface_stresses",
            &base.surface_stresses,
            &scaled.surface_stresses,
        )],
    );
}

#[test]
fn iso_surfaces_form_find_is_gauge_covariant() {
    check_iso_surfaces_gauge_covariance(LAMBDA);
}

/// Small-λ direction (task 6119): a small `λ` SHRINKS the pre-fix absolute
/// residual below its tolerance, stopping the iteration PREMATURELY on
/// unconverged geometry — the silent half of the defect a large-λ-only suite
/// cannot see (the large-λ direction fails loudly instead: the residual is
/// INFLATED and the solve never converges).
#[test]
fn iso_surfaces_form_find_is_gauge_covariant_small_lambda() {
    check_iso_surfaces_gauge_covariance(LAMBDA_SMALL);
}

// ---------------------------------------------------------------------------
// Anisotropic surfaces path — shares the identical criterion with
// `form_find_anchored_surfaces` (`free_equilibrium_residual_relative` /
// `SURFACE_EQUILIBRIUM_REL_TOL`), not restated here to avoid a bare line
// citation rotting on the next edit to form_find.rs (task 6119 review).
// ---------------------------------------------------------------------------

/// Solve the catenoid+ring-members fixture at both the base gauge and the
/// `lambda`-scaled gauge via [`form_find_anchored_surfaces_aniso`], and assert
/// the shared gauge-covariance invariants plus the principal-stress echoes.
/// Called at both [`LAMBDA`] and [`LAMBDA_SMALL`] — see
/// [`check_iso_surfaces_gauge_covariance`] for why both directions matter.
fn check_aniso_surfaces_gauge_covariance(lambda: f64) {
    let (nodes, surfaces, anchors, members, kinds) =
        build_catenoid_tube_with_ring_members(PERTURB);
    let q = ring_member_q(&kinds);
    let sigma_warp = 1.0_f64;
    let sigma_weft = 0.4_f64;
    let warp_dir = [0.0, 0.0, 1.0];
    let prestress = vec![
        AnisotropicSurfaceStress { warp_dir, sigma_warp, sigma_weft };
        surfaces.len()
    ];

    let base = form_find_anchored_surfaces_aniso(
        &nodes, &members, &kinds, &q, &surfaces, &prestress, &anchors,
    )
    .expect("base-gauge aniso solve must be feasible");

    let q_scaled: Vec<f64> = q.iter().map(|v| v * lambda).collect();
    let prestress_scaled = vec![
        AnisotropicSurfaceStress {
            warp_dir,
            sigma_warp: sigma_warp * lambda,
            sigma_weft: sigma_weft * lambda,
        };
        surfaces.len()
    ];
    let scaled = form_find_anchored_surfaces_aniso(
        &nodes, &members, &kinds, &q_scaled, &surfaces, &prestress_scaled, &anchors,
    )
    .expect("λ-gauge aniso solve must be feasible");

    // Principal-stress echo: recover_principal_stress reads σ_w/σ_f straight
    // from the input spec, so this is the aniso analogue of the isotropic
    // surface_stresses echo check.
    assert_eq!(base.principal_stresses.len(), scaled.principal_stresses.len());
    let major_base: Vec<f64> = base.principal_stresses.iter().map(|p| p.major).collect();
    let major_scaled: Vec<f64> = scaled.principal_stresses.iter().map(|p| p.major).collect();
    let minor_base: Vec<f64> = base.principal_stresses.iter().map(|p| p.minor).collect();
    let minor_scaled: Vec<f64> = scaled.principal_stresses.iter().map(|p| p.minor).collect();

    assert_gauge_covariant(
        "ANISO",
        lambda,
        base.converged,
        scaled.converged,
        &nodes,
        &base.nodes,
        &scaled.nodes,
        &base.member_forces,
        &scaled.member_forces,
        &base.force_densities,
        &scaled.force_densities,
        &[
            ("principal major-stress", &major_base, &major_scaled),
            ("principal minor-stress", &minor_base, &minor_scaled),
        ],
    );
}

#[test]
fn aniso_surfaces_form_find_is_gauge_covariant() {
    check_aniso_surfaces_gauge_covariance(LAMBDA);
}

/// Small-λ direction (task 6119) — see
/// [`iso_surfaces_form_find_is_gauge_covariant_small_lambda`] for why this
/// direction is a structurally distinct check from the large-λ test above.
#[test]
fn aniso_surfaces_form_find_is_gauge_covariant_small_lambda() {
    check_aniso_surfaces_gauge_covariance(LAMBDA_SMALL);
}
