//! F-inherit ζ regression corpus and CI example (task #4826), relocated from
//! `crates/reify-eval/tests/harness_fea_solver_e2e/objective_inheritance_e2e.rs`
//! by task #5718.
//!
//! ## Purpose
//!
//! 1. **D1 back-compat byte-identity corpus** (INV-2 / G6 branch-3, BT1/BT2/BT7):
//!    - BT1 – single scope with an explicit objective (minimize → solver clamp).
//!    - BT2 – two uncoupled scopes in both declaration orders (AB and BA);
//!      asserts stdout is byte-identical regardless of order (INV-2 proof).
//!    - BT7 – objective-less scope; synthetic-centrality midpoint.
//!
//! 2. **D2 BT5 CI-example** (`examples/objective_inheritance.ri`):
//!    - `reify eval` resolves C.k to a non-undef concrete value under
//!      inherited governance.
//!    - `reify explain` shows C.k `inherited from P` and P.w `source=explicit`.
//!
//! ## Why byte-identity — and the tripwire that would retire it (task #5618)
//!
//! BT2/BT7 pin the *centrality midpoint* of an objective-less scope's two-sided
//! box.  Task #5618 changed how that value is produced and drifted both goldens;
//! they were re-blessed on 2026-07-28 under an L2 ruling.  The reasoning is
//! recorded here so it is not re-litigated on the next sighting.
//!
//! BEFORE #5618 the solver seeded at a fixed 0.01 and the printed value was
//! wherever Nelder-Mead happened to terminate — an *iterative* result.  Scored
//! against the exact midpoint of each box as represented, all three values were
//! wrong: BT7 `k` by 1 ULP, BT2 `A.a` by 4 ULP, BT2 `B.b` by 1 ULP.
//!
//! AFTER #5618 `extract_initial_point` seeds a two-sided constraint-derived box
//! at `(box_lo + box_hi) / 2.0` (reify-constraints/src/solver.rs, arm 3).  That
//! point is the unique argmax of the synthesised Chebyshev-centre objective on a
//! box, and Nelder-Mead returns its best-ever vertex — so the solver returns the
//! seed bitwise, with no iteration contributing to the result.  All three values
//! are now exactly the represented midpoint; none were before.
//!
//! So byte-identity here became MORE defensible, not less.  The printed value is
//! now a two-operation closed form (one unit conversion, one halving), each a
//! single correctly-rounded IEEE-754 op — stable across libm, CPU, opt-level and
//! iteration count, none of which the old iterative values were.  Byte-identity
//! is the wrong contract for an iteratively-derived float (it pins termination
//! artifacts and fires on every legitimate numerical change) and the right one
//! for a closed-form float.  #5618 moved these three from the first case to the
//! second.
//!
//! TRIPWIRE: that argument holds only while centrality stays closed-form.  If it
//! ever becomes iterative again — a coupled or non-box feasible region, a
//! smoothed max-min, or any polish pass applied after the seed — byte-identity
//! reverts to pinning solver artifacts and MUST be replaced with a ULP-bounded
//! assertion.  Re-blessing these goldens under those conditions would be
//! absorbing a regression, which is exactly what this corpus exists to prevent.
//!
//! Note the INV-2 leg of BT2 (`stdout_ab == stdout_ba`) is a genuine byte-
//! identity property and is unconditionally correct to assert exactly; the
//! tripwire above concerns only the absolute values compared against the golden.
//!
//! ## Why these live in reify-cli (#5718)
//!
//! They spawn the `reify` CLI. Hosted in `reify-eval` they had NO cargo build
//! edge on `reify-cli`, so they resolved whatever `target/<profile>/reify`
//! happened to exist and asserted against a binary that could predate the
//! change under test — a false green, observed on #5618. Here, cargo builds
//! this package's `[[bin]]` before running its integration tests, so the binary
//! is fresh by construction. The former `resolve_reify_bin` profile-local /
//! debug-fallback dance is gone with it: it existed only because the merge
//! gate's release pass does not rebuild `reify-cli`, and these tests now simply
//! run in the debug `--workspace` pass that does.
//!
//! Fixtures and goldens deliberately stay under `crates/reify-eval/tests/`,
//! resolved via `common::eval_fixture_path` / `common::eval_golden_path`.

use crate::common;

// ── D1 corpus: back-compat byte-identity (BT1 / BT2 / BT7) ──────────────────

/// BT1 – single scope with an explicit `minimize` objective.
///
/// Fixture: `crates/reify-eval/tests/fixtures/backcompat/bt1_single_scope.ri`
/// Golden:  `crates/reify-eval/tests/golden/bt1_single_scope.txt`
///
#[test]
fn bt1_single_scope_byte_identity() {
    let fixture = common::eval_fixture_path("backcompat/bt1_single_scope.ri");
    let golden = common::eval_golden_path("bt1_single_scope");

    let (success, stdout, stderr) = common::run_eval_from_workspace_root(&fixture);
    assert!(
        success,
        "`reify eval` exited non-zero for BT1;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    common::assert_or_regenerate_golden(&stdout, &golden);
}

/// BT7 – objective-less scope; synthetic-centrality midpoint.
///
/// Fixture: `crates/reify-eval/tests/fixtures/backcompat/bt7_objectiveless_centrality.ri`
/// Golden:  `crates/reify-eval/tests/golden/bt7_objectiveless_centrality.txt`
///
#[test]
fn bt7_objectiveless_centrality_byte_identity() {
    let fixture = common::eval_fixture_path("backcompat/bt7_objectiveless_centrality.ri");
    let golden = common::eval_golden_path("bt7_objectiveless_centrality");

    let (success, stdout, stderr) = common::run_eval_from_workspace_root(&fixture);
    assert!(
        success,
        "`reify eval` exited non-zero for BT7;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    common::assert_or_regenerate_golden(&stdout, &golden);
}

/// BT2 – two uncoupled scopes; declaration order independence (INV-2).
///
/// Runs both `bt2_uncoupled_ab.ri` (A declared first) and `bt2_uncoupled_ba.ri`
/// (B declared first) and asserts:
///   (a) both match the shared golden `bt2_uncoupled.txt`, AND
///   (b) the two orderings produce BYTE-IDENTICAL stdout (direct INV-2 proof).
///
#[test]
fn bt2_uncoupled_declaration_order_independence() {
    let fixture_ab = common::eval_fixture_path("backcompat/bt2_uncoupled_ab.ri");
    let fixture_ba = common::eval_fixture_path("backcompat/bt2_uncoupled_ba.ri");
    let golden = common::eval_golden_path("bt2_uncoupled");

    let (success_ab, stdout_ab, stderr_ab) = common::run_eval_from_workspace_root(&fixture_ab);
    assert!(
        success_ab,
        "`reify eval` exited non-zero for BT2-AB;\nstdout:\n{stdout_ab}\nstderr:\n{stderr_ab}"
    );

    let (success_ba, stdout_ba, stderr_ba) = common::run_eval_from_workspace_root(&fixture_ba);
    assert!(
        success_ba,
        "`reify eval` exited non-zero for BT2-BA;\nstdout:\n{stdout_ba}\nstderr:\n{stderr_ba}"
    );

    // (b) Direct INV-2 proof: both orderings produce byte-identical output.
    assert_eq!(
        stdout_ab, stdout_ba,
        "BT2 INV-2 violation: bt2_uncoupled_ab.ri and bt2_uncoupled_ba.ri \
         produce DIFFERENT stdout — declaration order leaked into eval output"
    );

    // (a) Both match the shared golden (regen writes from AB; BA identity proved above).
    if common::assert_or_regenerate_golden(&stdout_ab, &golden) {
        return;
    }
    // If not regenerating, also verify BA matches the golden.
    let expected = std::fs::read_to_string(&golden).expect("golden bt2_uncoupled.txt");
    assert_eq!(
        stdout_ba, expected,
        "`reify eval bt2_uncoupled_ba.ri` drifted from bt2_uncoupled.txt"
    );
}

// ── D2: BT5 CI-example (examples/objective_inheritance.ri) ───────────────────

/// BT5 eval – child C resolves under inherited governance (non-undef).
///
/// Asserts exit 0 and that stdout contains `C.k =` but NOT `C.k = undef`.
/// NEVER asserts a specific numeric optimum (§3.2 honesty boundary).
///
#[test]
fn bt5_example_eval_resolves_under_inherited_governance() {
    let example = std::path::PathBuf::from(common::example_path("objective_inheritance.ri"));

    let (success, stdout, stderr) = common::run_eval_from_workspace_root(&example);
    assert!(
        success,
        "`reify eval examples/objective_inheritance.ri` exited non-zero;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // C.k must resolve to a concrete value (not undef).
    assert!(
        stdout.contains("C.k ="),
        "expected 'C.k =' in stdout but got:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("C.k = undef"),
        "C.k must NOT be undef — it should resolve under inherited governance;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// BT5 explain – C.k carries inherited_from=P provenance; P.w is source=explicit.
///
/// Asserts:
///   (a) exit 0
///   (b) C.k line contains "inherited from P" (source=inherited + clause)
///   (c) C.k line does NOT contain "synthetic-centrality" (centrality suppressed)
///   (d) P.w line contains "source=explicit" and NOT "inherited from"
///   (e) Two consecutive runs produce byte-identical stdout (determinism)
///
#[test]
fn bt5_example_explain_shows_inherited_provenance() {
    let example = std::path::PathBuf::from(common::example_path("objective_inheritance.ri"));

    let explain_output =
        || -> (bool, String, String) { common::run_cli_from_workspace_root("explain", &example) };

    let (success, stdout, stderr) = explain_output();

    // (a) exit 0
    assert!(
        success,
        "`reify explain examples/objective_inheritance.ri` exited non-zero;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // (b) C.k line must contain "inherited from P"
    let ck_line = stdout
        .lines()
        .find(|l| l.contains("C.k"))
        .unwrap_or_else(|| panic!("no C.k line in stdout:\n{stdout}\nstderr:\n{stderr}"));
    assert!(
        ck_line.contains("inherited from P"),
        "C.k line must contain 'inherited from P';\nline: {ck_line:?}\nstdout:\n{stdout}"
    );

    // (c) C.k must NOT show synthetic-centrality (inheritance suppresses it)
    assert!(
        !ck_line.contains("synthetic-centrality"),
        "C.k line must NOT contain 'synthetic-centrality' — inheritance suppresses centrality;\nline: {ck_line:?}\nstdout:\n{stdout}"
    );

    // (d) P.w line must contain "source=explicit" and must NOT contain "inherited from"
    let pw_line = stdout
        .lines()
        .find(|l| l.contains("P.w"))
        .unwrap_or_else(|| panic!("no P.w line in stdout:\n{stdout}\nstderr:\n{stderr}"));
    assert!(
        pw_line.contains("source=explicit"),
        "P.w line must contain 'source=explicit';\nline: {pw_line:?}\nstdout:\n{stdout}"
    );
    assert!(
        !pw_line.contains("inherited from"),
        "P.w line must NOT contain 'inherited from';\nline: {pw_line:?}\nstdout:\n{stdout}"
    );

    // (e) Determinism: second run produces byte-identical stdout
    let (_, stdout2, _) = explain_output();
    assert_eq!(
        stdout, stdout2,
        "`reify explain` output is not deterministic (runs 1 and 2 differ)"
    );
}
