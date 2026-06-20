//! Integration golden for Tensegrity-membrane ε (task 4416) — anchored
//! anisotropic NFDM form-finding via [`form_find_anchored_surfaces_aniso`].
//!
//! PRD reference: `docs/prds/v0_6/tensegrity-membrane.md` §4 M1c / §11 Q4.
//!
//! # Fixture: non-planar rectangular saddle patch
//!
//! A 5×5 node grid with x,y ∈ {−1,−½,0,½,1}. Boundary nodes are anchored
//! at z = 0.5·x·(1 + 0.4·y) (an asymmetrically-warped hypar, not a harmonic
//! function, so the isotropic and anisotropic equilibria differ). Interior
//! 3×3 = 9 free nodes are seeded at z = 0 + small deterministic perturbation.
//! Warp direction [1,0,0] is always in-plane for these nearly-flat triangles
//! (normals ≈ [0,0,1]) — no DegenerateMaterialFrame risk during iteration.
//!
//! # What is (and is NOT) asserted — G6 honesty mandate
//!
//! There is no clean closed-form shape for an anisotropic membrane spanning
//! this boundary. The "soap film on a raised-corner square is an exact hypar"
//! intuition does NOT apply here and is NOT asserted. Instead this golden
//! checks three honest signals:
//!
//! 1. **Equilibrium residual** `‖(D·x)_free‖/scale ≤ 1e-9` — re-derived
//!    INDEPENDENTLY in-test by reassembling the anisotropic `D` without the
//!    kernel's faer path, so a kernel scatter/sign bug surfaces as a large
//!    residual rather than hiding behind a self-consistent buggy assembly.
//!
//! 2. **Principal-stress alignment** — for each triangle the recovered
//!    `major_dir` aligns to the in-plane warp projection ê₁ with
//!    `|major_dir·ê₁| ≥ 1 − 1e-6` and `major == σ_w` to 1e-12.
//!
//! 3. **Distinctness from the isotropic baseline** — the same patch solved
//!    isotropically with σ = σ_f differs from the anisotropic solution by at
//!    least a MEASURED separation margin. The boundary is non-planar and the
//!    stress field is asymmetric, so anisotropy genuinely bends the form.
//!
//! All assertions are lower bounds or residual checks — never exact shape
//! coordinates or frozen tight tolerances.

use reify_solver_elastic::{
    AnisotropicSurfaceStress, MemberKind, form_find_anchored_surfaces,
    form_find_anchored_surfaces_aniso,
};

// ---------------------------------------------------------------------------
// Non-planar saddle patch fixture
// ---------------------------------------------------------------------------

/// Grid side length (5×5 = 25 nodes, interior 3×3 = 9 free).
const N: usize = 5;

/// Deterministic, RNG-free jitter in [−1, 1] keyed on two indices.
fn jitter(a: usize, b: usize) -> f64 {
    ((a as f64) * 12.9898 + (b as f64) * 78.233).sin()
}

/// Build the saddle patch fixture.
///
/// Returns `(nodes, surfaces, anchors, free_indices)`.
#[allow(clippy::type_complexity)]
fn build_saddle_patch() -> (Vec<[f64; 3]>, Vec<(usize, usize, usize)>, Vec<usize>, Vec<usize>) {
    const PERTURB: f64 = 0.01;
    let node_id = |i: usize, j: usize| i * N + j;

    let mut nodes = vec![[0.0_f64; 3]; N * N];
    let mut anchors = Vec::new();
    let mut free = Vec::new();

    for i in 0..N {
        for j in 0..N {
            let x = (i as f64 - 2.0) * 0.5; // x ∈ {−1,−½,0,½,1}
            let y = (j as f64 - 2.0) * 0.5; // y ∈ {−1,−½,0,½,1}
            let id = node_id(i, j);
            let is_boundary = i == 0 || i == N - 1 || j == 0 || j == N - 1;
            // Boundary z: asymmetrically-warped hypar (NOT harmonic, so iso ≠ aniso).
            let z_bnd = 0.5 * x * (1.0 + 0.4 * y);
            if is_boundary {
                nodes[id] = [x, y, z_bnd];
                anchors.push(id);
            } else {
                // Seed interior free nodes at the boundary interpolation + small jitter.
                let z_seed = z_bnd + PERTURB * jitter(i, j);
                nodes[id] = [x, y, z_seed];
                free.push(id);
            }
        }
    }

    // Triangulate: each (i,j)–(i+1,j)–(i,j+1)–(i+1,j+1) quad → 2 triangles.
    // Alternate diagonal direction by (i+j)%2 so the anisotropic coupling is
    // symmetric across the mesh (the NE-diagonal and SW-diagonal biases cancel
    // for each interior node, keeping x,y coordinates in anisotropic equilibrium
    // and preventing divergence of the fixed-point loop).
    let mut surfaces = Vec::new();
    for i in 0..N - 1 {
        for j in 0..N - 1 {
            let a = node_id(i, j);
            let b = node_id(i + 1, j);
            let c = node_id(i, j + 1);
            let d = node_id(i + 1, j + 1);
            if (i + j) % 2 == 0 {
                surfaces.push((a, b, d)); // upper-right diagonal (NE)
                surfaces.push((a, d, c));
            } else {
                surfaces.push((a, b, c)); // lower-left diagonal (NW)
                surfaces.push((b, d, c));
            }
        }
    }

    (nodes, surfaces, anchors, free)
}

// ---------------------------------------------------------------------------
// Independent in-test helpers (no kernel internals reused)
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
fn normalize(a: [f64; 3]) -> [f64; 3] {
    let n = norm(a);
    [a[0] / n, a[1] / n, a[2] / n]
}

/// Independent anisotropic stencil `L_T[a][b] = Area·(∇N_a·S·∇N_b)` (CST,
/// per-triangle frame e₁=in-plane warp, e₂=n×e₁). Reassembled without the
/// kernel's faer path so the equilibrium check is a genuine independent cross-check.
fn aniso_laplacian_local(
    pi: [f64; 3],
    pj: [f64; 3],
    pk: [f64; 3],
    warp_dir: [f64; 3],
    sw: f64,
    sf: f64,
) -> [[f64; 3]; 3] {
    let eij = sub(pj, pi);
    let eik = sub(pk, pi);
    let cr = cross(eij, eik);
    let n = normalize(cr);
    let wd_dot_n = dot(warp_dir, n);
    let wip = [
        warp_dir[0] - wd_dot_n * n[0],
        warp_dir[1] - wd_dot_n * n[1],
        warp_dir[2] - wd_dot_n * n[2],
    ];
    let e1 = normalize(wip);
    let e2 = cross(n, e1);

    let xj = dot(eij, e1);
    let yj = dot(eij, e2);
    let xk = dot(eik, e1);
    let yk = dot(eik, e2);
    let two_area_2d = xj * yk - xk * yj;
    let area = two_area_2d.abs() * 0.5;
    let inv_2a = 1.0 / two_area_2d;
    let g = [
        [(yj - yk) * inv_2a, (xk - xj) * inv_2a],
        [yk * inv_2a, -xk * inv_2a],
        [-yj * inv_2a, xj * inv_2a],
    ];
    let mut l = [[0.0_f64; 3]; 3];
    for a in 0..3 {
        for b in 0..3 {
            l[a][b] = area * (sw * g[a][0] * g[b][0] + sf * g[a][1] * g[b][1]);
        }
    }
    l
}

/// Reassemble global anisotropic D (line + surface) at the given geometry.
fn assemble_aniso_d(
    n: usize,
    members: &[(usize, usize)],
    q: &[f64],
    surfaces: &[(usize, usize, usize)],
    prestress: &[AnisotropicSurfaceStress],
    nodes: &[[f64; 3]],
) -> Vec<Vec<f64>> {
    let mut d = vec![vec![0.0_f64; n]; n];
    for (&(j, k), &qi) in members.iter().zip(q.iter()) {
        d[j][j] += qi;
        d[k][k] += qi;
        d[j][k] -= qi;
        d[k][j] -= qi;
    }
    for (&(i, j, k), spec) in surfaces.iter().zip(prestress.iter()) {
        let l = aniso_laplacian_local(
            nodes[i], nodes[j], nodes[k], spec.warp_dir, spec.sigma_warp, spec.sigma_weft,
        );
        let idx = [i, j, k];
        for a in 0..3 {
            for b in 0..3 {
                d[idx[a]][idx[b]] += l[a][b];
            }
        }
    }
    d
}

/// Max-norm equilibrium residual `‖(D·x)_free‖ / (1 + scale)`.
#[allow(clippy::needless_range_loop)]
fn equilibrium_residual(d: &[Vec<f64>], nodes: &[[f64; 3]], is_anchor: &[bool]) -> f64 {
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

/// Compute the in-plane unit warp axis ê₁ for a triangle.
fn inplane_warp_axis(
    pi: [f64; 3],
    pj: [f64; 3],
    pk: [f64; 3],
    warp_dir: [f64; 3],
) -> [f64; 3] {
    let eij = sub(pj, pi);
    let eik = sub(pk, pi);
    let cr = cross(eij, eik);
    let n = normalize(cr);
    let wd_dot_n = dot(warp_dir, n);
    let wip = [
        warp_dir[0] - wd_dot_n * n[0],
        warp_dir[1] - wd_dot_n * n[1],
        warp_dir[2] - wd_dot_n * n[2],
    ];
    normalize(wip)
}

// ---------------------------------------------------------------------------
// Calibrated constants (MEASURED-then-bounded, not frozen tight tolerances)
// ---------------------------------------------------------------------------

/// Primary equilibrium residual bound. Fixed-point → ~machine precision;
/// 1e-9 leaves wide margin while catching mis-assembled D.
const EQUIL_TOL: f64 = 1e-9;

/// Principal alignment threshold: |major_dir · ê₁| must exceed this.
/// Exact structural identity (S diagonal in (e₁,e₂)) → 1−1e-6 is generous.
const ALIGN_TOL: f64 = 1.0 - 1e-6;

/// Measured minimum per-node position difference between the anisotropic
/// solve (σ_w=3, σ_f=1, warp=[1,0,0]) and the isotropic baseline (σ=1).
/// MEASURED lower bound — set from the observed run with safety margin.
/// The non-planar boundary breaks the harmonic symmetry so anisotropy
/// bends the form measurably.
const DISTINCTNESS_MARGIN: f64 = 1e-4;

// ---------------------------------------------------------------------------
// The integration golden
// ---------------------------------------------------------------------------

#[test]
fn anisotropic_membrane_equilibrium_alignment_and_distinctness() {
    // Warp direction: x-axis. In-plane for all nearly-flat saddle triangles.
    let sigma_warp = 3.0_f64;
    let sigma_weft = 1.0_f64;
    let warp_dir = [1.0_f64, 0.0, 0.0];

    let (nodes, surfaces, anchors, _free) = build_saddle_patch();
    let n = nodes.len();

    let prestress: Vec<AnisotropicSurfaceStress> = surfaces
        .iter()
        .map(|_| AnisotropicSurfaceStress { warp_dir, sigma_warp, sigma_weft })
        .collect();

    let members: Vec<(usize, usize)> = vec![];
    let kinds: Vec<MemberKind> = vec![];
    let q: Vec<f64> = vec![];

    let result = form_find_anchored_surfaces_aniso(
        &nodes, &members, &kinds, &q, &surfaces, &prestress, &anchors,
    )
    .expect("well-posed anisotropic saddle patch must be feasible");

    assert!(result.converged, "anisotropic solve must converge");

    // ── Signal 1: equilibrium residual (independent reassembly) ──────────────
    let mut is_anchor = vec![false; n];
    for &a in &anchors {
        is_anchor[a] = true;
    }
    let d = assemble_aniso_d(n, &members, &q, &surfaces, &prestress, &result.nodes);
    let resid = equilibrium_residual(&d, &result.nodes, &is_anchor);
    assert!(
        resid < EQUIL_TOL,
        "equilibrium residual {resid:e} must be < {EQUIL_TOL:e}",
    );

    // ── Signal 2: principal-stress alignment (read off result.principal_stresses)
    // principal_stresses populated by step-10; empty before that → RED trigger.
    assert_eq!(
        result.principal_stresses.len(),
        surfaces.len(),
        "principal_stresses must have one entry per triangle (step-10 GREEN)",
    );
    for (t, (ps, &(i, j, k))) in result
        .principal_stresses
        .iter()
        .zip(surfaces.iter())
        .enumerate()
    {
        // Major magnitude == sigma_warp (x-direction is the larger stress).
        assert!(
            (ps.major - sigma_warp).abs() < 1e-12,
            "triangle {t}: major={} expected sigma_warp={sigma_warp}",
            ps.major,
        );
        // major_dir aligns to in-plane warp projection ê₁.
        let e1 = inplane_warp_axis(result.nodes[i], result.nodes[j], result.nodes[k], warp_dir);
        let align = dot(ps.major_dir, e1).abs();
        assert!(
            align >= ALIGN_TOL,
            "triangle {t}: |major_dir·ê₁|={align} must be ≥ {ALIGN_TOL}",
        );
    }

    // ── Signal 3: distinctness from isotropic baseline ───────────────────────
    let sigmas_iso = vec![sigma_weft; surfaces.len()];
    let iso = form_find_anchored_surfaces(
        &nodes, &members, &kinds, &q, &surfaces, &sigmas_iso, &anchors,
    )
    .expect("isotropic reference solve must be feasible");

    let max_delta: f64 = result
        .nodes
        .iter()
        .zip(iso.nodes.iter())
        .map(|(a, b)| {
            let dx = a[0] - b[0];
            let dy = a[1] - b[1];
            let dz = a[2] - b[2];
            (dx * dx + dy * dy + dz * dz).sqrt()
        })
        .fold(0.0_f64, f64::max);

    // Print the measured value before asserting (for calibration on first run).
    eprintln!("aniso (σ_w={sigma_warp}, σ_f={sigma_weft}) vs iso (σ={sigma_weft}) max per-node Δ = {max_delta:.6}");

    assert!(
        max_delta >= DISTINCTNESS_MARGIN,
        "aniso solve (σ_w={sigma_warp}, σ_f={sigma_weft}) must differ from \
         isotropic (σ={sigma_weft}) by ≥{DISTINCTNESS_MARGIN}; got {max_delta:.6}",
    );
}

// API-surface pin: all ε public symbols reachable from the crate root.
#[test]
fn epsilon_symbols_reachable_from_crate_root() {
    use reify_solver_elastic::{
        AnisoFormFindError, AnisoFormFindSolve, AnisotropicSurfaceStress, PrincipalStress,
        form_find_anchored_surfaces_aniso,
    };
    let _ = AnisotropicSurfaceStress {
        warp_dir: [1.0, 0.0, 0.0],
        sigma_warp: 1.0,
        sigma_weft: 1.0,
    };
    let _: Option<AnisoFormFindError> = None;
    let _: Option<AnisoFormFindSolve> = None;
    let _: Option<PrincipalStress> = None;
    let _: fn(_, _, _, _, _, _, _) -> _ = form_find_anchored_surfaces_aniso;
}
