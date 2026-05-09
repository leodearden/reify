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

use reify_types::GeometryError;

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
    let msg = unsafe { CStr::from_ptr(err_ptr) }.to_string_lossy().into_owned();
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
            Err(::reify_types::GeometryError::OperationFailed(format!(
                "{}: ierr={} ({})",
                $name, $ierr, msg
            )))
        } else {
            Ok::<(), ::reify_types::GeometryError>(())
        }
    }};
}

pub(crate) use gmsh_call;

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
    let cname = CString::new(name).map_err(|e| {
        GeometryError::OperationFailed(format!("model_add: invalid CString: {e}"))
    })?;
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
    if ierr != 0 {
        let msg = last_error_message();
        return Err(GeometryError::OperationFailed(format!(
            "gmshModelAddDiscreteEntity: ierr={ierr} ({msg})"
        )));
    }
    Ok(tag)
}

/// Add nodes to a 2D entity (`dim=2`) with the given tag.
///
/// `node_tags` and `coords` are parallel: 1 tag per 3 coords (`x`, `y`, `z`).
/// Tags must be strictly positive and unique within the model.
pub fn add_nodes_2d(
    surf_tag: i32,
    node_tags: &[u64],
    coords: &[f64],
) -> Result<(), GeometryError> {
    debug_assert_eq!(
        coords.len(),
        node_tags.len() * 3,
        "add_nodes_2d: coords.len() must equal node_tags.len() * 3"
    );
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
    if ierr != 0 {
        let msg = last_error_message();
        return Err(GeometryError::OperationFailed(format!(
            "gmshModelMeshGetNodes: ierr={ierr} ({msg})"
        )));
    }
    Ok((node_tags, coords))
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
    if ierr != 0 {
        let msg = last_error_message();
        return Err(GeometryError::OperationFailed(format!(
            "gmshModelMeshGetElementsByType: ierr={ierr} ({msg})"
        )));
    }
    Ok((elem_tags, node_tags))
}
