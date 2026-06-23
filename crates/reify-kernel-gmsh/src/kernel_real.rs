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

use std::collections::HashMap;
use std::sync::Mutex;

use reify_ir::{ElementOrderTag, ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value, VolumeMesh};

use crate::auto_size::{AutoSizeConfig, auto_mesh_size_from_features};
use crate::ffi;
use crate::init;

// ---------------------------------------------------------------------------
// GmshKernel
// ---------------------------------------------------------------------------

/// Real Gmsh kernel — drives libgmsh 4.15.2 surface→volume tet meshing.
///
/// # Realized-volume-mesh store (realization-read task γ)
///
/// `volume_mesh_store` is an interior-mutable handle→[`VolumeMesh`] map. The
/// realization-read accessor [`GeometryKernel::volume_mesh`] is
/// handle-addressable, but gmsh is otherwise stateless (it produces meshes via
/// the free [`Self::mesh_to_volume`] and every other trait method errors). The
/// store gives `volume_mesh(handle)` something to read back. The `Mutex`
/// preserves the trait's `&self` receiver (no `&mut self` churn at call sites)
/// and mirrors gmsh's existing `GMSH_LOCK` interior-mutability pattern.
///
/// **Population seam (downstream task 3429):** γ delivers retrieval
/// ([`GeometryKernel::volume_mesh`]) plus the [`Self::store_volume_mesh`]
/// population entry point; the production realize-time dispatch that calls
/// `store_volume_mesh` to fill the map is downstream task 3429. Until that
/// lands, the store is populated only by tests.
///
/// `Send + Sync`: `Mutex<VolumeMeshStore>` is `Send + Sync` because
/// `VolumeMesh` (plain `Vec`/enum fields) is `Send`. The gmsh *library* state
/// still lives behind `GMSH_LOCK` in `init.rs`; acquiring that lock at every
/// FFI entry point is what makes concurrent meshing safe — this struct-local
/// `Mutex` only guards the volume-mesh store.
pub struct GmshKernel {
    volume_mesh_store: Mutex<VolumeMeshStore>,
}

/// Interior state of the [`GmshKernel`] realized-volume-mesh store: a
/// monotonic id counter plus the handle→mesh map. Both live under one `Mutex`
/// so [`GmshKernel::store_volume_mesh`] can allocate a fresh id and insert
/// atomically.
struct VolumeMeshStore {
    /// Next handle id to mint. Starts at 1 (mirrors `reify-kernel-manifold`'s
    /// `next_id` convention; keeps 0 free as a never-allocated sentinel).
    next_id: u64,
    /// Realized volume meshes keyed by the handle returned from
    /// [`GmshKernel::store_volume_mesh`].
    meshes: HashMap<GeometryHandleId, VolumeMesh>,
}

impl GmshKernel {
    /// Construct a new `GmshKernel` with an empty volume-mesh store. The gmsh
    /// library is initialised lazily on the first `mesh_to_volume` call (via
    /// `init::ensure_initialized`).
    pub fn new() -> Self {
        Self {
            volume_mesh_store: Mutex::new(VolumeMeshStore {
                next_id: 1,
                meshes: HashMap::new(),
            }),
        }
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
        options: &crate::MeshingOptions,
        element_order: ElementOrderTag,
    ) -> Result<VolumeMesh, GeometryError> {
        // Validate the input mesh before acquiring the gmsh lock — fail fast
        // with a precise diagnostic rather than letting a silent floor-divide
        // (`vertices.len() / 3`, `indices.len() / 3`) discard trailing data
        // and hand a partially-malformed buffer to gmsh. Cheap insurance at
        // the FFI boundary. Bounds-checking each index also short-circuits
        // before gmsh would otherwise produce an opaque internal error.
        if !surface.vertices.len().is_multiple_of(3) {
            return Err(GeometryError::OperationFailed(format!(
                "mesh_to_volume: surface.vertices.len() = {} is not divisible by 3 \
                 (expected flat XYZ stride)",
                surface.vertices.len()
            )));
        }
        if !surface.indices.len().is_multiple_of(3) {
            return Err(GeometryError::OperationFailed(format!(
                "mesh_to_volume: surface.indices.len() = {} is not divisible by 3 \
                 (expected triangle stride)",
                surface.indices.len()
            )));
        }
        let n_verts = surface.vertices.len() / 3;
        if let Some(&bad) = surface.indices.iter().find(|&&i| (i as usize) >= n_verts) {
            return Err(GeometryError::OperationFailed(format!(
                "mesh_to_volume: surface.indices contains {bad}, which is out of bounds \
                 for a mesh with {n_verts} vertices (valid range 0..{n_verts})"
            )));
        }
        // Reject empty input outright: gmsh accepts empty add_nodes_2d /
        // add_elements_2d calls but the resulting `mesh_generate(3)` produces
        // a zero-tet VolumeMesh, which is never a useful caller outcome.
        // Failing fast at the boundary keeps the diagnostic close to the
        // real cause (caller passed nothing to mesh) rather than letting a
        // silent zero-tet result propagate downstream.
        if surface.vertices.is_empty() || surface.indices.is_empty() {
            return Err(GeometryError::OperationFailed(format!(
                "mesh_to_volume: empty surface mesh \
                 (vertices.len()={}, indices.len()={})",
                surface.vertices.len(),
                surface.indices.len()
            )));
        }

        // Recover from a poisoned lock rather than propagating the failure:
        // every call begins with `ffi::clear()` immediately below, which
        // wipes any half-built model state left over from a panicked prior
        // call. Without this, a single panic anywhere under the lock would
        // permanently disable meshing for the rest of the process lifetime.
        let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init::ensure_initialized();
        ffi::clear()?;
        // Silence gmsh's stdout chatter — keeps test output readable.
        ffi::option_set_number("General.Terminal", 0.0)?;

        // Resolve mesh size: caller override > auto-derived from smallest
        // triangle edge. `auto_mesh_size_from_features` returns 0.0 for
        // empty meshes; we leave the gmsh defaults in place in that case
        // (skip the SetNumber call).
        let resolved_size = match options.mesh_size {
            Some(s) => s,
            None => {
                auto_mesh_size_from_features(surface, AutoSizeConfig::default()).map_err(|e| {
                    GeometryError::OperationFailed(format!(
                        "auto_mesh_size_from_features failed: {e}"
                    ))
                })?
            }
        };
        if resolved_size > 0.0 {
            ffi::option_set_number("Mesh.MeshSizeMin", resolved_size)?;
            ffi::option_set_number("Mesh.MeshSizeMax", resolved_size)?;
        }

        // HXT explicit: gmsh's Algorithm3D codes — 10 = HXT, the modern
        // parallel tet-meshing kernel. Pinning the choice insulates us from
        // gmsh's default-algorithm churn across point releases.
        ffi::option_set_number("Mesh.Algorithm3D", 10.0)?;

        // Thread count: deterministic mode forces 1; otherwise honour
        // caller override; otherwise probe available parallelism. We avoid
        // introducing `num_cpus` as a workspace dep — `available_parallelism`
        // is the std-library equivalent landed in 1.59.
        let num_threads: f64 = if options.deterministic {
            1.0
        } else {
            match options.threads {
                Some(t) => t as f64,
                None => std::thread::available_parallelism()
                    .map(|n| n.get() as f64)
                    .unwrap_or(1.0),
            }
        };
        ffi::option_set_number("General.NumThreads", num_threads)?;

        // Element order: must be set BEFORE mesh_generate(3) so HXT emits
        // tets of the requested order. Readback later uses the matching
        // gmsh element-type code (4 = P1 4-node tet, 11 = P2 10-node tet).
        let element_order_value: f64 = match element_order {
            ElementOrderTag::P1 => 1.0,
            ElementOrderTag::P2 => 2.0,
        };
        ffi::option_set_number("Mesh.ElementOrder", element_order_value)?;

        ffi::model_add("reify_volume_mesh")?;
        let surf_tag = ffi::add_discrete_entity(2, &[])?;

        // Push surface vertices: 1-indexed gmsh tags 1..=N, parallel coord
        // array widened from f32 -> f64. `n_verts` was validated and computed
        // above before lock acquisition.
        let node_tags: Vec<u64> = (1..=n_verts as u64).collect();
        let coords_f64: Vec<f64> = surface.vertices.iter().map(|&v| v as f64).collect();
        ffi::add_nodes_2d(surf_tag, &node_tags, &coords_f64)?;

        // Push surface triangles: gmsh element type 2 = 3-node triangle.
        // Reshape u32 indices -> u64, +1 (gmsh node tags are 1-based).
        let n_tris = surface.indices.len() / 3;
        let tri_tags: Vec<u64> = (1..=n_tris as u64).collect();
        let tri_node_tags: Vec<u64> = surface.indices.iter().map(|&i| i as u64 + 1).collect();
        ffi::add_elements_2d(surf_tag, 2, &tri_tags, &tri_node_tags)?;

        // Reclassify the discrete surface and build geometry so 3D meshing
        // has a parametric region to fill. Dihedral threshold π/2 (90°)
        // splits cube faces into separate B-rep surface entities. curve_angle
        // π/4 (45°) is the bend-angle threshold above which curve-curve
        // junctions are emitted as B-rep vertex entities — sharper than the
        // π default and recognizes 90°-junction corners during classification.
        //
        // Note: this affects only the B-rep classification. `create_geometry`
        // and `mesh_generate(3)` below re-mesh from the resulting parametric
        // geometry, so output node identities are gmsh's choice and do NOT
        // preserve the input discrete vertex set — confirmed by the
        // diagnostic test in `tests/gmsh_classify_diagnostics.rs`. See task
        // 3591 for the broader NodeAttachment-producer redesign that
        // implication motivates.
        ffi::classify_surfaces(std::f64::consts::FRAC_PI_2, 1, 1, std::f64::consts::FRAC_PI_4, 0)?;
        ffi::create_geometry(&[])?;

        // After classify+createGeometry, gmsh creates new geometric surface
        // entities whose tags supersede the original discrete-entity
        // `surf_tag`; query them so `geo_add_surface_loop` references the
        // correct entities. (`surf_tag` is the discrete-mesh entity tag and
        // is no longer referenced from this point on.)
        let surface_tags = ffi::get_entity_tags(2)?;
        if surface_tags.is_empty() {
            return Err(GeometryError::OperationFailed(
                "gmsh produced no dim=2 entities after classify_surfaces+create_geometry — \
                 input surface mesh may be open or non-manifold"
                    .into(),
            ));
        }

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
        // Defend the chunks_exact zip below: if gmsh ever returns mismatched
        // buffers, surfacing the real readback-stride mismatch beats a
        // silent prefix-truncation that would later masquerade as an
        // "unknown node tag" connectivity error.
        if coord_buf.len() != node_tags.len() * 3 {
            return Err(GeometryError::OperationFailed(format!(
                "gmsh get_nodes_all stride mismatch: node_tags.len()={}, \
                 coord_buf.len()={} (expected {} = node_tags.len()*3)",
                node_tags.len(),
                coord_buf.len(),
                node_tags.len() * 3,
            )));
        }
        let (_elem_tags, elem_node_tags) = ffi::get_elements_by_type(elem_type)?;
        let nodes_per_elem: usize = match element_order {
            ElementOrderTag::P1 => 4,
            ElementOrderTag::P2 => 10,
        };
        if !elem_node_tags.len().is_multiple_of(nodes_per_elem) {
            return Err(GeometryError::OperationFailed(format!(
                "gmsh get_elements_by_type stride mismatch: elem_node_tags.len()={} \
                 is not a multiple of {nodes_per_elem} (expected {nodes_per_elem} \
                 nodes per {element_order:?} tet)",
                elem_node_tags.len(),
            )));
        }

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

        // HashMap (not BTreeMap): we never iterate `tag_to_idx` in tag order;
        // the only access is the per-element O(1) lookup below.
        let mut tag_to_idx: HashMap<u64, u32> = HashMap::with_capacity(paired.len());
        let mut vertices: Vec<f32> = Vec::with_capacity(paired.len() * 3);
        for (idx, (tag, xyz)) in paired.iter().enumerate() {
            // VolumeMesh.tet_indices is u32; if a future huge-mesh regression
            // pushes the count past 2^32, fail explicitly rather than wrap.
            let idx_u32 = u32::try_from(idx).map_err(|_| {
                GeometryError::OperationFailed(format!(
                    "mesh has {} nodes, exceeding the u32 connectivity limit \
                     of VolumeMesh.tet_indices",
                    paired.len()
                ))
            })?;
            tag_to_idx.insert(*tag, idx_u32);
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
        // starts from a known-empty state. Errors here are deliberately
        // ignored — the next call's leading `ffi::clear()?` (line ~120)
        // covers re-entry, so a hiccup during teardown shouldn't turn a
        // successfully produced VolumeMesh into a user-visible failure.
        let _ = ffi::clear();

        Ok(VolumeMesh {
            vertices,
            tet_indices,
            element_order,
            normals: None,
        })
    }

    /// Store a realized [`VolumeMesh`] and return a fresh handle that
    /// [`GeometryKernel::volume_mesh`] can later read it back through.
    ///
    /// This is the population half of the realization-read VolumeMesh
    /// projection arm (task γ). The receiver is `&self` (interior mutability
    /// via the store `Mutex`) so realize-time dispatch can call it without an
    /// exclusive borrow of the kernel.
    ///
    /// **Downstream seam (task 3429):** the production dispatch that calls this
    /// at realize-time — wiring the `Convert{from:Mesh}→VolumeMesh` repr-map
    /// arm to `mesh_to_volume` + `store_volume_mesh` — is owned by task 3429.
    /// γ provides this entry point and the matching retrieval accessor; 3429
    /// fills the store from the production path.
    pub fn store_volume_mesh(&self, vm: VolumeMesh) -> GeometryHandleId {
        // Poison recovery mirrors `mesh_to_volume`'s GMSH_LOCK handling: a
        // panic in another caller must not permanently wedge the store.
        let mut store = self
            .volume_mesh_store
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let id = GeometryHandleId(store.next_id);
        store.next_id += 1;
        store.meshes.insert(id, vm);
        id
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

    /// Read back a realized volume mesh by handle (realization-read task γ).
    ///
    /// Returns the stored clone for a handle previously minted by
    /// [`Self::store_volume_mesh`], or `Err(QueryError::InvalidHandle(handle))`
    /// for any handle this kernel never stored. Unlike the other trait methods
    /// (which uniformly error via `TRAIT_DISPATCH_MSG`), this is a real,
    /// store-backed accessor — the one trait method gmsh genuinely services.
    fn volume_mesh(&self, handle: GeometryHandleId) -> Result<VolumeMesh, QueryError> {
        let store = self
            .volume_mesh_store
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        store
            .meshes
            .get(&handle)
            .cloned()
            .ok_or(QueryError::InvalidHandle(handle))
    }
}
