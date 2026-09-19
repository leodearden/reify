//! Volume mesh refinement via Gmsh per-vertex size-field hints.
//!
//! PRD reference: `docs/prds/v0_4/a-posteriori-error-estimation.md` task #2.
//!
//! Exposes [`refine_volume_with_size_field`] with a uniform signature in both
//! `cfg(has_gmsh)` (real FFI) and `cfg(not(has_gmsh))` (stub) build modes —
//! mirrors the convention established by [`crate::mesh_profile_2d::mesh_plane_2d`].
//!
//! # Cache-invalidation contract
//!
//! Different `vertex_sizes` slices produce byte-distinct `VolumeMesh` outputs
//! (different tet counts and connectivity) so upstream cache keys — which are
//! keyed on all inputs — diverge automatically. No new cache-key field is
//! needed; the existing `volume_mesh_cache_key` derivation already covers this.
//!
//! # Global mesh-size options: inherits nothing, leaves nothing
//!
//! Gmsh's option table is process-global and survives `gmshClear()`. Since
//! task #6211 this function
//!
//! * **inbound**: sets `Mesh.MeshSizeMin`/`Mesh.MeshSizeMax` itself on every
//!   call rather than inheriting whatever a sibling entry point last left
//!   behind, so its output is a function of its own arguments alone and not of
//!   call order within the process; and
//! * **outbound**: restores that same option pair to gmsh's documented
//!   defaults before returning (via [`crate::mesh_size_clamp::MeshSizeClampReset`]),
//!   so a later *defaults-relying* call — e.g. `mesh_plane_2d` with no
//!   requested size, which deliberately writes no clamp — is not silently
//!   pinned to a fine `MeshSizeMax` left over from an adaptive-refinement
//!   iteration.
//!
//! Since task #7447 the same outbound discipline covers the size-SOURCE trio
//! this function writes — `Mesh.MeshSizeFromPoints` / `MeshSizeFromCurvature`
//! / `MeshSizeExtendFromBoundary` — via
//! [`crate::mesh_size_clamp::MeshSizeSourceReset`]. #7447 is what made that
//! mandatory rather than tidy: switching to a background size field changed
//! the `FromPoints` write from `1` to `0`, and a leaked `0` DISABLES
//! point-driven sizing for every later call in the process, where the leaked
//! `1` had merely re-asserted gmsh's own default.
//!
//! That guard now lives in [`crate::mesh_size_clamp`] rather than in this
//! file: since task #6298 it is shared infrastructure with a second consumer,
//! `kernel_real::GmshKernel::mesh_to_volume`, and one implementation cannot
//! drift from itself the way two hand-written resets could.
//!
//! Each half has its own guard in `tests/refine_volume_tests.rs`, so neither
//! can rot into a comment: inbound is
//! `uniform_size_field_refines_monotonically_under_leaked_global_clamp`
//! (assertion 2) and `non_uniform_size_field_refines_marked_region_and_caps_
//! the_rest`; outbound is
//! `refine_leaves_the_default_clamp_behind_for_a_later_defaults_relying_call`,
//! which straddles a refine with exactly the `mesh_plane_2d` call named above.
//!
//! Scope of that guarantee: it covers THIS entry point. Task #6212 stays open
//! and still owns bringing `mesh_profile_2d::mesh_plane_2d` and
//! `mesh_boundary`'s surface remesh onto the same seam, and adding the
//! `option_get_number` FFI getter that would let a restore be *as found*
//! rather than to gmsh's defaults. Only the clamp half has a behavioural
//! guard; the size-source half has no victim to observe it through, and
//! `mesh_size_clamp`'s module doc records why and what would be owed if one
//! appeared.
//!
//! # Cost basis: full remesh from surface
//!
//! `gmshModelMeshRefine()` refines uniformly across the entire existing mesh,
//! defeating localized-refinement requirements, so every call regenerates the
//! entire volume mesh from the surface boundary.
//!
//! Sizing comes from a gmsh BACKGROUND size field — a `"SS"` post view read by
//! a `PostView` mesh-size field, built by [`crate::BackgroundSizeField`]. This
//! file used to claim that per-vertex `gmshModelMeshSetSize` with
//! `Mesh.MeshSizeFromPoints=1` was "the only Gmsh path that honours localised
//! size hints". That is measurably false, and it is the belief that produced
//! task #7447's defect: `classify_surfaces` yields eight 0D entities on a box,
//! and eight corner scalars interpolate monotonically along each axis, so a
//! field with an INTERIOR minimum is unrepresentable that way. Measured on the
//! unit cube with `0.04 + 0.9*|x - 0.5|` (finest at mid-span), mean tet edge by
//! centroid band:
//!
//! | sizing mechanism           | mid-span | ends  | ratio |
//! |----------------------------|----------|-------|-------|
//! | 0D corner anchors          | 0.5399   | 0.338 | 1.60  |
//! | PostView background field  | 0.0949   | 0.312 | 0.304 |
//!
//! Corner anchoring left the mid-span COARSER than the ends rather than merely
//! unrefined; the background field tracks an analytic MathEval reference to ~1%.
//!
//! This is the explicit cost-basis the v0.4 PRD names as the trigger criterion
//! for the MMG3D bookmark (task #3003): if a refinement loop spends >30% of
//! wallclock in remeshing, swap to MMG3D.

use std::collections::HashMap;

use reify_ir::{ElementOrderTag, GeometryError, Mesh, VolumeConnectivity, VolumeMesh};

use crate::background_size_field::BackgroundSizeField;
use crate::options::MeshingOptions;

#[cfg(has_gmsh)]
use crate::mesh_size_clamp::{GMSH_MESH_SIZE_MIN_DEFAULT, MeshSizeClampReset, MeshSizeSourceReset};

/// Remesh the volume enclosed by `surface`, sized by a background size field.
///
/// `size_field` carries a target characteristic element edge length at every
/// vertex of its own sizing mesh, INTERIOR vertices included — which is the
/// point. It is installed as a gmsh background mesh size field rather than
/// projected onto the boundary, so an interior minimum survives. Build one
/// with [`BackgroundSizeField::from_tet_mesh`].
///
/// The function performs a **full remesh** from the surface boundary rather
/// than incrementally refining the current volume mesh (see module-level doc
/// for the cost/accuracy rationale).
///
/// # Threading: pinned to one worker, `options` unread
///
/// Every field of `options` is ignored. The background field forces
/// `General.NumThreads = 1` — gmsh 4.15.2 deadlocks in `mesh_generate` when it
/// evaluates a `PostView` size field from several mesher threads (the measured
/// table lives at the option write). `threads` is contractually a pure
/// performance hint excluded from the cache key, so narrowing it cannot change
/// the mesh; and one worker makes this path bit-deterministic with respect to
/// threading whether or not `deterministic` is set, which is why that flag is
/// not read either. The parameter stays in the signature because the stub arm
/// and every sibling entry point take it.
///
/// # Errors
///
/// `cfg(has_gmsh)`: returns `GeometryError::OperationFailed` on FFI failure
/// or if Gmsh produces no volume elements.
///
/// `cfg(not(has_gmsh))`: always returns `GeometryError::OperationFailed`
/// containing [`crate::STUB_UNAVAILABLE_MARKER`] — downstream callers
/// detect this via `msg.contains(STUB_UNAVAILABLE_MARKER)`.
/// Real FFI-backed remesh implementation.
///
/// Mirrors `crates/reify-kernel-gmsh/src/kernel_real.rs::mesh_to_volume` and
/// diverges from it in three places:
/// 1. After `geo_synchronize`, install `size_field` as a `"SS"` post view and
///    point a `PostView` mesh-size field at it as the background mesh.
/// 2. Turn `Mesh.MeshSizeFromPoints` OFF, so that background field is the only
///    thing deciding element size.
/// 3. Pin `General.NumThreads` to 1 instead of deriving it from `options` —
///    gmsh deadlocks evaluating the field from several mesher threads.
#[cfg(has_gmsh)]
pub fn refine_volume_with_size_field(
    surface: &Mesh,
    size_field: &BackgroundSizeField,
    _options: &MeshingOptions,
    order: ElementOrderTag,
) -> Result<VolumeMesh, GeometryError> {
    use crate::{ffi, init};

    // --- Input validation (mirrors mesh_to_volume) ---
    //
    // The size field needs none: `BackgroundSizeField::from_tet_mesh` validates
    // at construction, so an empty or non-finite field is unconstructible.
    if !surface.vertices.len().is_multiple_of(3) {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: surface.vertices.len()={} is not divisible by 3",
            surface.vertices.len()
        )));
    }
    if !surface.indices.len().is_multiple_of(3) {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: surface.indices.len()={} is not divisible by 3",
            surface.indices.len()
        )));
    }
    let n_verts = surface.vertices.len() / 3;
    if let Some(&bad) = surface.indices.iter().find(|&&i| (i as usize) >= n_verts) {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: surface.indices contains {bad}, out of bounds \
             for mesh with {n_verts} vertices"
        )));
    }
    if surface.vertices.is_empty() || surface.indices.is_empty() {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: empty surface mesh \
             (vertices.len()={}, indices.len()={})",
            surface.vertices.len(),
            surface.indices.len()
        )));
    }

    // --- Acquire lock + initialise ---
    let _guard = init::lock()?;
    init::ensure_initialized();
    ffi::clear()?;
    ffi::option_set_number("General.Terminal", 0.0)?;

    // --- Gmsh options (mirrors mesh_to_volume, except the thread pin) ---
    //
    // SINGLE-THREADED, UNCONDITIONALLY. Unlike `mesh_to_volume`, this path
    // ignores `options.threads` and `options.deterministic` and always asks
    // gmsh for one worker, because gmsh 4.15.2 DEADLOCKS in
    // `gmshModelMeshGenerate(3)` when a `PostView` background size field —
    // installed unconditionally a few lines below — is evaluated from several
    // mesher threads at once. Measured on `tests/mesher_poison_recovery.rs`,
    // varying nothing but this value:
    //
    // | General.NumThreads | outcome                   |
    // |--------------------|---------------------------|
    // | 1                  | 5 passed, 17.58 s         |
    // | 2                  | 5 passed, 24.37 s         |
    // | 8                  | hang, SIGKILLed at 100 s  |
    // | 32 (= nproc)       | hang, SIGKILLed at 100 s  |
    //
    // A block, not slowness: at 8+ the process sat with CPU time frozen at
    // 00:00:40 across 2.5 minutes of wall clock, RSS flat at 33 MB, and all 35
    // threads in `futex_do_wait`. It reproduces for a valid unit cube, so it is
    // not confined to the mesher's failure path. `Mesh.MaxNumThreads3D = 1`
    // with `General.NumThreads = 32` still hangs, so pinning the 3D stage alone
    // is not a fix; only the global worker count is.
    //
    // The threshold above is that binary's, and it does NOT generalise — which
    // is why this is a pin at 1 and not a cap at some measured ceiling. The
    // same sweep on a CLEAN process (`tests/refine_volume_tests.rs`, unit cube,
    // uniform field) passes at 8 and hangs at 16, 24 and 32. `mesher_poison_
    // recovery` hangs at 8 because its gmsh has already been through this
    // module's `finalize`/`initialize` recovery cycle. Two fixtures, two
    // different safe ceilings; 1 is the only value measured safe on both.
    //
    // This costs the caller nothing it was promised. `MeshingOptions::threads`
    // is documented as a pure performance hint that is deliberately excluded
    // from the cache key because it cannot change the answer, and `None` hands
    // the decision to the kernel outright — so clamping it narrows performance,
    // never output. The bonus: with one worker this path is bit-deterministic
    // with respect to threading whatever `options.deterministic` says, which is
    // why that flag is no longer read here at all.
    ffi::option_set_number("General.NumThreads", 1.0)?;
    let element_order_value: f64 = match order {
        ElementOrderTag::P1 => 1.0,
        ElementOrderTag::P2 => 2.0,
    };
    ffi::option_set_number("Mesh.ElementOrder", element_order_value)?;
    ffi::option_set_number("Mesh.Algorithm3D", 10.0)?;

    // --- Add discrete surface entity and push surface mesh ---
    ffi::model_add("reify_refine_volume")?;
    let surf_tag = ffi::add_discrete_entity(2, &[])?;

    let node_tags: Vec<u64> = (1..=n_verts as u64).collect();
    let coords_f64: Vec<f64> = surface.vertices.iter().map(|&v| v as f64).collect();
    ffi::add_nodes_2d(surf_tag, &node_tags, &coords_f64)?;

    let n_tris = surface.indices.len() / 3;
    let tri_tags: Vec<u64> = (1..=n_tris as u64).collect();
    let tri_node_tags: Vec<u64> = surface.indices.iter().map(|&i| i as u64 + 1).collect();
    ffi::add_elements_2d(surf_tag, 2, &tri_tags, &tri_node_tags)?;

    // --- Classify and create geometry ---
    //
    // Use a tighter dihedral-angle threshold (PI/12 ≈ 15°) than
    // `mesh_to_volume`'s `CLASSIFY_FEATURE_ANGLE` (PI/4) so that virtually
    // every mesh edge is treated as a "hard" edge.  For the unit-cube test
    // geometry (90° dihedral angles at each edge), this ensures all 12 edges
    // become 1D curve entities and all 8 cube-corner vertices become 0D point
    // entities.  A PI/2 threshold would emit no corner entities at all (gmsh's
    // sharp-edge test is strictly-greater-than and 90° is NOT > PI/2), leaving
    // nowhere to attach per-vertex size hints; that is also what broke
    // `mesh_to_volume` in #6200, which is why it no longer uses PI/2 either.
    // PI/12 stays deliberately sharper than PI/4 (this path wants every edge
    // hard, not just the feature edges), so it is NOT folded into the shared
    // constant.
    //
    // For the `curveAngle` (4th argument) we use the same PI/12 so that
    // vertices at intersections of curves separated by < 15° are still
    // classified as hard corners; this keeps the corner count stable across
    // test geometries.
    ffi::classify_surfaces(
        std::f64::consts::PI / 12.0,
        1,
        1,
        std::f64::consts::PI / 12.0,
        0,
    )?;
    ffi::create_geometry(&[])?;

    let surface_tags = ffi::get_entity_tags(2)?;
    if surface_tags.is_empty() {
        return Err(GeometryError::OperationFailed(
            "refine_volume_with_size_field: no dim=2 entities after classify+create_geometry; \
             surface may be open or non-manifold"
                .into(),
        ));
    }

    let loop_tag = ffi::geo_add_surface_loop(&surface_tags)?;
    let _vol_tag = ffi::geo_add_volume(&[loop_tag])?;
    ffi::geo_synchronize()?;

    // --- Background size field ---
    //
    // The whole size field, interior included. Taken once, used by all three
    // guards below, so the `clamp_reset_witness` accessor keeps a single call
    // site here (see its doc in `init.rs`).
    let witness = _guard.clamp_reset_witness();
    let _background_field = BackgroundFieldGuard::install(witness, size_field)?;

    // --- Size sources: the background field and nothing else ---
    //
    // `Mesh.MeshSizeFromPoints=0`: the background field decides size, so any 0D
    // entity `classify_surfaces` happened to create must not compete with it.
    //
    // `Mesh.MeshSizeFromCurvature=0`: no curvature-driven refinement, so gmsh
    // does not independently insert small elements where the surface curves.
    //
    // `Mesh.MeshSizeExtendFromBoundary=0`: do NOT propagate the gradient of the
    // 2D boundary mesh sizes into the 3D volume. With it on (gmsh's default) a
    // fine patch on one face extends its fineness deep into the interior,
    // overriding what the background field asks for there.
    //
    // All three are process-global and survive `gmshClear()`. `FromPoints = 0`
    // deviates from gmsh's default of 1 in the DANGEROUS direction — it
    // disables point-driven sizing for every later call in the process, where
    // the `1` this function used to write merely re-asserted the default.
    // `MeshSizeSourceReset` closes that outbound direction on every exit path,
    // early `?`-returns included, so the trio is returned to gmsh's defaults
    // and nothing downstream inherits this call's size sources (task #7447).
    let _size_source_reset = MeshSizeSourceReset::armed(witness);
    ffi::option_set_number("Mesh.MeshSizeFromPoints", 0.0)?;
    ffi::option_set_number("Mesh.MeshSizeFromCurvature", 0.0)?;
    ffi::option_set_number("Mesh.MeshSizeExtendFromBoundary", 0.0)?;

    // --- Mesh-size clamp: set explicitly, never inherited (task #6211) ---
    //
    // INVARIANT: `size_field` alone decides element size here. Gmsh's option
    // table is process-global and is NOT reset by `gmshClear()`, and the
    // sibling entry points `mesh_profile_2d::mesh_plane_2d` and
    // `mesh_boundary`'s surface remesh still write
    // `Mesh.MeshSizeMin`/`MeshSizeMax` without restoring them. Without the two
    // writes below, either of those running earlier in the process pins every
    // element of THIS remesh to ITS size and the per-vertex field becomes
    // inert (task #6211: one identical tet count for every hint).
    //
    // `kernel_real::GmshKernel::mesh_to_volume` used to belong on that list and
    // no longer does — since task #6298 it arms the same
    // `mesh_size_clamp::MeshSizeClampReset` on entry. These two writes stay
    // load-bearing regardless: the other two entry points are still open, and
    // an inbound clamp that depends on no sibling's outbound discipline is the
    // only form that makes this function's output a pure function of its own
    // arguments.
    //
    // Both writes are load-bearing, not belt-and-braces: with a leaked
    // Min == Max, lowering only Max leaves Min > Max (gmsh still floors at the
    // leaked value) and lowering only Min leaves the leaked Max capping
    // everything.
    //
    // Min = gmsh's default: no floor, so the finest hint is honoured.
    // Deliberately not the field's finest value, which would forbid gmsh from
    // going finer than the finest hint anywhere in the domain — a new, untested
    // constraint on the localized-refinement path for no measured benefit.
    //
    // Max = the COARSEST requested hint, because with
    // `Mesh.MeshSizeExtendFromBoundary = 0` (set above) the 3D mesher is
    // otherwise free to grow interior elements arbitrarily coarser than
    // anything the caller asked for. Deliberately not `options.mesh_size`:
    // that is the baseline target the per-vertex field exists to supersede, and
    // feeding it back in would re-create the very clamp this defends against.
    //
    // No degenerate-input fallback is needed: `BackgroundSizeField` validates
    // every emitted size at construction, so `max_size()` is finite and
    // positive by construction.
    //
    // `MeshSizeClampReset` closes the outbound direction: this pair is returned
    // to gmsh's defaults on every exit path, so the same leak does not run from
    // here into a later defaults-relying call.
    let _clamp_reset = MeshSizeClampReset::armed(witness);
    ffi::option_set_number("Mesh.MeshSizeMin", GMSH_MESH_SIZE_MIN_DEFAULT)?;
    ffi::option_set_number("Mesh.MeshSizeMax", size_field.max_size())?;

    // --- Tet meshing ---
    // Via `init::mesh_generate_with_recovery`: the mesher is process-global, so
    // a failure here must not outlive this call. See that function.
    init::mesh_generate_with_recovery(&_guard, 3)?;

    // --- Readback (mirrors mesh_to_volume verbatim) ---
    let (out_node_tags, coord_buf) = ffi::get_nodes_all()?;
    if coord_buf.len() != out_node_tags.len() * 3 {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: get_nodes_all stride mismatch: \
             node_tags.len()={}, coord_buf.len()={} (expected {})",
            out_node_tags.len(),
            coord_buf.len(),
            out_node_tags.len() * 3,
        )));
    }
    let elem_node_tags = init::read_tet_connectivity("refine_volume_with_size_field", order)?;

    let mut paired: Vec<(u64, [f64; 3])> = out_node_tags
        .iter()
        .copied()
        .zip(coord_buf.chunks_exact(3))
        .map(|(t, c)| (t, [c[0], c[1], c[2]]))
        .collect();
    paired.sort_by_key(|(t, _)| *t);

    let mut tag_to_idx: HashMap<u64, u32> = HashMap::with_capacity(paired.len());
    let mut vertices: Vec<f32> = Vec::with_capacity(paired.len() * 3);
    for (idx, (tag, xyz)) in paired.iter().enumerate() {
        let idx_u32 = u32::try_from(idx).map_err(|_| {
            GeometryError::OperationFailed(format!(
                "refine_volume_with_size_field: {} nodes exceeds u32 tet_indices limit",
                paired.len()
            ))
        })?;
        tag_to_idx.insert(*tag, idx_u32);
        vertices.extend(xyz.iter().map(|&v| v as f32));
    }

    let mut tet_indices: Vec<u32> = Vec::with_capacity(elem_node_tags.len());
    for &tag in &elem_node_tags {
        let idx = *tag_to_idx.get(&tag).ok_or_else(|| {
            GeometryError::OperationFailed(format!(
                "refine_volume_with_size_field: element references unknown node tag {tag}"
            ))
        })?;
        tet_indices.push(idx);
    }

    let _ = ffi::clear();

    Ok(VolumeMesh {
        vertices,
        connectivity: VolumeConnectivity::Tet {
            indices: tet_indices,
            order,
        },
        normals: None,
        boundary: None,
    })
}

/// Stub-build companion: always returns `GeometryError::OperationFailed`
/// containing [`crate::STUB_UNAVAILABLE_MARKER`].
#[cfg(not(has_gmsh))]
pub fn refine_volume_with_size_field(
    _surface: &Mesh,
    _size_field: &BackgroundSizeField,
    _options: &MeshingOptions,
    _order: ElementOrderTag,
) -> Result<VolumeMesh, GeometryError> {
    Err(GeometryError::OperationFailed(format!(
        "refine_volume_with_size_field: {} in this build \
         (libgmsh not detected at build time)",
        crate::STUB_UNAVAILABLE_MARKER,
    )))
}

/// RAII removal of the post view + `PostView` mesh-size field that
/// [`refine_volume_with_size_field`] installs.
///
/// Both are PROCESS-GLOBAL and survive `gmshClear()`. The trailing
/// `ffi::clear()` on the success path takes the view down, but every early
/// `?` return between the install and that line would otherwise leak one into
/// every later mesh in the process — the same defect class as task #6211.
///
/// Borrows the `GMSH_LOCK` guard for its lifetime on the same reasoning as
/// [`MeshSizeClampReset`]: the FFI calls in `drop` mutate process-global gmsh
/// state, so the type is unconstructible without a live lock guard in hand and
/// dropck forces the removal to land before that lock is released.
#[cfg(has_gmsh)]
struct BackgroundFieldGuard<'g> {
    view_tag: i32,
    /// `None` until `FieldAdd` succeeds, so a failure between the view and the
    /// field still tears the view down and never removes a field-tag gmsh
    /// never assigned.
    field_tag: Option<i32>,
    _lock: std::marker::PhantomData<&'g std::sync::MutexGuard<'g, ()>>,
}

#[cfg(has_gmsh)]
impl<'g> BackgroundFieldGuard<'g> {
    /// Install `size_field` as the model's background mesh size field.
    ///
    /// Armed before the first fallible step that needs cleaning up, so the
    /// `?`s below unwind through this type's own `Drop` rather than through a
    /// hand-written error path that could forget one.
    fn install(
        guard: &'g std::sync::MutexGuard<'g, ()>,
        size_field: &BackgroundSizeField,
    ) -> Result<Self, GeometryError> {
        use crate::ffi;

        let _ = guard;
        let view_tag = ffi::view_add("reify_refine_bgm")?;
        let mut installed = Self {
            view_tag,
            field_tag: None,
            _lock: std::marker::PhantomData,
        };

        ffi::view_add_list_data(
            view_tag,
            "SS",
            size_field.element_count(),
            size_field.list_data(),
        )?;

        let field_tag = ffi::field_add("PostView")?;
        installed.field_tag = Some(field_tag);
        // "ViewTag", never "ViewIndex": the latter is a position in the
        // currently-loaded view list and silently selects a different view as
        // views are removed.
        ffi::field_set_number(field_tag, "ViewTag", f64::from(view_tag))?;
        ffi::field_set_as_background_mesh(field_tag)?;

        Ok(installed)
    }
}

#[cfg(has_gmsh)]
impl Drop for BackgroundFieldGuard<'_> {
    fn drop(&mut self) {
        // Best-effort, like `MeshSizeClampReset` and the trailing
        // `ffi::clear()`: a failure here cannot be reported from `drop` and
        // must not mask the real result.
        if let Some(field_tag) = self.field_tag {
            let _ = crate::ffi::field_remove(field_tag);
        }
        let _ = crate::ffi::view_remove(self.view_tag);
    }
}
