//! PLINECITE — hard line cites in source comments.
//!
//! A comment that cites code as `<file>.rs:<line>` or `<file>.ri:<line>`
//! rots silently: any edit above the target moves it and nothing re-checks
//! the number. PLINECITE reports each such cite so the author cites the
//! enclosing symbol instead.
//!
//! It reports and never certifies. The grammar (`line_cites`) is
//! deliberately decline-only: prose, line-wrapped and string-literal cites go
//! unseen, so a clean run does not prove a file free of line cites.
//!
//! Every finding is [`Severity::Medium`], so the lane is report-only and
//! exit-neutral. Kinds, carried as the summary prefix:
//!
//! - `line-cite:` — one per citing file and cite, at its first line, unless
//!   the committed ledger ([`crate::plinecite_baseline`]) holds that
//!   `(file, cite)` row;
//! - `stale-baseline-entry:` — a ledger row no live cite matches;
//! - `allow-missing-reason:` — a `plinecite:allow` marker with no reason,
//!   which exempts nothing;
//! - `scope-empty:` — no tracked file is in [`in_scope`] ([`ScopeEmpty`]):
//!   the run's only finding, so a tree that was never read cannot report
//!   clean.
//!
//! Rationale, scope, measured surface and the normative grammar:
//! `docs/prds/reify-audit-stale-line-cite-detector.md`.

use crate::plinecite_baseline::{BASELINE_PATH, CiteRow, parse_baseline};
use crate::ratchet_ledger::{Ledger, committed_rows};
use crate::scan_util::{allow_marker_body, find_word_boundary_token, is_word_byte};
use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};
use std::collections::BTreeMap;
use std::fmt;

/// The per-line escape hatch: `plinecite:allow — <reason>`.
const ALLOW_TOKEN: &str = "plinecite:allow";

const STDLIB_ROOT: &str = "crates/reify-compiler/stdlib/";

/// The detector's own crate: self-match and fixture-provenance records.
const EXCLUDED_PREFIXES: &[&str] = &["crates/reify-audit/"];

/// The escalation anchor a cite may carry between its extension and its line
/// number.
const ESC_ANCHOR: &[u8] = b" esc-";

/// Whether PLINECITE scans the tracked file `path` — the lane's single scope
/// boundary: the bundled stdlib's `.ri` files, and every `.rs` under a
/// crate's `src/` outside `EXCLUDED_PREFIXES`.
// G-allow: pub for the cross-crate real-tree smoke in tests/plinecite.rs; the production caller, check(), is same-file, and the orphan audit counts only cross-file call sites
pub fn in_scope(path: &str) -> bool {
    let stdlib_ri = path.ends_with(".ri") && path.starts_with(STDLIB_ROOT);
    let crate_src_rs = path.ends_with(".rs")
        && !EXCLUDED_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
        && is_under_a_crate_src(path);
    stdlib_ri || crate_src_rs
}

/// Whether `path` is `crates/<crate>/src/<rest>`, at any depth under `src/`.
fn is_under_a_crate_src(path: &str) -> bool {
    let mut parts = path.splitn(4, '/');
    matches!(
        [parts.next(), parts.next(), parts.next(), parts.next()],
        [Some("crates"), Some(_), Some("src"), Some(_)]
    )
}

/// Every line cite the committed ledger lacks ([`Ledger::new_debt`]), one
/// `stale-baseline-entry:` per row no live cite matches ([`Ledger::stale`]),
/// and every malformed allow marker — sorted by `(category, name, path)`. An
/// empty scope yields its one `scope-empty:` finding instead. An unreadable
/// file is skipped.
pub fn check(ctx: &AuditContext<'_>) -> Vec<Finding> {
    let Audit { scan, ledger } = match audit(ctx) {
        Ok(audit) => audit,
        Err(empty) => return vec![scope_empty(empty).finding],
    };
    let Scan {
        mut findings,
        mut debt,
    } = scan;
    findings.extend(ledger.new_debt().iter().filter_map(|row| debt.remove(row)));
    findings.extend(ledger.stale().iter().map(stale_baseline_entry));
    findings.sort_by(|a, b| a.key().cmp(&b.key()));
    findings.into_iter().map(|k| k.finding).collect()
}

/// The live debt and the committed ledger, for the generator — the SAME
/// derivation [`check`] settles, so a regenerated ledger is by construction
/// one the ratchet accepts. Refuses an empty scope rather than deriving a
/// ledger that would wipe the committed one.
pub fn baseline_ledger(ctx: &AuditContext<'_>) -> Result<Ledger<CiteRow>, ScopeEmpty> {
    audit(ctx).map(|audit| audit.ledger)
}

/// No tracked file is in PLINECITE's scope. A failed `git ls-files` reads
/// this way, so [`check`] reports it and [`baseline_ledger`] refuses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeEmpty;

impl fmt::Display for ScopeEmpty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "no tracked file is in PLINECITE's scope, so {BASELINE_PATH} cannot be \
             settled — every committed row would read as stale; check that \
             --project-root is the git worktree and that `git ls-files` succeeds there."
        )
    }
}

/// Every in-scope file scanned, plus the committed ledger its debt is settled
/// against — the one computation behind [`check`] and [`baseline_ledger`].
struct Audit {
    scan: Scan,
    ledger: Ledger<CiteRow>,
}

fn audit(ctx: &AuditContext<'_>) -> Result<Audit, ScopeEmpty> {
    let tracked = ctx.git.ls_files();
    let scanned: Vec<&String> = tracked.iter().filter(|path| in_scope(path)).collect();
    if scanned.is_empty() {
        return Err(ScopeEmpty);
    }
    let mut scan = Scan::default();
    for path in scanned {
        if let Some(content) = ctx.read_relative(path) {
            scan.file(path, &content);
        }
    }
    let ledger = Ledger {
        live: scan.debt.keys().cloned().collect(),
        committed: committed_rows(ctx, &tracked, BASELINE_PATH, parse_baseline),
    };
    Ok(Audit { scan, ledger })
}

/// The scanned files' findings, split by what a ledger row can absorb.
#[derive(Default)]
struct Scan {
    /// Findings no ledger row absorbs: reasonless allow markers.
    findings: Vec<Keyed>,
    /// Each `(path, cite)` row's first-occurrence `line-cite:` finding —
    /// exactly what [`check`] reports against an empty ledger.
    debt: BTreeMap<CiteRow, Keyed>,
}

impl Scan {
    fn file(&mut self, path: &str, content: &str) {
        for (idx, line) in content.lines().enumerate() {
            let line_no = idx + 1;
            if find_word_boundary_token(line, ALLOW_TOKEN).is_some() {
                if allow_marker_body(line, ALLOW_TOKEN).is_some() {
                    continue;
                }
                self.findings.push(allow_missing_reason(path, line_no));
            }
            for cite in line_cites(line) {
                let row = CiteRow {
                    path: path.to_string(),
                    cite: cite.to_string(),
                };
                self.debt
                    .entry(row)
                    .or_insert_with(|| line_cite(path, line_no, cite));
            }
        }
    }
}

/// A finding with its `(category, name, path)` sort key.
struct Keyed {
    category: &'static str,
    name: String,
    finding: Finding,
}

impl Keyed {
    fn key(&self) -> (&str, &str, &str) {
        (self.category, &self.name, &self.finding.task_id)
    }
}

/// One finding about `path`, named by `name`; the summary names the same
/// handle the evidence carries.
fn keyed(
    category: &'static str,
    name: String,
    path: &str,
    evidence: EvidenceRef,
    detail: &str,
) -> Keyed {
    let finding = Finding {
        pattern: Pattern::PLineCite,
        severity: Severity::Medium,
        task_id: path.to_string(),
        summary: format!("{category}: {name} — {detail}"),
        evidence: vec![evidence],
    };
    Keyed {
        category,
        name,
        finding,
    }
}

fn file_line(path: &str, line: usize, symbol: Option<&str>) -> EvidenceRef {
    EvidenceRef::FileLine {
        path: path.to_string(),
        line,
        symbol: symbol.map(str::to_string),
    }
}

fn line_cite(path: &str, line: usize, cite: &str) -> Keyed {
    keyed(
        "line-cite",
        cite.to_string(),
        path,
        file_line(path, line, Some(cite)),
        &format!(
            "{path}:{line} cites code by line number, which rots silently; cite \
             the enclosing symbol instead, or mark a deliberate dated cite \
             `{ALLOW_TOKEN} — <reason>`"
        ),
    )
}

fn allow_missing_reason(path: &str, line: usize) -> Keyed {
    keyed(
        "allow-missing-reason",
        format!("{path}:{line}"),
        path,
        file_line(path, line, None),
        &format!(
            "`{ALLOW_TOKEN}` with no reason body exempts nothing; write \
             `{ALLOW_TOKEN} — <reason>`"
        ),
    )
}

/// A finding about the ledger file itself rather than one citing line.
fn ledger_finding(category: &'static str, name: String, detail: &str) -> Keyed {
    let evidence = EvidenceRef::File {
        path: BASELINE_PATH.to_string(),
    };
    keyed(category, name, BASELINE_PATH, evidence, detail)
}

fn scope_empty(empty: ScopeEmpty) -> Keyed {
    ledger_finding("scope-empty", BASELINE_PATH.to_string(), &empty.to_string())
}

/// The finding for a committed row that absorbs nothing, named by the row's
/// own text so the line to delete is the one quoted.
fn stale_baseline_entry(row: &CiteRow) -> Keyed {
    ledger_finding(
        "stale-baseline-entry",
        row.to_string(),
        &format!(
            "listed in {BASELINE_PATH} but matches no live cite, so it absorbs \
             nothing; delete the line, or regenerate with \
             `cargo run -p reify-audit --bin plinecite-baseline-gen`"
        ),
    )
}

/// The hard cite tokens `line` carries, in line order; none unless `line` is
/// a comment line. Each token runs from its stem through the first digit run
/// of its line number. The grammar is pure: allow markers are the caller's
/// business.
fn line_cites(line: &str) -> Vec<&str> {
    if !line.trim_start().starts_with("//") {
        return Vec::new();
    }
    let bytes = line.as_bytes();
    let mut cites = Vec::new();
    let mut floor = 0;
    let mut at = 0;
    while at < bytes.len() {
        match cite_at(bytes, at, floor) {
            Some((start, end)) => {
                cites.push(&line[start..end]);
                floor = end;
                at = end;
            }
            None => at += 1,
        }
    }
    cites
}

/// The `(start, end)` byte span of the cite whose extension dot sits at
/// `dot`, with its stem reaching back no further than `floor`. Every byte the
/// span starts or ends next to is ASCII, so both offsets are char boundaries.
fn cite_at(bytes: &[u8], dot: usize, floor: usize) -> Option<(usize, usize)> {
    let extension = bytes.get(dot..dot + 3)?;
    if extension != b".rs" && extension != b".ri" {
        return None;
    }
    if dot == floor || !is_word_byte(bytes[dot - 1]) {
        return None;
    }
    let end = line_number_end(bytes, dot + 3)?;
    let mut start = dot;
    while start > floor && is_stem_byte(bytes[start - 1]) {
        start -= 1;
    }
    Some((start, end))
}

/// Past the first digit run of the line number that follows an extension
/// ending at `after`: either `:<digits>` or ` esc-<id>:<digits>`.
fn line_number_end(bytes: &[u8], after: usize) -> Option<usize> {
    let rest = &bytes[after..];
    let colon = if rest.starts_with(b":") {
        after
    } else if rest.starts_with(ESC_ANCHOR) {
        let id_start = after + ESC_ANCHOR.len();
        let id_end = run_end(bytes, id_start, |b| b.is_ascii_digit() || b == b'-');
        (id_end > id_start && bytes.get(id_end) == Some(&b':')).then_some(id_end)?
    } else {
        return None;
    };
    let digits_start = colon + 1;
    let digits_end = run_end(bytes, digits_start, |b| b.is_ascii_digit());
    (digits_end > digits_start).then_some(digits_end)
}

fn is_stem_byte(b: u8) -> bool {
    is_word_byte(b) || matches!(b, b'.' | b'/' | b'-')
}

/// The offset just past the run of `pred` bytes starting at `from`.
fn run_end(bytes: &[u8], from: usize, pred: impl Fn(u8) -> bool) -> usize {
    from + bytes[from..].iter().take_while(|&&b| pred(b)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Vec<&str> = Vec::new();

    #[test]
    fn a_cite_token_stops_at_its_first_digit_run() {
        assert_eq!(line_cites("// see cache.rs:617-621"), vec!["cache.rs:617"]);
    }

    #[test]
    fn a_ri_target_is_a_cite_too() {
        assert_eq!(
            line_cites("// (units.ri:116) composes"),
            vec!["units.ri:116"]
        );
    }

    #[test]
    fn a_path_qualified_cite_is_delimited_by_backticks() {
        assert_eq!(
            line_cites("/// `crates/reify-eval/src/unfold.rs:292-358` populates"),
            vec!["crates/reify-eval/src/unfold.rs:292"]
        );
    }

    #[test]
    fn several_cites_on_one_line_come_back_in_line_order() {
        assert_eq!(
            line_cites("// orientation.rs:264, orientation.rs:440"),
            vec!["orientation.rs:264", "orientation.rs:440"]
        );
    }

    #[test]
    fn doc_comment_lines_and_indented_comment_lines_are_comment_lines() {
        assert_eq!(line_cites("    /// see a.rs:1"), vec!["a.rs:1"]);
        assert_eq!(line_cites("//! see b.ri:2"), vec!["b.ri:2"]);
        assert_eq!(line_cites("\t// see c.rs:3"), vec!["c.rs:3"]);
    }

    #[test]
    fn a_line_that_is_not_a_comment_line_cites_nothing() {
        for line in [
            "let at = lookup(\"cache.rs:561\");",
            "assert!(ok, \"sanity (cache.rs:561)\");",
            "let x = 1; // cache.rs:561",
        ] {
            assert_eq!(line_cites(line), NONE, "{line:?} is not a comment line");
        }
    }

    #[test]
    fn the_esc_anchored_form_reports_through_its_line_number() {
        assert_eq!(
            line_cites("// (units.ri esc-4026-121:80-91 precedent)"),
            vec!["units.ri esc-4026-121:80"]
        );
    }

    #[test]
    fn a_tilde_lead_is_a_cite_reported_verbatim() {
        assert_eq!(
            line_cites("// The guards (graph.rs ~429, unfold.rs ~308)"),
            vec!["graph.rs ~429", "unfold.rs ~308"]
        );
    }

    #[test]
    fn a_parenthesised_tilde_lead_is_a_cite() {
        assert_eq!(
            line_cites("// args lookup in entity.rs (~2726-2790):"),
            vec!["entity.rs (~2726"]
        );
    }

    #[test]
    fn a_colon_tilde_lead_is_a_cite() {
        assert_eq!(
            line_cites("/// pre-pass (entity.rs:~531) and (guards.rs:~183)."),
            vec!["entity.rs:~531", "guards.rs:~183"]
        );
    }

    #[test]
    fn line_and_lines_leads_are_cites() {
        assert_eq!(
            line_cites("/// The inline arm (lib.rs line 444) guards"),
            vec!["lib.rs line 444"]
        );
        assert_eq!(
            line_cites("/// (kernel.rs lines 178–215): x"),
            vec!["kernel.rs lines 178"]
        );
        assert_eq!(
            line_cites("// see units.ri line 12"),
            vec!["units.ri line 12"]
        );
    }

    #[test]
    fn the_esc_anchor_composes_with_every_lead() {
        assert_eq!(
            line_cites("// (units.ri esc-4026-121 ~80)"),
            vec!["units.ri esc-4026-121 ~80"]
        );
        assert_eq!(
            line_cites("// units.ri esc-4026-121:~80"),
            vec!["units.ri esc-4026-121:~80"]
        );
    }

    #[test]
    fn colon_and_spelled_cites_on_one_line_come_back_in_line_order() {
        assert_eq!(
            line_cites("// a.rs:1 then b.rs ~2 and c.rs line 3"),
            vec!["a.rs:1", "b.rs ~2", "c.rs line 3"]
        );
    }

    #[test]
    fn spelled_near_misses_cite_nothing() {
        for line in [
            "// beam.rs (2)",
            "// foo.rs ~ 12",
            "// foo.rs~12",
            "// foo.rs  ~12",
            "// foo.rs, line 12",
            "// foo.rs (line 12)",
            "// foo.rs line ~12",
            "// foo.rs line",
            "// foo.rs lines of code",
            "// foo.rs linear 3",
            "// foo.rs ~",
            "// foo.rs (~)",
            "// foo.rs:~",
            "// foo.rsx ~1",
        ] {
            assert_eq!(line_cites(line), NONE, "{line:?} carries no hard cite");
        }
    }

    #[test]
    fn legal_spellings_without_a_line_number_cite_nothing() {
        for line in [
            "// units.ri esc-4026-121",
            "// units.ri",
            "// foo.rs::bar",
            "// foo.rs: see",
            "// foo.rsx:1",
            "// foo.rib:2",
            "// .rs:1",
        ] {
            assert_eq!(line_cites(line), NONE, "{line:?} carries no hard cite");
        }
    }

    #[test]
    fn documented_out_of_grammar_forms_cite_nothing() {
        for line in [
            "// lines 12-34 of foo.rs",
            "// documented in (structural_physical.ri:",
            "// see widget.js:12",
            "// see occt.h:30",
        ] {
            assert_eq!(line_cites(line), NONE, "{line:?} is out of grammar");
        }
    }

    #[test]
    fn non_ascii_text_neither_panics_nor_hides_a_cite() {
        assert_eq!(line_cites("// µm °C — foo.rs:12"), vec!["foo.rs:12"]);
        assert_eq!(line_cites("// é.rs:1 ü"), NONE);
    }

    #[test]
    fn the_grammar_is_pure_an_allow_marker_does_not_suppress_here() {
        assert_eq!(
            line_cites("// foo.rs:12 plinecite:allow — dated"),
            vec!["foo.rs:12"],
            "suppression is check()'s job, so the marker report stays \
             independent of this grammar"
        );
    }
}
