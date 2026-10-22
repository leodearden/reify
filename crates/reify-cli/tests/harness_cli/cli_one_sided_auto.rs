//! CLI leaf signal for the one-sided strict-auto refusal (task #6692, P1-ε):
//! PRD `docs/prds/v0_6/solver-driver-parity.md` §5 B4 says `reify check` exits
//! nonzero on a strict auto bounded on one side only, and no driver reports
//! the 10 m default-box edge. B5 and the F2 auto(free) twin are the
//! discriminating negatives.

use crate::common;

fn prd_gate_fixture(name: &str) -> String {
    common::workspace_root()
        .join("tests/prd-gate/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn check_refuses_the_one_sided_strict_auto() {
    let (status, stdout, stderr) =
        common::run_subcommand("check", &prd_gate_fixture("driver_parity_one_sided_auto.ri"));
    let combined = format!("{stdout}{stderr}");
    assert!(
        !status.success(),
        "B4: `reify check` must exit nonzero on a one-sided strict auto.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        combined.contains("no constraint bounds `OneSided.rod.length` above"),
        "B4: the refusal must name the missing upper bound.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !combined.contains("OneSided.rod.length = 10 m"),
        "B4: no driver may report the 10 m default edge.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn check_accepts_the_two_sided_strict_auto() {
    let (status, stdout, stderr) =
        common::run_subcommand("check", &prd_gate_fixture("driver_parity_two_sided_auto.ri"));
    assert!(
        status.success(),
        "B5: `reify check` must exit 0 on a two-sided strict auto.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !format!("{stdout}{stderr}").contains("not uniquely determined"),
        "B5: no refusal text.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn eval_resolves_the_two_sided_strict_auto_to_the_centre() {
    let (status, stdout, stderr) =
        common::run_subcommand("eval", &prd_gate_fixture("driver_parity_two_sided_auto.ri"));
    assert!(
        status.success(),
        "B5: `reify eval` must succeed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("TwoSided.rod.length = 0.01 m"),
        "B5: the two-sided auto resolves to the centre of [8mm, 12mm].\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn check_accepts_the_one_sided_auto_free_twin() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &prd_gate_fixture("driver_parity_one_sided_auto_free.ri"),
    );
    assert!(
        status.success(),
        "F2: `reify check` must exit 0 on a one-sided auto(free).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !format!("{stdout}{stderr}").contains("no constraint bounds"),
        "F2: auto(free) is never refused.\nstdout: {stdout}\nstderr: {stderr}"
    );
}
