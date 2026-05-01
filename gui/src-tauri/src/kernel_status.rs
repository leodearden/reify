//! Kernel availability status — surfaced to the GUI at startup.
//!
//! Public API: [`KernelStatus`] (IPC type), [`KERNEL_UNAVAILABLE_MESSAGE`] (constant),
//! [`kernel_status_for`] (helper), [`current_kernel_status`] (reads `OCCT_AVAILABLE`).
//! The geometry kernel itself is registered via `inventory::submit!` in
//! `reify-kernel-occt::register`; this module no longer mutates planner state.

use serde::{Deserialize, Serialize};

/// User-facing message shown when the geometry kernel (OCCT) is not linked.
pub const KERNEL_UNAVAILABLE_MESSAGE: &str =
    "Geometry kernel not available — OCCT not linked";

/// IPC-serializable record of whether the geometry kernel is available.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KernelStatus {
    pub available: bool,
    pub message: Option<String>,
}

/// Compute `KernelStatus` from a boolean `occt_available` flag.
///
/// When `true`, returns `available: true, message: None`.
/// When `false`, returns `available: false, message: Some(KERNEL_UNAVAILABLE_MESSAGE)`.
pub fn kernel_status_for(occt_available: bool) -> KernelStatus {
    if occt_available {
        KernelStatus { available: true, message: None }
    } else {
        KernelStatus {
            available: false,
            message: Some(KERNEL_UNAVAILABLE_MESSAGE.to_owned()),
        }
    }
}

/// Read the current kernel status from the build-time `OCCT_AVAILABLE` constant.
///
/// Only available under the `gui` feature (which enables `reify-kernel-occt`).
#[cfg(feature = "gui")]
pub fn current_kernel_status() -> KernelStatus {
    kernel_status_for(reify_kernel_occt::OCCT_AVAILABLE)
}

