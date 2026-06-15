//! CLI integration tests for `reify check --cfg` (Task δ / Slice D).
//!
//! These exercise the end-to-end driver plumbing: `reify check` parses repeated
//! `--cfg key=value` / `--cfg flag` arguments, builds the active `CfgSet` (target
//! host-defaulted, overridable by `--cfg target=`), and threads it into the
//! cfg-gated user-import DAG so task γ's import gating fires.
//!
//! User-observable signal: an entry referencing a pub name defined ONLY in a
//! `#cfg(target = "linux")`-gated import resolves under `--cfg target=linux`
//! (exit 0) and fails to resolve under `--cfg target=wasm` (an unresolved-name
//! `error:` diagnostic → exit non-zero).

mod common;

use std::fs;
use std::path::Path;

/// Write a two-way cfg-gated project into `dir` and return the `main.ri` path.
///
/// `main.ri` imports `platform_linux` under `#cfg(target = "linux")` and
/// `platform_wasm` under `#cfg(target = "wasm")`, then references `LinuxOnly`
/// (a pub structure defined ONLY in `platform_linux`) in **type position**
/// (`param marker : LinuxOnly`). A type-position reference is used because an
/// unknown **type** is a hard `unresolved type` compile error — the signal these
/// tests need.  Note: task 4528 added compile-time validation of sub
/// `structure_name`s too, so a `sub w = LinuxOnly(...)` form would also emit an
/// error when the import is inactive — but this fixture uses no `sub` and the
/// type-position form is the cfg-import-gating signal.
fn write_two_way_project(dir: &Path) -> String {
    let entry_src = "#cfg(target = \"linux\")\nimport platform_linux\n\
                     #cfg(target = \"wasm\")\nimport platform_wasm\n\
                     \n\
                     structure def Entry {\n\
                     \x20   param marker : LinuxOnly\n\
                     }\n";
    fs::write(dir.join("main.ri"), entry_src).unwrap();
    fs::write(
        dir.join("platform_linux.ri"),
        "pub structure def LinuxOnly { param x: Real }\n",
    )
    .unwrap();
    fs::write(
        dir.join("platform_wasm.ri"),
        "pub structure def WasmOnly { param x: Real }\n",
    )
    .unwrap();
    dir.join("main.ri").to_str().unwrap().to_string()
}

/// Write a host-default cfg-gated project into `dir` and return the `main.ri`
/// path.
///
/// `main.ri` imports `platform_host` gated on `#cfg(target = "<host>")` (where
/// `host` is the compiling host's platform string) and references `HostOnly`
/// (defined ONLY in `platform_host`) in type position. With no `--cfg` flag,
/// `reify check` must seed the host default (`target = std::env::consts::OS`),
/// follow the import, and resolve `HostOnly`.
fn write_host_default_project(dir: &Path, host: &str) -> String {
    let entry_src = format!(
        "#cfg(target = \"{host}\")\nimport platform_host\n\
         \n\
         structure def Entry {{\n\
         \x20   param marker : HostOnly\n\
         }}\n"
    );
    fs::write(dir.join("main.ri"), entry_src).unwrap();
    fs::write(
        dir.join("platform_host.ri"),
        "pub structure def HostOnly { param x: Real }\n",
    )
    .unwrap();
    dir.join("main.ri").to_str().unwrap().to_string()
}

/// (a) Two-way signal: `--cfg target=linux` follows `platform_linux` so
/// `LinuxOnly` resolves (exit 0); `--cfg target=wasm` gates it out so `LinuxOnly`
/// is unresolved (an `error:` diagnostic referencing it → exit non-zero).
#[test]
fn check_cfg_target_linux_resolves_but_wasm_gates_out() {
    let tmp = tempfile::tempdir().unwrap();
    let main_path = write_two_way_project(tmp.path());

    let (status, stdout, stderr) =
        common::run_with_args(&["check", "--cfg", "target=linux", &main_path]);
    assert!(
        status.success(),
        "reify check --cfg target=linux should exit 0 (platform_linux followed, \
         LinuxOnly resolves).\nstdout: {stdout}\nstderr: {stderr}"
    );

    let (status, stdout, stderr) =
        common::run_with_args(&["check", "--cfg", "target=wasm", &main_path]);
    assert!(
        !status.success(),
        "reify check --cfg target=wasm should exit non-zero (platform_linux gated \
         out, LinuxOnly unresolved).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain an 'error:' diagnostic for the unresolved name, \
         got: {stderr}"
    );
    assert!(
        stderr.contains("LinuxOnly"),
        "stderr should reference the unresolved name 'LinuxOnly', got: {stderr}"
    );
}

/// (b) Host default: with no `--cfg`, the active target defaults to the host
/// platform (`std::env::consts::OS`), so an import gated on that target is
/// followed and the entry resolves (exit 0).
#[test]
fn check_no_cfg_uses_host_default_target() {
    let host = std::env::consts::OS;
    let tmp = tempfile::tempdir().unwrap();
    let main_path = write_host_default_project(tmp.path(), host);

    let (status, stdout, stderr) = common::run_with_args(&["check", &main_path]);
    assert!(
        status.success(),
        "reify check (no --cfg) should exit 0 via the host-default target '{host}' \
         (platform_host followed, HostOnly resolves).\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// (c) Flag error: `--cfg` with no following value is a usage error (exit
/// non-zero) explaining the flag requires a value.
#[test]
fn check_cfg_missing_value_exits_failure_with_usage() {
    let (status, _stdout, stderr) = common::run_with_args(&["check", "--cfg"]);
    assert!(
        !status.success(),
        "reify check --cfg with no value should exit non-zero.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("requires a value"),
        "stderr should explain that --cfg requires a value, got: {stderr}"
    );
}

/// (c) Flag error: a malformed `--cfg` value (`=bad`, empty key) fails before
/// compilation (exit non-zero) with an empty-key message.
#[test]
fn check_cfg_malformed_value_exits_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let main_path = write_two_way_project(tmp.path());

    let (status, _stdout, stderr) = common::run_with_args(&["check", "--cfg", "=bad", &main_path]);
    assert!(
        !status.success(),
        "reify check --cfg =bad should exit non-zero (empty key).\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("empty key"),
        "stderr should explain the empty key, got: {stderr}"
    );
}
