//! `reify check` fails on a constraint whose pure operation refuses defined
//! plain-data operands (task 10027).
//!
//! Such a constraint is Indeterminate on every run, so it is reported as an
//! error naming the refusing operation, never as a definite VIOLATED. Drives
//! the binary directly: this harness's seam rule forbids `tests/common` helpers.

use std::process::{Command, Output};

/// Write `module <stem>` + `body` to `<stem>.ri` (the stem must match the module,
/// or E_MODULE_PATH_MISMATCH fires) and run `reify check` on it with `extra_args`.
fn check_module(stem: &str, body: &str, extra_args: &[&str]) -> Output {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join(format!("{stem}.ri"));
    std::fs::write(&path, format!("module {stem}\n\n{body}\n")).expect("failed to write module");
    Command::new(reify_test_support::test_package::bin_exe("reify"))
        .arg("check")
        .arg(&path)
        .args(extra_args)
        .output()
        .expect("failed to spawn reify")
}

#[test]
fn check_fails_on_a_pure_operation_refusal() {
    let output = check_module(
        "op_refused_r1",
        "structure def Root {\n    param x : Real = 0.0\n    param y : Real = -1.0\n    \
         constraint 1.0 / x > 0.0\n    constraint sqrt(y) > 0.0\n}",
        &[],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let context = format!("\nstdout:\n{stdout}\nstderr:\n{stderr}");

    for line in [
        "INDETERMINATE Root#constraint[0]: `/` is undefined for these operands: Real, Real",
        "INDETERMINATE Root#constraint[1]: `sqrt` is undefined for these operands: Real",
    ] {
        assert!(
            stdout.contains(line),
            "expected `{line}` on stdout;{context}"
        );
    }
    assert!(
        !stdout.contains("VIOLATED"),
        "a refusal is never a definite VIOLATED;{context}"
    );
    assert!(
        stderr.contains("error: constraint Root#constraint[0] indeterminate:"),
        "expected the refusal as an error diagnostic;{context}"
    );
    assert!(
        !output.status.success(),
        "a pure refusal must fail the check: {:?};{context}",
        output.status
    );
}

/// `==` and `!=` refuse dimension-incomparable operands exactly as `>` does,
/// so none of the three reports a definite verdict.
#[test]
fn equality_refuses_dimension_incomparable_operands_like_ordering() {
    let output = check_module(
        "op_refused_r2",
        "purpose wide_enough(subject : Structure) {\n    \
         constraint subject.width > 0mm\n    \
         constraint subject.width == 0mm\n    \
         constraint subject.width != 0mm\n}\n\n\
         structure def Plate {\n    param width : Real = 5.0\n}",
        &["--purpose", "wide_enough=Plate"],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let context = format!("\nstdout:\n{stdout}\nstderr:\n{stderr}");

    for (index, operator) in [">", "==", "!="].into_iter().enumerate() {
        let line = format!(
            "INDETERMINATE purpose:wide_enough@Plate#constraint[{index}]: \
             `{operator}` is undefined for these operands: Real, Scalar<m>"
        );
        assert!(
            stdout.contains(&line),
            "expected `{line}` on stdout;{context}"
        );
    }
    assert!(
        !stdout.contains("VIOLATED"),
        "a refusal is never a definite VIOLATED;{context}"
    );
    assert!(
        !stdout.contains("OK purpose:wide_enough@Plate"),
        "a refusal is never a definite OK;{context}"
    );
    assert!(
        stderr.contains("error: constraint purpose:wide_enough@Plate#constraint[1] indeterminate:"),
        "expected the `==` refusal as an error diagnostic;{context}"
    );
    assert!(
        !output.status.success(),
        "a pure refusal must fail the check: {:?};{context}",
        output.status
    );
}
