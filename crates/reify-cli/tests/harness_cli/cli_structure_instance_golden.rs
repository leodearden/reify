//! SIR-α user-observable CLI golden, relocated by task #5718 from
//! `crates/reify-eval/tests/structure_instance_e2e.rs`.
//!
//! ## Why this lives in reify-cli (#5718)
//!
//! It spawns the `reify` CLI to compare stdout against a committed golden.
//! Hosted in `reify-eval` it had NO cargo build edge on `reify-cli` and resolved
//! `target/<profile>/reify` by path, so the golden — whose entire value is
//! detecting output drift — was compared against a binary that could predate the
//! change under test. `env!("CARGO_BIN_EXE_reify")` makes the binary fresh by
//! construction: cargo builds this package's `[[bin]]` before its integration
//! tests run.
//!
//! The golden itself stays at `crates/reify-eval/tests/golden/structure_instance.txt`
//! (resolved via `common::eval_golden_path`) and its bytes are unchanged by the
//! move.

use crate::common;

/// `reify eval examples/structure-instance.ri` must print inspectable
/// structure-shaped values (not `undef`), and its stdout must match the
/// committed golden. Regenerate with `REIFY_REGENERATE_GOLDEN=1`.
#[test]
fn cli_reify_eval_prints_inspectable_structure_values() {
    let example = std::path::PathBuf::from(common::example_path("structure-instance.ri"));
    let golden = common::eval_golden_path("structure_instance");

    let (success, stdout, stderr) = common::run_eval_from_workspace_root(&example);

    assert!(
        success,
        "`reify eval` exited non-zero.\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );

    if common::assert_or_regenerate_golden(&stdout, &golden) {
        return;
    }

    assert!(
        stdout.contains("Steel_AISI_1045 {"),
        "the SIR-α signal requires an inspectable Steel_AISI_1045 structure value \
         (not `undef`) in `reify eval` output; got:\n{stdout}"
    );
}
