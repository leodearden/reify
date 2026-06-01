// `node * 6 + axis` is the dominant DOF-index idiom in this file; allowing
// `+ 0` and `1 *` keeps the formula structure visible at every call site.
#![allow(clippy::identity_op)]

//! Integration tests for the shell FEA pipeline (PRD v0.4 task #21).
//!
//! # Scope: smoke tests, NOT validated benchmarks
//!
//! This file exercises the end-to-end shell assembly pipeline
//! (`shell_element_stiffness` → `assemble_global_stiffness` →
//! `apply_dirichlet_row_elimination` → dense-LU solve) on four canonical
//! shell-formulation geometries: the pinched cylinder, Scordelis-Lo roof,
//! hemisphere-with-point-loads, and twisted cantilever beam.
//!
//! These geometries are **drawn from** MacNeal & Harder (1985), but the
//! present tests are **smoke tests**, not validated benchmarks. The
//! tip-displacement assertions check only:
//!   - **sign** (response is in the loaded direction),
//!   - **finiteness** (not NaN, not infinite),
//!   - **order of magnitude** (within a wide physically-plausible band).
//!
//! The four **curved** smoke tests do **not** assert against the published
//! MacNeal-Harder reference values: bare MITC3 suffers from severe membrane
//! locking on curved geometry (factor 21–2200× under-prediction at coarse mesh
//! resolution). Tightening those bands needs an ANS-membrane correction on a
//! curved substrate (task 4065) — flat-facet enrichment cannot cure *membrane*
//! locking, which is a curvature phenomenon.
//!
//! The **twisted-cantilever** test additionally carries a genuine relative
//! observable signal: `twisted_beam_mitc3_plus_tip_deflection_is_closer_to_reference_than_bare`
//! shows that the flat-facet MITC3+ element (`shell_element_stiffness_mitc3_plus`,
//! task 3392) relieves transverse-shear locking and moves the tip deflection
//! measurably closer to the published reference than bare MITC3. The MITC3+
//! shear cure lives in the *nodal* assumed-shear field (interior-tying Eq. 9),
//! NOT the cubic bubble — the bubble is inert in transverse shear on a flat
//! facet (K_NB^shear ≡ 0; see the `shell_assembly.rs` header) and becomes live
//! only on the curved director substrate of task 4065.
//!
//! In addition to the four geometry smoke tests, this file contains:
//!   - a **locking-detection** test verifying that MITC3 does not collapse
//!     under decreasing thickness (signature of shear-locking),
//!   - a flat-plate cantilever sanity test exercising the same end-to-end
//!     pipeline on a non-curved patch.
//!
//! # Drilling rotation (no test-local workaround)
//!
//! MITC3 carries zero stiffness along the local drilling rotation (θ_z,
//! rotation about the element normal). On the four curved benchmarks the
//! symmetry-plane Dirichlet BCs already pin enough θ_z DOFs that the
//! drilling kernel is fully constrained, so each test calls
//! `shell_element_stiffness` directly — no Hughes-Brezzi penalty helper
//! is interposed and the production assembly path is exercised
//! end-to-end. The flat-plate sanity test additionally pins θ_z=0 at its
//! free nodes (its elements all share a single normal, so the kernel
//! survives without that pin). A future curved-element MITC3+ formulation
//! may add a real drilling rotation field at which point these explicit
//! pins can be reviewed. Review escalation: `esc-3034-165`.
//!
//! # PRD reference
//!
//! `docs/prds/v0_4/structural-analysis-shells.md`, task #21
//! ("Validation & polish").

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, DirichletBc, ElementStiffness, IsotropicElastic,
    apply_dirichlet_row_elimination, assemble_global_stiffness, shell_element_stiffness,
    shell_element_stiffness_degenerate, shell_element_stiffness_degenerate_ans,
    shell_element_stiffness_mitc3_plus,
};

// ─── test-local helpers ──────────────────────────────────────────────────────

/// Assemble, apply Dirichlet BCs, and solve a pure-shell FEA system via
/// dense LU factorization.
///
/// # Arguments
///
/// * `elements` — assembled shell elements (each with connectivity and K_e)
/// * `n_nodes` — total number of nodes; K_global is `(6·n_nodes)²`
/// * `dirichlet_bcs` — prescribed DOF values (row-elimination method)
/// * `point_loads_per_dof` — `(dof_index, force_value)` pairs accumulated
///   into the RHS vector
///
/// # Returns
///
/// Dense displacement vector `u` of length `6·n_nodes`: for node `n`, the
/// six entries `u[6n .. 6n+6]` are `[u_x, u_y, u_z, θ_x, θ_y, θ_z]`.
///
/// # Solve method
///
/// Dense LU via `faer`: replicates the pattern established in the existing
/// `dirichlet_bc_elimination_satisfies_original_equilibrium_at_free_dofs`
/// test (`crates/reify-solver-elastic/src/boundary/dirichlet.rs:696-732`).
/// Dense LU is adequate for coarse-mesh benchmarks (≤ ~300 DOFs).
fn solve_shell_system(
    elements: &[AssemblyElement<'_>],
    n_nodes: usize,
    dirichlet_bcs: &[DirichletBc],
    point_loads_per_dof: &[(usize, f64)],
) -> Vec<f64> {
    use faer::linalg::solvers::Solve;

    let ndof = 6 * n_nodes;

    // Assemble global stiffness matrix (pure-shell: D = 6 DOFs/node).
    let mut k = assemble_global_stiffness(n_nodes, elements, AssemblyMode::Deterministic);

    // Build load vector by accumulating point loads.
    let mut f = vec![0.0_f64; ndof];
    for &(dof, value) in point_loads_per_dof {
        f[dof] += value;
    }

    // Deduplicate BCs: apply_dirichlet_row_elimination panics on duplicate DOF
    // indices in debug builds. Corner nodes that lie on multiple symmetry planes
    // may appear in more than one BC group; we sort and dedup, keeping the first
    // occurrence. All our symmetry BCs are homogeneous (value=0.0), so collapsing
    // identical entries is safe — but a future test that prescribes a non-zero
    // displacement at a corner that also lands on a homogeneous symmetry plane
    // would have one of its values silently dropped here. Debug-assert that any
    // duplicate DOFs share the same value so the footgun surfaces in test runs.
    let mut bcs: Vec<DirichletBc> = dirichlet_bcs.to_vec();
    bcs.sort_by_key(|bc| bc.dof);
    if cfg!(debug_assertions) {
        for window in bcs.windows(2) {
            if window[0].dof == window[1].dof {
                assert_eq!(
                    window[0].value, window[1].value,
                    "duplicate Dirichlet BC at DOF {} with conflicting values \
                     ({} vs {}); solve_shell_system would silently drop one of them. \
                     Supply a single combined entry instead.",
                    window[0].dof, window[0].value, window[1].value,
                );
            }
        }
    }
    bcs.dedup_by_key(|bc| bc.dof);

    // Apply Dirichlet BCs via symmetric row elimination.
    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    // Dense LU solve: K · u = f.
    // Follows the pattern in dirichlet.rs:729-733.
    let k_dense = k.to_dense();
    let plu = k_dense.partial_piv_lu();
    let mut rhs = faer::Mat::<f64>::from_fn(ndof, 1, |i, _| f[i]);
    plu.solve_in_place(&mut rhs);

    rhs.col_as_slice(0_usize).to_vec()
}

/// Compute per-element shell stiffness matrices for every triangle in
/// `connectivity`.
///
/// # Arguments
///
/// * `nodes` — node coordinates (`nodes[k]` is the 3-D position of node `k`)
/// * `connectivity` — element node indices (`connectivity[e]` is `[n0, n1, n2]`)
/// * `thickness` — shell thickness (uniform across all elements)
/// * `mat` — isotropic elastic material properties
///
/// # Returns
///
/// `Vec<ElementStiffness>` with one entry per element, in the same order as
/// `connectivity`. Each entry is the 18×18 element stiffness matrix in the
/// element's local 6-DOF-per-node ordering.
fn build_shell_stiffnesses(
    nodes: &[[f64; 3]],
    connectivity: &[[usize; 3]],
    thickness: f64,
    mat: &IsotropicElastic,
) -> Vec<ElementStiffness> {
    connectivity
        .iter()
        .map(|conn| {
            let elem_nodes = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]]];
            shell_element_stiffness(&elem_nodes, thickness, mat)
        })
        .collect()
}

/// MITC3+ counterpart of [`build_shell_stiffnesses`]: builds one per-element
/// stiffness matrix per triangle via the genuine flat-facet MITC3+ element
/// [`shell_element_stiffness_mitc3_plus`] (Lee, Lee & Bathe 2014). Used to run
/// the twisted-cantilever benchmark with both formulations on the identical
/// mesh / BCs / loads so the shear-locking improvement is observable end-to-end.
fn build_shell_stiffnesses_mitc3_plus(
    nodes: &[[f64; 3]],
    connectivity: &[[usize; 3]],
    thickness: f64,
    mat: &IsotropicElastic,
) -> Vec<ElementStiffness> {
    connectivity
        .iter()
        .map(|conn| {
            let elem_nodes = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]]];
            shell_element_stiffness_mitc3_plus(&elem_nodes, thickness, mat)
        })
        .collect()
}

/// Degenerate-shell counterpart of [`build_shell_stiffnesses_mitc3_plus`]: one
/// per-element stiffness via the curved-substrate element
/// [`shell_element_stiffness_degenerate`] (task 4068), fed per-node directors.
///
/// `directors[k]` is the unit through-thickness director (vertex normal) at
/// global node `k`; each element reads the three directors of its nodes. Used
/// to run a bending-dominated curved benchmark with both the flat MITC3+ and the
/// degenerate substrate on the identical mesh / BCs / loads, so the
/// geometric-fidelity improvement is observable end-to-end (mirroring the
/// twisted-beam plus-vs-bare template).
fn build_shell_stiffnesses_degenerate(
    nodes: &[[f64; 3]],
    connectivity: &[[usize; 3]],
    directors: &[[f64; 3]],
    thickness: f64,
    mat: &IsotropicElastic,
) -> Vec<ElementStiffness> {
    connectivity
        .iter()
        .map(|conn| {
            let elem_nodes = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]]];
            let elem_dirs = [directors[conn[0]], directors[conn[1]], directors[conn[2]]];
            let elem_th = [thickness; 3];
            shell_element_stiffness_degenerate(&elem_nodes, &elem_dirs, &elem_th, mat)
        })
        .collect()
}

/// ANS-membrane counterpart of [`build_shell_stiffnesses_degenerate`]: one
/// per-element stiffness via the curved substrate with the assumed-natural-strain
/// **membrane** field active ([`shell_element_stiffness_degenerate_ans`], task
/// 4069). Identical wiring to [`build_shell_stiffnesses_degenerate`] — same
/// per-node directors, same uniform thickness — so a benchmark can run the
/// non-ANS and ANS degenerate elements on the IDENTICAL mesh / BCs / loads and
/// observe the membrane-locking cure end-to-end (the ANS field softens the
/// over-stiff response strictly toward the reference).
fn build_shell_stiffnesses_degenerate_ans(
    nodes: &[[f64; 3]],
    connectivity: &[[usize; 3]],
    directors: &[[f64; 3]],
    thickness: f64,
    mat: &IsotropicElastic,
) -> Vec<ElementStiffness> {
    connectivity
        .iter()
        .map(|conn| {
            let elem_nodes = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]]];
            let elem_dirs = [directors[conn[0]], directors[conn[1]], directors[conn[2]]];
            let elem_th = [thickness; 3];
            shell_element_stiffness_degenerate_ans(&elem_nodes, &elem_dirs, &elem_th, mat)
        })
        .collect()
}

/// Unit-normalize a 3-vector (directors must be unit-norm for the degenerate
/// element). Falls back to `+z` for a (near-)zero vector.
fn normalize3(v: [f64; 3]) -> [f64; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n > 1e-30 {
        [v[0] / n, v[1] / n, v[2] / n]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Build the `AssemblyElement` handle slice for `assemble_global_stiffness`.
///
/// `AssemblyElement<'a>` borrows both `connectivity` and `stiffness` — so
/// both must outlive the returned vec. This helper is intentionally kept
/// separate from `build_shell_stiffnesses` to satisfy Rust's lifetime rules:
/// the caller must own the `Vec<ElementStiffness>` on its stack before
/// calling this helper.
///
/// # Arguments
///
/// * `connectivity` — element node indices (same slice used to build stiffness)
/// * `stiffness` — per-element stiffness matrices (output of
///   `build_shell_stiffnesses`)
///
/// # Returns
///
/// `Vec<AssemblyElement<'_>>` in the same order as `connectivity`.
fn assembly_elements_for<'a>(
    connectivity: &'a [[usize; 3]],
    stiffness: &'a [ElementStiffness],
) -> Vec<AssemblyElement<'a>> {
    connectivity
        .iter()
        .zip(stiffness.iter())
        .enumerate()
        .map(|(i, (conn, k_e))| AssemblyElement {
            id: i,
            connectivity: conn,
            k_e,
        })
        .collect()
}

/// Build the pinched-cylinder octant symmetry Dirichlet BCs for the 1/8
/// octant model (θ ∈ [0, π/2], z ∈ [0, L/2]).
///
/// # Arguments
///
/// * `nodes` — node coordinates (`nodes[k]` is the 3-D position of node `k`)
/// * `l` — total cylinder length; the diaphragm plane is at `z = l/2`
/// * `tol` — node-on-boundary detection tolerance.  **Must be strictly less
///   than half the smallest inter-node spacing** in the mesh; exceeding this
///   bound silently over-constrains interior nodes and produces a spuriously
///   stiff solution that may still satisfy loose smoke-test bounds.  In
///   debug builds, a `debug_assert` checks this invariant by computing the
///   minimum pairwise node distance (O(n²), negligible for small test meshes).
///   Typical values: `1.0` for R=300, L=600; `0.1` for R=1, L=2.
///
/// # BC groups
///
/// ## Diaphragm end (z=L/2): MacNeal-Harder "rigid end diaphragm" condition
///
/// From MacNeal & Harder (1985): "Support: Rigid end diaphragm (v=0, w=0)"
/// where v is the circumferential displacement and w = u_z is the axial.
///
/// In global Cartesian (cylinder axis = z):
///   w = u_z = 0  (axial constraint)
///   v = −sin θ · u_x + cos θ · u_y = 0  (circumferential)
///
/// The exact circumferential constraint v=0 is an oblique (rotated) BC that
/// cannot be expressed as a simple per-DOF constraint with the current API.
/// For the OCTANT model:
///   • Corner θ=0  (j=0, y=0 plane): v = u_y = 0 already from y=0 symmetry BC.
///   • Corner θ=π/2 (j=nx, x=0 plane): v = −u_x = 0 already from x=0 symmetry BC.
///   • Intermediate θ (j=1..nx-1): v=0 is omitted; these nodes can slide
///     tangentially — an accepted coarse-mesh approximation. The radial degree
///     of freedom (u_r) is intentionally left FREE so the cylinder can breathe.
///
/// θ_z=0 (torsion) is added for numerical robustness at interior diaphragm
/// nodes not already stabilised by the symmetry-plane BCs.
///
/// ## Symmetry at y=0 (θ=0): xz-plane
///   u_y=0, θ_x=0, θ_z=0
///
/// ## Symmetry at x=0 (θ=π/2): yz-plane
///   u_x=0, θ_y=0, θ_z=0
///
/// ## Mid-span symmetry at z=0
///
/// Physical z-symmetry conditions for the Reissner-Mindlin shell:
///   u_z = 0 (axial translation, antisymmetric about z=0).
///   meridional rotation = 0: the director tilt in the z-direction must
///     vanish at the symmetry plane. In cylindrical terms this is the
///     rotation about the circumferential direction e_θ = (−sin θ, cos θ, 0):
///       β = −sin θ · θ_x + cos θ · θ_y = 0  (oblique, varies with θ)
///
/// The oblique constraint cannot be expressed as a per-DOF BC with the
/// current DirichletBc API except at axis-aligned corner nodes:
///   θ=0   (j=0, is_y0):   −sin 0 · θ_x + cos 0 · θ_y = θ_y = 0
///   θ=π/2 (j=nx, is_x0): −sin(π/2) · θ_x + cos(π/2) · θ_y = −θ_x = 0
///
/// For intermediate nodes (j=1..nx-1), the meridional rotation constraint is
/// omitted — the same accepted approximation used for the tangential BC at the
/// diaphragm end. These nodes are slightly more flexible than the symmetric
/// reference solution; the resulting over-estimate is within the coarse-mesh
/// tolerance band.
///
/// Cross-reference: reviewer escalation `esc-3034-165`.
fn pinched_cylinder_octant_symmetry_bcs(nodes: &[[f64; 3]], l: f64, tol: f64) -> Vec<DirichletBc> {
    // Guard: tol must be < 0.5 * min_spacing so no interior node is
    // accidentally captured as a boundary node.  Only active in debug builds;
    // O(n²) over the node count (negligible for small test meshes).
    if cfg!(debug_assertions) {
        let mut min_sq = f64::INFINITY;
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let dx = nodes[i][0] - nodes[j][0];
                let dy = nodes[i][1] - nodes[j][1];
                let dz = nodes[i][2] - nodes[j][2];
                let sq = dx * dx + dy * dy + dz * dz;
                if sq < min_sq {
                    min_sq = sq;
                }
            }
        }
        debug_assert!(
            min_sq > 0.0,
            "pinched_cylinder_octant_symmetry_bcs: mesh contains coincident nodes"
        );
        debug_assert!(
            tol * tol < 0.25 * min_sq,
            "pinched_cylinder_octant_symmetry_bcs: tol={:.4e} >= 0.5*min_spacing={:.4e}; \
             tol must be < half the smallest inter-node distance to avoid \
             pinning interior nodes as boundary nodes",
            tol,
            min_sq.sqrt() * 0.5,
        );
    }

    let mut bcs = Vec::new();
    for (node, n) in nodes.iter().enumerate() {
        let is_diaphragm = (n[2] - l / 2.0).abs() < tol; // z=L/2 end ring
        let is_y0 = n[1].abs() < tol; // θ=0 symmetry plane
        let is_x0 = n[0].abs() < tol; // θ=π/2 symmetry plane
        let is_z0 = n[2].abs() < tol; // z=0 mid-span plane

        let dof = |d: usize| node * 6 + d;

        if is_diaphragm {
            bcs.push(DirichletBc {
                dof: dof(2),
                value: 0.0,
            }); // w = u_z = 0
            bcs.push(DirichletBc {
                dof: dof(5),
                value: 0.0,
            }); // θ_z = 0 (torsion)
        }
        if is_y0 {
            bcs.push(DirichletBc {
                dof: dof(1),
                value: 0.0,
            }); // u_y
            bcs.push(DirichletBc {
                dof: dof(3),
                value: 0.0,
            }); // θ_x
            bcs.push(DirichletBc {
                dof: dof(5),
                value: 0.0,
            }); // θ_z
        }
        if is_x0 {
            bcs.push(DirichletBc {
                dof: dof(0),
                value: 0.0,
            }); // u_x
            bcs.push(DirichletBc {
                dof: dof(4),
                value: 0.0,
            }); // θ_y
            bcs.push(DirichletBc {
                dof: dof(5),
                value: 0.0,
            }); // θ_z
        }
        if is_z0 {
            bcs.push(DirichletBc {
                dof: dof(2),
                value: 0.0,
            }); // u_z = 0
            // Meridional rotation at axis-aligned corners only:
            if is_y0 {
                bcs.push(DirichletBc {
                    dof: dof(4),
                    value: 0.0,
                }); // θ_y = 0 at θ=0
            }
            if is_x0 {
                bcs.push(DirichletBc {
                    dof: dof(3),
                    value: 0.0,
                }); // θ_x = 0 at θ=π/2
            }
        }
    }
    bcs
}

// ─── step-2: pinched cylinder (MacNeal-Harder §3.3) ─────────────────────────

/// Pinched cylinder smoke test — geometry from MacNeal-Harder (1985) §3.3.
///
/// **MITC3 (flat-facet) capability test, NOT a validated benchmark.**
/// True MacNeal-Harder convergence requires curved-element MITC3+; see
/// the file header in `shell_assembly.rs` for the K_NB ≡ 0 proof that
/// shows flat-facet bubble enrichment is mathematically inert. The band
/// here documents observed bare-MITC3 behaviour on a 4×4 octant mesh —
/// roughly 76× under the published reference due to residual membrane
/// locking on curved geometry.
///
/// A thin cylindrical shell (R=300, L=600, t=3, E=3×10⁶, ν=0.3) is
/// loaded by two equal and opposite radial point loads P=1 at the midspan
/// (z=0). By 1/8 octant symmetry (z ∈ [0, L/2], θ ∈ [0, π/2]) the
/// octant model applies P/4 at the loaded corner (θ=π/2, z=0).
///
/// # Boundary conditions
///
/// | Plane | Physical role | Constrained DOFs |
/// |-------|---------------|-----------------|
/// | z=L/2 | Rigid diaphragm end | u_x=0, u_y=0, θ_z=0 |
/// | y=0   | θ=0 symmetry (xz-plane) | u_y=0, θ_x=0, θ_z=0 |
/// | x=0   | θ=π/2 symmetry (yz-plane) | u_x=0, θ_y=0, θ_z=0 |
/// | z=0   | Mid-span symmetry | u_z=0, θ_x=0, θ_y=0 |
///
/// # Smoke-test envelope (bare MITC3, flat-facet)
///
/// MacNeal-Harder (1985) reference: **1.8248×10⁻⁵**.
/// Observed bare-MITC3 4×4 octant displacement: **~2.4×10⁻⁷** (~76× under).
/// Acceptance band: `[1.0×10⁻⁷, 1.0×10⁻⁶]` — a factor-~2 window around the
/// observed value to absorb normal numerical drift. A future curved-element
/// MITC3+ implementation will move radial_disp UPWARD toward the published
/// reference and require widening (or replacement) of this band.
#[test]
fn pinched_cylinder_octant_smoke_test_radial_displacement_is_finite_and_inward() {
    const R: f64 = 300.0;
    const L: f64 = 600.0;
    const T: f64 = 3.0;
    const NX: usize = 4; // θ-direction divisions
    const NY: usize = 4; // z-direction divisions
    const P: f64 = 1.0; // total applied load
    const MACNEAL_HARDER_REF: f64 = 1.8248e-5;
    // Bare-MITC3 observed value is ~2.4e-7; band brackets observed × 2.
    const LOWER: f64 = 1.0e-7;
    const UPPER: f64 = 1.0e-6;

    let mat = IsotropicElastic {
        youngs_modulus: 3e6,
        poisson_ratio: 0.3,
    };

    // Mesh: nodes at (R·cos θ, R·sin θ, z).
    let (nodes, connectivity) = cylinder_octant_mesh(NX, NY, R, L);
    let n_nodes = nodes.len();

    // Build per-element stiffness via the production code path.
    let stiffness = build_shell_stiffnesses(&nodes, &connectivity, T, &mat);
    let elements = assembly_elements_for(&connectivity, &stiffness);

    // Build Dirichlet BCs (octant symmetry + rigid end diaphragm).
    // Tolerance 1.0 is well within each mesh spacing (~75 for z, ~115 arc-len
    // for θ at R=300), so node-on-boundary detection is exact for this mesh.
    // Full BC rationale lives in `pinched_cylinder_octant_symmetry_bcs`.
    let tol = 1.0_f64;
    let bcs = pinched_cylinder_octant_symmetry_bcs(&nodes, L, tol);

    // Load node: corner at (0, R, 0) = θ=π/2, z=0.
    // Both is_x0 and is_z0 apply → only u_y is free after BCs.
    let load_node = nodes
        .iter()
        .position(|n| n[0].abs() < tol && (n[1] - R).abs() < tol && n[2].abs() < tol)
        .expect("load node (0, R, 0) not found in mesh");

    // Radially inward load: F_y = -P/4 (by 1/8 octant symmetry, see plan).
    let point_loads = vec![(load_node * 6 + 1, -P / 4.0)];

    let u = solve_shell_system(&elements, n_nodes, &bcs, &point_loads);

    // Radial displacement = -u_y (inward = positive).
    let radial_disp = -u[load_node * 6 + 1];

    assert!(
        radial_disp.is_finite(),
        "pinched cylinder: radial_disp = {radial_disp} is not finite"
    );
    assert!(
        radial_disp > 0.0,
        "pinched cylinder: radial_disp = {radial_disp:.4e} \
         must be positive (inward) under inward radial load; sign reversal \
         indicates a BC or load-direction bug"
    );
    assert!(
        (LOWER..=UPPER).contains(&radial_disp),
        "pinched cylinder: radial_disp = {radial_disp:.6e} \
         outside bare-MITC3 envelope [{LOWER:.0e}, {UPPER:.0e}] \
         (MacNeal-Harder reference {MACNEAL_HARDER_REF:.4e}; bare MITC3 ~76× under). \
         A future curved-element MITC3+ fix will exceed this band and require widening."
    );
}

// ─── mesh helpers ────────────────────────────────────────────────────────────

/// Mesh the 1/8 octant of a pinched cylinder.
///
/// # Geometry
///
/// Cylindrical mid-surface: θ ∈ [0, π/2], z ∈ [0, L/2], radius `r`, total
/// length `l`. Node positions: `(r·cos θ, r·sin θ, z)` with θ and z
/// uniformly spaced.
///
/// # Parameters
///
/// * `nx` — θ-direction divisions (columns); produces `nx+1` nodes per row
/// * `ny` — z-direction divisions (rows); produces `ny+1` node rows
/// * `r` — cylinder radius
/// * `l` — total cylinder length (half-span in z is `l/2`)
///
/// # Outputs
///
/// `(nodes, connectivity)` where `nodes[k]` is the 3-D position of node `k`
/// and `connectivity[e]` gives the three node indices for triangle `e`.
///
/// # Node ordering
///
/// Row-major (z varies slowly, θ varies fast):
/// `node(i, j) = i*(nx+1) + j`, `i` = z-index ∈ [0, ny], `j` = θ-index ∈ [0, nx].
///
/// # Element count
///
/// `2·nx·ny` MITC3 triangles — each quad cell split along the A→D diagonal.
fn cylinder_octant_mesh(nx: usize, ny: usize, r: f64, l: f64) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    use std::f64::consts::FRAC_PI_2;

    let mut nodes = Vec::with_capacity((nx + 1) * (ny + 1));
    for i in 0..=ny {
        let z = i as f64 * (l / 2.0) / ny as f64;
        for j in 0..=nx {
            let theta = j as f64 * FRAC_PI_2 / nx as f64;
            nodes.push([r * theta.cos(), r * theta.sin(), z]);
        }
    }

    let mut connectivity = Vec::with_capacity(2 * nx * ny);
    for i in 0..ny {
        for j in 0..nx {
            // Four corners of the rectangular cell (i, j):
            //   A───B      i+1: C, D
            //   │   │      i:   A, B
            //   C───D      j:   left/right (θ)
            let a = i * (nx + 1) + j; // (i, j)
            let b = i * (nx + 1) + (j + 1); // (i, j+1)
            let c = (i + 1) * (nx + 1) + j; // (i+1, j)
            let d = (i + 1) * (nx + 1) + (j + 1); // (i+1, j+1)
            // Split along A→D diagonal: two counter-clockwise triangles.
            connectivity.push([a, b, d]);
            connectivity.push([a, d, c]);
        }
    }

    (nodes, connectivity)
}

/// Mesh the 1/4 quadrant of the MacNeal-Harder Scordelis-Lo cylindrical roof.
///
/// # Geometry
///
/// Cylindrical mid-surface: θ ∈ [0°, 40°], x ∈ [0, L/2], R=25, L=50.
///
/// Node positions: `(x, R·sin θ, R·cos θ)` where:
/// - x is the cylinder axis (horizontal)
/// - z = R·cos θ is the vertical (upward) coordinate — gravity = −z
/// - Crown (θ=0): `(x, 0, R)` — top of roof
/// - Free edge (θ=40°): `(x, R·sin 40°, R·cos 40°)`
///
/// # Node ordering
///
/// Row-major (x varies slowly, θ varies fast):
/// `node(i, j) = i*(nx+1) + j`, `i` = x-index ∈ [0, ny], `j` = θ-index ∈ [0, nx].
///
/// # Element count
///
/// `2·nx·ny` MITC3 triangles — each quad cell split along the A→D diagonal.
fn roof_quadrant_mesh(nx: usize, ny: usize) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    const R: f64 = 25.0;
    const L: f64 = 50.0;
    let theta_max = 40.0_f64.to_radians();

    // Nodes: i = axial (x) index, j = angular (θ) index.
    let mut nodes = Vec::with_capacity((nx + 1) * (ny + 1));
    for i in 0..=ny {
        let x = i as f64 * (L / 2.0) / ny as f64;
        for j in 0..=nx {
            let theta = j as f64 * theta_max / nx as f64;
            nodes.push([x, R * theta.sin(), R * theta.cos()]);
        }
    }

    // Connectivity: split each quad cell into two CCW triangles.
    let mut connectivity = Vec::with_capacity(2 * nx * ny);
    for i in 0..ny {
        for j in 0..nx {
            let a = i * (nx + 1) + j; // (i,   j)
            let b = i * (nx + 1) + (j + 1); // (i,   j+1)
            let c = (i + 1) * (nx + 1) + j; // (i+1, j)
            let d = (i + 1) * (nx + 1) + (j + 1); // (i+1, j+1)
            connectivity.push([a, b, d]);
            connectivity.push([a, d, c]);
        }
    }

    (nodes, connectivity)
}

/// Mesh the 1/4 quadrant of the MacNeal-Harder hemisphere benchmark.
///
/// # Geometry
///
/// Spherical mid-surface with 18° polar cut-out:
/// - φ (polar) ∈ [18°, 90°] — from near-pole cut-out to equator
/// - θ (azimuthal) ∈ [0°, 90°] — quarter of the full hemisphere
/// - R = 10 (fixed by the benchmark)
///
/// Node positions: `(R·sin φ·cos θ, R·sin φ·sin θ, R·cos φ)`.
///
/// # Node ordering
///
/// Row-major (φ varies slowly, θ varies fast):
/// `node(i, j) = i*(ny+1) + j`, `i` = φ-index ∈ [0, nx], `j` = θ-index ∈ [0, ny].
///
/// - φ=90° equator (i=nx): z≈0, positioned at (R·sin φ·cos θ, R·sin φ·sin θ, 0)
/// - Equator corner at (R,0,0): node (nx, 0) = nx*(ny+1), at φ=90°, θ=0°
///
/// # Element count
///
/// `2·nx·ny` MITC3 triangles — each quad cell split along the A→D diagonal.
fn hemisphere_quadrant_mesh(nx: usize, ny: usize) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    use std::f64::consts::FRAC_PI_2;
    const R: f64 = 10.0;
    let phi_min = 18.0_f64.to_radians();
    let phi_max = FRAC_PI_2; // 90°

    // Nodes: i = polar (φ) index, j = azimuthal (θ) index.
    let mut nodes = Vec::with_capacity((nx + 1) * (ny + 1));
    for i in 0..=nx {
        let phi = phi_min + i as f64 * (phi_max - phi_min) / nx as f64;
        let sin_phi = phi.sin();
        let cos_phi = phi.cos();
        for j in 0..=ny {
            let theta = j as f64 * FRAC_PI_2 / ny as f64;
            nodes.push([
                R * sin_phi * theta.cos(),
                R * sin_phi * theta.sin(),
                R * cos_phi,
            ]);
        }
    }

    // Connectivity: split each quad cell (i, j) into two CCW triangles.
    // Cell corners, where rows are φ (i) and columns are θ (j):
    //   A───B      i:   A, B     A = node(i,   j)
    //   │   │      i+1: C, D     B = node(i,   j+1)
    //   C───D                    C = node(i+1, j)
    //                             D = node(i+1, j+1)
    let mut connectivity = Vec::with_capacity(2 * nx * ny);
    for i in 0..nx {
        for j in 0..ny {
            let a = i * (ny + 1) + j; // (i,   j)
            let b = i * (ny + 1) + (j + 1); // (i,   j+1)
            let c = (i + 1) * (ny + 1) + j; // (i+1, j)
            let d = (i + 1) * (ny + 1) + (j + 1); // (i+1, j+1)
            connectivity.push([a, b, d]);
            connectivity.push([a, d, c]);
        }
    }

    (nodes, connectivity)
}

/// Antisymmetry Dirichlet BCs for the MacNeal-Harder hemisphere quadrant model.
///
/// The hemisphere quadrant (x≥0, y≥0) has two symmetry planes:
///
/// | Plane      | Physical role     | Constrained DOFs          |
/// |------------|-------------------|---------------------------|
/// | x=0 (θ=90°) | x-antisymmetry  | u_x=0, θ_y=0, θ_z=0      |
/// | y=0 (θ=0°)  | y-antisymmetry  | u_y=0, θ_x=0, θ_z=0      |
///
/// `tol` must be well below the mesh spacing so no interior node is
/// accidentally pinned as a boundary node.  On the 4×4 quadrant mesh
/// `tol=0.5` is well inside every inter-node arc-length.
fn hemisphere_antisymmetry_bcs(nodes: &[[f64; 3]], tol: f64) -> Vec<DirichletBc> {
    let mut bcs = Vec::new();
    for (node, n) in nodes.iter().enumerate() {
        let dof = |d: usize| node * 6 + d;
        let is_x0 = n[0].abs() < tol; // θ=90° meridian
        let is_y0 = n[1].abs() < tol; // θ=0°  meridian
        if is_x0 {
            bcs.push(DirichletBc { dof: dof(0), value: 0.0 }); // u_x = 0
            bcs.push(DirichletBc { dof: dof(4), value: 0.0 }); // θ_y = 0
            bcs.push(DirichletBc { dof: dof(5), value: 0.0 }); // θ_z = 0
        }
        if is_y0 {
            bcs.push(DirichletBc { dof: dof(1), value: 0.0 }); // u_y = 0
            bcs.push(DirichletBc { dof: dof(3), value: 0.0 }); // θ_x = 0
            bcs.push(DirichletBc { dof: dof(5), value: 0.0 }); // θ_z = 0
        }
    }
    bcs
}

/// Locate the equator corner (R, 0, 0) in the hemisphere mesh.
///
/// Returns the index of the node satisfying
/// `|x − R| < tol  &&  |y| < tol  &&  |z| < tol`.
///
/// # Panics
///
/// Panics if no such node exists — use a `tol` well below the mesh spacing.
fn hemisphere_load_node(nodes: &[[f64; 3]], r: f64, tol: f64) -> usize {
    nodes
        .iter()
        .position(|n| (n[0] - r).abs() < tol && n[1].abs() < tol && n[2].abs() < tol)
        .expect("hemisphere_load_node: (R, 0, 0) not found in hemisphere mesh")
}

/// Mesh the helicoid mid-surface of the MacNeal-Harder twisted cantilever beam.
///
/// # Geometry
///
/// The beam runs along z from 0 to L=12. Each cross-section at height `z` is a
/// line of width w=1.1, rotated about the z-axis by angle α(z) = (π/2)·z/L.
///
/// Node positions: `(s·cos α, s·sin α, z)` where `s ∈ [−w/2, +w/2]`.
///
/// # Node ordering
///
/// Row-major (z varies slowly, width varies fast):
/// `node(i, j) = i*(ny+1) + j`, `i` = z-index ∈ [0, nz], `j` = width-index ∈ [0, ny].
///
/// - Root (i=0, z=0): nodes 0..=ny, at (s·cos 0, s·sin 0, 0) = (s, 0, 0).
/// - Tip (i=nz, z=L): nodes nz*(ny+1)..=nz*(ny+1)+ny, at (s·cos(π/2), s·sin(π/2), L)
///   = (0, s, L).
/// - Centroid at root (j=ny/2, s=0): (0, 0, 0). Centroid at tip: (0, 0, L).
///
/// # Element count
///
/// `2·nz·ny` MITC3 triangles — each quad cell split along the A→D diagonal.
fn twisted_beam_mesh(nz: usize, ny: usize) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    use std::f64::consts::FRAC_PI_2;
    const L: f64 = 12.0;
    const W: f64 = 1.1;

    // Nodes: i = z-index, j = width-index.
    let mut nodes = Vec::with_capacity((nz + 1) * (ny + 1));
    for i in 0..=nz {
        let z = i as f64 * L / nz as f64;
        let alpha = FRAC_PI_2 * z / L; // twist angle: 0 at root, π/2 at tip
        for j in 0..=ny {
            let s = (j as f64 / ny as f64 - 0.5) * W; // width param: -W/2..+W/2
            nodes.push([s * alpha.cos(), s * alpha.sin(), z]);
        }
    }

    // Connectivity: split each quad cell (i, j) into two CCW triangles.
    let mut connectivity = Vec::with_capacity(2 * nz * ny);
    for i in 0..nz {
        for j in 0..ny {
            let a = i * (ny + 1) + j; // (i,   j)
            let b = i * (ny + 1) + (j + 1); // (i,   j+1)
            let c = (i + 1) * (ny + 1) + j; // (i+1, j)
            let d = (i + 1) * (ny + 1) + (j + 1); // (i+1, j+1)
            connectivity.push([a, b, d]);
            connectivity.push([a, d, c]);
        }
    }

    (nodes, connectivity)
}

// ─── step-3: Scordelis-Lo roof (MacNeal-Harder §3.4) ─────────────────────────

/// Scordelis-Lo roof smoke test — geometry from MacNeal-Harder (1985) §3.4.
///
/// **NOT a validated benchmark** — see file-level docs. Asserts only that
/// the response is finite, signed, and within a wide order-of-magnitude
/// band; today's bare MITC3 element under-predicts the published reference
/// by ~21× due to membrane locking on curved geometry.
///
/// A cylindrical roof shell (R=25, L=50, t=0.25, semi-angle 80°, E=4.32×10⁸,
/// ν=0.0) loaded by self-weight (gravity 90 per unit area in the −z direction).
///
/// # Coordinate system
///
/// - x: cylinder axis (horizontal)
/// - z: vertical (upward); gravity = −z
/// - Node position: `(x, R·sin θ, R·cos θ)`
///   - Crown (θ=0): `(x, 0, R)` — top of roof, y=0
///   - Free edge (θ=40°): `(x, R·sin 40°, R·cos 40°)` — side of roof
///
/// # Quadrant model: θ ∈ [0°,40°], x ∈ [0, L/2]
///
/// The full roof has diaphragm supports at x=0 AND x=L=50 (both ends), with
/// maximum deflection at the longitudinal center (x=L/2=25). The 1/4 model
/// exploits x-symmetry (x ∈ [0, L/2]) and θ-symmetry (θ ∈ [0°, 40°]):
///
/// | Plane | Physical role | Constrained DOFs |
/// |-------|---------------|-----------------|
/// | x=0   | Rigid end diaphragm (full-model support) | u_x=0, u_z=0, θ_y=0 |
/// | y=0 (θ=0) | Crown / longitudinal mid-plane symmetry | u_y=0, θ_x=0, θ_z=0 |
/// | x=L/2 | Longitudinal midspan (x-symmetry) | u_x=0, θ_y=0 |
/// | θ=40° | Free longitudinal edge | (none) |
///
/// The point of maximum deflection is the free-edge center: x=L/2, θ=40°.
/// The x=L/2 plane is a symmetry plane (u_x=0 antisymmetric, θ_y=0 symmetric)
/// but the vertical displacement u_z is FREE and takes its maximum value there.
///
/// # Smoke-test envelope
///
/// Today's coarse-mesh MITC3 vertical deflection at the free-edge
/// longitudinal center is **~1.4×10⁻²** (bare MITC3, 4×4 quadrant,
/// downward). The published MacNeal-Harder (1985) reference is **0.3024**
/// (a ~21× gap due to membrane locking on curved geometry; only curved-
/// element MITC3+ closes the gap — flat-facet bubble enrichment is
/// mathematically inert, see the file header in `shell_assembly.rs`).
///
/// This test asserts only sign / finiteness / order-of-magnitude — see the
/// pinched-cylinder smoke test above for the same rationale.
#[test]
fn scordelis_lo_roof_quadrant_smoke_test_vertical_deflection_is_finite_and_downward() {
    const R: f64 = 25.0;
    const L: f64 = 50.0;
    const T: f64 = 0.25;
    const NX: usize = 4; // angular (θ) divisions
    const NY: usize = 4; // axial (x) divisions
    const G: f64 = 90.0; // gravity load per unit area
    const MACNEAL_HARDER_REF: f64 = 0.3024;

    let mat = IsotropicElastic {
        youngs_modulus: 4.32e8,
        poisson_ratio: 0.0,
    };

    let (nodes, connectivity) = roof_quadrant_mesh(NX, NY);
    let n_nodes = nodes.len();

    // Build per-element stiffness via the production code path.
    let stiffness = build_shell_stiffnesses(&nodes, &connectivity, T, &mat);
    let elements = assembly_elements_for(&connectivity, &stiffness);

    // Lumped gravity load: per-element area × G / 3 per node, in −z (DOF 2).
    let mut point_loads: Vec<(usize, f64)> = Vec::new();
    for conn in &connectivity {
        let a = nodes[conn[0]];
        let b = nodes[conn[1]];
        let c = nodes[conn[2]];
        // Triangle area = |AB × AC| / 2.
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let area = (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt() / 2.0;
        let f_node = -area * G / 3.0; // downward (−z)
        for &n in conn.iter() {
            point_loads.push((n * 6 + 2, f_node));
        }
    }

    // BCs: detect nodes by position.
    // tol is chosen well within each mesh spacing (spacing ≈ L/2/NY ≈ 6.25 in x;
    // arc-length ≈ R·40°·π/180/NX ≈ 4.36 in θ-direction).
    let tol = 0.5_f64;
    let mut bcs: Vec<DirichletBc> = Vec::new();

    for (node, n) in nodes.iter().enumerate() {
        let dof = |d: usize| node * 6 + d;
        let is_crown = n[1].abs() < tol; // y ≈ 0: θ=0 crown symmetry
        let is_diaphragm = n[0].abs() < tol; // x=0: rigid end diaphragm
        let is_midspan = (n[0] - L / 2.0).abs() < tol; // x=L/2: longitudinal midspan symmetry

        // Rigid end diaphragm at x=0 (one end of the full doubly-supported roof).
        //
        // MacNeal-Harder "rigid end diaphragm": the end cross-section is a rigid
        // plate in the yz-plane supported on axial rollers. The plate constrains
        // in-plane (yz) displacement but allows axial (x) sliding:
        //   u_y = 0  (circumferential: cross-section cannot spread sideways)
        //   u_z = 0  (vertical: cross-section cannot sink)
        //   u_x = FREE (axial: the plate slides freely along the cylinder axis)
        //
        // Note: the axial rigid-body mode is eliminated by the midspan symmetry
        // condition u_x=0 at x=L/2, not by this diaphragm constraint.
        if is_diaphragm {
            bcs.push(DirichletBc {
                dof: dof(1),
                value: 0.0,
            }); // u_y = 0 (circumferential)
            bcs.push(DirichletBc {
                dof: dof(2),
                value: 0.0,
            }); // u_z = 0 (vertical)
        }
        // Crown / longitudinal mid-plane symmetry (θ=0).
        if is_crown {
            bcs.push(DirichletBc {
                dof: dof(1),
                value: 0.0,
            }); // u_y = 0
            bcs.push(DirichletBc {
                dof: dof(3),
                value: 0.0,
            }); // θ_x = 0
            bcs.push(DirichletBc {
                dof: dof(5),
                value: 0.0,
            }); // θ_z = 0
        }
        // Longitudinal midspan symmetry at x=L/2.
        //
        // x=L/2 is the center of the full roof. For symmetric gravity loading:
        //   u_x(x=L/2) = 0  (axial displacement antisymmetric about midspan)
        //   θ_y(x=L/2) = 0  (rotation symmetric about midspan)
        //
        // Crucially, u_z is NOT constrained here — the midspan vertical
        // displacement is the quantity we measure (it equals the max deflection).
        if is_midspan {
            bcs.push(DirichletBc {
                dof: dof(0),
                value: 0.0,
            }); // u_x = 0 (antisymmetry)
            bcs.push(DirichletBc {
                dof: dof(4),
                value: 0.0,
            }); // θ_y = 0 (symmetry)
        }
    }

    let u = solve_shell_system(&elements, n_nodes, &bcs, &point_loads);

    // Free-edge longitudinal center: x=L/2, θ=40° → position (L/2, R·sin40°, R·cos40°).
    // This is the point of maximum vertical deflection in the doubly-supported roof.
    let theta_40 = 40.0_f64.to_radians();
    let target_x = L / 2.0;
    let target_y = R * theta_40.sin();
    let target_z = R * theta_40.cos();
    let free_edge_center = nodes
        .iter()
        .position(|n| {
            (n[0] - target_x).abs() < tol
                && (n[1] - target_y).abs() < 1.0
                && (n[2] - target_z).abs() < 1.0
        })
        .expect("free-edge center node (x=L/2, θ=40°) not found in mesh");

    // Gravity loads −z ⇒ u_z < 0.  Report downward deflection (positive).
    let vert_defl = -u[free_edge_center * 6 + 2];

    // Smoke-test envelope spans today's locked output (~1.4e-2) and the
    // published reference (0.3024). See pinched_cylinder rationale.
    //   floor = 1e-4 → 140× margin below observed locked output
    //   ceil  = 5.0  → 16× margin above published reference
    const SMOKE_FLOOR: f64 = 1.0e-4;
    const SMOKE_CEIL: f64 = 5.0;
    assert!(
        vert_defl.is_finite(),
        "Scordelis-Lo smoke test: vert_defl = {vert_defl} is not finite"
    );
    assert!(
        vert_defl > 0.0,
        "Scordelis-Lo smoke test: vert_defl = {vert_defl:.4e} \
         must be positive (downward) under gravity load; sign reversal \
         indicates a BC or load-direction bug"
    );
    assert!(
        vert_defl > SMOKE_FLOOR && vert_defl < SMOKE_CEIL,
        "Scordelis-Lo smoke test: vert_defl = {vert_defl:.4e} outside \
         envelope [{SMOKE_FLOOR:.0e}, {SMOKE_CEIL:.0e}]. Today's locked \
         MITC3 4×4 output is ~1.4e-2; published MacNeal-Harder reference \
         is {MACNEAL_HARDER_REF:.4e}. A correctness fix should land inside \
         this band, not outside it."
    );
}

// ─── step-7: hemisphere with point loads (MacNeal-Harder §3.5) ──────────────

/// Hemisphere-with-point-loads smoke test — geometry from MacNeal-Harder
/// (1985) §3.5.
///
/// **NOT a validated benchmark** — see file-level docs. Asserts only that
/// the response is finite, signed, and within a wide order-of-magnitude
/// band; today's bare MITC3 element under-predicts the published reference
/// by ~2200× due to severe membrane locking on this very thin shell
/// (R/t=250).
///
/// A hemispherical shell (R=10, t=0.04, E=6.825×10⁷, ν=0.3) with an 18°
/// polar cut-out, loaded by alternating ±P=±2 point loads along the x and y
/// axes at the equator.
///
/// # Quadrant model: φ ∈ [18°,90°], θ ∈ [0°,90°]
///
/// Two symmetry planes eliminate 3/4 of the geometry:
///
/// | Plane | Physical role | Constrained DOFs |
/// |-------|---------------|-----------------|
/// | x=0 (θ=90°) | x-antisymmetry | u_x=0, θ_y=0, θ_z=0 |
/// | y=0 (θ=0°)  | y-antisymmetry | u_y=0, θ_x=0, θ_z=0 |
///
/// The 18° polar cut-out edge and the equator edge (φ=90°) are both free.
///
/// # Loading
///
/// Full model: ±P=±2 alternating loads at the four equator points. In the 1/4
/// quadrant model, the equator corner at (R,0,0) receives P/4 = 0.5 in the
/// +x direction (outward radial load). The x=0 symmetry plane carries the
/// equal-and-opposite −P contribution from the other half, so only a P/4 net
/// load appears in the quadrant model.
///
/// # Smoke-test envelope
///
/// Today's coarse-mesh MITC3 radial displacement at the loaded equator
/// corner is **~4.3×10⁻⁵** (bare MITC3, 4×4 quadrant, outward).
/// The published MacNeal-Harder (1985) reference is **0.0940** (a ~2200×
/// gap due to severe membrane locking on this very thin shell, R/t=250;
/// only curved-element MITC3+ closes the gap — flat-facet bubble
/// enrichment is mathematically inert, see the file header in
/// `shell_assembly.rs`).
///
/// This test asserts only sign / finiteness / order-of-magnitude — see the
/// pinched-cylinder smoke test for the same rationale.
#[test]
fn hemisphere_with_point_loads_smoke_test_radial_displacement_is_finite_and_outward() {
    const R: f64 = 10.0;
    const T: f64 = 0.04;
    const NX: usize = 4; // polar angle (φ) divisions
    const NY: usize = 4; // azimuthal angle (θ) divisions
    const P: f64 = 2.0; // full load magnitude per load point
    const MACNEAL_HARDER_REF: f64 = 0.0940;

    let mat = IsotropicElastic {
        youngs_modulus: 6.825e7,
        poisson_ratio: 0.3,
    };

    // RED: `hemisphere_quadrant_mesh` is not yet defined — compile error expected.
    let (nodes, connectivity) = hemisphere_quadrant_mesh(NX, NY);
    let n_nodes = nodes.len();

    // Build per-element stiffness via the production code path.
    let stiffness = build_shell_stiffnesses(&nodes, &connectivity, T, &mat);
    let elements = assembly_elements_for(&connectivity, &stiffness);

    // BCs: x=0/y=0 antisymmetry.  tol=0.5 is well inside every inter-node
    // arc-length on the 4×4 mesh (≈ 3.1 in the φ-direction).
    let tol = 0.5_f64;
    let bcs = hemisphere_antisymmetry_bcs(&nodes, tol);

    // Load: P/4 in the +x direction at the equator corner (R, 0, 0).
    // The radial direction at (R, 0, 0) is +x, so F_x = +P/4 (outward).
    let load_node = hemisphere_load_node(&nodes, R, tol);
    let point_loads = vec![(load_node * 6 + 0, P / 4.0)]; // F_x = +P/4

    let u = solve_shell_system(&elements, n_nodes, &bcs, &point_loads);

    // Radial displacement at (R, 0, 0): the radial direction is +x at this
    // equator point, so the radial displacement = u_x (positive = outward).
    let radial_disp = u[load_node * 6 + 0];

    // Smoke-test envelope spans today's locked output (~4.3e-5) and the
    // published reference (0.0940). The huge ~2200× gap is the well-known
    // bare-MITC3 hemisphere result; envelope is intentionally wide enough
    // to absorb a future curved-element MITC3+ correctness fix without
    // regression.
    //   floor = 1e-7 → 430× margin below observed locked output
    //   ceil  = 1.0  → 11× margin above published reference
    const SMOKE_FLOOR: f64 = 1.0e-7;
    const SMOKE_CEIL: f64 = 1.0;
    assert!(
        radial_disp.is_finite(),
        "hemisphere smoke test: radial_disp = {radial_disp} is not finite"
    );
    assert!(
        radial_disp > 0.0,
        "hemisphere smoke test: radial_disp = {radial_disp:.4e} \
         must be positive (outward) under +F_x load; sign reversal \
         indicates a BC or load-direction bug"
    );
    assert!(
        radial_disp > SMOKE_FLOOR && radial_disp < SMOKE_CEIL,
        "hemisphere smoke test: radial_disp = {radial_disp:.4e} outside \
         envelope [{SMOKE_FLOOR:.0e}, {SMOKE_CEIL:.0e}]. Today's locked \
         MITC3 4×4 output is ~4.3e-5; published MacNeal-Harder reference \
         is {MACNEAL_HARDER_REF:.4e}. A correctness fix should land inside \
         this band, not outside it."
    );
}

// ─── step-9: twisted beam (MacNeal-Harder §3.6) ──────────────────────────────

/// Twisted cantilever beam smoke test — geometry from MacNeal-Harder
/// (1985) §3.6.
///
/// **NOT a validated benchmark** — see file-level docs. Asserts only that
/// the response is finite, signed, and within a wide order-of-magnitude
/// band; today's bare MITC3 element under-predicts the published reference
/// by ~1.7× (much milder than the curved benchmarks because the helicoid's
/// elements are nearly planar).
///
/// A flat strip twisted uniformly 90° about its long axis from root to tip,
/// clamped at the root and loaded by a transverse point load at the tip.
/// Parameters: L=12, w=1.1, t=0.32, E=29×10⁶, ν=0.22.
///
/// # Geometry
///
/// The mid-surface is a helicoid:
/// - Root (z=0): cross-section along +x, mid-surface in the xz-plane.
/// - Tip (z=L): cross-section along +y (rotated 90°), mid-surface in the yz-plane.
/// - Twist angle: α(z) = (π/2) · z/L.
/// - Node position: `(s·cos α, s·sin α, z)` where s ∈ [−w/2, +w/2].
///
/// # Load case: out-of-plane tip load (MacNeal-Harder reference = 1.754×10⁻³)
///
/// "Out-of-plane" is defined relative to the root cross-section: the root's
/// mid-surface is in the xz-plane, so the out-of-plane direction is +y.
/// Applying F_y = 1.0 at the tip (distributed equally among the ny+1 tip nodes)
/// produces a transverse bending + twisting response.
///
/// The measurement: u_y at the tip centroid (s=0, z=L) → node at (0,0,L).
///
/// # Smoke-test envelope
///
/// Today's coarse-mesh MITC3 out-of-plane tip displacement is
/// **~1.0×10⁻³** (bare MITC3, 12×2). The published MacNeal-Harder
/// (1985) reference is **1.754×10⁻³** (only a ~1.7× gap — best-performing
/// of the four geometries because the helicoid elements are nearly planar
/// and the assumed-strain MITC technique is effective there).
///
/// This test asserts only sign / finiteness / order-of-magnitude — see the
/// pinched-cylinder smoke test for the same rationale.
#[test]
fn twisted_beam_tip_out_of_plane_load_smoke_test_displacement_is_finite_and_signed() {
    const L: f64 = 12.0;
    const NZ: usize = 12; // segments along z
    const NY: usize = 2; // strips across width
    const MACNEAL_HARDER_REF: f64 = 1.754e-3;

    let mat = IsotropicElastic {
        youngs_modulus: 29.0e6,
        poisson_ratio: 0.22,
    };
    let thickness = 0.32;

    // RED: `twisted_beam_mesh` is not yet defined — compile error expected.
    let (nodes, connectivity) = twisted_beam_mesh(NZ, NY);
    let n_nodes = nodes.len();

    // Build per-element stiffness via the production code path.
    let stiffness = build_shell_stiffnesses(&nodes, &connectivity, thickness, &mat);
    let elements = assembly_elements_for(&connectivity, &stiffness);

    // BCs: fully clamp every z=0 node (all 6 DOFs = 0).
    // z-spacing = L/NZ = 1.0; tol = 0.1 is safely inside the first strip.
    let tol = 0.1_f64;
    let mut bcs: Vec<DirichletBc> = Vec::new();

    for (node, n) in nodes.iter().enumerate() {
        if n[2].abs() < tol {
            // Root (z=0): clamp all 6 DOFs.
            for dof_idx in 0..6_usize {
                bcs.push(DirichletBc {
                    dof: node * 6 + dof_idx,
                    value: 0.0,
                });
            }
        }
    }

    // Load: F_y = 1.0 distributed equally among the NY+1 tip nodes.
    // Each tip node receives F_y = 1.0 / (NY+1).
    // The out-of-plane direction at the root is +y, making F_y the
    // "out-of-plane" load case per the MacNeal-Harder convention.
    let tip_f = 1.0 / (NY + 1) as f64;
    let mut point_loads: Vec<(usize, f64)> = Vec::new();
    for (node, n) in nodes.iter().enumerate() {
        if (n[2] - L).abs() < tol {
            point_loads.push((node * 6 + 1, tip_f)); // F_y at each tip node
        }
    }

    let u = solve_shell_system(&elements, n_nodes, &bcs, &point_loads);

    // Tip centroid: node at (0, 0, L) — s=0 (centroid), z=L.
    // At z=L (α=90°): position = (s·cos 90°, s·sin 90°, L) = (0, 0, L) for s=0.
    let centroid_node = nodes
        .iter()
        .position(|n| (n[2] - L).abs() < tol && n[0].abs() < tol && n[1].abs() < tol)
        .expect("tip centroid node (0, 0, L) not found in twisted-beam mesh");

    // Out-of-plane displacement = u_y at tip centroid.
    let tip_defl = u[centroid_node * 6 + 1];

    // Smoke-test envelope spans today's locked output (~1.0e-3) and the
    // published reference (1.754e-3). The gap is small here, so the band
    // is correspondingly narrower than the curved-shell benchmarks.
    //   floor = 1e-5 → 100× margin below observed locked output
    //   ceil  = 1e-1 → 57× margin above published reference
    const SMOKE_FLOOR: f64 = 1.0e-5;
    const SMOKE_CEIL: f64 = 1.0e-1;
    assert!(
        tip_defl.is_finite(),
        "twisted beam smoke test: tip_defl = {tip_defl} is not finite"
    );
    assert!(
        tip_defl > 0.0,
        "twisted beam smoke test: tip_defl = {tip_defl:.4e} \
         must be positive (in +F_y direction) under +F_y tip load; \
         sign reversal indicates a BC or load-direction bug"
    );
    assert!(
        tip_defl > SMOKE_FLOOR && tip_defl < SMOKE_CEIL,
        "twisted beam smoke test: tip_defl = {tip_defl:.4e} outside \
         envelope [{SMOKE_FLOOR:.0e}, {SMOKE_CEIL:.0e}]. Today's locked \
         MITC3 12×2 output is ~1.0e-3; published MacNeal-Harder reference \
         is {MACNEAL_HARDER_REF:.4e}. A correctness fix should land inside \
         this band, not outside it."
    );
}

/// MITC3+ observable signal: on the MacNeal-Harder twisted cantilever, the
/// genuine flat-facet MITC3+ element ([`shell_element_stiffness_mitc3_plus`])
/// must produce a tip out-of-plane deflection that is STRICTLY CLOSER to the
/// published reference (1.754×10⁻³) than bare flat-facet MITC3 on the identical
/// mesh / BCs / loads — the relative shear-locking-relief deliverable for task
/// 3392 (the absolute ~50% MacNeal-Harder bound needs the curved substrate +
/// membrane cure of task 4065 and is NOT chased here).
#[test]
fn twisted_beam_mitc3_plus_tip_deflection_is_closer_to_reference_than_bare() {
    const L: f64 = 12.0;
    const NZ: usize = 12;
    const NY: usize = 2;
    const MACNEAL_HARDER_REF: f64 = 1.754e-3;

    let mat = IsotropicElastic {
        youngs_modulus: 29.0e6,
        poisson_ratio: 0.22,
    };
    let thickness = 0.32;

    let (nodes, connectivity) = twisted_beam_mesh(NZ, NY);
    let n_nodes = nodes.len();
    let tol = 0.1_f64;

    // BCs: fully clamp every z=0 root node (all 6 DOFs = 0).
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for (node, n) in nodes.iter().enumerate() {
        if n[2].abs() < tol {
            for dof_idx in 0..6_usize {
                bcs.push(DirichletBc {
                    dof: node * 6 + dof_idx,
                    value: 0.0,
                });
            }
        }
    }

    // Load: F_y = 1.0 distributed equally among the NY+1 tip nodes (the root
    // out-of-plane direction is +y).
    let tip_f = 1.0 / (NY + 1) as f64;
    let mut point_loads: Vec<(usize, f64)> = Vec::new();
    for (node, n) in nodes.iter().enumerate() {
        if (n[2] - L).abs() < tol {
            point_loads.push((node * 6 + 1, tip_f));
        }
    }

    let centroid_node = nodes
        .iter()
        .position(|n| (n[2] - L).abs() < tol && n[0].abs() < tol && n[1].abs() < tol)
        .expect("tip centroid node (0, 0, L) not found in twisted-beam mesh");

    // Bare flat-facet MITC3.
    let k_bare = build_shell_stiffnesses(&nodes, &connectivity, thickness, &mat);
    let e_bare = assembly_elements_for(&connectivity, &k_bare);
    let u_bare = solve_shell_system(&e_bare, n_nodes, &bcs, &point_loads);
    let tip_bare = u_bare[centroid_node * 6 + 1];

    // Genuine flat-facet MITC3+ (same mesh / BCs / loads).
    let k_plus = build_shell_stiffnesses_mitc3_plus(&nodes, &connectivity, thickness, &mat);
    let e_plus = assembly_elements_for(&connectivity, &k_plus);
    let u_plus = solve_shell_system(&e_plus, n_nodes, &bcs, &point_loads);
    let tip_plus = u_plus[centroid_node * 6 + 1];

    // Observed (12×2 mesh, t=0.32): bare MITC3 tip ≈ 1.0233e-3, MITC3+ tip ≈
    // 1.1637e-3, reference 1.754e-3 → the MITC3+ shear-locking relief moves the
    // tip ~19% closer to the reference (err 7.31e-4 → 5.90e-4) without overshoot.
    let err_bare = (tip_bare - MACNEAL_HARDER_REF).abs();
    let err_plus = (tip_plus - MACNEAL_HARDER_REF).abs();

    // Finite & physically signed (positive in the +F_y direction).
    assert!(
        tip_plus.is_finite() && tip_bare.is_finite(),
        "tip deflections must be finite: bare={tip_bare}, plus={tip_plus}"
    );
    assert!(
        tip_plus > 0.0,
        "MITC3+ tip deflection {tip_plus:.4e} must be positive under +F_y load"
    );
    // Sane upper ceiling to catch runaway (the reference is 1.754e-3).
    assert!(
        tip_plus < 1.0e-1,
        "MITC3+ tip deflection {tip_plus:.4e} exceeds the runaway ceiling 1e-1"
    );
    // The shear-locking-relief deliverable: MITC3+ is STRICTLY closer to the
    // MacNeal-Harder reference than bare MITC3.
    assert!(
        err_plus < err_bare,
        "MITC3+ must be strictly closer to the MacNeal-Harder reference than bare \
         MITC3: |{tip_plus:.6e} − {MACNEAL_HARDER_REF:.6e}| = {err_plus:.6e} must be \
         < |{tip_bare:.6e} − {MACNEAL_HARDER_REF:.6e}| = {err_bare:.6e}"
    );
}

// ─── step-11: locking detection ──────────────────────────────────────────────

/// MITC3 shear-locking detection test for the pinched cylinder.
///
/// # What this test checks
///
/// For a thin pinched cylinder under a fixed radial load P, a **locking-free**
/// element produces a displacement `u_r(t)` that scales so that the normalized
/// quantity `n(t) = u_r · E · t / P` remains **bounded above a positive floor**
/// as thickness `t` decreases. A **locking** element collapses `n(t) → 0` as
/// `t → 0`, because spurious stiffness blocks the inextensional bending mode.
///
/// Three thicknesses spanning two decades (`t ∈ {1.0, 0.1, 0.01}`, with
/// `R=1, L=2, E=1, ν=0.3, P=1`) test whether MITC3 maintains a non-trivial
/// bending response across the thin-shell range.
///
/// # Why MITC3 passes this test (assumed-strain decoupling)
///
/// MITC3's assumed-strain MITC technique interpolates the transverse-shear
/// strains at the mid-side tying points and evaluates them with reduced
/// integration. This decouples the bending/transverse-shear coupling that
/// makes naive Reissner-Mindlin elements lock as t → 0 (the "parasitic
/// transverse shear" mechanism identified by Hughes & Cohen 1978). As a result:
///
/// - MITC3 successfully transitions from membrane-dominated response (thick
///   shell, t ≈ R) to bending-dominated response (thin shell, t ≪ R).
/// - n(t) **increases** as t decreases: at t=1 (R/t=1, membrane regime)
///   n ≈ 4.0; at t=0.01 (R/t=100, bending regime) n ≈ 18.9.
/// - The bending-dominated scaling u_r ~ P/(E·t³)·R² yields n ~ R²/t², so
///   the factor ~4.7 increase in n from t=1.0 to t=0.01 is physically correct.
///
/// Note: bare flat-facet MITC3 (no curved-element MITC3+) still exhibits
/// **membrane locking** on curved geometry — the flat-element approximation
/// generates spurious in-plane strains. This compresses n below the
/// analytical thin-shell reference, but does NOT collapse it to zero.
///
/// # Observed n(t) values at this mesh (4×4 octant, verified 2026-05-09)
///
/// | Thickness t | R/t | n(t) = u_r·E·t/P | Regime           |
/// |-------------|-----|------------------|------------------|
/// | 1.0         |   1 | 4.00             | thick/membrane   |
/// | 0.1         |  10 | 14.68            | transitional     |
/// | 0.01        | 100 | 18.91            | thin/bending     |
/// | ratio [2]/[0] | — | 4.73             | INCREASING (✓)   |
///
/// A naive Reissner-Mindlin element would show ratio → 0 (collapsed), not 4.73.
///
/// # Regression bounds (pure, ~1.5× safety) — esc-3034-168
///
/// The bounds below are **pure regression constants** calibrated against
/// today's observed n(t) values (see table above), not against any analytical
/// reference. Each bound provides a ~1.5× safety margin: tight enough to catch
/// a ≥1.5× regression while tolerating minor mesh/solver drift.
///
/// | Bound        | Value | Observed    | Margin |
/// |--------------|-------|-------------|--------|
/// | n floor      |  2.5  | min =  4.00 | 1.6×   |
/// | n ceiling    | 30.0  | max = 18.91 | 1.59×  |
/// | ratio floor  |  3.0  | 4.73        | 1.58×  |
///
/// The membrane-limit framing (floor = lower bound of membrane-only response)
/// is dropped: the observed minimum (4.00) is well above any membrane-only
/// response, so that physical interpretation never matched the assertion. The
/// old floor=1.0 allowed a 3× regression to slip through undetected; floor=2.5
/// closes that gap. Cross-reference: reviewer escalation `esc-3034-168`.
///
/// If a future curved-element MITC3+ formulation is added, n(t) values will
/// INCREASE toward the analytical reference — these bounds can then be
/// tightened.
#[test]
fn mitc3_thin_shell_pinched_cylinder_does_not_lock_under_decreasing_thickness() {
    // Dimensionless cylinder octant (R=1, L=2 → L/2=1 half-length).
    const R: f64 = 1.0;
    const L: f64 = 2.0;
    const NX: usize = 4; // θ-direction divisions
    const NY: usize = 4; // z-direction divisions
    const P: f64 = 1.0; // total radial load

    let (nodes, connectivity) = cylinder_octant_mesh(NX, NY, R, L);
    let n_nodes = nodes.len();

    // Build Dirichlet BCs (same logic as the pinched-cylinder smoke test).
    // These are fixed for all thickness values — only stiffness changes with t.
    // Full BC rationale lives in `pinched_cylinder_octant_symmetry_bcs`.
    let tol = 0.1_f64; // safe well inside mesh spacing (~R·π/2/NX ≈ 0.39 arc)
    let bcs = pinched_cylinder_octant_symmetry_bcs(&nodes, L, tol);

    // Load node: corner at (0, R, 0) — θ=π/2, z=0.
    let load_node = nodes
        .iter()
        .position(|n| n[0].abs() < tol && (n[1] - R).abs() < tol && n[2].abs() < tol)
        .expect("load node (0, R, 0) not found");
    let point_loads = vec![(load_node * 6 + 1, -P / 4.0)]; // F_y = -P/4 (radially inward)

    // Loop over three thicknesses spanning two decades.
    let thicknesses = [1.0_f64, 0.1, 0.01];
    let mat_e = 1.0_f64;

    let mut n_vals = [0.0_f64; 3];
    for (idx, &t) in thicknesses.iter().enumerate() {
        let mat = IsotropicElastic {
            youngs_modulus: mat_e,
            poisson_ratio: 0.3,
        };

        let stiffness = build_shell_stiffnesses(&nodes, &connectivity, t, &mat);
        let elements = assembly_elements_for(&connectivity, &stiffness);

        let u = solve_shell_system(&elements, n_nodes, &bcs, &point_loads);
        let u_r = -u[load_node * 6 + 1]; // radial inward displacement (positive)
        n_vals[idx] = u_r * mat_e * t / P; // normalized dimensionless response
    }

    // ── Assertions (pure regression bounds, ~1.5× safety margin) ────────────────
    //
    // Bounds calibrated against observed values (4×4 octant, R=1, L=2, E=1, ν=0.3, P=1):
    //   n(t=1.0)  = 4.00   →  floor=2.5  gives 1.6× margin below observed min
    //   n(t=0.1)  = 14.68
    //   n(t=0.01) = 18.91  →  ceiling=30 gives 1.59× margin above observed max
    //   ratio     = 4.73   →  ratio_floor=3.0 gives 1.58× margin
    //
    // These are pure regression pins (not derived from any analytical reference).
    // See doc-comment section "Regression bounds (pure, ~1.5× safety)" for rationale.
    for (idx, &t) in thicknesses.iter().enumerate() {
        let n = n_vals[idx];
        assert!(
            n > 2.5,
            "locking floor violated at t={t}: n(t)={n:.4e} < floor=2.5 \
             (regression bound: 1.6× below observed MITC3 minimum 4.00 at t=1.0; \
             esc-3034-168). A naive Reissner-Mindlin element collapses n→0 at thin t."
        );
        assert!(
            n < 30.0,
            "locking ceiling violated at t={t}: n(t)={n:.4e} > ceiling=30.0 \
             (regression bound: 1.59× above observed MITC3 maximum 18.91 at t=0.01; \
             esc-3034-168). Indicates NaN, runaway, or a sign error in the assembly."
        );
    }
    // Ratio assertion: n(thin)/n(thick) must stay above 3.0.
    //
    // Physically: MITC3 transitions from membrane-dominated (n≈4 at t=1.0) to
    // bending-dominated (n≈18.9 at t=0.01) response — n INCREASES with thinner
    // shells. A locking element produces n(thin) < n(thick), making ratio < 1.
    // Observed ratio = 4.73; floor=3.0 is a 1.58× regression bound (esc-3034-168).
    let ratio = n_vals[2] / n_vals[0]; // n(t=0.01) / n(t=1.0)
    assert!(
        ratio > 3.0,
        "locking detected: n(0.01)/n(1.0) = {ratio:.4e} < floor=3.0 \
         (regression bound: 1.58× below observed ratio 4.73; esc-3034-168). \
         MITC3 should increase n as shell thins; ratio < 1 indicates spurious \
         stiffness blocking the bending-dominated response. \
         Observed: t=1.0→{:.4e}, t=0.1→{:.4e}, t=0.01→{:.4e}",
        n_vals[0],
        n_vals[1],
        n_vals[2],
    );
}

// ─── step-1: sanity check ────────────────────────────────────────────────────

/// Sanity check for the end-to-end shell pipeline.
///
/// A 2-element flat unit-square cantilever clamped at x=0 with a transverse
/// (+z) tip point load at the free edge (x=1) must develop a positive
/// transverse displacement at each free-edge node.
///
/// Mesh layout (XY plane):
/// ```text
///   2---3
///   |\ |
///   | \|
///   0---1
/// ```
/// - Nodes: 0:(0,0,0), 1:(1,0,0), 2:(0,1,0), 3:(1,1,0)
/// - Elements: [0,1,3] and [0,3,2]
/// - BCs: clamp nodes 0 and 2 (x=0 edge), all 6 DOFs fixed
/// - Load: Fz = 1 at nodes 1 and 3 (x=1 edge)
#[test]
fn flat_plate_cantilever_under_tip_load_displaces_in_load_direction() {
    let nodes: Vec<[f64; 3]> = vec![
        [0.0, 0.0, 0.0], // node 0 — clamped
        [1.0, 0.0, 0.0], // node 1 — free, loaded
        [0.0, 1.0, 0.0], // node 2 — clamped
        [1.0, 1.0, 0.0], // node 3 — free, loaded
    ];
    let connectivity: Vec<[usize; 3]> = vec![[0, 1, 3], [0, 3, 2]];
    let mat = IsotropicElastic {
        youngs_modulus: 1e6,
        poisson_ratio: 0.3,
    };
    let thickness = 0.1;
    let n_nodes = nodes.len();

    // Build per-element stiffness matrices via the production code path.
    let stiffness = build_shell_stiffnesses(&nodes, &connectivity, thickness, &mat);
    let elements = assembly_elements_for(&connectivity, &stiffness);

    // BCs: clamp nodes 0 and 2 (x=0 edge), all 6 DOFs = 0.
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for node in [0_usize, 2_usize] {
        for dof in 0..6_usize {
            bcs.push(DirichletBc {
                dof: node * 6 + dof,
                value: 0.0,
            });
        }
    }
    // Pin θ_z (drilling rotation about the element normal) at the free
    // nodes. On this flat patch every element shares the same normal
    // e3 = (0,0,1), so the local θ_z DOF coincides with the global θ_z
    // DOF and `shell_element_stiffness` carries zero stiffness for it
    // (MITC3 has no drilling rotation; a future curved-element MITC3+ with
    // a real drilling field would add it). Without this pin, `K_global` is
    // rank-deficient on a flat patch and the LU solve produces NaN. The four
    // curved MacNeal-Harder
    // smoke tests don't need this pin because their adjacent elements
    // have different normals — the variation across normals supplies the
    // missing local θ_z constraint at each interior node.
    for node in [1_usize, 3_usize] {
        bcs.push(DirichletBc {
            dof: node * 6 + 5,
            value: 0.0,
        });
    }

    // Load: transverse (+z) unit load at free-edge nodes 1 and 3.
    let point_loads: Vec<(usize, f64)> = vec![
        (1 * 6 + 2, 1.0), // node 1, z-DOF
        (3 * 6 + 2, 1.0), // node 3, z-DOF
    ];

    // Solve: returns 6 * n_nodes displacement vector.
    let u = solve_shell_system(&elements, n_nodes, &bcs, &point_loads);

    // Sign-only assertion: free-edge nodes must deflect in the +z direction.
    assert!(
        u[1 * 6 + 2] > 0.0,
        "node 1 z-displacement must be positive under +z load (got {})",
        u[1 * 6 + 2]
    );
    assert!(
        u[3 * 6 + 2] > 0.0,
        "node 3 z-displacement must be positive under +z load (got {})",
        u[3 * 6 + 2]
    );
}

// Note: the four MacNeal-Harder smoke tests + the locking-detection test +
// the flat-plate sanity test all call `shell_element_stiffness` directly
// (no test-local drilling penalty). That closes the integration-coverage
// gap raised in review escalation `esc-3034-165`: the curved benchmarks'
// symmetry-plane BCs already pin enough θ_z DOFs that the local drilling
// kernel of MITC3 is fully constrained, and the flat-plate test pins θ_z
// at its free nodes for the same reason. No production-singularity canary
// is needed because no production singularity manifests under these
// configurations. If a future test exercises a configuration where a
// drilling kernel survives (e.g. a flat patch loaded with no θ_z BCs),
// it will fail with NaN — that natural failure mode is the canary.

// ─── unit test: pinched_cylinder_octant_symmetry_bcs helper ─────────────────

/// Unit test for `pinched_cylinder_octant_symmetry_bcs` on a 1×1 mesh.
///
/// Uses a hand-crafted 4-node mesh (the four corner nodes of a 1/8 octant
/// with R=1, L=2) to verify the **exact** set of (node, DOF) pairs returned,
/// independent of `cylinder_octant_mesh`.  This gives the helper its own
/// coverage so that a future edit cannot silently inflate `n(t)` in the
/// locking test by dropping a BC.
///
/// # Node layout
///
/// ```text
///   node  position        planes
///   ────  ─────────────   ─────────────────────
///     0   (1, 0, 0)       y=0  z=0
///     1   (0, 1, 0)       x=0  z=0
///     2   (1, 0, 1)       y=0  diaphragm (z=L/2=1)
///     3   (0, 1, 1)       x=0  diaphragm (z=L/2=1)
/// ```
///
/// # Expected pinned DOFs (after sort+dedup, all value=0)
///
/// | Node | Planes       | DOFs pinned (offset = node×6)             |
/// |------|--------------|-------------------------------------------|
/// |  0   | y=0, z=0     | 1(u_y) 2(u_z) 3(θ_x) 4(θ_y) 5(θ_z)     |
/// |  1   | x=0, z=0     | 6(u_x) 8(u_z) 9(θ_x) 10(θ_y) 11(θ_z)   |
/// |  2   | diaphragm,y=0| 13(u_y) 14(u_z) 15(θ_x) 17(θ_z)         |
/// |  3   | diaphragm,x=0| 18(u_x) 20(u_z) 22(θ_y) 23(θ_z)         |
///
/// Node 2 gets `dof(5)=θ_z` from both the diaphragm group and the y=0 group
/// — a duplicate that `solve_shell_system` deduplicates.  The unit test
/// deduplicates before asserting so the result is canonical.
///
/// Node 0 does NOT get u_x pinned (DOF 0 is free): the x=0 plane does not
/// cover node 0, which sits at x=1 (the y=0 arc endpoint).
#[test]
fn pinched_cylinder_octant_symmetry_bcs_pins_exact_dofs_on_1x1_mesh() {
    // Four corner nodes of the 1/8 octant (R=1, L=2).
    // Constructed manually so this test is independent of `cylinder_octant_mesh`.
    let nodes: Vec<[f64; 3]> = vec![
        [1.0, 0.0, 0.0], // node 0: y=0, z=0
        [0.0, 1.0, 0.0], // node 1: x=0, z=0
        [1.0, 0.0, 1.0], // node 2: y=0, diaphragm (z=L/2)
        [0.0, 1.0, 1.0], // node 3: x=0, diaphragm (z=L/2)
    ];
    let l = 2.0_f64;
    let tol = 0.01_f64; // well below min spacing = 1.0

    let mut bcs = pinched_cylinder_octant_symmetry_bcs(&nodes, l, tol);
    // Sort + dedup: corner nodes appear in multiple BC groups; canonical form
    // matches what `solve_shell_system` produces before calling
    // `apply_dirichlet_row_elimination`.
    bcs.sort_by_key(|bc| bc.dof);
    bcs.dedup_by_key(|bc| bc.dof);

    // All BCs must be homogeneous.
    for bc in &bcs {
        assert_eq!(
            bc.value, 0.0,
            "BC at DOF {} must be homogeneous (value=0); got {}",
            bc.dof, bc.value
        );
    }

    let dofs: Vec<usize> = bcs.iter().map(|bc| bc.dof).collect();

    // Expected DOF indices (see table in doc-comment above).
    let expected: Vec<usize> = vec![
        1, 2, 3, 4, 5, // node 0: y=0 + z=0 (u_y, u_z, θ_x, θ_y, θ_z)
        6, 8, 9, 10, 11, // node 1: x=0 + z=0 (u_x, u_z, θ_x, θ_y, θ_z)
        13, 14, 15, 17, // node 2: diaphragm + y=0 (u_y, u_z, θ_x, θ_z)
        18, 20, 22, 23, // node 3: diaphragm + x=0 (u_x, u_z, θ_y, θ_z)
    ];

    assert_eq!(
        dofs, expected,
        "pinched_cylinder_octant_symmetry_bcs pinned unexpected DOFs.\n\
         got:      {dofs:?}\n\
         expected: {expected:?}\n\
         (sorted, deduplicated, value=0 for all)"
    );
}

/// **Observable signal (task 4068 step-21/22).** On the bending-dominated
/// pinched-cylinder benchmark, the degenerate substrate — per-node ANALYTIC
/// radial directors + the varying Jacobian — is STRICTLY closer to the
/// MacNeal-Harder reference than flat-facet MITC3+ on the IDENTICAL mesh / BCs /
/// loads. Mirrors the proven relative template
/// `twisted_beam_mitc3_plus_tip_deflection_is_closer_to_reference_than_bare`
/// (swapping bare-vs-plus for plus-vs-degenerate).
///
/// # Relative / directional ONLY — no absolute band is claimed
///
/// The absolute ~50% / 4×4 MacNeal-Harder accuracy target stays gated on task
/// 4065's ANS-membrane. Per-node directors + a varying Jacobian REINTRODUCE
/// membrane locking (design_decisions[3]; formulation-review s3), so both
/// formulations still under-predict here — flat MITC3+ ~76× under, the
/// degenerate substrate ~6.4× under. The geometric-fidelity / membrane-bending
/// coupling the directors add is what moves the displacement toward the
/// reference; the residual lock is 4065's job to remove.
///
/// # Why the pinched cylinder (bending-dominated) and not the roof / hemisphere
///
/// A bending/faceting-dominated benchmark is where directors-alone measurably
/// help; on the membrane-dominated hemisphere (R/t=250) the reintroduced lock
/// would mask the gain — reviewers should NOT expect improvement there. The
/// Scordelis-Lo roof was evaluated empirically at 4×4 and REJECTED: under the
/// degenerate substrate its free-edge mode is sign-reversed at this coarse mesh
/// (no clean directional signal). The pinched cylinder is the more
/// director-favourable of the two bending-dominated candidates.
///
/// # Observed (4×4 octant, t=3, R=300, E=3e6, ν=0.3)
///
/// | quantity              | value      |
/// |-----------------------|------------|
/// | flat MITC3+ radial    | 2.424e-7   |
/// | degenerate radial     | 2.847e-6   |
/// | MacNeal-Harder ref    | 1.8248e-5  |
/// | err flat              | 1.8006e-5  |
/// | err degenerate        | 1.5401e-5  |
///
/// The degenerate substrate moves the radial displacement ~11.7× toward the
/// reference (~14.5% closer in absolute error) — correctly signed (inward), no
/// overshoot.
#[test]
fn degenerate_shell_pinched_cylinder_is_closer_to_reference_than_flat_mitc3_plus() {
    const R: f64 = 300.0;
    const L: f64 = 600.0;
    const T: f64 = 3.0;
    const NX: usize = 4; // θ-direction divisions
    const NY: usize = 4; // z-direction divisions
    const P: f64 = 1.0;
    const MACNEAL_HARDER_REF: f64 = 1.8248e-5;

    let mat = IsotropicElastic {
        youngs_modulus: 3e6,
        poisson_ratio: 0.3,
    };

    let (nodes, connectivity) = cylinder_octant_mesh(NX, NY, R, L);
    let n_nodes = nodes.len();

    // Analytic per-node radial directors (the extraction-supplied stand-in): the
    // cylinder axis is z, so the outward surface normal at (x, y, z) is
    // (x/R, y/R, 0). This is the curvature the flat facet cannot see.
    let directors: Vec<[f64; 3]> = nodes.iter().map(|n| normalize3([n[0], n[1], 0.0])).collect();

    // Octant symmetry + rigid-diaphragm BCs (shared with the smoke test).
    let tol = 1.0_f64;
    let bcs = pinched_cylinder_octant_symmetry_bcs(&nodes, L, tol);

    // Inward radial point load F_y = −P/4 at the loaded corner (0, R, 0).
    let load_node = nodes
        .iter()
        .position(|n| n[0].abs() < tol && (n[1] - R).abs() < tol && n[2].abs() < tol)
        .expect("load node (0, R, 0) not found in mesh");
    let point_loads = vec![(load_node * 6 + 1, -P / 4.0)];

    // Flat-facet MITC3+ (the baseline to beat).
    let k_flat = build_shell_stiffnesses_mitc3_plus(&nodes, &connectivity, T, &mat);
    let e_flat = assembly_elements_for(&connectivity, &k_flat);
    let u_flat = solve_shell_system(&e_flat, n_nodes, &bcs, &point_loads);
    let radial_flat = -u_flat[load_node * 6 + 1]; // inward = positive

    // Degenerate substrate (same mesh / BCs / loads), analytic radial directors.
    let k_deg = build_shell_stiffnesses_degenerate(&nodes, &connectivity, &directors, T, &mat);
    let e_deg = assembly_elements_for(&connectivity, &k_deg);
    let u_deg = solve_shell_system(&e_deg, n_nodes, &bcs, &point_loads);
    let radial_deg = -u_deg[load_node * 6 + 1];

    let err_flat = (radial_flat - MACNEAL_HARDER_REF).abs();
    let err_deg = (radial_deg - MACNEAL_HARDER_REF).abs();

    // Finite & physically signed (inward, positive).
    assert!(
        radial_flat.is_finite() && radial_deg.is_finite(),
        "radial displacements must be finite: flat={radial_flat}, deg={radial_deg}"
    );
    assert!(
        radial_deg > 0.0,
        "degenerate radial displacement {radial_deg:.4e} must be positive (inward) \
         under inward radial load; a sign reversal indicates a BC/load/director bug"
    );
    // Runaway ceiling (the reference is 1.8248e-5; degenerate still under-predicts).
    assert!(
        radial_deg < 1.0e-3,
        "degenerate radial displacement {radial_deg:.4e} exceeds the runaway ceiling 1e-3"
    );
    // The substrate deliverable: the degenerate element is STRICTLY closer to the
    // MacNeal-Harder reference than flat-facet MITC3+ on the identical mesh.
    assert!(
        err_deg < err_flat,
        "degenerate substrate must be strictly closer to the MacNeal-Harder \
         reference than flat MITC3+: |{radial_deg:.6e} − {MACNEAL_HARDER_REF:.6e}| = \
         {err_deg:.6e} must be < |{radial_flat:.6e} − {MACNEAL_HARDER_REF:.6e}| = \
         {err_flat:.6e}"
    );
}

// ─── task 4065 pre-refinement baselines (prereq-1) ───────────────────────────
//
// The following ratios were measured against the DELIBERATELY-MINIMAL
// centroid-only ANS-membrane of task 4069 (commit 0e41f75554) composing the
// full integrated stack:
//
//   build_shell_stiffnesses_degenerate_ans
//     → shell_element_stiffness_degenerate_ans         (shell_assembly.rs:1104)
//       → degenerate_stiffness_core(assumed_membrane=true)     (shell_assembly.rs:858)
//           ├─ 4068 substrate: degenerate_membrane_bending_b (varying J, per-node V_i)
//           ├─ 4069 ANS-membrane: degenerate_assumed_membrane_b (centroid covariant)
//           └─ carried 3392 MITC3+ shear: degenerate_transverse_shear_b (interior tying)
//
// Pre-refinement absolute ratios (computed / MacNeal-Harder reference):
//   Pinched cylinder 4×4: ~3.359e-6 / 1.8248e-5 → ratio ≈ 0.184 (5.43× under)
//   Hemisphere 4×4:       ~9.4e-3   / 0.0940    → ratio ≈ 0.10  (~10× under)
//
// The pinched-cylinder absolute band test (step-3/4) asserts the refined stack
// reaches ratio ∈ [0.5, 2.0] (Bathe–Lee 2014, ~50%/4×4).  The hemisphere
// honest-report test (step-5/6) records the honest post-refinement ratio for
// FE review per design doc §6.
// ─────────────────────────────────────────────────────────────────────────────

/// Pre-refinement pinched-cylinder ratio (computed/ref) with the centroid-only
/// ANS membrane (task 4069 baseline, commit 0e41f75554).  Used as the "before"
/// anchor in the step-4 absolute-band test to confirm the refinement closes the
/// gap.
const PRE_REFINEMENT_PINCHED_RATIO: f64 = 0.184;

/// Pre-refinement hemisphere ratio (computed/ref) with the centroid-only ANS
/// membrane (task 4069 baseline).  The post-refinement honest ratio is compared
/// against this in the step-6 honest-report to confirm the refinement strictly
/// improves double-curvature accuracy.
const PRE_REFINEMENT_HEMISPHERE_RATIO: f64 = 0.10;

/// **Observable signal (task 4069 step-13/14).** Activating the
/// assumed-natural-strain **membrane** field on the curved degenerate substrate
/// — [`shell_element_stiffness_degenerate_ans`] — moves the pinched-cylinder
/// radial displacement STRICTLY closer to the MacNeal-Harder reference than the
/// non-ANS degenerate substrate ([`shell_element_stiffness_degenerate`], task
/// 4068) on the IDENTICAL mesh / BCs / loads / directors. This is the
/// membrane-locking cure: per-node directors + a varying Jacobian REINTRODUCE
/// membrane locking (the displacement-based membrane strain carries parasitic
/// curvature-coupled energy that over-stiffens the inextensional bending mode);
/// the ANS membrane field re-interpolates the covariant membrane strain from the
/// interior tying points so that parasitic energy is filtered, softening the
/// over-stiff response toward the reference.
///
/// # Relative / directional ONLY — no absolute band is claimed
///
/// The absolute ~50% / 4×4 MacNeal-Harder accuracy band (the published
/// Bathe-Lee 2014 figure) stays gated on tasks 4065 (full integration +
/// hemisphere/pinched GREEN) and 3513 (Scordelis-Lo), per
/// `docs/architecture-audit/fea-accuracy-achievability-survey-2026-05-29.md` §6.
/// Asserting an absolute tolerance HERE would be a formulation-vs-bound false
/// premise. This test mirrors the established relative template
/// (`degenerate_shell_pinched_cylinder_is_closer_to_reference_than_flat_mitc3_plus`,
/// swapping flat-vs-degenerate for non-ANS-vs-ANS): the non-ANS degenerate
/// substrate is ~6.4× under-predicting (ample headroom for a clean directional
/// move with no overshoot), so the ANS cure can only soften toward the reference.
///
/// # Why the pinched cylinder (bending-dominated) and not the roof / hemisphere
///
/// The pinched cylinder already runs the degenerate element end-to-end and is
/// ~6.4× under — clear, well-signed headroom for a clean directional move. Per
/// task 4068's findings the Scordelis-Lo free-edge mode is sign-reversed at 4×4
/// (no clean directional signal; that GREEN is 3513's job) and the
/// membrane-dominated hemisphere (R/t=250) would let the residual lock mask the
/// gain (4065's job).
#[test]
fn degenerate_shell_ans_membrane_pinched_cylinder_moves_toward_reference() {
    const R: f64 = 300.0;
    const L: f64 = 600.0;
    const T: f64 = 3.0;
    const NX: usize = 4; // θ-direction divisions
    const NY: usize = 4; // z-direction divisions
    const P: f64 = 1.0;
    const MACNEAL_HARDER_REF: f64 = 1.8248e-5;

    let mat = IsotropicElastic {
        youngs_modulus: 3e6,
        poisson_ratio: 0.3,
    };

    let (nodes, connectivity) = cylinder_octant_mesh(NX, NY, R, L);
    let n_nodes = nodes.len();

    // Analytic per-node radial directors (the extraction-supplied stand-in): the
    // cylinder axis is z, so the outward surface normal at (x, y, z) is
    // (x/R, y/R, 0). Shared verbatim with the non-ANS degenerate benchmark so the
    // ONLY difference between the two solves is the membrane B.
    let directors: Vec<[f64; 3]> = nodes.iter().map(|n| normalize3([n[0], n[1], 0.0])).collect();

    // Octant symmetry + rigid-diaphragm BCs (shared with the smoke test).
    let tol = 1.0_f64;
    let bcs = pinched_cylinder_octant_symmetry_bcs(&nodes, L, tol);

    // Inward radial point load F_y = −P/4 at the loaded corner (0, R, 0).
    let load_node = nodes
        .iter()
        .position(|n| n[0].abs() < tol && (n[1] - R).abs() < tol && n[2].abs() < tol)
        .expect("load node (0, R, 0) not found in mesh");
    let point_loads = vec![(load_node * 6 + 1, -P / 4.0)];

    // Non-ANS degenerate substrate (the baseline to beat) — task 4068.
    let k_noans = build_shell_stiffnesses_degenerate(&nodes, &connectivity, &directors, T, &mat);
    let e_noans = assembly_elements_for(&connectivity, &k_noans);
    let u_noans = solve_shell_system(&e_noans, n_nodes, &bcs, &point_loads);
    let radial_noans = -u_noans[load_node * 6 + 1]; // inward = positive

    // ANS-membrane degenerate substrate (same mesh / BCs / loads / directors).
    let k_ans = build_shell_stiffnesses_degenerate_ans(&nodes, &connectivity, &directors, T, &mat);
    let e_ans = assembly_elements_for(&connectivity, &k_ans);
    let u_ans = solve_shell_system(&e_ans, n_nodes, &bcs, &point_loads);
    let radial_ans = -u_ans[load_node * 6 + 1];

    let err_noans = (radial_noans - MACNEAL_HARDER_REF).abs();
    let err_ans = (radial_ans - MACNEAL_HARDER_REF).abs();

    // Finite & physically signed (inward, positive) under the inward radial load.
    assert!(
        radial_noans.is_finite() && radial_ans.is_finite(),
        "radial displacements must be finite: noans={radial_noans}, ans={radial_ans}"
    );
    assert!(
        radial_ans > 0.0,
        "ANS-degenerate radial displacement {radial_ans:.4e} must be positive (inward) \
         under inward radial load; a sign reversal indicates a membrane-B/BC/load bug"
    );
    // Runaway ceiling (the reference is 1.8248e-5; the ANS cure softens toward it
    // but must NOT overshoot into a runaway).
    assert!(
        radial_ans < 1.0e-3,
        "ANS-degenerate radial displacement {radial_ans:.4e} exceeds the runaway ceiling 1e-3"
    );
    // The task 4069 deliverable: the ANS membrane field is STRICTLY closer to the
    // MacNeal-Harder reference than the non-ANS degenerate substrate on the
    // identical mesh — the directional membrane-locking-cure signal.
    assert!(
        err_ans < err_noans,
        "ANS membrane field must move the radial displacement strictly toward the \
         MacNeal-Harder reference: |{radial_ans:.6e} − {MACNEAL_HARDER_REF:.6e}| = \
         {err_ans:.6e} must be < |{radial_noans:.6e} − {MACNEAL_HARDER_REF:.6e}| = \
         {err_noans:.6e}"
    );
}

/// **RED / orientation-robustness unit test (task 4128 step-1).**
///
/// A single degenerate element on a flat CCW xy-triangle with per-node
/// directors *opposing* the in-plane winding must have non-negative diagonal
/// entries in its stiffness matrix.  This test drives the sign-fix in
/// `degenerate_stiffness_core` (task 4068 bug).
///
/// # What is asserted (and what is not)
///
/// The test checks two necessary conditions:
///   (a) node-0 translational diagonals are strictly positive — a unit displacement
///       stores positive energy;
///   (b) no diagonal is negative beyond a 1e-9·kmax relative tolerance (the
///       drilling DOF θ_z about the flat normal is legitimately zero).
///
/// Diagonal non-negativity is a necessary but not sufficient condition for PSD.
/// It is sufficient here because the specific failure mode being guarded against
/// — a global sign flip of the entire K from the signed Jacobian determinant —
/// negates *all* entries uniformly, so non-negative diagonals directly catch the
/// bug.  A full eigenvalue / Cholesky PSD check is not added to keep the test
/// self-contained (no extra linear-algebra dependencies in the test crate).
///
/// # Why this configuration forces det(J) < 0
///
/// The triangle nodes [[0,0,0],[1,0,0],[0,1,0]] are wound CCW in the xy-plane,
/// so g_ξ × g_η ∝ +z.  The per-node directors are all [0,0,-1] (−z), so
/// g_ζ = (t/2)·d = (0.05)·[0,0,-1] ∝ −z.
/// det(J) = (g_ξ × g_η)·g_ζ < 0.
///
/// On the current (bug-present) code `scale = w * det < 0`, which negates the
/// entire BᵀDB contribution, flipping K to negative-definite.  The
/// translational diagonal entries k[0,0], k[1,1], k[2,2] (DOFs 0–2 of node 0)
/// become *negative* → RED.
///
/// After the fix (`det.abs()`), those diagonals are positive and no diagonal
/// is negative beyond a 1e-9 · kmax tolerance (the drilling DOF θ_z about the
/// flat normal is legitimately zero) → GREEN.
///
/// # Premises verified before writing this test
///
/// * `mat3_inverse` has a `debug_assert!(|det|>1e-12)` — the flat Jacobian
///   |det| = 0.05 >> 1e-12, so the assert passes.
/// * `lamina_frame` asserts coplanarity and non-zero area — the CCW triangle
///   satisfies both.
/// * The test is completely deterministic (no randomness, no mesh subtleties).
#[test]
fn degenerate_element_stiffness_diagonals_stay_nonnegative_when_directors_oppose_winding() {
    // Flat CCW triangle in the xy-plane.  Winding g_ξ×g_η = +z.
    let nodes: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    // Directors −z: opposing the +z winding → det(J) = (g_ξ×g_η)·g_ζ < 0.
    let dirs: [[f64; 3]; 3] = [[0.0, 0.0, -1.0], [0.0, 0.0, -1.0], [0.0, 0.0, -1.0]];

    let th: [f64; 3] = [0.1; 3];

    let mat = IsotropicElastic {
        youngs_modulus: 1.0e3,
        poisson_ratio: 0.3,
    };

    let k_e = shell_element_stiffness_degenerate(&nodes, &dirs, &th, &mat);

    const NDOF: usize = 18; // 3 nodes × 6 DOFs/node

    // Find the maximum diagonal value (used to set a relative tolerance on
    // the drilling DOF, which is legitimately zero on a flat element).
    let kmax = (0..NDOF)
        .map(|i| k_e.data[i * NDOF + i].abs())
        .fold(0.0_f64, f64::max);

    // (a) Node-0 translational diagonals must be strictly positive.
    //     On buggy code (signed det) the entire K is negated → they are negative.
    for dof in 0..3usize {
        let diag = k_e.data[dof * NDOF + dof];
        assert!(
            diag > 0.0,
            "node-0 translational diagonal k[{dof},{dof}] = {diag:.6e} must be \
             positive; a negative value means the signed Jacobian determinant \
             negated the entire stiffness matrix (task 4068 bug)"
        );
    }

    // (b) No diagonal may be negative beyond a 1e-9·kmax tolerance.
    //     The drilling DOF θ_z (about the flat normal) is exactly 0 on a flat
    //     element; the >= bound (not >) accommodates that exact zero.
    let tol = 1.0e-9 * kmax;
    for i in 0..NDOF {
        let diag = k_e.data[i * NDOF + i];
        assert!(
            diag >= -tol,
            "diagonal k[{i},{i}] = {diag:.6e} is negative (below −{tol:.2e}); \
             stiffness diagonals must be non-negative"
        );
    }
}

/// **Hemisphere acceptance + regression test (task 4128 step-3).**
///
/// Verifies that after the |det J| measure fix, both the non-ANS degenerate
/// substrate ([`shell_element_stiffness_degenerate`]) and the ANS-membrane
/// variant ([`shell_element_stiffness_degenerate_ans`]) produce a *positive
/// (outward)* radial displacement at the equator corner (R, 0, 0) under an
/// outward +x point load.
///
/// # What is asserted
///
/// For each variant — degenerate and degenerate+ANS — the resolved u_x at the
/// loaded corner must be:
///   (a) **finite** — no NaN / Inf (no singular stiffness matrix),
///   (b) **> 0.0** — outward, matching the load direction (the task's core
///       signal: before the fix both variants gave negative / inward displacement
///       because det(J)<0 on the hemisphere mesh flipped K),
///   (c) **< 1.0** — loose runaway ceiling matching the smoke test's `SMOKE_CEIL`
///       (10× above the MacNeal-Harder reference of 0.0940; prevents a solver
///       blow-up from masquerading as a pass).
///
/// **No absolute MacNeal-Harder band is asserted here.** Absolute hemisphere
/// accuracy (the ~0.0940 target) stays gated on task 4065's ANS-membrane
/// integration; per-node directors + varying Jacobian still reintroduce membrane
/// locking on R/t=250 — the residual lock is 4065's job. Asserting an absolute
/// tolerance at this task would be a formulation-vs-bound false premise,
/// mirroring the sign-only convention of the pinched-cylinder degenerate benchmarks.
///
/// # Geometry / load
///
/// MacNeal-Harder hemisphere (§3.5): quadrant mesh (4×4), R=10, T=0.04,
/// E=6.825e7, ν=0.3, with x=0/y=0 antisymmetry BCs and an outward +x load
/// P/4=0.5 at the equator corner (R,0,0).  Mesh and BCs replicated verbatim
/// from [`hemisphere_with_point_loads_smoke_test_radial_displacement_is_finite_and_outward`].
///
/// # No-regression check
///
/// Running the complete `shell_benchmarks` suite alongside this test confirms
/// that `det.abs()` is a bitwise no-op where det>0 (cylinder octant meshes),
/// so the existing degenerate pinched-cylinder and ANS pinched-cylinder
/// benchmarks remain unchanged.
#[test]
fn degenerate_shell_hemisphere_outward_load_radial_displacement_is_outward() {
    const R: f64 = 10.0;
    const T: f64 = 0.04;
    const NX: usize = 4; // polar angle (φ) divisions
    const NY: usize = 4; // azimuthal angle (θ) divisions
    const P: f64 = 2.0; // full load magnitude per load point
    // Loose runaway ceiling: 10× above the MacNeal-Harder reference (0.0940).
    // Absolute accuracy stays gated on task 4065.
    const SMOKE_CEIL: f64 = 1.0;

    let mat = IsotropicElastic {
        youngs_modulus: 6.825e7,
        poisson_ratio: 0.3,
    };

    let (nodes, connectivity) = hemisphere_quadrant_mesh(NX, NY);
    let n_nodes = nodes.len();

    // Analytic outward radial directors: sphere centred at origin, so the
    // outward surface normal at node n is normalize(n).
    let directors: Vec<[f64; 3]> =
        nodes.iter().map(|n| normalize3([n[0], n[1], n[2]])).collect();

    // BCs: x=0/y=0 antisymmetry.  tol=0.5 is well inside every inter-node
    // arc-length on the 4×4 mesh.
    let tol = 0.5_f64;
    let bcs = hemisphere_antisymmetry_bcs(&nodes, tol);

    // Outward +x load P/4 at the equator corner (R, 0, 0).
    let load_node = hemisphere_load_node(&nodes, R, tol);
    let point_loads = vec![(load_node * 6 + 0, P / 4.0)]; // F_x = +P/4 (outward)

    // ── Non-ANS degenerate substrate ─────────────────────────────────────────
    let k_deg = build_shell_stiffnesses_degenerate(&nodes, &connectivity, &directors, T, &mat);
    let e_deg = assembly_elements_for(&connectivity, &k_deg);
    let u_deg = solve_shell_system(&e_deg, n_nodes, &bcs, &point_loads);
    let radial_deg = u_deg[load_node * 6 + 0]; // u_x at (R,0,0): outward = positive

    assert!(
        radial_deg.is_finite(),
        "degenerate hemisphere: radial_disp = {radial_deg} is not finite"
    );
    assert!(
        radial_deg > 0.0,
        "degenerate hemisphere: radial_disp = {radial_deg:.4e} must be positive \
         (outward) under +F_x load; a negative value means the hemisphere mesh \
         winding still negates the stiffness (task 4068 |det| fix not applied)"
    );
    assert!(
        radial_deg < SMOKE_CEIL,
        "degenerate hemisphere: radial_disp = {radial_deg:.4e} exceeds the \
         runaway ceiling {SMOKE_CEIL}"
    );

    // ── ANS-membrane degenerate substrate ────────────────────────────────────
    let k_ans =
        build_shell_stiffnesses_degenerate_ans(&nodes, &connectivity, &directors, T, &mat);
    let e_ans = assembly_elements_for(&connectivity, &k_ans);
    let u_ans = solve_shell_system(&e_ans, n_nodes, &bcs, &point_loads);
    let radial_ans = u_ans[load_node * 6 + 0];

    assert!(
        radial_ans.is_finite(),
        "degenerate+ANS hemisphere: radial_disp = {radial_ans} is not finite"
    );
    assert!(
        radial_ans > 0.0,
        "degenerate+ANS hemisphere: radial_disp = {radial_ans:.4e} must be positive \
         (outward) under +F_x load; a negative value means the hemisphere mesh \
         winding still negates the stiffness (task 4068 |det| fix not applied)"
    );
    assert!(
        radial_ans < SMOKE_CEIL,
        "degenerate+ANS hemisphere: radial_disp = {radial_ans:.4e} exceeds the \
         runaway ceiling {SMOKE_CEIL}"
    );
}
