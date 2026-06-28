//! End-to-end CLI tests for the structural-query ε integration gate (task 3992).
//!
//! Covers two observables:
//!
//! (a) **Forall constraint expansion** (gap #1): `reify check` on a fixture with
//!     `constraint forall m in self.members: determined(m)` must exit 0 and report
//!     all constraints **satisfied** — not INDETERMINATE.  RED until step-4 extends
//!     the structural-query expansion pass to also rewrite constraint expressions.
//!
//! (b) **BOM eval smoke** (integration gate): `reify eval examples/structural_query_bom.ri`
//!     must exit 0 and print both `part_count` and `bolt_count` resolved values.
//!     RED until step-6 creates the example file.
//!
//! A dedicated structured golden (part_count==6, bolt_count==4) is in
//! `crates/reify-eval/tests/structural_query_bom.rs` to avoid stdout-parse fragility.

mod common;

// ── (a) Gap-#1: forall constraint expansion ──────────────────────────────────

/// `reify check` on `structural_query_bom_constraints.ri` must exit 0 and
/// report all constraints **satisfied** — not INDETERMINATE.
///
/// The fixture has two forall constraints:
///   `constraint forall m in self.members: determined(m)`
///   `constraint forall m in self.descendants: determined(m)`
///
/// RED today (step-3): the structural-query expansion pass in engine_eval.rs
/// only rewrites Let cells, leaving the `self.members`/`self.descendants`
/// placeholder inside constraint exprs as the raw `MiniAssembly.__self`
/// entity → `determined(MiniAssembly.__self)` is INDETERMINATE.
/// stdout will contain "INDETERMINATE" and NOT "All constraints satisfied".
///
/// GREEN after step-4: the expansion pass is extended to also rewrite
/// constraint exprs, substituting the concrete member list → forall evaluates
/// true for each present member → constraints SATISFIED → stdout contains
/// "All constraints satisfied".
#[test]
fn check_forall_members_constraint_is_satisfied_not_indeterminate() {
    let path = common::fixture_path("structural_query_bom_constraints.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    // No panic markers in either stream.
    for (name, stream) in [("stdout", &stdout), ("stderr", &stderr)] {
        assert!(
            !stream.contains("panicked") && !stream.contains("RUST_BACKTRACE"),
            "reify check must not panic; found panic marker in {name}:\n\
             stdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    // Exits 0 (INDETERMINATE is also exit-0, so this is not the RED discriminator).
    assert!(
        status.success(),
        "reify check structural_query_bom_constraints.ri should exit 0;\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // RED discriminator: without gap-#1 fix, the forall constraints are
    // INDETERMINATE → stdout contains "INDETERMINATE" (not "All constraints satisfied").
    assert!(
        !stdout.contains("INDETERMINATE"),
        "forall constraints must NOT be INDETERMINATE after expansion fix;\n\
         got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout must contain 'All constraints satisfied' when both forall constraints\n\
         are satisfied (gap-#1 fix: expansion extends to constraint exprs);\n\
         got stdout:\n{stdout}"
    );
    // Extra safety: must never have been violated either.
    assert!(
        !stdout.contains("VIOLATED"),
        "forall constraints must not be VIOLATED;\nstdout:\n{stdout}"
    );
}

// ── (b) Integration gate: BOM eval smoke ─────────────────────────────────────

/// `reify eval examples/structural_query_bom.ri` must exit 0 and print both
/// `part_count` and `bolt_count` resolved values.
///
/// RED today (step-5): `examples/structural_query_bom.ri` does not exist →
/// `reify eval` fails with a file-not-found error.
/// GREEN after step-6: the example is created and evals to part_count=6 / bolt_count=4.
///
/// The structured golden (exact Int values) lives in
/// `crates/reify-eval/tests/structural_query_bom.rs` to avoid stdout-parse fragility.
#[test]
fn eval_structural_query_bom_example_exits_zero() {
    let path = common::example_path("structural_query_bom.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // No panic markers.
    for (name, stream) in [("stdout", &stdout), ("stderr", &stderr)] {
        assert!(
            !stream.contains("panicked") && !stream.contains("RUST_BACKTRACE"),
            "reify eval must not panic; found panic marker in {name}:\n\
             stdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    // RED discriminator: file not found → exit non-zero.
    assert!(
        status.success(),
        "reify eval examples/structural_query_bom.ri should exit 0;\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // Both eval-observable cells must appear in stdout.
    assert!(
        stdout.contains("part_count"),
        "stdout must contain 'part_count';\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("bolt_count"),
        "stdout must contain 'bolt_count';\nstdout:\n{stdout}"
    );
}
