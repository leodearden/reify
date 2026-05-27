//! Hand-rolled `unsafe extern "C"` FFI bindings to libgmsh 4.15.2.
//!
//! Mirrors `crates/reify-constraints/src/slvs_sys.rs:197-242` (the canonical
//! hand-rolled-FFI reference in this repo). Direct externs are simpler than
//! a cxx-bridge here because `gmshc.h` is a pure-C surface — every gmsh
//! function uses errno-style `int* ierr` last params and ABI-stable
//! signatures, with none of the cxx-features OpenVDB needs (smart pointers,
//! C++ classes, exception translation).
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.
//!
//! # Error mapping
//!
//! Every gmsh API takes a `*mut c_int ierr` final argument; `ierr != 0`
//! signals failure. The [`gmsh_call!`] macro centralises the
//! initialise-`ierr` / call / branch-on-`ierr` ritual and converts non-zero
//! returns into a `GeometryError::OperationFailed` annotated with the
//! function name, the `ierr` value, and the message extracted from
//! `gmshLoggerGetLastError`.

#![allow(non_snake_case, non_camel_case_types)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;

use reify_ir::GeometryError;

// gmsh's C API types `size_t` for tags. `as_ptr() as *const usize` casts
// would be UB on a hypothetical platform where `size_t != usize`. On every
// platform Reify supports they are equal — assert at compile time so a
// future port to an exotic target catches the mismatch immediately.
const _: () = assert!(
    std::mem::size_of::<usize>() == std::mem::size_of::<u64>(),
    "reify-kernel-gmsh assumes size_t == u64 == usize for the gmsh FFI bindings; \
     port-time review required if this assumption ever breaks",
);

// ---------------------------------------------------------------------------
// extern "C" bindings — gmshc.h lifecycle surface
// ---------------------------------------------------------------------------

unsafe extern "C" {
    /// `void gmshInitialize(int argc, char** argv, int readConfigFiles, int run, int* ierr)`
    pub fn gmshInitialize(
        argc: c_int,
        argv: *mut *mut c_char,
        readConfigFiles: c_int,
        run: c_int,
        ierr: *mut c_int,
    );

    /// `int gmshIsInitialized(int* ierr)`
    pub fn gmshIsInitialized(ierr: *mut c_int) -> c_int;

    /// `void gmshFinalize(int* ierr)`
    pub fn gmshFinalize(ierr: *mut c_int);

    /// `void gmshClear(int* ierr)`
    pub fn gmshClear(ierr: *mut c_int);

    /// `void gmshFree(void* p)` — free a buffer allocated by gmsh.
    pub fn gmshFree(p: *mut c_void);

    /// `void gmshLoggerGetLastError(char** error, int* ierr)`
    pub fn gmshLoggerGetLastError(error: *mut *mut c_char, ierr: *mut c_int);

    // ---- model + mesh I/O ----

    /// `void gmshOptionSetNumber(const char* name, double value, int* ierr)`
    pub fn gmshOptionSetNumber(name: *const c_char, value: f64, ierr: *mut c_int);

    /// `void gmshModelAdd(const char* name, int* ierr)`
    pub fn gmshModelAdd(name: *const c_char, ierr: *mut c_int);

    /// `int gmshModelAddDiscreteEntity(int dim, int tag, const int* boundary, size_t boundary_n, int* ierr)`
    pub fn gmshModelAddDiscreteEntity(
        dim: c_int,
        tag: c_int,
        boundary: *const c_int,
        boundary_n: usize,
        ierr: *mut c_int,
    ) -> c_int;

    /// `void gmshModelMeshAddNodes(int dim, int tag, const size_t* nodeTags, size_t nodeTags_n, const double* coord, size_t coord_n, const double* paramCoord, size_t paramCoord_n, int* ierr)`
    pub fn gmshModelMeshAddNodes(
        dim: c_int,
        tag: c_int,
        nodeTags: *const usize,
        nodeTags_n: usize,
        coord: *const f64,
        coord_n: usize,
        paramCoord: *const f64,
        paramCoord_n: usize,
        ierr: *mut c_int,
    );

    /// `void gmshModelMeshAddElementsByType(int tag, int elementType, const size_t* elementTags, size_t elementTags_n, const size_t* nodeTags, size_t nodeTags_n, int* ierr)`
    pub fn gmshModelMeshAddElementsByType(
        tag: c_int,
        elementType: c_int,
        elementTags: *const usize,
        elementTags_n: usize,
        nodeTags: *const usize,
        nodeTags_n: usize,
        ierr: *mut c_int,
    );

    /// `void gmshModelMeshGetNodes(size_t** nodeTags, size_t* nodeTags_n, double** coord, size_t* coord_n, double** paramCoord, size_t* paramCoord_n, int dim, int tag, int includeBoundary, int returnParametricCoord, int* ierr)`
    pub fn gmshModelMeshGetNodes(
        nodeTags: *mut *mut usize,
        nodeTags_n: *mut usize,
        coord: *mut *mut f64,
        coord_n: *mut usize,
        paramCoord: *mut *mut f64,
        paramCoord_n: *mut usize,
        dim: c_int,
        tag: c_int,
        includeBoundary: c_int,
        returnParametricCoord: c_int,
        ierr: *mut c_int,
    );

    /// `void gmshModelMeshGetElementsByType(int elementType, size_t** elementTags, size_t* elementTags_n, size_t** nodeTags, size_t* nodeTags_n, int tag, size_t task, size_t numTasks, int* ierr)`
    pub fn gmshModelMeshGetElementsByType(
        elementType: c_int,
        elementTags: *mut *mut usize,
        elementTags_n: *mut usize,
        nodeTags: *mut *mut usize,
        nodeTags_n: *mut usize,
        tag: c_int,
        task: usize,
        numTasks: usize,
        ierr: *mut c_int,
    );

    /// `void gmshModelMeshClassifySurfaces(double angle, int boundary, int forReparametrization, double curveAngle, int exportDiscrete, int* ierr)`
    pub fn gmshModelMeshClassifySurfaces(
        angle: f64,
        boundary: c_int,
        forReparametrization: c_int,
        curveAngle: f64,
        exportDiscrete: c_int,
        ierr: *mut c_int,
    );

    /// `void gmshModelMeshCreateGeometry(const int* dimTags, size_t dimTags_n, int* ierr)`
    pub fn gmshModelMeshCreateGeometry(dimTags: *const c_int, dimTags_n: usize, ierr: *mut c_int);

    /// `int gmshModelGeoAddSurfaceLoop(const int* surfaceTags, size_t surfaceTags_n, int tag, int* ierr)`
    pub fn gmshModelGeoAddSurfaceLoop(
        surfaceTags: *const c_int,
        surfaceTags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    /// `int gmshModelGeoAddVolume(const int* shellTags, size_t shellTags_n, int tag, int* ierr)`
    pub fn gmshModelGeoAddVolume(
        shellTags: *const c_int,
        shellTags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    /// `void gmshModelGeoSynchronize(int* ierr)`
    pub fn gmshModelGeoSynchronize(ierr: *mut c_int);

    /// `void gmshModelMeshGenerate(int dim, int* ierr)`
    pub fn gmshModelMeshGenerate(dim: c_int, ierr: *mut c_int);

    /// `void gmshModelGetEntities(int** dimTags, size_t* dimTags_n, int dim, int* ierr)`
    pub fn gmshModelGetEntities(
        dimTags: *mut *mut c_int,
        dimTags_n: *mut usize,
        dim: c_int,
        ierr: *mut c_int,
    );

    /// `int gmshModelGeoAddPoint(double x, double y, double z, double meshSize, int tag, int* ierr)`
    pub fn gmshModelGeoAddPoint(
        x: f64,
        y: f64,
        z: f64,
        meshSize: f64,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    /// `int gmshModelGeoAddLine(int startTag, int endTag, int tag, int* ierr)`
    pub fn gmshModelGeoAddLine(
        startTag: c_int,
        endTag: c_int,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    /// `int gmshModelGeoAddCurveLoop(const int* curveTags, size_t curveTags_n, int tag, int reorient, int* ierr)`
    pub fn gmshModelGeoAddCurveLoop(
        curveTags: *const c_int,
        curveTags_n: usize,
        tag: c_int,
        reorient: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    /// `int gmshModelGeoAddPlaneSurface(const int* wireTags, size_t wireTags_n, int tag, int* ierr)`
    pub fn gmshModelGeoAddPlaneSurface(
        wireTags: *const c_int,
        wireTags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    /// `void gmshModelMeshSetRecombine(int dim, int tag, double angle, int* ierr)`
    pub fn gmshModelMeshSetRecombine(dim: c_int, tag: c_int, angle: f64, ierr: *mut c_int);

    /// `void gmshModelMeshSetSize(int* dimTags, size_t dimTags_n, double size, int* ierr)`
    ///
    /// Sets the characteristic mesh size associated with each (dim, tag) pair in
    /// `dimTags`. The `dimTags` array is a flat list of `(dim, tag)` pairs, so
    /// `dimTags_n` is twice the number of entities. With
    /// `Mesh.MeshSizeFromPoints=1`, sizes set on 0D entities (dim=0) drive
    /// interpolated mesh sizing across the entire domain.
    // At time of writing, consumed by same-file Rust wrapper
    // `mesh_set_size_at_entity` (~line 786) → `refine_volume.rs:262`
    // inside `reify_kernel_gmsh::refine_volume_with_size_field`. The
    // G-tool flags same-file callers as orphans; the call chain is live.
    // G-allow: same-file consumer `mesh_set_size_at_entity` → refine_volume.rs:262 (G-tool same-file-caller heuristic limitation).
    pub fn gmshModelMeshSetSize(
        dimTags: *const c_int,
        dimTags_n: usize,
        size: f64,
        ierr: *mut c_int,
    );
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

/// Read the last gmsh error message into an owned `String`, freeing the
/// gmsh-allocated buffer. Returns an empty string on read failure.
fn last_error_message() -> String {
    let mut err_ptr: *mut c_char = ptr::null_mut();
    let mut inner_ierr: c_int = 0;
    unsafe {
        gmshLoggerGetLastError(&mut err_ptr, &mut inner_ierr);
    }
    if err_ptr.is_null() {
        return String::new();
    }
    let msg = unsafe { CStr::from_ptr(err_ptr) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        gmshFree(err_ptr as *mut c_void);
    }
    msg
}

/// Wrap an `unsafe` gmsh call: declare `$ierr` in the caller's scope, run
/// the call, branch on the returned status. On non-zero returns
/// `Err(GeometryError::OperationFailed(...))` annotated with the call site
/// and the last-error message; on zero returns `Ok(())`.
///
/// The `$ierr:ident` capture is necessary because Rust macros are
/// hygienic — without it the `ierr` introduced inside the macro is not
/// visible to the caller's `&mut ierr` arg expression.
///
/// Usage: `gmsh_call!("gmshClear", ierr, gmshClear(&mut ierr))`.
macro_rules! gmsh_call {
    ($name:expr, $ierr:ident, $call:expr) => {{
        let mut $ierr: ::std::ffi::c_int = 0;
        unsafe {
            $call;
        }
        if $ierr != 0 {
            let msg = $crate::ffi::last_error_message();
            Err(::reify_ir::GeometryError::OperationFailed(format!(
                "{}: ierr={} ({})",
                $name, $ierr, msg
            )))
        } else {
            Ok::<(), ::reify_ir::GeometryError>(())
        }
    }};
}

// `gmsh_call!` is intentionally not re-exported via `pub(crate) use` —
// every safe wrapper that needs it lives in this module, and re-exporting
// would generate an unused-import warning in the (lib) target. If a sibling
// module ever needs to invoke the macro directly, add `pub(crate) use gmsh_call;`
// at that point.

/// Function-form companion to [`gmsh_call!`] for wrappers that need the
/// FFI return value (an `i32` tag, owned out-buffers, etc.) and therefore
/// can't be expressed as a single-expression macro call.
///
/// On `ierr == 0` returns `Ok(())`; on non-zero packages the message from
/// `last_error_message()` into a `GeometryError::OperationFailed` annotated
/// with the supplied `name` and the `ierr` code, in the same format the
/// `gmsh_call!` macro emits — so error strings from the two error paths are
/// indistinguishable to callers.
fn check_ierr(name: &str, ierr: c_int) -> Result<(), GeometryError> {
    if ierr == 0 {
        return Ok(());
    }
    let msg = last_error_message();
    Err(GeometryError::OperationFailed(format!(
        "{name}: ierr={ierr} ({msg})"
    )))
}

// ---------------------------------------------------------------------------
// Safe Rust wrappers
// ---------------------------------------------------------------------------

/// Initialise the gmsh library. Idempotent on the gmsh side, but callers
/// should funnel through [`crate::init::ensure_initialized`] which OnceLocks
/// the call.
pub fn initialize() -> Result<(), GeometryError> {
    gmsh_call!(
        "gmshInitialize",
        ierr,
        gmshInitialize(0, ptr::null_mut(), 0, 0, &mut ierr)
    )
}

/// Tear down the gmsh library state. Mostly used by lifecycle smoke tests;
/// the long-running engine path leaves gmsh initialised for the process
/// lifetime.
pub fn finalize() -> Result<(), GeometryError> {
    gmsh_call!("gmshFinalize", ierr, gmshFinalize(&mut ierr))
}

/// `true` if the gmsh library has been initialised and not yet finalised.
pub fn is_initialized() -> bool {
    let mut ierr: c_int = 0;
    let v = unsafe { gmshIsInitialized(&mut ierr) };
    // gmshIsInitialized never sets ierr in practice, but we still respect it:
    // any non-zero ierr is treated as "uninitialised" defensively.
    ierr == 0 && v == 1
}

/// Clear the current gmsh model + model state. Cheaper than finalise/init
/// and used between successive `mesh_to_volume` calls.
pub fn clear() -> Result<(), GeometryError> {
    gmsh_call!("gmshClear", ierr, gmshClear(&mut ierr))
}

/// Set a numerical option (e.g. `"Mesh.MeshSizeMin"`, `"General.NumThreads"`).
pub fn option_set_number(name: &str, value: f64) -> Result<(), GeometryError> {
    let cname = CString::new(name).map_err(|e| {
        GeometryError::OperationFailed(format!("option_set_number: invalid CString: {e}"))
    })?;
    gmsh_call!(
        "gmshOptionSetNumber",
        ierr,
        gmshOptionSetNumber(cname.as_ptr(), value, &mut ierr)
    )
}

/// Add a new model with the given name and make it the current model.
pub fn model_add(name: &str) -> Result<(), GeometryError> {
    let cname = CString::new(name)
        .map_err(|e| GeometryError::OperationFailed(format!("model_add: invalid CString: {e}")))?;
    gmsh_call!(
        "gmshModelAdd",
        ierr,
        gmshModelAdd(cname.as_ptr(), &mut ierr)
    )
}

/// Add a discrete entity of the given dimension and return its assigned tag.
///
/// Pass an empty `boundary` slice to let gmsh skip the boundary association
/// step (the typical mesh-input case).
pub fn add_discrete_entity(dim: i32, boundary: &[i32]) -> Result<i32, GeometryError> {
    let mut ierr: c_int = 0;
    let tag = unsafe {
        gmshModelAddDiscreteEntity(
            dim,
            -1,
            if boundary.is_empty() {
                ptr::null()
            } else {
                boundary.as_ptr()
            },
            boundary.len(),
            &mut ierr,
        )
    };
    check_ierr("gmshModelAddDiscreteEntity", ierr)?;
    Ok(tag)
}

/// Add nodes to a 2D entity (`dim=2`) with the given tag.
///
/// `node_tags` and `coords` are parallel: 1 tag per 3 coords (`x`, `y`, `z`).
/// Tags must be strictly positive and unique within the model.
pub fn add_nodes_2d(surf_tag: i32, node_tags: &[u64], coords: &[f64]) -> Result<(), GeometryError> {
    // Runtime check (not debug_assert): mismatched slices here would feed a
    // bad buffer to gmsh in release builds — opaque internal error or
    // out-of-bounds reads on the C side. This is a public FFI-boundary
    // function; pay the equality check on every call.
    if coords.len() != node_tags.len() * 3 {
        return Err(GeometryError::OperationFailed(format!(
            "add_nodes_2d: coords.len()={} must equal node_tags.len()*3={} \
             (node_tags.len()={})",
            coords.len(),
            node_tags.len() * 3,
            node_tags.len(),
        )));
    }
    // SAFETY: u64 == usize == size_t on every supported target (compile-time
    // assert at module top); the slice memory layout is identical.
    let node_tags_ptr = node_tags.as_ptr() as *const usize;
    gmsh_call!(
        "gmshModelMeshAddNodes",
        ierr,
        gmshModelMeshAddNodes(
            2,
            surf_tag,
            node_tags_ptr,
            node_tags.len(),
            coords.as_ptr(),
            coords.len(),
            ptr::null(),
            0,
            &mut ierr,
        )
    )
}

/// Add elements of a single type to a 2D entity (`dim=2`) with the given
/// tag.
///
/// `element_type` is gmsh's canonical type code (e.g. `2` for 3-node
/// triangle, `3` for 4-node quad). `element_tags` carries one tag per
/// element; `node_tags` is the flat connectivity array (3 nodes per
/// triangle, etc.).
pub fn add_elements_2d(
    surf_tag: i32,
    element_type: i32,
    element_tags: &[u64],
    node_tags: &[u64],
) -> Result<(), GeometryError> {
    // Mirror add_nodes_2d's stride check: `node_tags.len()` must be
    // `element_tags.len() * nodes_per_element`. Only the two element types
    // this crate actually feeds gmsh are checked — `2` (3-node triangle) and
    // `3` (4-node quad). For any other type the caller is on its own
    // (gmsh's own error message will surface) and we let the call through.
    let nodes_per_element: Option<usize> = match element_type {
        2 => Some(3),
        3 => Some(4),
        _ => None,
    };
    if let Some(npe) = nodes_per_element
        && node_tags.len() != element_tags.len() * npe
    {
        return Err(GeometryError::OperationFailed(format!(
            "add_elements_2d: node_tags.len()={} must equal element_tags.len()*{}={} \
             for element_type={} (element_tags.len()={})",
            node_tags.len(),
            npe,
            element_tags.len() * npe,
            element_type,
            element_tags.len(),
        )));
    }
    let element_tags_ptr = element_tags.as_ptr() as *const usize;
    let node_tags_ptr = node_tags.as_ptr() as *const usize;
    gmsh_call!(
        "gmshModelMeshAddElementsByType",
        ierr,
        gmshModelMeshAddElementsByType(
            surf_tag,
            element_type,
            element_tags_ptr,
            element_tags.len(),
            node_tags_ptr,
            node_tags.len(),
            &mut ierr,
        )
    )
}

/// Read all nodes from the current model into owned `(node_tags, coords)`
/// vectors. Coords are flat `[x0,y0,z0, x1,y1,z1, ...]`.
///
/// Equivalent to `gmshModelMeshGetNodes(-1, -1, includeBoundary=1,
/// returnParametricCoord=0, ...)`. The gmsh-allocated buffers are copied
/// into owned `Vec<u64>` / `Vec<f64>` and freed via `gmshFree` before
/// return — callers see safe Rust ownership only.
pub fn get_nodes_all() -> Result<(Vec<u64>, Vec<f64>), GeometryError> {
    let mut node_tags_ptr: *mut usize = ptr::null_mut();
    let mut node_tags_n: usize = 0;
    let mut coord_ptr: *mut f64 = ptr::null_mut();
    let mut coord_n: usize = 0;
    let mut param_ptr: *mut f64 = ptr::null_mut();
    let mut param_n: usize = 0;
    let mut ierr: c_int = 0;
    unsafe {
        gmshModelMeshGetNodes(
            &mut node_tags_ptr,
            &mut node_tags_n,
            &mut coord_ptr,
            &mut coord_n,
            &mut param_ptr,
            &mut param_n,
            -1,
            -1,
            1,
            0,
            &mut ierr,
        );
    }
    let node_tags: Vec<u64> = if node_tags_ptr.is_null() || node_tags_n == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(node_tags_ptr as *const u64, node_tags_n) }.to_vec()
    };
    let coords: Vec<f64> = if coord_ptr.is_null() || coord_n == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(coord_ptr, coord_n) }.to_vec()
    };
    unsafe {
        if !node_tags_ptr.is_null() {
            gmshFree(node_tags_ptr as *mut c_void);
        }
        if !coord_ptr.is_null() {
            gmshFree(coord_ptr as *mut c_void);
        }
        if !param_ptr.is_null() {
            gmshFree(param_ptr as *mut c_void);
        }
    }
    check_ierr("gmshModelMeshGetNodes", ierr)?;
    Ok((node_tags, coords))
}

/// Classify surfaces from the discrete mesh: build curve / surface
/// boundaries from the mesh edges using the supplied dihedral angle as
/// the feature-edge threshold.
///
/// `angle`: dihedral angle in radians (e.g. `π/2` = 90° splits cube faces).
/// `boundary`: nonzero to create boundary edges.
/// `for_reparam`: nonzero to also build a parametric representation suitable
///   for `gmshModelMeshCreateGeometry` to attach a B-rep.
/// `curve_angle`: dihedral angle in radians for curve-feature detection.
/// `export_discrete`: nonzero to overwrite the discrete model with the
///   reclassified one (we leave at 0; the discrete model stays for our reads).
pub fn classify_surfaces(
    angle: f64,
    boundary: i32,
    for_reparam: i32,
    curve_angle: f64,
    export_discrete: i32,
) -> Result<(), GeometryError> {
    gmsh_call!(
        "gmshModelMeshClassifySurfaces",
        ierr,
        gmshModelMeshClassifySurfaces(
            angle,
            boundary,
            for_reparam,
            curve_angle,
            export_discrete,
            &mut ierr,
        )
    )
}

/// Build geometry (B-rep curves/surfaces) from the classified mesh.
///
/// Pass an empty `dim_tags` slice to have gmsh process all classified
/// entities.
pub fn create_geometry(dim_tags: &[i32]) -> Result<(), GeometryError> {
    let (ptr_, n) = if dim_tags.is_empty() {
        (ptr::null(), 0)
    } else {
        (dim_tags.as_ptr(), dim_tags.len())
    };
    gmsh_call!(
        "gmshModelMeshCreateGeometry",
        ierr,
        gmshModelMeshCreateGeometry(ptr_, n, &mut ierr)
    )
}

/// Add a built-in-CAD surface loop from the given surface tags. Returns the
/// assigned loop tag (positive integer chosen by gmsh).
pub fn geo_add_surface_loop(surface_tags: &[i32]) -> Result<i32, GeometryError> {
    let mut ierr: c_int = 0;
    let tag = unsafe {
        gmshModelGeoAddSurfaceLoop(surface_tags.as_ptr(), surface_tags.len(), -1, &mut ierr)
    };
    check_ierr("gmshModelGeoAddSurfaceLoop", ierr)?;
    Ok(tag)
}

/// Add a built-in-CAD volume from the given surface-loop (shell) tags.
/// Returns the assigned volume tag.
pub fn geo_add_volume(shell_tags: &[i32]) -> Result<i32, GeometryError> {
    let mut ierr: c_int = 0;
    let tag =
        unsafe { gmshModelGeoAddVolume(shell_tags.as_ptr(), shell_tags.len(), -1, &mut ierr) };
    check_ierr("gmshModelGeoAddVolume", ierr)?;
    Ok(tag)
}

/// Synchronise the built-in CAD model into the gmsh internal model.
pub fn geo_synchronize() -> Result<(), GeometryError> {
    gmsh_call!(
        "gmshModelGeoSynchronize",
        ierr,
        gmshModelGeoSynchronize(&mut ierr)
    )
}

/// Generate the mesh up to the given dimension (`3` = volumetric tets).
pub fn mesh_generate(dim: i32) -> Result<(), GeometryError> {
    gmsh_call!(
        "gmshModelMeshGenerate",
        ierr,
        gmshModelMeshGenerate(dim, &mut ierr)
    )
}

/// Read all entity tags of the given dimension from the current model.
///
/// Returns the second elements of gmsh's `(dim, tag)` flat list — that is,
/// just the tags. Useful after `classify_surfaces` + `create_geometry`,
/// which create new geometric surface entities whose tags may differ from
/// the discrete-mesh entity tags initially passed to `add_discrete_entity`.
pub fn get_entity_tags(dim: i32) -> Result<Vec<i32>, GeometryError> {
    let mut dim_tags_ptr: *mut c_int = ptr::null_mut();
    let mut dim_tags_n: usize = 0;
    let mut ierr: c_int = 0;
    unsafe {
        gmshModelGetEntities(&mut dim_tags_ptr, &mut dim_tags_n, dim, &mut ierr);
    }
    let pairs: Vec<c_int> = if dim_tags_ptr.is_null() || dim_tags_n == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(dim_tags_ptr, dim_tags_n) }.to_vec()
    };
    unsafe {
        if !dim_tags_ptr.is_null() {
            gmshFree(dim_tags_ptr as *mut c_void);
        }
    }
    check_ierr("gmshModelGetEntities", ierr)?;
    // gmsh returns flat (dim, tag) pairs — collect every odd index.
    let tags: Vec<i32> = pairs.chunks_exact(2).map(|p| p[1]).collect();
    Ok(tags)
}

/// Read all elements of the given type from the current model into owned
/// `(element_tags, node_tags)` vectors.
///
/// `node_tags` is the flat connectivity array — for `element_type=2` (3-node
/// triangle), `node_tags.len() == element_tags.len() * 3`.
pub fn get_elements_by_type(element_type: i32) -> Result<(Vec<u64>, Vec<u64>), GeometryError> {
    let mut elem_tags_ptr: *mut usize = ptr::null_mut();
    let mut elem_tags_n: usize = 0;
    let mut node_tags_ptr: *mut usize = ptr::null_mut();
    let mut node_tags_n: usize = 0;
    let mut ierr: c_int = 0;
    unsafe {
        gmshModelMeshGetElementsByType(
            element_type,
            &mut elem_tags_ptr,
            &mut elem_tags_n,
            &mut node_tags_ptr,
            &mut node_tags_n,
            -1,
            0,
            1,
            &mut ierr,
        );
    }
    let elem_tags: Vec<u64> = if elem_tags_ptr.is_null() || elem_tags_n == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(elem_tags_ptr as *const u64, elem_tags_n) }.to_vec()
    };
    let node_tags: Vec<u64> = if node_tags_ptr.is_null() || node_tags_n == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(node_tags_ptr as *const u64, node_tags_n) }.to_vec()
    };
    unsafe {
        if !elem_tags_ptr.is_null() {
            gmshFree(elem_tags_ptr as *mut c_void);
        }
        if !node_tags_ptr.is_null() {
            gmshFree(node_tags_ptr as *mut c_void);
        }
    }
    check_ierr("gmshModelMeshGetElementsByType", ierr)?;
    Ok((elem_tags, node_tags))
}

/// Add a built-in-CAD point at `(x, y, z)`. Returns the gmsh-assigned tag.
///
/// `mesh_size_hint` is the target element-size at this point; pass `0.0` to
/// let gmsh choose (the global `Mesh.MeshSizeFactor` / `MeshSizeMin/Max`
/// options still apply). The internal `tag` argument is wired to `-1` so
/// gmsh picks the next free positive integer.
pub fn geo_add_point(x: f64, y: f64, z: f64, mesh_size_hint: f64) -> Result<i32, GeometryError> {
    let mut ierr: c_int = 0;
    let tag = unsafe { gmshModelGeoAddPoint(x, y, z, mesh_size_hint, -1, &mut ierr) };
    check_ierr("gmshModelGeoAddPoint", ierr)?;
    Ok(tag)
}

/// Add a built-in-CAD straight line from `start_tag` to `end_tag`. Returns
/// the gmsh-assigned line tag.
///
/// The internal `tag` argument is wired to `-1` so gmsh picks the next free
/// positive integer.
pub fn geo_add_line(start_tag: i32, end_tag: i32) -> Result<i32, GeometryError> {
    let mut ierr: c_int = 0;
    let tag = unsafe { gmshModelGeoAddLine(start_tag, end_tag, -1, &mut ierr) };
    check_ierr("gmshModelGeoAddLine", ierr)?;
    Ok(tag)
}

/// Add a built-in-CAD curve loop from the given curve (line/arc/…) tags.
/// Returns the gmsh-assigned loop tag.
///
/// The internal `tag` is `-1` (gmsh chooses); `reorient` is `0` (caller is
/// responsible for ordering the curves head-to-tail). Gmsh accepts either
/// CCW or CW ordering — orientation determines the loop's normal but is not
/// a validity constraint.
pub fn geo_add_curve_loop(curve_tags: &[i32]) -> Result<i32, GeometryError> {
    let mut ierr: c_int = 0;
    let tag = unsafe {
        gmshModelGeoAddCurveLoop(curve_tags.as_ptr(), curve_tags.len(), -1, 0, &mut ierr)
    };
    check_ierr("gmshModelGeoAddCurveLoop", ierr)?;
    Ok(tag)
}

/// Add a built-in-CAD plane surface bounded by the supplied curve-loop tags.
/// The first slot is the outer boundary; remaining slots are interpreted as
/// holes. Returns the gmsh-assigned surface tag.
///
/// The internal `tag` is `-1` (gmsh chooses the next free positive integer).
pub fn geo_add_plane_surface(curve_loop_tags: &[i32]) -> Result<i32, GeometryError> {
    let mut ierr: c_int = 0;
    let tag = unsafe {
        gmshModelGeoAddPlaneSurface(
            curve_loop_tags.as_ptr(),
            curve_loop_tags.len(),
            -1,
            &mut ierr,
        )
    };
    check_ierr("gmshModelGeoAddPlaneSurface", ierr)?;
    Ok(tag)
}

/// Scope recombination to a specific entity (`dim=2` + surface tag for a
/// plane surface). `angle` is the per-corner deviation tolerance (degrees)
/// gmsh uses to decide whether two triangles can be merged into a quad.
///
/// Preferred over the global `Mesh.RecombineAll` option because it scopes
/// recombination to the specific surface — important once a single gmsh
/// model holds multiple bodies (task 2989's batched-eval path).
pub fn mesh_set_recombine(dim: i32, tag: i32, angle: f64) -> Result<(), GeometryError> {
    gmsh_call!(
        "gmshModelMeshSetRecombine",
        ierr,
        gmshModelMeshSetRecombine(dim, tag, angle, &mut ierr)
    )
}

/// Set the target characteristic mesh size for a single model entity
/// identified by `(dim, tag)`.
///
/// With `Mesh.MeshSizeFromPoints=1`, sizes on 0D entities (dim=0) drive
/// interpolated mesh sizing across the domain. Returns an error if gmsh
/// rejects the entity (e.g. the entity doesn't exist in the current model).
///
/// Internally calls `gmshModelMeshSetSize` with a two-element flat
/// `[dim, tag]` array — the same format as passing one entry to the
/// vectorised API.
pub fn mesh_set_size_at_entity(dim: i32, tag: i32, size: f64) -> Result<(), GeometryError> {
    let dim_tags = [dim, tag];
    let mut ierr: c_int = 0;
    unsafe {
        gmshModelMeshSetSize(dim_tags.as_ptr(), 2, size, &mut ierr);
    }
    check_ierr("gmshModelMeshSetSize", ierr)
}

/// Read the mesh nodes that belong to a specific model entity `(dim, tag)`.
///
/// `includeBoundary=0` — returns only nodes directly on this entity, not
/// those inherited from lower-dimension boundary entities. For 0D entities
/// (dim=0), this is exactly one node (the corner mesh node).
///
/// Returns `(node_tags, coords)` where `coords` is flat `[x0,y0,z0, …]`.
/// Ownership is transferred from gmsh-allocated buffers to `Vec`; the raw
/// buffers are freed via `gmshFree` before return.
pub fn get_nodes_at_entity(dim: i32, tag: i32) -> Result<(Vec<u64>, Vec<f64>), GeometryError> {
    let mut node_tags_ptr: *mut usize = ptr::null_mut();
    let mut node_tags_n: usize = 0;
    let mut coord_ptr: *mut f64 = ptr::null_mut();
    let mut coord_n: usize = 0;
    let mut param_ptr: *mut f64 = ptr::null_mut();
    let mut param_n: usize = 0;
    let mut ierr: c_int = 0;
    unsafe {
        gmshModelMeshGetNodes(
            &mut node_tags_ptr,
            &mut node_tags_n,
            &mut coord_ptr,
            &mut coord_n,
            &mut param_ptr,
            &mut param_n,
            dim,
            tag,
            0, // includeBoundary=0: only nodes directly on this entity
            0, // returnParametricCoord=0: skip parametric coordinates
            &mut ierr,
        );
    }
    let node_tags: Vec<u64> = if node_tags_ptr.is_null() || node_tags_n == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(node_tags_ptr as *const u64, node_tags_n) }.to_vec()
    };
    let coords: Vec<f64> = if coord_ptr.is_null() || coord_n == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(coord_ptr, coord_n) }.to_vec()
    };
    unsafe {
        if !node_tags_ptr.is_null() {
            gmshFree(node_tags_ptr as *mut c_void);
        }
        if !coord_ptr.is_null() {
            gmshFree(coord_ptr as *mut c_void);
        }
        if !param_ptr.is_null() {
            gmshFree(param_ptr as *mut c_void);
        }
    }
    check_ierr("gmshModelMeshGetNodes(entity)", ierr)?;
    Ok((node_tags, coords))
}
