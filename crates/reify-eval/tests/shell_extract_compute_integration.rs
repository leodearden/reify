//! Integration tests for the `shell-extract::extract` ComputeNode trampoline
//! and registration wiring (task γ, #3834).
//!
//! See `docs/prds/v0_4/shell-extract-engine-bridge.md` §4–§8 and
//! `docs/prds/v0_3/compute-node-contract.md` §4 for the full specification.

use reify_eval::register_shell_extract_compute_fns;
use reify_test_support::make_simple_engine;

/// Verify that `register_shell_extract_compute_fns` installs the
/// `"shell-extract::extract"` target in the engine's compute dispatch table.
///
/// PRD §4 contract: after registration `engine.compute_dispatch(target).is_some()`.
#[test]
fn register_shell_extract_compute_fns_registers_extract_target() {
    let mut engine = make_simple_engine();
    register_shell_extract_compute_fns(&mut engine);
    assert!(
        engine.compute_dispatch("shell-extract::extract").is_some(),
        "expected \"shell-extract::extract\" to be registered after \
         register_shell_extract_compute_fns; got None"
    );
}
