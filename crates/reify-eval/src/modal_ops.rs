//! Compute trampoline for `modal::free_vibration` — the `fn modal_analysis`
//! `@optimized` target (task ζ, docs/prds/v0_3/modal-analysis.md §10).
//!
//! Hosts the modal free-vibration FEA solve (assemble K + M, free-DOF
//! eigensolve via `reify-solver-elastic`) and the `Value`-shaping trampoline.
//! Lives in `reify-eval` — not `reify-stdlib` — because the solve needs
//! `reify-solver-elastic` (which `reify-stdlib` does not depend on); `reify-eval`
//! depends on both. Mirrors `compute_targets/buckling.rs`.
//!
//! `solve_modal_core` is the core FEA eigensolve; the public
//! `solve_modal_analysis_trampoline` wires it into the `@optimized` dispatch
//! path (registered as `modal::free_vibration` in `compute_targets::mod`). The
//! trampoline transitively reaches the mesh / projection / density-guard helpers,
//! so they need no `#[allow(dead_code)]`. `ModalCoreResult` keeps a struct-level
//! `#[allow(dead_code)]`: several fields (eigenvalues, `phi_free`, the `m_free`
//! handle, the convergence counts) are read only by the unit tests; `phi_full`
//! is read by both the trampoline (serialized as `Mode.shape`) and the tests.

use std::f64::consts::PI;

use faer::sparse::{SparseRowMat, Triplet};

use reify_core::{Diagnostic, DimensionVector};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, DirichletBc, EigenSolverOptions, EigenSolverResult,
    ElementOrder, ElementStiffness, IsotropicElastic, JointStiffness, add_joint_stiffness,
    assemble_global_stiffness, consistent_element_mass_tet_p1, consistent_element_mass_tet_p2,
    element_stiffness, solve_eigen_dense, solve_eigen_shift_invert,
};
use reify_stdlib::dynamics::mass_props::resolve_density_strict;
use reify_stdlib::{mass_properties_from_value, resolve_body_mass};
use reify_stdlib::modal::free_vibration::{
    eigenvalue_to_frequency_hz, is_rigid_body_mode, mass_normalization_scale,
    modal_participation_mass, rayleigh_damping_ratio,
};
use reify_stdlib::modal::trampoline::{ModalCacheKey, TransientCacheKey};
use reify_stdlib::modal::transient::{
    PreparedIntegrator, dominant_antinode_index, harmonic_force_at, impulse_force_at,
    integrate_prepared, prepare_modal_integrator, reconstruct_series, sampled_force_at,
    solve_modal_response, step_force_at, uniform_time_grid,
};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

// ---------------------------------------------------------------------------
// Beam mesh
// ---------------------------------------------------------------------------

/// P1-tet beam mesh shared by [`solve_modal_core`] and its unit tests.
///
/// Layout: X = beam axis (length), Y = width, Z = height (bending axis) —
/// identical to `compute_targets::elastic_static::solve_cantilever_fea`.
pub(crate) struct BeamMesh {
    /// Node coordinates `[x, y, z]`, length `n_nodes`.
    pub(crate) nodes: Vec<[f64; 3]>,
    /// Element connectivity; each tet is `[n0, n1, n2, n3]` (positive Jacobian).
    pub(crate) tets: Vec<[usize; 4]>,
}

/// Build a Freudenthal hex-split P1-tet beam mesh with shear-locking-aware `nx`
/// scaling.
///
/// `nz = 6` fixed; `nx ∝ nz·(L/h)` (rounded, clamped ≥ 1) keeps the bending-plane
/// (XZ) elements near-cubic so the P1 constant-strain tets do not lock in
/// bending; `ny = 1` (bending is about Y). This mirrors `solve_cantilever_fea`'s
/// meshing so the modal mesh matches the validated elastic-static pattern.
// G-allow: modal::free_vibration ComputeFn pipeline (task #4066) — beam-mesh builder reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140), which the orphan audit cannot trace. Wired + tested in this file.
pub(crate) fn build_beam_mesh(length: f64, width: f64, height: f64) -> BeamMesh {
    let nz: usize = 6;
    // Clamp to ≥ 1 to handle degenerate geometry (height ≈ or ≫ length).
    let nx: usize = ((length / height * nz as f64).round() as usize).max(1);
    let ny: usize = 1;
    let nx1 = nx + 1;
    let ny1 = ny + 1;
    let nz1 = nz + 1;
    let n_nodes = nx1 * ny1 * nz1;

    let node_idx = |ix: usize, iy: usize, iz: usize| -> usize { iz * ny1 * nx1 + iy * nx1 + ix };
    let node_coord = |ix: usize, iy: usize, iz: usize| -> [f64; 3] {
        [
            ix as f64 * length / nx as f64,
            iy as f64 * width / ny as f64,
            iz as f64 * height / nz as f64,
        ]
    };

    let mut nodes = vec![[0.0_f64; 3]; n_nodes];
    for iz in 0..nz1 {
        for iy in 0..ny1 {
            for ix in 0..nx1 {
                nodes[node_idx(ix, iy, iz)] = node_coord(ix, iy, iz);
            }
        }
    }

    // Freudenthal 6-tet decomposition of each hex sharing the body diagonal
    // c[0]→c[6]; node order chosen for a positive Jacobian (cf. elastic_static).
    let mut tets: Vec<[usize; 4]> = Vec::with_capacity(nx * ny * nz * 6);
    for hz in 0..nz {
        for hy in 0..ny {
            for hx in 0..nx {
                let c = [
                    node_idx(hx, hy, hz),
                    node_idx(hx + 1, hy, hz),
                    node_idx(hx + 1, hy + 1, hz),
                    node_idx(hx, hy + 1, hz),
                    node_idx(hx, hy, hz + 1),
                    node_idx(hx + 1, hy, hz + 1),
                    node_idx(hx + 1, hy + 1, hz + 1),
                    node_idx(hx, hy + 1, hz + 1),
                ];
                tets.extend_from_slice(&[
                    [c[0], c[1], c[2], c[6]],
                    [c[0], c[2], c[3], c[6]],
                    [c[0], c[5], c[1], c[6]],
                    [c[0], c[3], c[7], c[6]],
                    [c[0], c[4], c[5], c[6]],
                    [c[0], c[7], c[4], c[6]],
                ]);
            }
        }
    }

    BeamMesh { nodes, tets }
}

/// The finite-element discretization [`solve_modal_core`] assembles `K`/`M` over,
/// with the element order carried by the variant.
///
/// `P1` borrows the original 4-node [`BeamMesh`] and assembles the constant-strain
/// path directly. `P2` carries a *pre-promoted* 10-node tet mesh (edge-midpoint
/// nodes already inserted). Promoting once in the caller and handing the result
/// here lets the Dirichlet BC realization and the K/M assembly share a single
/// `promote_beam_mesh_to_p2` walk instead of each recomputing it — eliminating the
/// duplicated O(elements) promotion and the latent risk of the two promotion sites
/// drifting (the trampoline previously promoted once for BCs and `solve_modal_core`
/// promoted again for assembly).
pub(crate) enum ModalMesh<'a> {
    /// P1 constant-strain path: the 4-node beam mesh, used directly.
    P1(&'a BeamMesh),
    /// P2 path: the pre-promoted 10-node tet mesh (`nodes`, `tets`).
    P2 {
        nodes: &'a [[f64; 3]],
        tets: &'a [[usize; 10]],
    },
}

impl ModalMesh<'_> {
    /// The node coordinates this discretization assembles against — the P1 mesh
    /// nodes, or the promoted P2 node set. The BC realization selects constrained
    /// DOFs by node coordinate over exactly this set, so the BC DOF indices line up
    /// with the assembled `K`/`M` node numbering.
    fn nodes(&self) -> &[[f64; 3]] {
        match self {
            ModalMesh::P1(mesh) => &mesh.nodes,
            ModalMesh::P2 { nodes, .. } => nodes,
        }
    }
}

// ---------------------------------------------------------------------------
// Core modal solve
// ---------------------------------------------------------------------------

/// Output of [`solve_modal_core`].
///
/// Field consumption is staged: `frequencies` / `phi_full` / `n_nodes` are
/// pinned now (step 3/4); `eigenvalues` / `phi_free` / `m_free` feed mass
/// normalization + participation mass (steps 5–8); `participation_mass` is the
/// per-mode effective mass along the reference direction (step 8);
/// `converged` / `n_converged` feed the convergence diagnostics (steps 9–10) and
/// the trampoline outcome (step 14). `#[allow(dead_code)]` covers the
/// not-yet-read fields during that staged build-up.
#[allow(dead_code)]
pub(crate) struct ModalCoreResult {
    /// Natural frequencies (Hz), ascending. One per returned mode.
    pub(crate) frequencies: Vec<f64>,
    /// Eigenvalues `λ = ω²` (rad²/s²), ascending by |λ|. One per mode.
    pub(crate) eigenvalues: Vec<f64>,
    /// Free-DOF mode shapes (length `n_free`), one per mode.
    pub(crate) phi_free: Vec<Vec<f64>>,
    /// Full-DOF mode shapes (length `3·n_nodes`, `0.0` at constrained DOFs).
    /// Read by the trampoline to serialize `Mode.shape` as per-node Vector3.
    pub(crate) phi_full: Vec<Vec<f64>>,
    /// Effective modal participation mass `m_eff,i = (φ_iᵀ·M_free·d_free)²`
    /// along the reference direction (φ mass-normalized), one per mode. Summed
    /// over a complete basis it equals the total translational mass along the
    /// reference direction (the completeness identity, PRD §4.1/§4.3).
    pub(crate) participation_mass: Vec<f64>,
    /// Free×free mass matrix `M_free` (feeds mass normalization + participation).
    pub(crate) m_free: SparseRowMat<usize, f64>,
    /// Mesh node count.
    pub(crate) n_nodes: usize,
    /// `true` iff the eigensolver returned all requested modes.
    pub(crate) converged: bool,
    /// Number of eigenpairs the underlying solver reported converged.
    pub(crate) n_converged: usize,
    /// Frobenius norm `‖M‖_F` of the full assembled consistent mass matrix —
    /// a BC-independent conditioning / sanity diagnostic surfaced on
    /// `ModalResult.mass_matrix_norm` (PRD §4.1).
    pub(crate) mass_matrix_norm: f64,
    /// Frobenius norm `‖K‖_F` of the full assembled stiffness matrix —
    /// the companion `ModalResult.stiffness_matrix_norm` diagnostic (PRD §4.1).
    pub(crate) stiffness_matrix_norm: f64,
    /// Non-fatal diagnostics surfaced by the solve: `W_ModalRigidBodyMode` (a
    /// near-zero / rigid-body mode → possible under-constraint) and
    /// `W_ModalConvergence` (fewer modes converged than requested). Message-
    /// based (`code: None`) per design_decision #6; the trampoline forwards
    /// these into the `ComputeOutcome` (step 14).
    pub(crate) diagnostics: Vec<Diagnostic>,
}

/// The assembled, BC- and `n_modes`-independent product of the modal solve: the
/// global stiffness `K` and consistent mass `M` (at the discretization's element
/// order), plus the `‖K‖_F`/`‖M‖_F` Frobenius norms and the node count.
///
/// This is exactly the expensive part the task-κ warm-state cache holds: it
/// depends only on the geometry + material + element order (the `ModalCacheKey`
/// inputs, in reify-stdlib), NOT on the boundary conditions, `n_modes`, or any
/// eigen knob — so one `ModalAssembly` is reused by [`eigensolve_modal`] across
/// calls that differ only in those. Deliberately omits node coordinates: a cache
/// HIT rebuilds the cheap mesh solely to realize the Dirichlet BCs by coordinate
/// (geometry + element order are in the key, so the rebuilt node count always
/// matches the cached assembly).
///
/// Holds faer `SparseRowMat<usize, f64>` directly — Vec-backed, hence
/// `Send + Sync + Clone + 'static`, so the cache wrapper stores it in an
/// `OpaqueState` with no CSR-component round-trip. `Clone` is the faer matrix
/// clone, used when recovering the cached assembly for reuse.
#[derive(Clone)]
pub(crate) struct ModalAssembly {
    /// Global stiffness matrix `K` over the full DOF set.
    pub(crate) k_full: SparseRowMat<usize, f64>,
    /// Global consistent mass matrix `M` over the full DOF set.
    pub(crate) m_full: SparseRowMat<usize, f64>,
    /// Frobenius norm `‖M‖_F` of the full consistent mass matrix (BC-independent
    /// conditioning diagnostic; copied onto `ModalResult.mass_matrix_norm`).
    pub(crate) mass_matrix_norm: f64,
    /// Frobenius norm `‖K‖_F` of the full stiffness matrix
    /// (`ModalResult.stiffness_matrix_norm`).
    pub(crate) stiffness_matrix_norm: f64,
    /// Mesh node count (P1 4-node, or P2 promoted 10-node); `K`/`M` order = 3·it.
    pub(crate) n_nodes: usize,
}

/// Assemble the global stiffness `K` and consistent mass `M` over a prebuilt
/// [`ModalMesh`] at its element order, returning a [`ModalAssembly`] (the
/// matrices plus their `‖K‖_F`/`‖M‖_F` norms and the node count).
///
/// The expensive, BC-/`n_modes`-independent half of [`solve_modal_core`] — the
/// product the task-κ warm-state cache holds so it can be amortized across calls
/// that change only the eigensolve inputs. The assembly logic is MOVED verbatim
/// from the original `solve_modal_core`, so its output is bit-identical.
///
/// P1 keeps the original constant-strain path bit-for-bit. P2 receives a
/// pre-promoted 10-node mesh (edge-midpoint nodes already inserted by the
/// caller) and assembles the quadratic stiffness + the exact
/// (degree-4-integrated) consistent mass over it — the lever that resolves
/// bending curvature and removes the P1 lock (task 4066). Both orders route
/// through the shared generic `assemble_global_matrix` (K and M differ only in
/// the per-element kernel). Everything downstream (free-DOF projection,
/// participation metric, eigensolve, scatter-back) is DOF-index based and
/// element-order agnostic, so it consumes the resulting `(n_nodes, k_full,
/// m_full)` unchanged regardless of order.
// G-allow: modal::free_vibration ComputeFn pipeline (task #4066) — K/M assembler reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140), which the orphan audit cannot trace. Wired + tested in this file.
pub(crate) fn assemble_modal_km(
    mesh: ModalMesh<'_>,
    density: f64,
    material: &IsotropicElastic,
) -> ModalAssembly {
    let (n_nodes, k_full, m_full) = match mesh {
        ModalMesh::P1(mesh) => {
            let k_full = assemble_global_matrix(&mesh.nodes, &mesh.tets, |phys| {
                element_stiffness(ElementOrder::P1, &phys[..], material)
            });
            let m_full = assemble_global_matrix(&mesh.nodes, &mesh.tets, |phys| {
                consistent_element_mass_tet_p1(phys, density)
            });
            (mesh.nodes.len(), k_full, m_full)
        }
        ModalMesh::P2 { nodes, tets } => {
            let k_full = assemble_global_matrix(nodes, tets, |phys| {
                element_stiffness(ElementOrder::P2, &phys[..], material)
            });
            let m_full = assemble_global_matrix(nodes, tets, |phys| {
                consistent_element_mass_tet_p2(phys, density)
            });
            (nodes.len(), k_full, m_full)
        }
    };

    // ---- Matrix-norm diagnostics (‖K‖_F, ‖M‖_F over the full assembly) -----
    // Computed before any free-DOF projection consumes the matrices: these are
    // BC-independent conditioning diagnostics of the discretization itself
    // (surfaced on ModalResult.{stiffness,mass}_matrix_norm).
    let stiffness_matrix_norm = frobenius_norm(&k_full);
    let mass_matrix_norm = frobenius_norm(&m_full);

    ModalAssembly {
        k_full,
        m_full,
        mass_matrix_norm,
        stiffness_matrix_norm,
        n_nodes,
    }
}

/// Eigensolve over a prebuilt [`ModalAssembly`]: project `K`/`M` to the free-DOF
/// subspace, solve `K_free φ = λ M_free φ`, and scatter the mode shapes back to
/// the full DOF space.
///
/// The cheap, BC-/`n_modes`-dependent half of [`solve_modal_core`]: it consumes
/// an assembly that [`assemble_modal_km`] (or the task-κ cache) produced, so the
/// expensive assembly is never redone for a call that only changes the BCs or an
/// eigen knob. `n_nodes` and the `‖K‖_F`/`‖M‖_F` norms are read straight off the
/// assembly and forwarded onto the returned [`ModalCoreResult`].
///
/// Operates in the free-DOF subspace (extracting `K_free` / `M_free` over the
/// non-Dirichlet DOFs) rather than via row elimination, which would inject
/// spurious unit-diagonal eigenpairs (design_decision #3, mirroring
/// `buckling_kernel`). Homogeneous Dirichlet BCs only; `DirichletBc.value` is
/// ignored.
///
/// `reference_direction` is the (unit) direction along which the per-mode
/// effective participation mass `m_eff,i = (φ_iᵀ·M_free·d_free)²` is computed;
/// it is broadcast to every free node's three translational DOFs to form
/// `d_free` (the caller is responsible for supplying a unit vector — see the
/// trampoline). It does not affect the eigensolve, only the participation field.
// G-allow: modal::free_vibration ComputeFn pipeline (task #4066) — generalized eigensolve reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140), which the orphan audit cannot trace. Wired + tested in this file.
pub(crate) fn eigensolve_modal(
    assembly: &ModalAssembly,
    reference_direction: [f64; 3],
    bcs: &[DirichletBc],
    eigen_opts: &EigenSolverOptions,
) -> ModalCoreResult {
    let n_nodes = assembly.n_nodes;
    let n_dofs = 3 * n_nodes;
    // Forward the assembly's BC-independent norms onto the result unchanged.
    let stiffness_matrix_norm = assembly.stiffness_matrix_norm;
    let mass_matrix_norm = assembly.mass_matrix_norm;

    // ---- Free-DOF subspace map (Dirichlet-only; no MPC) -------------------
    let mut is_constrained = vec![false; n_dofs];
    for bc in bcs {
        if bc.dof < n_dofs {
            is_constrained[bc.dof] = true;
        }
    }
    let mut free_of_full = vec![usize::MAX; n_dofs]; // full DOF → free index
    let mut full_of_free: Vec<usize> = Vec::new(); // free index → full DOF
    for (g, &constrained) in is_constrained.iter().enumerate() {
        if !constrained {
            free_of_full[g] = full_of_free.len();
            full_of_free.push(g);
        }
    }
    let n_free = full_of_free.len();

    // ---- Extract free×free submatrices ------------------------------------
    let k_free = project_free(&assembly.k_full, &free_of_full, n_free);
    let m_free = project_free(&assembly.m_full, &free_of_full, n_free);

    // ---- Participation metric  md = M_free · d_free -----------------------
    // d_free broadcasts the reference direction to every free node's three
    // translational DOFs (axis = full DOF index mod 3). Precomputing
    // md = M_free·d_free once lets the per-mode participation factor be a single
    // dot product p_i = φ_iᵀ·M_free·d_free = φ_i·md (M_free symmetric).
    let d_free: Vec<f64> = full_of_free
        .iter()
        .map(|&g| reference_direction[g % 3])
        .collect();
    let md = m_matvec(&m_free, &d_free);

    // ---- Generalized eigensolve  K_free φ = λ M_free φ --------------------
    // A connected 3-D elastic solid has a 6-dimensional rigid-body null space, so
    // K_free is SPD (hence Cholesky-factorable) only once the Dirichlet BCs remove
    // all six rigid-body modes — which needs at least 6 constrained DOFs. Fewer
    // than that leaves K_free singular, and solve_eigen_shift_invert factors K up
    // front (before its own dense fallback), so it would PANIC on such an
    // under-constrained model whenever n_free is large enough to take the
    // shift-invert path (e.g. the production default n_modes = 10 on n_free > 64).
    // Route these cases to the dense generalized solver, which tolerates a
    // singular K_free and lets the W_ModalRigidBodyMode diagnostic surface
    // gracefully regardless of mesh size — matching the small-mesh behaviour the
    // rigid-body diagnostic was designed for (suggestion 1 / robustness).
    const RIGID_BODY_DOFS: usize = 6;
    let under_constrained = n_dofs.saturating_sub(n_free) < RIGID_BODY_DOFS;
    let eig = solve_generalized_eigen(&k_free, &m_free, eigen_opts.clone(), under_constrained);

    // ---- Convert λ→f and scatter φ_free → φ_full --------------------------
    let n_modes_out = eig.eigenvalues.len();
    let mut frequencies = Vec::with_capacity(n_modes_out);
    let mut eigenvalues = Vec::with_capacity(n_modes_out);
    let mut phi_free = Vec::with_capacity(n_modes_out);
    let mut phi_full = Vec::with_capacity(n_modes_out);
    let mut participation_mass = Vec::with_capacity(n_modes_out);
    for i in 0..n_modes_out {
        let lambda = eig.eigenvalues[i];
        eigenvalues.push(lambda);
        frequencies.push(eigenvalue_to_frequency_hz(lambda));

        // Mass-normalize so that φᵀ·M_free·φ = 1 (PRD §7.5): scale the raw
        // eigenvector by 1/√(generalized mass). A degenerate (≤ 0) generalized
        // mass yields a 0.0 scale (the helper's guard) — the mode collapses to
        // zero rather than producing NaN/∞.
        let mut phi_f: Vec<f64> = eig.eigenvectors.col_as_slice(i).to_vec();
        let m_phi = m_matvec(&m_free, &phi_f);
        let generalized_mass: f64 = phi_f.iter().zip(m_phi.iter()).map(|(a, b)| a * b).sum();
        let scale = mass_normalization_scale(generalized_mass);
        for x in &mut phi_f {
            *x *= scale;
        }

        // Effective participation mass along the reference direction (φ now
        // mass-normalized): factor p_i = φ_iᵀ·M_free·d_free = φ_i·md, then
        // m_eff,i = p_i² (PRD §4.1/§4.3). Summed over a complete basis this
        // equals the total translational mass along d (completeness identity).
        let p_i: f64 = phi_f.iter().zip(md.iter()).map(|(a, b)| a * b).sum();
        participation_mass.push(modal_participation_mass(p_i));

        let mut phi_u = vec![0.0_f64; n_dofs];
        for (free_i, &g) in full_of_free.iter().enumerate() {
            phi_u[g] = phi_f[free_i];
        }
        phi_free.push(phi_f);
        phi_full.push(phi_u);
    }

    // ---- Enforce the ascending-frequency contract explicitly --------------
    // stdlib `first_frequency`/`mode_frequency` and the ModalResult contract
    // require modes[0] to be the fundamental. The eigensolver returns eigenpairs
    // ascending by |λ|, which equals ascending-frequency ONLY because λ = ω² ≥ 0
    // for free vibration (K PSD, M PD); a spurious negative-λ eigenpair (clamped
    // to f = 0 by eigenvalue_to_frequency_hz) could otherwise land out of |λ|
    // order and displace the fundamental. A stable sort by frequency is a no-op
    // in the normal case but makes the ordering self-enforcing rather than
    // dependent on the solver invariant (suggestion 3 / architecture).
    let mut order: Vec<usize> = (0..n_modes_out).collect();
    order.sort_by(|&a, &b| {
        frequencies[a]
            .partial_cmp(&frequencies[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if order.iter().enumerate().any(|(i, &src)| i != src) {
        frequencies = permute_by(frequencies, &order);
        eigenvalues = permute_by(eigenvalues, &order);
        participation_mass = permute_by(participation_mass, &order);
        phi_free = permute_by(phi_free, &order);
        phi_full = permute_by(phi_full, &order);
    }
    debug_assert!(
        frequencies.windows(2).all(|w| w[0] <= w[1]),
        "modal frequencies must be sorted ascending after the reorder",
    );

    // ---- Diagnostics (message-based, code: None; design_decision #6) ------
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Rigid-body / spurious near-zero modes: ω ≈ 0 signals an under-constrained
    // model. RIGID_BODY_OMEGA_TOL sits in the wide gap between rigid modes
    // (ω → 0) and the lowest flexible angular frequency of any realistic stiff
    // metal part (≫ 1 rad/s ≈ 0.16 Hz) — see step-9's measured spectrum.
    const RIGID_BODY_OMEGA_TOL: f64 = 1.0; // rad/s
    for (i, &f) in frequencies.iter().enumerate() {
        let omega = 2.0 * PI * f;
        if is_rigid_body_mode(omega, RIGID_BODY_OMEGA_TOL) {
            diagnostics.push(Diagnostic::warning(format!(
                "W_ModalRigidBodyMode: mode {i} has near-zero angular frequency \
                 ω = {omega:.3e} rad/s (≤ {RIGID_BODY_OMEGA_TOL:.1e}); the model \
                 may be under-constrained (rigid-body or spurious mode)."
            )));
        }
    }

    // Convergence shortfall: `eig.converged` is false iff fewer modes were
    // returned than requested (holds for both the dense and shift-invert paths).
    if !eig.converged {
        diagnostics.push(Diagnostic::warning(format!(
            "W_ModalConvergence: eigensolver returned {} of {} requested modes; \
             the result is partial (raise max_iters/tol or lower n_modes).",
            n_modes_out, eigen_opts.n_modes,
        )));
    }

    ModalCoreResult {
        frequencies,
        eigenvalues,
        phi_free,
        phi_full,
        participation_mass,
        m_free,
        n_nodes,
        converged: eig.converged,
        n_converged: eig.n_converged,
        mass_matrix_norm,
        stiffness_matrix_norm,
        diagnostics,
    }
}

/// Core free-vibration FEA eigensolve over a prebuilt [`ModalMesh`]: a thin
/// composition of [`assemble_modal_km`] (assemble `K` + consistent `M` — the
/// expensive, BC-/`n_modes`-independent step the task-κ warm-state cache reuses)
/// and [`eigensolve_modal`] (free-DOF projection + generalized eigensolve +
/// scatter-back).
///
/// Splitting the two lets the trampoline cache the assembled `(K, M)` across
/// calls that differ only in `n_modes`/BCs (task κ); callers with no cache — the
/// unit tests, and any non-caching path — compose them here and get behaviour
/// bit-identical to the pre-split `solve_modal_core`. See [`assemble_modal_km`]
/// for the P1/P2 assembly and [`eigensolve_modal`] for the `reference_direction`
/// / free-DOF-subspace semantics.
///
/// `#[allow(dead_code)]`: since task κ the production trampoline composes
/// [`assemble_modal_km`] + [`eigensolve_modal`] directly (to thread the cache
/// between them), so this convenience wrapper is exercised only by the
/// `modal_ops` unit tests (which assert the composed path stays bit-identical).
#[allow(dead_code)]
// G-allow: modal::free_vibration ComputeFn pipeline (task #4066) — composed assemble+eigensolve wrapper reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140), which the orphan audit cannot trace. Exercised by the modal_ops unit tests.
pub(crate) fn solve_modal_core(
    mesh: ModalMesh<'_>,
    density: f64,
    material: &IsotropicElastic,
    reference_direction: [f64; 3],
    bcs: &[DirichletBc],
    eigen_opts: &EigenSolverOptions,
) -> ModalCoreResult {
    let assembly = assemble_modal_km(mesh, density, material);
    eigensolve_modal(&assembly, reference_direction, bcs, eigen_opts)
}

/// Promote a P1 [`BeamMesh`] to a P2 (10-node) tet mesh by inserting
/// edge-midpoint nodes, returning the promoted `(nodes, tets)`.
///
/// Delegates to the shared `assembly::test_support::promote_tets_to_p2` — the
/// single source of truth for P1→P2 promotion (also driving the kernel-side
/// `tests/modal_benchmarks.rs` accuracy gate and the euler P2 buckling test) — so
/// the eval-side P2 modal path and the kernel-side benchmark promote with
/// identical node numbering. The trampoline calls this once and feeds the
/// promoted `(nodes, tets)` into BOTH the Dirichlet BC realization and
/// [`solve_modal_core`] (as a [`ModalMesh::P2`]), so the BC DOF indices and the
/// assembled `K`/`M` node numbering come from a single shared promotion.
fn promote_beam_mesh_to_p2(mesh: &BeamMesh) -> (Vec<[f64; 3]>, Vec<[usize; 10]>) {
    reify_solver_elastic::assembly::test_support::promote_tets_to_p2(&mesh.nodes, &mesh.tets)
}

/// Assemble one global matrix (`K` or `M`) for an `N`-node tet mesh: build each
/// element matrix via `element_matrix` (gathering the element's `N`
/// physical-node coordinates from the connectivity with `std::array::from_fn`),
/// then scatter through the shared `assemble_global_stiffness`.
///
/// Generic over the element node-count `N` so the P1 (`N = 4`) and P2 (`N = 10`)
/// paths share one assembly loop — called twice per order, once for stiffness and
/// once for the consistent mass. `assemble_global_stiffness` treats each element
/// matrix opaquely, so `K` and `M` scatter through the identical path; the only
/// per-call difference is the `element_matrix` kernel. This collapses the former
/// `assemble_p1_k_m` / `assemble_p2_k_m` (four near-identical assembly blocks)
/// into a single source of truth, so the K and M loops cannot diverge.
fn assemble_global_matrix<const N: usize>(
    nodes: &[[f64; 3]],
    tets: &[[usize; N]],
    element_matrix: impl Fn(&[[f64; 3]; N]) -> ElementStiffness,
) -> SparseRowMat<usize, f64> {
    let elems: Vec<ElementStiffness> = tets
        .iter()
        .map(|tet| {
            let phys: [[f64; 3]; N] = std::array::from_fn(|i| nodes[tet[i]]);
            element_matrix(&phys)
        })
        .collect();
    let assembly: Vec<AssemblyElement<'_>> = tets
        .iter()
        .zip(elems.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement {
            id,
            connectivity: conn,
            k_e,
        })
        .collect();
    assemble_global_stiffness(nodes.len(), &assembly, AssemblyMode::Deterministic)
}

/// Extract the free×free submatrix of `full` over the non-Dirichlet DOFs.
///
/// `free_of_full[g]` maps full DOF `g` to its free-subspace index, or
/// `usize::MAX` if `g` is constrained. This is the Dirichlet-only specialization
/// of `buckling_kernel`'s `project_with_expansion`: every free DOF expands to
/// itself with weight 1.0 and every constrained DOF to nothing. `faer`'s
/// `try_new_from_triplets` sums duplicate triplets, preserving CSR invariants.
fn project_free(
    full: &SparseRowMat<usize, f64>,
    free_of_full: &[usize],
    n_free: usize,
) -> SparseRowMat<usize, f64> {
    let sym = full.symbolic();
    let mut trips: Vec<Triplet<usize, usize, f64>> = Vec::new();
    for g_row in 0..full.nrows() {
        let r = free_of_full[g_row];
        if r == usize::MAX {
            continue;
        }
        let cols = sym.col_idx_of_row_raw(g_row);
        let vals = full.val_of_row(g_row);
        for (col_raw, &val) in cols.iter().zip(vals.iter()) {
            let c = free_of_full[*col_raw];
            if c == usize::MAX || val == 0.0 {
                continue;
            }
            trips.push(Triplet::new(r, c, val));
        }
    }
    SparseRowMat::try_new_from_triplets(n_free, n_free, &trips)
        .expect("free-DOF submatrix construction must not violate CSR invariants")
}

/// Sparse matvec `M · v` over the free×free mass matrix (CSR row dot products).
///
/// The reusable mass-metric primitive: the generalized mass `φᵀMφ` (step 6
/// normalization) and the participation factor `φᵀMd` (step 8) are both
/// `dot(·, M··)`.
fn m_matvec(m: &SparseRowMat<usize, f64>, v: &[f64]) -> Vec<f64> {
    let sym = m.symbolic();
    let mut out = vec![0.0_f64; m.nrows()];
    for (r, out_r) in out.iter_mut().enumerate() {
        let cols = sym.col_idx_of_row_raw(r);
        let vals = m.val_of_row(r);
        let mut acc = 0.0_f64;
        for (col_raw, &val) in cols.iter().zip(vals.iter()) {
            acc += val * v[*col_raw];
        }
        *out_r = acc;
    }
    out
}

/// Frobenius norm `‖A‖_F = √(Σ_ij a_ij²)` of a sparse matrix (sum of squares
/// over the stored nonzeros). Feeds the `ModalResult.{mass,stiffness}_matrix_norm`
/// conditioning diagnostics. Explicit zeros stored in the CSR contribute 0, so
/// the result is independent of structural-zero bookkeeping.
fn frobenius_norm(a: &SparseRowMat<usize, f64>) -> f64 {
    let mut sum_sq = 0.0_f64;
    for r in 0..a.nrows() {
        for &val in a.val_of_row(r) {
            sum_sq += val * val;
        }
    }
    sum_sq.sqrt()
}

/// Count the stored nonzeros of a CSR matrix (sum of per-row stored entries).
/// Used to size the donated warm state (`ModalAnalysisCache::estimated_size_bytes`)
/// for pool budgeting + `cost_per_byte`. Uses the same `val_of_row` row walk as
/// [`frobenius_norm`], so it counts exactly the entries the cache retains.
fn csr_nnz(a: &SparseRowMat<usize, f64>) -> usize {
    (0..a.nrows()).map(|r| a.val_of_row(r).len()).sum()
}

/// Reorder `items` so that result position `i` holds the original
/// `items[order[i]]`, moving elements out (no deep clone) via `std::mem::take`.
/// `order` must be a permutation of `0..items.len()` (each index used exactly
/// once) — guaranteed by the sort that produces it — so no element is taken
/// twice. Applies the ascending-frequency sort across `solve_modal_core`'s
/// parallel per-mode arrays in lockstep.
fn permute_by<T: Default>(mut items: Vec<T>, order: &[usize]) -> Vec<T> {
    order
        .iter()
        .map(|&i| std::mem::take(&mut items[i]))
        .collect()
}

/// Solve the generalized symmetric eigenproblem `K_free φ = λ M_free φ`,
/// returning eigenvalues ascending by |λ| with column-major eigenvectors.
///
/// Dispatches to the dense path directly in the small regime instead of always
/// going through [`solve_eigen_shift_invert`], which unconditionally
/// Cholesky-factors `K` up front and would panic on a singular / near-singular
/// `K_free` (e.g. an unconstrained fixture's rigid-body modes). The dense-regime
/// predicate `n ≤ max(64, 2·n_modes)` mirrors the wrapper's own internal
/// dense-fallback threshold, so the numerical path is identical to what the
/// wrapper would pick — minus the premature factorization. Larger constrained
/// problems (`K_free` SPD after BCs) take the shift-invert Lanczos path
/// (design_decision #4).
///
/// `force_dense` overrides the size heuristic to take the dense path regardless
/// of `n`. The caller sets it when the model is detected as under-constrained
/// (too few Dirichlet DOFs to remove the rigid-body null space), so a singular
/// `K_free` never reaches `solve_eigen_shift_invert`'s up-front Cholesky and
/// panics. NOTE: the caller's detector (constrained-DOF count) is a *necessary*
/// condition for SPD-ness, not a sufficient one — a pathological
/// ≥6-but-rank-deficient constraint set on a mesh large enough to take the
/// shift-invert path could still reach the panicking factorization. Closing that
/// residual edge would need an explicit SPD probe (a throwaway Cholesky attempt
/// with graceful fallback) and is deferred as a follow-up; the common
/// no/insufficient-supports user error is handled here.
fn solve_generalized_eigen(
    k_free: &SparseRowMat<usize, f64>,
    m_free: &SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
    force_dense: bool,
) -> EigenSolverResult {
    let n = k_free.nrows();
    if force_dense || n <= 64_usize.max(2 * opts.n_modes) {
        solve_eigen_dense(k_free, m_free, opts)
    } else {
        solve_eigen_shift_invert(k_free, m_free, opts)
    }
}

// ---------------------------------------------------------------------------
// Trampoline density guard (E_ModalNoMassMatrix)
// ---------------------------------------------------------------------------

/// Extract the material's mass density (kg/m³) for the consistent mass matrix,
/// or short-circuit to a degenerate result.
///
/// The trampoline's first guard. The consistent mass matrix is
/// `M = ∫ ρ NᵀN dV` — it cannot be assembled without a positive mass density,
/// and the generalized eigenproblem `K φ = λ M φ` is undefined with no `M`. So a
/// material that carries no usable `density` (field missing, not a scalar, or
/// ≤ 0) must NOT reach mesh assembly / eigensolve.
///
/// Returns `Ok(density)` for a positive `density` scalar (expected dimension
/// `MASS_DENSITY`; read in SI = kg/m³). Otherwise returns `Err(outcome)`, where
/// `outcome` is a [`ComputeOutcome::Completed`] carrying an `E_ModalNoMassMatrix`
/// `Error` diagnostic and a degenerate empty-modes `ModalResult` — the
/// trampoline forwards this verbatim (step 14). Message-based diagnostic
/// (`code: None`) per design_decision #6.
///
/// The dimension tag is intentionally NOT asserted here (the guard predicate is
/// "missing or ≤ 0", mirroring buckling's permissive `Scalar { si_value, .. }`
/// material reads in `compute_targets::buckling::extract_material`): a
/// wrong-dimension density is an upstream type-checker concern, not a runtime
/// modal one. `NaN` fails `> 0.0` and is therefore rejected as well.
///
/// `clippy::result_large_err` is allowed: the `Err` carries a [`ComputeOutcome`]
/// that the trampoline returns by value and consumes immediately (the whole
/// compute contract traffics in by-value `ComputeOutcome`), so boxing this
/// transient guard result would add an allocation for no benefit. `dead_code`
/// until the step-14 trampoline consumes it; until then only the step-11 unit
/// test calls it.
#[allow(clippy::result_large_err)]
fn extract_density_or_degenerate(material: &Value) -> Result<f64, ComputeOutcome> {
    // Marshal the material density into Option<f64>, keeping the existing
    // Scalar-only + positivity predicate (modal's own "no usable density" guard;
    // `> 0.0` also rejects NaN). Then delegate to the shared rung-walk so the
    // explicit→material ladder is defined in exactly one place (task 4470).
    let material_density = if let Value::StructureInstance(data) = material
        && let Some(Value::Scalar { si_value, .. }) = data.fields.get("density")
        && *si_value > 0.0
    {
        Some(*si_value)
    } else {
        None
    };
    // modal has no explicit density arg; only the material rung is in play
    match resolve_density_strict(None, material_density) {
        Some((rho, _)) => Ok(rho),
        None => Err(no_mass_matrix_outcome()),
    }
}

/// Build the degenerate short-circuit outcome for a missing / non-positive mass
/// density: an `E_ModalNoMassMatrix` `Error` diagnostic plus an empty-modes
/// `ModalResult` (no eigenproblem was solved).
fn no_mass_matrix_outcome() -> ComputeOutcome {
    let diagnostic = Diagnostic::error(
        "E_ModalNoMassMatrix: the material carries no positive mass density \
         (`density` missing or ≤ 0), so the consistent mass matrix M cannot be \
         assembled and the free-vibration eigenproblem Kφ = λMφ is undefined; \
         returning an empty modal result.",
    );
    ComputeOutcome::Completed {
        result: degenerate_modal_result(),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![diagnostic],
    }
}

/// Opaque `Part` placeholder — a zero-field `StructureInstance` whose
/// `type_name` is `"Part"`.  All four production echo sites emit this value
/// for the `part` field until the Part registry is wired (task 4578).
/// `StructureTypeId(u32::MAX)` is the registry-free sentinel, mirroring the
/// other degenerate builders in this file.
fn placeholder_part() -> Value {
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "Part".to_string(),
        version: 1,
        fields: PersistentMap::default(),
    }))
}

/// Build a degenerate `ModalResult` `Value::StructureInstance`: an empty `modes`
/// list and zeroed matrix norms — the result returned when the modal solve is
/// short-circuited (no mass matrix). Shaped to the α structure-def (6 fields,
/// `modal_analysis.ri`); the trait-typed `damping` field is left `Value::Undef`
/// (the tet-result convention for unpopulated fields, cf. buckling's
/// `pre_stress`), and `StructureTypeId(u32::MAX)` is the registry-free sentinel.
fn degenerate_modal_result() -> Value {
    let fields: PersistentMap<String, Value> = [
        ("part".to_string(), placeholder_part()),
        ("modes".to_string(), Value::List(Vec::new())),
        ("boundary_conditions".to_string(), Value::List(Vec::new())),
        ("damping".to_string(), Value::Undef),
        ("mass_matrix_norm".to_string(), Value::Real(0.0)),
        ("stiffness_matrix_norm".to_string(), Value::Real(0.0)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ModalResult".to_string(),
        version: 1,
        fields,
    }))
}

// ---------------------------------------------------------------------------
// Trampoline (modal::free_vibration)
// ---------------------------------------------------------------------------

/// Warm-state payload donated by the modal trampoline (task κ): the cache key
/// plus the expensive assembled `(K, M)` it certifies. Recovered on the next
/// invocation via `downcast_ref` and reused only when the incoming request's
/// [`ModalCacheKey`] matches — i.e. the cached assembly is still valid for the
/// new (geometry, material, element_order).
#[derive(Clone)]
pub(crate) struct ModalAnalysisCache {
    /// The `(K, M)`-determining inputs the cached `assembly` was built for.
    pub(crate) key: ModalCacheKey,
    /// The assembled stiffness/mass (+ norms, node count) to amortize.
    pub(crate) assembly: ModalAssembly,
}

impl ModalAnalysisCache {
    /// Estimated retained size of this cache in bytes: the CSR payload of the
    /// assembled `K` and `M` (one `usize` column index + one `f64` value per
    /// stored nonzero) plus the flat `ModalCacheKey`. Drives both the
    /// [`OpaqueState`] size hint (pool LRU budgeting) and the donated
    /// `cost_per_byte` — a bigger cached `(K, M)` is pricier to retain. Always
    /// ≥ `size_of::<ModalCacheKey>() > 0`, so the `cost_per_byte` reciprocal is
    /// well-defined for any real assembly.
    fn estimated_size_bytes(&self) -> usize {
        let per_nz = std::mem::size_of::<usize>() + std::mem::size_of::<f64>();
        let nnz = csr_nnz(&self.assembly.k_full) + csr_nnz(&self.assembly.m_full);
        nnz * per_nz + std::mem::size_of::<ModalCacheKey>()
    }

    /// Wrap this cache in an [`OpaqueState`] for donation to the warm-state
    /// pool, sized by [`estimated_size_bytes`](Self::estimated_size_bytes) so
    /// the pool's LRU budget accounts for the assembled `(K, M)` it holds.
    ///
    /// Returns that `size_bytes` alongside the state so the caller can derive
    /// `cost_per_byte` from the same measurement — the CSR payload is walked
    /// exactly once per donation instead of again inside this method.
    fn into_opaque_state(self) -> (OpaqueState, usize) {
        let size = self.estimated_size_bytes();
        (OpaqueState::new(self, size), size)
    }
}

/// Result of the in-crate modal core [`run_modal_analysis`]: the engine-facing
/// [`ComputeOutcome`] plus a white-box `reused_assembly` flag the unit tests
/// assert cache amortization against (the public `ComputeFn` returns only the
/// outcome).
pub(crate) struct ModalTrampolineRun {
    /// The compute outcome the public trampoline returns.
    pub(crate) outcome: ComputeOutcome,
    /// `true` iff this run reused a cached [`ModalAnalysisCache`] assembly rather
    /// than assembling `(K, M)` fresh. Observable only in-crate (amortization
    /// tests); the public `ComputeFn` discards it, hence `allow(dead_code)`.
    #[allow(dead_code)]
    pub(crate) reused_assembly: bool,
}

/// In-crate modal core behind [`solve_modal_analysis_trampoline`], adding the
/// task-κ warm-state cache — reuse the assembled `(K, M)` across calls whose
/// [`ModalCacheKey`] matches — on top of the assemble → eigensolve → shape
/// pipeline. Returns a [`ModalTrampolineRun`] so in-crate tests can also observe
/// whether the assembly was reused; the public trampoline takes only `.outcome`.
///
/// `@optimized("modal::free_vibration")` core for `fn modal_analysis`
/// (task ζ). Receives the five flat `value_inputs` matching the fn signature:
///
/// ```text
/// [0] material : ElasticMaterial  (StructureInstance — youngs_modulus, poisson_ratio, density)
/// [1] length   : Length           (Scalar { LENGTH })
/// [2] width    : Length           (Scalar { LENGTH })
/// [3] height   : Length           (Scalar { LENGTH })
/// [4] options  : ModalOptions     (StructureInstance — n_modes/tol/max_iters/sigma/
///                                  damping/reference_direction/boundary_conditions)
/// ```
///
/// Reconstructs the beam mesh from length/width/height (no Part→trampoline
/// geometry channel — the same deviation `solve_buckling` documents,
/// design_decision #1), realizes the Dirichlet BCs from the `boundary_conditions`
/// faces, runs [`solve_modal_core`], and shapes a `ModalResult`
/// `Value::StructureInstance` (6 fields, α struct-def; `StructureTypeId(u32::MAX)`
/// sentinel). Each mode is a `Mode` StructureInstance `{ frequency: Scalar<Frequency>(Hz),
/// shape: List<Vector3<Dimensionless>>, participation_mass: Real, damping_ratio: Real }`,
/// where `damping_ratio` is the Rayleigh ratio `ζ_i = (α + β·ω_i²)/(2·ω_i)` (0
/// for `NoDamping`). `Mode.shape` is the mass-normalized eigenvector reshaped
/// from `phi_full` (length `3·n_nodes`) into `n_nodes` per-node `Vector3`,
/// `(0,0,0)` at every Dirichlet-constrained node.
///
/// A material with no positive `density` short-circuits to a degenerate
/// empty-modes result plus an `E_ModalNoMassMatrix` Error (the
/// [`extract_density_or_degenerate`] guard) — no mesh / eigensolve runs.
// G-allow: modal::free_vibration ComputeFn entry point (task #4066) — reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140), which the orphan audit cannot trace. Wired + tested in this file.
pub(crate) fn run_modal_analysis(
    value_inputs: &[Value],
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ModalTrampolineRun {
    // ── (0) cancellation checkpoint: on entry, before any mesh/assembly work ──
    // Coarse cooperative cancellation (CN-contract §2 / PRD §6): poll at the two
    // natural checkpoints — here on entry, and again after assembly/recovery just
    // before the eigensolve (the costly step). Finer per-Lanczos-restart
    // granularity would need a cancellation hook on reify-solver-elastic's
    // `solve_eigen_shift_invert` (3-arg, no callback) — out of scope, owned by the
    // buckling-eigensolver PRD; coarse polling satisfies CN-contract §2 / PRD §6.
    if cancellation.is_cancelled() {
        return ModalTrampolineRun {
            outcome: ComputeOutcome::Cancelled,
            reused_assembly: false,
        };
    }

    // ── (1) density guard — no M without a positive density (short-circuit) ──
    // The guard's degenerate outcome already carries new_warm_state = None, so a
    // missing density neither reuses nor donates a cache (reused_assembly = false).
    let density = match extract_density_or_degenerate(&value_inputs[0]) {
        Ok(d) => d,
        Err(outcome) => {
            return ModalTrampolineRun {
                outcome,
                reused_assembly: false,
            };
        }
    };

    // ── (2) material elastic constants (E, ν) ────────────────────────────────
    let material = extract_isotropic_material(&value_inputs[0]);

    // ── (3) geometry scalars (SI metres) ─────────────────────────────────────
    let length = read_scalar_si(&value_inputs[1]);
    let width = read_scalar_si(&value_inputs[2]);
    let height = read_scalar_si(&value_inputs[3]);
    // Build the beam mesh once and share it between the BC realization (4) and
    // the eigensolve (5); both index DOFs against the same node numbering.
    let mesh = build_beam_mesh(length, width, height);

    // ── (4) ModalOptions: eigen knobs, excitation direction, damping, BCs ────
    let options = &value_inputs[4];
    let (n_modes, tol, max_iters, sigma) = extract_eigen_knobs(options);
    let reference_direction = extract_reference_direction(options);
    let (alpha, beta) = extract_damping(options);
    let element_order = extract_element_order(options);
    // Map the order to the cache-key discriminant from the SAME source that picks
    // the ModalMesh below, so the key and the assembled (K, M) can never disagree
    // (task 4066: P1 and P2 assemble distinct matrices and node counts).
    let element_order_disc: u8 = match element_order {
        ElementOrder::P1 => 0,
        ElementOrder::P2 => 1,
    };
    // P2 promotes the beam mesh to 10-node tets ONCE here; the promoted node set
    // then drives BOTH the Dirichlet BC realization AND the K/M assembly in
    // `solve_modal_core` (handed across as `ModalMesh::P2`). P2 promotion inserts
    // edge-midpoint nodes, so the face-coordinate BC selection must run over the
    // PROMOTED nodes — otherwise a clamped/pinned face would miss its midpoint
    // nodes and be only partially constrained. Promoting once (instead of once for
    // BCs and again inside the core solve) removes the duplicated O(elements) walk
    // and the risk of the two promotions drifting. P1 borrows the original mesh.
    let promoted_p2 = match element_order {
        ElementOrder::P1 => None,
        ElementOrder::P2 => Some(promote_beam_mesh_to_p2(&mesh)),
    };
    let modal_mesh = match &promoted_p2 {
        None => ModalMesh::P1(&mesh),
        Some((nodes, tets)) => ModalMesh::P2 { nodes, tets },
    };
    // BC selection reads only node coordinates, so it takes the order-correct node
    // slice directly (no half-populated `BeamMesh` sentinel): the P1 mesh nodes or
    // the promoted P2 node set, whichever `modal_mesh` carries.
    let bcs = build_dirichlet_bcs(options, modal_mesh.nodes(), length, width, height);
    let eigen_opts = EigenSolverOptions {
        n_modes,
        tol,
        max_iters,
        sigma,
    };

    // ── (5) cache lookup: reuse the assembled (K, M) on a key HIT ────────────
    // The key captures EXACTLY the (K, M)-determining inputs (geometry + material
    // + element_order); n_modes / tol / sigma / max_iters / boundary_conditions /
    // damping / reference_direction are excluded, so a call differing only in
    // those HITs. On a miss (or no prior) assemble fresh. The cheap mesh + BCs
    // above are rebuilt either way — a HIT still needs them to realize the
    // Dirichlet BCs by coordinate; only the expensive (K, M) assembly is reused.
    let key = ModalCacheKey::new(
        length,
        width,
        height,
        material.youngs_modulus,
        material.poisson_ratio,
        density,
        element_order_disc,
    );
    // Borrow the prior cache first, then clone the assembled (K, M) ONLY on a
    // confirmed key HIT. A deep clone copies both faer matrices (Vec-backed full
    // copies); doing it unconditionally — before the `matches` check — would waste
    // that work on a MISS (geometry/material/element_order changed), where the
    // clone is immediately discarded and we re-assemble anyway.
    let prior_cache = prior_warm_state.and_then(|s| s.downcast_ref::<ModalAnalysisCache>());
    let (assembly, reused_assembly) = match prior_cache {
        Some(cache) if cache.key.matches(&key) => (cache.assembly.clone(), true),
        _ => (assemble_modal_km(modal_mesh, density, &material), false),
    };

    // Cancellation checkpoint: after assembly/recovery, before the costly
    // eigensolve. A cancel observed here drops the (possibly freshly-assembled)
    // matrices without donating them; run_compute_dispatch restores the prior
    // warm state on a Cancelled outcome (so reused_assembly is reported false).
    if cancellation.is_cancelled() {
        return ModalTrampolineRun {
            outcome: ComputeOutcome::Cancelled,
            reused_assembly: false,
        };
    }

    // Free-DOF eigensolve over the reused-or-fresh assembly (the cheap half).
    let core = eigensolve_modal(&assembly, reference_direction, &bcs, &eigen_opts);

    // ── (6) modes list: one Mode StructureInstance per returned mode ─────────
    // phi_full and frequencies are pushed in lockstep by solve_modal_core; assert
    // the invariant in debug builds so a future upstream change trips loudly.
    debug_assert_eq!(
        core.phi_full.len(),
        core.frequencies.len(),
        "phi_full and frequencies must have equal length (got {} vs {})",
        core.phi_full.len(),
        core.frequencies.len()
    );
    let modes_list: Vec<Value> = core
        .frequencies
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            let omega = 2.0 * PI * f;
            let damping_ratio = rayleigh_damping_ratio(alpha, beta, omega);
            let participation_mass = core.participation_mass.get(i).copied().unwrap_or(0.0);
            let fields: PersistentMap<String, Value> = [
                // `Mode.frequency : Frequency` (modal_analysis.ri, task 4548) —
                // emit a dimensioned Scalar, not a bare Real placeholder.
                (
                    "frequency".to_string(),
                    Value::Scalar {
                        si_value: f,
                        dimension: DimensionVector::FREQUENCY,
                    },
                ),
                (
                    "shape".to_string(),
                    core.phi_full
                        .get(i)
                        .map(|p| mode_shape_value(p))
                        .unwrap_or(Value::Undef),
                ),
                (
                    "participation_mass".to_string(),
                    Value::Real(participation_mass),
                ),
                ("damping_ratio".to_string(), Value::Real(damping_ratio)),
            ]
            .into_iter()
            .collect();
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_id: StructureTypeId(u32::MAX),
                type_name: "Mode".to_string(),
                version: 1,
                fields,
            }))
        })
        .collect();

    // ── (7) ModalResult: echo the input BCs + damping, report matrix norms ───
    let boundary_conditions = field_or(options, "boundary_conditions", Value::List(Vec::new()));
    let damping = field_or(options, "damping", Value::Undef);
    let result_fields: PersistentMap<String, Value> = [
        ("part".to_string(), placeholder_part()),
        ("modes".to_string(), Value::List(modes_list)),
        ("boundary_conditions".to_string(), boundary_conditions),
        ("damping".to_string(), damping),
        (
            "mass_matrix_norm".to_string(),
            Value::Real(core.mass_matrix_norm),
        ),
        (
            "stiffness_matrix_norm".to_string(),
            Value::Real(core.stiffness_matrix_norm),
        ),
    ]
    .into_iter()
    .collect();
    let result = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ModalResult".to_string(),
        version: 1,
        fields: result_fields,
    }));

    // ── (8) donate the assembled (K, M) as warm state (task κ) ───────────────
    // `run_compute_dispatch` donates `new_warm_state` to the Compute node on a
    // Completed outcome (and restores the prior on Cancelled/Failed). `key` is a
    // `Copy` ModalCacheKey, so reusing it from the (5) match guard is fine.
    // `cost_per_byte` is the reciprocal of the cache's estimated byte size — a
    // bigger cached (K, M) is pricier to retain in the warm-state pool (mirrors
    // elastic_static.rs). `into_opaque_state` walks the CSR payload once and hands
    // back that `size_bytes`, so `cost_per_byte` reuses the single measurement
    // rather than recomputing it. `size_bytes` always includes the flat key (> 0),
    // so the `None` branch is unreachable for a real assembly but kept for parity.
    let cache = ModalAnalysisCache { key, assembly };
    let (state, size_bytes) = cache.into_opaque_state();
    let cost_per_byte = if size_bytes > 0 {
        Some(1.0 / size_bytes as f64)
    } else {
        None
    };
    let new_warm_state = Some(state);
    let outcome = ComputeOutcome::Completed {
        result,
        new_warm_state,
        cost_per_byte,
        diagnostics: core.diagnostics,
    };
    ModalTrampolineRun {
        outcome,
        reused_assembly,
    }
}

/// `@optimized("modal::free_vibration")` public `ComputeFn` for `fn
/// modal_analysis` (task ζ; registered in `compute_targets::mod`). A thin
/// wrapper over the in-crate core [`run_modal_analysis`]: it forwards the prior
/// warm state and the cancellation handle and surfaces only the
/// [`ComputeOutcome`]. Warm-state donation/recovery (the assembled `(K, M)`
/// cache) and cooperative cancellation live in the core (task κ); the core's
/// white-box `reused_assembly` flag is for in-crate amortization tests only.
pub fn solve_modal_analysis_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    run_modal_analysis(value_inputs, prior_warm_state, cancellation).outcome
}

// ---------------------------------------------------------------------------
// Mechanism-modal bridge (modal::mechanism_modal) — task 4271
// ---------------------------------------------------------------------------
//
// Lumped generalized-coordinate eigensolve: one generalized DOF per spanning-tree
// body.  Diagonal M[i,i] = body_i scalar mass; diagonal K[i,i] = body_i's inbound
// joint spring_rate (0 for rigid joints).  K is assembled via `add_joint_stiffness`
// over a zero base (the same primitive the kernel test joint_stiffness_modal_frequency.rs
// uses).  First-mode λ₀ = k/m (closed-form exact for a diagonal system), returned as
// f = √λ₀/(2π) Hz via `eigenvalue_to_frequency_hz`.
//
// When the assembled DOF count n_dof < 2, `solve_eigen_dense` (faer dense QZ) requires
// n ≥ 2 for its scratch-buffer allocation.  We pad one stiff anchor DOF:
//   λ_anchor = max(physical_λ_max, 1.0) · 1e8
//   K[n_dof, n_dof] = λ_anchor   M[n_dof, n_dof] = 1.0
// The anchor is always the highest mode; physical modes = eigenvalues[0..n_dof].
// Mirrors the anchor trick in joint_stiffness_modal_frequency.rs §10.1.

/// Extract a finite SI f64 from a `Value::Scalar` or `Value::Option(Some(Scalar))`.
/// Replicates the `scalar_si` convention from `reify_stdlib::flexures::common`
/// (which is `pub(super)` and not accessible here).
fn scalar_si_value(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, .. } if si_value.is_finite() => Some(*si_value),
        Value::Option(Some(inner)) => scalar_si_value(inner),
        _ => None,
    }
}

/// Assemble the diagonal stiffness K and mass M for a lumped
/// generalized-coordinate mechanism model.
///
/// Returns `Some((K, M, n_dof))` where:
/// - `K` is an `n_dof × n_dof` diagonal `SparseRowMat` with `K[i,i] =` the
///   spring_rate of body `i`'s inbound `at` joint (0 for rigid joints).
/// - `M` is an `n_dof × n_dof` diagonal `SparseRowMat` with `M[i,i] =` body
///   `i`'s scalar mass extracted via the canonical two-step
///   `resolve_body_mass` → `mass_properties_from_value`.
/// - `n_dof` = number of spanning-tree bodies (bodies.len() − loop_closures.len()).
///
/// Returns `None` when:
/// - `mechanism` is not a `Value::Map` with a "bodies" list, or
/// - `n_dof == 0` (no spanning-tree bodies), or
/// - any body's mass is unresolvable (short-circuits the whole assembly).
///
/// Mirrors `assemble_modal_km` for the FEA-beam path (step (3) of run_modal_analysis)
/// but uses the lumped DOF model instead of the 3·n_nodes FEA model.
type MechanismKm = (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>, usize);
fn assemble_mechanism_km(
    mechanism: &Value,
) -> Option<MechanismKm> {
    let mech_map = match mechanism {
        Value::Map(m) => m,
        _ => return None,
    };
    let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => b,
        _ => return None,
    };
    let n_loop = match mech_map.get(&Value::String("loop_closures".to_string())) {
        Some(Value::List(l)) => l.len(),
        _ => 0,
    };
    let n_dof = bodies.len().saturating_sub(n_loop);
    if n_dof == 0 {
        return None;
    }
    let mut mass_trips: Vec<Triplet<usize, usize, f64>> = Vec::new();
    let mut contributions: Vec<JointStiffness> = Vec::new();
    for (i, body) in bodies[..n_dof].iter().enumerate() {
        // Canonical two-step mass read-path (task constraint).
        let mp = resolve_body_mass(body)?;
        let (mass, _, _) = mass_properties_from_value(&mp)?;
        mass_trips.push(Triplet::new(i, i, mass));
        // Spring_rate from the inbound `at` joint (flexure) or absent (rigid).
        if let Value::Map(bm) = body
            && let Some(Value::Map(jm)) = bm.get(&Value::String("at".to_string()))
            && let Some(sr) = jm.get(&Value::String("spring_rate".to_string()))
            && let Some(k) = scalar_si_value(sr)
            && k.is_finite()
        {
            contributions.push(JointStiffness { dof: i, stiffness: k });
        }
    }
    let m_mat = SparseRowMat::try_new_from_triplets(n_dof, n_dof, &mass_trips)
        .expect("mechanism mass-matrix triplet build must succeed");
    // Zero base K: add_joint_stiffness over an empty (0-entry) sparse matrix.
    let k_zero = SparseRowMat::try_new_from_triplets(n_dof, n_dof, &[])
        .expect("zero stiffness-matrix build must succeed");
    let k_mat = add_joint_stiffness(&k_zero, &contributions);
    Some((k_mat, m_mat, n_dof))
}

/// Extract the value stored at diagonal position `i` from a sparse CSR matrix.
/// Returns `0.0` when the entry is structurally absent (rigid joints, etc.).
/// Used by [`run_mechanism_modal`] for the anchor-pad λ_max estimate.
fn get_sparse_diag(mat: &SparseRowMat<usize, f64>, i: usize) -> f64 {
    let sym = mat.symbolic();
    let cols = sym.col_idx_of_row_raw(i);
    let vals = mat.val_of_row(i);
    for (col_raw, &val) in cols.iter().zip(vals.iter()) {
        if *col_raw == i {
            return val;
        }
    }
    0.0
}

/// Core implementation for the `modal::mechanism_modal` compute target (task
/// #4271).
///
/// Builds a lumped generalized-coordinate eigenproblem from the assembled
/// `(K, M)` (via [`assemble_mechanism_km`]), solves it with
/// [`solve_eigen_dense`], and shapes the result as a `ModalResult`
/// `Value::StructureInstance` with frequency-only `Mode` records.
///
/// DOF model: one generalized DOF per spanning-tree body.  Diagonal M[i,i] =
/// body scalar mass; diagonal K[i,i] = body inbound joint spring_rate (0 for
/// rigid joints).  First-mode λ₀ = k/m (closed-form exact for diagonal
/// system) → f = √λ₀/(2π) Hz via [`eigenvalue_to_frequency_hz`].
///
/// Returns a degenerate empty-modes `ModalResult` with an `Error` diagnostic
/// when the mechanism has no spanning-tree bodies or a body mass is
/// unresolvable.
fn run_mechanism_modal(
    value_inputs: &[Value],
    _prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // ── (0) cancellation checkpoint on entry ──────────────────────────────────
    if cancellation.is_cancelled() {
        return ComputeOutcome::Cancelled;
    }

    // ── (1) M/K assembly guard — missing / unresolvable mass → degenerate ────
    let mechanism = value_inputs.first().unwrap_or(&Value::Undef);
    let (k_mat, m_mat, n_dof) = match assemble_mechanism_km(mechanism) {
        Some(km) => km,
        None => {
            let diag = Diagnostic::error(
                "E_MechanismModalNoMass: the mechanism has no spanning-tree bodies \
                 or a body's mass is unresolvable; the lumped generalized-coordinate \
                 eigenproblem Kφ = λMφ is undefined — returning an empty modal result.",
            );
            return ComputeOutcome::Completed {
                result: degenerate_modal_result(),
                new_warm_state: None,
                cost_per_byte: None,
                diagnostics: vec![diag],
            };
        }
    };

    // ── (2) anchor-pad when n_dof < 2 (faer dense QZ requires n ≥ 2) ─────────
    // Mirrors joint_stiffness_modal_frequency.rs §10.1: append one stiff
    // anchor DOF so it is always the highest mode and the physical mode stays
    // modes[0] after the ascending eigenvalue sort.
    // λ_anchor = max(physical_λ, 1.0) · 1e8 — scales 1e8 above the physical
    // eigenvalue for ANY user k/m ratio, including zero-spring rigid joints.
    let (k_solve, m_solve, padded_size) = if n_dof < 2 {
        let k0 = get_sparse_diag(&k_mat, 0);
        let m0 = get_sparse_diag(&m_mat, 0);
        let physical_lambda = if m0 > 0.0 { k0 / m0 } else { 0.0 };
        let lambda_anchor = physical_lambda.max(1.0) * 1e8;
        let n_pad = n_dof + 1; // = 2 when n_dof = 1

        // Build padded K and M from the physical diagonal entries + anchor.
        let mut k_trips: Vec<Triplet<usize, usize, f64>> = Vec::new();
        for i in 0..n_dof {
            let v = get_sparse_diag(&k_mat, i);
            if v != 0.0 {
                k_trips.push(Triplet::new(i, i, v));
            }
        }
        k_trips.push(Triplet::new(n_dof, n_dof, lambda_anchor));

        let mut m_trips: Vec<Triplet<usize, usize, f64>> = Vec::new();
        for i in 0..n_dof {
            let v = get_sparse_diag(&m_mat, i);
            if v != 0.0 {
                m_trips.push(Triplet::new(i, i, v));
            }
        }
        m_trips.push(Triplet::new(n_dof, n_dof, 1.0));

        let k_pad = SparseRowMat::try_new_from_triplets(n_pad, n_pad, &k_trips)
            .expect("anchor-padded K build must succeed");
        let m_pad = SparseRowMat::try_new_from_triplets(n_pad, n_pad, &m_trips)
            .expect("anchor-padded M build must succeed");
        (k_pad, m_pad, n_pad)
    } else {
        let sz = n_dof;
        (k_mat, m_mat, sz)
    };

    // ── (3) dense QZ eigensolve ───────────────────────────────────────────────
    // solve_eigen_dense returns eigenvalues ascending by |λ|; physical modes
    // are eigenvalues[0..n_dof].  The lumped model is always small (n_dof =
    // number of bodies), so the dense path is always correct here.
    let eigen_opts = EigenSolverOptions { n_modes: padded_size, ..Default::default() };
    let eigen_result = solve_eigen_dense(&k_solve, &m_solve, eigen_opts);

    // ── (4) convert physical eigenvalues [0..n_dof] to frequencies (Hz) ──────
    let n_physical = n_dof.min(eigen_result.eigenvalues.len());
    let frequencies: Vec<f64> = eigen_result.eigenvalues[..n_physical]
        .iter()
        .map(|&lambda| eigenvalue_to_frequency_hz(lambda))
        .collect();

    // ── (5) shape Mode records (frequency-only; lumped model has no 3D shape) ─
    // The stdlib accessors first_frequency/mode_frequency read only
    // Mode.frequency, so frequency-only modes fully satisfy the contract.
    let modes_list: Vec<Value> = frequencies
        .iter()
        .map(|&f| {
            let fields: PersistentMap<String, Value> = [
                (
                    "frequency".to_string(),
                    Value::Scalar { si_value: f, dimension: DimensionVector::FREQUENCY },
                ),
                ("shape".to_string(), Value::List(Vec::new())),
                ("participation_mass".to_string(), Value::Real(0.0)),
                ("damping_ratio".to_string(), Value::Real(0.0)),
            ]
            .into_iter()
            .collect();
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_id: StructureTypeId(u32::MAX),
                type_name: "Mode".to_string(),
                version: 1,
                fields,
            }))
        })
        .collect();

    // ── (6) shape ModalResult (6-field, mirroring run_modal_analysis step 7) ──
    let result_fields: PersistentMap<String, Value> = [
        ("part".to_string(), placeholder_part()),
        ("modes".to_string(), Value::List(modes_list)),
        ("boundary_conditions".to_string(), Value::List(Vec::new())),
        ("damping".to_string(), Value::Undef),
        ("mass_matrix_norm".to_string(), Value::Real(0.0)),
        ("stiffness_matrix_norm".to_string(), Value::Real(0.0)),
    ]
    .into_iter()
    .collect();
    let result = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ModalResult".to_string(),
        version: 1,
        fields: result_fields,
    }));
    ComputeOutcome::Completed {
        result,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
    }
}

// G-allow: modal::mechanism_modal ComputeFn entry point (task #4271) — reached
// only via the fn-pointer registered in compute_targets::register_compute_fns
// (mod.rs), which the orphan audit cannot trace. Wired + tested in this file.
pub fn solve_mechanism_modal_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    run_mechanism_modal(value_inputs, prior_warm_state, cancellation)
}

// ---------------------------------------------------------------------------
// Trampolines (modal::transient_response, modal::displacement_at) — task ι
// ---------------------------------------------------------------------------

/// Build a degenerate `DisplacementTimeHistory` `Value::StructureInstance`: an
/// empty `t_samples` list and empty `mode_coords`, echoing a degenerate (empty)
/// `ModalResult`. This is the result returned when the transient solve is
/// short-circuited (the step-14 empty-forcing guard) and by the step-10 stub
/// before the full mode-superposition solve lands (step-12). Shaped to the ι
/// structure-def (4 fields, `modal_analysis.ri`); `StructureTypeId(u32::MAX)` is
/// the registry-free sentinel, mirroring [`degenerate_modal_result`].
fn degenerate_displacement_history() -> Value {
    let fields: PersistentMap<String, Value> = [
        ("part".to_string(), placeholder_part()),
        ("modal_result".to_string(), degenerate_modal_result()),
        ("t_samples".to_string(), Value::List(Vec::new())),
        ("mode_coords".to_string(), Value::List(Vec::new())),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "DisplacementTimeHistory".to_string(),
        version: 1,
        fields,
    }))
}

/// Build the degenerate short-circuit outcome for a `ForcingTimeHistory` that
/// carries no usable sources (`sources` empty or absent): an
/// `E_TransientForcingMissing` `Error` diagnostic plus a degenerate (empty
/// `t_samples` / `mode_coords`) `DisplacementTimeHistory` — no transient was
/// integrated. The transient twin of [`no_mass_matrix_outcome`]; message-based
/// diagnostic (`code: None`) per design_decision #6, and no warm state is
/// donated (ι owns fn+dispatch; caching is λ's job).
///
/// The `ForcingTimeHistory` ctor's `sources.count > 0` constraint
/// (`modal_analysis.ri`) catches the common case at construction, so an e2e
/// cannot normally reach the trampoline with empty sources; this guard defends
/// the dispatch boundary against a hand-built / Undef / degenerate forcing.
fn forcing_missing_outcome() -> ComputeOutcome {
    let diagnostic = Diagnostic::error(
        "E_TransientForcingMissing: the forcing time-history carries no sources \
         (`sources` empty or absent), so there is no load to project onto the \
         modes and the mode-superposition transient is undefined; returning an \
         empty displacement history.",
    );
    ComputeOutcome::Completed {
        result: degenerate_displacement_history(),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![diagnostic],
    }
}

/// Build the degenerate short-circuit outcome for a `modal_result` input that
/// carries no usable modes (`modal_result` absent / `Value::Undef`, or a
/// `ModalResult` struct whose `modes` list is empty / absent): an
/// `E_TransientModalResultMissing` `Error` diagnostic plus a degenerate (empty
/// `t_samples` / `mode_coords`) `DisplacementTimeHistory` — no transient was
/// integrated. Mirrors [`forcing_missing_outcome`]; message-based diagnostic
/// (`code: None`) per design_decision #6, and no warm state is donated.
///
/// This guard closes the third silent-failure path in `run_transient_response`:
/// without it, the per-mode loop runs zero times and the trampoline returns a
/// structurally-valid but all-zero result that masks an upstream failed
/// `modal::free_vibration` node. Placed before the empty-forcing guard so that
/// a failed modal-analysis node (the root cause) is surfaced first.
fn modal_result_missing_outcome() -> ComputeOutcome {
    let diagnostic = Diagnostic::error(
        "E_TransientModalResultMissing: the modal result carries no modes \
         (`modal_result` absent/Undef or its `modes` list empty), so there are \
         no mode shapes to project the forcing onto and the mode-superposition \
         transient is undefined; returning an empty displacement history. This \
         typically indicates the upstream modal analysis (modal::free_vibration) \
         failed or was not yet evaluated.",
    );
    ComputeOutcome::Completed {
        result: degenerate_displacement_history(),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![diagnostic],
    }
}

/// Fetch field `name` from a StructureInstance `val` by reference (no clone);
/// `None` if `val` is not a StructureInstance or lacks the field. The borrowing
/// companion to [`field_or`], used by the transient trampolines to read forcing /
/// mode sub-values without cloning the whole field map.
fn field_ref<'a>(val: &'a Value, name: &str) -> Option<&'a Value> {
    if let Value::StructureInstance(data) = val {
        return data.fields.get(name);
    }
    None
}

/// Read a `Vector3` runtime value into `[f64; 3]`, tolerating both the
/// `Value::Vector` encoding [`mode_shape_value`] emits and a `Value::List` of
/// numerics; missing components / non-vector values read as `0.0` (defensive —
/// the type-checker guarantees the shape upstream).
fn read_vec3(val: &Value) -> [f64; 3] {
    let comps = match val {
        Value::Vector(c) | Value::List(c) => c,
        _ => return [0.0; 3],
    };
    let mut out = [0.0; 3];
    for (slot, v) in out.iter_mut().zip(comps.iter()) {
        *slot = read_scalar_si(v);
    }
    out
}

/// Read a `List<Scalar>` runtime value into `Vec<f64>` (SI magnitudes); a
/// non-list reads as empty. Used for `SampledForce.time_samples / force_samples`.
fn read_real_list(val: &Value) -> Vec<f64> {
    match val {
        Value::List(items) => items.iter().map(read_scalar_si).collect(),
        _ => Vec::new(),
    }
}

/// The `modes` list of a `ModalResult` StructureInstance, by reference; an empty
/// slice if absent / mis-shaped.
fn modal_result_modes(modal_result: &Value) -> &[Value] {
    if let Value::StructureInstance(data) = modal_result
        && let Some(Value::List(modes)) = data.fields.get("modes")
    {
        return modes;
    }
    &[]
}

/// Read one `Mode`'s `shape` field (a `List<Vector3>`) into per-node `[f64; 3]`
/// displacements; empty if absent / mis-shaped.
fn read_mode_shape(mode: &Value) -> Vec<[f64; 3]> {
    match field_ref(mode, "shape") {
        Some(Value::List(nodes)) => nodes.iter().map(read_vec3).collect(),
        _ => Vec::new(),
    }
}

/// Per-mode node shapes Φᵢ of a `ModalResult` (one `Vec<[f64; 3]>` per mode).
/// Shared by [`solve_transient_response_trampoline`] (forcing projection) and
/// [`displacement_at_trampoline`] (reconstruction) — both form Φᵢ[node]·dir.
fn extract_mode_shapes(modal_result: &Value) -> Vec<Vec<[f64; 3]>> {
    modal_result_modes(modal_result)
        .iter()
        .map(read_mode_shape)
        .collect()
}

/// The forcing sources of a `ForcingTimeHistory` StructureInstance, cloned; empty
/// if absent / mis-shaped. The step-14 guard distinguishes "empty-but-present"
/// (the `E_TransientForcingMissing` condition) from a well-formed source list.
fn extract_forcing_sources(forcing: &Value) -> Vec<Value> {
    match field_ref(forcing, "sources") {
        Some(Value::List(sources)) => sources.clone(),
        _ => Vec::new(),
    }
}

/// Resolve a forcing/query `location` string to a node index, geometry-free
/// (design-decision-3). A string that parses as a non-negative integer is an
/// explicit node index; any other string resolves to the fundamental-mode
/// (mode 0) antinode `dominant_antinode_index(Φ₀)` — the cantilever free-end
/// tip. The forcing projection and `displacement_at` share this resolver, so
/// "force at tip" / "query at tip" hit the same node.
///
/// CONTRACT (reviewer suggestion 4): an explicit numeric index that exceeds the
/// node count is INTENTIONALLY clamped to the last node (`idx.min(n_nodes − 1)`),
/// not rejected. Geometry-free resolution has no node table to validate an index
/// against, and clamping keeps both call sites total — they consume a bare
/// `usize` with no diagnostic channel threaded through. The v0.3 node-resolution
/// simplification is already escalated (design-decision-3); a full LocationId→
/// node topology (which could reject an out-of-range index) is deferred to the
/// Part type. Pinned by `resolve_location_node_clamps_out_of_range_index`.
fn resolve_location_node(location: &str, mode0_shape: &[[f64; 3]]) -> usize {
    if let Ok(idx) = location.trim().parse::<usize>() {
        return idx.min(mode0_shape.len().saturating_sub(1));
    }
    dominant_antinode_index(mode0_shape)
}

/// A forcing source with its invariant (time-independent) fields pre-extracted
/// once. Resolving each source into this form *before* the grid loop hoists the
/// per-source `field_ref` reads — and, for `Sampled`, the `Vec<f64>` table clone
/// — out of the per-timestep sampler, so [`ResolvedForcing::sample`] does only
/// arithmetic. This removes the O(T) redundant field lookups/allocations per
/// source that arose when the closed-form params were re-parsed on every one of
/// the T timesteps.
enum ResolvedForcing {
    Step {
        magnitude: f64,
        start_time: f64,
    },
    Harmonic {
        amplitude: f64,
        frequency: f64,
        phase: f64,
    },
    Impulse {
        impulse: f64,
        time: f64,
    },
    Sampled {
        times: Vec<f64>,
        forces: Vec<f64>,
    },
    /// Unrecognised / non-struct source — contributes no force at any `t`.
    Zero,
}

impl ResolvedForcing {
    /// Pre-extract a forcing source's invariant fields once, dispatched by
    /// `type_name` to the θ-solver closed-form sampler family
    /// (`reify-stdlib::modal::transient`). An unrecognised / non-struct source
    /// resolves to [`ResolvedForcing::Zero`].
    fn from_value(source: &Value) -> Self {
        let type_name = match source {
            Value::StructureInstance(data) => data.type_name.as_str(),
            _ => return ResolvedForcing::Zero,
        };
        let scalar = |name: &str| field_ref(source, name).map(read_scalar_si).unwrap_or(0.0);
        match type_name {
            "StepForce" => ResolvedForcing::Step {
                magnitude: scalar("magnitude"),
                start_time: scalar("start_time"),
            },
            "HarmonicForce" => ResolvedForcing::Harmonic {
                amplitude: scalar("amplitude"),
                frequency: scalar("frequency"),
                phase: scalar("phase"),
            },
            "ImpulseForce" => ResolvedForcing::Impulse {
                impulse: scalar("impulse"),
                time: scalar("time"),
            },
            "SampledForce" => ResolvedForcing::Sampled {
                times: field_ref(source, "time_samples")
                    .map(read_real_list)
                    .unwrap_or_default(),
                forces: field_ref(source, "force_samples")
                    .map(read_real_list)
                    .unwrap_or_default(),
            },
            _ => ResolvedForcing::Zero,
        }
    }

    /// Scalar forcing `p_src(t)` from the pre-extracted fields — pure arithmetic
    /// over the closed-form samplers, no field lookups. `dt` is the uniform grid
    /// step (only the `Impulse` discrete-pulse approximation needs it).
    fn sample(&self, t: f64, dt: f64) -> f64 {
        match self {
            ResolvedForcing::Step {
                magnitude,
                start_time,
            } => step_force_at(*magnitude, *start_time, t),
            ResolvedForcing::Harmonic {
                amplitude,
                frequency,
                phase,
            } => harmonic_force_at(*amplitude, *frequency, *phase, t),
            ResolvedForcing::Impulse { impulse, time } => impulse_force_at(*impulse, *time, t, dt),
            ResolvedForcing::Sampled { times, forces } => sampled_force_at(times, forces, t),
            ResolvedForcing::Zero => 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Task λ: TransientCache warm-state (mirrors ModalAnalysisCache / κ pattern)
// ---------------------------------------------------------------------------

/// Warm-state payload donated by the transient-response trampoline (task λ):
/// the cache key plus the forcing-independent setup it certifies — the uniform
/// time grid and per-mode prepared integrators. Recovered on the next
/// invocation via `downcast_ref` and reused only when the incoming request's
/// [`TransientCacheKey`] matches — i.e. the cached grid and integrator
/// coefficients are still valid for the new `(t_start, t_end, dt, modes)`.
///
/// Caching the prepared integrators lets a forcing-only re-evaluation (the
/// PRD §7.8 / §9.1 input-shaping workload) skip coefficient derivation and
/// run only the forcing projection + ODE recurrence — "only re-integrate the
/// ODE" in the PRD §9.1 sense.
#[derive(Clone)]
pub(crate) struct TransientCache {
    /// The forcing-independent determinants the cached `grid` + `prepared`
    /// were built for.
    pub(crate) key: TransientCacheKey,
    /// Uniform time grid `[t_start, t_start+dt, …]` — rebuilt from `key`
    /// determinants but cached to avoid re-deriving the `floor` count.
    pub(crate) grid: Vec<f64>,
    /// Per-mode prepared integrators, one per mode in the cached `ModalResult`.
    /// Each carries the Duhamel coefficients or the Newmark `(ω, ζ)` marker.
    pub(crate) prepared: Vec<PreparedIntegrator>,
}

impl TransientCache {
    /// Estimated retained size of this cache in bytes: the grid `f64`s plus
    /// per-mode `PreparedIntegrator` structs (each ≈ `DuhamelCoeffs` at 64 B)
    /// plus the flat key scalars + Vec overhead. Drives both the
    /// [`OpaqueState`] size hint (pool LRU budgeting) and the donated
    /// `cost_per_byte`. Always > 0, so the `cost_per_byte` reciprocal is
    /// well-defined.
    fn estimated_size_bytes(&self) -> usize {
        let grid_bytes = self.grid.len() * std::mem::size_of::<f64>();
        let prepared_bytes = self.prepared.len() * std::mem::size_of::<PreparedIntegrator>();
        // key: 3 f64s + Vec<(f64,f64)> — approximate the Vec payload.
        let key_bytes =
            3 * std::mem::size_of::<f64>() + self.key.modes.len() * 2 * std::mem::size_of::<f64>();
        (grid_bytes + prepared_bytes + key_bytes).max(1)
    }

    /// Wrap this cache in an [`OpaqueState`] for donation to the warm-state
    /// pool, sized by [`estimated_size_bytes`](Self::estimated_size_bytes).
    ///
    /// Returns that `size_bytes` alongside the state so the caller can derive
    /// `cost_per_byte` from the same measurement — the payload is walked
    /// exactly once per donation rather than again inside this method.
    pub(crate) fn into_opaque_state(self) -> (OpaqueState, usize) {
        let size = self.estimated_size_bytes();
        (OpaqueState::new(self, size), size)
    }
}

/// Result of the in-crate transient core [`run_transient_response`]: the
/// engine-facing [`ComputeOutcome`] plus a white-box `reused_setup` flag the
/// unit tests assert cache amortization against (the public `ComputeFn`
/// returns only the outcome). Mirrors [`ModalTrampolineRun`].
pub(crate) struct TransientTrampolineRun {
    /// The compute outcome the public trampoline returns.
    pub(crate) outcome: ComputeOutcome,
    /// `true` iff this run reused a cached [`TransientCache`] grid + prepared
    /// integrators rather than recomputing them. Observable only in-crate
    /// (amortization tests); the public `ComputeFn` discards it, hence
    /// `allow(dead_code)`.
    #[allow(dead_code)]
    pub(crate) reused_setup: bool,
}

/// In-crate transient core behind [`solve_transient_response_trampoline`],
/// adding the task-λ warm-state cache — reuse the pre-derived grid + per-mode
/// integrators across calls whose [`TransientCacheKey`] matches — on top of
/// the forcing-projection + mode-superposition pipeline. Returns a
/// [`TransientTrampolineRun`] so in-crate tests can also observe whether the
/// setup was reused; the public trampoline takes only `.outcome`.
///
/// Cache behaviour (mirrors `run_modal_analysis` / task κ):
/// - Empty-forcing guard and degenerate-grid guard short-circuit WITHOUT
///   donating a cache (those paths cannot form a valid `TransientCache`).
/// - A cold call (no prior, or a key MISS) builds the grid + prepared
///   integrators fresh and donates them.
/// - A warm hit (key HIT) reuses the cached `grid + prepared` and re-runs
///   only the forcing projection + ODE recurrence for each mode.
///
/// Cancellation (between-modes granularity, PRD §6):
/// - Entry checkpoint — cancel before any work.
/// - Per-mode poll at the top of each integration loop step.
///
/// Finer within-mode per-timestep polling is a localized future refinement
/// (would thread a probe closure into `integrate_prepared`, keeping
/// `reify-stdlib` dependency-free).
///
/// CONVENTION (post-32218afeb6): all field reads use borrowed `&str` literal
/// keys (e.g. `field_ref(mode, "frequency")`), NOT `.get(&"x".to_string())`.
pub(crate) fn run_transient_response(
    value_inputs: &[Value],
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> TransientTrampolineRun {
    // ── (0) entry cancellation checkpoint ────────────────────────────────────
    // Coarse cooperative cancellation (PRD §6): poll at entry + per-mode, mirroring
    // run_modal_analysis's entry + pre-eigensolve polls. Finer within-mode
    // per-timestep polling would require threading a probe closure into
    // integrate_prepared (keeping reify-stdlib dependency-free) — a localized
    // future refinement.
    if cancellation.is_cancelled() {
        return TransientTrampolineRun {
            outcome: ComputeOutcome::Cancelled,
            reused_setup: false,
        };
    }

    // value_inputs = [modal_result, forcing, t_start, t_end, dt]
    let modal_result = value_inputs.first().cloned().unwrap_or(Value::Undef);
    let forcing = value_inputs.get(1).cloned().unwrap_or(Value::Undef);
    let t_start = value_inputs.get(2).map(read_scalar_si).unwrap_or(0.0);
    let t_end = value_inputs.get(3).map(read_scalar_si).unwrap_or(0.0);
    let dt = value_inputs.get(4).map(read_scalar_si).unwrap_or(0.0);

    // ── (1) missing-modal-result guard ───────────────────────────────────────
    // modal_result is the primary positional input: Undef / non-ModalResult /
    // ModalResult with empty modes all yield &[] from modal_result_modes, which
    // would silently produce a zero-displacement history by summing over zero
    // modes. Fire this guard first (even when forcing is also empty) so the
    // diagnostic points at the root cause — the upstream modal solve — rather
    // than the secondary empty-forcing symptom. Mirrors forcing_missing_outcome.
    if modal_result_modes(&modal_result).is_empty() {
        return TransientTrampolineRun {
            outcome: modal_result_missing_outcome(),
            reused_setup: false,
        };
    }

    // ── (2) empty-forcing guard (no cache — mirrors density guard) ────────────
    let sources = extract_forcing_sources(&forcing);
    if sources.is_empty() {
        return TransientTrampolineRun {
            outcome: forcing_missing_outcome(),
            reused_setup: false,
        };
    }

    // ── (3) extract per-mode (frequency_hz, damping_ratio, shape) ────────────
    // Shapes are extracted fresh every call (they feed only the always-recomputed
    // forcing projection, not the cached coefficients; the echoed modal_result
    // supplies them, so a key match over (freq, zeta, dt, t_range) fully certifies
    // the cached payload — mode shapes are explicitly excluded from the key).
    let modes = modal_result_modes(&modal_result);
    let shapes = extract_mode_shapes(&modal_result);

    // Collect (frequency_hz, damping_ratio) pairs for the cache key; also used
    // to compute omega below. Borrowed from the modal_result via field_ref.
    let mode_params: Vec<(f64, f64)> = modes
        .iter()
        .map(|mode| {
            let freq = field_ref(mode, "frequency")
                .map(read_scalar_si)
                .unwrap_or(0.0);
            let zeta = field_ref(mode, "damping_ratio")
                .map(read_scalar_si)
                .unwrap_or(0.0);
            (freq, zeta)
        })
        .collect();

    // ── (4) cache lookup: try to reuse the grid + prepared integrators ────────
    // Key excludes `forcing` (cheap-varying) and mode shapes (not needed by
    // the coefficients); see TransientCacheKey docs.
    let key = TransientCacheKey::new(t_start, t_end, dt, mode_params.clone());
    let prior_cache = prior_warm_state.and_then(|s| s.downcast_ref::<TransientCache>());
    let (grid, prepared, reused_setup) = match prior_cache {
        Some(cache) if cache.key.matches(&key) => {
            // HIT: reuse the cached grid + integrators (Clone only on confirmed hit).
            (cache.grid.clone(), cache.prepared.clone(), true)
        }
        _ => {
            // MISS (or no prior): build grid + prepare integrators fresh.
            let grid = uniform_time_grid(t_start, t_end, dt);
            // Degenerate-grid guard (no cache — grid is empty if params are malformed).
            if grid.is_empty() {
                let diagnostic = Diagnostic::error(format!(
                    "E_TransientTimeGridDegenerate: the transient time parameters are \
                     degenerate (t_start = {t_start}, t_end = {t_end}, dt = {dt}); a \
                     uniform time grid requires dt > 0 and t_end ≥ t_start, so no \
                     timesteps were generated and an empty displacement history is \
                     returned.",
                ));
                return TransientTrampolineRun {
                    outcome: ComputeOutcome::Completed {
                        result: degenerate_displacement_history(),
                        new_warm_state: None,
                        cost_per_byte: None,
                        diagnostics: vec![diagnostic],
                    },
                    reused_setup: false,
                };
            }
            let prepared: Vec<PreparedIntegrator> = mode_params
                .iter()
                .map(|&(freq, zeta)| {
                    let omega = 2.0 * PI * freq;
                    prepare_modal_integrator(omega, zeta, &grid)
                })
                .collect();
            (grid, prepared, false)
        }
    };

    // ── (5) forcing projection (always, even on a cache HIT) ─────────────────
    // Per source: resolve node, direction, sample p_src(tⱼ) over the grid.
    let mode0_shape: &[[f64; 3]] = shapes.first().map(Vec::as_slice).unwrap_or(&[]);
    struct SourceProjection {
        node: usize,
        dir: [f64; 3],
        samples: Vec<f64>,
    }
    let projections: Vec<SourceProjection> = sources
        .iter()
        .map(|src| {
            let at = match field_ref(src, "at") {
                Some(Value::String(s)) => s.as_str(),
                _ => "",
            };
            let node = resolve_location_node(at, mode0_shape);
            let dir = field_ref(src, "direction")
                .map(read_vec3)
                .unwrap_or([0.0; 3]);
            let resolved = ResolvedForcing::from_value(src);
            let samples = grid.iter().map(|&t| resolved.sample(t, dt)).collect();
            SourceProjection { node, dir, samples }
        })
        .collect();

    // ── (6) per-mode integration (+ cancellation poll between modes) ──────────
    let mut mode_coords: Vec<Value> = Vec::with_capacity(modes.len());
    for (i, (_freq, zeta)) in mode_params.iter().enumerate() {
        // Between-modes cancellation poll (PRD §6 "between modes (per timestep)").
        if cancellation.is_cancelled() {
            return TransientTrampolineRun {
                outcome: ComputeOutcome::Cancelled,
                reused_setup: false,
            };
        }

        let shape_i: &[[f64; 3]] = shapes.get(i).map(Vec::as_slice).unwrap_or(&[]);
        let omega = 2.0 * PI * mode_params[i].0;
        let _ = (zeta, omega); // used via prepared[i] below

        let mut f_i = vec![0.0_f64; grid.len()];
        for p in &projections {
            let phi = shape_i.get(p.node).copied().unwrap_or([0.0; 3]);
            let coeff = phi[0] * p.dir[0] + phi[1] * p.dir[1] + phi[2] * p.dir[2];
            if coeff == 0.0 {
                continue;
            }
            for (slot, &p_t) in f_i.iter_mut().zip(p.samples.iter()) {
                *slot += coeff * p_t;
            }
        }

        // Use the (reused or fresh) prepared integrator — no coefficient re-derivation.
        let coords = if let Some(prep) = prepared.get(i) {
            integrate_prepared(prep, &grid, &f_i, 0.0, 0.0)
        } else {
            // Fallback for a mode count mismatch (shouldn't happen on a proper HIT,
            // but degrade gracefully rather than panic).
            solve_modal_response(omega, mode_params[i].1, &grid, &f_i, 0.0, 0.0).coords
        };
        mode_coords.push(Value::List(coords.into_iter().map(Value::Real).collect()));
    }

    // ── (7) shape the DisplacementTimeHistory ────────────────────────────────
    let t_samples = Value::List(
        grid.iter()
            .map(|&t| Value::Scalar {
                si_value: t,
                dimension: DimensionVector::TIME,
            })
            .collect(),
    );
    let fields: PersistentMap<String, Value> = [
        ("part".to_string(), placeholder_part()),
        ("modal_result".to_string(), modal_result),
        ("t_samples".to_string(), t_samples),
        ("mode_coords".to_string(), Value::List(mode_coords)),
    ]
    .into_iter()
    .collect();
    let result = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "DisplacementTimeHistory".to_string(),
        version: 1,
        fields,
    }));

    // ── (8) donate the grid + prepared integrators as warm state (task λ) ─────
    let cache = TransientCache {
        key,
        grid,
        prepared,
    };
    let (state, size_bytes) = cache.into_opaque_state();
    let cost_per_byte = if size_bytes > 0 {
        Some(1.0 / size_bytes as f64)
    } else {
        None
    };
    let outcome = ComputeOutcome::Completed {
        result,
        new_warm_state: Some(state),
        cost_per_byte,
        diagnostics: Vec::new(),
    };
    TransientTrampolineRun {
        outcome,
        reused_setup,
    }
}

/// `@optimized("modal::transient_response")` public `ComputeFn` (task ι;
/// registered in `compute_targets::mod`).
///
/// Mode-superposition transient solve (PRD §5.3 / §10 task ι):
///   1. read `t_start/t_end/dt`, build the uniform grid (`uniform_time_grid`);
///   2. read each mode's `(ω = 2π·frequency, ζ = damping_ratio, Φ shape)`;
///   3. per forcing source: resolve its node (`resolve_location_node`), read its
///      direction, and sample its scalar `p_src(tⱼ)` (`ResolvedForcing::sample`,
///      its invariant fields pre-resolved once per source);
///   4. project onto each mode: `f_i[j] = Σ_src (Φ_i[node]·dir)·p_src(tⱼ)`;
///   5. integrate each decoupled SDOF mode (`solve_modal_response`) → ξ_i(tⱼ);
///   6. shape the `DisplacementTimeHistory`, echoing the input `ModalResult` so
///      `displacement_at` can read each Φᵢ without re-running the eigensolve.
///
/// No warm state is donated — ι owns fn+dispatch; warm-state caching is λ's job —
/// mirroring [`no_mass_matrix_outcome`]. The empty-forcing guard
/// (`E_TransientForcingMissing`) lands in step-14.
pub fn solve_transient_response_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    run_transient_response(value_inputs, prior_warm_state, cancellation).outcome
}

/// `@optimized("modal::displacement_at")` public `ComputeFn` (task ι; registered
/// in `compute_targets::mod`).
///
/// Lazy single-location modal-superposition reconstruction (PRD §5.2):
///   1. resolve the query `location` to a node (`resolve_location_node`: numeric
///      string → explicit index, else the fundamental antinode over mode-0 Φ);
///   2. read the query `direction` and form each mode's projection coefficient
///      `coeff_i = Φ_i[node]·direction` (from the echoed `ModalResult`);
///   3. recombine with the stored modal coordinates:
///      `u(tⱼ) = Σ_i coeff_i · mode_coords[i][j]` (`reconstruct_series`).
///
/// Lazy: only the queried node's time series is reconstructed — the full
/// `n_nodes × n_times` displacement field is never materialized. Unlike the other
/// modal trampolines this returns a non-struct `Value::List(Real)` (PRD §5.2). No
/// warm state is donated (ι owns fn+dispatch; caching is λ's job).
pub fn displacement_at_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // value_inputs = [history, location, direction] (pre-2 signature).
    let history = match value_inputs.first() {
        Some(h) => h,
        None => return displacement_series_outcome(Vec::new()),
    };
    let location = match value_inputs.get(1) {
        Some(Value::String(s)) => s.as_str(),
        _ => "",
    };
    let direction = value_inputs.get(2).map(read_vec3).unwrap_or([0.0; 3]);

    // Per-mode node shapes Φᵢ (from the echoed ModalResult) — Φᵢ[node] supplies
    // each projection coefficient. Borrowed (not cloned) off the history.
    let shapes = match field_ref(history, "modal_result") {
        Some(modal_result) => extract_mode_shapes(modal_result),
        None => Vec::new(),
    };
    let mode0_shape: &[[f64; 3]] = shapes.first().map(Vec::as_slice).unwrap_or(&[]);
    let node = resolve_location_node(location, mode0_shape);

    // coeff_i = Φ_i[node]·direction (only the queried node is touched per mode).
    let coeffs: Vec<f64> = shapes
        .iter()
        .map(|shape_i| {
            let phi = shape_i.get(node).copied().unwrap_or([0.0; 3]);
            phi[0] * direction[0] + phi[1] * direction[1] + phi[2] * direction[2]
        })
        .collect();

    // The stored modal-coordinate matrix ξ_i(tⱼ) as Vec<Vec<f64>> (List<List<Real>>).
    let mode_coords: Vec<Vec<f64>> = match field_ref(history, "mode_coords") {
        Some(Value::List(series)) => series.iter().map(read_real_list).collect(),
        _ => Vec::new(),
    };

    displacement_series_outcome(reconstruct_series(&coeffs, &mode_coords))
}

/// Wrap a reconstructed displacement series in a `ComputeOutcome::Completed`
/// carrying a `Value::List(Real)` (PRD §5.2) — the non-struct result shape unique
/// to `displacement_at`. No warm state / diagnostics (ι donates neither).
fn displacement_series_outcome(series: Vec<f64>) -> ComputeOutcome {
    ComputeOutcome::Completed {
        result: Value::List(series.into_iter().map(Value::Real).collect()),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: Vec::new(),
    }
}

/// Read an SI scalar magnitude from a numeric `Value`, tolerating the runtime
/// spellings a stdlib numeric field takes: `Scalar { si_value }` (dimensioned —
/// geometry, density, E), `Real`, and `Int`. Non-numeric values read as `0.0`
/// (the upstream type-checker guarantees the shape; this is a defensive floor,
/// not a validation point — mirrors buckling's permissive scalar reads).
fn read_scalar_si(val: &Value) -> f64 {
    match val {
        Value::Scalar { si_value, .. } => *si_value,
        Value::Real(r) => *r,
        Value::Int(n) => *n as f64,
        _ => 0.0,
    }
}

/// Extract `IsotropicElastic { youngs_modulus, poisson_ratio }` from the
/// material StructureInstance (`youngs_modulus : Scalar<Pressure>`,
/// `poisson_ratio : Real`). Missing fields read as `0.0` via [`read_scalar_si`]
/// (defensive; the type-checker guarantees presence for a real ElasticMaterial).
fn extract_isotropic_material(val: &Value) -> IsotropicElastic {
    let mut youngs_modulus = 0.0;
    let mut poisson_ratio = 0.0;
    if let Value::StructureInstance(data) = val {
        if let Some(v) = data.fields.get("youngs_modulus") {
            youngs_modulus = read_scalar_si(v);
        }
        if let Some(v) = data.fields.get("poisson_ratio") {
            poisson_ratio = read_scalar_si(v);
        }
    }
    IsotropicElastic {
        youngs_modulus,
        poisson_ratio,
    }
}

/// Extract the eigensolver knobs `(n_modes, tol, max_iters, sigma)` from a
/// `ModalOptions` StructureInstance, falling back to the PRD §4.3 defaults
/// (`n_modes = 10`, `tol = 1e-9`, `max_iters = 200`, `sigma = 0`) when the value
/// is not a StructureInstance or a field is missing / malformed. Mirrors
/// buckling's `extract_buckling_options`.
fn extract_eigen_knobs(val: &Value) -> (usize, f64, usize, f64) {
    let default_n_modes = 10_usize;
    let default_tol = 1e-9_f64;
    let default_max_iters = 200_usize;
    let default_sigma = 0.0_f64;

    let data = match val {
        Value::StructureInstance(d) => d,
        _ => {
            return (
                default_n_modes,
                default_tol,
                default_max_iters,
                default_sigma,
            );
        }
    };
    let n_modes = match data.fields.get("n_modes") {
        Some(Value::Int(n)) => (*n).max(1) as usize,
        _ => default_n_modes,
    };
    let tol = match data.fields.get("tol") {
        Some(Value::Real(r)) if r.is_finite() && *r > 0.0 => *r,
        _ => default_tol,
    };
    let max_iters = match data.fields.get("max_iters") {
        Some(Value::Int(n)) => (*n).max(1) as usize,
        _ => default_max_iters,
    };
    let sigma = match data.fields.get("sigma") {
        Some(Value::Real(r)) if r.is_finite() => *r,
        _ => default_sigma,
    };
    (n_modes, tol, max_iters, sigma)
}

/// Extract the unit excitation `reference_direction` (along which per-mode
/// participation mass is projected) from a `ModalOptions` StructureInstance.
/// Reads the `Value::Vector` field's three components (each via
/// [`read_scalar_si`]) and normalizes to a unit vector — realizing the
/// `reference_direction.norm() > 0` invariant deferred from the structure-def to
/// this trampoline (modal_analysis.ri:382-389). A missing / degenerate
/// (zero-norm) direction falls back to the slender bending default `[0, 0, 1]`.
fn extract_reference_direction(val: &Value) -> [f64; 3] {
    let default_dir = [0.0, 0.0, 1.0];
    let raw = match val {
        Value::StructureInstance(data) => match data.fields.get("reference_direction") {
            Some(Value::Vector(items)) if items.len() == 3 => [
                read_scalar_si(&items[0]),
                read_scalar_si(&items[1]),
                read_scalar_si(&items[2]),
            ],
            _ => return default_dir,
        },
        _ => return default_dir,
    };
    let norm = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
    if norm > 0.0 {
        [raw[0] / norm, raw[1] / norm, raw[2] / norm]
    } else {
        default_dir
    }
}

/// Extract the Rayleigh damping coefficients `(α, β)` from a `ModalOptions`
/// StructureInstance's `damping` field. A `RayleighDamping { alpha, beta }`
/// StructureInstance yields its coefficients; `NoDamping` (or any other shape)
/// yields `(0, 0)` — the undamped case (ζ_i = 0 for every mode). The
/// discriminator is the runtime `type_name`, matching the SIR-α nominal type-tag
/// the structure-defs document.
fn extract_damping(val: &Value) -> (f64, f64) {
    if let Value::StructureInstance(data) = val
        && let Some(Value::StructureInstance(damping)) = data.fields.get("damping")
        && damping.type_name == "RayleighDamping"
    {
        let alpha = damping
            .fields
            .get("alpha")
            .map(read_scalar_si)
            .unwrap_or(0.0);
        let beta = damping
            .fields
            .get("beta")
            .map(read_scalar_si)
            .unwrap_or(0.0);
        return (alpha, beta);
    }
    (0.0, 0.0)
}

/// Extract the requested finite-element order from a `ModalOptions`
/// StructureInstance's `element_order` field.
///
/// An `ElementOrder.P2` enum value (runtime `Value::Enum { variant: "P2", .. }`)
/// selects [`ElementOrder::P2`] — the quadratic 10-node-tet path that resolves
/// bending curvature (task 4066). Everything else — a missing field, a non-enum
/// value, or the explicit `ElementOrder.P1` — defaults to [`ElementOrder::P1`],
/// keeping the constant-strain path and every existing P1 fixture/test bit-for-bit
/// unchanged (matching `ModalOptions.element_order`'s declared `ElementOrder.P1`
/// default). Mirrors [`extract_damping`]'s match-then-default defensive field read;
/// the enum is discriminated solely by its `variant` tag, the runtime
/// representation of an `ElementOrder` value (reify-ir `Value::Enum`).
fn extract_element_order(val: &Value) -> ElementOrder {
    if let Value::StructureInstance(data) = val
        && let Some(Value::Enum { variant, .. }) = data.fields.get("element_order")
        && variant == "P2"
    {
        return ElementOrder::P2;
    }
    ElementOrder::P1
}

/// Build the homogeneous Dirichlet BCs from the `boundary_conditions` faces.
///
/// Two realizations, discriminated by the named faces (design_decision #1; the
/// `Part`/`Support`-topology channel that would carry richer BC intent has not
/// landed, so the support *targets* encode the configuration):
///
///   • **Simply-supported (pin-pin)** — both beam-axis end faces (`"x_min"` AND
///     `"x_max"`) are named (the `simply_supported_beam_modes.ri` two-support
///     fixture). Delegates to [`simply_supported_pin_pin_bcs`]: pin only the
///     transverse (Z) DOF on both end faces + minimal axial/lateral anchors, so
///     the bending rotation stays free and the modes follow the `(nπ)²`
///     simply-supported family (NOT fixed-fixed).
///
///   • **Clamp the named face(s)** — any other target set (the cantilever's lone
///     `"x_min"` support). Every mesh node on each named face
///     (`"x_min"`/`"x_max"`/`"y_min"`/`"y_max"`/`"z_min"`/`"z_max"`) has all three
///     translational DOFs clamped — the cantilever root clamp (step-16).
///
/// Takes only the node coordinates (`&[[f64; 3]]`) of the discretization the
/// trampoline hands to [`solve_modal_core`] — BC selection is coordinate-only and
/// never touches element connectivity, so the node slice is the whole contract
/// (no half-populated `BeamMesh` sentinel needed for the P2 path). The DOF indices
/// line up with the solve's mesh because both index the same node set.
/// `length`/`width`/`height` still parameterize the face-coordinate thresholds.
/// Duplicate DOFs (a corner shared by two named faces) are harmless —
/// `solve_modal_core` records constraints idempotently.
fn build_dirichlet_bcs(
    options: &Value,
    nodes: &[[f64; 3]],
    length: f64,
    width: f64,
    height: f64,
) -> Vec<DirichletBc> {
    let targets = support_targets(options);

    // Simply-supported (pin-pin) discriminator: BOTH beam-axis end faces named.
    let pins_x_min = targets.iter().any(|t| t == "x_min");
    let pins_x_max = targets.iter().any(|t| t == "x_max");
    if pins_x_min && pins_x_max {
        return simply_supported_pin_pin_bcs(nodes, length, height);
    }

    // General "clamp the named face" realization (cantilever root clamp).
    let eps = 1e-9_f64;
    let mut bcs = Vec::new();
    for target in &targets {
        for (n, coord) in nodes.iter().enumerate() {
            let on_face = match target.as_str() {
                "x_min" => coord[0] <= eps,
                "x_max" => coord[0] >= length - eps,
                "y_min" => coord[1] <= eps,
                "y_max" => coord[1] >= width - eps,
                "z_min" => coord[2] <= eps,
                "z_max" => coord[2] >= height - eps,
                _ => false,
            };
            if on_face {
                for axis in 0..3 {
                    bcs.push(DirichletBc {
                        dof: 3 * n + axis,
                        value: 0.0,
                    });
                }
            }
        }
    }
    bcs
}

/// Realize the simply-supported (pin-pin) Dirichlet BCs for the beam (step-18).
///
/// A simply-supported beam pins the transverse deflection at both ends while
/// leaving the bending rotation `dw/dx` free, giving natural frequencies in the
/// `fₙ = ((nπ)²/2π)·√(EI/ρAL⁴)` family. Realizing that in the 3-D solid model
/// without spuriously clamping the rotation (which would yield the *fixed-fixed*
/// family, ~2.45× higher) requires care:
///
///   1. **Simple supports** — pin ONLY the transverse Z DOF on every node of
///      both end faces (`x ≈ 0` and `x ≈ L`). The bending rotation at a support
///      is carried by the *axial* displacement `u(z) = −(z − z_c)·dw/dx`, NOT by
///      `w`, so pinning `w` (not `u`) on the end face leaves `dw/dx` free — a
///      genuine simple support. Pinning `w` across the full end face also removes
///      three rigid-body modes whose `w`-field is nonzero there: the Z
///      translation, the X-axis twist, and the global rigid Y-rotation.
///
///   2. **Minimal anchors** — the three rigid-body modes left after step 1 (the X
///      translation, the Y translation, and the in-plane Z-rotation) must be
///      removed or `K_free` is singular and the shift-invert Cholesky fails.
///      They are killed at the two end-face NEUTRAL-axis nodes (`z = h/2`):
///      - pin **X** at the `x_min` neutral node → removes X translation;
///      - pin **Y** at the `x_min` AND `x_max` neutral nodes (separated by `L`
///        along x) → removes Y translation *and* the in-plane Z-rotation
///        (a single Y anchor cannot remove both — a rotation about the vertical
///        axis through that one node leaves it fixed; two anchors separated in
///        x pin the rotation too).
///
/// Both anchor families are non-intrusive to the vertical bending modes (the
/// task's headline signal): the vertical mode has `u = 0` at the neutral axis
/// (so the X anchor sits on its node line) and `v = 0` everywhere (so the Y
/// anchors never load it). Anchoring at the neutral axis — rather than clamping
/// `u` across a full face — is precisely what keeps the support rotation free.
fn simply_supported_pin_pin_bcs(nodes: &[[f64; 3]], length: f64, height: f64) -> Vec<DirichletBc> {
    // `width` is not a parameter: the Z simple-support spans the full end face by
    // node coordinate, and the anchors sit on the y = 0 neutral-axis node line.
    let eps = 1e-9_f64;
    let mut bcs = Vec::new();

    // (1) Simple supports: pin the transverse (Z) DOF on both end faces.
    for (n, coord) in nodes.iter().enumerate() {
        let on_end = coord[0] <= eps || coord[0] >= length - eps;
        if on_end {
            bcs.push(DirichletBc {
                dof: 3 * n + 2,
                value: 0.0,
            }); // Z (bending)
        }
    }

    // (2) Minimal anchors at the two end-face neutral-axis nodes (z = h/2).
    let root = nearest_node(nodes, [0.0, 0.0, height / 2.0]);
    let tip = nearest_node(nodes, [length, 0.0, height / 2.0]);
    bcs.push(DirichletBc {
        dof: 3 * root,
        value: 0.0,
    }); // X anchor (axial)
    bcs.push(DirichletBc {
        dof: 3 * root + 1,
        value: 0.0,
    }); // Y anchor (lateral, root)
    bcs.push(DirichletBc {
        dof: 3 * tip + 1,
        value: 0.0,
    }); // Y anchor (lateral, tip)
    bcs
}

/// Index of the mesh node nearest `target` in Euclidean distance.
///
/// Used to place the simply-supported anchors on the end-face neutral-axis nodes
/// robustly — by coordinate, independent of `build_beam_mesh`'s internal node
/// numbering (mirroring the unit tests' coordinate-based face selection).
fn nearest_node(nodes: &[[f64; 3]], target: [f64; 3]) -> usize {
    let dist2 = |p: &[f64; 3]| -> f64 {
        let dx = p[0] - target[0];
        let dy = p[1] - target[1];
        let dz = p[2] - target[2];
        dx * dx + dy * dy + dz * dz
    };
    nodes
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            dist2(a)
                .partial_cmp(&dist2(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .expect("beam mesh has at least one node")
}

/// Collect the `target` face names from the options' `boundary_conditions` list
/// (`FixedSupport { target : String }` instances). Non-StructureInstance entries
/// and entries without a string `target` are skipped.
fn support_targets(options: &Value) -> Vec<String> {
    let mut targets = Vec::new();
    if let Value::StructureInstance(data) = options
        && let Some(Value::List(items)) = data.fields.get("boundary_conditions")
    {
        for item in items {
            if let Value::StructureInstance(support) = item
                && let Some(Value::String(target)) = support.fields.get("target")
            {
                targets.push(target.clone());
            }
        }
    }
    targets
}

/// Reshape a full-DOF mode shape `phi_full` (length `3·n_nodes`, `0.0` at
/// constrained DOFs) into the `List<Vector3<Dimensionless>>` representation
/// declared on `Mode.shape`: one per-node displacement `Vector([Real;3])` per
/// mesh node, collected into a `List`.
fn mode_shape_value(phi_full: &[f64]) -> Value {
    debug_assert_eq!(
        phi_full.len() % 3,
        0,
        "phi_full must have exactly 3 DOFs per node (got len={})",
        phi_full.len()
    );
    Value::List(
        phi_full
            .chunks_exact(3)
            .map(|c| {
                Value::Vector(vec![
                    Value::Real(c[0]),
                    Value::Real(c[1]),
                    Value::Real(c[2]),
                ])
            })
            .collect(),
    )
}

/// Fetch field `name` from a StructureInstance `val`, cloning it; returns
/// `fallback` if `val` is not a StructureInstance or lacks the field. Used to
/// echo the input `boundary_conditions` / `damping` onto the `ModalResult`.
fn field_or(val: &Value, name: &str, fallback: Value) -> Value {
    if let Value::StructureInstance(data) = val
        && let Some(v) = data.fields.get(name)
    {
        return v.clone();
    }
    fallback
}

#[cfg(test)]
mod tests {
    use faer::sparse::SparseRowMat;
    use reify_core::{Diagnostic, DimensionVector, Severity};
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
    use reify_solver_elastic::assembly::test_support::promote_tets_to_p2;
    use reify_solver_elastic::{DirichletBc, EigenSolverOptions, IsotropicElastic};
    use reify_stdlib::dynamics::mass_props::resolve_density_strict;
    use reify_stdlib::modal::free_vibration::{is_rigid_body_mode, rayleigh_damping_ratio};
    use reify_stdlib::modal::trampoline::{ModalCacheKey, TransientCacheKey};
    use reify_stdlib::modal::transient::uniform_time_grid;

    use super::{
        ModalAnalysisCache, ModalAssembly, ModalCoreResult, ModalMesh, ModalTrampolineRun,
        TransientCache, assemble_mechanism_km, assemble_modal_km, build_beam_mesh,
        build_dirichlet_bcs, degenerate_displacement_history, degenerate_modal_result,
        displacement_at_trampoline, eigensolve_modal, extract_damping,
        extract_density_or_degenerate, extract_eigen_knobs, extract_reference_direction,
        mode_shape_value, placeholder_part, read_real_list, read_scalar_si,
        resolve_location_node, run_mechanism_modal, run_modal_analysis, run_transient_response,
        simply_supported_pin_pin_bcs, solve_mechanism_modal_trampoline,
        solve_modal_analysis_trampoline, solve_modal_core, solve_transient_response_trampoline,
    };
    use crate::{CancellationHandle, ComputeOutcome};

    /// `aᵀ · M · b` for the free×free mass matrix `M` (sparse CSR row matvec then
    /// dot). Test-local invariant probe; the production normalization path
    /// computes the same generalized mass via its own helper in step 6.
    fn m_quadratic_form(m: &SparseRowMat<usize, f64>, a: &[f64], b: &[f64]) -> f64 {
        let sym = m.symbolic();
        let mut acc = 0.0_f64;
        for (r, &a_r) in a.iter().enumerate() {
            let cols = sym.col_idx_of_row_raw(r);
            let vals = m.val_of_row(r);
            let mut mb_r = 0.0_f64;
            for (col_raw, &v) in cols.iter().zip(vals.iter()) {
                mb_r += v * b[*col_raw];
            }
            acc += a_r * mb_r;
        }
        acc
    }

    /// Steel-like isotropic material (E = 205 GPa, ν = 0.29) shared across the
    /// modal core-solver fixtures.
    fn steel() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 205e9,
            poisson_ratio: 0.29,
        }
    }

    /// Steel density (kg/m³) — feeds the consistent mass matrix.
    const STEEL_DENSITY: f64 = 7850.0;

    /// Build homogeneous Dirichlet BCs clamping every DOF on the x_min (root)
    /// face — the cantilever root clamp. Returns the BC list together with the
    /// constrained-DOF index list (for the zeroed-shape assertion).
    ///
    /// The face is identified by node coordinate (`x ≈ 0`) read from the shared
    /// [`build_beam_mesh`] mesh, so the test stays robust to the internal node
    /// numbering of `solve_modal_core` (which meshes via the same helper).
    fn clamp_x_min_face(nodes: &[[f64; 3]]) -> (Vec<DirichletBc>, Vec<usize>) {
        let mut bcs = Vec::new();
        let mut dofs = Vec::new();
        for (n, coord) in nodes.iter().enumerate() {
            if coord[0] <= 1e-9 {
                for axis in 0..3 {
                    bcs.push(DirichletBc {
                        dof: 3 * n + axis,
                        value: 0.0,
                    });
                    dofs.push(3 * n + axis);
                }
            }
        }
        (bcs, dofs)
    }

    /// step-3 (RED → GREEN in step-4): shape + sanity pin for `solve_modal_core`.
    ///
    /// Coarse root-clamped block fixture (X = length = 20 mm beam axis,
    /// Y = width = 50 mm, Z = height = 100 mm bending axis). The internal
    /// shear-locking-aware mesh yields nx=1, ny=1, nz=6 → 28 nodes, 42 free DOFs
    /// — small enough for the eigensolver's dense fallback (fast, deterministic).
    /// This is a structural pin, NOT an accuracy check (frequency accuracy is
    /// the e2e test's job, steps 15/17).
    #[test]
    fn solve_modal_core_returns_well_shaped_result_for_coarse_cantilever() {
        let length = 0.02_f64; // X — beam axis (short → coarse mesh)
        let width = 0.05_f64; // Y — width
        let height = 0.1_f64; // Z — bending axis

        let mesh = build_beam_mesh(length, width, height);
        let (bcs, constrained_dofs) = clamp_x_min_face(&mesh.nodes);
        assert!(
            !constrained_dofs.is_empty(),
            "fixture must clamp at least one face DOF",
        );

        let eigen_opts = EigenSolverOptions {
            n_modes: 3,
            tol: 1e-8,
            max_iters: 200,
            sigma: 0.0,
        };

        let result: ModalCoreResult = solve_modal_core(
            ModalMesh::P1(&mesh),
            STEEL_DENSITY,
            &steel(),
            [0.0, 0.0, 1.0], // reference_direction; unused by this assertion
            &bcs,
            &eigen_opts,
        );

        // (a) n_nodes matches the shared mesh; ≥ 1 mode returned.
        assert_eq!(
            result.n_nodes,
            mesh.nodes.len(),
            "result.n_nodes must equal the shared mesh node count",
        );
        assert!(
            !result.frequencies.is_empty(),
            "expect at least 1 mode; got {}",
            result.frequencies.len(),
        );

        // (b) frequencies finite, strictly positive, sorted ascending.
        for (i, &f) in result.frequencies.iter().enumerate() {
            assert!(
                f.is_finite() && f > 0.0,
                "frequency[{i}] = {f} must be finite and strictly positive",
            );
        }
        for w in result.frequencies.windows(2) {
            assert!(
                w[0] <= w[1],
                "frequencies must be sorted ascending: {} > {}",
                w[0],
                w[1],
            );
        }

        // (c) one full-DOF mode shape per frequency, each length 3·n_nodes.
        assert_eq!(
            result.phi_full.len(),
            result.frequencies.len(),
            "one full mode shape per returned frequency",
        );
        for (i, phi) in result.phi_full.iter().enumerate() {
            assert_eq!(
                phi.len(),
                3 * result.n_nodes,
                "mode {i} shape length must be 3·n_nodes = {}",
                3 * result.n_nodes,
            );
        }

        // (d) every constrained (Dirichlet) DOF is exactly 0.0 in every
        //     scattered mode shape (free-DOF subspace scatter-back).
        for (i, phi) in result.phi_full.iter().enumerate() {
            for &g in &constrained_dofs {
                assert_eq!(
                    phi[g], 0.0,
                    "mode {i}: constrained DOF {g} must be exactly 0.0",
                );
            }
        }
    }

    /// step-3 (RED → GREEN in step-4): the `assemble_modal_km` ↔
    /// `eigensolve_modal` split that lets the warm-state cache hold the
    /// BC-/n_modes-independent assembled `(K, M)`.
    ///
    /// `solve_modal_core` is split into a `assemble_modal_km` (the expensive
    /// per-element K/M assembly + the `‖K‖_F`/`‖M‖_F` norms — BC- and
    /// n_modes-independent) and a cheap `eigensolve_modal` (free-DOF projection +
    /// eigensolve). This pins both halves on the coarse cantilever fixture:
    ///
    /// (a) the `ModalAssembly`'s `n_nodes` and norms BIT-equal what a full
    ///     `solve_modal_core` reports for the same inputs — equal by construction
    ///     because step-4 MOVES the assembly + Frobenius-norm code unchanged.
    /// (b) ONE assembled `ModalAssembly` is reusable across requests that differ
    ///     only in `n_modes`: `eigensolve_modal` run with n_modes = 2 then 4 on
    ///     the SAME assembly returns 2 and 4 modes, and the fundamental is
    ///     bit-stable (both stay in the dense regime — `n_free = 42 ≤
    ///     max(64, 2·n_modes)` — so the lowest eigenpair of the fixed pencil is
    ///     identical regardless of the requested count). This is the cache's
    ///     reason for being: amortize the assembly across an `n_modes` sweep.
    ///
    /// RED: `assemble_modal_km` / `eigensolve_modal` / `ModalAssembly` do not
    /// exist until step-4.
    #[test]
    fn assemble_then_eigensolve_splits_core_and_reuses_assembly() {
        let length = 0.02_f64; // X — beam axis
        let width = 0.05_f64; // Y — width
        let height = 0.1_f64; // Z — bending axis

        let mesh = build_beam_mesh(length, width, height);
        let (bcs, _) = clamp_x_min_face(&mesh.nodes);
        let reference_direction = [0.0, 0.0, 1.0];

        // ── (a) assembly n_nodes / norms equal a full solve_modal_core's ──────
        let assembly: ModalAssembly =
            assemble_modal_km(ModalMesh::P1(&mesh), STEEL_DENSITY, &steel());
        let opts2 = EigenSolverOptions {
            n_modes: 2,
            tol: 1e-9,
            max_iters: 200,
            sigma: 0.0,
        };
        let core = solve_modal_core(
            ModalMesh::P1(&mesh),
            STEEL_DENSITY,
            &steel(),
            reference_direction,
            &bcs,
            &opts2,
        );
        assert_eq!(
            assembly.n_nodes, core.n_nodes,
            "assembly n_nodes must equal core n_nodes",
        );
        assert_eq!(
            assembly.mass_matrix_norm.to_bits(),
            core.mass_matrix_norm.to_bits(),
            "assembly ‖M‖_F must bit-equal core's (code moved unchanged)",
        );
        assert_eq!(
            assembly.stiffness_matrix_norm.to_bits(),
            core.stiffness_matrix_norm.to_bits(),
            "assembly ‖K‖_F must bit-equal core's (code moved unchanged)",
        );

        // ── (b) one assembly, two eigensolves differing only in n_modes ───────
        let r2: ModalCoreResult = eigensolve_modal(&assembly, reference_direction, &bcs, &opts2);
        let opts4 = EigenSolverOptions {
            n_modes: 4,
            tol: 1e-9,
            max_iters: 200,
            sigma: 0.0,
        };
        let r4: ModalCoreResult = eigensolve_modal(&assembly, reference_direction, &bcs, &opts4);
        assert_eq!(r2.frequencies.len(), 2, "n_modes = 2 must return 2 modes");
        assert_eq!(r4.frequencies.len(), 4, "n_modes = 4 must return 4 modes");

        // Fundamental is the lowest eigenpair of the SAME (K_free, M_free) pencil
        // in both runs → identical to a tight relative tolerance (the assembly
        // was reused, not rebuilt; both runs take the dense path).
        let (f2, f4) = (r2.frequencies[0], r4.frequencies[0]);
        assert!(
            f2 > 0.0 && f2.is_finite(),
            "fundamental must be finite/positive: {f2}"
        );
        let rel = (f2 - f4).abs() / f4.abs().max(1.0);
        assert!(
            rel < 1e-9,
            "fundamental must be invariant across n_modes on one reused assembly: \
             {f2} vs {f4} (rel {rel:e})",
        );
    }

    // ── task-κ cache-aware core (`run_modal_analysis`) fixtures ──────────────

    /// Build the 5 modal `value_inputs` (material, length, width, height,
    /// ModalOptions) the cache tests drive `run_modal_analysis` with. Geometry +
    /// `density` are the `(K,M)`-determining inputs; `n_modes` is excluded from
    /// the key; `element_order` (when `Some`) is the runtime enum value the
    /// trampoline maps to the key's discriminant. A single `x_min` clamp keeps
    /// `K_free` SPD (well-posed eigenproblem).
    fn modal_inputs(
        length: f64,
        width: f64,
        height: f64,
        density: f64,
        n_modes: i64,
        element_order: Option<Value>,
    ) -> Vec<Value> {
        let mut opts = vec![
            ("n_modes".to_string(), Value::Int(n_modes)),
            (
                "boundary_conditions".to_string(),
                Value::List(vec![fixed_support("x_min")]),
            ),
            (
                "reference_direction".to_string(),
                Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]),
            ),
        ];
        if let Some(eo) = element_order {
            opts.push(("element_order".to_string(), eo));
        }
        vec![
            material_with_density(Some(density)),
            length_scalar(length),
            length_scalar(width),
            length_scalar(height),
            modal_options(opts),
        ]
    }

    /// Number of `Mode`s in a `Completed` `ModalResult` outcome (panics if the
    /// outcome is not a well-shaped Completed ModalResult).
    fn modes_len(outcome: &ComputeOutcome) -> usize {
        let ComputeOutcome::Completed { result, .. } = outcome else {
            panic!("expected a Completed outcome, got {outcome:?}");
        };
        let Value::StructureInstance(data) = result else {
            panic!("expected a ModalResult StructureInstance, got {result:?}");
        };
        match data.fields.get("modes") {
            Some(Value::List(m)) => m.len(),
            other => panic!("ModalResult.modes must be a List; got {other:?}"),
        }
    }

    /// step-5 (RED → GREEN in step-6): the cache-aware core `run_modal_analysis`
    /// donates a `ModalAnalysisCache` warm state and reuses it when only
    /// `n_modes` changes.
    ///
    /// (1) A cold call (`prior = None`) assembles fresh (`reused_assembly ==
    ///     false`), Completes, and donates a `new_warm_state` that downcasts to a
    ///     `ModalAnalysisCache` whose `key` matches the inputs' `ModalCacheKey`.
    /// (2) Feeding that cache back as `prior` with IDENTICAL geometry + material +
    ///     element_order but a DIFFERENT `n_modes` HITs the cache
    ///     (`reused_assembly == true`), Completes, and returns the NEW mode count
    ///     — the assembly was amortized across the `n_modes` change (the PRD
    ///     amortization goal).
    ///
    /// RED: `run_modal_analysis` / `ModalTrampolineRun` / `ModalAnalysisCache` do
    /// not exist until step-6.
    #[test]
    fn run_modal_analysis_caches_and_reuses_assembly_across_n_modes() {
        let handle = CancellationHandle::new();

        // ── (1) cold call → fresh assembly, donates a matching cache ──────────
        let inputs2 = modal_inputs(0.02, 0.05, 0.1, STEEL_DENSITY, 2, None);
        let run1: ModalTrampolineRun = run_modal_analysis(&inputs2, None, &handle);
        assert!(
            !run1.reused_assembly,
            "cold call (prior None) must assemble fresh"
        );
        assert_eq!(
            modes_len(&run1.outcome),
            2,
            "cold call returns n_modes = 2 modes"
        );

        let ComputeOutcome::Completed { new_warm_state, .. } = &run1.outcome else {
            panic!("cold call must Complete, got {:?}", run1.outcome);
        };
        let cache: &ModalAnalysisCache = new_warm_state
            .as_ref()
            .expect("a Completed outcome must donate a warm state")
            .downcast_ref::<ModalAnalysisCache>()
            .expect("donated warm state must be a ModalAnalysisCache");
        // Inputs' (K,M) key: steel (E = 205e9, ν = 0.29), P1 → discriminant 0.
        let expected_key = ModalCacheKey::new(0.02, 0.05, 0.1, 205e9, 0.29, STEEL_DENSITY, 0);
        assert!(
            cache.key.matches(&expected_key),
            "donated cache key must match the inputs' (K,M) key",
        );

        // ── (2) feed the cache back, differing only in n_modes → HIT ──────────
        let prior = cache.clone().into_opaque_state().0;
        let inputs4 = modal_inputs(0.02, 0.05, 0.1, STEEL_DENSITY, 4, None);
        let run2 = run_modal_analysis(&inputs4, Some(&prior), &handle);
        assert!(
            run2.reused_assembly,
            "inputs differing only in n_modes must HIT the cached assembly",
        );
        assert_eq!(
            modes_len(&run2.outcome),
            4,
            "a cache HIT still returns the newly-requested n_modes = 4",
        );
    }

    /// step-7κ (RED → GREEN in step-8): the cache key discriminates EXACTLY the
    /// `(K, M)`-determining inputs — geometry + material + element_order — and
    /// nothing else.
    ///
    /// With a prior `ModalAnalysisCache` built for geometry L1 + steel + P1, drive
    /// `run_modal_analysis` with that cache as `prior`:
    /// (a) DIFFERENT geometry (length L2 ≠ L1) → MISS — a stale `K`/`M` must never
    ///     be served for a different shape.
    /// (b) DIFFERENT material density → MISS — the consistent mass `M` depends on ρ.
    /// (c) DIFFERENT element_order (P2 vs the P1 prior) → MISS — task 4066: P2
    ///     assembles a distinct `K`/`M` and node count.
    /// (d) SAME geometry + material + element_order, changing ONLY `n_modes` → HIT.
    #[test]
    fn run_modal_analysis_cache_key_discriminates_km_inputs_only() {
        let handle = CancellationHandle::new();
        let (l1, w, h) = (0.02_f64, 0.05_f64, 0.1_f64);

        // Prior cache: geometry L1 + steel + P1.
        let cold = run_modal_analysis(
            &modal_inputs(l1, w, h, STEEL_DENSITY, 3, None),
            None,
            &handle,
        );
        let ComputeOutcome::Completed { new_warm_state, .. } = &cold.outcome else {
            panic!("prior cold call must Complete, got {:?}", cold.outcome);
        };
        let prior = new_warm_state
            .as_ref()
            .expect("cold call must donate a cache")
            .downcast_ref::<ModalAnalysisCache>()
            .expect("donated state must be a ModalAnalysisCache")
            .clone()
            .into_opaque_state()
            .0;

        // (a) different length → MISS (re-assembled).
        let a = run_modal_analysis(
            &modal_inputs(l1 * 2.0, w, h, STEEL_DENSITY, 3, None),
            Some(&prior),
            &handle,
        );
        assert!(
            !a.reused_assembly,
            "different geometry must re-assemble (no stale K/M)"
        );

        // (b) different density → MISS.
        let b = run_modal_analysis(
            &modal_inputs(l1, w, h, STEEL_DENSITY * 1.1, 3, None),
            Some(&prior),
            &handle,
        );
        assert!(
            !b.reused_assembly,
            "different density must re-assemble (M depends on ρ)"
        );

        // (c) different element_order (P2 vs the P1 prior) → MISS.
        let p2 = Value::Enum {
            type_name: "ElementOrder".to_string(),
            variant: "P2".to_string(),
        };
        let c = run_modal_analysis(
            &modal_inputs(l1, w, h, STEEL_DENSITY, 3, Some(p2)),
            Some(&prior),
            &handle,
        );
        assert!(
            !c.reused_assembly,
            "P2 must re-assemble against a P1-built prior (distinct K/M per task 4066)",
        );

        // (d) only n_modes differs → HIT.
        let d = run_modal_analysis(
            &modal_inputs(l1, w, h, STEEL_DENSITY, 5, None),
            Some(&prior),
            &handle,
        );
        assert!(
            d.reused_assembly,
            "changing only n_modes must HIT the cached assembly"
        );
    }

    /// step-9 (RED → GREEN in step-10): cooperative cancellation in
    /// `run_modal_analysis`.
    ///
    /// (a) A pre-cancelled handle short-circuits to `ComputeOutcome::Cancelled`
    ///     (before the costly eigensolve completes). (b) Regression: a fresh
    ///     handle still Completes — the added coarse polls don't break the happy
    ///     path.
    ///
    /// RED: the core ignores the handle until step-10, so a pre-cancelled run
    /// still Completes (assertion (a) fails).
    #[test]
    fn run_modal_analysis_honors_cancellation() {
        let inputs = modal_inputs(0.02, 0.05, 0.1, STEEL_DENSITY, 2, None);

        // (a) pre-cancelled → Cancelled.
        let cancelled = CancellationHandle::new();
        cancelled.cancel();
        let run = run_modal_analysis(&inputs, None, &cancelled);
        assert!(
            matches!(run.outcome, ComputeOutcome::Cancelled),
            "a pre-cancelled handle must yield ComputeOutcome::Cancelled, got {:?}",
            run.outcome,
        );
        assert!(!run.reused_assembly, "a cancelled run reuses nothing");

        // (b) fresh handle → Completed (the polls leave the happy path intact).
        let fresh = CancellationHandle::new();
        let ok = run_modal_analysis(&inputs, None, &fresh);
        assert!(
            matches!(ok.outcome, ComputeOutcome::Completed { .. }),
            "a fresh handle must Complete, got {:?}",
            ok.outcome,
        );
    }

    /// step-7 (RED → GREEN in step-8): the P2 (`ElementOrder::P2`) path of
    /// `solve_modal_core`.
    ///
    /// A STRUCTURAL pin, not an accuracy check — the headline P2 modal-frequency
    /// accuracy gate lives in `reify-solver-elastic`'s
    /// `tests/modal_benchmarks.rs` (which can call `solve_eigen_dense` directly;
    /// this eval-side test only proves the orchestration runs the quadratic path
    /// end-to-end and returns a well-shaped result).
    ///
    /// The same coarse root-clamped cantilever fixture as the P1 pin above,
    /// solved with `ElementOrder::P2`. P2 promotion inserts edge-midpoint nodes,
    /// so the solve must operate over the PROMOTED node set:
    ///   • `result.n_nodes` equals the promoted node count, strictly greater than
    ///     the P1 count (proving the promotion ran, not the P1 mesh);
    ///   • the exact P2 consistent mass `M` is PD enough that the generalized
    ///     eigensolve completes (converged, no Cholesky panic) — the
    ///     degree-4-exact integration guarantee from steps 1–2;
    ///   • frequencies are finite, strictly positive, ascending, with one
    ///     full-DOF mode shape (length `3·n_nodes_p2`) per frequency.
    ///
    /// BCs are built over the PROMOTED node set (clamping the `x ≈ 0` root face by
    /// coordinate so the new edge-midpoint nodes on the face are caught too). The
    /// same promoted `(nodes_p2, tets_p2)` is then passed to `solve_modal_core` as
    /// a `ModalMesh::P2`, so the BC DOF indices line up with the assembled K/M node
    /// numbering by construction (a single shared promotion, no internal re-walk).
    ///
    /// RED: `solve_modal_core` has no `element_order` parameter / no P2 branch yet
    /// (compile-fail).
    #[test]
    fn solve_modal_core_p2_path_returns_well_shaped_promoted_result() {
        let length = 0.02_f64; // X — beam axis (short → coarse promoted mesh)
        let width = 0.05_f64; // Y — width
        let height = 0.1_f64; // Z — bending axis

        let mesh = build_beam_mesh(length, width, height);

        // Promote once with the shared helper; the SAME promoted mesh is handed to
        // solve_modal_core (as ModalMesh::P2) AND used to build the BCs, so the BC
        // DOF indices match the assembled K/M node numbering exactly.
        let (nodes_p2, tets_p2) = promote_tets_to_p2(&mesh.nodes, &mesh.tets);
        assert!(
            nodes_p2.len() > mesh.nodes.len(),
            "P2 promotion must add edge-midpoint nodes: {} !> {}",
            nodes_p2.len(),
            mesh.nodes.len(),
        );

        // Clamp the x ≈ 0 root face over the PROMOTED node set (catches P2
        // edge-midpoints by coordinate).
        let mut bcs = Vec::new();
        for (n, coord) in nodes_p2.iter().enumerate() {
            if coord[0] <= 1e-9 {
                for axis in 0..3 {
                    bcs.push(DirichletBc {
                        dof: 3 * n + axis,
                        value: 0.0,
                    });
                }
            }
        }
        assert!(
            !bcs.is_empty(),
            "fixture must clamp at least one root-face DOF"
        );

        let eigen_opts = EigenSolverOptions {
            n_modes: 3,
            tol: 1e-8,
            max_iters: 200,
            sigma: 0.0,
        };

        let result: ModalCoreResult = solve_modal_core(
            ModalMesh::P2 {
                nodes: &nodes_p2,
                tets: &tets_p2,
            },
            STEEL_DENSITY,
            &steel(),
            [0.0, 0.0, 1.0], // reference_direction; unused by this assertion
            &bcs,
            &eigen_opts,
        );

        // (a) n_nodes is the PROMOTED P2 count (> the P1 count) — the P2 branch
        //     assembled K/M over the promoted mesh, not the P1 one.
        assert_eq!(
            result.n_nodes,
            nodes_p2.len(),
            "P2 result.n_nodes must equal the promoted node count {}",
            nodes_p2.len(),
        );

        // (b) ≥ 1 mode returned and (d) the eigensolve converged — the exact P2
        //     mass is PD, so the generalized solve completes without panicking.
        assert!(
            !result.frequencies.is_empty(),
            "expect ≥ 1 P2 mode; got {}",
            result.frequencies.len(),
        );
        assert!(result.converged, "P2 generalized eigensolve must converge");

        // (c) frequencies finite, strictly positive, ascending.
        for (i, &f) in result.frequencies.iter().enumerate() {
            assert!(
                f.is_finite() && f > 0.0,
                "P2 frequency[{i}] = {f} must be finite and strictly positive",
            );
        }
        for w in result.frequencies.windows(2) {
            assert!(
                w[0] <= w[1],
                "P2 frequencies must be sorted ascending: {} > {}",
                w[0],
                w[1],
            );
        }

        // (e) one full-DOF mode shape per frequency, each length 3·n_nodes_p2.
        assert_eq!(
            result.phi_full.len(),
            result.frequencies.len(),
            "one full mode shape per returned P2 frequency",
        );
        for (i, phi) in result.phi_full.iter().enumerate() {
            assert_eq!(
                phi.len(),
                3 * result.n_nodes,
                "P2 mode {i} shape length must be 3·n_nodes_p2 = {}",
                3 * result.n_nodes,
            );
        }
    }

    /// step-5 (RED → GREEN in step-6): mass-normalization invariant.
    ///
    /// On the same coarse root-clamped fixture, after normalization each mode
    /// must have unit M-generalized mass `φ_free_iᵀ·M_free·φ_free_i = 1` (sound
    /// by construction: φ is divided by √(generalized mass) — pinned at 1e-12),
    /// and distinct modes must be M-orthogonal `φ_iᵀ·M_free·φ_j ≈ 0` (looser
    /// 1e-8: depends on the solver's orthogonalization, not a by-construction
    /// identity). RED: the raw eigenvectors carry arbitrary scale, so the
    /// diagonal generalized mass is not 1.
    #[test]
    fn solve_modal_core_modes_are_mass_normalized() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;

        let mesh = build_beam_mesh(length, width, height);
        let (bcs, _constrained) = clamp_x_min_face(&mesh.nodes);
        let eigen_opts = EigenSolverOptions {
            n_modes: 3,
            tol: 1e-8,
            max_iters: 200,
            sigma: 0.0,
        };

        let result: ModalCoreResult = solve_modal_core(
            ModalMesh::P1(&mesh),
            STEEL_DENSITY,
            &steel(),
            [0.0, 0.0, 1.0], // reference_direction; unused by this assertion
            &bcs,
            &eigen_opts,
        );

        assert!(!result.phi_free.is_empty(), "expect at least 1 mode");

        // (a) Diagonal: unit M-generalized mass (by construction, 1e-12).
        for (i, phi_i) in result.phi_free.iter().enumerate() {
            let m_ii = m_quadratic_form(&result.m_free, phi_i, phi_i);
            assert!(
                (m_ii - 1.0).abs() < 1e-12,
                "mode {i}: φᵀMφ = {m_ii}, expected 1.0 within 1e-12",
            );
        }

        // (b) Off-diagonal: cross-mode M-orthogonality (solver-dependent, 1e-8).
        for i in 0..result.phi_free.len() {
            for j in (i + 1)..result.phi_free.len() {
                let m_ij =
                    m_quadratic_form(&result.m_free, &result.phi_free[i], &result.phi_free[j]);
                assert!(
                    m_ij.abs() < 1e-8,
                    "modes {i},{j}: φ_iᵀMφ_j = {m_ij}, expected ≈ 0 within 1e-8",
                );
            }
        }
    }

    /// step-7 (RED → GREEN in step-8): participation-mass completeness identity.
    ///
    /// On the coarse root-clamped fixture solved for the FULL spectrum
    /// (`n_modes = n_free`, dense path), the per-mode effective masses must
    /// satisfy the Parseval/completeness identity for the reference direction
    /// `d`:
    ///
    /// ```text
    /// Σ_i (φ_free_iᵀ·M_free·d_free)²  =  d_freeᵀ·M_free·d_free
    /// ```
    ///
    /// i.e. `Σ_i participation_mass[i]` equals the total translational mass of
    /// the free DOFs along `d` — EXACTLY, because a complete M-orthonormal basis
    /// resolves the identity `Σ_i φ_i φ_iᵀ M = I`. Each φ is mass-normalized
    /// (φᵀMφ = 1, step 6) and the clamped fixture's eigenvalues are distinct, so
    /// the eigenvectors are mutually M-orthogonal and the basis is M-orthonormal.
    /// This pins the participation computation and the normalization together
    /// with a deterministic, by-construction-exact assertion (design_decision
    /// #5; avoids the fuzzy "≥99% capture" bound).
    ///
    /// RED: `participation_mass` (and the `reference_direction` parameter) are
    /// absent until step 8.
    #[test]
    fn solve_modal_core_participation_mass_satisfies_completeness() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;
        // Bending (Z) direction; a unit vector so the identity's RHS is the
        // exact total Z-translational mass of the free DOFs.
        let reference_direction = [0.0_f64, 0.0, 1.0];

        let mesh = build_beam_mesh(length, width, height);
        let (bcs, constrained_dofs) = clamp_x_min_face(&mesh.nodes);
        let n_dofs = 3 * mesh.nodes.len();
        let n_free = n_dofs - constrained_dofs.len();
        assert!(n_free > 0, "fixture must leave at least one free DOF");

        // Full spectrum: request every free mode so {φ_i} is a complete basis.
        let eigen_opts = EigenSolverOptions {
            n_modes: n_free,
            tol: 1e-8,
            max_iters: 200,
            sigma: 0.0,
        };

        let result: ModalCoreResult = solve_modal_core(
            ModalMesh::P1(&mesh),
            STEEL_DENSITY,
            &steel(),
            reference_direction,
            &bcs,
            &eigen_opts,
        );

        // Precondition: the dense path returned the entire free spectrum (so the
        // completeness sum is over a complete basis, not a truncated one).
        assert_eq!(
            result.participation_mass.len(),
            n_free,
            "full-spectrum solve must return n_free = {n_free} effective masses",
        );

        // Rebuild d_free (the reference direction broadcast to each free node's
        // translational DOFs; axis = full DOF index mod 3) from the same clamp,
        // independently of the solver's internal map.
        let mut is_constrained = vec![false; n_dofs];
        for &g in &constrained_dofs {
            is_constrained[g] = true;
        }
        let full_of_free: Vec<usize> = (0..n_dofs).filter(|&g| !is_constrained[g]).collect();
        let d_free: Vec<f64> = full_of_free
            .iter()
            .map(|&g| reference_direction[g % 3])
            .collect();

        // RHS: total translational mass of the free DOFs along d.
        let total_mass = m_quadratic_form(&result.m_free, &d_free, &d_free);
        assert!(
            total_mass > 0.0,
            "reference-direction mass must be positive"
        );

        // LHS: Σ_i participation_mass[i] = Σ_i (φ_iᵀ M d)².
        let captured: f64 = result.participation_mass.iter().sum();

        assert!(
            (captured - total_mass).abs() < 1e-9,
            "completeness identity: Σ participation = {captured}, total mass = \
             {total_mass}, |Δ| = {} exceeds 1e-9",
            (captured - total_mass).abs(),
        );
    }

    /// step-9 (RED → GREEN in step-10): rigid-body-mode diagnostic.
    ///
    /// An UNCONSTRAINED fixture (empty BCs) admits the six rigid-body modes of a
    /// free 3-D body (ω ≈ 0). The dense generalized path handles the singular
    /// `K_free` (no up-front Cholesky), so requesting `n_modes = n_free/2`
    /// (≥ 42, forcing the dense regime for this `n_free = 84` mesh) returns them
    /// as the lowest modes. `solve_modal_core` must (a) return ≥ 1 mode with
    /// ω ≈ 0 and (b) surface a `Warning` diagnostic whose message starts
    /// `"W_ModalRigidBodyMode"`.
    ///
    /// The near-zero tolerance (1.0 rad/s ≈ 0.16 Hz) sits in the measured
    /// 7-decade gap between the rigid modes (ω ≤ ~1e-2 rad/s) and the first
    /// flexible mode (ω ≥ ~1e5 rad/s). RED: the `diagnostics` field is absent.
    #[test]
    fn solve_modal_core_flags_rigid_body_modes_when_unconstrained() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;

        let mesh = build_beam_mesh(length, width, height);
        let n_free = 3 * mesh.nodes.len(); // empty BCs → all DOFs free
        // n_modes ≥ n_free/2 forces solve_generalized_eigen's dense regime
        // (n ≤ max(64, 2·n_modes)), avoiding the shift-invert Cholesky panic on
        // the singular (rigid-body) K_free.
        let eigen_opts = EigenSolverOptions {
            n_modes: n_free / 2,
            tol: 1e-8,
            max_iters: 200,
            sigma: 0.0,
        };

        let result: ModalCoreResult = solve_modal_core(
            ModalMesh::P1(&mesh),
            STEEL_DENSITY,
            &steel(),
            [0.0, 0.0, 1.0],
            &[], // unconstrained
            &eigen_opts,
        );

        // (a) at least one returned mode is a rigid-body mode (ω ≈ 0).
        let omega = |f: f64| 2.0 * std::f64::consts::PI * f;
        let rigid_count = result
            .frequencies
            .iter()
            .filter(|&&f| is_rigid_body_mode(omega(f), 1.0))
            .count();
        assert!(
            rigid_count >= 1,
            "unconstrained body must expose ≥1 rigid-body mode (ω≈0); got {rigid_count}",
        );

        // (b) a W_ModalRigidBodyMode Warning is surfaced.
        let has_rigid_warning = result.diagnostics.iter().any(|d| {
            d.severity == Severity::Warning && d.message.starts_with("W_ModalRigidBodyMode")
        });
        assert!(
            has_rigid_warning,
            "expected a Warning starting \"W_ModalRigidBodyMode\"; got {:?}",
            result.diagnostics,
        );
    }

    /// step-9 (RED → GREEN in step-10): convergence-shortfall diagnostic.
    ///
    /// Requesting more modes than the free-DOF count can yield (`n_modes` ≫
    /// `n_free` on the clamped fixture) makes the eigensolver return fewer modes
    /// than requested (`converged == false`). `solve_modal_core` must surface a
    /// `Warning` diagnostic whose message starts `"W_ModalConvergence"`. The
    /// clamped fixture has no rigid-body modes, isolating this signal. RED: the
    /// `diagnostics` field is absent.
    #[test]
    fn solve_modal_core_flags_convergence_shortfall_when_over_requested() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;

        let mesh = build_beam_mesh(length, width, height);
        let (bcs, constrained) = clamp_x_min_face(&mesh.nodes);
        let n_free = 3 * mesh.nodes.len() - constrained.len();

        // Request far more modes than exist → the dense path returns only n_free.
        let eigen_opts = EigenSolverOptions {
            n_modes: n_free + 64,
            tol: 1e-8,
            max_iters: 200,
            sigma: 0.0,
        };

        let result: ModalCoreResult = solve_modal_core(
            ModalMesh::P1(&mesh),
            STEEL_DENSITY,
            &steel(),
            [0.0, 0.0, 1.0],
            &bcs,
            &eigen_opts,
        );

        assert!(
            result.frequencies.len() < eigen_opts.n_modes,
            "fixture must return fewer modes than requested to trigger the warning",
        );

        let has_conv_warning = result.diagnostics.iter().any(|d| {
            d.severity == Severity::Warning && d.message.starts_with("W_ModalConvergence")
        });
        assert!(
            has_conv_warning,
            "expected a Warning starting \"W_ModalConvergence\"; got {:?}",
            result.diagnostics,
        );
    }

    /// Amendment (suggestion 1 / robustness): an under-constrained model must NOT
    /// panic the engine, regardless of mesh size.
    ///
    /// The production trampoline uses the default `n_modes = 10`. On a mesh whose
    /// free-DOF count exceeds `max(64, 2·n_modes) = 64`, the size heuristic alone
    /// would route to `solve_eigen_shift_invert`, whose up-front Cholesky PANICS
    /// on the singular `K_free` of a no/insufficient-supports model. The
    /// under-constraint guard (constrained DOFs < 6 rigid-body modes → force the
    /// dense path) must keep the solve graceful: it returns a result and surfaces
    /// the `W_ModalRigidBodyMode` diagnostic instead of crashing.
    ///
    /// This fixture has `n_free = 84 > 64` with empty BCs (0 constrained DOFs),
    /// so pre-fix it took the panicking shift-invert path under the default
    /// `n_modes` — unlike `solve_modal_core_flags_rigid_body_modes_when_unconstrained`,
    /// which masks the bug by hand-picking `n_modes = n_free/2` to force dense.
    #[test]
    fn solve_modal_core_unconstrained_default_n_modes_does_not_panic() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;
        let mesh = build_beam_mesh(length, width, height);
        assert!(
            3 * mesh.nodes.len() > 64,
            "fixture must exceed the dense-regime threshold to exercise the guard",
        );

        // Production default n_modes; empty BCs → 0 constrained DOFs (< 6).
        let eigen_opts = EigenSolverOptions {
            n_modes: 10,
            tol: 1e-8,
            max_iters: 200,
            sigma: 0.0,
        };

        let result: ModalCoreResult = solve_modal_core(
            ModalMesh::P1(&mesh),
            STEEL_DENSITY,
            &steel(),
            [0.0, 0.0, 1.0],
            &[], // unconstrained → singular K_free
            &eigen_opts,
        );

        // Graceful: ≥ 1 mode returned (no panic) and the rigid-body warning fires.
        assert!(
            !result.frequencies.is_empty(),
            "under-constrained solve must still return modes (not panic)",
        );
        let has_rigid_warning = result.diagnostics.iter().any(|d| {
            d.severity == Severity::Warning && d.message.starts_with("W_ModalRigidBodyMode")
        });
        assert!(
            has_rigid_warning,
            "expected a W_ModalRigidBodyMode Warning for the under-constrained \
             model; got {:?}",
            result.diagnostics,
        );
    }

    /// Build a minimal `ElasticMaterial`-shaped `Value::StructureInstance` with
    /// the usual elastic fields, optionally carrying a `density` scalar. Mirrors
    /// the runtime material shape the trampoline reads (cf. buckling's
    /// `extract_material`): `youngs_modulus : Scalar<Pressure>`,
    /// `poisson_ratio : Real`, and (when `Some`) `density : Scalar<MassDensity>`.
    fn material_with_density(density: Option<f64>) -> Value {
        let mut fields: Vec<(String, Value)> = vec![
            (
                "youngs_modulus".to_string(),
                Value::Scalar {
                    si_value: 205e9,
                    dimension: DimensionVector::PRESSURE,
                },
            ),
            ("poisson_ratio".to_string(), Value::Real(0.29)),
        ];
        if let Some(d) = density {
            fields.push((
                "density".to_string(),
                Value::Scalar {
                    si_value: d,
                    dimension: DimensionVector::MASS_DENSITY,
                },
            ));
        }
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "ElasticMaterial".to_string(),
            version: 1,
            fields: fields.into_iter().collect(),
        }))
    }

    /// Assert the density-guard short-circuit: the returned outcome is a
    /// `Completed` carrying (a) an `Error` diagnostic whose message starts
    /// `"E_ModalNoMassMatrix"` and (b) a degenerate `ModalResult` whose `modes`
    /// list is empty (no eigenproblem was solved). No panic on any path.
    fn assert_no_mass_degenerate(outcome: ComputeOutcome) {
        let ComputeOutcome::Completed {
            result,
            diagnostics,
            ..
        } = outcome
        else {
            panic!("expected a Completed degenerate outcome, got a non-Completed variant");
        };

        // (a) an Error diagnostic identifies the no-mass-matrix condition.
        let has_err = diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.starts_with("E_ModalNoMassMatrix"));
        assert!(
            has_err,
            "expected an Error starting \"E_ModalNoMassMatrix\"; got {diagnostics:?}",
        );

        // (b) the result is a degenerate ModalResult with an empty modes list.
        let data = match &result {
            Value::StructureInstance(d) => d,
            other => panic!("expected a ModalResult StructureInstance, got {other:?}"),
        };
        assert_eq!(
            data.type_name, "ModalResult",
            "degenerate result must be a ModalResult, got {}",
            data.type_name,
        );
        match data.fields.get("modes") {
            Some(Value::List(modes)) => assert!(
                modes.is_empty(),
                "degenerate ModalResult.modes must be empty; got {} modes",
                modes.len(),
            ),
            other => {
                panic!("expected ModalResult.modes to be an (empty) Value::List, got {other:?}")
            }
        }
    }

    /// step-11 (RED → GREEN in step-12): no-mass-matrix density guard at the
    /// trampoline boundary.
    ///
    /// The consistent mass matrix `M` cannot be assembled without a positive
    /// mass density, and `Kφ = λMφ` is meaningless with no `M`. So the
    /// trampoline's density-extraction entry must short-circuit — emit an
    /// `E_ModalNoMassMatrix` Error and a degenerate empty-modes `ModalResult` —
    /// when the material carries no usable `density` (missing or ≤ 0), rather
    /// than panicking or assembling/eigensolving. A positive density passes the
    /// guard and yields `Ok(density)` (PRD diagnostics; design_decision #6:
    /// message-based, `code: None`).
    ///
    /// RED: `extract_density_or_degenerate` is absent until step 12.
    #[test]
    fn trampoline_density_guard_flags_missing_or_nonpositive_density() {
        // (a) missing `density` field → degenerate + E_ModalNoMassMatrix.
        match extract_density_or_degenerate(&material_with_density(None)) {
            Err(outcome) => assert_no_mass_degenerate(outcome),
            Ok(d) => panic!("missing density must short-circuit; got Ok({d})"),
        }

        // (b) zero density → degenerate (≤ 0 fails the guard).
        match extract_density_or_degenerate(&material_with_density(Some(0.0))) {
            Err(outcome) => assert_no_mass_degenerate(outcome),
            Ok(d) => panic!("zero density must short-circuit; got Ok({d})"),
        }

        // (c) negative density → degenerate.
        match extract_density_or_degenerate(&material_with_density(Some(-1.0))) {
            Err(outcome) => assert_no_mass_degenerate(outcome),
            Ok(d) => panic!("negative density must short-circuit; got Ok({d})"),
        }

        // (d) positive density → Ok(density), no short-circuit.
        match extract_density_or_degenerate(&material_with_density(Some(7850.0))) {
            Ok(got) => assert!(
                (got - 7850.0).abs() < 1e-9,
                "positive density must pass through unchanged; got {got}",
            ),
            Err(_) => panic!("positive density must pass the guard"),
        }
    }

    // -- suggestion 2: trampoline extraction-helper + shaping coverage --------

    /// Build a `Value::StructureInstance` with the given `type_name` and fields
    /// (the `StructureTypeId(u32::MAX)` registry-free sentinel the trampoline
    /// uses). Shared constructor for the option/support/damping fixtures below.
    fn struct_instance(type_name: &str, fields: Vec<(String, Value)>) -> Value {
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: type_name.to_string(),
            version: 1,
            fields: fields.into_iter().collect(),
        }))
    }

    /// A `FixedSupport { target }` instance — the runtime support shape
    /// `support_targets` reads to discriminate the BC realization.
    fn fixed_support(target: &str) -> Value {
        struct_instance(
            "FixedSupport",
            vec![("target".to_string(), Value::String(target.to_string()))],
        )
    }

    /// A `RayleighDamping { alpha, beta }` instance — the damped shape
    /// `extract_damping` discriminates by `type_name`.
    fn rayleigh_damping(alpha: f64, beta: f64) -> Value {
        struct_instance(
            "RayleighDamping",
            vec![
                ("alpha".to_string(), Value::Real(alpha)),
                ("beta".to_string(), Value::Real(beta)),
            ],
        )
    }

    /// Assemble a `ModalOptions`-shaped instance from the given fields.
    fn modal_options(fields: Vec<(String, Value)>) -> Value {
        struct_instance("ModalOptions", fields)
    }

    /// A `Length` scalar (SI metres), as the trampoline reads geometry inputs.
    fn length_scalar(m: f64) -> Value {
        Value::Scalar {
            si_value: m,
            dimension: DimensionVector::LENGTH,
        }
    }

    /// Amendment (suggestion 2): `extract_eigen_knobs` reads populated fields and
    /// falls back to the PRD §4.3 defaults for missing / malformed / non-struct
    /// inputs.
    #[test]
    fn extract_eigen_knobs_reads_fields_and_falls_back() {
        // Populated: every field present and well-formed.
        let opts = modal_options(vec![
            ("n_modes".to_string(), Value::Int(7)),
            ("tol".to_string(), Value::Real(1e-7)),
            ("max_iters".to_string(), Value::Int(50)),
            ("sigma".to_string(), Value::Real(2.5)),
        ]);
        assert_eq!(extract_eigen_knobs(&opts), (7, 1e-7, 50, 2.5));

        // Missing fields → defaults (10, 1e-9, 200, 0.0).
        assert_eq!(
            extract_eigen_knobs(&modal_options(vec![])),
            (10, 1e-9, 200, 0.0)
        );

        // Malformed: non-positive n_modes clamps to ≥ 1; non-positive tol and
        // non-finite sigma fall back to their defaults.
        let bad = modal_options(vec![
            ("n_modes".to_string(), Value::Int(0)),
            ("tol".to_string(), Value::Real(-1.0)),
            ("sigma".to_string(), Value::Real(f64::NAN)),
        ]);
        assert_eq!(extract_eigen_knobs(&bad), (1, 1e-9, 200, 0.0));

        // Non-StructureInstance → all defaults.
        assert_eq!(extract_eigen_knobs(&Value::Undef), (10, 1e-9, 200, 0.0));
    }

    /// Amendment (suggestion 2): `extract_reference_direction` normalizes the
    /// vector and falls back to the bending default `[0,0,1]` for missing /
    /// zero-norm / non-struct inputs.
    #[test]
    fn extract_reference_direction_normalizes_and_falls_back() {
        let dir = |x: f64, y: f64, z: f64| {
            modal_options(vec![(
                "reference_direction".to_string(),
                Value::Vector(vec![Value::Real(x), Value::Real(y), Value::Real(z)]),
            )])
        };

        // Non-unit input is normalized to a unit vector.
        let got = extract_reference_direction(&dir(3.0, 0.0, 0.0));
        assert!((got[0] - 1.0).abs() < 1e-12 && got[1] == 0.0 && got[2] == 0.0);
        let got = extract_reference_direction(&dir(0.0, 0.0, 2.0));
        assert!(got[0] == 0.0 && got[1] == 0.0 && (got[2] - 1.0).abs() < 1e-12);

        // Zero-norm → bending default; missing field → default; non-struct → default.
        assert_eq!(
            extract_reference_direction(&dir(0.0, 0.0, 0.0)),
            [0.0, 0.0, 1.0]
        );
        assert_eq!(
            extract_reference_direction(&modal_options(vec![])),
            [0.0, 0.0, 1.0]
        );
        assert_eq!(extract_reference_direction(&Value::Undef), [0.0, 0.0, 1.0]);
    }

    /// Amendment (suggestion 2): `extract_damping` returns the Rayleigh
    /// coefficients only for a `RayleighDamping` instance; `NoDamping`, a missing
    /// field, and a non-struct all read as the undamped `(0, 0)`.
    #[test]
    fn extract_damping_discriminates_rayleigh_from_none() {
        let damped = modal_options(vec![("damping".to_string(), rayleigh_damping(0.5, 1e-6))]);
        assert_eq!(extract_damping(&damped), (0.5, 1e-6));

        let nodamp = modal_options(vec![(
            "damping".to_string(),
            struct_instance("NoDamping", vec![]),
        )]);
        assert_eq!(extract_damping(&nodamp), (0.0, 0.0));

        assert_eq!(extract_damping(&modal_options(vec![])), (0.0, 0.0));
        assert_eq!(extract_damping(&Value::Undef), (0.0, 0.0));
    }

    /// Amendment (suggestion 2): `build_dirichlet_bcs` selects the pin-pin
    /// realization iff BOTH beam-axis end faces are named, otherwise clamps the
    /// named face(s).
    #[test]
    fn build_dirichlet_bcs_selects_pin_pin_vs_clamp() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;
        let mesh = build_beam_mesh(length, width, height);
        let eps = 1e-9_f64;
        let on_x_min = |n: usize| mesh.nodes[n][0] <= eps;
        let on_end = |n: usize| mesh.nodes[n][0] <= eps || mesh.nodes[n][0] >= length - eps;

        // (a) Both x_min AND x_max named → pin-pin: some end-face node has ONLY
        //     its Z DOF constrained (X and Y free) — impossible under a full
        //     clamp, which constrains all three.
        let pin_opts = modal_options(vec![(
            "boundary_conditions".to_string(),
            Value::List(vec![fixed_support("x_min"), fixed_support("x_max")]),
        )]);
        let pin_set: std::collections::HashSet<usize> =
            build_dirichlet_bcs(&pin_opts, &mesh.nodes, length, width, height)
                .iter()
                .map(|b| b.dof)
                .collect();
        let z_only_end_node = (0..mesh.nodes.len()).any(|n| {
            on_end(n)
                && pin_set.contains(&(3 * n + 2))
                && !pin_set.contains(&(3 * n))
                && !pin_set.contains(&(3 * n + 1))
        });
        assert!(
            z_only_end_node,
            "pin-pin must leave an end-face node with only Z constrained"
        );

        // (b) Only x_min named → clamp: every x_min node has all three DOFs.
        let clamp_opts = modal_options(vec![(
            "boundary_conditions".to_string(),
            Value::List(vec![fixed_support("x_min")]),
        )]);
        let clamp_set: std::collections::HashSet<usize> =
            build_dirichlet_bcs(&clamp_opts, &mesh.nodes, length, width, height)
                .iter()
                .map(|b| b.dof)
                .collect();
        let all_x_min_clamped = (0..mesh.nodes.len()).filter(|&n| on_x_min(n)).all(|n| {
            clamp_set.contains(&(3 * n))
                && clamp_set.contains(&(3 * n + 1))
                && clamp_set.contains(&(3 * n + 2))
        });
        assert!(
            all_x_min_clamped,
            "clamp realization must constrain all three DOFs on every x_min node",
        );
    }

    /// Amendment (suggestion 2): `simply_supported_pin_pin_bcs` pins Z on every
    /// end-face node and adds exactly the three minimal anchors (1 axial X +
    /// 2 lateral Y) at the end-face neutral-axis nodes — the configuration that
    /// yields the simply-supported `(nπ)²` family rather than fixed-fixed.
    #[test]
    fn simply_supported_pin_pin_bcs_places_minimal_anchors() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;
        let mesh = build_beam_mesh(length, width, height);
        let eps = 1e-9_f64;

        let bcs = simply_supported_pin_pin_bcs(&mesh.nodes, length, height);

        // Count constraints per axis (dof % 3): X = axial anchor, Y = lateral
        // anchors, Z = simple supports.
        let (mut nx, mut ny, mut nz) = (0usize, 0usize, 0usize);
        for b in &bcs {
            match b.dof % 3 {
                0 => nx += 1,
                1 => ny += 1,
                _ => nz += 1,
            }
        }
        assert_eq!(nx, 1, "expected exactly one X (axial) anchor");
        assert_eq!(ny, 2, "expected exactly two Y (lateral) anchors");

        let n_end_nodes = (0..mesh.nodes.len())
            .filter(|&n| mesh.nodes[n][0] <= eps || mesh.nodes[n][0] >= length - eps)
            .count();
        assert_eq!(nz, n_end_nodes, "Z must be pinned on every end-face node");
    }

    /// Amendment (suggestion 2): `solve_modal_analysis_trampoline` happy path — a
    /// clamped steel beam with a `RayleighDamping` option yields a well-shaped
    /// `ModalResult` (non-empty modes, positive matrix norms, ascending finite
    /// frequencies) whose per-mode `damping_ratio` matches the Rayleigh formula
    /// ζ = (α + β·ω²)/(2ω) — exercising the trampoline shaping the e2e tests
    /// (steps 15/17) cover only release-gated and end-to-end.
    #[test]
    fn trampoline_shapes_modal_result_with_rayleigh_damping() {
        let (alpha, beta) = (0.5_f64, 1e-6_f64);
        let value_inputs = vec![
            material_with_density(Some(STEEL_DENSITY)),
            length_scalar(0.02), // length
            length_scalar(0.05), // width
            length_scalar(0.1),  // height
            modal_options(vec![
                ("n_modes".to_string(), Value::Int(3)),
                (
                    "boundary_conditions".to_string(),
                    Value::List(vec![fixed_support("x_min")]),
                ),
                ("damping".to_string(), rayleigh_damping(alpha, beta)),
                (
                    "reference_direction".to_string(),
                    Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]),
                ),
            ]),
        ];

        let outcome = solve_modal_analysis_trampoline(
            &value_inputs,
            &[],
            &Value::Undef,
            None,
            &CancellationHandle::new(),
        );

        let ComputeOutcome::Completed {
            result,
            diagnostics,
            ..
        } = outcome
        else {
            panic!("expected a Completed outcome");
        };
        // A well-constrained clamped beam produces no Error diagnostics.
        assert!(
            !diagnostics.iter().any(|d| d.severity == Severity::Error),
            "clamped beam must not produce Error diagnostics; got {diagnostics:?}",
        );

        let data = match &result {
            Value::StructureInstance(d) => d,
            other => panic!("expected a ModalResult StructureInstance, got {other:?}"),
        };
        assert_eq!(data.type_name, "ModalResult");

        // Matrix-norm diagnostics are positive (a real assembly ran).
        for field in ["mass_matrix_norm", "stiffness_matrix_norm"] {
            match data.fields.get(&field.to_string()) {
                Some(Value::Real(v)) => assert!(*v > 0.0, "{field} must be > 0; got {v}"),
                other => panic!("{field} must be a positive Real; got {other:?}"),
            }
        }

        let modes = match data.fields.get("modes") {
            Some(Value::List(m)) => m,
            other => panic!("ModalResult.modes must be a List; got {other:?}"),
        };
        assert!(!modes.is_empty(), "happy-path solve must return ≥ 1 mode");

        // Each mode is well-shaped; frequencies finite/positive/ascending; the
        // damping_ratio matches the Rayleigh formula for that mode's ω.
        let mut prev_f = f64::NEG_INFINITY;
        for (i, mode) in modes.iter().enumerate() {
            let m = match mode {
                Value::StructureInstance(d) => d,
                other => panic!("mode {i} must be a Mode StructureInstance; got {other:?}"),
            };
            assert_eq!(m.type_name, "Mode");

            // `Mode.frequency : Frequency` (modal_analysis.ri, task 4548) — the
            // producer emits a dimensioned `Value::Scalar { FREQUENCY }`, not a
            // bare `Value::Real`. Match the variant explicitly and assert the
            // dimension is FREQUENCY (Hz = s⁻¹) before extracting si_value.
            let f = match m.fields.get("frequency") {
                Some(Value::Scalar {
                    si_value,
                    dimension,
                }) if *dimension == DimensionVector::FREQUENCY => *si_value,
                other => {
                    panic!("mode {i} frequency must be Scalar<Frequency>; got {other:?}")
                }
            };
            assert!(
                f.is_finite() && f > 0.0,
                "mode {i} frequency {f} must be finite > 0"
            );
            assert!(
                f >= prev_f,
                "modes must be ascending by frequency: {f} < {prev_f}"
            );
            prev_f = f;

            let omega = 2.0 * std::f64::consts::PI * f;
            let expected = rayleigh_damping_ratio(alpha, beta, omega);
            assert!(
                expected > 0.0,
                "fixture (α, β) must give nonzero ζ (≠ NoDamping)"
            );
            match m.fields.get("damping_ratio") {
                Some(Value::Real(zeta)) => assert!(
                    (zeta - expected).abs() < 1e-12,
                    "mode {i} damping_ratio {zeta} != Rayleigh {expected}",
                ),
                other => panic!("mode {i} damping_ratio must be Real; got {other:?}"),
            }
            assert!(
                matches!(m.fields.get("participation_mass"), Some(Value::Real(_))),
                "mode {i} participation_mass must be Real",
            );
        }
    }

    /// step-1 (RED → GREEN in step-2): trampoline serializes Mode.shape as a
    /// per-node `Value::List<Vector3<Dimensionless>>`.
    ///
    /// Structural checks: shape is `Value::List` of length `n_nodes`; each
    /// element is `Value::Vector([Real, Real, Real])`; clamped-face nodes have
    /// exactly (0.0, 0.0, 0.0); at least one component is nonzero (mode
    /// carries real data, not the Undef placeholder).
    #[test]
    fn trampoline_serializes_mode_shape_as_per_node_vectors() {
        let value_inputs = vec![
            material_with_density(Some(STEEL_DENSITY)),
            length_scalar(0.02),
            length_scalar(0.05),
            length_scalar(0.1),
            modal_options(vec![
                ("n_modes".to_string(), Value::Int(3)),
                (
                    "boundary_conditions".to_string(),
                    Value::List(vec![fixed_support("x_min")]),
                ),
                (
                    "reference_direction".to_string(),
                    Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]),
                ),
            ]),
        ];
        let outcome = solve_modal_analysis_trampoline(
            &value_inputs,
            &[],
            &Value::Undef,
            None,
            &CancellationHandle::new(),
        );
        let ComputeOutcome::Completed { result, .. } = outcome else {
            panic!("expected a Completed outcome");
        };
        let data = match &result {
            Value::StructureInstance(d) => d,
            other => panic!("expected a ModalResult StructureInstance; got {other:?}"),
        };
        let modes = match data.fields.get("modes") {
            Some(Value::List(m)) => m,
            other => panic!("ModalResult.modes must be a List; got {other:?}"),
        };
        assert!(!modes.is_empty(), "happy-path solve must return ≥ 1 mode");

        let mesh = build_beam_mesh(0.02, 0.05, 0.1);
        let n_nodes = mesh.nodes.len();

        for (i, mode) in modes.iter().enumerate() {
            let m = match mode {
                Value::StructureInstance(d) => d,
                other => panic!("mode {i} must be a Mode StructureInstance; got {other:?}"),
            };
            let shape = match m.fields.get("shape") {
                Some(v) => v,
                None => panic!("mode {i} missing 'shape' field"),
            };
            let nodes = match shape {
                Value::List(ns) => ns,
                other => panic!("mode {i} shape must be Value::List; got {other:?}"),
            };
            assert_eq!(
                nodes.len(),
                n_nodes,
                "mode {i} shape must have {n_nodes} per-node vectors; got {}",
                nodes.len(),
            );
            let mut any_nonzero = false;
            for (j, node_val) in nodes.iter().enumerate() {
                let comps = match node_val {
                    Value::Vector(c) => c,
                    other => {
                        panic!("mode {i} shape[{j}] must be Value::Vector; got {other:?}")
                    }
                };
                assert_eq!(
                    comps.len(),
                    3,
                    "mode {i} shape[{j}] Vector must have 3 components; got {}",
                    comps.len(),
                );
                for (k, comp) in comps.iter().enumerate() {
                    assert!(
                        matches!(comp, Value::Real(_)),
                        "mode {i} shape[{j}][{k}] must be Value::Real; got {comp:?}",
                    );
                }
                // Clamped x_min face nodes must be exactly (0.0, 0.0, 0.0).
                if mesh.nodes[j][0] <= 1e-9 {
                    for (k, comp) in comps.iter().enumerate() {
                        let Value::Real(v) = comp else { unreachable!() };
                        assert_eq!(
                            *v, 0.0,
                            "mode {i} shape[{j}][{k}] on clamped face must be exactly 0.0; got {v}",
                        );
                    }
                }
                for comp in comps.iter() {
                    if let Value::Real(v) = comp
                        && *v != 0.0
                    {
                        any_nonzero = true;
                    }
                }
            }
            assert!(
                any_nonzero,
                "mode {i} shape must have ≥ 1 nonzero component (not Undef / all-zero)",
            );
        }
    }

    /// step-1 (RED → GREEN in step-2): trampoline's serialized `modes[0].shape`
    /// equals `solve_modal_core` phi_full[0] reshaped to `List<Vector3>`.
    ///
    /// Both paths use the same deterministic dense eigensolver with identical
    /// mesh/BCs/opts/material — exact `Value` equality holds (no tolerance).
    #[test]
    fn trampoline_mode_shape_matches_core_phi_full() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;

        // Oracle: direct solve_modal_core call with the same inputs the trampoline uses.
        let mesh = build_beam_mesh(length, width, height);
        let (bcs, _) = clamp_x_min_face(&mesh.nodes);
        let eigen_opts = EigenSolverOptions {
            n_modes: 3,
            tol: 1e-9,
            max_iters: 200,
            sigma: 0.0,
        };
        let core = solve_modal_core(
            ModalMesh::P1(&mesh),
            STEEL_DENSITY,
            &steel(),
            [0.0, 0.0, 1.0],
            &bcs,
            &eigen_opts,
        );
        assert!(
            !core.phi_full.is_empty(),
            "oracle must return ≥ 1 phi_full vector"
        );

        // Reshape phi_full[0] into the expected List<Vector3<Dimensionless>>.
        let expected = Value::List(
            core.phi_full[0]
                .chunks_exact(3)
                .map(|c| {
                    Value::Vector(vec![
                        Value::Real(c[0]),
                        Value::Real(c[1]),
                        Value::Real(c[2]),
                    ])
                })
                .collect(),
        );

        // Trampoline call with equivalent value_inputs.
        let value_inputs = vec![
            material_with_density(Some(STEEL_DENSITY)),
            length_scalar(length),
            length_scalar(width),
            length_scalar(height),
            modal_options(vec![
                ("n_modes".to_string(), Value::Int(3)),
                (
                    "boundary_conditions".to_string(),
                    Value::List(vec![fixed_support("x_min")]),
                ),
                (
                    "reference_direction".to_string(),
                    Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]),
                ),
            ]),
        ];
        let outcome = solve_modal_analysis_trampoline(
            &value_inputs,
            &[],
            &Value::Undef,
            None,
            &CancellationHandle::new(),
        );
        let ComputeOutcome::Completed { result, .. } = outcome else {
            panic!("expected a Completed outcome");
        };
        let data = match &result {
            Value::StructureInstance(d) => d,
            other => panic!("expected ModalResult StructureInstance; got {other:?}"),
        };
        let modes = match data.fields.get("modes") {
            Some(Value::List(m)) => m,
            other => panic!("ModalResult.modes must be a List; got {other:?}"),
        };
        assert!(!modes.is_empty(), "trampoline must return ≥ 1 mode");
        let mode0 = match &modes[0] {
            Value::StructureInstance(d) => d,
            other => panic!("modes[0] must be a Mode StructureInstance; got {other:?}"),
        };
        let got_shape = match mode0.fields.get("shape") {
            Some(v) => v.clone(),
            None => panic!("modes[0] missing 'shape' field"),
        };
        assert_eq!(
            got_shape, expected,
            "modes[0].shape must equal solve_modal_core phi_full[0] reshaped to List<Vector3>",
        );
    }

    /// step-9 (RED → GREEN in step-10): the trampoline honors
    /// `ModalOptions.element_order` end-to-end.
    ///
    /// `solve_modal_analysis_trampoline` must read the `element_order` enum field
    /// and dispatch `solve_modal_core` at that order. An `ElementOrder.P2` option
    /// promotes the beam mesh (inserting edge-midpoint nodes) BEFORE assembling
    /// K/M, so the serialized `Mode.shape` carries one per-node `Vector3` for every
    /// PROMOTED node — strictly more than the P1 node count. A missing
    /// `element_order` field must keep the P1 path (back-compat), so its shape
    /// length equals the P1 mesh node count.
    ///
    /// The two orders are distinguished by the serialized mode-shape length
    /// (= node count): P2 > P1. Both must Complete with a non-empty modes list and
    /// no Error diagnostics (the exact P2 mass is PD, so the eigensolve runs clean)
    /// — i.e. the P2 path genuinely ran rather than silently falling back to P1.
    ///
    /// RED: the trampoline hard-codes `ElementOrder::P1` and ignores the field, so
    /// the `element_order = P2` run produces the SAME (P1) node count as the
    /// default run — the `p2 == promoted` / `p2 > p1` assertions fail until step 10
    /// wires `extract_element_order` (and the promoted-node-set BC realization)
    /// through.
    #[test]
    fn trampoline_honors_element_order_p2() {
        let length = 0.02_f64; // X — beam axis (short → coarse promoted mesh)
        let width = 0.05_f64; // Y — width
        let height = 0.1_f64; // Z — bending axis

        // Expected node counts, via the SAME shared helpers the trampoline /
        // solve_modal_core use, so they track any mesh change: P1 = the beam-mesh
        // node count; P2 = the promoted (edge-midpoint-inserted) node count.
        let mesh = build_beam_mesh(length, width, height);
        let n_nodes_p1 = mesh.nodes.len();
        let (nodes_p2, _tets_p2) = promote_tets_to_p2(&mesh.nodes, &mesh.tets);
        let n_nodes_p2 = nodes_p2.len();
        assert!(
            n_nodes_p2 > n_nodes_p1,
            "P2 promotion must add nodes for the fixture to discriminate the order: \
             {n_nodes_p2} !> {n_nodes_p1}",
        );

        // Shared cantilever fixture inputs; only the `element_order` field differs.
        let make_inputs = |order_field: Option<Value>| {
            let mut opt_fields = vec![
                ("n_modes".to_string(), Value::Int(3)),
                (
                    "boundary_conditions".to_string(),
                    Value::List(vec![fixed_support("x_min")]),
                ),
                (
                    "reference_direction".to_string(),
                    Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]),
                ),
            ];
            if let Some(order) = order_field {
                opt_fields.push(("element_order".to_string(), order));
            }
            vec![
                material_with_density(Some(STEEL_DENSITY)),
                length_scalar(length),
                length_scalar(width),
                length_scalar(height),
                modal_options(opt_fields),
            ]
        };

        // Run the trampoline and return the serialized `modes[0].shape` length
        // (= the solve's node count for that order), asserting along the way that
        // the outcome Completed cleanly with a non-empty modes list and no Error
        // diagnostics (the P2 path actually ran, not a degenerate short-circuit).
        let run = |inputs: Vec<Value>| -> usize {
            let outcome = solve_modal_analysis_trampoline(
                &inputs,
                &[],
                &Value::Undef,
                None,
                &CancellationHandle::new(),
            );
            let ComputeOutcome::Completed {
                result,
                diagnostics,
                ..
            } = outcome
            else {
                panic!("expected a Completed outcome");
            };
            assert!(
                !diagnostics.iter().any(|d| d.severity == Severity::Error),
                "clamped beam must not produce Error diagnostics; got {diagnostics:?}",
            );
            let data = match &result {
                Value::StructureInstance(d) => d,
                other => panic!("expected a ModalResult StructureInstance; got {other:?}"),
            };
            let modes = match data.fields.get("modes") {
                Some(Value::List(m)) => m,
                other => panic!("ModalResult.modes must be a List; got {other:?}"),
            };
            assert!(!modes.is_empty(), "solve must return ≥ 1 mode");
            let mode0 = match &modes[0] {
                Value::StructureInstance(d) => d,
                other => panic!("modes[0] must be a Mode StructureInstance; got {other:?}"),
            };
            match mode0.fields.get("shape") {
                Some(Value::List(nodes)) => nodes.len(),
                other => panic!("modes[0].shape must be a List; got {other:?}"),
            }
        };

        // (a) `element_order = ElementOrder.P2` → the P2 path → promoted node count.
        let p2_order = Value::Enum {
            type_name: "ElementOrder".to_string(),
            variant: "P2".to_string(),
        };
        let p2_shape_len = run(make_inputs(Some(p2_order)));

        // (b) absent `element_order` → the P1 path (back-compat) → P1 node count.
        let p1_shape_len = run(make_inputs(None));

        assert_eq!(
            p2_shape_len, n_nodes_p2,
            "element_order=P2 must run the P2 path (promoted node count {n_nodes_p2}); \
             got a {p2_shape_len}-node mode shape",
        );
        assert_eq!(
            p1_shape_len, n_nodes_p1,
            "absent element_order must keep the P1 path (node count {n_nodes_p1}); \
             got a {p1_shape_len}-node mode shape",
        );

        // (c) the two paths are observably distinct — proving the field switched the
        //     element order rather than both falling through to a single default.
        assert!(
            p2_shape_len > p1_shape_len,
            "P2 mode shape ({p2_shape_len} nodes) must exceed P1 ({p1_shape_len}); \
             the trampoline must honor ModalOptions.element_order",
        );
    }

    // -- task ι: transient_response / displacement_at trampoline fixtures ------

    /// A `Vector3<Dimensionless>` runtime value — the per-node `Value::Vector(
    /// [Real;3])` encoding `mode_shape_value` and `read_vec3` traffic in.
    fn vec3_value(v: [f64; 3]) -> Value {
        Value::Vector(vec![
            Value::Real(v[0]),
            Value::Real(v[1]),
            Value::Real(v[2]),
        ])
    }

    /// A `Time` scalar (SI seconds), as the trampoline reads `t_start/t_end/dt`.
    fn time_scalar(s: f64) -> Value {
        Value::Scalar {
            si_value: s,
            dimension: DimensionVector::TIME,
        }
    }

    /// Build a synthetic `Mode` StructureInstance with a known frequency (Hz),
    /// damping ratio, and full-DOF mode shape `phi_full` (length 3·n_nodes,
    /// serialized via the production `mode_shape_value`). `participation_mass` is
    /// a placeholder — the transient trampolines never read it.
    fn mode_struct(frequency_hz: f64, damping_ratio: f64, phi_full: &[f64]) -> Value {
        struct_instance(
            "Mode",
            vec![
                // Faithful to the producer: `Mode.frequency : Frequency`
                // emits a dimensioned Scalar (task 4548), not a bare Real.
                (
                    "frequency".to_string(),
                    Value::Scalar {
                        si_value: frequency_hz,
                        dimension: DimensionVector::FREQUENCY,
                    },
                ),
                ("shape".to_string(), mode_shape_value(phi_full)),
                ("participation_mass".to_string(), Value::Real(0.0)),
                ("damping_ratio".to_string(), Value::Real(damping_ratio)),
            ],
        )
    }

    /// Build a synthetic `ModalResult` carrying the given `modes` — the only field
    /// the transient trampolines read; the rest mirror the degenerate shape.
    fn modal_result_with_modes(modes: Vec<Value>) -> Value {
        struct_instance(
            "ModalResult",
            vec![
                ("part".to_string(), Value::String(String::new())),
                ("modes".to_string(), Value::List(modes)),
                ("boundary_conditions".to_string(), Value::List(Vec::new())),
                ("damping".to_string(), Value::Undef),
                ("mass_matrix_norm".to_string(), Value::Real(0.0)),
                ("stiffness_matrix_norm".to_string(), Value::Real(0.0)),
            ],
        )
    }

    /// A `StepForce { at, direction, magnitude, start_time }` instance.
    fn step_force(at: &str, dir: [f64; 3], magnitude_n: f64, start_time_s: f64) -> Value {
        struct_instance(
            "StepForce",
            vec![
                ("at".to_string(), Value::String(at.to_string())),
                ("direction".to_string(), vec3_value(dir)),
                (
                    "magnitude".to_string(),
                    Value::Scalar {
                        si_value: magnitude_n,
                        dimension: DimensionVector::FORCE,
                    },
                ),
                ("start_time".to_string(), time_scalar(start_time_s)),
            ],
        )
    }

    /// A `ForcingTimeHistory { part, sources }` instance.
    fn forcing_history(sources: Vec<Value>) -> Value {
        struct_instance(
            "ForcingTimeHistory",
            vec![
                ("part".to_string(), Value::String(String::new())),
                ("sources".to_string(), Value::List(sources)),
            ],
        )
    }

    /// A synthetic `DisplacementTimeHistory { part, modal_result, t_samples,
    /// mode_coords }` instance — the fields `displacement_at` reads. `t_samples_s`
    /// is a List<Time> (SI seconds); `mode_coords` is the n_modes × n_times modal
    /// coordinate matrix, shaped as a List<List<Real>>.
    fn displacement_history(
        modal_result: Value,
        t_samples_s: &[f64],
        mode_coords: &[Vec<f64>],
    ) -> Value {
        let t_samples = Value::List(t_samples_s.iter().map(|&s| time_scalar(s)).collect());
        let coords = Value::List(
            mode_coords
                .iter()
                .map(|series| Value::List(series.iter().map(|&c| Value::Real(c)).collect()))
                .collect(),
        );
        struct_instance(
            "DisplacementTimeHistory",
            vec![
                ("part".to_string(), Value::String(String::new())),
                ("modal_result".to_string(), modal_result),
                ("t_samples".to_string(), t_samples),
                ("mode_coords".to_string(), coords),
            ],
        )
    }

    /// step-11 (RED → GREEN in step-12): the `transient_response` trampoline
    /// happy path produces a well-shaped `DisplacementTimeHistory`.
    ///
    /// Build a synthetic 2-mode ModalResult (known frequency / damping_ratio /
    /// per-node Φ shape) and a ForcingTimeHistory carrying one StepForce at the
    /// fundamental antinode ("tip"), then call the trampoline over a uniform grid.
    /// Assert the returned DisplacementTimeHistory:
    ///   - `t_samples` length == the `uniform_time_grid` count,
    ///   - `mode_coords` outer length == n_modes (2),
    ///   - each `mode_coords` inner length == n_times,
    ///   - `modal_result` is echoed (a 2-mode ModalResult),
    ///   - every modal coordinate (and time sample) is finite.
    ///
    /// RED: the step-10 stub returns a degenerate empty history (0 samples / 0
    /// modes), so the length assertions fail.
    #[test]
    fn transient_response_happy_path_shapes_displacement_history() {
        // Two modes, each a 3-node shape; node 2 is the fundamental antinode
        // (max ‖Φ₀‖), so a "tip" StepForce projects onto it with a nonzero coeff.
        let mode0 = mode_struct(40.0, 0.01, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 1.0]);
        let mode1 = mode_struct(250.0, 0.02, &[0.0, 0.0, 0.0, 0.0, 0.0, -0.7, 0.0, 0.0, 0.4]);
        let modal_result = modal_result_with_modes(vec![mode0, mode1]);

        let forcing = forcing_history(vec![step_force("tip", [0.0, 0.0, 1.0], 10.0, 0.0)]);

        let (t_start, t_end, dt) = (0.0_f64, 0.1_f64, 0.005_f64);
        let value_inputs = vec![
            modal_result,
            forcing,
            time_scalar(t_start),
            time_scalar(t_end),
            time_scalar(dt),
        ];

        let outcome = solve_transient_response_trampoline(
            &value_inputs,
            &[],
            &Value::Undef,
            None,
            &CancellationHandle::new(),
        );
        let ComputeOutcome::Completed {
            result,
            diagnostics,
            ..
        } = outcome
        else {
            panic!("expected a Completed outcome");
        };
        // The happy path (non-empty forcing) emits no Error diagnostics.
        assert!(
            !diagnostics.iter().any(|d| d.severity == Severity::Error),
            "happy-path transient_response must not emit Error diagnostics; got {diagnostics:?}",
        );

        let data = match &result {
            Value::StructureInstance(d) => d,
            other => panic!("expected a DisplacementTimeHistory StructureInstance; got {other:?}"),
        };
        assert_eq!(data.type_name, "DisplacementTimeHistory");

        let n_times = uniform_time_grid(t_start, t_end, dt).len();
        assert!(
            n_times > 1,
            "fixture grid must have > 1 sample (got {n_times})"
        );

        // t_samples: one finite Time scalar per grid point.
        match data.fields.get("t_samples") {
            Some(Value::List(ts)) => {
                assert_eq!(
                    ts.len(),
                    n_times,
                    "t_samples length must equal the grid count"
                );
                assert!(
                    ts.iter().all(|v| read_scalar_si(v).is_finite()),
                    "every t_sample must be finite",
                );
            }
            other => panic!("t_samples must be a Value::List; got {other:?}"),
        }

        // mode_coords: outer length == n_modes, each inner length == n_times, finite.
        match data.fields.get("mode_coords") {
            Some(Value::List(modes)) => {
                assert_eq!(
                    modes.len(),
                    2,
                    "mode_coords outer length must equal n_modes"
                );
                for (i, coords) in modes.iter().enumerate() {
                    match coords {
                        Value::List(series) => {
                            assert_eq!(
                                series.len(),
                                n_times,
                                "mode_coords[{i}] inner length must equal n_times",
                            );
                            assert!(
                                series.iter().all(|v| read_scalar_si(v).is_finite()),
                                "mode_coords[{i}] must be all finite",
                            );
                        }
                        other => panic!("mode_coords[{i}] must be a Value::List; got {other:?}"),
                    }
                }
            }
            other => panic!("mode_coords must be a Value::List; got {other:?}"),
        }

        // modal_result echoed: a ModalResult StructureInstance with the 2 modes.
        match data.fields.get("modal_result") {
            Some(Value::StructureInstance(mr)) => {
                assert_eq!(mr.type_name, "ModalResult", "echoed modal_result type");
                match mr.fields.get("modes") {
                    Some(Value::List(m)) => {
                        assert_eq!(
                            m.len(),
                            2,
                            "echoed modal_result must carry the 2 input modes"
                        )
                    }
                    other => panic!("echoed modal_result.modes must be a List; got {other:?}"),
                }
            }
            other => {
                panic!("modal_result must echo a ModalResult StructureInstance; got {other:?}")
            }
        }
    }

    /// step-13 (RED → GREEN in step-14): the `transient_response` trampoline's
    /// empty-forcing guard fires `E_TransientForcingMissing`.
    ///
    /// A `ForcingTimeHistory` whose `sources` list is empty is built directly
    /// (bypassing the `.ri` ctor's `sources.count > 0` constraint, which would
    /// otherwise reject it at construction). Even with a well-formed 2-mode
    /// ModalResult and a valid (non-empty) time grid, an empty forcing carries no
    /// load to project, so the trampoline must short-circuit to a *flagged*
    /// degenerate result rather than silently integrate a zero forcing.
    ///
    /// Assert the returned `Completed` outcome carries:
    ///   - an `Error`-severity diagnostic whose message contains
    ///     `"E_TransientForcingMissing"`, and
    ///   - a degenerate `DisplacementTimeHistory` with empty `t_samples` and
    ///     empty `mode_coords` (no transient was integrated).
    ///
    /// RED: step-12 integrates the zero forcing over the valid grid and returns a
    /// non-empty, un-flagged history (n_times `t_samples`, n_modes `mode_coords`),
    /// so both the diagnostic and the emptiness assertions fail.
    #[test]
    fn transient_response_empty_forcing_emits_forcing_missing() {
        // A well-formed 2-mode ModalResult — only the forcing is degenerate, so the
        // guard (not a missing modal result) must be what fires.
        let mode0 = mode_struct(40.0, 0.01, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 1.0]);
        let mode1 = mode_struct(250.0, 0.02, &[0.0, 0.0, 0.0, 0.0, 0.0, -0.7, 0.0, 0.0, 0.4]);
        let modal_result = modal_result_with_modes(vec![mode0, mode1]);

        // Empty sources, built directly — bypasses the ForcingTimeHistory ctor's
        // `sources.count > 0` constraint (an e2e cannot reach the trampoline here).
        let forcing = forcing_history(vec![]);

        // A valid, non-empty grid: the empty-grid floor must NOT be what fires, so
        // this test isolates the forcing guard specifically.
        let (t_start, t_end, dt) = (0.0_f64, 0.1_f64, 0.005_f64);
        assert!(
            uniform_time_grid(t_start, t_end, dt).len() > 1,
            "fixture grid must be non-empty so the empty-grid floor does not mask the forcing guard",
        );

        let value_inputs = vec![
            modal_result,
            forcing,
            time_scalar(t_start),
            time_scalar(t_end),
            time_scalar(dt),
        ];

        let outcome = solve_transient_response_trampoline(
            &value_inputs,
            &[],
            &Value::Undef,
            None,
            &CancellationHandle::new(),
        );
        let ComputeOutcome::Completed {
            result,
            diagnostics,
            ..
        } = outcome
        else {
            panic!("expected a Completed outcome");
        };

        // (a) an Error diagnostic identifies the missing-forcing condition.
        let has_err = diagnostics.iter().any(|d| {
            d.severity == Severity::Error && d.message.contains("E_TransientForcingMissing")
        });
        assert!(
            has_err,
            "expected an Error containing \"E_TransientForcingMissing\"; got {diagnostics:?}",
        );

        // (b) the result is a degenerate DisplacementTimeHistory: empty t_samples
        //     and empty mode_coords (no transient was integrated).
        let data = match &result {
            Value::StructureInstance(d) => d,
            other => panic!("expected a DisplacementTimeHistory StructureInstance; got {other:?}"),
        };
        assert_eq!(data.type_name, "DisplacementTimeHistory");
        match data.fields.get("t_samples") {
            Some(Value::List(ts)) => assert!(
                ts.is_empty(),
                "degenerate t_samples must be empty; got {} samples",
                ts.len(),
            ),
            other => panic!("t_samples must be a Value::List; got {other:?}"),
        }
        match data.fields.get("mode_coords") {
            Some(Value::List(mc)) => assert!(
                mc.is_empty(),
                "degenerate mode_coords must be empty; got {} modes",
                mc.len(),
            ),
            other => panic!("mode_coords must be a Value::List; got {other:?}"),
        }
    }

    /// Amendment (reviewer suggestion 3): a degenerate transient time grid
    /// (`dt ≤ 0` or `t_end < t_start` → no timesteps) is FLAGGED, not silent.
    ///
    /// With a well-formed 2-mode ModalResult and a non-empty forcing (so the
    /// empty-forcing guard does not fire first), a malformed `(t_start, t_end,
    /// dt)` must short-circuit to a *flagged* degenerate history: an `Error`
    /// diagnostic containing `"E_TransientTimeGridDegenerate"` plus an empty
    /// `DisplacementTimeHistory` — so swapped endpoints / `dt = 0` are observable
    /// rather than silently masked as an empty result.
    #[test]
    fn transient_response_degenerate_grid_is_flagged() {
        let mode0 = mode_struct(40.0, 0.01, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 1.0]);
        let mode1 = mode_struct(250.0, 0.02, &[0.0, 0.0, 0.0, 0.0, 0.0, -0.7, 0.0, 0.0, 0.4]);
        let modal_result = modal_result_with_modes(vec![mode0, mode1]);
        // Non-empty forcing so the E_TransientForcingMissing guard is NOT what fires.
        let forcing = forcing_history(vec![step_force("tip", [0.0, 0.0, 1.0], 10.0, 0.0)]);

        // Two degenerate triples: dt = 0, and swapped endpoints (t_end < t_start);
        // uniform_time_grid returns empty for both.
        for (t_start, t_end, dt) in [(0.0_f64, 0.1_f64, 0.0_f64), (0.1_f64, 0.0_f64, 0.005_f64)] {
            assert!(
                uniform_time_grid(t_start, t_end, dt).is_empty(),
                "fixture ({t_start}, {t_end}, {dt}) must yield an empty grid",
            );
            let value_inputs = vec![
                modal_result.clone(),
                forcing.clone(),
                time_scalar(t_start),
                time_scalar(t_end),
                time_scalar(dt),
            ];
            let outcome = solve_transient_response_trampoline(
                &value_inputs,
                &[],
                &Value::Undef,
                None,
                &CancellationHandle::new(),
            );
            let ComputeOutcome::Completed {
                result,
                diagnostics,
                ..
            } = outcome
            else {
                panic!("expected a Completed outcome");
            };
            assert!(
                diagnostics.iter().any(|d| {
                    d.severity == Severity::Error
                        && d.message.contains("E_TransientTimeGridDegenerate")
                }),
                "degenerate grid ({t_start}, {t_end}, {dt}) must emit an Error \
                 containing \"E_TransientTimeGridDegenerate\"; got {diagnostics:?}",
            );
            let data = match &result {
                Value::StructureInstance(d) => d,
                other => panic!("expected a DisplacementTimeHistory; got {other:?}"),
            };
            match data.fields.get("t_samples") {
                Some(Value::List(ts)) => {
                    assert!(ts.is_empty(), "degenerate grid must yield empty t_samples")
                }
                other => panic!("t_samples must be a Value::List; got {other:?}"),
            }
        }
    }

    /// Task 4131: a missing / degenerate `modal_result` input — either `Value::Undef`
    /// (upstream modal analysis failed) or a well-formed `ModalResult` struct with an
    /// empty `modes` list — is FLAGGED, not silent.
    ///
    /// With a valid non-empty forcing and a valid non-empty time grid (so neither the
    /// empty-forcing nor the degenerate-grid guard can fire), a degenerate
    /// `modal_result` must short-circuit to a *flagged* degenerate history: an `Error`
    /// diagnostic containing `"E_TransientModalResultMissing"` plus an empty
    /// `DisplacementTimeHistory` — so a silently zero-displacement result is never
    /// fabricated by summing over zero modes.
    ///
    /// Both degenerate shapes route through the same
    /// `modal_result_modes(&modal_result).is_empty()` branch.
    ///
    /// RED: the current trampoline loops over zero modes and returns an un-flagged
    /// empty history with NO diagnostic, so assertion (a) fails.
    #[test]
    fn transient_response_missing_modal_result_emits_modal_result_missing() {
        // Non-empty forcing — the empty-forcing guard must NOT fire.
        let forcing = forcing_history(vec![step_force("tip", [0.0, 0.0, 1.0], 10.0, 0.0)]);
        // Valid non-empty grid — the degenerate-grid guard must NOT fire.
        let (t_start, t_end, dt) = (0.0_f64, 0.1_f64, 0.005_f64);
        assert!(
            uniform_time_grid(t_start, t_end, dt).len() > 1,
            "fixture grid must be non-empty so neither the empty-forcing nor the \
             degenerate-grid guard masks the missing-modal-result guard",
        );

        // Four degenerate modal_result shapes: Undef (upstream node errored),
        // a well-formed ModalResult with an empty `modes` list, a ModalResult
        // whose `modes` field is absent, and a non-ModalResult struct. All four
        // route through the same `modal_result_modes(&modal_result).is_empty()`
        // branch and must emit the same E_TransientModalResultMissing diagnostic.
        let modal_result_no_modes_field = struct_instance(
            "ModalResult",
            vec![("part".to_string(), Value::String(String::new()))],
        );
        let non_modal_result_struct = struct_instance(
            "SomeOtherStruct",
            vec![("modes".to_string(), Value::List(vec![]))],
        );
        for (label, modal_result) in [
            ("Value::Undef", Value::Undef),
            (
                "modal_result_with_modes(vec![])",
                modal_result_with_modes(vec![]),
            ),
            (
                "ModalResult without modes field",
                modal_result_no_modes_field,
            ),
            ("non-ModalResult struct", non_modal_result_struct),
        ] {
            let value_inputs = vec![
                modal_result,
                forcing.clone(),
                time_scalar(t_start),
                time_scalar(t_end),
                time_scalar(dt),
            ];
            let outcome = solve_transient_response_trampoline(
                &value_inputs,
                &[],
                &Value::Undef,
                None,
                &CancellationHandle::new(),
            );
            let ComputeOutcome::Completed {
                result,
                diagnostics,
                ..
            } = outcome
            else {
                panic!("[{label}] expected a Completed outcome");
            };

            // (a) an Error diagnostic identifies the missing-modal-result condition.
            let has_err = diagnostics.iter().any(|d| {
                d.severity == Severity::Error && d.message.contains("E_TransientModalResultMissing")
            });
            assert!(
                has_err,
                "[{label}] expected an Error containing \"E_TransientModalResultMissing\"; \
                 got {diagnostics:?}",
            );

            // (b) the result is a degenerate DisplacementTimeHistory: empty t_samples
            //     and empty mode_coords (no transient was integrated).
            let data = match &result {
                Value::StructureInstance(d) => d,
                other => panic!(
                    "[{label}] expected a DisplacementTimeHistory StructureInstance; \
                     got {other:?}"
                ),
            };
            assert_eq!(data.type_name, "DisplacementTimeHistory");
            match data.fields.get("t_samples") {
                Some(Value::List(ts)) => assert!(
                    ts.is_empty(),
                    "[{label}] degenerate t_samples must be empty; got {} samples",
                    ts.len(),
                ),
                other => panic!("[{label}] t_samples must be a Value::List; got {other:?}"),
            }
            match data.fields.get("mode_coords") {
                Some(Value::List(mc)) => assert!(
                    mc.is_empty(),
                    "[{label}] degenerate mode_coords must be empty; got {} modes",
                    mc.len(),
                ),
                other => {
                    panic!("[{label}] mode_coords must be a Value::List; got {other:?}")
                }
            }
        }
    }

    /// step-15 (RED → GREEN in step-16): `displacement_at` reconstructs the exact
    /// Φ-projected single-location series u(tⱼ) = Σᵢ (Φᵢ[node]·dir)·mode_coords[i][j],
    /// returning a non-Undef `List<Real>` (PRD §5.2) — covering the task's
    /// "displacement_at returns the Φ-projected time history, not Undef" premise.
    ///
    /// A 2-mode DisplacementTimeHistory with known per-node Φ shapes and known
    /// mode_coords is queried along Z (the bending axis) at two locations:
    ///   - a NUMERIC "1" → explicit node index 1, and
    ///   - a NON-NUMERIC "tip" → the fundamental antinode (node 2, max ‖Φ₀‖).
    ///
    /// Each returns a finite `List<Real>` of length n_times equal to the
    /// closed-form reconstruction. The two cases resolve to DIFFERENT nodes
    /// (1 vs 2) and so yield different series — proving the resolver discriminates
    /// explicit-index from antinode.
    ///
    /// RED: the step-10 stub returns an empty list (length 0, not n_times).
    #[test]
    fn displacement_at_reconstructs_phi_projected_series() {
        // node 2 is the fundamental antinode (max ‖Φ₀‖); node 1 is a distinct,
        // lower-deflection node, so "1" and "tip" must give different series.
        let mode0 = mode_struct(40.0, 0.01, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 1.0]);
        let mode1 = mode_struct(250.0, 0.02, &[0.0, 0.0, 0.0, 0.0, 0.0, -0.7, 0.0, 0.0, 0.4]);
        let modal_result = modal_result_with_modes(vec![mode0, mode1]);

        let mc0 = vec![1.0, 2.0, 3.0, 4.0];
        let mc1 = vec![0.1, 0.2, 0.3, 0.4];
        let mode_coords = vec![mc0.clone(), mc1.clone()];
        let t_samples_s = [0.0, 0.01, 0.02, 0.03];
        let n_times = t_samples_s.len();
        let history = displacement_history(modal_result, &t_samples_s, &mode_coords);

        let dir = [0.0, 0.0, 1.0];

        // Invoke the trampoline for `location` and return the List<Real> as Vec<f64>.
        let query = |location: &str| -> Vec<f64> {
            let value_inputs = vec![
                history.clone(),
                Value::String(location.to_string()),
                vec3_value(dir),
            ];
            let outcome = displacement_at_trampoline(
                &value_inputs,
                &[],
                &Value::Undef,
                None,
                &CancellationHandle::new(),
            );
            let ComputeOutcome::Completed {
                result,
                diagnostics,
                ..
            } = outcome
            else {
                panic!("expected a Completed outcome");
            };
            assert!(
                !diagnostics.iter().any(|d| d.severity == Severity::Error),
                "displacement_at must not emit Error diagnostics; got {diagnostics:?}",
            );
            match result {
                Value::List(items) => {
                    assert!(
                        !items.is_empty(),
                        "displacement_at must not return an empty list"
                    );
                    assert_eq!(items.len(), n_times, "series length must equal n_times");
                    assert!(
                        items.iter().map(read_scalar_si).all(f64::is_finite),
                        "every reconstructed sample must be finite",
                    );
                    items.iter().map(read_scalar_si).collect()
                }
                other => panic!("displacement_at must return a Value::List(Real); got {other:?}"),
            }
        };

        // Closed-form expectation u[j] = c0·mc0[j] + c1·mc1[j] (same mode-order
        // summation as `reconstruct_series`).
        let expect = |c0: f64, c1: f64| -> Vec<f64> {
            (0..n_times)
                .map(|j| c0 * mc0[j] + c1 * mc1[j])
                .collect::<Vec<_>>()
        };

        // Case A — numeric "1" → node 1: c0 = Φ₀[1]·ẑ = 0.5, c1 = Φ₁[1]·ẑ = -0.7.
        let got_node1 = query("1");
        let want_node1 = expect(0.5, -0.7);
        for (j, (g, w)) in got_node1.iter().zip(want_node1.iter()).enumerate() {
            assert!(
                (g - w).abs() < 1e-12,
                "node-1 series[{j}]: got {g}, want {w}"
            );
        }

        // Case B — non-numeric "tip" → antinode node 2: c0 = Φ₀[2]·ẑ = 1.0,
        // c1 = Φ₁[2]·ẑ = 0.4.
        let got_tip = query("tip");
        let want_tip = expect(1.0, 0.4);
        for (j, (g, w)) in got_tip.iter().zip(want_tip.iter()).enumerate() {
            assert!(
                (g - w).abs() < 1e-12,
                "tip (antinode) series[{j}]: got {g}, want {w}"
            );
        }

        // The two locations resolve to different nodes → observably different series.
        assert_ne!(
            got_node1, got_tip,
            "numeric index and antinode must resolve distinctly"
        );
    }

    /// Amendment (reviewer suggestion 4): pin the out-of-range numeric-index
    /// clamp as an intentional contract — an explicit index past the last node
    /// resolves to the last node (not an error, not node 0). Geometry-free
    /// resolution has no node table to reject against; this test makes the clamp
    /// a deliberate contract rather than incidental behavior.
    #[test]
    fn resolve_location_node_clamps_out_of_range_index() {
        // 3-node fundamental shape; node 2 is the antinode (max ‖Φ₀‖).
        let shape = [[0.0, 0.0, 0.0], [0.0, 0.0, 0.5], [0.0, 0.0, 1.0]];

        // In-range explicit indices map verbatim.
        assert_eq!(resolve_location_node("0", &shape), 0);
        assert_eq!(resolve_location_node("1", &shape), 1);
        assert_eq!(resolve_location_node("2", &shape), 2);

        // Out-of-range explicit indices clamp to the last node (n_nodes − 1 = 2).
        assert_eq!(
            resolve_location_node("3", &shape),
            2,
            "index just past the end clamps"
        );
        assert_eq!(
            resolve_location_node("99", &shape),
            2,
            "far out-of-range clamps"
        );

        // A non-numeric string resolves to the antinode (node 2 here), not the clamp path.
        assert_eq!(resolve_location_node("tip", &shape), 2);

        // Empty shape: the saturating clamp floor is 0 (no panic / underflow).
        assert_eq!(
            resolve_location_node("5", &[]),
            0,
            "empty shape clamps to 0 without panic"
        );
    }

    // ── task λ: run_transient_response cache-hit tests (RED → GREEN step-6) ──

    /// Build a `value_inputs` slice for `run_transient_response` / `solve_transient_response_trampoline`:
    /// `[modal_result, forcing, t_start, t_end, dt]`.
    fn transient_inputs(
        modal_result: Value,
        forcing: Value,
        t_start: f64,
        t_end: f64,
        dt: f64,
    ) -> Vec<Value> {
        vec![
            modal_result,
            forcing,
            time_scalar(t_start),
            time_scalar(t_end),
            time_scalar(dt),
        ]
    }

    /// Extract the per-mode `Vec<Vec<f64>>` modal-coordinate matrix from a
    /// `Completed` `DisplacementTimeHistory` outcome.
    fn mode_coords_from_outcome(outcome: &ComputeOutcome) -> Vec<Vec<f64>> {
        let ComputeOutcome::Completed { result, .. } = outcome else {
            panic!("expected Completed outcome");
        };
        match result {
            Value::StructureInstance(d) => match d.fields.get("mode_coords") {
                Some(Value::List(series)) => series.iter().map(read_real_list).collect(),
                other => panic!("mode_coords must be a Value::List; got {other:?}"),
            },
            other => panic!("expected a DisplacementTimeHistory StructureInstance; got {other:?}"),
        }
    }

    /// step-5λ (RED → GREEN in step-6): cache-hit test for `run_transient_response`.
    ///
    /// (1) Cold call (prior=None) → `reused_setup == false`, Completes, donates a
    ///     `new_warm_state` that `downcast_ref::<TransientCache>()` and whose
    ///     `key.matches` the inputs' `TransientCacheKey`.
    /// (2) Feed that cache back with SAME modal_result + t_range + dt but DIFFERENT
    ///     forcing → `reused_setup == true`, Completes, and the returned `mode_coords`
    ///     DIFFER from the cold call's (the ODE was re-integrated against the new
    ///     forcing) AND are within 1e-12 of a fresh cold run with the same f2 (the
    ///     cached setup produces the correct reference result).
    /// Discrimination: with the F1 cache as prior, (a) different mode `frequency_hz`
    /// → MISS (`reused_setup == false`); (b) different `dt` → MISS; (c) different
    /// `t_end` → MISS.
    ///
    /// RED: `run_transient_response` / `TransientTrampolineRun` / `TransientCache`
    /// are absent — fails to compile.
    #[test]
    fn run_transient_response_caches_and_reuses_setup() {
        // 2-mode ModalResult: 3-node shapes, node 2 is the tip antinode.
        let mode0 = mode_struct(40.0, 0.01, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 1.0]);
        let mode1 = mode_struct(250.0, 0.02, &[0.0, 0.0, 0.0, 0.0, 0.0, -0.7, 0.0, 0.0, 0.4]);
        let modal = modal_result_with_modes(vec![mode0, mode1]);

        let (t_start, t_end, dt) = (0.0_f64, 0.1_f64, 0.005_f64);
        let f1 = forcing_history(vec![step_force("tip", [0.0, 0.0, 1.0], 10.0, 0.0)]);
        let f2 = forcing_history(vec![step_force("tip", [0.0, 0.0, 1.0], 20.0, 0.0)]);

        // ── (1) cold call ──
        let handle = CancellationHandle::new();
        let cold = run_transient_response(
            &transient_inputs(modal.clone(), f1.clone(), t_start, t_end, dt),
            None,
            &handle,
        );
        assert!(
            !cold.reused_setup,
            "cold call (prior=None) must report reused_setup = false"
        );
        let ComputeOutcome::Completed {
            new_warm_state: ref ws1,
            ..
        } = cold.outcome
        else {
            panic!("cold call must Complete; got {:?}", cold.outcome);
        };
        // Donated warm state must be a TransientCache with the correct key.
        let cache1: &TransientCache = ws1
            .as_ref()
            .expect("cold Completed must donate a warm state")
            .downcast_ref::<TransientCache>()
            .expect("donated state must be a TransientCache");
        let expected_key =
            TransientCacheKey::new(t_start, t_end, dt, vec![(40.0, 0.01), (250.0, 0.02)]);
        assert!(
            cache1.key.matches(&expected_key),
            "donated cache key must match (t_start, t_end, dt, modes)",
        );
        let coords_f1 = mode_coords_from_outcome(&cold.outcome);

        // ── (2) warm re-call with different forcing → HIT, different result ──
        let prior = cache1.clone().into_opaque_state().0;
        let warm = run_transient_response(
            &transient_inputs(modal.clone(), f2.clone(), t_start, t_end, dt),
            Some(&prior),
            &handle,
        );
        assert!(
            warm.reused_setup,
            "call with same modal_result/t_range/dt but different forcing must HIT"
        );
        assert!(
            matches!(warm.outcome, ComputeOutcome::Completed { .. }),
            "warm hit must Completed; got {:?}",
            warm.outcome,
        );
        let coords_f2 = mode_coords_from_outcome(&warm.outcome);
        // The ODE was re-integrated against f2 (magnitude 20 vs 10): at least one
        // coordinate must differ.
        assert!(
            coords_f1
                .iter()
                .zip(coords_f2.iter())
                .any(|(v1, v2)| { v1.iter().zip(v2.iter()).any(|(a, b)| (a - b).abs() > 1e-15) }),
            "forcing-only re-integration must change the mode_coords",
        );

        // The warm-hit result must equal a fresh cold run with f2 (prior=None).
        // This pins that the cached setup (grid + prepared integrators) produces
        // the correct reference answer, not just a result that happens to differ
        // from the f1 cold run.
        let cold_f2 = run_transient_response(
            &transient_inputs(modal.clone(), f2.clone(), t_start, t_end, dt),
            None,
            &handle,
        );
        let coords_f2_cold = mode_coords_from_outcome(&cold_f2.outcome);
        assert_eq!(
            coords_f2.len(),
            coords_f2_cold.len(),
            "warm-hit and cold-f2 must produce the same number of modes",
        );
        for (i, (warm_mode, cold_mode)) in coords_f2.iter().zip(coords_f2_cold.iter()).enumerate() {
            assert_eq!(
                warm_mode.len(),
                cold_mode.len(),
                "mode {i}: warm-hit and cold-f2 must produce the same number of time steps",
            );
            for (j, (w, c)) in warm_mode.iter().zip(cold_mode.iter()).enumerate() {
                assert!(
                    (w - c).abs() < 1e-12,
                    "mode {i} step {j}: warm-hit coord {w:.6e} differs from cold-f2 ref \
                     {c:.6e} by {:.6e} (must be < 1e-12 — cached setup must reproduce \
                     the from-scratch result)",
                    (w - c).abs(),
                );
            }
        }

        // ── discrimination: cache MISS for changed inputs ──

        // (a) different frequency_hz → MISS
        let mode0_alt = mode_struct(45.0, 0.01, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 1.0]);
        let mode1_alt = mode_struct(250.0, 0.02, &[0.0, 0.0, 0.0, 0.0, 0.0, -0.7, 0.0, 0.0, 0.4]);
        let modal_alt_freq = modal_result_with_modes(vec![mode0_alt, mode1_alt]);
        let prior2 = cache1.clone().into_opaque_state().0;
        let miss_freq = run_transient_response(
            &transient_inputs(modal_alt_freq, f1.clone(), t_start, t_end, dt),
            Some(&prior2),
            &handle,
        );
        assert!(
            !miss_freq.reused_setup,
            "changed frequency_hz must be a cache MISS"
        );

        // (b) different dt → MISS
        let prior3 = cache1.clone().into_opaque_state().0;
        let miss_dt = run_transient_response(
            &transient_inputs(modal.clone(), f1.clone(), t_start, t_end, 0.010),
            Some(&prior3),
            &handle,
        );
        assert!(!miss_dt.reused_setup, "changed dt must be a cache MISS");

        // (c) different t_end → MISS
        let prior4 = cache1.clone().into_opaque_state().0;
        let miss_tend = run_transient_response(
            &transient_inputs(modal.clone(), f1.clone(), t_start, 0.2, dt),
            Some(&prior4),
            &handle,
        );
        assert!(
            !miss_tend.reused_setup,
            "changed t_end must be a cache MISS"
        );
    }

    // ── task λ step-7: cancellation tests (RED → GREEN in step-8) ────────────

    /// step-7λ (RED → GREEN in step-8): cooperative cancellation in
    /// `run_transient_response`.
    ///
    /// (a) A pre-cancelled `CancellationHandle` with valid 2-mode ModalResult
    ///     + non-empty forcing + valid grid yields `ComputeOutcome::Cancelled`
    ///     and `reused_setup == false` (a cancelled run reuses nothing).
    /// (b) Regression: a fresh handle still yields `ComputeOutcome::Completed`
    ///     (the added polls leave the happy path intact).
    ///
    /// RED: step-6's core ignores the handle at entry, so a pre-cancelled run
    /// still Completes (assertion (a) fails).
    #[test]
    fn run_transient_response_honors_cancellation() {
        let mode0 = mode_struct(40.0, 0.01, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 1.0]);
        let mode1 = mode_struct(250.0, 0.02, &[0.0, 0.0, 0.0, 0.0, 0.0, -0.7, 0.0, 0.0, 0.4]);
        let modal = modal_result_with_modes(vec![mode0, mode1]);
        let forcing = forcing_history(vec![step_force("tip", [0.0, 0.0, 1.0], 10.0, 0.0)]);
        let (t_start, t_end, dt) = (0.0_f64, 0.1_f64, 0.005_f64);
        let inputs = transient_inputs(modal, forcing, t_start, t_end, dt);

        // (a) pre-cancelled → Cancelled, reused_setup = false
        let cancelled = CancellationHandle::new();
        cancelled.cancel();
        let run = run_transient_response(&inputs, None, &cancelled);
        assert!(
            matches!(run.outcome, ComputeOutcome::Cancelled),
            "a pre-cancelled handle must yield ComputeOutcome::Cancelled; got {:?}",
            run.outcome,
        );
        assert!(
            !run.reused_setup,
            "a cancelled run must report reused_setup = false"
        );

        // (b) fresh handle → Completed (happy path unaffected by the added polls)
        let fresh = CancellationHandle::new();
        let ok = run_transient_response(&inputs, None, &fresh);
        assert!(
            matches!(ok.outcome, ComputeOutcome::Completed { .. }),
            "a fresh handle must Complete; got {:?}",
            ok.outcome,
        );
    }

    // ── task-4470: cross-path density convergence ────────────────────────────

    /// Helper: build a body `Value::StructureInstance` whose `material` field
    /// is the given material value (for feeding to `resolve_body_density`).
    fn body_with_material(material: Value) -> Value {
        struct_instance("Body", vec![("material".to_string(), material)])
    }

    /// task-4470 step-3 (RED → GREEN in step-4): assert that a single
    /// `ElasticMaterial` Value with a **positive** dimensioned-Scalar `density`
    /// yields **identical** density values from both the modal and dynamics
    /// resolution paths, and that the two paths diverge by design at the
    /// missing-density tail (modal strict → E_ModalNoMassMatrix; dynamics →
    /// None + E_DynamicsNoDensity Error — ambient-default-material C task 4498
    /// replaced the former water default with E_DynamicsNoDensity, task 4498).
    ///
    /// **Scope of the convergence invariant:** identical ρ holds only on the
    /// *positive* material rung.  A non-positive (≤ 0) or NaN material density
    /// intentionally diverges earlier, on the *material* rung itself: modal's
    /// local positivity guard (`*si_value > 0.0`) short-circuits to
    /// `Err(E_ModalNoMassMatrix)`, while dynamics returns the raw value verbatim
    /// for the downstream PSD hook to validate.  This positivity filtering is
    /// deliberately **not** part of the shared `resolve_density_strict` rung-walk
    /// (mass_props.rs is Value-free and does not validate magnitude — see
    /// design decision "Keep modal's positivity validation in the eval layer").
    ///
    /// The explicit-rung agreement (when a caller-supplied explicit density
    /// overrides the material rung) is verified separately by
    /// `strict_shared_rung_walk_invariant` in
    /// `crates/reify-stdlib/src/dynamics/mass_props.rs`, which covers the case
    /// modal intentionally never exercises (modal passes `explicit = None`).
    ///
    /// RED: `crate::dynamics_ops::resolve_body_density` is private until
    /// step-4 bumps it to `pub(crate)` → compile error.
    #[test]
    fn cross_path_density_convergence_modal_equals_dynamics() {
        // ── shared positive-density material ────────────────────────────────
        let material = material_with_density(Some(7850.0));

        // (1) Modal path: extract_density_or_degenerate reads the density
        //     directly from the material Value.
        let modal_rho = match extract_density_or_degenerate(&material) {
            Ok(rho) => rho,
            Err(_) => panic!("positive density must pass the modal guard"),
        };

        // (2) Dynamics path: resolve_body_density reads body.material.density
        //     via body_material_density / cell_f64.
        let body = body_with_material(material.clone());
        let mut diags: Vec<Diagnostic> = Vec::new();
        let dyn_rho = crate::dynamics_ops::resolve_body_density(&body, None, &mut diags)
            .expect("body with positive material density must resolve");

        // (3) Both paths must return the same value.
        assert!(
            (modal_rho - 7850.0).abs() < 1e-9,
            "modal path must return 7850.0; got {modal_rho}",
        );
        assert!(
            (dyn_rho - 7850.0).abs() < 1e-9,
            "dynamics path must return 7850.0; got {dyn_rho}",
        );
        assert!(
            (modal_rho - dyn_rho).abs() < 1e-9,
            "modal and dynamics paths must agree; modal={modal_rho} dyn={dyn_rho}",
        );
        // Both agree with the shared rung-walk.
        let strict_rho = resolve_density_strict(None, Some(7850.0))
            .map(|(d, _)| d)
            .expect("material rung must return Some");
        assert!(
            (modal_rho - strict_rho).abs() < 1e-9,
            "modal rho must equal resolve_density_strict result; modal={modal_rho} strict={strict_rho}",
        );
        assert!(
            (dyn_rho - strict_rho).abs() < 1e-9,
            "dynamics rho must equal resolve_density_strict result; dyn={dyn_rho} strict={strict_rho}",
        );
        // No diagnostic emitted on the positive-density path.
        assert!(
            diags.is_empty(),
            "no diagnostics expected on the positive-density dynamics path; got {diags:?}",
        );

        // ── tail divergence by design ────────────────────────────────────────

        // (4) Modal STRICT tail: missing density → E_ModalNoMassMatrix, never water.
        let missing_material = material_with_density(None);
        match extract_density_or_degenerate(&missing_material) {
            Err(outcome) => assert_no_mass_degenerate(outcome),
            Ok(d) => panic!("missing density must short-circuit; got Ok({d})"),
        }

        // (5) Dynamics no-density tail: bare body with no material density →
        //     None + E_DynamicsNoDensity Error (water default removed in task 4498).
        let bare_body = struct_instance("Body", vec![]);
        let mut water_diags: Vec<Diagnostic> = Vec::new();
        let no_density_result =
            crate::dynamics_ops::resolve_body_density(&bare_body, None, &mut water_diags);
        assert!(
            no_density_result.is_none(),
            "bare body with no material density must return None (E_DynamicsNoDensity); \
             got {no_density_result:?}",
        );
        let has_no_density_error = water_diags.iter().any(|d| {
            d.code == Some(reify_core::DiagnosticCode::DynamicsNoDensity)
                && d.severity == reify_core::Severity::Error
        });
        assert!(
            has_no_density_error,
            "dynamics no-density tail must emit E_DynamicsNoDensity Error (task 4498); \
             got {water_diags:?}",
        );
    }

    // ── Part-field echo migration (step-5 RED / step-6 GREEN) ────────────────

    #[test]
    fn echo_part_field_is_part_instance() {
        // placeholder_part() must return a zero-field opaque Part StructureInstance.
        // The two non-degenerate runtime paths (run_modal_analysis, run_transient_response)
        // call placeholder_part() directly, so a regression there would also break
        // this assertion via the shared helper — no separate integration test needed.
        let part_val = placeholder_part();
        match &part_val {
            Value::StructureInstance(si) => {
                assert_eq!(
                    si.type_name, "Part",
                    "placeholder_part type_name must be Part"
                );
                assert!(
                    si.fields.is_empty(),
                    "placeholder_part must be zero-field; got {:?}",
                    si.fields
                );
            }
            other => panic!("placeholder_part() must be StructureInstance; got {other:?}"),
        }

        // degenerate_modal_result().fields["part"] must be a Part StructureInstance.
        let modal_result = degenerate_modal_result();
        match &modal_result {
            Value::StructureInstance(si) => {
                let part = si
                    .fields
                    .get("part")
                    .expect("degenerate_modal_result must have a 'part' field");
                match part {
                    Value::StructureInstance(p) => {
                        assert_eq!(p.type_name, "Part");
                        assert!(p.fields.is_empty());
                    }
                    other => {
                        panic!(
                            "degenerate_modal_result.part must be Part StructureInstance; got {other:?}"
                        )
                    }
                }
            }
            other => panic!("degenerate_modal_result() must be StructureInstance; got {other:?}"),
        }

        // degenerate_displacement_history().fields["part"] must also be Part.
        let dth = degenerate_displacement_history();
        match &dth {
            Value::StructureInstance(si) => {
                let part = si
                    .fields
                    .get("part")
                    .expect("degenerate_displacement_history must have a 'part' field");
                match part {
                    Value::StructureInstance(p) => {
                        assert_eq!(p.type_name, "Part");
                        assert!(p.fields.is_empty());
                    }
                    other => {
                        panic!(
                            "degenerate_displacement_history.part must be Part StructureInstance; got {other:?}"
                        )
                    }
                }
            }
            other => {
                panic!("degenerate_displacement_history() must be StructureInstance; got {other:?}")
            }
        }
    }

    // ── Mechanism-modal M/K assembly tests (task 4271 steps 3–4) ────────────

    /// Read entry `(r, c)` from a `SparseRowMat`; returns `0.0` if absent.
    /// Mirrors the helper in `reify_solver_elastic::joint_stiffness` tests.
    fn get_entry(mat: &SparseRowMat<usize, f64>, r: usize, c: usize) -> f64 {
        let sym = mat.symbolic();
        let cols = sym.col_idx_of_row_raw(r);
        let vals = mat.val_of_row(r);
        for (col_raw, &val) in cols.iter().zip(vals.iter()) {
            if *col_raw == c {
                return val;
            }
        }
        0.0
    }

    /// Build a minimal `MassProperties` StructureInstance for test fixtures.
    /// `mass` is in kg (SI).
    fn mass_props_solid(mass: f64) -> Value {
        let zero3 = Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let zero_row = vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)];
        let fields: PersistentMap<String, Value> = [
            (
                "mass".to_string(),
                Value::Scalar {
                    si_value: mass,
                    dimension: DimensionVector::MASS,
                },
            ),
            ("com".to_string(), zero3),
            (
                "inertia".to_string(),
                Value::Matrix(vec![
                    zero_row.clone(),
                    zero_row.clone(),
                    zero_row.clone(),
                ]),
            ),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "MassProperties".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Build a minimal flexure joint `Value::Map` with a scalar `spring_rate`.
    /// `spring_rate` is in SI (N/m or N·m/rad).
    fn flexure_joint(spring_rate: f64) -> Value {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("notch_flexure".to_string()),
        );
        m.insert(
            Value::String("spring_rate".to_string()),
            Value::Scalar {
                si_value: spring_rate,
                dimension: DimensionVector::DIMENSIONLESS,
            },
        );
        Value::Map(m)
    }

    /// Build a minimal rigid joint `Value::Map` (no `spring_rate` key).
    fn rigid_joint() -> Value {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("revolute".to_string()),
        );
        Value::Map(m)
    }

    /// Build a one-body mechanism `Value::Map` with the given solid and joint.
    fn one_body_mechanism(solid: Value, joint: Value) -> Value {
        use std::collections::BTreeMap;
        // body record: { "id": 0, "solid": solid, "at": joint, "parent": Undef, "pose": Undef }
        let mut body = BTreeMap::new();
        body.insert(Value::String("id".to_string()), Value::Int(0));
        body.insert(Value::String("solid".to_string()), solid);
        body.insert(Value::String("at".to_string()), joint);
        body.insert(Value::String("parent".to_string()), Value::Undef);
        body.insert(Value::String("pose".to_string()), Value::Undef);
        let body_val = Value::Map(body);
        // mechanism: { "kind": "mechanism", "bodies": [body], "joint_parents": {}, "loop_closures": [], "next_id": 1 }
        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("bodies".to_string()),
            Value::List(vec![body_val]),
        );
        mech.insert(
            Value::String("joint_parents".to_string()),
            Value::Map(BTreeMap::new()),
        );
        mech.insert(
            Value::String("loop_closures".to_string()),
            Value::List(vec![]),
        );
        mech.insert(Value::String("next_id".to_string()), Value::Int(1));
        Value::Map(mech)
    }

    /// Step-3 RED / Step-4 GREEN: `assemble_mechanism_km` returns the correct
    /// diagonal K and M for a one-body mechanism.
    ///
    /// Case A: flexure joint with spring_rate k → n_dof=1, M[0,0]=m, K[0,0]=k.
    /// Case B: rigid joint (no spring_rate) → n_dof=1, M[0,0]=m, K[0,0]=0.
    ///
    /// RED until step-4 adds `assemble_mechanism_km`.
    #[test]
    fn assemble_mechanism_km_returns_diagonal_k_and_m() {
        let m_val = 0.5_f64;
        let k_val = 1234.5_f64;

        // Case A: flexure joint → K[0,0] = k, M[0,0] = m.
        {
            let mech = one_body_mechanism(mass_props_solid(m_val), flexure_joint(k_val));
            let (k, m, n_dof) = assemble_mechanism_km(&mech)
                .expect("one-body flexure mechanism must yield Some((K,M,n_dof))");
            assert_eq!(n_dof, 1, "one tree body → n_dof=1");
            assert_eq!(m.nrows(), 1, "M must be 1×1");
            assert_eq!(k.nrows(), 1, "K must be 1×1");
            let m00 = get_entry(&m, 0, 0);
            let k00 = get_entry(&k, 0, 0);
            assert!(
                (m00 - m_val).abs() < 1e-12,
                "M[0,0] = {m00} should equal body mass {m_val}"
            );
            assert!(
                (k00 - k_val).abs() < 1e-12,
                "K[0,0] = {k00} should equal spring_rate {k_val}"
            );
        }

        // Case B: rigid joint → K[0,0] = 0, M[0,0] = m.
        {
            let mech = one_body_mechanism(mass_props_solid(m_val), rigid_joint());
            let (k, m, n_dof) = assemble_mechanism_km(&mech)
                .expect("one-body rigid mechanism must yield Some((K,M,n_dof))");
            assert_eq!(n_dof, 1, "one tree body → n_dof=1");
            let m00 = get_entry(&m, 0, 0);
            let k00 = get_entry(&k, 0, 0);
            assert!(
                (m00 - m_val).abs() < 1e-12,
                "M[0,0] = {m00} should equal body mass {m_val}"
            );
            assert!(
                k00.abs() < 1e-15,
                "K[0,0] = {k00} must be 0 for a rigid joint (no spring_rate)"
            );
        }
    }

    // ── Mechanism-modal first-mode frequency tests (task 4271 steps 5–6) ────

    /// Build a two-body mechanism `Value::Map` (no loop_closures) with the
    /// given solids and inbound joints for body 0 and body 1.
    fn two_body_mechanism(solid0: Value, joint0: Value, solid1: Value, joint1: Value) -> Value {
        use std::collections::BTreeMap;
        let mut body0 = BTreeMap::new();
        body0.insert(Value::String("id".to_string()), Value::Int(0));
        body0.insert(Value::String("solid".to_string()), solid0);
        body0.insert(Value::String("at".to_string()), joint0);
        body0.insert(Value::String("parent".to_string()), Value::Undef);
        body0.insert(Value::String("pose".to_string()), Value::Undef);
        let mut body1 = BTreeMap::new();
        body1.insert(Value::String("id".to_string()), Value::Int(1));
        body1.insert(Value::String("solid".to_string()), solid1);
        body1.insert(Value::String("at".to_string()), joint1);
        body1.insert(Value::String("parent".to_string()), Value::Int(0));
        body1.insert(Value::String("pose".to_string()), Value::Undef);
        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("bodies".to_string()),
            Value::List(vec![Value::Map(body0), Value::Map(body1)]),
        );
        mech.insert(
            Value::String("joint_parents".to_string()),
            Value::Map(BTreeMap::new()),
        );
        mech.insert(
            Value::String("loop_closures".to_string()),
            Value::List(vec![]),
        );
        mech.insert(Value::String("next_id".to_string()), Value::Int(2));
        Value::Map(mech)
    }

    /// Step-5 RED / Step-6 GREEN: `solve_mechanism_modal_trampoline` returns
    /// the correct first-mode frequency for a flexure-jointed mechanism.
    ///
    /// Mirrors `joint_stiffness_modal_frequency.rs` (the κ-kernel test):
    /// for a diagonal lumped spring-mass system λ₀ = k/m is closed-form exact,
    /// so the recovered f = √(k/m)/(2π) should agree to ≪ 2% tolerance.
    ///
    /// Case A (n_dof=1 → anchor-pad path): single body, mass m=0.5 kg,
    ///   spring_rate k = 2.65·E·I/L (Howell cantilever values from the kernel test).
    ///   Asserts modes[0].frequency ≈ √(k/m)/(2π) within 2%.
    ///
    /// Case B (n_dof=2 → direct solve, no pad): two bodies with distinct
    ///   spring rates k0,k1 and masses m0,m1 such that √(k0/m0) < √(k1/m1).
    ///   Asserts modes[0].frequency ≈ √(k0/m0)/(2π) (lowest mode, ascending).
    ///
    /// RED until step-6 implements run_mechanism_modal / solve_mechanism_modal_trampoline.
    #[test]
    fn mechanism_modal_first_mode_frequency() {
        use std::f64::consts::PI;
        let options = struct_instance("ModalOptions", vec![]);

        // ── Case A: single body, n_dof=1 → anchor-pad path ──────────────────
        {
            // Howell cantilever geometry from the kernel test (§10.1)
            let e = 200e9_f64;
            let l = 0.020_f64;
            let b = 0.005_f64;
            let h = 0.0005_f64;
            let i_sect = b * h.powi(3) / 12.0;
            let k = 2.65 * e * i_sect / l;
            let m = 0.5_f64;
            let expected_f = (k / m).sqrt() / (2.0 * PI);

            let mech = one_body_mechanism(mass_props_solid(m), flexure_joint(k));
            let value_inputs = vec![mech, options.clone()];
            let outcome = solve_mechanism_modal_trampoline(
                &value_inputs,
                &[],
                &Value::Undef,
                None,
                &CancellationHandle::new(),
            );
            let ComputeOutcome::Completed { result, diagnostics, .. } = outcome else {
                panic!("Case A: expected Completed outcome");
            };
            assert!(
                !diagnostics.iter().any(|d| d.severity == reify_core::Severity::Error),
                "Case A: must not produce Error diagnostics; got {diagnostics:?}",
            );
            let data = match &result {
                Value::StructureInstance(d) => d,
                other => panic!("Case A: expected ModalResult StructureInstance, got {other:?}"),
            };
            assert_eq!(data.type_name, "ModalResult", "Case A: result must be ModalResult");
            let modes = match data.fields.get("modes") {
                Some(Value::List(m)) => m,
                other => panic!("Case A: modes must be a List; got {other:?}"),
            };
            assert!(!modes.is_empty(), "Case A: must return ≥ 1 mode");
            let mode0 = match &modes[0] {
                Value::StructureInstance(d) => d,
                other => panic!("Case A: modes[0] must be a Mode StructureInstance; got {other:?}"),
            };
            let f0 = match mode0.fields.get("frequency") {
                Some(Value::Scalar { si_value, .. }) => *si_value,
                other => panic!("Case A: modes[0].frequency must be a Scalar; got {other:?}"),
            };
            let rel_err = (f0 - expected_f).abs() / expected_f;
            assert!(
                rel_err <= 0.02,
                "Case A: first-mode frequency relative error {rel_err:.2e} > 2%: \
                 f0={f0:.6} Hz, expected={expected_f:.6} Hz (k={k:.4e} N/m, m={m} kg)",
            );
        }

        // ── Case B: two bodies, n_dof=2 → direct solve, no anchor pad ───────
        {
            // Body 0: lower frequency (smaller ω₀ = √(k0/m0))
            let k0 = 1_000.0_f64;
            let m0 = 2.0_f64;
            // Body 1: higher frequency (larger ω₁ = √(k1/m1))
            let k1 = 50_000.0_f64;
            let m1 = 0.1_f64;
            // Verify ordering: √(k0/m0) < √(k1/m1) ⟹ f0 < f1.
            assert!(
                (k0 / m0).sqrt() < (k1 / m1).sqrt(),
                "test precondition: body-0 frequency must be lower than body-1"
            );
            let expected_f0 = (k0 / m0).sqrt() / (2.0 * PI);

            let mech = two_body_mechanism(
                mass_props_solid(m0),
                flexure_joint(k0),
                mass_props_solid(m1),
                flexure_joint(k1),
            );
            let value_inputs = vec![mech, options.clone()];
            let outcome = solve_mechanism_modal_trampoline(
                &value_inputs,
                &[],
                &Value::Undef,
                None,
                &CancellationHandle::new(),
            );
            let ComputeOutcome::Completed { result, .. } = outcome else {
                panic!("Case B: expected Completed outcome");
            };
            let data = match &result {
                Value::StructureInstance(d) => d,
                other => panic!("Case B: expected ModalResult StructureInstance, got {other:?}"),
            };
            let modes = match data.fields.get("modes") {
                Some(Value::List(m)) => m,
                other => panic!("Case B: modes must be a List; got {other:?}"),
            };
            assert!(modes.len() >= 2, "Case B: 2-DOF solve must return ≥ 2 modes");
            let mode0 = match &modes[0] {
                Value::StructureInstance(d) => d,
                other => panic!("Case B: modes[0] must be a Mode StructureInstance; got {other:?}"),
            };
            let f0 = match mode0.fields.get("frequency") {
                Some(Value::Scalar { si_value, .. }) => *si_value,
                other => panic!("Case B: modes[0].frequency must be a Scalar; got {other:?}"),
            };
            let rel_err = (f0 - expected_f0).abs() / expected_f0;
            assert!(
                rel_err <= 0.02,
                "Case B: first-mode frequency relative error {rel_err:.2e} > 2%: \
                 f0={f0:.6} Hz, expected={expected_f0:.6} Hz (k0={k0}, m0={m0})",
            );
        }
    }

}
