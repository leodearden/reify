//! PPRDSTATUS: PRD status-prose drift. A PRD's `Status:` header and its
//! status-annotated task cites are claims about the task graph, and nothing
//! re-checks them once the decomposition lands. This module reads the loaded
//! task corpus and flags the prose that has drifted from it.
//!
//! # Lanes
//!
//! - `stale-status-header:` — every decomposition leaf whose `metadata.prd`
//!   names the PRD is terminal, yet the PRD's Status header is live or absent.
//!   The finding names the stamp to apply: SHIPPED when any leaf landed,
//!   WITHDRAWN when every leaf was cancelled.
//! - `cite-status-contradiction:` — in a PRD whose header is NOT terminal, a
//!   canonical `#NNNN` cite (PTODO's grammar, so `task #5` and `invariant #2`
//!   are not task cites) is immediately followed by a parenthetical whose
//!   first token is a status word, and that word's class (live, done or
//!   cancelled) differs from the cited task's. Dated parentheticals, unknown
//!   ids and live-vs-live differences are silent.
//!
//! Every finding is High and keyed by the PRD's path.
//!
//! # Authority and inputs
//!
//! The terminal vocabulary, and the rule that the first token after the Status
//! label decides, belong to `.claude/skills/prd/project.md` → "PRD terminal
//! status — closed vocabulary + decompose-close stamp". This module consumes
//! that list and does not define it. Task status and `prd` come from
//! `ctx.task_metadata` (the fused-memory live loader, or `--tasks-file`),
//! never from a direct task-DB read. PRD membership comes from `ls_files()`,
//! and PRD text from the working tree. Capability manifests are excluded:
//! they are decompose-time gate artifacts with no Status header.
//!
//! # Why opt-in, and not a merge gate
//!
//! Task state lives only with the main checkout's fused-memory, so a task
//! worktree's verify gate cannot see it. The findings also track a standing
//! backlog that a human adjudicates doc by doc, and the CLI's exit code is the
//! High count. So the detector runs only under `--pattern PPRDSTATUS`.
//!
//! # Scope boundary
//!
//! The cite lane deliberately does NOT cover:
//!
//! - The MODAL form ("task N would retire …"), which carries no status token.
//!   It has no bounded grammar, and forward modals near terminal cites
//!   legitimately narrate history and counterfactuals. Building it needs a
//!   live-corpus false-positive enumeration first.
//! - Prose OUTSIDE `docs/prds/`: code comments, YAML and test headers. There a
//!   bare cite-liveness check is near-all false positives, and PTODO is
//!   deliberately anchor-scoped (`docs/prds/reify-audit-ptodo-detector.md`
//!   §8.1).
//! - Capability manifests: decompose-time gate artifacts whose status words
//!   are author-time evidence.
//! - Terminal-header PRDs: frozen AS-AUTHORED records.

use crate::ptodo::canonical_cite_occurrences;
use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity, TaskMetadata};
use std::collections::{BTreeMap, HashMap};

const PRDS_PREFIX: &str = "docs/prds/";
const CAPABILITY_MANIFEST_SUFFIX: &str = ".capability-manifest.md";
const STALE_STATUS_HEADER: &str = "stale-status-header";
const CITE_STATUS_CONTRADICTION: &str = "cite-status-contradiction";
const TERMINAL_STATUS_SECTION: &str = ".claude/skills/prd/project.md \
     \"PRD terminal status — closed vocabulary + decompose-close stamp\"";

/// The CLOSED vocabulary of terminal PRD statuses. Authority:
/// `.claude/skills/prd/project.md` → "PRD terminal status — closed vocabulary
/// + decompose-close stamp". Every other word is a live status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalToken {
    Shipped,
    Superseded,
    Withdrawn,
}

impl TerminalToken {
    const ALL: [TerminalToken; 3] = [Self::Shipped, Self::Superseded, Self::Withdrawn];

    /// ASCII case-insensitive EXACT match, so near-synonyms such as `landed`,
    /// `retired` or `shipped-ish` stay live.
    fn parse(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|terminal| terminal.as_str().eq_ignore_ascii_case(token))
    }

    /// The preferred ALL-CAPS spelling.
    fn as_str(self) -> &'static str {
        match self {
            Self::Shipped => "SHIPPED",
            Self::Superseded => "SUPERSEDED",
            Self::Withdrawn => "WITHDRAWN",
        }
    }
}

/// What a PRD's Status header says, as decided by its first label.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StatusHeader {
    Terminal(TerminalToken),
    /// `line` is the 1-based line of the label.
    Live {
        token: String,
        line: usize,
    },
    Absent,
}

/// How many leading lines may carry the Status label: the overlay's "within
/// the first ~10 lines", with margin for the deepest live label measured.
const STATUS_HEADER_WINDOW: usize = 12;

const STATUS_LABEL: &str = "Status";

/// The status token after the first Status label on `line`, or `None` when
/// the line carries no label. An empty token is still a label.
fn status_label_token(line: &str) -> Option<&str> {
    line.match_indices(STATUS_LABEL)
        .find_map(|(at, _)| token_after_label(line, at))
}

/// The token after the `Status` occurrence at byte `at`, if that occurrence
/// is a label: a left boundary, optional `*`/`_` closers, then `:`. That shape
/// rejects `Status legend:` and a `| Status |` table header by construction.
fn token_after_label(line: &str, at: usize) -> Option<&str> {
    if at > 0 && line.as_bytes()[at - 1].is_ascii_alphanumeric() {
        return None;
    }
    let value = line[at + STATUS_LABEL.len()..]
        .trim_start_matches(['*', '_'])
        .strip_prefix(':')?
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '*' | '_' | '`'));
    let end = value
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(value.len());
    Some(&value[..end])
}

/// The first Status label within [`STATUS_HEADER_WINDOW`] decides.
fn read_status_header(text: &str) -> StatusHeader {
    text.lines()
        .take(STATUS_HEADER_WINDOW)
        .enumerate()
        .find_map(|(index, line)| status_label_token(line).map(|token| (token, index + 1)))
        .map_or(StatusHeader::Absent, |(token, line)| {
            classify_status_token(token, line)
        })
}

fn classify_status_token(token: &str, line: usize) -> StatusHeader {
    match TerminalToken::parse(token) {
        Some(terminal) => StatusHeader::Terminal(terminal),
        None => StatusHeader::Live {
            token: token.to_string(),
            line,
        },
    }
}

/// A PRD proper: a Markdown file under `docs/prds/` that is not a capability
/// manifest.
fn is_prd_path(path: &str) -> bool {
    path.starts_with(PRDS_PREFIX)
        && path.ends_with(".md")
        && !path.ends_with(CAPABILITY_MANIFEST_SUFFIX)
}

fn normalise_prd_path(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed.strip_prefix("./").unwrap_or(trimmed)
}

/// A tracked PRD as both lanes see it: one read, one header parse.
struct PrdDoc {
    header: StatusHeader,
    text: String,
}

/// Every tracked, readable PRD, keyed and so sorted by path.
fn tracked_prds(ctx: &AuditContext) -> BTreeMap<String, PrdDoc> {
    ctx.git
        .ls_files()
        .into_iter()
        .filter(|path| is_prd_path(path))
        .filter_map(|path| {
            let text = ctx.read_relative(&path)?;
            let header = read_status_header(&text);
            Some((path, PrdDoc { header, text }))
        })
        .collect()
}

/// A status's terminality class. Live statuses churn between pending,
/// in-progress and deferred on a timescale no document tracks, so only a
/// difference in CLASS is drift; a done-vs-cancelled mismatch never heals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusClass {
    Live,
    Done,
    Cancelled,
}

/// The module's one status classifier, for a task's status and for the word
/// a cite's parenthetical asserts alike: the task statuses, plus the prose
/// spellings `active` and `canceled`.
const STATUS_WORDS: &[(&str, StatusClass)] = &[
    ("pending", StatusClass::Live),
    ("in-progress", StatusClass::Live),
    ("blocked", StatusClass::Live),
    ("deferred", StatusClass::Live),
    ("review", StatusClass::Live),
    ("active", StatusClass::Live),
    ("done", StatusClass::Done),
    ("cancelled", StatusClass::Cancelled),
    ("canceled", StatusClass::Cancelled),
];

/// The class of an exact status word; `None` for a word [`STATUS_WORDS`]
/// lacks.
fn status_class(word: &str) -> Option<StatusClass> {
    STATUS_WORDS
        .iter()
        .find(|(known, _)| *known == word)
        .map(|&(_, class)| class)
}

/// One PRD's decomposition leaves, split by status class. An empty or
/// unknown status counts as live, so it keeps the PRD silent.
#[derive(Debug, Default)]
struct LeafTally {
    done: Vec<String>,
    cancelled: Vec<String>,
    live: usize,
}

impl LeafTally {
    fn add(&mut self, task: &TaskMetadata) {
        match status_class(&task.status) {
            Some(StatusClass::Done) => self.done.push(task.task_id.clone()),
            Some(StatusClass::Cancelled) => self.cancelled.push(task.task_id.clone()),
            Some(StatusClass::Live) | None => self.live += 1,
        }
    }

    /// Every terminal leaf id, in numeric order.
    fn terminal_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self
            .done
            .iter()
            .chain(&self.cancelled)
            .map(String::as_str)
            .collect();
        ids.sort_by_key(|id| task_id_order(id));
        ids
    }
}

/// Numeric where the id parses, the string itself as the fallback.
fn task_id_order(id: &str) -> (Option<u64>, &str) {
    (id.parse().ok(), id)
}

fn leaf_tallies(tasks: &HashMap<String, TaskMetadata>) -> BTreeMap<String, LeafTally> {
    let mut tallies: BTreeMap<String, LeafTally> = BTreeMap::new();
    for task in tasks.values() {
        let Some(prd) = task.prd.as_deref().map(normalise_prd_path) else {
            continue;
        };
        if is_prd_path(prd) {
            tallies.entry(prd.to_string()).or_default().add(task);
        }
    }
    tallies
}

/// SHIPPED tolerates cancelled leaves beside a landed one; WITHDRAWN is for a
/// PRD none of whose leaves landed.
fn recommended_stamp(tally: &LeafTally) -> TerminalToken {
    if tally.done.is_empty() {
        TerminalToken::Withdrawn
    } else {
        TerminalToken::Shipped
    }
}

/// Lane 1, sorted by PRD path. A PRD that is untracked or unreadable is
/// silent: a missing file cannot carry a stale header.
fn stale_status_findings(
    tasks: &HashMap<String, TaskMetadata>,
    prds: &BTreeMap<String, PrdDoc>,
) -> Vec<Finding> {
    leaf_tallies(tasks)
        .into_iter()
        .filter(|(_, tally)| tally.live == 0)
        .filter_map(|(path, tally)| stale_status_finding(&path, &tally, &prds.get(&path)?.header))
        .collect()
}

fn stale_status_finding(path: &str, tally: &LeafTally, header: &StatusHeader) -> Option<Finding> {
    let header_reading = match header {
        StatusHeader::Terminal(_) => return None,
        StatusHeader::Live { token, line } => format!("reads '{token}' (line {line})"),
        StatusHeader::Absent => "is absent".to_string(),
    };
    let leaf_ids = tally
        .terminal_ids()
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ");
    let summary = format!(
        "{STALE_STATUS_HEADER}: {path} — all {total} decomposition leaves are terminal \
         ({done} done, {cancelled} cancelled: {leaf_ids}) but its Status header \
         {header_reading}; stamp {stamp} (or SUPERSEDED naming the successor) with the \
         freeze header per {TERMINAL_STATUS_SECTION}",
        total = tally.done.len() + tally.cancelled.len(),
        done = tally.done.len(),
        cancelled = tally.cancelled.len(),
        stamp = recommended_stamp(tally).as_str(),
    );
    Some(prd_finding(path, summary))
}

fn prd_finding(path: &str, summary: String) -> Finding {
    Finding {
        pattern: Pattern::PPrdStatus,
        severity: Severity::High,
        task_id: path.to_string(),
        summary,
        evidence: vec![EvidenceRef::File {
            path: path.to_string(),
        }],
    }
}

/// The status a parenthetical asserts: its first token (a run of ASCII
/// letters and `-`, after any markup) when that token is a status word or a
/// `<word>-` compound such as `pending-high`.
fn asserted_status(paren_body: &str) -> Option<(&str, StatusClass)> {
    let body =
        paren_body.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '`' | '*' | '_'));
    let end = body
        .find(|c: char| !(c.is_ascii_alphabetic() || c == '-'))
        .unwrap_or(body.len());
    let token = &body[..end];
    let lower = token.to_ascii_lowercase();
    STATUS_WORDS
        .iter()
        .find(|(word, _)| lower == *word || lower.starts_with(&format!("{word}-")))
        .map(|&(_, class)| (token, class))
}

/// An as-of assertion is the sanctioned snapshot form, so it is not drift: an
/// ISO `DDDD-DD-DD` date, or the phrases `as of` / `at freeze`. A false
/// positive here only silences a cite, which is the fail-safe direction.
fn is_dated(paren_body: &str) -> bool {
    let lower = paren_body.to_ascii_lowercase();
    lower.contains("as of") || lower.contains("at freeze") || contains_iso_date(paren_body)
}

fn contains_iso_date(text: &str) -> bool {
    const SHAPE: &[u8] = b"dddd-dd-dd";
    text.as_bytes().windows(SHAPE.len()).any(|window| {
        window.iter().zip(SHAPE).all(|(&byte, &shape)| match shape {
            b'd' => byte.is_ascii_digit(),
            literal => byte == literal,
        })
    })
}

/// The body of the parenthetical immediately after the cite whose `#` is at
/// byte `cite_at`: past the digit run, any `*`/`_`/`` ` `` closers and at most
/// one space. The body runs to the first `)`, or to the end of the line.
fn adjacent_parenthetical(line: &str, cite_at: usize) -> Option<&str> {
    let after_cite = line[cite_at + 1..]
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_start_matches(['*', '_', '`']);
    let body = after_cite
        .strip_prefix(' ')
        .unwrap_or(after_cite)
        .strip_prefix('(')?;
    Some(body.split_once(')').map_or(body, |(inside, _)| inside))
}

/// A cite whose adjacent parenthetical asserts a status the task contradicts.
struct Contradiction<'a> {
    asserted: &'a str,
    real: &'a str,
}

fn cite_contradiction<'a>(
    line: &'a str,
    cite_at: usize,
    id: u32,
    tasks: &'a HashMap<String, TaskMetadata>,
) -> Option<Contradiction<'a>> {
    let body = adjacent_parenthetical(line, cite_at)?;
    if is_dated(body) {
        return None;
    }
    let (asserted, asserted_class) = asserted_status(body)?;
    let real = tasks.get(&id.to_string())?.status.as_str();
    (status_class(real)? != asserted_class).then_some(Contradiction { asserted, real })
}

/// Lane 2 over one non-terminal PRD, deduplicated and sorted on (line, id).
fn contradictions_in(
    path: &str,
    text: &str,
    tasks: &HashMap<String, TaskMetadata>,
) -> Vec<Finding> {
    let mut by_line_and_id: BTreeMap<(usize, u32), Finding> = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        for (cite_at, id) in canonical_cite_occurrences(line) {
            if let Some(contradiction) = cite_contradiction(line, cite_at, id, tasks) {
                by_line_and_id
                    .entry((index + 1, id))
                    .or_insert_with(|| contradiction_finding(path, index + 1, id, &contradiction));
            }
        }
    }
    by_line_and_id.into_values().collect()
}

fn contradiction_finding(
    path: &str,
    line: usize,
    id: u32,
    contradiction: &Contradiction,
) -> Finding {
    let summary = format!(
        "{CITE_STATUS_CONTRADICTION}: {path} line {line}: #{id} is asserted '{asserted}' but the \
         task is {real}; cite the task id without a status word (.claude/skills/prd/project.md \
         \"PRD terminal status\" → \"Cite task IDs, never task status\")",
        asserted = contradiction.asserted,
        real = contradiction.real,
    );
    prd_finding(path, summary)
}

/// Lane 2, sorted by (path, line, id). A terminal-header PRD is skipped: its
/// freeze header marks the body as an AS-AUTHORED record, not a current
/// statement of fact, and that body must not be edited.
fn cite_contradiction_findings(
    tasks: &HashMap<String, TaskMetadata>,
    prds: &BTreeMap<String, PrdDoc>,
) -> Vec<Finding> {
    prds.iter()
        .filter(|(_, doc)| !matches!(doc.header, StatusHeader::Terminal(_)))
        .flat_map(|(path, doc)| contradictions_in(path, &doc.text, tasks))
        .collect()
}

/// Run PPRDSTATUS over the tracked PRDs against the loaded task corpus: lane-1
/// findings, then lane-2 findings.
///
/// An empty corpus yields no findings, which means "not checked", never
/// "clean", so a caller must not report it as clean. The CLI skips this
/// detector on an empty corpus and prints a breadcrumb instead.
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let prds = tracked_prds(ctx);
    let mut findings = stale_status_findings(&ctx.task_metadata, &prds);
    findings.extend(cite_contradiction_findings(&ctx.task_metadata, &prds));
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `docs/prds/kernel-seam-contracts.md` before its SHIPPED re-stamp
    /// (`edd9703fae^`): the live `Status: contract` header.
    const KERNEL_SEAM_CONTRACTS_PRE_FIX: &str =
        include_str!("../tests/fixtures/pprdstatus/kernel-seam-contracts.pre-edd9703fae.md");

    /// `docs/prds/kernel-seam-contracts.md` after its SHIPPED re-stamp
    /// (`edd9703fae`).
    const KERNEL_SEAM_CONTRACTS_POST_FIX: &str =
        include_str!("../tests/fixtures/pprdstatus/kernel-seam-contracts.post-edd9703fae.md");

    /// `docs/prds/v0_6/data-carrying-enums.md`, lines 1-3.
    const DATA_CARRYING_ENUMS: &str =
        include_str!("../tests/fixtures/pprdstatus/data-carrying-enums.head.md");

    /// `docs/prds/v0_6/process-dfm-geometry-metrology.md`, lines 1-3.
    const PROCESS_DFM_GEOMETRY_METROLOGY: &str = r#"# PRD (SUPERSEDED → SPLIT): `std.process` geometry-metrology DFM engine

**Status:** SUPERSEDED 2026-06-08 · split into two PRDs after a feasibility sweep · **Milestone:** v0_6
"#;

    /// `docs/prds/auto-type-param-resolution.md`, lines 1-3.
    const AUTO_TYPE_PARAM_RESOLUTION: &str = r#"# PRD: `auto` Type-Parameter Resolution (`Bearing<auto: Seal>`)

Status: Superseded by docs/prds/v0_3/auto-type-param-resolution-completion.md (v0.3 completion contract). Applied after residuals α/β/γ/δ landed.
"#;

    /// `docs/prds/kinematic-constraints.md`, lines 1-6.
    const KINEMATIC_CONSTRAINTS: &str = r#"# Kinematic Constraints — Forward, Open-Chain, Library-Level

## §0 — Superseded

Status: deferred — superseded by `docs/prds/v0_3/kinematic-constraints-completion.md`
(authored 2026-05-17; decomposition landed 2026-07-06).
"#;

    /// `docs/prds/merge-gate-guard-diagnosability.md`, lines 1-5.
    const MERGE_GATE_GUARD_DIAGNOSABILITY: &str = r#"# PRD: kLOC-cap guard diagnosability + at-source trigger

**Date:** 2026-07-22 · **Status:** approved for decomposition · version-agnostic
(root `docs/prds/`). **Approach: B** (two point-hardenings of an existing,
landed guard — no new mechanism, no new seam).
"#;

    /// `docs/prds/naming-convergence/P1-structured-featureid-feature-value.md`, lines 1-4.
    const P1_STRUCTURED_FEATUREID_FEATURE_VALUE: &str = r#"# P1 — Structured `FeatureId` + first-class `Feature` value + fallible codec

> **Status:** active (Wave 1, independent foundation). Naming & Selection Convergence program,
> P1 of P0–P4. Date: 2026-06-24. Approach **B + H** (contract + two-way boundary tests).
"#;

    /// `docs/prds/v0_4/fea-result-model.capability-manifest.md`, lines 1-7.
    const FEA_RESULT_MODEL_CAPABILITY_MANIFEST: &str = r#"# Capability Manifest — fea-result-model.md

Mechanizes G3 + G6 per leaf (overlay → *Capability Manifest — reify evidence forms*). Each task's user-observable/RED signal is decomposed into asserted capabilities, each bound to evidence ∈ `{grep:file:line-wired | producer:task-upstream | grammar-fixture:parses | floor:bound>X | field-population}`. A binding resolving to `{declared-only | test-only | producer-absent | producer-downstream | fixture-ERROR | bound≤floor}` **blocks** queueing until resolved.

Sentinel for this PRD: `Value::Undef` (and the `{ ElasticResult() }` stub body, `scalar_channels: HashMap::new()`, `displaced_positions: None`). Evidence current as of 2026-05-30; G3 fixtures `/tmp/prd-gate-fixtures/fea-result-model-{1,2}.ri` parse with 0 ERROR nodes.

**Status legend:** ✅ PASS · ⏳ FAIL-today-resolved-by-this-batch (in-batch producer is upstream; DAG-correct) · ⛔ BLOCK (must resolve before queue).
"#;

    /// `docs/prds/merge-gate-health.capability-manifest.md`, line 12.
    const MERGE_GATE_HEALTH_CAPABILITY_MANIFEST_TABLE_HEADER: &str = r#"| Leaf | Capability asserted | Evidence binding | Status |
"#;

    /// `docs/prds/kernel-seam-contracts.capability-manifest.md`, lines 1-10.
    const KERNEL_SEAM_CONTRACTS_CAPABILITY_MANIFEST: &str = r#"# Capability Manifest — kernel-seam-contracts

> **AS-AUTHORED GATE ARTIFACT (2026-07-06) — do not refresh.** This manifest records the
> **pre-decomposition** evidence check that cleared this PRD's leaves to queue; its `PASS` verdicts
> are statements about *binding quality at author time*, not about landed state. All 16 leaves have
> since landed (α #5102 … ξ #5116, plus #4876) — see the parent PRD's SHIPPED header. Consequently
> its forward-looking phrasings ("post-landing grep", "red on current main", "the leaf must
> establish") and its hard `file:line` / `@NNNN` anchors are 2026-07-06 provenance and have drifted.
> Rewriting them would destroy the record of what was actually gated. Parent:
> `docs/prds/kernel-seam-contracts.md`.
"#;

    /// `docs/prds/v0_6/tolerance-stackup-analysis.md`, lines 1-12.
    const TOLERANCE_STACKUP_ANALYSIS: &str = r#"# Tolerance Stack-Up Analysis

> A designer who has dimensioned a stacked/assembled set of parts wants one question
> answered before release: **does the accumulated ±tolerance keep a critical gap or fit
> within spec?** Reify already lets you *declare* per-feature dimensional tolerances
> (`stdlib/tolerancing.ri`: `DimensionalTolerance`, GD&T traits, `Fit`). What is missing is
> the *analysis* that propagates those tolerances along a dimension chain and reports the
> resulting gap distribution — worst-case, statistical (RSS), and Monte-Carlo. This PRD adds
> that analysis as a set of stdlib builtins surfaced through `reify eval`, mirroring the
> existing stress-analysis builtin pattern (`stdlib/analysis.ri` + `reify-stdlib/src/analysis.rs`).

Status: contract (B+H). Authored 2026-05-27 in a `/prd` spec-gap-filling batch.
"#;

    /// `docs/prds/v0_3/auto-type-param-constraint-seeding-gaps.md`, lines 1-3.
    const AUTO_TYPE_PARAM_CONSTRAINT_SEEDING_GAPS: &str = r#"# `auto:` Constraint-Seeding Gaps — Computed Defaults (C) and Nested Member Access (D)

Status: completion-residual contract for
"#;

    fn live(token: &str, line: usize) -> StatusHeader {
        StatusHeader::Live {
            token: token.to_string(),
            line,
        }
    }

    #[test]
    fn plain_live_label_reports_its_token_and_line() {
        assert_eq!(
            read_status_header(KERNEL_SEAM_CONTRACTS_PRE_FIX),
            live("contract", 3)
        );
    }

    #[test]
    fn bold_label_with_trailing_period_is_terminal() {
        assert_eq!(
            read_status_header(KERNEL_SEAM_CONTRACTS_POST_FIX),
            StatusHeader::Terminal(TerminalToken::Shipped)
        );
    }

    #[test]
    fn bold_token_after_bold_label_is_terminal() {
        assert_eq!(
            read_status_header(DATA_CARRYING_ENUMS),
            StatusHeader::Terminal(TerminalToken::Shipped)
        );
    }

    /// The successor is not named on the Status line; the token alone decides.
    #[test]
    fn superseded_token_decides_without_a_named_successor() {
        assert_eq!(
            read_status_header(PROCESS_DFM_GEOMETRY_METROLOGY),
            StatusHeader::Terminal(TerminalToken::Superseded)
        );
    }

    #[test]
    fn title_case_terminal_token_is_terminal() {
        assert_eq!(
            read_status_header(AUTO_TYPE_PARAM_RESOLUTION),
            StatusHeader::Terminal(TerminalToken::Superseded)
        );
    }

    /// A substring match on "superseded" would misclassify this header.
    #[test]
    fn only_the_first_token_decides_terminality() {
        assert_eq!(
            read_status_header(KINEMATIC_CONSTRAINTS),
            live("deferred", 5)
        );
    }

    #[test]
    fn withdrawn_is_terminal_in_either_case() {
        for text in [
            "# Retired PRD\n\n**Status:** WITHDRAWN — every leaf was cancelled, no successor.\n",
            "# Retired PRD\n\nStatus: withdrawn after the 2026-09 review.\n",
        ] {
            assert_eq!(
                read_status_header(text),
                StatusHeader::Terminal(TerminalToken::Withdrawn),
                "{text}"
            );
        }
    }

    #[test]
    fn mid_line_label_is_found() {
        assert_eq!(
            read_status_header(MERGE_GATE_GUARD_DIAGNOSABILITY),
            live("approved", 3)
        );
    }

    #[test]
    fn blockquote_label_is_found() {
        assert_eq!(
            read_status_header(P1_STRUCTURED_FEATUREID_FEATURE_VALUE),
            live("active", 3)
        );
    }

    #[test]
    fn status_legend_is_not_a_label() {
        assert_eq!(
            read_status_header(FEA_RESULT_MODEL_CAPABILITY_MANIFEST),
            StatusHeader::Absent
        );
    }

    #[test]
    fn table_header_status_column_is_not_a_label() {
        assert_eq!(
            read_status_header(MERGE_GATE_HEALTH_CAPABILITY_MANIFEST_TABLE_HEADER),
            StatusHeader::Absent
        );
    }

    #[test]
    fn terminal_word_in_prose_without_a_label_is_absent() {
        assert_eq!(
            read_status_header(KERNEL_SEAM_CONTRACTS_CAPABILITY_MANIFEST),
            StatusHeader::Absent
        );
    }

    #[test]
    fn header_window_ends_after_the_deepest_measured_label() {
        assert_eq!(
            read_status_header(TOLERANCE_STACKUP_ANALYSIS),
            live("contract", 12)
        );
        let past_window = format!(
            "{}Status: SHIPPED\n",
            "filler\n".repeat(STATUS_HEADER_WINDOW)
        );
        assert_eq!(read_status_header(&past_window), StatusHeader::Absent);
    }

    #[test]
    fn dropped_synonyms_are_not_terminal() {
        assert_eq!(
            read_status_header("# Old PRD\n\nStatus: landed 2026-07-01, all leaves done.\n"),
            live("landed", 3)
        );
        assert_eq!(
            read_status_header("# Old PRD\n\nStatus: retired in favour of the v0.3 PRD.\n"),
            live("retired", 3)
        );
    }

    #[test]
    fn first_label_in_the_window_wins() {
        let text = "# Two labels\n\nStatus: active\n\n\nStatus: SHIPPED\n";
        assert_eq!(read_status_header(text), live("active", 3));
    }

    #[test]
    fn hyphenated_token_is_kept_whole() {
        assert_eq!(
            read_status_header(AUTO_TYPE_PARAM_CONSTRAINT_SEEDING_GAPS),
            live("completion-residual", 3)
        );
    }
}
