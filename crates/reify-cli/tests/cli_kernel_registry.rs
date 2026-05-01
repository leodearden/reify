mod common;

/// Regression pin: `reify_eval::kernel_registry::registry()` visible from inside
/// the CLI's link closure contains the `"occt"` entry when OCCT is available.
///
/// This pin protects against accidental future removal of `reify-kernel-occt`
/// as a direct CLI dependency (which would cause the inventory::submit! to stop
/// firing in the binary's static section and the registry to go empty).
///
/// The assertion passes both before and after the boot-path migration (step-4),
/// because the `[dependencies]` section in `crates/reify-cli/Cargo.toml` already
/// includes `reify-kernel-occt` — the registry is populated regardless of whether
/// the binary uses the planner-based or the inventory-based boot path.
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
        "registry() in the CLI's link closure must contain \"occt\" when \
         reify-kernel-occt is a [dependencies] entry and cfg(has_occt) is set; \
         got keys: {:?}",
        reg.keys().collect::<Vec<_>>()
    );
    assert!(
        reg.len() >= 1,
        "registry() must have at least one entry when OCCT is available"
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
