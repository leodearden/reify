//! Tensegrity CLI smokes, relocated by task #5718 from
//! `crates/reify-eval/tests/harness_fea_solver_e2e/tensegrity_t1a_form_find.rs`
//! (cable-net and membrane form-find) and `.../tensegrity_pavilion_e2e.rs`
//! (pavilion dual-result).
//!
//! ## Why these live in reify-cli (#5718)
//!
//! They spawn the `reify` CLI. Hosted in `reify-eval` they had NO cargo build
//! edge on `reify-cli` and resolved `target/<profile>/reify` by path, so they
//! asserted against a binary that could predate the change under test — a
//! false green (observed on #5618). `env!("CARGO_BIN_EXE_reify")` gives them a
//! real build edge: cargo builds this package's `[[bin]]` before running its
//! integration tests.
//!
//! That also retires the three near-identical `resolve_reify_bin` copies these
//! tests carried, and the reason they existed — the merge gate's release pass
//! does not rebuild `reify-cli`, so `target/release/reify` was absent and the
//! tests fell back across profiles to `target/debug/reify`. Here they simply
//! run in the debug `--workspace` pass, which builds the binary they need.
//!
//! `cargo run -p reify-cli` remains rejected as an alternative: even with the
//! binary already compiled it re-fingerprints the whole workspace and blocks on
//! the global cargo build-lock before exec, which under concurrent multi-worktree
//! verify load pushes the test past its budget (esc-4340-32, exit 124).

use crate::common;

/// (c) CLI smoke: `reify eval examples/tensegrity_cable_net.ri` exits zero and
/// prints the solved z (0.5) — the user-observable `result.nodes` signal.
#[test]
fn cli_cable_net_prints_solved_z() {
    let example = std::path::PathBuf::from(common::example_path("tensegrity_cable_net.ri"));

    let (success, stdout, stderr) = common::run_eval_from_workspace_root(&example);

    assert!(
        success,
        "`reify eval examples/tensegrity_cable_net.ri` exited non-zero.\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
    // Tight assertion: the solved free node 0 prints at the anchor centroid
    // (0, 0, 0.5) m, i.e. the exact token `point(0 m, 0 m, 0.5 m)`. A bare "0.5"
    // substring would also match "0.50" / "10.5" / any incidental 0.5 in another
    // cell, so a *wrong* solve could pass; the full point string ties z = 0.5 to
    // node 0 being at the centroid. The 1×1 reduced solve is bit-exact here
    // (2 / 4 = 0.5 in IEEE-754), so the printed form needs no tolerance.
    assert!(
        stdout.contains("point(0 m, 0 m, 0.5 m)"),
        "expected the solved node 0 at the anchor centroid `point(0 m, 0 m, 0.5 m)` \
         in `reify eval` stdout; got:\n{stdout}"
    );
}

/// (c) CLI smoke: `reify eval examples/tensegrity_membrane_formfind.ri` exits zero
/// and prints the form-found membrane result — `FormFindResult { converged: true,
/// … }`. No exact coordinate is asserted (the curved minimal-surface shape is a
/// MEASURED mesh-convergence bound that lives only in the kernel golden, never at
/// the .ri level); `converged: true` is the honest user-observable γ signal.
#[test]
fn cli_membrane_prints_converged() {
    let example =
        std::path::PathBuf::from(common::example_path("tensegrity_membrane_formfind.ri"));

    let (success, stdout, stderr) = common::run_eval_from_workspace_root(&example);

    assert!(
        success,
        "`reify eval examples/tensegrity_membrane_formfind.ri` exited non-zero.\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
    // The form cell renders as `MembraneFormFind.form = FormFindResult { converged:
    // true, … }` (fields alphabetised, so `converged` leads). Asserting the
    // `FormFindResult { converged: true,` prefix ties the convergence flag to the
    // form-find result — the user-observable γ signal — without pinning any
    // (non-honest) solved coordinate.
    assert!(
        stdout.contains("FormFindResult { converged: true,"),
        "expected a form-found `FormFindResult {{ converged: true, … }}` in \
         `reify eval` stdout; got:\n{stdout}"
    );
}

/// (f) CLI dual-result smoke: `reify eval examples/tensegrity_pavilion.ri` exits
/// 0 and stdout contains BOTH `FormFindResult { converged: true,` (the δ signal)
/// AND `MembraneLoadResult {` (the η signal) — the user-observable θ proof that
/// the pavilion form-finds AND carries load.
#[test]
fn pavilion_cli_prints_both_form_find_and_load_results() {
    let example = std::path::PathBuf::from(common::example_path("tensegrity_pavilion.ri"));

    let (success, stdout, stderr) = common::run_eval_from_workspace_root(&example);

    assert!(
        success,
        "`reify eval examples/tensegrity_pavilion.ri` exited non-zero.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}",
    );

    // θ signal (δ half): the pavilion form-finds to convergence.
    assert!(
        stdout.contains("FormFindResult { converged: true,"),
        "expected `FormFindResult {{ converged: true, … }}` in `reify eval` stdout; \
         got:\n{stdout}"
    );

    // θ signal (η half): the pavilion carries load (MembraneLoadResult present).
    assert!(
        stdout.contains("MembraneLoadResult {"),
        "expected `MembraneLoadResult {{…}}` in `reify eval` stdout — the θ load signal; \
         got:\n{stdout}"
    );
}
