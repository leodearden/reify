use crate::kernel_status::{KERNEL_UNAVAILABLE_MESSAGE, KernelStatus, kernel_status_for};

#[test]
fn kernel_status_for_true_returns_available() {
    let status = kernel_status_for(true);
    assert_eq!(
        status,
        KernelStatus {
            available: true,
            message: None
        }
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
    use reify_kernel_occt::OCCT_AVAILABLE;

    #[test]
    fn current_kernel_status_matches_occt_availability() {
        let status = kernel_status::current_kernel_status();
        assert_eq!(status.available, OCCT_AVAILABLE);
        if !OCCT_AVAILABLE {
            assert_eq!(status.message.as_deref(), Some(KERNEL_UNAVAILABLE_MESSAGE));
        } else {
            assert!(status.message.is_none());
        }
    }

    /// Pins the registry-population invariant: `reify_eval::kernel_registry::registry()`
    /// contains the `"occt"` entry when OCCT is available (feature = "gui" and
    /// cfg(has_occt) is set).
    ///
    /// Note: this test runs inside a cargo unit-test binary, not the `reify-gui`
    /// Tauri binary itself.  Both share the same dep-tree (reify-kernel-occt is an
    /// `optional = true` dep gated on feature = "gui"), so both see the same
    /// registry contents.  The neighboring test
    /// `engine_session_with_registered_kernel_picks_occt_for_primitive_box_build`
    /// is the authoritative behavioral pin for the production boot path; this test
    /// pins only that `registry()` itself returns the expected map when OCCT is
    /// linked.
    ///
    /// Skipped via `eprintln!` in stub mode so CI logs make the skip visible.
    #[test]
    fn gui_registry_population_contains_occt() {
        if !OCCT_AVAILABLE {
            eprintln!(
                "skipping gui_registry_population_contains_occt: \
                 OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
            );
            return;
        }
        let reg = reify_eval::kernel_registry::registry();
        assert!(
            reg.contains_key("occt"),
            "registry() must contain \"occt\" when reify-kernel-occt is the \
             optional dep and cfg(has_occt) is set; \
             got keys: {:?}",
            reg.keys().collect::<Vec<_>>()
        );
    }

    /// Regression pin: `EngineSession::with_registered_kernel` boot path
    /// constructs a working session via the inventory-based kernel registry.
    ///
    /// When OCCT is available, constructs a session via the inventory-based
    /// `with_registered_kernel` constructor (the production GUI boot path), loads a
    /// primitive-box source, invokes a STEP export, and asserts the output file is
    /// non-empty.  This proves the registered OCCT kernel actually fired through the
    /// full parse → compile → check → build pipeline, mirroring the CLI sibling pin
    /// `cli_build_with_primitive_box_produces_step_output`.
    ///
    /// Skipped via `eprintln!` in stub mode (`OCCT_AVAILABLE = false`) so CI logs
    /// make the skip visible.
    #[test]
    fn engine_session_with_registered_kernel_picks_occt_for_primitive_box_build() {
        if !OCCT_AVAILABLE {
            eprintln!("Skipping: OCCT not available in this build (stub mode)");
            return;
        }
        use crate::engine::EngineSession;
        use reify_constraints::SimpleConstraintChecker;
        use reify_ir::ExportFormat;
        let mut session = EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker));
        session
            .load_from_source(
                "structure S { let b = box(10mm, 10mm, 10mm) }",
                "primitive_box_build",
            )
            .expect("load_from_source should succeed with registered OCCT kernel");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("primitive_box.step");
        let result = session.export(ExportFormat::Step, &path);
        assert!(
            result.is_ok(),
            "export should succeed when OCCT kernel is registered: {:?}",
            result.err()
        );
        let data = std::fs::read(&path).expect("exported STEP file should be readable");
        assert!(
            !data.is_empty(),
            "STEP output must be non-empty — OCCT kernel must have fired through \
             with_registered_kernel boot path"
        );
    }
}
