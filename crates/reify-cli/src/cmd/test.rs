//! `reify test <file>`: runs the module's `@test` entities and reports each
//! verdict with its constraint lines and diagnostics.

use std::io::Write;
use std::process::ExitCode;

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_eval::{TestResult, TestStatus};

use crate::constraint_report::report_eval_output;
use crate::parse_and_compile;

pub(crate) fn cmd_test(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: reify test <file>");
        return ExitCode::FAILURE;
    }

    let compiled = match parse_and_compile(&args[0]) {
        Ok(c) => c,
        Err(code) => return code,
    };

    if compiled
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return ExitCode::FAILURE;
    }

    let results = reify_eval::run_tests(&compiled, || Box::new(SimpleConstraintChecker));

    let mut passed: usize = 0;
    let mut failed: usize = 0;
    let mut indeterminate: usize = 0;

    for result in &results {
        let label = match result.status {
            TestStatus::Pass => {
                passed += 1;
                "PASS"
            }
            TestStatus::Fail => {
                failed += 1;
                "FAIL"
            }
            TestStatus::Indeterminate => {
                indeterminate += 1;
                "INDETERMINATE"
            }
        };
        report_test_result(
            result,
            label,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
    }

    let overall = if failed > 0 { "FAIL" } else { "ok" };
    println!(
        "test result: {}. {} passed; {} failed; {} indeterminate",
        overall, passed, failed, indeterminate
    );

    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Write `result`'s status line to `out` with its constraint lines nested two
/// spaces under it, then its diagnostics to `err`: `report_eval_output`'s
/// rendering and order, so `test` and `check` cannot drift.
fn report_test_result(
    result: &TestResult,
    label: &str,
    out: &mut impl Write,
    err: &mut impl Write,
) {
    let _ = writeln!(out, "  {label}  {}", result.name);
    let mut verdicts = Vec::new();
    let mut diagnostics = Vec::new();
    report_eval_output(
        &result.constraint_results,
        &result.diagnostics,
        &mut verdicts,
        &mut diagnostics,
    );
    for line in String::from_utf8_lossy(&verdicts).lines() {
        let _ = writeln!(out, "  {line}");
    }
    let _ = err.write_all(&diagnostics);
}
