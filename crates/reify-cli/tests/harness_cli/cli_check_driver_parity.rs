//! CLI signal for solver-driver parity: PRD
//! `docs/prds/v0_6/solver-driver-parity.md` §5 B1, B3 and B6. `reify check`
//! and `reify build` resolve autos the way `reify eval` does, so an
//! auto-bearing design gets a definite verdict from every driver.
//!
//! - B1: a constructor-argument auto pinned by a constant equality is
//!   Satisfied, so `--strict` passes it too.
//! - B3: an auto pinned by an equality against a derived let is Satisfied at
//!   its ~6e-16 solve residual, not reported violated.
//! - B6: two equalities that pin one auto to different values are an
//!   infeasible solve, which fails both drivers; build still writes the file.

use std::process::ExitStatus;

use crate::common;

const AUTO_CTL: &str = "tests/prd-gate/fixtures/driver_parity_auto_ctl.ri";
const DERIVED_EQUALITY: &str = "auto_derived_equality.ri";
const INFEASIBLE_PAIR: &str = "infeasible_equality_pair.ri";

const INFEASIBLE_RESIDUAL: &str = "max absolute residual: 5.00e-3";

fn auto_ctl_path() -> String {
    common::workspace_root()
        .join(AUTO_CTL)
        .to_string_lossy()
        .into_owned()
}

fn check(args: &[&str]) -> (ExitStatus, String, String) {
    let mut argv = vec!["check"];
    argv.extend_from_slice(args);
    common::run_with_args(&argv)
}

#[test]
fn check_satisfies_a_constant_pinned_constructor_auto() {
    let (status, stdout, stderr) = check(&[&auto_ctl_path()]);
    assert!(
        status.success(),
        "B1: `reify check` must exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("OK AutoCtl#constraint[0]"),
        "B1: the pinning equality must be Satisfied.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "B1: the summary must report every constraint satisfied.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("INDETERMINATE"),
        "B1: the auto is resolved, so nothing is Indeterminate.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn check_strict_passes_a_constant_pinned_constructor_auto() {
    let (status, stdout, stderr) = check(&["--strict", &auto_ctl_path()]);
    assert!(
        status.success(),
        "B1: `reify check --strict` must exit 0 once the auto resolves.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("Strict check failed"),
        "B1: no constraint is Indeterminate, so strict mode has nothing to fail on.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn build_satisfies_a_constant_pinned_constructor_auto() {
    let result = common::run_build_at(&auto_ctl_path());
    assert!(
        result.status.success(),
        "B1: `reify build` must exit 0.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stdout.contains("OK AutoCtl#constraint[0]"),
        "B1: the pinning equality must be Satisfied.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        !result.stdout.contains("INDETERMINATE"),
        "B1: the auto is resolved, so nothing is Indeterminate.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.output_path.exists(),
        "B1: build must write the output file.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
}

#[test]
fn check_satisfies_an_auto_pinned_to_a_derived_let() {
    let (status, stdout, stderr) = check(&[&common::fixture_path(DERIVED_EQUALITY)]);
    assert!(
        status.success(),
        "B3: `reify check` must exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("OK DerivedEq#constraint[0]"),
        "B3: the equality must be Satisfied at its solve residual.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "B3: the summary must report every constraint satisfied.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("violated") && !stderr.contains("violated"),
        "B3: a ~6e-16 residual must not be reported as a violation.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn check_fails_an_infeasible_equality_pair() {
    let (status, stdout, stderr) = check(&[&common::fixture_path(INFEASIBLE_PAIR)]);
    assert!(
        !status.success(),
        "B6: `reify check` must exit non-zero on an infeasible solve.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains(INFEASIBLE_RESIDUAL),
        "B6: stderr must report the infeasible solve's residual.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn check_strict_fails_an_infeasible_equality_pair() {
    let (status, stdout, stderr) = check(&["--strict", &common::fixture_path(INFEASIBLE_PAIR)]);
    assert!(
        !status.success(),
        "B6: `reify check --strict` must exit non-zero.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Strict check failed"),
        "B6: the unresolved auto leaves its constraints Indeterminate, which strict \
         mode fails on.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn build_fails_an_infeasible_equality_pair_but_writes_the_file() {
    let result = common::run_build(INFEASIBLE_PAIR);
    assert!(
        !result.status.success(),
        "B6: `reify build` must exit non-zero on an infeasible solve.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.output_path.exists(),
        "B6: the geometry does not read the auto, so build still writes the file.\n\
         stdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stderr.contains(INFEASIBLE_RESIDUAL),
        "B6: stderr must report the infeasible solve's residual.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
}
