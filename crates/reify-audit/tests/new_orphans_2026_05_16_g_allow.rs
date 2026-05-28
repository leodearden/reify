//! Pin: the 15 remaining `pub fn` surfaced as new-since-baseline orphans by the
//! 2026-05-16 G-tool audit must each carry a `// G-allow:` marker citing
//! the tracked owner task.  The 15 functions span 5 crates:
//! reify-compiler, reify-eval, reify-kernel-occt, reify-solver-elastic,
//! reify-types.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test new_orphans_2026_05_16_g_allow`
//!
//! Anti-gaming rationale: every new public producer must cite a tracked
//! consumer task so reviewers can verify the intended call-site wiring.
//! The orphan-producer audit script (`scripts/audit-orphan-producers.sh`)
//! enforces presence of a `// G-allow:` marker on the line immediately
//! above each `pub fn`; this test additionally asserts list membership
//! (absent from `orphans[]`, present in `allowed[]`) and that the reason
//! string cites *some* tracked owner task as `#NNNN`.  The specific task
//! number lives only in the source `// G-allow:` marker (single source of
//! truth), so this test never carries a second copy that could drift.
//! Neither assertion implies `orphan_count == 0`; 400+ pre-existing baseline
//! orphans in unrelated files are intentionally not in scope here.
//!
//! **Removal contract**: each PINS entry is owned by the task cited in its
//! source `// G-allow:` marker.  Once that task wires its consumer the
//! function gains a non-test caller, leaves `allowed[]`, and assertion (b)
//! below will fail with "found 0 entries".  The owning task MUST delete its
//! row from `PINS` as part of the consumer-wiring commit.  Delete this file
//! entirely when all rows are removed.  The failure message includes the fn
//! name — search for it in this file when
//! `assert_eq!(matching_allowed.len(), 1)` fires unexpectedly.
//!
//! Graceful skip: if `python3`, `git`, or the audit script are absent
//! from PATH/disk the test prints a note to stderr and returns without
//! failing.  The shared helper is `reify_test_support::run_orphan_audit`.
//!
//! Review batch: review_id 20260516T112551.

use reify_test_support::run_orphan_audit;

/// (file_suffix, fn_name)
///
/// `file_suffix` is the suffix of the `file` field in the JSON output
/// (repo-relative path from the workspace root).
const PINS: &[(&str, &str)] = &[
    (
        "crates/reify-compiler/src/annotations/schema.rs",
        "lookup_schema",
    ),
    (
        "crates/reify-compiler/src/lib.rs",
        "__validate_annotations_for_parity_test",
    ),
    (
        "crates/reify-eval/src/dispatcher.rs",
        "kernel_pragma_unsatisfiable_diagnostic",
    ),
    (
        "crates/reify-eval/src/dispatcher.rs",
        "pinned_kernel_missing_diagnostic",
    ),
    (
        "crates/reify-eval/src/dispatcher.rs",
        "unpinned_kernel_loaded_diagnostic",
    ),
    (
        "crates/reify-eval/src/dispatcher.rs",
        "kernel_version_mismatch_diagnostic",
    ),
    (
        "crates/reify-eval/src/engine_admin.rs",
        "drain_and_record_warm_pool_events",
    ),
    (
        "crates/reify-eval/src/geometry_ops.rs",
        "cap_kind_translation",
    ),
    (
        "crates/reify-eval/src/persistent_cache.rs",
        "sweep_stale_tempfiles",
    ),
    (
        "crates/reify-eval/src/persistent_cache.rs",
        "prune_orphan_engine_version_dirs",
    ),
    (
        "crates/reify-kernel-occt/src/lib.rs",
        "store_vertex_at_for_test",
    ),
    (
        "crates/reify-solver-elastic/src/assembly/global.rs",
        "detect_orphan_dofs",
    ),
    (
        "crates/reify-ir/src/geometry.rs",
        "capability_kind",
    ),
    (
        "crates/reify-ir/src/value.rs",
        "format_display_triple",
    ),
];

#[test]
fn new_orphans_2026_05_16_are_g_allow_marked() {
    let Some(result) = run_orphan_audit("crates/reify-*/src") else {
        return;
    };

    for &(file_suffix, fn_name) in PINS {
        // (a) Must NOT appear in orphans[] for the given file.
        let in_orphans = result["orphans"]
            .as_array()
            .expect("orphans must be an array")
            .iter()
            .any(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(file_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            });

        assert!(
            !in_orphans,
            "`{fn_name}` in {file_suffix} is still listed as an orphan — \
             the `// G-allow:` marker may be missing or misplaced (must be \
             on the line immediately above `pub fn`, with no blank line).\n\
             Full orphans list:\n{:#}",
            result["orphans"]
        );

        // (b) Must appear EXACTLY ONCE in allowed[] for the given file.
        let matching_allowed: Vec<_> = result["allowed"]
            .as_array()
            .expect("allowed must be an array")
            .iter()
            .filter(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(file_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            })
            .collect();

        assert_eq!(
            matching_allowed.len(),
            1,
            "`{fn_name}` in {file_suffix} must appear exactly once in \
             allowed[]; found {} entries.  If you just wired a consumer \
             for `{fn_name}`, delete its row from \
             PINS in `crates/reify-audit/tests/new_orphans_2026_05_16_g_allow.rs`.\n\
             Full allowed list:\n{:#}",
            matching_allowed.len(),
            result["allowed"]
        );

        // (c) The allow_reason must cite SOME tracked owner task as `#NNNN`.
        // The specific number lives only in the source `// G-allow:` marker
        // (single source of truth), so it cannot drift against a second copy here.
        let reason = matching_allowed[0]["allow_reason"].as_str().unwrap_or_default();
        let bytes = reason.as_bytes();
        let cites_task = bytes.iter().enumerate().any(|(i, &b)| {
            b == b'#' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
        });
        assert!(
            cites_task,
            "`{fn_name}` allow_reason must cite a tracked task as `#NNNN`; got: {reason:?}"
        );
    }
}
