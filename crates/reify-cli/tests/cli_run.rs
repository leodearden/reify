mod common;

/// `reify run` is an alias for `reify eval` — both subcommands must produce
/// byte-for-byte identical output on the same input.
///
/// RED driver: today `reify run <f>` hits the `other =>` arm in main.rs and
/// returns ExitCode::FAILURE with "Unknown command: run" on stderr.  After
/// step-2 wires `"run" => cmd_eval(&args[2..])`, both tests go GREEN.
#[test]
fn run_is_alias_for_eval() {
    let f = common::fixture_path("affine_constructors.ri");
    let (run_status, run_stdout, run_stderr) = common::run_subcommand("run", &f);
    let (eval_status, eval_stdout, eval_stderr) = common::run_subcommand("eval", &f);

    assert_eq!(
        run_status.code(),
        eval_status.code(),
        "`reify run` and `reify eval` must exit with the same code"
    );
    assert_eq!(
        run_stdout, eval_stdout,
        "`reify run` and `reify eval` must produce identical stdout"
    );
    assert_eq!(
        run_stderr, eval_stderr,
        "`reify run` and `reify eval` must produce identical stderr"
    );
}

/// `reify run` on a shell .ri file must take the MITC3 shell route:
/// exit 0, stdout contains "ShellStress" (populated shell_channels cell),
/// stderr must NOT contain "falling back to tet meshing".
///
/// The shell-route signals are already GREEN on the `eval` path (task 4244
/// registered register_shell_extract_compute_fns in configured_eval_engine;
/// see cli_eval_shell_no_tet_warning.rs).  These assertions are positive
/// regression guards pinning the flagship `reify run` path specifically.
/// RED driver: same as above — `reify run` is an unknown command today.
#[test]
fn run_shell_route_smoke() {
    let path = common::example_path("fea_shell_flexure.ri");
    let (status, stdout, stderr) = common::run_subcommand("run", &path);

    assert!(
        status.success(),
        "`reify run fea_shell_flexure.ri` must exit 0; stderr:\n{stderr}"
    );
    assert!(
        stdout.contains("ShellStress"),
        "`reify run` on a shell file must print ShellStress (MITC3 shell route); stdout:\n{stdout}"
    );
    assert!(
        !stderr.contains("falling back to tet meshing"),
        "`reify run` must not fall back to tet meshing on a shell file; stderr:\n{stderr}"
    );
}
