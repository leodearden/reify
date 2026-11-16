//! `reify test`, `report`, `explain` and `doc` compile the entry as a program (task #5518,
//! PRD resolution-unification.md δ): an imported pub structure resolves, and `--cfg` selects
//! `#cfg`-gated imports as `reify check` does. Inputs are the committed fixture dirs
//! `tests/fixtures/multi_file/` and `tests/fixtures/multi_file_cfg/`. The same behaviour for
//! `eval` and `build` (γ) is pinned in `harness_cli/cli_eval_build_multi_file.rs`.

use std::process::{Command, Output, Stdio};

const UNKNOWN_STRUCTURE: &str = "unknown structure";

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn reify(argv: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary")
}

/// Run `reify <cmd> [--cfg <cfg>] <fixture>`. `--cfg` sits before the file on purpose: a
/// parser that takes its first argument as the file would read the flag instead.
fn run(cmd: &[&str], cfg: Option<&str>, fixture_name: &str) -> Output {
    let path = fixture(fixture_name);
    let mut argv: Vec<&str> = cmd.to_vec();
    if let Some(cfg) = cfg {
        argv.extend(["--cfg", cfg]);
    }
    argv.push(&path);
    reify(&argv)
}

fn describe(out: &Output) -> String {
    format!(
        "status: {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn each_command_resolves_an_imported_structure() {
    let rows: &[(&[&str], &str, &str)] = &[
        (&["test"], "multi_file/test_rig.ri", "PASS  TestRig"),
        (&["report", "--bom"], "multi_file/bom_kit.ri", "BOLT-M6-20"),
        (
            &["explain"],
            "multi_file/tuned_rig.ri",
            "Rig.gap: objective=1 term(s)",
        ),
    ];
    for &(cmd, fixture, expected) in rows {
        let out = run(cmd, None, fixture);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success()
                && stdout.contains(expected)
                && !stderr.contains(UNKNOWN_STRUCTURE),
            "`reify {}` on {fixture} must resolve the imported structure and print {expected:?}\n{}",
            cmd.join(" "),
            describe(&out)
        );
    }
}

#[test]
fn each_command_follows_a_cfg_gated_import_only_when_selected() {
    let commands: &[&[&str]] = &[
        &["test"],
        &["report", "--bom"],
        &["explain"],
        &["doc", "--format", "markdown"],
    ];
    for &cmd in commands {
        for (cfg, resolves) in [("target=wasm", true), ("target=linux", false)] {
            let out = run(cmd, Some(cfg), "multi_file_cfg/wasm_only.ri");
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                out.status.success() == resolves && stderr.contains(UNKNOWN_STRUCTURE) != resolves,
                "`reify {} --cfg {cfg}` must {} the wasm-gated import\n{}",
                cmd.join(" "),
                if resolves { "resolve" } else { "drop" },
                describe(&out)
            );
        }
    }
}

#[test]
fn doc_resolves_an_import_but_documents_only_the_entry_s_own_structures() {
    let out = run(&["doc", "--format", "markdown"], None, "multi_file/main.ri");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success()
            && !stderr.contains(UNKNOWN_STRUCTURE)
            && stdout.contains("structure Rig")
            && !stdout.contains("structure Pulley"),
        "`reify doc` must resolve the imported Pulley, then document the entry's own Rig and \
         not Pulley (D-12, docs/prds/v0_6/resolution-unification.md)\n{}",
        describe(&out)
    );
}

/// `--stdlib` documents the standard library, which no `--cfg` selects into, so the flag is
/// refused like `--stdlib`'s other conflicting flags instead of being silently dropped.
#[test]
fn doc_rejects_cfg_with_stdlib() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let out_dir = dir.path().join("never-written");
    let out_dir_arg = out_dir.to_string_lossy();
    let out = reify(&[
        "doc",
        "--stdlib",
        "--out",
        &out_dir_arg,
        "--cfg",
        "target=wasm",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success()
            && stderr.contains("Error: --cfg is not valid with --stdlib")
            && !out_dir.exists(),
        "`reify doc --stdlib --cfg ...` must be a usage error that writes nothing\n{}",
        describe(&out)
    );
}

/// Taking `--cfg` out first hands the rest of argv to the command's single-file grammar, so
/// a stray flag or a second file is still a usage error rather than silently ignored.
#[test]
fn single_file_commands_reject_a_stray_flag_or_second_file_beside_cfg() {
    let path = fixture("multi_file/test_rig.ri");
    for cmd in ["test", "explain"] {
        let cases = [
            (
                ["--bogus", path.as_str()],
                format!("Error: unknown flag for `{cmd}`: --bogus"),
            ),
            (
                [path.as_str(), "extra"],
                "Error: unexpected extra positional argument: extra".to_string(),
            ),
        ];
        for (tail, error) in cases {
            let mut argv = vec![cmd, "--cfg", "target=wasm"];
            argv.extend(tail);
            let out = reify(&argv);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                !out.status.success()
                    && stderr.contains(&error)
                    && stderr.contains(&format!("Usage: reify {cmd}")),
                "`reify {}` must print {error:?} and the usage banner\n{}",
                argv.join(" "),
                describe(&out)
            );
        }
    }
}
