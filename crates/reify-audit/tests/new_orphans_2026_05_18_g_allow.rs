//! Pin: the 8 `pub fn` in `crates/reify-stdlib/src/loop_closure_value.rs`
//! introduced by task #3763 (KCC-α-pre) must each carry a `// G-allow:`
//! marker citing tracked successor task #3765 (KCC-γ).
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test new_orphans_2026_05_18_g_allow`
//!
//! Anti-gaming rationale: every new public producer must cite a tracked
//! consumer task so reviewers can verify the intended call-site wiring.
//! The orphan-producer audit script (`scripts/audit-orphan-producers.sh`)
//! enforces presence of a `// G-allow:` marker on the line immediately
//! above each `pub fn`; this test additionally asserts list membership
//! (absent from `orphans[]`, present in `allowed[]`) and that the reason
//! string contains the expected owner-task citation `#3765`.  Neither
//! assertion implies `orphan_count == 0`; pre-existing baseline orphans
//! in unrelated files are intentionally not in scope here.
//!
//! **Owner task**: #3765 (KCC-γ: Widen value_for_joint/joint_range_midpoint/
//! chain_transform to JointValue; multi-DOF chain participation; analytic J
//! planar+spherical).
//!
//! **Removal contract**: when KCC-γ (#3765) wires real consumers for these
//! functions, each function gains a non-test caller, leaves `allowed[]`, and
//! assertion (b) below will fail with "found 0 entries".  KCC-γ MUST delete
//! this file entirely as part of the consumer-wiring commit.
//!
//! Graceful skip: if `python3`, `git`, or the audit script are absent
//! from PATH/disk the test prints a note to stderr and returns without
//! failing.  The shared helper is `reify_test_support::run_orphan_audit`.
//!
//! Review batch: review_id 20260518T092329.

use reify_test_support::run_orphan_audit;

/// (file_suffix, fn_name, expected_task_substring)
///
/// `file_suffix` is the suffix of the `file` field in the JSON output
/// (repo-relative path from the workspace root).
/// `expected_task_substring` is a string that must appear in the
/// `allow_reason` field of the matching `allowed[]` entry.
const PINS: &[(&str, &str, &str)] = &[
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "dof_count",
        "3765",
    ),
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "as_f64_slice",
        "3765",
    ),
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "from_slice",
        "3765",
    ),
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "renormalize_quaternion",
        "3765",
    ),
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "from_str",
        "3765",
    ),
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "flat_len",
        "3765",
    ),
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "flatten_dofs",
        "3765",
    ),
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "unflatten_dofs",
        "3765",
    ),
];

#[test]
fn new_orphans_2026_05_18_are_g_allow_marked() {
    let Some(result) = run_orphan_audit("crates/reify-*/src") else {
        return;
    };

    for &(file_suffix, fn_name, expected_task_substr) in PINS {
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
             for task #{expected_task_substr}, delete this fn's row from \
             PINS in `crates/reify-audit/tests/new_orphans_2026_05_18_g_allow.rs`.\n\
             Full allowed list:\n{:#}",
            matching_allowed.len(),
            result["allowed"]
        );

        // (c) The allow_reason must cite the expected owner task as `#NNNN`
        // (anchored on the `#` prefix to avoid false matches on bare
        // numeric substrings such as line numbers or unrelated task IDs).
        let reason = matching_allowed[0]["allow_reason"]
            .as_str()
            .unwrap_or_default();
        let expected_task_citation = format!("#{expected_task_substr}");
        assert!(
            reason.contains(&expected_task_citation),
            "`{fn_name}` allow_reason must contain the task citation \
             \"{expected_task_citation}\"; got: {reason:?}"
        );
    }
}
