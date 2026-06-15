//! Baseline ratchet tests for the PTODO detector (task δ, §6.6).
//!
//! Tests:
//!
//! (A) **`baseline_is_well_formed`** — always-on, hermetic. Reads
//!   `crates/reify-audit/ptodo-baseline.txt` (resolved via `CARGO_MANIFEST_DIR`
//!   so it works in any worktree), asserts the file EXISTS, and validates every
//!   non-empty line against the `path :: kind :: text` grammar. The committed
//!   baseline is intentionally EMPTY — the §6.4 zero-residual-debt end state —
//!   which this test accepts as well-formed (it asserts existence + grammar, not
//!   non-emptiness).
//!
//! (A′) **`validate_*`** — always-on, hermetic unit tests that drive crafted
//!   content through the shared `validate_baseline_content` validator, so the
//!   grammar/taxonomy/sort rules have real coverage that does NOT go inert while
//!   the committed baseline is empty.
//!
//! (B) **`live_findings_are_within_baseline`** — on-demand, `#[ignore]`.
//!   Runs `ptodo::check` over the real working tree and asserts every live
//!   source-marker fingerprint is a member of the committed baseline set
//!   (`live ⊆ baseline`).  Graceful skip if the repo root or git is unavailable;
//!   requires `REIFY_PTODO_TASKS_DB` for the liveness lane (see the test doc).
//!   Mirrors the `baseline_report_freshness` pattern.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test ptodo_baseline`               (A + A′)
//!   `cargo test -p reify-audit --test ptodo_baseline -- --ignored`  (A + A′ + B)
//!
//! On (B) failure — regenerate the baseline with the canonical generator
//! (`src/bin/ptodo-baseline-gen.rs`). It is the SINGLE source of truth: it maps
//! `ptodo::check` findings through the SAME `ptodo::fingerprint` this test uses,
//! so generation and the ratchet check can never drift (PRD §6.6). Do NOT hand-
//! derive fingerprints with `sed`/`jq` — a second derivation reintroduces the
//! drift this design exists to prevent.
//!   ```text
//!   REIFY_PTODO_TASKS_DB=/home/leo/src/reify/.taskmaster/tasks/tasks.db \
//!     cargo run -p reify-audit --bin ptodo-baseline-gen -- \
//!       --project-root /home/leo/src/reify \
//!     > crates/reify-audit/ptodo-baseline.txt
//!   ```
//!   `REIFY_PTODO_TASKS_DB` must point at the real `tasks.db` so the β liveness
//!   lane runs and orphaned/unknown-id residue is captured as a SUPERSET (a task
//!   worktree's `.taskmaster/` is untracked, so without it the lane degrades to
//!   structural-only).

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

/// Validate one `path :: kind :: text` fingerprint line against the §6.6
/// grammar. Returns `Err(reason)` when the line is ill-formed.
///
/// Pure (no I/O) so the rules it encodes are exercised by the `validate_*`
/// unit tests over synthetic content — independent of whether the committed
/// `ptodo-baseline.txt` happens to be empty.
fn check_baseline_line(line: &str) -> Result<(), String> {
    // Grammar: exactly two ` :: ` separators → three fields.
    let parts: Vec<&str> = line.splitn(3, " :: ").collect();
    if parts.len() != 3 {
        return Err(format!("expected 3 fields separated by ` :: ` but got {}", parts.len()));
    }
    let (fp_path, fp_kind, fp_text) = (parts[0], parts[1], parts[2]);

    if fp_path.is_empty() {
        return Err("empty path field".to_string());
    }
    if fp_kind.is_empty() {
        return Err("empty kind field".to_string());
    }
    if fp_text.is_empty() {
        // The no-colon fingerprint() branch emits exactly this shape; rejecting it
        // here is what keeps such a finding out of the committed baseline.
        return Err("empty text field".to_string());
    }
    // kind ∈ §8.3 taxonomy.
    if !VALID_KINDS.contains(&fp_kind) {
        return Err(format!("unknown kind {fp_kind:?}; valid kinds={VALID_KINDS:?}"));
    }
    // path has a swept extension …
    if !is_swept_ext(fp_path) {
        return Err(format!("path {fp_path:?} does not have a swept extension"));
    }
    // … and is NOT allowlisted (allowlisted paths never produce findings).
    if is_allowlisted(fp_path) {
        return Err(format!("path {fp_path:?} is allowlisted — it must not appear in the baseline"));
    }
    Ok(())
}

/// Validate baseline *content* against the full well-formedness contract: every
/// non-empty line is a well-formed triple (`check_baseline_line`) AND the lines
/// are strictly sorted ascending (which also forbids duplicates). Returns
/// `Err(reason)` on the first violation.
///
/// An EMPTY input is valid — it is the §6.4 zero-residual end state. Because
/// this is pure, the grammar/taxonomy/sort rules have real, permanent coverage
/// via the `validate_*` unit tests below, rather than going inert whenever the
/// committed baseline is empty.
fn validate_baseline_content(content: &str) -> Result<(), String> {
    let mut prev: Option<&str> = None;
    for (lineno, line) in content.lines().enumerate() {
        let n = lineno + 1;
        if line.is_empty() {
            continue;
        }
        check_baseline_line(line).map_err(|e| format!("line {n}: {e}; line={line:?}"))?;
        if let Some(prev) = prev
            && line <= prev
        {
            return Err(format!(
                "line {n}: baseline is not strictly sorted (duplicate or out of order); \
                 {prev:?} >= {line:?}"
            ));
        }
        prev = Some(line);
    }
    Ok(())
}

/// Asserts that `ptodo-baseline.txt` EXISTS and is well-formed
/// (`validate_baseline_content`): every non-empty line is a `path :: kind ::
/// text` triple with a §8.3-taxonomy `kind` on a swept, non-allowlisted source
/// `path`, and the lines are strictly sorted ascending with no duplicates.
///
/// An empty baseline PASSES — it is the §6.4 "zero residual debt" success state,
/// not a failure. This test therefore asserts existence + well-formedness, NOT
/// non-emptiness; the grammar rules themselves stay covered, whether or not the
/// committed file is empty, by the `validate_*` unit tests below.
#[test]
fn baseline_is_well_formed() {
    let path = baseline_path();

    assert!(
        path.exists(),
        "ptodo-baseline.txt not found at {path:?}.\n\
         Generate it with the canonical generator:\n\
         REIFY_PTODO_TASKS_DB=/home/leo/src/reify/.taskmaster/tasks/tasks.db \\\n\
           cargo run -p reify-audit --bin ptodo-baseline-gen -- \\\n\
             --project-root /home/leo/src/reify \\\n\
           > crates/reify-audit/ptodo-baseline.txt"
    );

    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));

    if let Err(e) = validate_baseline_content(&content) {
        panic!(
            "ptodo-baseline.txt is malformed: {e}\n\
             Regenerate it with the canonical generator (see the module doc)."
        );
    }
}

// -----------------------------------------------------------------------
// (A′) Synthetic-content coverage for the well-formedness rules
//
// The committed baseline is legitimately empty (zero residual debt), so
// `baseline_is_well_formed` alone would leave every grammar/taxonomy/sort rule
// inert (it would only exercise the `path.exists()` branch). These hermetic
// unit tests drive crafted content straight through the SAME
// `validate_baseline_content` validator, so the rules have real coverage that
// does not depend on what the committed file contains.
// -----------------------------------------------------------------------

#[test]
fn validate_accepts_empty_baseline() {
    // The §6.4 zero-residual end state: an empty (or newline-only) file is valid.
    assert!(validate_baseline_content("").is_ok());
    assert!(validate_baseline_content("\n").is_ok());
}

#[test]
fn validate_accepts_wellformed_sorted_triples() {
    let good = "crates/reify-eval/src/dispatcher.rs :: orphaned :: #4592 status=done: x\n\
                crates/reify-eval/src/engine_eval.rs :: untracked :: // TODO: y\n";
    assert!(validate_baseline_content(good).is_ok(), "well-formed sorted content must pass");
}

#[test]
fn validate_rejects_wrong_field_count() {
    assert!(validate_baseline_content("crates/x/y.rs :: untracked\n").is_err());
    assert!(validate_baseline_content("no separators at all\n").is_err());
}

#[test]
fn validate_rejects_empty_text_field() {
    // Exactly the shape the no-colon fingerprint() branch emits — it must be
    // rejected so such a finding can never silently enter the baseline.
    assert!(validate_baseline_content("crates/x/y.rs :: untracked :: \n").is_err());
}

#[test]
fn validate_rejects_unknown_kind() {
    assert!(validate_baseline_content("crates/x/y.rs :: bogus-kind :: // TODO: z\n").is_err());
}

#[test]
fn validate_rejects_non_swept_extension() {
    assert!(validate_baseline_content("docs/notes.md :: untracked :: prose\n").is_err());
}

#[test]
fn validate_rejects_allowlisted_path() {
    // crates/reify-audit/ is allowlisted (the detector's own crate self-matches).
    assert!(
        validate_baseline_content("crates/reify-audit/src/ptodo.rs :: untracked :: x\n").is_err()
    );
}

#[test]
fn validate_rejects_unsorted_or_duplicate() {
    let unsorted = "crates/b.rs :: untracked :: x\n\
                    crates/a.rs :: untracked :: y\n";
    assert!(validate_baseline_content(unsorted).is_err(), "out-of-order lines must fail");

    let duplicate = "crates/a.rs :: untracked :: x\n\
                     crates/a.rs :: untracked :: x\n";
    assert!(validate_baseline_content(duplicate).is_err(), "duplicate lines must fail");
}

// -----------------------------------------------------------------------
// (B) On-demand convergence test
// -----------------------------------------------------------------------

/// On-demand: run `ptodo::check` over the real repo and assert every live
/// source-marker fingerprint is ∈ the committed baseline (a subset check —
/// `live ⊆ baseline`).
///
/// **Task-DB requirement.** `ptodo::check` opens its OWN task DB via
/// `tasks_db_path(project_root)`, which honors the `REIFY_PTODO_TASKS_DB`
/// override (it does NOT read `ctx.conn`/`ctx.task_metadata` — those are
/// P1/P2/P5 inputs the PTODO lanes ignore). For the β liveness lane to run,
/// point `REIFY_PTODO_TASKS_DB` at the real `tasks.db`:
///
/// ```text
/// REIFY_PTODO_TASKS_DB=/home/leo/src/reify/.taskmaster/tasks/tasks.db \
///   cargo test -p reify-audit --test ptodo_baseline -- --ignored
/// ```
///
/// Without it (e.g. a task worktree whose `.taskmaster/` is untracked) the
/// liveness lane degrades to STRUCTURAL-only. The subset check stays SOUND
/// either way: the committed baseline is generated WITH the DB (a superset of
/// orphaned/unknown-id + structural fingerprints), so a structural-only live
/// set is still a subset. Liveness convergence is only meaningfully exercised
/// when the DB is supplied.
///
/// Graceful-skip if:
/// - The baseline file does not exist (not yet generated).
/// - `git` is not available (CI environments without a full checkout).
/// - The repo root cannot be determined.
///
/// On failure: regenerate the baseline with the canonical generator (see the
/// module doc), then re-run this test.
#[ignore = "on-demand convergence check; run via --ignored. Requires a real \
    repo checkout with git and, for the liveness lane, REIFY_PTODO_TASKS_DB \
    pointed at the real tasks.db. Graceful-skip when env is unavailable."]
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
    //
    // `conn` (in-memory), `jc`, and `task_metadata` are INERT placeholders: the
    // PTODO lanes read none of them. The β liveness lane opens its own task DB
    // via `tasks_db_path(project_root)` (honoring REIFY_PTODO_TASKS_DB; see the
    // test doc), so liveness classification depends on that env var, NOT on this
    // empty `conn`. With the DB absent the lane degrades to structural-only and
    // the subset check below still holds against the (superset) baseline.
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
         Regenerate the baseline with the canonical generator (it reuses the \
         SAME ptodo::fingerprint, so it cannot drift from this check):\n\
         REIFY_PTODO_TASKS_DB=/home/leo/src/reify/.taskmaster/tasks/tasks.db \\\n\
           cargo run -p reify-audit --bin ptodo-baseline-gen -- \\\n\
             --project-root /home/leo/src/reify \\\n\
           > crates/reify-audit/ptodo-baseline.txt",
        violations.len(),
        violations.join("\n"),
    );
}
