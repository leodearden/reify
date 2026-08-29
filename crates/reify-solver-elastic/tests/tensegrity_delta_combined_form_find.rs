//! Integration golden for Tensegrity-membrane δ (task 4415) — free-standing
//! combined struts+cables+membrane form-finding via [`form_find_free_surfaces`].
//!
//! PRD reference: `docs/prds/v0_6/tensegrity-membrane.md` §4 M1b / D3 (δ).
//!
//! # The user-observable signal
//!
//! Given the triplex (3 struts + 9 cables) topology and two isotropic membrane
//! triangles (top {0,1,2} and bottom {3,4,5}) with uniform surface stress σ=0.2,
//! the combined free-standing kernel must find a self-stressed equilibrium of
//! D = CᵀQC + Σ_T σ_T·L_T with nullity exactly 4, struts compressive, cables
//! tensile, and an independent equilibrium residual < 1e-9.
//!
//! This test drives [`form_find_free_surfaces`] through its **public crate-root
//! surface** and reassembles D = CᵀQC + Σ_T σ_T·L_T INDEPENDENTLY (faer-free,
//! via an inlined cotangent-Laplacian) so that a flipped sign / mis-scatter in
//! the kernel surfaces as a large residual rather than hiding behind its own
//! assembly.

use reify_solver_elastic::{
    ForceDensitySpec, FreeFormError, FreeFormResult, MemberKind, form_find_free_surfaces,
};

// ---------------------------------------------------------------------------
// Fixture — the complete triplex (same as T1b golden)
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
    vec![0,0,0, 1,1,1,1,1,1, 2,2,2]
}

/// Canonical symmetric T-prism.
fn canonical_prism() -> Vec<[f64; 3]> {
    let s = 3.0_f64.sqrt() / 2.0;
    let h = 1.0_f64;
    vec![
        [ 1.0,  0.0, h],  [ -0.5,  s, h],  [-0.5, -s, h],
        [-0.5, -s, -h],   [  1.0,  0.0, -h], [-0.5, s, -h],
    ]
}

/// Perturbed prism guess, with the fixed `PERTURB` displacement table scaled
/// by `k` — lets regression tests probe how far off-symmetry a starting guess
/// can be while still converging. `perturbed_prism_guess()` delegates here at
/// `k = 1.0`, so the landed GroupRatios golden and step-1's tests are
/// byte-identical to before this generalisation.
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
/// triangles, struts-then-cables order. At the free-standing equilibrium both
/// membrane triangles are equilateral, so every cotangent in the surface stencil
/// is cot(60°) = 1/√3 and `Σ_T σ_T·L_T` collapses to a uniform extra edge weight
/// w = σ·cot(60°)/2 = σ/(2√3) on exactly the six horizontal cables. Hence
/// D_combined(q) ≡ D_line(q + w·1{horizontal}), and a valid form needs
/// q + w·1{horizontal} ∝ the triplex self-stress (-√3, 1, √3). Pinning the
/// horizontals at 1 gives λ = 1 + σ/(2√3), i.e. q_strut = -(√3 + σ/2),
/// q_horiz = 1, q_vert = +(√3 + σ/2).
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
// Independent (faer-free) reassembly helpers — the honest verification path
// ---------------------------------------------------------------------------

/// Reassemble D = CᵀQC + Σ_T σ_T·L_T as a dense [n][n] array, using an
/// inlined cotangent-Laplacian formula independent of the kernel's faer path.
#[allow(clippy::needless_range_loop)]
fn reassemble_d_combined(
    n: usize,
    members: &[(usize, usize)],
    q: &[f64],
    surfaces: &[(usize, usize, usize)],
    sigmas: &[f64],
    nodes: &[[f64; 3]],
) -> Vec<Vec<f64>> {
    let mut d = vec![vec![0.0_f64; n]; n];
    for (&(j, k), &qi) in members.iter().zip(q.iter()) {
        d[j][j] += qi; d[k][k] += qi; d[j][k] -= qi; d[k][j] -= qi;
    }
    let vsub = |a: [f64; 3], b: [f64; 3]| -> [f64; 3] {
        [a[0]-b[0], a[1]-b[1], a[2]-b[2]]
    };
    let vdot = |a: [f64; 3], b: [f64; 3]| -> f64 {
        a[0]*b[0] + a[1]*b[1] + a[2]*b[2]
    };
    let vcross = |a: [f64; 3], b: [f64; 3]| -> [f64; 3] {
        [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
    };
    for (&(gi, gj, gk), &s) in surfaces.iter().zip(sigmas.iter()) {
        let pi = nodes[gi]; let pj = nodes[gj]; let pk = nodes[gk];
        let eij = vsub(pj, pi); let eik = vsub(pk, pi); let ejk = vsub(pk, pj);
        let cross = vcross(eij, eik);
        let two_area = vdot(cross, cross).sqrt();
        let cot_i = vdot(eij, eik) / two_area;
        let cot_j = vdot(vsub(pi, pj), ejk) / two_area;
        let cot_k = vdot(vsub(pi, pk), vsub(pj, pk)) / two_area;
        let half_s = 0.5 * s;
        let mut add_edge = |a: usize, b: usize, w: f64| {
            d[a][a] += w; d[b][b] += w; d[a][b] -= w; d[b][a] -= w;
        };
        add_edge(gi, gj, half_s * cot_k);
        add_edge(gj, gk, half_s * cot_i);
        add_edge(gk, gi, half_s * cot_j);
    }
    d
}

/// ‖D·x‖∞/(1+scale) for the ALL-node free-standing case.
#[allow(clippy::needless_range_loop)]
fn free_residual_scaled(d: &[Vec<f64>], nodes: &[[f64; 3]]) -> f64 {
    let n = nodes.len();
    let mut resid = 0.0_f64;
    let mut scale = 0.0_f64;
    for i in 0..n {
        for axis in 0..3 {
            let net: f64 = (0..n).map(|j| d[i][j] * nodes[j][axis]).sum();
            resid = resid.max(net.abs());
        }
    }
    for p in nodes { for c in p { scale = scale.max(c.abs()); } }
    resid / (1.0 + scale)
}

// ---------------------------------------------------------------------------
// Crate-root export check
// ---------------------------------------------------------------------------

/// Signature pin: proves `form_find_free_surfaces` is reachable from the crate
/// root with the expected argument types. A rename / signature change trips this
/// at compile time before any test logic runs.
#[test]
#[allow(clippy::type_complexity)]
fn form_find_free_surfaces_is_exported_from_crate_root() {
    let _: fn(
        &[[f64; 3]],
        &[(usize, usize)],
        &[MemberKind],
        &[(usize, usize, usize)],
        &[f64],
        &ForceDensitySpec,
    ) -> Result<FreeFormResult, FreeFormError> = form_find_free_surfaces;
}

// ---------------------------------------------------------------------------
// Combined golden
// ---------------------------------------------------------------------------

const EQUIL_TOL: f64 = 1e-9;

/// Combined triplex + isotropic membrane σ=0.2 golden.
///
/// The equilateral prism geometry makes the cotangent-Laplacian a UNIFORM
/// horizontal-edge reweighting (σ/(2√3) per edge), equivalent to boosting the
/// horizontal cable densities. Combined D has nullity 4 for any modest σ>0, so
/// the search finds it and the fixed point converges to ~machine-zero residual.
#[test]
fn combined_prism_membrane_golden() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_prism_guess();
    let surfaces = prism_surfaces();
    let sigma = 0.2_f64;
    let sigmas = vec![sigma; 2];

    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };

    let result = form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
        .expect("combined prism+membrane must form-find");

    assert!(result.converged, "combined solve must converge");
    assert_eq!(result.nullity, 4, "combined D must have nullity 4");

    // surface_stresses echo.
    assert_eq!(result.surface_stresses.len(), 2);
    for (t, &s) in result.surface_stresses.iter().enumerate() {
        assert!(
            (s - sigma).abs() < 1e-12,
            "surface_stresses[{t}] = {s}, expected {sigma}",
        );
    }

    // Force signs.
    for (idx, (&kind, &n_i)) in kinds.iter().zip(result.member_forces.iter()).enumerate() {
        match kind {
            MemberKind::Strut => assert!(n_i < 0.0, "strut {idx} N={n_i} must be compressive"),
            MemberKind::Cable => assert!(n_i > 0.0, "cable {idx} N={n_i} must be tensile"),
        }
    }

    // Primary honest signal: independent reassembly + all-node residual.
    let d = reassemble_d_combined(6, &members, &result.force_densities, &surfaces, &sigmas, &result.nodes);
    let resid = free_residual_scaled(&d, &result.nodes);
    assert!(
        resid < EQUIL_TOL,
        "combined equilibrium residual ‖D(x)·x‖∞/(1+scale) = {resid:.3e}, expected < {EQUIL_TOL:.0e}",
    );
}

// ---------------------------------------------------------------------------
// Explicit + surfaces coverage (task 6537) — the coverage gap the analysis
// confirmed: all prior tests in this file drive ForceDensitySpec::GroupRatios
// only. These two close that gap.
// ---------------------------------------------------------------------------

/// Headline RED: `Explicit` combined free-standing form-finding, from a
/// perturbed (non-symmetric) starting guess, must converge to the closed-form
/// combined self-stress. Today's kernel drives geometry only through the
/// eigen-gap descent, which pushes D toward nullity 4 but never moves the
/// coordinate vector x INTO null(D) — so this returns `SearchDidNotConverge`
/// pre-fix. MEASURED pre-fix: `Err(SearchDidNotConverge)`.
#[test]
fn combined_explicit_analytic_q_from_perturbed_guess_converges() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_prism_guess();
    let surfaces = prism_surfaces();
    let sigma = 0.2_f64;
    let sigmas = vec![sigma; 2];
    let q = analytic_combined_q(sigma);

    let spec = ForceDensitySpec::Explicit(q);

    let result = form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
        .expect("combined explicit q from perturbed guess must form-find");

    assert!(result.converged, "combined solve must converge");
    assert_eq!(result.nullity, 4, "combined D must have nullity 4");

    // surface_stresses echo.
    assert_eq!(result.surface_stresses.len(), 2);
    for (t, &s) in result.surface_stresses.iter().enumerate() {
        assert!(
            (s - sigma).abs() < 1e-12,
            "surface_stresses[{t}] = {s}, expected {sigma}",
        );
    }

    // Shape check: the closed form is derived from the premise that both
    // membrane triangles are equilateral at the solved equilibrium (every
    // cotangent in the derivation is cot(60°) = 1/√3) — assert that directly
    // against the recovered geometry. (A force-sign loop over
    // `result.member_forces` would be tautological here: the kernel computes
    // member_forces[i] = q[i] * len_i with len_i >= 0, and q's signs are
    // already validated against the kind contract before the solve can run,
    // so it cannot fail for any Explicit input that reaches this point — it
    // re-checks an input precondition, not a solver output.)
    let edge_len = |a: usize, b: usize| {
        let (pa, pb) = (result.nodes[a], result.nodes[b]);
        ((pa[0] - pb[0]).powi(2) + (pa[1] - pb[1]).powi(2) + (pa[2] - pb[2]).powi(2)).sqrt()
    };
    // Tolerance is 1e-6, not the kernel's 1e-9 EQUIL_TOL: EQUIL_TOL bounds the
    // combined-D residual ‖D(x)·x‖∞/(1+scale), a different (and not linearly
    // comparable) quantity from an edge-length difference recovered via
    // eigendecomposition + `recover_coordinates`'s normalisation. MEASURED
    // actual gap at O(0.45) edge-length scale: ~1e-9-1.7e-9 absolute — 1e-6
    // clears that with ~600x margin while staying ~1000x tighter than the
    // smallest guess perturbation (~1e-3), so a real regression still trips it.
    const EQUILATERAL_TOL: f64 = 1e-6;
    let (top01, top12, top20) = (edge_len(0, 1), edge_len(1, 2), edge_len(2, 0));
    assert!(
        (top01 - top12).abs() < EQUILATERAL_TOL && (top12 - top20).abs() < EQUILATERAL_TOL,
        "top membrane triangle must be equilateral: |01|={top01:.6e} |12|={top12:.6e} |20|={top20:.6e}",
    );
    assert!(
        top01 > 1e-6,
        "top membrane triangle must be non-degenerate, got edge length {top01:.3e}",
    );
    let (bot34, bot45, bot53) = (edge_len(3, 4), edge_len(4, 5), edge_len(5, 3));
    assert!(
        (bot34 - bot45).abs() < EQUILATERAL_TOL && (bot45 - bot53).abs() < EQUILATERAL_TOL,
        "bottom membrane triangle must be equilateral: |34|={bot34:.6e} |45|={bot45:.6e} |53|={bot53:.6e}",
    );
    assert!(
        bot34 > 1e-6,
        "bottom membrane triangle must be non-degenerate, got edge length {bot34:.3e}",
    );

    // Primary honest signal: independent reassembly + all-node residual.
    let d = reassemble_d_combined(6, &members, &result.force_densities, &surfaces, &sigmas, &result.nodes);
    let resid = free_residual_scaled(&d, &result.nodes);
    assert!(
        resid < EQUIL_TOL,
        "combined equilibrium residual ‖D(x)·x‖∞/(1+scale) = {resid:.3e}, expected < {EQUIL_TOL:.0e}",
    );
}

/// Negative guard: pins WHY the task's reported "analytic q = (-√3, 1, +√3)
/// stalls" is correct behaviour, not the defect. With σ = 0.2 present, the
/// membrane already contributes w = σ/(2√3) to every horizontal edge, so the
/// line-only self-stress leaves D_combined with the wrong nullity — it is not
/// a combined self-stress. Must fail from BOTH starting guesses, so the fix
/// landed for the headline test above does not manufacture a false positive
/// here.
#[test]
fn combined_explicit_line_only_q_is_not_a_combined_self_stress() {
    let (members, kinds) = triplex_topology();
    let surfaces = prism_surfaces();
    let sigma = 0.2_f64;
    let sigmas = vec![sigma; 2];
    let s = 3.0_f64.sqrt();
    let line_only_q = vec![
        -s, -s, -s, // struts
        1.0, 1.0, 1.0, // top horizontals
        1.0, 1.0, 1.0, // bottom horizontals
        s, s, s, // verticals
    ];
    let spec = ForceDensitySpec::Explicit(line_only_q);

    for (label, guess) in [
        ("canonical", canonical_prism()),
        ("perturbed", perturbed_prism_guess()),
    ] {
        assert_eq!(
            form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
                .unwrap_err(),
            FreeFormError::SearchDidNotConverge,
            "line-only q from {label} guess must not converge under σ={sigma} \
             (the membrane already contributes w=σ/(2√3) to every horizontal \
             edge, so this q leaves D_combined with the wrong nullity)",
        );
    }
}

/// Regression lock (task 6537): the Explicit combined form-find fix must hold
/// across a range of surface stresses σ and starting-guess perturbation
/// magnitudes, not just the single (σ=0.2, perturbed×1) cell exercised by
/// `combined_explicit_analytic_q_from_perturbed_guess_converges` above.
#[test]
fn combined_explicit_analytic_q_converges_across_sigma_and_perturbation() {
    let (members, kinds) = triplex_topology();
    let surfaces = prism_surfaces();

    for &sigma in &[0.05_f64, 0.2, 0.5, 2.0] {
        let sigmas = vec![sigma; 2];
        let q = analytic_combined_q(sigma);
        let spec = ForceDensitySpec::Explicit(q);

        let guesses: [(&str, Vec<[f64; 3]>); 4] = [
            ("canonical", canonical_prism()),
            ("scaled x1", perturbed_prism_guess_scaled(1.0)),
            ("scaled x10", perturbed_prism_guess_scaled(10.0)),
            ("scaled x50", perturbed_prism_guess_scaled(50.0)),
        ];

        for (label, guess) in guesses {
            let cell = format!("sigma={sigma}, guess={label}");
            let result =
                form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
                    .unwrap_or_else(|e| panic!("[{cell}] must form-find, got {e:?}"));

            assert!(result.converged, "[{cell}] combined solve must converge");
            assert_eq!(result.nullity, 4, "[{cell}] combined D must have nullity 4");

            let d = reassemble_d_combined(
                6,
                &members,
                &result.force_densities,
                &surfaces,
                &sigmas,
                &result.nodes,
            );
            let resid = free_residual_scaled(&d, &result.nodes);
            assert!(
                resid < EQUIL_TOL,
                "[{cell}] combined equilibrium residual ‖D(x)·x‖∞/(1+scale) = {resid:.3e}, \
                 expected < {EQUIL_TOL:.0e}",
            );
        }
    }
}
