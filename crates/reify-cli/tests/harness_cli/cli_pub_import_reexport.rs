//! A structure reached through a `pub import` facade resolves for `check` and
//! `eval`, and a plain import chain leaks nothing (task #6953, PRD
//! resolution-unification μ, boundary #8). Inputs are the committed fixture
//! dir `tests/fixtures/multi_file_facade/`: `main.ri` imports the facade
//! `lib.ri`, which `pub import`s `parts.ri`; `plain_main.ri` imports
//! `plain_lib.ri`, which imports `parts.ri` without `pub`.

use crate::common;

fn facade_fixture(name: &str) -> String {
    common::fixture_path(&format!("multi_file_facade/{name}"))
}

#[test]
fn check_resolves_a_structure_through_a_pub_import_facade() {
    let (status, stdout, stderr) = common::run_with_args(&["check", &facade_fixture("main.ri")]);
    assert!(
        status.success(),
        "check should succeed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("unknown structure"),
        "Pulley should resolve through lib's `pub import parts`.\nstderr: {stderr}"
    );
}

#[test]
fn eval_prints_a_structure_reached_through_a_pub_import_facade() {
    let (status, stdout, stderr) = common::run_with_args(&["eval", &facade_fixture("main.ri")]);
    assert!(
        status.success(),
        "eval should succeed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Rig.p = Pulley {"),
        "eval should instantiate the re-exported Pulley.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn check_does_not_leak_through_a_plain_import_chain() {
    let (status, stdout, stderr) =
        common::run_with_args(&["check", &facade_fixture("plain_main.ri")]);
    assert!(
        !status.success(),
        "check should fail: a plain import re-exports nothing.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("unknown structure \"Pulley\""),
        "check should report Pulley unknown.\nstdout: {stdout}\nstderr: {stderr}"
    );
}
