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
