//! Integration golden for Tensegrity-membrane γ (task 4414) — anchored isotropic
//! NFDM (Natural Force-Density) surface form-finding via
//! [`form_find_anchored_surfaces`].
//!
//! PRD reference: `docs/prds/v0_6/tensegrity-membrane.md` §4 (D1/D3) and §11 Q1.
//!
//! # The user-observable signal — and what is (and is NOT) asserted
//!
//! A pure isotropic membrane (surface stress σ > 0, no struts/cables) spanning a
//! fixed boundary form-finds to a **minimal surface** (area-stationary, vanishing
//! mean curvature). The classic closed-form minimal surface of revolution between
//! two coaxial rings is the **catenoid** `r(z) = c·cosh(z/c)`. This golden builds
//! a triangulated catenoid tube at a coarse and a finer resolution, seeds the
//! free interior nodes on the analytic catenoid plus a small deterministic
//! perturbation, solves, and checks two honest signals:
//!
//!   1. **Equilibrium residual** `‖(D x)_free‖ / scale ~ 1e-9` — the PRIMARY
//!      honest signal. At a force-density fixed point the net force on every free
//!      node is ~machine-zero. `D` here is reassembled INDEPENDENTLY of the
//!      kernel's faer path (a fresh cotangent-Laplacian in this file), so a
//!      flipped sign / wrong orientation / mis-scatter in the kernel surfaces as
//!      a large residual rather than hiding behind its own buggy assembly.
//!
//!   2. **Relative-L2 shape error** of the solved radii vs `c·cosh(z/c)`, below a
//!      MEASURED bound (the [`COARSE_SHAPE_REL_L2_BOUND`] /
//!      [`FINE_SHAPE_REL_L2_BOUND`] consts, set from the actually-observed error
//!      with margin). The radius is compared at each node's OWN solved `z`, so the
//!      metric is invariant to tangential drift along the surface (a node anywhere
//!      ON the catenoid scores ~0). A SOFT refinement check asserts the finer mesh
//!      does not score worse than the coarse one.
//!
//! ## Honesty mandate (PRD §11 Q1 / G6 — read before tightening anything)
//!
//! The catenoid is used as a **measured mesh-convergence bound above the
//! cotangent-Laplacian O(h²) discretization floor** — it is:
//!   - NEVER an exact coordinate (the discrete minimal surface ≠ the smooth
//!     catenoid; it only approaches it as `h → 0`),
//!   - NEVER a frozen tight tolerance (the bound is calibrated to the observed
//!     error, not a machine-epsilon constant),
//!   - NEVER a strict O(h²) convergence RATE (only a soft "refining does not
//!     worsen" monotonicity check),
//!   - NEVER hyperbolic-paraboloid exactness (the "soap film on a raised-corner
//!     square is an exact hypar" intuition is FALSE and is not asserted).
//!
//! Every assertion holds whether the kernel does a single linear solve or iterates
//! the cotangent fixed point (the method is left to the kernel, PRD §11 Q1).

use reify_solver_elastic::{MemberKind, form_find_anchored_surfaces};

// ---------------------------------------------------------------------------
// Catenoid geometry
// ---------------------------------------------------------------------------

/// Catenoid waist parameter `c` in `r(z) = c·cosh(z/c)` (minimum radius, at the
/// waist `z = 0`).
const C: f64 = 1.0;

/// Half-height: the two fixed boundary rings sit at `z = ±H`. With `c = 1`,
/// `H/c = 0.8` keeps the patch on the STABLE, globally-area-minimizing catenoid
/// branch (well below the `t·tanh(t) = 1 ⇒ t ≈ 1.2` existence limit and the
/// `t ≈ 1.056` Goldschmidt two-disk threshold), so the form-finding has a clean
/// minimum to settle into.
const H: f64 = 0.8;

/// Analytic catenoid radius at height `z`.
fn catenoid_radius(z: f64) -> f64 {
    C * (z / C).cosh()
}

/// Deterministic, RNG-free perturbation in `[-1, 1]` keyed on two indices. (RNG
/// is unavailable to workflow/golden code and would break reproducibility; a
/// fixed trig hash gives a stable, well-spread jitter.)
fn jitter(a: usize, b: usize) -> f64 {
    ((a as f64) * 12.9898 + (b as f64) * 78.233).sin()
}

/// Build a structured, triangulated catenoid tube.
///
/// `n_theta` nodes per azimuthal ring; `n_axial` axial segments (so `n_axial + 1`
/// rings, from `z = -H` to `z = +H`). The top and bottom rings are anchored
/// exactly on the analytic catenoid (the fixed boundary of the BVP); the interior
/// rings are free, seeded on the analytic catenoid plus a small deterministic
/// radial + axial perturbation so the solve must genuinely move them back.
///
/// Returns `(nodes, surfaces, anchors, free_indices)`.
#[allow(clippy::type_complexity)]
fn build_catenoid_tube(
    n_theta: usize,
    n_axial: usize,
    perturb: f64,
) -> (
    Vec<[f64; 3]>,
    Vec<(usize, usize, usize)>,
    Vec<usize>,
    Vec<usize>,
) {
    let n_rings = n_axial + 1;
    let node_id = |ring: usize, j: usize| ring * n_theta + (j % n_theta);
    let mut nodes = vec![[0.0_f64; 3]; n_rings * n_theta];
    let mut anchors = Vec::new();
    let mut free = Vec::new();

    for ring in 0..n_rings {
        let z = -H + 2.0 * H * (ring as f64) / (n_axial as f64);
        let r_true = catenoid_radius(z);
        let is_boundary = ring == 0 || ring == n_rings - 1;
        for j in 0..n_theta {
            let theta = 2.0 * std::f64::consts::PI * (j as f64) / (n_theta as f64);
            let id = node_id(ring, j);
            if is_boundary {
                // Anchors stay EXACTLY on the catenoid — the fixed BVP data.
                nodes[id] = [r_true * theta.cos(), r_true * theta.sin(), z];
                anchors.push(id);
            } else {
                // Free interior node: seed on the catenoid + small perturbation
                // (radial via jitter(ring,j), axial via the swapped-index phase).
                let r = r_true + perturb * jitter(ring, j);
                let dz = 0.5 * perturb * jitter(j, ring);
                nodes[id] = [r * theta.cos(), r * theta.sin(), z + dz];
                free.push(id);
            }
        }
    }

    // Triangulate each quad between adjacent rings (consistent diagonal split).
    let mut surfaces = Vec::new();
    for ring in 0..n_axial {
        for j in 0..n_theta {
            let a = node_id(ring, j);
            let b = node_id(ring, j + 1);
            let c = node_id(ring + 1, j);
            let d = node_id(ring + 1, j + 1);
            surfaces.push((a, b, c));
            surfaces.push((b, d, c));
        }
    }

    (nodes, surfaces, anchors, free)
}

// ---------------------------------------------------------------------------
// Self-contained vector + cotangent-Laplacian helpers.
//
// Integration tests cannot reach the kernel's private `triangle_cotangent_-
// laplacian`, so the equilibrium residual is checked against a FRESH assembly
// here — an independent cross-check that the kernel's faer solve reached genuine
// force-density equilibrium of the correctly-assembled D (not equilibrium of a
// privately-buggy one).
// ---------------------------------------------------------------------------

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// Independent per-triangle isotropic cotangent-Laplacian `σ·L_T` (rows/cols
/// `0=i, 1=j, 2=k`): edge weight opposite vertex `v` is `(σ/2)·cot(θ_v)` with
/// `cot(θ_v) = (e_a·e_b)/(2·Area)`, scattered by the rank-1 edge pattern.
fn cot_laplacian_local(pi: [f64; 3], pj: [f64; 3], pk: [f64; 3], sigma: f64) -> [[f64; 3]; 3] {
    let eij = sub(pj, pi);
    let eik = sub(pk, pi);
    let eji = sub(pi, pj);
    let ejk = sub(pk, pj);
    let eki = sub(pi, pk);
    let ekj = sub(pj, pk);

    let two_area = norm(cross(eij, eik));
    let cot_i = dot(eij, eik) / two_area;
    let cot_j = dot(eji, ejk) / two_area;
    let cot_k = dot(eki, ekj) / two_area;

    let hs = 0.5 * sigma;
    let w_ij = hs * cot_k; // edge i–j opposite k
    let w_jk = hs * cot_i; // edge j–k opposite i
    let w_ki = hs * cot_j; // edge k–i opposite j

    let mut l = [[0.0_f64; 3]; 3];
    let mut add = |a: usize, b: usize, w: f64| {
        l[a][a] += w;
        l[b][b] += w;
        l[a][b] -= w;
        l[b][a] -= w;
    };
    add(0, 1, w_ij);
    add(1, 2, w_jk);
    add(2, 0, w_ki);
    l
}

/// Reassemble the global surface force-density matrix `D = Σ_T σ_T·L_T` (dense)
/// at the given geometry — independently of the kernel's faer path.
fn assemble_surface_d(
    n: usize,
    surfaces: &[(usize, usize, usize)],
    sigmas: &[f64],
    nodes: &[[f64; 3]],
) -> Vec<Vec<f64>> {
    let mut d = vec![vec![0.0_f64; n]; n];
    for (&(i, j, k), &s) in surfaces.iter().zip(sigmas.iter()) {
        let l = cot_laplacian_local(nodes[i], nodes[j], nodes[k], s);
        let idx = [i, j, k];
        for a in 0..3 {
            for b in 0..3 {
                d[idx[a]][idx[b]] += l[a][b];
            }
        }
    }
    d
}

/// Max-norm of the free-node equilibrium residual `(D x)_free`, scaled by the
/// coordinate magnitude so the bound is coordinate-scale-free.
#[allow(clippy::needless_range_loop)]
fn equilibrium_residual_scaled(d: &[Vec<f64>], nodes: &[[f64; 3]], is_anchor: &[bool]) -> f64 {
    let n = nodes.len();
    let mut resid = 0.0_f64;
    let mut scale = 0.0_f64;
    for i in 0..n {
        if is_anchor[i] {
            continue;
        }
        for axis in 0..3 {
            let mut net = 0.0;
            for j in 0..n {
                net += d[i][j] * nodes[j][axis];
            }
            resid = resid.max(net.abs());
        }
    }
    for p in nodes {
        for c in p {
            scale = scale.max(c.abs());
        }
    }
    resid / (1.0 + scale)
}

/// Relative-L2 error of the solved radii vs the analytic catenoid radius at each
/// node's OWN solved `z` (so the metric is invariant to tangential drift along
/// the surface). Measured over the FREE nodes only — anchors are exact by
/// construction and would dilute the metric.
fn rel_l2_shape_error(nodes: &[[f64; 3]], free: &[usize]) -> f64 {
    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for &i in free {
        let p = nodes[i];
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
        let r_true = catenoid_radius(p[2]);
        num += (r - r_true).powi(2);
        den += r_true * r_true;
    }
    (num / den).sqrt()
}

// ---------------------------------------------------------------------------
// Solve + measure
// ---------------------------------------------------------------------------

struct MeshResult {
    converged: bool,
    resid: f64,
    shape_err: f64,
    n_free: usize,
    n_tri: usize,
}

fn solve_and_measure(n_theta: usize, n_axial: usize, sigma: f64) -> MeshResult {
    // Small perturbation off the catenoid — large enough that the solve must do
    // real work, small enough to stay in the catenoid's basin of attraction.
    const PERTURB: f64 = 0.02;
    let (nodes, surfaces, anchors, free) = build_catenoid_tube(n_theta, n_axial, PERTURB);
    let sigmas = vec![sigma; surfaces.len()];
    // Pure membrane: no struts / cables.
    let members: Vec<(usize, usize)> = vec![];
    let kinds: Vec<MemberKind> = vec![];
    let q: Vec<f64> = vec![];

    let solve =
        form_find_anchored_surfaces(&nodes, &members, &kinds, &q, &surfaces, &sigmas, &anchors)
            .expect("a well-posed σ>0 catenoid membrane must be feasible");

    let mut is_anchor = vec![false; nodes.len()];
    for &a in &anchors {
        is_anchor[a] = true;
    }
    let d = assemble_surface_d(nodes.len(), &surfaces, &sigmas, &solve.nodes);
    let resid = equilibrium_residual_scaled(&d, &solve.nodes, &is_anchor);
    let shape_err = rel_l2_shape_error(&solve.nodes, &free);

    MeshResult {
        converged: solve.converged,
        resid,
        shape_err,
        n_free: free.len(),
        n_tri: surfaces.len(),
    }
}

// ---------------------------------------------------------------------------
// Calibrated bounds (set in step-6 from the MEASURED error + margin)
// ---------------------------------------------------------------------------

/// Equilibrium-residual bound (the PRIMARY honest signal). A force-density fixed
/// point reaches ~machine precision, so 1e-9 leaves wide margin while still
/// catching a non-converged or mis-assembled solve.
const EQUIL_TOL: f64 = 1e-9;

/// Relative-L2 catenoid shape bound at the COARSE (16×3) mesh.
///
/// MEASURED-then-bounded: the converged coarse-mesh error is `7.27e-3` (the
/// cotangent-Laplacian O(h²) discretization floor at this mesh — the discrete
/// minimal surface ≠ the smooth catenoid), so the bound is set ~1.4× above it.
/// It is NOT a frozen tight tolerance and NOT an exact coordinate: a correct
/// assembly lands well under it, a broken one (flipped Laplacian sign, wrong
/// scatter) blows the error up by orders of magnitude or fails to converge.
const COARSE_SHAPE_REL_L2_BOUND: f64 = 1.0e-2;

/// Relative-L2 catenoid shape bound at the FINE (32×6) mesh.
///
/// MEASURED-then-bounded: the converged fine-mesh error is `1.59e-3` (≈¼ the
/// coarse error — the O(h²) floor shrinks ~4× when `h` halves), so the bound is
/// set ~1.6× above it and comfortably below [`COARSE_SHAPE_REL_L2_BOUND`]. Same
/// honesty caveat: measured floor + margin, never a frozen tolerance.
const FINE_SHAPE_REL_L2_BOUND: f64 = 2.5e-3;

/// Multiplicative slack for the SOFT refinement check. The finer mesh should not
/// score worse than the coarse one; a small slack absorbs the non-monotone noise
/// of two unrelated meshes without asserting a strict convergence rate.
const REFINE_SLACK: f64 = 1e-3;

#[test]
fn catenoid_membrane_form_finds_minimal_surface_within_measured_bound() {
    let sigma = 1.0;

    // Coarse and finer triangulated catenoid tubes. The azimuthal resolution is
    // ~4.5× the axial so the quads are SQUARE-ish (azimuthal spacing 2πr/n_theta
    // ≈ axial spacing 2H/n_axial): square quads split into right-ish triangles
    // whose interior angles stay ≤ 90°, keeping every cotangent weight
    // non-negative — a wide quad (long azimuthal chord) would make strongly
    // obtuse triangles, negative weights, and an unstable fixed point. The fine
    // mesh halves h in BOTH directions (clean O(h²) refinement).
    let coarse = solve_and_measure(16, 3, sigma);
    let fine = solve_and_measure(32, 6, sigma);

    // Print every measured number BEFORE asserting, so a single (RED) run
    // reveals the values step-6 calibrates the bounds against.
    eprintln!(
        "coarse mesh (free={}, tri={}): converged={} resid={:e} shape_rel_l2={:e}",
        coarse.n_free, coarse.n_tri, coarse.converged, coarse.resid, coarse.shape_err,
    );
    eprintln!(
        "fine   mesh (free={}, tri={}): converged={} resid={:e} shape_rel_l2={:e}",
        fine.n_free, fine.n_tri, fine.converged, fine.resid, fine.shape_err,
    );

    // --- (1) Convergence + equilibrium residual (PRIMARY honest signal). ---
    assert!(coarse.converged, "coarse catenoid solve must converge");
    assert!(fine.converged, "fine catenoid solve must converge");
    assert!(
        coarse.resid < EQUIL_TOL,
        "coarse equilibrium residual {:e} must be < {:e}",
        coarse.resid,
        EQUIL_TOL,
    );
    assert!(
        fine.resid < EQUIL_TOL,
        "fine equilibrium residual {:e} must be < {:e}",
        fine.resid,
        EQUIL_TOL,
    );

    // --- (2) Catenoid shape error below the MEASURED bound (above the O(h²)
    // discretization floor — never an exact coordinate). ---
    assert!(
        coarse.shape_err < COARSE_SHAPE_REL_L2_BOUND,
        "coarse shape rel-L2 {:e} must be < measured bound {:e}",
        coarse.shape_err,
        COARSE_SHAPE_REL_L2_BOUND,
    );
    assert!(
        fine.shape_err < FINE_SHAPE_REL_L2_BOUND,
        "fine shape rel-L2 {:e} must be < measured bound {:e}",
        fine.shape_err,
        FINE_SHAPE_REL_L2_BOUND,
    );

    // --- (3) SOFT refinement: the finer mesh does not score worse (not a strict
    // O(h²) rate — only "refining does not worsen"). ---
    assert!(
        fine.shape_err <= coarse.shape_err * (1.0 + REFINE_SLACK),
        "finer mesh shape error {:e} must not exceed coarse {:e} (×(1+{:e}))",
        fine.shape_err,
        coarse.shape_err,
        REFINE_SLACK,
    );
}
