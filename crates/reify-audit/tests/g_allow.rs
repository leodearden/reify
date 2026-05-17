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
//! Anti-gaming rationale: this pin defends against gaming the orphan-producer script
//! via boilerplate `// G-allow:` markers — the script's regex (`//\s*G-allow:\s*(.+)`)
//! only requires non-blank reason text. Semantic accuracy (the reason names a real
//! deferred consumer or tracked task) is enforced by reviewer review; this test
//! surfaces surface-level approvals to reviewers. See esc-3667-113 triage.
//!
//! Graceful skip: if `python3` or `git` are absent from PATH, the test prints
//! a note to stderr and returns. Mirrors
//! `crates/reify-kernel-gmsh/tests/rpath_smoke.rs`.
//! The shared helper is in `reify_test_support::run_orphan_audit`.

use reify_test_support::run_orphan_audit;

#[test]
fn reify_audit_pub_fns_are_g_allow_marked() {
    let Some(result) = run_orphan_audit("crates/reify-audit/src") else {
        return;
    };

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
