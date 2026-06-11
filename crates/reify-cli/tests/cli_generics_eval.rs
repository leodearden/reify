//! End-to-end CLI tests for type-level generic user functions (task 4233 δ).
//!
//! Covers B1 (identity), B2 (single/container), and B5 (constant_field/unbound_param)
//! via `reify check` and `reify eval` against examples in examples/generics/.
//!
//! Do NOT assert stderr is empty — a benign W_MODULE_DECL_MISSING warning appears
//! on stderr for every file that omits a top-of-file `module` declaration (the
//! entire examples corpus). This matches the pattern in cli_stackup_eval.rs.

mod common;

/// B1: `reify eval examples/generics/identity.ri` succeeds and stdout contains
/// the expected values for both the generic `id<T>` call and the monomorphic twin
/// `id_length`.
///
/// Pins example-tier erasure parity: `id(5mm)` and `id_length(5mm)` produce
/// identical output `0.005 m`.
///
/// RED until step-2 creates examples/generics/identity.ri.
#[test]
fn eval_identity_example_b1() {
    let path = common::example_path("generics/identity.ri");

    // `reify check` must exit 0 and report all constraints satisfied.
    let (status, stdout, stderr) = common::run_subcommand("check", &path);
    assert!(
        status.success(),
        "reify check generics/identity.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.'; got: {stdout}\nstderr: {stderr}"
    );

    // `reify eval` must exit 0 and contain both value cells.
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);
    assert!(
        status.success(),
        "reify eval generics/identity.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("a = 0.005 m"),
        "stdout should contain 'a = 0.005 m'; got: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("a_mono = 0.005 m"),
        "stdout should contain 'a_mono = 0.005 m'; got: {stdout}\nstderr: {stderr}"
    );
}

/// B2: `reify eval examples/generics/container.ri` succeeds and stdout contains
/// the list value `[0.005 m]`, exercising `Type::TypeParam` inside `List<T>` at
/// eval (the :1335 fix).
///
/// RED until step-4 creates examples/generics/container.ri.
#[test]
fn eval_container_example_b2() {
    let path = common::example_path("generics/container.ri");

    // `reify check` must exit 0.
    let (status, stdout, stderr) = common::run_subcommand("check", &path);
    assert!(
        status.success(),
        "reify check generics/container.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );

    // `reify eval` must exit 0 and contain the list value.
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);
    assert!(
        status.success(),
        "reify eval generics/container.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("[0.005 m]"),
        "stdout should contain '[0.005 m]'; got: {stdout}\nstderr: {stderr}"
    );
}

/// B5: `reify eval examples/generics/unbound_param.ri` succeeds and stdout
/// contains the Real value `42.5`, exercising that a nested unbound type-param D
/// inside `Field<D, C>` is TOLERATED (no `E_FN_TYPE_ARG_UNRESOLVED`).
///
/// RED until step-6 creates examples/generics/unbound_param.ri.
#[test]
fn eval_unbound_param_example_b5() {
    let path = common::example_path("generics/unbound_param.ri");

    // `reify check` must exit 0 (checks clean — B5 "checks clean").
    let (status, stdout, stderr) = common::run_subcommand("check", &path);
    assert!(
        status.success(),
        "reify check generics/unbound_param.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );

    // `reify eval` must exit 0 and contain the Real payload.
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);
    assert!(
        status.success(),
        "reify eval generics/unbound_param.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("42.5"),
        "stdout should contain '42.5'; got: {stdout}\nstderr: {stderr}"
    );
}
