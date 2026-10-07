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
//! Different size fields produce byte-distinct `VolumeMesh` outputs
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
//! * **outbound**: restores every size option to gmsh's documented defaults
//!   before returning (via [`crate::mesh_size_scope::MeshSizeScope`]),
//!   so a later *defaults-relying* call — e.g. `mesh_plane_2d` with no
//!   requested size, which deliberately writes no clamp — is not silently
//!   pinned to a fine `MeshSizeMax` left over from an adaptive-refinement
//!   iteration.
//!
//! That guard now lives in [`crate::mesh_size_scope`] rather than in this
//! file: since task #6298 it is shared infrastructure with a second consumer,
//! `kernel_real::GmshKernel::mesh_to_volume`, and one implementation cannot
//! drift from itself the way two hand-written resets could.
//!
//! Each half has its own guard in `tests/refine_volume_tests.rs`, so neither
//! can rot into a comment: inbound is
//! `uniform_size_field_refines_monotonically_under_leaked_global_clamp`
//! (assertion 2) and `non_uniform_size_field_refines_marked_region_and_does_
//! not_coarsen_the_rest`; outbound is
//! `refine_leaves_the_default_clamp_behind_for_a_later_defaults_relying_call`,
//! which straddles a refine with exactly the `mesh_plane_2d` call named above.
//!
//! Scope of that guarantee: since task #6968 it covers all five size options,
//! not just the `MeshSizeMin`/`MeshSizeMax` pair. The
//! `Mesh.MeshSizeFromPoints` / `MeshSizeFromCurvature` /
//! `MeshSizeExtendFromBoundary` writes below used to be left behind for a
//! later caller to inherit; `MeshSizeScope` now restores them too. Two of the
//! three deviate from gmsh 4.15.2's measured defaults.
//! `MeshSizeExtendFromBoundary` — written `0` here against a default of `1` —
//! was the only one before task #7447, and it deviated far enough to change a
//! later caller's mesh, which is why the leak was invisible for so long.
//! `MeshSizeFromPoints` joined it when #7447 moved sizing onto a background
//! field: it is now written `0` against a default of `1`, and a leaked `0`
//! would DISABLE point-driven sizing for every later call in the process,
//! where the `1` this function used to write merely re-asserted the default.
//! See the inline rationale at the option writes below, and
//! [`crate::mesh_size_scope`] for which guard holds this writer; that map is
//! kept in one place rather than restated per writer.
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
use crate::log_capture::LogCapture;
#[cfg(has_gmsh)]
use crate::mesh_size_scope::{GMSH_MESH_SIZE_MIN_DEFAULT, MeshSizeScope};

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
/// # Threading
///
/// `options.threads` and `options.deterministic` are honoured through
/// [`MeshingOptions::resolved_num_threads`], as in the sibling entry points.
/// `options.mesh_size` is deliberately unread; see the `Mesh.MeshSizeMax` write.
///
/// # Errors
///
/// `cfg(has_gmsh)`: returns `GeometryError::OperationFailed` on FFI failure
/// or if Gmsh produces no volume elements. A failure inside the capture window
/// also carries the tail of gmsh's captured Info/Warning stream (see
/// [`crate::log_capture`]).
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
/// 3. Pre-build the view's lookup octree before meshing (see
///    `BackgroundFieldGuard::install`).
#[cfg(has_gmsh)]
pub fn refine_volume_with_size_field(
    surface: &Mesh,
    size_field: &BackgroundSizeField,
    options: &MeshingOptions,
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
    // Declared after `_guard` so it drops first (Rust drops locals in reverse
    // declaration order): its restore writes land while GMSH_LOCK is still
    // held. Hoisted above the first `?` below so every early return is
    // covered, not only the success path — see `mesh_size_scope`.
    let _size_scope = MeshSizeScope::entered(_guard.size_scope_witness())?;
    ffi::clear()?;
    ffi::option_set_number("General.Terminal", 0.0)?;
    // gmsh's diagnosis now reaches only the capture; armed and dropped exactly
    // as at `kernel_real::GmshKernel::mesh_to_volume`'s arm site (`log_capture`).
    // Declared before `_background_field`, so that guard's teardown — which
    // logs into the capture after a recovery recycle — is drained here too.
    let log_capture = LogCapture::armed(&_guard);

    // --- Gmsh options (mirrors mesh_to_volume) ---
    //
    // Threads resolve exactly as in the sibling entry points. Safe only because
    // `BackgroundFieldGuard::install` pre-builds the view's lookup octree; see
    // docs/notes/gmsh-postview-background-field-threading.md.
    ffi::option_set_number(
        "General.NumThreads",
        f64::from(options.resolved_num_threads()),
    )?;
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

    build_refine_region(&_guard).map_err(|e| log_capture.annotate(e))?;

    // --- Background size field ---
    //
    // The whole size field, interior included. Declared after `_size_scope`,
    // so the field is torn down before the size options are restored, and
    // both land before `_guard` releases the lock.
    let _background_field = BackgroundFieldGuard::install(_guard.size_scope_witness(), size_field)
        .map_err(|e| log_capture.annotate(e))?;

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
    // All three are written unconditionally and independently of
    // `mesh_size_scope::GMSH_SIZE_OPTION_DEFAULTS`: each is a REQUIREMENT of
    // the background field, so this function states it rather than inheriting
    // it from a default that is gmsh's to change. `FromCurvature = 0` happens
    // to coincide with today's default; see "no test can tell" below for what
    // that costs. `FromPoints = 0` and `ExtendFromBoundary = 0` deviate from
    // it, and `_size_scope` (armed above) returns all three to gmsh's defaults
    // on every exit path, early `?`-returns included, so nothing downstream
    // inherits this call's size sources.
    ffi::option_set_number("Mesh.MeshSizeFromPoints", 0.0)?;
    ffi::option_set_number("Mesh.MeshSizeFromCurvature", 0.0)?;
    ffi::option_set_number("Mesh.MeshSizeExtendFromBoundary", 0.0)?;

    // --- Mesh-size clamp: set explicitly, never inherited (task #6211) ---
    //
    // INVARIANT: `size_field` alone decides element size here. Gmsh's option
    // table is process-global and is NOT reset by `gmshClear()`, so a sibling
    // entry point that wrote `Mesh.MeshSizeMin`/`MeshSizeMax` and never
    // restored them used to pin every element of THIS remesh to ITS size,
    // leaving the per-vertex field inert (task #6211: one identical tet count
    // for every hint). Since task #6968 every entry point in this crate enters
    // a `mesh_size_scope::MeshSizeScope`, so the table these writes land on
    // holds gmsh's defaults whatever ran earlier in the process.
    //
    // NO TEST CAN TELL whether a write whose value coincides with gmsh's
    // current default is present: while the scope is armed it is a behavioural
    // no-op, so deleting it leaves `tests/` green. Stated here rather than
    // left for a future author to discover by deleting one and finding the
    // suite still green. They stay because they are this function's
    // requirements, and specifically so `MeshSizeMin` and `MeshSizeMax` read
    // as ONE clamp rather than half of one: written as a
    // pair against a hostile Min == Max, lowering only Max would leave
    // Min > Max (gmsh still floors at the leaked value) and lowering only Min
    // would leave the leaked Max capping everything. That is the shape an
    // inbound clamp needs if it is ever to stand without the scope beneath it.
    //
    // Min = gmsh's default: no floor, so the finest hint is honoured.
    // Deliberately not the field's finest value, which would forbid gmsh from
    // going finer than the finest hint anywhere in the domain — a new, untested
    // constraint on the localized-refinement path for no measured benefit.
    //
    // Max = the COARSEST requested hint. It was written for the 0D corner-anchor
    // path, where nothing sized the interior and, with
    // `Mesh.MeshSizeExtendFromBoundary = 0`, the 3D mesher was free to grow
    // interior elements arbitrarily coarser than anything the caller asked for.
    // Under the background field the cap CANNOT BIND, measured (task #7447,
    // libgmsh 4.15.2). Inside the sizing mesh the field interpolates vertex
    // sizes that are all `<= max_size()`. Outside it, gmsh's `PostView` field
    // takes the NEAREST node's size (its `UseClosest` option, left at gmsh's
    // default) instead of falling back to an unsized value.
    // `tests/refine_volume_tests.rs::the_region_outside_a_partial_sizing_mesh_takes_the_nearest_hint_not_the_cap`
    // pins the outside half. Forcing the cap to gmsh's default leaves both of
    // that file's non-uniform fixtures bit-identical, and every test in
    // reify-kernel-gmsh and in reify-solver-elastic's refinement suites green.
    //
    // It is kept anyway because removing it is still a behaviour change: on a
    // UNIFORM field it moves the mesh by a few tets (17484 -> 17412 for 0.25 on
    // `[0,4]^3`). Every uniform-field seed mesh in the solver-elastic
    // calibrations was produced with it, and their recorded figures move with
    // it (the L-shaped gate's localization ratio reads 5.33 capped, 5.24
    // uncapped). Deleting the write means re-measuring those figures, which is
    // a change of its own, not a side effect of this one.
    //
    // Deliberately not `options.mesh_size`: that is the baseline target the
    // per-vertex field exists to supersede, and feeding it back in would
    // re-create the very clamp this defends against.
    //
    // No degenerate-input fallback is needed: `BackgroundSizeField` validates
    // every emitted size at construction, so `max_size()` is finite and
    // positive by construction.
    //
    // `MeshSizeScope` closes the outbound direction: every size option — this
    // pair and the three set above — is returned to gmsh's defaults on every
    // exit path, so the same leak does not run from here into a later
    // defaults-relying call.
    ffi::option_set_number("Mesh.MeshSizeMin", GMSH_MESH_SIZE_MIN_DEFAULT)?;
    ffi::option_set_number("Mesh.MeshSizeMax", size_field.max_size())?;

    // --- Tet meshing ---
    // Via `init::mesh_generate_with_recovery`: the mesher is process-global, so
    // a failure here must not outlive this call. See that function.
    //
    // Outside every `log_capture` seam: it annotates its own failure, so a seam
    // over it would append the tail twice — pinned by
    // `mesher_poison_recovery::a_failed_refine_reports_gmshs_captured_log_not_just_the_last_error`.
    init::mesh_generate_with_recovery(&_guard, 3)?;

    let volume = read_back_refined_volume(&_guard, order).map_err(|e| log_capture.annotate(e))?;
    let _ = ffi::clear();
    Ok(volume)
}

/// Turn the discrete surface already pushed into gmsh's model into a closed
/// B-rep region that HXT can fill — refine's own span, classified at π/12.
///
/// The [`crate::init::GmshGuard`] is an admission ticket only, as in
/// `kernel_real::build_meshable_region`. That helper is not reused here because
/// the classification angles differ by design (see the comment below). Split
/// out of [`refine_volume_with_size_field`] so this span's failures reach the
/// caller's `LogCapture` at one seam.
#[cfg(has_gmsh)]
fn build_refine_region(_guard: &crate::init::GmshGuard) -> Result<(), GeometryError> {
    use crate::ffi;

    // --- Classify and create geometry ---
    //
    // Use a tighter dihedral-angle threshold (PI/12 ≈ 15°) than
    // `mesh_to_volume`'s `CLASSIFY_FEATURE_ANGLE` (PI/4) so that virtually
    // every mesh edge is treated as a "hard" edge.  For the unit-cube test
    // geometry (90° dihedral angles at each edge), this ensures all 12 edges
    // become 1D curve entities and all 8 cube-corner vertices become 0D point
    // entities.  A PI/2 threshold would emit no corner entities at all (gmsh's
    // sharp-edge test is strictly-greater-than and 90° is NOT > PI/2); that
    // is what broke `mesh_to_volume` in #6200, which is why it no longer uses
    // PI/2 either.  Before #7447 this path also attached its size hints to
    // those corner entities; since #7447 sizing comes from the background
    // field the caller installs next, and `Mesh.MeshSizeFromPoints=0` keeps the
    // corners out of it.
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
    Ok(())
}

/// Read the tets gmsh just generated back out of the process-global model and
/// remap them onto a [`VolumeMesh`]'s 0-based local indices.
///
/// Same lock ticket as [`build_refine_region`], and split out of
/// [`refine_volume_with_size_field`] for the same reason: one `LogCapture` seam
/// for the whole readback.
#[cfg(has_gmsh)]
fn read_back_refined_volume(
    _guard: &crate::init::GmshGuard,
    order: ElementOrderTag,
) -> Result<VolumeMesh, GeometryError> {
    use crate::{ffi, init};

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
/// [`MeshSizeScope`]: the FFI calls in `drop` mutate process-global gmsh
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

/// Where `BackgroundFieldGuard::install` probes its view. Any point works: a
/// hit and a miss both build the view's lookup octree.
#[cfg(has_gmsh)]
const OCTREE_BUILD_PROBE_POINT: [f64; 3] = [0.0; 3];

#[cfg(has_gmsh)]
impl<'g> BackgroundFieldGuard<'g> {
    /// Install `size_field` as the model's background mesh size field, ready
    /// to be evaluated concurrently by the multi-threaded mesher.
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
        // gmsh 4.15.2 builds a view's lookup octree lazily behind an orphaned
        // OpenMP barrier that deadlocks when first reached from inside the
        // multi-threaded mesher. Probing once here, on the calling thread,
        // builds it outside any parallel region; see
        // docs/notes/gmsh-postview-background-field-threading.md.
        ffi::view_probe(view_tag, OCTREE_BUILD_PROBE_POINT)?;

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
        // Best-effort, like `MeshSizeScope`'s restore and the trailing
        // `ffi::clear()`: a failure here cannot be reported from `drop` and
        // must not mask the real result.
        if let Some(field_tag) = self.field_tag {
            let _ = crate::ffi::field_remove(field_tag);
        }
        let _ = crate::ffi::view_remove(self.view_tag);
    }
}
