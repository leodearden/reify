//! Force-Density (FD) form-finding kernel — anchored case (Tensegrity T1a).
//!
//! # PRD reference
//!
//! `docs/prds/v0_6/tensegrity-structures.md` §4 / Tier-1 leaf T1a. This is the
//! pure numeric kernel behind the `solver::form_find` ComputeNode target: given
//! a tensegrity's node coordinates, member connectivity (struts then cables),
//! per-member force densities `q`, and a set of anchored node indices, it solves
//! the reduced linear Force-Density system for the free-node coordinates.
//!
//! # Method (anchored case)
//!
//! For `m` members over `N` nodes, the branch-node connectivity matrix `C` is
//! `m×N` with `+1` at the start node `j` and `−1` at the end node `k` of each
//! member. With `Q = diag(q)`, the force-density stiffness is `D = Cᵀ Q C`
//! (`N×N`). Partitioning node indices into free `F` and anchored `A`, the
//! prestress-only equilibrium (no external load) is
//!
//! ```text
//!     D_ff · X_f = − D_fa · X_a
//! ```
//!
//! solved per coordinate axis. All three axes share the same `D_ff` factor and
//! are solved together as an `|F|×3` right-hand side.
//!
//! # Sign convention
//!
//! Cables carry tension (`q > 0`), struts carry compression (`q < 0`). See the
//! validation guards in [`form_find_anchored`] for the enforced contract; the
//! free-standing eigenvalue/ratio form-finding variant is deferred to T1b.
//!
//! # Scope
//!
//! Anchored, explicit-`q`, no-load form-finding only. Force-density *ratio*
//! auto-scaling, the free-standing (unanchored) eigenvalue case, external loads,
//! and stability/buckling analysis are out of scope (tracked as T1b/T2/T3).

use faer::Mat;
use faer::linalg::solvers::Solve;

/// Member type tag. Determines the enforced sign of the member's force density:
/// cables carry tension (`q > 0`), struts carry compression (`q < 0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    /// Compression member (force density `q < 0`).
    Strut,
    /// Tension member (force density `q > 0`).
    Cable,
}

/// Reason an anchored form-find solve is infeasible. Surfaced by the trampoline
/// as an `E_FormFindInfeasible` diagnostic (PRD §8.1 contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormFindError {
    /// A member's force density violates its kind's sign contract
    /// (a cable with `q ≤ 0` or a strut with `q ≥ 0`).
    SignViolation,
    /// The reduced force-density stiffness `D_ff` is singular or
    /// ill-conditioned (e.g. a free node with no path to any anchor).
    SingularReducedStiffness,
    /// Every node is anchored — there is no free node to solve for.
    EmptyFreeSet,
    /// Input arrays disagree in length (`members`, `kinds`, `q`).
    DimensionMismatch,
    /// A surface triangle is degenerate (collinear / zero-area corners), so its
    /// cotangent weights `cot(θ) = (e_a·e_b)/(2·Area)` diverge as `2·Area → 0`.
    /// Surfaced instead of assembling a NaN/∞ stencil. (γ / NFDM surfaces.)
    DegenerateTriangle,
    /// A membrane surface stress `σ ≤ 0` — a non-tension (slack/compressed)
    /// surface is infeasible prestress input, the surface analogue of a cable
    /// with `q ≤ 0`. (γ / NFDM surfaces.)
    NonTensionSurfaceStress,
    /// `surfaces` and `surface_stresses` disagree in length — each triangle
    /// needs exactly one isotropic σ. (γ / NFDM surfaces.)
    SurfaceCountMismatch,
}

/// Result of an anchored Force-Density form-find solve.
#[derive(Debug, Clone)]
pub struct FormFindSolve {
    /// Solved node coordinates in original node order (anchors unchanged,
    /// free nodes at their equilibrium positions).
    pub nodes: Vec<[f64; 3]>,
    /// Per-member axial force `Nᵢ = qᵢ · Lᵢ` on the solved geometry, in
    /// struts-then-cables member order.
    pub member_forces: Vec<f64>,
    /// Echo of the input force densities (struts-then-cables order).
    pub force_densities: Vec<f64>,
    /// Per-triangle echo of the prescribed isotropic surface stress σ (in
    /// `surfaces` declaration order); empty on the line-only path. The
    /// equilibrium form was solved holding these σ fixed, so the echo is the
    /// physically-carried per-triangle stress. (γ / NFDM surfaces.)
    pub surface_stresses: Vec<f64>,
    /// Whether the solve succeeded (non-singular `D_ff`).
    pub converged: bool,
}

/// Solve the anchored Force-Density form-finding problem.
///
/// `nodes` are the node coordinates (free-node entries are an unused initial
/// guess; anchor coordinates are read here). `members` are `(start, end)` index
/// pairs in struts-then-cables order, `kinds` tags each member, `q` is the
/// per-member force density (same order), and `anchors` lists the anchored node
/// indices.
///
/// Returns the solved [`FormFindSolve`] on success, or a [`FormFindError`]
/// describing why the input is infeasible.
pub fn form_find_anchored(
    nodes: &[[f64; 3]],
    members: &[(usize, usize)],
    kinds: &[MemberKind],
    q: &[f64],
    anchors: &[usize],
) -> Result<FormFindSolve, FormFindError> {
    let n = nodes.len();

    // ---- Up-front feasibility guards (PRD §8.1: infeasible input must yield a
    // clean diagnostic, never a silent wrong answer or a panic). ----

    // `members`, `kinds`, and `q` describe the same member set in the same
    // (struts-then-cables) order; disagreeing lengths mean the caller mis-built
    // the problem, so reject before indexing them together below.
    if members.len() != kinds.len() || members.len() != q.len() {
        return Err(FormFindError::DimensionMismatch);
    }

    // Sign convention (PRD §4), enforced as a HARD per-member constraint:
    // cables carry tension (q > 0), struts carry compression (q < 0). A
    // violation is *infeasible input*, not something to silently coerce — the
    // FD system would still factor and return a geometry, but it would be a
    // sign-inconsistent (physically meaningless) one, so we surface a clean
    // diagnostic instead of a silent wrong answer. The deferred T1b alternative
    // is the free-standing eigenvalue/ratio search, which *solves for* a
    // feasible q (and the self-stress mode) rather than taking q as given here.
    for (&kind, &qi) in kinds.iter().zip(q.iter()) {
        let sign_ok = match kind {
            MemberKind::Cable => qi > 0.0,
            MemberKind::Strut => qi < 0.0,
        };
        if !sign_ok {
            return Err(FormFindError::SignViolation);
        }
    }

    // Force-density Laplacian D = CᵀQC for the line members (no surfaces on
    // this entry). Assembled by the shared `assemble_d` so the surface-aware
    // entry adds Σ_T σ_T·L_T into the identical matrix before the same solve.
    let d = assemble_d(n, members, q, &[], &[], nodes)?;

    // Partition node indices into anchored A and free F (both ascending).
    let mut is_anchor = vec![false; n];
    for &a in anchors {
        is_anchor[a] = true;
    }
    let free_indices: Vec<usize> = (0..n).filter(|&i| !is_anchor[i]).collect();
    let anchor_indices: Vec<usize> = (0..n).filter(|&i| is_anchor[i]).collect();
    let nf = free_indices.len();

    // Every node anchored ⇒ no free DOF to solve for. Guard before assembling a
    // 0×0 system (whose LU/solve is degenerate).
    if nf == 0 {
        return Err(FormFindError::EmptyFreeSet);
    }

    // Reduced free-node system D_ff X_f = −D_fa X_a (prestress-only: no external
    // load term), solved once via the shared `solve_reduced` core (partition →
    // faer partial-pivot LU → non-finite + scaled-residual guard). The
    // surface-aware entry reuses the identical core per fixed-point iteration.
    let out_nodes = solve_reduced(&d, nodes, &free_indices, &anchor_indices)?;

    // Per-member axial force Nᵢ = qᵢ · Lᵢ on the solved geometry, in
    // struts-then-cables member order (the input ordering).
    let member_forces: Vec<f64> = members
        .iter()
        .zip(q.iter())
        .map(|(&(j, k), &qi)| {
            let pj = out_nodes[j];
            let pk = out_nodes[k];
            let len = ((pj[0] - pk[0]).powi(2)
                + (pj[1] - pk[1]).powi(2)
                + (pj[2] - pk[2]).powi(2))
            .sqrt();
            qi * len
        })
        .collect();

    Ok(FormFindSolve {
        nodes: out_nodes,
        member_forces,
        force_densities: q.to_vec(),
        // Line-only path carries no surfaces.
        surface_stresses: Vec::new(),
        converged: true,
    })
}

// ── NFDM surface assembly (γ / task 4414) ─────────────────────────────────────
//
// Natural Force-Density surface contributions add into the SAME global
// force-density matrix D the line FDM builds (PRD §4, D1/D3): for an isotropic
// membrane each triangle contributes a cotangent-Laplacian (discrete
// Laplace–Beltrami operator) scaled by its surface stress σ, assembled with the
// identical rank-1 edge pattern the line solve uses for the member q.

/// Equilibrium-residual convergence tolerance for the cotangent fixed point. The
/// iteration stops once the free-node net force `‖(D·x)_free‖∞ / (1+scale)`
/// drops below this — the honest physical signal (prestress-only equilibrium),
/// and the SAME quantity the catenoid integration golden re-checks independently.
/// Set ~10× below the golden's `1e-9` acceptance bound so a converged solve
/// clears it with margin.
///
/// This replaces the earlier *coordinate-change* criterion: the Picard rate
/// approaches 1 as the mesh refines, so a machine-epsilon coordinate-change tol
/// could not be reached within any sane iteration cap on a fine membrane — yet
/// the residual (what actually matters) is already tiny there. Judging on the
/// residual directly converges finer meshes honestly.
const SURFACE_EQUILIBRIUM_TOL: f64 = 1e-10;

/// Iteration cap for the cotangent fixed point. The Picard iteration converges
/// linearly with a rate that approaches 1 under mesh refinement, so a fine
/// membrane can need ~1–2k solves to reach [`SURFACE_EQUILIBRIUM_TOL`]; the cap
/// is a generous backstop above that, reached only by a pathological /
/// non-settling input (which then honestly reports `converged == false`). Each
/// iteration is a single assemble + faer solve (per axis), so the cap bounds
/// worst-case work without affecting well-posed inputs that break out early.
const MAX_SURFACE_ITERS: usize = 5000;

/// Solve the anchored Force-Density form-finding problem WITH isotropic NFDM
/// surface (membrane) contributions (PRD §4, D1/D3 — γ / task 4414).
///
/// The line contribution is the landed `D = CᵀQC`; each surface triangle adds
/// its cotangent-Laplacian `σ_T·L_T` into the SAME global matrix `D`. Because
/// the cotangent edge weights depend on geometry, the solve iterates a
/// force-density fixed point: assemble `D` at the current geometry, solve the
/// reduced anchored system `D_ff X_f = −D_fa X_a` for the free nodes, and repeat
/// until the free coordinates settle. At a fixed point `x*`, `D(x*)·x* = 0` on
/// the free rows — prestress-only equilibrium of the combined cable/strut +
/// membrane network. With no surfaces this reduces to a single solve, identical
/// to [`form_find_anchored`].
///
/// `surfaces` are triangle corner index triples `(i, j, k)` into `nodes`;
/// `surface_stresses` is the matching per-triangle isotropic stress `σ` (one per
/// triangle, struts/cables order for `members`/`q` as before). Returns a
/// [`FormFindSolve`] whose `surface_stresses` echoes the prescribed σ.
///
/// # Errors
/// - [`FormFindError::DimensionMismatch`] — `members`/`kinds`/`q` disagree.
/// - [`FormFindError::SurfaceCountMismatch`] — `surfaces`/`surface_stresses`
///   disagree.
/// - [`FormFindError::SignViolation`] — a member violates its q-sign contract.
/// - [`FormFindError::NonTensionSurfaceStress`] — a surface `σ ≤ 0`.
/// - [`FormFindError::DegenerateTriangle`] — a zero-area surface triangle.
/// - [`FormFindError::EmptyFreeSet`] — every node anchored.
/// - [`FormFindError::SingularReducedStiffness`] — rank-deficient `D_ff`.
pub fn form_find_anchored_surfaces(
    nodes: &[[f64; 3]],
    members: &[(usize, usize)],
    kinds: &[MemberKind],
    q: &[f64],
    surfaces: &[(usize, usize, usize)],
    surface_stresses: &[f64],
    anchors: &[usize],
) -> Result<FormFindSolve, FormFindError> {
    let n = nodes.len();

    // ---- Feasibility guards (mirror the line contract; PRD §8.1). ----
    if members.len() != kinds.len() || members.len() != q.len() {
        return Err(FormFindError::DimensionMismatch);
    }
    if surfaces.len() != surface_stresses.len() {
        return Err(FormFindError::SurfaceCountMismatch);
    }
    // Member sign contract: cables carry tension (q > 0), struts compression.
    for (&kind, &qi) in kinds.iter().zip(q.iter()) {
        let sign_ok = match kind {
            MemberKind::Cable => qi > 0.0,
            MemberKind::Strut => qi < 0.0,
        };
        if !sign_ok {
            return Err(FormFindError::SignViolation);
        }
    }
    // Surface tension contract: isotropic prestress σ must be strictly positive
    // (a slack / compressed membrane is infeasible — the surface analogue of the
    // cable q > 0 rule).
    for &s in surface_stresses {
        if s <= 0.0 {
            return Err(FormFindError::NonTensionSurfaceStress);
        }
    }

    // Partition node indices into anchored A and free F (both ascending).
    let mut is_anchor = vec![false; n];
    for &a in anchors {
        is_anchor[a] = true;
    }
    let free_indices: Vec<usize> = (0..n).filter(|&i| !is_anchor[i]).collect();
    let anchor_indices: Vec<usize> = (0..n).filter(|&i| is_anchor[i]).collect();
    if free_indices.is_empty() {
        return Err(FormFindError::EmptyFreeSet);
    }

    // Iterate the cotangent fixed point. With no surfaces `D` is
    // geometry-independent, so a single solve is exact (the line-only case);
    // with surfaces the cotangent weights depend on geometry, so re-assemble and
    // re-solve until the free-node net force ‖(D·x)_free‖ settles to ~0
    // (prestress-only equilibrium).
    let mut current = nodes.to_vec();
    let mut converged = false;
    let max_iters = if surfaces.is_empty() { 1 } else { MAX_SURFACE_ITERS };
    // TODO(perf, scalability ceiling): each iteration re-allocates a fresh dense
    // n×n `D` in `assemble_d` (via `Mat::zeros`) and re-factors a fresh nf×nf
    // partial-pivot LU in `solve_reduced`. That is O(iters·n²) allocation churn
    // and O(iters·nf³) factorization — fine for the small DSL-level examples and
    // goldens here, but on a refined membrane mesh (where ~1–2k iterations are
    // expected, see MAX_SURFACE_ITERS) this dense, allocate-per-iter approach
    // dominates. A scalable path would reuse pre-allocated D / D_ff / RHS buffers
    // across iterations (clear-and-refill instead of `zeros()`) and move to a
    // sparse assembly + factor-reuse strategy for large meshes.
    for _ in 0..max_iters {
        let d = assemble_d(n, members, q, surfaces, surface_stresses, &current)?;

        // Convergence is judged on the EQUILIBRIUM RESIDUAL of the current
        // geometry under the freshly-assembled `D` — the honest physical signal
        // (and the exact quantity the integration golden re-checks). It reuses
        // the assembly we already need for the solve, so it adds no extra matrix
        // build. At a force-density fixed point `D(x*)·x*` ≈ 0 on the free rows.
        if !surfaces.is_empty()
            && free_equilibrium_residual(&d, &current, &free_indices) <= SURFACE_EQUILIBRIUM_TOL
        {
            converged = true;
            break;
        }

        let solved = solve_reduced(&d, &current, &free_indices, &anchor_indices)?;
        current = solved;

        // Line-only path: `D` is geometry-independent, so the single solve above
        // is already the exact equilibrium — no iteration needed.
        if surfaces.is_empty() {
            converged = true;
            break;
        }
    }
    let out_nodes = current;

    // Per-member axial force Nᵢ = qᵢ · Lᵢ on the solved geometry.
    let member_forces: Vec<f64> = members
        .iter()
        .zip(q.iter())
        .map(|(&(j, k), &qi)| {
            let pj = out_nodes[j];
            let pk = out_nodes[k];
            let len = ((pj[0] - pk[0]).powi(2)
                + (pj[1] - pk[1]).powi(2)
                + (pj[2] - pk[2]).powi(2))
            .sqrt();
            qi * len
        })
        .collect();

    Ok(FormFindSolve {
        nodes: out_nodes,
        member_forces,
        force_densities: q.to_vec(),
        surface_stresses: surface_stresses.to_vec(),
        converged,
    })
}

/// Assemble the global force-density matrix `D = CᵀQC` (line members) `+ Σ_T
/// σ_T·L_T` (surface cotangent-Laplacians, at the given geometry) into a dense
/// `n×n` faer matrix. Shared by both anchored entries; the line loop is the
/// landed FDM rank-1 update, the surface loop scatters each per-triangle local
/// 3×3 into the triangle's global node indices. Propagates
/// [`FormFindError::DegenerateTriangle`] from a zero-area triangle.
fn assemble_d(
    n: usize,
    members: &[(usize, usize)],
    q: &[f64],
    surfaces: &[(usize, usize, usize)],
    surface_stresses: &[f64],
    nodes: &[[f64; 3]],
) -> Result<Mat<f64>, FormFindError> {
    let mut d = Mat::<f64>::zeros(n, n);
    // Line members: rank-1 FDM update — qᵢ to D[j,j], D[k,k]; −qᵢ to D[j,k], D[k,j].
    for (&(j, k), &qi) in members.iter().zip(q.iter()) {
        d[(j, j)] += qi;
        d[(k, k)] += qi;
        d[(j, k)] -= qi;
        d[(k, j)] -= qi;
    }
    // Surface triangles: add σ_T·L_T into the SAME matrix.
    for (&(i, j, k), &sigma) in surfaces.iter().zip(surface_stresses.iter()) {
        let l = triangle_cotangent_laplacian(nodes[i], nodes[j], nodes[k], sigma)?;
        let idx = [i, j, k];
        for a in 0..3 {
            for b in 0..3 {
                d[(idx[a], idx[b])] += l[a][b];
            }
        }
    }
    Ok(d)
}

/// Solve the reduced anchored system `D_ff X_f = −D_fa X_a` once for the given
/// (already-assembled) `D` and geometry, scattering the solved free rows back
/// into a full node vector. The partition → faer partial-pivot LU → non-finite +
/// scaled-residual guard is the landed line-solve core, extracted verbatim so
/// the line and surface entries share it (the surface entry calls it once per
/// fixed-point iteration). Returns [`FormFindError::SingularReducedStiffness`]
/// when the reduced system is rank-deficient.
fn solve_reduced(
    d: &Mat<f64>,
    nodes: &[[f64; 3]],
    free_indices: &[usize],
    anchor_indices: &[usize],
) -> Result<Vec<[f64; 3]>, FormFindError> {
    let nf = free_indices.len();
    // All three coordinate axes are solved at once as an |F|×3 RHS so D_ff is
    // factored only once.
    let mut dff = Mat::<f64>::zeros(nf, nf);
    let mut rhs = Mat::<f64>::zeros(nf, 3);
    for (fi, &gi) in free_indices.iter().enumerate() {
        for (fj, &gj) in free_indices.iter().enumerate() {
            dff[(fi, fj)] = d[(gi, gj)];
        }
        for &ga in anchor_indices {
            let coupling = d[(gi, ga)];
            let xa = nodes[ga];
            rhs[(fi, 0)] -= coupling * xa[0];
            rhs[(fi, 1)] -= coupling * xa[1];
            rhs[(fi, 2)] -= coupling * xa[2];
        }
    }

    // Retain the unmodified RHS — `solve_in_place` overwrites `rhs` with the
    // solution, but the post-solve residual check below needs the original.
    let rhs_orig = rhs.clone();
    let plu = dff.partial_piv_lu();
    plu.solve_in_place(&mut rhs);

    // Scatter solved free-node rows back into original node order; anchors keep
    // their exact input coordinates (no solve round-trip).
    let mut out_nodes = nodes.to_vec();
    for (fi, &gi) in free_indices.iter().enumerate() {
        out_nodes[gi] = [rhs[(fi, 0)], rhs[(fi, 1)], rhs[(fi, 2)]];
    }

    // Post-solve guard: a singular / disconnected D_ff makes the LU solve
    // produce a non-finite or non-equilibrium result — surface
    // SingularReducedStiffness rather than NaNs / a silently wrong geometry.
    let any_nonfinite = out_nodes.iter().any(|p| p.iter().any(|c| !c.is_finite()));
    let mut residual_inf = 0.0_f64;
    let mut rhs_scale = 0.0_f64;
    for fi in 0..nf {
        for axis in 0..3 {
            let mut row_dot = 0.0;
            for fj in 0..nf {
                row_dot += dff[(fi, fj)] * rhs[(fj, axis)];
            }
            residual_inf = residual_inf.max((row_dot - rhs_orig[(fi, axis)]).abs());
            rhs_scale = rhs_scale.max(rhs_orig[(fi, axis)].abs());
        }
    }
    if any_nonfinite || residual_inf > 1e-6 * (1.0 + rhs_scale) {
        return Err(FormFindError::SingularReducedStiffness);
    }

    Ok(out_nodes)
}

/// Free-node equilibrium residual `‖(D·x)_free‖∞ / (1+scale)` — the prestress-only
/// net force on the free nodes, scaled by the coordinate magnitude so the bound
/// is coordinate-scale-free. It is ~0 at a force-density fixed point, so the
/// cotangent iteration uses it as its convergence signal; it mirrors the
/// independent check the catenoid integration golden runs (same formula), so the
/// kernel's stop condition and the test's acceptance bound measure the SAME
/// quantity.
#[allow(clippy::needless_range_loop)]
fn free_equilibrium_residual(d: &Mat<f64>, nodes: &[[f64; 3]], free_indices: &[usize]) -> f64 {
    let n = nodes.len();
    let mut resid = 0.0_f64;
    for &i in free_indices {
        for axis in 0..3 {
            let mut net = 0.0;
            for j in 0..n {
                net += d[(i, j)] * nodes[j][axis];
            }
            resid = resid.max(net.abs());
        }
    }
    let mut scale = 0.0_f64;
    for p in nodes {
        for &c in p {
            scale = scale.max(c.abs());
        }
    }
    resid / (1.0 + scale)
}

/// Relative threshold below which a triangle is judged degenerate: when
/// `2·Area ≤ ε · (max squared edge length)` the corners are effectively
/// collinear and the cotangents diverge. Relative (not absolute) so the test is
/// scale-free — a millimetre-scale and a kilometre-scale triangle of the same
/// shape are judged identically.
const DEGENERATE_AREA_EPS: f64 = 1e-10;

#[inline]
fn v_sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn v_dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn v_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Per-triangle cotangent-Laplacian (discrete Laplace–Beltrami) local
/// contribution for an isotropic NFDM surface element.
///
/// For triangle `(i, j, k)` with isotropic surface stress `sigma`, the discrete
/// Laplace–Beltrami edge weight on the edge *opposite* vertex `v` is
/// `(σ/2)·cot(θ_v)`, with `cot(θ_v) = (e_a·e_b) / |e_a×e_b|` where `e_a`, `e_b`
/// are the two triangle edges out of `v` and `|e_a×e_b| = 2·Area` (the same for
/// every vertex). The returned local 3×3 `L` is assembled with the landed FDM
/// rank-1 pattern — each edge weight `w` adds `+w` to its two incident diagonal
/// entries and `−w` to the two symmetric off-diagonal slots — so `D_T = L` is
/// symmetric and each row sums to zero (a graph Laplacian).
///
/// Rows/cols are indexed `0=i, 1=j, 2=k`, matching the argument order; the
/// caller scatters `L[a][b]` into the global `D` at the triangle's global node
/// indices, exactly as the line loop scatters its member rank-1 update.
///
/// Returns `Err(FormFindError::DegenerateTriangle)` when `2·Area` is negligible
/// relative to the triangle's edge scale (collinear / zero-area corners), where
/// the cotangents would diverge — a clean diagnostic rather than a NaN/∞ stencil
/// that would silently poison the assembled system.
fn triangle_cotangent_laplacian(
    pi: [f64; 3],
    pj: [f64; 3],
    pk: [f64; 3],
    sigma: f64,
) -> Result<[[f64; 3]; 3], FormFindError> {
    // The six directed edge vectors (two out of each vertex).
    let eij = v_sub(pj, pi); // i → j
    let eik = v_sub(pk, pi); // i → k
    let eji = v_sub(pi, pj); // j → i
    let ejk = v_sub(pk, pj); // j → k
    let eki = v_sub(pi, pk); // k → i
    let ekj = v_sub(pj, pk); // k → j

    // 2·Area = |e_a × e_b| is invariant to which vertex's edge pair we cross.
    let cross = v_cross(eij, eik);
    let two_area = v_dot(cross, cross).sqrt();

    // Degenerate guard (relative): reject before the divisions below blow up.
    let scale = v_dot(eij, eij).max(v_dot(eik, eik)).max(v_dot(ejk, ejk));
    if two_area <= DEGENERATE_AREA_EPS * scale {
        return Err(FormFindError::DegenerateTriangle);
    }

    // cot(θ_v) = (e_a · e_b) / (2·Area), e_a/e_b the two edges out of v.
    let cot_i = v_dot(eij, eik) / two_area;
    let cot_j = v_dot(eji, ejk) / two_area;
    let cot_k = v_dot(eki, ekj) / two_area;

    // Edge weight opposite vertex v is (σ/2)·cot(θ_v): edge (i,j) is opposite k,
    // edge (j,k) opposite i, edge (k,i) opposite j.
    let half_sigma = 0.5 * sigma;
    let w_ij = half_sigma * cot_k;
    let w_jk = half_sigma * cot_i;
    let w_ki = half_sigma * cot_j;

    // Assemble the symmetric local Laplacian via the rank-1 edge pattern
    // (+w on the two incident diagonals, −w on the symmetric off-diagonal pair).
    let mut l = [[0.0_f64; 3]; 3];
    let mut add_edge = |a: usize, b: usize, w: f64| {
        l[a][a] += w;
        l[b][b] += w;
        l[a][b] -= w;
        l[b][a] -= w;
    };
    add_edge(0, 1, w_ij); // edge i–j
    add_edge(1, 2, w_jk); // edge j–k
    add_edge(2, 0, w_ki); // edge k–i

    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A membrane test case: `(nodes, surface triangles, anchor indices)`.
    /// Aliased to keep the surface-test helper signatures readable (and to
    /// silence `clippy::type_complexity` on the bare nested-tuple return).
    type MembraneCase = (Vec<[f64; 3]>, Vec<(usize, usize, usize)>, Vec<usize>);

    /// Tolerance for the analytic FD-identity goldens. The reduced linear solve
    /// reproduces these exact identities to ~1e-13; 1e-9 leaves ~4 orders of
    /// margin while still catching a wrong solve.
    const TOL: f64 = 1e-9;

    /// Max absolute componentwise difference between two 3-vectors.
    fn max_coord_err(a: [f64; 3], b: [f64; 3]) -> f64 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max)
    }

    // (a) A single free node cabled to 4 anchors with equal force density solves
    // to the (unweighted) centroid of the anchors — the weighted-centroid FD
    // identity x_f = Σ qᵢ x_{aᵢ} / Σ qᵢ with all qᵢ equal. Anchors are placed
    // symmetrically in x,y so the centroid is (0, 0, 0.5).
    #[test]
    fn single_free_node_equal_q_solves_to_anchor_centroid() {
        let nodes = vec![
            [0.3, 0.2, 0.4], // free node 0 — deliberately off-solution
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 1.0],
            [0.0, -1.0, 1.0],
        ];
        let members = [(0, 1), (0, 2), (0, 3), (0, 4)];
        let kinds = [MemberKind::Cable; 4];
        let q = [1.0, 1.0, 1.0, 1.0];
        let anchors = [1, 2, 3, 4];

        let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
            .expect("equal-q anchored cable net must be feasible");

        let expected = [0.0, 0.0, 0.5];
        assert!(
            max_coord_err(solve.nodes[0], expected) < TOL,
            "nodes[0] = {:?}, expected anchor centroid {:?}",
            solve.nodes[0],
            expected,
        );
    }

    // (b) Unequal force densities give the *weighted* centroid
    // x_f = Σ qᵢ x_{aᵢ} / Σ qᵢ. Same geometry as (a) but q = [2,1,1,1]; the
    // expected point is computed from the identity rather than hard-coded.
    #[test]
    fn single_free_node_unequal_q_solves_to_weighted_centroid() {
        let anchor_pts = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 1.0],
            [0.0, -1.0, 1.0],
        ];
        let nodes = vec![
            [0.3, 0.2, 0.4], // free node 0
            anchor_pts[0],
            anchor_pts[1],
            anchor_pts[2],
            anchor_pts[3],
        ];
        let members = [(0, 1), (0, 2), (0, 3), (0, 4)];
        let kinds = [MemberKind::Cable; 4];
        let q = [2.0, 1.0, 1.0, 1.0];
        let anchors = [1, 2, 3, 4];

        // Analytic weighted centroid Σ qᵢ x_i / Σ qᵢ.
        let qsum: f64 = q.iter().sum();
        let mut expected = [0.0_f64; 3];
        for (w, p) in q.iter().zip(anchor_pts.iter()) {
            for (e, c) in expected.iter_mut().zip(p.iter()) {
                *e += w * c;
            }
        }
        for e in expected.iter_mut() {
            *e /= qsum;
        }

        let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
            .expect("unequal-q anchored cable net must be feasible");

        assert!(
            max_coord_err(solve.nodes[0], expected) < TOL,
            "nodes[0] = {:?}, expected weighted centroid {:?}",
            solve.nodes[0],
            expected,
        );
    }

    // (c) Two free nodes in a uniform-tension chain
    // anchor(x=0) — node0 — node1 — anchor(x=3), all cables q=1. The interior
    // nodes settle to evenly-spaced positions x0=1, x1=2. This exercises the
    // off-diagonal D_ff coupling: the node0–node1 cable couples the two free
    // equations, so a diagonal-only solve would get this wrong.
    #[test]
    fn two_free_node_chain_solves_to_uniform_spacing() {
        let nodes = vec![
            [0.5, 0.0, 0.0], // free node 0 — off-solution
            [2.5, 0.0, 0.0], // free node 1 — off-solution
            [0.0, 0.0, 0.0], // anchor at x=0
            [3.0, 0.0, 0.0], // anchor at x=3
        ];
        let members = [(2, 0), (0, 1), (1, 3)];
        let kinds = [MemberKind::Cable; 3];
        let q = [1.0, 1.0, 1.0];
        let anchors = [2, 3];

        let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
            .expect("uniform-tension chain must be feasible");

        assert!(
            max_coord_err(solve.nodes[0], [1.0, 0.0, 0.0]) < TOL,
            "free node 0 = {:?}, expected (1,0,0)",
            solve.nodes[0],
        );
        assert!(
            max_coord_err(solve.nodes[1], [2.0, 0.0, 0.0]) < TOL,
            "free node 1 = {:?}, expected (2,0,0)",
            solve.nodes[1],
        );
    }

    // Member forces, force-density echo, and convergence flag on the case-(a)
    // geometry. Each axial force is Nᵢ = qᵢ · Lᵢ where Lᵢ is the Euclidean
    // length of member i measured on the *solved* coordinates (here all four
    // cables are √1.25 long and q=1, so each force equals that length). The
    // expected length is recomputed from the returned nodes so the assertion
    // tracks the solve rather than a hard-coded constant.
    #[test]
    fn member_forces_are_q_times_solved_length_and_q_is_echoed() {
        let nodes = vec![
            [0.3, 0.2, 0.4], // free node 0 — off-solution
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 1.0],
            [0.0, -1.0, 1.0],
        ];
        let members = [(0, 1), (0, 2), (0, 3), (0, 4)];
        let kinds = [MemberKind::Cable; 4];
        let q = [1.0, 1.0, 1.0, 1.0];
        let anchors = [1, 2, 3, 4];

        let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
            .expect("equal-q anchored cable net must be feasible");

        assert_eq!(
            solve.member_forces.len(),
            members.len(),
            "one axial force per member",
        );
        for (i, &(j, k)) in members.iter().enumerate() {
            let pj = solve.nodes[j];
            let pk = solve.nodes[k];
            let len = ((pj[0] - pk[0]).powi(2)
                + (pj[1] - pk[1]).powi(2)
                + (pj[2] - pk[2]).powi(2))
            .sqrt();
            let expected = q[i] * len;
            assert!(
                (solve.member_forces[i] - expected).abs() < TOL,
                "member_forces[{i}] = {}, expected q·L = {}",
                solve.member_forces[i],
                expected,
            );
        }

        // force_densities is an exact echo of the input q (a copy, not a
        // computed quantity), so exact equality must hold.
        assert_eq!(
            solve.force_densities,
            q.to_vec(),
            "force_densities must echo input q exactly",
        );

        assert!(
            solve.converged,
            "a well-posed anchored solve must report converged == true",
        );
    }

    // (a) Sign-convention contract: a cable must carry tension (q > 0). A cable
    // with q ≤ 0 is infeasible input.
    #[test]
    fn cable_with_nonpositive_q_is_sign_violation() {
        let nodes = vec![
            [0.0, 0.0, 0.5], // free node 0
            [1.0, 0.0, 0.0], // anchor 1
            [-1.0, 0.0, 0.0], // anchor 2
        ];
        let members = [(0, 1), (0, 2)];
        let kinds = [MemberKind::Cable, MemberKind::Cable];
        let q = [1.0, -1.0]; // cable 1 violates the q > 0 tension contract
        let anchors = [1, 2];

        assert_eq!(
            form_find_anchored(&nodes, &members, &kinds, &q, &anchors).unwrap_err(),
            FormFindError::SignViolation,
        );
    }

    // (a) Mirror: a strut must carry compression (q < 0). A strut with q ≥ 0 is
    // infeasible input.
    #[test]
    fn strut_with_nonnegative_q_is_sign_violation() {
        let nodes = vec![
            [0.0, 0.0, 0.0], // free node 0
            [1.0, 0.0, 0.0], // anchor 1
        ];
        let members = [(0, 1)];
        let kinds = [MemberKind::Strut];
        let q = [1.0]; // strut requires q < 0; +1 violates the compression contract
        let anchors = [1];

        assert_eq!(
            form_find_anchored(&nodes, &members, &kinds, &q, &anchors).unwrap_err(),
            FormFindError::SignViolation,
        );
    }

    // (b) A free node with no member path to any anchor leaves a zero row in the
    // reduced stiffness D_ff → singular. The solve cannot recover that node.
    #[test]
    fn disconnected_free_node_is_singular_reduced_stiffness() {
        let nodes = vec![
            [0.0, 0.0, 0.0], // free node 0 — connected to the anchor
            [5.0, 0.0, 0.0], // free node 1 — floating: no members touch it
            [1.0, 0.0, 0.0], // anchor 2
        ];
        let members = [(0, 2)]; // only node 0 ↔ anchor; node 1 has no path
        let kinds = [MemberKind::Cable];
        let q = [1.0];
        let anchors = [2];

        assert_eq!(
            form_find_anchored(&nodes, &members, &kinds, &q, &anchors).unwrap_err(),
            FormFindError::SingularReducedStiffness,
        );
    }

    // (c) Anchoring every node leaves no free DOF to solve for.
    #[test]
    fn all_nodes_anchored_is_empty_free_set() {
        let nodes = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        let members = [(0, 1)];
        let kinds = [MemberKind::Cable];
        let q = [1.0];
        let anchors = [0, 1]; // every node anchored → empty free set

        assert_eq!(
            form_find_anchored(&nodes, &members, &kinds, &q, &anchors).unwrap_err(),
            FormFindError::EmptyFreeSet,
        );
    }

    // (d) members / kinds / q must agree in length. A short q is a dimension
    // mismatch, caught up front before any solve.
    #[test]
    fn length_mismatch_is_dimension_mismatch() {
        let nodes = vec![
            [0.0, 0.0, 0.5],
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
        ];
        let members = [(0, 1), (0, 2)];
        let kinds = [MemberKind::Cable, MemberKind::Cable];
        let q = [1.0]; // one density for two members → mismatch
        let anchors = [1, 2];

        assert_eq!(
            form_find_anchored(&nodes, &members, &kinds, &q, &anchors).unwrap_err(),
            FormFindError::DimensionMismatch,
        );
    }

    // ── γ (task 4414): per-triangle cotangent-Laplacian (NFDM surface) stencil ──

    /// Tolerance for the closed-form cotangent-stencil identity. The local 3×3
    /// contribution is a handful of exact float ops (two dots, one cross, one
    /// divide), so it reproduces the hand-computed weights to ~machine epsilon;
    /// 1e-12 is honest closed-form exactness, NOT a mesh-convergence claim.
    const STENCIL_TOL: f64 = 1e-12;

    // (a) The per-triangle cotangent-Laplacian stencil is EXACT for a
    // right-isosceles triangle A=(0,0,0), B=(1,0,0), C=(0,1,0) with isotropic
    // surface stress σ=1. Interior angles: 90° at A (cot 0), 45° at B and C
    // (cot 1). The discrete Laplace–Beltrami edge weight opposite vertex v is
    // (σ/2)·cot(θ_v), so the assembled local contribution D_T = σ·L_T is
    //   off-diagonals  D[A,B]=D[A,C]=−σ/2=−0.5,  D[B,C]=0
    //   diagonals      D[A,A]=σ=1,  D[B,B]=D[C,C]=σ/2=0.5
    // This is closed-form exactness (a known cotangent), NOT a convergence claim.
    #[test]
    #[allow(clippy::needless_range_loop)] // explicit 3×3 stencil index checks incl. transpose l[col][r]
    fn triangle_cotangent_laplacian_stencil_is_exact_for_right_isosceles() {
        let a = [0.0, 0.0, 0.0];
        let b = [1.0, 0.0, 0.0];
        let c = [0.0, 1.0, 0.0];
        let sigma = 1.0;

        let l = triangle_cotangent_laplacian(a, b, c, sigma)
            .expect("a non-degenerate triangle must yield a cotangent-Laplacian");

        // Expected local 3×3 (rows/cols 0=A, 1=B, 2=C).
        let expected = [
            [1.0, -0.5, -0.5],
            [-0.5, 0.5, 0.0],
            [-0.5, 0.0, 0.5],
        ];
        for r in 0..3 {
            for col in 0..3 {
                assert!(
                    (l[r][col] - expected[r][col]).abs() < STENCIL_TOL,
                    "L[{r}][{col}] = {}, expected {} (right-isosceles cotangent stencil)",
                    l[r][col],
                    expected[r][col],
                );
            }
        }

        // The FDM rank-1 pattern writes each edge weight to BOTH off-diagonal
        // slots, so L must be symmetric.
        for r in 0..3 {
            for col in 0..3 {
                assert!(
                    (l[r][col] - l[col][r]).abs() < STENCIL_TOL,
                    "cotangent-Laplacian must be symmetric; L[{r}][{col}] != L[{col}][{r}]",
                );
            }
        }

        // A graph Laplacian annihilates the constant function, so every row must
        // sum to ~0 (diag = Σ incident edge weights, off-diags = −those weights).
        for r in 0..3 {
            let row_sum: f64 = l[r].iter().sum();
            assert!(
                row_sum.abs() < STENCIL_TOL,
                "cotangent-Laplacian row {r} must sum to 0, got {row_sum}",
            );
        }
    }

    // (b) A degenerate (collinear, zero-area) triangle makes
    // cot(θ)=dot/(2·Area) blow up as 2·Area→0. The helper must return
    // DegenerateTriangle rather than a NaN/∞ stencil that would silently poison
    // the assembled global system.
    #[test]
    fn triangle_cotangent_laplacian_rejects_degenerate_triangle() {
        // Three collinear points on the x-axis → zero area.
        let a = [0.0, 0.0, 0.0];
        let b = [1.0, 0.0, 0.0];
        let c = [2.0, 0.0, 0.0];

        assert_eq!(
            triangle_cotangent_laplacian(a, b, c, 1.0).unwrap_err(),
            FormFindError::DegenerateTriangle,
        );
    }

    // ── γ (task 4414): surface-aware form_find_anchored_surfaces ───────────────

    /// Independent (faer-free) reassembly of the global force-density matrix
    /// `D = CᵀQC` (lines) `+ Σ_T σ_T·L_T` (surface cotangent-Laplacians) at the
    /// given geometry, as a dense `Vec<Vec<f64>>`. Used to check the *equilibrium*
    /// residual ‖(D x)_free‖ at the solved geometry — the primary honest signal
    /// that the solver reached a genuine force-density fixed point (net force on
    /// each free node ≈ 0), computed without re-using the kernel's faer path.
    fn reassemble_d(
        n: usize,
        members: &[(usize, usize)],
        q: &[f64],
        surfaces: &[(usize, usize, usize)],
        sigmas: &[f64],
        nodes: &[[f64; 3]],
    ) -> Vec<Vec<f64>> {
        let mut d = vec![vec![0.0_f64; n]; n];
        for (&(j, k), &qi) in members.iter().zip(q.iter()) {
            d[j][j] += qi;
            d[k][k] += qi;
            d[j][k] -= qi;
            d[k][j] -= qi;
        }
        for (&(i, j, k), &s) in surfaces.iter().zip(sigmas.iter()) {
            let l = triangle_cotangent_laplacian(nodes[i], nodes[j], nodes[k], s)
                .expect("non-degenerate triangle in equilibrium reassembly");
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
    /// node-coordinate magnitude so the bound is coordinate-scale-free.
    #[allow(clippy::needless_range_loop)] // explicit row/axis indexing of the dense D and node coords
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

    /// "Tent" membrane: a diamond boundary of 4 anchored corners in the z=0
    /// plane plus one free interior node (seeded off-plane at z=0.3), fanned by
    /// 4 triangles. The minimal surface spanning a planar boundary is flat, so a
    /// correct cotangent assembly pulls the free node back into the boundary
    /// plane (z→0) and leaves a ~0 equilibrium residual; a wrong assembly drives
    /// it off-plane or blows up the residual (non-circular signal). The in-plane
    /// (x,y) position is NOT unique — the flat surface has constant area for any
    /// interior position, so the cotangent-Laplacian vanishes across the whole
    /// interior — hence the tests assert planarity + residual, not an (x,y).
    fn tent_membrane() -> MembraneCase {
        let nodes = vec![
            [0.1, 0.1, 0.3],  // 0: free interior — deliberately off-solution
            [1.0, 0.0, 0.0],  // 1: anchor
            [0.0, 1.0, 0.0],  // 2: anchor
            [-1.0, 0.0, 0.0], // 3: anchor
            [0.0, -1.0, 0.0], // 4: anchor
        ];
        let surfaces = vec![(0, 1, 2), (0, 2, 3), (0, 3, 4), (0, 4, 1)];
        let anchors = vec![1, 2, 3, 4];
        (nodes, surfaces, anchors)
    }

    /// Equilibrium-residual bound for the surface solve: a linear solve iterated
    /// to a cotangent fixed point reaches ~machine precision, so 1e-9 leaves wide
    /// margin while still catching a non-converged or mis-assembled solve.
    const SURFACE_EQUIL_TOL: f64 = 1e-9;

    // (a) A fixed-boundary membrane with one free interior node and isotropic σ>0
    // solves: converged, the equilibrium residual at the solved geometry is
    // ~machine-precision, and surface_stresses echoes one σ per triangle.
    #[test]
    fn surfaces_membrane_solves_to_equilibrium_and_echoes_sigma() {
        let (nodes, surfaces, anchors) = tent_membrane();
        let sigma = 2.0;
        let sigmas = vec![sigma; surfaces.len()];
        // Pure membrane: no struts/cables.
        let members: Vec<(usize, usize)> = vec![];
        let kinds: Vec<MemberKind> = vec![];
        let q: Vec<f64> = vec![];

        let solve =
            form_find_anchored_surfaces(&nodes, &members, &kinds, &q, &surfaces, &sigmas, &anchors)
                .expect("a well-posed σ>0 membrane must be feasible");

        assert!(solve.converged, "the cotangent fixed point must converge");

        // surface_stresses echoes the prescribed σ, one per triangle.
        assert_eq!(
            solve.surface_stresses.len(),
            surfaces.len(),
            "one surface stress echo per triangle",
        );
        for (t, &s) in solve.surface_stresses.iter().enumerate() {
            assert!(
                (s - sigma).abs() < 1e-12,
                "surface_stresses[{t}] = {s}, expected echoed σ = {sigma}",
            );
        }

        // Primary honest signal: the free-node equilibrium residual (net force)
        // at the SOLVED geometry is ~0.
        let mut is_anchor = vec![false; nodes.len()];
        for &a in &anchors {
            is_anchor[a] = true;
        }
        let d = reassemble_d(nodes.len(), &members, &q, &surfaces, &sigmas, &solve.nodes);
        let resid = equilibrium_residual_scaled(&d, &solve.nodes, &is_anchor);
        assert!(
            resid < SURFACE_EQUIL_TOL,
            "equilibrium residual ‖(D x)_free‖/scale = {resid:e}, expected < {SURFACE_EQUIL_TOL:e}",
        );

        // The boundary is planar (all anchors at z=0), so the equilibrium
        // membrane is flat: the free node's z is the cotangent-weighted average
        // of the anchor z's (all 0), hence exactly 0 at any non-degenerate
        // equilibrium — a genuine signal here, since the seed sits at z=0.3.
        //
        // We deliberately do NOT assert the in-plane (x,y) position. With a
        // planar boundary the flat surface has CONSTANT area for any interior
        // position (the fan triangles always tile the same diamond), so the
        // area gradient — the cotangent-Laplacian (D x)_free — vanishes across
        // the WHOLE interior: a continuum of equilibria, not a unique centroid.
        // The solver lands on the seed-nearest one (verified: net force ~1e-16
        // at (0,0,0), (0.092,0.092,0), (-0.2,0.3,0), … alike). Pinning an exact
        // (x,y) would assert a non-unique coordinate and violate the G6 honesty
        // mandate ("never an exact coordinate"); the equilibrium residual above
        // is the honest in-plane signal.
        let n0 = solve.nodes[0];
        assert!(
            n0[2].abs() < 1e-9,
            "free node z = {}, expected ~0 (planar boundary ⇒ flat membrane)",
            n0[2],
        );
    }

    // (b) A non-positive surface stress (σ ≤ 0) is a non-tension membrane —
    // infeasible input (the surface analogue of the cable q>0 sign contract).
    #[test]
    fn surfaces_nonpositive_sigma_is_non_tension() {
        let (nodes, surfaces, anchors) = tent_membrane();
        let mut sigmas = vec![1.0; surfaces.len()];
        sigmas[2] = -0.5; // triangle 2 violates the σ>0 tension contract
        let members: Vec<(usize, usize)> = vec![];
        let kinds: Vec<MemberKind> = vec![];
        let q: Vec<f64> = vec![];

        assert_eq!(
            form_find_anchored_surfaces(&nodes, &members, &kinds, &q, &surfaces, &sigmas, &anchors)
                .unwrap_err(),
            FormFindError::NonTensionSurfaceStress,
        );
    }

    // (c) surfaces and surface_stresses must agree in length — a per-triangle σ
    // is required for each triangle.
    #[test]
    fn surfaces_count_mismatch_is_surface_count_mismatch() {
        let (nodes, surfaces, anchors) = tent_membrane();
        let sigmas = vec![1.0; surfaces.len() - 1]; // one short
        let members: Vec<(usize, usize)> = vec![];
        let kinds: Vec<MemberKind> = vec![];
        let q: Vec<f64> = vec![];

        assert_eq!(
            form_find_anchored_surfaces(&nodes, &members, &kinds, &q, &surfaces, &sigmas, &anchors)
                .unwrap_err(),
            FormFindError::SurfaceCountMismatch,
        );
    }

    // (d) The pure-line path (empty surfaces) through the surface-aware entry
    // must return exactly the landed form_find_anchored result, with an empty
    // surface_stresses echo — the additive-extension invariant.
    #[test]
    fn surfaces_empty_matches_line_only_form_find_anchored() {
        let nodes = vec![
            [0.3, 0.2, 0.4],
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 1.0],
            [0.0, -1.0, 1.0],
        ];
        let members = [(0, 1), (0, 2), (0, 3), (0, 4)];
        let kinds = [MemberKind::Cable; 4];
        let q = [1.0, 1.0, 1.0, 1.0];
        let anchors = [1, 2, 3, 4];

        let line = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
            .expect("line-only reference solve");
        let surf =
            form_find_anchored_surfaces(&nodes, &members, &kinds, &q, &[], &[], &anchors)
                .expect("empty-surface path must match the line-only solve");

        assert!(surf.surface_stresses.is_empty(), "no surfaces ⇒ empty echo");
        assert_eq!(surf.converged, line.converged);
        assert_eq!(surf.force_densities, line.force_densities);
        assert_eq!(surf.member_forces.len(), line.member_forces.len());
        for (a, b) in surf.member_forces.iter().zip(line.member_forces.iter()) {
            assert!((a - b).abs() < 1e-12, "member force mismatch: {a} vs {b}");
        }
        assert_eq!(surf.nodes.len(), line.nodes.len());
        for (a, b) in surf.nodes.iter().zip(line.nodes.iter()) {
            assert!(max_coord_err(*a, *b) < 1e-12, "node mismatch: {a:?} vs {b:?}");
        }
    }
}
