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
//!   The baseline is STRUCTURAL-ONLY by construction: it is generated
//!   DB-absent, so it carries only kinds the structural lane emits
//!   (`reify_audit::ptodo::STRUCTURAL_KINDS`), and any liveness or inverse
//!   kind is rejected here. Rationale: PRD §19. Read its contents from the
//!   file, never from this doc.
//!
//! (A′) **`validate_*`** — always-on, hermetic unit tests that drive crafted
//!   content through the shared `validate_baseline_content` validator, so the
//!   grammar/taxonomy/sort rules have real coverage independent of whatever the
//!   committed baseline happens to contain.
//!
//!   The live-set-versus-baseline check itself is NOT here: it is scenario (a)
//!   of `tests/infra/test_reify_audit_ptodo.sh`, which runs the generator
//!   DB-absent and compares in both directions (PRD §19).
//!
//! (C) **`generator_emits_scan_evidence_*`** — always-on, hermetic. Runs the
//!   real `ptodo-baseline-gen` binary over staged tempdir git fixtures and pins
//!   the §6.6 scan-evidence contract it emits on stderr
//!   (`@@PTODO_SCAN@@ files_scanned=<N> markers_examined=<M> tasks_db=<mode>`):
//!   exactly one such line per run, carrying the REAL counts, never leaking
//!   onto stdout, and still emitted when the tree is clean and stdout is empty.
//!   The `tasks_db` token reports whether the DB-dependent lanes ran: `present`
//!   when the default tasks.db resolves, `absent` when an unresolvable
//!   `REIFY_PTODO_TASKS_DB` override beats it (the DB-absent mode PRD §19
//!   requires of the ratchet). Graceful-skip if `git` is unavailable.
//!
//! (D) **fixture git-env hygiene** — always-on. Pins that the two fixture
//!   command builders (C) drives the real binary through strip every
//!   `reify_audit::git_env::REPO_REDIRECT_VARS` entry, and replays (C) under a
//!   real ambient hook git environment. Rationale lives in
//!   `reify_test_support::git_env`; not restated here.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test ptodo_baseline`   (A + A′ + C + D)
//!
//! Regenerating the baseline — use the canonical generator
//! (`src/bin/ptodo-baseline-gen.rs`), DB-absent, in any worktree. It is the
//! SINGLE source of truth: it maps `ptodo::check` findings through the SAME
//! `ptodo::fingerprint` the ratchet uses, so generation and the ratchet check
//! can never drift (PRD §6.6). Do NOT hand-derive fingerprints with `sed`/`jq`.
//!   ```text
//!   REIFY_PTODO_TASKS_DB=/dev/null/tasks.db \
//!     cargo run --release -p reify-audit --bin ptodo-baseline-gen -- \
//!       --project-root . > crates/reify-audit/ptodo-baseline.txt
//!   ```

mod common;

use reify_audit::ptodo::{STRUCTURAL_KINDS, is_allowlisted, is_swept_ext};
use std::path::Path;
use std::process::Command;

/// Resolve the path to `ptodo-baseline.txt`:
///   CARGO_MANIFEST_DIR = `crates/reify-audit` → `./ptodo-baseline.txt`
fn baseline_path() -> std::path::PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("ptodo-baseline.txt")
}

/// A `REIFY_PTODO_TASKS_DB` override that can never resolve: no path beneath a
/// character device can exist.
const TASKS_DB_ABSENT: &str = "/dev/null/tasks.db";

/// The one command that regenerates the baseline DB-absent (PRD §19).
fn regen_db_absent() -> String {
    format!(
        "REIFY_PTODO_TASKS_DB={TASKS_DB_ABSENT} cargo run --release \
         -p reify-audit --bin ptodo-baseline-gen -- --project-root . \
         > crates/reify-audit/ptodo-baseline.txt"
    )
}

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
    if !STRUCTURAL_KINDS.contains(&fp_kind) {
        return Err(format!(
            "kind {fp_kind:?} is not a structural kind {STRUCTURAL_KINDS:?}; the \
             baseline is structural-only by construction (PRD §19), so liveness and \
             inverse kinds never enter it. Regenerate it DB-absent: {}",
            regen_db_absent()
        ));
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
/// A line's `kind` must be one of `STRUCTURAL_KINDS` (PRD §19). An EMPTY input
/// is valid — the §6.4 zero-residual end state. Because this is pure, the
/// grammar/taxonomy/sort rules have permanent coverage via the `validate_*`
/// unit tests below regardless of the committed content.
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
/// text` triple with a structural `kind` on a swept, non-allowlisted source
/// `path`, and the lines are strictly sorted ascending with no duplicates.
///
/// An empty baseline PASSES — it is the §6.4 "zero residual debt" success state,
/// not a failure. This test asserts existence + well-formedness, NOT emptiness
/// in either direction.
#[test]
fn baseline_is_well_formed() {
    let path = baseline_path();

    assert!(
        path.exists(),
        "ptodo-baseline.txt not found at {path:?}.\n\
         Generate it DB-absent with the canonical generator:\n{}",
        regen_db_absent()
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
// that a specific lane δ-A entry is PRESENT in the committed baseline. It would
// read a static file the same commit authored, so the only state it could
// detect is someone editing the baseline. Recognizer regressions are caught
// elsewhere: scenario (a) of tests/infra/test_reify_audit_ptodo.sh is
// two-directional (PRD §19), so a baseline line whose recognizer stops firing
// reds there as stale; and `check_allow_dead_code_deferral_lane`
// (tests/ptodo.rs) drives `ptodo::check` end-to-end against a seeded `done`
// cite and asserts the High `orphaned:` summary.

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
    let good = "crates/reify-eval/src/dispatcher.rs :: malformed-cite :: // TODO(task 4592): x\n\
                crates/reify-eval/src/engine_eval.rs :: untracked :: // TODO: y\n";
    assert!(validate_baseline_content(good).is_ok(), "well-formed sorted content must pass");
}

/// A DB-dependent kind never enters the baseline, even on an otherwise
/// well-formed line: the kind is the only defect in each fixture, and the
/// rejection must name it.
#[test]
fn validate_rejects_db_dependent_kinds() {
    for kind in [
        "orphaned",
        "unknown-id",
        "parked-on-anchor",
        "g-allow-orphaned",
        "g-allow-unknown-id",
        "task-cites-deleted-path",
        "task-cites-renamed-path",
    ] {
        let line = format!("crates/x/y.rs :: {kind} :: #1234 status=done: x\n");
        match validate_baseline_content(&line) {
            Ok(()) => panic!("a `{kind}` line must be rejected; line={line:?}"),
            Err(e) => assert!(
                e.contains(kind),
                "the rejection must name `{kind}`; got {e:?}"
            ),
        }
    }
}

/// Over-narrowing guard, green on arrival: every structural kind stays
/// accepted. The literal list is this test's independent oracle.
#[test]
fn validate_accepts_every_structural_kind() {
    for kind in [
        "untracked",
        "malformed-cite",
        "phantom-tracking",
        "bare-ignore",
    ] {
        let line = format!("crates/x/y.rs :: {kind} :: x\n");
        assert!(
            validate_baseline_content(&line).is_ok(),
            "a `{kind}` line must be accepted; line={line:?}"
        );
    }
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

// ---------------------------------------------------------------------------
// (C) Generator scan-evidence contract (task #6241, PRD §6.6)
//
// `ptodo-baseline-gen` emits, on STDERR, one machine-readable line per run:
//
//     @@PTODO_SCAN@@ files_scanned=<N> markers_examined=<M> tasks_db=<mode>
//
// That line is the RUN evidence the §6.6 vacuity floor in
// tests/infra/test_reify_audit_ptodo.sh keys on, and its `tasks_db` token is
// the DB-absent evidence the same scenario's second floor keys on (PRD §19).
// These tests drive the real
// binary over hermetic git fixtures and pin the contract end to end: the line
// exists, carries the REAL counts (not a constant), stays off stdout (stdout is
// the baseline stream — a leak would corrupt the next regen), and is emitted
// even when the tree is clean and stdout is empty.
//
// Rationale lives in docs/prds/reify-audit-ptodo-detector.md §6.6 and §19 and
// is deliberately not restated here.
// ---------------------------------------------------------------------------

/// Assemble a comment marker at RUNTIME so this test source never self-matches
/// the detector it drives (the same self-match-safety idiom the hermetic
/// scenarios in tests/infra/test_reify_audit_ptodo.sh use).
fn untracked_marker(body: &str) -> String {
    format!("// {}{}: {body}\n", "TO", "DO")
}

/// A marker citing task `id` canonically, assembled at runtime like
/// [`untracked_marker`].
fn cited_marker(id: u32, body: &str) -> String {
    format!("// {}{}(#{id}): {body}\n", "TO", "DO")
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

/// The fields of one `@@PTODO_SCAN@@` line.
#[derive(Debug)]
struct ScanEvidence {
    files_scanned: usize,
    markers_examined: usize,
    /// The `tasks_db=` token's value. OPTIONAL in the grammar: only the two
    /// counters are required (§6.6 EXTENSIBILITY).
    tasks_db: Option<String>,
}

/// Extract the single `@@PTODO_SCAN@@` line's fields from `stderr`, asserting
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
fn parse_scan_line(stderr: &str) -> ScanEvidence {
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
    let mut tasks_db: Option<String> = None;
    for field in rest.split_whitespace() {
        if let Some(v) = field.strip_prefix("files_scanned=") {
            files = Some(v.parse().unwrap_or_else(|e| {
                panic!("files_scanned must be an integer ({v:?}): {e}")
            }));
        } else if let Some(v) = field.strip_prefix("markers_examined=") {
            markers = Some(v.parse().unwrap_or_else(|e| {
                panic!("markers_examined must be an integer ({v:?}): {e}")
            }));
        } else if let Some(v) = field.strip_prefix("tasks_db=") {
            tasks_db = Some(v.to_string());
        }
        // else: an unrecognised token is an ADDITIVE extension of the grammar —
        // ignored by contract, never a failure (PRD §6.6).
    }
    ScanEvidence {
        files_scanned: files.unwrap_or_else(|| panic!("scan line lacks files_scanned: {line:?}")),
        markers_examined: markers
            .unwrap_or_else(|| panic!("scan line lacks markers_examined: {line:?}")),
        tasks_db,
    }
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
    let ScanEvidence {
        files_scanned,
        markers_examined,
        ..
    } = parse_scan_line(&stderr);
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

    let ScanEvidence {
        files_scanned,
        markers_examined,
        ..
    } = parse_scan_line(&stderr);
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

    let ScanEvidence {
        files_scanned,
        markers_examined,
        ..
    } = parse_scan_line(stderr);

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

/// A staged fixture whose one marker cites task 4444, with a tasks.db seeded
/// AFTER staging at §6.7's default path (so it is reachable with no override
/// and is never itself scanned) recording 4444 as `done`.
fn cited_fixture_with_default_tasks_db() -> tempfile::TempDir {
    let fixture = staged_fixture(&[("src/cited.rs", cited_marker(4444, "wire the fixture up"))]);
    common::schema::seed_tasks_db_at(
        &fixture.path().join(".taskmaster/tasks/tasks.db"),
        &[("master", 4444, "done")],
    );
    fixture
}

/// (C4) CONTROL — with the default tasks.db reachable, the DB-dependent lanes
/// run: the scan line says `tasks_db=present` and the done cite surfaces as
/// one `orphaned` fingerprint.
#[test]
fn generator_emits_scan_evidence_tasks_db_present_when_default_db_resolves() {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("ptodo_baseline: skipping tasks_db control test — git not available");
        return;
    }

    let fixture = cited_fixture_with_default_tasks_db();
    let out = run_generator(fixture.path());
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        out.status.success(),
        "generator must exit 0; status={:?}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert_eq!(
        parse_scan_line(&stderr).tasks_db.as_deref(),
        Some("present"),
        "a reachable default tasks.db must be reported; stderr:\n{stderr}"
    );
    let kinds: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.split(" :: ").nth(1).unwrap_or(""))
        .collect();
    assert_eq!(
        kinds,
        ["orphaned"],
        "the DB lanes must really run (one orphaned fingerprint); stdout:\n{stdout}"
    );
}

/// (C5) An unresolvable `REIFY_PTODO_TASKS_DB` override beats a reachable
/// default: the scan line says `tasks_db=absent` and no DB-dependent
/// fingerprint is emitted. This is the main-checkout case of PRD §19 finding 2,
/// reproduced hermetically.
#[test]
fn generator_emits_scan_evidence_tasks_db_absent_when_override_is_unresolvable() {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("ptodo_baseline: skipping tasks_db override test — git not available");
        return;
    }

    let fixture = cited_fixture_with_default_tasks_db();
    let out = generator_cmd(fixture.path())
        .env("REIFY_PTODO_TASKS_DB", TASKS_DB_ABSENT)
        .output()
        .expect("ptodo-baseline-gen spawns");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        out.status.success(),
        "generator must exit 0; status={:?}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert_eq!(
        parse_scan_line(&stderr).tasks_db.as_deref(),
        Some("absent"),
        "an unresolvable override must force the DB-absent mode; stderr:\n{stderr}"
    );
    assert!(
        stdout.is_empty(),
        "DB-absent, a cited marker yields no fingerprint; stdout:\n{stdout}"
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
/// Floor 4 is the selection measured today: (C1)
/// `generator_emits_scan_evidence_with_real_counts`, (C2)
/// `generator_emits_scan_evidence_on_a_marker_free_repo`, and the two
/// `generator_emits_scan_evidence_tasks_db_*` tests (C4)/(C5). (C3)
/// `parse_scan_line_ignores_unrecognised_tokens` and this test's own name both
/// fall outside the filter, so the replay cannot select itself and the floor is
/// not vacuous.
#[test]
fn hook_env_replay_of_generator_scan_evidence_tests() {
    common::git_env::replay_self_under_hook_git_env(&["generator_emits_scan_evidence"], 4);
}
