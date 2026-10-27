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
    ForceDensitySpec, FormDefect, FreeFormError, FreeFormResult, MemberKind,
    form_find_free_surfaces,
};
use reify_test_support::tensegrity_fixtures::{
    TALL_TRIPLEX_BOTTOM_Z, triplex_node_coords, zero_twist_prism_node_coords,
};
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Fixture — the complete triplex topology over the shared TALL prism
// (bottom z = -1, height 2), not T1b's unit-height one
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

/// The tall triplex displaced by the fixed `PERTURB` table scaled by `k`, so
/// regression tests can probe how far off-symmetry a starting guess can be
/// while still converging. Even at `k = 50` no node moves more than 0.1 from
/// the triplex equilibrium; `zero_twist_prism_node_coords` is the guess far
/// from it. `perturbed_prism_guess()` is the `k = 1.0` guess.
fn perturbed_prism_guess_scaled(k: f64) -> Vec<[f64; 3]> {
    const PERTURB: [[f64; 3]; 6] = [
        [0.0009, -0.0011, 0.0007],
        [-0.0013, 0.0006, 0.0010],
        [0.0012, 0.0008, -0.0009],
        [-0.0007, -0.0012, 0.0011],
        [0.0010, -0.0008, -0.0013],
        [-0.0011, 0.0013, 0.0006],
    ];
    triplex_node_coords(TALL_TRIPLEX_BOTTOM_Z)
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
// Shape-check helper — pins the closed-form derivation's own equilateral-
// triangle premise against the recovered geometry, for both membrane
// triangles at once. One copy in this file (used by the σ/guess table below).
// ---------------------------------------------------------------------------

/// Euclidean length of the edge between recovered node indices `a` and `b`.
fn edge_len(nodes: &[[f64; 3]], a: usize, b: usize) -> f64 {
    let (pa, pb) = (nodes[a], nodes[b]);
    ((pa[0] - pb[0]).powi(2) + (pa[1] - pb[1]).powi(2) + (pa[2] - pb[2]).powi(2)).sqrt()
}

/// Tolerance is 1e-6, not the kernel's 1e-9 `EQUIL_TOL`: `EQUIL_TOL` bounds
/// the combined-D residual ‖D(x)·x‖∞/(1+scale), a different (and not linearly
/// comparable) quantity from an edge-length difference recovered via
/// eigendecomposition + `recover_coordinates`'s normalisation. MEASURED
/// (σ=0.2) actual gap at O(0.45) edge-length scale: ~1e-9-1.7e-9 absolute —
/// 1e-6 clears that with ~600x margin while staying ~1000x tighter than the
/// smallest guess perturbation (~1e-3), so a real regression still trips it.
/// Applied uniformly across every σ cell in the table below rather than
/// re-measured per σ, since `EQUIL_TOL` (the residual bound each cell is
/// independently gated on) does not vary with σ either.
const EQUILATERAL_TOL: f64 = 1e-6;

/// Assert both membrane triangles ({0,1,2} top, {3,4,5} bottom) are
/// equilateral and non-degenerate in the recovered geometry `nodes` — the
/// closed-form `analytic_combined_q` derivation's own premise (every
/// cotangent in the surface stencil is cot(60°) = 1/√3 only if both
/// triangles are equilateral), checked against the solver's actual output
/// rather than assumed. `cell` labels the assertion messages with the
/// (σ, guess) pair under test.
fn assert_prism_membranes_equilateral(nodes: &[[f64; 3]], cell: &str) {
    let (top01, top12, top20) = (
        edge_len(nodes, 0, 1),
        edge_len(nodes, 1, 2),
        edge_len(nodes, 2, 0),
    );
    assert!(
        (top01 - top12).abs() < EQUILATERAL_TOL && (top12 - top20).abs() < EQUILATERAL_TOL,
        "[{cell}] top membrane triangle must be equilateral: |01|={top01:.6e} |12|={top12:.6e} |20|={top20:.6e}",
    );
    assert!(
        top01 > 1e-6,
        "[{cell}] top membrane triangle must be non-degenerate, got edge length {top01:.3e}",
    );
    let (bot34, bot45, bot53) = (
        edge_len(nodes, 3, 4),
        edge_len(nodes, 4, 5),
        edge_len(nodes, 5, 3),
    );
    assert!(
        (bot34 - bot45).abs() < EQUILATERAL_TOL && (bot45 - bot53).abs() < EQUILATERAL_TOL,
        "[{cell}] bottom membrane triangle must be equilateral: |34|={bot34:.6e} |45|={bot45:.6e} |53|={bot53:.6e}",
    );
    assert!(
        bot34 > 1e-6,
        "[{cell}] bottom membrane triangle must be non-degenerate, got edge length {bot34:.3e}",
    );
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
/// the search finds it and the fixed point converges to ~machine-zero residual,
/// from a guess near that equilibrium and from the zero-twist prism far from it.
#[test]
fn combined_prism_membrane_golden() {
    let (members, kinds) = triplex_topology();
    let surfaces = prism_surfaces();
    let sigma = 0.2_f64;
    let sigmas = vec![sigma; 2];

    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };

    for (label, guess) in [
        ("perturbed", perturbed_prism_guess()),
        (
            "zero-twist",
            zero_twist_prism_node_coords(TALL_TRIPLEX_BOTTOM_Z),
        ),
    ] {
        let result = form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
            .unwrap_or_else(|e| {
                panic!("[{label}] combined prism+membrane must form-find, got {e:?}")
            });

        assert!(result.converged, "[{label}] combined solve must converge");
        assert_eq!(
            result.nullity, 4,
            "[{label}] combined D must have nullity 4"
        );

        // surface_stresses echo.
        assert_eq!(
            result.surface_stresses.len(),
            2,
            "[{label}] surface_stresses length"
        );
        for (t, &s) in result.surface_stresses.iter().enumerate() {
            assert!(
                (s - sigma).abs() < 1e-12,
                "[{label}] surface_stresses[{t}] = {s}, expected {sigma}",
            );
        }

        // Force signs.
        for (idx, (&kind, &n_i)) in kinds.iter().zip(result.member_forces.iter()).enumerate() {
            match kind {
                MemberKind::Strut => {
                    assert!(
                        n_i < 0.0,
                        "[{label}] strut {idx} N={n_i} must be compressive"
                    )
                }
                MemberKind::Cable => {
                    assert!(n_i > 0.0, "[{label}] cable {idx} N={n_i} must be tensile")
                }
            }
        }

        // Primary honest signal: independent reassembly + all-node residual.
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
            "[{label}] combined equilibrium residual ‖D(x)·x‖∞/(1+scale) = {resid:.3e}, \
             expected < {EQUIL_TOL:.0e}",
        );
    }
}

// ---------------------------------------------------------------------------
// Explicit + surfaces coverage (task 6537) — the coverage gap the analysis
// confirmed: all prior tests in this file drive ForceDensitySpec::GroupRatios
// only. The tests below close that gap: a negative guard pinning why the
// task's reported line-only q is infeasible once σ>0, and a regression-locked
// table (further below) covering the positive (feasible-q) path across σ and
// starting-guess magnitude.
// ---------------------------------------------------------------------------

/// Negative guard: pins WHY the task's reported "analytic q = (-√3, 1, +√3)
/// stalls" is correct behaviour, not the defect. With σ = 0.2 present, the
/// membrane already contributes w = σ/(2√3) to every horizontal edge, so the
/// line-only self-stress leaves D_combined with the wrong nullity — it is not
/// a combined self-stress. Must fail from every starting guess, near the
/// triplex equilibrium and far from it, so the fix covered by the
/// positive-path regression table below does not manufacture a false positive
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
        ("triplex", triplex_node_coords(TALL_TRIPLEX_BOTTOM_Z)),
        ("perturbed", perturbed_prism_guess()),
        (
            "zero-twist",
            zero_twist_prism_node_coords(TALL_TRIPLEX_BOTTOM_Z),
        ),
    ] {
        let result = form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec);
        assert!(
            matches!(result, Err(FreeFormError::SearchDidNotConverge { .. })),
            "line-only q from {label} guess must not converge under σ={sigma}, got {result:?} \
             (the membrane already contributes w=σ/(2√3) to every horizontal \
             edge, so this q leaves D_combined with the wrong nullity)",
        );
    }
}

/// Regression lock (task 6537): the Explicit combined form-find fix must hold
/// across a range of surface stresses σ and starting guesses: the triplex
/// equilibrium itself, `PERTURB` displacements of it up to ×50, and the
/// zero-twist prism, 150° of strut azimuth away from it. This is the sole
/// positive-path (feasible-q) coverage for `Explicit` + surfaces in this file —
/// it subsumes what was originally a separate single-cell (σ=0.2, perturbed×1)
/// headline test: every cell here
/// additionally asserts the `surface_stresses` echo and that both recovered
/// membrane triangles are equilateral and non-degenerate (the closed-form
/// derivation's own premise), not just convergence + residual.
#[test]
fn combined_explicit_analytic_q_converges_across_sigma_and_perturbation() {
    let (members, kinds) = triplex_topology();
    let surfaces = prism_surfaces();

    for &sigma in &[0.05_f64, 0.2, 0.5, 1.0, 2.0] {
        let sigmas = vec![sigma; 2];
        let q = analytic_combined_q(sigma);
        let spec = ForceDensitySpec::Explicit(q);

        let guesses: [(&str, Vec<[f64; 3]>); 5] = [
            ("triplex", triplex_node_coords(TALL_TRIPLEX_BOTTOM_Z)),
            ("scaled x1", perturbed_prism_guess_scaled(1.0)),
            ("scaled x10", perturbed_prism_guess_scaled(10.0)),
            ("scaled x50", perturbed_prism_guess_scaled(50.0)),
            (
                "zero-twist",
                zero_twist_prism_node_coords(TALL_TRIPLEX_BOTTOM_Z),
            ),
        ];

        for (label, guess) in guesses {
            let cell = format!("sigma={sigma}, guess={label}");
            let result =
                form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec)
                    .unwrap_or_else(|e| panic!("[{cell}] must form-find, got {e:?}"));

            assert!(result.converged, "[{cell}] combined solve must converge");
            assert_eq!(result.nullity, 4, "[{cell}] combined D must have nullity 4");

            // surface_stresses echo.
            assert_eq!(
                result.surface_stresses.len(),
                2,
                "[{cell}] surface_stresses length",
            );
            for (t, &s) in result.surface_stresses.iter().enumerate() {
                assert!(
                    (s - sigma).abs() < 1e-12,
                    "[{cell}] surface_stresses[{t}] = {s}, expected {sigma}",
                );
            }

            // Shape check (see `assert_prism_membranes_equilateral`). (A
            // force-sign loop over `result.member_forces` would be
            // tautological here: the kernel computes
            // member_forces[i] = q[i] * len_i with len_i >= 0, and q's signs
            // are already validated against the kind contract before the
            // solve can run, so it cannot fail for any Explicit input that
            // reaches this point — it re-checks an input precondition, not a
            // solver output.)
            assert_prism_membranes_equilateral(&result.nodes, &cell);

            // Primary honest signal: independent reassembly + all-node residual.
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

// ---------------------------------------------------------------------------
// Certification of the combined fixed point (task 7801): a model that is not
// ONE connected structure is never a converged free-standing form. Every
// fixture starts from the δ golden's own converged state, where a
// residual-only stop test would report Ok(converged) on it.
// ---------------------------------------------------------------------------

const DELTA_SIGMA: f64 = 0.2;

fn delta_group_ratios_spec() -> ForceDensitySpec {
    ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    }
}

/// The δ GroupRatios golden's converged `(x*, q*)`, solved once per test binary.
fn delta_fixed_point() -> (Vec<[f64; 3]>, Vec<f64>) {
    static FIXED_POINT: OnceLock<(Vec<[f64; 3]>, Vec<f64>)> = OnceLock::new();
    FIXED_POINT
        .get_or_init(|| {
            let (members, kinds) = triplex_topology();
            let golden = form_find_free_surfaces(
                &perturbed_prism_guess(),
                &members,
                &kinds,
                &prism_surfaces(),
                &[DELTA_SIGMA; 2],
                &delta_group_ratios_spec(),
            )
            .expect("δ GroupRatios golden must form-find");
            (golden.nodes, golden.force_densities)
        })
        .clone()
}

fn assert_two_piece_non_convergence(result: &Result<FreeFormResult, FreeFormError>) {
    assert!(
        matches!(
            result,
            Err(FreeFormError::SearchDidNotConverge {
                defect: Some(FormDefect { components: 2, .. }),
            })
        ),
        "a model of two connected pieces must not converge, got {result:?}",
    );
}

#[test]
fn combined_explicit_isolated_node_is_not_certified() {
    let (members, kinds) = triplex_topology();
    let (mut nodes, q_star) = delta_fixed_point();
    nodes.push([0.3, 0.2, 0.4]);

    let result = form_find_free_surfaces(
        &nodes,
        &members,
        &kinds,
        &prism_surfaces(),
        &[DELTA_SIGMA; 2],
        &ForceDensitySpec::Explicit(q_star),
    );
    assert_two_piece_non_convergence(&result);
}

#[test]
fn combined_group_ratios_isolated_node_is_not_certified() {
    let (members, kinds) = triplex_topology();
    let (mut nodes, _q_star) = delta_fixed_point();
    nodes.push([0.3, 0.2, 0.4]);

    let result = form_find_free_surfaces(
        &nodes,
        &members,
        &kinds,
        &prism_surfaces(),
        &[DELTA_SIGMA; 2],
        &delta_group_ratios_spec(),
    );
    assert_two_piece_non_convergence(&result);
}

#[test]
fn combined_explicit_disconnected_twin_is_not_certified() {
    const OFFSET: usize = 6;
    let (mut members, mut kinds) = triplex_topology();
    let mut surfaces = prism_surfaces();
    let (mut nodes, mut q) = delta_fixed_point();

    let twin_nodes: Vec<[f64; 3]> = nodes.iter().map(|p| [p[0] + 5.0, p[1], p[2]]).collect();
    nodes.extend(twin_nodes);
    let twin_members: Vec<(usize, usize)> = members
        .iter()
        .map(|&(a, b)| (a + OFFSET, b + OFFSET))
        .collect();
    members.extend(twin_members);
    kinds.extend(kinds.clone());
    let twin_surfaces: Vec<(usize, usize, usize)> = surfaces
        .iter()
        .map(|&(a, b, c)| (a + OFFSET, b + OFFSET, c + OFFSET))
        .collect();
    surfaces.extend(twin_surfaces);
    q.extend(q.clone());

    let result = form_find_free_surfaces(
        &nodes,
        &members,
        &kinds,
        &surfaces,
        &[DELTA_SIGMA; 4],
        &ForceDensitySpec::Explicit(q),
    );
    // Two exact copies of a nullity-4 form, converged at iteration 0, so the
    // equilibrium it reached is not a form. The nullity is structural, so it
    // is pinned along with the piece count.
    assert_eq!(
        result.unwrap_err(),
        FreeFormError::NotAForm(FormDefect {
            nullity: 8,
            components: 2,
        }),
    );
}

// A cable pair rather than a twin: under GroupRatios the twin ends in
// SingularRecovery from the inner search's forced-nullity recovery.
#[test]
fn combined_group_ratios_disconnected_cable_pair_is_not_certified() {
    let (mut members, mut kinds) = triplex_topology();
    let (mut nodes, _q_star) = delta_fixed_point();
    nodes.push([0.3, 0.2, 0.4]);
    nodes.push([0.3, 0.2, 0.9]);
    members.push((6, 7));
    kinds.push(MemberKind::Cable);
    let mut group_ids = triplex_group_ids();
    group_ids.push(3);
    let spec = ForceDensitySpec::GroupRatios {
        group_ids,
        seed_ratios: vec![-1.0, 1.0, 1.0, 1.0],
        reference_group: 1,
    };

    let result = form_find_free_surfaces(
        &nodes,
        &members,
        &kinds,
        &prism_surfaces(),
        &[DELTA_SIGMA; 2],
        &spec,
    );
    assert_two_piece_non_convergence(&result);
}
