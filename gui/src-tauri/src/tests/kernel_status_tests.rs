use crate::kernel_status::{KernelStatus, kernel_status_for, KERNEL_UNAVAILABLE_MESSAGE};

#[test]
fn kernel_status_for_true_returns_available() {
    let status = kernel_status_for(true);
    assert_eq!(
        status,
        KernelStatus { available: true, message: None }
    );
}

#[test]
fn kernel_status_for_false_returns_unavailable_with_message() {
    let status = kernel_status_for(false);
    assert_eq!(status.available, false);
    assert_eq!(status.message.as_deref(), Some(KERNEL_UNAVAILABLE_MESSAGE));
}

#[test]
fn kernel_status_for_false_message_exact_wording() {
    let status = kernel_status_for(false);
    assert_eq!(
        status.message.as_deref(),
        Some("Geometry kernel not available — OCCT not linked")
    );
}

#[test]
fn kernel_status_ipc_contract() {
    super::assert_ipc_contract::<KernelStatus>();
}
