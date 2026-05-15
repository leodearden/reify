//! Pin: every `pub fn` in `crates/reify-audit/src/` is either called by a
//! non-test caller or carries a `// G-allow:` marker.
//!
//! User-observable signal (per task description and design decisions):
//!   `cargo test -p reify-audit --test g_allow`
//!
//! The test shells out to `scripts/audit-orphan-producers.sh` — the
//! source-of-truth for orphan detection — with `--scope crates/reify-audit/src`
//! so it only checks this crate. Running it workspace-wide would fail for
//! reasons outside this task (422 pre-existing orphans captured in the
//! baseline report).
//!
//! Graceful skip: if `python3` or `git` are absent from PATH, the test prints
//! a note to stderr and returns. Mirrors
//! `crates/reify-kernel-gmsh/tests/rpath_smoke.rs`.

use std::path::Path;
use std::process::Command;

#[test]
fn reify_audit_pub_fns_are_g_allow_marked() {
    // Resolve script path: CARGO_MANIFEST_DIR = crates/reify-audit
    // Go up two parents → repo root → scripts/audit-orphan-producers.sh
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let script = Path::new(manifest_dir)
        .parent()
        .expect("crates/reify-audit has a parent")
        .parent()
        .expect("crates/ has a parent (repo root)")
        .join("scripts/audit-orphan-producers.sh");

    let repo_root = script
        .parent()
        .expect("scripts/ dir exists")
        .parent()
        .expect("repo root exists");

    // Graceful skip: check python3 is available
    match Command::new("python3").arg("--version").output() {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("python3 not on PATH; skipping g_allow orphan check");
            return;
        }
        Err(e) => panic!("unexpected error probing python3: {e}"),
    }

    // Graceful skip: check the script itself exists
    if !script.exists() {
        eprintln!(
            "scripts/audit-orphan-producers.sh not found at {:?}; skipping",
            script
        );
        return;
    }

    let output = Command::new(&script)
        .args(["--strict", "--scope", "crates/reify-audit/src", "--quiet", "--format", "json"])
        .current_dir(repo_root)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke audit-orphan-producers.sh: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse the JSON so we can print a helpful diagnostic
    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "audit-orphan-producers.sh output was not valid JSON: {e}\nstdout: {stdout}"
        )
    });

    let orphan_count = result["orphan_count"]
        .as_u64()
        .expect("orphan_count field present in JSON output");

    assert_eq!(
        orphan_count,
        0,
        "reify-audit has {orphan_count} unmarked orphan pub fn(s); \
         each needs a `// G-allow: ...` comment on the line immediately \
         above the `pub fn` declaration.\nOrphans:\n{:#}",
        result["orphans"]
    );
}
