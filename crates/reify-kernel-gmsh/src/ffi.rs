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

use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr;

use reify_types::GeometryError;

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
