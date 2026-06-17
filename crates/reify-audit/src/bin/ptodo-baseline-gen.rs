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
//! REIFY_PTODO_TASKS_DB=/path/to/.taskmaster/tasks/tasks.db \
//!   cargo run -p reify-audit --bin ptodo-baseline-gen -- \
//!     --project-root /path/to/repo \
//!   > crates/reify-audit/ptodo-baseline.txt
//! ```
//!
//! `REIFY_PTODO_TASKS_DB` must point at the real `tasks.db` so the β liveness
//! lane runs and orphaned/unknown-id residue is captured as a SUPERSET (in a
//! task worktree `.taskmaster/` is untracked, so without it the lane degrades
//! to structural-only). The fingerprint set is keyed only by findings on a
//! swept source path — the same boundary `baseline_is_well_formed` enforces —
//! so ζ inverse-lane task-keyed findings are correctly excluded.
//!
//! Output: one `path :: kind :: text` fingerprint per line, sorted ascending,
//! deduplicated, with a single trailing newline (empty output → an empty
//! baseline, the §6.4 zero-residual end state). Diagnostics go to stderr.

use std::collections::BTreeSet;
use std::path::PathBuf;

use reify_audit::{
    AuditContext, ChangedSymbol, DeadSymbol, JCodemunchOps, LayerViolation, RealGitOps,
    SymbolReference, UntestedSymbol,
};

/// Inert [`JCodemunchOps`] — `ptodo::check` never touches the jcodemunch seam
/// (it is P1/PDEAD-only), but `AuditContext` requires the field. Mirrors the
/// `NoopJCodemunchOps` in the main `reify-audit` bin.
struct NoopJCodemunchOps;

impl JCodemunchOps for NoopJCodemunchOps {
    fn get_changed_symbols(&self, _since_sha: &str, _until_sha: &str) -> Vec<ChangedSymbol> {
        vec![]
    }
    fn find_references(&self, _symbol: &ChangedSymbol) -> Vec<SymbolReference> {
        vec![]
    }
    fn get_dead_code(&self, _min_confidence: f64) -> Vec<DeadSymbol> {
        vec![]
    }
    fn get_untested_symbols(&self, _min_confidence: f64) -> Vec<UntestedSymbol> {
        vec![]
    }
    fn get_layer_violations(&self) -> Vec<LayerViolation> {
        vec![]
    }
}

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
                     Set REIFY_PTODO_TASKS_DB to the real tasks.db for the liveness lane.\n\
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

    let findings = reify_audit::ptodo::check(&ctx);

    // Keep only findings on a swept source path — the same boundary
    // `baseline_is_well_formed` (and the convergence test) enforce. ζ inverse
    // findings are keyed by TASK ID (not a swept path) and are excluded.
    let fingerprints: BTreeSet<String> = findings
        .iter()
        .filter(|f| reify_audit::ptodo::is_swept_ext(&f.task_id))
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
    eprintln!("ptodo-baseline-gen: {} fingerprint(s) emitted", fingerprints.len());
}
