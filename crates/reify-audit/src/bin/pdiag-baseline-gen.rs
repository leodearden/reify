//! `pdiag-baseline-gen` — the SINGLE canonical regenerator for
//! `crates/reify-audit/pdiag-baseline.txt` (task #5405, PRD §6.6/§7).
//!
//! It is a thin renderer over `pdiag::census_summary` (`pdiag::live_counts`
//! plus the swept-file total) — the exact census `pdiag::check` diffs against
//! this manifest. Using ONE Rust derivation for
//! both generation and enforcement is what makes drift structurally
//! impossible: a freshly regenerated baseline is, by construction, one the
//! ratchet accepts. (The same reason `ptodo-baseline-gen` calls
//! `ptodo::fingerprint` rather than re-deriving fingerprints, and the reason
//! neither has a `sed`/`awk` recipe.)
//!
//! Usage:
//! ```text
//! cargo run -p reify-audit --bin pdiag-baseline-gen -- --project-root . \
//!   > crates/reify-audit/pdiag-baseline.txt
//! ```
//!
//! Output: a `#` header block, then one `<path> <count>` row per swept file
//! carrying at least one code-less `Diagnostic::error(...)` /
//! `Diagnostic::warning(...)` site, ascending by path (`BTreeMap` order — the
//! order `parse_baseline` requires). Files with zero code-less sites get NO row
//! — a `0` row is a parse error, since a clean file's absence is the only
//! spelling of "clean". Diagnostics go to stderr; stdout is the manifest and
//! nothing else.
//!
//! Exit codes: `0` on a census that reached at least one swept file — this tool
//! reports the tree, it does not judge it, so a big backlog still exits 0
//! (judging is `reify-audit --pattern PDIAG`'s job). `2` on a bad argument.
//! `3` on a DEGENERATE census: `git ls-files` reached zero swept files, so
//! stdout stays empty and the recipe above cannot truncate the ratchet's own
//! manifest to a header-only file. `RealGitOps::ls_files` degrades to an empty
//! list on ANY git failure — spawn error, non-zero exit, non-UTF-8 output — so
//! without that branch, running this outside the worktree or with a mistyped
//! `--project-root` would silently wipe the baseline and still exit 0. The
//! symmetric guard on the enforcement side is `pdiag::check`'s
//! `pdiag-census-empty` High; both halves of the ratchet now fail loud when
//! their census vanishes.
//!
//! **Regenerating is not a remediation.** Rerunning this after adding a
//! code-less diagnostic simply re-blesses it. The remediation triad — attach a
//! `DiagnosticCode`, take the reviewed `// pdiag:allow — reason` opt-out, or
//! shrink the row by fixing sites — is in
//! `docs/notes/diagnostic-severity-policy.md` §3. The one High finding that IS
//! answered by rerunning this is a pure move or rename, whose sites all predate
//! the move and so have nothing to edit (§3(d) there).

use std::path::PathBuf;

// `NoopJCodemunchOps` is the library's: PDIAG is a purely structural lane
// (`ls_files` plus working-tree reads) and never touches the jcodemunch seam,
// but `AuditContext` requires the field.
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
                    eprintln!("pdiag-baseline-gen: --project-root requires a value");
                    std::process::exit(2);
                });
            }
            "-h" | "--help" => {
                eprintln!(
                    "Usage: pdiag-baseline-gen [--project-root <path>]\n\
                     Emits the PDIAG per-file code-less-site manifest to stdout,\n\
                     ascending by path. Redirect into \
                     crates/reify-audit/pdiag-baseline.txt.\n\
                     Policy + remediation: docs/notes/diagnostic-severity-policy.md"
                );
                return;
            }
            other if !other.starts_with('-') => project_root = other.to_string(),
            other => {
                eprintln!("pdiag-baseline-gen: unknown argument {other:?}");
                std::process::exit(2);
            }
        }
    }

    let root = PathBuf::from(&project_root);
    let git = RealGitOps::new(root.clone());
    // `conn`/`task_metadata` are inert placeholders the PDIAG lane never reads.
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

    // `census_summary` is `live_counts` plus the swept-file count: the counts
    // already omit zero-count files and yield ascending path order, which is
    // exactly the manifest's grammar — hence "thin renderer" — while `swept`
    // is what distinguishes a clean tree from a census that never happened.
    let (swept, counts) = reify_audit::pdiag::census_summary(&ctx);

    // The generator's half of the fail-loud-on-a-vanished-census posture.
    // Checked BEFORE anything reaches stdout: the documented recipe redirects
    // stdout over the committed manifest, and the shell truncates that file
    // before this process even starts, so "print a header and exit 0" IS the
    // wipe. `swept == 0` is the only degenerate case — an empty `counts` with a
    // real sweep is a clean tree, and a clean tree's zero-row manifest is the
    // end state the ratchet is aimed at.
    if swept == 0 {
        eprintln!(
            "pdiag-baseline-gen: git enumeration returned no swept files — refusing to \
             emit a manifest that would truncate crates/reify-audit/pdiag-baseline.txt \
             to its header. Check that the run is inside the git worktree, that \
             --project-root ({project_root:?}) points at it, and that `git ls-files` \
             succeeds there; regenerate only once enumeration works again."
        );
        std::process::exit(3);
    }

    // Both halves — the census AND the `#` preamble plus row rendering — are
    // the library's, so this binary owns no derivation of the manifest format
    // at all. `pdiag::BASELINE_HEADER` and `pdiag::render_baseline` are what
    // `tests/pdiag_baseline.rs` asserts on; while they lived here they were
    // unreachable from any test, and the generator's real bytes went
    // unpinned.

    print!("{}", reify_audit::pdiag::render_baseline(&counts));

    let sites: u32 = counts.values().sum();
    eprintln!(
        "pdiag-baseline-gen: {swept} swept file(s), {} with rows, {sites} code-less site(s)",
        counts.len()
    );
}
