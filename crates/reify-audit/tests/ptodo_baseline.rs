//! Baseline ratchet tests for the PTODO detector (task δ, §6.6).
//!
//! Two tests:
//!
//! (A) **`baseline_is_well_formed`** — always-on, hermetic. Reads
//!   `crates/reify-audit/ptodo-baseline.txt` (resolved via `CARGO_MANIFEST_DIR`
//!   so it works in any worktree), asserts the file EXISTS, and validates every
//!   non-empty line against the `path :: kind :: text` grammar.  RED until the
//!   baseline is generated in step-11; GREEN permanently after.
//!
//! (B) **`live_findings_are_within_baseline`** — on-demand, `#[ignore]`.
//!   Runs `ptodo::check` over the real working tree and asserts every live
//!   fingerprint is a member of the committed baseline set.  Graceful skip if
//!   the repo root or git is unavailable.  Mirrors the
//!   `baseline_report_freshness` pattern.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test ptodo_baseline`               (A only)
//!   `cargo test -p reify-audit --test ptodo_baseline -- --ignored`  (A + B)
//!
//! On (B) failure — regenerate the baseline (step-11 command):
//!   ```
//!   REIFY_PTODO_TASKS_DB=/home/leo/src/reify/.taskmaster/tasks/tasks.db \
//!     cargo run -p reify-audit --bin reify-audit -- \
//!       --pattern PTODO --project-root /home/leo/src/reify 2>/dev/null \
//!     | grep '^\[Medium\] PTodo' \
//!     | while read line; do
//!         path=$(echo "$line" | sed 's/.*task=\([^:]*\):.*/\1/')
//!         kind=$(echo "$line" | sed 's/.*task=[^:]*: \([a-z-]*\):.*/\1/')
//!         text=$(echo "$line" | sed 's/.*task=[^:]*: [a-z-]*: line [0-9]*: //')
//!         echo "$path :: $kind :: $text"
//!       done | sort -u > crates/reify-audit/ptodo-baseline.txt
//!   ```
//!   (Or use the `fingerprint()` Rust derivation via a small binary/script
//!   that calls `ptodo::check` and maps findings through `ptodo::fingerprint`.)

use reify_audit::ptodo::{fingerprint, is_allowlisted, is_swept_ext};
use std::collections::HashSet;
use std::path::Path;

/// Resolve the path to `ptodo-baseline.txt`:
///   CARGO_MANIFEST_DIR = `crates/reify-audit` → `./ptodo-baseline.txt`
fn baseline_path() -> std::path::PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("ptodo-baseline.txt")
}

/// Resolve the repo root from `CARGO_MANIFEST_DIR`:
///   `crates/reify-audit` → two `.parent()` → repo root
fn repo_root() -> std::path::PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .expect("crates/reify-audit has a parent (crates/)")
        .parent()
        .expect("crates/ has a parent (repo root)")
        .to_path_buf()
}

/// Valid `kind` tokens per the §8.3 finding taxonomy.
const VALID_KINDS: &[&str] =
    &["untracked", "malformed-cite", "phantom-tracking", "bare-ignore", "orphaned", "unknown-id"];

// -----------------------------------------------------------------------
// (A) Always-on well-formedness test
// -----------------------------------------------------------------------

/// Asserts that `ptodo-baseline.txt` exists and every non-empty line is a
/// well-formed `path :: kind :: text` fingerprint triple.
///
/// Checks performed per line:
/// - Matches the three-field `::` grammar (`path`, `kind`, `text` all non-empty).
/// - `kind` ∈ the §8.3 taxonomy (untracked / malformed-cite / phantom-tracking /
///   bare-ignore / orphaned / unknown-id).
/// - `path` has a swept extension (`is_swept_ext`).
/// - `path` is NOT allowlisted (`is_allowlisted`).
/// - Lines are strictly sorted ascending (no duplicates, lexicographic order).
///
/// RED until step-11 generates the file; GREEN permanently after.
#[test]
fn baseline_is_well_formed() {
    let path = baseline_path();

    assert!(
        path.exists(),
        "ptodo-baseline.txt not found at {path:?}.\n\
         Run step-11 to generate it:\n\
         REIFY_PTODO_TASKS_DB=/home/leo/src/reify/.taskmaster/tasks/tasks.db \\\n\
           cargo run -p reify-audit --bin reify-audit -- \\\n\
             --pattern PTODO --project-root /home/leo/src/reify"
    );

    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));

    // The file must end with a single newline (or be empty).
    // Each non-empty line is one fingerprint triple.
    let mut prev_line: Option<&str> = None;
    for (lineno, line) in content.lines().enumerate() {
        let n = lineno + 1;
        if line.is_empty() {
            continue;
        }

        // (1) Grammar: exactly two ` :: ` separators.
        let parts: Vec<&str> = line.splitn(3, " :: ").collect();
        assert_eq!(
            parts.len(),
            3,
            "line {n}: expected 3 fields separated by ` :: ` but got {}; line={line:?}",
            parts.len()
        );
        let (fp_path, fp_kind, fp_text) = (parts[0], parts[1], parts[2]);

        assert!(!fp_path.is_empty(), "line {n}: empty path field; line={line:?}");
        assert!(!fp_kind.is_empty(), "line {n}: empty kind field; line={line:?}");
        assert!(!fp_text.is_empty(), "line {n}: empty text field; line={line:?}");

        // (2) kind ∈ taxonomy.
        assert!(
            VALID_KINDS.contains(&fp_kind),
            "line {n}: unknown kind {fp_kind:?}; valid kinds={VALID_KINDS:?}; line={line:?}"
        );

        // (3) path has a swept extension.
        assert!(
            is_swept_ext(fp_path),
            "line {n}: path {fp_path:?} does not have a swept extension; line={line:?}"
        );

        // (4) path is NOT allowlisted (allowlisted paths never appear in findings).
        assert!(
            !is_allowlisted(fp_path),
            "line {n}: path {fp_path:?} is allowlisted — it must not appear in the baseline; line={line:?}"
        );

        // (5) Strictly sorted ascending (no duplicates).
        if let Some(prev) = prev_line {
            assert!(
                line > prev,
                "line {n}: baseline is not strictly sorted; {prev:?} >= {line:?}"
            );
        }
        prev_line = Some(line);
    }
}

// -----------------------------------------------------------------------
// (B) On-demand convergence test
// -----------------------------------------------------------------------

/// On-demand: run `ptodo::check` over the real repo and assert every live
/// fingerprint is ∈ the committed baseline.
///
/// Graceful-skip if:
/// - The baseline file does not exist (step-11 not yet run).
/// - `git` is not available (CI environments without a full checkout).
/// - The repo root cannot be determined.
///
/// On failure: regenerate the baseline per the step-11 command in the module
/// doc, then re-run this test.
#[ignore = "on-demand convergence check; run via --ignored. Requires a real \
    repo checkout with git and (for liveness findings) a tasks.db. \
    Graceful-skip when env is unavailable."]
#[test]
fn live_findings_are_within_baseline() {
    // Graceful-skip if baseline not yet generated.
    let bp = baseline_path();
    if !bp.exists() {
        eprintln!(
            "ptodo_baseline: skipping convergence test — baseline file not found at {bp:?}"
        );
        return;
    }

    // Graceful-skip if git is not available.
    let git_ok = std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok();
    if !git_ok {
        eprintln!("ptodo_baseline: skipping convergence test — git not available");
        return;
    }

    let root = repo_root();

    // Graceful-skip if this does not look like a real repo.
    if !root.join(".git").exists() && !root.join(".git").is_file() {
        eprintln!(
            "ptodo_baseline: skipping convergence test — {root:?} is not a git repo"
        );
        return;
    }

    // Load the committed baseline into a HashSet<String>.
    let baseline_content = std::fs::read_to_string(&bp)
        .unwrap_or_else(|e| panic!("failed to read {bp:?}: {e}"));
    let baseline: HashSet<String> = baseline_content
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    // Run ptodo::check over the real working tree.
    use reify_audit::{AuditContext, MockJCodemunchOps, RealGitOps};
    use rusqlite::Connection;
    use std::collections::HashMap;

    let git = RealGitOps::new(root.clone());
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    let jc = MockJCodemunchOps::new();
    let ctx = AuditContext {
        project_root: root.clone(),
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };

    let findings = reify_audit::ptodo::check(&ctx);

    // Map every finding through fingerprint() and assert membership.
    //
    // Restrict the convergence check to findings representable in the
    // source-marker baseline grammar (the same boundary `baseline_is_well_formed`
    // enforces): a swept, non-allowlisted SOURCE PATH key and a §8.3 taxonomy
    // kind. The α structural and β liveness lanes are path-keyed (`task_id` = the
    // swept file), so they pass this gate. The ζ inverse lane, by contrast, emits
    // `task-cites-deleted-path` findings keyed by TASK ID (e.g. `task_id = "2560"`)
    // with a kind outside the baseline taxonomy — a task-DB-metadata hygiene
    // class, not source-marker debt. Such findings can NEVER appear in
    // `ptodo-baseline.txt` (they would fail `baseline_is_well_formed`'s swept-ext
    // and kind-taxonomy assertions), so demanding their membership here would be a
    // category error that no well-formed baseline could satisfy. They remain
    // surfaced by the `reify-audit --pattern PTODO` binary and are remediated via
    // task-metadata curation; they are simply out of scope for the source-marker
    // baseline ratchet this test guards.
    let mut violations: Vec<String> = Vec::new();
    for f in &findings {
        if !is_swept_ext(&f.task_id) {
            continue;
        }
        let fp = fingerprint(f);
        if !baseline.contains(&fp) {
            violations.push(fp);
        }
    }

    assert!(
        violations.is_empty(),
        "{} live PTODO finding(s) are not in the committed baseline:\n{}\n\n\
         Regenerate the baseline (step-11 command):\n\
         REIFY_PTODO_TASKS_DB=/home/leo/src/reify/.taskmaster/tasks/tasks.db \\\n\
           cargo run -p reify-audit --bin reify-audit -- \\\n\
             --pattern PTODO --project-root /home/leo/src/reify 2>/dev/null \\\n\
           | tee /tmp/ptodo-findings.txt\n\
         Then derive fingerprints via ptodo::fingerprint() and write to \
         crates/reify-audit/ptodo-baseline.txt (sorted, deduplicated).",
        violations.len(),
        violations.join("\n"),
    );
}
