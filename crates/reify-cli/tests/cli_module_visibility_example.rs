//! CLI integration gate for `reify check` on the committed module-visibility
//! CI example (task ε #3980 / PRD docs/prds/v0_6/module-and-visibility-hardening.md §6).
//!
//! Exercises the user-observable end-to-end signal from the committed
//! `examples/module_visibility/` directory through the real `reify` binary.
//! Covers four rows of the §6 two-way boundary table:
//!
//! 1. Declared path matches location (producer.ri) → exit 0, no E_MODULE_PATH_MISMATCH.
//! 2. `priv` param hidden from importer (consumer.ri) → E_PRIV_MEMBER_ACCESS on rated_torque.
//! 3. Default-visible param still resolves (consumer.ri) → shaft_diameter NOT flagged.
//! 4. Declared path mismatches (mismatch_variant.ri) → exit 1, E_MODULE_PATH_MISMATCH.

mod common;

/// `reify check` on producer.ri exits 0 and emits no module-path or priv
/// diagnostics: the `module producer` decl matches the file stem, and the
/// pub/priv `Motor` definition is self-consistent without an importer.
///
/// Covers §6 rows: "declared path matches location" and proves the pub/priv
/// def compiles clean standalone.
#[test]
fn check_producer_correct_module_decl_and_priv_def_exits_success() {
    let path = common::example_path("module_visibility/producer.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check producer.ri should exit 0 (module decl matches stem, \
         pub/priv Motor def compiles clean).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied'.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_MODULE_PATH_MISMATCH"),
        "stderr should NOT contain E_MODULE_PATH_MISMATCH \
         (module producer matches stem producer).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "stderr should NOT contain W_MODULE_DECL_MISSING \
         (module decl is present).\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// `reify check` on consumer.ri exits nonzero and emits E_PRIV_MEMBER_ACCESS
/// naming `rated_torque` (the `priv` param of Motor), while `shaft_diameter`
/// (the default-visible param) does NOT appear in the diagnostic output.
///
/// Covers §6 rows: "priv param hidden from importer" AND "default-visible
/// param still works" — both proven in a single `reify check` invocation.
#[test]
fn check_consumer_priv_param_hidden_visible_resolves() {
    let path = common::example_path("module_visibility/consumer.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        !status.success(),
        "reify check consumer.ri should exit nonzero \
         (E_PRIV_MEMBER_ACCESS on rated_torque).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("E_PRIV_MEMBER_ACCESS"),
        "stderr should contain E_PRIV_MEMBER_ACCESS.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("rated_torque"),
        "stderr should name the private member 'rated_torque'.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("shaft_diameter"),
        "stderr should NOT mention 'shaft_diameter' \
         (the default-visible member resolved cleanly).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

/// `reify check` on mismatch_variant.ri exits nonzero and emits
/// E_MODULE_PATH_MISMATCH: the `module wrong.path.here` decl does not match
/// the file stem `mismatch_variant`.
///
/// Covers §6 row: "declared path mismatches location".
#[test]
fn check_mismatched_module_decl_emits_path_mismatch() {
    let path = common::example_path("module_visibility/mismatch_variant.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        !status.success(),
        "reify check mismatch_variant.ri should exit nonzero \
         (E_MODULE_PATH_MISMATCH: wrong.path.here != mismatch_variant).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("E_MODULE_PATH_MISMATCH"),
        "stderr should contain E_MODULE_PATH_MISMATCH.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}
