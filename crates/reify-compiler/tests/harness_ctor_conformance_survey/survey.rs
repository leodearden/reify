//! The upper layers of the ctor-conformance corpus survey, above the
//! disposition resolver: the markdown renderer, the stamp guard and the
//! `#[ignore]`d generator.
//!
//! What the survey is for, why its corpus walk is `#[ignore]`d while every
//! decision is gate-resident, and how to retire it are stated once, in the
//! harness root `crates/reify-compiler/tests/harness_ctor_conformance_survey.rs`.

use std::path::PathBuf;

use crate::corpus::{CorpusHalf, corpus_parity, tracked_ri_corpus, tracked_rust_hosts};
use crate::disposition::{
    CTOR_CONFORMANCE_CORPUS_RESIDUAL, CTOR_CONFORMANCE_SITE_SEVERITY, Disposition,
    EXAMPLES_PREFIX, assert_no_unwaived_ctor_conformance_sites, disposition_of,
};
use crate::owner::{Owner, remedy_hint};
use crate::survey_site::{DefOrigin, SurveySite, synth_inline_site, synth_site};
use crate::sweep::{INLINE_TEMPLATE_REASON, SurveyRun, survey_corpus, survey_inline_corpus};
use crate::stamp::{FULL_SHA_LEN, SurveyStamp, survey_stamp};
use crate::workspace_git::{WORKSPACE_ROOT, git_is_available, git_read, git_succeeds};

// ─── step 11/12: markdown rendering ─────────────────────────────────────────

/// The EXACT command that regenerates the artifact, committed inside it.
///
/// The `env` prefix is not decoration: reify's PreToolUse hook rewrites bare
/// `cargo test` invocations into condensed `PASS: N | FAIL: M` output. That is
/// harmless here — the generator WRITES the file rather than being scraped from
/// stdout — but it will confuse a reader of the run log who expects to see the
/// usual per-test lines, so the bypass is baked into the published command.
const REGEN_COMMAND: &str = "env cargo test -p reify-compiler \
     --test harness_ctor_conformance_survey -- --ignored --exact \
     survey::generate_ctor_conformance_corpus_survey";

/// Render `text` safe for a markdown table cell.
///
/// Both `|` and newlines are neutralised: either one inside a diagnostic
/// message would silently split or truncate the row, and a survey whose table
/// breaks on its most interesting entries is worse than no survey.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
        .replace(['\n', '\r'], " ")
        .trim()
        .to_owned()
}

/// Render an optional column: `—` when unrecoverable, never empty, never a guess.
fn opt_cell(value: Option<&String>) -> String {
    match value {
        Some(v) => cell(v),
        None => "—".to_owned(),
    }
}

/// The header key that introduces the drift disclosure.
///
/// One spelling, so the renderer and the tests asserting on its presence — and
/// on its absence from an undrifted run — cannot disagree about what a
/// disclosure looks like.
///
/// Names no extension: a drifted member can now be either half's — a tracked
/// `.ri`, or a `.rs` host whose bytes an inline row describes.
const DRIFT_DISCLOSURE_KEY: &str = "**Drifted corpus members since the anchor:**";

/// One cell of a site table, derived from the site.
type SiteCell = fn(&SurveySite) -> String;

/// The site-table columns BOTH halves render, in order.
///
/// One list, so the two halves cannot acquire independently-drifting layouts —
/// which is the whole reason the renderer is shared rather than copied. The
/// header row and the body rows are both generated from it, so they also cannot
/// disagree about column order or count.
const SHARED_SITE_COLUMNS: &[(&str, SiteCell)] = &[
    ("site", |s| format!("`{}:{}`", cell(&s.file), s.line)),
    ("def", |s| opt_cell(s.def.as_ref())),
    ("def source", |s| cell(s.def_origin.label())),
    ("field", |s| opt_cell(s.field.as_ref())),
    ("expected", |s| opt_cell(s.expected.as_ref())),
    ("found", |s| opt_cell(s.found.as_ref())),
    ("code", |s| format!("`{}`", cell(&s.code))),
    ("severity", |s| cell(&s.severity)),
    ("hint (advisory)", |s| {
        cell(&remedy_hint(s.expected.as_deref(), s.found.as_deref()))
    }),
    (DISPOSITION_COLUMN, |s| cell(&disposition_of(s).label())),
    ("message", |s| cell(&s.message)),
];

/// [`SHARED_SITE_COLUMNS`] for `half`.
///
/// The inline half inserts EXACTLY ONE extra column, immediately after `site`:
/// its rows' `site` cell is the HOST `.rs` position, and a reader needs the
/// snippet-relative coordinate as well to find the declaration inside the
/// literal. Everything else is shared verbatim.
fn site_columns(half: CorpusHalf) -> Vec<(&'static str, SiteCell)> {
    let mut columns = SHARED_SITE_COLUMNS.to_vec();
    if half == CorpusHalf::InlineRustHost {
        columns.insert(
            1,
            (SNIPPET_LINE_COLUMN, |s| {
                s.snippet_line
                    .map_or_else(|| "—".to_owned(), |n| n.to_string())
            }),
        );
    }
    columns
}

/// `run`'s sites bucketed by [`Owner`], in [`Owner::render_order`], each bucket
/// in the artifact's `(file, line, field)` order.
///
/// Derived from the enum for BOTH halves rather than listed per half: a future
/// `Owner` variant then appears in both or in neither, which is the same
/// silent-drop failure `Owner::render_order` itself exists to prevent.
fn sites_by_owner(sites: &[SurveySite]) -> Vec<(Owner, Vec<&SurveySite>)> {
    Owner::render_order()
        .into_iter()
        .map(|owner| {
            let mut group: Vec<&SurveySite> = sites.iter().filter(|s| s.owner == owner).collect();
            group.sort_by(|a, b| (&a.file, a.line, &a.field).cmp(&(&b.file, b.line, &b.field)));
            (owner, group)
        })
        .collect()
}

/// Append `half`'s site table for `group`.
fn push_site_table(md: &mut String, half: CorpusHalf, group: &[&SurveySite]) {
    use std::fmt::Write as _;

    let columns = site_columns(half);
    for (name, _) in &columns {
        let _ = write!(md, "| {name} ");
    }
    md.push_str("|\n|");
    for _ in &columns {
        md.push_str("---|");
    }
    md.push('\n');
    for site in group {
        for (_, render) in &columns {
            let _ = write!(md, "| {} ", render(site));
        }
        md.push_str("|\n");
    }
}

/// Append a `| file | reason |` coverage table, or `empty_note` when there is
/// nothing to disclose.
///
/// Shared by both halves and by both of each half's buckets: a bounded sweep
/// that does not state what it skipped reads as full coverage, and one renderer
/// means neither half can quietly stop saying so.
fn push_coverage_table(md: &mut String, rows: &[(String, String)], empty_note: &str) {
    use std::fmt::Write as _;

    if rows.is_empty() {
        md.push_str(empty_note);
        return;
    }
    md.push_str("| file | reason |\n|---|---|\n");
    for (file, reason) in rows {
        let _ = writeln!(md, "| `{}` | `{}` |", cell(file), cell(reason));
    }
    md.push('\n');
}

/// The heading of the inline half's coverage subsection.
const INLINE_COVERAGE_HEADING: &str = "### Inline coverage";

/// Append named limitation 1 — what the inline walker reaches, and what it does
/// not.
///
/// Every quantity comes from `inline`, never from prose: a hand-typed count is
/// right on the day it is written and silently wrong afterwards, and this
/// artifact's own provenance section promises zero hand-derived counts. The
/// UNREACHED classes are therefore named by the Rust construct — or, for a host
/// the walker never opens, the PATH — to grep for, and deliberately carry no
/// frozen number: the reader counts them at the stamped commit, against a
/// corpus this generator does not enumerate. Both dimensions have to be
/// disclosed, because a missed LITERAL shape and an unopened HOST are equally
/// invisible to a reader of the rows above, and the next change to the walker's
/// scope or to the conformance severity needs this list rather than a search.
fn push_inline_limitation(md: &mut String, inline: &SurveyRun) {
    use std::fmt::Write as _;

    let templates = inline
        .not_surveyed
        .iter()
        .filter(|(_, reason)| reason == INLINE_TEMPLATE_REASON)
        .count();

    let _ = write!(
        md,
        "1. {INLINE_LIMITATION_KEY}, and only as those.**\n\
           The *Inline Rust fixtures* section above sweeps every tracked `.rs` under\n\
           `crates/` — test file or production source alike, since a `#[cfg(test)] mod\n\
           tests` hosts fixtures like any other — for raw-string literals (`r\"…\"`,\n\
           `r#\"…\"#`) whose text reads as Reify declaration grammar, and compiles each\n\
           through the same pipeline as a tracked `.ri`. That reached **{total} inline\n\
           member(s)**, of which **{templates}** were `format!` template(s) — listed\n\
           above under their own coverage reason rather than dropped, because a\n\
           template's `{{…}}` holes are not Reify syntax and a parse failure on one would\n\
           say nothing about conformance.\n\
           What a raw-string walker does **not** reach, each named by the construct to\n\
           grep for: Reify text carried in an ORDINARY `\"…\"` string literal (including\n\
           the backslash-continued multi-line form); text assembled by `concat!`; and text\n\
           built at run time by a `String` helper (`push_str`, `join`). Those are\n\
           unreached BY CONSTRUCTION, not by oversight — recovering them needs\n\
           const-evaluation or execution where this needs only a lexer — so a site in one\n\
           of those shapes is absent from the section above rather than reported clean.\n\
           `include_str!` and `read_to_string` goldens, by contrast, need no machinery at\n\
           all: their target `.ri` files are tracked, so the FIRST half already\n\
           enumerated them.\n\
           Unreached HOST files are the other half of this residual, and they are a SCOPE\n\
           decision rather than a walker limitation: `gui/src-tauri/**/*.rs` (the Tauri\n\
           sidecar) and `tree-sitter-reify/**/*.rs` (the grammar crate) are separate cargo\n\
           and grammar projects, so the host predicate anchors at `crates/` and never\n\
           opens them. Enumerate them with\n\
           `git ls-files -- 'gui/src-tauri/**/*.rs' 'tree-sitter-reify/**/*.rs'`; a\n\
           conformance site inside one is absent from the section above, not clean.\n",
        total = inline.total,
    );
}

/// Append the whole `## Inline Rust fixtures` section.
///
/// A section of its own, not extra rows in `## Sites`, because the two halves
/// answer different questions: a `.ri` row is a file a reader opens and may have
/// to fix, while an inline row is a census entry whose verdict its host test
/// owns ([`Disposition::InlineCensus`]). Merging them would make the artifact's
/// site count unsizeable and its owner groups mean two different things at once.
fn push_inline_section(md: &mut String, inline: &SurveyRun) {
    use std::fmt::Write as _;

    let _ = writeln!(md, "{INLINE_SECTION_HEADING}\n");
    md.push_str(
        "Reify snippets embedded in Rust test sources as raw-string literals, swept by\n\
         the SAME pipeline as the tracked `.ri` corpus above. A row's `site` cell is the\n\
         HOST `.rs` position to open; the `snippet line` cell locates the declaration\n\
         inside the literal.\n\n\
         Every row here carries the `census` disposition: its host test owns the\n\
         verdict, and the rows are enumerated rather than ruled on so the class stays\n\
         countable and cannot recur unnoticed on the next severity change.\n\n",
    );

    if inline.sites.is_empty() {
        md.push_str(
            "**No ctor-conformance sites were found in the inline Rust fixtures.** This is\n\
             an explicit zero, not a truncated run — see the coverage subsection below for\n\
             what was and was not swept.\n\n",
        );
    }
    for (owner, group) in sites_by_owner(&inline.sites) {
        let _ = writeln!(md, "### {} — {} site(s)\n", owner.title(), group.len());
        if group.is_empty() {
            md.push_str("_(none)_\n\n");
            continue;
        }
        push_site_table(md, CorpusHalf::InlineRustHost, &group);
        md.push('\n');
    }

    let _ = writeln!(md, "{INLINE_COVERAGE_HEADING}\n");
    let _ = writeln!(
        md,
        "Of {} inline member(s) — one per extracted snippet, plus one per host that \
         could not be read at all — **{} were swept** and **{} were not**. A further \
         **{}** were swept only PARTIALLY. A member is keyed `<host>:<line>`, the host \
         line the snippet's own line 1 sits on.\n",
        inline.total,
        inline.surveyed,
        inline.not_surveyed.len(),
        inline.partial.len()
    );
    md.push_str("#### Not swept (contributed no sites)\n\n");
    push_coverage_table(
        md,
        &inline.not_surveyed,
        "_(none — every extracted snippet reached the compile phase)_\n\n",
    );
    md.push_str(
        "#### Partially swept (sites collected, but the snippet also failed to compile)\n\n",
    );
    push_coverage_table(md, &inline.partial, "_(none)_\n\n");
}

/// Render the survey artifact.
///
/// Follows the house convention for a generated markdown artifact set by
/// `docs/architecture-audit/g-tool-baseline-report.md`: a
/// `**Captured:** / **Tool:** / **Design:**` header block above a
/// `## How to regenerate` section holding the literal command.
///
/// Deliberate divergence from that report: it pairs with an `#[ignore]`d
/// tolerance-based freshness test because it is a STANDING baseline. This
/// survey is a point-in-time SNAPSHOT that γ will legitimately invalidate — a
/// freshness gate would go red on every γ commit and would be driven to an
/// EMPTY artifact the moment γ reaches its stated signal, destroying the very
/// census that sized it. So the base commit SHA is stamped instead.
///
/// That stamp is the merge base, never the branch tip ([`survey_stamp`]). When
/// tracked `.ri` have drifted from it, they are NAMED in the header rather than
/// refused, so the snapshot stays honest by disclosure ([`SurveyStamp`]).
fn render_survey(run: &SurveyRun, inline: &SurveyRun, stamp: &SurveyStamp) -> String {
    use std::fmt::Write as _;

    let mut md = String::new();
    let site_count = run.sites.len();

    // ── header ──────────────────────────────────────────────────────────────
    md.push_str("# Struct-ctor field-type conformance — corpus survey\n\n");
    let _ = writeln!(md, "**Base commit:** `{}`", stamp.anchor);
    let _ = writeln!(
        md,
        "**Tool:** `crates/reify-compiler/tests/harness_ctor_conformance_survey.rs`"
    );
    md.push_str("**Design:** `docs/prds/struct-ctor-field-type-conformance.md` (task β, §8)\n");
    // Stated as two numbers, never their sum: the halves carry different
    // dispositions and different owners, and one total erases both.
    let _ = writeln!(
        md,
        "**Sites:** {site_count} in the tracked `.ri` corpus; {} in inline Rust fixtures",
        inline.sites.len()
    );
    let _ = writeln!(
        md,
        "**Corpus:** {} members of the `{}` (enumeration parity floor {}); {} snippets \
         extracted from the `{}` (enumeration parity floor {} hosts)",
        run.total,
        CorpusHalf::TrackedRi.label(),
        CorpusHalf::TrackedRi.floor(),
        inline.total,
        CorpusHalf::InlineRustHost.label(),
        CorpusHalf::InlineRustHost.floor(),
    );
    let _ = writeln!(
        md,
        "**`.ri` coverage:** {} surveyed, {} not surveyed, {} partial",
        run.surveyed,
        run.not_surveyed.len(),
        run.partial.len()
    );

    // Rendered ONLY when something drifted: an undrifted run must carry no
    // disclosure at all, so the artifact grows no permanent "0 files drifted"
    // row and two undrifted runs stay byte-comparable.
    if !stamp.drifted.is_empty() {
        let _ = write!(
            md,
            "\n\
            {DRIFT_DISCLOSURE_KEY} {n} tracked corpus members — `.ri` files, `.rs` hosts,\n\
            or both — differ between the anchor and the commit surveyed, so for those files\n\
            the anchor names OLDER bytes than the rows below describe. The list is filtered\n\
            to the two corpora, so it names exactly the files whose bytes a row could\n\
            describe and no unrelated churn. They are disclosed rather than refused because\n\
            they are COMMITTED: each is reachable from the surveyed commit, so a reader can\n\
            read back exactly what was swept. (Uncommitted bytes are reachable from no\n\
            commit, which is why a dirty tree is refused outright instead — see\n\
            `stamp_decision`.)\n\
            \n",
            n = stamp.drifted.len(),
        );
        for path in &stamp.drifted {
            let _ = writeln!(md, "- `{}`", cell(path));
        }
    }

    md.push_str(
        "\n\
        This is a point-in-time **snapshot**, not a freshness-gated golden file. γ will\n\
        legitimately invalidate it — that is the point. Its job is to enumerate and size,\n\
        once, at the base commit stamped above.\n\
        \n\
        ## Provenance\n\
        \n\
        Every row and every count here is **machine-generated — zero hand-derived\n\
        entries**. The corpus is `git ls-files -- '*.ri'`; each member is compiled\n\
        in-process by the compiler at the commit surveyed (`parse_with_stdlib` →\n\
        `compile_with_stdlib`) and every diagnostic carrying one of the seven\n\
        ctor-conformance codes becomes one row. The `file:line` comes from the\n\
        diagnostic's own label span; `expected`/`found` come from the label message.\n\
        Nothing below was typed in by hand, and a column that could not be recovered\n\
        renders as `—` rather than as a guess.\n\
        \n\
        Three things to know before reading a row:\n\
        \n\
        - **`line` is the CTOR CALL-SITE line, not the offending argument's line.** α\n\
          anchors the label at the `Foo(...)` call's own span (PRD §10 Q1;\n\
          `compile_builder/entities_phase.rs`), so a multi-line ctor reports the line of\n\
          its opening `Foo(`. The offending argument is named in the `field` column and\n\
          sits within that call — e.g.\n\
          `examples/trajectory/printer_print_envelope.ri:169` is the `TOTSShaper(` line,\n\
          while `velocity_limit: 300.0` is three lines further down.\n\
        - **`def` is whatever identifier sits at that anchor, and `def source` says where\n\
          it came from.** The recovery reads an identifier followed by `(` — which cannot\n\
          by itself tell a `structure def` ctor from a plain function call. A few rows\n\
          carry codes that reach this survey from a NON-ctor path (selector composition,\n\
          overload resolution), where that identifier is a *function* name. Those are not\n\
          left in the actionable group: a recovered name is cross-checked against every\n\
          `structure def` declared in the corpus and the stdlib, and a name that does not\n\
          resolve is filed under *name recovered, but it is not a known structure def*.\n\
        - **A `—` in `def` is explained, not asserted.** The `def source` column carries\n\
          the machine-derived reason recovery failed for that specific row (span starts at\n\
          a non-identifier, identifier not followed by `(`, span out of range, …), so no\n\
          prose here has to guess a cause on a reader's behalf.\n\
        \n\
        The **`disposition` column is γ's RULING**, projected from the site's measured\n\
        severity and wording and the three per-site tables (`CTOR_CONFORMANCE_CORPUS_RESIDUAL`\n\
        and `CTOR_CONFORMANCE_REJECTION_FIXTURES` in the survey's `disposition` module,\n\
        `CTOR_CONFORMANCE_MIGRATION_DEBT` in `reify_test_support::ctor_conformance_debt`)\n\
        rather than typed here. It has five states, and they call for five DIFFERENT\n\
        actions:\n\
        \n\
        - **`deferred`** names the LIVE task that owns retiring the site, and the reason\n\
        migrating it here would destroy something — most of these are committed RED\n\
        before-images whose violation IS the fixture's content. Leave them alone.\n\
        - **`n/a`** carries a ctor-conformance CODE but names no ctor ARGUMENT, so the\n\
        conformance knob is not what emitted it and it is outside the\n\
        zero-ctor-conformance-sites signal entirely. Every such row today is a\n\
        deliberate REJECTION fixture reached from a NON-ctor path (selector\n\
        composition, overload resolution, trait conformance): the rejection IS the\n\
        behaviour under test. **Not actionable, and not residual either** — it carries no\n\
        owner because it needs none, and reading it as unclaimed work would send you to\n\
        delete another PRD’s signal.\n\
        - **`intended rejection`** is an in-scope site whose violation IS the\n\
        deliverable: a committed PRD §7 boundary-row fixture that `reify check` is\n\
        asserted to REJECT by a probe in\n\
        `tests/prd-gate/struct-ctor-conformance-probe-set.json`. It differs from `n/a` on\n\
        scope — the knob's ctor-argument walk really is what emitted it — and from\n\
        `deferred` on ownership: no task retires it, so it names none. **Leave it alone**;\n\
        migrating the site deletes δ's own signal and reds that CLI gate.\n\
        - **`unattributed`** is an in-scope site claimed by nobody: that is the\n\
        actionable state, and after γ the tracked `.ri` corpus holds none.\n\
        - **`census`** is every row from the **inline** half — a Reify snippet embedded\n\
        in a Rust test fixture — and its host test owns the verdict. δ (#5306) fixed the\n\
        inline sites its severity flip exposed and kept the rest as deliberate Error\n\
        pins, so a surviving row is one its host test asserts, tolerates, or never\n\
        compiles. Rows are listed so the class stays countable and cannot recur\n\
        unnoticed on the next severity change. **Do not read a census row as unclaimed\n\
        work, and do not read it as waived either** — no waiver table names it, because\n\
        the tables key on `.ri` files.\n\
        \n\
        The **`hint` column is ADVISORY**, derived purely from the (expected, found)\n\
        type pair. It is **not** a D9 ruling. PRD §4 D9 defines the split between class\n\
        (1) *call-site bug* and class (2) *wrong declared field type* as \"per-case\n\
        judgment … whichever is the actual bug\" and assigns it to **γ**. γ has now\n\
        ruled, and the ruling is the `disposition` column beside the hint: where the two\n\
        disagree, the disposition wins. What β decided mechanically is the `owner`\n\
        grouping below.\n\
        \n\
        ## Format (PRD §10 Q6)\n\
        \n\
        **Q6 is answered here — grouped by D9 owner class, with a flat\n\
        `(file, line, field)`-sorted table inside each group.** Grouping first by owner\n\
        makes the FEA do-not-touch partition unmissable for γ, whose actual consumption\n\
        question is *which sites may I touch*; a flat table inside each group keeps the\n\
        result directly sizable and sortable. Recorded in this artifact header rather\n\
        than by editing the PRD's §10, which the sibling α/γ/δ/ζ tasks concurrently read.\n\
        \n",
    );

    // ── site groups, FEA first ──────────────────────────────────────────────
    md.push_str("## Sites\n\n");
    if site_count == 0 {
        md.push_str(
            "**No ctor-conformance sites were found in the surveyed corpus.** This is an\n\
             explicit zero, not a truncated run — see the coverage section below for what\n\
             was and was not surveyed.\n\n",
        );
    }
    for owner in Owner::render_order() {
        let mut group: Vec<&SurveySite> = run.sites.iter().filter(|s| s.owner == owner).collect();
        group.sort_by(|a, b| (&a.file, a.line, &a.field).cmp(&(&b.file, b.line, &b.field)));

        let _ = writeln!(md, "### {} — {} site(s)\n", owner.title(), group.len());
        match owner {
            Owner::FeaDeferredToV06 => md.push_str(
                "Per PRD §4 D9, these defs are declared in the FEA stdlib modules: γ may make\n\
                 **call-site changes ONLY**. Field-type flips remain v0.6-owned\n\
                 (`docs/prds/v0_6/fea-load-support-selector-migration.md`). **DO NOT FIX the\n\
                 declared field types here.**\n\n",
            ),
            Owner::NonFea => md.push_str(
                "The recovered name IS a `structure def` declared in the corpus or the stdlib,\n\
                 and it is not FEA-owned. D9's per-case judgment applied here, and **γ has\n\
                 now ruled every in-scope row in this group** — read the `disposition` column.\n\
                 A site γ judged a call-site bug was fixed and is simply absent below; a site γ\n\
                 deferred names the LIVE task that owns retiring it.\n\n\
                 That check is against ONE GLOBAL namespace — *some* corpus or stdlib file\n\
                 declares the name, not necessarily one this row's file can see. See named\n\
                 limitation 3 below before treating a row here as actionable.\n\n",
            ),
            Owner::UnresolvedDef => md.push_str(
                "An identifier was recovered at the diagnostic's anchor, but it is not a\n\
                 `structure def` declared anywhere in the corpus or the stdlib. Recovery reads\n\
                 an identifier followed by `(`, which cannot distinguish a ctor from a plain\n\
                 function call, so these are typically FUNCTION names reaching the survey from\n\
                 a non-ctor path (selector composition, overload resolution) — the `severity`\n\
                 and `message` columns show which. Held out of the actionable group rather than\n\
                 sized into it. **Triage manually before touching.**\n\n",
            ),
            Owner::Unknown => md.push_str(
                "No def name could be attributed. The `def source` column gives the\n\
                 machine-derived reason PER ROW rather than asserting one cause for the group:\n\
                 known shapes that land here include the sub `=` per-arg anchor and param\n\
                 default-initializer checks, both of which anchor the label somewhere other\n\
                 than a `Def(` call site. Deliberately its own group: folding an\n\
                 unattributable site into the touchable pile is the one classification error\n\
                 with a real cost. **Triage manually before touching.**\n\n",
            ),
        }
        if group.is_empty() {
            md.push_str("_(none)_\n\n");
            continue;
        }
        push_site_table(&mut md, CorpusHalf::TrackedRi, &group);
        md.push('\n');
    }

    push_inline_section(&mut md, inline);

    // ── coverage + limitations ──────────────────────────────────────────────
    md.push_str("## Coverage and limitations\n\n");
    let _ = writeln!(
        md,
        "Of {} tracked `.ri` members, **{} were surveyed** and **{} were not**. \
         A further **{}** were surveyed only PARTIALLY. Both are listed below rather \
         than dropped: a bounded sweep that does not state what it skipped reads as \
         full coverage and would under-size γ.\n",
        run.total,
        run.surveyed,
        run.not_surveyed.len(),
        run.partial.len()
    );

    md.push_str("### Not surveyed (contributed no sites)\n\n");
    push_coverage_table(
        &mut md,
        &run.not_surveyed,
        "_(none — every tracked member reached the compile phase)_\n\n",
    );

    md.push_str(
        "### Partially surveyed (sites collected, but the file also failed to compile)\n\n",
    );
    push_coverage_table(&mut md, &run.partial, "_(none)_\n\n");

    md.push_str("### Named limitations\n\n");
    push_inline_limitation(&mut md, inline);
    md.push_str(
        "2. **`compile_with_stdlib` is the SINGLE-FILE path.** `reify check` instead uses\n\
           `module_dag::compile_entry_with_stdlib_cfg_checked`, which follows `#cfg`-gated\n\
           user imports and runs `SimpleConstraintChecker`. Multi-module corpus members\n\
           (the `examples/module_visibility/consumer.ri` class) therefore cannot resolve\n\
           standalone and appear above under *not surveyed* or *partially surveyed* with\n\
           their reason, rather than being silently dropped.\n\
        3. **`def` resolution uses ONE GLOBAL namespace, not per-file module scope.** The\n\
           `owner` grouping cross-checks a recovered name against every `structure def`\n\
           declared anywhere in the corpus plus the stdlib — it does NOT ask whether that\n\
           declaration is visible from the file the row sits in. Two consequences, and\n\
           only one of them is safe:\n\
           **(a)** a row lands in *non-FEA — γ per-case judgment* whenever SOME corpus\n\
           file declares that name, even if the row's own file cannot see it. That is\n\
           over-inclusion in the TOUCHABLE direction, so **before treating a `non-FEA` row\n\
           as actionable, confirm the `def` is declared in that row's own file or one of\n\
           its imports.** The `message` and `severity` columns usually settle it in one\n\
           read.\n\
           **(b)** symmetrically, a non-FEA file that happened to declare a name the FEA\n\
           stdlib also declares would pull its sites into the do-not-touch partition. That\n\
           direction over-defers rather than over-touches, so it costs γ sizing accuracy,\n\
           never a wrong edit.\n\
           Per-member scoping (each file's own declarations plus its imports) would remove\n\
           the approximation, at the cost of resolving the import graph for every member —\n\
           more machinery than a one-shot snapshot warrants, so the approximation is stated\n\
           here instead of hidden.\n\
        \n",
    );

    // ── regeneration ────────────────────────────────────────────────────────
    md.push_str("## How to regenerate\n\n```bash\n");
    let _ = writeln!(md, "{REGEN_COMMAND}");
    md.push_str("```\n\n");
    md.push_str(
        "The generator is `#[ignore]`d: it compiles the whole tracked corpus, which is\n\
        ~2.5× the `examples/` walk already documented as the most expensive thing that\n\
        test binary does, and paying that on every merge gate would fight\n\
        `docs/prds/merge-gate-compile-cost.md`. Everything the generator *decides* —\n\
        enumeration, span→line, def/field/type extraction, D9 classification and this\n\
        rendering — is unit-tested on every gate run against synthetic inputs, plus one\n\
        cheap three-file end-to-end sweep, so the pipeline cannot bit-rot between runs.\n\
        \n\
        Set `REIFY_CTOR_SURVEY_OUT` to write elsewhere (e.g. to diff a fresh run against\n\
        the committed copy without dirtying the tree).\n\
        \n\
        > The `env` prefix on the command above bypasses reify's PreToolUse hook, which\n\
        > condenses `cargo test` output. It is harmless here — the generator writes the\n\
        > file rather than being scraped from stdout — but without it a reader of the run\n\
        > log sees only a `PASS: N | FAIL: M` summary and may think the sweep did nothing.\n",
    );

    md
}

#[test]
fn render_survey_states_a_site_count_that_equals_the_rendered_rows() {
    let run = SurveyRun {
        total: 660,
        surveyed: 640,
        not_surveyed: vec![("x.ri".to_owned(), "parse-error".to_owned())],
        partial: vec![("y.ri".to_owned(), "compile-error".to_owned())],
        sites: vec![
            synth_site("a.ri", 3, "PointLoad", "point", Owner::FeaDeferredToV06),
            synth_site("b.ri", 7, "Widget", "label", Owner::NonFea),
        ],
    };
    let md = render_survey(&run, &SurveyRun::default(), &SurveyStamp::at("deadbeef"));

    // The stated count is COMPUTED, never typed — that is the task's
    // "site count stated" signal, and it must equal the rows actually drawn.
    let rows = rendered_site_rows(&md);
    assert_eq!(rows, 2, "two sites must draw two table rows, got:\n{md}");
    assert!(
        md.contains("**Sites:** 2"),
        "the header must state the site count; got:\n{md}"
    );
    assert!(md.contains("deadbeef"), "the base commit must be stamped");
    assert!(md.contains("660"), "the live corpus total must be stated");
    assert!(
        md.contains("640"),
        "the surveyed count must be stated so the denominator is visible"
    );

    // …and the same identity must hold for a run carrying one site of EVERY
    // owner class, which is the case the two-site run above cannot see. If the
    // renderer ever stops emitting a group, the stated `**Sites:** N` keeps
    // counting those sites while the table stops drawing them — a class of sites
    // silently vanishing from the artifact behind an unchanged count. Built from
    // `Owner::render_order()` so a newly-added variant is covered the moment it
    // is declared, with no edit here.
    let every_class: Vec<SurveySite> = Owner::render_order()
        .into_iter()
        .enumerate()
        .map(|(i, owner)| synth_site(&format!("f{i}.ri"), 1, "W", "field", owner))
        .collect();
    let n = every_class.len();
    let run = SurveyRun {
        total: n,
        surveyed: n,
        not_surveyed: vec![],
        partial: vec![],
        sites: every_class,
    };
    let md = render_survey(&run, &SurveyRun::default(), &SurveyStamp::at("deadbeef"));
    assert_eq!(
        rendered_site_rows(&md),
        run.sites.len(),
        "every site handed in must be DRAWN, not merely counted — one owner class \
         per site, {n} sites:\n{md}"
    );
    assert!(
        md.contains(&format!("**Sites:** {n}")),
        "the stated count must equal the rows drawn; got:\n{md}"
    );
}

/// Count the site rows actually drawn in a rendered artifact.
///
/// Site rows are the only table rows whose first cell is a `` `<file>.ri:<line>` ``
/// anchor, so this cannot pick up the coverage tables.
#[cfg(test)]
fn rendered_site_rows(md: &str) -> usize {
    md.lines()
        .filter(|l| l.starts_with("| `") && l.contains(".ri:"))
        .count()
}

#[test]
fn render_survey_groups_by_d9_owner_with_fea_first_and_marked_do_not_fix() {
    let mut unresolved = synth_site("u.ri", 1, "union", "arg", Owner::UnresolvedDef);
    unresolved.def_origin = DefOrigin::CallSiteAnchor;
    let mut unknown = synth_site("m.ri", 1, "Mystery", "f", Owner::Unknown);
    unknown.def = None;
    unknown.def_origin = DefOrigin::SpanNotIdentifier;

    let run = SurveyRun {
        total: 4,
        surveyed: 4,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![
            synth_site("z.ri", 1, "Widget", "label", Owner::NonFea),
            synth_site("a.ri", 1, "PointLoad", "point", Owner::FeaDeferredToV06),
            unknown,
            unresolved,
        ],
    };
    let md = render_survey(&run, &SurveyRun::default(), &SurveyStamp::at("cafe1234"));

    // Anchor every ordering probe on the rendered `### <title>` heading, never on
    // a bare substring: the artifact's own `## Format` prose mentions "FEA"
    // above `## Sites`, so `md.find("FEA")` would return a fixed header offset
    // that precedes EVERY group and make this ordering assertion vacuously
    // green — including when the FEA group is emitted last or dropped entirely.
    let heading_at = |owner: Owner| {
        md.find(&format!("### {}", owner.title()))
            .unwrap_or_else(|| panic!("missing group heading for {owner:?}:\n{md}"))
    };
    let fea_at = heading_at(Owner::FeaDeferredToV06);
    let non_fea_at = heading_at(Owner::NonFea);
    assert!(
        fea_at < non_fea_at,
        "the do-not-touch FEA partition must come FIRST — γ's first question is \
         which sites it may touch:\n{md}"
    );
    assert!(
        md.contains("DO NOT FIX"),
        "the FEA group must be explicitly labelled do-not-fix:\n{md}"
    );
    assert!(
        md.contains("unattributed"),
        "the Unknown bucket must be rendered as its own group, not folded away"
    );

    // EVERY owner class must render its own group. A class the renderer forgets
    // is a class of sites that silently vanishes from the artifact — the
    // survey's one unacceptable failure.
    //
    // The list is `Owner::render_order()`, i.e. DERIVED from the enum via
    // `strum::EnumIter`, never a hand-written copy: a copy here would have to be
    // kept in step with the renderer's own copy, and two literals that must
    // agree is precisely the drift this test claims to catch. Because the run
    // above is also built from that derived list, adding a fifth variant makes
    // this test demand a fifth group rather than quietly ignoring it.
    for owner in Owner::render_order() {
        let heading = format!("### {} — 1 site(s)", owner.title());
        assert!(
            md.contains(&heading),
            "every owner class must render its own group with its own count; missing \
             {heading:?}:\n{md}"
        );
    }

    // The two non-actionable triage buckets must sort AFTER the actionable one,
    // so γ reads its own work first and does not mistake a function-call row for
    // a ctor site it owns.
    let unresolved_at = heading_at(Owner::UnresolvedDef);
    assert!(
        non_fea_at < unresolved_at,
        "the actionable non-FEA group must precede the manual-triage buckets:\n{md}"
    );

    // …and the recovered-but-unknown name is NOT sized into γ's actionable pile.
    let non_fea_block = &md[non_fea_at..unresolved_at];
    assert!(
        !non_fea_block.contains("`u.ri:1`"),
        "`union` is a function name, not a structure def — it must not appear in the \
         actionable non-FEA group:\n{non_fea_block}"
    );
}

#[test]
fn render_survey_orders_rows_deterministically_within_a_group() {
    let ordered = [
        synth_site("a.ri", 2, "W", "alpha", Owner::NonFea),
        synth_site("a.ri", 9, "W", "beta", Owner::NonFea),
        synth_site("b.ri", 1, "W", "gamma", Owner::NonFea),
    ];
    let mut shuffled = vec![ordered[2].clone(), ordered[0].clone(), ordered[1].clone()];
    // A same-(file,line) pair discriminated only by field must still sort.
    shuffled.push(synth_site("a.ri", 2, "W", "aardvark", Owner::NonFea));

    let mk = |sites: Vec<SurveySite>| SurveyRun {
        total: sites.len(),
        surveyed: sites.len(),
        not_surveyed: vec![],
        partial: vec![],
        sites,
    };
    let mut sorted = shuffled.clone();
    sorted.sort_by(|a, b| (&a.file, a.line, &a.field).cmp(&(&b.file, b.line, &b.field)));

    assert_eq!(
        render_survey(
            &mk(shuffled),
            &SurveyRun::default(),
            &SurveyStamp::at("sha")
        ),
        render_survey(&mk(sorted), &SurveyRun::default(), &SurveyStamp::at("sha")),
        "rows must render in (file, line, field) order regardless of input order"
    );
}

#[test]
fn render_survey_escapes_pipes_and_newlines_so_a_message_cannot_break_the_table() {
    let mut site = synth_site("a.ri", 1, "W", "f", Owner::NonFea);
    site.message = "a | b\nsecond line | c".to_owned();
    site.field = Some("has|pipe".to_owned());
    let run = SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![site],
    };
    let md = render_survey(&run, &SurveyRun::default(), &SurveyStamp::at("sha"));

    let row = md
        .lines()
        .find(|l| l.starts_with("| `a.ri:1`"))
        .expect("the site row");
    assert!(
        !row.contains("a | b"),
        "a raw pipe inside a message would split the cell: {row:?}"
    );
    assert!(
        row.contains("second line"),
        "the escaped message must still carry its full text: {row:?}"
    );
    assert!(
        !md.contains("has|pipe"),
        "a pipe inside ANY cell must be escaped, not just the message"
    );
}

#[test]
fn render_survey_writes_an_em_dash_for_every_unrecoverable_cell() {
    let site = SurveySite {
        file: "a.ri".to_owned(),
        line: 1,
        def: None,
        def_origin: DefOrigin::SpanNotIdentifier,
        field: None,
        expected: None,
        found: None,
        code: "CtorArity".to_owned(),
        severity: "Error".to_owned(),
        message: "E_CTOR_ARITY: Bar() expects at most 1 argument, got 2".to_owned(),
        owner: Owner::Unknown,
        snippet_line: None,
    };
    let run = SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![site],
    };
    let md = render_survey(&run, &SurveyRun::default(), &SurveyStamp::at("sha"));
    let row = md
        .lines()
        .find(|l| l.starts_with("| `a.ri:1`"))
        .expect("the site row");
    // Check the four cells BY POSITION rather than counting em-dashes in the
    // whole row: the neutral hint string legitimately contains one too, so a
    // raw count is an imprecise proxy for what this test actually means.
    let cells: Vec<&str> = row.split('|').map(str::trim).collect();
    for (idx, name) in [(2, "def"), (4, "field"), (5, "expected"), (6, "found")] {
        assert_eq!(
            cells[idx], "—",
            "the {name} cell must render as an em-dash, never an empty or invented \
             cell: {row:?}"
        );
    }
    // …but the `def source` cell is NEVER an em-dash: an unrecovered def has a
    // machine-derived REASON, which is what stops the artifact from asserting a
    // cause for the whole group in prose.
    assert_eq!(
        cells[3],
        DefOrigin::SpanNotIdentifier.label(),
        "an unrecovered def must carry its machine-derived recovery-failure reason, \
         not a second em-dash: {row:?}"
    );
    assert!(
        !row.contains("||"),
        "no cell may be rendered empty: {row:?}"
    );
    assert!(
        cells[11].starts_with("E_CTOR_ARITY:"),
        "the raw message must still be carried verbatim: {row:?}"
    );
}

#[test]
fn render_survey_renders_the_zero_site_case_explicitly() {
    let run = SurveyRun {
        total: 660,
        surveyed: 660,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![],
    };
    let md = render_survey(&run, &SurveyRun::default(), &SurveyStamp::at("sha"));
    assert!(
        md.contains("**Sites:** 0"),
        "the count must still be stated"
    );
    assert!(
        md.to_lowercase().contains("no ctor-conformance"),
        "a zero-site outcome must be rendered as an explicit statement, never an \
         empty table a reader could mistake for a truncated run:\n{md}"
    );
}

#[test]
fn render_survey_reports_the_recovery_reason_instead_of_asserting_a_cause() {
    // The whole point of the `def source` column: an earlier Unknown-group blurb
    // asserted "these sites come through the sub `=` per-arg anchor", which was
    // false for BOTH rows actually in the committed artifact (they are param
    // default initializers). The renderer must therefore report per row what the
    // code measured, rather than naming one cause as THE cause for the group.
    //
    // That property is pinned BEHAVIOURALLY below (the row's own
    // `DefOrigin` label and the `def source` column must both reach the
    // artifact), never by a negative pin on the group blurb's wording. An
    // earlier draft carried `!md.contains("these sites come\nthrough the sub
    // `=` per-arg anchor")`, which embedded a newline at a position the
    // renderer never wraps at: it was vacuously true, would have stayed true
    // under any rewording, and pinned nothing. Do not reintroduce it — a
    // wording pin on generated prose can only rot.
    let mut site = synth_site("a.ri", 1, "W", "f", Owner::Unknown);
    site.def = None;
    site.def_origin = DefOrigin::SpanNotIdentifier;
    let run = SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![site],
    };
    let md = render_survey(&run, &SurveyRun::default(), &SurveyStamp::at("sha"));

    assert!(
        md.contains(DefOrigin::SpanNotIdentifier.label()),
        "the row's machine-derived recovery-failure reason must appear in the \
         artifact:\n{md}"
    );
    assert!(
        md.contains("| def source |"),
        "the `def source` column must be part of the table header:\n{md}"
    );

    // And a RECOVERED def reports where it came from, not a blank.
    let recovered = SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![synth_site("b.ri", 2, "Widget", "label", Owner::NonFea)],
    };
    let md = render_survey(&recovered, &SurveyRun::default(), &SurveyStamp::at("sha"));
    assert!(
        md.contains(DefOrigin::CallSiteAnchor.label()),
        "a recovered def must still say HOW it was recovered:\n{md}"
    );
}

#[test]
fn render_survey_names_every_drifted_ri_without_disturbing_the_anchor() {
    // The disclosure's whole job: a reader must be able to see WHICH files the
    // anchor no longer describes, by name, without running git. Machine-derived
    // from the same `git diff --name-only` read the header stamps — no path
    // here is hand-typed, the same rule every other row in this artifact lives
    // under.
    let run = one_site_run();
    let drifted = SurveyStamp {
        anchor: "cafe1234".to_owned(),
        drifted: vec![
            "tests/prd-gate/fixtures/one.ri".to_owned(), // pg-drift:allow — synthetic drift path; no such fixture exists and nothing compiled reads one.ri
            "tree-sitter-reify/test/fixtures/two.ri".to_owned(),
        ],
    };
    let md = render_survey(&run, &SurveyRun::default(), &drifted);

    assert!(
        md.contains(DRIFT_DISCLOSURE_KEY),
        "a drifted stamp must disclose; got:\n{md}"
    );
    for path in &drifted.drifted {
        assert!(
            md.contains(path.as_str()),
            "the disclosure must name {path} — a path it drops is a path no \
             reader can know about; got:\n{md}"
        );
    }
    assert!(
        md.contains(&format!("{} tracked", drifted.drifted.len())),
        "the stated count must be COMPUTED from the disclosed list, never \
         typed; got:\n{md}"
    );
    // Drift discloses; it never re-anchors. Read through the same parser the
    // gate-resident ancestry guard uses, so this pins the line that guard reads.
    assert_eq!(
        parse_stamped_base_commit(&md),
        Some("cafe1234"),
        "the merge base stays THE stamped anchor — a rewritable branch tip must \
         never be promoted into that line; got:\n{md}"
    );
}

#[test]
fn render_survey_omits_the_disclosure_entirely_when_nothing_drifted() {
    // The undrifted run is the common one, and it must render EXACTLY what it
    // rendered before the disclosure existed: no "0 files drifted" noise row,
    // so two runs generated on `main` stay byte-comparable with each other.
    let run = one_site_run();
    let without = render_survey(&run, &SurveyRun::default(), &SurveyStamp::at("cafe1234"));
    assert!(
        !without.contains(DRIFT_DISCLOSURE_KEY),
        "an undrifted stamp must render no disclosure at all; got:\n{without}"
    );

    // …and the disclosure is purely ADDITIVE: it is inserted, and changes not
    // one byte above or below itself. Asserted as prefix/suffix identity rather
    // than by eyeballing the two renderings.
    let with = render_survey(
        &run,
        &SurveyRun::default(),
        &SurveyStamp {
            anchor: "cafe1234".to_owned(),
            drifted: vec!["tests/prd-gate/fixtures/one.ri".to_owned()], // pg-drift:allow — same synthetic path as above
        },
    );
    let at = with
        .find(DRIFT_DISCLOSURE_KEY)
        .expect("the drifted rendering must carry the disclosure key");
    assert_eq!(
        &with[..at],
        &without[..at],
        "everything ABOVE the disclosure must be byte-identical either way"
    );
    assert!(
        with.ends_with(&without[at..]),
        "everything BELOW the disclosure must be byte-identical either way"
    );
}

/// A one-site run: the smallest thing that renders every section of the
/// artifact, for tests whose subject is the header rather than the rows.
#[cfg(test)]
fn one_site_run() -> SurveyRun {
    SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![synth_site("a.ri", 3, "Widget", "label", Owner::NonFea)],
    }
}

#[test]
fn render_survey_carries_the_regeneration_command_and_the_coverage_section() {
    let run = SurveyRun {
        total: 4,
        surveyed: 2,
        not_surveyed: vec![
            ("bad.ri".to_owned(), "parse-error".to_owned()),
            ("gone.ri".to_owned(), "read-error".to_owned()),
        ],
        partial: vec![("multi.ri".to_owned(), "compile-error".to_owned())],
        sites: vec![synth_site("a.ri", 1, "W", "f", Owner::NonFea)],
    };
    let md = render_survey(&run, &SurveyRun::default(), &SurveyStamp::at("sha"));

    assert!(
        md.contains("## How to regenerate"),
        "house convention for a generated artifact (cf. \
         docs/architecture-audit/g-tool-baseline-report.md)"
    );
    assert!(
        md.contains(REGEN_COMMAND),
        "the EXACT regeneration command must appear verbatim:\n{md}"
    );

    // Coverage: every not-surveyed and partial member, with its reason.
    for (name, reason) in [
        ("bad.ri", "parse-error"),
        ("gone.ri", "read-error"),
        ("multi.ri", "compile-error"),
    ] {
        assert!(
            md.contains(name) && md.contains(reason),
            "the coverage section must list {name} with reason {reason}:\n{md}"
        );
    }
}

/// The header label of the disposition column, and the one place the tests read
/// it from — so a row's disposition is located BY COLUMN NAME rather than by a
/// hard-coded index that silently shifts when a column is inserted.
#[cfg(test)]
const DISPOSITION_COLUMN: &str = "disposition (γ ruling)";

/// The heading that opens the INLINE half's section of the artifact.
///
/// One spelling, read by the renderer and by every test that locates the
/// section — including the ones asserting a row is NOT in the `.ri` half, which
/// is the stronger claim and the one a drifted spelling would silently pass.
const INLINE_SECTION_HEADING: &str = "## Inline Rust fixtures";

/// The header label of the inline table's snippet-relative coordinate column.
const SNIPPET_LINE_COLUMN: &str = "snippet line";

/// The bold lead of named limitation 1, which states what the inline walker
/// reaches and — the part that matters — what it does not.
///
/// One spelling, read by the renderer and by every test that locates the
/// limitation. Locating it by key rather than by ordinal means inserting a
/// limitation above it cannot silently re-point the assertions at neighbouring
/// prose.
const INLINE_LIMITATION_KEY: &str = "**Inline Reify snippets are reached as RAW-STRING LITERALS";

/// The disposition cell of the site row anchored at `row_anchor`.
#[cfg(test)]
fn disposition_cell(md: &str, row_anchor: &str) -> String {
    let header = md
        .lines()
        .find(|l| l.starts_with("| site |"))
        .unwrap_or_else(|| panic!("the site table header must be rendered:\n{md}"));
    cell_by_column(md, header, DISPOSITION_COLUMN, row_anchor)
}

/// The cell of `row_anchor`'s row lying under `column` of `header`.
///
/// Located BY COLUMN NAME, never by a hard-coded index: a column inserted to the
/// left of the one under test would otherwise silently shift every assertion
/// onto a neighbour and keep passing.
#[cfg(test)]
fn cell_by_column(md: &str, header: &str, column: &str, row_anchor: &str) -> String {
    let idx = header
        .split('|')
        .map(str::trim)
        .position(|c| c == column)
        .unwrap_or_else(|| panic!("the table header must carry a `{column}` column:\n{header}"));
    let row = md
        .lines()
        .find(|l| l.starts_with(row_anchor))
        .unwrap_or_else(|| panic!("no row anchored at {row_anchor:?}:\n{md}"));
    row.split('|')
        .map(str::trim)
        .nth(idx)
        .unwrap_or_else(|| panic!("row {row:?} has no cell at the `{column}` index {idx}"))
        .to_owned()
}

/// Every site row carries a disposition RESOLVED FROM THE TABLES.
///
/// PRD §4 D9 requires γ to record each of its choices in the survey artifact.
/// Recording them as hand-written prose would rot the moment a table entry moves
/// or an owning task lands, so the artifact projects the tables instead: the
/// tables are the single source, and this column is a view of them.
///
/// Driven entirely by SYNTHETIC sites, like every other renderer test here — no
/// corpus compile, and no assertion on the committed artifact's own text, which
/// would be a documentation meta-test.
///
/// # Degrades to the table-free rows when a table drains
///
/// The two table-keyed rows borrow the FIRST live entry of each table, so no key
/// is copied here and an individual retirement cannot stale this test. Draining a
/// table EMPTY is the terminal success state of this whole effort, though — the
/// last engineer to delete a residual row must not be greeted by a red gate for
/// having finished the work — so each table-keyed half is skipped with a printed
/// reason, exactly as the `git_is_available` probes in this module skip. The rows
/// that depend on NO table entry (unattributed, unkeyable, `n/a`) always run, so
/// the renderer's projection keeps real coverage after both tables are gone.
#[test]
fn render_survey_resolves_each_site_disposition_from_the_tables() {
    let mut unkeyable = synth_site("no_param.ri", 4, "Widget", "ignored", Owner::Unknown);
    unkeyable.field = None;
    // A ctor-conformance CODE that the conformance knob did not emit: a
    // deliberate rejection fixture, owned by nobody. Synthetic rather than
    // borrowed from the corpus, and deliberately NOT in either table, so it
    // proves the SCOPE test alone decides the `n/a` state.
    //
    // The message is the wording γ measured at `bt6_kind_typed_param.ri:24` and
    // committed to the artifact, not `synth_site`'s ctor-argument wording: after
    // δ the severity is no longer what separates this row from a knob site, so a
    // fixture carrying `argument '…'` prose would no longer be modelling a
    // rejection fixture at all.
    let mut rejection = synth_site(
        "rejection_fixture.ri",
        5,
        "needs_face",
        "faces",
        Owner::NonFea,
    );
    rejection.code = "SelectorKindMismatch".to_owned();
    rejection.message = "no matching overload for needs_face(EdgeSelector), \
         candidates: needs_face(FaceSelector) -> Int"
        .to_owned();

    let mut sites = vec![
        synth_site("not_in_any_table.ri", 3, "Widget", "label", Owner::NonFea),
        unkeyable,
        rejection,
    ];

    let residual = CTOR_CONFORMANCE_CORPUS_RESIDUAL.first();
    let debt = reify_test_support::ctor_conformance_debt::CTOR_CONFORMANCE_MIGRATION_DEBT.first();
    let debt_path = debt.map(|(key, _, _)| format!("{EXAMPLES_PREFIX}{key}"));
    if let Some((path, param, _, _)) = residual {
        sites.push(synth_site(path, 1, "Widget", param, Owner::NonFea));
    }
    if let (Some((_, param, _)), Some(path)) = (debt, debt_path.as_deref()) {
        sites.push(synth_site(path, 2, "TOTSShaper", param, Owner::NonFea));
    }
    let n = sites.len();
    let md = render_survey(
        &SurveyRun {
            total: n,
            surveyed: n,
            not_surveyed: vec![],
            partial: vec![],
            sites,
        },
        &SurveyRun::default(),
        &SurveyStamp::at("sha"),
    );

    for (anchor, what) in [
        ("| `not_in_any_table.ri:3`", "a site named by neither table"),
        (
            "| `no_param.ri:4`",
            "a site whose param could not be recovered, so no table can key it",
        ),
    ] {
        let unattributed = disposition_cell(&md, anchor);
        assert!(
            unattributed.contains("unattributed"),
            "{what} must render as unattributed — the conservative default. Reading \
             as deferred would attribute an owner nobody assigned; got \
             {unattributed:?}"
        );
        assert!(
            !unattributed.contains('#'),
            "{what} must name no owner at all; got {unattributed:?}"
        );
    }

    // The third state, and the one the `why` column exists to protect: an
    // out-of-scope row must NOT read as unclaimed work.
    let rejection_cell = disposition_cell(&md, "| `rejection_fixture.ri:5`");
    assert!(
        rejection_cell.starts_with("n/a"),
        "a ctor-conformance-coded site that names no ctor argument must render as \
         `n/a` — the conformance knob is not what emitted it and nobody owns \
         retiring it; got {rejection_cell:?}"
    );
    assert!(
        !rejection_cell.contains("unattributed") && !rejection_cell.contains('#'),
        "an `n/a` row must neither read as actionable nor name an owner: it is a \
         deliberate rejection fixture, so both would send a reader to delete another \
         PRD's signal; got {rejection_cell:?}"
    );

    // A deferred cell must be a visible deferral, not merely a bare cite.
    if let Some((residual_path, _, residual_owner, residual_why)) = residual {
        let residual_cell = disposition_cell(&md, &format!("| `{residual_path}:1`"));
        assert!(
            residual_cell.contains("deferred")
                && residual_cell.contains(&format!("owned by {residual_owner}")),
            "a deferred site must SAY it is deferred and who owns it, so the cell \
             reads on its own; got {residual_cell:?}"
        );
        assert!(
            residual_cell.contains(&cell(residual_why)),
            "a CTOR_CONFORMANCE_CORPUS_RESIDUAL site must render its recorded reason \
             beside the owner, so a reader meets the deferral and its justification in \
             the same cell; why {residual_why:?}, got {residual_cell:?}"
        );
    } else {
        println!(
            "skipped: CTOR_CONFORMANCE_CORPUS_RESIDUAL is empty — every owner has \
             landed, so there is no residual row to project"
        );
    }

    if let (Some((_, _, debt_owner)), Some(debt_path)) = (debt, debt_path.as_deref()) {
        let debt_cell = disposition_cell(&md, &format!("| `{debt_path}:2`"));
        assert!(
            debt_cell.contains("deferred") && debt_cell.contains(&format!("owned by {debt_owner}")),
            "a CTOR_CONFORMANCE_MIGRATION_DEBT site must render THAT table's owner as a \
             visible deferral. The debt list is keyed relative to `examples/` and this \
             row is repo-relative, so a miss here means the two key forms stopped being \
             bridged and every examples/ site silently reads as unattributed; owner \
             {debt_owner}, got {debt_cell:?}"
        );
    } else {
        println!(
            "skipped: CTOR_CONFORMANCE_MIGRATION_DEBT is empty — every owner has \
             landed, so there is no examples/-keyed row to project"
        );
    }
}

// ─── step 13/14: output path + the generator entry point ─────────────────────

/// Env var that redirects the generator's output to a scratch path.
const OUT_ENV: &str = "REIFY_CTOR_SURVEY_OUT";

/// The committed artifact's repo-relative path.
const ARTIFACT_REL: &str = "docs/prds/struct-ctor-field-type-conformance.survey.md";

/// Resolve the output path from an already-read override value.
///
/// A pure seam so the override can be tested without setting a process-global
/// env var, which would race every other test in this binary. An empty override
/// falls back to the default: an accidental `REIFY_CTOR_SURVEY_OUT=` must not
/// drop the artifact into the current working directory.
fn survey_output_path_for(override_value: Option<String>) -> PathBuf {
    match override_value {
        Some(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => PathBuf::from(WORKSPACE_ROOT).join(ARTIFACT_REL),
    }
}

/// Where the generator writes: the committed artifact, unless
/// [`OUT_ENV`] redirects it.
///
/// Defaulting to the real location is what lets [`REGEN_COMMAND`] carry no path
/// argument — so the command committed inside the artifact cannot drift from
/// where the artifact actually lives.
fn survey_output_path() -> PathBuf {
    survey_output_path_for(std::env::var(OUT_ENV).ok())
}

// ─── rendering the second half ───────────────────────────────────────────────

/// The slice of `md` from `heading` up to the next `## ` heading.
///
/// Section-scoped, so an assertion that a row is in the inline half cannot be
/// satisfied by the `.ri` half carrying it — which a whole-document `contains`
/// would happily do.
#[cfg(test)]
fn section_of(md: &str, heading: &str) -> String {
    let start = md
        .find(heading)
        .unwrap_or_else(|| panic!("the artifact must carry a {heading:?} section:\n{md}"));
    let body = &md[start + heading.len()..];
    let end = body.find("\n## ").map_or(body.len(), |i| i + 1);
    body[..end].to_owned()
}

/// One INLINE row, as [`survey_inline_corpus`] builds them.
#[cfg(test)]
fn synth_inline_row(
    file: &str,
    line: u32,
    snippet_line: u32,
    field: &str,
    owner: Owner,
) -> SurveySite {
    let mut site = synth_inline_site(file, Some(field), CTOR_CONFORMANCE_SITE_SEVERITY);
    site.line = line;
    site.snippet_line = Some(snippet_line);
    site.owner = owner;
    site
}

/// A populated inline run covering every [`Owner`] group and every coverage
/// reason the inline sweep can record.
#[cfg(test)]
fn synth_inline_run() -> SurveyRun {
    SurveyRun {
        total: 7,
        surveyed: 4,
        not_surveyed: vec![
            ("gone.rs".to_owned(), "read-error".to_owned()),
            ("host.rs:200".to_owned(), "parse-error".to_owned()),
            ("host.rs:300".to_owned(), "format-template".to_owned()),
        ],
        partial: vec![("host.rs:400".to_owned(), "compile-error".to_owned())],
        sites: vec![
            synth_inline_row("host.rs", 1453, 2, "material", Owner::Unknown),
            synth_inline_row("host.rs", 1517, 3, "youngs_modulus", Owner::NonFea),
            synth_inline_row("other.rs", 42, 7, "z", Owner::FeaDeferredToV06),
            synth_inline_row("other.rs", 99, 1, "q", Owner::UnresolvedDef),
        ],
    }
}

/// The inline half renders in its OWN section, and its rows never leak into the
/// `.ri` half's.
#[test]
fn render_survey_renders_the_inline_half_in_its_own_section() {
    let ri = SurveyRun {
        total: 1,
        surveyed: 1,
        sites: vec![synth_site("a.ri", 1, "Widget", "label", Owner::NonFea)],
        ..SurveyRun::default()
    };
    let md = render_survey(&ri, &synth_inline_run(), &SurveyStamp::at("sha"));

    let inline = section_of(&md, INLINE_SECTION_HEADING);
    let sites = section_of(&md, "## Sites");

    assert!(
        inline.contains("host.rs:1453"),
        "an inline row must render in the inline section:\n{inline}"
    );
    assert!(
        !sites.contains("host.rs:1453"),
        "an inline row must NOT leak into the `.ri` half's `## Sites` section — the \
         two halves answer different questions and are sized separately:\n{sites}"
    );
    assert!(
        !inline.contains("a.ri:1"),
        "a tracked `.ri` row must NOT leak into the inline section:\n{inline}"
    );
}

/// The inline section is grouped by the same derived [`Owner::render_order`].
///
/// Derived, not re-listed: a future `Owner` variant added to the enum must
/// appear in BOTH halves or in neither. A local literal in either renderer is
/// exactly how one half would silently drop a group.
#[test]
fn render_survey_groups_the_inline_half_by_the_same_owner_render_order() {
    let md = render_survey(
        &SurveyRun::default(),
        &synth_inline_run(),
        &SurveyStamp::at("sha"),
    );
    let inline = section_of(&md, INLINE_SECTION_HEADING);

    let mut cursor = 0usize;
    for owner in Owner::render_order() {
        let at = inline[cursor..].find(owner.title()).unwrap_or_else(|| {
            panic!(
                "the inline section must carry an `{}` group, in `Owner::render_order` \
                 order:\n{inline}",
                owner.title()
            )
        });
        cursor += at + owner.title().len();
    }
}

/// Every inline row carries its snippet-relative coordinate, in a column located
/// BY NAME.
#[test]
fn render_survey_gives_the_inline_half_a_snippet_line_column() {
    let md = render_survey(
        &SurveyRun::default(),
        &synth_inline_run(),
        &SurveyStamp::at("sha"),
    );
    let inline = section_of(&md, INLINE_SECTION_HEADING);
    let header = inline
        .lines()
        .find(|l| l.starts_with("| site |"))
        .unwrap_or_else(|| panic!("the inline table header must be rendered:\n{inline}"));

    for (anchor, expected) in [("| `host.rs:1453`", "2"), ("| `other.rs:42`", "7")] {
        assert_eq!(
            cell_by_column(&inline, header, SNIPPET_LINE_COLUMN, anchor),
            expected,
            "the `{SNIPPET_LINE_COLUMN}` cell must carry the coordinate WITHIN the \
             snippet; the `site` cell already carries the host position, and a reader \
             needs both to find the declaration inside the literal:\n{inline}"
        );
    }
}

/// The stated inline site count equals the inline rows actually rendered.
///
/// Mirrors [`render_survey_states_a_site_count_that_equals_the_rendered_rows`]
/// for the second half: a header number that can disagree with the table below
/// it is worse than no number.
#[test]
fn render_survey_states_an_inline_site_count_that_equals_the_rendered_rows() {
    let inline_run = synth_inline_run();
    let md = render_survey(&SurveyRun::default(), &inline_run, &SurveyStamp::at("sha"));
    // Scoped to the GROUP tables: the coverage subsection below them renders
    // `| `member` | `reason` |` rows of its own, which are members and not
    // sites, and counting those would make the assertion meaningless.
    let inline = section_of(&md, INLINE_SECTION_HEADING);
    let groups = inline
        .split_once(INLINE_COVERAGE_HEADING)
        .map_or(inline.as_str(), |(before, _)| before);

    let rendered = groups.lines().filter(|l| l.starts_with("| `")).count();
    assert_eq!(
        rendered,
        inline_run.sites.len(),
        "the inline section rendered {rendered} row(s) for {} site(s):\n{inline}",
        inline_run.sites.len()
    );
    let claimed: usize = groups
        .lines()
        .filter_map(|l| l.strip_prefix("### "))
        .filter_map(|l| l.rsplit_once(" — "))
        .filter_map(|(_, tail)| tail.split_whitespace().next()?.parse::<usize>().ok())
        .sum();
    assert_eq!(
        claimed, rendered,
        "the per-group counts must sum to the rendered rows:\n{inline}"
    );
}

/// A zero-inline-site run renders an EXPLICIT zero, not an empty section.
///
/// An empty section reads as a truncated run. The `.ri` half already states its
/// zero explicitly; the second half must not be the one that reads as silence.
#[test]
fn render_survey_renders_the_zero_inline_site_case_explicitly() {
    let md = render_survey(
        &SurveyRun::default(),
        &SurveyRun::default(),
        &SurveyStamp::at("sha"),
    );
    let inline = section_of(&md, INLINE_SECTION_HEADING);
    assert!(
        inline.contains("No ctor-conformance sites"),
        "the zero-inline case must say so in words:\n{inline}"
    );
}

/// The inline section carries its OWN coverage subsection, naming every
/// unsurveyable snippet with its reason.
#[test]
fn render_survey_lists_every_unsurveyable_inline_snippet_with_its_reason() {
    let md = render_survey(
        &SurveyRun::default(),
        &synth_inline_run(),
        &SurveyStamp::at("sha"),
    );
    let inline = section_of(&md, INLINE_SECTION_HEADING);

    for (member, reason) in [
        ("gone.rs", "read-error"),
        ("host.rs:200", "parse-error"),
        ("host.rs:300", "format-template"),
        ("host.rs:400", "compile-error"),
    ] {
        assert!(
            inline.contains(member) && inline.contains(reason),
            "the inline coverage subsection must name `{member}` with reason \
             `{reason}` — a bounded sweep that does not state what it skipped reads \
             as full coverage:\n{inline}"
        );
    }
}

/// The header states BOTH corpus sizes and BOTH parity floors.
///
/// A reader sizing the census has to be able to tell a thin artifact from a thin
/// CORPUS, and the floors are what make "thin" checkable rather than a feeling.
#[test]
fn render_survey_header_states_both_corpus_sizes_and_both_parity_floors() {
    let ri = SurveyRun {
        total: 689,
        surveyed: 616,
        ..SurveyRun::default()
    };
    let md = render_survey(&ri, &synth_inline_run(), &SurveyStamp::at("sha"));
    let header = section_of(&md, "# Struct-ctor field-type conformance — corpus survey");

    for half in [CorpusHalf::TrackedRi, CorpusHalf::InlineRustHost] {
        assert!(
            header.contains(half.label()),
            "the header must name the `{}` half:\n{header}",
            half.label()
        );
        assert!(
            header.contains(&half.floor().to_string()),
            "the header must state the `{}` half's parity floor ({}):\n{header}",
            half.label(),
            half.floor()
        );
    }
}

/// The `**Sites:**` line distinguishes the two totals rather than summing them.
///
/// One unattributed number would be the single most misleading thing the header
/// could say: the two halves have different dispositions, different owners and
/// different actionability, and adding them erases all three.
#[test]
fn render_survey_states_the_two_site_totals_separately() {
    let ri = SurveyRun {
        total: 3,
        surveyed: 3,
        sites: vec![
            synth_site("a.ri", 1, "W", "f", Owner::NonFea),
            synth_site("b.ri", 2, "W", "g", Owner::NonFea),
        ],
        ..SurveyRun::default()
    };
    let inline_run = synth_inline_run();
    let md = render_survey(&ri, &inline_run, &SurveyStamp::at("sha"));
    let sites_line = md
        .lines()
        .find(|l| l.starts_with("**Sites:**"))
        .unwrap_or_else(|| panic!("the header must carry a `**Sites:**` line:\n{md}"));

    let sum = (ri.sites.len() + inline_run.sites.len()).to_string();
    assert!(
        !sites_line.split_whitespace().any(|w| w == sum),
        "the `**Sites:**` line must not collapse the two halves into the single \
         number {sum}: they carry different dispositions and different owners, and \
         one total erases that; got {sites_line:?}"
    );
    assert!(
        sites_line.contains(&ri.sites.len().to_string())
            && sites_line.contains(&inline_run.sites.len().to_string()),
        "the `**Sites:**` line must state BOTH totals; got {sites_line:?}"
    );
}

/// Adding an inline half leaves the `.ri` half's section byte-identical.
///
/// The cross-contamination regression test. The two halves share the
/// owner-grouping and table-body helpers, so a change made for one is a change
/// made for both — this is what says which of those changes is allowed to be
/// visible in the `.ri` half.
#[test]
fn render_survey_keeps_the_ri_half_byte_identical_when_an_inline_half_is_added() {
    let ri = SurveyRun {
        total: 5,
        surveyed: 4,
        not_surveyed: vec![("broken.ri".to_owned(), "parse-error".to_owned())],
        partial: vec![("multi.ri".to_owned(), "compile-error".to_owned())],
        sites: vec![
            synth_site("a.ri", 1, "Widget", "label", Owner::NonFea),
            synth_site("b.ri", 2, "Beam", "material", Owner::FeaDeferredToV06),
        ],
    };
    let without = render_survey(&ri, &SurveyRun::default(), &SurveyStamp::at("sha"));
    let with = render_survey(&ri, &synth_inline_run(), &SurveyStamp::at("sha"));

    assert_eq!(
        section_of(&without, "## Sites"),
        section_of(&with, "## Sites"),
        "the `.ri` half's counts, groups and rows must not move when an inline half \
         is added"
    );
}

// ─── retiring the now-false named limitation 1 ───────────────────────────────

/// The body of named limitation 1, from its key to the start of limitation 2.
///
/// Scoped rather than whole-document, because several of the strings this
/// limitation must name — `format!` above all — also occur elsewhere in the
/// artifact (the inline coverage table's `format-template` reason). A
/// whole-document `contains` would be satisfied by those and would assert
/// nothing about the limitation.
#[cfg(test)]
fn inline_limitation(md: &str) -> String {
    let start = md.find(INLINE_LIMITATION_KEY).unwrap_or_else(|| {
        panic!("named limitation 1 must open with {INLINE_LIMITATION_KEY:?}:\n{md}")
    });
    let body = &md[start..];
    let end = body
        .find("\n2. ")
        .unwrap_or_else(|| panic!("limitation 1 must be followed by limitation 2:\n{body}"));
    body[..end].to_owned()
}

/// The artifact names the walker's REAL residual, by identifier.
///
/// The positive claim only. An earlier draft also asserted the ABSENCE of the
/// two prose claims this task disproved, and that pin was wrong in both
/// directions: rewording "are not file-enumerable" to "are not enumerable as
/// files" left it green with the disproved claim still in the artifact, while
/// any innocuous rewrite of unrelated prose that happened to contain the phrase
/// would red the merge gate. What the artifact must SAY is testable by
/// identifier, which survives a full reword; what it must not say is not.
#[test]
fn render_survey_retires_the_disproved_limitation_and_names_the_real_residual() {
    let md = render_survey(
        &SurveyRun::default(),
        &synth_inline_run(),
        &SurveyStamp::at("sha"),
    );

    // The residual is stated as CLASSES, each named by the Rust construct — or,
    // for a host the walker never opens, the PATH — a reader would grep for, and
    // each named INSIDE the limitation rather than anywhere in the document.
    // Unreached LITERAL SHAPES alone are not the whole residual: the next scope
    // or severity change needs this list rather than a search, so a host root
    // outside the walker's scope
    // has to be as greppable as an unreached construct, else a site the walker
    // never opened is indistinguishable from one it found clean.
    let limitation = inline_limitation(&md);
    for residual in [
        "raw-string literal",
        "concat!",
        "format!",
        "include_str!",
        "read_to_string",
        "push_str",
        "crates/",
        "gui/src-tauri",
        "tree-sitter-reify",
    ] {
        assert!(
            limitation.contains(residual),
            "the replacement limitation must name the `{residual}` class: a reader \
             has to be able to tell a snippet the walker MISSED from one it found \
             clean. Got:\n{limitation}"
        );
    }
}

/// The limitation's quantities come from the RUN, not from frozen prose.
///
/// A hand-typed count is a number nothing recomputes: it is right on the day it
/// is written and silently wrong forever after. Two runs differing only in their
/// inline half must therefore state different figures — and the difference has
/// to be in the LIMITATION, which is the paragraph a reader consults to size
/// what was missed.
#[test]
fn the_inline_limitation_states_figures_the_run_recomputed() {
    let stamp = SurveyStamp::at("sha");
    let small = render_survey(&SurveyRun::default(), &SurveyRun::default(), &stamp);
    let large = render_survey(&SurveyRun::default(), &synth_inline_run(), &stamp);

    assert_ne!(
        inline_limitation(&small),
        inline_limitation(&large),
        "the limitation's stated quantities must be recomputed per run; if they are \
         identical across runs with different inline halves, they were typed"
    );
    assert_ne!(
        section_of(&small, INLINE_SECTION_HEADING),
        section_of(&large, INLINE_SECTION_HEADING),
        "so must the inline section's"
    );
}

/// A drifted member of EITHER half is named in the rendered disclosure.
#[test]
fn the_drift_disclosure_is_not_scoped_to_ri_alone() {
    let drifted = SurveyStamp {
        anchor: "cafe1234".to_owned(),
        drifted: vec!["examples/a.ri".to_owned(), "crates/c/tests/h.rs".to_owned()],
    };
    let md = render_survey(&SurveyRun::default(), &SurveyRun::default(), &drifted);
    for path in &drifted.drifted {
        assert!(
            md.contains(path.as_str()),
            "a drifted {path} must be named in the disclosure:\n{md}"
        );
    }
}

/// **The survey generator.** Sweeps BOTH corpus halves and writes the artifact.
///
/// `#[ignore]`d because it compiles the entire tracked corpus — ~2.5× the
/// `examples/` walk that `examples_smoke.rs` already documents as "the single
/// most expensive thing this binary does" — and then, on top of that, every
/// Reify snippet embedded in a tracked `.rs` under `crates/`, which is several
/// times as many compiles again. Running it on every merge gate would directly fight
/// `docs/prds/merge-gate-compile-cost.md`.
///
/// The ignore reason is deliberately OPERATIONAL, not blocker-prose: per
/// `docs/prds/reify-audit-ptodo-detector.md` §8 (row 8, the
/// `#[ignore = "requires OCCT"]` class) an operational reason produces no PTODO
/// finding and needs no `#NNNN` cite. One is deliberately NOT written here — a
/// cite would be liveness-checked and would go orphaned the moment task #5304
/// closes.
///
/// Nothing is lost to the ignore: every DECISION this test makes lives in the
/// pure helpers above, each unit-tested on every gate run, plus one cheap
/// three-file end-to-end sweep.
#[test]
#[ignore = "corpus survey generator over BOTH halves — every tracked .ri (~700 files) plus the Reify snippets embedded in every tracked .rs under crates/ (~1,870 files, ~3,300 admitted snippets), so several times the cost of the .ri walk alone; run explicitly with --ignored — see docs/prds/struct-ctor-field-type-conformance.survey.md"]
fn generate_ctor_conformance_corpus_survey() {
    let root = std::path::Path::new(WORKSPACE_ROOT);
    let corpus = tracked_ri_corpus();
    let hosts = tracked_rust_hosts();

    // BEFORE either sweep, deliberately: a walker that silently narrowed would
    // otherwise spend the whole (expensive) run producing a falsely-thin
    // artifact, and a thin artifact reads exactly like a clean one.
    corpus_parity(&[
        (CorpusHalf::TrackedRi, corpus),
        (CorpusHalf::InlineRustHost, hosts),
    ])
    .unwrap_or_else(|e| panic!("ctor_conformance_corpus_survey: {e}"));

    let run = survey_corpus(root, corpus);
    let inline = survey_inline_corpus(root, hosts);
    let rendered = render_survey(&run, &inline, &survey_stamp(corpus, hosts));
    let out = survey_output_path();
    std::fs::write(&out, &rendered)
        .unwrap_or_else(|e| panic!("cannot write survey to {}: {e}", out.display()));
    println!(
        "ctor-conformance survey: {} sites across {} tracked .ri ({} surveyed, \
         {} not surveyed, {} partial); {} sites across {} inline member(s) from \
         {} .rs host(s) ({} swept, {} not swept, {} partial) -> {}",
        run.sites.len(),
        run.total,
        run.surveyed,
        run.not_surveyed.len(),
        run.partial.len(),
        inline.sites.len(),
        inline.total,
        hosts.len(),
        inline.surveyed,
        inline.not_surveyed.len(),
        inline.partial.len(),
        out.display()
    );

    // Asserted AFTER the write, deliberately: a failing run still leaves a
    // regenerated artifact on disk, so the operator can read the disposition
    // column to see which sites the panic is talking about.
    //
    // The `.ri` run ONLY, and correct by construction rather than by a filter
    // here: an inline row resolves to [`Disposition::InlineCensus`], never
    // `Unattributed`, so passing the inline run would assert nothing — while a
    // severity or scope test written here would be a second, silently divergent
    // copy of `disposition_of`'s scope statement.
    assert_no_unwaived_ctor_conformance_sites(&run);
}

#[test]
fn survey_output_path_defaults_to_the_committed_artifact_location() {
    // The default must match the artifact's real location exactly, so the
    // regeneration command committed INSIDE the artifact needs no path argument
    // and cannot drift from where the file actually lives.
    //
    // Asserted against the PURE seam `survey_output_path_for(None)`, never
    // against `survey_output_path()`: the latter reads the process-global
    // `REIFY_CTOR_SURVEY_OUT`, which this artifact itself tells operators to
    // export when diffing a fresh run against the committed copy. A
    // gate-resident test that reads it reds for an operator doing exactly what
    // the artifact documents — no defect, pure false alarm.
    let path = survey_output_path_for(None);
    let expected = std::path::Path::new(WORKSPACE_ROOT)
        .join("docs/prds/struct-ctor-field-type-conformance.survey.md");
    assert_eq!(
        path, expected,
        "the default output path must be the committed artifact location"
    );
    let parent = path.parent().expect("the artifact path has a parent");
    assert!(
        parent.is_dir(),
        "the artifact's parent directory {} must exist",
        parent.display()
    );
}

#[test]
fn survey_output_path_honours_the_scratch_override() {
    // The override exists so the sweep can be re-run into a scratch path and
    // diffed against the committed copy WITHOUT dirtying the tree — which is
    // exactly how step 15 proves byte-for-byte reproducibility.
    assert_eq!(
        survey_output_path_for(Some("/tmp/scratch-survey.md".to_owned())),
        std::path::PathBuf::from("/tmp/scratch-survey.md"),
        "REIFY_CTOR_SURVEY_OUT must override the default"
    );
    // Both fallbacks are compared against the DEFAULT PATH ITSELF rather than
    // against `survey_output_path()`, so nothing here depends on the ambient
    // value of `REIFY_CTOR_SURVEY_OUT`.
    let default_path = std::path::Path::new(WORKSPACE_ROOT).join(ARTIFACT_REL);
    assert_eq!(
        survey_output_path_for(None),
        default_path,
        "an unset override must fall back to the committed location"
    );
    assert_eq!(
        survey_output_path_for(Some(String::new())),
        default_path,
        "an EMPTY override must fall back too — an accidental `REIFY_CTOR_SURVEY_OUT=` \
         must not write the artifact to the current directory"
    );
}

// ─── step 19/20: the committed stamp must stay reachable ─────────────────────

/// The `<sha>` from the artifact's ``**Base commit:** `<sha>` `` header line.
///
/// `None` when the line is absent or does not have that exact shape — the
/// caller treats that as a FAILURE, never as a skip, so a renderer change
/// cannot silently defeat the parse and take the guard with it.
fn parse_stamped_base_commit(md: &str) -> Option<&str> {
    md.lines()
        .filter_map(|line| line.trim_end().strip_prefix("**Base commit:** `"))
        .find_map(|rest| rest.strip_suffix('`'))
}

/// TRIPWIRE: the committed artifact still renders the CURRENT
/// [`Disposition::NotApplicable`] cell.
///
/// Makes artifact staleness a repeatable gate-resident signal instead of
/// something only a human review catches. Before this guard, step 8 re-scoped the
/// resolver and δ moved the knob while the committed artifact went on rendering
/// 13 `Warning` severity cells and the pre-re-scope `n/a` wording — a divergence
/// that survived a full merge gate.
///
/// # WHY ONE STRING IS ENOUGH
///
/// A generator run rewrites the WHOLE file, so the severity column and the
/// disposition labels can never go stale independently of one another. A
/// tripwire on any single code-owned rendered string therefore detects staleness
/// in every dimension at once. This is a tripwire, NOT a golden-file comparison:
/// it asks whether the artifact was produced by roughly today's renderer, never
/// whether it is byte-current. A plain substring, because parsing the rendered
/// markdown table here would be a second copy of the renderer.
///
/// # WHY IT MUST NOT BE STRENGTHENED INTO A FRESHNESS GATE
///
/// Re-deriving the artifact to compare against it would compile all 723 tracked
/// `.ri` on every merge-gate run — exactly what
/// `docs/prds/merge-gate-compile-cost.md` forbids, and exactly why the generator
/// is `#[ignore]`d in the first place. This gates the artifact on the RESOLVER'S
/// SEMANTICS, which are current code. It never gates the CENSUS — the counts and
/// the row set — which the artifact's own header deliberately declares a
/// point-in-time snapshot.
///
/// # Why `NotApplicable` is the right anchor
///
/// Its cell is fixed by construction. [`Disposition::Deferred`] and
/// [`Disposition::IntendedRejection`] both interpolate a per-row `why`, so
/// neither has a stable rendering to pin. [`Disposition::Unattributed`]'s is
/// fixed too, but a correct artifact may legitimately contain ZERO unattributed
/// rows — that is the goal state — so pinning it would red on success.
///
/// Skips on absence for the same reason
/// [`committed_survey_stamps_a_commit_that_is_an_ancestor_of_head`] does:
/// deleting a consumed census is its documented end of life, not a defect.
#[test]
fn committed_survey_renders_the_current_disposition_vocabulary() {
    let artifact = std::path::Path::new(WORKSPACE_ROOT).join(ARTIFACT_REL);
    if !artifact.is_file() {
        println!(
            "skipped: {ARTIFACT_REL} is not present — the survey snapshot has been \
             retired or not yet generated; nothing to check"
        );
        return;
    }
    let text = std::fs::read_to_string(&artifact)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", artifact.display()));

    let expected = Disposition::NotApplicable.label();
    assert!(
        text.contains(&expected),
        "{ARTIFACT_REL} does not render the current `Disposition::NotApplicable` cell, so \
         it was produced by an older renderer and EVERY code-owned string in it is \
         suspect — including the severity column, which δ (#5306) moved.\n\n\
         Expected to find: {expected:?}\n\n\
         REGENERATE it, on a CLEAN tree (`stamp_decision` refuses a dirty one):\n  \
         {REGEN_COMMAND}\n\n\
         Do NOT weaken this into a comparison against a freshly derived artifact: that \
         would compile every tracked `.ri` on each merge-gate run, which is what \
         docs/prds/merge-gate-compile-cost.md forbids and why the generator is \
         `#[ignore]`d."
    );
}

#[test]
fn committed_survey_stamps_a_commit_that_is_an_ancestor_of_head() {
    // Makes the dangling-anchor defect class LOUD on every gate run instead of
    // leaving it for a human reviewer to catch. Cost is one file read plus
    // three git calls — so unlike the `#[ignore]`d generator this belongs here.
    //
    // ORDERING DEPENDENCY: this guard stays green across future rebases ONLY
    // because `survey_stamp()` stamps `git merge-base main HEAD` rather than the
    // branch tip. Adding it while the anchor was still a branch tip would
    // convert every rebase of this branch into a merge-blocking red.
    //
    // Unlike `survey_stamp()`, nothing here needs a `main` ref to exist.
    //
    // SKIP, not fail, when git cannot be spawned: every assertion below is a
    // question put to git, and "there is no git" is not an answer about the
    // artifact. See `git_is_available` for why this module must not be what
    // makes git a hard requirement of the reify-compiler suite.
    if !git_is_available() {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    }

    // SKIP, not fail, when the artifact is ABSENT.
    //
    // This module's header states the artifact is a point-in-time snapshot, not
    // a freshness-gated golden file, and γ (task #5305) will legitimately
    // invalidate it. The natural end of life for a consumed census is DELETION —
    // see "Retiring this module" in the header. Panicking on absence would make
    // that one-file deletion red the whole `reify-compiler` merge gate with a
    // message that reads like a compiler defect, and whoever deletes a document
    // under `docs/prds/` has no reason to look inside a compiler test binary for
    // the cause.
    //
    // What is NOT relaxed: if the file IS present, every assertion below is
    // hard. An artifact that exists while naming an unresolvable or orphaned
    // anchor is a real defect, and that is the case this guard was written for.
    let artifact = std::path::Path::new(WORKSPACE_ROOT).join(ARTIFACT_REL);
    if !artifact.is_file() {
        println!(
            "skipped: {ARTIFACT_REL} is not present — the survey snapshot has been \
             retired or not yet generated; nothing to check"
        );
        return;
    }
    let md = std::fs::read_to_string(&artifact).unwrap_or_else(|e| {
        // Present but unreadable is an I/O failure, not a retirement.
        panic!(
            "cannot read the committed survey at {}: {e}",
            artifact.display()
        )
    });

    // (a) The header line must be present AND parseable. A missing or
    //     reshaped line FAILS rather than skipping: a renderer change must not
    //     be able to defeat the parse and silently disarm (b) and (c) with it.
    let sha = parse_stamped_base_commit(&md).unwrap_or_else(|| {
        panic!(
            "{} must carry a `**Base commit:** `<sha>`` header line — without it \
             the artifact's snapshot claim names nothing checkable",
            ARTIFACT_REL
        )
    });
    assert!(
        sha.len() == FULL_SHA_LEN && sha.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
        "the stamped base commit must be {FULL_SHA_LEN} lowercase hex characters, \
         got {sha:?}"
    );

    // The one environment that could produce a FALSE red: in a shallow clone
    // the anchor object may legitimately be absent, and a false red here would
    // deadlock the merge queue. Verified `false` in this worktree, so the
    // assertions below really do run.
    if git_read(&["rev-parse", "--is-shallow-repository"]) == "true" {
        return;
    }

    // (b) The object actually exists in this repository. This alone catches a
    //     stamp that survives only as a dangling object in one worktree.
    assert!(
        git_succeeds(&["cat-file", "-e", &format!("{sha}^{{commit}}")]),
        "the stamped base commit {sha} does not exist as a commit in this \
         repository — the artifact header names an unresolvable object"
    );

    // (c) It is reachable from the tip, so it survives the `--no-ff` merge onto
    //     main. A rebase orphaning the stamped commit reds exactly here.
    assert!(
        git_succeeds(&["merge-base", "--is-ancestor", sha, "HEAD"]),
        "the stamped base commit {sha} is not an ancestor of HEAD — it was \
         orphaned (a rebase, most likely) and will vanish when this branch \
         lands. Re-run the survey generator to re-stamp the merge base."
    );
}
