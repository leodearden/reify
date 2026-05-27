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
    element_stiffness, solve_cg, tet_volume_p1,
};
use reify_ir::{ElementOrderTag, VolumeMesh};

use crate::MorphOptions;
use crate::options::StiffnessRule;

// ── Spatially-varying stiffness helpers ───────────────────────────────────────

/// ε guard for degenerate tets: divisor is clamped to at least this value so
/// that E_e stays finite even when V_e = 0 or all edges coincide. Mirrors the
/// `MIN_JACOBIAN_DET = 1.0e-30` precedent in
/// `reify-solver-elastic/src/result.rs:39`. PRD task #21 will replace this
/// placeholder with a mesh-scale-aware degeneracy detector and structured
/// error variant; until then we fail-finite-but-garbage on degenerate input.
///
/// Even when this clamp engages and produces ~1e30:1 K-conditioning across
/// mixed degenerate/healthy tets, the engine pipeline (PRD task #10) catches
/// the resulting degenerate or inverted morphed elements via the quality pass:
/// `QualityVerdict::HardFail` on negative scaled-Jacobian and
/// `QualityVerdict::SoftFail` with `degenerate_morphed_element = Some(_)` on
/// `sj == 0.0` (both via `quality_check` in `quality.rs`), independently of
/// the configured floor thresholds. PRD task #21 will replace this placeholder
/// with a structured error variant; the quality pass acts as the safety net
/// until then.
const MIN_VOLUME: f64 = 1.0e-30;

/// Analogous ε guard for the `InverseEdgeLengthSquared` rule. See `MIN_VOLUME`
/// for the safety-net rationale (quality pass catches degenerate output even
/// when this clamp produces extreme K-conditioning).
const MIN_LENGTH_SQ: f64 = 1.0e-30;

/// Average of the 6 squared edge lengths of a P1 tet.
///
/// Edges: (0,1), (0,2), (0,3), (1,2), (1,3), (2,3) — all 6 pairs of the 4
/// vertices. Using the **mean** (not max) keeps sliver tets with one extreme
/// edge from dominating; it is also invariant to vertex ordering.
///
/// The result is guaranteed ≥ 0. It equals 0.0 only when all four vertices
/// are coincident (a fully degenerate tet). Callers clamp with `MIN_LENGTH_SQ`
/// before using as a divisor.
#[inline]
fn mean_squared_edge_length(phys: &[[f64; 3]; 4]) -> f64 {
    let edges: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
    let sum: f64 = edges
        .iter()
        .map(|&(i, j)| {
            let dx = phys[i][0] - phys[j][0];
            let dy = phys[i][1] - phys[j][1];
            let dz = phys[i][2] - phys[j][2];
            dx * dx + dy * dy + dz * dz
        })
        .sum();
    sum / 6.0
}

/// Compute the element-local Young's modulus for the given `rule`.
///
/// - `Uniform`: returns `e_base` unchanged — bit-identical to the task #7
///   baseline.
/// - `InverseVolume`: returns `e_base / max(V_e, MIN_VOLUME)` where
///   `V_e = tet_volume_p1(phys)`.
/// - `InverseEdgeLengthSquared`: returns `e_base /
///   max(mean_squared_edge_length(phys), MIN_LENGTH_SQ)`.
///
/// Because the homogeneous BVP `K · u = 0` is invariant under uniform E
/// rescaling, only the **ratios** E_i/E_j across elements affect the solution.
/// The absolute base value `e_base` cancels; it is preserved here so that
/// `Uniform` remains a no-op alias for the shared material in task #7.
#[inline]
fn per_element_youngs_modulus(rule: StiffnessRule, phys: &[[f64; 3]; 4], e_base: f64) -> f64 {
    match rule {
        StiffnessRule::Uniform => e_base,
        StiffnessRule::InverseVolume => {
            let v = tet_volume_p1(phys).max(MIN_VOLUME);
            e_base / v
        }
        StiffnessRule::InverseEdgeLengthSquared => {
            let l_sq = mean_squared_edge_length(phys).max(MIN_LENGTH_SQ);
            e_base / l_sq
        }
    }
}

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

    /// An entry in `old_mesh.tet_indices` references a node index ≥ `n_nodes`
    /// (overflow-safe check via `(*idx as usize) >= n_nodes`). Validated
    /// upfront before assembly to avoid coupling to
    /// `assemble_global_stiffness`'s debug-only panic shape. The first
    /// offending index is returned as the payload.
    InvalidTetIndex(u32),

    /// `old_mesh.tet_indices` is empty (no continuum elements) but
    /// `prescribed_positions` is non-empty. Silently dropping the BCs would
    /// violate the documented `output = vertices_old + u` contract — the
    /// caller supplied displacements that have nowhere to be applied. This
    /// variant surfaces the mismatch as a recoverable failure so the engine's
    /// projection layer can choose a fallback rather than silently discarding
    /// user intent.
    ///
    /// Note: the empty-mesh case (`vertices.is_empty()`) with non-empty
    /// `prescribed_positions` routes through `InvalidNodeIndex` (the
    /// prescribed-positions bounds-check fires first), not this variant.
    NoElementsForPrescribedDisplacements,

    /// `old_mesh.tet_indices.len()` is not a multiple of 4. Each tet is
    /// identified by exactly 4 node indices, so a length that is not a
    /// multiple of 4 has stray 1–3 trailing entries that would be silently
    /// dropped by the FEA pipeline's `chunks_exact(4)` assembly loop.
    ///
    /// Surfacing this upfront treats it as a likely caller data-corruption
    /// signal rather than swallowing it. Payload `len` carries the offending
    /// length for diagnostic logging without re-walking the slice.
    ///
    /// Mirrors the structured-failure pattern of
    /// `NoElementsForPrescribedDisplacements`: don't silently drop user input.
    MalformedTetIndices {
        /// The offending `tet_indices.len()` (not a multiple of 4).
        len: usize,
    },
}

// ── elasticity_morph / elasticity_morph_with_cg_opts ─────────────────────────

/// Linear-elasticity mesh morph with explicit CG solver options.
///
/// Full implementation — [`elasticity_morph`] delegates here with
/// [`CgSolverOptions::default()`].
///
/// ## When to reach for this function
///
/// - **Test injectability**: inject deliberately tight opts
///   (`max_iter: 1, tolerance: 1e-20`) to exercise the
///   [`ElasticityFailure::SolverNotConverged`] path without relying on a
///   pathological mesh.
/// - **Future PRD task #16 ElasticOptions resolution layer**: the production
///   morph engine will eventually surface CG opts to callers; this entry point
///   lets that layer forward custom opts without changing the stable
///   `elasticity_morph` signature.
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
/// - `cg_opts` — [`CgSolverOptions`] forwarded directly to [`solve_cg`].
///   Panics if `cg_opts.max_iter == 0` or `cg_opts.tolerance` is
///   non-finite/non-positive (those are preconditions of `solve_cg`).
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
pub fn elasticity_morph_with_cg_opts(
    old_mesh: &VolumeMesh,
    prescribed_positions: &[(u32, [f64; 3])],
    options: &MorphOptions,
    cg_opts: CgSolverOptions,
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

    // Short-circuit when there are no tets to assemble. Without this guard a
    // no-tet mesh falls into the FEA pipeline and panics:
    // assemble_global_stiffness emits a 3N×3N matrix with zero stored entries,
    // apply_dirichlet_row_elimination asserts 'DirichletBc has no explicit
    // diagonal entry' (debug build), or solve_cg panics in
    // extract_diag_jacobi on a zero/missing diagonal.
    //
    // Tightened contract (task 3362): if prescribed_positions is non-empty,
    // silently discarding BCs would violate the output = vertices_old + u
    // contract — surface the mismatch as a structured failure.
    // The empty-mesh case (vertices.is_empty()) with non-empty
    // prescribed_positions already fires InvalidNodeIndex via the
    // prescribed-positions bounds-check above; only the tet_indices gate is
    // needed here.
    if old_mesh.tet_indices.is_empty() {
        if !prescribed_positions.is_empty() {
            return Err(ElasticityFailure::NoElementsForPrescribedDisplacements);
        }
        return Ok(VolumeMesh {
            vertices: old_mesh.vertices.clone(),
            tet_indices: old_mesh.tet_indices.clone(),
            element_order: old_mesh.element_order,
            normals: None,
        });
    }

    // ── Pipeline ─────────────────────────────────────────────────────────────

    // Structural-shape check: reject non-multiple-of-4 lengths upfront, before
    // computing n_nodes or walking the index values. A malformed length is a
    // more fundamental violation than a single out-of-range index — structure
    // first, then semantics. Prevents the chunks_exact(4) silent-drop pathology
    // for in-range tails (e.g. [0,1,2,3, 0,1,2] would otherwise quietly discard
    // the trailing triple). The existing bounds-check loop below becomes a pure
    // semantic validator once this gate is in place.
    if !old_mesh.tet_indices.len().is_multiple_of(4) {
        return Err(ElasticityFailure::MalformedTetIndices {
            len: old_mesh.tet_indices.len(),
        });
    }

    let n_nodes = old_mesh.vertices.len() / 3;

    // Validates every tet-index value is in-range. Length-shape is already
    // validated upfront by the malformed-length check above, so this loop only
    // checks index bounds. Walks the full slice (equivalent to `chunks_exact(4)`
    // here since length is guaranteed to be a multiple of 4) — kept as a flat
    // loop for clarity.
    for tet_idx in &old_mesh.tet_indices {
        if (*tet_idx as usize) >= n_nodes {
            return Err(ElasticityFailure::InvalidTetIndex(*tet_idx));
        }
    }

    let n_elements = old_mesh.tet_indices.len() / 4;

    // Panic message for the contract assertions below — extracted to avoid
    // repeating a long string literal four times in the chunks_exact loop.
    const TET_IDX_PRE_VALIDATED: &str =
        "tet_indices validated upfront — InvalidTetIndex returned earlier";

    // Per-element data — Vec storage keeps `ElementStiffness` and
    // connectivity buffers alive for the `AssemblyElement` borrows.
    let mut k_elements: Vec<ElementStiffness> = Vec::with_capacity(n_elements);
    let mut connectivities: Vec<[usize; 4]> = Vec::with_capacity(n_elements);
    for tet in old_mesh.tet_indices.chunks_exact(4) {
        // Per-tet physical-node coordinates. The upfront validation above
        // guarantees all tet indices are in-range; vertex_f64 cannot return
        // None here.
        let phys: [[f64; 3]; 4] = [
            old_mesh.vertex_f64(tet[0]).expect(TET_IDX_PRE_VALIDATED),
            old_mesh.vertex_f64(tet[1]).expect(TET_IDX_PRE_VALIDATED),
            old_mesh.vertex_f64(tet[2]).expect(TET_IDX_PRE_VALIDATED),
            old_mesh.vertex_f64(tet[3]).expect(TET_IDX_PRE_VALIDATED),
        ];
        // Per-element Young's modulus: the stiffness_rule controls how E_e is
        // derived from e_base. K_e = ∫ BᵀDB dV is linear in E (D is linear in
        // E), so scaling E elementwise is exactly equivalent to scaling K_e
        // elementwise — the existing element_stiffness API is reused unchanged.
        // The single shared IsotropicElastic from task #7 is replaced by a
        // per-tet construct; only E changes per element, ν stays uniform.
        //
        // The dispatch is unconditional: per_element_youngs_modulus (#[inline])
        // handles all three rules including `Uniform → e_base` via its match
        // arm (a single-instruction return, zero extra computation). The no-op-
        // for-Uniform property is pinned by a unit test in the test module
        // asserting Uniform returns e_base for both canonical and
        // near-degenerate tets.
        //
        // NOTE — duplicate volume/geometry computation for `InverseVolume` and
        // `InverseEdgeLengthSquared`: per_element_youngs_modulus calls
        // `tet_volume_p1(&phys)` / `mean_squared_edge_length(&phys)` to derive
        // E_e, while element_stiffness (below) independently re-computes the
        // same Jacobian determinant for the same tet. Eliminating the duplicate
        // would require extending `reify-solver-elastic`'s element_stiffness API
        // to return the determinant/volume alongside K_e — a cross-crate API
        // change out of scope for this task. Defer to a future profiler-driven
        // optimisation once real multi-million-tet workloads motivate the change.
        let e_e = per_element_youngs_modulus(
            options.stiffness_rule,
            &phys,
            options.fictitious_youngs_modulus_base,
        );
        let material_e = IsotropicElastic {
            youngs_modulus: e_e,
            poisson_ratio: options.fictitious_poisson_ratio,
        };
        let k_e = element_stiffness(ElementOrder::P1, &phys, &material_e);
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
    // above. The `cg_opts` parameter controls tolerance and max_iter; default
    // opts (tolerance 1e-8, max_iter 1000) are calibrated for general FEA
    // workloads. Custom opts (e.g. tight tolerance + max_iter=1) let tests
    // exercise the SolverNotConverged path without a pathological mesh.
    let cg_result = solve_cg(&k_global, &f, cg_opts, SolverMode::Deterministic);
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

/// Linear-elasticity mesh morph — delegates to [`elasticity_morph_with_cg_opts`]
/// with [`CgSolverOptions::default()`] (tolerance 1e-8, max_iter 1000).
///
/// See [`elasticity_morph_with_cg_opts`] for full parameter, output-normal, and
/// failure-mode documentation.
pub fn elasticity_morph(
    old_mesh: &VolumeMesh,
    prescribed_positions: &[(u32, [f64; 3])],
    options: &MorphOptions,
) -> Result<VolumeMesh, ElasticityFailure> {
    elasticity_morph_with_cg_opts(
        old_mesh,
        prescribed_positions,
        options,
        CgSolverOptions::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::{ElementOrderTag, VolumeMesh};

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
        let result = elasticity_morph(
            &mesh,
            &[(5, [9.0, 9.0, 9.0])],
            &crate::MorphOptions::default(),
        );
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

        let out = elasticity_morph(&mesh, &prescribed, &crate::MorphOptions::default()).unwrap();

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

        let out = elasticity_morph(&mesh, &prescribed, &crate::MorphOptions::default()).unwrap();

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
        let out = elasticity_morph(&mesh, &prescribed, &crate::MorphOptions::default()).unwrap();
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

        let out_a = elasticity_morph(&mesh, &prescribed, &crate::MorphOptions::default()).unwrap();
        let out_b = elasticity_morph(&mesh, &prescribed, &crate::MorphOptions::default()).unwrap();

        assert_eq!(out_a.vertices, out_b.vertices);
        assert_eq!(out_a.tet_indices, out_b.tet_indices);
        assert_eq!(out_a.element_order, out_b.element_order);
        // `normals` is unconditionally None — asserting equality would be
        // tautological. Pinned by step-11.
    }

    // ── task 3422 step-1: per_element_youngs_modulus Uniform invariant ──────────

    /// Pins the load-bearing invariant for the step-2 dedup refactor: for
    /// `StiffnessRule::Uniform`, `per_element_youngs_modulus` returns `e_base`
    /// unchanged regardless of the tet geometry. Asserts exact equality
    /// (`assert_eq!`, not approx) because `Uniform` matches the single-instruction
    /// `StiffnessRule::Uniform => e_base` arm with no geometric computation.
    ///
    /// Two geometrically distinct tets are tested:
    ///  1. The canonical unit tet (a, b, c, d at unit axes).
    ///  2. A near-degenerate tet (all vertices near the origin) whose
    ///     `InverseVolume` and `InverseEdgeLengthSquared` E_e values would differ
    ///     dramatically from `e_base` — confirming the Uniform arm is untouched.
    ///
    /// Protects against future drift in `per_element_youngs_modulus` that might
    /// accidentally introduce geometry-dependent side effects for the Uniform
    /// variant (e.g. a mistaken match-arm reorder or an extra computation).
    #[test]
    fn uniform_rule_returns_e_base_for_any_geometry() {
        let e_base = 42.0_f64;

        // 1. Canonical unit tet.
        let unit_tet: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        assert_eq!(
            per_element_youngs_modulus(StiffnessRule::Uniform, &unit_tet, e_base),
            e_base,
            "Uniform rule on unit tet must return e_base unchanged"
        );

        // 2. Near-degenerate tet (tiny volume) — InverseVolume would give
        //    ~1e30×e_base but Uniform must still return e_base exactly.
        let tiny_tet: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0e-10, 0.0, 0.0],
            [0.0, 1.0e-10, 0.0],
            [0.0, 0.0, 1.0e-10],
        ];
        assert_eq!(
            per_element_youngs_modulus(StiffnessRule::Uniform, &tiny_tet, e_base),
            e_base,
            "Uniform rule on near-degenerate tet must return e_base unchanged"
        );
    }

    // ── task 2945 step-5 + step-7: asymmetric-cone fixture + rule tests ─────────

    /// Helper that builds the asymmetric cone fixture (5 nodes, 4 tets, `p`
    /// near `a`) and the non-rigid prescribed positions (b stretched to
    /// (2, 0, 0)). Shared by the `InverseVolume vs Uniform` test (step-5) and
    /// the `InverseEdgeLengthSquared distinctness + determinism` test (step-7)
    /// — single source of truth for the fixture definition.
    fn asymmetric_cone_fixture() -> (reify_ir::VolumeMesh, Vec<(u32, [f64; 3])>) {
        let mesh = reify_ir::VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // 0: a
                1.0, 0.0, 0.0, // 1: b
                0.0, 1.0, 0.0, // 2: c
                0.0, 0.0, 1.0, // 3: d
                0.05, 0.05, 0.05, // 4: p (near a → three small tets, one large)
            ],
            // NOTE: these tet orderings are NOT all consistently right-handed;
            // some yield negative Jacobian determinants.  This is intentional —
            // the fixture exercises element_stiffness's internal `det.abs()`
            // handling for mixed-orientation tets.
            tet_indices: vec![
                0, 1, 2, 4, // a, b, c, p  — small vol
                0, 1, 3, 4, // a, b, d, p  — small vol
                0, 2, 3, 4, // a, c, d, p  — small vol
                1, 2, 3, 4, // b, c, d, p  — large vol
            ],
            element_order: reify_ir::ElementOrderTag::P1,
            normals: None,
        };
        let prescribed: Vec<(u32, [f64; 3])> = vec![
            (0, [0.0, 0.0, 0.0]), // a fixed
            (2, [0.0, 1.0, 0.0]), // c fixed
            (3, [0.0, 0.0, 1.0]), // d fixed
            (1, [2.0, 0.0, 0.0]), // b stretched
        ];
        (mesh, prescribed)
    }

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
        let (mesh, prescribed) = asymmetric_cone_fixture();

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

    // ── task 2945 step-7: InverseEdgeLengthSquared distinctness + determinism ──

    /// Green-on-arrival characterization + regression guard.
    ///
    /// Asserts that `InverseEdgeLengthSquared` is:
    /// (a) distinct from `Uniform` and `InverseVolume` (genuinely different rule)
    /// (b) deterministic — two calls on the same input produce bit-equal results
    ///
    /// Also asserts all output vertices are finite (no NaN/Inf), guarding
    /// against degenerate-element regressions.
    ///
    /// Passes immediately because step-6 already implemented all three rules.
    /// Acts as a regression guard against future refactors that might collapse
    /// two rules into one or break determinism for non-Uniform paths.
    #[test]
    fn elasticity_morph_inverse_edge_length_squared_rule_is_distinct_from_uniform_and_inverse_volume_and_is_deterministic()
     {
        let (mesh, prescribed) = asymmetric_cone_fixture();

        let opts_uniform = crate::MorphOptions {
            stiffness_rule: crate::options::StiffnessRule::Uniform,
            ..crate::MorphOptions::default()
        };
        let opts_inv_vol = crate::MorphOptions {
            stiffness_rule: crate::options::StiffnessRule::InverseVolume,
            ..crate::MorphOptions::default()
        };
        let opts_inv_edge = crate::MorphOptions {
            stiffness_rule: crate::options::StiffnessRule::InverseEdgeLengthSquared,
            ..crate::MorphOptions::default()
        };

        let out_uniform =
            elasticity_morph(&mesh, &prescribed, &opts_uniform).expect("Uniform should succeed");
        let out_inv_vol = elasticity_morph(&mesh, &prescribed, &opts_inv_vol)
            .expect("InverseVolume should succeed");
        let out_inv_edge = elasticity_morph(&mesh, &prescribed, &opts_inv_edge)
            .expect("InverseEdgeLengthSquared should succeed");

        // (b) Determinism: running InverseEdgeLengthSquared twice yields bit-equal output.
        // The assertion is intentionally bit-exact (not tolerance-based): this is the
        // correct contract while AssemblyMode::Deterministic + SolverMode::Deterministic
        // are in effect (see elasticity_morph's assembly and CG calls). If the solver is
        // ever parallelised (e.g. rayon-based reductions), this assertion will go flaky
        // — update it to a tolerance-based check and document the new determinism
        // contract at that point.
        let out_inv_edge_2 = elasticity_morph(&mesh, &prescribed, &opts_inv_edge)
            .expect("InverseEdgeLengthSquared second run should succeed");
        assert_eq!(
            out_inv_edge.vertices, out_inv_edge_2.vertices,
            "InverseEdgeLengthSquared must be deterministic (bit-equal on identical input)"
        );

        // (b) All output vertices are finite — no NaN/Inf from degenerate-element paths.
        for (i, &v) in out_inv_edge.vertices.iter().enumerate() {
            assert!(
                v.is_finite(),
                "InverseEdgeLengthSquared output vertex[{i}] = {v} is not finite"
            );
        }

        // (a) Genuinely distinct from Uniform.
        assert_ne!(
            out_inv_edge.vertices, out_uniform.vertices,
            "InverseEdgeLengthSquared must produce different vertices than Uniform on a graded mesh"
        );

        // (a) Genuinely distinct from InverseVolume.
        assert_ne!(
            out_inv_edge.vertices, out_inv_vol.vertices,
            "InverseEdgeLengthSquared must produce different vertices than InverseVolume on an irregular mesh"
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
        let invalid_tet = ElasticityFailure::InvalidTetIndex(7);
        let no_elements = ElasticityFailure::NoElementsForPrescribedDisplacements;
        let malformed = ElasticityFailure::MalformedTetIndices { len: 12 };

        for failure in [
            &invalid,
            &unsupported,
            &not_converged,
            &invalid_tet,
            &no_elements,
            &malformed,
        ] {
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
                ElasticityFailure::InvalidTetIndex(idx) => {
                    assert_eq!(*idx, 7);
                }
                ElasticityFailure::NoElementsForPrescribedDisplacements => {
                    // No payload to assert — unit variant. The arm's presence
                    // in this no-wildcard match is the verification.
                }
                ElasticityFailure::MalformedTetIndices { len } => {
                    assert_eq!(*len, 12);
                }
            }
        }
    }

    // ── Robustness: non-empty vertices, empty tet_indices, empty prescribed ──

    /// A P1 mesh with non-empty `vertices` but empty `tet_indices` (orphan
    /// nodes) and **empty** `prescribed_positions` is handled by the
    /// short-circuit — no FEA pipeline runs, input vertices are returned
    /// unchanged and normals are dropped.
    ///
    /// The tightened contract (task 3362): the short-circuit ONLY fires when
    /// `prescribed_positions` is empty. Non-empty prescribed positions trigger
    /// `NoElementsForPrescribedDisplacements` instead (tested separately
    /// below). This preserves the documented `output = vertices_old + u`
    /// contract: silently dropping BCs would be semantically wrong.
    ///
    /// Without the `tet_indices.is_empty()` guard the no-BC path would fall
    /// into the FEA pipeline and panic: `assemble_global_stiffness` emits a
    /// 3N×3N matrix with zero stored entries and `solve_cg` panics in
    /// `extract_diag_jacobi` on a zero/missing diagonal.
    #[test]
    fn elasticity_morph_with_non_empty_vertices_but_empty_tet_indices_returns_input_vertices() {
        let mesh = VolumeMesh {
            vertices: vec![0.0_f32, 0.0, 0.0, 1.0, 1.0, 1.0],
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: Some(vec![1.0_f32, 0.0, 0.0, 0.0, 1.0, 0.0]),
        };
        // Empty prescribed_positions → short-circuit fires, not
        // NoElementsForPrescribedDisplacements.
        let result = elasticity_morph(&mesh, &[], &crate::MorphOptions::default());
        let out = result.unwrap();
        // Vertices returned unchanged.
        assert_eq!(out.vertices, mesh.vertices);
        assert!(out.tet_indices.is_empty());
        assert_eq!(out.element_order, ElementOrderTag::P1);
        // Normals dropped regardless of input.
        assert!(out.normals.is_none());
    }

    // ── task 3362 step-5: NoElementsForPrescribedDisplacements structured failure ──

    /// A P1 mesh with non-empty `vertices`, empty `tet_indices`, and
    /// **non-empty** `prescribed_positions` must return
    /// `Err(ElasticityFailure::NoElementsForPrescribedDisplacements)`.
    ///
    /// Silently dropping the BCs (the old behaviour before task 3362) would
    /// violate the documented `output = vertices_old + u` contract — the caller
    /// supplied BCs that have nowhere to be applied. Surfacing a structured
    /// failure lets the engine's projection layer handle the mismatch rather
    /// than silently discarding user intent.
    #[test]
    fn elasticity_morph_with_non_empty_prescribed_positions_but_empty_tet_indices_returns_no_elements_for_prescribed_displacements()
     {
        // 2 valid nodes → n_nodes == 2; node index 0 is in-range.
        let mesh = VolumeMesh {
            vertices: vec![0.0_f32, 0.0, 0.0, 1.0, 1.0, 1.0],
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        // Node 0 is valid for this mesh — the prescribed-positions bounds
        // check passes. The no-tet branch with non-empty prescribed_positions
        // must return NoElementsForPrescribedDisplacements.
        let result = elasticity_morph(
            &mesh,
            &[(0_u32, [0.5_f64, 0.5, 0.5])],
            &crate::MorphOptions::default(),
        );
        match result {
            Err(ElasticityFailure::NoElementsForPrescribedDisplacements) => {}
            other => panic!("expected Err(NoElementsForPrescribedDisplacements), got: {other:?}"),
        }
    }

    // ── task 3362 step-1: SolverNotConverged regression via elasticity_morph_with_cg_opts ──

    /// Regression guard for `SolverNotConverged` via `elasticity_morph_with_cg_opts`.
    ///
    /// Uses the same 4-tet cone fixture as
    /// `elasticity_morph_with_rigid_translation_on_cone_*` (5 nodes, four
    /// surface nodes a/b/c/d pinned to a rigid translation, interior node p
    /// free). With `CgSolverOptions { max_iter: 1, tolerance: 1e-20 }`, a
    /// single CG iteration cannot drive the relative residual below 1e-20 for
    /// this 3-DOF post-Dirichlet system — guaranteed cap-out.
    ///
    /// Asserts `Err(ElasticityFailure::SolverNotConverged { iterations: 1 })`,
    /// exercising the variant-construction path at elasticity.rs:221-223 with
    /// a realistic (non-trivial) FEA system.
    #[test]
    fn elasticity_morph_with_cg_opts_drives_cg_to_capout_returns_solver_not_converged() {
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // 0: a
                1.0, 0.0, 0.0, // 1: b
                0.0, 1.0, 0.0, // 2: c
                0.0, 0.0, 1.0, // 3: d
                0.25, 0.25, 0.25, // 4: p (interior)
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

        let result = elasticity_morph_with_cg_opts(
            &mesh,
            &prescribed,
            &crate::MorphOptions::default(),
            CgSolverOptions {
                tolerance: 1e-20,
                max_iter: 1,
            },
        );
        match result {
            Err(ElasticityFailure::SolverNotConverged { iterations }) => {
                assert_eq!(
                    iterations, 1,
                    "expected iterations == 1 (max_iter), got {iterations}"
                );
            }
            other => panic!("expected Err(SolverNotConverged {{ iterations: 1 }}), got: {other:?}"),
        }
    }

    // ── task 3362 step-3: InvalidTetIndex structured failure ─────────────────

    /// A tet mesh where `tet_indices` contains an out-of-range index (index 99
    /// when `n_nodes == 4`) must return
    /// `Err(ElasticityFailure::InvalidTetIndex(99))`.
    ///
    /// The prescribed_positions has a single valid entry `(0, [0,0,0])` so the
    /// prescribed-positions validation passes (node 0 is in-range for 4 nodes)
    /// and the new upfront tet_indices validation fires first.
    #[test]
    fn elasticity_morph_with_out_of_range_tet_index_returns_invalid_tet_index() {
        // 4 nodes → n_nodes == 4; tet_index 99 >= 4 triggers InvalidTetIndex.
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // node 0
                1.0, 0.0, 0.0, // node 1
                0.0, 1.0, 0.0, // node 2
                0.0, 0.0, 1.0, // node 3
            ],
            tet_indices: vec![0, 1, 2, 99], // index 99 >= n_nodes (4)
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        // Prescribed node 0 is valid (index 0 < 4); passes the prescribed-
        // positions validation gate so the tet_indices upfront check fires.
        let result = elasticity_morph(
            &mesh,
            &[(0_u32, [0.0_f64, 0.0, 0.0])],
            &crate::MorphOptions::default(),
        );
        match result {
            Err(ElasticityFailure::InvalidTetIndex(idx)) => {
                assert_eq!(
                    idx, 99,
                    "expected InvalidTetIndex(99), got InvalidTetIndex({idx})"
                );
            }
            other => panic!("expected Err(InvalidTetIndex(99)), got: {other:?}"),
        }
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

    // ── task 3449: non-multiple-of-4 tet_indices — precedence ordering ────────

    /// Pins **precedence ordering**: when `tet_indices.len() % 4 != 0` AND the
    /// tail contains an out-of-range index, the malformed-length check fires
    /// first and `MalformedTetIndices` is returned — NOT `InvalidTetIndex`.
    ///
    /// Guards against a future refactor that places the bounds-check before the
    /// malformed-length check, which would silently invert the precedence and
    /// break the all-in-range tail case (covered by the companion test
    /// `elasticity_morph_with_malformed_tet_indices_length_returns_malformed_tet_indices`).
    #[test]
    fn elasticity_morph_with_malformed_tet_indices_length_and_out_of_range_tail_returns_malformed_tet_indices()
     {
        // 4 nodes → n_nodes == 4.
        // tet_indices = [0, 1, 2, 3, 99]: one complete tet (indices 0..3)
        // plus a stray tail entry (index 99 >= 4) — len == 5, 5 % 4 == 1.
        // The malformed-length check fires first (len % 4 != 0), returning
        // MalformedTetIndices { len: 5 } before the bounds-check loop sees 99.
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // node 0
                1.0, 0.0, 0.0, // node 1
                0.0, 1.0, 0.0, // node 2
                0.0, 0.0, 1.0, // node 3
            ],
            tet_indices: vec![0, 1, 2, 3, 99], // valid tet + stray OOR tail
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let result = elasticity_morph(&mesh, &[], &crate::MorphOptions::default());
        match result {
            Err(ElasticityFailure::MalformedTetIndices { len }) => {
                assert_eq!(len, 5, "expected len=5, got len={len}");
            }
            other => panic!("expected Err(MalformedTetIndices {{ len: 5 }}), got: {other:?}"),
        }
    }

    // ── task 3449: surface non-multiple-of-4 tet_indices length as MalformedTetIndices ──

    /// Pins that a non-multiple-of-4 `tet_indices` length is rejected upfront
    /// as a structured failure rather than being silently truncated by the FEA
    /// pipeline's `chunks_exact(4)` loop.
    ///
    /// All indices are in-range (0..=3 with 4 nodes), so this case is NOT
    /// caught by the existing bounds-check — it would silently drop the
    /// trailing 3 entries before this fix. Mirrors the structured-failure
    /// pattern of `NoElementsForPrescribedDisplacements`: don't silently drop
    /// user input.
    #[test]
    fn elasticity_morph_with_malformed_tet_indices_length_returns_malformed_tet_indices() {
        // 4 nodes → n_nodes == 4.
        // tet_indices = [0, 1, 2, 3, 0, 1, 2]: one valid tet + 3-entry in-range
        // tail — len == 7, 7 % 4 == 3.
        // All indices are in-range (< 4), so the existing bounds-check would NOT
        // catch this — chunks_exact(4) would silently discard the trailing triple.
        let mesh = VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // node 0
                1.0, 0.0, 0.0, // node 1
                0.0, 1.0, 0.0, // node 2
                0.0, 0.0, 1.0, // node 3
            ],
            tet_indices: vec![0, 1, 2, 3, 0, 1, 2], // valid tet + in-range tail
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let result = elasticity_morph(&mesh, &[], &crate::MorphOptions::default());
        match result {
            Err(ElasticityFailure::MalformedTetIndices { len }) => {
                assert_eq!(len, 7, "expected len=7, got len={len}");
            }
            other => panic!("expected Err(MalformedTetIndices {{ len: 7 }}), got: {other:?}"),
        }
    }
}
