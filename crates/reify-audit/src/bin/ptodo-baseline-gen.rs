//! `ptodo-baseline-gen` — the SINGLE canonical regenerator for
//! `crates/reify-audit/ptodo-baseline.txt` (task δ §6.6).
//!
//! It runs `ptodo::check` over the real working tree and maps every
//! source-marker finding through `ptodo::fingerprint` — the exact derivation
//! the ε ratchet check uses — then sorts and deduplicates. Using ONE Rust
//! derivation for both baseline generation and the live ratchet comparison is
//! what makes the drift PRD §6.6 warns about structurally impossible. (The
//! previous doc-only `sed` recipe re-implemented the derivation by hand —
//! stripping `line N:` unconditionally, not folding internal whitespace, and
//! sorting under the default locale — and could silently disagree with
//! `fingerprint()`. This binary replaces it.)
//!
//! Usage:
//! ```text
//! REIFY_PTODO_TASKS_DB=/dev/null/tasks.db \
//!   cargo run --release -p reify-audit --bin ptodo-baseline-gen -- \
//!     --project-root . > crates/reify-audit/ptodo-baseline.txt
//! ```
//!
//! The committed baseline is structural-only and is generated DB-absent, as
//! above (PRD §19). With a reachable tasks.db the liveness kinds are emitted
//! too: fine for diagnostics, never valid for the committed baseline, because
//! `baseline_is_well_formed` rejects them. The fingerprint set is keyed only by
//! findings on a swept source path — the same boundary `baseline_is_well_formed`
//! enforces — so ζ inverse-lane task-keyed findings are correctly excluded.
//!
//! Output: one `path :: kind :: text` fingerprint per line, sorted ascending,
//! deduplicated, with a single trailing newline (empty output → an empty
//! baseline, the §6.4 zero-residual end state). Diagnostics go to stderr.
//!
//! ## Stderr MACHINE CONTRACT (§6.6 scan evidence)
//!
//! Every run emits exactly one machine-readable line to STDERR:
//!
//! ```text
//! @@PTODO_SCAN@@ files_scanned=<N> markers_examined=<M> tasks_db=<absent|present>
//! ```
//!
//! The counters come straight from `ptodo::check_with_stats` (counted inside
//! the single sweep, so they cannot drift from what was actually walked). The
//! line is emitted UNCONDITIONALLY on the normal exit path — including when
//! stdout is empty — and never on stdout, which is the baseline stream: a leak
//! there would corrupt `ptodo-baseline.txt` on the next regen.
//!
//! Its consumer is the vacuity floor in `tests/infra/test_reify_audit_ptodo.sh`,
//! which passes iff the line is present with `files_scanned >= 1`. That floor
//! keys on evidence the detector RAN rather than on what it FOUND; the rationale
//! is in `docs/prds/reify-audit-ptodo-detector.md` §6.6 and is not restated here.
//! A binary predating this contract emits no such line, so a stale/reverted
//! generator fails the floor on evidence rather than on a freshness heuristic.
//!
//! `tasks_db` is `ScanStats::tasks_db`: `present` iff the task DB opened and
//! the DB-dependent lanes (β, ζ, G-allow) resolved, `absent` iff the §6.7
//! degrade path fired. It is an ADDITIVE field under §6.6's extensibility
//! rule, so the vacuity floor and `parse_scan_line`'s required-field check
//! ignore it. Scenario (a)'s DB-absent floor in the same shell test requires
//! `tasks_db=absent` (PRD §19).
//!
//! The human-readable `N fingerprint(s) emitted` line is kept alongside it as
//! the operator-facing diagnostic; nothing keys on that one.

use std::collections::BTreeSet;
use std::path::PathBuf;

// `NoopJCodemunchOps` is the library's: `ptodo::check` never touches the
// jcodemunch seam (it is P1/PDEAD-only), but `AuditContext` requires the field.
use reify_audit::{AuditContext, NoopJCodemunchOps, RealGitOps};

fn main() {
    // Minimal arg parse: `--project-root <path>` (default "."). A bare first
    // positional argument is also accepted as the project root for convenience.
    let mut project_root = ".".to_string();
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--project-root" => {
                project_root = argv.next().unwrap_or_else(|| {
                    eprintln!("ptodo-baseline-gen: --project-root requires a value");
                    std::process::exit(2);
                });
            }
            "-h" | "--help" => {
                eprintln!(
                    "Usage: ptodo-baseline-gen [--project-root <path>]\n\
                     For the committed baseline, run DB-absent: REIFY_PTODO_TASKS_DB=/dev/null/tasks.db.\n\
                     Emits sorted, deduplicated `path :: kind :: text` fingerprints to stdout."
                );
                return;
            }
            other if !other.starts_with('-') => project_root = other.to_string(),
            other => {
                eprintln!("ptodo-baseline-gen: unknown argument {other:?}");
                std::process::exit(2);
            }
        }
    }

    let root = PathBuf::from(&project_root);
    let git = RealGitOps::new(root.clone());
    // `ptodo::check` opens its own tasks DB via `tasks_db_path(project_root)`
    // (honoring REIFY_PTODO_TASKS_DB); `conn`/`task_metadata` here are inert
    // placeholders the PTODO lanes never read.
    let conn = rusqlite::Connection::open_in_memory()
        .expect("in-memory sqlite connection for AuditContext placeholder");
    let jc = NoopJCodemunchOps;
    let ctx = AuditContext {
        project_root: root,
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: std::collections::HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };

    let (findings, stats) = reify_audit::ptodo::check_with_stats(&ctx);

    // Keep only source-marker findings: swept source path (same boundary as
    // `baseline_is_well_formed`) AND not a G-allow advisory finding.
    //
    // ζ inverse findings are keyed by TASK ID (not a swept path) and are
    // excluded by the `is_swept_ext` filter.
    //
    // G-allow advisory findings (g-allow-orphaned / g-allow-unknown-id) are
    // path-keyed (swept .rs files) so they pass the `is_swept_ext` filter, but
    // their kind strings ("g-allow-*") are outside `ptodo::STRUCTURAL_KINDS`,
    // which `baseline_is_well_formed` enforces — including them would make a
    // future regen fail the kind check. Exclude them explicitly here, mirroring
    // the ζ exclusion. (A DB-absent run emits none anyway.)
    let fingerprints: BTreeSet<String> = findings
        .iter()
        .filter(|f| {
            reify_audit::ptodo::is_swept_ext(&f.task_id)
                && !reify_audit::ptodo::is_g_allow_finding(f)
        })
        .map(reify_audit::ptodo::fingerprint)
        .collect();

    let mut out = String::new();
    for fp in &fingerprints {
        out.push_str(fp);
        out.push('\n');
    }
    // Single write; `out` already carries exactly one trailing newline per line
    // (and is empty when there are no findings → an empty baseline file).
    print!("{out}");
    // MACHINE CONTRACT (§6.6) — emitted on STDERR every run, before the human
    // diagnostic. Grammar and consumer are documented in the module doc above.
    eprintln!(
        "@@PTODO_SCAN@@ files_scanned={} markers_examined={} tasks_db={}",
        stats.files_scanned,
        stats.markers_examined,
        stats.tasks_db.as_token()
    );
    eprintln!("ptodo-baseline-gen: {} fingerprint(s) emitted", fingerprints.len());
}
