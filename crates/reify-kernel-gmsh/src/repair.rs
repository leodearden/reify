//! Surface-mesh repair pre-stage — collapse slivers, merge near-coincident vertices.
//!
//! Per the v0.3 FEA PRD, raw OCCT BRepMesh output contains pathologies that
//! cause Gmsh to fail on tight features:
//!
//!   - Sliver triangles (area below an engineering-significant threshold)
//!     produce zero-volume tetrahedra during volume meshing.
//!   - Pairs of near-coincident vertices generate degenerate edges.
//!
//! This pre-stage normalises the surface mesh before handing it to Gmsh.
//! The implementation is pure-Rust and unit-testable without libgmsh.
//!
//! # Performance bound
//!
//! The vertex-merge scan is currently O(n²): each vertex is compared against
//! every prior vertex. For v0.3 surface meshes (typically <100k vertices) this
//! is acceptable. A future spatial-hash refinement (cell-binned hash by
//! `vertex_merge_epsilon`) would drop the bound to O(n) average; the function
//! signature is stable enough that swapping implementations is a contained
//! follow-up.

use reify_ir::Mesh;

/// Configuration for the [`repair_surface_mesh`] pre-stage.
///
/// Defaults are conservative — `1e-9` for sliver area and `1e-12` for vertex
/// merge — chosen so a well-formed mesh passes through unchanged unless the
/// caller deliberately raises the thresholds.
#[derive(Debug, Clone, Copy)]
pub struct RepairConfig {
    /// Minimum triangle area; triangles below this are dropped.
    pub sliver_area_threshold: f64,
    /// Vertex pairs closer than this Euclidean distance are merged into a
    /// single surviving position.
    pub vertex_merge_epsilon: f64,
}

impl Default for RepairConfig {
    fn default() -> Self {
        Self {
            sliver_area_threshold: 1e-9,
            vertex_merge_epsilon: 1e-12,
        }
    }
}

/// Repair a surface mesh by collapsing sliver triangles and merging
/// near-coincident vertices.
///
/// Returns a fresh `Mesh` with:
///   - merged-away vertex positions removed (vertices array compacted),
///   - triangles re-indexed onto the surviving positions,
///   - sliver and now-degenerate triangles dropped from `indices`.
///
/// Optional `normals` are dropped; the v0.3 mesher recomputes them downstream
/// if needed (carrying old normals across a re-indexing introduces subtle
/// alignment bugs and the volume mesher does not require them).
///
/// # Transitive (chain) merging
///
/// The algorithm performs a single first-match-wins pass, so it is **transitive**
/// by construction: if A↔B and B↔C are each within `vertex_merge_epsilon` but
/// A↔C is not, all three vertices still collapse to A's position. The classic
/// chain case is three near-collinear vertices A, B, C separated by ε each —
/// A↔C is 2ε apart yet the function unifies them via B. This is intentional for
/// v0.3 — it produces a well-defined survivor for arbitrarily long chains and
/// matches the typical caller intent (collapse pathological clusters of OCCT
/// duplicates regardless of pair-wise spacing). Future maintainers reading this
/// function should NOT assume the function only merges directly-coincident
/// pairs; pairs at >ε can also be unified through intermediate vertices.
pub fn repair_surface_mesh(mesh: &Mesh, cfg: RepairConfig) -> Mesh {
    let vert_count = mesh.vertices.len() / 3;

    // Perf canary: the O(n²) merge scan is cheap on v0.3 surface meshes
    // (typically <100k vertices) but visibly slow above that. Rather than a
    // `debug_assert!` (which hard-crashes debug/test builds and CI), we emit a
    // `tracing::warn!` so the concern stays visible in any build without
    // crashing tests. Operators can filter via `RUST_LOG=reify_kernel_gmsh::repair=warn`.
    // A future spatial-hash bucket refinement (cell-binned by vertex_merge_epsilon)
    // would drop the bound to O(n) average; the function signature is stable
    // enough that swapping implementations is a contained follow-up.
    // Threshold constant kept for ease of future tuning.
    const LARGE_VERT_THRESHOLD: usize = 100_000;
    if vert_count > LARGE_VERT_THRESHOLD {
        tracing::warn!(
            target: "reify_kernel_gmsh::repair",
            reason = "large_mesh_perf",
            vert_count = vert_count,
            threshold = LARGE_VERT_THRESHOLD,
            "repair_surface_mesh: vertex count exceeds the O(n²) scan's comfort \
             threshold; consider landing the spatial-hash bucket refinement \
             before relying on this code path at scale"
        );
    }

    // -----------------------------------------------------------------
    // (1) Build vertex-merge map: each vertex → its lowest-index near-coincident
    //     survivor. Self-mapping is the identity for unique vertices.
    // -----------------------------------------------------------------
    let mut merge_map: Vec<u32> = (0..vert_count as u32).collect();
    let eps_sq = cfg.vertex_merge_epsilon * cfg.vertex_merge_epsilon;

    for i in 0..vert_count {
        for j in 0..i {
            // Compare i to every prior j; if i is near j, merge i → merge_map[j].
            let xi = mesh.vertices[i * 3] as f64;
            let yi = mesh.vertices[i * 3 + 1] as f64;
            let zi = mesh.vertices[i * 3 + 2] as f64;
            let xj = mesh.vertices[j * 3] as f64;
            let yj = mesh.vertices[j * 3 + 1] as f64;
            let zj = mesh.vertices[j * 3 + 2] as f64;
            let dx = xi - xj;
            let dy = yi - yj;
            let dz = zi - zj;
            if dx * dx + dy * dy + dz * dz <= eps_sq {
                merge_map[i] = merge_map[j];
                break; // first-match wins; preserves lowest-index survivor.
            }
        }
    }

    // -----------------------------------------------------------------
    // (2) Re-index triangles via merge_map; drop triangles that became
    //     degenerate (any two corners share an index after merging) AND
    //     triangles whose surviving area is below the sliver threshold.
    // -----------------------------------------------------------------
    let mut survivors: Vec<u32> = Vec::with_capacity(mesh.indices.len());
    let area_thresh = cfg.sliver_area_threshold;

    for tri in mesh.indices.chunks_exact(3) {
        let a = merge_map[tri[0] as usize];
        let b = merge_map[tri[1] as usize];
        let c = merge_map[tri[2] as usize];
        if a == b || b == c || a == c {
            continue; // degenerate after merge
        }
        // Compute area via cross-product magnitude / 2.
        let ax = mesh.vertices[(a as usize) * 3] as f64;
        let ay = mesh.vertices[(a as usize) * 3 + 1] as f64;
        let az = mesh.vertices[(a as usize) * 3 + 2] as f64;
        let bx = mesh.vertices[(b as usize) * 3] as f64;
        let by = mesh.vertices[(b as usize) * 3 + 1] as f64;
        let bz = mesh.vertices[(b as usize) * 3 + 2] as f64;
        let cx = mesh.vertices[(c as usize) * 3] as f64;
        let cy = mesh.vertices[(c as usize) * 3 + 1] as f64;
        let cz = mesh.vertices[(c as usize) * 3 + 2] as f64;
        let ux = bx - ax;
        let uy = by - ay;
        let uz = bz - az;
        let vx = cx - ax;
        let vy = cy - ay;
        let vz = cz - az;
        let cross_x = uy * vz - uz * vy;
        let cross_y = uz * vx - ux * vz;
        let cross_z = ux * vy - uy * vx;
        let area = 0.5 * (cross_x * cross_x + cross_y * cross_y + cross_z * cross_z).sqrt();
        if area < area_thresh {
            continue;
        }
        survivors.push(a);
        survivors.push(b);
        survivors.push(c);
    }

    // -----------------------------------------------------------------
    // (3) Compact vertex array: only keep positions that some surviving
    //     triangle still references; build a remap to translate old indices.
    // -----------------------------------------------------------------
    let mut keep: Vec<bool> = vec![false; vert_count];
    for &idx in &survivors {
        keep[idx as usize] = true;
    }
    let mut remap: Vec<u32> = vec![u32::MAX; vert_count];
    let mut new_vertices: Vec<f32> = Vec::with_capacity(mesh.vertices.len());
    let mut new_idx: u32 = 0;
    for (old_idx, &k) in keep.iter().enumerate() {
        if k {
            new_vertices.push(mesh.vertices[old_idx * 3]);
            new_vertices.push(mesh.vertices[old_idx * 3 + 1]);
            new_vertices.push(mesh.vertices[old_idx * 3 + 2]);
            remap[old_idx] = new_idx;
            new_idx += 1;
        }
    }
    let new_indices: Vec<u32> = survivors.into_iter().map(|i| remap[i as usize]).collect();

    Mesh {
        vertices: new_vertices,
        indices: new_indices,
        normals: None,
    }
}
