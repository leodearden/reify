//! Linear-elasticity morph (PRD task #7).
//!
//! Implements the primary morph algorithm specified in PRD
//! `docs/prds/v0_3/mesh-morphing.md` §"Linear-elasticity morph": treat the
//! source mesh as a fictitious-elastic continuum, prescribe surface-node
//! displacements as Dirichlet BCs, and solve the linear-elastostatic BVP
//! `K · u = 0` to obtain interior-node displacements. The output mesh is
//! `vertices_old + u`.
//!
//! Composes four primitives shipped by `reify-solver-elastic`:
//! [`element_stiffness`] (per-tet `K_e`), [`assemble_global_stiffness`]
//! (sparse `K`), [`apply_dirichlet_row_elimination`] (in-place BC application),
//! and [`solve_cg`] (Jacobi-preconditioned CG). All heavy lifting lives in
//! the FEA crate; this module is plumbing.
//!
//! Engine wiring (PRD task #10) selects between this morph and
//! [`crate::laplacian::laplacian_smooth`] based on the magnitude of the
//! parameter change and the laplacian-quickpass-threshold.

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, CgSolverOptions, DirichletBc, ElementOrder, ElementStiffness,
    IsotropicElastic, SolverMode, apply_dirichlet_row_elimination, assemble_global_stiffness,
    element_stiffness, solve_cg,
};
use reify_types::{ElementOrderTag, VolumeMesh};

use crate::MorphOptions;

// ── ElasticityFailure ────────────────────────────────────────────────────────

/// Failure modes from [`elasticity_morph`].
///
/// Mirrors the shape of [`crate::LaplacianFailure`] for the first two
/// variants — engine wiring (PRD task #10) sees uniform `Result<…, *Failure>`
/// returns from `laplacian_smooth` and `elasticity_morph` and projects both
/// into [`crate::MorphFailure::SolverError`]. `SolverNotConverged` is
/// elasticity-specific and surfaces a CG cap-out.
#[derive(Debug, Clone, PartialEq)]
pub enum ElasticityFailure {
    /// A node index in `prescribed_positions` is out of range for
    /// `old_mesh.vertices` (i.e. `node_idx * 3 + 2 >= old_mesh.vertices.len()`).
    InvalidNodeIndex(u32),

    /// `old_mesh.element_order` is not [`ElementOrderTag::P1`].
    ///
    /// P2 stiffness assembly is shipped by `reify-solver-elastic`, but the
    /// morph pipeline only exercises the P1 path: PRD task #10 gates the
    /// elasticity-morph branch on `element_order == P1` and falls through to
    /// the Laplacian quick-pass otherwise. Returning this variant lets the
    /// engine's projection layer convert it into a structured failure rather
    /// than a panic.
    UnsupportedElementOrder(ElementOrderTag),

    /// The CG solver hit `max_iter` without meeting the relative-residual
    /// tolerance. Defensive: for the in-prod case where every surface node is
    /// pinned by [`crate::compute_dirichlet_bcs`], the post-Dirichlet K is SPD
    /// on the unconstrained block and CG converges in ≤ k iterations
    /// (Cauchy-interlacing argument). Cap-out only occurs for genuinely
    /// under-constrained systems where rigid-body modes survive Dirichlet.
    SolverNotConverged {
        /// Number of CG iterations executed before giving up
        /// (`== CgSolverOptions::max_iter`).
        iterations: usize,
    },
}

// ── elasticity_morph ─────────────────────────────────────────────────────────

/// Linear-elasticity mesh morph — compute interior-node displacements
/// consistent with prescribed surface-node positions by solving the
/// fictitious-elastic BVP `K · u = 0` with `bcs = prescribed_displacements`.
///
/// ## Parameters
///
/// - `old_mesh` — the source tetrahedral mesh.
/// - `prescribed_positions` — `(node_index, new_position)` pairs identifying
///   surface nodes and their target positions; the natural producer is
///   [`crate::compute_dirichlet_bcs`] (PRD task #5). The internal pipeline
///   converts each pair into a per-axis [`DirichletBc`] with
///   `value = new_position[axis] - old_position[axis]` (delta, not absolute).
///   **Duplicate `node_index` entries are a precondition violation**: each
///   occurrence appends three more `DirichletBc` entries for the same DOFs,
///   and `apply_dirichlet_row_elimination` asserts uniqueness in debug builds
///   (boundary/dirichlet.rs:170-186). The natural producer
///   (`compute_dirichlet_bcs` via `BTreeMap`) always emits each node once.
/// - `options` — supplies the fictitious-stiffness parameters
///   (`fictitious_youngs_modulus_base`, `fictitious_poisson_ratio`) used to
///   build the [`IsotropicElastic`] material driving the FEA solve.
///
/// ## Output normals
///
/// The returned mesh always has `normals: None`, regardless of whether the
/// input mesh carried per-vertex normals. Vertex motion under the elasticity
/// solve makes any pre-existing normals geometrically stale; dropping them
/// fails closed (a consumer that needs surface normals must recompute them
/// after morphing). Same convention as [`crate::laplacian::laplacian_smooth`].
///
/// ## Failure modes
///
/// See [`ElasticityFailure`].
pub fn elasticity_morph(
    old_mesh: &VolumeMesh,
    prescribed_positions: &[(u32, [f64; 3])],
    options: &MorphOptions,
) -> Result<VolumeMesh, ElasticityFailure> {
    if old_mesh.element_order != ElementOrderTag::P1 {
        return Err(ElasticityFailure::UnsupportedElementOrder(
            old_mesh.element_order,
        ));
    }

    // Validate every prescribed_positions index up front (before any
    // allocation) — delegates to VolumeMesh::vertex for the overflow-safe
    // bounds check. Same discipline as laplacian.rs:103-107.
    for (node_idx, _) in prescribed_positions {
        old_mesh
            .vertex(*node_idx)
            .ok_or(ElasticityFailure::InvalidNodeIndex(*node_idx))?;
    }

    // Short-circuit when there are no tets to assemble: covers both the empty-
    // mesh case (vertices.is_empty()) and a mesh with vertices but no tets
    // (tet_indices.is_empty()). Without this guard, a no-tet mesh falls into
    // the FEA pipeline and panics: assemble_global_stiffness emits a 3N×3N
    // matrix with zero stored entries, and apply_dirichlet_row_elimination
    // asserts `DirichletBc has no explicit diagonal entry` (debug build) or
    // solve_cg panics in extract_diag_jacobi on a zero/missing diagonal.
    // Return the input vertices unchanged; drop normals per the contract above.
    if old_mesh.vertices.is_empty() || old_mesh.tet_indices.is_empty() {
        return Ok(VolumeMesh {
            vertices: old_mesh.vertices.clone(),
            tet_indices: old_mesh.tet_indices.clone(),
            element_order: old_mesh.element_order,
            normals: None,
        });
    }

    // ── Pipeline ─────────────────────────────────────────────────────────────
    let n_nodes = old_mesh.vertices.len() / 3;
    let n_elements = old_mesh.tet_indices.len() / 4;

    let material = IsotropicElastic {
        youngs_modulus: options.fictitious_youngs_modulus_base,
        poisson_ratio: options.fictitious_poisson_ratio,
    };

    // Per-element data — Vec storage keeps `ElementStiffness` and
    // connectivity buffers alive for the `AssemblyElement` borrows.
    let mut k_elements: Vec<ElementStiffness> = Vec::with_capacity(n_elements);
    let mut connectivities: Vec<[usize; 4]> = Vec::with_capacity(n_elements);
    for tet in old_mesh.tet_indices.chunks_exact(4) {
        // Per-tet physical-node coordinates. Out-of-range tet indices are
        // a precondition violation per the doc-comment on tet_indices; the
        // substituted [0;3] keeps element_stiffness total, but the raw
        // index is forwarded into AssemblyElement.connectivity and
        // assemble_global_stiffness will panic with a structured
        // 'connectivity references node N >= n_nodes' message.
        let phys: [[f64; 3]; 4] = [
            old_mesh.vertex_f64(tet[0]).unwrap_or([0.0; 3]),
            old_mesh.vertex_f64(tet[1]).unwrap_or([0.0; 3]),
            old_mesh.vertex_f64(tet[2]).unwrap_or([0.0; 3]),
            old_mesh.vertex_f64(tet[3]).unwrap_or([0.0; 3]),
        ];
        let k_e = element_stiffness(ElementOrder::P1, &phys, &material);
        k_elements.push(k_e);
        connectivities.push([
            tet[0] as usize,
            tet[1] as usize,
            tet[2] as usize,
            tet[3] as usize,
        ]);
    }

    let elements: Vec<AssemblyElement<'_>> = (0..n_elements)
        .map(|i| AssemblyElement {
            id: i,
            connectivity: &connectivities[i],
            k_e: &k_elements[i],
        })
        .collect();

    // AssemblyMode::Deterministic — bit-stable across runs and machines (load-
    // bearing for the FEA warm-start cache, PRD task #15). Parallel-mode
    // policy lives in PRD task #16's ElasticOptions resolution layer, not in
    // this primitive.
    let mut k_global = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

    // f = 0 — the morph BVP has no body forces or surface tractions; surface
    // motion is prescribed entirely via Dirichlet BCs.
    let mut f = vec![0.0_f64; 3 * n_nodes];

    // Build per-axis Dirichlet BCs: displacement = new_position - old_position.
    // DOF index 3*node_idx + axis matches AssemblyElement's node-major,
    // axis-minor layout (assembly/global.rs:23-26).
    let mut bcs: Vec<DirichletBc> = Vec::with_capacity(prescribed_positions.len() * 3);
    for (node_idx, new_position) in prescribed_positions {
        // Bounds check above ensures vertex_f64 returns Some.
        let old_position = old_mesh
            .vertex_f64(*node_idx)
            .expect("node index validated by up-front bounds check");
        for axis in 0..3 {
            bcs.push(DirichletBc {
                dof: 3 * (*node_idx as usize) + axis,
                value: new_position[axis] - old_position[axis],
            });
        }
    }
    apply_dirichlet_row_elimination(&mut k_global, &mut f, &bcs);

    // SolverMode::Deterministic — same rationale as AssemblyMode::Deterministic
    // above. CgSolverOptions::default() (tolerance 1e-8, max_iter 1000) is
    // calibrated for general FEA workloads; CG-tuning surface stays internal
    // (PRD task #16's ElasticOptions resolution layer can swap in custom opts).
    let cg_result = solve_cg(
        &k_global,
        &f,
        CgSolverOptions::default(),
        SolverMode::Deterministic,
    );
    if !cg_result.converged {
        return Err(ElasticityFailure::SolverNotConverged {
            iterations: cg_result.iterations,
        });
    }

    // Apply displacement: new_vertex = old_vertex + u (f64 arithmetic at the
    // read/write boundary, narrowed back to f32 for the output VolumeMesh).
    let mut out_vertices = Vec::with_capacity(old_mesh.vertices.len());
    for i in 0..n_nodes {
        for axis in 0..3 {
            let old_v = old_mesh.vertices[3 * i + axis] as f64;
            let new_v = old_v + cg_result.u[3 * i + axis];
            out_vertices.push(new_v as f32);
        }
    }

    Ok(VolumeMesh {
        vertices: out_vertices,
        tet_indices: old_mesh.tet_indices.clone(),
        element_order: old_mesh.element_order,
        normals: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{ElementOrderTag, VolumeMesh};

    fn empty_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: Vec::new(),
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    // ── Step-1: smoke test for the public API surface ─────────────────────────

    /// Pins the public signature
    /// `fn elasticity_morph(&VolumeMesh, &[(u32, [f64;3])], &MorphOptions)
    ///     -> Result<VolumeMesh, ElasticityFailure>` and the empty-mesh
    /// short-circuit (skip the FEA solve, return an empty mesh with the
    /// canonical `normals: None` contract). Mirrors the
    /// `laplacian_smooth_with_empty_mesh_*` smoke test.
    #[test]
    fn elasticity_morph_with_empty_mesh_and_no_prescribed_positions_returns_empty_mesh() {
        let result = elasticity_morph(&empty_mesh(), &[], &crate::MorphOptions::default());
        assert!(result.is_ok(), "got: {result:?}");
        let mesh = result.unwrap();
        assert!(mesh.vertices.is_empty());
        assert!(mesh.tet_indices.is_empty());
        assert_eq!(mesh.element_order, ElementOrderTag::P1);
        assert!(mesh.normals.is_none());
    }

    // ── Step-5: out-of-range prescribed-position node index ──────────────────

    /// Mirrors `laplacian_smooth_with_node_index_out_of_mesh_vertices_range_*`
    /// (laplacian.rs:263-278) — same overflow-safe index validation, same
    /// structured failure shape. The 2-vertex P1 fixture means
    /// `vertices.len() == 6`; node index 5 → base = 15 ≥ 6 so the bounds
    /// check fires before any allocation.
    #[test]
    fn elasticity_morph_with_node_index_out_of_mesh_vertices_range_returns_invalid_node_index() {
        let mesh = VolumeMesh {
            vertices: vec![0.0_f32, 0.0, 0.0, 1.0, 1.0, 1.0],
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let result = elasticity_morph(&mesh, &[(5, [9.0, 9.0, 9.0])], &crate::MorphOptions::default());
        match result {
            Err(ElasticityFailure::InvalidNodeIndex(idx)) => {
                assert_eq!(idx, 5);
            }
            other => panic!("expected InvalidNodeIndex(5), got: {other:?}"),
        }
    }

    // ── Step-7: smallest end-to-end test — zero-displacement BCs on single tet ─

    /// Smallest end-to-end test of the full FEA pipeline: one tet, four
    /// vertices, all four corners pinned to themselves (zero displacement).
    /// With every DOF Dirichlet-pinned (12/12), the post-Dirichlet K becomes
    /// `diag(1.0)`; CG converges in ≤ 1 iteration; `u = prescribed
    /// displacements = 0`; output positions equal input positions within fp
    /// tolerance. Exercises element_stiffness + assemble_global_stiffness +
    /// apply_dirichlet_row_elimination + solve_cg in one shot.
    #[test]
    fn elasticity_morph_with_zero_displacement_bcs_on_single_tet_returns_input_positions_within_fp_tolerance()
     {
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // node 0
                1.0, 0.0, 0.0, // node 1
                0.0, 1.0, 0.0, // node 2
                0.0, 0.0, 1.0, // node 3
            ],
            tet_indices: vec![0, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        // All 4 nodes pinned to themselves → zero displacement everywhere.
        let prescribed = vec![
            (0_u32, [0.0_f64, 0.0, 0.0]),
            (1, [1.0, 0.0, 0.0]),
            (2, [0.0, 1.0, 0.0]),
            (3, [0.0, 0.0, 1.0]),
        ];

        let out =
            elasticity_morph(&mesh, &prescribed, &crate::MorphOptions::default()).unwrap();

        let tol = 1e-6_f32;
        let expected: [f32; 12] = [
            0.0, 0.0, 0.0, // node 0
            1.0, 0.0, 0.0, // node 1
            0.0, 1.0, 0.0, // node 2
            0.0, 0.0, 1.0, // node 3
        ];
        assert_eq!(out.vertices.len(), expected.len());
        for (axis, want) in expected.iter().enumerate() {
            let got = out.vertices[axis];
            assert!(
                (got - want).abs() <= tol,
                "vertices[{axis}]: out={got} expected={want}",
            );
        }

        // Structural fields carry through unchanged.
        assert_eq!(out.tet_indices, vec![0u32, 1, 2, 3]);
        assert_eq!(out.element_order, ElementOrderTag::P1);
        assert!(out.normals.is_none());
    }

    // ── Step-9: rigid-translation propagates to interior node ────────────────

    /// 4-tet "cone" fixture: 5 vertices, 4 tets all sharing the interior
    /// node `p`. Pinning `a, b, c, d` to translated positions and leaving
    /// `p` free: rigid-body translation lies in the kernel of any continuum
    /// stiffness operator, so the unique elastic-equilibrium solution under
    /// uniform-displacement Dirichlet on the boundary IS the rigid
    /// translation of the entire mesh — `p_new = p_old + delta`.
    ///
    /// Closed-form expected value gives a strong regression guard for
    /// pipeline-correctness bugs:
    /// - DOF mapping (e.g. `dof = node_idx + 3*axis` instead of
    ///   `3*node_idx + axis`) would make the K → f column-into-RHS step
    ///   write to the wrong global rows; the interior-node displacement
    ///   would diverge from the boundary translation.
    /// - Sign error in `value = new_position - old_position` would invert
    ///   the propagated displacement.
    ///
    /// Adapts the `laplacian_smooth_with_one_iteration_*` cone fixture
    /// (laplacian.rs:336-397).
    #[test]
    fn elasticity_morph_with_rigid_translation_on_cone_propagates_translation_to_interior_node_within_fp_tolerance()
     {
        // Layout: nodes 0..3 = a, b, c, d (surface); node 4 = p (interior).
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // 0: a
                1.0, 0.0, 0.0, // 1: b
                0.0, 1.0, 0.0, // 2: c
                0.0, 0.0, 1.0, // 3: d
                0.25, 0.25, 0.25, // 4: p
            ],
            // Four tets all sharing p (node 4).
            tet_indices: vec![
                0, 1, 2, 4, // a, b, c, p
                0, 1, 3, 4, // a, b, d, p
                0, 2, 3, 4, // a, c, d, p
                1, 2, 3, 4, // b, c, d, p
            ],
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // Pin a, b, c, d to translated positions; leave p free.
        let delta = [0.5_f64, 0.7, -0.3];
        let prescribed = vec![
            (0_u32, [0.0 + delta[0], 0.0 + delta[1], 0.0 + delta[2]]),
            (1, [1.0 + delta[0], 0.0 + delta[1], 0.0 + delta[2]]),
            (2, [0.0 + delta[0], 1.0 + delta[1], 0.0 + delta[2]]),
            (3, [0.0 + delta[0], 0.0 + delta[1], 1.0 + delta[2]]),
        ];

        let out =
            elasticity_morph(&mesh, &prescribed, &crate::MorphOptions::default()).unwrap();

        // p_new = p_old + delta (rigid-body translation is in the kernel of
        // K, so the elastic-equilibrium displacement field IS the rigid
        // translation of every node).
        let p_old = [0.25_f64, 0.25, 0.25];
        let expected_p = [
            p_old[0] + delta[0],
            p_old[1] + delta[1],
            p_old[2] + delta[2],
        ];
        let p_base = 4 * 3;
        let tol = 1e-6_f32;
        for (axis, want) in expected_p.iter().enumerate() {
            let got = out.vertices[p_base + axis];
            let want = *want as f32;
            assert!(
                (got - want).abs() <= tol,
                "p[{axis}]: got={got} expected={want} (delta={tol})",
            );
        }
    }

    // ── Step-11: drops normals on output ─────────────────────────────────────

    /// Vertex motion under the elasticity solve makes any pre-existing
    /// per-vertex normals geometrically stale; the output mesh must have
    /// `normals: None` regardless of input. Mirrors
    /// `laplacian_smooth_drops_normals_on_output_even_when_input_has_some_normals`
    /// (laplacian.rs:671-685).
    #[test]
    fn elasticity_morph_drops_normals_on_output_even_when_input_has_some_normals() {
        // Single-tet mesh with 4 per-vertex normals (12 floats).
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // 0
                1.0, 0.0, 0.0, // 1
                0.0, 1.0, 0.0, // 2
                0.0, 0.0, 1.0, // 3
            ],
            tet_indices: vec![0, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: Some(vec![
                1.0_f32, 0.0, 0.0, // normal for node 0
                0.0, 1.0, 0.0, // normal for node 1
                0.0, 0.0, 1.0, // normal for node 2
                1.0, 1.0, 0.0, // normal for node 3
            ]),
        };
        // Pin every node to itself so the solve is well-conditioned.
        let prescribed = vec![
            (0_u32, [0.0_f64, 0.0, 0.0]),
            (1, [1.0, 0.0, 0.0]),
            (2, [0.0, 1.0, 0.0]),
            (3, [0.0, 0.0, 1.0]),
        ];
        let out =
            elasticity_morph(&mesh, &prescribed, &crate::MorphOptions::default()).unwrap();
        assert!(
            out.normals.is_none(),
            "expected normals: None, got: {:?}",
            out.normals
        );
    }

    // ── Step-13: determinism across runs with same input ────────────────────

    /// Two `elasticity_morph` calls on the same input must produce
    /// bit-equal `vertices`. Pins the contract that the FEA warm-start
    /// cache (PRD task #15) and reproducible morphed-mesh caching rely on.
    /// Defends against a future refactor swapping `AssemblyMode::Deterministic`
    /// or `SolverMode::Deterministic` for their `Parallel` counterparts —
    /// those produce tolerance-equivalent but not bit-equal results across
    /// thread counts (per solver.rs:33-50). Reuses the cone fixture from
    /// step-9. Mirrors `laplacian_smooth_is_deterministic_across_runs_with_same_input`
    /// (laplacian.rs:619-658).
    #[test]
    fn elasticity_morph_is_deterministic_across_runs_with_same_input() {
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // 0: a
                1.0, 0.0, 0.0, // 1: b
                0.0, 1.0, 0.0, // 2: c
                0.0, 0.0, 1.0, // 3: d
                0.25, 0.25, 0.25, // 4: p
            ],
            tet_indices: vec![
                0, 1, 2, 4, // a, b, c, p
                0, 1, 3, 4, // a, b, d, p
                0, 2, 3, 4, // a, c, d, p
                1, 2, 3, 4, // b, c, d, p
            ],
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let delta = [0.5_f64, 0.7, -0.3];
        let prescribed = vec![
            (0_u32, [0.0 + delta[0], 0.0 + delta[1], 0.0 + delta[2]]),
            (1, [1.0 + delta[0], 0.0 + delta[1], 0.0 + delta[2]]),
            (2, [0.0 + delta[0], 1.0 + delta[1], 0.0 + delta[2]]),
            (3, [0.0 + delta[0], 0.0 + delta[1], 1.0 + delta[2]]),
        ];

        let out_a =
            elasticity_morph(&mesh, &prescribed, &crate::MorphOptions::default()).unwrap();
        let out_b =
            elasticity_morph(&mesh, &prescribed, &crate::MorphOptions::default()).unwrap();

        assert_eq!(out_a.vertices, out_b.vertices);
        assert_eq!(out_a.tet_indices, out_b.tet_indices);
        assert_eq!(out_a.element_order, out_b.element_order);
        // `normals` is unconditionally None — asserting equality would be
        // tautological. Pinned by step-11.
    }

    // ── task 2945 step-5: InverseVolume vs Uniform on graded mesh ────────────

    /// Asymmetric cone fixture: 5 nodes, 4 tets all sharing interior node `p`.
    /// Node `p` is placed near vertex `a` (at 0.05, 0.05, 0.05), so three
    /// tets touching `a` have small volume and the fourth (`b, c, d, p`) has
    /// large volume — a deliberately graded mesh.
    ///
    /// Non-rigid pinning (a, c, d fixed; b stretched to (2, 0, 0)) forces a
    /// non-rigid deformation mode, which exposes the effect of per-element E
    /// scaling: under `InverseVolume` the small-V tets are stiffer and the
    /// interior node `p` moves differently than under `Uniform`.
    ///
    /// Asserts that the two rules produce demonstrably different interior-node
    /// positions (L_∞ norm > 1e-3). No directional bias is asserted — the sign
    /// depends on a non-trivial energy balance and would create fragility.
    #[test]
    fn elasticity_morph_inverse_volume_rule_produces_different_interior_node_position_than_uniform()
    {
        // Layout: 0=a, 1=b, 2=c, 3=d, 4=p (interior, near a)
        let mesh = reify_types::VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0,  // 0: a
                1.0, 0.0, 0.0,       // 1: b
                0.0, 1.0, 0.0,       // 2: c
                0.0, 0.0, 1.0,       // 3: d
                0.05, 0.05, 0.05,    // 4: p (near a → three small tets, one large)
            ],
            tet_indices: vec![
                0, 1, 2, 4, // a, b, c, p  — small vol
                0, 1, 3, 4, // a, b, d, p  — small vol
                0, 2, 3, 4, // a, c, d, p  — small vol
                1, 2, 3, 4, // b, c, d, p  — large vol
            ],
            element_order: reify_types::ElementOrderTag::P1,
            normals: None,
        };

        // Non-rigid BCs: a, c, d pinned to their original positions; b
        // stretched to (2, 0, 0). This imposes a stretching mode — NOT a
        // rigid-body translation, so the per-element E scaling has an effect
        // on the interior-node equilibrium position.
        let prescribed = vec![
            (0_u32, [0.0_f64, 0.0, 0.0]), // a fixed
            (2, [0.0_f64, 1.0, 0.0]),      // c fixed
            (3, [0.0_f64, 0.0, 1.0]),      // d fixed
            (1, [2.0_f64, 0.0, 0.0]),      // b stretched
        ];

        let opts_uniform = crate::MorphOptions {
            stiffness_rule: crate::options::StiffnessRule::Uniform,
            ..crate::MorphOptions::default()
        };
        let opts_inv_vol = crate::MorphOptions {
            stiffness_rule: crate::options::StiffnessRule::InverseVolume,
            ..crate::MorphOptions::default()
        };

        let out_uniform = elasticity_morph(&mesh, &prescribed, &opts_uniform)
            .expect("Uniform elasticity_morph should succeed");
        let out_inv_vol = elasticity_morph(&mesh, &prescribed, &opts_inv_vol)
            .expect("InverseVolume elasticity_morph should succeed");

        // Interior node 4 = p is at vertex positions [12, 13, 14] (0-indexed).
        let p_base = 4 * 3;
        let linf_norm = (0..3)
            .map(|axis| {
                (out_uniform.vertices[p_base + axis] - out_inv_vol.vertices[p_base + axis]).abs()
            })
            .fold(0.0_f32, f32::max);

        assert!(
            linf_norm > 1e-3,
            "InverseVolume and Uniform rules should produce different interior-node \
             positions on a graded mesh under a non-rigid BVP, but L_inf difference = {linf_norm}"
        );
    }

    // ── Step-15: exhaustive variant fence for ElasticityFailure ──────────────

    /// No-wildcard match guarantees that adding/removing/renaming a variant
    /// breaks compilation immediately — same discipline as
    /// `laplacian_failure_variants_construct_and_pattern_match_exhaustively`
    /// (laplacian.rs:725-739) and
    /// `morph_failure_four_variants_construct_and_pattern_match_exhaustively`
    /// (options.rs:148-188). Each arm probes the carried payload via field
    /// accessors so a constructor that drops or swaps a field is caught
    /// (not merely PartialEq reflexivity).
    #[test]
    fn elasticity_failure_variants_construct_and_pattern_match_exhaustively() {
        let invalid = ElasticityFailure::InvalidNodeIndex(5);
        let unsupported = ElasticityFailure::UnsupportedElementOrder(ElementOrderTag::P2);
        let not_converged = ElasticityFailure::SolverNotConverged { iterations: 1000 };

        for failure in [&invalid, &unsupported, &not_converged] {
            match failure {
                ElasticityFailure::InvalidNodeIndex(idx) => {
                    assert_eq!(*idx, 5);
                }
                ElasticityFailure::UnsupportedElementOrder(order) => {
                    assert_eq!(*order, ElementOrderTag::P2);
                }
                ElasticityFailure::SolverNotConverged { iterations } => {
                    assert_eq!(*iterations, 1000);
                }
            }
        }
    }

    // ── Robustness: non-empty vertices, empty tet_indices ────────────────────

    /// A P1 mesh with non-empty `vertices` but empty `tet_indices` (orphan
    /// nodes) is handled by the short-circuit — no FEA pipeline runs.
    ///
    /// Without the `tet_indices.is_empty()` guard, `assemble_global_stiffness`
    /// produces a 3N×3N matrix with zero stored entries. Subsequent calls to
    /// `apply_dirichlet_row_elimination` then panic in debug builds with
    /// "DirichletBc has no explicit diagonal entry" (boundary/dirichlet.rs:271),
    /// and `solve_cg` panics in `extract_diag_jacobi` on zero/missing diagonal
    /// when prescribed_positions is empty.
    ///
    /// The output preserves the input vertices unchanged and drops normals.
    #[test]
    fn elasticity_morph_with_non_empty_vertices_but_empty_tet_indices_returns_input_vertices() {
        let mesh = VolumeMesh {
            vertices: vec![0.0_f32, 0.0, 0.0, 1.0, 1.0, 1.0],
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: Some(vec![1.0_f32, 0.0, 0.0, 0.0, 1.0, 0.0]),
        };
        // Node 0 is valid; prescribed_positions must not cause a panic because
        // we short-circuit before any assembly.
        let result =
            elasticity_morph(&mesh, &[(0, [0.5, 0.5, 0.5])], &crate::MorphOptions::default());
        let out = result.unwrap();
        // Vertices returned unchanged.
        assert_eq!(out.vertices, mesh.vertices);
        assert!(out.tet_indices.is_empty());
        assert_eq!(out.element_order, ElementOrderTag::P1);
        // Normals dropped regardless of input.
        assert!(out.normals.is_none());
    }

    // ── Step-3: P2 element order rejection ────────────────────────────────────

    /// P2 element order must be rejected with
    /// `ElasticityFailure::UnsupportedElementOrder(P2)`. The fixture has a
    /// non-empty `vertices` buffer so the empty-mesh short-circuit doesn't
    /// fire first (which would mask a missing P1 guard). Mirrors
    /// `laplacian_smooth_rejects_p2_element_order_*`.
    #[test]
    fn elasticity_morph_rejects_p2_element_order_with_unsupported_element_order_failure() {
        let mesh = VolumeMesh {
            // 1 vertex so vertices.is_empty() == false — the P1 guard must
            // fire before any short-circuit.
            vertices: vec![0.0_f32, 0.0, 0.0],
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P2,
            normals: None,
        };
        let result = elasticity_morph(&mesh, &[], &crate::MorphOptions::default());
        match result {
            Err(ElasticityFailure::UnsupportedElementOrder(order)) => {
                assert_eq!(order, ElementOrderTag::P2);
            }
            other => panic!("expected UnsupportedElementOrder(P2), got: {other:?}"),
        }
    }
}
