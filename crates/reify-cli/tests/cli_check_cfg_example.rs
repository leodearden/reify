//! CLI integration gate for `reify check --cfg` on the committed CI example
//! (Task ε / PRD docs/prds/v0_6/conditional-compilation.md §2 Slice E / §8 Task ε).
//!
//! Exercises the user-observable end-to-end signal from the committed
//! `examples/conditional_compilation/` directory: two cfg-gated imports of sibling
//! modules that each define a SAME-named `Platform` structure differently.  Under
//! `--cfg target=linux` the linux variant resolves; under `--cfg target=wasm` the
//! wasm variant resolves.  Both targets exit 0 (symmetric two-way table, PRD §6).
//!
//! The off-target selection signal: `warning: import "<module>" not resolved by
//! this entry point` is present for the gated-out module and absent for the
//! on-target module — proving the platform-correct variant was followed and the
//! other platform module is absent from the DAG.

mod common;

/// Assert the two-way platform-selection signal for one `--cfg` target.
///
/// Verifies that `reify check --cfg <cfg>` on the committed CI example:
/// - exits 0 (`on_module` followed, the platform-correct `Platform` variant resolves),
/// - reports `"All constraints satisfied"` on stdout,
/// - emits no `"error:"` diagnostics on stderr,
/// - warns that `off_module` was not resolved by this entry point (gated out of DAG),
/// - does NOT warn that `on_module` was not resolved (it is the on-target import).
///
/// The `on_module` / `off_module` contrast makes the symmetric two-way table explicit:
/// both targets exit 0 but with opposite gating-out warnings, proving the DAG selected
/// the platform-correct variant in each case.
fn assert_cfg_selects(cfg: &str, on_module: &str, off_module: &str) {
    let main = common::example_path("conditional_compilation/main.ri");
    let (status, stdout, stderr) =
        common::run_with_args(&["check", "--cfg", cfg, &main]);

    assert!(
        status.success(),
        "reify check --cfg {cfg} should exit 0 ({on_module} followed, \
         Platform resolves to the on-target variant).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied'.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("error:"),
        "stderr should contain no 'error:' diagnostics under {cfg}.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains(&format!("import \"{off_module}\" not resolved")),
        "stderr should warn that the off-target {off_module} import is not resolved \
         by this entry point.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains(&format!("import \"{on_module}\" not resolved")),
        "stderr should NOT warn about {on_module} (it is the on-target import that \
         was followed).\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// `--cfg target=linux` resolves the linux `Platform` variant (exit 0), stdout
/// confirms satisfaction, and only the wasm module emits the off-target warning.
#[test]
fn check_cfg_target_linux_selects_linux_platform_variant() {
    assert_cfg_selects("target=linux", "platform_linux", "platform_wasm");
}

/// `--cfg target=wasm` resolves the wasm `Platform` variant (exit 0), stdout
/// confirms satisfaction, and only the linux module emits the off-target warning.
#[test]
fn check_cfg_target_wasm_selects_wasm_platform_variant() {
    assert_cfg_selects("target=wasm", "platform_wasm", "platform_linux");
}
