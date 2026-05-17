//! Pin: the `reify-gui` workspace binary embeds a RUNPATH entry and does
//! NOT link conda-forge OCCT 7.9. Mirrors
//! `crates/reify-cli/tests/rpath_smoke.rs`; see its module doc for full
//! rationale. Only runs when the `gui` feature is enabled (the binary
//! requires it via `required-features`).

#![cfg(all(target_os = "linux", feature = "gui"))]

use std::process::Command;

#[test]
fn reify_gui_binary_embeds_runpath() {
    let exe = env!("CARGO_BIN_EXE_reify-gui");
    let Some(stdout) = readelf_d(exe) else {
        return;
    };
    let has_rpath = stdout.contains("(RPATH)") || stdout.contains("(RUNPATH)");
    assert!(
        has_rpath,
        "readelf -d {exe} produced no (RPATH) or (RUNPATH) entry — \
         gui/src-tauri/build.rs may have regressed and dropped its \
         emit_rpath_for_bins call.\n\nFull readelf -d output:\n{stdout}"
    );
}

#[test]
fn reify_gui_binary_does_not_link_conda_occt_7_9() {
    let exe = env!("CARGO_BIN_EXE_reify-gui");
    let Some(stdout) = readelf_d(exe) else {
        return;
    };
    let needed_tk: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains("(NEEDED)") && l.contains("libTK"))
        .collect();
    if needed_tk.is_empty() {
        eprintln!("no NEEDED libTK* entries in {exe}; OCCT likely stubbed — skipping");
        return;
    }
    let leaks_7_9: Vec<&&str> = needed_tk.iter().filter(|l| l.contains(".7.9")).collect();
    assert!(
        leaks_7_9.is_empty(),
        "{exe} NEEDS conda-forge OCCT 7.9 libs:\n{leaks}\n\nFull NEEDED \
         libTK* lines:\n{all}",
        leaks = leaks_7_9.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n"),
        all = needed_tk.join("\n"),
    );
}

fn readelf_d(path: &str) -> Option<String> {
    let output = match Command::new("readelf").args(["-d", path]).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("readelf unavailable on PATH; skipping");
            return None;
        }
        Err(e) => panic!("readelf invocation failed: {e}"),
    };
    if !output.status.success() {
        eprintln!("readelf -d exited non-zero (status={:?}); skipping", output.status);
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}
