//! Shared allocation-counting helpers for alloc-sensitive integration test binaries.
//!
//! Include the type and counter in a test binary with:
//!
//! ```rust,ignore
//! mod common;
//!
//! #[global_allocator]
//! static GLOBAL: common::alloc_counter::CountingAllocator =
//!     common::alloc_counter::CountingAllocator;
//! ```
//!
//! Then reference `common::alloc_counter::ALLOCATIONS` to snapshot the counter.
//!
//! # Why the `#[global_allocator]` static stays in each binary root
//!
//! The `#[global_allocator]` attribute is process-wide.  Rust enforces exactly one
//! per final binary, and the attributed `static` must live in the binary crate root.
//! Only the *type*, *impl*, and *counter* are shared here; the `static GLOBAL`
//! declaration remains in each test file.
//!
//! # Counter isolation
//!
//! Because each file under `tests/` compiles to a separate binary, each binary gets
//! its own copy of `ALLOCATIONS` — there is no cross-binary sharing.  This is the
//! desired behaviour: counters are isolated per process.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Thin wrapper around [`std::alloc::System`] that counts every `alloc` call.
///
/// `#[allow(dead_code)]` because shared `tests/common/*` modules are re-compiled
/// into every test binary under `tests/`, and binaries that don't use this
/// helper would otherwise trip the `dead_code` lint.
#[allow(dead_code)]
pub struct CountingAllocator;

/// Global counter incremented on every allocation.
///
/// Each test binary that includes this module via `mod common;` gets its own
/// independent copy of this static — sharing only the type definition, not state.
#[allow(dead_code)]
pub static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: delegating to the system allocator with the same layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: delegating to the system allocator with the same layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}
