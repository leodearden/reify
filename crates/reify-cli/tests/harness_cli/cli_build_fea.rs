/// Integration tests: FEA compute trampoline registration in `reify build` and
/// cmd_check posture lock.
///
/// ## Why these tests exist
///
/// `cmd_build` previously constructed its engine via
/// `Engine::with_registered_kernel(checker)` without wrapping in
/// `configured_eval_engine`, so it omitted `register_compute_fns` and
/// `register_shell_extract_compute_fns`.  Without those registrations:
///
///  (a) The engine emits an `Error`-severity diagnostic
///      "@optimized target ...: no registered compute trampoline (falling back to
///      body-inlining)" on stderr;
///  (b) `result.max_von_mises` receives the body-inline fallback → `Undef`, so
///      FEA-result constraints evaluate to `Indeterminate` and the build silently
///      exits 0 regardless of the actual stress;
///  (c) `cmd_build`'s exit code ignored `Severity::Error` diagnostics (driven
///      only by constraint outcome + geometry presence), causing `Error + exit 0`.
///
/// After the fix (task 4458):
///  - `cmd_build` wraps engine construction in `configured_eval_engine` → trampolines
///    registered → no trampoline-error diagnostic, `result.max_von_mises` has a real
///    value, and a violated FEA-result constraint exits non-zero.
///  - `cmd_build` also gates exit on `Severity::Error` diagnostics (matching `cmd_eval`).
///  - `cmd_check` is DELIBERATELY left as-is: lightweight, no compute trampolines,
///    FEA-result constraints remain Indeterminate under check (posture-lock test (3)).
///
/// ## OCCT independence
///
/// The FEA solver is pure-Rust (reify-solver-elastic) and OCCT-independent.
/// `report_eval_output` prints constraint status lines BEFORE the geometry-output
/// branch, so `VIOLATED`/`OK` lines appear in stdout whether or not OCCT realizes
/// geometry.  Tests (1) and (2) therefore hold unconditionally.
use crate::common;

/// (1) RED before fix, GREEN after: `reify build examples/fea_cantilever_smoke.ri`
/// must not emit the trampoline-missing error on stderr.
///
/// The smoke file carries an `@optimized("solver::elastic_static")` annotated
/// solve.  Before the fix, `cmd_build` emits:
///   "no registered compute trampoline (falling back to body-inlining)"
/// on stderr as a `Severity::Error` diagnostic.  After the fix (trampoline
/// registered via `configured_eval_engine`), the dispatch succeeds silently.
///
/// Positive guard: `stdout.contains("Wrote")` confirms geometry was exported
/// (not just that no error appeared).  The smoke fixture has no constraint
/// violations, so the build exits 0.
#[test]
fn build_fea_cantilever_emits_no_trampoline_error() {
    let path = common::example_path("fea_cantilever_smoke.ri");
    let result = common::run_build_at(&path);

    assert!(
        !result.stderr.contains("no registered compute trampoline"),
        "reify build should not emit the trampoline-missing error once \
         configured_eval_engine registers compute fns.\nstderr:\n{}",
        result.stderr
    );

    assert!(
        result.status.success(),
        "reify build fea_cantilever_smoke.ri should exit 0 (no constraints violated).\n\
         stdout:\n{}\nstderr:\n{}",
        result.stdout,
        result.stderr
    );

    assert!(
        result.stdout.contains("Wrote"),
        "stdout should contain 'Wrote' confirming geometry was exported.\n\
         stdout:\n{}\nstderr:\n{}",
        result.stdout,
        result.stderr
    );
}

/// (2) RED before fix, GREEN after: `reify build fea_cantilever_violated.ri`
/// must exit non-zero with `VIOLATED` in stdout and `OK` in stdout.
///
/// The fixture has:
///   `constraint peak_stress < 1MPa`    — deliberately VIOLATED (~5.14 MPa)
///   `constraint peak_stress < 100MPa`  — SATISFIED
///
/// Before the fix (no trampoline): both constraints are Undef → INDETERMINATE,
/// stdout shows INDETERMINATE, exit is 0.
///
/// After the fix: `result.max_von_mises` is a real value; the `< 1 MPa`
/// constraint evaluates to VIOLATED and the build exits non-zero via the
/// existing `SomeViolated → ExitCode::FAILURE` path.  The simultaneous
/// `OK` for `< 100 MPa` proves the FEA solve produced a real numeric value
/// (a blanket Indeterminate path would show INDETERMINATE for both).
#[test]
fn build_fea_violated_constraint_exits_nonzero() {
    let result = common::run_build("fea_cantilever_violated.ri");

    assert!(
        result.stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED' — the < 1 MPa FEA constraint must \
         evaluate against a real max_von_mises value.\nstdout:\n{}\nstderr:\n{}",
        result.stdout,
        result.stderr
    );

    assert!(
        result.stdout.contains("  OK "),
        "stdout should contain '  OK ' — the < 100 MPa constraint must be \
         satisfied, proving real numeric evaluation (not blanket Indeterminate).\n\
         stdout:\n{}\nstderr:\n{}",
        result.stdout,
        result.stderr
    );

    assert!(
        !result.status.success(),
        "reify build fea_cantilever_violated.ri should exit non-zero when the \
         FEA-result constraint is violated.\nstdout:\n{}\nstderr:\n{}",
        result.stdout,
        result.stderr
    );

    assert!(
        !result.stderr.contains("no registered compute trampoline"),
        "trampoline-missing error must not appear on stderr after the fix.\n\
         stderr:\n{}",
        result.stderr
    );
}

/// (3) Posture-lock: `reify check fea_cantilever_violated.ri` exits 0 with
/// INDETERMINATE — cmd_check deliberately does NOT register compute trampolines.
///
/// This test is GREEN before AND after the fix.  It encodes the deliberate
/// contrast: `reify build` gates on FEA results; `reify check` does not.
/// The Indeterminate outcome under check is correct — `@optimized` targets
/// body-inline to `undef`, making FEA-result constraints Indeterminate, which
/// is not a violation and does not gate the exit code.
///
/// Rationale for the check posture: registering compute trampolines in `check`
/// would run a potentially slow FEA solve inside the static-check path.  Use
/// `reify build` or `reify eval` as the FEA gate.
///
/// Corrected for task #5748: this rationale used to read "violating 'check
/// attaches no kernel by design'".  Post-#5748 that phrasing is false — `check`
/// DOES attach a geometry kernel (`Engine::with_registered_kernel`,
/// engine_admin.rs) for any geometry-bearing module.  The accurate statement is
/// that `check` never registers a compute TRAMPOLINE: `register_compute_
/// trampolines` lives only in `configured_eval_engine`, which `cmd_check`
/// deliberately does not call on either sub-path.  The two axes are
/// independent, and #5748 moves only the geometry-kernel one — which is exactly
/// why this test still sees INDETERMINATE + exit 0 here while `reify eval` on
/// the same fixture reports VIOLATED + exit 1.  Assertions unchanged.
///
/// WHAT HOLDS THE EXIT-0 CONTRACT CHANGED IN #5403 — read this before touching
/// the assertions.
///
/// Before γ, this test passed because `reify check` gated on essentially
/// nothing: an Error-severity diagnostic could not move its exit code at all.
/// Post-γ it passes for a much narrower reason — `check_gating_error` gates on
/// EVERY `Severity::Error` diagnostic in the merged set, and the ONLY Error
/// this fixture produces is the trampoline one, which
/// `CHECK_ERROR_EXIT_ALLOWLIST` entry #1 (cite #5311) excuses.
///
/// That makes this test the executable proof that the allowlist is LOAD-BEARING
/// rather than dead code: delete entry #1 and this test goes red.
///
/// Hence the added stderr assertion. Without it, an engine-side change that
/// stopped emitting the trampoline diagnostic would leave this test green while
/// silently making entry #1 unreachable — and the entry would then look
/// retirable when in fact nothing had been demoted.
///
/// BURN-DOWN NOTE: when #5311 demotes the diagnostic to `Severity::Warning` and
/// #5404 removes the allowlist entry, THIS TEST STILL PASSES unchanged — a
/// Warning never gates, and the stderr assertion only requires the text to be
/// present, not its severity. So the burn-down needs no edit here.
#[test]
fn check_fea_violated_constraint_is_not_gated() {
    let path = common::fixture_path("fea_cantilever_violated.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check fea_cantilever_violated.ri should exit 0 — FEA-result \
         constraints are Indeterminate (not violated) under the lightweight \
         check posture, and its only Severity::Error is the ALLOWLISTED \
         trampoline diagnostic.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    assert!(
        stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE' — both FEA constraints evaluate \
         to Undef under check's unregistered-trampoline posture.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );

    assert!(
        stderr.contains("no registered compute trampoline"),
        "the trampoline Error must still be EMITTED, or this test passes \
         vacuously and CHECK_ERROR_EXIT_ALLOWLIST entry #1 (#5311) looks \
         retirable when nothing has been demoted.\nstderr:\n{stderr}"
    );
}
