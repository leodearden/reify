/// Integration test: `reify eval examples/fea_shell_flexure.ri` must not emit
/// the soft-fallback warning "falling back to tet meshing" on stderr.
///
/// ## Why this test exists
///
/// `configured_eval_engine` (main.rs) previously registered only
/// `register_compute_fns` and omitted `register_shell_extract_compute_fns`.
/// For shell-classified FEA fixtures the `insert_shell_extract_upstream`
/// lowering dispatches a `shell-extract::extract` ComputeNode; with the target
/// unregistered, `run_compute_dispatch` returns `DispatchError::Failed` and the
/// Auto/Off failure policy emits a Warning diagnostic —
/// "shell-extract::extract failed; falling back to tet meshing
/// (ShellForce::Auto/Off soft fallback)" — to stderr.  The FEA trampoline still
/// re-classifies and solves as shell (result is correct, exit 0), but the
/// warning is cosmetically misleading.
///
/// After registering `register_shell_extract_compute_fns` the dispatch succeeds,
/// the segmentation edge is wired, and the warning is never pushed.
///
/// ## OCCT independence
///
/// `shell-extract` dispatch operates on a synthetic slab SDF built by the
/// lowering (`build_slab_sdf`), not OCCT geometry.  `fea_shell_flexure.ri`'s
/// `box(...)` is a deferred GHR-beta handle and its FEA solve is pure-Rust
/// (reify-solver-elastic).  Both `status.success()` and the absence of the
/// warning hold unconditionally — no `cfg(has_occt)` gate is needed.
mod common;

#[test]
fn eval_shell_fixture_emits_no_tet_fallback_warning() {
    let path = common::example_path("fea_shell_flexure.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval exited non-zero.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    assert!(
        !stderr.contains("falling back to tet meshing"),
        "Unexpected soft-fallback warning on stderr — shell-extract dispatch \
         should succeed once register_shell_extract_compute_fns is registered \
         in configured_eval_engine.\nstderr:\n{stderr}"
    );
}
