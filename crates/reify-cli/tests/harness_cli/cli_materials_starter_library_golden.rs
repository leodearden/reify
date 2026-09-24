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

    // Defence-in-depth: the wave-2 signal itself, independent of golden content —
    // all three materials must be named. Asserted on `stdout`, matching the two
    // sibling relocated goldens (`cli_structure_instance_golden.rs`,
    // `cli_tensegrity_t0a_golden.rs`). Equivalent to asserting on the golden text
    // at this point, since the only way to reach here is
    // `assert_or_regenerate_golden` having already proved `stdout == golden`, but
    // it avoids a second read of a file just compared byte-for-byte.
    assert!(
        stdout.contains("Aluminium_6061_T6 {"),
        "wave-2 signal: expected an inspectable Aluminium_6061_T6 structure value \
         (not `undef`) in `reify eval` output; got:\n{stdout}"
    );
    assert!(
        stdout.contains("Titanium_Ti6Al4V {"),
        "wave-2 signal: expected an inspectable Titanium_Ti6Al4V structure value \
         (not `undef`) in `reify eval` output; got:\n{stdout}"
    );
    assert!(
        stdout.contains("ABS_Plastic {"),
        "wave-2 signal: expected an inspectable ABS_Plastic structure value \
         (not `undef`) in `reify eval` output; got:\n{stdout}"
    );
}
