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
//! string cites *some* tracked owner task as `#NNNN`.  The specific task
//! number lives only in the source `// G-allow:` marker (single source of
//! truth), so this test never carries a second copy that could drift.
//! Neither assertion implies `orphan_count == 0`; pre-existing baseline
//! orphans in unrelated files are intentionally not in scope here.
//!
//! **Owner task**: #3765 (KCC-γ: Widen value_for_joint/joint_range_midpoint/
//! chain_transform to JointValue; multi-DOF chain participation; analytic J
//! planar+spherical).
//!
//! **Removal contract**: KCC-γ (#3765) MUST delete this file as part of the
//! consumer-wiring commit.  **Important scope caveat**: this test runs the
//! audit with `--scope crates/reify-stdlib/src` (see note below), so a
//! consumer wired in another crate (e.g. `reify-solver-elastic`, `reify-eval`)
//! will NOT be visible to the audit and will NOT decrement the caller count
//! here.  Assertion (b) will therefore NOT auto-trip when the consumer lands.
//! KCC-γ is responsible for manually deleting this file; it cannot rely on
//! CI-breakage as the retirement signal.
//!
//! Graceful skip: if `python3`, `git`, or the audit script are absent
//! from PATH/disk the test prints a note to stderr and returns without
//! failing.  The shared helper is `reify_test_support::run_orphan_audit`.
//!
//! Review batch: review_id 20260518T092329.

use reify_test_support::run_orphan_audit;

/// (file_suffix, fn_name)
///
/// `file_suffix` is the suffix of the `file` field in the JSON output
/// (repo-relative path from the workspace root).
const PINS: &[(&str, &str)] = &[
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "dof_count",
    ),
    // as_f64_slice: wired by KCC-γ #3765 — callers in loop_closure.rs +
    // loop_closure_solver.rs; no longer an orphan in crates/reify-stdlib/src.
    // from_slice: wired by KCC-γ #3765 — callers in loop_closure_solver.rs.
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "renormalize_quaternion",
    ),
    // from_str: wired by KCC-γ #3765 — callers in snapshot.rs +
    // loop_closure_solver.rs.
    // flat_len: wired by KCC-γ #3765 — callers in snapshot.rs +
    // loop_closure_solver.rs.
    (
        "crates/reify-stdlib/src/loop_closure_value.rs",
        "flatten_dofs",
    ),
    // unflatten_dofs: wired by KCC-γ #3765 — callers in snapshot.rs.
];

#[test]
fn new_orphans_2026_05_18_are_g_allow_marked() {
    // Scope is intentionally narrowed to crates/reify-stdlib/src rather than
    // the wide "crates/reify-*/src" glob.  Reason: common names such as
    // from_slice/from_str/dof_count collide with same-named pub fns in other
    // crates (e.g. PreludeContext::from_slice in reify-compiler), producing
    // false-positive callers>0 that push these fns out of orphans[]/allowed[].
    // file_suffix .ends_with() matching still works correctly at this scope.
    //
    // TRADE-OFF: the narrow scope means a consumer wired in another crate will
    // NOT increment caller counts here, so assertion (b) will NOT auto-fail
    // when KCC-γ lands consumers.  Deletion of this file is therefore a MANUAL
    // obligation for KCC-γ (#3765) — see "Removal contract" in the module doc.
    let Some(result) = run_orphan_audit("crates/reify-stdlib/src") else {
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
             PINS in `crates/reify-audit/tests/new_orphans_2026_05_18_g_allow.rs`.\n\
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
