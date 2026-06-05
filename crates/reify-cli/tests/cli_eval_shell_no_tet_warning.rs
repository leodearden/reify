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
///
/// ## Positive guard — `stdout.contains("ShellStress")`
///
/// The shell-classified solve produces an `ElasticResult` whose `shell_channels`
/// field is a real `ShellStress` struct (not `undef`); `reify eval` prints
/// this to stdout.  Asserting the substring `"ShellStress"` appears in stdout
/// gives a positive signal that the shell path ran to completion — and makes
/// the test fail loudly if the fixture's aspect ratio is later changed so it
/// no longer auto-classifies as a shell solve (which would regress the entire
/// shell pipeline silently).
///
/// Note: `fea_shell_flexure.ri` carries two pre-existing, unrelated `warning:`
/// diagnostics on stderr (missing `module` declaration; a topology-attribute
/// selector tie).  Both are present before and after this task's fix, so the
/// negative assertion relies on anchoring to the specific target name
/// (`"shell-extract::extract failed"`) rather than asserting `stderr.is_empty()`.
mod common;

#[test]
fn eval_shell_fixture_emits_no_tet_fallback_warning() {
    let path = common::example_path("fea_shell_flexure.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval exited non-zero.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // Specific regression guard: asserts the tet-fallback phrase is absent from
    // stderr.  Uses the shorter stable token "falling back to tet meshing"
    // rather than the full "shell-extract::extract failed; falling back to tet
    // meshing" prefix: if the target-name prefix in engine_eval.rs:3197 is ever
    // reworded, the full-string form would silently become vacuous (always true),
    // whereas the core fallback phrase is less likely to change.  The positive
    // `stdout.contains("ShellStress")` guard below provides the compensating
    // check that the shell path actually ran.
    assert!(
        !stderr.contains("falling back to tet meshing"),
        "Unexpected soft-fallback warning on stderr — shell-extract dispatch \
         should succeed once register_shell_extract_compute_fns is registered \
         in configured_eval_engine.\nstderr:\n{stderr}"
    );

    // Positive guard: confirms the shell-classified FEA solve ran to completion
    // and produced a real `ShellStress` result (not `shell_channels: undef`
    // which the tet path would emit).  Fails loudly if the fixture's aspect
    // ratio changes so it no longer auto-classifies as a shell solve, catching
    // silent fixture-classification regressions.
    assert!(
        stdout.contains("ShellStress"),
        "Expected 'ShellStress' in eval stdout — shell-classified solve should \
         produce a real ShellStress shell_channels field.\nstdout:\n{stdout}\
         \nstderr:\n{stderr}"
    );
}
