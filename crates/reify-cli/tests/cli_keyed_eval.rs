//! End-to-end CLI tests for keyed-sub evaluation (task 3931 γ).
//!
//! User-observable leaf signal:
//!   `reify eval examples/keyed_vents.ri` resolves `vents["intake"].area` to 5mm.
//! And the missing-key fixture fails cleanly — a named diagnostic + Undef,
//! never a panic (spec §3.4).

mod common;

/// Locate the `<lhs> = <rhs>` eval-output line whose LHS names value cell
/// `cell` (either bare `a` or a dotted `…​.a`), returning the trimmed RHS.
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

/// Success path: `reify eval examples/keyed_vents.ri` exits 0 and resolves the
/// `a` cell (`= vents["intake"].area`) to 5mm (0.005 m in SI), proving keyed
/// member access evaluates end-to-end through the CLI.
#[test]
fn eval_keyed_vents_resolves_member_by_key() {
    let path = common::example_path("keyed_vents.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval keyed_vents.ri should exit 0;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let rhs = find_cell_rhs(&stdout, "a")
        .unwrap_or_else(|| panic!("no `a = ...` line found in stdout:\n{stdout}"));
    assert!(
        !rhs.starts_with("undef"),
        "a (= vents[\"intake\"].area) must resolve, not be undef;\nstdout:\n{stdout}"
    );
    let si_token = rhs.split_whitespace().next().unwrap_or("");
    let si: f64 = si_token
        .parse()
        .unwrap_or_else(|_| panic!("RHS leading token {si_token:?} is not f64;\nstdout:\n{stdout}"));
    assert!(
        (si - 0.005).abs() < 1e-9,
        "a must be 5mm (0.005 m in SI), got {si} m;\nstdout:\n{stdout}"
    );
}

/// Missing-key path: `reify eval <fixture>` must NOT panic. The run terminates
/// cleanly and the missing key is surfaced — either a named compile diagnostic
/// or the `ghost` cell as undef — while the valid `a` still resolves.
#[test]
fn eval_keyed_missing_key_fails_cleanly_without_panic() {
    let path = common::fixture_path("keyed_missing_key.ri");
    let (_status, stdout, stderr) = common::run_subcommand("eval", &path);

    // (1) No panic / backtrace markers in either stream.
    for (name, stream) in [("stdout", &stdout), ("stderr", &stderr)] {
        assert!(
            !stream.contains("panicked") && !stream.contains("RUST_BACKTRACE"),
            "missing-key eval must not panic; found a panic marker in {name}:\n\
             stdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    // (2) The missing key is surfaced: the named diagnostic and/or an undef
    //     ghost cell. (Exact exit status is not asserted — the contract is a
    //     clean, no-panic failure, not a specific code.)
    let surfaced = stderr.contains("no keyed member 'ghost'")
        || stdout.contains("no keyed member 'ghost'")
        || find_cell_rhs(&stdout, "ghost").is_some_and(|rhs| rhs.starts_with("undef"));
    assert!(
        surfaced,
        "missing key must be surfaced (named diagnostic or undef ghost cell);\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
