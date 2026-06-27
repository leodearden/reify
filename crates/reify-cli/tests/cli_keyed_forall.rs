//! End-to-end CLI tests for keyed-sub value-binder forall iteration (task 3933 ε).
//!
//! User-observable leaf signal:
//!   `reify eval examples/keyed_forall.ri` exits 0 and resolves both
//!   member areas (a = 5mm, b = 8mm) without panic.
//!
//! Mirrors cli_keyed_eval.rs (task 3931). RED until step-5 creates
//! examples/keyed_forall.ri.

mod common;

/// Locate the `<lhs> = <rhs>` eval-output line whose LHS names value cell
/// `cell` (either bare `a` or a dotted `…​.a`), returning the trimmed RHS.
/// Reused from cli_keyed_eval.rs.
fn find_cell_rhs<'a>(stdout: &'a str, cell: &str) -> Option<&'a str> {
    stdout.lines().find_map(|l| {
        let (lhs, rhs) = l.split_once('=')?;
        let lhs = lhs.trim();
        if lhs == cell || lhs.ends_with(&format!(".{cell}")) {
            Some(rhs.trim())
        } else {
            None
        }
    })
}

/// `reify eval examples/keyed_forall.ri` must exit 0, resolve `a`
/// (= vents["intake"].area) to 5mm (0.005 m SI) and `b`
/// (= vents["exhaust"].area) to 8mm (0.008 m SI), without panicking.
///
/// RED today: `examples/keyed_forall.ri` does not exist, so the subcommand
/// fails with a file-not-found error and `status.success()` is false.
/// Flips GREEN in step-5 when the example is created.
#[test]
fn eval_keyed_forall_resolves_both_member_areas() {
    let path = common::example_path("keyed_forall.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // (1) No panic / backtrace markers in either stream.
    for (name, stream) in [("stdout", &stdout), ("stderr", &stderr)] {
        assert!(
            !stream.contains("panicked") && !stream.contains("RUST_BACKTRACE"),
            "eval must not panic; found a panic marker in {name}:\n\
             stdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    // (2) Successful exit.
    assert!(
        status.success(),
        "reify eval keyed_forall.ri should exit 0;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // (3) `a` resolves to 5mm (0.005 m in SI).
    let a_rhs = find_cell_rhs(&stdout, "a")
        .unwrap_or_else(|| panic!("no `a = ...` line found in stdout:\n{stdout}"));
    assert!(
        !a_rhs.starts_with("undef"),
        "a (= vents[\"intake\"].area) must resolve, not be undef;\nstdout:\n{stdout}"
    );
    let a_si_token = a_rhs.split_whitespace().next().unwrap_or("");
    let a_si: f64 = a_si_token
        .parse()
        .unwrap_or_else(|_| panic!("a RHS leading token {a_si_token:?} is not f64;\nstdout:\n{stdout}"));
    assert!(
        (a_si - 0.005).abs() < 1e-9,
        "a must be 5mm (0.005 m SI), got {a_si} m;\nstdout:\n{stdout}"
    );

    // (4) `b` resolves to 8mm (0.008 m in SI).
    let b_rhs = find_cell_rhs(&stdout, "b")
        .unwrap_or_else(|| panic!("no `b = ...` line found in stdout:\n{stdout}"));
    assert!(
        !b_rhs.starts_with("undef"),
        "b (= vents[\"exhaust\"].area) must resolve, not be undef;\nstdout:\n{stdout}"
    );
    let b_si_token = b_rhs.split_whitespace().next().unwrap_or("");
    let b_si: f64 = b_si_token
        .parse()
        .unwrap_or_else(|_| panic!("b RHS leading token {b_si_token:?} is not f64;\nstdout:\n{stdout}"));
    assert!(
        (b_si - 0.008).abs() < 1e-9,
        "b must be 8mm (0.008 m SI), got {b_si} m;\nstdout:\n{stdout}"
    );
}
