mod common;

/// End-to-end CLI signal for task 4794: a user model that instantiates the
/// prelude `Vec3<Q>` alias cross-module must `reify check` clean.
///
/// The example file (`examples/parametric_vec3_cross_module.ri`) uses
/// `Vec3<Pressure>` from the stdlib prelude — it does NOT declare `Vec3` itself,
/// exercising the PRELUDE-ONLY cross-module path.  Previously this produced an
/// "unresolved type" Error (arity mismatch: 0-param Vec3 used with 1 arg);
/// after task 4794 the alias is parametric and the check exits success.
///
/// RED on base: `examples/parametric_vec3_cross_module.ri` does not yet exist
/// → `reify check` exits non-zero (file-not-found).
#[test]
fn check_parametric_vec3_cross_module_exits_success() {
    let path = common::example_path("parametric_vec3_cross_module.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check must exit 0 for parametric_vec3_cross_module.ri.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout must contain 'All constraints satisfied'; got: {stdout}"
    );
}
