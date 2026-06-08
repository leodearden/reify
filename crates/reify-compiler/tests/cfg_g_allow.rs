//! Pin: `cfg_satisfied` in `crates/reify-compiler/src/cfg.rs` carries a
//! `// G-allow:` marker because it is an intermediate producer-before-consumer
//! (Bucket 2): the cfg-predicate evaluator landed with task beta/3987 but its
//! production consumer — task gamma/3990 (#cfg DAG import-gating) — is still
//! pending. The orphan-producer convention requires the marker until task 3990
//! wires a real caller; task 3990 is responsible for deleting this test when
//! that consumer lands.
//!
//! User-observable signal:
//!   `cargo test -p reify-compiler --test cfg_g_allow`
//!
//! The test shells out to `scripts/audit-orphan-producers.sh` with
//! `--scope crates/reify-compiler/src` and asserts *list membership*:
//! `cfg_satisfied` must be ABSENT from `orphans[]` and PRESENT exactly once
//! in `allowed[]`.
//!
//! Crucially, we do NOT assert `orphan_count == 0`: reify-compiler has 50+
//! pre-existing baseline orphans outside this task's scope, so such an
//! assertion would be spuriously RED.
//!
//! Graceful skip: if `python3`, `git`, or the script are absent from
//! PATH/disk, the shared helper prints a note to stderr and returns `None`;
//! the test then returns early. Mirrors `diagnostics_g_allow.rs` in
//! `reify-mesh-morph`. The shared helper is `reify_test_support::run_orphan_audit`.

use reify_test_support::run_orphan_audit;

#[test]
fn cfg_satisfied_is_g_allow_marked() {
    let Some(result) = run_orphan_audit("crates/reify-compiler/src") else {
        return;
    };

    let cfg_suffix = "crates/reify-compiler/src/cfg.rs";
    let fn_name = "cfg_satisfied";

    // (a) must NOT appear in orphans[] for cfg.rs
    let in_orphans = result["orphans"]
        .as_array()
        .expect("orphans must be an array")
        .iter()
        .any(|entry| {
            entry["file"]
                .as_str()
                .map(|f| f.ends_with(cfg_suffix))
                .unwrap_or(false)
                && entry["name"].as_str() == Some(fn_name)
        });

    assert!(
        !in_orphans,
        "`{fn_name}` in {cfg_suffix} is still listed as an orphan — \
         the `// G-allow:` marker may be missing or misplaced.\n\
         Full orphans list:\n{:#}",
        result["orphans"]
    );

    // (b) must appear in allowed[] exactly once, for cfg.rs
    let matching_allowed: Vec<_> = result["allowed"]
        .as_array()
        .expect("allowed must be an array")
        .iter()
        .filter(|entry| {
            entry["file"]
                .as_str()
                .map(|f| f.ends_with(cfg_suffix))
                .unwrap_or(false)
                && entry["name"].as_str() == Some(fn_name)
        })
        .collect();

    assert_eq!(
        matching_allowed.len(),
        1,
        "`{fn_name}` in {cfg_suffix} must appear exactly once in allowed[]; \
         found {} entries.\nFull allowed list:\n{:#}",
        matching_allowed.len(),
        result["allowed"]
    );

    // (c) the allow_reason must be non-empty. We deliberately do NOT pin
    //     any specific words: the wording is documentation, not behavior.
    let reason = matching_allowed[0]["allow_reason"]
        .as_str()
        .unwrap_or_default();
    assert!(
        !reason.is_empty(),
        "`{fn_name}` allow_reason must be non-empty"
    );
}
