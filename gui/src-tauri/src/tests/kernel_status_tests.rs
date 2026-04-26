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
    assert!(!status.available);
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

#[cfg(feature = "gui")]
mod gui_tests {
    use crate::kernel_status::{self, KERNEL_UNAVAILABLE_MESSAGE};
    use reify_geometry::DispatchPlanner;
    use reify_kernel_occt::OCCT_AVAILABLE;

    #[test]
    fn current_kernel_status_matches_occt_availability() {
        let status = kernel_status::current_kernel_status();
        assert_eq!(status.available, OCCT_AVAILABLE);
        if !OCCT_AVAILABLE {
            assert_eq!(
                status.message.as_deref(),
                Some(KERNEL_UNAVAILABLE_MESSAGE)
            );
        } else {
            assert!(status.message.is_none());
        }
    }

    #[test]
    fn configure_planner_matches_availability() {
        let mut planner = DispatchPlanner::new();
        let status = kernel_status::configure_planner(&mut planner);
        assert_eq!(status.available, OCCT_AVAILABLE);
        assert_eq!(planner.has_kernel(), OCCT_AVAILABLE);
    }
}
