mod common;

/// Pins the registry-population invariant: `reify_eval::kernel_registry::registry()`
/// contains the `"occt"` entry when OCCT is available (cfg(has_occt) is set).
///
/// Note: this test runs inside a cargo integration-test binary, not the `reify`
/// CLI binary itself.  Both share the same dep-tree (reify-kernel-occt is a
/// `[dependencies]` entry in reify-cli/Cargo.toml), so both see the same
/// registry contents.  The behavioral pin below
/// (`cli_build_with_primitive_box_produces_step_output`) is the authoritative
/// regression guard for the production binary's boot path; this test pins only
/// that `registry()` itself returns the expected map when OCCT is linked.
///
/// Skipped via `eprintln!` in stub mode (OCCT_AVAILABLE = false) so CI logs
/// make the skip visible rather than producing a silent no-op.
#[test]
fn cli_link_closure_registry_contains_occt() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping cli_link_closure_registry_contains_occt: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }
    let reg = reify_eval::kernel_registry::registry();
    assert!(
        reg.contains_key("occt"),
        "registry() must contain \"occt\" when reify-kernel-occt is a \
         [dependencies] entry and cfg(has_occt) is set; \
         got keys: {:?}",
        reg.keys().collect::<Vec<_>>()
    );
}

/// Behavioral regression pin: `reify build` against a primitive box fixture
/// produces non-empty STEP output through the registered OCCT kernel.
///
/// Passes both before and after the boot-path migration (step-4) because the
/// CLI already includes `reify-kernel-occt` as a direct dep; what changes in
/// step-4 is HOW the kernel is wired (planner-based → inventory-based), not
/// WHETHER it works.  An empty output file after step-4 would indicate the
/// registered factory never fired — this pin catches that regression.
///
/// Skipped in stub mode (see note in [`cli_link_closure_registry_contains_occt`]).
#[test]
fn cli_build_with_primitive_box_produces_step_output() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping cli_build_with_primitive_box_produces_step_output: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }
    let result = common::run_build("bracket.ri");
    assert!(
        result.status.success(),
        "reify build should succeed for bracket.ri.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.output_path.exists(),
        "output STEP file should be written for bracket.ri build"
    );
    let content = std::fs::read(&result.output_path)
        .expect("should be able to read output STEP file");
    assert!(
        !content.is_empty(),
        "output STEP file should be non-empty (OCCT kernel must have fired)"
    );
}
