//! ζ §11.2 row #4 "CLI/GUI/binary surface" — real-binary checker-injection smoke.
//!
//! Proves that the REAL `reify` binary (reify-cli/src/main.rs) injects
//! `SimpleConstraintChecker` into the compile path, not just the test harness.
//!
//! Two tests, mirroring the compile-time split from the e2e harness:
//!
//! (a) `check_bearing_constraint_select_real_checker_exits_success`:
//!     `reify check examples/auto/bearing_constraint_select.ri` exits 0.
//!     Under the STUB checker this would be an Ambiguous Error (exit FAILURE);
//!     under the REAL checker ThinSeal is the unique feasible survivor (Selected)
//!     → no Error diagnostic → exit 0.  Proves β-inject is wired in the binary.
//!
//! (b) `check_bearing_unsat_exits_failure_with_no_candidate_message`:
//!     `reify check examples/auto/bearing_unsat.ri` exits FAILURE.
//!     stderr contains "rejected by constraint" — the CLI surface of the new
//!     E_AUTO_TYPE_PARAM_NO_CANDIDATE diagnostic.

mod common;

/// ζ §11.2 row #4 — real-binary proves SimpleConstraintChecker is injected.
///
/// Under the stub checker `bearing_constraint_select.ri` would emit
/// E_AUTO_TYPE_PARAM_AMBIGUOUS (both Seal candidates stub-feasible) and exit
/// FAILURE.  Under the real checker only ThinSeal satisfies
/// `seal.thickness < bore_radius`, so no Error is emitted → exit 0.
#[test]
fn check_bearing_constraint_select_real_checker_exits_success() {
    let path = common::example_path("auto/bearing_constraint_select.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check bearing_constraint_select.ri must exit 0 under the real checker \
         (ThinSeal is the unique feasible survivor — stub would emit Ambiguous/exit-FAILURE).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("multiple feasible"),
        "stderr must NOT contain 'multiple feasible' (Ambiguous message) — real checker \
         selected the unique survivor ThinSeal; got stderr: {stderr}"
    );
    // No compile-time error for AutoTypeParamAmbiguous means the binary's
    // compile_entry_with_stdlib_cfg_checked call at main.rs used the real checker.
    assert!(
        !stderr.contains("error: auto type parameter"),
        "stderr must NOT contain an auto-type-param error under the real checker; \
         got stderr: {stderr}"
    );
}

/// ζ §11.2 row #4 — CLI surface of E_AUTO_TYPE_PARAM_NO_CANDIDATE.
///
/// `bearing_unsat.ri` has two Seal candidates (ThickSeal=5mm, HugeSeal=8mm)
/// that both violate `seal.thickness < bore_radius=3mm`.  The real binary must:
///   (a) exit FAILURE (Error diagnostic in the compiled module)
///   (b) surface the NoCandidate message naming the violated constraint
///       ("rejected by constraint") in stderr.
#[test]
fn check_bearing_unsat_exits_failure_with_no_candidate_message() {
    let path = common::example_path("auto/bearing_unsat.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        !status.success(),
        "reify check bearing_unsat.ri must exit FAILURE \
         (both Seal candidates violate seal.thickness < bore_radius=3mm → NoCandidate Error).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("rejected by constraint"),
        "stderr must contain 'rejected by constraint' — the NoCandidate message names each \
         candidate's violated constraint; got stderr: {stderr}"
    );
}
