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

use reify_types::Mesh;

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
pub fn repair_surface_mesh(mesh: &Mesh, cfg: RepairConfig) -> Mesh {
    let vert_count = mesh.vertices.len() / 3;

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
    let new_indices: Vec<u32> = survivors
        .into_iter()
        .map(|i| remap[i as usize])
        .collect();

    Mesh {
        vertices: new_vertices,
        indices: new_indices,
        normals: None,
    }
}
