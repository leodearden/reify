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

    /// Regression pin: `reify_eval::kernel_registry::registry()` visible from
    /// inside the GUI's link closure contains the `"occt"` entry when OCCT
    /// is available (feature = "gui" and cfg(has_occt)).
    ///
    /// Protects against accidental future removal of `reify-kernel-occt` as the
    /// GUI's optional dep (or mis-gating the gui feature), which would silently
    /// empty the registry and leave `EngineSession::with_registered_kernel`
    /// constructing a no-kernel engine.
    ///
    /// Skipped via `eprintln!` in stub mode so CI logs make the skip visible.
    #[test]
    fn gui_link_closure_registry_contains_occt() {
        if !OCCT_AVAILABLE {
            eprintln!(
                "skipping gui_link_closure_registry_contains_occt: \
                 OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
            );
            return;
        }
        let reg = reify_eval::kernel_registry::registry();
        assert!(
            reg.contains_key("occt"),
            "registry() in the GUI's link closure must contain \"occt\" when \
             reify-kernel-occt is the optional dep and cfg(has_occt) is set; \
             got keys: {:?}",
            reg.keys().collect::<Vec<_>>()
        );
        assert!(
            reg.len() >= 1,
            "registry() must have at least one entry when OCCT is available"
        );
    }

    /// Regression pin: `EngineSession::with_registered_kernel` boot path
    /// constructs a working session via the inventory-based kernel registry.
    ///
    /// When OCCT is available, constructs a session, loads a primitive box source,
    /// and asserts that the load succeeds without errors. The geometry build itself
    /// is exercised by the CLI pin and the reify-eval kernel_registry_inventory test;
    /// here we pin only that the production boot path compiles and runs without error.
    #[test]
    fn engine_session_with_registered_kernel_picks_occt_for_primitive_box_build() {
        if !OCCT_AVAILABLE {
            eprintln!("Skipping: OCCT not available in this build (stub mode)");
            return;
        }
        use crate::engine::EngineSession;
        use reify_constraints::SimpleConstraintChecker;
        let mut session =
            EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker));
        let _ = session
            .load_from_source(
                "structure S { let b = box(10mm, 10mm, 10mm) }",
                "primitive_box_build",
            )
            .expect("load_from_source should succeed with registered OCCT kernel");
    }
}
