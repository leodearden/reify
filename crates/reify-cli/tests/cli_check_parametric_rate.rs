mod common;

/// End-to-end CLI signal for task 4792: a user model that instantiates the
/// prelude `Rate<Q>` alias cross-module must `reify check` clean.
///
/// The example file (`examples/parametric_rate_cross_module.ri`) uses
/// `Rate<Length>` from the stdlib prelude — it does NOT declare `Rate` itself,
/// exercising the PRELUDE-ONLY cross-module path.  Previously this produced an
/// "unresolved type" Error + the task-2777 Info diagnostic; after task 4792
/// both are gone and the check exits success.
///
/// RED on base: `examples/parametric_rate_cross_module.ri` does not yet exist
/// → `reify check` exits non-zero (file-not-found or unresolved-type error).
#[test]
fn check_parametric_rate_cross_module_exits_success() {
    let path = common::example_path("parametric_rate_cross_module.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check must exit 0 for parametric_rate_cross_module.ri.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout must contain 'All constraints satisfied'; got: {stdout}"
    );
}
