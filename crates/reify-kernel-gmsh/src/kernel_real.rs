//! Real `GmshKernel` — backed by hand-rolled extern "C" FFI to libgmsh 4.15.2.
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.
//!
//! # Public surface
//!
//! `GmshKernel::mesh_to_volume(&Mesh, &MeshingOptions, ElementOrderTag) ->
//! Result<VolumeMesh, GeometryError>` is the typed entry point real callers
//! (e.g. sibling task #2928's FEA orchestration) use to drive surface→volume
//! tet meshing. The dispatcher's `Convert{from:Mesh}→VolumeMesh` route lives
//! at the capability-descriptor layer; trait dispatch through
//! `GeometryKernel::execute()` continues to error with a descriptive
//! message that points callers at `mesh_to_volume`.
//!
//! # Concurrency
//!
//! Every entry point that touches gmsh state acquires
//! [`crate::init::GMSH_LOCK`] before its first FFI call. Gmsh has process-
//! wide model + option state; concurrent callers from sibling crates would
//! corrupt each other's outputs. The lock is `pub static` so test binaries
//! can serialise their own gmsh access against the production code path.

use std::collections::BTreeMap;

use reify_types::{
    ElementOrderTag, ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId,
    GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value, VolumeMesh,
};

use crate::ffi;
use crate::init;

// ---------------------------------------------------------------------------
// GmshKernel
// ---------------------------------------------------------------------------

/// Real Gmsh kernel — drives libgmsh 4.15.2 surface→volume tet meshing.
///
/// `_private: ()` prevents external struct-literal construction; callers go
/// through [`Self::new`] / [`Self::default`]. Mirrors the OpenVDB stub-vs-real
/// shape convention.
///
/// `Send + Sync` are auto-derived: the struct holds no state — the gmsh
/// library state lives behind `GMSH_LOCK` in `init.rs`. Acquiring the lock
/// at every entry point is what makes concurrent calls safe, not field
/// ownership.
pub struct GmshKernel {
    _private: (),
}

impl GmshKernel {
    /// Construct a new `GmshKernel`. The gmsh library is initialised lazily
    /// on the first `mesh_to_volume` call (via
    /// `init::ensure_initialized`).
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Convert a closed surface mesh to a volumetric tet mesh via gmsh's
    /// HXT 3D-meshing pipeline.
    ///
    /// `surface`: triangle-soup boundary mesh — must be closed and outward-
    /// winding for HXT to find an interior region.
    /// `options`: user-tunable knobs (see [`MeshingOptions`](crate::MeshingOptions)).
    /// `element_order`: P1 (4-node) or P2 (10-node) tets.
    ///
    /// # Errors
    ///
    /// Returns `GeometryError::OperationFailed` annotated with the gmsh
    /// function name and last-error message on any FFI failure (lock
    /// acquisition, model setup, mesh generation, readback). Common
    /// failure modes: open / non-manifold input mesh, degenerate triangles,
    /// HXT internal errors.
    pub fn mesh_to_volume(
        &self,
        surface: &Mesh,
        _options: &crate::MeshingOptions,
        element_order: ElementOrderTag,
    ) -> Result<VolumeMesh, GeometryError> {
        let _guard = init::GMSH_LOCK.lock().map_err(|e| {
            GeometryError::OperationFailed(format!("GMSH_LOCK poisoned: {e}"))
        })?;
        init::ensure_initialized();
        ffi::clear()?;
        // Silence gmsh's stdout chatter — keeps test output readable.
        ffi::option_set_number("General.Terminal", 0.0)?;

        ffi::model_add("reify_volume_mesh")?;
        let surf_tag = ffi::add_discrete_entity(2, &[])?;

        // Push surface vertices: 1-indexed gmsh tags 1..=N, parallel coord
        // array widened from f32 -> f64.
        let n_verts = surface.vertices.len() / 3;
        let node_tags: Vec<u64> = (1..=n_verts as u64).collect();
        let coords_f64: Vec<f64> = surface.vertices.iter().map(|&v| v as f64).collect();
        ffi::add_nodes_2d(surf_tag, &node_tags, &coords_f64)?;

        // Push surface triangles: gmsh element type 2 = 3-node triangle.
        // Reshape u32 indices -> u64, +1 (gmsh node tags are 1-based).
        let n_tris = surface.indices.len() / 3;
        let tri_tags: Vec<u64> = (1..=n_tris as u64).collect();
        let tri_node_tags: Vec<u64> =
            surface.indices.iter().map(|&i| i as u64 + 1).collect();
        ffi::add_elements_2d(surf_tag, 2, &tri_tags, &tri_node_tags)?;

        // Reclassify the discrete surface and build geometry so 3D meshing
        // has a parametric region to fill. Dihedral threshold π/2 (90°)
        // splits the cube faces; π for curve-feature detection accepts any
        // sharp edge.
        ffi::classify_surfaces(
            std::f64::consts::FRAC_PI_2,
            1,
            1,
            std::f64::consts::PI,
            0,
        )?;
        ffi::create_geometry(&[])?;

        // After classify+createGeometry, gmsh creates new geometric surface
        // entities whose tags may differ from `surf_tag`. Query them so
        // `geo_add_surface_loop` references the correct entities.
        let surface_tags = ffi::get_entity_tags(2)?;
        if surface_tags.is_empty() {
            return Err(GeometryError::OperationFailed(
                "gmsh produced no dim=2 entities after classify_surfaces+create_geometry — \
                 input surface mesh may be open or non-manifold"
                    .into(),
            ));
        }
        let _ = surf_tag; // Original discrete-entity tag is no longer used.

        // Wrap the reclassified surface(s) in a surface loop and a volume
        // so HXT has a closed region to mesh.
        let loop_tag = ffi::geo_add_surface_loop(&surface_tags)?;
        let _vol_tag = ffi::geo_add_volume(&[loop_tag])?;
        ffi::geo_synchronize()?;

        // Tet meshing.
        ffi::mesh_generate(3)?;

        // Element type for readback: P1 = 4 (4-node tet), P2 = 11 (10-node tet).
        let elem_type = match element_order {
            ElementOrderTag::P1 => 4,
            ElementOrderTag::P2 => 11,
        };

        let (node_tags, coord_buf) = ffi::get_nodes_all()?;
        let (_elem_tags, elem_node_tags) = ffi::get_elements_by_type(elem_type)?;

        // Build (gmsh_tag → 0-based local idx) by sorting node tags and
        // assigning indices in tag order. Vertices are emitted in the same
        // sorted order so tag-N → index-N once remapped.
        let mut paired: Vec<(u64, [f64; 3])> = node_tags
            .iter()
            .copied()
            .zip(coord_buf.chunks_exact(3))
            .map(|(t, c)| (t, [c[0], c[1], c[2]]))
            .collect();
        paired.sort_by_key(|(t, _)| *t);

        let mut tag_to_idx: BTreeMap<u64, u32> =
            BTreeMap::new();
        let mut vertices: Vec<f32> = Vec::with_capacity(paired.len() * 3);
        for (idx, (tag, xyz)) in paired.iter().enumerate() {
            tag_to_idx.insert(*tag, idx as u32);
            vertices.extend(xyz.iter().map(|&v| v as f32));
        }

        // Remap connectivity from gmsh tags to 0-based local indices.
        let mut tet_indices: Vec<u32> = Vec::with_capacity(elem_node_tags.len());
        for &tag in &elem_node_tags {
            let idx = *tag_to_idx.get(&tag).ok_or_else(|| {
                GeometryError::OperationFailed(format!(
                    "gmsh element references unknown node tag {tag} (mesh corruption?)"
                ))
            })?;
            tet_indices.push(idx);
        }

        // Defensive cleanup: clear the model so the next mesh_to_volume call
        // starts from a known-empty state.
        ffi::clear()?;

        Ok(VolumeMesh {
            vertices,
            tet_indices,
            element_order,
            normals: None,
        })
    }
}

impl Default for GmshKernel {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GeometryKernel trait — execute() / query() / etc. all error
// ---------------------------------------------------------------------------

const TRAIT_DISPATCH_MSG: &str = "Gmsh trait dispatch through GeometryKernel::execute is not \
    yet routed for Mesh→VolumeMesh; call `GmshKernel::mesh_to_volume` directly.";

impl GeometryKernel for GmshKernel {
    fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(TRAIT_DISPATCH_MSG.into()))
    }

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(TRAIT_DISPATCH_MSG.into()))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(TRAIT_DISPATCH_MSG.into()))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(TRAIT_DISPATCH_MSG.into()))
    }
}
