//! `pdiag-baseline-gen` — the SINGLE canonical regenerator for
//! `crates/reify-audit/pdiag-baseline.txt` (task #5405, PRD §6.6/§7).
//!
//! It is a thin renderer over `pdiag::live_counts` — the exact census
//! `pdiag::check` diffs against this manifest. Using ONE Rust derivation for
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
//! Always exits 0: this tool reports the tree, it does not judge it. Judging is
//! `reify-audit --pattern PDIAG`'s job.
//!
//! **Regenerating is not a remediation.** Rerunning this after adding a
//! code-less diagnostic simply re-blesses it. The remediation triad — attach a
//! `DiagnosticCode`, take the reviewed `// pdiag:allow — reason` opt-out, or
//! shrink the row by fixing sites — is in
//! `docs/notes/diagnostic-severity-policy.md` §3.

use std::path::PathBuf;

// `NoopJCodemunchOps` is the library's: PDIAG is a purely structural lane
// (`ls_files` plus working-tree reads) and never touches the jcodemunch seam,
// but `AuditContext` requires the field.
use reify_audit::{AuditContext, NoopJCodemunchOps, RealGitOps};

/// The `#` preamble every generated manifest carries.
///
/// It exists so the three things a reader needs — how to regenerate, where the
/// policy lives, and the fact that regenerating is not a fix — travel WITH the
/// file. A manifest found in a diff without them invites exactly the
/// re-bless-the-regression move the ratchet exists to prevent.
const HEADER: &str = "\
# PDIAG baseline — per-file allowance of code-less Diagnostic::error/warning
# construction sites (INV-SF-6 diagnostics-carry-codes).
#
# GENERATED — do not hand-edit. Regenerate with:
#   cargo run -p reify-audit --bin pdiag-baseline-gen -- --project-root . \\
#     > crates/reify-audit/pdiag-baseline.txt
#
# Counts may only DECREASE. A row going up, or a new file appearing here, is a
# hard-gate (High) finding from `reify-audit --pattern PDIAG`.
#
# Regenerating is NOT a remediation — it just re-blesses the new sites. If your
# diff went RED, the three real fixes (attach a DiagnosticCode / take the
# reviewed `// pdiag:allow — reason` opt-out / fix the sites and shrink the row
# IN THE SAME COMMIT) are in docs/notes/diagnostic-severity-policy.md §3.
#
# Format: `<repo-relative-path> <count>`, ascending by path, no zero rows.
";

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

    // `live_counts` already omits zero-count files and yields ascending path
    // order, which is exactly the manifest's grammar — hence "thin renderer".
    let counts = reify_audit::pdiag::live_counts(&ctx);

    let mut out = String::from(HEADER);
    for (path, count) in &counts {
        out.push_str(&format!("{path} {count}\n"));
    }
    print!("{out}");

    let sites: u32 = counts.values().sum();
    eprintln!(
        "pdiag-baseline-gen: {} file(s), {sites} code-less site(s)",
        counts.len()
    );
}
