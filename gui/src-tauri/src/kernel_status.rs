//! Kernel availability status — surfaced to the GUI at startup.

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

/// Register the OCCT kernel on `planner` if available, then return the status.
///
/// When OCCT is not linked (`OCCT_AVAILABLE == false`), the kernel is intentionally
/// *not* registered so that downstream geometry ops fail with "no kernel registered"
/// rather than a silent OCCT-stub error — paired with the startup banner that
/// explains why.
///
/// Only available under the `gui` feature (which enables `reify-kernel-occt`).
#[cfg(feature = "gui")]
pub fn configure_planner(planner: &mut reify_geometry::DispatchPlanner) -> KernelStatus {
    use reify_kernel_occt::OcctKernelHandle;

    let status = current_kernel_status();
    if status.available {
        let handle = OcctKernelHandle::spawn();
        planner.register_kernel(Box::new(handle));
    }
    status
}
