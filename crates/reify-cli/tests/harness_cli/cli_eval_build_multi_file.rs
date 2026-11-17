//! `reify eval` and `reify build` compile the entry as a program (task #5517,
//! PRD resolution-unification.md γ): a plain `import` reaches its pub
//! structures, and repeated `--cfg` selects `#cfg`-gated platform modules
//! exactly as `reify check` does, and an import of a missing module fails both
//! commands. Inputs are the committed fixture dirs `tests/fixtures/multi_file/`,
//! `tests/fixtures/multi_file_cfg/` and `tests/fixtures/multi_file_missing_import/`.

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
    for (target, size, expected_code) in [("linux", "0.01 m", 0), ("wasm", "0.02 m", 1)] {
        let cfg = format!("target={target}");
        let (status, stdout, stderr) = common::run_with_args(&["eval", "--cfg", &cfg, &main]);
        assert_eq!(
            status.code(),
            Some(expected_code),
            "--cfg {cfg}: eval exited {status}, expected exit code {expected_code}.\nstdout: {stdout}\nstderr: {stderr}"
        );
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

#[test]
fn eval_and_build_fail_on_an_import_of_a_missing_module() {
    let main = common::fixture_path("multi_file_missing_import/main.ri");
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("out.step");
    let out_arg = out.to_str().expect("utf-8 tempdir");
    for args in [
        vec!["eval", main.as_str()],
        vec!["build", main.as_str(), "-o", out_arg],
    ] {
        let (status, stdout, stderr) = common::run_with_args(&args);
        assert!(
            status.code() == Some(1) && stderr.contains("module 'nosuch' not found"),
            "{args:?} must fail on the missing import.\nstatus: {status}\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
    assert!(
        !out.exists(),
        "build wrote geometry despite the missing import"
    );
}
