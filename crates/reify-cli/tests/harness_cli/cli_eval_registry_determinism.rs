//! B11 of `docs/prds/v0_6/solver-driver-parity.md` §5: repeated `reify eval`
//! runs, through `SolverRegistry::production()`, are byte-identical.
//!
//! Each run is a separate process, so it draws a fresh hash seed, and uses a
//! fresh `--cache-dir`, so it really solves rather than reading a persisted
//! result. The in-process twin, which sweeps hash seeds across fresh threads,
//! is `crates/reify-constraints/tests/resolution_seam_determinism.rs`.

use crate::common;
use std::process::ExitStatus;

const RUNS: usize = 12;

/// `(status, stdout, stderr)` of `RUNS` cold `reify eval` runs of `fixture`.
fn cold_eval_runs(fixture: &str) -> Vec<(ExitStatus, String, String)> {
    let path = common::fixture_path(fixture);
    (0..RUNS)
        .map(|_| {
            let cache_dir = tempfile::TempDir::new().expect("create a fresh cache dir");
            let cache = cache_dir.path().to_str().expect("utf-8 tempdir path");
            common::run_with_args(&["eval", "--cache-dir", cache, &path])
        })
        .collect()
}

#[test]
fn eval_two_infeasible_components_is_identical_across_runs_and_reports_the_first_declared_component()
 {
    let runs = cold_eval_runs("registry_determinism_two_infeasible.ri");
    let (_, first_stdout, first_stderr) = &runs[0];

    for (run, (status, stdout, stderr)) in runs.iter().enumerate() {
        assert!(
            !status.success(),
            "run {run}: an infeasible model must exit non-zero;\nstdout:\n{stdout}\nstderr:\n{stderr}",
        );
        assert_eq!(
            (stdout, stderr),
            (first_stdout, first_stderr),
            "run {run} diverged from run 0",
        );
    }
    assert!(
        first_stderr.contains("max absolute residual: 5.00e-3"),
        "the reported failure must be a's (the first-declared contradiction, \
         residual 5.00e-3), not b's (5.00e-4);\nstderr:\n{first_stderr}",
    );
}

#[test]
fn eval_multi_component_model_is_identical_across_runs() {
    let runs = cold_eval_runs("registry_determinism_multi_component.ri");
    let (_, first_stdout, _) = &runs[0];

    for (run, (status, stdout, stderr)) in runs.iter().enumerate() {
        assert!(
            status.success(),
            "run {run}: the model must solve;\nstdout:\n{stdout}\nstderr:\n{stderr}",
        );
        assert_eq!(stdout, first_stdout, "run {run} diverged from run 0");
    }
}
