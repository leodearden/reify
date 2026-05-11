mod common;

/// Behavioral regression pin: `reify build` against a primitive box fixture
/// produces non-empty STEP output through the registered OCCT kernel.
///
/// Passes both before and after the boot-path migration (step-4) because the
/// CLI already includes `reify-kernel-occt` as a direct dep; what changes in
/// step-4 is HOW the kernel is wired (planner-based → inventory-based), not
/// WHETHER it works.  An empty output file after step-4 would indicate the
/// registered factory never fired — this pin catches that regression.
///
/// Skipped via `eprintln!` in stub mode (OCCT_AVAILABLE = false, i.e.
/// cfg(has_occt) not set) so CI logs make the skip visible rather than
/// producing a silent no-op.
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
    let content =
        std::fs::read(&result.output_path).expect("should be able to read output STEP file");
    assert!(
        !content.is_empty(),
        "output STEP file should be non-empty (OCCT kernel must have fired)"
    );
}
