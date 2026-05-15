//! Pin: `record_morph_attempt`, `record_remesh`, and `record_rejection` in
//! `crates/reify-mesh-morph/src/stats.rs` each carry a `// G-allow:` marker
//! citing tasks #2947-#2949 (engine call-site wiring is deferred).
//!
//! User-observable signal:
//!   `cargo test -p reify-mesh-morph --test stats_g_allow`
//!
//! The test shells out to `scripts/audit-orphan-producers.sh` with
//! `--scope crates/reify-mesh-morph/src` and asserts *list membership* —
//! each of the three named functions must be ABSENT from `orphans[]` and
//! PRESENT in `allowed[]` with a reason citing both "2947" and "2949".
//!
//! Crucially, we do NOT assert `orphan_count == 0`: reify-mesh-morph has 8
//! pre-existing baseline orphans in boundary/elasticity/laplacian/lib/quality
//! that are outside this task's scope and would make such an assertion spurious.
//!
//! Graceful skip: if `python3` or the script are absent from PATH/disk, the
//! test prints a note to stderr and returns. Mirrors
//! `crates/reify-audit/tests/g_allow.rs`.

use std::path::Path;
use std::process::Command;

/// The three `pub fn` in stats.rs whose only callers are same-crate
/// `#[cfg(test)]` code; engine wiring is deferred to tasks #2947-#2949.
const TARGET_FNS: &[&str] = &["record_morph_attempt", "record_remesh", "record_rejection"];

#[test]
fn stats_record_fns_are_g_allow_marked() {
    // Resolve script path: CARGO_MANIFEST_DIR = crates/reify-mesh-morph
    // Go up two parents → repo root → scripts/audit-orphan-producers.sh
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let script = Path::new(manifest_dir)
        .parent()
        .expect("crates/reify-mesh-morph has a parent")
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
            eprintln!("python3 not on PATH; skipping stats_g_allow orphan check");
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
        .args([
            "--scope",
            "crates/reify-mesh-morph/src",
            "--quiet",
            "--format",
            "json",
        ])
        .current_dir(repo_root)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke audit-orphan-producers.sh: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "audit-orphan-producers.sh output was not valid JSON: {e}\n\
             status: {:?}\nstdout: {stdout}\nstderr: {stderr}",
            output.status
        )
    });

    let stats_suffix = "crates/reify-mesh-morph/src/stats.rs";

    for fn_name in TARGET_FNS {
        // (a) must NOT appear in orphans[] (for the stats.rs file)
        let in_orphans = result["orphans"]
            .as_array()
            .expect("orphans must be an array")
            .iter()
            .any(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(stats_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            });

        assert!(
            !in_orphans,
            "`{fn_name}` in {stats_suffix} is still listed as an orphan — \
             the `// G-allow:` marker may be missing or misplaced.\n\
             Full orphans list:\n{:#}",
            result["orphans"]
        );

        // (b) must appear in allowed[] with a reason citing tasks #2947 and #2949
        let matching_allowed: Vec<_> = result["allowed"]
            .as_array()
            .expect("allowed must be an array")
            .iter()
            .filter(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(stats_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            })
            .collect();

        assert_eq!(
            matching_allowed.len(),
            1,
            "`{fn_name}` in {stats_suffix} must appear exactly once in allowed[]; \
             found {} entries.\nFull allowed list:\n{:#}",
            matching_allowed.len(),
            result["allowed"]
        );

        let reason = matching_allowed[0]["allow_reason"]
            .as_str()
            .unwrap_or_default();
        assert!(
            reason.contains("2947") && reason.contains("2949"),
            "`{fn_name}` allow_reason must cite tasks #2947 and #2949; got: {reason:?}"
        );
    }
}
