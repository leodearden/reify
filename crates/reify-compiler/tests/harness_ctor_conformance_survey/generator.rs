//! The survey's entry point — writes the artifact — and the gate-resident
//! tripwires on the committed copy.
//!
//! What the survey is for, why its corpus walk is `#[ignore]`d while every
//! decision is gate-resident, and how to retire it are stated once, in the
//! harness root `crates/reify-compiler/tests/harness_ctor_conformance_survey.rs`.

use std::path::PathBuf;

use crate::corpus::{CorpusHalf, corpus_parity, tracked_ri_corpus, tracked_rust_hosts};
use crate::disposition::{Disposition, assert_no_unwaived_ctor_conformance_sites};
use crate::render::{REGEN_COMMAND, parse_stamped_base_commit, render_survey};
use crate::stamp::{FULL_SHA_LEN, survey_stamp};
use crate::sweep::{survey_corpus, survey_inline_corpus};
use crate::workspace_git::{WORKSPACE_ROOT, git_is_available, git_read, git_succeeds};

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
    .unwrap_or_else(|e| panic!("harness_ctor_conformance_survey::generator: {e}"));

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
    // artifact. See `git_is_available` for why this survey must not be what
    // makes git a hard requirement of the reify-compiler suite.
    if !git_is_available() {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    }

    // SKIP, not fail, when the artifact is ABSENT.
    //
    // The harness root's header states the artifact is a point-in-time snapshot, not
    // a freshness-gated golden file, and γ (task #5305) will legitimately
    // invalidate it. The natural end of life for a consumed census is DELETION —
    // see "Retiring this unit" in the header. Panicking on absence would make
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
