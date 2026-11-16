//! `reify test`, `report`, `explain` and `doc` compile the entry as a program (task #5518,
//! PRD resolution-unification.md δ): an imported pub structure resolves, and `--cfg` selects
//! `#cfg`-gated imports as `reify check` does. Inputs are the committed fixture dirs
//! `tests/fixtures/multi_file/` and `tests/fixtures/multi_file_cfg/`.

use std::process::{Command, Output, Stdio};

const UNKNOWN_STRUCTURE: &str = "unknown structure";

/// Run `reify <cmd> [--cfg <cfg>] <fixture>`. `--cfg` sits before the file on purpose: a
/// parser that takes its first argument as the file would read the flag instead.
fn run(cmd: &[&str], cfg: Option<&str>, fixture: &str) -> Output {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let mut argv: Vec<&str> = cmd.to_vec();
    if let Some(cfg) = cfg {
        argv.extend(["--cfg", cfg]);
    }
    argv.push(&path);
    Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary")
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
        (
            &["doc", "--format", "markdown"],
            "multi_file/main.ri",
            "structure Rig",
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
fn doc_documents_only_the_entry_s_own_structures() {
    let out = run(&["doc", "--format", "markdown"], None, "multi_file/main.ri");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success()
            && stdout.contains("structure Rig")
            && !stdout.contains("structure Pulley"),
        "`reify doc` must document the entry's own Rig and not the imported Pulley \
         (D-12, docs/prds/v0_6/resolution-unification.md)\n{}",
        describe(&out)
    );
}
