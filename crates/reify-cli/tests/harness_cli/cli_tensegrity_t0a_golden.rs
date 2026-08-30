//! T0a / M0 CLI goldens, relocated by task #5718 from
//! `crates/reify-eval/tests/tensegrity_t0a.rs`.
//!
//! ## Why these live in reify-cli (#5718)
//!
//! Both spawn the `reify` CLI and compare stdout against a committed golden,
//! and each carried a DIFFERENT broken build edge in reify-eval:
//!
//! - `cli_reify_eval_prints_t_prism_wireframe` resolved `target/<profile>/reify`
//!   by path — no build edge at all, so it could assert against a binary that
//!   predates the change under test (the #5618 false-green class).
//! - `cli_reify_eval_prints_membrane_patch` spawned `cargo run -q -p reify-cli
//!   --bin reify`, which DOES have a real build edge but re-fingerprints the
//!   whole workspace and blocks on the global cargo build-lock per invocation —
//!   the documented cause of esc-4340-32 (exit 124).
//!
//! `env!("CARGO_BIN_EXE_reify")` fixes both at once: cargo builds this package's
//! `[[bin]]` before its integration tests run, so the binary is fresh by
//! construction with no per-invocation cargo overhead.
//!
//! The goldens stay at `crates/reify-eval/tests/golden/` (resolved via
//! `common::eval_golden_path`) and their bytes are unchanged by the move.

use crate::common;

/// `reify eval examples/tensegrity_t_prism.ri` must print the T-prism instance
/// and 6 tagged TensegrityWire values. Output compared against the committed
/// golden at `crates/reify-eval/tests/golden/tensegrity_t_prism.txt`.
/// Regenerate with `REIFY_REGENERATE_GOLDEN=1`.
#[test]
fn cli_reify_eval_prints_t_prism_wireframe() {
    let example = std::path::PathBuf::from(common::example_path("tensegrity_t_prism.ri"));
    let golden = common::eval_golden_path("tensegrity_t_prism");

    let (success, stdout, stderr) = common::run_eval_from_workspace_root(&example);

    assert!(
        success,
        "`reify eval examples/tensegrity_t_prism.ri` exited non-zero.\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );

    if common::assert_or_regenerate_golden(&stdout, &golden) {
        return;
    }

    // Defense-in-depth: pins the T0a signal independent of golden content.
    // Fields are sorted alphabetically in the output, so `from_index` precedes
    // `kind`. We match on `kind: "strut"` / `kind: "cable"` substrings.
    assert!(
        stdout.contains("kind: \"strut\""),
        "T0a signal: expected at least one TensegrityWire with kind=\"strut\"; got:\n{stdout}"
    );
    assert!(
        stdout.contains("kind: \"cable\""),
        "T0a signal: expected at least one TensegrityWire with kind=\"cable\"; got:\n{stdout}"
    );
}

/// `reify eval examples/tensegrity_membrane_patch.ri` must print the membrane
/// patch instance and TensegritySurface values tagged kind: "membrane".
/// Output compared against the committed golden at
/// `crates/reify-eval/tests/golden/tensegrity_membrane_patch.txt`.
/// Regenerate with `REIFY_REGENERATE_GOLDEN=1`.
#[test]
fn cli_reify_eval_prints_membrane_patch() {
    let example = std::path::PathBuf::from(common::example_path("tensegrity_membrane_patch.ri"));
    let golden = common::eval_golden_path("tensegrity_membrane_patch");

    let (success, stdout, stderr) = common::run_eval_from_workspace_root(&example);

    assert!(
        success,
        "`reify eval examples/tensegrity_membrane_patch.ri` exited non-zero.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}",
    );

    if common::assert_or_regenerate_golden(&stdout, &golden) {
        return;
    }

    // Defense-in-depth: M0 signal — independent of golden content.
    assert!(
        stdout.contains("kind: \"membrane\""),
        "M0 signal: expected at least one TensegritySurface with kind=\"membrane\"; \
         got:\n{stdout}"
    );
}
