//! Wave-2 materials CLI golden, relocated by task #5718 from
//! `crates/reify-eval/tests/materials_starter_library.rs`.
//!
//! ## Why this lives in reify-cli (#5718)
//!
//! It spawns the `reify` CLI and compares stdout against a committed golden.
//! Hosted in `reify-eval` it was the worst of the six sites: it hardcoded
//! `<CARGO_TARGET_DIR|workspace_root/target>/debug/reify` with no profile-local
//! branch at all, so in the merge gate's release pass it reached across profiles
//! by construction, and in every pass it had NO cargo build edge on `reify-cli`
//! — the binary could arbitrarily predate the change under test (#5618).
//! `env!("CARGO_BIN_EXE_reify")` replaces both the `CARGO_TARGET_DIR` env probe
//! and the hardcoded `debug` segment with a binary cargo guarantees is freshly
//! built before this package's integration tests run.
//!
//! `cargo run` stays rejected as an alternative: even with the binary already
//! compiled it re-fingerprints the whole workspace before exec, and under high
//! build concurrency that overhead pushes the suite past its time budget
//! (esc-4340-32, exit 124).
//!
//! OCCT still resolves without going through cargo: the cargo runner
//! (`.cargo/run-with-occt.sh`) exports `LD_LIBRARY_PATH` into this test
//! process's environment, which the spawned child inherits.
//!
//! The golden stays at `crates/reify-eval/tests/golden/materials_starter_library.txt`
//! (resolved via `common::eval_golden_path`) and its bytes are unchanged.

use crate::common;

/// `reify eval examples/materials_starter_library.ri` must print inspectable
/// structure-shaped values (not `undef`) for all three wave-2 materials, and
/// its stdout must match the committed golden. Regenerate with
/// `REIFY_REGENERATE_GOLDEN=1`.
#[test]
fn cli_reify_eval_prints_inspectable_material_values() {
    let example = std::path::PathBuf::from(common::example_path("materials_starter_library.ri"));
    let golden = common::eval_golden_path("materials_starter_library");

    let (success, stdout, stderr) = common::run_eval_from_workspace_root(&example);

    assert!(
        success,
        "`reify eval` exited non-zero.\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );

    if common::assert_or_regenerate_golden(&stdout, &golden) {
        return;
    }

    // Defence-in-depth: assert the committed golden itself names all three
    // materials. Checked against the on-disk golden (not `stdout`) so the intent
    // is explicit — this fires if someone regenerated the golden against a
    // regressed binary before the byte comparison above is reached.
    let expected = std::fs::read_to_string(&golden).expect(
        "golden crates/reify-eval/tests/golden/materials_starter_library.txt missing; \
         run once with REIFY_REGENERATE_GOLDEN=1",
    );
    assert!(
        expected.contains("Aluminium_6061_T6 {"),
        "committed golden must mention Aluminium_6061_T6 — golden may have been \
         regenerated against a regressed binary; re-run with REIFY_REGENERATE_GOLDEN=1 \
         after fixing the regression.\ngolden:\n{expected}"
    );
    assert!(
        expected.contains("Titanium_Ti6Al4V {"),
        "committed golden must mention Titanium_Ti6Al4V — golden may have been \
         regenerated against a regressed binary; re-run with REIFY_REGENERATE_GOLDEN=1 \
         after fixing the regression.\ngolden:\n{expected}"
    );
    assert!(
        expected.contains("ABS_Plastic {"),
        "committed golden must mention ABS_Plastic — golden may have been \
         regenerated against a regressed binary; re-run with REIFY_REGENERATE_GOLDEN=1 \
         after fixing the regression.\ngolden:\n{expected}"
    );
}
