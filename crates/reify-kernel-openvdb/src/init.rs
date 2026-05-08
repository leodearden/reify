//! Shared OpenVDB library-initialisation helper.
//!
//! `openvdb::initialize()` populates the I/O dispatch table with built-in
//! grid types (FloatGrid, Vec3SGrid, etc.). It must be called before any
//! `openvdb::io::File::open` / `readGrid` call — otherwise downcasts via
//! `gridPtrCast<FloatGrid>` return `nullptr` and the read path emits the
//! misleading `"is not a FloatGrid"` error from
//! `cpp/openvdb_wrapper.cpp::read_vdb_grid_ffi`.
//!
//! All cfg(has_openvdb) entry points that touch OpenVDB I/O — `OpenVdbKernel::new()`
//! and the `read_vdb_file` body in `ingest.rs` — call [`ensure_initialized`]
//! before reaching the FFI. The `OnceLock` guards idempotence so the cost of
//! repeated calls from kernel-mediated paths is one cached-cell branch.
//!
//! `openvdb::initialize()` itself is also internally idempotent (per OpenVDB's
//! source comment), but channelling all callers through a single OnceLock
//! lets future contributors trace init-ordering bugs to one location instead
//! of every `OpenVdbKernel::new()` / `read_vdb_file` call site.
//!
//! Only compiled when `cfg(has_openvdb)` is set.

use std::sync::OnceLock;

use crate::ffi::ffi as openvdb_ffi;

static OPENVDB_INIT: OnceLock<()> = OnceLock::new();

/// Initialise the OpenVDB library on first call; cached for subsequent calls.
///
/// Safe to call from any thread — `OnceLock::get_or_init` synchronises
/// concurrent first-callers so `openvdb_initialize()` runs exactly once.
pub(crate) fn ensure_initialized() {
    OPENVDB_INIT.get_or_init(|| {
        openvdb_ffi::openvdb_initialize();
    });
}
