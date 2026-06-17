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

/// B9: `reify eval examples/generics/dim_param.ri` succeeds with BOTH dimensions.
///
/// The SAME generic fn `scale_q<Q: Dimension>(x: Scalar<Q>, k: Real) -> Scalar<Q>`
/// is applied at LENGTH (`scale_q(10mm, 3.0) == 30mm`) and PRESSURE
/// (`scale_q(5MPa, 2.0) == 10MPa`), Q bound per-call, the return scalar
/// carrying the bound dimension.  This is the PRD §1 / §4.4 D8 / §8 B9 signal.
///
/// The constraint gate is the STRONG B9 check: both the LENGTH and PRESSURE
/// constraints must pass (`reify check` exit 0 + "All constraints satisfied.").
/// A mis-substituted result type (dimension-mismatch) or unbound ScalarParam
/// (poisoned call) would fail the check → this test is RED until step-10 creates
/// the example file.
///
/// The LENGTH result (`scale_q(10mm, 3.0)`) is also asserted directly as the
/// eval-output substring "0.03 m".
///
/// The PRESSURE result (`scale_q(5MPa, 2.0)`) is asserted directly as the
/// SI-base substring "press = 10000000".  Pressure scalars display in SI-base
/// form — "10000000 kg·m^-1·s^-2" (1e7 Pa) rather than "Pa" or "MPa" — so the
/// assertion uses the stable SI-base magnitude.  This makes the second-dimension
/// value gate direct rather than purely constraint-mediated.
///
/// RED until step-10 creates examples/generics/dim_param.ri.
#[test]
fn eval_dim_param_example_b9() {
    let path = common::example_path("generics/dim_param.ri");

    // `reify check` must exit 0 AND report all constraints satisfied (both dims).
    let (status, stdout, stderr) = common::run_subcommand("check", &path);
    assert!(
        status.success(),
        "reify check generics/dim_param.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.' (both LENGTH and PRESSURE \
         constraints must hold); got: {stdout}\nstderr: {stderr}"
    );

    // `reify eval` must exit 0 AND contain both dimension results.
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);
    assert!(
        status.success(),
        "reify eval generics/dim_param.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // LENGTH result: scale_q(10mm, 3.0) = 0.03 m.
    assert!(
        stdout.contains("0.03 m"),
        "stdout should contain '0.03 m' (scale_q(10mm, 3.0)); got: {stdout}\nstderr: {stderr}"
    );
    // PRESSURE result: scale_q(5MPa, 2.0) = 1e7 Pa = 10000000 (SI-base).
    // Pressure renders as "10000000 kg·m^-1·s^-2" — assert the SI-base magnitude
    // directly so a mis-bound Q (wrong dimension) or wrong value is caught without
    // relying solely on the constraint window.
    assert!(
        stdout.contains("press = 10000000"),
        "stdout should contain 'press = 10000000' (scale_q(5MPa, 2.0) = 1e7 Pa SI-base); \
         got: {stdout}\nstderr: {stderr}"
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
