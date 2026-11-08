//! `reify eval` and `reify build` compile the entry as a program (task #5517,
//! PRD resolution-unification.md γ): a plain `import` reaches its pub
//! structures, and repeated `--cfg` selects `#cfg`-gated platform modules
//! exactly as `reify check` does. Inputs are the committed fixture dirs
//! `tests/fixtures/multi_file/` and `tests/fixtures/multi_file_cfg/`.

use crate::common;

const UNRESOLVED_IMPORT_WARNING: &str = "not resolved by this entry point";

#[test]
fn eval_prints_an_imported_structure_instance() {
    let (status, stdout, stderr) =
        common::run_with_args(&["eval", &common::fixture_path("multi_file/main.ri")]);
    assert!(
        status.success(),
        "eval should succeed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    for expected in ["Rig.p = Pulley {", "Rig.p.diameter = 0.04 m"] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?}.\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
    assert!(
        !stderr.contains(UNRESOLVED_IMPORT_WARNING),
        "import left unresolved.\nstderr: {stderr}"
    );
}

#[test]
fn build_writes_geometry_for_an_importing_entry() {
    let out = common::run_build("multi_file/main.ri");
    assert!(
        out.status.success(),
        "build should succeed.\nstdout: {}\nstderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(
        out.output_path.exists(),
        "build wrote no geometry.\nstderr: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains(UNRESOLVED_IMPORT_WARNING),
        "import left unresolved.\nstderr: {}",
        out.stderr
    );
}

#[test]
fn eval_cfg_target_selects_the_platform_module() {
    let main = common::fixture_path("multi_file_cfg/main.ri");
    for (target, size) in [("linux", "0.01 m"), ("wasm", "0.02 m")] {
        let cfg = format!("target={target}");
        let (_status, stdout, stderr) = common::run_with_args(&["eval", "--cfg", &cfg, &main]);
        let expected = format!("Rig.housing.size = {size}");
        assert!(
            stdout.contains(&expected),
            "--cfg {cfg}: missing {expected:?}.\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

#[test]
fn build_cfg_target_selects_the_platform_module_as_check_does() {
    let main = common::fixture_path("multi_file_cfg/main.ri");
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("out.step");
    let out_arg = out.to_str().expect("utf-8 tempdir");
    for (target, verdict) in [
        ("linux", "OK Rig#constraint[0]"),
        ("wasm", "VIOLATED Rig#constraint[0]"),
    ] {
        let cfg = format!("target={target}");
        let (_status, check_stdout, check_stderr) =
            common::run_with_args(&["check", "--cfg", &cfg, &main]);
        assert!(
            check_stdout.contains(verdict),
            "check --cfg {cfg}: missing {verdict:?}.\nstdout: {check_stdout}\nstderr: {check_stderr}"
        );
        let _ = std::fs::remove_file(&out);
        let (_status, stdout, stderr) =
            common::run_with_args(&["build", "--cfg", &cfg, &main, "-o", out_arg]);
        assert!(
            stdout.contains(verdict),
            "build --cfg {cfg}: missing {verdict:?} (check's verdict).\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            out.exists(),
            "build --cfg {cfg} wrote no geometry.\nstderr: {stderr}"
        );
    }
}
