//! Baseline ratchet tests for the PTODO detector (task δ, §6.6).
//!
//! Tests:
//!
//! (A) **`baseline_is_well_formed`** — always-on, hermetic. Reads
//!   `crates/reify-audit/ptodo-baseline.txt` (resolved via `CARGO_MANIFEST_DIR`
//!   so it works in any worktree), asserts the file EXISTS, and validates every
//!   non-empty line against the `path :: kind :: text` grammar. This test
//!   asserts existence + grammar, not emptiness either way.
//!
//!   The committed baseline was EMPTY (the §6.4 zero-residual-debt end state)
//!   until task #6087 added the §8.1 lane δ-A recognizer — an
//!   `#[allow(…dead_code…)]` attribute whose trailing rationale defers the
//!   work. That lane surfaced a pre-existing population of 14 findings which
//!   fingerprint (line-number-erased, deduped) to the 5 committed entries, and
//!   they were seeded in the same diff as a SHRINK-ONLY grandfather set.
//!   §6.6's ratchet cannot grow, so those entries can only be burned down: when
//!   an underlying comment is re-pointed at a live task or the deferred work
//!   lands, its baseline line is deleted. Every seeded entry was hand-inspected
//!   as a genuine deferral; none is a false positive. Three kinds are
//!   represented, per §8.3's lane-independent taxonomy: one `orphaned` (cites a
//!   `done` task), one `malformed-cite` (the legacy `task NNNN` form), and three
//!   `untracked` (no cite at all).
//!
//! (A′) **`validate_*`** — always-on, hermetic unit tests that drive crafted
//!   content through the shared `validate_baseline_content` validator, so the
//!   grammar/taxonomy/sort rules have real coverage independent of whatever the
//!   committed baseline happens to contain (they were written while it was
//!   empty and stay meaningful now that it is not).
//!
//! (B) **`live_findings_are_within_baseline`** — on-demand, `#[ignore]`.
//!   Runs `ptodo::check` over the real working tree and asserts every live
//!   source-marker fingerprint is a member of the committed baseline set
//!   (`live ⊆ baseline`).  Graceful skip if the repo root or git is unavailable;
//!   requires `REIFY_PTODO_TASKS_DB` for the liveness lane (see the test doc).
//!   Mirrors the `baseline_report_freshness` pattern.
//!
//! (C) **`generator_emits_scan_evidence_*`** — always-on, hermetic. Runs the
//!   real `ptodo-baseline-gen` binary over staged tempdir git fixtures and pins
//!   the §6.6 scan-evidence contract it emits on stderr
//!   (`@@PTODO_SCAN@@ files_scanned=<N> markers_examined=<M>`): exactly one such
//!   line per run, carrying the REAL counts, never leaking onto stdout, and
//!   still emitted when the tree is clean and stdout is empty. Graceful-skip if
//!   `git` is unavailable.
//!
//! (D) **fixture git-env hygiene** — always-on. Pins that the two fixture
//!   command builders (C) drives the real binary through strip every
//!   `reify_audit::git_env::REPO_REDIRECT_VARS` entry, and replays (C) under a
//!   real ambient hook git environment. Rationale lives in
//!   `reify_test_support::git_env`; not restated here.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test ptodo_baseline`               (A + A′ + C + D)
//!   `cargo test -p reify-audit --test ptodo_baseline -- --ignored`  (A + A′ + B + C + D)
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

mod common;

use reify_audit::ptodo::{fingerprint, is_allowlisted, is_g_allow_finding, is_swept_ext};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

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
/// unit tests over synthetic content — independent of what the committed
/// `ptodo-baseline.txt` happens to contain.
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
/// An EMPTY input remains valid — it is the §6.4 zero-residual end state, and
/// the shrink-only ratchet's goal. It is no longer the CURRENT state: task
/// #6087 seeded 5 grandfathered lane δ-A entries. Because this is pure, the
/// grammar/taxonomy/sort rules have real, permanent coverage via the
/// `validate_*` unit tests below regardless of the committed content.
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
/// not a failure. This test asserts existence + well-formedness, NOT emptiness
/// in either direction; the grammar rules themselves stay covered, whatever the
/// committed file contains, by the `validate_*` unit tests below. The committed
/// file currently carries the 5 lane δ-A entries seeded by task #6087.
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

// NOTE (task #6087, amendment): there is deliberately NO test here asserting
// that a specific lane δ-A entry is PRESENT in the committed baseline. Such a
// test cannot provide the recognizer-regression coverage it would appear to —
// it reads a static file the same commit authored, and the §6.6 ratchet is a
// SUBSET oracle (`live ⊆ baseline`), so a recognizer that stops firing shrinks
// the live set and leaves both green. The only state it detects is someone
// editing the baseline, which is exactly the shrink-only burn-down flow the
// ratchet exists to allow. Real regression coverage for the δ-A user-observable
// signal lives in `check_allow_dead_code_deferral_lane` (tests/ptodo.rs), which
// drives `ptodo::check` end-to-end against a seeded `done` cite and asserts the
// High `orphaned:` summary.

// -----------------------------------------------------------------------
// (A′) Synthetic-content coverage for the well-formedness rules
//
// These hermetic unit tests drive crafted content straight through the SAME
// `validate_baseline_content` validator, so every grammar/taxonomy/sort rule has
// real coverage that does not depend on what the committed file contains. They
// were written while the baseline was empty — when `baseline_is_well_formed`
// alone would have exercised only the `path.exists()` branch — and they remain
// the permanent home of that coverage now that it is not.
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
    // `task-cites-deleted-path` and `task-cites-renamed-path` findings keyed by
    // TASK ID (e.g. `task_id = "2560"`) with kinds outside the baseline
    // taxonomy — a task-DB-metadata hygiene class, not source-marker debt. Such
    // findings can NEVER appear in `ptodo-baseline.txt` (they would fail
    // `baseline_is_well_formed`'s swept-ext and kind-taxonomy assertions), so
    // demanding their membership here would be a category error that no
    // well-formed baseline could satisfy. They remain surfaced by the
    // `reify-audit --pattern PTODO` binary and are remediated via task-metadata
    // curation; they are simply out of scope for the source-marker baseline
    // ratchet this test guards.
    let mut violations: Vec<String> = Vec::new();
    for f in &findings {
        if !is_swept_ext(&f.task_id) {
            // ζ inverse findings are keyed by TASK ID (not a swept path) —
            // excluded from the source-marker baseline (they would fail the
            // swept-ext and kind-taxonomy assertions in baseline_is_well_formed).
            continue;
        }
        if is_g_allow_finding(f) {
            // G-allow advisory findings (g-allow-orphaned / g-allow-unknown-id)
            // are path-keyed (.rs files) so they pass the is_swept_ext gate, but
            // their kind strings are outside VALID_KINDS — a regen including them
            // would fail baseline_is_well_formed's kind check. Exclude explicitly,
            // mirroring the ζ exclusion above and the ptodo-baseline-gen filter.
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

// ---------------------------------------------------------------------------
// (C) Generator scan-evidence contract (task #6241, PRD §6.6)
//
// `ptodo-baseline-gen` emits, on STDERR, one machine-readable line per run:
//
//     @@PTODO_SCAN@@ files_scanned=<N> markers_examined=<M>
//
// That line is the RUN evidence the §6.6 vacuity floor in
// tests/infra/test_reify_audit_ptodo.sh keys on. These tests drive the real
// binary over hermetic git fixtures and pin the contract end to end: the line
// exists, carries the REAL counts (not a constant), stays off stdout (stdout is
// the baseline stream — a leak would corrupt the next regen), and is emitted
// even when the tree is clean and stdout is empty.
//
// Rationale lives in docs/prds/reify-audit-ptodo-detector.md §6.6 and is
// deliberately not restated here.
// ---------------------------------------------------------------------------

/// Assemble a comment marker at RUNTIME so this test source never self-matches
/// the detector it drives (the same self-match-safety idiom the hermetic
/// scenarios in tests/infra/test_reify_audit_ptodo.sh use).
fn untracked_marker(body: &str) -> String {
    format!("// {}{}: {body}\n", "TO", "DO")
}

/// A `git` command targeting the fixture repo at `root`.
///
/// Built through the shared `git -C <root>` constructor, as this crate's
/// sibling git-fixture test binaries are. The rule lives in
/// [`reify_audit::git_env`] and the failure mode it prevents in
/// [`reify_test_support::git_env`]; neither is restated here.
fn fixture_git_cmd(root: &Path) -> Command {
    common::git_env::git_cmd(root)
}

/// Run `git` in `root` with ambient git env stripped, panicking on failure.
fn git_in(root: &Path, args: &[&str]) {
    let out = fixture_git_cmd(root)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Stage a hermetic fixture repo containing `files` (relative path → content)
/// and return its tempdir handle (kept alive by the caller).
fn staged_fixture(files: &[(&str, String)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git_in(root, &["init", "-q"]);
    for (rel, content) in files {
        let full = root.join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("create_dir_all");
        }
        std::fs::write(&full, content).expect("write fixture file");
    }
    // `RealGitOps::ls_files` lists INDEX entries, so staging is enough — no
    // commit (and therefore no user.name/user.email config) is required.
    git_in(root, &["add", "-A"]);
    dir
}

/// The real generator binary aimed at `root`, with `REIFY_PTODO_TASKS_DB`
/// removed (the β liveness lane then degrades fail-soft, which is what a
/// hermetic fixture wants).
///
/// Sanitized DIRECTLY rather than built through [`reify_audit::git_env`]'s
/// `git -C <root>` constructor: the program here is a reify binary that runs
/// git internally, not git itself, which is the other-shape case
/// [`reify_test_support::git_env::sanitize`] sanctions.
fn generator_cmd(root: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ptodo-baseline-gen"));
    cmd.arg("--project-root")
        .arg(root)
        .env_remove("REIFY_PTODO_TASKS_DB");
    reify_audit::git_env::sanitize(&mut cmd);
    cmd
}

/// Run the real generator binary against `root`.
fn run_generator(root: &Path) -> std::process::Output {
    generator_cmd(root)
        .output()
        .expect("ptodo-baseline-gen spawns")
}

/// Extract the single `@@PTODO_SCAN@@` line's counters from `stderr`, asserting
/// there is EXACTLY one such line and that both REQUIRED fields are well formed.
///
/// Mirrors the PRD §6.6 grammar rules exactly, so the two consumers of this
/// machine contract agree on how strict it is:
///   - MULTIPLICITY: exactly one line per run.  This is the strict consumer and
///     asserts it; the shell floor deliberately reads only the first (`grep -m1`
///     in `tests/infra/test_reify_audit_ptodo.sh`) rather than policing the count.
///   - EXTENSIBILITY: the field list is OPEN for additive extension.  An
///     unrecognised `key=value` token is IGNORED, so appending a future counter
///     stays backward compatible and cannot turn this contract test RED.  Only a
///     MISSING required field (`files_scanned` / `markers_examined`) or an
///     unparseable value panics.
fn parse_scan_line(stderr: &str) -> (usize, usize) {
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("@@PTODO_SCAN@@"))
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "expected exactly one @@PTODO_SCAN@@ line on stderr; got {}:\n{stderr}",
        lines.len()
    );
    let line = lines[0].trim();
    let rest = line
        .strip_prefix("@@PTODO_SCAN@@ ")
        .unwrap_or_else(|| panic!("scan line must start with the bare token: {line:?}"));
    let mut files: Option<usize> = None;
    let mut markers: Option<usize> = None;
    for field in rest.split_whitespace() {
        if let Some(v) = field.strip_prefix("files_scanned=") {
            files = Some(v.parse().unwrap_or_else(|e| {
                panic!("files_scanned must be an integer ({v:?}): {e}")
            }));
        } else if let Some(v) = field.strip_prefix("markers_examined=") {
            markers = Some(v.parse().unwrap_or_else(|e| {
                panic!("markers_examined must be an integer ({v:?}): {e}")
            }));
        }
        // else: an unrecognised token is an ADDITIVE extension of the grammar —
        // ignored by contract, never a failure (PRD §6.6).
    }
    (
        files.unwrap_or_else(|| panic!("scan line lacks files_scanned: {line:?}")),
        markers.unwrap_or_else(|| panic!("scan line lacks markers_examined: {line:?}")),
    )
}

/// (C1) The generator emits the §6.6 scan-evidence line on stderr with the REAL
/// counters, and the line never leaks onto stdout.
///
/// Fixture: two staged swept files — `src/fresh.rs` carrying exactly one
/// marker, `src/clean.rs` carrying none — so the expected evidence is
/// `files_scanned=2 markers_examined=1` by construction.
#[test]
fn generator_emits_scan_evidence_with_real_counts() {
    if std::process::Command::new("git").arg("--version").output().is_err() {
        eprintln!("ptodo_baseline: skipping scan-evidence test — git not available");
        return;
    }

    let fixture = staged_fixture(&[
        ("src/fresh.rs", untracked_marker("wire the fixture up")),
        ("src/clean.rs", "pub fn clean() -> u32 { 7 }\n".to_string()),
    ]);
    let out = run_generator(fixture.path());
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    // (iv) exit status.
    assert!(
        out.status.success(),
        "generator must exit 0; status={:?}\nstderr:\n{stderr}",
        out.status.code()
    );

    // (i)+(ii) exactly one well-formed scan line, carrying the real counts.
    let (files_scanned, markers_examined) = parse_scan_line(&stderr);
    assert_eq!(
        files_scanned, 2,
        "files_scanned must be the fixture's swept staged file count (src/fresh.rs, \
         src/clean.rs); stderr:\n{stderr}"
    );
    assert_eq!(
        markers_examined, 1,
        "markers_examined must be the fixture's marker-line count (1 in src/fresh.rs, \
         0 in src/clean.rs); stderr:\n{stderr}"
    );

    // (iii) stdout is still the fingerprint stream, and the scan line did NOT
    // leak onto it (a leak would corrupt ptodo-baseline.txt on the next regen).
    assert!(
        !stdout.contains("@@PTODO_SCAN@@"),
        "the scan line must never reach stdout (it is the baseline stream):\n{stdout}"
    );
    let fp_lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        fp_lines.len(),
        1,
        "expected exactly one fingerprint on stdout; got {fp_lines:?}"
    );
    let parts: Vec<&str> = fp_lines[0].split(" :: ").collect();
    assert_eq!(
        parts.len(),
        3,
        "stdout must keep the `path :: kind :: text` grammar; got {:?}",
        fp_lines[0]
    );
    assert_eq!(parts[0], "src/fresh.rs", "fingerprint path key");
    assert_eq!(parts[1], "untracked", "fingerprint kind token");
}

/// (C2) MARKER-FREE REPO — the generator still emits the scan line (with
/// `files_scanned >= 1`) while stdout is EMPTY.
///
/// This is the generator-level witness of the "detector ran, tree is clean"
/// partition, and the exact shape the §6.6 shell floor keys on: a floor on the
/// emitted fingerprint count cannot tell this state apart from "the generator
/// never ran", whereas the scan line can.
#[test]
fn generator_emits_scan_evidence_on_a_marker_free_repo() {
    if std::process::Command::new("git").arg("--version").output().is_err() {
        eprintln!("ptodo_baseline: skipping marker-free scan-evidence test — git not available");
        return;
    }

    let fixture = staged_fixture(&[
        ("src/clean_a.rs", "pub fn a() -> u32 { 1 }\n".to_string()),
        ("src/clean_b.rs", "pub fn b() {}\n".to_string()),
    ]);
    let out = run_generator(fixture.path());
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        out.status.success(),
        "generator must exit 0 on a clean tree; status={:?}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(
        stdout.is_empty(),
        "a marker-free repo emits no fingerprints; got stdout:\n{stdout}"
    );

    let (files_scanned, markers_examined) = parse_scan_line(&stderr);
    assert_eq!(
        files_scanned, 2,
        "both marker-free swept files must count as scanned; stderr:\n{stderr}"
    );
    assert!(
        files_scanned >= 1,
        "scan evidence must be non-vacuous even with an empty baseline stream; \
         stderr:\n{stderr}"
    );
    assert_eq!(
        markers_examined, 0,
        "a marker-free repo examines no markers; stderr:\n{stderr}"
    );
}

/// (C3) EXTENSIBILITY, parser level — `parse_scan_line` IGNORES an unrecognised
/// `key=value` token and still returns the two required counters.
///
/// C1/C2 above drive the REAL generator, which emits exactly `files_scanned`
/// and `markers_examined`, so the parser's additive-extension branch never
/// executes there and the documented promise ("appending a future counter
/// stays backward compatible and cannot turn this contract test RED", PRD §6.6)
/// went unexercised on both sides of the contract. Driving the parser directly
/// — no fixture, no spawned binary — is what makes that branch reachable at all.
///
/// The fixture deliberately carries TWO shapes of extra token:
///   * `future_counter=9` — the plain additive case;
///   * `skipped_files_scanned=0` — the ADVERSARIAL one, whose name ends with a
///     required key. A parser matching by SUBSTRING rather than by whole token
///     reads this 0 as the file count; that is exactly the defect the shell
///     consumer shipped with (see fixture (vi) in
///     tests/infra/test_reify_audit_ptodo.sh, the mirror of this test). Pinning
///     it on BOTH sides is what keeps one grammar from growing two parsers.
///
/// Field ORDER is also varied here (`markers_examined` first) — the grammar is
/// a token set, not a sequence, and neither consumer may assume otherwise.
#[test]
fn parse_scan_line_ignores_unrecognised_tokens() {
    let stderr = "ptodo-baseline-gen: 4 fingerprint(s) emitted\n\
                  @@PTODO_SCAN@@ markers_examined=4 future_counter=9 \
                  files_scanned=7 skipped_files_scanned=0\n";

    let (files_scanned, markers_examined) = parse_scan_line(stderr);

    assert_eq!(
        files_scanned, 7,
        "files_scanned must come from the token NAMED files_scanned, never from \
         one merely ending with it (skipped_files_scanned=0 here)"
    );
    assert_eq!(
        markers_examined, 4,
        "markers_examined must survive both an unrecognised token and a \
         non-canonical field order"
    );
}

// ---------------------------------------------------------------------------
// (D) Fixture git-env hygiene
//
// (C) above drives the real generator over hermetic tempdir git fixtures, and
// these tests run ALWAYS-ON under `hooks/pre-commit` -> `hooks/project-checks`
// -> `scripts/verify.sh` — i.e. inside a git process tree, which is exactly the
// ambient condition `reify_test_support::git_env` documents. The failure mode
// and its measured signatures are argued there and are deliberately not
// restated here.
// ---------------------------------------------------------------------------

/// (D1) Both fixture command builders must remove EVERY repo-redirect git env
/// var, iterating the canonical set rather than a local copy.
///
/// Iterating `reify_audit::git_env::REPO_REDIRECT_VARS` is the point: the set
/// may GROW without editing this test (its deletion guard already lives at the
/// definition site, `repo_redirect_vars_covers_the_removal_floor`), and a local
/// list of names here would be precisely the "re-derive `REPO_REDIRECT_VARS` by
/// hand" step that `reify_test_support::git_env::sanitize`'s doc names as how
/// this bug class reaches a new helper.
///
/// Removals are read through `removed_vars` — `std` encodes `env_remove` as a
/// `(key, None)` pair — so an overwrite, or a value merely inherited from the
/// parent, cannot pass as a removal.
///
/// Hermetic and always-on: no tempdir and no git spawn, so no availability
/// probe is needed.
#[test]
fn fixture_commands_remove_every_repo_redirect_var() {
    let root = Path::new("/some/root");

    for (label, cmd) in [
        ("fixture_git_cmd", fixture_git_cmd(root)),
        ("generator_cmd", generator_cmd(root)),
    ] {
        let removed = reify_test_support::git_env::removed_vars(&cmd);
        for var in reify_audit::git_env::REPO_REDIRECT_VARS {
            assert!(
                removed.iter().any(|r| r == var),
                "{label}() must REMOVE `{var}` (env_remove -> `(key, None)`), not \
                 merely overwrite it; removals seen: {removed:?}"
            );
        }
    }

    // Separately: the hermeticity property (C1)/(C2) rest on. An ambient tasks
    // DB would wake the β liveness lane and change the fingerprint set, so a
    // rewrite of `generator_cmd` may not silently drop this removal.
    let removed = reify_test_support::git_env::removed_vars(&generator_cmd(root));
    assert!(
        removed.iter().any(|r| r == "REIFY_PTODO_TASKS_DB"),
        "generator_cmd() must REMOVE `REIFY_PTODO_TASKS_DB` so the fixture stays \
         hermetic (an ambient tasks DB wakes the β liveness lane and changes the \
         fingerprint set); removals seen: {removed:?}"
    );
}

/// (D2) COMPANION — replay the (C) scan-evidence tests under a real *ambient*
/// hook git environment, mirroring `cli.rs`'s
/// `hook_env_replay_of_ptodo_git_fixture_tests`.
///
/// This is NOT the RED half of (D): it passes both before and after
/// `fixture_git_cmd`/`generator_cmd` route through the shared sanitizer,
/// because the shared harness poisons only the three vars git exports into a
/// hook's process tree (`GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`) and the
/// hand-rolled trio already removed exactly those. It is regression protection
/// for the ambient condition itself — (D1) is what pins the rest of the set.
///
/// Floor 2 is the selection measured today: (C1)
/// `generator_emits_scan_evidence_with_real_counts` and (C2)
/// `generator_emits_scan_evidence_on_a_marker_free_repo`. (C3)
/// `parse_scan_line_ignores_unrecognised_tokens` and this test's own name both
/// fall outside the filter, so the replay cannot select itself and the floor is
/// not vacuous.
#[test]
fn hook_env_replay_of_generator_scan_evidence_tests() {
    common::git_env::replay_self_under_hook_git_env(&["generator_emits_scan_evidence"], 2);
}
