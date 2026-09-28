//! A per-volume-vertex mesh size field, serialised for gmsh's post-view API.
//!
//! [`BackgroundSizeField`] is what lets [`crate::refine_volume_with_size_field`]
//! honour a size field with an INTERIOR minimum. Prescribing sizes on 0D model
//! entities instead — gmsh's `Mesh.MeshSizeFromPoints` path — gives a box eight
//! corner scalars, and eight corners interpolate monotonically along each axis,
//! so an interior minimum is structurally unrepresentable there.
//!
//! The type is plain data with no FFI of its own, so it is deliberately NOT
//! `cfg(has_gmsh)`-gated: both build arms of `refine_volume_with_size_field`
//! take it as a parameter, and that uniform signature is the convention stated
//! in [`crate::refine_volume`]'s module doc.

use reify_ir::{GeometryError, VolumeMesh};

/// Doubles in one `"SS"` (scalar-on-tetrahedron) record: 4 x-coordinates,
/// 4 y, 4 z, then 4 values.
const SS_STRIDE: usize = 16;

/// Corner nodes of a tetrahedron. A P2 element's six edge midpoints are not
/// among them: gmsh's `"SS"` primitive is a LINEAR tet.
const TET_CORNERS: usize = 4;

fn fail(msg: String) -> GeometryError {
    GeometryError::OperationFailed(msg)
}

/// A mesh size field defined on the tetrahedra of a sizing mesh, ready to hand
/// to `gmshViewAddListData` as `"SS"` list data.
///
/// Construct with [`BackgroundSizeField::from_tet_mesh`] — the only
/// constructor, so the buffer layout is decided in exactly one place and no
/// consumer can re-derive it wrongly.
#[derive(Debug, Clone)]
pub struct BackgroundSizeField {
    list_data: Vec<f64>,
    element_count: usize,
    max_size: f64,
}

impl BackgroundSizeField {
    /// Build the field from a tet `volume_mesh` and one size per mesh vertex.
    ///
    /// Sizes are read only at vertices some tet references; a vertex no
    /// element indexes cannot reach the buffer, which is what makes the
    /// `f64::INFINITY` that
    /// `reify_solver_elastic::volume_refine::project_per_element_sizes_to_vertices`
    /// leaves at orphaned vertices harmless without a filter anyone could
    /// forget to write.
    pub fn from_tet_mesh(
        volume_mesh: &VolumeMesh,
        vertex_sizes: &[f64],
    ) -> Result<Self, GeometryError> {
        let indices = volume_mesh.tet_indices().ok_or_else(|| {
            fail(
                "BackgroundSizeField::from_tet_mesh: requires a tet mesh; \
                 got hex/wedge connectivity"
                    .to_string(),
            )
        })?;

        let vertex_count = volume_mesh.vertices.len() / 3;
        if vertex_sizes.len() != vertex_count {
            return Err(fail(format!(
                "BackgroundSizeField::from_tet_mesh: got {} sizes for {} mesh vertices",
                vertex_sizes.len(),
                vertex_count
            )));
        }

        let stride = volume_mesh.nodes_per_element();
        if indices.len() % stride != 0 {
            return Err(fail(format!(
                "BackgroundSizeField::from_tet_mesh: {} tet indices is not a whole number of \
                 {stride}-node elements",
                indices.len()
            )));
        }
        let element_count = indices.len() / stride;
        if element_count == 0 {
            return Err(fail(
                "BackgroundSizeField::from_tet_mesh: sizing mesh has no elements; \
                 an empty field would silently leave the remesh unsized"
                    .to_string(),
            ));
        }

        let mut list_data = Vec::with_capacity(SS_STRIDE * element_count);
        let mut max_size = f64::NEG_INFINITY;

        for element in indices.chunks_exact(stride) {
            // Only the first four nodes: gmsh's "SS" primitive is a linear tet,
            // and a P2 element's edge midpoints carry no sizing information its
            // corners do not.
            let mut corners = [0_usize; TET_CORNERS];
            for (corner, &node) in corners.iter_mut().zip(&element[..TET_CORNERS]) {
                let node = node as usize;
                if node >= vertex_count {
                    return Err(fail(format!(
                        "BackgroundSizeField::from_tet_mesh: tet index {node} is out of range \
                         for {vertex_count} mesh vertices"
                    )));
                }
                *corner = node;
            }

            // Coordinates are grouped by AXIS, then the values — gmshc.h:3200-3204.
            // The ASCII `.pos` "parsed" format groups per point instead, and
            // gmsh accepts that grouping here with ierr=0 while meshing a
            // scrambled field, so the ordering is pinned by a byte-exact test.
            for axis in 0..3 {
                list_data.extend(
                    corners
                        .iter()
                        .map(|&node| f64::from(volume_mesh.vertices[3 * node + axis])),
                );
            }
            for &node in &corners {
                let size = vertex_sizes[node];
                if !size.is_finite() || size <= 0.0 {
                    return Err(fail(format!(
                        "BackgroundSizeField::from_tet_mesh: vertex {node} is referenced by a \
                         tet but carries a non-finite or non-positive size ({size})"
                    )));
                }
                max_size = max_size.max(size);
                list_data.push(size);
            }
        }

        Ok(Self {
            list_data,
            element_count,
            max_size,
        })
    }

    /// The `"SS"` list-data buffer: `SS_STRIDE * element_count` doubles.
    pub fn list_data(&self) -> &[f64] {
        &self.list_data
    }

    /// Element count, which `gmshViewAddListData` takes separately from the
    /// buffer length and does not cross-check against it.
    pub fn element_count(&self) -> usize {
        self.element_count
    }

    /// The coarsest size actually emitted.
    ///
    /// This becomes `Mesh.MeshSizeMax`, preserving the "cap at the coarsest
    /// hint the caller requested, never at `options.mesh_size`" contract.
    /// Computed during construction over the EMITTED values, so the orphan
    /// infinities that never reach the buffer cannot poison it either.
    pub fn max_size(&self) -> f64 {
        self.max_size
    }
}
