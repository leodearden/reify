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
/// - `_options` — `MorphOptions` carries the fictitious-stiffness parameters
///   (`fictitious_youngs_modulus_base`, `fictitious_poisson_ratio`). Currently
///   only consulted in step-8's full pipeline; this stub ignores it.
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
    _options: &MorphOptions,
) -> Result<VolumeMesh, ElasticityFailure> {
    let _ = prescribed_positions;
    if old_mesh.element_order != ElementOrderTag::P1 {
        return Err(ElasticityFailure::UnsupportedElementOrder(
            old_mesh.element_order,
        ));
    }
    if old_mesh.vertices.is_empty() {
        return Ok(VolumeMesh {
            vertices: Vec::new(),
            tet_indices: old_mesh.tet_indices.clone(),
            element_order: old_mesh.element_order,
            normals: None,
        });
    }
    unimplemented!("step-8: full elasticity pipeline lands here")
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
