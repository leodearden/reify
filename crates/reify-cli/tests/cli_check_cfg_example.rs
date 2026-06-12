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

/// `--cfg target=linux` resolves the linux `Platform` variant (exit 0), stdout
/// confirms satisfaction, and only the wasm module emits the off-target warning.
#[test]
fn check_cfg_target_linux_selects_linux_platform_variant() {
    let main = common::example_path("conditional_compilation/main.ri");
    let (status, stdout, stderr) =
        common::run_with_args(&["check", "--cfg", "target=linux", &main]);

    assert!(
        status.success(),
        "reify check --cfg target=linux should exit 0 (platform_linux followed, \
         Platform resolves to the linux variant).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied'.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("error:"),
        "stderr should contain no 'error:' diagnostics under target=linux.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("import \"platform_wasm\" not resolved"),
        "stderr should warn that the off-target platform_wasm import is not resolved \
         by this entry point.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("import \"platform_linux\" not resolved"),
        "stderr should NOT warn about platform_linux (it is the on-target import that \
         was followed).\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// `--cfg target=wasm` resolves the wasm `Platform` variant (exit 0), stdout
/// confirms satisfaction, and only the linux module emits the off-target warning.
#[test]
fn check_cfg_target_wasm_selects_wasm_platform_variant() {
    let main = common::example_path("conditional_compilation/main.ri");
    let (status, stdout, stderr) =
        common::run_with_args(&["check", "--cfg", "target=wasm", &main]);

    assert!(
        status.success(),
        "reify check --cfg target=wasm should exit 0 (platform_wasm followed, \
         Platform resolves to the wasm variant).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied'.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("error:"),
        "stderr should contain no 'error:' diagnostics under target=wasm.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("import \"platform_linux\" not resolved"),
        "stderr should warn that the off-target platform_linux import is not resolved \
         by this entry point.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("import \"platform_wasm\" not resolved"),
        "stderr should NOT warn about platform_wasm (it is the on-target import that \
         was followed).\nstdout: {stdout}\nstderr: {stderr}"
    );
}
