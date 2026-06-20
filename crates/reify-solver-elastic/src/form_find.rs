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
    // TODO(perf, scalability ceiling): each iteration re-allocates a fresh dense // ptodo:allow permanent perf note, no live owner task
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

// ══════════════════════════════════════════════════════════════════════════════
// ε (task 4416): anisotropic warp/weft NFDM extension
// ══════════════════════════════════════════════════════════════════════════════
//
// Generalise the landed isotropic NFDM (γ task 4414 / δ task 4415) to
// ANISOTROPIC warp/weft prestress σ_w ≠ σ_f.  The per-triangle stencil is
//
//   L_T[a][b] = Area · (∇N_a · S · ∇N_b)
//
// where S = diag(σ_w, σ_f) in the per-triangle material frame (e₁, e₂) with
// e₁ = normalised in-plane projection of the user warp direction, e₂ = n×e₁.
// When σ_w = σ_f = σ, S = σI and L_T collapses EXACTLY to the cotangent
// Laplacian — a frame-independent mathematical identity (step-1 RED test).
//
// API: additive sibling entry points; isotropic γ/δ code paths UNTOUCHED.

/// Per-triangle anisotropic surface stress specification (ε / task 4416).
///
/// The stress tensor `S = diag(σ_w, σ_f)` is expressed in a per-triangle
/// in-plane material frame: `e₁` is the normalised in-plane projection of
/// `warp_dir` onto the triangle plane, `e₂ = n × e₁` (`n` = unit normal).
#[derive(Debug, Clone)]
pub struct AnisotropicSurfaceStress {
    /// Warp direction hint (any non-zero vector; projected in-plane per triangle).
    pub warp_dir: [f64; 3],
    /// Warp (e₁) surface stress `σ_w > 0`.
    pub sigma_warp: f64,
    /// Weft (e₂) surface stress `σ_f > 0`.
    pub sigma_weft: f64,
}

/// Reason an anisotropic anchored form-find solve is infeasible.
///
/// A **separate** enum from [`FormFindError`] — adding a variant to that enum
/// would break the exhaustive match in
/// `reify-eval/src/compute_targets/form_find.rs:308`, which is out of scope
/// for this leaf (ε plan.json D1). Mirrors the precedent set by δ (task 4415),
/// which introduced `FreeFormError` as a separate enum for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnisoFormFindError {
    /// `members` / `kinds` / `q` disagree in length.
    DimensionMismatch,
    /// A member's force density violates its kind's sign contract.
    SignViolation,
    /// Every node is anchored — no free node to solve for.
    EmptyFreeSet,
    /// The reduced stiffness `D_ff` is singular or ill-conditioned.
    SingularReducedStiffness,
    /// `surfaces` and `surface_prestress` disagree in length.
    SurfaceCountMismatch,
    /// A surface has `σ_w ≤ 0` or `σ_f ≤ 0` (non-tension).
    NonTensionSurfaceStress,
    /// A triangle is degenerate (collinear / zero-area corners).
    DegenerateTriangle,
    /// `warp_dir` is parallel (or nearly so) to the triangle normal, so its
    /// in-plane projection is negligible and the material frame is undefined.
    DegenerateMaterialFrame,
}

/// Result of an anisotropic anchored Force-Density form-find solve (ε / task 4416).
#[derive(Debug, Clone)]
pub struct AnisoFormFindSolve {
    /// Solved node coordinates in original node order.
    pub nodes: Vec<[f64; 3]>,
    /// Per-member axial force `Nᵢ = qᵢ · Lᵢ` on the solved geometry.
    pub member_forces: Vec<f64>,
    /// Echo of the input force densities.
    pub force_densities: Vec<f64>,
    /// Per-triangle recovered principal stresses on the solved geometry (one per
    /// surface, in declaration order). Populated after the fixed-point loop
    /// converges by calling [`recover_principal_stress`] per triangle.
    pub principal_stresses: Vec<PrincipalStress>,
    /// Whether the fixed-point loop converged.
    pub converged: bool,
}

/// Map a [`FormFindError`] from the shared `solve_reduced` core to an
/// [`AnisoFormFindError`] at the aniso boundary. Only `SingularReducedStiffness`
/// and `EmptyFreeSet` can arise from that function; the rest are pre-checked.
fn map_ff_error(e: FormFindError) -> AnisoFormFindError {
    match e {
        FormFindError::SingularReducedStiffness => AnisoFormFindError::SingularReducedStiffness,
        FormFindError::EmptyFreeSet => AnisoFormFindError::EmptyFreeSet,
        // Remaining variants are pre-checked above solve_reduced; map defensively.
        FormFindError::DimensionMismatch => AnisoFormFindError::DimensionMismatch,
        FormFindError::SignViolation => AnisoFormFindError::SignViolation,
        FormFindError::DegenerateTriangle => AnisoFormFindError::DegenerateTriangle,
        FormFindError::NonTensionSurfaceStress => AnisoFormFindError::NonTensionSurfaceStress,
        FormFindError::SurfaceCountMismatch => AnisoFormFindError::SurfaceCountMismatch,
    }
}

/// Assemble `D = CᵀQC` (line members) `+ Σ_T L_T(aniso)` (anisotropic surface
/// stencils) into a dense `n×n` faer matrix. Parallels `assemble_d` but
/// scatters `triangle_anisotropic_laplacian` instead of the cotangent stencil.
fn assemble_d_aniso(
    n: usize,
    members: &[(usize, usize)],
    q: &[f64],
    surfaces: &[(usize, usize, usize)],
    surface_prestress: &[AnisotropicSurfaceStress],
    nodes: &[[f64; 3]],
) -> Result<Mat<f64>, AnisoFormFindError> {
    let mut d = Mat::<f64>::zeros(n, n);
    for (&(j, k), &qi) in members.iter().zip(q.iter()) {
        d[(j, j)] += qi;
        d[(k, k)] += qi;
        d[(j, k)] -= qi;
        d[(k, j)] -= qi;
    }
    for (&(i, j, k), spec) in surfaces.iter().zip(surface_prestress.iter()) {
        let l = triangle_anisotropic_laplacian(nodes[i], nodes[j], nodes[k], spec)?;
        let idx = [i, j, k];
        for a in 0..3 {
            for b in 0..3 {
                d[(idx[a], idx[b])] += l[a][b];
            }
        }
    }
    Ok(d)
}

/// Solve the anchored Force-Density form-finding problem with ANISOTROPIC NFDM
/// surface contributions (ε / task 4416).
///
/// Mirrors [`form_find_anchored_surfaces`] but accepts an
/// [`AnisotropicSurfaceStress`] per triangle (warp direction + `σ_w`, `σ_f`)
/// instead of a single isotropic `σ`. The fixed-point loop, convergence
/// criterion, and all line-member machinery are reused unchanged.
///
/// `principal_stresses` on the returned [`AnisoFormFindSolve`] is populated
/// per triangle on the solved geometry by [`recover_principal_stress`].
///
/// # Errors
/// - [`AnisoFormFindError::DimensionMismatch`] — `members`/`kinds`/`q` disagree.
/// - [`AnisoFormFindError::SurfaceCountMismatch`] — `surfaces`/`surface_prestress` disagree.
/// - [`AnisoFormFindError::SignViolation`] — a member violates its `q`-sign contract.
/// - [`AnisoFormFindError::NonTensionSurfaceStress`] — `σ_w ≤ 0` or `σ_f ≤ 0`.
/// - [`AnisoFormFindError::DegenerateTriangle`] — zero-area surface triangle.
/// - [`AnisoFormFindError::DegenerateMaterialFrame`] — `warp_dir ∥ n` for a triangle.
/// - [`AnisoFormFindError::EmptyFreeSet`] — every node anchored.
/// - [`AnisoFormFindError::SingularReducedStiffness`] — rank-deficient `D_ff`.
pub fn form_find_anchored_surfaces_aniso(
    nodes: &[[f64; 3]],
    members: &[(usize, usize)],
    kinds: &[MemberKind],
    q: &[f64],
    surfaces: &[(usize, usize, usize)],
    surface_prestress: &[AnisotropicSurfaceStress],
    anchors: &[usize],
) -> Result<AnisoFormFindSolve, AnisoFormFindError> {
    let n = nodes.len();

    if members.len() != kinds.len() || members.len() != q.len() {
        return Err(AnisoFormFindError::DimensionMismatch);
    }
    if surfaces.len() != surface_prestress.len() {
        return Err(AnisoFormFindError::SurfaceCountMismatch);
    }
    for (&kind, &qi) in kinds.iter().zip(q.iter()) {
        let sign_ok = match kind {
            MemberKind::Cable => qi > 0.0,
            MemberKind::Strut => qi < 0.0,
        };
        if !sign_ok {
            return Err(AnisoFormFindError::SignViolation);
        }
    }
    for spec in surface_prestress {
        if spec.sigma_warp <= 0.0 || spec.sigma_weft <= 0.0 {
            return Err(AnisoFormFindError::NonTensionSurfaceStress);
        }
    }

    let mut is_anchor = vec![false; n];
    for &a in anchors {
        is_anchor[a] = true;
    }
    let free_indices: Vec<usize> = (0..n).filter(|&i| !is_anchor[i]).collect();
    let anchor_indices: Vec<usize> = (0..n).filter(|&i| is_anchor[i]).collect();
    if free_indices.is_empty() {
        return Err(AnisoFormFindError::EmptyFreeSet);
    }

    let mut current = nodes.to_vec();
    let mut converged = false;
    let max_iters = if surfaces.is_empty() { 1 } else { MAX_SURFACE_ITERS };
    for _iter in 0..max_iters {
        let d = match assemble_d_aniso(n, members, q, surfaces, surface_prestress, &current) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[DIAG-ANISO] assemble_d_aniso failed at iter={_iter}: {e:?}");
                // Find which triangle failed.
                for (t, (&(i, j, k), spec)) in surfaces.iter().zip(surface_prestress.iter()).enumerate() {
                    if triangle_anisotropic_laplacian(current[i], current[j], current[k], spec).is_err() {
                        let pi = current[i];
                        let pj = current[j];
                        let pk = current[k];
                        let eij = v_sub(pj, pi);
                        let eik = v_sub(pk, pi);
                        let cr = v_cross(eij, eik);
                        let two_area = v_dot(cr, cr).sqrt();
                        let scale = v_dot(eij, eij).max(v_dot(eik, eik));
                        eprintln!("[DIAG-ANISO]   triangle t={t} ({i},{j},{k}): pi={pi:?} pj={pj:?} pk={pk:?}");
                        eprintln!("[DIAG-ANISO]   two_area={two_area:.6e} scale={scale:.6e} ratio={:.6e}", two_area / scale.max(1e-300).sqrt());
                    }
                }
                return Err(e);
            }
        };

        if !surfaces.is_empty()
            && free_equilibrium_residual(&d, &current, &free_indices) <= SURFACE_EQUILIBRIUM_TOL
        {
            converged = true;
            break;
        }

        let solved = solve_reduced(&d, &current, &free_indices, &anchor_indices)
            .map_err(map_ff_error)?;
        current = solved;

        if surfaces.is_empty() {
            converged = true;
            break;
        }
    }
    let out_nodes = current;

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

    // Populate principal stresses per triangle on the solved geometry (declaration
    // order). Since S = diag(σ_w, σ_f) is diagonal in the material frame, recovery
    // is closed-form via recover_principal_stress. Errors (DegenerateTriangle /
    // DegenerateMaterialFrame) propagate as AnisoFormFindError — if a triangle
    // degenerates at the converged shape, the result is ill-defined.
    //
    // BREADCRUMB (D1, task ε / plan.json): the free-standing combined anisotropic
    // form_find_free path (aniso NFDM + null-space q search on a free network) is
    // a recorded out-of-scope follow-up for this leaf. The anchored carrier is the
    // ε signal: a fixed-boundary patch that converges to a shape DISTINCT from the
    // isotropic minimal surface. Keeping this leaf minimal avoids dragging in the
    // free-standing GroupRatios / nullity-4 search machinery (PRD D1).
    let mut principal_stresses: Vec<PrincipalStress> = Vec::with_capacity(surfaces.len());
    for (t, (&(i, j, k), spec)) in surfaces.iter().zip(surface_prestress.iter()).enumerate() {
        let ps = match recover_principal_stress(out_nodes[i], out_nodes[j], out_nodes[k], spec) {
            Ok(ps) => ps,
            Err(e) => {
                let pi = out_nodes[i];
                let pj = out_nodes[j];
                let pk = out_nodes[k];
                let eij = v_sub(pj, pi);
                let eik = v_sub(pk, pi);
                let cr = v_cross(eij, eik);
                let two_area = v_dot(cr, cr).sqrt();
                let scale = v_dot(eij, eij).max(v_dot(eik, eik));
                eprintln!("[DIAG-ANISO] recover_principal_stress failed on triangle t={t} ({i},{j},{k}): {e:?}");
                eprintln!("[DIAG-ANISO]   pi={pi:?} pj={pj:?} pk={pk:?}");
                eprintln!("[DIAG-ANISO]   two_area={two_area:.6e} scale={scale:.6e} ratio={:.6e}", two_area / scale.sqrt());
                return Err(e);
            }
        };
        principal_stresses.push(ps);
    }
    Ok(AnisoFormFindSolve {
        nodes: out_nodes,
        member_forces,
        force_densities: q.to_vec(),
        principal_stresses,
        converged,
    })
}

/// Build the per-triangle in-plane material frame `(e₁, e₂, n)` where
/// `e₁ = normalise(project(warp_dir, onto triangle plane))` and `e₂ = n×e₁`.
/// Shared by [`triangle_anisotropic_laplacian`] and [`recover_principal_stress`].
#[allow(clippy::type_complexity)]
fn build_material_frame(
    pi: [f64; 3],
    pj: [f64; 3],
    pk: [f64; 3],
    warp_dir: [f64; 3],
) -> Result<([f64; 3], [f64; 3], [f64; 3]), AnisoFormFindError> {
    let eij = v_sub(pj, pi);
    let eik = v_sub(pk, pi);
    let ejk = v_sub(pk, pj);

    // Unit normal and 2·Area.
    let cross = v_cross(eij, eik);
    let two_area = v_dot(cross, cross).sqrt();
    let scale = v_dot(eij, eij).max(v_dot(eik, eik)).max(v_dot(ejk, ejk));
    if two_area <= DEGENERATE_AREA_EPS * scale {
        return Err(AnisoFormFindError::DegenerateTriangle);
    }
    let n = [cross[0] / two_area, cross[1] / two_area, cross[2] / two_area];

    // In-plane projection of warp_dir.
    let wd_dot_n = v_dot(warp_dir, n);
    let wip = [
        warp_dir[0] - wd_dot_n * n[0],
        warp_dir[1] - wd_dot_n * n[1],
        warp_dir[2] - wd_dot_n * n[2],
    ];

    // Guard: warp_dir ∥ n ⇒ projection ≈ 0.
    let wip_norm_sq = v_dot(wip, wip);
    let wd_norm_sq = v_dot(warp_dir, warp_dir);
    // Relative threshold: (wip_norm / wd_norm) < 1e-8 → degenerate.
    if wip_norm_sq < 1e-16 * wd_norm_sq {
        return Err(AnisoFormFindError::DegenerateMaterialFrame);
    }
    let wip_norm = wip_norm_sq.sqrt();

    let e1 = [wip[0] / wip_norm, wip[1] / wip_norm, wip[2] / wip_norm];
    let e2 = v_cross(n, e1); // right-hand in-plane perpendicular
    Ok((e1, e2, n))
}

/// Recovered per-triangle principal stress directions and magnitudes for an
/// anisotropic NFDM surface element (ε / task 4416).
///
/// Since `S = diag(σ_w, σ_f)` is diagonal in the material frame `(e₁, e₂)`,
/// the principal stresses are the two diagonal entries and the principal
/// directions are the corresponding frame axes.
#[derive(Debug, Clone)]
pub struct PrincipalStress {
    /// Major principal direction (unit vector, in the triangle plane): the
    /// frame axis carrying the larger of `σ_w` / `σ_f`.
    pub major_dir: [f64; 3],
    /// Minor principal direction (unit, in-plane, ⊥ `major_dir`).
    pub minor_dir: [f64; 3],
    /// Major principal stress magnitude (`max(σ_w, σ_f)`).
    pub major: f64,
    /// Minor principal stress magnitude (`min(σ_w, σ_f)`).
    pub minor: f64,
}

/// Recover the principal stresses and directions for a triangle under the
/// given [`AnisotropicSurfaceStress`].
///
/// Since `S = diag(σ_w, σ_f)` is already diagonal in the per-triangle
/// material frame `(e₁, e₂)`, recovery is closed-form: the larger eigenvalue
/// picks its frame axis as `major_dir`. Designed to be called on the **solved**
/// geometry so the triangle normal (and thus the frame) reflects the
/// equilibrium shape.
///
/// # Errors
/// - [`AnisoFormFindError::DegenerateTriangle`] — zero-area triangle.
/// - [`AnisoFormFindError::DegenerateMaterialFrame`] — `warp_dir ∥ n`.
pub(crate) fn recover_principal_stress(
    pi: [f64; 3],
    pj: [f64; 3],
    pk: [f64; 3],
    spec: &AnisotropicSurfaceStress,
) -> Result<PrincipalStress, AnisoFormFindError> {
    let (e1, e2, _n) = build_material_frame(pi, pj, pk, spec.warp_dir)?;
    let sw = spec.sigma_warp;
    let sf = spec.sigma_weft;
    if sw >= sf {
        Ok(PrincipalStress { major_dir: e1, minor_dir: e2, major: sw, minor: sf })
    } else {
        Ok(PrincipalStress { major_dir: e2, minor_dir: e1, major: sf, minor: sw })
    }
}

/// Per-triangle anisotropic NFDM stencil `L_T[a][b] = Area·(∇N_a·S·∇N_b)`
/// in the per-triangle material frame `(e₁, e₂)`, `S = diag(σ_w, σ_f)`.
/// Rows/cols indexed `0=i, 1=j, 2=k` (matching the argument order).
///
/// **Correctness anchor** (σ_w = σ_f = σ): the stencil equals
/// [`triangle_cotangent_laplacian`] entrywise to machine precision — a true
/// mathematical identity, frame-independently (step-1 RED/GREEN test).
///
/// The returned 3×3 is symmetric and each row sums to zero (graph Laplacian).
///
/// # Errors
/// - [`AnisoFormFindError::DegenerateTriangle`] — zero-area triangle.
/// - [`AnisoFormFindError::DegenerateMaterialFrame`] — `warp_dir ∥ n`.
pub(crate) fn triangle_anisotropic_laplacian(
    pi: [f64; 3],
    pj: [f64; 3],
    pk: [f64; 3],
    spec: &AnisotropicSurfaceStress,
) -> Result<[[f64; 3]; 3], AnisoFormFindError> {
    let (e1, e2, _n) = build_material_frame(pi, pj, pk, spec.warp_dir)?;

    let eij = v_sub(pj, pi);
    let eik = v_sub(pk, pi);

    // Project vertices to 2D coords (e₁, e₂) with pᵢ as origin.
    // pᵢ → (0, 0);  pⱼ → (xⱼ, yⱼ);  pₖ → (xₖ, yₖ).
    let xj = v_dot(eij, e1);
    let yj = v_dot(eij, e2);
    let xk = v_dot(eik, e1);
    let yk = v_dot(eik, e2);

    // Signed 2D area  A₂ = (xⱼ yₖ − xₖ yⱼ) / 2.
    let two_area_2d = xj * yk - xk * yj;
    let area = two_area_2d.abs() * 0.5;
    let inv_2a = 1.0 / two_area_2d;

    // CST shape-function gradients (constant across triangle):
    //   ∇N_i = [(yⱼ−yₖ)·inv2a,  (xₖ−xⱼ)·inv2a]
    //   ∇N_j = [yₖ·inv2a,        −xₖ·inv2a     ]
    //   ∇N_k = [−yⱼ·inv2a,        xⱼ·inv2a     ]
    // Row sums of x-grads: (yⱼ−yₖ+yₖ−yⱼ)·inv2a = 0. Same for y-grads → row
    // sums of L_T are zero by construction regardless of σ_w, σ_f values.
    let g = [
        [(yj - yk) * inv_2a, (xk - xj) * inv_2a],
        [yk * inv_2a, -xk * inv_2a],
        [-yj * inv_2a, xj * inv_2a],
    ];

    // L_T[a][b] = Area · (σ_w · gₓ[a]·gₓ[b]  +  σ_f · g_y[a]·g_y[b]).
    // Symmetric by construction; row sums = 0 (proven above).
    let sw = spec.sigma_warp;
    let sf = spec.sigma_weft;
    let mut l = [[0.0_f64; 3]; 3];
    for a in 0..3 {
        for b in 0..3 {
            l[a][b] = area * (sw * g[a][0] * g[b][0] + sf * g[a][1] * g[b][1]);
        }
    }
    Ok(l)
}

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
pub(crate) fn triangle_cotangent_laplacian(
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

    // ── ε (task 4416): anisotropic warp/weft NFDM stencil ─────────────────────

    /// Tolerance for the anisotropic stencil reduction test (σ_w=σ_f → isotropic).
    /// Exact mathematical identity → machine precision, same convention as STENCIL_TOL.
    const ANISO_STENCIL_TOL: f64 = 1e-12;

    // (a) REDUCTION / CORRECTNESS ANCHOR: when sigma_warp == sigma_weft == σ,
    // the anisotropic stencil must equal the isotropic cotangent-Laplacian to
    // ≤1e-12, regardless of the in-plane warp_dir chosen (frame-independence at
    // σ_w=σ_f — a true mathematical identity, not a convergence claim).
    // Two different warp_dirs are tested on the same right-isosceles triangle.
    // Tests will not compile until step-2 introduces `AnisotropicSurfaceStress`
    // and `triangle_anisotropic_laplacian` (RED).
    #[test]
    fn aniso_stencil_reduces_to_isotropic_when_sigma_equal() {
        let pi = [0.0, 0.0, 0.0];
        let pj = [1.0, 0.0, 0.0];
        let pk = [0.0, 1.0, 0.0];
        let sigma = 2.5_f64;
        let iso = triangle_cotangent_laplacian(pi, pj, pk, sigma)
            .expect("non-degenerate right-isosceles triangle must yield cotangent stencil");

        // warp_dir aligned to one edge — in-plane, non-zero
        let spec1 = AnisotropicSurfaceStress {
            warp_dir: [1.0, 0.0, 0.0],
            sigma_warp: sigma,
            sigma_weft: sigma,
        };
        let aniso1 = triangle_anisotropic_laplacian(pi, pj, pk, &spec1)
            .expect("aniso stencil with warp=[1,0,0] must succeed on non-degenerate input");
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (aniso1[r][c] - iso[r][c]).abs() <= ANISO_STENCIL_TOL,
                    "reduction [1,0,0]: aniso[{r}][{c}]={} iso={} diff={}",
                    aniso1[r][c],
                    iso[r][c],
                    (aniso1[r][c] - iso[r][c]).abs(),
                );
            }
        }

        // warp_dir at 45° in-plane — frame-independence: same result
        let spec2 = AnisotropicSurfaceStress {
            warp_dir: [1.0, 1.0, 0.0],
            sigma_warp: sigma,
            sigma_weft: sigma,
        };
        let aniso2 = triangle_anisotropic_laplacian(pi, pj, pk, &spec2)
            .expect("aniso stencil with warp=[1,1,0] must succeed on non-degenerate input");
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (aniso2[r][c] - iso[r][c]).abs() <= ANISO_STENCIL_TOL,
                    "reduction [1,1,0]: aniso[{r}][{c}]={} iso={} diff={}",
                    aniso2[r][c],
                    iso[r][c],
                    (aniso2[r][c] - iso[r][c]).abs(),
                );
            }
        }
    }

    // (b) The anisotropic stencil is symmetric and every row sums to ~0 (a graph
    // Laplacian), even when sigma_warp ≠ sigma_weft.
    #[test]
    #[allow(clippy::needless_range_loop)]
    fn aniso_stencil_is_symmetric_and_row_sums_to_zero() {
        let pi = [0.0, 0.0, 0.0];
        let pj = [1.0, 0.0, 0.0];
        let pk = [0.0, 1.0, 0.0];
        let spec = AnisotropicSurfaceStress {
            warp_dir: [1.0, 0.0, 0.0],
            sigma_warp: 3.0,
            sigma_weft: 1.0,
        };
        let l = triangle_anisotropic_laplacian(pi, pj, pk, &spec)
            .expect("anisotropic stencil must succeed on non-degenerate input");
        // Symmetry
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (l[r][c] - l[c][r]).abs() <= ANISO_STENCIL_TOL,
                    "aniso stencil must be symmetric: l[{r}][{c}]={} l[{c}][{r}]={}",
                    l[r][c],
                    l[c][r],
                );
            }
        }
        // Row sums to zero
        for r in 0..3 {
            let row_sum: f64 = l[r].iter().sum();
            assert!(
                row_sum.abs() <= ANISO_STENCIL_TOL,
                "aniso stencil row {r} must sum to 0, got {row_sum}",
            );
        }
    }

    // ── ε step-3 RED: direction sensitivity and guards ─────────────────────────

    // (a) DIRECTION-SENSITIVITY: when σ_w ≠ σ_f the aniso stencil differs
    // measurably from the isotropic one, confirming the tensor actually acts.
    // Also: rotating warp_dir 90° in-plane (warp↔weft swap) yields the stencil
    // with σ_w/σ_f swapped — an exact identity (entrywise ≤ 1e-12).
    #[test]
    #[allow(clippy::needless_range_loop)]
    fn aniso_stencil_direction_sensitivity_and_90deg_swap() {
        let pi = [0.0, 0.0, 0.0];
        let pj = [1.0, 0.0, 0.0];
        let pk = [0.0, 1.0, 0.0];
        let sw = 3.0_f64;
        let sf = 1.0_f64;

        // Original warp along e₁=[1,0,0], weft along e₂=[0,1,0].
        let spec_orig = AnisotropicSurfaceStress {
            warp_dir: [1.0, 0.0, 0.0],
            sigma_warp: sw,
            sigma_weft: sf,
        };
        let l_orig = triangle_anisotropic_laplacian(pi, pj, pk, &spec_orig)
            .expect("aniso stencil must succeed");

        // Isotropic baseline with σ = sw.
        let l_iso = triangle_cotangent_laplacian(pi, pj, pk, sw)
            .expect("isotropic stencil must succeed");

        // Measurable difference: max |entry diff| well above zero.
        let max_diff: f64 = (0..3)
            .flat_map(|r| (0..3).map(move |c| (l_orig[r][c] - l_iso[r][c]).abs()))
            .fold(0.0_f64, f64::max);
        assert!(
            max_diff > 0.1,
            "aniso stencil with σ_w≠σ_f must differ from isotropic; max_diff={max_diff}",
        );

        // 90° in-plane rotation: new warp_dir = old e₂ direction.
        // This should give the stencil with σ_w and σ_f swapped.
        let spec_swap = AnisotropicSurfaceStress {
            warp_dir: [0.0, 1.0, 0.0],
            sigma_warp: sw,
            sigma_weft: sf,
        };
        let l_swap = triangle_anisotropic_laplacian(pi, pj, pk, &spec_swap)
            .expect("90°-rotated aniso stencil must succeed");

        // The swap stencil must equal the original with σ_w↔σ_f.
        let spec_swapped_sigma = AnisotropicSurfaceStress {
            warp_dir: [1.0, 0.0, 0.0],
            sigma_warp: sf, // swapped
            sigma_weft: sw, // swapped
        };
        let l_swapped_sigma = triangle_anisotropic_laplacian(pi, pj, pk, &spec_swapped_sigma)
            .expect("swapped-sigma stencil must succeed");

        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (l_swap[r][c] - l_swapped_sigma[r][c]).abs() <= ANISO_STENCIL_TOL,
                    "90° swap identity: l_swap[{r}][{c}]={} vs swapped-sigma[{r}][{c}]={} diff={}",
                    l_swap[r][c],
                    l_swapped_sigma[r][c],
                    (l_swap[r][c] - l_swapped_sigma[r][c]).abs(),
                );
            }
        }
    }

    // (b) GUARDS: degenerate triangle and degenerate material frame.
    #[test]
    fn aniso_stencil_degenerate_triangle_guard() {
        let a = [0.0, 0.0, 0.0];
        let b = [1.0, 0.0, 0.0];
        let c = [2.0, 0.0, 0.0]; // collinear → zero area
        let spec = AnisotropicSurfaceStress {
            warp_dir: [0.0, 1.0, 0.0],
            sigma_warp: 1.0,
            sigma_weft: 1.0,
        };
        assert_eq!(
            triangle_anisotropic_laplacian(a, b, c, &spec).unwrap_err(),
            AnisoFormFindError::DegenerateTriangle,
        );
    }

    #[test]
    fn aniso_stencil_degenerate_material_frame_guard() {
        // Triangle in the z=0 plane; warp_dir = [0,0,1] is the triangle normal
        // → in-plane projection ≈ 0 → DegenerateMaterialFrame.
        let pi = [0.0, 0.0, 0.0];
        let pj = [1.0, 0.0, 0.0];
        let pk = [0.0, 1.0, 0.0];
        let spec = AnisotropicSurfaceStress {
            warp_dir: [0.0, 0.0, 1.0], // parallel to normal [0,0,1]
            sigma_warp: 1.0,
            sigma_weft: 1.0,
        };
        assert_eq!(
            triangle_anisotropic_laplacian(pi, pj, pk, &spec).unwrap_err(),
            AnisoFormFindError::DegenerateMaterialFrame,
        );
    }

    // ── ε step-5 RED: recover_principal_stress ─────────────────────────────────

    // For S = diag(σ_w, σ_f) in frame (e₁, e₂):
    // - major == max(σ_w, σ_f), minor == min; directions are the frame axes.
    // - major_dir ⊥ minor_dir; both lie in the triangle plane (|dir·n| ≤ 1e-12).
    // - |major_dir · ê₁| ≥ 1−1e-12 when σ_w > σ_f (major along warp axis).
    // - |major_dir · ê₂| ≥ 1−1e-12 when σ_f > σ_w (major along weft axis).
    // Fails until step-6 introduces `PrincipalStress` and `recover_principal_stress`.
    #[test]
    fn principal_stress_recovery_warp_major() {
        let pi = [0.0, 0.0, 0.0];
        let pj = [1.0, 0.0, 0.0];
        let pk = [0.0, 1.0, 0.0];
        // n = [0,0,1]; e1 = [1,0,0] (warp); e2 = [0,1,0] (weft).
        let spec = AnisotropicSurfaceStress {
            warp_dir: [1.0, 0.0, 0.0],
            sigma_warp: 3.0,
            sigma_weft: 1.0,
        };
        let ps = recover_principal_stress(pi, pj, pk, &spec)
            .expect("well-posed triangle must return PrincipalStress");

        // Magnitudes match σ_w and σ_f.
        assert!(
            (ps.major - 3.0).abs() < 1e-12,
            "major={} expected 3.0",
            ps.major
        );
        assert!(
            (ps.minor - 1.0).abs() < 1e-12,
            "minor={} expected 1.0",
            ps.minor
        );

        // major_dir aligns to ê₁ (warp axis [1,0,0]).
        let e1 = [1.0_f64, 0.0, 0.0];
        let align = ps.major_dir[0] * e1[0] + ps.major_dir[1] * e1[1] + ps.major_dir[2] * e1[2];
        assert!(
            align.abs() >= 1.0 - 1e-12,
            "|major_dir·ê₁|={} expected ≥1−1e-12",
            align.abs()
        );

        // minor_dir aligns to ê₂ ([0,1,0]).
        let e2 = [0.0_f64, 1.0, 0.0];
        let align2 = ps.minor_dir[0] * e2[0] + ps.minor_dir[1] * e2[1] + ps.minor_dir[2] * e2[2];
        assert!(
            align2.abs() >= 1.0 - 1e-12,
            "|minor_dir·ê₂|={} expected ≥1−1e-12",
            align2.abs()
        );

        // Orthogonality: major_dir ⊥ minor_dir.
        let dot_dirs = ps.major_dir[0] * ps.minor_dir[0]
            + ps.major_dir[1] * ps.minor_dir[1]
            + ps.major_dir[2] * ps.minor_dir[2];
        assert!(
            dot_dirs.abs() < 1e-12,
            "major_dir · minor_dir = {} (must be 0)",
            dot_dirs
        );

        // Both directions lie in the triangle plane (|dir·n| ≤ 1e-12).
        let n = [0.0_f64, 0.0, 1.0];
        let major_n =
            ps.major_dir[0] * n[0] + ps.major_dir[1] * n[1] + ps.major_dir[2] * n[2];
        let minor_n =
            ps.minor_dir[0] * n[0] + ps.minor_dir[1] * n[1] + ps.minor_dir[2] * n[2];
        assert!(
            major_n.abs() < 1e-12,
            "|major_dir·n|={} must be <1e-12",
            major_n.abs()
        );
        assert!(
            minor_n.abs() < 1e-12,
            "|minor_dir·n|={} must be <1e-12",
            minor_n.abs()
        );
    }

    #[test]
    fn principal_stress_recovery_weft_major() {
        // Swap: σ_f > σ_w → major along ê₂.
        let pi = [0.0, 0.0, 0.0];
        let pj = [1.0, 0.0, 0.0];
        let pk = [0.0, 1.0, 0.0];
        let spec = AnisotropicSurfaceStress {
            warp_dir: [1.0, 0.0, 0.0],
            sigma_warp: 1.0,
            sigma_weft: 4.0,
        };
        let ps = recover_principal_stress(pi, pj, pk, &spec)
            .expect("well-posed triangle must return PrincipalStress");

        assert!(
            (ps.major - 4.0).abs() < 1e-12,
            "major={} expected 4.0",
            ps.major
        );
        assert!(
            (ps.minor - 1.0).abs() < 1e-12,
            "minor={} expected 1.0",
            ps.minor
        );

        // major_dir aligns to ê₂ ([0,1,0]) when weft dominates.
        let e2 = [0.0_f64, 1.0, 0.0];
        let align = ps.major_dir[0] * e2[0] + ps.major_dir[1] * e2[1] + ps.major_dir[2] * e2[2];
        assert!(
            align.abs() >= 1.0 - 1e-12,
            "|major_dir·ê₂|={} expected ≥1−1e-12 (weft-major swap case)",
            align.abs()
        );
    }

    #[test]
    fn principal_stress_recovery_degenerate_frame() {
        let pi = [0.0, 0.0, 0.0];
        let pj = [1.0, 0.0, 0.0];
        let pk = [0.0, 1.0, 0.0];
        let spec = AnisotropicSurfaceStress {
            warp_dir: [0.0, 0.0, 1.0], // warp ∥ n → degenerate
            sigma_warp: 1.0,
            sigma_weft: 1.0,
        };
        assert_eq!(
            recover_principal_stress(pi, pj, pk, &spec).unwrap_err(),
            AnisoFormFindError::DegenerateMaterialFrame,
        );
    }

    // ── ε step-7 RED: form_find_anchored_surfaces_aniso guards + iso-equiv ──────

    /// Minimal fixed-boundary tent fixture reused for aniso solve tests.
    /// One free interior node, 4 anchored corners in the z=0 plane, 4 triangles.
    #[allow(clippy::type_complexity)]
    fn tent_aniso_fixture() -> (
        Vec<[f64; 3]>,
        Vec<(usize, usize, usize)>,
        Vec<AnisotropicSurfaceStress>,
        Vec<usize>,
    ) {
        let nodes = vec![
            [0.1, 0.1, 0.3],  // 0: free interior node (off-plane seed)
            [1.0, 0.0, 0.0],  // 1: anchor
            [0.0, 1.0, 0.0],  // 2: anchor
            [-1.0, 0.0, 0.0], // 3: anchor
            [0.0, -1.0, 0.0], // 4: anchor
        ];
        let surfaces = vec![(0, 1, 2), (0, 2, 3), (0, 3, 4), (0, 4, 1)];
        let anchors = vec![1, 2, 3, 4];
        let sigma = 2.0;
        let prestress = vec![
            AnisotropicSurfaceStress { warp_dir: [1.0, 0.0, 0.0], sigma_warp: sigma, sigma_weft: sigma };
            surfaces.len()
        ];
        (nodes, surfaces, prestress, anchors)
    }

    // (a) surfaces/surface_prestress length mismatch → SurfaceCountMismatch.
    #[test]
    fn aniso_solve_surface_count_mismatch() {
        let (nodes, surfaces, prestress, anchors) = tent_aniso_fixture();
        let short_prestress = prestress[..prestress.len() - 1].to_vec();
        let members: Vec<(usize, usize)> = vec![];
        let kinds: Vec<MemberKind> = vec![];
        let q: Vec<f64> = vec![];
        assert_eq!(
            form_find_anchored_surfaces_aniso(
                &nodes, &members, &kinds, &q, &surfaces, &short_prestress, &anchors
            )
            .unwrap_err(),
            AnisoFormFindError::SurfaceCountMismatch,
        );
    }

    // (b) members/kinds/q length mismatch → DimensionMismatch.
    #[test]
    fn aniso_solve_dimension_mismatch() {
        let (nodes, surfaces, prestress, anchors) = tent_aniso_fixture();
        let members = vec![(0usize, 1usize)];
        let kinds = vec![MemberKind::Cable];
        let q: Vec<f64> = vec![]; // length 0 ≠ 1
        assert_eq!(
            form_find_anchored_surfaces_aniso(
                &nodes, &members, &kinds, &q, &surfaces, &prestress, &anchors
            )
            .unwrap_err(),
            AnisoFormFindError::DimensionMismatch,
        );
    }

    // (c) A cable member with q ≤ 0 → SignViolation.
    #[test]
    fn aniso_solve_sign_violation() {
        let (nodes, surfaces, prestress, anchors) = tent_aniso_fixture();
        let members = vec![(0usize, 1usize)];
        let kinds = vec![MemberKind::Cable];
        let q = vec![-1.0_f64]; // cable must be > 0
        assert_eq!(
            form_find_anchored_surfaces_aniso(
                &nodes, &members, &kinds, &q, &surfaces, &prestress, &anchors
            )
            .unwrap_err(),
            AnisoFormFindError::SignViolation,
        );
    }

    // (d) σ_w ≤ 0 → NonTensionSurfaceStress.
    #[test]
    fn aniso_solve_non_tension_surface_stress() {
        let (nodes, surfaces, _prestress, anchors) = tent_aniso_fixture();
        let bad_prestress = vec![
            AnisotropicSurfaceStress {
                warp_dir: [1.0, 0.0, 0.0],
                sigma_warp: -1.0, // ≤ 0
                sigma_weft: 1.0,
            };
            surfaces.len()
        ];
        let members: Vec<(usize, usize)> = vec![];
        let kinds: Vec<MemberKind> = vec![];
        let q: Vec<f64> = vec![];
        assert_eq!(
            form_find_anchored_surfaces_aniso(
                &nodes, &members, &kinds, &q, &surfaces, &bad_prestress, &anchors
            )
            .unwrap_err(),
            AnisoFormFindError::NonTensionSurfaceStress,
        );
    }

    // (e) All nodes anchored → EmptyFreeSet.
    #[test]
    fn aniso_solve_empty_free_set() {
        let (nodes, surfaces, prestress, _) = tent_aniso_fixture();
        let all_anchors: Vec<usize> = (0..nodes.len()).collect();
        let members: Vec<(usize, usize)> = vec![];
        let kinds: Vec<MemberKind> = vec![];
        let q: Vec<f64> = vec![];
        assert_eq!(
            form_find_anchored_surfaces_aniso(
                &nodes, &members, &kinds, &q, &surfaces, &prestress, &all_anchors
            )
            .unwrap_err(),
            AnisoFormFindError::EmptyFreeSet,
        );
    }

    // (f) ISOTROPIC-EQUIVALENCE: aniso solve with σ_w = σ_f = σ matches the
    // isotropic solve to ≤ 1e-9 on node positions; converged == true.
    #[test]
    fn aniso_solve_isotropic_equivalence() {
        let (nodes, surfaces, prestress, anchors) = tent_aniso_fixture();
        let sigma = prestress[0].sigma_warp; // 2.0
        let sigmas = vec![sigma; surfaces.len()];
        let members: Vec<(usize, usize)> = vec![];
        let kinds: Vec<MemberKind> = vec![];
        let q: Vec<f64> = vec![];

        let aniso = form_find_anchored_surfaces_aniso(
            &nodes, &members, &kinds, &q, &surfaces, &prestress, &anchors,
        )
        .expect("σ_w=σ_f aniso solve must be feasible");

        let iso = form_find_anchored_surfaces(
            &nodes, &members, &kinds, &q, &surfaces, &sigmas, &anchors,
        )
        .expect("isotropic reference solve must be feasible");

        assert!(aniso.converged, "aniso solve must converge");
        assert_eq!(aniso.nodes.len(), iso.nodes.len());
        for (i, (a, b)) in aniso.nodes.iter().zip(iso.nodes.iter()).enumerate() {
            let err = max_coord_err(*a, *b);
            assert!(
                err < 1e-9,
                "node[{i}] aniso={a:?} iso={b:?} max_coord_err={err:e}",
            );
        }
    }
}

