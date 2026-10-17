//! The region-building and tet-readback span shared by the three tet producers
//! ([`GmshKernel::mesh_to_volume`](crate::kernel_real::GmshKernel::mesh_to_volume),
//! [`crate::refine_volume::refine_volume_with_size_field`] and the attributed
//! `mesh_boundary` producer).
//!
//! Sits below all three: it reaches gmsh through [`crate::ffi`] and validates the
//! element buffer through [`crate::tet_readback`], and imports nothing from the
//! producers or from the `GmshKernel` trait-impl file.
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.

use std::collections::HashMap;

use reify_ir::{ElementOrderTag, GeometryError, VolumeConnectivity, VolumeMesh};

use crate::ffi;
use crate::init;
use crate::tet_readback;

/// Dihedral-angle threshold (radians) above which `classify_surfaces` treats a
/// mesh edge as a sharp feature separating two B-rep surfaces.
///
/// **Must stay strictly below π/2** (#6200). gmsh's sharp-edge test is
/// strictly-greater-than and a box's dihedral angle is exactly π/2, so a π/2
/// threshold registers none of a box's own edges, and HXT fills a B-rep region
/// smaller than the box even though the shell is closed and watertight. The
/// measured failure lives with its two guards: `tests/classify_feature_angle.rs`
/// (the mechanism: gmsh's B-rep census and log) and
/// `tests/volume_fill_fraction.rs` (the symptom: fill fraction).
///
/// FRAC_PI_4 (45°) is the value the sibling attributed producer in
/// `mesh_boundary.rs` has shipped against real OCCT geometry all along, rather
/// than a fourth distinct magic angle; that producer now classifies through the
/// same region builder at this same constant, so the crate holds exactly one
/// definition.
///
/// Exported so `tests/classify_feature_angle.rs` can census the B-rep
/// decomposition against the PRODUCTION value instead of a copied literal that
/// could silently drift away from it.
pub const CLASSIFY_FEATURE_ANGLE: f64 = std::f64::consts::FRAC_PI_4;

/// Bend-angle threshold (radians) above which curve–curve junctions are emitted
/// as B-rep vertex entities during `classify_surfaces`.
///
/// Sharper than gmsh's π default so 90°-junction corners are recognised.
/// Exported alongside [`CLASSIFY_FEATURE_ANGLE`] for the same reason.
pub const CLASSIFY_CURVE_ANGLE: f64 = std::f64::consts::FRAC_PI_4;

/// The two `classify_surfaces` thresholds (radians) a producer hands
/// [`build_meshable_region`]: named fields, so the pair cannot be swapped at a
/// call site the way two positional `f64`s can.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClassifyAngles {
    /// Dihedral angle above which a mesh edge separates two B-rep surfaces.
    /// Must stay strictly below π/2 (#6200; see [`CLASSIFY_FEATURE_ANGLE`]).
    pub(crate) feature: f64,
    /// Bend angle above which a curve–curve junction becomes a B-rep vertex.
    pub(crate) curve: f64,
}

/// The angles `mesh_to_volume` and the attributed producer classify at.
pub(crate) const FEATURE_EDGE_CLASSIFY_ANGLES: ClassifyAngles = ClassifyAngles {
    feature: CLASSIFY_FEATURE_ANGLE,
    curve: CLASSIFY_CURVE_ANGLE,
};

/// Turn the discrete surface already pushed into gmsh's model into a closed
/// B-rep region that HXT can fill, classifying it at `angles`.
///
/// Carries no mesh data in or out: it communicates only through gmsh's
/// process-global model state, which the caller has already populated with the
/// surface nodes and triangles. The [`init::GmshGuard`] is that state's
/// admission ticket — never touched, taken so "the caller holds
/// [`init::GMSH_LOCK`]" is a precondition the compiler checks rather than a
/// comment a later caller can skip reading. On error it leaves the partial
/// model in place: every entry point opens with `ffi::clear()?` under that
/// lock, so no later call inherits it.
///
/// Split out of `GmshKernel::mesh_to_volume` so this span — six `?` sites,
/// each of whose failures gmsh explains in its captured message stream rather
/// than in the last-error line — folds that stream into its error at ONE seam
/// instead of six. All three tet producers build their region here;
/// `entry_point` names the caller in the one error this function raises
/// itself.
pub(crate) fn build_meshable_region(
    _guard: &init::GmshGuard,
    entry_point: &str,
    angles: ClassifyAngles,
) -> Result<(), GeometryError> {
    // Reclassify the discrete surface and build geometry so 3D meshing has
    // a parametric region to fill. `angles.feature` must stay below π/2;
    // why is at `CLASSIFY_FEATURE_ANGLE` (#6200).
    //
    // Note: this affects only the B-rep classification. `create_geometry`
    // and `mesh_generate(3)` below re-mesh from the resulting parametric
    // geometry, so output node identities are gmsh's choice and do NOT
    // preserve the input discrete vertex set — confirmed by the
    // diagnostic test in `tests/gmsh_classify_diagnostics.rs`. See task
    // 3591 for the broader NodeAttachment-producer redesign that
    // implication motivates.
    ffi::classify_surfaces(angles.feature, 1, 1, angles.curve, 0)?;
    ffi::create_geometry(&[])?;

    // After classify+createGeometry, gmsh creates new geometric surface
    // entities whose tags supersede the original discrete-entity
    // `surf_tag`; query them so `geo_add_surface_loop` references the
    // correct entities. (`surf_tag` is the discrete-mesh entity tag and
    // is no longer referenced from this point on.)
    let surface_tags = ffi::get_entity_tags(2)?;
    if surface_tags.is_empty() {
        return Err(GeometryError::OperationFailed(format!(
            "{entry_point}: no dim=2 entities after classify+create_geometry; \
             surface may be open or non-manifold"
        )));
    }

    // Wrap the reclassified surface(s) in a surface loop and a volume
    // so HXT has a closed region to mesh.
    let loop_tag = ffi::geo_add_surface_loop(&surface_tags)?;
    let _vol_tag = ffi::geo_add_volume(&[loop_tag])?;
    ffi::geo_synchronize()?;
    Ok(())
}

/// A tet mesh read back out of gmsh's model by [`read_back_tet_mesh`].
pub(crate) struct TetReadback {
    pub(crate) mesh: VolumeMesh,
    /// gmsh node tag → that node's 0-based index in `mesh`. The attributed
    /// producer translates its entity-membership queries through it.
    #[cfg_attr(
        not(feature = "mesh-morph"),
        expect(dead_code, reason = "only reader is the mesh-morph-gated mesh_boundary")
    )]
    pub(crate) tag_to_idx: HashMap<u64, u32>,
}

/// Read the tets gmsh just generated back out of the process-global model and
/// remap them onto a [`VolumeMesh`]'s 0-based local indices.
///
/// Companion to [`build_meshable_region`]: same lock ticket, same model, same
/// error-path contract, and split out for the same reason. Every failure here
/// is gmsh handing back something inconsistent with the mesh it had just
/// reported building — precisely the failure whose explanation is in the
/// captured stream and not in the last-error line — so the caller folds this
/// span into that stream at its own single seam. `entry_point` prefixes every
/// error, as it does in [`tet_readback::read_tet_connectivity`].
pub(crate) fn read_back_tet_mesh(
    _guard: &init::GmshGuard,
    entry_point: &str,
    element_order: ElementOrderTag,
) -> Result<TetReadback, GeometryError> {
    let (node_tags, coord_buf) = ffi::get_nodes_all()?;
    // Defend the chunks_exact zip below: if gmsh ever returns mismatched
    // buffers, surfacing the real readback-stride mismatch beats a
    // silent prefix-truncation that would later masquerade as an
    // "unknown node tag" connectivity error.
    if coord_buf.len() != node_tags.len() * 3 {
        return Err(GeometryError::OperationFailed(format!(
            "{entry_point}: gmsh get_nodes_all stride mismatch: node_tags.len()={}, \
             coord_buf.len()={} (expected {} = node_tags.len()*3)",
            node_tags.len(),
            coord_buf.len(),
            node_tags.len() * 3,
        )));
    }
    let elem_node_tags = tet_readback::read_tet_connectivity(entry_point, element_order)?;

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

    // HashMap (not BTreeMap): nothing iterates `tag_to_idx` in tag order;
    // every access is an O(1) per-tag lookup.
    let mut tag_to_idx: HashMap<u64, u32> = HashMap::with_capacity(paired.len());
    let mut vertices: Vec<f32> = Vec::with_capacity(paired.len() * 3);
    for (idx, (tag, xyz)) in paired.iter().enumerate() {
        // VolumeMesh.tet_indices is u32; if a future huge-mesh regression
        // pushes the count past 2^32, fail explicitly rather than wrap.
        let idx_u32 = u32::try_from(idx).map_err(|_| {
            GeometryError::OperationFailed(format!(
                "{entry_point}: mesh has {} nodes, exceeding the u32 connectivity limit \
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
                "{entry_point}: gmsh element references unknown node tag {tag} \
                 (mesh corruption?)"
            ))
        })?;
        tet_indices.push(idx);
    }
    Ok(TetReadback {
        mesh: VolumeMesh {
            vertices,
            connectivity: VolumeConnectivity::Tet {
                indices: tet_indices,
                order: element_order,
            },
            normals: None,
            boundary: None,
        },
        tag_to_idx,
    })
}
