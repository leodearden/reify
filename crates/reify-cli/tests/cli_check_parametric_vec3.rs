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
/// Primary behavioral guard: `vec3_alias_resolves_via_real_stdlib_prelude` in
/// `cross_module_alias_propagation_tests.rs` asserts the compiler resolves
/// `Vec3<Pressure>` to `Type::vec3(Pressure)` with zero Error/Info diagnostics.
///
/// This CLI test is an end-to-end integration gate.  The `stderr` assertion
/// below also guards against a specific regression: if the `Vec3<Q>` alias
/// reverts to 0-param while the example file still exists, `Vec3<Pressure>` in
/// the example would produce an "unresolved type" Error and non-zero exit — the
/// `status.success()` assertion catches that; the `stderr` check makes the
/// failure message specific to the arity regression rather than generic.
#[test]
fn check_parametric_vec3_cross_module_exits_success() {
    let path = common::example_path("parametric_vec3_cross_module.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        !stderr.contains("unresolved type"),
        "stderr must not contain 'unresolved type' — would fire if Vec3 alias \
         reverts to 0-param (arity regression guard);\nstderr: {stderr}"
    );
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
